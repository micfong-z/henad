//! GPU stepping for a host that drives a [`GpuSimState`] without a sim thread.
//!
//! Native only. A browser cannot block on the GPU.

use henad_core::action::{Fire, RefusedActions, Schedule};
use henad_core::view::StatEntry;

use crate::fault::{Fault, FaultKind, STEPPING};
use crate::gpu::{GpuContext, GpuSimState, MAX_STEPS_PER_SUBMISSION};

/// Submits `count` steps on the GPU without waiting for them.
///
/// Each command buffer holds at most [`MAX_STEPS_PER_SUBMISSION`] steps.
pub fn submit_steps(state: &mut dyn GpuSimState, ctx: &GpuContext, count: u64) {
    // Submissions to one queue run in order, so batch N+1 still reads batch N's output. They all
    // queue up and the caller's one wait drains them, letting the CPU encode ahead of the GPU.
    let mut remaining = count;
    while remaining > 0 {
        let n = remaining.min(u64::from(MAX_STEPS_PER_SUBMISSION)) as u32;
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_steps"),
        });
        state.encode_steps(&mut encoder, n, None);
        ctx.queue.submit(Some(encoder.finish()));
        remaining -= u64::from(n);
    }
}

/// Submits `count` steps of `state` as one command buffer, with its stats passes after them when `sample` is set.
///
/// A sample's readback begins once the buffer is submitted, and [`GpuSimState::poll_stats_readback`] collects it.
/// Nothing waits on the GPU.
///
/// # Panics
///
/// Panics when `count` exceeds [`MAX_STEPS_PER_SUBMISSION`]. A debug build also panics on a sample while the
/// previous readback is pending, since the readback would skip it and report the stale values.
pub fn submit_slice(state: &mut dyn GpuSimState, ctx: &GpuContext, count: u32, sample: bool) -> wgpu::SubmissionIndex {
    assert!(
        count <= MAX_STEPS_PER_SUBMISSION,
        "a slice of {count} steps exceeds one submission"
    );
    debug_assert!(
        !sample || !state.stats_readback_pending(),
        "a sample while a readback is pending would be skipped"
    );
    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("henad_gpu_slice"),
    });
    state.encode_steps(&mut encoder, count, None);
    if sample {
        state.encode_stats_passes(&mut encoder);
    }
    let submission_index = ctx.queue.submit(Some(encoder.finish()));
    if sample {
        state.begin_stats_readback();
    }
    submission_index
}

/// Blocks until the GPU has finished everything submitted, then reports any fault it raised.
///
/// Note that `queue.submit()` returns before the GPU has executed anything. A timer around submission alone measures
/// the CPU's dispatch cost, and the throughput it reports is fiction. A timed run has to end in this wait inside the
/// timer.
///
/// # Errors
///
/// Returns the first fault the device raised, or a [`FaultKind::Poll`] fault when the wait itself fails.
pub fn wait(ctx: &GpuContext) -> Result<(), Fault> {
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(poll_fault)?;

    if let Some(fault) = ctx.faults.take() {
        return Err(fault);
    }
    Ok(())
}

/// Blocks until the GPU has finished `submission_index` and everything submitted before it.
///
/// Unlike [`wait`], it leaves any fault the device raised in `ctx.faults`.
///
/// # Errors
///
/// Returns a [`FaultKind::Poll`] fault when the wait itself fails.
pub fn await_submission(ctx: &GpuContext, submission_index: wgpu::SubmissionIndex) -> Result<(), Fault> {
    ctx.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: None,
        })
        .map_err(poll_fault)?;
    Ok(())
}

fn poll_fault(error: wgpu::PollError) -> Fault {
    Fault {
        during: STEPPING,
        kind: FaultKind::Poll(error),
    }
}

/// Runs `count` steps on the GPU and blocks until the GPU has finished them.
///
/// # Errors
///
/// Returns the fault [`wait`] reports.
pub fn run_steps(state: &mut dyn GpuSimState, ctx: &GpuContext, count: u64) -> Result<(), Fault> {
    if count == 0 {
        return Ok(());
    }
    submit_steps(state, ctx, count);
    wait(ctx)
}

/// As [`run_steps`], stopping at each tick the schedule names to encode its actions.
///
/// `fire` picks the side of the step an action fires on, as [`Schedule::fire_ticks`] spells out.
/// [`Fire::BeforeStep`] leaves the tick the run stops on to the caller, and [`Fire::AfterStep`] leaves
/// the tick it starts on. Back-to-back runs under one rule then fire each tick once.
///
/// Every batch of steps and every action is submitted before one wait at the end, and a run of no steps waits for
/// nothing. Under [`Fire::AfterStep`] that wait covers an action on the tick the run stops on. The entries the state
/// refused come back in the order they were due.
///
/// # Errors
///
/// Returns the fault [`wait`] reports.
pub fn run_steps_acting<'s>(
    state: &mut dyn GpuSimState,
    ctx: &GpuContext,
    count: u64,
    schedule: &'s Schedule,
    fire: Fire,
) -> Result<RefusedActions<'s>, Fault> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let end = state.tick() + count;
    let mut refused = Vec::new();
    for tick in schedule.fire_ticks(state.tick(), count, fire) {
        let now = state.tick();
        submit_steps(state, ctx, tick - now);
        refused.extend(run_due(state, ctx, schedule));
    }
    let now = state.tick();
    submit_steps(state, ctx, end - now);
    wait(ctx)?;
    Ok(refused)
}

/// Encodes whatever is due at the state's current tick, and returns the entries the state refused.
///
/// An action goes in a submission of its own, between two batches of steps, since a uniform
/// written mid-encoder would not be visible until the whole encoder submitted.
#[must_use = "a refused action is reported only through the returned entries"]
pub fn run_due<'s>(state: &mut dyn GpuSimState, ctx: &GpuContext, schedule: &'s Schedule) -> RefusedActions<'s> {
    let mut refused = Vec::new();
    for action in schedule.due(state.tick()) {
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_action"),
        });
        if state.encode_action(&mut encoder, action.index) {
            ctx.queue.submit(Some(encoder.finish()));
        } else {
            refused.push(action);
        }
    }
    refused
}

/// Returns the stats of the state's current tick, blocking until the GPU reads them back.
///
/// Note that a fault the sample raises is left for the next [`wait`] to report.
pub fn sample_stats(state: &mut dyn GpuSimState, ctx: &GpuContext) -> Vec<StatEntry> {
    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("henad_gpu_stats_sample"),
    });
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
    state.stats()
}
