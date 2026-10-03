//! GPU stepping for a host that drives a [`GpuSimState`] without a sim thread.
//!
//! Native only. A browser cannot block on the GPU.

use henad_core::action::{Fire, RefusedActions, Schedule};
use henad_core::view::StatEntry;

use crate::fault::{Fault, FaultKind, STEPPING};
use crate::gpu::{GpuContext, GpuSimState, MAX_STEPS_PER_SUBMISSION, StatsPoll};

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
/// Returns a [`FaultKind::DeviceLost`] fault once the device is lost, the first fault the device raised otherwise,
/// or a [`FaultKind::Poll`] fault when the wait itself fails. A lost device fails its work as ordinary errors, and
/// the loss is their cause. It leaves any such error in `ctx.faults`.
pub fn wait(ctx: &GpuContext) -> Result<(), Fault> {
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(poll_fault)?;

    // wgpu reports a loss to the callback alone, and polls go on succeeding.
    if ctx.is_lost() {
        return Err(Fault::device_lost(STEPPING));
    }
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
/// The sample records the stats passes alone, as a sweep track's does, with no display pass. A readback still in
/// flight is collected first. Otherwise the sample's copy would be skipped, and the stats
/// returned would be the older sample's. Note that a device error the sample raises while its readback lands is left
/// for the next [`wait`] to report.
///
/// # Errors
///
/// Returns a [`FaultKind::DeviceLost`] fault once the device is lost. A readback that did not land returns the fault
/// in `ctx.faults`, or a [`FaultKind::Refused`] fault when an error scope holds the device's error. Either way the
/// stats the state holds are an earlier sample's.
pub fn sample_stats(state: &mut dyn GpuSimState, ctx: &GpuContext) -> Result<Vec<StatEntry>, Fault> {
    if state.stats_readback_pending() {
        state.poll_stats_readback(&ctx.device, true);
    }
    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("henad_gpu_stats_sample"),
    });
    state.encode_stats_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    let landed = state.poll_stats_readback(&ctx.device, true);
    if ctx.is_lost() {
        return Err(Fault::device_lost(STEPPING));
    }
    match landed {
        StatsPoll::Landed => Ok(state.stats()),
        StatsPoll::Pending | StatsPoll::Failed => Err(ctx
            .faults
            .take()
            .unwrap_or_else(|| Fault::refused(STEPPING, "the GPU did not read the stats back"))),
    }
}

#[cfg(test)]
mod tests {
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::view::{StatEntry, StatValue};

    use super::{sample_stats, submit_slice, submit_steps};
    use crate::gpu::primitives::readback::{CounterReadback, StatsPoll};
    use crate::gpu::{GpuSimState, headless_context};
    use crate::snapshot::GpuSnapshot;

    /// Reads its own tick back from the GPU as its one stat.
    struct TickReadback {
        tick: u32,
        queue: wgpu::Queue,
        readback: CounterReadback,
    }

    impl SimState for TickReadback {
        fn step(&mut self) {
            self.tick += 1;
        }
        fn tick(&self) -> u64 {
            u64::from(self.tick)
        }
        fn stats(&self) -> Vec<StatEntry> {
            vec![StatEntry {
                label: "Tick",
                value: StatValue::Scalar(f64::from(self.readback.values()[0])),
                color: [0; 4],
            }]
        }
        fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
            false
        }
        fn population(&self) -> u64 {
            0
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    impl GpuSimState for TickReadback {
        fn encode_steps(
            &mut self,
            _encoder: &mut wgpu::CommandEncoder,
            count: u32,
            _timestamps: Option<&wgpu::QuerySet>,
        ) {
            self.tick += count;
        }
        fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
            let wgpu::BindingResource::Buffer(storage) = self.readback.binding() else {
                panic!("the readback binds a buffer");
            };
            // The write lands as the encoder's submission starts, ahead of the copy.
            self.queue
                .write_buffer(storage.buffer, 0, bytemuck::bytes_of(&self.tick));
            self.readback.encode_copy(encoder);
        }
        fn begin_stats_readback(&mut self) {
            self.readback.begin_map();
        }
        fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
            if block {
                self.readback.poll_blocking(device)
            } else {
                self.readback.poll(device)
            }
        }
        fn stats_readback_pending(&self) -> bool {
            self.readback.is_pending()
        }
        fn view(&self) -> GpuSnapshot {
            GpuSnapshot {
                display: None,
                agents: None,
            }
        }
    }

    /// The regression. A sample taken while an earlier readback was in flight skipped its own copy and reported
    /// the earlier sample's values.
    #[test]
    fn a_sample_behind_an_unfinished_readback_reports_the_current_tick() {
        let Some(ctx) = headless_context("henad_sample_stats_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_sample_behind_an_unfinished_readback_reports_the_current_tick: no adapter");
            return;
        };
        let mut state = TickReadback {
            tick: 0,
            queue: ctx.queue.clone(),
            readback: CounterReadback::new(&ctx.device, "henad_sample_stats_test", 1),
        };

        submit_slice(&mut state, &ctx, 3, true);
        assert!(
            state.stats_readback_pending(),
            "a sampled slice leaves its readback in flight"
        );
        submit_steps(&mut state, &ctx, 4);
        let stats = sample_stats(&mut state, &ctx).expect("the sample lands");
        assert_eq!(stats[0].value.scalar(), 7.0, "the sample reported the slice's tick");
        assert!(ctx.faults.take().is_none());
    }
}
