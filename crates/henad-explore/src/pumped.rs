//! Sweeps and searches stepped from a host's frames, one CPU run at a time, for a target that cannot spawn a thread.
//!
//! A pumped sweep holds its files in memory. Each pump probes the sweep, builds a run, or steps the run in progress by
//! a slice of about half the frame budget. A search also asks for a batch, or tells the searcher a finished one, in a
//! pump of its own.

use std::sync::Arc;

use web_time::Instant;

use henad_compute::fault::install_panic_hook;
use henad_compute::runner::{PUMP_BUDGET_MS, Pace, SimLoop};
use henad_core::explore::plan::{Plan, Shard};
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::Backend;
use henad_models::registry::ModelEntry;

use crate::cursor::{CursorState, RunCursor};
use crate::exec::{ActiveRuns, BatchEnd, Concurrency, ExecutionError, RunRequest, RunWatch, SliceSize, SweepControl};
use crate::handle::{SweepChannel, SweepRunOptions, SweepStartError};
use crate::output::OutputWriter;
use crate::output::manifest::{Manifest, ManifestRuntime};
use crate::output::memory::{SweepFiles, memory_writer};
use crate::progress::{Progress as _, ProgressEvent};
use crate::search_run::{AskedBatch, SearchPlan, SearchPreparation, SearchWriters, watched_values};
use crate::sweep::{
    ExploreError, Provenance, SpecSource, SweepInputs, SweepOptions, SweepPreparation, SweepRecord, finish_manifest,
};

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

/// Model, spec and settings of a pumped sweep.
struct SweepSetup {
    entry: ModelEntry,
    spec: SweepSpec,
    plan: Arc<Plan>,
    /// Plan of a search, `None` for a sweep.
    search_plan: Option<Arc<SearchPlan>>,
    source: SpecSource,
    provenance: Provenance,
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
            source: &self.source,
            provenance: &self.provenance,
            options: &self.options,
        }
    }
}

/// Stage of a pumped sweep.
enum PumpStage {
    /// Planned, with the probe still to build.
    Probing,
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
        if entry.metadata.backend == Backend::Gpu {
            return Err(SweepStartError::GpuNeedsNative);
        }
        let active_runs = ActiveRuns::new();
        let sweep_options = SweepOptions {
            output_dir: None,
            concurrency: Concurrency::Fixed(std::num::NonZeroUsize::MIN),
            memory_budget: options.memory_budget,
            gpu_memory: None,
            dry_run: false,
            control: SweepControl::new(),
            shard: Shard::WHOLE,
            resume: false,
            retry_failed: false,
            active_runs: Some(active_runs.clone()),
        };
        Ok(Self {
            setup: SweepSetup {
                entry,
                spec,
                plan,
                search_plan,
                source: options.source,
                provenance: options.provenance,
                runtime: options.runtime.unwrap_or_else(|| ManifestRuntime::new(None)),
                options: sweep_options,
                active_runs,
            },
            channel,
            stage: PumpStage::Probing,
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

    /// Plans and probes the sweep, reports its outline and opens its files.
    fn probe(&mut self) {
        install_panic_hook();
        if let Some(search_plan) = &self.setup.search_plan {
            return self.probe_search(Arc::clone(search_plan));
        }
        let inputs = self.setup.inputs();
        let preparation = match SweepPreparation::new(&inputs, Some(Arc::clone(&self.setup.plan))) {
            Ok(preparation) => preparation,
            Err(error) => return self.fail(&error),
        };
        preparation.announce(inputs.provenance, &mut self.channel);
        let opened = preparation.manifest(&inputs).and_then(|manifest| {
            let writer = memory_writer(
                preparation.plan(),
                &inputs.entry.param_descriptors,
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

    /// Probes the search of `search_plan`, reports its outline and opens its files.
    fn probe_search(&mut self, search_plan: Arc<SearchPlan>) {
        let inputs = self.setup.inputs();
        let preparation = match SearchPreparation::new(&inputs, Some(search_plan)) {
            Ok(preparation) => preparation,
            Err(error) => return self.fail(&error),
        };
        preparation.announce(inputs.provenance, &mut self.channel);
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
        let pumped = match &mut self.stage {
            PumpStage::Ended => return Pace::Idle,
            PumpStage::Probing => {
                self.probe();
                return Pace::Now;
            }
            PumpStage::Running(runs) if self.setup.options.control.is_aborted() => Ok(if runs.is_drained() {
                PumpWork::Drained
            } else {
                PumpWork::Aborted
            }),
            PumpStage::Running(_) if self.setup.options.control.is_paused() => return Pace::Idle,
            PumpStage::Running(runs) => runs.pump(&self.setup, &mut self.channel),
            PumpStage::Searching(_) if self.setup.options.control.is_aborted() => Ok(PumpWork::Aborted),
            PumpStage::Searching(_) if self.setup.options.control.is_paused() => return Pace::Idle,
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
