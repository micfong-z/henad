//! Live preview of the Editor's graph model: a run of the document at a small size, rebuilt from tick 0 after each
//! valid edit.
//!
//! A *preview* runs the document's compiled program with its parameters at their defaults, and each side of the grid at
//! the smaller of the model's own side and [`PREVIEW_SIDE`]. The values of the next Build stay as they are. Edits are
//! *debounced*: the preview builds once [`DEBOUNCE`] has passed without another edit, on the build thread on native and
//! inline in a browser.
//!
//! The preview replaces the live simulation only when nothing is loaded, when the loaded run is at tick 0, or when the
//! loaded run is a preview. Otherwise it waits for that run to end, and a Build started after the edit replaces the
//! waiting preview. An edit that leaves the document invalid leaves the running preview alone.

use std::time::Duration;

use henad_compute::entry::ModelEntry;
use henad_compute::fault::Fault;
use henad_compute::simulation::RunSetup;
use henad_core::action::Schedule;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamValue;
use web_time::Instant;

use crate::state::{AppState, PreviewRun, setup_message, too_large_message};

/// Largest side of a preview's grid in cells.
pub const PREVIEW_SIDE: u32 = 256;

/// Time without an edit after which the preview builds.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// Text the Editor shows while the preview waits for another run.
pub const PREVIEW_WAITS: &str = "Preview waits for the running model.";

/// State of the preview, as the Editor shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PreviewState {
    /// No preview runs or waits.
    #[default]
    Idle,
    /// A preview waits for the loaded run to end.
    Waiting,
    /// The preview runs at this size.
    Running(PreviewRun),
    /// The preview was not built, with every problem found.
    Refused(Vec<CodeDiagnostic>),
    /// The preview stopped on a fault of its model.
    Faulted(CodeDiagnostic),
}

/// Live preview of the Editor's graph model.
#[derive(Debug, Default)]
pub struct Preview {
    /// Preview key of the graph the preview last took up, built or waiting.
    key: Option<[u64; 3]>,
    /// Time the waiting preview builds, `None` while none waits.
    due: Option<Instant>,
    /// Number of the app's builds when the waiting preview's edit was made.
    builds: u64,
    /// State the Editor shows.
    pub state: PreviewState,
}

impl Preview {
    /// Shows `fault`, a fault of the preview's run, and drops any waiting preview.
    pub fn show_fault(&mut self, fault: &Fault) {
        self.due = None;
        self.state = PreviewState::Faulted(CodeDiagnostic::error(crate::ui::fault::detail(fault)));
    }

    /// Returns whether a preview waits for its time or for the loaded run to end.
    #[cfg(test)]
    pub fn is_due(&self) -> bool {
        self.due.is_some()
    }
}

/// Builds the preview once its time has come and the live simulation can give way, as [`poll_at`] does now.
pub fn poll(ctx: &egui::Context, app: &mut AppState) {
    poll_at(ctx, app, Instant::now());
}

/// Builds the preview at `now` once its time has come and the live simulation can give way.
///
/// A valid edit since the last call sets the time to `now` plus [`DEBOUNCE`], and an invalid one drops a waiting
/// preview. Called once per frame.
pub fn poll_at(ctx: &egui::Context, app: &mut AppState, now: Instant) {
    let builds = app.builds();
    let previewing = app.preview().is_some();
    let can_load = app.preview_can_load();
    let compute = app.gpu_ctx.is_some();
    let panel = &mut app.editor;
    let preview = &mut panel.preview;
    if matches!(preview.state, PreviewState::Running(_)) && !previewing {
        preview.state = PreviewState::Idle;
    }
    let Some(graph) = panel.graph.as_deref() else {
        preview.due = None;
        preview.key = None;
        if preview.state == PreviewState::Waiting {
            preview.state = PreviewState::Idle;
        }
        return;
    };
    if !compute {
        return;
    }
    let key = graph.preview_key();
    if preview.key != Some(key) {
        preview.key = Some(key);
        if graph.has_errors() {
            preview.due = None;
            if preview.state == PreviewState::Waiting {
                preview.state = PreviewState::Idle;
            }
        } else {
            preview.due = Some(now + DEBOUNCE);
            preview.builds = builds;
        }
    }
    let Some(due) = preview.due else {
        return;
    };
    if now < due {
        ctx.request_repaint_after(due - now);
        return;
    }
    if builds != preview.builds {
        // A Build started after the edit replaces the preview that waited.
        preview.due = None;
        preview.state = PreviewState::Idle;
        return;
    }
    if !can_load {
        preview.state = PreviewState::Waiting;
        return;
    }
    preview.due = None;
    build_preview(app);
}

/// Compiles the Editor's graph, and builds it as the preview from tick 0, or shows why it cannot.
fn build_preview(app: &mut AppState) {
    let Some(graph) = app.editor.graph.as_deref_mut() else {
        return;
    };
    let program = graph.preview_program();
    // Dropping a viewed output that no longer shows is no edit to preview again.
    app.editor.preview.key = Some(graph.preview_key());
    let entry = match program {
        Ok(program) => program.entry(),
        Err(problems) => {
            app.editor.preview.state = PreviewState::Refused(problems);
            return;
        }
    };
    let (values, run) = preview_values(&entry);
    let shortfalls = entry.shortfalls(&values, &app.runtime.granted);
    if !shortfalls.is_empty() {
        let problem = CodeDiagnostic::error(too_large_message(&shortfalls));
        app.editor.preview.state = PreviewState::Refused(vec![problem]);
        return;
    }
    match RunSetup::from_parts(&entry, &values, None, Schedule::default()) {
        Ok(setup) => {
            // A browser builds inline, and a fault of the build replaces this state.
            app.editor.preview.state = PreviewState::Running(run);
            app.load_preview(setup, run);
        }
        Err(error) => {
            let problem = CodeDiagnostic::error(setup_message(&error));
            app.editor.preview.state = PreviewState::Refused(vec![problem]);
        }
    }
}

/// Returns the values a preview of `entry` builds with, each parameter's default with each side of the grid at most
/// [`PREVIEW_SIDE`], and the preview's size.
pub fn preview_values(entry: &ModelEntry) -> (Vec<ParamValue>, PreviewRun) {
    let mut run = PreviewRun { width: 0, height: 0 };
    let values = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            let value = descriptor.kind().default_value();
            let side = match descriptor.id() {
                "grid_width" => &mut run.width,
                "grid_height" => &mut run.height,
                _ => return value,
            };
            match value {
                ParamValue::U32(default) => {
                    *side = default.min(PREVIEW_SIDE);
                    ParamValue::U32(*side)
                }
                other => other,
            }
        })
        .collect();
    (values, run)
}
