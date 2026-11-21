//! Preparation of a sweep: its inputs, the directory checks, the probe and the layout, and the run of its pending
//! runs.

#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use web_time::Instant;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::PlannedRun;
#[cfg(not(target_arch = "wasm32"))]
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;

use super::{
    ExploreError, ManifestParts, SweepEnd, SweepOptions, SweepOutline, SweepRecord, SweepReport, SweepWarning,
    build_warnings, finish_manifest, running_manifest,
};
use crate::exec::{BatchEnd, ExecutionBudget, ExecutionLayout, choose_layout, gpu_memory_budget};
#[cfg(not(target_arch = "wasm32"))]
use crate::exec::{Executor, RunRequest, RunSink};
use crate::output::OutputWriter;
use crate::output::manifest::{
    Manifest, ManifestBlock, ManifestMode, ManifestPlan, ManifestRuntime, ResultCounts, hex, now_unix_ms,
};
use crate::output::memory::SweepFiles;
use crate::output::resume::{ResumeScan, check_directory_code};
use crate::output::runs_csv::column_names;
use crate::output::{OutputDir, OutputError, runs_csv, series_csv};
use crate::probe::{ProbeReport, TimedProbe, check_capacity};
#[cfg(not(target_arch = "wasm32"))]
use crate::progress::ProgressMeter;
use crate::progress::{Progress, ProgressEvent};

/// Model, device, spec and records of one sweep, and the host's settings.
pub(crate) struct SweepInputs<'a> {
    pub(crate) entry: &'a ModelEntry,
    pub(crate) gpu: Option<&'a GpuContext>,
    pub(crate) runtime: &'a ManifestRuntime,
    pub(crate) spec: &'a SweepSpec,
    pub(crate) options: &'a SweepOptions,
    /// Directory the results go in or a dry run reads, `None` for a sweep held in memory.
    pub(crate) folder: Option<&'a Path>,
    /// Whether the sweep is planned and probed and stops there, writing nothing.
    pub(crate) dry_run: bool,
}

/// Directory checks, probe and measure plan that a sweep and a search both prepare from their plan.
pub(crate) struct PreparedStart<'a> {
    /// Clock reading when planning started.
    pub(crate) started: Instant,
    pub(crate) started_unix_ms: u64,
    /// Directory the run resumes, `None` for one that starts afresh.
    pub(crate) resume_dir: Option<&'a Path>,
    /// Directory a resume holds locked from before its scan until its last write, `None` for a dry run or a run that
    /// starts afresh.
    pub(crate) locked: Option<OutputDir>,
    pub(crate) probe: ProbeReport,
    pub(crate) measure: MeasurePlan,
}

impl<'a> PreparedStart<'a> {
    /// Checks the folder of `inputs` and the code it records for `mode`, checks `plan` against the device, and probes
    /// it. `probe`, when given, replaces the report that [`ProbeReport::for_plan`] returns for `plan`, and its clock
    /// readings replace the ones taken on entry.
    pub(crate) fn new(
        inputs: &SweepInputs<'a>,
        mode: ManifestMode,
        plan: &Plan,
        probe: Option<TimedProbe>,
    ) -> Result<Self, ExploreError> {
        let (entry, gpu, options) = (inputs.entry, inputs.gpu, inputs.options);
        let (started, started_unix_ms, probe) = match probe {
            Some(timed) => (timed.started, timed.started_unix_ms, Some(timed.report)),
            None => (Instant::now(), now_unix_ms(), None),
        };
        let resume_dir = inputs
            .folder
            .filter(|output_dir| options.resume && OutputDir::holds_results(output_dir));
        let locked = match (resume_dir, inputs.folder) {
            (None, Some(output_dir)) => {
                OutputDir::check_free(output_dir)?;
                None
            }
            // The directory is locked before the scan and held until the last write. Otherwise another writer could
            // change the tables between the two.
            (Some(output_dir), _) if !inputs.dry_run => Some(OutputDir::open(output_dir)?),
            _ => None,
        };
        if let Some(output_dir) = resume_dir {
            check_directory_code(
                output_dir,
                mode,
                plan.model(),
                entry.source(),
                entry.metadata().time.as_ref(),
            )?;
        }
        if let Some(ctx) = gpu {
            check_capacity(entry, plan, &ctx.device.limits(), options.shortfall_words.as_ref())?;
        }
        let probe = match probe {
            Some(probe) => probe,
            None => options.in_build_pool(|| ProbeReport::for_plan(entry, gpu, plan))?,
        };
        let measure = MeasurePlan::new(plan.run_settings(), plan.measure_settings(), probe.columns.clone())?;
        Ok(Self {
            started,
            started_unix_ms,
            resume_dir,
            locked,
            probe,
            measure,
        })
    }

    /// Returns the header lines of `runs.csv` and `series.csv` that a run of `plan` on `entry` writes.
    pub(crate) fn header_lines(&self, entry: &ModelEntry, plan: &Plan) -> (String, String) {
        let params = entry.param_descriptors();
        let columns = column_names(params, plan.actions(), self.measure.reducers().names());
        let timed = series_csv::tick_length_position(entry.metadata().time.as_ref(), params).is_some();
        (
            runs_csv::header_line(&columns),
            series_csv::header_line(self.measure.columns(), timed),
        )
    }
}

/// A sweep planned, checked and probed, with its layout chosen.
pub(crate) struct SweepPreparation {
    plan: Arc<Plan>,
    probe: ProbeReport,
    measure: Arc<MeasurePlan>,
    /// Runs of the shard that this sweep executes, in plan order.
    pending: Vec<PlannedRun>,
    /// Directory the sweep resumes, `None` for a sweep that starts afresh.
    resumed: Option<ResumeScan>,
    /// Directory a resume holds locked from before its scan until its last write, `None` for a dry run or a sweep
    /// that starts afresh.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "a sweep in a browser writes no directory")
    )]
    locked: Option<OutputDir>,
    outline: SweepOutline,
    /// Clock reading when planning started.
    started: Instant,
    started_unix_ms: u64,
}

impl SweepPreparation {
    /// Checks and probes `plan`, the plan of `inputs.spec`, and chooses its layout. `probe`, when given, replaces the
    /// report that [`ProbeReport::for_plan`] returns for the plan, and its clock readings replace the ones taken on
    /// entry.
    pub(crate) fn new(
        inputs: &SweepInputs<'_>,
        plan: Arc<Plan>,
        probe: Option<TimedProbe>,
    ) -> Result<Self, ExploreError> {
        let (entry, gpu, options) = (inputs.entry, inputs.gpu, inputs.options);
        debug_assert!(
            inputs.spec.search.is_none(),
            "a spec with a [search] table runs as a search"
        );
        let start = PreparedStart::new(inputs, ManifestMode::Sweep, &plan, probe)?;
        let resumed = start
            .resume_dir
            .map(|output_dir| {
                let (runs_header, series_header) = start.header_lines(entry, &plan);
                ResumeScan::read(
                    output_dir,
                    &plan,
                    entry.source(),
                    options.shard,
                    options.retry_failed,
                    &runs_header,
                    &series_header,
                )
            })
            .transpose()?;
        let PreparedStart {
            started,
            started_unix_ms,
            locked,
            probe,
            measure,
            ..
        } = start;
        let pending: Vec<PlannedRun> = plan
            .runs_in_shard(options.shard)
            .filter(|run| {
                resumed
                    .as_ref()
                    .is_none_or(|scan| !scan.finished().contains(&run.run_id))
            })
            .collect();

        // The layout is sized from the first config that builds without a fault or the last config, whichever holds
        // more.
        let last_probe = options.in_build_pool(|| ProbeReport::for_last_config(entry, gpu, &plan, &probe));
        let sizing_probe = match &last_probe {
            Some(last_probe) if last_probe.footprint() > probe.footprint() => last_probe,
            _ => &probe,
        };
        let pending_count = pending.len() as u64;
        let (layout, projected_bytes) = sized_layout(entry, gpu, options, sizing_probe, pending_count)?;
        let outline = SweepOutline {
            model: entry.id().to_owned(),
            backend: entry.metadata().backend,
            configs: Some(plan.configs().len() as u64),
            replicates: plan.replicates(),
            runs: plan.run_count(),
            blocks: plan.blocks().to_vec(),
            shard: options.shard,
            skipped: resumed.as_ref().map_or(0, |scan| scan.finished().len() as u64),
            pending: pending_count,
            layout,
            projected_bytes,
            series_rows: pending_count.saturating_mul(measure.series_row_count()),
            stat_columns: series_csv::stat_column_names(measure.columns()),
            reducer_columns: measure.reducers().names().to_vec(),
            dry_run: inputs.dry_run,
            search: None,
        };
        Ok(Self {
            plan,
            probe,
            measure: Arc::new(measure),
            pending,
            resumed,
            locked,
            outline,
            started,
            started_unix_ms,
        })
    }

    pub(crate) fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    pub(crate) fn measure(&self) -> &Arc<MeasurePlan> {
        &self.measure
    }

    /// Runs this sweep executes, in plan order.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn pending(&self) -> &[PlannedRun] {
        &self.pending
    }

    /// Reports the outline, then each warning of the plan and of a resume under a build that differs from the engine
    /// or model build of `inputs`.
    pub(crate) fn announce(&self, inputs: &SweepInputs<'_>, progress: &mut dyn Progress) {
        let recorded = self.resumed.as_ref().map(|scan| &scan.recorded);
        announce(inputs, &self.outline, &self.plan, recorded, progress);
    }

    /// Writes the manifest with status `running`, runs every pending run into `output_dir` and replaces the manifest
    /// with the sweep's final status.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn write_directory(
        &self,
        inputs: &SweepInputs<'_>,
        output_dir: &Path,
        progress: &mut dyn Progress,
    ) -> Result<SweepRecord, ExploreError> {
        let created;
        let dir = if let Some(dir) = &self.locked {
            dir
        } else {
            created = OutputDir::create(output_dir)?;
            &created
        };
        let mut manifest = self.manifest(inputs)?;
        dir.write_manifest(&manifest)?;
        let (end, counts) = match self.run_into(dir, inputs, progress) {
            Ok(finished) => finished,
            Err(error) => {
                manifest.fail(now_unix_ms());
                // The sweep's own error is the one to report. A manifest that cannot be written says `running`.
                drop(dir.write_manifest(&manifest));
                return Err(error);
            }
        };
        let end = finish_manifest(&mut manifest, end, counts);
        dir.write_manifest(&manifest)?;
        Ok(SweepRecord {
            report: self.report(end, counts, Some(output_dir.to_owned())),
            manifest,
            files: None,
            search: None,
        })
    }

    /// Repairs a resumed directory, runs every pending run into `dir`, puts the tables in run order and rebuilds the
    /// summary.
    ///
    /// Returns the end of the batch and the counts of the rows `runs.csv` holds.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_into(
        &self,
        dir: &OutputDir,
        inputs: &SweepInputs<'_>,
        progress: &mut dyn Progress,
    ) -> Result<(BatchEnd, ResultCounts), ExploreError> {
        if let Some(scan) = &self.resumed {
            scan.repair(dir)?;
        }
        let params = inputs.entry.param_descriptors();
        let time = inputs.entry.metadata().time.as_ref();
        let writer = if self.resumed.is_some() {
            dir.append_writer(&self.plan, params, time)?
        } else {
            dir.open_writer(&self.plan, params, &self.measure, time)?
        };
        let (end, writer) = self.run_pending(inputs, writer, progress)?;
        let written = writer.counts();
        writer.finish().map_err(|source| OutputError::Write {
            path: dir.path().to_owned(),
            source,
        })?;
        let kept = self.resumed.as_ref().map(ResumeScan::counts).unwrap_or_default();
        let last_kept = self.resumed.as_ref().and_then(|scan| scan.finished().last().copied());
        if self.pending.first().is_some_and(|run| Some(run.run_id) < last_kept) {
            dir.order_tables()?;
        }
        dir.write_summary()?;
        Ok((end, kept + written))
    }

    /// Runs every pending run and commits each to `writer` in plan order, reporting each to `progress`.
    ///
    /// Returns the end of the batch and the writer.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn run_pending<W: Write>(
        &self,
        inputs: &SweepInputs<'_>,
        writer: OutputWriter<W>,
        progress: &mut dyn Progress,
    ) -> Result<(BatchEnd, OutputWriter<W>), ExploreError> {
        let executor = executor(inputs, &self.plan, &self.measure, self.outline.layout)?;
        let requests: Vec<RunRequest<'_>> = self
            .pending
            .iter()
            .map(|&run| RunRequest::planned(&self.plan, run))
            .collect();
        let mut sink = SweepSink {
            writer,
            progress,
            meter: ProgressMeter::new(self.outline.pending),
        };
        let end = executor.run_batch(&requests, &mut sink)?;
        Ok((end, sink.writer))
    }

    /// Returns the manifest of the sweep while it runs.
    ///
    /// A resume keeps the start time, sessions and merged shards the directory recorded.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Manifest`] when the spec cannot be written as JSON.
    pub(crate) fn manifest(&self, inputs: &SweepInputs<'_>) -> Result<Manifest, OutputError> {
        running_manifest(
            inputs,
            &ManifestParts {
                mode: ManifestMode::Sweep,
                plan: &self.plan,
                probe: &self.probe,
                outline: &self.outline,
                manifest_plan: self.manifest_plan(),
                started_unix_ms: self.started_unix_ms,
                recorded: self.resumed.as_ref().map(|scan| &scan.recorded),
                search: None,
            },
        )
    }

    /// Returns the record of the plan's hashes, counts and blocks.
    fn manifest_plan(&self) -> ManifestPlan {
        ManifestPlan {
            plan_hash: hex(self.plan.plan_hash()),
            results_fingerprint: hex(self.plan.results_fingerprint()),
            configs: self.outline.configs,
            replicates: self.outline.replicates,
            runs: self.outline.runs,
            blocks: self
                .plan
                .blocks()
                .iter()
                .map(|block| ManifestBlock {
                    design: block.design.as_str().to_owned(),
                    configs: block.configs.end - block.configs.start,
                    design_seed: block.design_seed,
                })
                .collect(),
        }
    }

    /// Returns the record of a sweep held in memory whose batch ended as `end`, with its files and `manifest` in its
    /// final status.
    ///
    /// # Errors
    ///
    /// Returns the error of [`SweepFiles::assemble`].
    pub(crate) fn finish_in_memory(
        &self,
        writer: OutputWriter<Vec<u8>>,
        mut manifest: Manifest,
        end: BatchEnd,
    ) -> Result<SweepRecord, ExploreError> {
        let counts = writer.counts();
        let end = finish_manifest(&mut manifest, end, counts);
        let files = SweepFiles::assemble(writer, &manifest)?;
        Ok(SweepRecord {
            report: self.report(end, counts, None),
            manifest,
            files: Some(files),
            search: None,
        })
    }

    pub(crate) fn report(&self, end: SweepEnd, counts: ResultCounts, output_dir: Option<PathBuf>) -> SweepReport {
        SweepReport::new(&self.outline, self.started, end, counts, output_dir)
    }
}

/// Reports `outline`, then each warning of `plan` and, for a resume of the directory that `recorded` describes, of a
/// recorded build that differs from the engine or model build of `inputs`.
pub(crate) fn announce(
    inputs: &SweepInputs<'_>,
    outline: &SweepOutline,
    plan: &Plan,
    recorded: Option<&Manifest>,
    progress: &mut dyn Progress,
) {
    progress.report(&ProgressEvent::Planned(outline));
    let mut warnings: Vec<SweepWarning> = plan.warnings().iter().cloned().map(SweepWarning::Plan).collect();
    if let Some(recorded) = recorded {
        warnings.extend(build_warnings(recorded, &inputs.options.provenance, inputs.entry));
    }
    for warning in &warnings {
        progress.report(&ProgressEvent::Warned(warning));
    }
}

/// Returns the executor that runs the runs of `plan` for `inputs`, measured by `measure` in `layout`.
///
/// # Errors
///
/// Returns the error of [`Executor::new`].
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn executor<'a>(
    inputs: &SweepInputs<'a>,
    plan: &Plan,
    measure: &Arc<MeasurePlan>,
    layout: ExecutionLayout,
) -> Result<Executor<'a>, ExploreError> {
    let options = inputs.options;
    let executor = Executor::new(
        inputs.entry,
        inputs.gpu,
        Arc::clone(measure),
        layout,
        options.control.clone(),
    )?;
    Ok(executor
        .with_timeout(plan.run_settings().timeout)
        .with_active_runs(options.active_runs.clone())
        .with_gpu_memory_budget(options.gpu_memory_budget)
        .with_shortfall_words(options.shortfall_words.clone())
        .with_build_pool(options.build_pool.clone()))
}

/// Returns the layout for `runs` runs of `entry` sized from `sizing_probe`, and the bytes the live runs are projected
/// to hold.
///
/// Note that a probe sizes its buffers to rayon's global pool, and a run in a lane to the lane's pool. When the
/// layout splits the workers into lanes, the probe is built again on a pool as wide as a lane and the lanes are
/// budgeted from that build.
///
/// # Errors
///
/// Returns [`ExploreError::Probe`] when the probe cannot be built again on a lane's pool.
pub(crate) fn sized_layout(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    options: &SweepOptions,
    sizing_probe: &ProbeReport,
    runs: u64,
) -> Result<(ExecutionLayout, u64), ExploreError> {
    let backend = entry.metadata().backend;
    let detected = ExecutionBudget::detect();
    let unbudgeted = choose_layout(options.concurrency, &detected, backend, sizing_probe, runs);
    let rebuilt = if unbudgeted.cpu_lanes > 1 && unbudgeted.threads_per_lane != detected.workers {
        Some(sizing_probe.rebuilt_on(entry, unbudgeted.threads_per_lane)?)
    } else {
        None
    };
    let resources = ExecutionBudget {
        memory_budget: options.memory_budget,
        gpu_memory_budget: gpu.map(|ctx| gpu_memory_budget(options.gpu_memory_budget, ctx)),
        ..detected
    };
    let lane_probe = rebuilt.as_ref().unwrap_or(sizing_probe);
    let layout = choose_layout(options.concurrency, &resources, backend, lane_probe, runs);
    let run_probe = if layout.cpu_lanes > 1 { lane_probe } else { sizing_probe };
    Ok((layout, layout.projected_bytes(run_probe)))
}

/// Sink that writes each run to the output and reports the sweep's progress.
#[cfg(not(target_arch = "wasm32"))]
struct SweepSink<'s, W: Write> {
    writer: OutputWriter<W>,
    progress: &'s mut dyn Progress,
    meter: ProgressMeter,
}

#[cfg(not(target_arch = "wasm32"))]
impl<W: Write> RunSink for SweepSink<'_, W> {
    fn commit(&mut self, outcome: RunOutcome) -> io::Result<()> {
        self.writer.write_run(&outcome)?;
        self.progress.report(&ProgressEvent::RunCommitted(&outcome));
        Ok(())
    }

    fn finished(&mut self, outcome: &RunOutcome) {
        if let Some(update) = self.meter.record_finished_run(outcome.status) {
            self.progress.report(&ProgressEvent::Progressed(update));
        }
    }
}
