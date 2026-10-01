//! Sweep sessions, each one sweep or search started from the Sweep tab, from its start until the tab is cleared.

use std::path::PathBuf;
use std::sync::Arc;

use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;
use henad_core::params::ParamValue;
use henad_explore::exec::Concurrency;
use henad_explore::handle::{SweepEvent, SweepOutput, SweepPhase, SweepProgress, SweepRun, SweepRunOptions};
use henad_explore::search_run::{SearchPlan, SearchUpdate};
use henad_explore::sweep::{Provenance, SweepEnd, SweepOutline, SweepReport};

use crate::state::{AppState, lookup_message};
use crate::ui::results::ResultsPanel;
use crate::ui::sweep::draft::{SweepDraft, capitalize, describe_error};
use crate::ui::sweep::plan::PlanSummary;

/// Draft that started a session, kept for Save spec and the Plan panel while the session lasts.
pub struct KeptDraft {
    pub draft: SweepDraft,
    /// Parameters tab values the draft was checked against at the start.
    pub panel_values: Vec<ParamValue>,
    /// Plan of the draft as it started.
    pub plan: PlanSummary,
}

/// Stage of a session, as the tab's title, header and footer show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Planning and probing, before the first run starts.
    Planning,
    Running,
    Paused,
    /// Every run is written.
    Finished,
    Aborted,
    /// Ended by the loss of the GPU device.
    Stopped,
    /// Ended on an error of its own, outside any run.
    Failed,
}

impl SessionState {
    /// Returns whether the session is still to send its last event.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Planning | Self::Running | Self::Paused)
    }

    /// Returns whether the session is held between two slices of steps.
    pub fn is_paused(self) -> bool {
        self == Self::Paused
    }
}

/// A sweep or search started from the Sweep tab, running or ended.
pub struct SweepSession {
    run: SweepRun,
    /// Name of the model the sweep runs, as the Model tab shows it.
    pub model_name: String,
    /// Folder the results stream into, `None` for results held in memory.
    pub output_dir: Option<PathBuf>,
    /// Warnings of the sweep, in the order they arrived.
    pub warnings: Vec<String>,
    /// Report of a sweep that ran to its end or was aborted.
    pub report: Option<SweepReport>,
    /// Error that ended the sweep outside any run.
    pub failure: Option<String>,
    /// Update of a search after the last batch it was told, `None` for a sweep or before the first batch.
    pub latest_search_update: Option<Arc<SearchUpdate>>,
    /// Generations of a genetic algorithm that finished before the last batch it was told, the index of the
    /// generation that batch belongs to.
    pub latest_generation: u64,
    /// Generations of a genetic algorithm that finished with the batches told so far.
    finished_generations: u64,
    /// Size and layout of the sweep once planned, `None` while it plans.
    pub outline: Option<Box<SweepOutline>>,
    /// Draft that started the session, `None` for a session that resumes a folder.
    pub kept: Option<Box<KeptDraft>>,
}

/// Settings of a sweep that decide how many runs step at once. None of them change its results.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionExecution {
    /// Number of runs stepped at once.
    pub concurrency: Concurrency,
    /// Bytes of host memory the live runs can hold together, `None` for no limit.
    pub memory_budget: Option<u64>,
    /// Bytes of GPU memory the live runs can hold together, `None` for the device's largest buffer.
    pub gpu_memory_budget: Option<u64>,
}

impl SweepSession {
    /// Starts a sweep of `spec` on the model `spec` names, and pauses the live simulation.
    ///
    /// The results go to `output_dir`, or stay in memory when it is `None`. A GPU model steps on a device of the
    /// sweep's own. Planning happens before this returns, and the runs after it.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no such model, or the sweep cannot start.
    pub fn start(
        app: &mut AppState,
        spec: SweepSpec,
        execution: SessionExecution,
        output_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let entry = app.lookup(&spec.model).map_err(|error| lookup_message(&error))?.clone();
        let model_name = entry.name().to_owned();
        let output = match &output_dir {
            None => SweepOutput::Memory,
            #[cfg(not(target_arch = "wasm32"))]
            Some(dir) => SweepOutput::Directory(dir.clone()),
            #[cfg(target_arch = "wasm32")]
            Some(_) => return Err("Writing results to a folder is unavailable in a browser".to_owned()),
        };
        let mut options = SweepRunOptions::new(provenance());
        options.concurrency = execution.concurrency;
        options.memory_budget = execution.memory_budget;
        options.gpu_memory = execution.gpu_memory_budget;
        options.wake = Some(app.repaint_waker());
        let run = SweepRun::start(entry, None, spec, output, options).map_err(|error| describe_error(&error))?;
        app.pause_simulation();
        Ok(Self {
            run,
            model_name,
            output_dir,
            warnings: Vec::new(),
            report: None,
            failure: None,
            latest_search_update: None,
            latest_generation: 0,
            finished_generations: 0,
            outline: None,
            kept: None,
        })
    }

    /// Resumes the sweep whose results `folder` holds, a sweep of the model `model_id`, and pauses the live
    /// simulation. A GPU model steps on a device of the sweep's own.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no such model, or the sweep cannot resume.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resume_folder(app: &mut AppState, folder: PathBuf, model_id: &str) -> Result<Self, String> {
        let entry = app.lookup(model_id).map_err(|error| lookup_message(&error))?.clone();
        let model_name = entry.name().to_owned();
        let mut options = SweepRunOptions::new(provenance());
        options.wake = Some(app.repaint_waker());
        let run = SweepRun::resume_directory(entry, None, &folder, options).map_err(|error| describe_error(&error))?;
        app.pause_simulation();
        Ok(Self {
            run,
            model_name,
            output_dir: Some(folder),
            warnings: Vec::new(),
            report: None,
            failure: None,
            latest_search_update: None,
            latest_generation: 0,
            finished_generations: 0,
            outline: None,
            kept: None,
        })
    }

    /// Plan of the sweep's configs and runs, or of a search's fixed values and actions alone.
    pub fn plan(&self) -> &Arc<Plan> {
        self.run.plan()
    }

    /// Plan of a search, `None` for a sweep.
    pub fn search_plan(&self) -> Option<&Arc<SearchPlan>> {
        self.run.search_plan()
    }

    /// Returns "search" for a search and "sweep" for a sweep, as the progress view names it.
    pub fn noun(&self) -> &'static str {
        if self.search_plan().is_some() {
            "search"
        } else {
            "sweep"
        }
    }

    /// Steps the sweep within the frame's budget in a browser, and hands every waiting event to `results` at once.
    pub fn update(&mut self, dt: f64, results: &mut ResultsPanel) {
        self.run.update(dt);
        let mut events = Vec::new();
        while let Some(event) = self.run.try_recv() {
            match &event {
                SweepEvent::Warned(warning) => self.warnings.push(capitalize(&warning.to_string())),
                SweepEvent::Finished(record) => self.report = Some(record.report.clone()),
                SweepEvent::Failed(message) => self.failure = Some(message.clone()),
                SweepEvent::SearchBatchTold(update) => {
                    self.latest_generation = self.finished_generations;
                    if let Some(last) = update.generations.last() {
                        self.finished_generations = self.finished_generations.max(last.generation + 1);
                    }
                    self.latest_search_update = Some(Arc::clone(update));
                }
                SweepEvent::Planned(outline) => self.outline = Some(outline.clone()),
                SweepEvent::RunFinished { .. } => {}
            }
            events.push(event);
        }
        results.ingest(events);
    }

    pub fn progress(&self) -> SweepProgress {
        self.run.progress()
    }

    /// Returns the stage of the session, read from `progress` while it runs and from its end once it has ended.
    pub fn state(&self, progress: &SweepProgress) -> SessionState {
        if self.is_running() {
            return match progress.phase {
                SweepPhase::Planning => SessionState::Planning,
                SweepPhase::Paused => SessionState::Paused,
                // The last event is still on its way.
                SweepPhase::Running | SweepPhase::Ended(_) | SweepPhase::Failed => SessionState::Running,
            };
        }
        match (&self.report, &self.failure) {
            (_, Some(_)) => SessionState::Failed,
            (Some(report), None) => match report.end {
                SweepEnd::Complete | SweepEnd::Planned => SessionState::Finished,
                SweepEnd::Aborted => SessionState::Aborted,
                SweepEnd::DeviceLost => SessionState::Stopped,
            },
            (None, None) => SessionState::Finished,
        }
    }

    /// Returns whether the session resumes a folder, and has no draft of its own.
    pub fn is_resumed(&self) -> bool {
        self.kept.is_none()
    }

    /// Returns whether the sweep is still to send its last event.
    pub fn is_running(&self) -> bool {
        !self.run.is_ended()
    }

    pub fn is_paused(&self) -> bool {
        self.run.is_paused()
    }

    pub fn pause(&mut self) {
        self.run.pause();
    }

    pub fn resume(&mut self) {
        self.run.resume();
    }

    pub fn abort(&mut self) {
        self.run.abort();
    }
}

/// Returns the build of this app for a sweep's manifest.
fn provenance() -> Provenance {
    Provenance {
        engine_name: "henad".to_owned(),
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        commit: env!("HENAD_COMMIT").to_owned(),
        commit_date: env!("HENAD_COMMIT_DATE").to_owned(),
        debug_build: cfg!(debug_assertions),
        argv: std::env::args_os()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use henad_compute::fault::Fault;
    use henad_core::explore::spec::SweepSpec;
    use henad_explore::sweep::SweepEnd;

    use super::{SessionExecution, SessionState, SweepSession};
    use crate::state::AppState;
    use crate::ui::results::ResultsPanel;

    /// Returns the app over a headless device, or `None` to skip a GPU test on a machine without one.
    ///
    /// # Panics
    ///
    /// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
    fn headless_app() -> Option<AppState> {
        let models = henad_models::example_models();
        match henad_explore::device::acquire_headless(models.gpu_needs()) {
            Ok(ctx) => {
                let runtime = ctx
                    .runtime_info()
                    .expect("a headless device carries its runtime info")
                    .clone();
                Some(AppState::new(
                    egui::Context::default(),
                    models,
                    ctx.clone(),
                    Some(ctx),
                    runtime,
                ))
            }
            Err(error) => {
                let required =
                    std::env::var_os("HENAD_REQUIRE_GPU").is_some_and(|value| !value.is_empty() && value != "0");
                assert!(!required, "HENAD_REQUIRE_GPU is set but {error}");
                None
            }
        }
    }

    #[test]
    fn a_gpu_sweep_and_the_app_keep_their_faults_apart() {
        let Some(mut app) = headless_app() else {
            return;
        };
        app.render_ctx
            .faults
            .set_once(Fault::refused("drawing the viewport", "a fault of the app's"));
        let mut spec = SweepSpec::new("gpu_sir");
        spec.fixed = vec![
            ("grid_width".to_owned(), "32".to_owned()),
            ("grid_height".to_owned(), "32".to_owned()),
        ];
        spec.run.steps = 20;
        spec.run.replicates = 2;
        spec.measure.stats_every = 4;
        spec.measure.series_every = 4;

        let mut session =
            SweepSession::start(&mut app, spec, SessionExecution::default(), None).expect("the sweep starts");
        let mut results = ResultsPanel::default();
        let started = Instant::now();
        while session.is_running() {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "the sweep did not end in time"
            );
            session.update(0.0, &mut results);
            std::thread::sleep(Duration::from_millis(2));
        }

        let state = session.state(&session.progress());
        assert_eq!(state, SessionState::Finished, "the sweep failed: {:?}", session.failure);
        let report = session.report.as_ref().expect("a finished sweep has a report");
        assert_eq!((report.end, report.counts.ok), (SweepEnd::Complete, 2));
        assert!(
            app.render_ctx.faults.take().is_some(),
            "the app's fault stays with the app"
        );
    }
}
