//! The sheet editor of the Editor tab: a sheet model shown as its sheet and its cards.
//!
//! A *sheet* is a model file of `kind = "sheet"`, the author document of [`henad_graph::sheet`]. The *Sheet* pane
//! lists what the model holds (its purpose, world, properties, parameters, measures, colours and buttons), and the
//! *Card* pane lists each card's numbered lines as sentences, or draws them read-only as the *Diagram*. The world
//! itself stays in the dock's Viewport, Playback, Pacing, Parameters and Charts tabs.
//!
//! A sheet opens through the sheet reader alone, so a sheet with checker errors still opens here. Its preview builds
//! only while the sheet reads, checks and lowers and holds a property, and the preview waits paused at tick 0 until
//! Play. Every edit of the Sheet pane and the Card pane runs [`SheetEdit`]
//! commands. Until the preview passes tick 0 or a button is pressed, an edit draws the start again at tick 0. After
//! that, a changed Starts as waits for Start again, and any other edit starts the world again from tick 0 with a
//! notice.
//!
//! The *name row*, the second row of the Editor's header, holds the model's name and Undo and Redo. A click on the
//! name turns it into a field. Undo keeps the canonical text of the sheet before each edit, and an undo or a redo reads
//! that text back as one more edit of the world.

pub mod card_drag;
pub mod card_lines;
pub mod card_pane;
pub mod delete_question;
pub mod diagram;
pub mod help;
pub mod new_model;
pub mod picker;
pub mod sheet_pane;
pub mod widgets;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use widgets::FocusTooltip as _;

use egui::{
    Button, CentralPanel, Color32, Frame, Key, Margin, Panel, RichText, ScrollArea, TextEdit, TextStyle, UiBuilder,
};
use henad_compute::entry::ModelEntry;
use henad_compute::snapshot::Snapshot;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::ModelCode;
use henad_explore::model_file::sheet_probes;
use henad_graph::sheet::ConvertRefusal;
use henad_graph::sheet::check::{SheetReport, check};
use henad_graph::sheet::document::{
    CELLS_ID, GROUND_ID, MadeOf, SheetDocument, SheetParameter, SheetProperty, StartsAs,
};
use henad_graph::sheet::edit::{SheetBuilder, SheetEdit, SheetEditError};
use henad_graph::sheet::sentence::{
    CONVERT_HEADING, DIAGRAM, DIAGRAM_TOOLTIP, LINES_VIEW, MODEL_ACTIONS, MODEL_NAME_FIELD, REDO, REDO_TOOLTIP,
    RENAME_MODEL_TOOLTIP, RESTART_NOTICE, UNDO, UNDO_TOOLTIP, converted_link, model_name_button, rename_model_named,
};
use henad_graph::sheet::{ParsedSheet, SheetFile};

use super::history::History;
use super::program::{CompiledProgram, GraphProgram, LOADS};
use super::undo_redo_keys;
use crate::icons::material_design_icons::{MDI_DOTS_HORIZONTAL, MDI_REDO, MDI_UNDO};
use crate::ui::dock::Layout;
use crate::ui::focus;
use crate::ui::sweep::layout::add_button;

/// Heading of the pane that lists what the model holds.
pub const SHEET_PANE: &str = "Sheet";

/// Heading of the pane that lists the cards' lines.
pub const CARD_PANE: &str = "Card";

/// Default share of the Editor's width the Sheet pane takes. The Card pane takes the rest.
const SHEET_PANE_SHARE: f32 = 0.4;

/// Narrowest width in points of each pane.
const PANE_MIN_WIDTH: f32 = 160.0;

/// Narrowest width in points of the model's name in the name row, before it ends in an ellipsis.
const NAME_MIN_WIDTH: f32 = 80.0;

/// Returns the id of the Sheet pane in `layout`, one per layout.
///
/// The pane's share of the Editor's width is kept under the id, so a share dragged in the narrow default layout leaves
/// the Editor layout alone.
fn sheet_pane_id(layout: Layout) -> egui::Id {
    egui::Id::new("henad_sheet_pane").with(layout)
}

/// Returns the width in points the Sheet pane takes in an Editor `width` points wide, by the share of the pane `id`
/// keeps, and sets the width egui keeps for the pane to it.
///
/// egui keeps a panel's width in points. A pane set from its share keeps its share as the window narrows or widens.
fn sheet_pane_width(ctx: &egui::Context, id: egui::Id, width: f32) -> f32 {
    let share = ctx
        .data_mut(|data| data.get_persisted::<f32>(id.with("share")))
        .unwrap_or(SHEET_PANE_SHARE);
    let pane = (share * width).clamp(PANE_MIN_WIDTH, (width - PANE_MIN_WIDTH).max(PANE_MIN_WIDTH));
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(pane, 1.0));
    ctx.data_mut(|data| data.insert_persisted(id, egui::containers::panel::PanelState { outer_rect: rect }));
    pane
}

/// Keeps the share of the Editor's `width` that the Sheet pane `id` takes after a drag of its divider left it `pane`
/// points wide, where the width it was set to is `set`.
fn keep_sheet_pane_share(ctx: &egui::Context, id: egui::Id, width: f32, set: f32, pane: f32) {
    let dragged = ctx.input(|input| input.pointer.any_down() || input.pointer.any_released());
    if dragged && (pane - set).abs() > 0.5 && width > 0.0 {
        ctx.data_mut(|data| data.insert_persisted(id.with("share"), pane / width));
    }
}

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
    /// Code identity of [`Self::file`], `None` while a problem stops the sheet lowering.
    code: Option<ModelCode>,
    /// Ids of the lines with a fire count in the last lowering of the sheet.
    ///
    /// Note that an edit that stops the sheet lowering keeps the ids of the lowering before it, and the cards keep
    /// their column of counts until the sheet lowers again.
    counted: BTreeSet<String>,
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
    /// Whether the Card pane shows the Diagram in place of the lines.
    pub diagram: bool,
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
            code: None,
            counted: BTreeSet::new(),
            built_starts: None,
            notice: None,
            pane: sheet_pane::PaneState::new(load),
            card: card_pane::CardState::new(load),
            history: History::default(),
            renaming: None,
            diagram: false,
        };
        editor.refresh();
        editor
    }

    /// Writes the canonical text of the document again, with its report, its lowering and its code identity, and drops
    /// the compiled program.
    fn refresh(&mut self) {
        let document = self.builder.document();
        let (text, report, file) = match document.to_text() {
            Ok(text) => {
                let parsed = ParsedSheet::parse(&text).ok();
                let mut report = parsed.as_ref().map_or_else(|| check(document), ParsedSheet::check);
                let file = SheetFile::parse(&text).map_err(|error| error.diagnostics().to_vec());
                // A sheet that checks but does not lower takes the lowering's problems into its report, so the card
                // draws each problem about a line under that line.
                if file.is_err()
                    && !report.has_errors()
                    && let Some(parsed) = &parsed
                {
                    let problems = SheetFile::lowering_problems(parsed, &report);
                    report.diagnostics.extend(problems);
                }
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
        self.code = file.as_ref().ok().map(SheetFile::code);
        if let Ok(file) = &file {
            self.counted = file.fire_lines().lines().iter().cloned().collect();
        }
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

    /// Sheet read, checked and lowered from [`Self::text`], `None` while a problem stops it lowering.
    pub fn file(&self) -> Option<&SheetFile> {
        self.file.as_ref().ok()
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

    /// Returns the program the preview runs: the graph the sheet lowers to, compiled on first use with the probes of
    /// its change lines, as Apply builds it.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the sheet lowering, or the compiler's problems, each placed at the sheet line
    /// whose node it names.
    pub fn program(&mut self) -> CompiledProgram {
        let file = &self.file;
        self.program
            .get_or_insert_with(|| match file {
                Ok(file) => GraphProgram::compile_with_probes(file.lowered(), sheet_probes(file)).map_err(|problems| {
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
        let Some(kept) = self.document().parameters.iter().find(|known| known.id == parameter) else {
            return Err(SheetEditError::Missing(format!(
                "the sheet has no parameter '{parameter}'"
            )));
        };
        let edit = set_parameter_edit(kept, kept.min.min(value), kept.max.max(value), value, kept.unit.clone());
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

/// Fire counts of the run of the sheet: the number of cells or agents each change line applied to in one tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineFires {
    /// Tick the counted step reached.
    pub tick: u64,
    /// Number of cells or agents each counted line applied to in that tick, by line id.
    pub counts: BTreeMap<String, u64>,
}

/// Returns the fire counts of `sheet` in `snapshot`, the latest snapshot of the run of `loaded`.
///
/// The counts are `None` before the run's first counted step, for counts of another number of probes than the sheet's,
/// and while the run is not the sheet as it now lowers (its code differs, as after an edit before the preview builds
/// again). The code identity compared is the one [`SheetEditor`] keeps from its last edit.
pub fn line_fires(sheet: &SheetEditor, loaded: Option<&ModelEntry>, snapshot: Option<&Snapshot>) -> Option<LineFires> {
    let fires = &snapshot?.fire_counts;
    let file = sheet.file()?;
    let lines = file.fire_lines();
    if fires.tick == 0 || fires.is_empty() || fires.counts.len() != lines.probes().len() {
        return None;
    }
    if loaded?.source().code() != Some(sheet.code.as_ref()?) {
        return None;
    }
    let counts = lines.counts_by_line(&fires.counts);
    (!counts.is_empty()).then_some(LineFires {
        tick: fires.tick,
        counts,
    })
}

/// Things the sheet editor shows besides the sheet, for one frame.
#[derive(Debug)]
pub struct SheetShown<'a> {
    /// Whether the preview has passed tick 0 or a button was pressed in it.
    pub past_start: bool,
    /// Values the world tries for the sheet's parameters.
    pub trials: &'a [sheet_pane::SheetTrial],
    /// Whether the New model dialog is closed. Undo and Redo then take their keys while no modal of the app is open
    /// and no text field holds the focus.
    pub keys: bool,
    /// Reason the sheet does not convert, `None` for a sheet that converts.
    pub refusal: Option<ConvertRefusal>,
    /// Fire counts of the run of the sheet on its latest snapshot, `None` while the run counts none.
    pub fires: Option<&'a LineFires>,
    /// Layout the dock shows. The Sheet pane keeps a width of its own in each layout.
    pub layout: Layout,
}

/// Request of the sheet editor that the Editor runs once the tab has drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetRequest {
    /// Opens the conversion dialog.
    Convert,
    /// Opens the node graph copy converted from the sheet.
    OpenCopy,
}

/// Draws the sheet editor: the Sheet pane on the left and the Card pane beside it, each scrolling on its own, and
/// returns the request of a pressed control.
///
/// Each pane is a group named by its heading.
pub fn sheet_ui(ui: &mut egui::Ui, sheet: &mut SheetEditor, shown: &SheetShown<'_>) -> Option<SheetRequest> {
    let past_start = shown.past_start;
    let trials = shown.trials;
    let keys = shown.keys;
    let mut request = None;
    // egui reports the top modal layer of the previous frame. Every modal stays open past the frame that opens it.
    let modal_open = ui.ctx().memory(|memory| memory.top_modal_layer().is_some());
    if keys && !modal_open && !sheet.dialog_open() && !ui.ctx().text_edit_focused() {
        let (undo, redo) = ui.ctx().input_mut(undo_redo_keys);
        if redo {
            sheet.redo(past_start);
        }
        if undo {
            sheet.undo(past_start);
        }
    }
    let width = ui.available_width();
    let pane_id = sheet_pane_id(shown.layout);
    let set = sheet_pane_width(ui.ctx(), pane_id, width);
    let ctx = ui.ctx().clone();
    let pane = focus::without_focus_stop(&ctx, focus::panel_divider(pane_id), || {
        Panel::left(pane_id)
            .resizable(true)
            .default_size(set)
            .min_size(PANE_MIN_WIDTH)
            .max_size((width - PANE_MIN_WIDTH).max(PANE_MIN_WIDTH))
            .frame(Frame::NONE.inner_margin(Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                ui.heading(SHEET_PANE);
                // The pane keeps the width its divider gives it, and clips anything wider at its edge.
                let rect = ui.available_rect_before_wrap();
                let mut pane = ui.new_child(UiBuilder::new().max_rect(rect).layout(*ui.layout()));
                pane.shrink_clip_rect(rect);
                ScrollArea::vertical()
                    .id_salt("henad_sheet_pane_scroll")
                    .auto_shrink([false; 2])
                    .show(&mut pane, |ui| sheet_pane::sheet_pane(ui, sheet, past_start, trials));
                ui.advance_cursor_after_rect(rect);
                pane_group(ui, SHEET_PANE);
            })
    });
    keep_sheet_pane_share(ui.ctx(), pane_id, width, set, pane.response.rect.width());
    CentralPanel::no_frame().show(ui, |ui| {
        Frame::NONE.inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(CARD_PANE);
                ui.add_space(8.0);
                // Lines and Diagram read as the two segments of one toggle, each on the keyboard path.
                ui.spacing_mut().item_spacing.x = 4.0;
                let lines = widgets::toggle_label(ui, LINES_VIEW, !sheet.diagram);
                widgets::segment_rule(ui);
                let toggle = widgets::toggle_label(ui, DIAGRAM, sheet.diagram).on_hover_or_focus_text(DIAGRAM_TOOLTIP);
                if toggle.clicked() || (lines.clicked() && sheet.diagram) {
                    sheet.diagram = !sheet.diagram;
                    sheet.card.close();
                }
            });
            ScrollArea::vertical()
                .id_salt("henad_card_pane_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if sheet.diagram {
                        if diagram::diagram_ui(ui, sheet.document(), &sheet.report, shown.refusal) {
                            request = Some(SheetRequest::Convert);
                        }
                    } else {
                        card_pane::card_pane(ui, sheet, past_start, trials, shown.fires);
                    }
                });
            pane_group(ui, CARD_PANE);
        });
    });
    request
}

/// Things the name row shows besides the sheet, for one frame.
#[derive(Debug, Clone, Copy)]
pub struct NameRow<'a> {
    /// Whether the preview has passed tick 0 or a button was pressed in it.
    pub past_start: bool,
    /// Reason the sheet does not convert, `None` for a sheet that converts.
    pub refusal: Option<ConvertRefusal>,
    /// Whether the sheet holds edits since it was opened, made or last saved.
    pub unsaved: bool,
    /// Name of the file the sheet was opened from or saved to, `None` for a sheet with no file.
    pub file_name: Option<&'a str>,
    /// Name of the node graph copy converted from the sheet in this session, `None` for none.
    pub copy: Option<&'a str>,
}

/// Draws the name row of the Editor's header for `sheet`, and returns the request of a pressed control.
///
/// The row holds the model's name in the body colour, a button that turns into a field named "Model name", the mark of
/// unsaved changes, the file's name in the weak colour, then Undo, Redo and the menu of the model's actions, which holds
/// Convert to node graph. The line that opens the sheet's copy follows under the row.
///
/// In the field, Enter or a click elsewhere keeps the name and Escape puts the old name back. An empty name keeps the
/// old one. Convert to node graph is disabled with the tooltip of the refusal while the sheet does not convert.
pub fn name_row(ui: &mut egui::Ui, sheet: &mut SheetEditor, shown: &NameRow<'_>) -> Option<SheetRequest> {
    let mut request = None;
    ui.horizontal(|ui| name_row_ui(ui, sheet, shown, &mut request));
    if let Some(copy) = shown.copy
        && ui.link(converted_link(copy)).clicked()
    {
        request = Some(SheetRequest::OpenCopy);
    }
    request
}

/// Draws the controls of the name row, as [`name_row`] describes them.
fn name_row_ui(ui: &mut egui::Ui, sheet: &mut SheetEditor, shown: &NameRow<'_>, request: &mut Option<SheetRequest>) {
    let past_start = shown.past_start;
    let refusal = shown.refusal;
    let name = sheet.document().sheet.name.clone();
    let trailing_id = ui.id().with("henad_name_row_trailing");
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
        let trailing = ui.data(|data| data.get_temp::<f32>(trailing_id)).unwrap_or_default();
        let button = name_button_ui(ui, &name, trailing);
        if button.clicked() {
            sheet.renaming = Some(name);
        }
    }
    let after_name = ui.cursor().left();
    if shown.unsaved {
        widgets::unsaved_mark(ui);
    }
    if let Some(file_name) = shown.file_name {
        ui.weak(file_name);
    }
    let height = ui.spacing().interact_size.y;
    widgets::vertical_hairline(ui, height);
    let undo = add_button(
        ui,
        sheet.can_undo(),
        Button::new(format!("{MDI_UNDO} {UNDO}")).frame_when_inactive(false),
        UNDO,
    )
    .on_hover_or_focus_text(UNDO_TOOLTIP);
    if undo.clicked() {
        sheet.undo(past_start);
    }
    let redo = add_button(
        ui,
        sheet.can_redo(),
        Button::new(format!("{MDI_REDO} {REDO}")).frame_when_inactive(false),
        REDO,
    )
    .on_hover_or_focus_text(REDO_TOOLTIP);
    if redo.clicked() {
        sheet.redo(past_start);
    }
    widgets::vertical_hairline(ui, height);
    let dots = Button::new(MDI_DOTS_HORIZONTAL).frame_when_inactive(false);
    let (menu, _) = egui::containers::menu::MenuButton::from_button(dots).ui(ui, |ui| {
        let convert = add_button(ui, refusal.is_none(), Button::new(CONVERT_HEADING), CONVERT_HEADING);
        let convert = match refusal {
            Some(refusal) => convert.on_disabled_hover_text(refusal.tooltip()),
            None => convert,
        };
        if convert.clicked() {
            *request = Some(SheetRequest::Convert);
            ui.close();
        }
    });
    menu.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, MODEL_ACTIONS));
    let trailing = ui.cursor().left() - after_name;
    ui.data_mut(|data| data.insert_temp(trailing_id, trailing));
}

/// Adds the button that shows the model's name `name` and opens the field that renames it, and returns its response.
///
/// The name leaves `trailing` points of the row, the width the rest of the row took on the frame before, and ends in an
/// ellipsis past them. Its tooltip then names it in full.
fn name_button_ui(ui: &mut egui::Ui, name: &str, trailing: f32) -> egui::Response {
    let room = (ui.available_width() - trailing).max(NAME_MIN_WIDTH);
    let full = ui
        .painter()
        .layout_no_wrap(
            name.to_owned(),
            TextStyle::Button.resolve(ui.style()),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x;
    let tooltip = if full > room {
        rename_model_named(name)
    } else {
        RENAME_MODEL_TOOLTIP.to_owned()
    };
    let words = RichText::new(name).color(ui.visuals().text_color());
    ui.scope(|ui| {
        ui.set_max_width(room);
        add_button(
            ui,
            true,
            Button::new(words).frame_when_inactive(false).truncate(),
            &model_name_button(name),
        )
    })
    .inner
    .on_hover_or_focus_text(&tooltip)
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
    all_properties(document)
        .map(|property| (property.id.clone(), property.starts_as.clone()))
        .collect()
}

/// Returns the command that gives `parameter` the range `min` to `max`, the value `default` and the unit `unit`,
/// keeping its step and its meaning.
fn set_parameter_edit(parameter: &SheetParameter, min: f64, max: f64, default: f64, unit: Option<String>) -> SheetEdit {
    SheetEdit::SetParameter {
        parameter: parameter.id.clone(),
        min,
        max,
        default,
        step: parameter.step,
        unit,
        means: parameter.means.clone(),
    }
}

// Kinds of thing and their properties

/// Returns the properties of the kind of thing `thing`: `cells`, `ground` or an agent kind's id. A kind of thing the
/// sheet lacks has none.
fn properties_of<'a>(document: &'a SheetDocument, thing: &str) -> &'a [SheetProperty] {
    match thing {
        CELLS_ID => document.cells.as_deref().unwrap_or_default(),
        GROUND_ID => document
            .ground
            .as_ref()
            .map(|ground| ground.properties.as_slice())
            .unwrap_or_default(),
        _ => document
            .agents
            .iter()
            .find(|kind| kind.id == thing)
            .map(|kind| kind.properties.as_slice())
            .unwrap_or_default(),
    }
}

/// Returns every property of `document`: the cells', each kind of agent's, then the ground's.
fn all_properties(document: &SheetDocument) -> impl Iterator<Item = &SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
}

/// Returns the property `id` of any kind of thing of `document`.
fn find_property<'a>(document: &'a SheetDocument, id: &str) -> Option<&'a SheetProperty> {
    all_properties(document).find(|property| property.id == id)
}

/// Returns whether a card over the kind of thing `every` runs over agents, any kind but the cells.
fn over_agents(every: &str) -> bool {
    every != CELLS_ID
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
        node.set_bounds(accessible_bounds(rect));
    });
}

/// Returns `rect` as the bounds of an accessible node.
pub fn accessible_bounds(rect: egui::Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect {
        x0: rect.min.x.into(),
        y0: rect.min.y.into(),
        x1: rect.max.x.into(),
        y1: rect.max.y.into(),
    }
}
