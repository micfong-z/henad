//! Sweeps and searches stepped from a host's frames, one CPU run at a time, for a target that cannot spawn a thread.
//!
//! A pumped sweep holds its files in memory. Each pump makes one probe build, prepares the sweep, builds a run, or
//! steps the run in progress by a slice of about half the frame budget. A search also asks for a batch, or tells the
//! searcher a finished one, in a pump of its own.

use std::sync::Arc;

use web_time::Instant;

use henad_compute::entry::ModelEntry;
use henad_compute::runner::{PUMP_BUDGET_MS, Pace, SimLoop};
use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::Backend;

use crate::cursor::{CursorState, RunCursor};
use crate::exec::{ActiveRuns, BatchEnd, Concurrency, ExecutionError, RunRequest, RunWatch, SliceSize, SweepControl};
use crate::handle::{SweepChannel, SweepRunOptions, SweepStartError};
use crate::output::OutputWriter;
use crate::output::manifest::{Manifest, ManifestRuntime};
use crate::output::memory::{SweepFiles, memory_writer};
use crate::probe::{PlanProbe, TimedProbe};
use crate::progress::{Progress as _, ProgressEvent};
use crate::search_run::{AskedBatch, SearchPlan, SearchPreparation, SearchWriters, watched_values};
use crate::sweep::{ExploreError, SweepInputs, SweepOptions, SweepPreparation, SweepRecord, finish_manifest};

/// Wall time in milliseconds one slice of steps aims to take.
const PUMP_SLICE_MS: f64 = PUMP_BUDGET_MS / 2.0;

/// Command to a sweep pumped from a host's frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepCommand {
    Pause,
    Resume,
    /// Ends the sweep at its next pump, writing the runs finished so far.
    Abort,
}

/// A sweep or search of a CPU model, advanced by [`SimLoop::pump`].
pub struct PumpedSweep {
    setup: SweepSetup,
    channel: SweepChannel,
    stage: PumpStage,
}

/// Prints the model and the plan's size, and leaves out the live runs.
impl std::fmt::Debug for PumpedSweep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PumpedSweep")
            .field("model", &self.setup.entry.id())
            .field("runs", &self.setup.plan.run_count())
            .finish_non_exhaustive()
    }
}

/// Model, spec and settings of a pumped sweep.
struct SweepSetup {
    entry: ModelEntry,
    spec: SweepSpec,
    plan: Arc<Plan>,
    /// Plan of a search, `None` for a sweep.
    search_plan: Option<Arc<SearchPlan>>,
    runtime: ManifestRuntime,
    options: SweepOptions,
    active_runs: ActiveRuns,
}

impl SweepSetup {
    fn inputs(&self) -> SweepInputs<'_> {
        SweepInputs {
            entry: &self.entry,
            gpu: None,
            runtime: &self.runtime,
            spec: &self.spec,
            source: &self.options.spec_source,
            provenance: &self.options.provenance,
            options: &self.options,
            folder: None,
            dry_run: false,
        }
    }

    /// Plan the probe builds, the plan of a search's fixed values for a search.
    fn probed_plan(&self) -> &Plan {
        self.search_plan
            .as_ref()
            .map_or(&self.plan, |search_plan| search_plan.base())
    }
}

/// Stage of a pumped sweep.
enum PumpStage {
    /// Planned, with the probe building one config per pump until one builds.
    Probing(PlanProbe),
    /// Probed, with the sweep to prepare at the next pump.
    Probed(Box<TimedProbe>),
    Running(Box<RunQueue>),
    Searching(Box<SearchQueue>),
    Ended,
}

/// Runs of a probed sweep, and the files they are written to.
struct RunQueue {
    preparation: SweepPreparation,
    manifest: Manifest,
    writer: OutputWriter<Vec<u8>>,
    /// Position in the pending runs of the next run to build.
    next_position: usize,
    /// Run in progress, `None` between two runs.
    current: Option<CurrentRun>,
}

/// Batches of a probed search, and the files they are written to.
struct SearchQueue {
    preparation: SearchPreparation,
    manifest: Manifest,
    output: SearchWriters<Vec<u8>>,
    /// Batch in progress, `None` between two batches.
    batch: Option<PumpedBatch>,
}

/// Batch of a search in progress.
struct PumpedBatch {
    batch: AskedBatch,
    /// Position among the batch's runs left to run of the next run to build.
    next_position: usize,
    /// Watched values of each run written, in run order.
    values: Vec<Vec<Option<f64>>>,
    /// Run in progress, `None` between two runs.
    current: Option<CurrentRun>,
}

/// Run in progress, with its entry in the active runs and the size of its next slice.
struct CurrentRun {
    cursor: RunCursor,
    watch: RunWatch,
    slice: SliceSize,
}

/// Work one pump of a running sweep did.
enum PumpWork {
    /// A run was built or stepped, or written once it finished.
    Stepped,
    /// Every run is written.
    Drained,
    /// The control aborted the sweep with runs left.
    Aborted,
}

impl PumpedSweep {
    /// Returns a sweep of `plan`, the plan of `spec` over `entry`, that reports to `channel`.
    ///
    /// The sweep runs every run of the plan, one at a time, and holds its files in memory. With a `search_plan`, the
    /// search runs in place of the plan, whose one config its candidates start from. `options.concurrency` is left
    /// unread.
    ///
    /// # Errors
    ///
    /// Returns [`SweepStartError::GpuNeedsNative`] for a GPU model.
    pub(crate) fn new(
        entry: ModelEntry,
        spec: SweepSpec,
        plan: Arc<Plan>,
        search_plan: Option<Arc<SearchPlan>>,
        channel: SweepChannel,
        options: SweepRunOptions,
    ) -> Result<Self, SweepStartError> {
        if entry.metadata().backend == Backend::Gpu {
            return Err(SweepStartError::GpuNeedsNative);
        }
        let active_runs = ActiveRuns::new();
        let mut sweep_options = SweepOptions::new(options.provenance);
        sweep_options.concurrency = Concurrency::Fixed(std::num::NonZeroUsize::MIN);
        sweep_options.memory_budget = options.memory_budget;
        sweep_options.gpu_memory = options.gpu_memory;
        sweep_options.active_runs = Some(active_runs.clone());
        sweep_options.spec_source = options.spec_source;
        Ok(Self {
            setup: SweepSetup {
                entry,
                spec,
                plan,
                search_plan,
                runtime: ManifestRuntime::new(None),
                options: sweep_options,
                active_runs,
            },
            channel,
            stage: PumpStage::Probing(PlanProbe::new()),
        })
    }

    /// Switch that pauses and aborts the sweep.
    pub fn control(&self) -> &SweepControl {
        &self.setup.options.control
    }

    /// Table of the run in progress.
    pub fn active_runs(&self) -> &ActiveRuns {
        &self.setup.active_runs
    }

    /// Prepares the sweep from `probe`, the report of its first config that builds, reports its outline and opens
    /// its files.
    fn prepare(&mut self, probe: TimedProbe) {
        if let Some(search_plan) = &self.setup.search_plan {
            return self.prepare_search(Arc::clone(search_plan), probe);
        }
        let inputs = self.setup.inputs();
        let preparation = match SweepPreparation::new(&inputs, Some(Arc::clone(&self.setup.plan)), Some(probe)) {
            Ok(preparation) => preparation,
            Err(error) => return self.fail(&error),
        };
        preparation.announce(&inputs, &mut self.channel);
        let opened = preparation.manifest(&inputs).and_then(|manifest| {
            let writer = memory_writer(
                preparation.plan(),
                inputs.entry.param_descriptors(),
                preparation.measure(),
            )?;
            Ok((manifest, writer))
        });
        match opened {
            Ok((manifest, writer)) => {
                self.stage = PumpStage::Running(Box::new(RunQueue {
                    preparation,
                    manifest,
                    writer,
                    next_position: 0,
                    current: None,
                }));
            }
            Err(error) => self.fail(&ExploreError::Output(error)),
        }
    }

    /// Prepares the search of `search_plan` from `probe`, the report of its fixed values, reports its outline and
    /// opens its files.
    fn prepare_search(&mut self, search_plan: Arc<SearchPlan>, probe: TimedProbe) {
        let inputs = self.setup.inputs();
        let preparation = match SearchPreparation::new(&inputs, Some(search_plan), Some(probe)) {
            Ok(preparation) => preparation,
            Err(error) => return self.fail(&error),
        };
        preparation.announce(&inputs, &mut self.channel);
        match preparation.memory_output(&inputs) {
            Ok((output, manifest)) => {
                self.stage = PumpStage::Searching(Box::new(SearchQueue {
                    preparation,
                    manifest,
                    output,
                    batch: None,
                }));
            }
            Err(error) => self.fail(&error),
        }
    }

    /// Writes the files of the runs written so far, and sends the record of a sweep whose batch ended as `end`.
    fn end(&mut self, end: BatchEnd) {
        let stage = std::mem::replace(&mut self.stage, PumpStage::Ended);
        if let PumpStage::Searching(search) = stage {
            let SearchQueue {
                preparation,
                manifest,
                output,
                ..
            } = *search;
            match preparation.finish_in_memory(output, manifest, end) {
                Ok(record) => {
                    self.channel.report(&ProgressEvent::Ended(&record.report));
                    self.channel.finish(record);
                }
                Err(error) => self.channel.fail(&error),
            }
            return;
        }
        let PumpStage::Running(runs) = stage else {
            return;
        };
        let RunQueue {
            preparation,
            mut manifest,
            writer,
            ..
        } = *runs;
        let counts = writer.counts();
        let end = finish_manifest(&mut manifest, end, counts);
        match SweepFiles::assemble(writer, &manifest) {
            Ok(files) => {
                let report = preparation.report(end, counts, None);
                self.channel.report(&ProgressEvent::Ended(&report));
                self.channel.finish(SweepRecord {
                    report,
                    manifest,
                    files: Some(files),
                    search: None,
                });
            }
            Err(error) => self.channel.fail(&ExploreError::Output(error)),
        }
    }

    fn fail(&mut self, error: &ExploreError) {
        self.stage = PumpStage::Ended;
        self.channel.fail(error);
    }
}

impl RunQueue {
    /// Returns whether every run is written.
    fn is_drained(&self) -> bool {
        self.current.is_none() && self.next_position >= self.preparation.pending().len()
    }

    /// Builds the next run, steps the run in progress by one slice, or writes it once it finishes.
    fn pump(&mut self, setup: &SweepSetup, channel: &mut SweepChannel) -> Result<PumpWork, ExploreError> {
        let Some(current) = &mut self.current else {
            let Some(&run) = self.preparation.pending().get(self.next_position) else {
                return Ok(PumpWork::Drained);
            };
            self.next_position += 1;
            let plan = self.preparation.plan();
            let measure = self.preparation.measure();
            let watch = setup.active_runs.watch(run, measure.total());
            let request = RunRequest::planned(plan, run);
            let cursor = RunCursor::new(&setup.entry, measure, &request, plan.run_settings().timeout);
            self.current = Some(CurrentRun {
                cursor,
                watch,
                slice: SliceSize::aiming_at(PUMP_SLICE_MS),
            });
            return Ok(PumpWork::Stepped);
        };
        let started = Instant::now();
        let state = current.cursor.advance(current.slice.steps());
        current.slice.adapt(started.elapsed());
        match state {
            CursorState::Running => current.watch.reach(current.cursor.tick()),
            CursorState::Finished(outcome) => {
                self.current = None;
                self.writer
                    .write_run(&outcome)
                    .map_err(|error| ExploreError::Execution(ExecutionError::Sink(error)))?;
                channel.report(&ProgressEvent::RunCommitted(&outcome));
            }
        }
        Ok(PumpWork::Stepped)
    }
}

impl SearchQueue {
    /// Returns whether the next pump builds a run or steps the run in progress.
    fn runs_next(&self) -> bool {
        self.batch
            .as_ref()
            .is_some_and(|pumped| pumped.current.is_some() || pumped.batch.request(pumped.next_position).is_some())
    }

    /// Asks for the next batch, builds the next run, steps the run in progress by one slice, writes it once it
    /// finishes, or tells the searcher a finished batch.
    fn pump(&mut self, setup: &SweepSetup, channel: &mut SweepChannel) -> Result<PumpWork, ExploreError> {
        let Some(pumped) = &mut self.batch else {
            return Ok(match self.output.session.ask()? {
                Some(batch) => {
                    self.batch = Some(PumpedBatch {
                        batch,
                        next_position: 0,
                        values: Vec::new(),
                        current: None,
                    });
                    PumpWork::Stepped
                }
                None => PumpWork::Drained,
            });
        };
        let measure = self.preparation.measure();
        if let Some(current) = &mut pumped.current {
            let started = Instant::now();
            let state = current.cursor.advance(current.slice.steps());
            current.slice.adapt(started.elapsed());
            match state {
                CursorState::Running => current.watch.reach(current.cursor.tick()),
                CursorState::Finished(outcome) => {
                    pumped.current = None;
                    self.output
                        .writer
                        .write_config_run(&outcome, pumped.batch.config_of(&outcome.run))
                        .map_err(|error| ExploreError::Execution(ExecutionError::Sink(error)))?;
                    pumped
                        .values
                        .push(watched_values(&outcome, self.output.session.watched_reducers()));
                    channel.report(&ProgressEvent::RunCommitted(&outcome));
                }
            }
            return Ok(PumpWork::Stepped);
        }
        if let Some(request) = pumped.batch.request(pumped.next_position) {
            pumped.next_position += 1;
            let watch = setup.active_runs.watch(request.run, measure.total());
            let timeout = self.preparation.plan().base().run_settings().timeout;
            let cursor = RunCursor::new(&setup.entry, measure, &request, timeout);
            pumped.current = Some(CurrentRun {
                cursor,
                watch,
                slice: SliceSize::aiming_at(PUMP_SLICE_MS),
            });
            return Ok(PumpWork::Stepped);
        }
        let PumpedBatch { batch, values, .. } = self.batch.take().expect("a batch is in progress");
        let update = self.output.session.tell(batch, values);
        self.output
            .tables
            .write_batch(&update)
            .map_err(|error| ExploreError::Execution(ExecutionError::Sink(error)))?;
        channel.report(&ProgressEvent::SearchBatchTold(&update));
        Ok(PumpWork::Stepped)
    }
}

impl SimLoop for PumpedSweep {
    type Command = SweepCommand;

    /// Applies `command` to the sweep's control. The sweep ends itself, so this never asks the driver to stop.
    fn handle_command(&mut self, command: SweepCommand) -> bool {
        let control = self.control();
        match command {
            SweepCommand::Pause => control.pause(),
            SweepCommand::Resume => control.resume(),
            SweepCommand::Abort => control.abort(),
        }
        false
    }

    fn pump(&mut self) -> Pace {
        let control = &self.setup.options.control;
        let pumped = match &mut self.stage {
            PumpStage::Ended => return Pace::Idle,
            PumpStage::Probing(probe) => match probe.step(&self.setup.entry, None, self.setup.probed_plan()) {
                Ok(None) => return Pace::Now,
                Ok(Some(timed)) => {
                    self.stage = PumpStage::Probed(Box::new(timed));
                    return Pace::Now;
                }
                Err(error) => Err(error.into()),
            },
            PumpStage::Probed(_) => {
                if let PumpStage::Probed(timed) = std::mem::replace(&mut self.stage, PumpStage::Ended) {
                    self.prepare(*timed);
                }
                return Pace::Now;
            }
            PumpStage::Running(runs) if control.is_aborted() => Ok(if runs.is_drained() {
                PumpWork::Drained
            } else {
                PumpWork::Aborted
            }),
            // A pause holds the runs alone. A sweep paused after its last run still ends.
            PumpStage::Running(runs) if control.is_paused() && !runs.is_drained() => return Pace::Idle,
            PumpStage::Running(runs) => runs.pump(&self.setup, &mut self.channel),
            PumpStage::Searching(_) if control.is_aborted() => Ok(PumpWork::Aborted),
            PumpStage::Searching(search) if control.is_paused() && search.runs_next() => return Pace::Idle,
            PumpStage::Searching(search) => search.pump(&self.setup, &mut self.channel),
        };
        match pumped {
            Ok(PumpWork::Stepped) => Pace::Now,
            Ok(PumpWork::Drained) => {
                self.end(BatchEnd::Complete);
                Pace::Idle
            }
            Ok(PumpWork::Aborted) => {
                self.end(BatchEnd::Aborted);
                Pace::Idle
            }
            Err(error) => {
                self.fail(&error);
                Pace::Idle
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    use henad_compute::entry::{ModelEntry, register_grid_model};
    use henad_compute::fault::install_panic_hook;
    use henad_compute::runner::{Pace, SimLoop as _};
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};

    use super::{PumpedSweep, SweepCommand};
    use crate::handle::{SweepChannel, SweepEvent, SweepRunOptions};
    use crate::output::manifest::now_unix_ms;
    use crate::probe::MAX_PROBED_CONFIGS;
    use crate::search_run::SearchPlan;
    use crate::sweep::SweepEnd;
    use crate::tests::broken::DividesByParam;
    use crate::tests::support::{entry, provenance};

    /// Most pumps a test makes before it gives up on the sweep.
    const MAX_PUMPS: usize = 10_000;

    fn options() -> SweepRunOptions {
        SweepRunOptions::new(provenance())
    }

    fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|&(id, value)| (id.to_owned(), value.to_owned()))
            .collect()
    }

    /// Returns a pumped sweep of `spec` over `entry`, a search when `spec` has one, and the receiver of its events.
    fn pumped(entry: ModelEntry, spec: SweepSpec) -> (PumpedSweep, Receiver<SweepEvent>) {
        let schema = entry.schema();
        let (plan, search_plan) = if spec.search.is_some() {
            let search_plan = Arc::new(SearchPlan::new(&spec, &schema).expect("a valid search"));
            (Arc::clone(search_plan.base()), Some(search_plan))
        } else {
            (Arc::new(spec.plan(&schema).expect("a valid spec")), None)
        };
        let (channel, events, _) = SweepChannel::open(&options());
        let sweep = PumpedSweep::new(entry, spec, plan, search_plan, channel, options()).expect("a CPU model pumps");
        (sweep, events)
    }

    /// Returns a sweep over `DividesByParam` on an 8 by 8 grid, varying `init_divisor` over `levels`.
    fn init_divisors(levels: &[&str]) -> SweepSpec {
        let mut spec = SweepSpec::new("divides_by_param");
        spec.fixed = fixed(&[("grid_width", "8"), ("grid_height", "8")]);
        spec.run.steps = 4;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "init_divisor",
                LevelSpec::Values(levels.iter().map(|&level| level.to_owned()).collect()),
            )],
            design_seed: None,
        }];
        spec
    }

    /// Pumps `sweep` until `events` has received `count` runs, and returns every event received.
    fn pump_until_runs(sweep: &mut PumpedSweep, events: &Receiver<SweepEvent>, count: usize) -> Vec<SweepEvent> {
        let mut received = Vec::new();
        for _ in 0..MAX_PUMPS {
            let runs = received
                .iter()
                .filter(|event| matches!(event, SweepEvent::RunFinished { .. }))
                .count();
            if runs == count {
                return received;
            }
            assert!(matches!(sweep.pump(), Pace::Now), "the sweep ended before its last run");
            received.extend(events.try_iter());
        }
        panic!("the sweep did not write {count} runs in {MAX_PUMPS} pumps");
    }

    /// Pumps `sweep` until it has nothing due, and returns the events `events` received meanwhile.
    fn pump_until_idle(sweep: &mut PumpedSweep, events: &Receiver<SweepEvent>) -> Vec<SweepEvent> {
        for _ in 0..MAX_PUMPS {
            if matches!(sweep.pump(), Pace::Idle) {
                return events.try_iter().collect();
            }
        }
        panic!("the sweep never went idle in {MAX_PUMPS} pumps");
    }

    fn finished_end(events: &[SweepEvent]) -> Option<SweepEnd> {
        match events.last() {
            Some(SweepEvent::Finished(record)) => Some(record.report.end),
            _ => None,
        }
    }

    #[test]
    fn each_pump_makes_one_probe_build() {
        install_panic_hook();
        let divides_by_param = || register_grid_model::<DividesByParam>();
        let (mut sweep, events) = pumped(divides_by_param(), init_divisors(&["0", "0", "0", "1"]));
        for config_id in 0..4 {
            assert!(matches!(sweep.pump(), Pace::Now));
            assert!(
                events.try_iter().next().is_none(),
                "config {config_id} is the one build of its pump"
            );
        }
        assert!(matches!(sweep.pump(), Pace::Now));
        assert!(
            matches!(events.try_iter().next(), Some(SweepEvent::Planned(_))),
            "the sweep is prepared in a pump of its own"
        );

        let zeros = vec!["0"; MAX_PROBED_CONFIGS + 1];
        let (mut sweep, events) = pumped(divides_by_param(), init_divisors(&zeros));
        for _ in 1..MAX_PROBED_CONFIGS {
            assert!(matches!(sweep.pump(), Pace::Now));
        }
        assert!(events.try_iter().next().is_none(), "one config is left to try");
        assert!(matches!(sweep.pump(), Pace::Idle));
        assert!(matches!(events.try_iter().last(), Some(SweepEvent::Failed(_))));
    }

    #[test]
    fn the_start_time_comes_before_the_probe() {
        let mut spec = SweepSpec::new("game_of_life");
        spec.fixed = fixed(&[("grid_width", "8"), ("grid_height", "8")]);
        spec.run.steps = 4;
        let (mut sweep, events) = pumped(entry("game_of_life", None), spec);
        let created_unix_ms = now_unix_ms();
        let delay = Duration::from_millis(20);
        std::thread::sleep(delay);
        let ended = pump_until_idle(&mut sweep, &events);
        let Some(SweepEvent::Finished(record)) = ended.last() else {
            panic!("the sweep did not finish: {:?}", ended.last());
        };
        assert!(record.manifest.timestamps.started_unix_ms <= created_unix_ms);
        assert!(record.report.elapsed >= delay, "{:?}", record.report.elapsed);
    }

    #[test]
    fn a_sweep_paused_after_its_last_run_ends() {
        let mut spec = SweepSpec::new("game_of_life");
        spec.fixed = fixed(&[("grid_width", "8"), ("grid_height", "8")]);
        spec.run.steps = 4;
        spec.run.replicates = 2;
        let (mut sweep, events) = pumped(entry("game_of_life", None), spec);
        let received = pump_until_runs(&mut sweep, &events, 2);
        assert_eq!(
            finished_end(&received),
            None,
            "the last run is written and the sweep has not ended"
        );

        sweep.handle_command(SweepCommand::Pause);
        let ended = pump_until_idle(&mut sweep, &events);
        assert_eq!(finished_end(&ended), Some(SweepEnd::Complete));
    }

    #[test]
    fn a_search_paused_after_its_last_run_ends() {
        let mut spec = SweepSpec::new("sir");
        spec.fixed = fixed(&[("grid_width", "8"), ("grid_height", "8")]);
        spec.run.steps = 4;
        spec.measure.default_reducers = false;
        spec.measure.reducers = vec!["Infected:max".parse().expect("a valid reducer")];
        spec.search = Some(SearchSpec {
            algorithm: SearchAlgorithm::Random,
            max_evaluations: 2,
            batch_size: 2,
            objective: Some(Objective {
                column: "Infected:max".to_owned(),
                goal: Goal::Minimize,
                aggregate: Aggregate::Median,
            }),
            space: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.1,
                    max: 0.9,
                    step: None,
                },
            )],
        });
        let (mut sweep, events) = pumped(entry("sir", None), spec);
        let received = pump_until_runs(&mut sweep, &events, 2);
        assert_eq!(
            finished_end(&received),
            None,
            "the last run is written and the search has not ended"
        );

        sweep.handle_command(SweepCommand::Pause);
        let ended = pump_until_idle(&mut sweep, &events);
        assert!(
            matches!(ended.first(), Some(SweepEvent::SearchBatchTold(_))),
            "a pause leaves the searcher to be told"
        );
        assert_eq!(finished_end(&ended), Some(SweepEnd::Complete));
    }
}
