//! Sweep sessions, each one sweep or search started from the Sweep tab, from its start until the tab is cleared.

use std::path::PathBuf;
use std::sync::Arc;

use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;
use henad_core::params::ParamValue;
use henad_core::provenance::BuildInfo;
use henad_explore::exec::Concurrency;
use henad_explore::handle::{SweepEvent, SweepOutput, SweepPhase, SweepProgress, SweepRun, SweepRunOptions};
use henad_explore::output::manifest::ManifestExecution;
use henad_explore::search_run::{SearchPlan, SearchUpdate};
use henad_explore::sweep::{Provenance, SweepEnd, SweepOutline, SweepReport};

use crate::state::{AppState, lookup_message};
use crate::ui::results::ResultsPanel;
use crate::ui::sweep::builder::budget_text;
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
    /// Ended by an error outside any run.
    Failed,
}

impl SessionState {
    /// Returns whether the session has yet to send its last event.
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
    /// Number of generations of a genetic algorithm finished before the last batch it was told. It equals the
    /// index of the generation that batch belongs to.
    pub latest_generation: u64,
    /// Number of generations of a genetic algorithm finished with the batches told so far.
    finished_generations: u64,
    /// Size and layout of the sweep once planned, `None` while it plans.
    pub outline: Option<Box<SweepOutline>>,
    /// Draft that started the session, `None` for a session that resumes a folder.
    pub kept: Option<Box<KeptDraft>>,
    /// Concurrency and budgets the session runs with.
    pub execution: SessionExecution,
}

/// Settings of a sweep that set the number of runs stepped at once. None of them change its results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionExecution {
    /// Number of runs stepped at once, or automatic.
    pub concurrency: Concurrency,
    /// Host memory budget in bytes for all live runs together, `None` for no limit.
    pub memory_budget: Option<u64>,
    /// GPU memory budget in bytes for all live runs together, `None` for the device's largest buffer.
    pub gpu_memory_budget: Option<u64>,
}

impl SessionExecution {
    /// Returns the memory budgets held by `recorded`, a manifest's execution table, with automatic concurrency
    /// instead of the recorded concurrency.
    pub fn recorded(recorded: &ManifestExecution) -> Self {
        Self {
            concurrency: Concurrency::Auto,
            memory_budget: recorded.memory_budget,
            gpu_memory_budget: recorded.gpu_memory_budget,
        }
    }

    /// Returns the line that lists the recorded memory budgets a resume of the `noun` will use, `None` without a
    /// budget.
    pub fn resume_text(&self, noun: &str) -> Option<String> {
        let budgets = budget_text(self.memory_budget, self.gpu_memory_budget)?;
        Some(format!(
            "Missing runs will run with Memory budget {budgets}, as the {noun} recorded."
        ))
    }
}

impl SweepSession {
    /// Starts a sweep of `spec` on the model that `spec` refers to, and pauses the live simulation.
    ///
    /// The results go to `output_dir`, or stay in memory when it is `None`. A GPU model runs on the sweep's
    /// own device. Planning happens before this returns, and the runs after it.
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
        let mut options = SweepRunOptions::new(provenance(app.product.host));
        options.concurrency = execution.concurrency;
        options.memory_budget = execution.memory_budget;
        options.gpu_memory_budget = execution.gpu_memory_budget;
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
            execution,
        })
    }

    /// Resumes the sweep whose results `folder` holds, a sweep of the model `model_id`, with the concurrency and
    /// budgets of `execution`, and pauses the live simulation. A GPU model runs on the sweep's own device.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no such model, or the sweep cannot resume.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resume_folder(
        app: &mut AppState,
        folder: PathBuf,
        model_id: &str,
        execution: SessionExecution,
    ) -> Result<Self, String> {
        let entry = app.lookup(model_id).map_err(|error| lookup_message(&error))?.clone();
        let model_name = entry.name().to_owned();
        let mut options = SweepRunOptions::new(provenance(app.product.host));
        options.concurrency = execution.concurrency;
        options.memory_budget = execution.memory_budget;
        options.gpu_memory_budget = execution.gpu_memory_budget;
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
            execution,
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

    /// Steps the sweep within the frame's budget in a browser, and passes every waiting event to `results` at once.
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

    /// Returns whether the session resumes a folder. Such a session has no draft.
    pub fn is_resumed(&self) -> bool {
        self.kept.is_none()
    }

    /// Returns whether the sweep has yet to send its last event.
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

/// Returns the provenance a sweep's manifest records: `host`, the build of this app, with its command line.
fn provenance(host: BuildInfo) -> Provenance {
    let arguments = std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    Provenance::new(host, arguments)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use henad_compute::fault::Fault;
    use henad_core::explore::spec::SweepSpec;
    use henad_explore::sweep::SweepEnd;

    use std::num::NonZeroUsize;

    use henad_explore::exec::Concurrency;
    use henad_explore::output::manifest::ManifestExecution;

    use super::{SessionExecution, SessionState, SweepSession};
    use crate::state::AppState;
    use crate::ui::results::ResultsPanel;

    /// Returns the app over a headless device, or `None` to skip a GPU test on a machine without a GPU.
    fn headless_app() -> Option<AppState> {
        AppState::headless(henad_models::example_models(), true)
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

    /// Steps `session` until it ends, within a minute.
    fn finish(session: &mut SweepSession) -> ResultsPanel {
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
        results
    }

    #[test]
    fn a_resume_names_the_budgets_the_sweep_recorded() {
        let recorded = ManifestExecution {
            backend: "cpu".to_owned(),
            concurrency: "2".to_owned(),
            cpu_lanes: 2,
            threads_per_lane: 1,
            gpu_tracks: 0,
            projected_bytes: 0,
            memory_budget: Some(4 << 30),
            gpu_memory_budget: Some(2 << 30),
        };
        let execution = SessionExecution::recorded(&recorded);
        assert_eq!(
            execution,
            SessionExecution {
                concurrency: Concurrency::Auto,
                memory_budget: Some(4 << 30),
                gpu_memory_budget: Some(2 << 30),
            },
            "the recorded concurrency is left out"
        );
        assert_eq!(
            execution.resume_text("sweep").as_deref(),
            Some("Missing runs will run with Memory budget 4.0 GB, GPU 2.0 GB, as the sweep recorded.")
        );
        let automatic = ManifestExecution {
            concurrency: "auto".to_owned(),
            memory_budget: None,
            gpu_memory_budget: None,
            ..recorded
        };
        assert_eq!(SessionExecution::recorded(&automatic), SessionExecution::default());
        assert_eq!(SessionExecution::default().resume_text("search"), None);
    }

    /// A resume from the Results tab runs with the budgets the sweep recorded.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_resume_runs_with_the_budgets_the_sweep_recorded() {
        use henad_explore::result_set::ResultSet;

        let Some(mut app) = headless_app() else {
            return;
        };
        let folder = std::env::temp_dir().join(format!("henad-app-resume-execution-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&folder));
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![
            ("grid_width".to_owned(), "8".to_owned()),
            ("grid_height".to_owned(), "8".to_owned()),
        ];
        spec.run.steps = 3;
        spec.run.replicates = 2;
        let execution = SessionExecution {
            concurrency: Concurrency::Fixed(NonZeroUsize::MIN),
            memory_budget: Some(1 << 30),
            gpu_memory_budget: None,
        };
        let mut session =
            SweepSession::start(&mut app, spec, execution, Some(folder.clone())).expect("the sweep starts");
        finish(&mut session);
        let manifest_execution = |folder: &std::path::Path| {
            let set = ResultSet::open_dir(folder, usize::MAX).expect("the folder reads");
            set.manifest().execution.clone()
        };
        let recorded = manifest_execution(&folder);
        assert_eq!(recorded.concurrency, "1");
        let budgets = SessionExecution {
            concurrency: Concurrency::Auto,
            ..execution
        };
        assert_eq!(SessionExecution::recorded(&recorded), budgets);

        let mut session =
            SweepSession::resume_folder(&mut app, folder.clone(), "sir", budgets).expect("the sweep resumes");
        finish(&mut session);
        assert_eq!(session.state(&session.progress()), SessionState::Finished);
        let resumed = manifest_execution(&folder);
        assert_eq!(resumed.concurrency, "auto", "the resume ran with automatic concurrency");
        assert_eq!(
            SessionExecution::recorded(&resumed),
            budgets,
            "the resume kept the budgets"
        );
        drop(std::fs::remove_dir_all(&folder));
    }
}
