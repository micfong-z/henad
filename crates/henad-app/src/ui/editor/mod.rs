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
mod history;
pub mod preview;
pub mod program;
pub mod sheet;

use egui::{
    Align, Button, CentralPanel, Frame, Key, KeyboardShortcut, Layout, Margin, Modal, Modifiers, Panel, RichText,
    ScrollArea, vec2,
};
use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::fault::{Fault, FaultKind};
use henad_compute::gpu::runtime_grid::GridRule;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::explore::fingerprint::fnv1a64;
use henad_core::provenance::{CodeOrigin, ModelCode, ModelSource};
use henad_explore::model_file::ModelFile;
use henad_graph::model_text::read_header;
use henad_graph::sheet::document::SheetDocument;
use henad_graph::sheet::edit::{IdSpace, mint_id};
use henad_graph::sheet::sentence::{
    CANCEL, DISCARD_CHANGES, PREVIEW_WAITS as SHEET_PREVIEW_WAITS, REPLACE, REPLACE_APPLIED_HEADING, SAVE_MODEL,
    SHEET_EMPTY, STALE_WORLD_NOTICE, UNSAVED_HEADING, WORLD_EMPTY, replace_applied, unsaved_changes,
};
use henad_graph::sheet::{KIND_SHEET, ParsedSheet, SHEET_CODE_FORMAT};
use henad_graph::starters::{Starter, agent_starters, starters};
use henad_graph::{GraphFile, KIND_GRAPH};

use crate::icons::material_design_icons::{
    MDI_CHECK, MDI_FILE_DOCUMENT_PLUS_OUTLINE, MDI_FILE_PLUS_OUTLINE, MDI_FOLDER_OPEN_OUTLINE, MDI_GRAPH_OUTLINE,
    MDI_TRAY_ARROW_DOWN,
};
use crate::state::{AppState, PreviewRun};
use crate::ui::dock::Tab;
use crate::ui::files::{DialogFile, OpenResult, OpenTarget, SaveTarget};
use crate::ui::params::sheet_number;
use crate::ui::sweep::layout::{add_button, filled_button};
use preview::{PREVIEW_WAITS, PreviewState};
use sheet::widgets::{DialogButton, DialogButtonKind, FocusTooltip as _, Notice, NoticeKind};

/// Text the Editor shows on an adapter that cannot run compute shaders.
pub const NO_COMPUTE: &str =
    "Grid rules and graph models are unavailable on this device. They need a GPU with compute support.";

/// Reason Apply and Save model are disabled while the Editor holds no model.
pub const EMPTY_EDITOR: &str =
    "Editor is empty. Press New model, New node graph, New grid rule or Open model to start.";

/// Label of the button that opens the New model dialog.
pub const NEW_MODEL_BUTTON: &str = henad_graph::sheet::sentence::NEW_MODEL;

/// Label of the menu that opens a graph starter on the canvas.
pub const NEW_NODE_GRAPH: &str = "New node graph";

/// Name of a saved model file whose text does not read as a model file.
const FALLBACK_FILE_NAME: &str = "model.toml";

/// Line the Editor shows while the model has not been applied since it was opened, made or last edited.
const PRESS_APPLY: &str = "Press Apply to check and install the model.";

/// Notice while the preview of a graph runs the graph as it was before an edit that stops it building.
pub const STALE_PREVIEW: &str = "Preview shows the model before the last change. Fix the problems below to update it.";

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

    /// Returns a mark of the model's document that changes with every edit: a hash of a grid rule's or a sheet's
    /// text, or the revision of a graph, which every edit, undo and redo moves on.
    ///
    /// Note that a graph edited back to an earlier document keeps a new mark, where a text edited back takes its old
    /// mark again.
    pub fn edit_mark(&self) -> u64 {
        match self {
            Self::Empty => fnv1a64(&[]),
            Self::Rule(text) => fnv1a64(text.as_bytes()),
            Self::Graph(graph) => graph.revision(),
            Self::Sheet(sheet) => fnv1a64(sheet.text().as_bytes()),
        }
    }

    /// Returns whether the model holds edits since its edit mark was `saved_mark`. An empty model holds none.
    pub fn is_edited_since(&self, saved_mark: u64) -> bool {
        !self.is_empty() && self.edit_mark() != saved_mark
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
    pub fn preview_program(&mut self) -> Option<program::CompiledProgram> {
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
    pub file_status: Option<FileStatus>,
    /// Source of the entry the last Apply installed. A fault of a run built from it belongs to the edit until the run
    /// passes tick 0.
    applied: Option<ModelSource>,
    /// Edit mark of the model at the last Apply, [`EditorModel::edit_mark`]. An edit since then drops an outcome that
    /// reads as applied.
    applied_mark: Option<u64>,
    /// Edit mark of the model when it was opened, made or last saved. Apply leaves it alone.
    saved_mark: u64,
    /// Saves of the model shown, each from its start until it reports.
    saving: Vec<PendingSave>,
    /// Number of saves started, of the model shown and of the model held beside it. The next save takes the number
    /// after it.
    saves_started: u64,
    /// Action that replaces the models, waiting on the unsaved changes dialog, `None` while the dialog is closed.
    pub ask: Option<Replacement>,
    /// Model that Apply replaces, a model applied from another sheet under the same id, while the dialog that asks
    /// before Apply replaces it is open, `None` while the dialog is closed.
    pub replace_question: Option<ReplaceQuestion>,
    /// Action that replaces the models, waiting for the saves that the dialog's Save model started.
    after_save: Option<AfterSave>,
    /// Edit marks of the model shown and of the model held beside it when the Open model dialog started, `None` while
    /// no such dialog is open.
    open_marks: Option<EditMarks>,
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
    pub beside: Option<Box<expert::Beside>>,
    /// Banner of the copy in the Editor, with the load, code revision and hash of the original's text it was worked
    /// out for.
    banner: Option<(BannerKey, Option<expert::CopyBanner>)>,
    /// Whether the side panel lists the changes of the copy since the conversion and the canvas marks them.
    pub show_changes: bool,
    /// Runs of the copy and its original that Compare runs started, `None` before Compare runs and after Close
    /// comparison.
    pub comparison: Option<Box<compare::Comparison>>,
}

/// Status line of the Editor's last open, save or other action on a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    /// Text of the line, such as "Opened sir.toml".
    pub text: String,
    /// Whether the line reports a failure, such as a save that failed.
    pub failed: bool,
}

impl FileStatus {
    /// Returns a line that reports what was done, such as "Opened sir.toml".
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failed: false,
        }
    }

    /// Returns a line that reports a failure, such as "Open failed: ...".
    pub fn failure(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            failed: true,
        }
    }

    /// Returns the line that reports the file `name` as no text, "Open failed: ... is not UTF-8 text".
    pub fn not_text(name: &str) -> Self {
        Self::failure(format!("Open failed: {name} is not UTF-8 text"))
    }

    /// Text of the line.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Load, code revision and hash of the original's text that a copy's banner and comparison were worked out for.
pub type BannerKey = (u64, u64, Option<u64>);

/// Edit marks of the model the Editor shows and of the model it holds beside it, `None` for no model beside it.
type EditMarks = (u64, Option<u64>);

/// Model applied from another sheet that Apply would replace, which the replace dialog names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceQuestion {
    /// Name of the applied model.
    pub name: String,
    /// Id the applied model and the Editor's model share.
    pub id: String,
}

/// A save of a model the Editor holds, from its start until it reports.
///
/// The save stays with its model when the model and the counterpart change places, and goes with a model that is
/// replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingSave {
    /// Target the save reports to, with the number of the save: the model shown or the model held beside it, when the
    /// save started.
    pub target: SaveTarget,
    /// Edit mark of the model when the save started.
    pub mark: u64,
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
    /// The opened file does not read as the kind its header names, with every problem found. Nothing was applied.
    Unread(Vec<CodeDiagnostic>),
    /// The model was installed, and the device or the engine refused it when it was built.
    Faulted(Vec<CodeDiagnostic>),
    /// The model was installed and not built, since Build or Rebuild is disabled for it, with the reason they give, as
    /// for a size the device cannot hold at the values of the next build or a Seed field that does not parse.
    BuildRefused(Vec<CodeDiagnostic>),
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
            // A refused build reads as a failed one, and names its reason below.
            Self::Faulted(_) | Self::BuildRefused(_) => ("Model build failed. Fix the problem below.", true),
        }
    }

    /// Problems the outcome lists, empty for an outcome without any.
    pub fn diagnostics(&self) -> &[CodeDiagnostic] {
        match self {
            Self::Refused(diagnostics)
            | Self::Unread(diagnostics)
            | Self::Faulted(diagnostics)
            | Self::BuildRefused(diagnostics) => diagnostics,
            Self::Applied | Self::AwaitingRebuild | Self::Installed | Self::Unchanged => &[],
        }
    }
}

impl EditorPanel {
    /// Returns whether `source` is the source of the entry the last Apply installed.
    pub fn is_applied(&self, source: &ModelSource) -> bool {
        self.applied.as_ref() == Some(source)
    }

    /// Returns whether the Editor holds a model that no Apply has installed as it now reads: one not applied since it
    /// was opened or made, one edited since its last Apply, or one the last Apply refused.
    ///
    /// The Editor then reads "Press Apply to check and install the model." or lists the problems that refused it.
    pub fn holds_unapplied(&self) -> bool {
        !self.model.is_empty()
            && (self.applied_mark != Some(self.model.edit_mark())
                || matches!(self.outcome, Some(ApplyOutcome::Refused(_) | ApplyOutcome::Unread(_))))
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
        self.replace(EditorModel::Rule(text), file_name);
        self.replace_view();
    }

    /// Replaces the Editor's model with the graph `file`, from the file `file_name` or from no file.
    ///
    /// The outcome of the last Apply and the state of the preview are cleared, as [`Self::load`] clears them for a
    /// text.
    pub fn load_graph(&mut self, file: &GraphFile, file_name: Option<String>) {
        self.replace(EditorModel::Graph(Box::new(graph::GraphEditor::new(file))), file_name);
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
        self.replace(EditorModel::Sheet(Box::new(sheet)), file_name);
    }

    /// Replaces the Editor's model with `model`, from the file `file_name` or from no file, as saved, and clears what
    /// belonged to the model it replaces.
    ///
    /// A replacement that waited for the saves of the model it replaces goes with it. Otherwise the report of such a
    /// save would run that replacement over the new model.
    fn replace(&mut self, model: EditorModel, file_name: Option<String>) {
        self.clear_expert();
        self.model = model;
        self.file_name = file_name;
        self.clear_outcome();
        self.saved_mark = self.model.edit_mark();
        self.saving.clear();
        self.after_save = None;
    }

    /// Clears the outcome of the last Apply, the question asked before it replaced a model, the line to scroll to and
    /// the state of the preview.
    fn clear_outcome(&mut self) {
        self.outcome = None;
        self.replace_question = None;
        self.applied = None;
        self.applied_mark = None;
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
        self.clear_outcome();
        self.banner = None;
        self.convert = None;
        self.show_changes = false;
    }

    /// Returns the edit marks of the model shown and of the model held beside it.
    fn edit_marks(&self) -> EditMarks {
        let beside = self
            .counterpart
            .as_ref()
            .map(|counterpart| counterpart.model.edit_mark());
        (self.model.edit_mark(), beside)
    }

    /// Records the edit marks of the models as the Open model dialog starts. A model edited while the dialog is open
    /// then asks before the file picked in it replaces the model, as [`replace_model`] does.
    pub fn note_open_started(&mut self) {
        self.open_marks = Some(self.edit_marks());
    }

    /// Returns whether the model shown holds edits since it was opened, made or last saved. An empty model holds none.
    pub fn is_unsaved(&self) -> bool {
        self.model.is_edited_since(self.saved_mark)
    }

    /// Returns the names of the models that hold unsaved edits: the model shown, then the model held beside it.
    pub fn unsaved_names(&self) -> Vec<String> {
        self.names_of(&self.unsaved_models(None))
    }

    /// Returns the models with unsaved edits that `replacement` drops, or that any replacement drops for `None`: the
    /// model shown, then the model held beside it.
    ///
    /// A conversion keeps the sheet shown, and drops the model held beside it alone.
    fn unsaved_models(&self, replacement: Option<&Replacement>) -> Vec<HeldModel> {
        let mut models = Vec::new();
        if self.is_unsaved() && !matches!(replacement, Some(Replacement::Convert)) {
            models.push(HeldModel::Shown);
        }
        if self
            .counterpart
            .as_ref()
            .is_some_and(|counterpart| counterpart.is_unsaved())
        {
            models.push(HeldModel::Beside);
        }
        models
    }

    /// Returns the names of `models`, in their order.
    fn names_of(&self, models: &[HeldModel]) -> Vec<String> {
        models
            .iter()
            .filter_map(|held| match held {
                HeldModel::Shown => Some(model_name(&self.model, self.file_name.as_deref())),
                HeldModel::Beside => self
                    .counterpart
                    .as_ref()
                    .map(|counterpart| model_name(&counterpart.model, counterpart.file_name.as_deref())),
            })
            .collect()
    }

    /// Returns the number of the next save, one past the number of the last save started.
    fn next_save(&mut self) -> u64 {
        self.saves_started += 1;
        self.saves_started
    }

    /// Takes the outcome of the save `target`, with the file name it saved under or `None` for a save that saved
    /// nothing.
    ///
    /// The model the save started for, shown or held beside, takes the file name and reads as saved at the edit mark it
    /// had when the save started. A save of a model the Editor no longer holds names no model. Each save carries its
    /// own number, so two saves pending under one target, as of a model and of the model that replaced it, or two
    /// saves of one model, each name and mark the model it saved, in the order they report.
    pub fn save_finished(&mut self, target: SaveTarget, saved: Option<String>) {
        let take = |saving: &mut Vec<PendingSave>| {
            let index = saving.iter().position(|save| save.target == target)?;
            Some(saving.remove(index))
        };
        let (file_name, file_path, saved_mark, save) = if let Some(save) = take(&mut self.saving) {
            (&mut self.file_name, &mut self.file_path, &mut self.saved_mark, save)
        } else if let Some(counterpart) = self.counterpart.as_mut()
            && let Some(save) = take(&mut counterpart.saving)
        {
            (
                &mut counterpart.file_name,
                &mut counterpart.file_path,
                &mut counterpart.saved_mark,
                save,
            )
        } else {
            return;
        };
        if let Some(name) = saved {
            *file_name = Some(name);
            // The file was saved where the dialog chose, which can be another folder.
            *file_path = None;
            *saved_mark = save.mark;
        }
    }

    /// Clears the code field's undo history and returns the view to the top left on the field's next frame, after
    /// the text of another file replaced the text.
    fn replace_view(&mut self) {
        self.reset_history = true;
        self.reset_scroll = true;
    }

    /// Brings an outcome that waited up to date. `loaded` is the source of the loaded model, `next_build` the source of
    /// the model Playback's Build builds now, `rebuild_pending` whether the Rebuild model dialog is open, and `refusal`
    /// the reason Rebuild is disabled while the dialog is open, or else the reason Build is disabled, `None` while the
    /// button is enabled.
    ///
    /// An outcome that waited for Rebuild or Build, that named the reason they were disabled, or that reported a fault
    /// of the build, reads as applied once the loaded model runs the applied code, from the start of its build. A
    /// fault of a build of the applied code leaves nothing loaded. One that waited for Rebuild waits for Build once
    /// the dialog closes without a rebuild. An outcome that read as applied waits for Build once nothing is loaded, as
    /// after Cancel or Offload.
    ///
    /// An outcome that named the reason Build or Rebuild was disabled names the reason they give now, and waits for
    /// the button once `refusal` is `None`. It is dropped once Build builds another model.
    ///
    /// An outcome that waits for Build is dropped once Build builds another model, as while it replays the preview of
    /// the Editor's sheet. `next_build` is `None` then. An outcome that reads as applied or unchanged is dropped once
    /// the model is edited since the Apply.
    pub fn settle_outcome(
        &mut self,
        loaded: Option<&ModelSource>,
        next_build: Option<&ModelSource>,
        rebuild_pending: bool,
        refusal: Option<&str>,
    ) {
        let edited = self.applied_mark.is_some_and(|mark| mark != self.model.edit_mark());
        let applied = matches!(
            self.outcome,
            Some(
                ApplyOutcome::Applied
                    | ApplyOutcome::AwaitingRebuild
                    | ApplyOutcome::Installed
                    | ApplyOutcome::Unchanged
            )
        );
        if edited && applied {
            self.outcome = None;
            return;
        }
        let waiting = matches!(
            self.outcome,
            Some(
                ApplyOutcome::AwaitingRebuild
                    | ApplyOutcome::Installed
                    | ApplyOutcome::Faulted(_)
                    | ApplyOutcome::BuildRefused(_)
            )
        );
        let builds_other = next_build.is_some() && next_build != self.applied.as_ref();
        if waiting && loaded.is_some() && loaded == self.applied.as_ref() {
            self.outcome = Some(ApplyOutcome::Applied);
        } else if let Some(ApplyOutcome::BuildRefused(problems)) = &mut self.outcome {
            match refusal {
                _ if builds_other && !rebuild_pending => self.outcome = None,
                None if rebuild_pending => self.outcome = Some(ApplyOutcome::AwaitingRebuild),
                None => self.outcome = Some(ApplyOutcome::Installed),
                Some(reason) => {
                    if !matches!(problems.as_slice(), [problem] if problem.message == reason) {
                        *problems = vec![CodeDiagnostic::error(reason)];
                    }
                }
            }
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

/// Action that replaces the Editor's models. It waits on the unsaved changes dialog while a model the Editor holds has
/// unsaved edits.
#[derive(Debug)]
pub enum Replacement {
    /// Opens the New model dialog.
    NewModel,
    /// Opens the Life starter as a grid rule.
    NewGridRule,
    /// Opens the graph starter with this file name.
    NewGraphModel(&'static str),
    /// Opens a dialog that picks a model file.
    OpenModel,
    /// Opens a model read from a model's recorded code, as Open in Editor and Edit model do.
    Open(EditableModel),
    /// Converts the sheet shown to a new node graph copy, after Convert in the conversion dialog. The copy held beside
    /// the sheet is dropped, and the sheet is kept.
    Convert,
    /// Opens the file picked in the Open model dialog, after a model was edited while the dialog was open.
    OpenFile(Box<OpenedFile>),
    /// Closes the app's window, after a close request of the window.
    #[cfg(not(target_arch = "wasm32"))]
    CloseWindow,
}

/// A model file picked in the Open model dialog, read as text.
#[derive(Debug)]
pub struct OpenedFile {
    /// Name of the file.
    pub name: String,
    /// Text of the file.
    pub text: String,
    /// Path of the file on native, `None` in a browser.
    pub path: Option<std::path::PathBuf>,
}

/// Replacement waiting for the saves that the unsaved changes dialog's Save model started.
#[derive(Debug)]
struct AfterSave {
    replacement: Replacement,
    /// Models whose saves are yet to report, the one whose save runs now first.
    remaining: Vec<HeldModel>,
    /// Save that runs now, `None` before the first starts.
    running: Option<SaveTarget>,
}

/// Model the Editor holds: the one it shows, or the one it holds beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeldModel {
    /// The model shown.
    Shown,
    /// The model held beside the one shown, its counterpart.
    Beside,
}

/// Choice made in the unsaved changes dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// Width in points of the unsaved changes dialog.
const UNSAVED_DIALOG_WIDTH: f32 = 420.0;

/// Width in points of the dialog that asks before Apply replaces a model applied from another sheet.
const REPLACE_DIALOG_WIDTH: f32 = 420.0;

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

/// Maps a request of the sheet editor to the Editor's request of the same name.
impl From<sheet::SheetRequest> for EditorRequest {
    fn from(request: sheet::SheetRequest) -> Self {
        match request {
            sheet::SheetRequest::Convert => Self::Convert,
            sheet::SheetRequest::OpenCopy => Self::OpenCopy,
        }
    }
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

    let copy = expert::counterpart_copy(panel);
    let refusal = panel.model.sheet().and_then(expert::convert_refusal);
    let header = HeaderShown {
        compute,
        empty,
        past_start,
        refusal,
        copy: copy.as_deref(),
    };
    header_ui(ui, panel, &header, &mut request);
    status_area_ui(ui, panel, compute, past_start, &mut request);

    let changes = expert::shown_changes(panel);

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
                refusal,
                fires: fires.as_ref(),
                layout,
            };
            if let Some(sheet_request) = sheet::sheet_ui(ui, sheet, &shown) {
                request = Some(sheet_request.into());
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

/// Things the Editor's header shows besides the Editor's own state, for one frame.
#[derive(Debug, Clone, Copy)]
struct HeaderShown<'a> {
    /// Whether the device runs compute shaders.
    compute: bool,
    /// Whether the Editor holds no model.
    empty: bool,
    /// Whether the preview of the Editor's sheet has passed tick 0 or a button was pressed in it.
    past_start: bool,
    /// Reason the Editor's sheet does not convert, `None` for a sheet that converts or another model.
    refusal: Option<henad_graph::sheet::ConvertRefusal>,
    /// Name of the node graph copy converted from the Editor's sheet in this session, `None` for none.
    copy: Option<&'a str>,
}

/// Draws the header of the Editor tab: its buttons with Apply at the right end, the name row of the model, and the
/// banner of a copy converted from a sheet. A pressed button sets `request`.
fn header_ui(ui: &mut egui::Ui, panel: &mut EditorPanel, shown: &HeaderShown<'_>, request: &mut Option<EditorRequest>) {
    let compute = shown.compute;
    Panel::top("henad_editor_header")
        .resizable(false)
        .frame(Frame::NONE.inner_margin(Margin {
            left: 0,
            right: 0,
            top: 4,
            bottom: 6,
        }))
        .show(ui, |ui| {
            header_buttons(ui, compute, shown.empty, request);
            ui.add_space(4.0);
            name_row(ui, panel, shown, request);
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
    // Only a refused build reads the reason, which can cost a probe of the device's limits. While Build replays the
    // preview, Apply builds the applied model, and the reason stays the one Apply meets.
    let refusal = match app.editor.outcome {
        Some(ApplyOutcome::BuildRefused(_)) if rebuild_pending => app.rebuild_refusal(),
        Some(ApplyOutcome::BuildRefused(_)) => app.build_refusal(),
        _ => None,
    };
    app.editor.settle_outcome(
        loaded.as_ref(),
        next_build.as_ref(),
        rebuild_pending,
        refusal.as_deref(),
    );
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

/// Draws the name row of the header: the model's name in the body colour, the mark of unsaved changes and the file's
/// name in the weak colour. A sheet's row holds its rename button, Undo, Redo and the menu of its actions too. Nothing
/// shows while the Editor holds no model.
fn name_row(ui: &mut egui::Ui, panel: &mut EditorPanel, shown: &HeaderShown<'_>, request: &mut Option<EditorRequest>) {
    if panel.model.is_empty() {
        return;
    }
    let unsaved = panel.is_unsaved();
    let name = model_name(&panel.model, panel.file_name.as_deref());
    // A grid rule goes by its file's name, which the row then shows once.
    let file_name = panel.file_name.as_deref().filter(|file_name| *file_name != name);
    if let EditorModel::Sheet(sheet) = &mut panel.model {
        let row = sheet::NameRow {
            past_start: shown.past_start,
            refusal: shown.refusal,
            unsaved,
            file_name,
            copy: shown.copy,
        };
        if let Some(sheet_request) = sheet::name_row(ui, sheet, &row) {
            *request = Some(sheet_request.into());
        }
        return;
    }
    ui.horizontal(|ui| {
        // The name lines up with the words of a sheet's rename button.
        ui.add_space(ui.spacing().button_padding.x);
        ui.label(RichText::new(&name).color(ui.visuals().text_color()));
        if unsaved {
            sheet::widgets::unsaved_mark(ui);
        }
        if let Some(file_name) = file_name {
            ui.weak(file_name);
        }
    });
}

/// Draws the header buttons, each setting `request` when pressed.
///
/// The buttons are New model, New grid rule and New node graph, a hairline, Open model and Save model, then Apply, the
/// one primary button, at the right end. The first five wrap onto a second row when the tab is too narrow for them.
fn header_buttons(ui: &mut egui::Ui, compute: bool, empty: bool, request: &mut Option<EditorRequest>) {
    let apply_text = format!("{MDI_CHECK} Apply");
    let apply_width = button_width(ui, &apply_text);
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() - apply_width - ui.spacing().item_spacing.x).max(0.0);
        ui.allocate_ui_with_layout(
            vec2(width, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Min).with_main_wrap(true),
            |ui| file_buttons(ui, empty, request),
        );
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            apply_button(ui, compute, empty, apply_text, request);
        });
    });
}

/// Returns the width in points of a button showing `text`.
fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER);
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// Draws New model, New grid rule, New node graph, a hairline, Open model and Save model, each setting `request` when
/// pressed.
fn file_buttons(ui: &mut egui::Ui, empty: bool, request: &mut Option<EditorRequest>) {
    let model = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FILE_DOCUMENT_PLUS_OUTLINE} {NEW_MODEL_BUTTON}")),
        NEW_MODEL_BUTTON,
    )
    .on_hover_or_focus_text("Start new model");
    if model.clicked() {
        *request = Some(EditorRequest::NewModel);
    }
    let new = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FILE_PLUS_OUTLINE} New grid rule")),
        "New grid rule",
    )
    .on_hover_or_focus_text("Replace text with Game of Life rule");
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
    graph.on_hover_or_focus_text("Start node graph from example");
    sheet::widgets::vertical_hairline(ui, ui.spacing().interact_size.y);
    let open = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FOLDER_OPEN_OUTLINE} Open model")),
        "Open model",
    )
    .on_hover_or_focus_text("Replace model with TOML model file");
    if open.clicked() {
        *request = Some(EditorRequest::OpenModel);
    }
    let save = add_button(
        ui,
        !empty,
        Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save model")),
        "Save model",
    )
    .on_hover_or_focus_text("Save model to TOML model file")
    .on_disabled_hover_text(EMPTY_EDITOR);
    if save.clicked() {
        *request = Some(EditorRequest::SaveModel);
    }
}

/// Draws Apply, the primary button of the header, showing `text` and setting `request` when pressed. It is disabled
/// with its reason as the tooltip without compute or while the Editor holds no model.
fn apply_button(ui: &mut egui::Ui, compute: bool, empty: bool, text: String, request: &mut Option<EditorRequest>) {
    let refusal = if !compute {
        Some(NO_COMPUTE)
    } else if empty {
        Some(EMPTY_EDITOR)
    } else {
        None
    };
    let apply = add_button(ui, refusal.is_none(), filled_button(text), "Apply")
        .on_hover_or_focus_text("Check model and install it")
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

/// Draws the status area under the panes, a resizable panel that scrolls: the status of the last open or save, then
/// the notices of the model's kind. A press on a placed problem of a grid rule sets `request`.
///
/// `past_start` is whether the preview of a sheet has passed tick 0 or a button was pressed in it.
fn status_area_ui(
    ui: &mut egui::Ui,
    panel: &mut EditorPanel,
    compute: bool,
    past_start: bool,
    request: &mut Option<EditorRequest>,
) {
    Panel::bottom("henad_editor_diagnostics")
        .resizable(true)
        .default_size(DIAGNOSTICS_HEIGHT)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("henad_editor_diagnostics_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    file_status_ui(ui, panel, compute);
                    if panel.model.graph().is_some() {
                        graph_outcome_ui(ui, panel);
                    } else if panel.model.sheet().is_some() {
                        sheet_outcome_ui(ui, panel, past_start);
                    } else {
                        outcome_ui(ui, panel, request);
                    }
                });
        });
}

/// Draws the outcome of the last Apply of a graph model, the message of the last refused edit, and every problem the
/// checker finds now. A press on a problem at a node opens its pass and brings the node into view.
fn graph_outcome_ui(ui: &mut egui::Ui, panel: &mut EditorPanel) {
    let Some(graph) = panel.model.graph_mut() else {
        return;
    };
    let live: Vec<CodeDiagnostic> = graph.diagnostics().cloned().collect();
    let shown = PreviewShown {
        plays: true,
        past_start: false,
        stale: graph.has_errors(),
    };
    preview_ui(ui, &panel.preview.state, shown);
    match &panel.outcome {
        Some(outcome) => {
            outcome_line(ui, outcome);
            // The refusal of an Apply repeats the checker's errors, which the list below shows as they are now.
            for diagnostic in outcome
                .diagnostics()
                .iter()
                .filter(|diagnostic| !live.iter().any(|known| known.message == diagnostic.message))
            {
                Notice::new(NoticeKind::Error, &diagnostic_text(diagnostic)).show(ui);
            }
        }
        None => {
            Notice::new(NoticeKind::Info, PRESS_APPLY).show(ui);
        }
    }
    if let Some(status) = &graph.status {
        let kind = if status.warning {
            NoticeKind::Warning
        } else {
            NoticeKind::Info
        };
        Notice::new(kind, &status.message).show(ui);
    }
    let mut framing = None;
    for diagnostic in &live {
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
        let text = format!("{place}{}", diagnostic.message);
        let target = scope.zip(
            diagnostic
                .node
                .as_ref()
                .and_then(|node| node.parse::<henad_graph::ids::NodeId>().ok()),
        );
        let notice = Notice::new(NoticeKind::of(diagnostic.severity), &text);
        match target {
            Some((scope, node)) => {
                let response = notice.clickable().show(ui).on_hover_text("Show node on canvas");
                if response.clicked() {
                    framing = Some(graph::GraphRequest::Frame { scope, node });
                }
            }
            None => {
                notice.show(ui);
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
///
/// `past_start` is whether the preview has passed tick 0 or a button was pressed in it.
fn sheet_outcome_ui(ui: &mut egui::Ui, panel: &EditorPanel, past_start: bool) {
    let Some(sheet) = panel.model.sheet() else {
        return;
    };
    if sheet.shows_nothing() {
        Notice::new(NoticeKind::Info, WORLD_EMPTY).show(ui);
    } else {
        let shown = PreviewShown {
            plays: false,
            past_start,
            stale: sheet.has_errors(),
        };
        preview_ui(ui, &panel.preview.state, shown);
    }
    // An edit past the start restarts the world, and the notice warns of the run it took away.
    if let Some(notice) = sheet.notice() {
        Notice::new(NoticeKind::Warning, notice).show(ui);
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
            outcome_line(ui, outcome);
            for diagnostic in outcome
                .diagnostics()
                .iter()
                .filter(|diagnostic| !known.iter().any(|known| known.message == diagnostic.message))
            {
                Notice::new(NoticeKind::Error, &diagnostic.message).show(ui);
            }
        }
        None => {
            Notice::new(NoticeKind::Info, PRESS_APPLY).show(ui);
        }
    }
    // The message alone. The line and column of a problem point into the sheet's text, which sheet mode never shows.
    for diagnostic in live {
        Notice::new(NoticeKind::of(diagnostic.severity), &diagnostic.message).show(ui);
    }
}

/// State of the model a preview line describes, beside the state of the preview.
#[derive(Debug, Clone, Copy)]
struct PreviewShown {
    /// Whether the preview plays once built, as a graph's does, rather than waiting at tick 0 for Play as a sheet's
    /// does.
    plays: bool,
    /// Whether the preview of a sheet has passed tick 0 or a button was pressed in it.
    past_start: bool,
    /// Whether the model holds a problem that stops it building. A preview that runs then runs the model as it was
    /// before the edit that brought the problem.
    stale: bool,
}

/// Draws the state of the preview: its size while it runs, a warning while it runs the model as it was before an edit
/// that stops the model building, and the problems that stopped it.
///
/// A sheet's problems are listed without their line and column, which point into the sheet's text.
fn preview_ui(ui: &mut egui::Ui, state: &PreviewState, shown: PreviewShown) {
    let sheet = !shown.plays;
    let (line, problems) = match state {
        PreviewState::Idle => return,
        PreviewState::Waiting => {
            let waits = if sheet { SHEET_PREVIEW_WAITS } else { PREVIEW_WAITS };
            Notice::new(NoticeKind::Info, waits).show(ui);
            return;
        }
        PreviewState::Running(_) if shown.stale => {
            let stale = if sheet { STALE_WORLD_NOTICE } else { STALE_PREVIEW };
            Notice::new(NoticeKind::Warning, stale).show(ui);
            return;
        }
        PreviewState::Running(run) => {
            let text = preview_running_text(*run, shown.plays, shown.past_start);
            Notice::new(NoticeKind::Info, &text).show(ui);
            return;
        }
        PreviewState::Refused(problems) => ("Preview not built. Fix the problems below.", problems.as_slice()),
        PreviewState::Faulted(problem) => ("Preview stopped. Fix the problem below.", std::slice::from_ref(problem)),
    };
    Notice::new(NoticeKind::Error, line).show(ui);
    for problem in problems {
        let text = if sheet {
            problem.message.clone()
        } else {
            diagnostic_text(problem)
        };
        Notice::new(NoticeKind::Error, &text).show(ui);
    }
}

/// Returns the line of a preview that runs or waits at the size of `run`. `plays` is whether the preview plays once
/// built, and `past_start` whether the preview of a sheet has passed tick 0 or a button was pressed in it.
///
/// A graph's preview reads "Preview runs at 256 × 256 cells from tick 0. Press Apply to run the model at its own
/// size." A sheet's preview waits at tick 0, and its line names Play until the world has left its start.
pub fn preview_running_text(run: PreviewRun, plays: bool, past_start: bool) -> String {
    let size = run.size_text();
    if plays {
        format!("Preview runs at {size} from tick 0. Press Apply to run the model at its own size.")
    } else if past_start {
        format!("Preview runs at {size}. Press Apply to run the model at its own size.")
    } else {
        format!("Preview at {size} starts at tick 0. Press Play to run it, or Apply to run the model at its own size.")
    }
}

/// Draws the outcome of the last Apply: its line, then one label per problem. A press on a placed problem sets
/// `request` to move the cursor there.
fn outcome_ui(ui: &mut egui::Ui, panel: &EditorPanel, request: &mut Option<EditorRequest>) {
    let Some(outcome) = &panel.outcome else {
        let hint = if panel.model.is_empty() {
            EMPTY_EDITOR
        } else {
            PRESS_APPLY
        };
        Notice::new(NoticeKind::Info, hint).show(ui);
        return;
    };
    outcome_line(ui, outcome);
    for diagnostic in outcome.diagnostics() {
        let text = diagnostic_text(diagnostic);
        let notes: Vec<String> = diagnostic.notes.iter().map(|note| format!("note: {note}")).collect();
        let placed = diagnostic.line.zip(diagnostic.column);
        let notice = Notice::new(NoticeKind::of(diagnostic.severity), &text)
            .mono()
            .selectable()
            .notes(&notes);
        let response = match placed {
            Some(_) => notice.clickable().show(ui).on_hover_text("Move cursor to problem"),
            None => notice.show(ui),
        };
        if let Some((line, column)) = placed
            && response.clicked()
        {
            *request = Some(EditorRequest::MoveCursor { line, column });
        }
    }
}

/// Draws the line of `outcome` as a notice, an error for a refusal or a fault and information otherwise.
fn outcome_line(ui: &mut egui::Ui, outcome: &ApplyOutcome) {
    let (line, error) = outcome.summary();
    let kind = if error { NoticeKind::Error } else { NoticeKind::Info };
    Notice::new(kind, line).show(ui);
}

/// Draws the status of the last open or save as a notice, an error for a failure, and the reason nothing runs on a
/// device without compute as a warning.
fn file_status_ui(ui: &mut egui::Ui, panel: &EditorPanel, compute: bool) {
    if let Some(status) = &panel.file_status {
        let kind = if status.failed {
            NoticeKind::Error
        } else {
            NoticeKind::Info
        };
        Notice::new(kind, status.text()).show(ui);
    }
    if !compute {
        Notice::new(NoticeKind::Warning, NO_COMPUTE).show(ui);
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
        EditorRequest::NewModel => replace_model(app, Replacement::NewModel),
        EditorRequest::NewGridRule => replace_model(app, Replacement::NewGridRule),
        EditorRequest::NewGraphModel(file_name) => replace_model(app, Replacement::NewGraphModel(file_name)),
        EditorRequest::OpenModel => replace_model(app, Replacement::OpenModel),
        EditorRequest::PickImport => app.open_file(OpenTarget::Function),
        EditorRequest::SaveModel => {
            save_model(app);
        }
        EditorRequest::Apply => apply(app),
        EditorRequest::Convert => expert::open_convert(&mut app.editor),
        EditorRequest::ConfirmConvert => replace_model(app, Replacement::Convert),
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

/// Returns the id a new model of the Editor takes in place of `id`: `id` itself while no model of `models` holds it
/// as compiled code, and otherwise `prefix` and `id`, such as `sheet_game_of_life`, numbered by [`mint_id`] while a
/// compiled model holds that too.
///
/// A model applied from the Editor earlier is runtime code, and a new model under its id replaces it on Apply.
pub fn new_model_id(models: &ModelSet, id: &str, prefix: &str) -> String {
    let compiled = |candidate: &str| {
        models
            .get(candidate)
            .is_some_and(|entry| entry.source().origin() == CodeOrigin::Compiled)
    };
    if !compiled(id) {
        return id.to_owned();
    }
    let prefixed = if id.starts_with(prefix) {
        id.to_owned()
    } else {
        format!("{prefix}{id}")
    };
    mint_id(&prefixed, IdSpace::Card, compiled).unwrap_or(prefixed)
}

/// Returns the text of the Life starter, the model file of [`GridRule::life`].
pub fn life_starter() -> String {
    rule_text(GridRule::life())
}

/// Returns the model file text of `rule`.
fn rule_text(rule: GridRule) -> String {
    // The Life starter holds no text a model file cannot hold.
    ModelFile::from_rule(rule).map_or_else(|_| String::new(), |file| file.to_toml())
}

/// Replaces the Editor's text with the Life starter and brings the Editor to the front.
///
/// The rule takes another id than `life_rule` while a compiled model of the app's set holds it ([`new_model_id`]).
pub fn new_grid_rule(app: &mut AppState) {
    let mut rule = GridRule::life();
    let id = new_model_id(&app.models, &rule.id, "");
    let text = if id == rule.id {
        life_starter()
    } else {
        rule.id = id;
        rule_text(rule)
    };
    app.editor.load(text, None);
    app.editor.file_status = None;
    app.focus_request = Some(Tab::Editor);
}

/// Replaces the Editor's model with the starter whose file is named `file_name`, and brings the Editor to the front.
///
/// The graph takes another id than the starter's while a compiled model of the app's set holds it ([`new_model_id`]).
pub fn new_graph_model(app: &mut AppState, file_name: &str) {
    let Some(mut file) =
        henad_graph::starters::starter(file_name).and_then(|starter| GraphFile::parse(starter.text).ok())
    else {
        app.editor.file_status = Some(FileStatus::failure(format!("Starter {file_name} cannot be read")));
        return;
    };
    let id = new_model_id(&app.models, &file.document().target.id, "node_");
    if id != file.document().target.id {
        let mut document = file.document().clone();
        document.id.clone_from(&id);
        document.target.id = id;
        if let Ok(renamed) = GraphFile::from_document(&document, file.editor()) {
            file = renamed;
        }
    }
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
/// to the front. A model of the Editor with unsaved edits asks first, as [`replace_model`] does.
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
    if applied {
        app.focus_request = Some(Tab::Editor);
    } else {
        replace_model(app, Replacement::Open(model));
    }
}

/// Replaces the Editor's model with `model`, from no file, and brings the Editor to the front.
pub fn open_editable(app: &mut AppState, model: EditableModel) {
    app.editor.load_model(model);
    app.focus_request = Some(Tab::Editor);
}

/// Opens a dialog that picks a model file for the Editor, and records the edit marks of the models as it starts.
pub fn open_model(app: &mut AppState) {
    app.editor.note_open_started();
    app.open_file(OpenTarget::Model);
}

/// Opens a dialog that saves the Editor's model: a grid rule's text as written, comments included, or a graph model's
/// or a sheet's canonical text. Returns the target the save reports to, `None` for an empty model, which is not saved.
pub fn save_model(app: &mut AppState) -> Option<SaveTarget> {
    if app.editor.model.is_empty() {
        return None;
    }
    let name = save_name(&app.editor);
    let bytes = app.editor.model.file_text().into_bytes();
    let target = SaveTarget::Model(app.editor.next_save());
    app.editor.saving.push(PendingSave {
        target,
        mark: app.editor.model.edit_mark(),
    });
    app.save_as(target, &name, bytes);
    Some(target)
}

/// Opens a dialog that saves the model the Editor holds beside the one shown, as [`save_model`] saves the one shown,
/// and returns the target the save reports to.
fn save_counterpart(app: &mut AppState) -> Option<SaveTarget> {
    let number = app.editor.next_save();
    let counterpart = app
        .editor
        .counterpart
        .as_mut()
        .filter(|counterpart| !counterpart.model.is_empty())?;
    let name = save_name_of(&counterpart.model, counterpart.file_name.as_deref());
    let bytes = counterpart.model.file_text().into_bytes();
    let target = SaveTarget::Counterpart(number);
    counterpart.saving.push(PendingSave {
        target,
        mark: counterpart.model.edit_mark(),
    });
    app.save_as(target, &name, bytes);
    Some(target)
}

/// Returns the file name a save of the Editor's text suggests: the name it was opened or saved under, then the model's
/// id, then [`FALLBACK_FILE_NAME`].
///
/// A sheet suggests its id even while it holds errors.
pub fn save_name(panel: &EditorPanel) -> String {
    save_name_of(&panel.model, panel.file_name.as_deref())
}

/// Returns the file name a save of `model`, from the file `file_name`, suggests, as [`save_name`] does.
fn save_name_of(model: &EditorModel, file_name: Option<&str>) -> String {
    if let Some(name) = file_name {
        return name.to_owned();
    }
    if let Some(sheet) = model.sheet() {
        return format!("{}.toml", sheet.document().sheet.id);
    }
    ModelFile::parse(&model.file_text())
        .map_or_else(|_| FALLBACK_FILE_NAME.to_owned(), |file| format!("{}.toml", file.id()))
}

/// Returns the name the unsaved changes dialog gives `model`, from the file `file_name`: a sheet's or a graph's name,
/// or the file name a grid rule saves under.
fn model_name(model: &EditorModel, file_name: Option<&str>) -> String {
    match model {
        EditorModel::Sheet(sheet) => sheet.document().sheet.name.clone(),
        EditorModel::Graph(graph) => graph.document().name.clone(),
        EditorModel::Empty | EditorModel::Rule(_) => save_name_of(model, file_name),
    }
}

/// Runs `replacement` at once while no model it drops has unsaved edits, and otherwise opens the unsaved changes
/// dialog, which runs it once the edits are saved or discarded.
pub fn replace_model(app: &mut AppState, replacement: Replacement) {
    if app.editor.unsaved_models(Some(&replacement)).is_empty() {
        run_replacement(app, replacement);
    } else {
        app.editor.ask = Some(replacement);
    }
}

/// Runs `replacement` without asking.
fn run_replacement(app: &mut AppState, replacement: Replacement) {
    match replacement {
        Replacement::NewModel => sheet::new_model::open_new_model(app),
        Replacement::NewGridRule => new_grid_rule(app),
        Replacement::NewGraphModel(file_name) => new_graph_model(app, file_name),
        Replacement::OpenModel => open_model(app),
        Replacement::Open(model) => open_editable(app, model),
        Replacement::Convert => expert::convert_sheet(app),
        Replacement::OpenFile(file) => open_file_text(app, *file),
        #[cfg(not(target_arch = "wasm32"))]
        Replacement::CloseWindow => app.close_window(),
    }
}

/// Starts the save of `held`, the model shown or the model held beside it, and returns the target the save reports
/// to, `None` for an empty model, which is not saved.
fn save_held(app: &mut AppState, held: HeldModel) -> Option<SaveTarget> {
    match held {
        HeldModel::Shown => save_model(app),
        HeldModel::Beside => save_counterpart(app),
    }
}

/// Carries on with the replacement waiting for the saves the unsaved changes dialog started, once the save `target`
/// reports whether it `saved`.
///
/// The next save starts, or the replacement runs once each model is saved. A save that saved nothing, as a canceled
/// dialog, drops the replacement, and the models stay. A model edited after its save started asks again, as
/// [`replace_model`] does. The report of any other save, such as a second Save model pressed while the dialog is
/// open, carries the replacement no further.
pub fn continue_replacement(app: &mut AppState, target: SaveTarget, saved: bool) {
    let Some(after) = app.editor.after_save.as_mut() else {
        return;
    };
    if after.running != Some(target) {
        return;
    }
    if !saved {
        app.editor.after_save = None;
        return;
    }
    after.remaining.remove(0);
    start_next_save(app);
}

/// Starts the save of the next model the waiting replacement saves, or runs the replacement once each model is saved.
/// A model that holds nothing to save is passed over.
fn start_next_save(app: &mut AppState) {
    while let Some(next) = app
        .editor
        .after_save
        .as_ref()
        .map(|after| after.remaining.first().copied())
    {
        let Some(held) = next else {
            if let Some(after) = app.editor.after_save.take() {
                replace_model(app, after.replacement);
            }
            return;
        };
        let running = save_held(app, held);
        let Some(after) = app.editor.after_save.as_mut() else {
            return;
        };
        if running.is_some() {
            after.running = running;
            return;
        }
        after.remaining.remove(0);
    }
}

/// Draws the unsaved changes dialog while a replacement waits on it: its title, a sentence naming each model with
/// unsaved edits that the replacement drops, then Cancel, Discard changes and Save model.
///
/// Save model saves each such model, the one shown first, and runs the replacement once both are saved. Discard
/// changes runs it at once, and Enter presses Save model. Cancel, Escape and a click outside close the dialog and keep
/// the models.
pub fn unsaved_modal(ctx: &egui::Context, app: &mut AppState) {
    let Some(asked) = &app.editor.ask else {
        return;
    };
    let models = app.editor.unsaved_models(Some(asked));
    let names = app.editor.names_of(&models);
    if names.is_empty() {
        // The edits went, as after Undo back to the saved text, and nothing is left to lose.
        if let Some(replacement) = app.editor.ask.take() {
            run_replacement(app, replacement);
        }
        return;
    }
    let sentence = unsaved_changes(&names);
    let mut choice = None;
    let modal = Modal::new(egui::Id::new("henad_unsaved_modal")).frame(sheet::widgets::dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        sheet::widgets::dialog_title(ui, &sentence, UNSAVED_HEADING, UNSAVED_DIALOG_WIDTH);
        ui.label(&sentence);
        let buttons = [
            DialogButton::new(CANCEL, DialogButtonKind::Plain),
            DialogButton::new(DISCARD_CHANGES, DialogButtonKind::Plain),
            DialogButton::new(SAVE_MODEL, DialogButtonKind::Primary),
        ];
        choice = match sheet::widgets::dialog_buttons(ui, &buttons) {
            Some(0) => Some(UnsavedChoice::Cancel),
            Some(1) => Some(UnsavedChoice::Discard),
            Some(_) => Some(UnsavedChoice::Save),
            None => None,
        };
    });
    if choice.is_none() && response.should_close() {
        choice = Some(UnsavedChoice::Cancel);
    }
    let Some(choice) = choice else {
        return;
    };
    let Some(replacement) = app.editor.ask.take() else {
        return;
    };
    match choice {
        UnsavedChoice::Cancel => {}
        UnsavedChoice::Discard => run_replacement(app, replacement),
        UnsavedChoice::Save => {
            app.editor.after_save = Some(AfterSave {
                replacement,
                remaining: models,
                running: None,
            });
            start_next_save(app);
        }
    }
}

/// Draws the dialog that asks before Apply replaces a model applied from another sheet under the same id, while
/// [`EditorPanel::replace_question`] holds the model: its title, the question that names the model and the id, then
/// Cancel and Replace.
///
/// Replace applies the Editor's model in place of the applied model, and Enter presses it. Cancel, Escape and a click
/// outside close the dialog and leave the applied model as it is.
pub fn replace_question_modal(ctx: &egui::Context, app: &mut AppState) {
    let Some(question) = &app.editor.replace_question else {
        return;
    };
    let sentence = replace_applied(&question.name, &question.id);
    let mut pressed = None;
    let modal = Modal::new(egui::Id::new("henad_replace_applied_modal")).frame(sheet::widgets::dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        sheet::widgets::dialog_title(
            ui,
            REPLACE_APPLIED_HEADING,
            REPLACE_APPLIED_HEADING,
            REPLACE_DIALOG_WIDTH,
        );
        ui.label(&sentence);
        let buttons = [
            DialogButton::new(CANCEL, DialogButtonKind::Plain),
            DialogButton::new(REPLACE, DialogButtonKind::Destructive),
        ];
        pressed = sheet::widgets::dialog_buttons(ui, &buttons);
    });
    match pressed {
        Some(1) => apply_replacing(app),
        Some(_) => app.editor.replace_question = None,
        None if response.should_close() => app.editor.replace_question = None,
        None => {}
    }
}

/// Consumes the undo and redo shortcuts of `input`, and returns whether undo and whether redo was pressed.
///
/// Redo, command shift Z or command Y, is read first. Otherwise command Z would take command shift Z.
fn undo_redo_keys(input: &mut egui::InputState) -> (bool, bool) {
    let redo = input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
        || input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y));
    let undo = input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));
    (undo, redo)
}

/// Handles the result of an Open model dialog. The picked file replaces the Editor's model, and is applied only on
/// Apply.
///
/// The `kind` in the file's header decides where it opens, as [`open_text`] says. A model edited while the dialog was
/// open asks first, as [`replace_model`] does. The decision taken when the dialog opened covers the edits made before.
pub fn receive_open(app: &mut AppState, result: OpenResult) {
    let marks = app.editor.open_marks.take();
    let Some(DialogFile { name, bytes, path }) = opened_file(&mut app.editor, result) else {
        return;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        app.editor.file_status = Some(FileStatus::not_text(&name));
        return;
    };
    let file = OpenedFile { name, text, path };
    if marks.is_some_and(|marks| marks != app.editor.edit_marks()) {
        replace_model(app, Replacement::OpenFile(Box::new(file)));
    } else {
        open_file_text(app, file);
    }
}

/// Replaces the Editor's model with the model of `file`, picked in the Open model dialog, and brings the Editor to the
/// front.
fn open_file_text(app: &mut AppState, file: OpenedFile) {
    let OpenedFile { name, text, path } = file;
    app.editor.file_status = Some(FileStatus::note(format!("Opened {name}")));
    open_text(&mut app.editor, text, Some(name));
    app.editor.file_path = path;
    expert::find_original(&mut app.editor);
    app.focus_request = Some(Tab::Editor);
}

/// Returns the first file of `result`, the outcome of an open dialog, `None` for a folder or a cancel. A failed open
/// sets the file status of `panel` to its message.
fn opened_file(panel: &mut EditorPanel, result: OpenResult) -> Option<DialogFile> {
    match result {
        OpenResult::Files(files) => files.into_iter().next(),
        OpenResult::Failed(message) => {
            panel.file_status = Some(FileStatus::failure(format!("Open failed: {message}")));
            None
        }
        OpenResult::Folder(_) | OpenResult::Canceled => None,
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
    let file = opened_file(&mut app.editor, result);
    let (Some(DialogFile { name, bytes, .. }), Some(graph)) = (file, app.editor.model.graph_mut()) else {
        return;
    };
    match String::from_utf8(bytes) {
        Ok(text) => graph.set_import_source(name, text),
        Err(_) => app.editor.file_status = Some(FileStatus::not_text(&name)),
    }
}

/// Reads the Editor's model file, compiles its model, and installs and runs the model.
///
/// A text that is refused leaves the set and the running model alone, and the Editor lists the problems. A model of an
/// id that holds a model applied from another sheet, a sheet of another name that the Editor did not apply last, waits
/// on the dialog that asks before it replaces that model ([`replace_question_modal`]). Otherwise the model replaces the
/// set's entry with its id, or joins the set, and becomes the selection:
///
/// - a run of the model past tick 0 keeps going and waits on the Rebuild model dialog, as
///   [`AppState::install_models`] leaves it, and a run of the model at tick 0 is rebuilt at once. The dialog opens
///   again for a run whose code differs from the set's entry, as after a canceled dialog.
/// - the Editor's preview is replaced by a build of the model at its own size, at any tick.
/// - with no run, a run of another model still at tick 0, or a build of another model that no run to a tick waits
///   for, the model is built as Build builds it. A model that Build would refuse is not built, and the Editor names
///   the reason, such as the sizes the device lacks or a Seed field that holds no seed.
/// - a run of another model past tick 0 keeps going, and the model waits for Build.
///
/// A model that waits for a Rebuild or a Build that is disabled reads as [`ApplyOutcome::BuildRefused`] with the reason
/// the button gives, and as waiting for the button once the reason is gone.
///
/// A build runs on the app's build thread on native, and the outcome reads as applied from its start. A fault of the
/// build that the Editor claims replaces the outcome when the build returns it.
pub fn apply(app: &mut AppState) {
    let Some(entry) = checked_entry(app) else {
        return;
    };
    if let Some(question) = replace_question(app, &entry) {
        app.editor.replace_question = Some(question);
        return;
    }
    install_applied(app, entry);
}

/// Applies the Editor's model as [`apply`] does, in place of a model applied from another sheet, after Replace in the
/// dialog that asks first.
///
/// A model of another id than the one the dialog named, as one opened while the dialog was open, is applied as
/// [`apply`] applies it, asking again where it replaces a model applied from another sheet.
pub fn apply_replacing(app: &mut AppState) {
    let Some(question) = app.editor.replace_question.take() else {
        return;
    };
    let Some(entry) = checked_entry(app) else {
        return;
    };
    if entry.id() == question.id {
        install_applied(app, entry);
    } else {
        apply(app);
    }
}

/// Returns the entry of the Editor's model file, or `None` once the file is refused, as without compute, with the
/// problems as the outcome.
fn checked_entry(app: &mut AppState) -> Option<ModelEntry> {
    if app.gpu_ctx.is_none() {
        app.editor.outcome = Some(ApplyOutcome::Refused(vec![CodeDiagnostic::error(NO_COMPUTE)]));
        return None;
    }
    match ModelFile::parse(&app.editor.model.file_text()).and_then(|file| file.entry()) {
        Ok(entry) => Some(entry),
        Err(error) => {
            app.editor.outcome = Some(ApplyOutcome::Refused(error.diagnostics().to_vec()));
            None
        }
    }
}

/// Returns the question to ask before `entry` replaces the model of its id, `None` when Apply replaces it without
/// asking.
///
/// Apply asks when the model was applied from a sheet whose name differs from the name of `entry`, and the Editor did
/// not apply it last, as before a rename of the same sheet.
fn replace_question(app: &AppState, entry: &ModelEntry) -> Option<ReplaceQuestion> {
    let held = app.models.get(entry.id())?;
    let from_sheet = held
        .source()
        .author()
        .is_some_and(|author| author.format() == SHEET_CODE_FORMAT);
    let other = from_sheet && held.name() != entry.name() && !app.editor.is_applied(held.source());
    other.then(|| ReplaceQuestion {
        name: held.name().to_owned(),
        id: entry.id().to_owned(),
    })
}

/// Installs `entry`, the Editor's model, and runs it as [`apply`] says.
fn install_applied(app: &mut AppState, entry: ModelEntry) {
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
    app.editor.applied_mark = Some(app.editor.model.edit_mark());
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
        // Build is disabled for the same reason, a seed the Seed field does not show included. In a browser the
        // engine's own check of the sizes would end the page.
        if let Some(message) = app.build_refusal() {
            let problem = CodeDiagnostic::error(message);
            app.editor.outcome = Some(ApplyOutcome::BuildRefused(vec![problem]));
            return;
        }
        app.reset_simulation();
        if app.editor.outcome.is_some() {
            return;
        }
    }
    let awaits_rebuild = app.pending_rebuild.as_ref().is_some_and(|pending| pending.model == id);
    let outcome = if unchanged {
        ApplyOutcome::Unchanged
    } else if runs_source(app) {
        ApplyOutcome::Applied
    } else {
        // The line never asks for a press of a disabled Rebuild or Build, and names the reason instead.
        let refusal = if awaits_rebuild {
            app.rebuild_refusal()
        } else {
            app.build_refusal()
        };
        match refusal {
            Some(reason) => ApplyOutcome::BuildRefused(vec![CodeDiagnostic::error(reason)]),
            None if awaits_rebuild => ApplyOutcome::AwaitingRebuild,
            None => ApplyOutcome::Installed,
        }
    };
    app.editor.outcome = Some(outcome);
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
        assert!(ApplyOutcome::BuildRefused(Vec::new()).summary().1);
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
