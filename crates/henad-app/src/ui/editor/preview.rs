//! Live preview of the Editor's graph model or sheet: a run of the document at a small size, rebuilt from tick 0 after
//! each valid edit.
//!
//! A graph's preview plays once it is built. A sheet's waits paused at tick 0, with its start drawn, until Play.
//!
//! A *preview* runs the document's compiled program with its parameters at their defaults, each side of a grid at the
//! smaller of the model's own side and [`PREVIEW_SIDE`], and the agents of an agent model at the smaller of the model's
//! own count and [`PREVIEW_AGENTS`], in a world of the model's own size. The values of the next Build stay as they
//! are. Edits are *debounced*: the preview builds once [`DEBOUNCE`] has passed without another edit, on the build
//! thread on native and inline in a browser.
//!
//! The preview replaces the live simulation only when nothing is loaded, when the loaded run is at tick 0, or when the
//! loaded run is a preview. Otherwise it waits for that run to end, and a Build started after the edit replaces the
//! waiting preview. An edit that leaves the document invalid leaves the running preview alone.
//!
//! A *tried value* is a value of a parameter that the Parameters tab sets in the preview while the model keeps its own.
//! The preview runs with it and every rebuild of the preview keeps it, until the value the model keeps changes,
//! Reset or Keep ends it, or the Editor loads another document.

use std::time::Duration;

use henad_compute::entry::ModelEntry;
use henad_compute::simulation::RunSetup;
use henad_core::action::Schedule;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::explore::value::check_value;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use web_time::Instant;

use super::EditorModel;
use crate::state::{AppState, PreviewRun, setup_message, too_large_message};

/// Largest side of a preview's grid in cells.
pub const PREVIEW_SIDE: u32 = 256;

/// Largest number of a preview's agents.
pub const PREVIEW_AGENTS: u32 = 16384;

/// Time without an edit after which the preview builds.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// Text the Editor shows while the preview waits for another run.
pub const PREVIEW_WAITS: &str = "Preview waits for the running model.";

/// Ids of the parameters an entry puts in front of a graph's own: the size of its grid, or the number of its agents
/// and the size of their world. The preview sets the sizes itself, and the Parameters tab tries none of them.
pub const SIZE_PARAMS: [&str; 5] = ["grid_width", "grid_height", "num_agents", "world_width", "world_height"];

/// A value the Parameters tab tries in the preview while the model keeps its own.
#[derive(Debug, Clone, PartialEq)]
pub struct TriedValue {
    /// Id of the parameter.
    pub id: String,
    /// Value the preview runs with.
    pub value: ParamValue,
    /// Value the model kept when the value was tried. A change of the value the model keeps ends the trial.
    pub keeps: ParamValue,
}

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
    /// Whether a button of the model was pressed in the running preview since it was built.
    pub acted: bool,
    /// Values the Parameters tab tries in the preview, at most one per parameter.
    tried: Vec<TriedValue>,
}

impl Preview {
    /// Returns the value tried for parameter `id`, `None` while the preview runs with the value the model keeps.
    pub fn tried(&self, id: &str) -> Option<&TriedValue> {
        self.tried.iter().find(|tried| tried.id == id)
    }

    /// Every value tried in the preview.
    pub fn tried_values(&self) -> &[TriedValue] {
        &self.tried
    }

    /// Tries `value` for parameter `id` while the model keeps `keeps`. A value equal to `keeps` ends the trial.
    pub fn try_value(&mut self, id: &str, value: ParamValue, keeps: ParamValue) {
        self.tried.retain(|tried| tried.id != id);
        if value != keeps {
            self.tried.push(TriedValue {
                id: id.to_owned(),
                value,
                keeps,
            });
        }
    }

    /// Ends the trial of parameter `id`, and returns whether one ran.
    pub fn end_trial(&mut self, id: &str) -> bool {
        let before = self.tried.len();
        self.tried.retain(|tried| tried.id != id);
        self.tried.len() != before
    }

    /// Ends each trial whose parameter no longer keeps the value it kept when tried, as `keeps` returns it.
    fn end_stale_trials(&mut self, keeps: impl Fn(&str) -> Option<ParamValue>) {
        self.tried
            .retain(|tried| keeps(&tried.id).as_ref() == Some(&tried.keeps));
    }

    /// Rebuilds the preview at tick 0 once [`DEBOUNCE`] has passed after `now`, keeping its key and its tried values.
    ///
    /// `builds` is the number of the app's builds now. A Build started before the time comes replaces the rebuild, as
    /// it replaces a preview that waits after an edit.
    pub fn rebuild_after(&mut self, now: Instant, builds: u64) {
        self.due = Some(now + DEBOUNCE);
        self.builds = builds;
    }

    /// Shows `problem`, the fault of the preview's run, and drops any waiting preview.
    pub fn show_problem(&mut self, problem: CodeDiagnostic) {
        self.due = None;
        self.state = PreviewState::Faulted(problem);
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
    // A sheet with no property shows nothing, and the preview of what it held before goes.
    if app.preview().is_some() && app.editor.model.preview_source().is_some_and(|source| source.empty) {
        app.offload_simulation();
    }
    end_stale_trials(app);
    let builds = app.builds();
    let previewing = app.preview().is_some();
    let can_load = app.preview_can_load();
    let compute = app.gpu_ctx.is_some();
    let panel = &mut app.editor;
    let preview = &mut panel.preview;
    if matches!(preview.state, PreviewState::Running(_)) && !previewing {
        preview.state = PreviewState::Idle;
    }
    let Some(source) = panel.model.preview_source() else {
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
    if preview.key != Some(source.key) {
        preview.key = Some(source.key);
        if source.has_errors {
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

/// Builds the Editor's sheet again as the preview from tick 0, at the same seed, so it starts as it started before.
///
/// Playback's Build calls it while the preview of a sheet runs. It waits paused at tick 0, as a new preview does.
pub fn rebuild_preview(app: &mut AppState) {
    app.editor.preview.due = None;
    build_preview(app);
}

/// Compiles the Editor's graph or sheet, and builds it as the preview from tick 0, or shows why it cannot.
fn build_preview(app: &mut AppState) {
    let Some(program) = app.editor.model.preview_program() else {
        return;
    };
    // Dropping a viewed output that no longer shows is no edit to preview again.
    app.editor.preview.key = app.editor.model.preview_source().map(|source| source.key);
    let entry = match program {
        Ok(program) => program.entry(),
        Err(problems) => {
            app.editor.preview.state = PreviewState::Refused(problems);
            return;
        }
    };
    let (mut values, run) = preview_values(&entry);
    with_tried(&entry, &mut values, app.editor.preview.tried_values());
    let shortfalls = app.entry_shortfalls(&entry, &values);
    if !shortfalls.is_empty() {
        let problem = CodeDiagnostic::error(too_large_message(&shortfalls));
        app.editor.preview.state = PreviewState::Refused(vec![problem]);
        return;
    }
    let plays = app.editor.model.preview_plays();
    match RunSetup::from_parts(&entry, &values, None, Schedule::default()) {
        Ok(setup) => {
            // A browser builds inline, and a fault of the build replaces this state.
            app.editor.preview.state = PreviewState::Running(run);
            app.editor.preview.acted = false;
            if let EditorModel::Sheet(sheet) = &mut app.editor.model {
                sheet.mark_built();
            }
            app.load_preview(setup, run, plays);
        }
        Err(error) => {
            let problem = CodeDiagnostic::error(setup_message(&error));
            app.editor.preview.state = PreviewState::Refused(vec![problem]);
        }
    }
}

/// Returns the values a preview of `entry` builds with, each parameter's default with each side of a grid at most
/// [`PREVIEW_SIDE`] and the number of agents at most [`PREVIEW_AGENTS`], and the preview's size.
pub fn preview_values(entry: &ModelEntry) -> (Vec<ParamValue>, PreviewRun) {
    let (mut width, mut height, mut agents) = (0, 0, None);
    let values = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            let value = descriptor.kind().default_value();
            let (size, limit) = match descriptor.id() {
                "grid_width" => (&mut width, PREVIEW_SIDE),
                "grid_height" => (&mut height, PREVIEW_SIDE),
                "num_agents" => (agents.insert(0), PREVIEW_AGENTS),
                _ => return value,
            };
            match value {
                ParamValue::U32(default) => {
                    *size = default.min(limit);
                    ParamValue::U32(*size)
                }
                other => other,
            }
        })
        .collect();
    let run = match agents {
        Some(count) => PreviewRun::Agents { count },
        None => PreviewRun::Grid { width, height },
    };
    (values, run)
}

/// Puts each value of `tried` into `values`, the values of `entry`, in place of its parameter's value, where `entry`
/// has that parameter and its descriptor accepts the value. A size is never tried.
fn with_tried(entry: &ModelEntry, values: &mut [ParamValue], tried: &[TriedValue]) {
    for (descriptor, value) in entry.param_descriptors().iter().zip(values.iter_mut()) {
        if SIZE_PARAMS.contains(&descriptor.id()) {
            continue;
        }
        if let Some(tried) = tried.iter().find(|tried| tried.id == descriptor.id())
            && check_value(descriptor.kind(), &tried.value).is_ok()
        {
            value.clone_from(&tried.value);
        }
    }
}

/// Returns the value that `model`, the Editor's document, keeps for the parameter `descriptor` describes: the value of
/// the sheet's parameter or the default of the graph's, as `descriptor` types it. The default of `descriptor` stands
/// for a parameter the document does not hold.
pub fn model_value(model: &EditorModel, descriptor: &ParamDescriptor) -> ParamValue {
    let fallback = descriptor.kind().default_value();
    match model {
        EditorModel::Sheet(sheet) => sheet
            .document()
            .parameters
            .iter()
            .find(|parameter| parameter.id == descriptor.id())
            .and_then(|parameter| typed(descriptor.kind(), parameter.default))
            .unwrap_or(fallback),
        EditorModel::Graph(graph) => graph
            .document()
            .params
            .iter()
            .find(|param| param.id() == descriptor.id())
            .map_or(fallback, |param| param.kind().default_value()),
        EditorModel::Empty | EditorModel::Rule(_) => fallback,
    }
}

/// Returns `number` as a value of `kind`, as the sheet's lowering narrows a parameter's value, or `None` for a kind no
/// sheet parameter lowers to or a number the kind cannot hold.
fn typed(kind: &ParamKind, number: f64) -> Option<ParamValue> {
    match kind {
        ParamKind::F32 { .. } => {
            let narrowed = number as f32;
            narrowed.is_finite().then_some(ParamValue::F32(narrowed))
        }
        ParamKind::U32 { .. } => (number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&number))
            .then_some(ParamValue::U32(number as u32)),
        _ => None,
    }
}

/// Ends each trial of the preview whose parameter the Editor's document now keeps at another value, as an edit of
/// that value on the sheet does.
fn end_stale_trials(app: &mut AppState) {
    if app.editor.preview.tried.is_empty() {
        return;
    }
    let Some(entry) = app.preview().and(app.loaded_entry()).cloned() else {
        return;
    };
    let panel = &mut app.editor;
    let model = &panel.model;
    panel.preview.end_stale_trials(|id| {
        entry
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id() == id)
            .map(|descriptor| model_value(model, descriptor))
    });
}
