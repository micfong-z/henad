//! The sheet editor of the Editor tab: a sheet model shown as its sheet and its cards.
//!
//! A *sheet* is a model file of `kind = "sheet"`, the author document of [`henad_graph::sheet`]. The *Sheet* pane
//! lists what the model holds (its purpose, world, properties, parameters, measures, colours and buttons), and the
//! *Card* pane lists each card's numbered lines as sentences. The world itself stays in the dock's Viewport, Playback,
//! Pacing, Parameters and Charts tabs.
//!
//! A sheet opens through the sheet reader alone, so a sheet with checker errors still opens here. Its preview builds
//! only while the sheet reads, checks and lowers and holds a property, and the preview waits paused at tick 0 until
//! Play. Every edit of the Sheet pane and the Card pane runs [`SheetEdit`](henad_graph::sheet::edit::SheetEdit)
//! commands. Until the preview passes tick 0 or a button is pressed, an edit draws the start again at tick 0. After
//! that, a changed Starts as waits for Start again, and any other edit starts the world again from tick 0 with a
//! notice.
//!
//! The *toolbar* above the panes holds the model's name and Undo and Redo. A click on the name turns it into a field.
//! Undo keeps the canonical text of the sheet before each edit, and an undo or a redo reads that text back as one
//! more edit of the world.

pub mod card_pane;
pub mod help;
pub mod new_model;
pub mod picker;
pub mod sheet_pane;
pub mod widgets;

use std::sync::atomic::Ordering;

use egui::{Button, CentralPanel, Frame, Key, KeyboardShortcut, Margin, Modifiers, Panel, ScrollArea, TextEdit};
use henad_core::diagnostic::CodeDiagnostic;
use henad_graph::sheet::check::{SheetReport, check};
use henad_graph::sheet::document::{MadeOf, SheetDocument, StartsAs};
use henad_graph::sheet::edit::{SheetBuilder, SheetEdit, SheetEditError};
use henad_graph::sheet::sentence::{
    MODEL_NAME_FIELD, REDO, REDO_TOOLTIP, RENAME_MODEL_TOOLTIP, RESTART_NOTICE, UNDO, UNDO_TOOLTIP, model_name_button,
};
use henad_graph::sheet::{ParsedSheet, SheetFile};

use super::graph::history::History;
use super::graph::{CompiledProgram, GraphProgram, LOADS};
use crate::icons::material_design_icons::{MDI_REDO, MDI_UNDO};
use crate::ui::sweep::layout::add_button;

/// Heading of the pane that lists what the model holds.
pub const SHEET_PANE: &str = "Sheet";

/// Heading of the pane that lists the cards' lines.
pub const CARD_PANE: &str = "Card";

/// Default share of the Editor's width the Sheet pane takes. The Card pane takes the rest.
const SHEET_PANE_SHARE: f32 = 0.4;

/// Narrowest width in points of each pane.
const PANE_MIN_WIDTH: f32 = 160.0;

/// A sheet model in the Editor.
pub struct SheetEditor {
    /// Document the sheet holds, with the keys its new draws take.
    builder: SheetBuilder,
    /// Canonical text of the document. Save model writes it and Apply reads it.
    text: String,
    /// Report of the sheet's check, each problem placed in [`Self::text`].
    report: SheetReport,
    /// Sheet read, checked and lowered from [`Self::text`], or the problems that stop it lowering.
    file: Result<SheetFile, Vec<CodeDiagnostic>>,
    /// Number of the load, or of the last edit that started the preview again. It keys the preview.
    load: u64,
    /// Program the preview runs, compiled on first use.
    program: Option<CompiledProgram>,
    /// Start of each property when the preview was last built, `None` before the first build.
    built_starts: Option<Vec<(String, Option<StartsAs>)>>,
    /// Notice of the last edit, such as [`RESTART_NOTICE`], `None` while none shows.
    notice: Option<&'static str>,
    /// State of the Sheet pane's forms, fields and dialogs.
    pane: sheet_pane::PaneState,
    /// State of the Card pane's pickers, rename field and dialog.
    card: card_pane::CardState,
    /// Canonical texts of the sheet before each edit, and after each undo, for Undo and Redo.
    history: History,
    /// Text of the model name's field while it is open, `None` while the name shows as a button.
    renaming: Option<String>,
}

/// Prints the sheet's id, the load and whether the sheet lowers.
impl std::fmt::Debug for SheetEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetEditor")
            .field("id", &self.document().sheet.id)
            .field("load", &self.load)
            .field("lowers", &self.file.is_ok())
            .finish_non_exhaustive()
    }
}

impl SheetEditor {
    /// Returns an editor of `document`, checked and lowered from its canonical text.
    ///
    /// Note that a document whose canonical text does not write, as with a number that is not finite, keeps an empty
    /// text and the writer's error as the problem that stops it lowering. The sheet reader refuses such a number.
    pub fn new(document: SheetDocument) -> Self {
        let load = LOADS.fetch_add(1, Ordering::Relaxed);
        let mut editor = Self {
            builder: SheetBuilder::new(document),
            text: String::new(),
            report: SheetReport::default(),
            file: Err(Vec::new()),
            load,
            program: None,
            built_starts: None,
            notice: None,
            pane: sheet_pane::PaneState::new(load),
            card: card_pane::CardState::new(load),
            history: History::default(),
            renaming: None,
        };
        editor.refresh();
        editor
    }

    /// Writes the canonical text of the document again, with its report and its lowering, and drops the compiled
    /// program.
    fn refresh(&mut self) {
        let document = self.builder.document();
        let (text, report, file) = match document.to_text() {
            Ok(text) => {
                let report = ParsedSheet::parse(&text).map_or_else(|_| check(document), |parsed| parsed.check());
                let file = SheetFile::parse(&text).map_err(|error| error.diagnostics().to_vec());
                (text, report, file)
            }
            Err(error) => (
                String::new(),
                check(document),
                Err(vec![CodeDiagnostic::error(error.to_string())]),
            ),
        };
        self.text = text;
        self.report = report;
        self.file = file;
        self.program = None;
    }

    /// Runs `edit`, one edit of the sheet made of one or more commands, on a copy of the sheet, and keeps the copy
    /// when every command is accepted. A kept edit is one step of Undo.
    ///
    /// `past_start` is whether the preview has passed tick 0 or a button was pressed in it, and `starts_only` whether
    /// the edit leaves the running world alone apart from how properties start, as a new start, a new name or a new
    /// purpose does. Every edit starts the preview again from tick 0, except an edit that changes only starts past the
    /// start. That edit waits for Start again. A restart past the start shows [`RESTART_NOTICE`].
    ///
    /// # Errors
    ///
    /// Returns the reason a command refused the edit. The sheet is then unchanged.
    pub fn edit(
        &mut self,
        past_start: bool,
        starts_only: bool,
        edit: impl FnOnce(&mut SheetBuilder) -> Result<(), SheetEditError>,
    ) -> Result<(), SheetEditError> {
        let mut builder = self.builder.clone();
        edit(&mut builder)?;
        if builder.document() == self.builder.document() {
            return Ok(());
        }
        self.history.record(self.text.clone(), None);
        self.builder = builder;
        self.settle(past_start, starts_only);
        Ok(())
    }

    /// Writes the sheet's text again after an edit, and starts the preview again unless `past_start` and
    /// `starts_only` both hold, as [`Self::edit`] says.
    fn settle(&mut self, past_start: bool, starts_only: bool) {
        self.refresh();
        let restart = !(past_start && starts_only);
        if restart {
            self.load = LOADS.fetch_add(1, Ordering::Relaxed);
        }
        self.notice = (past_start && restart && !self.has_errors() && !self.shows_nothing()).then_some(RESTART_NOTICE);
    }

    /// Undoes the last edit, and returns whether one was undone. The preview follows as after an edit, `past_start`
    /// saying whether it has left its start.
    pub fn undo(&mut self, past_start: bool) -> bool {
        match self.history.undo(self.text.clone()) {
            Some(text) => self.restore(&text, past_start),
            None => false,
        }
    }

    /// Redoes the last edit undone, and returns whether one was redone. The preview follows as after an edit,
    /// `past_start` saying whether it has left its start.
    pub fn redo(&mut self, past_start: bool) -> bool {
        match self.history.redo(self.text.clone()) {
            Some(text) => self.restore(&text, past_start),
            None => false,
        }
    }

    /// Returns whether Undo has an edit to undo.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Returns whether Redo has an edit to redo.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Replaces the sheet with the one `text` holds, a text of the history, closes the pane's forms and returns
    /// whether the text read. Past the start, an edit of starts, the name or the purpose alone leaves the running world
    /// alone, as after [`Self::edit`].
    fn restore(&mut self, text: &str, past_start: bool) -> bool {
        let Ok(parsed) = ParsedSheet::parse(text) else {
            return false;
        };
        let document = parsed.into_document();
        let starts_only = without_starts_or_words(self.document()) == without_starts_or_words(&document);
        self.builder = SheetBuilder::new(document);
        self.pane.close_forms();
        self.card.close();
        self.renaming = None;
        self.settle(past_start, starts_only);
        true
    }

    /// Returns whether a form of the Sheet pane or a picker of the Card pane holds a dialog open. An open dialog takes
    /// the keys of Undo and Redo.
    pub fn dialog_open(&self) -> bool {
        self.pane.dialog_open() || self.card.dialog_open()
    }

    /// Records the start of each property as the preview was just built with it.
    pub fn mark_built(&mut self) {
        self.built_starts = Some(starts_of(self.document()));
    }

    /// Returns whether property `property` starts otherwise than when the preview was last built.
    pub fn start_changed(&self, property: &str) -> bool {
        let Some(built) = &self.built_starts else {
            return false;
        };
        let start = |starts: &[(String, Option<StartsAs>)]| {
            starts
                .iter()
                .find(|(id, _)| id == property)
                .map(|(_, start)| start.clone())
        };
        start(built) != start(&starts_of(self.document()))
    }

    /// Notice of the last edit, `None` while none shows.
    pub fn notice(&self) -> Option<&'static str> {
        self.notice
    }

    /// Returns whether the sheet is a world of cells with no property. Such a world shows nothing until a property is
    /// added.
    pub fn shows_nothing(&self) -> bool {
        let document = self.document();
        document.world.made_of == MadeOf::Cells && document.cells.as_ref().is_none_or(Vec::is_empty)
    }

    /// Document the sheet holds.
    pub fn document(&self) -> &SheetDocument {
        self.builder.document()
    }

    /// Canonical text of the sheet. Save model writes it and Apply reads it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Report of the sheet's check, each problem placed in [`Self::text`].
    pub fn report(&self) -> &SheetReport {
        &self.report
    }

    /// Returns whether the sheet holds a problem that stops it lowering. The preview is then left unbuilt.
    pub fn has_errors(&self) -> bool {
        self.file.is_err()
    }

    /// Problems that stop the sheet lowering, empty for a sheet that lowers.
    pub fn lowering_problems(&self) -> &[CodeDiagnostic] {
        self.file.as_ref().err().map_or(&[], Vec::as_slice)
    }

    /// Returns the key of what the preview shows. Every load of a sheet takes a key of its own.
    pub fn preview_key(&self) -> [u64; 3] {
        [self.load, 0, 0]
    }

    /// Returns the program the preview runs: the graph the sheet lowers to, compiled on first use.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the sheet lowering, or the compiler's problems, each placed at the sheet line
    /// whose node it names.
    pub fn program(&mut self) -> CompiledProgram {
        let file = &self.file;
        self.program
            .get_or_insert_with(|| match file {
                Ok(file) => GraphProgram::compile(file.lowered()).map_err(|problems| {
                    problems
                        .into_iter()
                        .map(|problem| file.place_graph_diagnostic(problem))
                        .collect()
                }),
                Err(problems) => Err(problems.clone()),
            })
            .clone()
    }

    /// Makes `value` the value parameter `parameter` keeps, widening its range to hold it, as one edit of the sheet
    /// and one step of Undo.
    ///
    /// The preview keeps its key. It already runs with the value.
    ///
    /// # Errors
    ///
    /// Returns the reason the edit was refused, such as a parameter the sheet does not hold. The sheet is then
    /// unchanged.
    pub fn keep_value(&mut self, parameter: &str, value: f64) -> Result<(), SheetEditError> {
        let Some(kept) = self
            .document()
            .parameters
            .iter()
            .find(|known| known.id == parameter)
            .cloned()
        else {
            return Err(SheetEditError::Missing(format!("no parameter has the id {parameter}")));
        };
        let edit = SheetEdit::SetParameter {
            parameter: kept.id,
            min: kept.min.min(value),
            max: kept.max.max(value),
            default: value,
            step: kept.step,
            unit: kept.unit,
            means: kept.means,
        };
        let mut builder = self.builder.clone();
        builder.apply(edit)?;
        self.history.record(self.text.clone(), None);
        self.builder = builder;
        self.refresh();
        Ok(())
    }

    /// Returns `diagnostic`, a problem of the graph the sheet lowers to, placed in the sheet: at the line of its node,
    /// at the table of its pass, or at `[sheet]`. A sheet that does not lower returns it as it is.
    pub fn place(&self, diagnostic: CodeDiagnostic) -> CodeDiagnostic {
        match &self.file {
            Ok(file) => file.place_graph_diagnostic(diagnostic),
            Err(_) => diagnostic,
        }
    }
}

/// Draws the sheet editor: the toolbar, then the Sheet pane on the left and the Card pane beside it, each scrolling on
/// its own.
///
/// Each pane is a group named by its heading. `past_start` is whether the preview has passed tick 0 or a button was
/// pressed in it, and `trials` holds the values the world tries for the sheet's parameters. `keys` is whether the New
/// model dialog is closed. Undo and Redo then take their keys while no modal of the app is open and no text field
/// holds the focus.
pub fn sheet_ui(
    ui: &mut egui::Ui,
    sheet: &mut SheetEditor,
    past_start: bool,
    trials: &[sheet_pane::SheetTrial],
    keys: bool,
) {
    // egui reports the top modal layer of the previous frame. Every modal stays open past the frame that opens it.
    let modal_open = ui.ctx().memory(|memory| memory.top_modal_layer().is_some());
    if keys && !modal_open && !sheet.dialog_open() && !ui.ctx().text_edit_focused() {
        let (undo, redo) = ui.ctx().input_mut(|input| {
            let redo = input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
                || input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y));
            let undo = input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));
            (undo, redo)
        });
        if redo {
            sheet.redo(past_start);
        }
        if undo {
            sheet.undo(past_start);
        }
    }
    Panel::top("henad_sheet_toolbar")
        .resizable(false)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(8, 4)))
        .show(ui, |ui| ui.horizontal(|ui| toolbar_ui(ui, sheet, past_start)));
    let width = ui.available_width();
    Panel::left("henad_sheet_pane")
        .resizable(true)
        .default_size(width * SHEET_PANE_SHARE)
        .min_size(PANE_MIN_WIDTH)
        .max_size((width - PANE_MIN_WIDTH).max(PANE_MIN_WIDTH))
        .frame(Frame::NONE.inner_margin(Margin::symmetric(8, 4)))
        .show(ui, |ui| {
            ui.heading(SHEET_PANE);
            ScrollArea::vertical()
                .id_salt("henad_sheet_pane_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| sheet_pane::sheet_pane(ui, sheet, past_start, trials));
            pane_group(ui, SHEET_PANE);
        });
    CentralPanel::no_frame().show(ui, |ui| {
        Frame::NONE.inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.heading(CARD_PANE);
            ScrollArea::vertical()
                .id_salt("henad_card_pane_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| card_pane::card_pane(ui, sheet, past_start, trials));
            pane_group(ui, CARD_PANE);
        });
    });
}

/// Draws the toolbar: the model's name, a button that turns into a field named "Model name", then Undo and Redo.
///
/// In the field, Enter or a click elsewhere keeps the name and Escape puts the old name back. An empty name keeps the
/// old one.
fn toolbar_ui(ui: &mut egui::Ui, sheet: &mut SheetEditor, past_start: bool) {
    let name = sheet.document().sheet.name.clone();
    if let Some(text) = &mut sheet.renaming {
        let field = ui.add(TextEdit::singleline(text).desired_width(200.0));
        widgets::set_name(ui, field.id, MODEL_NAME_FIELD);
        if !field.lost_focus() && !ui.memory(|memory| memory.has_focus(field.id)) {
            field.request_focus();
        }
        if field.lost_focus() {
            let kept = text.trim().to_owned();
            sheet.renaming = None;
            let canceled = ui.input(|input| input.key_pressed(Key::Escape));
            if !canceled && !kept.is_empty() && kept != name {
                let rename = SheetEdit::SetSheetName { name: kept };
                // A name the sheet refuses keeps the old one.
                sheet
                    .edit(past_start, true, |builder| builder.apply(rename).map(drop))
                    .unwrap_or_default();
            }
        }
    } else {
        let button = add_button(ui, true, Button::new(&name).frame(false), &model_name_button(&name))
            .on_hover_text(RENAME_MODEL_TOOLTIP);
        if button.clicked() {
            sheet.renaming = Some(name);
        }
    }
    ui.separator();
    let undo =
        add_button(ui, sheet.can_undo(), Button::new(format!("{MDI_UNDO} {UNDO}")), UNDO).on_hover_text(UNDO_TOOLTIP);
    if undo.clicked() {
        sheet.undo(past_start);
    }
    let redo =
        add_button(ui, sheet.can_redo(), Button::new(format!("{MDI_REDO} {REDO}")), REDO).on_hover_text(REDO_TOOLTIP);
    if redo.clicked() {
        sheet.redo(past_start);
    }
}

/// Returns `document` with no name, no purpose and no property holding a start.
///
/// Two sheets equal apart from these differ only in what leaves the running world alone, as [`SheetEditor::edit`]
/// treats a new start, a new name or a new purpose.
fn without_starts_or_words(document: &SheetDocument) -> SheetDocument {
    let mut document = document.clone();
    document.sheet.name.clear();
    document.sheet.purpose = None;
    let agents = document.agents.iter_mut().flat_map(|kind| &mut kind.properties);
    let ground = document.ground.iter_mut().flat_map(|ground| &mut ground.properties);
    for property in document.cells.iter_mut().flatten().chain(agents).chain(ground) {
        property.starts_as = None;
    }
    document
}

/// Returns the start of each property of `document`, by id.
fn starts_of(document: &SheetDocument) -> Vec<(String, Option<StartsAs>)> {
    sheet_pane::all_properties(document)
        .map(|property| (property.id.clone(), property.starts_as.clone()))
        .collect()
}

/// Gives the node of `ui` the role of a group named `name` with the bounds of what `ui` holds, as a pane of the sheet
/// editor takes.
fn pane_group(ui: &egui::Ui, name: &str) {
    accessible_group(
        ui.ctx(),
        ui.unique_id(),
        egui::accesskit::Role::Group,
        name,
        ui.min_rect(),
    );
}

/// Gives the accessible node `id` of a `Ui` the role `role`, the name `name` and the bounds `rect`.
///
/// egui gives the node of a `Ui` no bounds, and an accessibility client places a named group by them.
pub fn accessible_group(ctx: &egui::Context, id: egui::Id, role: egui::accesskit::Role, name: &str, rect: egui::Rect) {
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(role);
        node.set_label(name);
        node.set_bounds(egui::accesskit::Rect {
            x0: rect.min.x.into(),
            y0: rect.min.y.into(),
            x1: rect.max.x.into(),
            y1: rect.max.y.into(),
        });
    });
}
