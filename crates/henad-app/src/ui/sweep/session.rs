//! Sweep sessions, each one sweep or search started from the Sweep tab, from its start until the tab is cleared.

use std::path::PathBuf;
use std::sync::Arc;

use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;
use henad_explore::exec::Concurrency;
use henad_explore::handle::{SweepEvent, SweepOutput, SweepPhase, SweepProgress, SweepRun, SweepRunOptions};
use henad_explore::output::manifest::ManifestRuntime;
use henad_explore::search_run::{SearchPlan, SearchUpdate};
use henad_explore::sweep::{Provenance, SweepEnd, SweepOutline, SweepReport};
use henad_models::registry::model_registry;

use crate::state::AppState;
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
    /// Paused after a GPU error that no run could be tied to.
    PausedByFault,
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
        matches!(
            self,
            Self::Planning | Self::Running | Self::Paused | Self::PausedByFault
        )
    }

    /// Returns whether the session is held between two slices of steps.
    pub fn is_paused(self) -> bool {
        matches!(self, Self::Paused | Self::PausedByFault)
    }
}

/// A sweep or search started from the Sweep tab, running or ended.
pub struct SweepSession {
    run: SweepRun,
    /// Name of the model the sweep runs, as the Model tab shows it.
    pub model_name: String,
    pub backend: Backend,
    /// Folder the results stream into, `None` for results held in memory.
    pub output_dir: Option<PathBuf>,
    /// Warnings of the sweep, in the order they arrived.
    pub warnings: Vec<String>,
    /// Report of a sweep that ran to its end or was aborted.
    pub report: Option<SweepReport>,
    /// Error that ended the sweep outside any run.
    pub failure: Option<String>,
    /// Whether a GPU error paused the sweep.
    pub paused_by_fault: bool,
    /// Update of a search after the last batch it was told, `None` for a sweep or before the first batch.
    pub latest_search_update: Option<Box<SearchUpdate>>,
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

impl SweepSession {
    /// Starts a sweep of `spec` on the model `spec` names, and pauses the live simulation.
    ///
    /// The results go to `output_dir`, or stay in memory when it is `None`. Planning happens before this returns,
    /// and the runs after it.
    ///
    /// # Errors
    ///
    /// Returns a message when this device has no such model, or the sweep cannot start.
    pub fn start(
        app: &mut AppState,
        spec: SweepSpec,
        concurrency: Concurrency,
        output_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let entry = model_registry(app.gpu_ctx.clone())
            .into_iter()
            .find(|entry| entry.id == spec.model)
            .ok_or_else(|| format!("{} is unavailable on this device", spec.model))?;
        let model_name = entry.name.clone();
        let backend = entry.metadata.backend;
        let output = match &output_dir {
            None => SweepOutput::Memory,
            #[cfg(not(target_arch = "wasm32"))]
            Some(dir) => SweepOutput::Directory(dir.clone()),
            #[cfg(target_arch = "wasm32")]
            Some(_) => return Err("Writing results to a folder is unavailable in a browser".to_owned()),
        };
        let options = SweepRunOptions {
            concurrency,
            provenance: provenance(),
            runtime: Some(ManifestRuntime::new(Some(&app.runtime))),
            wake: Some(app.repaint_waker()),
            ..SweepRunOptions::default()
        };
        let run = SweepRun::start(entry, app.gpu_ctx.clone(), spec, output, options)
            .map_err(|error| describe_error(&error))?;
        app.pause_simulation();
        Ok(Self {
            run,
            model_name,
            backend,
            output_dir,
            warnings: Vec::new(),
            report: None,
            failure: None,
            paused_by_fault: false,
            latest_search_update: None,
            latest_generation: 0,
            finished_generations: 0,
            outline: None,
            kept: None,
        })
    }

    /// Resumes the sweep whose results `folder` holds, a sweep of the model `model_id`, and pauses the live
    /// simulation.
    ///
    /// # Errors
    ///
    /// Returns a message when this device has no such model, or the sweep cannot resume.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resume_folder(app: &mut AppState, folder: PathBuf, model_id: &str) -> Result<Self, String> {
        let entry = model_registry(app.gpu_ctx.clone())
            .into_iter()
            .find(|entry| entry.id == model_id)
            .ok_or_else(|| format!("{model_id} is unavailable on this device"))?;
        let model_name = entry.name.clone();
        let backend = entry.metadata.backend;
        let options = SweepRunOptions {
            provenance: provenance(),
            runtime: Some(ManifestRuntime::new(Some(&app.runtime))),
            wake: Some(app.repaint_waker()),
            ..SweepRunOptions::default()
        };
        let run = SweepRun::resume_directory(entry, app.gpu_ctx.clone(), &folder, options)
            .map_err(|error| describe_error(&error))?;
        app.pause_simulation();
        Ok(Self {
            run,
            model_name,
            backend,
            output_dir: Some(folder),
            warnings: Vec::new(),
            report: None,
            failure: None,
            paused_by_fault: false,
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

    /// Steps the sweep within the frame's budget in a browser, and hands every waiting event to `results`.
    pub fn update(&mut self, dt: f64, results: &mut ResultsPanel) {
        self.run.update(dt);
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
                    self.latest_search_update = Some(update.clone());
                }
                SweepEvent::Planned(outline) => self.outline = Some(outline.clone()),
                SweepEvent::RunFinished { .. } => {}
            }
            results.ingest(event);
        }
    }

    pub fn progress(&self) -> SweepProgress {
        self.run.progress()
    }

    /// Returns the stage of the session, read from `progress` while it runs and from its end once it has ended.
    pub fn state(&self, progress: &SweepProgress) -> SessionState {
        if self.is_running() {
            return match progress.phase {
                SweepPhase::Planning => SessionState::Planning,
                SweepPhase::Paused if self.paused_by_fault => SessionState::PausedByFault,
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
        self.paused_by_fault = false;
        self.run.resume();
    }

    pub fn abort(&mut self) {
        self.run.abort();
    }

    /// Pauses a running GPU sweep after a device error nothing could tie to one side.
    pub fn pause_after_gpu_fault(&mut self) {
        if self.backend == Backend::Gpu && self.is_running() && !self.is_paused() {
            self.run.pause();
            self.paused_by_fault = true;
        }
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
