//! Checks that compare two runs of one model: at two thread counts, on one seed, on two seeds, and at two sampling
//! cadences.

use std::ops::ControlFlow;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::GpuContext;
use henad_compute::simulation::{ExportError, RunSetup, Simulation};
use henad_core::action::Schedule;
use henad_core::metadata::Structure;
use henad_core::params::{ParamKind, ParamValue};
use henad_core::view::StatEntry;

use super::settings::CheckSettings;
use super::{Ran, SEED, SkipReason, fault_text, first_stat_difference};

/// Coarser of the two cadences [`super::ModelCheck::SamplingCadence`] compares. The finer samples every tick.
pub(super) const COARSE_CADENCE: u64 = 7;

/// End of one run: its stats and, for a CPU model, its state as `--export` writes it.
struct RunEnd {
    stats: Vec<StatEntry>,
    state: Option<Vec<u8>>,
}

impl RunEnd {
    /// Returns what differs between `self` and `other`, or `None` when they agree bit for bit.
    fn difference(&self, other: &Self) -> Option<String> {
        first_stat_difference(&self.stats, &other.stats)
            .or_else(|| (self.state != other.state).then(|| "the exported state".to_owned()))
    }
}

/// Builds `entry` at `values` on `seed`, on `gpu` for a GPU model.
fn simulation(
    entry: &ModelEntry,
    values: &[ParamValue],
    seed: u64,
    gpu: Option<&GpuContext>,
) -> Result<Simulation, String> {
    RunSetup::from_parts(entry, values, Some(seed), Schedule::default())
        .map_err(|error| format!("The check values were refused: {error}."))?
        .build(gpu)
        .map_err(|fault| format!("The model did not build. {}", fault_text(&fault)))
}

/// Returns the state of `simulation` as `--export` writes it, `None` for a GPU model.
fn exported(simulation: &mut Simulation) -> Result<Option<Vec<u8>>, String> {
    let mut bytes = Vec::new();
    match simulation.write_state(&mut bytes) {
        Ok(()) => Ok(Some(bytes)),
        Err(ExportError::GpuState) => Ok(None),
        Err(error) => Err(format!("The state was not exported: {error}.")),
    }
}

/// Runs `entry` at `values` on `seed` for `ticks` ticks and returns its end.
fn run(
    entry: &ModelEntry,
    values: &[ParamValue],
    seed: u64,
    ticks: u64,
    gpu: Option<&GpuContext>,
) -> Result<RunEnd, String> {
    let mut simulation = simulation(entry, values, seed, gpu)?;
    simulation.run_for(ticks).map_err(|fault| fault_text(&fault))?;
    let stats = simulation
        .stats()
        .map_err(|fault| fault_text(&fault))?
        .entries()
        .to_vec();
    let state = exported(&mut simulation)?;
    Ok(RunEnd { stats, state })
}

/// Checks [`super::ModelCheck::ThreadCount`] at a size that splits a step into twice the high thread count in jobs.
///
/// At one job a kernel with order-dependent interior mutability, such as a shared accumulator, agrees with itself at
/// any thread count.
pub(super) fn thread_count(entry: &ModelEntry, settings: &CheckSettings) -> Result<Ran, String> {
    let (low, high) = settings.low_and_high_threads();
    let ticks = settings.run_ticks();
    let mut values = settings.check_values(entry)?;
    let pinned = size_for_jobs(entry, settings, &mut values, high.saturating_mul(2))?;
    let jobs = jobs_at(entry, &values)?;
    if jobs <= 1 {
        return Ok(Ran::Skipped(match pinned {
            Some(param_id) => SkipReason::OneJobAtOverride(param_id.to_owned()),
            None => SkipReason::OneJob,
        }));
    }
    let at_low = in_pool(low, || run(entry, &values, SEED, ticks, None))?;
    let at_high = in_pool(high, || run(entry, &values, SEED, ticks, None))?;
    match at_low.difference(&at_high) {
        None => Ok(Ran::PassedAtJobs(jobs)),
        Some(difference) => Err(format!(
            "Runs at {low} and {high} threads differ in {difference} after {ticks} ticks, with a step split into \
             {jobs} jobs."
        )),
    }
}

/// Checks [`super::ModelCheck::SameSeed`].
pub(super) fn same_seed(entry: &ModelEntry, settings: &CheckSettings, gpu: Option<&GpuContext>) -> Result<(), String> {
    let values = settings.check_values(entry)?;
    let ticks = settings.run_ticks();
    let first = run(entry, &values, SEED, ticks, gpu)?;
    let second = run(entry, &values, SEED, ticks, gpu)?;
    match first.difference(&second) {
        None => Ok(()),
        Some(difference) => Err(format!(
            "Two builds on seed {SEED} differ in {difference} after {ticks} ticks."
        )),
    }
}

/// Checks [`super::ModelCheck::SeedSensitivity`] over the stats and, for a CPU model, the exported state.
pub(super) fn seed_sensitivity(
    entry: &ModelEntry,
    settings: &CheckSettings,
    gpu: Option<&GpuContext>,
) -> Result<(), String> {
    let values = settings.check_values(entry)?;
    let ticks = settings.run_ticks();
    let other = SEED + 1;
    let first = run(entry, &values, SEED, ticks, gpu)?;
    let second = run(entry, &values, other, ticks, gpu)?;
    match first.difference(&second) {
        Some(_) => Ok(()),
        None => Err(format!(
            "Seeds {SEED} and {other} give the same stats and state after {ticks} ticks. A model that draws no \
             random number takes an exemption."
        )),
    }
}

/// Checks [`super::ModelCheck::SamplingCadence`]. A sample prepares the views, and a view preparation that changes
/// the state would make a run's results depend on how often it is sampled.
pub(super) fn sampling_cadence(
    entry: &ModelEntry,
    settings: &CheckSettings,
    gpu: Option<&GpuContext>,
) -> Result<(), String> {
    let values = settings.check_values(entry)?;
    let ticks = settings.run_ticks();
    let (every_tick, every_tick_end) = sampled(entry, &values, ticks, 1, gpu)?;
    let (coarse, coarse_end) = sampled(entry, &values, ticks, COARSE_CADENCE, gpu)?;
    for (tick, stats) in &coarse {
        let Some((_, fine)) = every_tick.iter().find(|(fine_tick, _)| fine_tick == tick) else {
            return Err(format!("The run sampled every tick took no sample at tick {tick}."));
        };
        if let Some(difference) = first_stat_difference(fine, stats) {
            return Err(format!(
                "Runs sampled every tick and every {COARSE_CADENCE} ticks differ in {difference} at tick {tick}."
            ));
        }
    }
    if every_tick_end != coarse_end {
        return Err(format!(
            "Runs sampled every tick and every {COARSE_CADENCE} ticks export different states at tick {ticks}."
        ));
    }
    Ok(())
}

/// Stats of each sample of a run, by tick.
type Samples = Vec<(u64, Vec<StatEntry>)>;

/// Runs `entry` at `values` to `ticks`, sampling every `interval` ticks, and returns the samples and the exported
/// state at the end.
fn sampled(
    entry: &ModelEntry,
    values: &[ParamValue],
    ticks: u64,
    interval: u64,
    gpu: Option<&GpuContext>,
) -> Result<(Samples, Option<Vec<u8>>), String> {
    let mut simulation = simulation(entry, values, SEED, gpu)?;
    let mut samples = Vec::new();
    let end = simulation
        .run_sampled::<()>(ticks, interval, |sample| {
            samples.push((sample.tick(), sample.entries().to_vec()));
            ControlFlow::Continue(())
        })
        .map_err(|fault| fault_text(&fault))?;
    if end.is_break() {
        return Err(format!(
            "A run sampled every {interval} ticks stopped before tick {ticks}."
        ));
    }
    let state = exported(&mut simulation)?;
    Ok((samples, state))
}

/// Runs `task` inside a rayon pool of `threads` workers.
fn in_pool<T: Send>(threads: usize, task: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|error| format!("A pool of {threads} threads was not built: {error}."))?
        .install(task)
}

/// Returns the number of jobs a step of `entry` at `values` splits into.
fn jobs_at(entry: &ModelEntry, values: &[ParamValue]) -> Result<usize, String> {
    let mut built = entry
        .build(values, Some(SEED), None)
        .map_err(|fault| format!("The model did not build. {}", fault_text(&fault)))?;
    let jobs = match &mut built {
        ModelState::Cpu(state) => state.parallel_jobs(),
        ModelState::Gpu(_) => None,
    };
    Ok(jobs.unwrap_or(1))
}

/// Sets the size parameter of `entry` in `values` so a step splits into as close to `target` jobs as its bounds
/// allow, from below.
///
/// An agent or network model takes `num_agents` at `target` chunks. A grid model takes the largest `grid_height`
/// that splits into no more than `target` jobs, found by building it, since the rows a job holds depend on the width.
/// A parameter an override sets keeps its value.
///
/// Returns the id of that parameter when an override sets it, `None` otherwise.
fn size_for_jobs(
    entry: &ModelEntry,
    settings: &CheckSettings,
    values: &mut [ParamValue],
    target: usize,
) -> Result<Option<&'static str>, String> {
    let (param_id, chunk) = match &entry.metadata().structure {
        Structure::Agents { chunk, .. } | Structure::Network { chunk, .. } => ("num_agents", Some(*chunk)),
        Structure::Grid { .. } => ("grid_height", None),
        Structure::GpuGrid { .. } | Structure::GpuAgents { .. } => return Ok(None),
    };
    if settings.overrides(entry, param_id) {
        return Ok(Some(param_id));
    }
    let Some(index) = entry.param_index(param_id) else {
        return Ok(None);
    };
    let ParamKind::U32 { min, max, .. } = entry.param_descriptors()[index].kind else {
        return Ok(None);
    };
    let within = |size: usize| u32::try_from(size).unwrap_or(u32::MAX).clamp(min, max);
    if let Some(chunk) = chunk {
        values[index] = ParamValue::U32(within(target.saturating_mul(chunk)));
        return Ok(None);
    }

    let ParamValue::U32(start) = values[index] else {
        return Ok(None);
    };
    let mut jobs_with = |size: u32| {
        values[index] = ParamValue::U32(size);
        jobs_at(entry, values)
    };
    // Doubles until a size splits into too many jobs, then bisects for the largest size that does not. A start that
    // already splits into too many, as a grid an override widens can, bisects up from the lower bound instead.
    let (mut fits, mut too_many) = if jobs_with(start)? > target {
        (min, Some(start))
    } else {
        (start, None)
    };
    if too_many.is_none() {
        while fits < max {
            let next = fits.saturating_mul(2).min(max);
            if jobs_with(next)? > target {
                too_many = Some(next);
                break;
            }
            fits = next;
        }
    }
    if let Some(mut too_many) = too_many {
        while too_many - fits > 1 {
            let middle = fits + (too_many - fits) / 2;
            if jobs_with(middle)? <= target {
                fits = middle;
            } else {
                too_many = middle;
            }
        }
    }
    values[index] = ParamValue::U32(fits);
    Ok(None)
}
