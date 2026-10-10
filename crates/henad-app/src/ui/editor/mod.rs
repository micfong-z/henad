//! The Editor tab: the text of one model file, applied to the running app.
//!
//! A *model file* holds a grid rule, a GPU grid model written as data with a WGSL step, as
//! [`henad_explore::model_file`] describes it. Apply reads the text, compiles the rule, and installs its entry through
//! [`AppState::install_models`]. A run of the rule past tick 0 then waits on the Rebuild model dialog, and a run at
//! tick 0 is rebuilt at once. A text that is refused leaves the running model alone, and the diagnostics list under
//! the code names each problem at its line and column. A press on a diagnostic moves the cursor to it.
//!
//! The device can refuse a rule that the compiler passed. A fault of a run built from the applied code belongs to the
//! edit until the run passes tick 0, and the tab shows it in place of the fault dialog. On native the rule builds on
//! the app's build thread, and a fault of the build arrives a frame or more after the build starts. In a browser the
//! device reports a rule it refuses after the build. A fault that no code causes, such as a lost device, opens the
//! dialog as before.

pub mod code_view;

use egui::{Button, CentralPanel, Frame, Label, Margin, Panel, RichText, ScrollArea, Sense};
use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::fault::{Fault, FaultKind};
use henad_compute::gpu::runtime_grid::GridRule;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::ModelSource;
use henad_explore::model_file::ModelFile;

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CHECK, MDI_FILE_PLUS_OUTLINE, MDI_FOLDER_OPEN_OUTLINE, MDI_TRAY_ARROW_DOWN,
};
use crate::state::{AppState, too_large_message};
use crate::ui::dock::Tab;
use crate::ui::files::{DialogFile, OpenResult, OpenTarget, SaveTarget};
use crate::ui::sweep::layout::add_button;

/// Text the Editor shows on an adapter that cannot run compute shaders.
pub const NO_COMPUTE: &str = "Grid rules cannot run on this device. They need a GPU with compute support.";

/// Reason Apply and Save model are disabled while the Editor holds no text.
pub const EMPTY_EDITOR: &str = "Editor is empty. Press New grid rule or Open model to start.";

/// Name of a saved model file whose text does not read as a model file.
const FALLBACK_FILE_NAME: &str = "model.toml";

/// Default height in points of the diagnostics list.
const DIAGNOSTICS_HEIGHT: f32 = 120.0;

/// State of the Editor tab: the text being edited and the outcome of the last Apply.
#[derive(Debug, Default)]
pub struct EditorPanel {
    /// Text of the model file, as typed.
    pub text: String,
    /// Name of the file the text was opened from or saved to, `None` for a text that has no file.
    pub file_name: Option<String>,
    /// Outcome of the last Apply, `None` once the text is replaced.
    pub outcome: Option<ApplyOutcome>,
    /// Status of the last open or save.
    pub file_status: Option<String>,
    /// Source of the entry the last Apply installed. A fault of a run built from it belongs to the edit until the run
    /// passes tick 0.
    applied: Option<ModelSource>,
    /// Character of the text the code view scrolls to on its next frame, after a press on a diagnostic.
    scroll_to: Option<usize>,
    /// Whether the code field's undo history is cleared on its next frame, after the text was replaced from outside
    /// the field.
    reset_history: bool,
    /// Whether the code view returns to the top left on its next frame, after another file's text replaced the text.
    reset_scroll: bool,
}

/// Outcome of a press on Apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The model was installed, and runs from tick 0, or builds to run from it.
    Applied,
    /// The model was installed, and its own run past tick 0 waits on the Rebuild model dialog.
    AwaitingRebuild,
    /// The model was installed and waits for Build, behind a run of another model or a canceled Rebuild model dialog,
    /// or once its own build or run has stopped.
    Installed,
    /// The text holds the code the loaded model runs.
    Unchanged,
    /// The text was refused, with every problem found.
    Refused(Vec<CodeDiagnostic>),
    /// The model was installed, and the device or the engine refused it when it was built, or the device cannot hold
    /// it at the values of the next build.
    Faulted(Vec<CodeDiagnostic>),
}

impl ApplyOutcome {
    /// Returns the line the Editor shows for the outcome, and whether it reports an error.
    pub fn summary(&self) -> (&'static str, bool) {
        match self {
            Self::Applied => ("Model applied.", false),
            Self::AwaitingRebuild => ("Model applied. Press Rebuild to run it from tick 0.", false),
            Self::Installed => ("Model applied. Press Build to run it.", false),
            Self::Unchanged => ("Model code unchanged.", false),
            Self::Refused(_) => ("Model not applied. Fix the problems below.", true),
            Self::Faulted(_) => ("Model build failed. Fix the problem below.", true),
        }
    }

    /// Problems the outcome lists, empty for an outcome without any.
    pub fn diagnostics(&self) -> &[CodeDiagnostic] {
        match self {
            Self::Refused(diagnostics) | Self::Faulted(diagnostics) => diagnostics,
            Self::Applied | Self::AwaitingRebuild | Self::Installed | Self::Unchanged => &[],
        }
    }
}

impl EditorPanel {
    /// Returns whether `source` is the source of the entry the last Apply installed.
    pub fn is_applied(&self, source: &ModelSource) -> bool {
        self.applied.as_ref() == Some(source)
    }

    /// Returns whether `fault`, a fault of a run built from an entry of `source`, belongs to the last edit applied
    /// here.
    ///
    /// A fault that an edit of the code cannot fix stays with the fault dialog: a lost device, a failed wait for the
    /// GPU, and a device out of memory. The caller also checks that the run has not passed tick 0.
    pub fn claims(&self, source: &ModelSource, fault: &Fault) -> bool {
        let code_fault = !matches!(fault.kind, FaultKind::DeviceLost | FaultKind::Poll(_)) && !fault.is_out_of_memory();
        code_fault && self.is_applied(source)
    }

    /// Shows `fault`, a fault of a run built from the applied code, as the outcome of the edit.
    pub fn show_fault(&mut self, fault: &Fault) {
        let problem = CodeDiagnostic::error(crate::ui::fault::detail(fault));
        self.outcome = Some(ApplyOutcome::Faulted(vec![problem]));
    }

    /// Replaces the text with `text`, from the file `file_name` or from no file.
    ///
    /// The outcome of the last Apply is cleared, and a later fault of the applied code no longer belongs to the
    /// Editor.
    fn load(&mut self, text: String, file_name: Option<String>) {
        self.text = text;
        self.file_name = file_name;
        self.outcome = None;
        self.applied = None;
        self.scroll_to = None;
        self.replace_view();
    }

    /// Clears the code field's undo history and returns the view to the top left on the field's next frame, after
    /// the text of another file replaced the text.
    fn replace_view(&mut self) {
        self.reset_history = true;
        self.reset_scroll = true;
    }

    /// Brings an outcome that waited up to date. `loaded` is the source of the loaded model, and `rebuild_pending`
    /// whether the Rebuild model dialog is open.
    ///
    /// An outcome that waited for Rebuild or Build reads as applied once the loaded model runs the applied code, from
    /// the start of its build. One that waited for Rebuild waits for Build once the dialog closes without a rebuild.
    /// An outcome that read as applied waits for Build once nothing is loaded, as after Cancel or Offload.
    pub fn settle_outcome(&mut self, loaded: Option<&ModelSource>, rebuild_pending: bool) {
        let waiting = matches!(
            self.outcome,
            Some(ApplyOutcome::AwaitingRebuild | ApplyOutcome::Installed)
        );
        if waiting && loaded.is_some() && loaded == self.applied.as_ref() {
            self.outcome = Some(ApplyOutcome::Applied);
        } else if (self.outcome == Some(ApplyOutcome::AwaitingRebuild) && !rebuild_pending)
            || (self.outcome == Some(ApplyOutcome::Applied) && loaded.is_none())
        {
            self.outcome = Some(ApplyOutcome::Installed);
        }
    }
}

/// Request from the Editor, applied once the tab has drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorRequest {
    NewGridRule,
    OpenModel,
    SaveModel,
    Apply,
    /// Moves the cursor to a line and a column of the text, both counted from 1.
    MoveCursor {
        line: u32,
        column: u32,
    },
}

/// Draws the Editor tab: its buttons and status, the code, and the outcome of the last Apply.
pub fn editor_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let compute = app.gpu_ctx.is_some();
    let mut request = None;
    let loaded = app.loaded_entry().map(|entry| entry.source().clone());
    let rebuild_pending = app.pending_rebuild.is_some();
    let panel = &mut app.editor;
    panel.settle_outcome(loaded.as_ref(), rebuild_pending);
    let empty = panel.text.is_empty();

    Panel::top("henad_editor_header")
        .resizable(false)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                header_buttons(ui, compute, empty, &mut request);
            });
            if let Some(name) = &panel.file_name {
                ui.weak(name);
            }
            if let Some(status) = &panel.file_status {
                ui.label(status);
            }
            if !compute {
                ui.colored_label(ui.visuals().warn_fg_color, format!("{MDI_ALERT} {NO_COMPUTE}"));
            }
        });

    Panel::bottom("henad_editor_diagnostics")
        .resizable(true)
        .default_size(DIAGNOSTICS_HEIGHT)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("henad_editor_diagnostics_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| outcome_ui(ui, panel, &mut request));
        });

    let marked: Vec<u32> = panel
        .outcome
        .as_ref()
        .map(|outcome| {
            outcome
                .diagnostics()
                .iter()
                .filter_map(|diagnostic| diagnostic.line)
                .collect()
        })
        .unwrap_or_default();
    let scroll_to = panel.scroll_to.take();
    let reset = code_view::ViewReset {
        history: std::mem::take(&mut panel.reset_history),
        scroll: std::mem::take(&mut panel.reset_scroll),
    };
    CentralPanel::no_frame().show(ui, |ui| {
        code_view::code_view(ui, &mut panel.text, &marked, scroll_to, reset);
    });

    if let Some(request) = request {
        apply_request(ui.ctx(), app, request);
    }
}

/// Draws New grid rule, Open model, Save model and Apply, each setting `request` when pressed.
fn header_buttons(ui: &mut egui::Ui, compute: bool, empty: bool, request: &mut Option<EditorRequest>) {
    let new = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FILE_PLUS_OUTLINE} New grid rule")),
        "New grid rule",
    )
    .on_hover_text("Replace text with Game of Life rule");
    if new.clicked() {
        *request = Some(EditorRequest::NewGridRule);
    }
    let open = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FOLDER_OPEN_OUTLINE} Open model")),
        "Open model",
    )
    .on_hover_text("Replace text with TOML model file");
    if open.clicked() {
        *request = Some(EditorRequest::OpenModel);
    }
    let save = add_button(
        ui,
        !empty,
        Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save model")),
        "Save model",
    )
    .on_hover_text("Save text to TOML model file")
    .on_disabled_hover_text(EMPTY_EDITOR);
    if save.clicked() {
        *request = Some(EditorRequest::SaveModel);
    }
    let refusal = if !compute {
        Some(NO_COMPUTE)
    } else if empty {
        Some(EMPTY_EDITOR)
    } else {
        None
    };
    let apply = add_button(
        ui,
        refusal.is_none(),
        Button::new(format!("{MDI_CHECK} Apply")),
        "Apply",
    )
    .on_hover_text("Check model and install it")
    .on_disabled_hover_text(refusal.unwrap_or_default());
    if apply.clicked() {
        *request = Some(EditorRequest::Apply);
    }
}

/// Draws the outcome of the last Apply: its line, then one label per problem. A press on a placed problem sets
/// `request` to move the cursor there.
fn outcome_ui(ui: &mut egui::Ui, panel: &EditorPanel, request: &mut Option<EditorRequest>) {
    let Some(outcome) = &panel.outcome else {
        if panel.text.is_empty() {
            ui.weak(EMPTY_EDITOR);
        } else {
            ui.weak("Press Apply to check and install the model.");
        }
        return;
    };
    let (line, error) = outcome.summary();
    if error {
        ui.colored_label(ui.visuals().error_fg_color, line);
    } else {
        ui.label(line);
    }
    for diagnostic in outcome.diagnostics() {
        let text = RichText::new(diagnostic_text(diagnostic))
            .monospace()
            .color(ui.visuals().error_fg_color);
        let placed = diagnostic.line.zip(diagnostic.column);
        let label = Label::new(text).selectable(true);
        let response = match placed {
            Some(_) => ui
                .add(label.sense(Sense::click()))
                .on_hover_text("Move cursor to problem"),
            None => ui.add(label),
        };
        if let Some((line, column)) = placed
            && response.clicked()
        {
            *request = Some(EditorRequest::MoveCursor { line, column });
        }
        for note in &diagnostic.notes {
            ui.weak(format!("    note: {note}"));
        }
    }
}

/// Returns the line of `diagnostic` in the list, `<line>:<column> <message>`, or the message alone for a problem
/// without a place.
pub fn diagnostic_text(diagnostic: &CodeDiagnostic) -> String {
    match (diagnostic.line, diagnostic.column) {
        (Some(line), Some(column)) => format!("{line}:{column} {}", diagnostic.message),
        (Some(line), None) => format!("{line} {}", diagnostic.message),
        _ => diagnostic.message.clone(),
    }
}

/// Applies `request`, once the tab has drawn.
fn apply_request(ctx: &egui::Context, app: &mut AppState, request: EditorRequest) {
    match request {
        EditorRequest::NewGridRule => new_grid_rule(app),
        EditorRequest::OpenModel => open_model(app),
        EditorRequest::SaveModel => save_model(app),
        EditorRequest::Apply => apply(app),
        EditorRequest::MoveCursor { line, column } => move_cursor_to(ctx, &mut app.editor, line, column),
    }
}

/// Moves the cursor of the code field to `column` on `line` of the text, both counted from 1, focuses the field, and
/// scrolls the place into view on the next frame.
///
/// A place past the end of the text, which an edit since the last Apply can leave, moves the cursor to the nearest
/// character.
pub fn move_cursor_to(ctx: &egui::Context, panel: &mut EditorPanel, line: u32, column: u32) {
    let index = code_view::char_index_at(&panel.text, line, column);
    code_view::move_cursor(ctx, index);
    panel.scroll_to = Some(index);
}

/// Returns the text of the Life starter, the model file of [`GridRule::life`].
pub fn life_starter() -> String {
    // The Life starter holds no text a model file cannot hold.
    ModelFile::from_rule(GridRule::life()).map_or_else(|_| String::new(), |file| file.to_toml())
}

/// Replaces the Editor's text with the Life starter and brings the Editor to the front.
pub fn new_grid_rule(app: &mut AppState) {
    app.editor.load(life_starter(), None);
    app.editor.file_status = None;
    app.focus_request = Some(Tab::Editor);
}

/// Returns the model file text of `entry`, the canonical text of a grid rule applied at run time, or `None` for a
/// model whose code the Editor cannot show.
pub fn editable_text(entry: &ModelEntry) -> Option<&str> {
    entry
        .source()
        .code()
        .filter(|code| code.format() == GridRule::CODE_FORMAT)
        .map(henad_core::provenance::ModelCode::text)
}

/// Replaces the Editor's text with the model file of the selected model, and brings the Editor to the front.
///
/// The text stays, edits since included, when the selected model's code is the code the Editor applied last.
pub fn edit_model(app: &mut AppState) {
    let Some(entry) = app.selected_entry() else {
        return;
    };
    let Some(text) = editable_text(entry).map(str::to_owned) else {
        return;
    };
    if !app.editor.is_applied(entry.source()) {
        app.editor.load(text, None);
        app.editor.file_status = None;
    }
    app.focus_request = Some(Tab::Editor);
}

/// Opens a dialog that picks a model file for the Editor.
pub fn open_model(app: &AppState) {
    app.open_file(OpenTarget::Model);
}

/// Opens a dialog that saves the Editor's text as written, comments included.
pub fn save_model(app: &mut AppState) {
    if app.editor.text.is_empty() {
        return;
    }
    let name = save_name(&app.editor);
    let bytes = app.editor.text.clone().into_bytes();
    app.save_as(SaveTarget::Model, &name, bytes);
}

/// Returns the file name a save of the Editor's text suggests: the name it was opened or saved under, then the rule's
/// id, then [`FALLBACK_FILE_NAME`].
fn save_name(panel: &EditorPanel) -> String {
    if let Some(name) = &panel.file_name {
        return name.clone();
    }
    ModelFile::parse(&panel.text).map_or_else(
        |_| FALLBACK_FILE_NAME.to_owned(),
        |file| format!("{}.toml", file.rule().id),
    )
}

/// Handles the result of an Open model dialog. The picked file replaces the Editor's text, and is applied only on
/// Apply.
pub fn receive_open(app: &mut AppState, result: OpenResult) {
    let file = match result {
        OpenResult::Files(files) => files.into_iter().next(),
        OpenResult::Failed(message) => {
            app.editor.file_status = Some(format!("Open failed: {message}"));
            return;
        }
        OpenResult::Folder(_) | OpenResult::Canceled => return,
    };
    let Some(DialogFile { name, bytes, .. }) = file else {
        return;
    };
    match String::from_utf8(bytes) {
        Ok(text) => {
            app.editor.file_status = Some(format!("Opened {name}"));
            app.editor.load(text, Some(name));
            app.focus_request = Some(Tab::Editor);
        }
        Err(_) => app.editor.file_status = Some(format!("Open failed: {name} is not UTF-8 text")),
    }
}

/// Reads the Editor's text as a model file, compiles its rule, and installs and runs the model.
///
/// A text that is refused leaves the set and the running model alone, and the Editor lists the problems. Otherwise
/// the model replaces the set's entry with its id, or joins the set, and becomes the selection:
///
/// - a run of the model past tick 0 keeps going and waits on the Rebuild model dialog, as
///   [`AppState::install_models`] leaves it, and a run of the model at tick 0 is rebuilt at once. The dialog opens
///   again for a run whose code differs from the set's entry, as after a canceled dialog.
/// - with no run, a run of another model still at tick 0, or a build of another model that no run to a tick waits
///   for, the model is built as Build builds it. A model the device cannot hold at the values of the next build is
///   not built, and the Editor names the sizes the device lacks.
/// - a run of another model past tick 0 keeps going, and the model waits for Build.
///
/// A build runs on the app's build thread on native, and the outcome reads as applied from its start. A fault of the
/// build that the Editor claims replaces the outcome when the build returns it.
pub fn apply(app: &mut AppState) {
    if app.gpu_ctx.is_none() {
        app.editor.outcome = Some(ApplyOutcome::Refused(vec![CodeDiagnostic::error(NO_COMPUTE)]));
        return;
    }
    let entry = match ModelFile::parse(&app.editor.text).and_then(|file| file.entry()) {
        Ok(entry) => entry,
        Err(error) => {
            app.editor.outcome = Some(ApplyOutcome::Refused(error.diagnostics().to_vec()));
            return;
        }
    };
    let id = entry.id().to_owned();
    let source = entry.source().clone();
    let runs_source = |app: &AppState| app.loaded_entry().is_some_and(|loaded| loaded.source() == &source);
    let unchanged = runs_source(app) && app.models.get(&id).is_some_and(|held| held.source() == &source);

    app.editor.outcome = None;
    app.editor.applied = Some(source.clone());
    if !unchanged {
        let mut models = ModelSet::new(henad_core::build_info!());
        if let Err(error) = models.insert(entry) {
            app.editor.outcome = Some(ApplyOutcome::Refused(vec![CodeDiagnostic::error(error.to_string())]));
            return;
        }
        app.install_models(&models);
        // `install_models` passes over an entry the set already holds, as after a canceled dialog. The run still
        // differs from the applied code, and the dialog has to open again.
        let loaded_differs = app
            .loaded_entry()
            .is_some_and(|loaded| loaded.id() == id && loaded.source() != &source);
        if loaded_differs && app.pending_rebuild.is_none() {
            app.settle_loaded();
        }
    }
    app.select_model(&id);
    if app.editor.outcome.is_some() {
        // In a browser the rebuild at tick 0 builds inline, and its fault went to the Editor already.
        return;
    }
    if !runs_source(app) && app.pending_rebuild.is_none() && app.run_at_start() {
        // Build is disabled for the same reason. In a browser the engine's own check would end the page.
        let shortfalls = app.selection_shortfalls();
        if !shortfalls.is_empty() {
            let problem = CodeDiagnostic::error(too_large_message(&shortfalls));
            app.editor.outcome = Some(ApplyOutcome::Faulted(vec![problem]));
            return;
        }
        app.reset_simulation();
        if app.editor.outcome.is_some() {
            return;
        }
    }
    let awaits_rebuild = app.pending_rebuild.as_ref().is_some_and(|pending| pending.model == id);
    app.editor.outcome = Some(if unchanged {
        ApplyOutcome::Unchanged
    } else if runs_source(app) {
        ApplyOutcome::Applied
    } else if awaits_rebuild {
        ApplyOutcome::AwaitingRebuild
    } else {
        ApplyOutcome::Installed
    });
}

#[cfg(test)]
mod tests {
    use henad_core::diagnostic::CodeDiagnostic;

    use super::{ApplyOutcome, diagnostic_text};

    #[test]
    fn a_diagnostic_reads_line_column_and_message() {
        let placed = CodeDiagnostic::error("loops are not allowed in a grid rule").with_span(10..14, 7, 5);
        assert_eq!(diagnostic_text(&placed), "7:5 loops are not allowed in a grid rule");
        let unplaced = CodeDiagnostic::error("the template failed");
        assert_eq!(diagnostic_text(&unplaced), "the template failed");
    }

    #[test]
    fn only_a_refusal_or_a_fault_reads_as_an_error() {
        for outcome in [
            ApplyOutcome::Applied,
            ApplyOutcome::AwaitingRebuild,
            ApplyOutcome::Installed,
            ApplyOutcome::Unchanged,
        ] {
            assert!(!outcome.summary().1, "{outcome:?}");
            assert!(outcome.diagnostics().is_empty());
        }
        assert!(ApplyOutcome::Refused(Vec::new()).summary().1);
        assert!(ApplyOutcome::Faulted(Vec::new()).summary().1);
    }
}
