//! Checks that step a GPU model on the device, native only.
//!
//! Each builds at the declared defaults, the size a host builds first. A watchdog trips on the time one command
//! buffer runs, and a small model never runs long enough to trip it.

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::{GpuContext, GpuSimState, MAX_STEPS_PER_SUBMISSION, StatsPoll, stepping};
use henad_core::params::ParamValue;

use super::settings::CheckSettings;
use super::{SEED, fault_text, first_stat_difference};

/// Builds GPU model `entry` at `values` on `ctx`.
fn build(entry: &ModelEntry, values: &[ParamValue], ctx: &GpuContext) -> Result<Box<dyn GpuSimState>, String> {
    match entry.build(values, Some(SEED), Some(ctx)) {
        Ok(ModelState::Gpu(state)) => Ok(state),
        Ok(ModelState::Cpu(_)) => {
            Err("The entry declares the GPU backend, but its factory returns a CPU state.".to_owned())
        }
        Err(fault) => Err(format!("The model did not build. {}", fault_text(&fault))),
    }
}

/// Checks [`super::ModelCheck::BaselineBuild`]. The build and the declared demand pin each other: an under-reported
/// pass fails the build, and an over-reported one fails the demand.
pub(super) fn baseline_build(entry: &ModelEntry, settings: &CheckSettings, ctx: &GpuContext) -> Result<(), String> {
    let values = settings.default_values(entry)?;
    let shortfalls = entry.shortfalls(&values, &ctx.device.limits());
    match (build(entry, &values, ctx), shortfalls.is_empty()) {
        (Ok(_), true) => Ok(()),
        (Ok(_), false) => Err(format!(
            "The model builds on the device, but its declared demand says it does not fit: {}.",
            shortfalls.join("; ")
        )),
        (Err(message), true) => Err(format!("The declared demand fits the device. {message}")),
        (Err(_), false) => Err(format!("The model does not fit the device: {}.", shortfalls.join("; "))),
    }
}

/// Checks [`super::ModelCheck::FullSubmission`]. An oversized submission stops running with no error and no panic,
/// leaving the tick advanced and every readback zero.
///
/// A model that replays exactly first runs `MAX_STEPS_PER_SUBMISSION` submissions of one step and reads their stats,
/// then one submission of them all, and compares the two. The single steps run first. A device the full submission
/// poisons reads zeros from then on, and would read them in both runs. A model that does not replay exactly checks
/// that the readback landed and that some stat is not zero. Either way a device lost on the way fails the check.
pub(super) fn full_submission(entry: &ModelEntry, settings: &CheckSettings, ctx: &GpuContext) -> Result<(), String> {
    let values = settings.default_values(entry)?;
    let sliced_stats = if entry.metadata().replays_exactly {
        let mut sliced = build(entry, &values, ctx)?;
        let mut last = None;
        for _ in 0..MAX_STEPS_PER_SUBMISSION {
            last = Some(stepping::submit_slice(&mut *sliced, ctx, 1, false));
        }
        if let Some(last) = last {
            stepping::await_submission(ctx, last).map_err(|fault| fault_text(&fault))?;
        }
        Some(stepping::sample_stats(&mut *sliced, ctx))
    } else {
        None
    };

    let mut whole = build(entry, &values, ctx)?;
    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("henad_full_submission"),
    });
    whole.encode_steps(&mut encoder, MAX_STEPS_PER_SUBMISSION, None);
    whole.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    whole.begin_stats_readback();
    let landed = whole.poll_stats_readback(&ctx.device, true) == StatsPoll::Landed;
    if ctx.is_lost() {
        return Err(format!(
            "The device was lost running {MAX_STEPS_PER_SUBMISSION} steps in one submission."
        ));
    }
    if !landed {
        return Err("The stats readback after one full submission did not land.".to_owned());
    }
    if whole.tick() != u64::from(MAX_STEPS_PER_SUBMISSION) {
        return Err(format!(
            "The tick after one submission of {MAX_STEPS_PER_SUBMISSION} steps reads {}.",
            whole.tick()
        ));
    }
    let stats = whole.stats();

    let Some(sliced_stats) = sliced_stats else {
        if stats.iter().all(|stat| stat.value.scalar() == 0.0) {
            return Err(format!(
                "Every stat read back zero after a submission of {MAX_STEPS_PER_SUBMISSION} steps, as a dropped \
                 submission reads."
            ));
        }
        return Ok(());
    };
    match first_stat_difference(&sliced_stats, &stats) {
        None => Ok(()),
        Some(difference) => Err(format!(
            "{MAX_STEPS_PER_SUBMISSION} submissions of one step and one submission of {MAX_STEPS_PER_SUBMISSION} \
             steps differ in {difference}."
        )),
    }
}

/// Checks [`super::ModelCheck::SampledSlice`]: a slice sampled through the stats passes alone reads back what the
/// snapshot passes do, display included.
pub(super) fn sampled_slice(entry: &ModelEntry, settings: &CheckSettings, ctx: &GpuContext) -> Result<(), String> {
    let values = settings.default_values(entry)?;
    let mut state = build(entry, &values, ctx)?;
    for count in [0, 17, MAX_STEPS_PER_SUBMISSION] {
        let tick = state.tick() + u64::from(count);
        let submission = stepping::submit_slice(&mut *state, ctx, count, true);
        if !state.stats_readback_pending() {
            return Err(format!("A sampled slice of {count} steps began no readback."));
        }
        stepping::await_submission(ctx, submission).map_err(|fault| fault_text(&fault))?;
        if state.poll_stats_readback(&ctx.device, false) != StatsPoll::Landed {
            return Err(format!(
                "The readback of a finished slice of {count} steps did not land on the next poll."
            ));
        }
        if state.tick() != tick {
            return Err(format!(
                "The tick after a slice of {count} steps reads {}, not {tick}.",
                state.tick()
            ));
        }
        let sliced = state.stats();
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_sampled_slice_snapshot"),
        });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        if state.poll_stats_readback(&ctx.device, true) != StatsPoll::Landed {
            return Err(format!("The readback of a snapshot at tick {tick} did not land."));
        }
        let snapshot = state.stats();
        if let Some(difference) = first_stat_difference(&sliced, &snapshot) {
            return Err(format!(
                "A sampled slice and a snapshot differ in {difference} at tick {tick}."
            ));
        }
    }
    Ok(())
}
