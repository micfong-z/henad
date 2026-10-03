//! Run cursors, each holding one CPU run of a sweep from its build to its [`RunOutcome`].
//!
//! A cursor steps its run a slice at a time, fires the run's actions and samples it at every tick its
//! [`MeasurePlan`] names. The CPU executors drive their runs through cursors. A GPU run steps on a track of
//! `exec::gpu` instead, and both end a run through `run_outcome`.

use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::{BUILDING, Fault, FaultKind, STEPPING, catching};
use henad_core::action::{RefusedActions, Schedule, Scheduled};
use henad_core::explore::measure::{MeasurePlan, Sampler};
use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus, StopReason};
use henad_core::export::StatsWriteError;
use henad_core::model::SimState;

use crate::exec::RunRequest;

/// Reason a cursor refuses to build a GPU model.
#[cfg(not(target_arch = "wasm32"))]
const GPU_REFUSAL: &str = "a GPU model cannot run on a CPU lane";

/// Reason a cursor refuses to build a GPU model. A browser cannot block on the device.
#[cfg(target_arch = "wasm32")]
const GPU_REFUSAL: &str = "a GPU sweep needs a native build";

/// Result of one [`RunCursor::advance`].
#[derive(Debug)]
pub enum CursorState {
    /// The run has ticks left to step.
    Running,
    /// The run has ended, at its final tick, on its stop condition, on a fault or past its timeout.
    Finished(RunOutcome),
}

/// One CPU run of a sweep, built and stepped a slice at a time.
///
/// A run steps from tick 0 to the plan's total, unless its stop condition holds at a sample first. The actions due
/// at tick 0 fire before the first step, and those due at a later tick after the step that reaches it.
pub struct RunCursor {
    run: PlannedRun,
    run_key: u64,
    phase: Phase,
}

/// Prints the run, and leaves out the live simulation.
impl std::fmt::Debug for RunCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunCursor")
            .field("run", &self.run)
            .field("run_key", &self.run_key)
            .finish_non_exhaustive()
    }
}

enum Phase {
    Live(Box<LiveRun>),
    /// A run whose build failed, holding the outcome to hand out.
    BuildFailed(Box<RunOutcome>),
    /// A run whose outcome was handed out.
    Spent,
}

/// A built model and the ticks its run has reached.
struct LiveRun {
    state: Box<dyn SimState>,
    timeline: Timeline,
    schedule: Schedule,
    build_ms: f64,
    /// Time spent stepping and sampling.
    wall: Duration,
    /// Time spent stepping and sampling after which the run is abandoned, checked between slices.
    timeout: Option<Duration>,
}

/// Ticks a run has reached, and the samples taken on the way.
struct Timeline {
    sampler: Sampler,
    /// Tick the model has reached.
    tick: u64,
    /// Next tick to sample, `None` once the run has taken its last sample.
    next_sample: Option<u64>,
    /// Population at the latest sample.
    population: u64,
    /// Whether the actions due at tick 0 have fired.
    fired_start_actions: bool,
    /// One note per action the model refused, in the order they were due.
    refusals: Vec<String>,
}

/// Status and note of a run that ended on a fault.
#[derive(Debug, Clone)]
pub(crate) struct RunFailure {
    pub(crate) status: RunStatus,
    pub(crate) note: String,
}

impl From<Fault> for RunFailure {
    fn from(fault: Fault) -> Self {
        let status = match fault.kind {
            FaultKind::Panic { .. } => RunStatus::Panicked,
            FaultKind::Refused(_) => RunStatus::Refused,
            // A device error, a failed wait or a lost device.
            _ => RunStatus::GpuError,
        };
        Self {
            status,
            note: fault.to_string(),
        }
    }
}

impl From<StatsWriteError> for RunFailure {
    fn from(error: StatsWriteError) -> Self {
        Self {
            status: RunStatus::ShapeError,
            note: error.to_string(),
        }
    }
}

/// End of a run whose model was built.
pub(crate) enum RunEnd {
    /// The run took its last sample, for the reason inside.
    Stopped(StopReason),
    Failed(RunFailure),
    /// The run passed its timeout with ticks left.
    TimedOut,
}

/// Parts of a [`RunOutcome`] gathered while its run stepped.
pub(crate) struct OutcomeParts {
    pub(crate) sampler: Sampler,
    /// Tick the run ended on.
    pub(crate) ticks: u64,
    /// Population at the latest sample.
    pub(crate) population: u64,
    pub(crate) build_ms: f64,
    /// Time the run was live, builds and pauses left out. A GPU track counts an even share of each round among the
    /// live tracks.
    pub(crate) wall: Duration,
    pub(crate) timeout: Option<Duration>,
    /// One note per action the model refused, in the order they were due.
    pub(crate) refusals: Vec<String>,
}

/// Returns the outcome of `run` once it ends as `end`.
///
/// A run that reached its end with a non-finite sample is `non_finite`. The notes of the refused actions come
/// before the note of the end.
pub(crate) fn run_outcome(run: PlannedRun, run_key: u64, end: RunEnd, parts: OutcomeParts) -> RunOutcome {
    let ticks = parts.ticks;
    let measured = parts.sampler.finish();
    let (status, stop_reason, note) = match (end, measured.non_finite) {
        (RunEnd::Failed(failure), _) => (failure.status, StopReason::Fault, Some(failure.note)),
        (RunEnd::TimedOut, _) => {
            let seconds = parts.timeout.unwrap_or_default().as_secs_f64();
            let note = format!("timed out after {seconds} s at tick {ticks}");
            (RunStatus::TimedOut, StopReason::Timeout, Some(note))
        }
        (RunEnd::Stopped(reason), Some(sample)) => (RunStatus::NonFinite, reason, Some(sample.to_string())),
        (RunEnd::Stopped(reason), None) => (RunStatus::Ok, reason, None),
    };
    let mut notes = parts.refusals;
    notes.extend(note);
    RunOutcome {
        run,
        run_key,
        status,
        stop_reason,
        ticks,
        population: parts.population,
        build_ms: parts.build_ms,
        wall_ms: milliseconds(parts.wall),
        reducers: measured.reducers,
        series: measured.series,
        note: (!notes.is_empty()).then(|| notes.join("; ")),
    }
}

/// Returns the outcome of `run`, whose build failed with `failure` after `build_ms` milliseconds.
pub(crate) fn failed_build_outcome(
    run: PlannedRun,
    run_key: u64,
    measure: &Arc<MeasurePlan>,
    build_ms: f64,
    failure: RunFailure,
) -> RunOutcome {
    let measured = Sampler::new(Arc::clone(measure)).finish();
    RunOutcome {
        run,
        run_key,
        status: failure.status,
        stop_reason: StopReason::Fault,
        ticks: 0,
        population: 0,
        build_ms,
        wall_ms: 0.0,
        reducers: measured.reducers,
        series: measured.series,
        note: Some(failure.note),
    }
}

/// Returns the note of `action`, refused by the model.
pub(crate) fn refusal_note(action: &Scheduled) -> String {
    format!("model refused action '{}' at tick {}", action.id, action.tick)
}

impl RunCursor {
    /// Builds the model of `request` with the request's seed, and times the build.
    ///
    /// A run ends with the slice in which its stepping and sampling pass `timeout`. A cursor whose build failed
    /// finishes on its first [`Self::advance`], with the failure as its status. A GPU model is refused.
    pub fn new(
        entry: &ModelEntry,
        measure: &Arc<MeasurePlan>,
        request: &RunRequest<'_>,
        timeout: Option<Duration>,
    ) -> Self {
        let started = Instant::now();
        let built = if entry.gpu_needs().is_some() {
            Err(Fault::refused(BUILDING, GPU_REFUSAL))
        } else {
            entry
                .build(request.params, Some(request.run.seed), None)
                .and_then(cpu_state)
        };
        let build_ms = milliseconds(started.elapsed());
        let phase = match built {
            Ok(state) => Phase::Live(Box::new(LiveRun {
                state,
                timeline: Timeline {
                    sampler: Sampler::new(Arc::clone(measure)),
                    tick: 0,
                    next_sample: Some(measure.first_sample()),
                    population: 0,
                    fired_start_actions: false,
                    refusals: Vec::new(),
                },
                schedule: request.schedule.clone(),
                build_ms,
                wall: Duration::ZERO,
                timeout,
            })),
            Err(fault) => Phase::BuildFailed(Box::new(failed_build_outcome(
                request.run,
                request.run_key,
                measure,
                build_ms,
                fault.into(),
            ))),
        };
        Self {
            run: request.run,
            run_key: request.run_key,
            phase,
        }
    }

    /// Tick the run has reached, 0 for a run whose build failed.
    pub fn tick(&self) -> u64 {
        match &self.phase {
            Phase::Live(live) => live.timeline.tick,
            Phase::BuildFailed(_) | Phase::Spent => 0,
        }
    }

    /// Steps the run by at most `max_steps` steps, firing the actions and taking the samples due on the way.
    ///
    /// A sample due at the tick the run has reached is taken even when `max_steps` is 0. A fault ends the run, and
    /// its outcome keeps the samples taken before it.
    ///
    /// # Panics
    ///
    /// Panics when called again after it returned [`CursorState::Finished`].
    pub fn advance(&mut self, max_steps: u64) -> CursorState {
        match std::mem::replace(&mut self.phase, Phase::Spent) {
            Phase::Live(mut live) => match live.advance(max_steps) {
                None => {
                    self.phase = Phase::Live(live);
                    CursorState::Running
                }
                Some(end) => CursorState::Finished(live.finish(self.run, self.run_key, end)),
            },
            Phase::BuildFailed(outcome) => CursorState::Finished(*outcome),
            Phase::Spent => panic!("run {} already finished", self.run.run_id),
        }
    }
}

impl LiveRun {
    /// Steps by at most `max_steps` steps, and returns the end of the run once it has one.
    fn advance(&mut self, max_steps: u64) -> Option<RunEnd> {
        let started = Instant::now();
        let (timeline, schedule, state) = (&mut self.timeline, &self.schedule, &mut self.state);
        let driven = catching(STEPPING, || timeline.drive(&mut **state, schedule, max_steps));
        self.wall += started.elapsed();
        match driven {
            Ok(Ok(None)) if self.timeout.is_some_and(|timeout| self.wall >= timeout) => Some(RunEnd::TimedOut),
            Ok(Ok(None)) => None,
            Ok(Ok(Some(reason))) => Some(RunEnd::Stopped(reason)),
            Ok(Err(failure)) => Some(RunEnd::Failed(failure)),
            Err(fault) => Some(RunEnd::Failed(fault.into())),
        }
    }

    fn finish(self, run: PlannedRun, run_key: u64, end: RunEnd) -> RunOutcome {
        let ticks = match &end {
            RunEnd::Stopped(_) | RunEnd::TimedOut => self.timeline.tick,
            RunEnd::Failed(_) => self.state.tick(),
        };
        let parts = OutcomeParts {
            sampler: self.timeline.sampler,
            ticks,
            population: self.timeline.population,
            build_ms: self.build_ms,
            wall: self.wall,
            timeout: self.timeout,
            refusals: self.timeline.refusals,
        };
        run_outcome(run, run_key, end, parts)
    }
}

impl Timeline {
    /// Steps, fires and samples until `max_steps` steps are spent or the run takes its last sample.
    ///
    /// Returns the reason the run ended, or `None` while it has ticks left.
    fn drive(
        &mut self,
        state: &mut dyn SimState,
        schedule: &Schedule,
        max_steps: u64,
    ) -> Result<Option<StopReason>, RunFailure> {
        if !self.fired_start_actions {
            self.fired_start_actions = true;
            note_refused(schedule.run_due(state), &mut self.refusals);
        }
        let mut budget = max_steps;
        loop {
            let Some(next_sample) = self.next_sample else {
                return Ok(Some(StopReason::Steps));
            };
            if self.tick == next_sample {
                // The view is prepared first, as a publish does. A stat that walks the graph is computed there.
                state.prepare_view();
                let stats = state.stats();
                self.population = state.population();
                let stops = self.sampler.push(self.tick, &stats)?;
                if stops {
                    self.next_sample = None;
                    return Ok(Some(StopReason::Condition));
                }
                self.next_sample = self.sampler.plan().next_sample(self.tick);
            } else if budget == 0 {
                return Ok(None);
            } else {
                let count = (next_sample - self.tick).min(budget);
                step_by(state, count, schedule, &mut self.refusals);
                self.tick += count;
                budget -= count;
            }
        }
    }
}

/// Steps `state` by `count` ticks, firing the actions due after each step, and adds a note to `refusals` for each
/// one refused.
fn step_by(state: &mut dyn SimState, count: u64, schedule: &Schedule, refusals: &mut Vec<String>) {
    if schedule.is_empty() {
        for _ in 0..count {
            state.step();
        }
    } else {
        for _ in 0..count {
            state.step();
            note_refused(schedule.run_due(state), refusals);
        }
    }
}

/// Adds a note to `refusals` for each action in `refused`.
fn note_refused(refused: RefusedActions<'_>, refusals: &mut Vec<String>) {
    refusals.extend(refused.into_iter().map(refusal_note));
}

/// Returns the CPU state of `state`, refusing a GPU model.
fn cpu_state(state: ModelState) -> Result<Box<dyn SimState>, Fault> {
    match state {
        ModelState::Cpu(state) => Ok(state),
        ModelState::Gpu(_) => Err(Fault::refused(BUILDING, GPU_REFUSAL)),
    }
}

pub(crate) fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use henad_compute::entry::{ModelEntry, ModelState, register_grid_model};
    use henad_compute::fault::install_panic_hook;
    use henad_core::action::Schedule;
    use henad_core::explore::measure::MeasurePlan;
    use henad_core::explore::outcome::{PlannedRun, RunStatus, StopReason};
    use henad_core::explore::spec::{MeasureSettings, RunSettings};
    use henad_core::export::StatColumns;
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_models::example_models;

    use super::{CursorState, RunCursor};
    use crate::exec::RunRequest;
    use crate::tests::broken::DividesByParam;

    const SEED: u64 = 11;

    fn entry(id: &str) -> ModelEntry {
        example_models().get(id).cloned().expect("the model is registered")
    }

    fn cpu_state(entry: &ModelEntry, params: &[ParamValue]) -> Box<dyn SimState> {
        match entry.build(params, Some(SEED), None) {
            Ok(ModelState::Cpu(state)) => state,
            _ => panic!("{} builds on the CPU", entry.id()),
        }
    }

    fn params(entry: &ModelEntry, width: u32) -> Vec<ParamValue> {
        let mut params: Vec<ParamValue> = entry
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        params[0] = ParamValue::U32(width);
        params[1] = ParamValue::U32(width);
        params
    }

    fn measure(entry: &ModelEntry, params: &[ParamValue], warmup: u64, steps: u64, stats_every: u64) -> MeasurePlan {
        let mut probe = cpu_state(entry, params);
        probe.prepare_view();
        let run = RunSettings {
            steps,
            warmup,
            ..RunSettings::default()
        };
        let settings = MeasureSettings {
            stats_every,
            series_every: stats_every,
            ..MeasureSettings::default()
        };
        MeasurePlan::new(&run, &settings, StatColumns::plan(&probe.stats())).expect("the columns bind")
    }

    fn request(params: &[ParamValue]) -> RunRequest<'_> {
        RunRequest {
            run: PlannedRun {
                run_id: 3,
                config_id: 1,
                rep: 0,
                seed: SEED,
            },
            run_key: 99,
            params,
            schedule: Schedule::from_entries(Vec::new()),
        }
    }

    /// Advances `cursor` in slices of `slice` steps until it finishes.
    fn drive(cursor: &mut RunCursor, slice: u64) -> henad_core::explore::outcome::RunOutcome {
        loop {
            if let CursorState::Finished(outcome) = cursor.advance(slice) {
                return outcome;
            }
        }
    }

    #[test]
    fn a_cursor_samples_what_a_hand_stepped_run_gives() {
        let entry = entry("sir");
        let params = params(&entry, 24);
        let plan = Arc::new(measure(&entry, &params, 2, 20, 6));
        let mut cursor = RunCursor::new(&entry, &plan, &request(&params), None);
        let outcome = drive(&mut cursor, 5);
        assert_eq!(outcome.status, RunStatus::Ok);
        assert_eq!(outcome.stop_reason, StopReason::Steps);
        assert_eq!((outcome.ticks, outcome.run.run_id, outcome.run_key), (22, 3, 99));
        assert_eq!(outcome.series.ticks(), [2, 8, 14, 20, 22]);

        let mut state = cpu_state(&entry, &params);
        let mut row = Vec::new();
        for (i, &tick) in outcome.series.ticks().iter().enumerate() {
            while state.tick() < tick {
                state.step();
            }
            state.prepare_view();
            plan.columns()
                .extract(tick, &state.stats(), &mut row)
                .expect("the layout holds");
            assert_eq!(outcome.series.row(i), row.as_slice(), "tick {tick}");
        }
        assert_eq!(outcome.population, state.population());
    }

    #[test]
    fn the_slice_size_does_not_change_the_outcome() {
        let entry = entry("game_of_life");
        let params = params(&entry, 16);
        let plan = Arc::new(measure(&entry, &params, 0, 30, 4));
        let outcomes: Vec<_> = [1, 3, 1000]
            .into_iter()
            .map(|slice| {
                let mut cursor = RunCursor::new(&entry, &plan, &request(&params), None);
                let outcome = drive(&mut cursor, slice);
                (outcome.reducers, outcome.series, outcome.ticks)
            })
            .collect();
        assert_eq!(outcomes[0], outcomes[1]);
        assert_eq!(outcomes[0], outcomes[2]);
    }

    #[test]
    fn a_failed_build_finishes_on_the_first_advance() {
        install_panic_hook();
        let entry = register_grid_model::<DividesByParam>();
        let good = params(&entry, 8);
        let plan = Arc::new(measure(&entry, &good, 0, 10, 5));
        let mut broken = good.clone();
        broken[3] = ParamValue::U32(0);
        let mut cursor = RunCursor::new(&entry, &plan, &request(&broken), None);
        let CursorState::Finished(outcome) = cursor.advance(0) else {
            panic!("a failed build has nothing to step");
        };
        assert_eq!(outcome.status, RunStatus::Panicked);
        assert_eq!((outcome.stop_reason, outcome.ticks), (StopReason::Fault, 0));
        assert!(outcome.series.is_empty());
        assert!(outcome.reducers.iter().all(Option::is_none));
        let note = outcome.note.unwrap_or_default();
        assert!(note.starts_with("while building the model"), "{note}");
        assert!(note.contains("broken.rs:"), "{note}");
    }

    #[test]
    fn a_panic_mid_run_keeps_the_samples_before_it() {
        install_panic_hook();
        let entry = register_grid_model::<DividesByParam>();
        let good = params(&entry, 8);
        let plan = Arc::new(measure(&entry, &good, 0, 10, 5));
        let mut broken = good.clone();
        broken[2] = ParamValue::U32(0);
        let mut cursor = RunCursor::new(&entry, &plan, &request(&broken), None);
        let outcome = drive(&mut cursor, 100);
        assert_eq!(outcome.status, RunStatus::Panicked);
        assert_eq!((outcome.stop_reason, outcome.ticks), (StopReason::Fault, 0));
        assert_eq!(
            outcome.series.ticks(),
            [0],
            "the sample at tick 0 came before the first step"
        );
        assert_eq!(outcome.reducers[0], Some(1.0), "Cells:final");
        let note = outcome.note.unwrap_or_default();
        assert!(note.starts_with("while stepping the simulation"), "{note}");
        assert!(note.contains("broken.rs:"), "{note}");

        let mut cursor = RunCursor::new(&entry, &plan, &request(&good), None);
        assert_eq!(
            drive(&mut cursor, 100).status,
            RunStatus::Ok,
            "a divisor of 1 runs clean"
        );
    }
}
