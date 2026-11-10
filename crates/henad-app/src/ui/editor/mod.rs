//! The Editor tab: one model file, edited and applied to the running app.
//!
//! A *model file* holds a grid rule, a GPU grid model written as data with a WGSL step, a graph model, or a sheet, as
//! [`henad_explore::model_file`] describes them. The tab shows a grid rule as text in the code view, a graph model on
//! the canvas of [`graph`] with its passes and declarations beside it, and a sheet as its sheet and cards in [`sheet`].
//! A file opens by the `kind` in its header. Apply reads the file, compiles the model, and installs its entry through
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
pub mod compare;
pub mod expert;
pub mod graph;
pub mod preview;
pub mod sheet;

use egui::{Button, CentralPanel, Frame, Label, Margin, Panel, RichText, ScrollArea, Sense};
use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::fault::{Fault, FaultKind};
use henad_compute::gpu::runtime_grid::GridRule;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::{ModelCode, ModelSource};
use henad_explore::model_file::ModelFile;
use henad_graph::model_text::read_header;
use henad_graph::sheet::document::SheetDocument;
use henad_graph::sheet::sentence::{PREVIEW_WAITS as SHEET_PREVIEW_WAITS, SHEET_EMPTY, WORLD_EMPTY};
use henad_graph::sheet::{KIND_SHEET, ParsedSheet, SHEET_CODE_FORMAT};
use henad_graph::starters::{Starter, agent_starters, starters};
use henad_graph::{GraphFile, KIND_GRAPH};

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CHECK, MDI_FILE_DOCUMENT_PLUS_OUTLINE, MDI_FILE_PLUS_OUTLINE, MDI_FOLDER_OPEN_OUTLINE,
    MDI_GRAPH_OUTLINE, MDI_INFORMATION, MDI_PLAY_OUTLINE, MDI_TRAY_ARROW_DOWN,
};
use crate::state::{AppState, PreviewRun, too_large_message};
use crate::ui::dock::Tab;
use crate::ui::files::{DialogFile, OpenResult, OpenTarget, SaveTarget};
use crate::ui::params::sheet_number;
use crate::ui::sweep::layout::add_button;
use preview::{PREVIEW_WAITS, PreviewState};

/// Text the Editor shows on an adapter that cannot run compute shaders.
pub const NO_COMPUTE: &str =
    "Grid rules and graph models cannot run on this device. They need a GPU with compute support.";

/// Reason Apply and Save model are disabled while the Editor holds no model.
pub const EMPTY_EDITOR: &str =
    "Editor is empty. Press New model, New node graph, New grid rule or Open model to start.";

/// Label of the button that opens the New model dialog.
pub const NEW_MODEL_BUTTON: &str = henad_graph::sheet::sentence::NEW_MODEL;

/// Label of the menu that opens a graph starter on the canvas.
pub const NEW_NODE_GRAPH: &str = "New node graph";

/// Name of a saved model file whose text does not read as a model file.
const FALLBACK_FILE_NAME: &str = "model.toml";

/// Default height in points of the diagnostics list.
const DIAGNOSTICS_HEIGHT: f32 = 120.0;

/// Model under edit in the Editor tab, one variant per kind of model file.
#[derive(Debug, Default)]
pub enum EditorModel {
    /// No model. The code view shows an empty text.
    #[default]
    Empty,
    /// Text of a grid rule's model file, as typed. It may be empty, or not read as a model file at all.
    Rule(String),
    /// Graph model under edit on the canvas.
    Graph(Box<graph::GraphEditor>),
    /// Sheet model shown as its sheet and cards.
    Sheet(Box<sheet::SheetEditor>),
}

/// Key and error state of the model a preview runs, from [`EditorModel::preview_source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewSource {
    /// Key of what the preview shows. A preview built under another key is out of date.
    pub key: [u64; 3],
    /// Whether the model has errors. A model with errors leaves the running preview alone.
    pub has_errors: bool,
    /// Whether the model shows nothing, as a sheet with no property. The running preview is then taken down.
    pub empty: bool,
}

impl EditorModel {
    /// Returns whether the Editor holds no model: nothing, or a grid rule's text that is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Rule(text) => text.is_empty(),
            Self::Graph(_) | Self::Sheet(_) => false,
        }
    }

    /// Returns the text Apply and Save model read: a graph model's or a sheet's canonical text, the grid rule's text as
    /// typed, or an empty text.
    pub fn file_text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Rule(text) => text.clone(),
            Self::Graph(graph) => graph.text(),
            Self::Sheet(sheet) => sheet.text().to_owned(),
        }
    }

    /// Text of the grid rule, `None` while the Editor holds another model or nothing.
    pub fn rule_text(&self) -> Option<&str> {
        match self {
            Self::Rule(text) => Some(text),
            Self::Empty | Self::Graph(_) | Self::Sheet(_) => None,
        }
    }

    /// Graph model under edit, `None` while the Editor holds another model or nothing.
    pub fn graph(&self) -> Option<&graph::GraphEditor> {
        match self {
            Self::Graph(graph) => Some(graph),
            Self::Empty | Self::Rule(_) | Self::Sheet(_) => None,
        }
    }

    /// Graph model under edit, mutable, `None` while the Editor holds another model or nothing.
    pub fn graph_mut(&mut self) -> Option<&mut graph::GraphEditor> {
        match self {
            Self::Graph(graph) => Some(graph),
            Self::Empty | Self::Rule(_) | Self::Sheet(_) => None,
        }
    }

    /// Sheet in the Editor, `None` while the Editor holds another model or nothing.
    pub fn sheet(&self) -> Option<&sheet::SheetEditor> {
        match self {
            Self::Sheet(sheet) => Some(sheet),
            Self::Empty | Self::Rule(_) | Self::Graph(_) => None,
        }
    }

    /// Returns whether the model runs as a preview while it is in the Editor: a graph model or a sheet.
    pub fn holds_preview_model(&self) -> bool {
        matches!(self, Self::Graph(_) | Self::Sheet(_))
    }

    /// Returns whether the model's preview plays once it is built. A graph's preview does, and a sheet's waits at
    /// tick 0 for Play.
    pub fn preview_plays(&self) -> bool {
        matches!(self, Self::Graph(_))
    }

    /// Returns the key and error state of the model the preview runs, or `None` for a model with no preview.
    pub fn preview_source(&self) -> Option<PreviewSource> {
        match self {
            Self::Graph(graph) => Some(PreviewSource {
                key: graph.preview_key(),
                has_errors: graph.has_errors(),
                empty: false,
            }),
            Self::Sheet(sheet) => Some(PreviewSource {
                key: sheet.preview_key(),
                has_errors: sheet.has_errors() || sheet.shows_nothing(),
                empty: sheet.shows_nothing(),
            }),
            Self::Empty | Self::Rule(_) => None,
        }
    }

    /// Compiles the program the preview runs, or returns `None` for a model with no preview.
    ///
    /// Note that a graph whose viewed output no longer reads stops viewing it. The key of [`Self::preview_source`]
    /// then changes.
    pub fn preview_program(&mut self) -> Option<graph::CompiledProgram> {
        match self {
            Self::Graph(graph) => Some(graph.preview_program()),
            Self::Sheet(sheet) => Some(sheet.program()),
            Self::Empty | Self::Rule(_) => None,
        }
    }

    /// Returns `problem`, a problem of the model's run, placed in the model: a sheet places it at the line of its
    /// node, at the table of its pass, or at `[sheet]`. Any other model returns it as it is.
    pub fn place(&self, problem: CodeDiagnostic) -> CodeDiagnostic {
        match self {
            Self::Sheet(sheet) => sheet.place(problem),
            Self::Empty | Self::Rule(_) | Self::Graph(_) => problem,
        }
    }
}

/// State of the Editor tab: the model file being edited and the outcome of the last Apply.
#[derive(Debug, Default)]
pub struct EditorPanel {
    /// Model under edit: nothing, a grid rule's text, a graph model or a sheet.
    pub model: EditorModel,
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
    /// Live preview of the graph model or the sheet.
    pub preview: preview::Preview,
    /// New model dialog, `None` while it is closed.
    pub new_model: Option<sheet::new_model::NewModelDialog>,
    /// Path of the file the model was opened from on native, `None` for a model with no file, after a save, or in a
    /// browser.
    pub file_path: Option<std::path::PathBuf>,
    /// Name of the sheet the conversion dialog converts, `None` while the dialog is closed.
    pub convert: Option<String>,
    /// Model held beside the one shown: the sheet of a copy converted in this session, or the copy of a sheet opened
    /// from it.
    pub counterpart: Option<Box<expert::Counterpart>>,
    /// Sheet a copy was converted from, read from beside the copy's file, with its path.
    pub beside: Option<(std::path::PathBuf, Box<henad_graph::sheet::SheetFile>)>,
    /// Banner of the copy in the Editor, with the load, code revision and hash of the original's text it was worked
    /// out for.
    banner: Option<(BannerKey, Option<expert::CopyBanner>)>,
    /// Whether the side panel lists the changes of the copy since the conversion and the canvas marks them.
    pub show_changes: bool,
    /// Runs of the copy and its original that Compare runs started, `None` before Compare runs and after Close
    /// comparison.
    pub comparison: Option<Box<compare::Comparison>>,
}

/// Load, code revision and hash of the original's text that a copy's banner and comparison were worked out for.
pub type BannerKey = (u64, u64, Option<u64>);

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
    /// The opened file does not read as the kind its header names, with every problem found. Nothing was applied.
    Unread(Vec<CodeDiagnostic>),
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
            Self::Unchanged => ("Model unchanged.", false),
            Self::Refused(_) => ("Model not applied. Fix the problems below.", true),
            Self::Unread(_) => ("Model file has problems. Fix them, then press Apply.", true),
            Self::Faulted(_) => ("Model build failed. Fix the problem below.", true),
        }
    }

    /// Problems the outcome lists, empty for an outcome without any.
    pub fn diagnostics(&self) -> &[CodeDiagnostic] {
        match self {
            Self::Refused(diagnostics) | Self::Unread(diagnostics) | Self::Faulted(diagnostics) => diagnostics,
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
        Self::is_code_fault(fault) && self.is_applied(source)
    }

    /// Returns whether an edit of the code can fix `fault`: any fault but a lost device, a failed wait for the GPU and a
    /// device out of memory.
    pub fn is_code_fault(fault: &Fault) -> bool {
        !matches!(fault.kind, FaultKind::DeviceLost | FaultKind::Poll(_)) && !fault.is_out_of_memory()
    }

    /// Shows `fault`, a fault of a run built from the applied code, as the outcome of the edit. A sheet places the
    /// problem as [`EditorModel::place`] does.
    pub fn show_fault(&mut self, fault: &Fault) {
        let problem = self.model.place(CodeDiagnostic::error(crate::ui::fault::detail(fault)));
        self.outcome = Some(ApplyOutcome::Faulted(vec![problem]));
    }

    /// Shows `fault`, a fault of the preview's run, as the state of the preview. A sheet places the problem as
    /// [`EditorModel::place`] does.
    pub fn show_preview_fault(&mut self, fault: &Fault) {
        let problem = self.model.place(CodeDiagnostic::error(crate::ui::fault::detail(fault)));
        self.preview.show_problem(problem);
    }

    /// Replaces the text with `text`, from the file `file_name` or from no file.
    ///
    /// The outcome of the last Apply and the state of the preview are cleared, and a later fault of the applied code
    /// no longer belongs to the Editor.
    fn load(&mut self, text: String, file_name: Option<String>) {
        self.clear_expert();
        self.model = EditorModel::Rule(text);
        self.file_name = file_name;
        self.outcome = None;
        self.applied = None;
        self.scroll_to = None;
        self.preview = preview::Preview::default();
        self.replace_view();
    }

    /// Replaces the Editor's model with the graph `file`, from the file `file_name` or from no file.
    ///
    /// The outcome of the last Apply and the state of the preview are cleared, as [`Self::load`] clears them for a
    /// text.
    pub fn load_graph(&mut self, file: &GraphFile, file_name: Option<String>) {
        self.clear_expert();
        self.model = EditorModel::Graph(Box::new(graph::GraphEditor::new(file)));
        self.file_name = file_name;
        self.outcome = None;
        self.applied = None;
        self.scroll_to = None;
        self.preview = preview::Preview::default();
    }

    /// Replaces the Editor's model with `model`, from no file, and clears the file status line.
    pub fn load_model(&mut self, model: EditableModel) {
        match model {
            EditableModel::Sheet(document) => self.load_sheet(sheet::SheetEditor::new(*document), None),
            EditableModel::Graph(file) => self.load_graph(&file, None),
            EditableModel::Rule(text) => self.load(text, None),
        }
        self.file_status = None;
    }

    /// Replaces the Editor's model with `sheet`, from the file `file_name` or from no file.
    ///
    /// The outcome of the last Apply and the state of the preview are cleared, as [`Self::load`] clears them for a
    /// text.
    pub fn load_sheet(&mut self, sheet: sheet::SheetEditor, file_name: Option<String>) {
        self.clear_expert();
        self.model = EditorModel::Sheet(Box::new(sheet));
        self.file_name = file_name;
        self.outcome = None;
        self.applied = None;
        self.scroll_to = None;
        self.preview = preview::Preview::default();
    }

    /// Forgets the file path, the counterpart, the original read from beside the file, the banner and the conversion
    /// dialog, as a model from elsewhere replaces the Editor's.
    fn clear_expert(&mut self) {
        self.file_path = None;
        self.convert = None;
        self.counterpart = None;
        self.beside = None;
        self.banner = None;
        self.show_changes = false;
        self.comparison = None;
    }

    /// Clears the outcome of the last Apply, the state of the preview and the banner after the model and its
    /// counterpart changed places.
    pub fn settle_swap(&mut self) {
        self.outcome = None;
        self.applied = None;
        self.scroll_to = None;
        self.preview = preview::Preview::default();
        self.banner = None;
        self.convert = None;
        self.show_changes = false;
    }

    /// Clears the code field's undo history and returns the view to the top left on the field's next frame, after
    /// the text of another file replaced the text.
    fn replace_view(&mut self) {
        self.reset_history = true;
        self.reset_scroll = true;
    }

    /// Brings an outcome that waited up to date. `loaded` is the source of the loaded model, `next_build` the source of
    /// the model Playback's Build builds now, and `rebuild_pending` whether the Rebuild model dialog is open.
    ///
    /// An outcome that waited for Rebuild or Build reads as applied once the loaded model runs the applied code, from
    /// the start of its build. One that waited for Rebuild waits for Build once the dialog closes without a rebuild.
    /// An outcome that read as applied waits for Build once nothing is loaded, as after Cancel or Offload.
    ///
    /// An outcome that waits for Build is dropped once Build builds another model, as while it replays the preview of
    /// the Editor's sheet. `next_build` is `None` then.
    pub fn settle_outcome(
        &mut self,
        loaded: Option<&ModelSource>,
        next_build: Option<&ModelSource>,
        rebuild_pending: bool,
    ) {
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
        if self.outcome == Some(ApplyOutcome::Installed) && next_build != self.applied.as_ref() {
            self.outcome = None;
        }
    }
}

/// Request from the Editor, applied once the tab has drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorRequest {
    NewModel,
    NewGridRule,
    /// Opens the starter with this file name as a new graph model.
    NewGraphModel(&'static str),
    OpenModel,
    /// Opens a dialog that picks a model file to import a function from.
    PickImport,
    SaveModel,
    Apply,
    /// Opens the conversion dialog for the sheet.
    Convert,
    /// Converts the sheet, after Convert in the conversion dialog.
    ConfirmConvert,
    /// Opens the original of the copy.
    OpenOriginal,
    /// Opens the copy of the sheet.
    OpenCopy,
    /// Lists the changes of the copy since the conversion, or stops listing them.
    ShowChanges,
    /// Runs the copy beside its original and compares their measures.
    CompareRuns,
    /// Stops the comparison and closes it.
    CloseComparison,
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
    settle_outcome(app);
    let past_start = app.sheet_preview_past_start();
    let trials = sheet_trials(app);
    let fires = sheet_fires(app);
    let layout = app.layout;
    let panel = &mut app.editor;
    let empty = panel.model.is_empty();
    // The New model dialog takes the keys of Undo and Redo while it is open.
    let keys = panel.new_model.is_none();

    header_ui(ui, panel, compute, empty, &mut request);

    Panel::bottom("henad_editor_diagnostics")
        .resizable(true)
        .default_size(DIAGNOSTICS_HEIGHT)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("henad_editor_diagnostics_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if panel.model.graph().is_some() {
                        graph_outcome_ui(ui, panel);
                    } else if panel.model.sheet().is_some() {
                        sheet_outcome_ui(ui, panel);
                    } else {
                        outcome_ui(ui, panel, &mut request);
                    }
                });
        });

    let copy = expert::counterpart_copy(panel);
    let changes = expert::shown_changes(panel);
    let refusal = panel.model.sheet().and_then(expert::convert_refusal);

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
    CentralPanel::no_frame().show(ui, |ui| match &mut panel.model {
        EditorModel::Graph(graph) => {
            graph.shown_changes = changes;
            graph::graph_ui(ui, graph);
        }
        EditorModel::Sheet(sheet) => {
            let shown = sheet::SheetShown {
                past_start,
                trials: &trials,
                keys,
                copy: copy.as_deref(),
                refusal,
                fires: fires.as_ref(),
                layout,
            };
            if let Some(sheet_request) = sheet::sheet_ui(ui, sheet, &shown) {
                request = Some(match sheet_request {
                    sheet::SheetRequest::Convert => EditorRequest::Convert,
                    sheet::SheetRequest::OpenCopy => EditorRequest::OpenCopy,
                });
            }
        }
        EditorModel::Rule(text) => {
            code_view::code_view(ui, text, &marked, scroll_to, reset);
        }
        EditorModel::Empty => {
            // Typing into the empty code view starts a grid rule's text.
            let mut text = String::new();
            code_view::code_view(ui, &mut text, &marked, scroll_to, reset);
            if !text.is_empty() {
                panel.model = EditorModel::Rule(text);
            }
        }
    });
    if panel
        .model
        .graph_mut()
        .is_some_and(graph::GraphEditor::take_pick_import)
    {
        request = Some(EditorRequest::PickImport);
    }

    if expert::convert_modal(ui.ctx(), panel) {
        request = Some(EditorRequest::ConfirmConvert);
    }

    if let Some(request) = request {
        apply_request(ui.ctx(), app, request);
    }
}

/// Draws the header of the Editor tab: its buttons, the file name, the status of the last open or save, the reason
/// nothing runs without compute, and the banner of a copy converted from a sheet. A pressed button sets `request`.
fn header_ui(
    ui: &mut egui::Ui,
    panel: &mut EditorPanel,
    compute: bool,
    empty: bool,
    request: &mut Option<EditorRequest>,
) {
    Panel::top("henad_editor_header")
        .resizable(false)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| header_buttons(ui, compute, empty, request));
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
    if let Some(banner) = expert::copy_banner(panel) {
        Panel::top("henad_expert_banner")
            .resizable(false)
            .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
            .show(ui, |ui| expert::banner_ui(ui, &banner, panel.show_changes, request));
    }
    // A comparison of an earlier copy or original describes a model the Editor no longer holds.
    if let Some(key) = expert::current_copy_key(panel)
        && panel
            .comparison
            .as_ref()
            .is_some_and(|comparison| comparison.key != key)
    {
        panel.comparison = None;
    }
    if let Some(comparison) = panel.comparison.as_mut() {
        comparison.poll();
    }
    if panel.model.graph().is_some()
        && let Some(comparison) = &panel.comparison
    {
        compare::comparison_ui(ui, comparison, request);
    }
}

/// Returns the fire counts of the Editor's sheet in the latest snapshot of the loaded run, `None` for no sheet or a
/// run that counts none of its lines.
fn sheet_fires(app: &AppState) -> Option<sheet::LineFires> {
    let sheet = app.editor.model.sheet()?;
    sheet::line_fires(sheet, app.loaded_entry(), app.snapshot.as_ref())
}

/// Brings the outcome of the last Apply up to date with the loaded model, the model Build builds and the Rebuild model
/// dialog, as [`EditorPanel::settle_outcome`] does.
fn settle_outcome(app: &mut AppState) {
    // A preview runs no applied code, and an outcome that read as applied waits for Build behind it.
    let loaded = app
        .loaded_entry()
        .filter(|_| app.preview().is_none())
        .map(|entry| entry.source().clone());
    let next_build = app
        .selected_entry()
        .filter(|_| !app.build_replays_preview())
        .map(|entry| entry.source().clone());
    let rebuild_pending = app.pending_rebuild.is_some();
    app.editor
        .settle_outcome(loaded.as_ref(), next_build.as_ref(), rebuild_pending);
}

/// Returns each value the preview of the Editor's sheet tries, as the Sheet pane shows it beside its parameter. Empty
/// while no preview of the sheet runs.
fn sheet_trials(app: &AppState) -> Vec<sheet::sheet_pane::SheetTrial> {
    if !app.previews_editor_sheet() {
        return Vec::new();
    }
    app.editor
        .preview
        .tried_values()
        .iter()
        .filter_map(|tried| {
            Some(sheet::sheet_pane::SheetTrial {
                parameter: tried.id.clone(),
                value: sheet_number(&tried.value)?,
            })
        })
        .collect()
}

/// Draws the header buttons, each setting `request` when pressed.
///
/// The buttons are New model, New grid rule, New node graph, Open model, Save model and Apply.
fn header_buttons(ui: &mut egui::Ui, compute: bool, empty: bool, request: &mut Option<EditorRequest>) {
    let model = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FILE_DOCUMENT_PLUS_OUTLINE} {NEW_MODEL_BUTTON}")),
        NEW_MODEL_BUTTON,
    )
    .on_hover_text("Start new model");
    if model.clicked() {
        *request = Some(EditorRequest::NewModel);
    }
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
    let graph = ui
        .menu_button(format!("{MDI_GRAPH_OUTLINE} {NEW_NODE_GRAPH}"), |ui| {
            if let Some(file_name) = starter_menu(ui) {
                *request = Some(EditorRequest::NewGraphModel(file_name));
                ui.close();
            }
        })
        .response;
    graph.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, NEW_NODE_GRAPH));
    graph.on_hover_text("Start node graph from an example");
    let open = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FOLDER_OPEN_OUTLINE} Open model")),
        "Open model",
    )
    .on_hover_text("Replace model with TOML model file");
    if open.clicked() {
        *request = Some(EditorRequest::OpenModel);
    }
    let save = add_button(
        ui,
        !empty,
        Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save model")),
        "Save model",
    )
    .on_hover_text("Save model to TOML model file")
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

/// Draws one button per starter, grid starters first, labelled with the starter's model name, and returns the file name
/// of the one pressed.
pub fn starter_menu(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut picked = None;
    for starter in starters().iter().chain(agent_starters()) {
        let name = starter_name(starter);
        if ui.button(name).on_hover_text(starter.file_name).clicked() {
            picked = Some(starter.file_name);
        }
    }
    picked
}

/// Returns the model name of `starter`, or its file name when it does not read.
fn starter_name(starter: &Starter) -> String {
    GraphFile::parse(starter.text).map_or_else(|_| starter.file_name.to_owned(), |file| file.document().name.clone())
}

/// Draws the outcome of the last Apply of a graph model, the message of the last refused edit, and every problem the
/// checker finds now. A press on a problem at a node opens its pass and brings the node into view.
fn graph_outcome_ui(ui: &mut egui::Ui, panel: &mut EditorPanel) {
    let Some(graph) = panel.model.graph_mut() else {
        return;
    };
    let live: Vec<CodeDiagnostic> = graph.diagnostics().cloned().collect();
    preview_ui(ui, &panel.preview.state, true);
    match &panel.outcome {
        Some(outcome) => {
            let (line, error) = outcome.summary();
            if error {
                ui.colored_label(ui.visuals().error_fg_color, line);
            } else {
                ui.label(line);
            }
            // The refusal of an Apply repeats the checker's errors, which the list below shows as they are now.
            for diagnostic in outcome
                .diagnostics()
                .iter()
                .filter(|diagnostic| !live.iter().any(|known| known.message == diagnostic.message))
            {
                ui.colored_label(ui.visuals().error_fg_color, diagnostic_text(diagnostic));
            }
        }
        None => {
            ui.weak("Press Apply to check and install the model.");
        }
    }
    if let Some(status) = &graph.status {
        if status.warning {
            ui.colored_label(ui.visuals().warn_fg_color, &status.message);
        } else {
            ui.label(&status.message);
        }
    }
    let mut framing = None;
    for diagnostic in &live {
        let color = if diagnostic.severity == henad_core::diagnostic::DiagnosticSeverity::Error {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().warn_fg_color
        };
        let scope = graph.diagnostic_scope(diagnostic);
        let body = match &scope {
            Some(henad_graph::edit::Scope::Function(id)) => Some(format!("function {id}")),
            _ => diagnostic.pass.clone(),
        };
        let place = match (&body, &diagnostic.node) {
            (Some(body), Some(node)) => format!("{body}, {node}: "),
            (Some(body), None) => format!("{body}: "),
            _ => String::new(),
        };
        let text = RichText::new(format!("{place}{}", diagnostic.message)).color(color);
        let target = scope.zip(
            diagnostic
                .node
                .as_ref()
                .and_then(|node| node.parse::<henad_graph::ids::NodeId>().ok()),
        );
        let label = Label::new(text).selectable(false);
        match target {
            Some((scope, node)) => {
                let response = ui.add(label.sense(Sense::click())).on_hover_text("Show node on canvas");
                if response.clicked() {
                    framing = Some(graph::GraphRequest::Frame { scope, node });
                }
            }
            None => {
                ui.add(label);
            }
        }
    }
    if let Some(framing) = framing {
        graph.request(framing);
    }
}

/// Draws the state of the preview, the outcome of the last Apply of a sheet, every problem of the sheet's check that is
/// not drawn on the card, and the problems that stop it lowering. A problem about a line the card shows sits under
/// that line on the card.
fn sheet_outcome_ui(ui: &mut egui::Ui, panel: &EditorPanel) {
    let Some(sheet) = panel.model.sheet() else {
        return;
    };
    if sheet.shows_nothing() {
        ui.weak(WORLD_EMPTY);
    } else {
        preview_ui(ui, &panel.preview.state, false);
    }
    if let Some(notice) = sheet.notice() {
        ui.label(format!("{MDI_INFORMATION} {notice}"));
    }
    let report = &sheet.report().diagnostics;
    let mut known: Vec<&CodeDiagnostic> = report.iter().map(|diagnostic| &diagnostic.diagnostic).collect();
    // A problem of a line the card shows is drawn under the line, and the list keeps the others.
    // The Cells section says how to start a sheet with no property.
    let mut live: Vec<&CodeDiagnostic> = report
        .iter()
        .filter(|diagnostic| diagnostic.diagnostic.message != SHEET_EMPTY)
        .filter(|diagnostic| !sheet::card_pane::draws_diagnostic(sheet.document(), diagnostic))
        .map(|diagnostic| &diagnostic.diagnostic)
        .collect();
    // The check's errors are among the problems that stop a sheet lowering. The lowering's own problems follow them.
    for problem in sheet.lowering_problems() {
        if !known.iter().any(|known| known.message == problem.message) {
            known.push(problem);
            live.push(problem);
        }
    }
    match &panel.outcome {
        Some(outcome) => {
            let (line, error) = outcome.summary();
            if error {
                ui.colored_label(ui.visuals().error_fg_color, line);
            } else {
                ui.label(line);
            }
            for diagnostic in outcome
                .diagnostics()
                .iter()
                .filter(|diagnostic| !known.iter().any(|known| known.message == diagnostic.message))
            {
                ui.colored_label(ui.visuals().error_fg_color, &diagnostic.message);
            }
        }
        None => {
            ui.weak("Press Apply to check and install the model.");
        }
    }
    // The message alone. The line and column of a problem point into the sheet's text, which sheet mode never shows.
    for diagnostic in live {
        let color = if diagnostic.severity == henad_core::diagnostic::DiagnosticSeverity::Error {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().warn_fg_color
        };
        ui.colored_label(color, &diagnostic.message);
    }
}

/// Draws the state of the preview: its size while it runs, and the problems that stopped it. `plays` is whether the
/// preview plays once built, as a graph's does, rather than waiting at tick 0 for Play as a sheet's does.
///
/// A sheet's problems are listed without their line and column, which point into the sheet's text.
fn preview_ui(ui: &mut egui::Ui, state: &PreviewState, plays: bool) {
    let sheet = !plays;
    let (line, problems) = match state {
        PreviewState::Idle => return,
        PreviewState::Waiting => {
            let waits = if sheet { SHEET_PREVIEW_WAITS } else { PREVIEW_WAITS };
            ui.label(format!("{MDI_INFORMATION} {waits}"));
            return;
        }
        PreviewState::Running(run) => {
            ui.label(format!("{MDI_PLAY_OUTLINE} {}", preview_running_text(*run, plays)));
            return;
        }
        PreviewState::Refused(problems) => ("Preview not built. Fix the problems below.", problems.as_slice()),
        PreviewState::Faulted(problem) => ("Preview stopped. Fix the problem below.", std::slice::from_ref(problem)),
    };
    ui.colored_label(ui.visuals().error_fg_color, line);
    for problem in problems {
        let text = if sheet {
            problem.message.clone()
        } else {
            diagnostic_text(problem)
        };
        ui.colored_label(ui.visuals().error_fg_color, text);
    }
}

/// Returns the line of a preview that runs or waits at the size of `run`. `plays` is whether the preview plays once
/// built.
///
/// A graph's preview reads "Preview runs at 256 × 256 cells from tick 0. Press Apply to run the model at its own
/// size." A sheet's preview waits at tick 0, and its line names Play.
pub fn preview_running_text(run: PreviewRun, plays: bool) -> String {
    let size = run.size_text();
    if plays {
        format!("Preview runs at {size} from tick 0. Press Apply to run the model at its own size.")
    } else {
        format!("Preview at {size} starts at tick 0. Press Play to run it, or Apply to run the model at its own size.")
    }
}

/// Draws the outcome of the last Apply: its line, then one label per problem. A press on a placed problem sets
/// `request` to move the cursor there.
fn outcome_ui(ui: &mut egui::Ui, panel: &EditorPanel, request: &mut Option<EditorRequest>) {
    let Some(outcome) = &panel.outcome else {
        if panel.model.is_empty() {
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
        EditorRequest::NewModel => sheet::new_model::open_new_model(app),
        EditorRequest::NewGridRule => new_grid_rule(app),
        EditorRequest::NewGraphModel(file_name) => new_graph_model(app, file_name),
        EditorRequest::OpenModel => open_model(app),
        EditorRequest::PickImport => app.open_file(OpenTarget::Function),
        EditorRequest::SaveModel => save_model(app),
        EditorRequest::Apply => apply(app),
        EditorRequest::Convert => expert::open_convert(&mut app.editor),
        EditorRequest::ConfirmConvert => expert::convert_sheet(app),
        EditorRequest::OpenOriginal => expert::open_original_of(&mut app.editor),
        EditorRequest::OpenCopy => expert::swap_counterpart(&mut app.editor),
        EditorRequest::ShowChanges => app.editor.show_changes = !app.editor.show_changes,
        EditorRequest::CompareRuns => compare::start_comparison(app),
        EditorRequest::CloseComparison => app.editor.comparison = None,
        EditorRequest::MoveCursor { line, column } => move_cursor_to(ctx, &mut app.editor, line, column),
    }
}

/// Moves the cursor of the code field to `column` on `line` of the text, both counted from 1, focuses the field, and
/// scrolls the place into view on the next frame.
///
/// A place past the end of the text, which an edit since the last Apply can leave, moves the cursor to the nearest
/// character.
pub fn move_cursor_to(ctx: &egui::Context, panel: &mut EditorPanel, line: u32, column: u32) {
    let index = code_view::char_index_at(panel.model.rule_text().unwrap_or_default(), line, column);
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

/// Replaces the Editor's model with the starter whose file is named `file_name`, and brings the Editor to the front.
pub fn new_graph_model(app: &mut AppState, file_name: &str) {
    let Some(file) = henad_graph::starters::starter(file_name).and_then(|starter| GraphFile::parse(starter.text).ok())
    else {
        app.editor.file_status = Some(format!("Starter {file_name} does not read"));
        return;
    };
    app.editor.load_graph(&file, None);
    app.editor.file_status = None;
    app.focus_request = Some(Tab::Editor);
}

/// Model the Editor can open from a model's recorded code.
#[derive(Debug)]
pub enum EditableModel {
    /// Sheet read from the author text of a model built from a sheet.
    Sheet(Box<SheetDocument>),
    /// Graph model read from its code.
    Graph(Box<GraphFile>),
    /// Model file text of a grid rule.
    Rule(String),
}

/// Returns the model the Editor opens from `author`, a model's author text, and `code`, its code: the sheet that
/// `author` holds, then the graph or the grid rule that `code` holds, or `None` for neither.
///
/// An author text or a code that does not read counts as missing.
pub fn editable_model(author: Option<&ModelCode>, code: Option<&ModelCode>) -> Option<EditableModel> {
    let sheet = author
        .filter(|author| author.format() == SHEET_CODE_FORMAT)
        .and_then(|author| ParsedSheet::parse(author.text()).ok());
    if let Some(parsed) = sheet {
        return Some(EditableModel::Sheet(Box::new(parsed.into_document())));
    }
    let code = code?;
    match code.format() {
        henad_graph::CODE_FORMAT => GraphFile::from_code(code)
            .ok()
            .map(|file| EditableModel::Graph(Box::new(file))),
        GridRule::CODE_FORMAT => Some(EditableModel::Rule(code.text().to_owned())),
        _ => None,
    }
}

/// Returns whether the Editor can open the code of `entry`: a grid rule, a graph model or a sheet applied at run time.
pub fn can_edit(entry: &ModelEntry) -> bool {
    editable_model(entry.source().author(), entry.source().code()).is_some()
}

/// Replaces the Editor's model with the selected model, its sheet when it was built from one, and brings the Editor
/// to the front.
///
/// The model stays, edits since included, when the selected model's code is the code the Editor applied last.
pub fn edit_model(app: &mut AppState) {
    let Some(entry) = app.selected_entry() else {
        return;
    };
    let applied = app.editor.is_applied(entry.source());
    let Some(model) = editable_model(entry.source().author(), entry.source().code()) else {
        return;
    };
    if !applied {
        app.editor.load_model(model);
    }
    app.focus_request = Some(Tab::Editor);
}

/// Replaces the Editor's model with `model`, from no file, and brings the Editor to the front.
pub fn open_editable(app: &mut AppState, model: EditableModel) {
    app.editor.load_model(model);
    app.focus_request = Some(Tab::Editor);
}

/// Opens a dialog that picks a model file for the Editor.
pub fn open_model(app: &AppState) {
    app.open_file(OpenTarget::Model);
}

/// Opens a dialog that saves the Editor's model: a grid rule's text as written, comments included, or a graph model's
/// or a sheet's canonical text.
pub fn save_model(app: &mut AppState) {
    if app.editor.model.is_empty() {
        return;
    }
    let name = save_name(&app.editor);
    let bytes = app.editor.model.file_text().into_bytes();
    app.save_as(SaveTarget::Model, &name, bytes);
}

/// Returns the file name a save of the Editor's text suggests: the name it was opened or saved under, then the model's
/// id, then [`FALLBACK_FILE_NAME`].
///
/// A sheet suggests its id even while it holds errors.
pub fn save_name(panel: &EditorPanel) -> String {
    if let Some(name) = &panel.file_name {
        return name.clone();
    }
    if let Some(sheet) = panel.model.sheet() {
        return format!("{}.toml", sheet.document().sheet.id);
    }
    ModelFile::parse(&panel.model.file_text())
        .map_or_else(|_| FALLBACK_FILE_NAME.to_owned(), |file| format!("{}.toml", file.id()))
}

/// Handles the result of an Open model dialog. The picked file replaces the Editor's model, and is applied only on
/// Apply.
///
/// The `kind` in the file's header decides where it opens, as [`open_text`] says.
pub fn receive_open(app: &mut AppState, result: OpenResult) {
    let file = match result {
        OpenResult::Files(files) => files.into_iter().next(),
        OpenResult::Failed(message) => {
            app.editor.file_status = Some(format!("Open failed: {message}"));
            return;
        }
        OpenResult::Folder(_) | OpenResult::Canceled => return,
    };
    let Some(DialogFile { name, bytes, path }) = file else {
        return;
    };
    match String::from_utf8(bytes) {
        Ok(text) => {
            app.editor.file_status = Some(format!("Opened {name}"));
            open_text(&mut app.editor, text, Some(name));
            app.editor.file_path = path;
            expert::find_original(&mut app.editor);
            app.focus_request = Some(Tab::Editor);
        }
        Err(_) => app.editor.file_status = Some(format!("Open failed: {name} is not UTF-8 text")),
    }
}

/// Replaces the Editor's model with the model file `text`, from the file `file_name` or from no file, by the `kind` in
/// its header.
///
/// A sheet that the sheet reader reads opens as its sheet and cards, checker errors included. A graph that reads opens
/// on the canvas. Any other text opens in the code view, where Apply names its problems at their lines. A sheet that
/// does not read opens there too, with the reader's problem listed under it.
pub fn open_text(panel: &mut EditorPanel, text: String, file_name: Option<String>) {
    match read_header(&text).as_deref() {
        Some(KIND_SHEET) => match ParsedSheet::parse(&text) {
            Ok(parsed) => panel.load_sheet(sheet::SheetEditor::new(parsed.into_document()), file_name),
            Err(error) => {
                panel.load(text, file_name);
                panel.outcome = Some(ApplyOutcome::Unread(error.diagnostics().to_vec()));
            }
        },
        Some(KIND_GRAPH) => match GraphFile::parse(&text) {
            Ok(file) => panel.load_graph(&file, file_name),
            Err(_) => panel.load(text, file_name),
        },
        _ => panel.load(text, file_name),
    }
}

/// Takes the picked model file as the file the Editor's graph model imports functions from.
pub fn receive_import(app: &mut AppState, result: OpenResult) {
    let file = match result {
        OpenResult::Files(files) => files.into_iter().next(),
        OpenResult::Failed(message) => {
            app.editor.file_status = Some(format!("Open failed: {message}"));
            return;
        }
        OpenResult::Folder(_) | OpenResult::Canceled => return,
    };
    let (Some(DialogFile { name, bytes, .. }), Some(graph)) = (file, app.editor.model.graph_mut()) else {
        return;
    };
    match String::from_utf8(bytes) {
        Ok(text) => graph.set_import_source(name, text),
        Err(_) => app.editor.file_status = Some(format!("Open failed: {name} is not UTF-8 text")),
    }
}

/// Reads the Editor's model file, compiles its model, and installs and runs the model.
///
/// A text that is refused leaves the set and the running model alone, and the Editor lists the problems. Otherwise
/// the model replaces the set's entry with its id, or joins the set, and becomes the selection:
///
/// - a run of the model past tick 0 keeps going and waits on the Rebuild model dialog, as
///   [`AppState::install_models`] leaves it, and a run of the model at tick 0 is rebuilt at once. The dialog opens
///   again for a run whose code differs from the set's entry, as after a canceled dialog.
/// - the Editor's preview is replaced by a build of the model at its own size, at any tick.
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
    let entry = match ModelFile::parse(&app.editor.model.file_text()).and_then(|file| file.entry()) {
        Ok(entry) => entry,
        Err(error) => {
            app.editor.outcome = Some(ApplyOutcome::Refused(error.diagnostics().to_vec()));
            return;
        }
    };
    let id = entry.id().to_owned();
    let source = entry.source().clone();
    // A preview runs the source at another size, and Apply replaces it.
    let runs_source =
        |app: &AppState| app.preview().is_none() && app.loaded_entry().is_some_and(|loaded| loaded.source() == &source);
    let unchanged = runs_source(app) && app.models.get(&id).is_some_and(|held| held.source() == &source);

    // A note edit of a sheet leaves the source equal. The set's entry takes the new author text without a rebuild,
    // so a sweep started next records the sheet as it now reads.
    let author_changed = app
        .models
        .get(&id)
        .is_some_and(|held| !held.source().same_author(&source));

    app.editor.outcome = None;
    app.editor.applied = Some(source.clone());
    if unchanged && author_changed {
        let mut models = ModelSet::new(henad_core::build_info!());
        if models.insert(entry.clone()).is_ok() {
            app.install_models(&models);
        }
    }
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
    if !runs_source(app) && app.pending_rebuild.is_none() && (app.run_at_start() || app.preview().is_some()) {
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

    use henad_graph::GraphFile;

    use super::{ApplyOutcome, EditorModel, diagnostic_text, graph, sheet};

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
        assert!(ApplyOutcome::Unread(Vec::new()).summary().1);
        assert!(ApplyOutcome::Faulted(Vec::new()).summary().1);
    }

    #[test]
    fn each_editor_model_answers_by_its_kind() {
        let empty = EditorModel::default();
        assert!(empty.is_empty() && empty.file_text().is_empty());
        assert!(!empty.holds_preview_model() && empty.preview_source().is_none());
        assert!(empty.rule_text().is_none() && empty.graph().is_none());

        assert!(
            EditorModel::Rule(String::new()).is_empty(),
            "an empty text holds no model"
        );
        let rule = EditorModel::Rule("format = \"henad-model\"\n".to_owned());
        assert!(!rule.is_empty());
        assert_eq!(rule.file_text(), "format = \"henad-model\"\n");
        assert_eq!(rule.rule_text(), Some("format = \"henad-model\"\n"));
        assert!(!rule.holds_preview_model() && rule.preview_source().is_none());

        let starter = henad_graph::starters::starters().first().expect("a grid starter");
        let file = GraphFile::parse(starter.text).expect("the starter reads");
        let mut model = EditorModel::Graph(Box::new(graph::GraphEditor::new(&file)));
        assert!(!model.is_empty() && model.holds_preview_model());
        assert!(model.rule_text().is_none());
        assert_eq!(
            GraphFile::parse(&model.file_text()).map(|read| read.code()).ok(),
            Some(file.code())
        );
        let source = model.preview_source().expect("a graph previews");
        assert!(!source.has_errors);
        assert!(model.preview_program().is_some_and(|program| program.is_ok()));

        let starter = henad_graph::sheet::starters().first().expect("a sheet starter");
        let parsed = henad_graph::sheet::ParsedSheet::parse(starter.text).expect("the starter reads");
        let mut model = EditorModel::Sheet(Box::new(sheet::SheetEditor::new(parsed.into_document())));
        assert!(!model.is_empty() && model.holds_preview_model());
        assert!(model.rule_text().is_none() && model.graph().is_none() && model.sheet().is_some());
        assert_eq!(model.file_text(), starter.text, "the starter is canonical");
        let source = model.preview_source().expect("a sheet previews");
        assert!(!source.has_errors);
        assert!(model.preview_program().is_some_and(|program| program.is_ok()));
    }
}
