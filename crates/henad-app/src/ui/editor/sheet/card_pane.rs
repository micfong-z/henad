//! The Card pane: each card of the sheet under its heading, with its lines numbered from 1 in display order, written
//! and edited on the card.
//!
//! A line reads as its sentence from [`henad_graph::sheet::sentence`], drawn part by part: words, a chosen value as a
//! *chip* coloured by what it refers to, an empty slot as a dashed *blank*, a select inside the line, the grey
//! "always" and the muted "otherwise" words. A line inside a When is indented one step per level, and the notes of
//! the sheet's check about a line sit under it.
//!
//! A click on a blank or a chip opens its *picker* ([`super::picker`]), and a choice runs one edit of the sheet. A
//! select lists its options, the grey "always" and the arrow after a condition word open "Choose when", and the add
//! buttons under a card add a line at its end. Each line has one menu, "Line 2 actions", always drawn, which renames,
//! moves, draws new numbers for and deletes the line.
//!
//! A *block* is a When or Repeat line and the lines inside it. Each block ends with a row "+ Add inside line 4" whose
//! menu adds a line of any kind at the end of the block, inner blocks closing before outer ones. A move follows the
//! outline: the first line of a block moves up out of it, a line beside a block moves into it, and a block moves with
//! its lines.
//!
//! *Focus rules.* Only a new line's first blank opens by itself. After a choice, the focus moves to the next blank or
//! select of the line without opening it. Two kinds of blank open as the focus reaches them: an empty state blank (a
//! Count's state after its property, a Become's old state after its new state) and the empty upper number of a range
//! after its lower number.
//!
//! A card over agents offers the agent lines of round 1 under the same add buttons, and "+ Repeat..." after them: looks
//! at the agents within a distance with their summaries, picks and looks at cells of the ground, random numbers and
//! directions, Choose and Depending on, Steer, Keep between, Move, Leave and Record. A summary of a look at agents
//! names itself after what it sums up, and "+" after the summaries adds another. A problem the lowering finds in a
//! line, such as a Set of a property another line steers, shows under that line.
//!
//! Each line is a group named "Line 2: " and its sentence, and each control inside it is named by its visible words,
//! a blank or a select by what it wants and where it is ("Line 2: choose the old state"). Hovering or focusing
//! "otherwise" or a result chip highlights the line it names.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use egui::accesskit::Role;
use egui::{
    Align, Button, Color32, CornerRadius, Frame, Id, Key, Label, Layout, Margin, Popup, PopupCloseBehavior, Pos2, Rect,
    Response, RichText, Sense, Shape, Stroke, TextEdit, TextStyle, Ui, UiBuilder, WidgetInfo, WidgetType, vec2,
};
use henad_core::diagnostic::DiagnosticSeverity;
use henad_graph::sheet::check::{SheetDiagnostic, SheetReport, holds_draw};
use henad_graph::sheet::document::{
    Around, CardRuns, CellWhich, ChooseRow, CompareTest, LineKind, LineMode, LookSummary, MadeOf, PickRule,
    PropertyType, SheetCard, SheetChip, SheetDocument, SheetLine, SheetProperty, SheetValue, Slot, SummaryKind,
};
use henad_graph::sheet::edit::{LineField, MoveDirection, SheetBuilder, SheetEdit, SheetEditError};
use henad_graph::sheet::rates::{chance_note, conversion, count_as_draw, grow_count, parameter_defaults, rate_note};
use henad_graph::sheet::relations::{CardRelations, RelationNote, RelationNoteKind};
use henad_graph::sheet::sentence::{
    ADD_AT_END_NOTE, ALWAYS, ALWAYS_TOOLTIP, ANY_STATE_ID, AddButton, AddItem, BLANK_MODEL_CARD, BLANK_TEXT,
    BUTTON_CARD_EMPTY, Blank, CONDITION_ARROW_TOOLTIP, ChipKind, DELETE_WHEN_TOOLTIP, DRAW_NEW_NUMBERS, DrawKind,
    FOR_EACH_BLANK_WORDS, FOR_EACH_WORDS, FROM_TOOLTIP, FROM_WORD, FieldPlace, LAW, LineSelect, ONE_SIDED_RANGE,
    PartPlace, PlaceTarget, PlacedPart, REMOVE_THIS_WHEN, SHARED_DRAW_GROUP, SentenceChip, SentencePart, Sentences,
    UNIT_PLACEHOLDER, add_inside_label, add_inside_name, add_value_name, always_name, card_header,
    condition_arrow_name, count_used_as_draw, delete_line, for_each_change_name, for_each_optional_name,
    for_each_optional_tooltip, for_each_words, line_actions_name, line_count, line_list_name, line_name, line_prefix,
    line_word, move_into_label, move_into_name, move_line_down, move_line_up, operation_words, otherwise_notice,
    otherwise_tooltip, otherwise_words, remove_when_name, rename_field_name, rename_line, result_below_tooltip,
    result_inside_tooltip, result_name_taken, sentence_text, use_for_each_label, use_for_each_name, use_instead_label,
    use_instead_name, uses_result_below, uses_result_inside,
};
use henad_graph::sheet::sentence::{
    CHANGE_TO_CHANCE, CHANGE_TO_RATE, COMPETING_WORD, RANDOM_VALUE_GROUP, add_choose_row_name, add_for_each_label,
    add_for_each_name, add_summary_name, comparison_words, competing_parts, competing_tooltip, fire_count,
    fire_count_description, include_me_tool, leave_me_out_tool, long_tick_note, rate_grow_prompt, tick_set_notice,
};

use super::help::{Help, help_button};
use super::picker::{
    CalcDraft, CardPicker, Choice, DraftUpdate, LineContext, SelectOption, apply_choice, around_options,
    calculate_edit, choose_when, comparison_options, is_result_line, list_len, look_summary_options, looked_at_picker,
    mode_options, operation_options, phrase_of, pick_options, picker_for, read_property, slot_at, summary_options,
    summary_picker, typed_text, unit_options, value_at, which_options,
};
use super::sheet_pane::SheetTrial;
use super::widgets::{
    CHIP_PADDING, DialogOutcome, NewParameterDialog, NumberUse, allocate_control, blank_shapes, blank_ui, chip_with_id,
    message_ui, new_parameter_modal, set_name,
};
use super::{LineFires, SheetEditor, accessible_group};
use crate::icons::material_design_icons::{MDI_CHEVRON_DOWN, MDI_DOTS_HORIZONTAL, MDI_INFORMATION};
use crate::ui::mcs;
use crate::ui::sweep::layout::add_button;

/// Indent in points of a line for each block around it.
const INDENT: f32 = 20.0;

/// Width in points of the column of line numbers.
const NUMBER_WIDTH: f32 = 22.0;

/// Width in points of the column at the right of a card that runs each tick, which holds the fire count of each line.
const COUNT_WIDTH: f32 = 72.0;

/// Distance in points from the left edge of the rows to the bracket of a competing group, inside the pane's margin.
const BRACKET_OFFSET: f32 = 5.0;

/// Width in points of the field that renames a result.
const RENAME_WIDTH: f32 = 200.0;

// State between frames

/// State of the Card pane between frames: the id its controls are keyed under, the rename field, the values of empty
/// Calculate lines, the lines whose range is being filled, the New parameter dialog and the focus to move.
#[derive(Debug)]
pub struct CardState {
    /// Id the pane's controls are keyed under, one for each sheet opened.
    widgets: Id,
    /// Rename field of a result line, `None` while none is open.
    renaming: Option<Renaming>,
    /// Values chosen for each Calculate line whose formula is not written yet, by line id.
    drafts: BTreeMap<String, CalcDraft>,
    /// Ids of the lines whose "is less than ... or more than ..." is being filled. The one-side offer waits on them.
    filling: BTreeSet<String>,
    /// New parameter dialog with the place it fills and whether it has been drawn, `None` while it is closed.
    new_parameter: Option<(PlaceRef, NewParameterDialog, bool)>,
    /// Control whose picker opens on the next frame, `None` for none.
    open_next: Option<Id>,
    /// Control that takes the focus on the next frame, `None` for none.
    focus_next: Option<Id>,
    /// Reason the last edit of the card was refused, `None` while none shows.
    refusal: Option<String>,
    /// Ids of the lines whose "otherwise" has been explained in this session, `None` before the pane is first drawn.
    /// A new or opened model counts its "otherwise" words as explained.
    explained: Option<BTreeSet<String>>,
    /// Notice that a line now starts with "otherwise", `None` while none shows.
    otherwise_notice: Option<String>,
    /// Notice that a rate's unit set the length of one tick, `None` while none shows.
    tick_notice: Option<String>,
}

impl CardState {
    /// Returns the state of a new pane, its controls keyed by `identity`, the number of the load that opened the sheet.
    pub fn new(identity: u64) -> Self {
        Self {
            widgets: Id::new(("henad_card_pane", identity)),
            renaming: None,
            drafts: BTreeMap::new(),
            filling: BTreeSet::new(),
            new_parameter: None,
            open_next: None,
            focus_next: None,
            refusal: None,
            explained: None,
            otherwise_notice: None,
            tick_notice: None,
        }
    }

    /// Closes the rename field and the New parameter dialog, drops every draft and message, and ends every range being
    /// filled, as an undo or a redo does.
    pub fn close(&mut self) {
        self.renaming = None;
        self.drafts.clear();
        self.filling.clear();
        self.new_parameter = None;
        self.open_next = None;
        self.focus_next = None;
        self.refusal = None;
        self.otherwise_notice = None;
        self.tick_notice = None;
    }

    /// Returns whether the New parameter dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.new_parameter.is_some()
    }

    /// Drops the notice that a rate's unit set the length of one tick, as an edit of that length on the Sheet pane
    /// does.
    pub fn drop_tick_notice(&mut self) {
        self.tick_notice = None;
    }

    /// Returns the id of the control of line `line` at `target`. `tag` tells apart two controls at one target, as the
    /// words "for each" and their count.
    fn control_id(&self, line: &str, target: &PlaceTarget, tag: u8) -> Id {
        self.widgets.with((line, target, tag))
    }
}

/// The field that renames a result line.
#[derive(Debug, Clone)]
struct Renaming {
    /// Id of the line.
    line: String,
    /// Text in the field.
    text: String,
    /// Message under the field after a refused name, `None` while none shows.
    message: Option<String>,
    /// Whether the field has taken the focus once.
    focused: bool,
}

/// A place of a line: the line's id and the place in it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaceRef {
    /// Id of the line.
    line: String,
    /// The place.
    place: PartPlace,
}

// Edits

/// What happens to the focus after an edit of the card.
#[derive(Debug, Clone)]
enum After {
    /// The focus stays where it is.
    Stay,
    /// The focus moves past the control at `target` of line `line`, and opens it when the rules say so.
    Next {
        /// Id of the line.
        line: String,
        /// Target of the control the edit came from.
        target: PlaceTarget,
        /// What the place wanted.
        want: Option<Blank>,
    },
    /// The new line's first blank opens. The line's id is the last id the edit made.
    NewLine,
    /// The picker at `target` of line `line` opens.
    Open {
        /// Id of the line.
        line: String,
        /// Target of the control.
        target: PlaceTarget,
    },
}

/// Draft of a Calculate line that the commands of an edit work out while they run.
type DraftCell = Rc<RefCell<Option<DraftUpdate>>>;

/// The commands of one edit of the card, as a closure that returns the ids of what they made.
type CardRun = Box<dyn FnOnce(&mut SheetBuilder) -> Result<Vec<String>, SheetEditError>>;

/// An edit collected while the card is drawn, run once the card is drawn.
struct CardEdit {
    /// The commands.
    run: CardRun,
    /// What happens to the focus after it.
    after: After,
    /// Whether a refusal belongs under the rename field.
    rename: bool,
    /// Draft of a Calculate line that the edit leaves, `None` for an edit that leaves the drafts alone.
    draft: Option<DraftUpdate>,
    /// Draft that the commands work out while they run, read once they ran.
    late_draft: Option<DraftCell>,
}

impl CardEdit {
    /// Returns the edit that runs `edits` in order, then moves the focus as `after` says.
    fn commands(edits: Vec<SheetEdit>, after: After) -> Self {
        Self {
            run: Box::new(move |builder| {
                let mut made = Vec::new();
                for edit in edits {
                    made.extend(builder.apply(edit)?);
                }
                Ok(made)
            }),
            after,
            rename: false,
            draft: None,
            late_draft: None,
        }
    }
}

/// Draws every card of `sheet` and runs the edit made on it. `past_start` is whether the preview has passed tick 0 or
/// a button was pressed in it, and `trials` holds the values the world tries for the sheet's parameters.
///
/// The notes under rate and chance lines read the parameters' defaults, with the values the world tries in their
/// place while a run of the sheet is loaded.
pub fn card_pane(
    ui: &mut Ui,
    sheet: &mut SheetEditor,
    past_start: bool,
    trials: &[SheetTrial],
    fires: Option<&LineFires>,
) {
    let mut pending = None;
    {
        let document = sheet.builder.document();
        let state = &mut sheet.card;
        for notice in [&state.tick_notice, &state.otherwise_notice].into_iter().flatten() {
            ui.add(Label::new(format!("{MDI_INFORMATION} {notice}")).wrap());
        }
        let mut values = parameter_defaults(document);
        for trial in trials {
            if let Some(value) = values.get_mut(&trial.parameter) {
                *value = trial.value;
            }
        }
        let shown = CardsShown {
            report: &sheet.report,
            values: &values,
            fires,
            counted: &sheet.counted,
        };
        cards_ui(ui, document, &shown, state, &mut pending);
        if let Some(refusal) = &state.refusal {
            message_ui(ui, refusal);
        }
        new_parameter_dialog(ui, document, state, &mut pending);
    }
    if let Some(edit) = pending {
        run_card_edit(sheet, past_start, edit);
    }
}

/// Runs `edit` on `sheet`, then moves the focus and keeps the drafts and marks it leaves.
fn run_card_edit(sheet: &mut SheetEditor, past_start: bool, edit: CardEdit) {
    let CardEdit {
        run,
        after,
        rename,
        draft,
        late_draft,
    } = edit;
    let mut made = Vec::new();
    let timed = sheet.document().time.is_some();
    let result = sheet.edit(past_start, false, |builder| {
        made = run(builder)?;
        Ok(())
    });
    let time = sheet.document().time;
    let state = &mut sheet.card;
    match result {
        Ok(()) => {
            state.refusal = None;
            state.otherwise_notice = None;
            // A rate's unit chosen while one tick has no length sets one tick to 1 of that unit.
            state.tick_notice = time.filter(|_| !timed).map(|time| tick_set_notice(time.tick_unit));
            if rename {
                state.renaming = None;
            }
        }
        Err(error) => {
            match (&mut state.renaming, &error) {
                (Some(renaming), SheetEditError::Taken(_)) if rename => {
                    renaming.message = Some(result_name_taken(renaming.text.trim()));
                    // The field keeps its text and takes the focus again for another name.
                    renaming.focused = false;
                }
                _ => state.refusal = Some(error.to_string()),
            }
            return;
        }
    }
    let late = late_draft.and_then(|cell| cell.borrow_mut().take());
    for (line, draft) in draft.into_iter().chain(late) {
        match draft {
            Some(draft) => state.drafts.insert(line, draft),
            None => state.drafts.remove(&line),
        };
    }
    let document = sheet.builder.document();
    let state = &mut sheet.card;
    match after {
        After::Stay => {}
        After::NewLine => {
            if let Some(line) = made.last() {
                let parts = Sentences::new(document).placed_parts(line, &[]);
                let first = parts.iter().find_map(|placed| match (&placed.part, &placed.place) {
                    (SentencePart::Blank(_), Some(place)) => Some(place.target.clone()),
                    _ => None,
                });
                if let Some(target) = first {
                    state.open_next = Some(state.control_id(line, &target, 0));
                }
            }
        }
        After::Open { line, target } => state.open_next = Some(state.control_id(&line, &target, 0)),
        After::Next { line, target, want } => {
            update_filling(document, state, &line, want);
            let parts = Sentences::new(document).placed_parts(&line, &[]);
            if let Some((next, opens)) = next_control(&parts, &target, want) {
                let id = state.control_id(&line, &next, 0);
                if opens {
                    state.open_next = Some(id);
                } else {
                    state.focus_next = Some(id);
                }
            }
        }
    }
}

/// Marks line `line` as being filled when an edit of one side of its range leaves the other side empty, and ends the
/// mark once both sides are filled. `want` is what the edited place wanted.
fn update_filling(document: &SheetDocument, state: &mut CardState, line: &str, want: Option<Blank>) {
    let Some(sides) = document.line(line).and_then(|(_, line)| range_sides(line)) else {
        state.filling.remove(line);
        return;
    };
    match sides {
        (true, true) | (false, false) => {
            state.filling.remove(line);
        }
        _ if matches!(want, Some(Blank::LowerNumber | Blank::UpperNumber)) => {
            state.filling.insert(line.to_owned());
        }
        _ => {}
    }
}

/// Returns whether each side of the range of `line`'s own condition is filled, `None` for a line whose condition is no
/// "is less than ... or more than ...".
fn range_sides(line: &SheetLine) -> Option<(bool, bool)> {
    match line.condition.value()? {
        SheetValue::Phrase(phrase) if phrase.kind == LineKind::Compare && phrase.test == Some(CompareTest::Outside) => {
            Some((!phrase.low.is_blank(), !phrase.high.is_blank()))
        }
        _ => None,
    }
}

/// Returns the control after the control at `target` among `parts`, with whether its picker opens.
///
/// A target no longer there, as a condition whose form changed, is found by the slot path it starts. The next control
/// opens when it is an empty state blank, or the empty upper number after a lower number.
fn next_control(parts: &[PlacedPart], target: &PlaceTarget, want: Option<Blank>) -> Option<(PlaceTarget, bool)> {
    let controls: Vec<(&SentencePart, &PartPlace)> = parts
        .iter()
        .filter_map(|placed| placed.place.as_ref().map(|place| (&placed.part, place)))
        .filter(|(part, _)| !matches!(part, SentencePart::ForEachBlank(_) | SentencePart::ForEach(_)))
        .collect();
    let position = controls.iter().position(|(_, place)| place.target == *target);
    let next = match position {
        Some(position) => controls.get(position + 1),
        // The form of the place changed: its first control takes the focus.
        None => controls.iter().find(|(_, place)| starts_with(&place.target, target)),
    }?;
    let (part, place) = next;
    let opens = match part {
        SentencePart::Blank(Blank::State | Blank::OldState) => true,
        SentencePart::Blank(Blank::UpperNumber) => want == Some(Blank::LowerNumber),
        _ => false,
    };
    Some((place.target.clone(), opens))
}

/// Returns whether `target` lies at or under the slot `outer` names.
fn starts_with(target: &PlaceTarget, outer: &PlaceTarget) -> bool {
    let PlaceTarget::Slot(outer) = outer else {
        return false;
    };
    let path = match target {
        PlaceTarget::Slot(path) | PlaceTarget::ListItem { slot: path, .. } => path,
        PlaceTarget::Field { phrase, .. } => phrase,
        _ => return false,
    };
    path == outer || path.starts_with(&format!("{outer}."))
}

// The cards

/// Draws every card of `document` in file order, with the notes of `report` under their lines and the add buttons
/// under each card. The notes read the parameter values `values`, by id.
fn cards_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    shown: &CardsShown<'_>,
    state: &mut CardState,
    pending: &mut Option<CardEdit>,
) {
    let report = shown.report;
    let values = shown.values;
    let sentences = Sentences::new(document);
    let highlight_id = ui.id().with("henad_card_highlight");
    let highlight: Vec<String> = ui.data(|data| data.get_temp(highlight_id)).unwrap_or_default();
    let mut hovered: Vec<String> = Vec::new();
    let every = match document.world.made_of {
        MadeOf::Agents => document.agents.first().map(|kind| kind.id.as_str()),
        _ => Some("cells"),
    };
    let has_tick_card = |every: &str| {
        document
            .cards
            .iter()
            .any(|card| card.every == every && card.runs == CardRuns::EachTick)
    };
    if let Some(every) = every
        && !has_tick_card(every)
    {
        // A blank model holds no card. Its first line adds the card that runs each tick.
        ui.label(RichText::new(card_header(&sentences.thing_word(every))).strong());
        ui.weak(BLANK_MODEL_CARD);
        add_buttons_ui(ui, document, state, None, every, pending);
        ui.add_space(16.0);
    }
    let mut overridden: Vec<(String, String)> = Vec::new();
    for (index, card) in document.cards.iter().enumerate() {
        if index > 0 {
            ui.add_space(16.0);
        }
        let lines = card_lines(document, &sentences, report, index);
        overridden.extend(
            lines
                .iter()
                .filter(|line| !line.otherwise.is_empty())
                .map(|line| (line.id.clone(), line.otherwise_notice.clone())),
        );
        let drawing = CardDrawing {
            document,
            sentences: &sentences,
            report,
            card,
            values,
            highlight: &highlight,
            fires: shown.fires.filter(|_| card.runs == CardRuns::EachTick),
            counted: card.runs == CardRuns::EachTick && card.line_tables.keys().any(|id| shown.counted.contains(id)),
        };
        card_ui(ui, &drawing, &lines, state, &mut hovered, pending);
    }
    explain_otherwise(ui.ctx(), state, overridden);
    if hovered != highlight {
        ui.data_mut(|data| data.insert_temp(highlight_id, hovered));
        ui.ctx().request_repaint();
    }
}

/// Shows the notice that a line now starts with "otherwise" the first time a line of `overridden` gets it in the
/// session, where `overridden` holds each line that reads "otherwise" with its notice.
///
/// On the pane's first frame every such line counts as explained, as a new or opened model's lines do. The notice is
/// drawn above the cards, so a new one requests another frame of `ctx`.
fn explain_otherwise(ctx: &egui::Context, state: &mut CardState, overridden: Vec<(String, String)>) {
    let Some(explained) = &mut state.explained else {
        state.explained = Some(overridden.into_iter().map(|(line, _)| line).collect());
        return;
    };
    for (line, notice) in overridden {
        if explained.insert(line) {
            state.otherwise_notice = Some(notice);
            ctx.request_repaint();
        }
    }
}

/// Values the cards read besides the sheet while they are drawn.
struct CardsShown<'a> {
    /// Report of the sheet's check.
    report: &'a SheetReport,
    /// Value of each parameter the notes read, by id.
    values: &'a BTreeMap<String, f64>,
    /// Fire counts of the run of the sheet, `None` while the run counts none.
    fires: Option<&'a LineFires>,
    /// Ids of the lines with a fire count in the last lowering of the sheet.
    counted: &'a BTreeSet<String>,
}

/// What every line of one card reads while the card is drawn.
struct CardDrawing<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// Writer of the sheet's sentences.
    sentences: &'a Sentences<'a>,
    /// Report of the sheet's check.
    report: &'a SheetReport,
    /// The card.
    card: &'a SheetCard,
    /// Value of each parameter the notes read, by id.
    values: &'a BTreeMap<String, f64>,
    /// Ids of the lines to highlight.
    highlight: &'a [String],
    /// Fire counts of the run of the sheet on a card that runs each tick, `None` on any other card or while the run
    /// counts none.
    fires: Option<&'a LineFires>,
    /// Whether the card runs each tick and holds a line with a fire count. The card then keeps a column at its right
    /// for the counts, whether or not the run has counted a tick.
    counted: bool,
}

/// Draws the heading of the card of `drawing`, the law under a card that runs every tick, the numbered `lines` and the
/// add buttons.
///
/// The ids of the lines that a hovered or focused control names are added to `hovered`.
fn card_ui(
    ui: &mut Ui,
    drawing: &CardDrawing<'_>,
    lines: &[CardLine],
    state: &mut CardState,
    hovered: &mut Vec<String>,
    pending: &mut Option<CardEdit>,
) {
    let card = drawing.card;
    let each_tick = card.runs == CardRuns::EachTick;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(drawing.sentences.card_heading(card)).strong());
        ui.weak(line_count(lines.len()));
        if each_tick {
            help_button(ui, Help::Lines);
        }
    });
    if each_tick {
        Frame::NONE
            .fill(mcs::GRAY_900)
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(Label::new(RichText::new(LAW).weak()).wrap());
            });
    }
    ui.add_space(4.0);
    if lines.is_empty() {
        ui.weak(if each_tick { BLANK_MODEL_CARD } else { BUTTON_CARD_EMPTY });
    }
    let list_name = line_list_name(button_of(drawing.document, card));
    let relations = CardRelations::new(drawing.document, card);
    let report = drawing.report;
    let notes = relations.notes(drawing.sentences, |line| !report.is_skipped(line));
    let closing = closing_blocks(&relations, lines);
    let list = ui.scope(|ui| {
        let mut rows = Vec::with_capacity(lines.len());
        for (index, line) in lines.iter().enumerate() {
            rows.push(line_ui(ui, drawing, &relations, &notes, line, state, hovered, pending));
            for block in closing.get(&index).into_iter().flatten() {
                add_inside_ui(ui, drawing, block, state, pending);
            }
        }
        for bracket in brackets(&relations, lines) {
            bracket_ui(ui, &rows, &bracket);
        }
        ui.unique_id()
    });
    accessible_group(ui.ctx(), list.inner, Role::List, &list_name, list.response.rect);
    if card.line_tables.values().any(|line| is_block(line.kind)) {
        ui.add_space(4.0);
        ui.weak(ADD_AT_END_NOTE);
    }
    add_buttons_ui(ui, drawing.document, state, Some(card), &card.every, pending);
}

/// Returns whether a line of `kind` holds lines: a When or a Repeat.
fn is_block(kind: LineKind) -> bool {
    matches!(kind, LineKind::When | LineKind::Repeat)
}

/// Returns the competing groups of the card of `relations` as the brackets that join them: each the indices in
/// `lines` of the group's lines, top to bottom.
fn brackets(relations: &CardRelations<'_>, lines: &[CardLine]) -> Vec<Vec<usize>> {
    relations
        .competing_groups()
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .filter_map(|id| lines.iter().position(|line| line.id == id))
                .collect::<Vec<_>>()
        })
        .filter(|indices| indices.len() > 1)
        .collect()
}

/// Returns the numbers of the lines each bracket of card `card_index` of `document` joins, one list per competing
/// group.
#[cfg(test)]
pub fn competing_brackets(document: &SheetDocument, card_index: usize) -> Vec<Vec<usize>> {
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    let sentences = Sentences::new(document);
    let report = henad_graph::sheet::check::check(document);
    let lines = card_lines(document, &sentences, &report, card_index);
    brackets(&CardRelations::new(document, card), &lines)
        .into_iter()
        .map(|bracket| bracket.into_iter().map(|index| lines[index].number).collect())
        .collect()
}

/// Draws the thin bracket that joins the rows `rows` of the lines of `bracket`, indices in `lines`, in the margin left
/// of the rows.
///
/// A row that wraps starts its next lines at the left edge of the card, under the line numbers. The margin is the one
/// place beside the rows that no text reaches.
fn bracket_ui(ui: &Ui, rows: &[Rect], bracket: &[usize]) {
    let (Some(&first), Some(&last)) = (bracket.first(), bracket.last()) else {
        return;
    };
    let (Some(top), Some(bottom)) = (rows.get(first), rows.get(last)) else {
        return;
    };
    let x = top.left() - BRACKET_OFFSET;
    let (y0, y1) = (top.top() + 3.0, bottom.bottom() - 3.0);
    let stroke = Stroke::new(1.0, mcs::GRAY_500);
    let clip = ui.clip_rect();
    let painter = ui.painter().with_clip_rect(Rect::from_min_max(
        Pos2::new(clip.left().min(x - 1.0), clip.top()),
        clip.max,
    ));
    painter.add(Shape::line(
        vec![
            Pos2::new(x + 3.0, y0),
            Pos2::new(x, y0),
            Pos2::new(x, y1),
            Pos2::new(x + 3.0, y1),
        ],
        stroke,
    ));
}

/// A When block whose add row follows a line.
#[derive(Debug, Clone)]
struct ClosingBlock {
    /// Id of the When line.
    id: String,
    /// Number of the When line on its card.
    number: usize,
    /// Number of blocks around the lines inside the block.
    depth: usize,
}

/// Returns the When and Repeat blocks of `lines` that close after each line, by the line's index, inner blocks first.
///
/// A block closes after the last line inside it, or after its When or Repeat line while it holds none.
fn closing_blocks(relations: &CardRelations<'_>, lines: &[CardLine]) -> BTreeMap<usize, Vec<ClosingBlock>> {
    let mut closing: BTreeMap<usize, Vec<ClosingBlock>> = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        if relations.line(&line.id).is_none_or(|data| !is_block(data.kind)) {
            continue;
        }
        let last = lines
            .iter()
            .enumerate()
            .skip(index + 1)
            .take_while(|(_, inner)| relations.is_within(&inner.id, &line.id))
            .last()
            .map_or(index, |(last, _)| last);
        closing.entry(last).or_default().push(ClosingBlock {
            id: line.id.clone(),
            number: line.number,
            depth: line.depth + 1,
        });
    }
    for blocks in closing.values_mut() {
        blocks.sort_by_key(|block| std::cmp::Reverse(block.depth));
    }
    closing
}

/// Draws the row that ends `block`, "+ Add inside line 4", indented to the block's inner level. Its menu, "Add a
/// line inside line 4", lists every kind of line in groups, and an item adds its line at the end of the block.
///
/// On a card that runs once the menu holds the Change group alone, as the card's own add buttons offer "+ Change"
/// alone.
fn add_inside_ui(
    ui: &mut Ui,
    drawing: &CardDrawing<'_>,
    block: &ClosingBlock,
    state: &CardState,
    pending: &mut Option<CardEdit>,
) {
    let states = pattern_states(drawing.document, &drawing.card.every);
    let states = states.as_ref().map(|(new, old)| (new.as_str(), old.as_str()));
    let label = add_inside_label(block.number);
    let title = add_inside_name(block.number);
    let groups = add_inside_groups(drawing.card);
    let agents = drawing.card.every != "cells";
    let id = state.widgets.with(("add inside", &block.id));
    let chosen = ui
        .horizontal(|ui| {
            ui.add_space(INDENT * block.depth as f32 + NUMBER_WIDTH);
            let response = add_button(
                ui,
                true,
                Button::new(label.as_str()).wrap_mode(egui::TextWrapMode::Extend),
                &label,
            );
            let popup = Popup::menu(&response).id(id.with("menu")).show(|ui| {
                let menu = ui.scope(|ui| {
                    ui.strong(&title);
                    let mut chosen = None;
                    for &button in groups {
                        ui.add_space(4.0);
                        ui.label(RichText::new(button.group()).weak().small());
                        if let Some(item) = add_items_ui(ui, button, agents, states) {
                            chosen = Some(item);
                        }
                    }
                    (chosen, ui.unique_id())
                });
                let (chosen, menu_id) = menu.inner;
                accessible_group(ui.ctx(), menu_id, Role::Dialog, &title, menu.response.rect);
                chosen
            })?;
            popup.inner
        })
        .inner;
    if let Some(item) = chosen {
        *pending = Some(add_line_edit(
            Some(drawing.card.id.clone()),
            Some(block.id.clone()),
            item,
            &drawing.card.every,
        ));
    }
}

/// Returns the groups of the menu of a block's add row on `card`: the Change group alone on a card that runs once, and
/// otherwise every group of the card's kind of thing, Repeat among them on a card over agents.
fn add_inside_groups(card: &SheetCard) -> &'static [AddButton] {
    if card.runs == CardRuns::Once {
        &[AddButton::Change]
    } else if card.every != "cells" {
        &AddButton::AGENTS_INSIDE_ORDER
    } else {
        &AddButton::ADD_INSIDE_ORDER
    }
}

/// Returns every button that card `card_index` of `document` draws under its lines and as its When blocks' add rows,
/// each with its visible words and its accessible name, in the state where no range is being filled. A card that the
/// pane cannot edit draws none.
#[cfg(test)]
pub fn card_buttons(
    document: &SheetDocument,
    sentences: &Sentences<'_>,
    report: &SheetReport,
    card_index: usize,
) -> Vec<LineControl> {
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    let values = parameter_defaults(document);
    let drawing = CardDrawing {
        document,
        sentences,
        report,
        card,
        values: &values,
        highlight: &[],
        fires: None,
        counted: false,
    };
    let relations = CardRelations::new(document, card);
    let notes = relations.notes(sentences, |line| !report.is_skipped(line));
    let lines = card_lines(document, sentences, report, card_index);
    let mut buttons = Vec::new();
    for line in &lines {
        let Some(line_data) = relations.line(&line.id) else {
            continue;
        };
        buttons.extend(
            card_notes(&drawing, line, line_data, &notes, false)
                .into_iter()
                .filter_map(|note| note.action)
                .map(|action| LineControl {
                    visible: action.label,
                    name: action.name,
                    select: false,
                }),
        );
    }
    buttons.extend(closing_blocks(&relations, &lines).into_values().flatten().map(|block| {
        let label = add_inside_label(block.number);
        LineControl {
            visible: label.clone(),
            name: label,
            select: false,
        }
    }));
    buttons
}

/// Returns the name of the button that runs `card` once, `None` for a card that runs every tick or that no button
/// runs.
fn button_of<'a>(document: &'a SheetDocument, card: &SheetCard) -> Option<&'a str> {
    if card.runs == CardRuns::EachTick {
        return None;
    }
    document
        .buttons
        .iter()
        .find(|button| {
            button
                .does
                .iter()
                .any(|step| step.card.as_deref() == Some(card.id.as_str()))
        })
        .map(|button| button.name.as_str())
}

// The add buttons

/// Draws the add buttons under `card`, or under the heading of the card that runs each tick over `every` while there is
/// none. A card that runs once offers "+ Change" alone, and a card over agents adds "+ Repeat...".
fn add_buttons_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    state: &CardState,
    card: Option<&SheetCard>,
    every: &str,
    pending: &mut Option<CardEdit>,
) {
    let once = card.is_some_and(|card| card.runs == CardRuns::Once);
    let agents = every != "cells";
    let buttons: &[AddButton] = match (once, agents) {
        (true, _) => &[AddButton::Change],
        (false, true) => &AddButton::AGENTS,
        (false, false) => &AddButton::ALL,
    };
    let states = pattern_states(document, every);
    let card_id = card.map(|card| card.id.clone());
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for button in buttons {
            let id = state.widgets.with(("add", card_id.as_deref(), button.label()));
            let response = add_button(
                ui,
                true,
                Button::new(button.label()).wrap_mode(egui::TextWrapMode::Extend),
                button.label(),
            )
            .on_hover_text(button.tooltip_for(agents));
            let chosen = Popup::menu(&response)
                .id(id.with("menu"))
                .show(|ui| {
                    add_menu_ui(
                        ui,
                        *button,
                        agents,
                        states.as_ref().map(|(new, old)| (new.as_str(), old.as_str())),
                    )
                })
                .and_then(|popup| popup.inner);
            if let Some(item) = chosen {
                *pending = Some(add_line_edit(card_id.clone(), None, item, every));
            }
        }
    });
}

/// Draws the menu of the add button `button`, on a card over agents when `agents` holds: its heading, and each item
/// with its pattern in grey under it. Returns the item chosen.
///
/// The pattern is painted, so it stays out of the accessibility tree and the item is named by its label alone.
fn add_menu_ui(ui: &mut Ui, button: AddButton, agents: bool, states: Option<(&str, &str)>) -> Option<AddItem> {
    ui.strong(button.heading_for(agents));
    add_items_ui(ui, button, agents, states)
}

/// Draws each item of the menu of the add button `button`, on a card over agents when `agents` holds, with its pattern
/// in grey under it, and returns the item chosen.
fn add_items_ui(ui: &mut Ui, button: AddButton, agents: bool, states: Option<(&str, &str)>) -> Option<AddItem> {
    let mut chosen = None;
    for item in button.items_for(agents) {
        let group = match item {
            AddItem::WithChance => Some(SHARED_DRAW_GROUP),
            AddItem::RandomNumber => Some(RANDOM_VALUE_GROUP),
            _ => None,
        };
        if let Some(group) = group {
            ui.add_space(4.0);
            ui.label(RichText::new(group).weak().small());
        }
        if ui.add(Button::new(item.label()).frame(false)).clicked() {
            chosen = Some(*item);
        }
        let font = TextStyle::Small.resolve(ui.style());
        let galley = ui
            .painter()
            .layout_no_wrap(item.pattern(states), font, ui.visuals().weak_text_color());
        let (_, rect) = ui.allocate_space(galley.size() + vec2(8.0, 0.0));
        ui.painter()
            .galley(rect.left_top() + vec2(8.0, 0.0), galley, ui.visuals().weak_text_color());
    }
    chosen
}

/// Returns the second and first states of the first States property with two or more states of the kind of thing
/// `every`, as `(new, old)`, which the Become patterns of the add menus name.
fn pattern_states(document: &SheetDocument, every: &str) -> Option<(String, String)> {
    let properties: &[SheetProperty] = if every == "cells" {
        document.cells.as_deref().unwrap_or_default()
    } else {
        document
            .agents
            .iter()
            .find(|kind| kind.id == every)
            .map_or(&[], |kind| kind.properties.as_slice())
    };
    let property = properties
        .iter()
        .find(|property| property.property_type == PropertyType::States && property.states.len() >= 2)?;
    Some((property.states[1].name.clone(), property.states[0].name.clone()))
}

/// Returns the line that `item` adds on a card over the kind of thing `every`, with its blanks empty.
///
/// A look at agents looks at the card's own kind, leaves the agent itself out and starts with one average, a Pick a
/// cell picks the open cell with the most of a property, and a Look at a cell looks at the cell an agent is on.
fn new_line(item: AddItem, every: &str) -> SheetLine {
    let compare = || {
        let mut compare = SheetLine::new(LineKind::Compare);
        compare.test = Some(CompareTest::Is);
        compare
    };
    let phrase = |line: SheetLine| Slot::Filled(SheetValue::Phrase(Box::new(line)));
    match item {
        AddItem::Count => {
            let mut line = SheetLine::new(LineKind::Count);
            line.around = Some(henad_graph::sheet::document::Around::Cells8);
            line.whose = phrase(compare());
            line
        }
        AddItem::TotalOrAverage => {
            let mut line = SheetLine::new(LineKind::LookAround);
            line.around = Some(henad_graph::sheet::document::Around::Cells8);
            let mut summary = LookSummary::new("total", "Total");
            summary.summary = Some(SummaryKind::Total);
            line.summaries = vec![summary];
            line
        }
        AddItem::BecomeWithChance => {
            let mut line = SheetLine::new(LineKind::Become);
            line.condition = phrase(SheetLine::new(LineKind::Chance));
            line
        }
        AddItem::BecomeAtRate => {
            let mut line = SheetLine::new(LineKind::Become);
            line.condition = phrase(SheetLine::new(LineKind::Rate));
            line
        }
        AddItem::WithChance => SheetLine::new(LineKind::Chance),
        AddItem::AtRate => SheetLine::new(LineKind::Rate),
        AddItem::Compare => compare(),
        AddItem::AllOf => SheetLine::new(LineKind::AllOf),
        AddItem::AnyOf => SheetLine::new(LineKind::AnyOf),
        AddItem::Calculate => SheetLine::new(LineKind::Calculate),
        AddItem::Set => SheetLine::new(LineKind::Set),
        AddItem::When => SheetLine::new(LineKind::When),
        AddItem::CountAgents | AddItem::LookAroundAgents => {
            let count = item == AddItem::CountAgents;
            let mut line = SheetLine::new(if count { LineKind::Count } else { LineKind::LookAround });
            line.around = Some(Around::AgentsWithin);
            line.agent_kind = Some(every.to_owned());
            line.include_me = Some(false);
            if count {
                line.whose = phrase(compare());
            } else {
                let mut summary = LookSummary::new("", "");
                summary.summary = Some(SummaryKind::Average);
                line.summaries = vec![summary];
            }
            line
        }
        AddItem::PickCell => {
            let mut line = SheetLine::new(LineKind::PickCell);
            line.around = Some(Around::Cells8);
            line.pick = Some(PickRule::Most);
            line.open_only = Some(true);
            line
        }
        AddItem::CellHereOrAhead => {
            let mut line = SheetLine::new(LineKind::LookAtCell);
            line.which = Some(CellWhich::Here);
            line
        }
        AddItem::RandomNumber => SheetLine::new(LineKind::RandomNumber),
        AddItem::RandomDirection => SheetLine::new(LineKind::RandomDirection),
        AddItem::Choose => {
            let mut line = SheetLine::new(LineKind::Choose);
            line.rows = vec![ChooseRow::default()];
            line
        }
        AddItem::DependingOn => SheetLine::new(LineKind::DependingOn),
        AddItem::Steer | AddItem::MoveBy | AddItem::MoveTo => {
            let (kind, mode) = match item {
                AddItem::Steer => (LineKind::Steer, LineMode::By),
                AddItem::MoveBy => (LineKind::Move, LineMode::By),
                _ => (LineKind::Move, LineMode::To),
            };
            let mut line = SheetLine::new(kind);
            line.mode = Some(mode);
            line
        }
        AddItem::KeepBetween => SheetLine::new(LineKind::KeepBetween),
        AddItem::Leave => SheetLine::new(LineKind::Leave),
        AddItem::Record => SheetLine::new(LineKind::Record),
        AddItem::Repeat => SheetLine::new(LineKind::Repeat),
        _ => SheetLine::new(LineKind::Become),
    }
}

/// Returns the edit that adds the line of `item` at the end of the card `card` over the kind of thing `every`, or of a
/// new card that runs each tick over `every` for `None`, or at the end of the block `inside` when one is given.
fn add_line_edit(card: Option<String>, inside: Option<String>, item: AddItem, every: &str) -> CardEdit {
    let line = new_line(item, every);
    let every = every.to_owned();
    CardEdit {
        run: Box::new(move |builder| {
            let card = match card {
                Some(card) => card,
                None => builder
                    .apply(SheetEdit::AddCard { every })?
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
            };
            builder.apply(SheetEdit::AddLine {
                card,
                inside,
                line: Box::new(line),
            })
        }),
        after: After::NewLine,
        rename: false,
        draft: None,
        late_draft: None,
    }
}

// The lines

/// Colour of a note under a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteColor {
    /// A note that asks the author to look again, in the theme's warn colour.
    Amber,
    /// A note of a line that cannot run because of what it uses, or of an error, in the theme's error colour.
    Red,
    /// A note that explains or confirms, in grey.
    Grey,
}

/// A button of a note. A click runs one edit of the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteAction {
    /// Words of the button.
    pub label: String,
    /// Accessible name of the button.
    pub name: String,
    /// The edit the button runs.
    pub edit: SheetEdit,
}

/// A note under a line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineNote {
    /// Words of the note.
    pub text: String,
    /// Colour of the note.
    pub color: NoteColor,
    /// Button of the note, `None` for a note with none.
    pub action: Option<NoteAction>,
    /// Tooltip and accessible description of the note, `None` for a note with none.
    pub description: Option<String>,
}

/// A control inside a line: a chip, a blank, a select, "always", "otherwise" or the optional "for each ..." blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineControl {
    /// Words the control shows.
    pub visible: String,
    /// Accessible name of the control.
    pub name: String,
    /// Whether the control is a select, whose visible words are its value rather than its label.
    pub select: bool,
}

/// One line of a card as the Card pane shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    /// Id of the line.
    pub id: String,
    /// Number of the line on its card, from 1.
    pub number: usize,
    /// Number of blocks around the line.
    pub depth: usize,
    /// Prefix of the line's accessible names, such as "Line 2" or "Clear line 1".
    pub prefix: String,
    /// Line word of the line in a tool's name, such as "line 2" or "Clear line 1".
    pub word: String,
    /// Sentence of the line as plain text, with its "otherwise" words.
    pub sentence: String,
    /// Parts of the sentence, in order.
    pub parts: Vec<SentencePart>,
    /// Place of each part of [`Self::parts`], `None` for words and for a part that no edit sets.
    pub places: Vec<Option<PartPlace>>,
    /// Ids of the higher lines that override this line, empty for a line with no "otherwise".
    pub otherwise: Vec<String>,
    /// Tooltip and accessible description of the "otherwise" words, empty for a line with no "otherwise".
    pub otherwise_tooltip: String,
    /// Notice shown the first time in a session that the line starts with "otherwise", empty for a line with no
    /// "otherwise".
    pub otherwise_notice: String,
    /// Ids of the other lines of the competing group the line starts with "or" in, empty for a line with no "or".
    pub competing: Vec<String>,
    /// Tooltip and accessible description of the word "or", empty for a line with no "or".
    pub competing_tooltip: String,
    /// Index in [`Self::parts`] of each result chip that the line cannot use, with the chip's tooltip.
    pub red_chips: Vec<(usize, String)>,
    /// Index in [`Self::parts`] of each state chip, with the colour of its state. A state that no property has is
    /// grey.
    pub swatches: Vec<(usize, Color32)>,
    /// Notes of the sheet's check about the line, in report order, a skip note first.
    pub notes: Vec<LineNote>,
    /// Number of skip notes at the start of [`Self::notes`]. The check skips a line that has one.
    pub skips: usize,
}

impl CardLine {
    /// Returns the accessible name of the line, such as "Line 2: Become Alive from Dead, when Alive neighbours is 3".
    pub fn label(&self) -> String {
        line_name(&self.prefix, &self.sentence)
    }

    /// Returns the control that `part` of the line draws, with its visible words and its accessible name, `None` for
    /// words.
    pub fn control(&self, part: &SentencePart) -> Option<LineControl> {
        let (visible, name, select) = match part {
            SentencePart::Chip(chip) => (chip.text(), chip.text(), false),
            SentencePart::Blank(blank) => (blank.word().to_owned(), blank.name(&self.prefix), false),
            SentencePart::Select(select, words) => {
                (select_words(*select, words).to_owned(), select.name(&self.prefix), true)
            }
            SentencePart::Always => (ALWAYS.to_owned(), always_name(&self.prefix), false),
            SentencePart::Otherwise(lines) => (otherwise_words(lines), otherwise_words(lines), false),
            SentencePart::Competing(_) => (COMPETING_WORD.to_owned(), COMPETING_WORD.to_owned(), false),
            SentencePart::ForEachBlank(_) => (
                FOR_EACH_BLANK_WORDS.to_owned(),
                for_each_optional_name(&self.prefix),
                false,
            ),
            SentencePart::ForEach(_) => (FOR_EACH_WORDS.to_owned(), for_each_change_name(&self.prefix), false),
            _ => return None,
        };
        Some(LineControl { visible, name, select })
    }

    /// Returns every control of the line in order.
    #[cfg(test)]
    pub fn controls(&self) -> Vec<LineControl> {
        self.parts.iter().filter_map(|part| self.control(part)).collect()
    }
}

/// Returns the lines of card `card_index` of `document` in display order, each with its number, depth, sentence and
/// parts, and the notes of `report` about it.
pub fn card_lines(
    document: &SheetDocument,
    sentences: &Sentences<'_>,
    report: &SheetReport,
    card_index: usize,
) -> Vec<CardLine> {
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    let button = button_of(document, card);
    let relations = CardRelations::new(document, card);
    let groups = relations.competing_groups();
    relations
        .order()
        .iter()
        .enumerate()
        .map(|(index, &line_id)| {
            let mut depth = 0;
            let mut block = relations.parent(line_id);
            // Display order holds each block once, before its lines. The bound guards against a cycle all the same.
            while let Some(parent) = block
                && depth < relations.order().len()
            {
                depth += 1;
                block = relations.parent(parent);
            }
            let number = relations.number(line_id).unwrap_or(index + 1);
            let prefix = line_prefix(button, number);
            let otherwise_numbers = relations.otherwise_numbers(line_id);
            let mut placed = sentences.placed_parts(line_id, &otherwise_numbers);
            // A lower line of a competing group starts with "or", unless a line outside the group overrides it.
            let group = groups
                .iter()
                .find(|group| group.contains(&line_id))
                .filter(|group| group.first() != Some(&line_id) && otherwise_numbers.is_empty());
            let (competing, competing_tooltip) = match group {
                Some(group) => {
                    let numbers: Vec<usize> = group.iter().filter_map(|id| relations.number(id)).collect();
                    let state = relations
                        .line(line_id)
                        .and_then(|line| state_of(document, line.property.as_deref(), line.start_state.as_deref()))
                        .unwrap_or(BLANK_TEXT);
                    let tooltip = competing_tooltip(&numbers, state);
                    placed = competing_parts(placed, numbers);
                    let others = group.iter().filter(|id| **id != line_id).map(|id| (*id).to_owned());
                    (others.collect(), tooltip)
                }
                None => (Vec::new(), String::new()),
            };
            let (parts, places): (Vec<SentencePart>, Vec<Option<PartPlace>>) =
                placed.into_iter().map(|placed| (placed.part, placed.place)).unzip();
            let red_chips = red_chips(&relations, line_id, index, &parts);
            let (notes, skips) = line_notes(report, card_index, line_id, &red_chips);
            let swatches = swatches(document, &parts);
            let (otherwise_tooltip, otherwise_notice) = if otherwise_numbers.is_empty() {
                (String::new(), String::new())
            } else {
                otherwise_words_of(document, &relations, line_id, &prefix, &otherwise_numbers)
            };
            CardLine {
                id: line_id.to_owned(),
                number,
                depth,
                prefix,
                word: line_word(button, number),
                sentence: sentence_text(&parts),
                otherwise: relations.otherwise_of(line_id).into_iter().map(str::to_owned).collect(),
                otherwise_tooltip,
                otherwise_notice,
                competing,
                competing_tooltip,
                parts,
                places,
                red_chips: red_chips.into_iter().map(|red| (red.index, red.tooltip)).collect(),
                swatches,
                notes,
                skips,
            }
        })
        .collect()
}

/// Returns the index of each state chip of `parts` with the colour of its state in `document`.
fn swatches(document: &SheetDocument, parts: &[SentencePart]) -> Vec<(usize, Color32)> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let SentencePart::Chip(SentenceChip {
                kind: ChipKind::State { property, state },
                ..
            }) = part
            else {
                return None;
            };
            let color = property
                .as_deref()
                .and_then(|property| property_of(document, property))
                .and_then(|property| property.states.iter().find(|candidate| candidate.id == *state))
                .map_or(mcs::GRAY_500, |state| {
                    let [red, green, blue, alpha] = state.color;
                    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
                });
            Some((index, color))
        })
        .collect()
}

/// A result chip that its line cannot use.
struct RedChip {
    /// Index of the chip in the line's parts.
    index: usize,
    /// Tooltip of the chip.
    tooltip: String,
    /// Skip note the sheet's check writes for the chip.
    note: String,
}

/// Returns the result chips of line `line_id`, at `position` in display order, that name a result below it or a
/// result inside a When the line is not in.
fn red_chips(relations: &CardRelations<'_>, line_id: &str, position: usize, parts: &[SentencePart]) -> Vec<RedChip> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let SentencePart::Chip(SentenceChip {
                kind: ChipKind::Result(result),
                name,
                ..
            }) = part
            else {
                return None;
            };
            let result_name = relations
                .line(result)
                .and_then(|line| line.name.clone())
                .unwrap_or_else(|| name.clone());
            let result_position = relations.index(result)?;
            if result_position >= position {
                return Some(RedChip {
                    index,
                    tooltip: result_below_tooltip(&result_name),
                    note: uses_result_below(&result_name),
                });
            }
            let block = relations.parent(result)?;
            if relations.is_within(line_id, block) {
                return None;
            }
            let number = relations.number(block).unwrap_or(0);
            Some(RedChip {
                index,
                tooltip: result_inside_tooltip(&result_name),
                note: uses_result_inside(&result_name, number),
            })
        })
        .collect()
}

/// Returns the notes of `report` about line `line_id` of card `card_index`, the skip notes first, with the number of
/// skip notes.
///
/// A skip note about a chip in `red_chips` and an error are red, and any other note amber.
fn line_notes(report: &SheetReport, card_index: usize, line_id: &str, red_chips: &[RedChip]) -> (Vec<LineNote>, usize) {
    let about_line = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.card == Some(card_index) && diagnostic.line.as_deref() == Some(line_id));
    let (skips, others): (Vec<_>, Vec<_>) = about_line.partition(|diagnostic| diagnostic.skips_line);
    let skip_count = skips.len();
    let notes = skips
        .into_iter()
        .chain(others)
        .map(|diagnostic| {
            let text = diagnostic.diagnostic.message.clone();
            let red = diagnostic.diagnostic.severity == DiagnosticSeverity::Error
                || red_chips.iter().any(|chip| chip.note == text);
            LineNote {
                text,
                color: if red { NoteColor::Red } else { NoteColor::Amber },
                action: None,
                description: None,
            }
        })
        .collect();
    (notes, skip_count)
}

/// Returns whether the card pane draws `diagnostic` under a line, which it does for a problem about a line in the
/// display order of its card.
///
/// Note that the checker also places a problem at a line the card never shows, such as a line table no lines list
/// names. Such a problem stays in the list under the panes.
pub fn draws_diagnostic(document: &SheetDocument, diagnostic: &SheetDiagnostic) -> bool {
    let (Some(card), Some(line)) = (diagnostic.card, diagnostic.line.as_deref()) else {
        return false;
    };
    document
        .cards
        .get(card)
        .is_some_and(|card| CardRelations::new(document, card).index(line).is_some())
}

/// Returns the tooltip of the "otherwise" words of line `line_id`, whose line prefix is `prefix`, overridden by the
/// lines numbered `numbers`, and the notice shown the first time the line starts with "otherwise".
///
/// Both name the property the lines change, and the old state a cell must be in for both to apply.
fn otherwise_words_of(
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line_id: &str,
    prefix: &str,
    numbers: &[usize],
) -> (String, String) {
    let line = relations.line(line_id);
    let property = line
        .and_then(|line| line.property.as_deref())
        .and_then(|id| property_of(document, id));
    let state = line.and_then(|line| state_of(document, line.property.as_deref(), line.start_state.as_deref()));
    let property_name = property.map_or(BLANK_TEXT, |property| property.name.as_str());
    (
        otherwise_tooltip(numbers, property_name, state),
        otherwise_notice(prefix, numbers, property_name, state),
    )
}

/// Returns the name of the named state `state` of the property `property` of `document`, `None` for any state or a
/// state the property does not hold.
fn state_of<'a>(document: &'a SheetDocument, property: Option<&str>, state: Option<&str>) -> Option<&'a str> {
    let state = state.filter(|state| *state != ANY_STATE_ID)?;
    property_of(document, property?)?
        .states
        .iter()
        .find(|candidate| candidate.id == state)
        .map(|state| state.name.as_str())
}

/// Returns the property `id` of any kind of thing of `document`.
fn property_of<'a>(document: &'a SheetDocument, id: &str) -> Option<&'a SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
        .find(|property| property.id == id)
}

/// Returns the notes the card adds under `line` to those of the check, in the order of the wording table: the one-side
/// offer after the skip note, then the count used as a chance or a rate, then the note of a rate with its mean stay as
/// its description and the long tick, or the note of a chance for each, then the offer to grow a rate for each count,
/// then the line's `relation_notes`: the gap, coverage and order notes, the offers to fold a When, and the notes
/// inside and under a When. The author's own note under the line comes last.
///
/// Note that the unit mismatch of a rate is a warning of the check, already among the line's notes.
fn card_notes(
    drawing: &CardDrawing<'_>,
    line: &CardLine,
    line_data: &SheetLine,
    relation_notes: &[RelationNote],
    filling: bool,
) -> Vec<LineNote> {
    let mut notes = line.notes.clone();
    if line.skips > 0
        && !filling
        && let Some((low, high)) = range_sides(line_data)
        && low != high
        && let Some(SheetValue::Phrase(range)) = line_data.condition.value()
    {
        let (comparison, number) = if high {
            (CompareTest::MoreThan, range.high.value())
        } else {
            (CompareTest::LessThan, range.low.value())
        };
        let number = number
            .map(|value| drawing.sentences.value(value, None))
            .unwrap_or_default();
        let words = format!("{} {number}", comparison_words(comparison, false));
        notes.insert(
            line.skips,
            LineNote {
                text: ONE_SIDED_RANGE.to_owned(),
                color: NoteColor::Amber,
                action: Some(NoteAction {
                    label: use_instead_label(&words),
                    name: use_instead_name(&line.prefix, &words),
                    edit: SheetEdit::UseOneSide { line: line.id.clone() },
                }),
                description: None,
            },
        );
    }
    if let Some((count, draw)) = count_as_draw(drawing.document, &line.id) {
        let each = for_each_words(&count);
        notes.push(LineNote {
            text: count_used_as_draw(&count, draw, &each),
            color: NoteColor::Amber,
            action: Some(NoteAction {
                label: use_for_each_label(draw, &each),
                name: use_for_each_name(&line.prefix, draw, &each),
                edit: SheetEdit::UseCountForEach { line: line.id.clone() },
            }),
            description: None,
        });
    }
    if let Some(note) = rate_note(drawing.document, &line.id, drawing.values) {
        notes.push(LineNote {
            text: note.text,
            color: NoteColor::Grey,
            action: None,
            description: note.mean_stay,
        });
        if let Some(largest) = note.long_tick {
            notes.push(LineNote {
                text: long_tick_note(&largest),
                color: NoteColor::Grey,
                action: None,
                description: None,
            });
        }
    }
    if let Some(text) = chance_note(drawing.document, &line.id, drawing.values) {
        notes.push(LineNote {
            text,
            color: NoteColor::Grey,
            action: None,
            description: None,
        });
    }
    if let Some(count) = grow_count(drawing.document, &line.id)
        && let Some(path) = tries_path(line_data)
    {
        let each = for_each_words(&count.name);
        notes.push(LineNote {
            text: rate_grow_prompt(&count.name),
            color: NoteColor::Amber,
            action: Some(NoteAction {
                label: add_for_each_label(&each),
                name: add_for_each_name(&line.prefix, &each),
                edit: SheetEdit::SetSlot {
                    line: line.id.clone(),
                    path,
                    value: Slot::Filled(SheetValue::Chip(SheetChip::Result {
                        line: count.line,
                        part: None,
                        property: None,
                    })),
                },
            }),
            description: None,
        });
    }
    notes.extend(
        relation_notes
            .iter()
            .filter(|note| note.line == line.id)
            .map(|note| relation_line_note(line, note)),
    );
    notes.extend(author_note(line_data));
    notes
}

/// Returns the author's own note under `line`, in grey, `None` for a line with none.
fn author_note(line: &SheetLine) -> Option<LineNote> {
    Some(LineNote {
        text: line.note.clone()?,
        color: NoteColor::Grey,
        action: None,
        description: None,
    })
}

/// Returns `note`, a relation note under `line`, as the card draws it: grey, or amber with its button for an offer to
/// fold a When.
fn relation_line_note(line: &CardLine, note: &RelationNote) -> LineNote {
    let (color, action) = match &note.kind {
        RelationNoteKind::WhenRepeats => (
            NoteColor::Amber,
            Some(NoteAction {
                label: REMOVE_THIS_WHEN.to_owned(),
                name: remove_when_name(&line.prefix),
                edit: SheetEdit::DeleteLine { line: line.id.clone() },
            }),
        ),
        RelationNoteKind::OnlyOneInside { number, .. } => (
            NoteColor::Amber,
            Some(NoteAction {
                label: move_into_label(*number),
                name: move_into_name(&line.prefix, *number),
                edit: SheetEdit::MoveConditionInto { when: line.id.clone() },
            }),
        ),
        _ => (NoteColor::Grey, None),
    };
    LineNote {
        text: note.text.clone(),
        color,
        action,
        description: None,
    }
}

/// Returns the path of the "for each" slot of the rate or chance line `line` draws with, on the line or in its
/// condition.
fn tries_path(line: &SheetLine) -> Option<String> {
    let PlaceTarget::Slot(path) = draw_value_target(line)? else {
        return None;
    };
    let holder = path.rsplit_once('.').map_or("", |(holder, _)| holder);
    Some(if holder.is_empty() {
        "tries".to_owned()
    } else {
        format!("{holder}.tries")
    })
}

/// What the parts of one line gather while they are drawn.
struct LineDrawing<'a> {
    /// Ids of the lines that a hovered or focused "otherwise" or result chip names.
    hovered: &'a mut Vec<String>,
    /// Shape and place of each empty "for each ..." blank, painted once the line is known to be hovered or focused.
    for_each_shapes: Vec<(egui::layers::ShapeIdx, Rect)>,
    /// Whether a control of the line holds the keyboard focus.
    focused: bool,
    /// Whether a picker of the line is open.
    picker_open: bool,
    /// Whether a picker of the line closed this frame with no value chosen.
    closed_empty: bool,
    /// Whether the arrow after the line's condition word is drawn. A line draws one arrow, after its first word.
    arrow_drawn: bool,
}

/// Draws `line`: its number, its sentence part by part, its menu, its rename field and its notes under it, with the
/// relation notes of the card among them. Returns the row of the sentence.
#[expect(
    clippy::too_many_arguments,
    reason = "the card, its relations and notes, the line and the pane's state each take a part"
)]
fn line_ui(
    ui: &mut Ui,
    drawing: &CardDrawing<'_>,
    relations: &CardRelations<'_>,
    relation_notes: &[RelationNote],
    line: &CardLine,
    state: &mut CardState,
    hovered: &mut Vec<String>,
    pending: &mut Option<CardEdit>,
) -> Rect {
    let Some(line_data) = relations.line(&line.id) else {
        return Rect::NOTHING;
    };
    let context = LineContext {
        document: drawing.document,
        sentences: drawing.sentences,
        relations,
        line_id: &line.id,
        line: line_data,
    };
    let background = ui.painter().add(Shape::Noop);
    let mut gathered = LineDrawing {
        hovered,
        for_each_shapes: Vec::new(),
        focused: false,
        picker_open: false,
        closed_empty: false,
        arrow_drawn: false,
    };
    let column = if drawing.counted { COUNT_WIDTH } else { 0.0 };
    let right = ui.max_rect().right();
    let row = ui.horizontal_wrapped(|ui| {
        ui.set_max_width((ui.available_width() - column).max(0.0));
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(INDENT * line.depth as f32);
        ui.add_sized(
            [NUMBER_WIDTH, ui.spacing().interact_size.y],
            Label::new(RichText::new(line.number.to_string()).weak()),
        );
        for (index, part) in line.parts.iter().enumerate() {
            let place = line.places.get(index).cloned().flatten();
            let parts = PartDrawing {
                context: &context,
                line,
                index,
                place,
            };
            part_ui(ui, &parts, part, state, &mut gathered, pending);
            if let Some(place) = parts.place.as_ref().and_then(|place| list_end(line_data, place)) {
                let plus = PartDrawing {
                    place: Some(place),
                    ..parts
                };
                add_value_ui(ui, &plus, state, &mut gathered, pending);
            }
        }
        if line_data.kind == LineKind::Choose {
            let parts = PartDrawing {
                context: &context,
                line,
                index: line.parts.len(),
                place: Some(PartPlace {
                    target: PlaceTarget::Slot(format!("row.{}.value", line_data.rows.len())),
                    want: Some(Blank::Value),
                }),
            };
            add_row_ui(ui, &parts, state, &mut gathered, pending);
        }
        if line_data.around == Some(Around::AgentsWithin) && line_data.kind == LineKind::LookAround {
            let parts = PartDrawing {
                context: &context,
                line,
                index: line.parts.len(),
                place: None,
            };
            add_summary_ui(ui, &parts, state, &mut gathered, pending);
        }
        ui.add_space(6.0);
        line_menu_ui(ui, drawing.document, relations, line, line_data, state, pending);
        ui.unique_id()
    });
    let rect = row.response.rect;
    accessible_group(ui.ctx(), row.inner, Role::Group, &line.label(), rect);
    fire_count_ui(ui, drawing, line, rect.top(), right);
    if drawing.highlight.contains(&line.id) {
        ui.painter()
            .set(background, Shape::rect_filled(rect, CornerRadius::ZERO, mcs::BLUE_950));
    }
    // The empty "for each ..." blank stays in the accessibility tree, and is painted only on a hovered or focused line.
    if gathered.focused || ui.rect_contains_pointer(rect) {
        for (shape, blank) in gathered.for_each_shapes.drain(..) {
            ui.painter()
                .set(shape, Shape::Vec(blank_shapes(ui, blank, FOR_EACH_BLANK_WORDS)));
        }
    }
    end_filling(ui, state, &line.id, rect, &gathered);
    if state.renaming.as_ref().is_some_and(|renaming| renaming.line == line.id) {
        rename_ui(ui, line, state, pending);
    }
    let filling = state.filling.contains(&line.id);
    notes_ui(
        ui,
        line,
        line_data,
        card_notes(drawing, line, line_data, relation_notes, filling),
        pending,
    );
    rect
}

/// Draws the fire count of `line` in grey at the right of the card, in the column of [`COUNT_WIDTH`] that ends at
/// `right`, level with the row's top at `top`, when the card's counts hold one for the line. Its hover text and
/// accessible description name the line, the number of things the card runs on and the tick.
fn fire_count_ui(ui: &mut Ui, drawing: &CardDrawing<'_>, line: &CardLine, top: f32, right: f32) {
    let Some(fires) = drawing.fires else {
        return;
    };
    let Some(&count) = fires.counts.get(&line.id) else {
        return;
    };
    let thing = drawing.sentences.thing_word(&drawing.card.every);
    let place = Rect::from_min_max(
        Pos2::new(right - COUNT_WIDTH, top),
        Pos2::new(right, top + ui.spacing().interact_size.y),
    );
    let description = fire_count_description(&line.prefix, count, &thing, fires.tick);
    // A child placed over the row's top takes no space, so the notes under the row keep their place.
    let mut column = ui.new_child(
        UiBuilder::new()
            .max_rect(place)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let label = column.add(Label::new(RichText::new(fire_count(count)).weak()).selectable(false));
    ui.ctx()
        .accesskit_node_builder(label.id, |node| node.set_description(description.as_str()));
    label.on_hover_text(description);
}

/// Draws `notes` under `line`, each note's button running its edit.
fn notes_ui(ui: &mut Ui, line: &CardLine, line_data: &SheetLine, notes: Vec<LineNote>, pending: &mut Option<CardEdit>) {
    for note in notes {
        let color = match note.color {
            NoteColor::Amber => ui.visuals().warn_fg_color,
            NoteColor::Red => ui.visuals().error_fg_color,
            NoteColor::Grey => ui.visuals().weak_text_color(),
        };
        ui.horizontal_wrapped(|ui| {
            ui.add_space(INDENT * line.depth as f32 + NUMBER_WIDTH);
            let label = ui.add(Label::new(RichText::new(note.text.as_str()).color(color)).wrap());
            if let Some(description) = &note.description {
                ui.ctx()
                    .accesskit_node_builder(label.id, |node| node.set_description(description.as_str()));
                label.on_hover_text(description.as_str());
            }
            if let Some(action) = note.action {
                let button = add_button(
                    ui,
                    true,
                    Button::new(&action.label).wrap_mode(egui::TextWrapMode::Extend),
                    &action.name,
                );
                if button.clicked() {
                    let after = match action.edit {
                        // The count moves into "for each", and the emptied chance or rate opens its picker.
                        SheetEdit::UseCountForEach { .. } => {
                            draw_value_target(line_data).map_or(After::Stay, |target| After::Open {
                                line: line.id.clone(),
                                target,
                            })
                        }
                        _ => After::Stay,
                    };
                    *pending = Some(CardEdit::commands(vec![action.edit], after));
                }
            }
        });
    }
}

/// Returns the target of the chance or rate that line `line` draws with, on the line or in its condition.
fn draw_value_target(line: &SheetLine) -> Option<PlaceTarget> {
    let path = match line.kind {
        LineKind::Chance => "chance",
        LineKind::Rate => "rate",
        _ => match line.condition.value()? {
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::Chance => "condition.chance",
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::Rate => "condition.rate",
            _ => return None,
        },
    };
    Some(PlaceTarget::Slot(path.to_owned()))
}

/// Ends the mark of line `line_id` as being filled when one of its pickers closed with no value, or when a click or
/// the focus lands outside the line, `rect`, and its pickers.
fn end_filling(ui: &Ui, state: &mut CardState, line_id: &str, rect: Rect, gathered: &LineDrawing<'_>) {
    if !state.filling.contains(line_id) {
        return;
    }
    let clicked_outside = ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|position| !rect.contains(position))
    });
    let focus_elsewhere = !gathered.focused && ui.memory(|memory| memory.focused().is_some());
    if gathered.closed_empty || (!gathered.picker_open && (clicked_outside || focus_elsewhere)) {
        state.filling.remove(line_id);
    }
}

/// The part being drawn: its line, its index and its place.
struct PartDrawing<'a> {
    /// What the picker of the part reads.
    context: &'a LineContext<'a>,
    /// The line.
    line: &'a CardLine,
    /// Index of the part in the line's parts.
    index: usize,
    /// Place of the part in the line, `None` for a part no edit sets.
    place: Option<PartPlace>,
}

/// Draws `part`, gathers what `gathered` holds, and opens its picker or select when its control is clicked.
fn part_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    part: &SentencePart,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Option<CardEdit>,
) {
    let line = drawing.line;
    let name = line.control(part).map(|control| control.name).unwrap_or_default();
    let tag = u8::from(matches!(part, SentencePart::ForEach(_)));
    let id = drawing
        .place
        .as_ref()
        .map(|place| state.control_id(&line.id, &place.target, tag));
    let response = match part {
        SentencePart::Text(words) => {
            ui.label(words.as_str());
            return;
        }
        // A word that does not wrap moves to the next row whole when the row has no room left for it.
        SentencePart::ConditionWord(words) => {
            ui.add(Label::new(RichText::new(*words).strong()).extend());
            if !gathered.arrow_drawn && takes_condition(drawing.context.line.kind) {
                gathered.arrow_drawn = true;
                condition_arrow_ui(ui, drawing, state, gathered, pending);
            }
            return;
        }
        SentencePart::From => {
            ui.add(Label::new(FROM_WORD).extend()).on_hover_text(FROM_TOOLTIP);
            return;
        }
        SentencePart::Chip(chip) => {
            let red = line
                .red_chips
                .iter()
                .find(|(red, _)| *red == drawing.index)
                .map(|(_, tooltip)| tooltip);
            let swatch = line
                .swatches
                .iter()
                .find(|(state, _)| *state == drawing.index)
                .map(|(_, color)| *color);
            let shown = draft_chip(drawing, state, chip);
            let response = chip_with_id(ui, id, &shown, red.is_some(), swatch);
            let response = match red {
                Some(tooltip) => response.on_hover_text(tooltip.as_str()),
                None => response,
            };
            if let ChipKind::Result(result) = &shown.kind
                && (response.contains_pointer() || response.has_focus())
            {
                gathered.hovered.push(result.clone());
            }
            response
        }
        SentencePart::Blank(blank) => {
            match draft_value(drawing, state) {
                // A value chosen for an empty Calculate line shows as its chip until the formula is written.
                Some(chip) => chip_with_id(ui, id, &chip, false, None),
                None => blank_ui(ui, id, blank.word(), &name, true).1,
            }
        }
        SentencePart::Select(select, words) => {
            let words = match (select, words.is_empty(), drawing.place.as_ref()) {
                (LineSelect::Operation, true, Some(_)) => {
                    let operator = state.drafts.get(&line.id).map_or('+', |draft| draft.operator);
                    operation_words(&operator.to_string()).unwrap_or_default().to_owned()
                }
                _ => words.clone(),
            };
            select_ui(ui, id, select_words(*select, &words), &name, words.is_empty())
        }
        SentencePart::Always => text_control(ui, id, ALWAYS, mcs::GRAY_400, &name).on_hover_text(ALWAYS_TOOLTIP),
        SentencePart::Otherwise(lines) => lead_word_ui(
            ui,
            &otherwise_words(lines),
            &line.otherwise_tooltip,
            &line.otherwise,
            gathered,
        ),
        SentencePart::Competing(_) => {
            lead_word_ui(ui, COMPETING_WORD, &line.competing_tooltip, &line.competing, gathered)
        }
        SentencePart::ForEachBlank(draw) => {
            ui.add_space(4.0);
            let (shape, response) = blank_ui(ui, id, FOR_EACH_BLANK_WORDS, &name, false);
            if let Some(shape) = shape {
                gathered.for_each_shapes.push((shape, response.rect));
            }
            response.on_hover_text(for_each_optional_tooltip(*draw))
        }
        SentencePart::ForEach(_) => text_control(ui, id, FOR_EACH_WORDS, mcs::GRAY_100, &name),
        _ => return,
    };
    gathered.focused |= response.has_focus();
    let (Some(place), Some(id)) = (drawing.place.as_ref(), id) else {
        return;
    };
    control_picker(ui, drawing, place, part, &response, id, state, gathered, pending);
}

/// Draws the muted word `words` that leads a change, "otherwise (when line 2 does not apply)" or "or", focusable and
/// described by `tooltip`. Hovering or focusing it highlights the lines `lines` on the next frame.
fn lead_word_ui(ui: &mut Ui, words: &str, tooltip: &str, lines: &[String], gathered: &mut LineDrawing<'_>) -> Response {
    let response = ui
        .add(Label::new(RichText::new(words).weak()).sense(Sense::click()))
        .on_hover_text(tooltip);
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(words);
        node.set_description(tooltip);
    });
    if response.contains_pointer() || response.has_focus() {
        gathered.hovered.extend(lines.iter().cloned());
    }
    response
}

/// Opens the picker or the select of the control `id` at `place`, whose response is `response`, when it is clicked
/// or due to open, and keeps the edit a choice in it makes.
#[expect(
    clippy::too_many_arguments,
    reason = "the control, its place and the pane's state each take a part"
)]
fn control_picker(
    ui: &Ui,
    drawing: &PartDrawing<'_>,
    place: &PartPlace,
    part: &SentencePart,
    response: &Response,
    id: Id,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Option<CardEdit>,
) {
    if state.focus_next == Some(id) {
        response.request_focus();
        state.focus_next = None;
    }
    let popup_id = id.with("picker");
    if state.open_next == Some(id) {
        Popup::open_id(ui.ctx(), popup_id);
        state.open_next = None;
    }
    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    if !was_open && !response.clicked() {
        return;
    }
    if let SentencePart::Select(..) = part {
        let chosen = select_popup(ui, drawing, place, response, popup_id);
        gathered.picker_open |= Popup::is_id_open(ui.ctx(), popup_id);
        if let Some(option) = chosen {
            *pending = select_edit(drawing, state, place, option);
        }
        return;
    }
    let picker = place_picker(drawing, place, part);
    let mut open = false;
    let choice = picker.show(ui, response, popup_id, &mut open);
    gathered.picker_open |= open;
    gathered.closed_empty |= was_open && !open && choice.is_none();
    if let Some(choice) = choice {
        *pending = choice_edit(drawing, state, place, &choice);
    }
}

/// Adds a frameless button showing `text` in `color`, named `name`, under the widget id `id` when one is given, and
/// returns its response.
///
/// It is laid out as one unbroken piece, so a row that has no room for it moves it to the next row whole.
fn text_control(ui: &mut Ui, id: Option<Id>, text: &str, color: Color32, name: &str) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = galley.size() + CHIP_PADDING * 2.0;
    let height = size.y.max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = allocate_control(ui, id, vec2(size.x, height));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.has_focus() || response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::GRAY_500),
                egui::StrokeKind::Inside,
            );
        }
        let position = Pos2::new(rect.left() + CHIP_PADDING.x, rect.center().y - galley.size().y / 2.0);
        painter.galley(position, galley, color);
    }
    response
}

/// Returns the place after the last item of the list `place` is an item of, the place of "+" after the list, `None`
/// when `place` is no last item of a list.
fn list_end(line: &SheetLine, place: &PartPlace) -> Option<PartPlace> {
    let target = match &place.target {
        PlaceTarget::ListItem { slot, index } if index + 1 == list_len(line, slot) => PlaceTarget::ListItem {
            slot: slot.clone(),
            index: index + 1,
        },
        PlaceTarget::Slot(path) => {
            let (outer, index) = path.rsplit_once('.')?;
            let index: usize = index.parse().ok()?;
            let holder = match outer.strip_suffix("items")?.strip_suffix('.') {
                None if outer == "items" => line,
                Some(phrase) => match slot_at(line, phrase)?.value()? {
                    SheetValue::Phrase(phrase) => phrase,
                    _ => return None,
                },
                None => return None,
            };
            if index + 1 != holder.items.len() || !matches!(holder.kind, LineKind::AllOf | LineKind::AnyOf) {
                return None;
            }
            PlaceTarget::Slot(format!("{outer}.{}", index + 1))
        }
        _ => return None,
    };
    Some(PartPlace {
        target,
        want: place.want,
    })
}

/// Draws "+" after a list, named "Line 3: add another value". A click opens the list's picker for a new item.
fn add_value_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Option<CardEdit>,
) {
    let Some(place) = drawing.place.as_ref() else {
        return;
    };
    let id = state.control_id(&drawing.line.id, &place.target, 0);
    ui.add_space(4.0);
    let response = text_control(ui, Some(id), "+", mcs::GRAY_100, &add_value_name(&drawing.line.prefix));
    gathered.focused |= response.has_focus();
    let blank = SentencePart::Blank(place.want.unwrap_or(Blank::NumberList));
    control_picker(ui, drawing, place, &blank, &response, id, state, gathered, pending);
}

/// Draws "+" after a Choose line, named "Line 7: add another row". A click opens the picker of a value, and a choice
/// adds a row that gives it, with its condition empty.
fn add_row_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Option<CardEdit>,
) {
    let Some(place) = drawing.place.as_ref() else {
        return;
    };
    let id = state.control_id(&drawing.line.id, &place.target, 0);
    ui.add_space(4.0);
    let response = text_control(
        ui,
        Some(id),
        "+",
        mcs::GRAY_100,
        &add_choose_row_name(&drawing.line.prefix),
    );
    gathered.focused |= response.has_focus();
    control_picker(
        ui,
        drawing,
        place,
        &SentencePart::Blank(Blank::Value),
        &response,
        id,
        state,
        gathered,
        pending,
    );
}

/// Draws "+" after the summaries of a look at agents, named "Line 1: add another summary". A click opens the picker of
/// a new summary, and a choice adds it with its value empty.
fn add_summary_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    state: &CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Option<CardEdit>,
) {
    let line = drawing.line;
    let target = PlaceTarget::Slot("summaries".to_owned());
    let id = state.control_id(&line.id, &target, 3);
    ui.add_space(4.0);
    let response = text_control(ui, Some(id), "+", mcs::GRAY_100, &add_summary_name(&line.prefix));
    gathered.focused |= response.has_focus();
    let popup_id = id.with("picker");
    if !Popup::is_id_open(ui.ctx(), popup_id) && !response.clicked() {
        return;
    }
    let mut open = false;
    let choice = summary_picker().show(ui, &response, popup_id, &mut open);
    gathered.picker_open |= open;
    if let Some(Choice::Summary(summary)) = choice {
        let index = drawing.context.line.summaries.len();
        // A count has no value to fill.
        let after = if summary == SummaryKind::Count {
            After::Stay
        } else {
            After::Open {
                line: line.id.clone(),
                target: PlaceTarget::Slot(format!("summaries.{index}.value")),
            }
        };
        *pending = Some(CardEdit::commands(
            vec![SheetEdit::AddSummary {
                line: line.id.clone(),
                summary,
            }],
            after,
        ));
    }
}

/// Returns whether a line of `kind` holds a condition of its own: a Become, a Set, a Leave, a Record or a When.
fn takes_condition(kind: LineKind) -> bool {
    matches!(
        kind,
        LineKind::Become | LineKind::Set | LineKind::Leave | LineKind::Record | LineKind::When
    )
}

/// Draws the arrow after the condition word of a change. A click opens "Choose when" for its condition.
fn condition_arrow_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Option<CardEdit>,
) {
    let line = drawing.line;
    let target = PlaceTarget::Slot("condition".to_owned());
    let id = state.control_id(&line.id, &target, 2);
    let name = condition_arrow_name(&line.prefix);
    let response = select_ui(ui, Some(id), "", &name, true).on_hover_text(CONDITION_ARROW_TOOLTIP);
    gathered.focused |= response.has_focus();
    let popup_id = id.with("picker");
    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    if !was_open && !response.clicked() {
        return;
    }
    let place = PartPlace {
        target,
        want: Some(Blank::When),
    };
    let picker = choose_when(drawing.context, &otherwise_if_always(drawing.context));
    let mut open = false;
    let choice = picker.show(ui, &response, popup_id, &mut open);
    gathered.picker_open |= open;
    if let Some(choice) = choice {
        *pending = choice_edit(drawing, state, &place, &choice);
    }
}

/// Returns the numbers of the higher lines that would override the line of `context` were its condition always.
fn otherwise_if_always(context: &LineContext<'_>) -> Vec<usize> {
    let Some((card_index, _)) = context.document.line(context.line_id) else {
        return Vec::new();
    };
    let mut document = context.document.clone();
    let Some(card) = document.cards.get_mut(card_index) else {
        return Vec::new();
    };
    if let Some(line) = card.line_tables.get_mut(context.line_id) {
        line.condition = Slot::Filled(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::Always))));
    }
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    CardRelations::new(&document, card).otherwise_numbers(context.line_id)
}

/// Returns the picker of the part `part` at `place`.
fn place_picker(drawing: &PartDrawing<'_>, place: &PartPlace, part: &SentencePart) -> CardPicker {
    let context = drawing.context;
    let want = place.want.unwrap_or(Blank::Value);
    if want == Blank::When {
        return choose_when(context, &otherwise_if_always(context));
    }
    let line = context.line;
    if let PlaceTarget::Slot(path) = &place.target
        && path.starts_with("summaries.")
    {
        return looked_at_picker(context);
    }
    let value = value_at(line, &place.target);
    let property = match &place.target {
        PlaceTarget::Field {
            field: FieldPlace::StartState,
            ..
        } => line.property.as_deref(),
        PlaceTarget::Slot(path) | PlaceTarget::ListItem { slot: path, .. } => {
            phrase_of(line, path).and_then(|phrase| read_property(phrase.left.value()))
        }
        _ => None,
    };
    let draw = match &place.target {
        PlaceTarget::Slot(path) if path.ends_with("tries") => match phrase_of(line, path) {
            Some(phrase) if phrase.kind == LineKind::Rate => DrawKind::Rate,
            _ => DrawKind::Chance,
        },
        _ => DrawKind::Chance,
    };
    let filled = matches!(part, SentencePart::Chip(_) | SentencePart::ForEach(_));
    let mut picker = picker_for(context, want, property, filled, &typed_text(value), draw);
    // "+" after a list offers what the list takes, with nothing to remove.
    if matches!(&place.target, PlaceTarget::ListItem { index, slot } if list_len(line, slot) == *index) {
        picker.remove = None;
    }
    picker
}

/// Returns the chip a part shows: `chip` itself, or the value chosen for an empty Calculate line in its place.
fn draft_chip(drawing: &PartDrawing<'_>, state: &CardState, chip: &SentenceChip) -> SentenceChip {
    draft_value(drawing, state).unwrap_or_else(|| chip.clone())
}

/// Returns the chip of the value chosen for the place of an empty Calculate line, `None` while none is chosen.
fn draft_value(drawing: &PartDrawing<'_>, state: &CardState) -> Option<SentenceChip> {
    let place = drawing.place.as_ref()?;
    let draft = state.drafts.get(&drawing.line.id)?;
    if drawing.context.line.formula.is_some() {
        return None;
    }
    let operand = match &place.target {
        PlaceTarget::Field {
            phrase,
            field: FieldPlace::FirstOperand,
        } if phrase.is_empty() => draft.first.as_deref(),
        PlaceTarget::Field {
            phrase,
            field: FieldPlace::SecondOperand,
        } if phrase.is_empty() => draft.second.as_deref(),
        _ => None,
    }?;
    Some(drawing.context.sentences.formula_operand(operand))
}

// Choices and the edits they run

/// Returns the edit that puts `choice` at `place` of the line being drawn, or `None` when the choice opens a dialog or
/// only changes a draft.
fn choice_edit(
    drawing: &PartDrawing<'_>,
    state: &mut CardState,
    place: &PartPlace,
    choice: &Choice,
) -> Option<CardEdit> {
    let line_id = drawing.line.id.clone();
    if *choice == Choice::NewParameter {
        let number_use = match place.want {
            Some(Blank::Chance) => NumberUse::Chance,
            Some(Blank::Rate) => NumberUse::Rate,
            Some(Blank::Times) => NumberUse::Times,
            _ => NumberUse::Number,
        };
        let line = drawing.context.line;
        let typed = value_at(line, &place.target).and_then(|value| match value {
            SheetValue::Literal(literal) => literal.number(),
            _ => None,
        });
        let unit = (number_use == NumberUse::Rate)
            .then(|| rate_unit(drawing.context.document, line))
            .flatten();
        state.new_parameter = Some((
            PlaceRef {
                line: line_id,
                place: place.clone(),
            },
            NewParameterDialog::new(number_use, typed, unit),
            false,
        ));
        return None;
    }
    let draft = state.drafts.get(&line_id).cloned();
    let applied = apply_choice(drawing.context.document, draft, &line_id, place, choice);
    if applied.edits.is_empty() {
        if let Some((line, draft)) = applied.draft {
            match draft {
                Some(draft) => state.drafts.insert(line, draft),
                None => state.drafts.remove(&line),
            };
        }
        return None;
    }
    let mut edit = CardEdit::commands(
        applied.edits,
        After::Next {
            line: line_id,
            target: place.target.clone(),
            want: place.want,
        },
    );
    edit.draft = applied.draft;
    Some(edit)
}

/// Returns the words of the unit a new parameter of a rate on `line` takes, "per day" from the rate's unit or the
/// tick's, `None` with neither.
fn rate_unit(document: &SheetDocument, line: &SheetLine) -> Option<String> {
    let phrase = match line.kind {
        LineKind::Rate => Some(line),
        _ => match line.condition.value() {
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Rate => Some(phrase.as_ref()),
            _ => None,
        },
    };
    let unit = phrase
        .and_then(|phrase| phrase.rate_unit)
        .or_else(|| document.time.map(|time| time.tick_unit))?;
    Some(henad_graph::sheet::sentence::unit_suggestion(unit))
}

// The selects

/// Draws the options of the select at `place` under `anchor`, and returns the option chosen.
fn select_popup(
    ui: &Ui,
    drawing: &PartDrawing<'_>,
    place: &PartPlace,
    anchor: &Response,
    popup_id: Id,
) -> Option<SelectOption> {
    let line = drawing.context.line;
    let options = match &place.target {
        PlaceTarget::Field {
            phrase,
            field: FieldPlace::Test,
        } => {
            let phrase = if phrase.is_empty() {
                Some(line)
            } else {
                slot_at(line, phrase)
                    .and_then(Slot::value)
                    .and_then(|value| match value {
                        SheetValue::Phrase(phrase) => Some(phrase.as_ref()),
                        _ => None,
                    })
            };
            let of_states = phrase
                .and_then(|phrase| read_property(phrase.left.value()))
                .and_then(|id| property_of(drawing.context.document, id))
                .is_some_and(|property| property.property_type == PropertyType::States);
            comparison_options(of_states)
        }
        PlaceTarget::Field {
            field: FieldPlace::Around,
            ..
        } => around_options(),
        PlaceTarget::Field {
            field: FieldPlace::Summary,
            ..
        } => summary_options(),
        PlaceTarget::Field {
            field: FieldPlace::Operation,
            ..
        } => operation_options(),
        PlaceTarget::Field {
            field: FieldPlace::RateUnit,
            ..
        } => unit_options(),
        PlaceTarget::Field {
            field: FieldPlace::Mode,
            ..
        } => mode_options(line.kind == LineKind::Steer),
        PlaceTarget::Field {
            field: FieldPlace::Pick,
            ..
        } => pick_options(),
        PlaceTarget::Field {
            field: FieldPlace::Which,
            ..
        } => which_options(),
        PlaceTarget::Field {
            field: FieldPlace::LookSummary(index),
            ..
        } => look_summary_options(*index, line.summaries.len() > 1),
        _ => Vec::new(),
    };
    if anchor.clicked() {
        Popup::toggle_id(ui.ctx(), popup_id);
    }
    let popup = Popup::from_response(anchor)
        .id(popup_id)
        .open_memory(None)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mut chosen = None;
            for (words, option) in &options {
                if ui.add(Button::new(words).frame(false)).clicked() {
                    chosen = Some(*option);
                }
            }
            if chosen.is_some() {
                ui.close();
            }
            chosen
        })?;
    popup.inner
}

/// Returns the edit that chooses `option` in the select at `place`, or `None` when it only changes a draft.
fn select_edit(
    drawing: &PartDrawing<'_>,
    state: &mut CardState,
    place: &PartPlace,
    option: SelectOption,
) -> Option<CardEdit> {
    let line_id = drawing.line.id.clone();
    let line = drawing.context.line;
    let PlaceTarget::Field { phrase, .. } = &place.target else {
        return None;
    };
    let field = match option {
        SelectOption::Mode(mode) => LineField::Mode(Some(mode)),
        SelectOption::Pick(pick) => LineField::Pick(Some(pick)),
        SelectOption::Which(which) => LineField::Which(Some(which)),
        SelectOption::LookSummary(index, kind) => {
            let mut summaries = line.summaries.clone();
            let summary = summaries.get_mut(index)?;
            summary.summary = Some(kind);
            if kind == SummaryKind::Count {
                summary.value = Slot::Blank;
            }
            LineField::Summaries(summaries)
        }
        SelectOption::RemoveSummary(index) => {
            return Some(CardEdit::commands(
                vec![SheetEdit::DeleteSummary { line: line_id, index }],
                After::Stay,
            ));
        }
        SelectOption::Test(test) => LineField::Test(Some(test)),
        SelectOption::Around(around) => LineField::Around(Some(around)),
        SelectOption::Unit(unit) => LineField::RateUnit(Some(unit)),
        SelectOption::Summary(kind) => {
            let mut summary = line.summaries.first()?.clone();
            summary.summary = Some(kind);
            LineField::Summaries(vec![summary])
        }
        SelectOption::Operation(_) if !phrase.is_empty() => return None,
        SelectOption::Operation(operator) => {
            let draft = state.drafts.get(&line_id).cloned();
            let applied = calculate_edit(&line_id, line, draft, |draft| {
                draft.operator = operator;
            });
            if applied.edits.is_empty() {
                if let Some((line, Some(draft))) = applied.draft {
                    state.drafts.insert(line, draft);
                }
                return None;
            }
            let mut edit = CardEdit::commands(applied.edits, After::Stay);
            edit.draft = applied.draft;
            return Some(edit);
        }
    };
    let edits = vec![SheetEdit::SetField {
        line: line_id.clone(),
        phrase: phrase.clone(),
        field,
    }];
    Some(CardEdit::commands(
        edits,
        After::Next {
            line: line_id,
            target: place.target.clone(),
            want: None,
        },
    ))
}

/// Returns the words a select shows: its chosen words, or while empty [`UNIT_PLACEHOLDER`] for a unit and
/// [`BLANK_TEXT`] otherwise.
fn select_words(select: LineSelect, words: &str) -> &str {
    match (words.is_empty(), select) {
        (false, _) => words,
        (true, LineSelect::UnitOfTime) => UNIT_PLACEHOLDER,
        (true, _) => BLANK_TEXT,
    }
}

/// Adds a select inside a line showing `words` and a chevron, named `name`, under the widget id `id` when one is given,
/// and returns its response. An `empty` select shows its words in grey.
fn select_ui(ui: &mut Ui, id: Option<Id>, words: &str, name: &str, empty: bool) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let color = if empty { mcs::GRAY_400 } else { mcs::GRAY_100 };
    let painter = ui.painter();
    let text = painter.layout_no_wrap(words.to_owned(), font.clone(), color);
    let chevron = painter.layout_no_wrap(format!(" {MDI_CHEVRON_DOWN}"), font, mcs::GRAY_400);
    let width = text.size().x + chevron.size().x + CHIP_PADDING.x * 2.0;
    let height = (text.size().y + CHIP_PADDING.y * 2.0).max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = allocate_control(ui, id, vec2(width, height));
    response.widget_info(|| WidgetInfo {
        current_text_value: (!words.is_empty()).then(|| words.to_owned()),
        ..WidgetInfo::labeled(
            if words.is_empty() {
                WidgetType::Button
            } else {
                WidgetType::ComboBox
            },
            true,
            name,
        )
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let outline = if response.has_focus() || response.hovered() {
            mcs::GRAY_300
        } else {
            mcs::GRAY_700
        };
        painter.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, outline),
            egui::StrokeKind::Inside,
        );
        let text_size = text.size();
        let left = Pos2::new(rect.left() + CHIP_PADDING.x, rect.center().y - text_size.y / 2.0);
        painter.galley(left, text, color);
        let chevron_size = chevron.size();
        painter.galley(
            Pos2::new(left.x + text_size.x, rect.center().y - chevron_size.y / 2.0),
            chevron,
            mcs::GRAY_400,
        );
    }
    response
}

// The line menu and the rename field

/// A tool of a line's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineTool {
    /// Rename the line's result.
    Rename,
    /// Count the agent itself among the agents a look at agents looks at, or leave it out.
    IncludeMe,
    /// Move the line up.
    MoveUp,
    /// Move the line down.
    MoveDown,
    /// Give the line's draws a new key.
    DrawNewNumbers,
    /// Turn the line's chance into a rate.
    ChangeToRate,
    /// Turn the line's rate into a chance.
    ChangeToChance,
    /// Delete the line.
    Delete,
}

/// Draws the menu of `line`, "Line 2 actions": Rename on a result line, Move up and Move down where the outline
/// lets the line move, Draw new numbers on a line that draws, Change to a rate or Change to a chance on a line whose
/// typed chance or rate can change, and Delete.
fn line_menu_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line: &CardLine,
    line_data: &SheetLine,
    state: &mut CardState,
    pending: &mut Option<CardEdit>,
) {
    let id = state.widgets.with((&line.id, "menu"));
    let response = text_control(
        ui,
        Some(id),
        MDI_DOTS_HORIZONTAL,
        mcs::GRAY_400,
        &line_actions_name(&line.prefix),
    );
    let is_result = is_result_line(line_data.kind);
    let looks_at_agents = line_data.around == Some(Around::AgentsWithin);
    let includes_me = line_data.include_me == Some(true);
    let (up, down) = can_move(relations, &line.id);
    let draws = holds_draw(line_data);
    let change = conversion(document, &line.id);
    let is_when = line_data.kind == LineKind::When;
    let chosen = Popup::menu(&response)
        .id(id.with("popup"))
        .show(|ui| {
            let mut chosen = None;
            let mut tool = |ui: &mut Ui, shown: bool, enabled: bool, words: String, tool: LineTool| {
                if shown && ui.add_enabled(enabled, Button::new(words)).clicked() {
                    chosen = Some(tool);
                }
            };
            tool(ui, is_result, true, rename_line(&line.word), LineTool::Rename);
            let include = if includes_me {
                leave_me_out_tool(&line.word)
            } else {
                include_me_tool(&line.word)
            };
            tool(ui, looks_at_agents, true, include, LineTool::IncludeMe);
            tool(ui, true, up, move_line_up(&line.word), LineTool::MoveUp);
            tool(ui, true, down, move_line_down(&line.word), LineTool::MoveDown);
            tool(ui, draws, true, DRAW_NEW_NUMBERS.to_owned(), LineTool::DrawNewNumbers);
            let to_rate = change == Some(DrawKind::Rate);
            tool(ui, to_rate, true, CHANGE_TO_RATE.to_owned(), LineTool::ChangeToRate);
            let to_chance = change == Some(DrawKind::Chance);
            tool(
                ui,
                to_chance,
                true,
                CHANGE_TO_CHANCE.to_owned(),
                LineTool::ChangeToChance,
            );
            let delete = ui.button(delete_line(&line.word));
            let delete = if is_when {
                delete.on_hover_text(DELETE_WHEN_TOOLTIP)
            } else {
                delete
            };
            if delete.clicked() {
                chosen = Some(LineTool::Delete);
            }
            chosen
        })
        .and_then(|popup| popup.inner);
    let edit = |edit: SheetEdit| Some(CardEdit::commands(vec![edit], After::Stay));
    let line_id = line.id.clone();
    match chosen {
        Some(LineTool::Rename) => {
            state.renaming = Some(Renaming {
                line: line_id,
                text: line_data.name.clone().unwrap_or_default(),
                message: None,
                focused: false,
            });
        }
        Some(LineTool::IncludeMe) => {
            *pending = edit(SheetEdit::SetField {
                line: line_id,
                phrase: String::new(),
                field: LineField::IncludeMe(Some(!includes_me)),
            });
        }
        Some(LineTool::MoveUp) => {
            *pending = edit(SheetEdit::MoveLine {
                line: line_id,
                direction: MoveDirection::Up,
            });
        }
        Some(LineTool::MoveDown) => {
            *pending = edit(SheetEdit::MoveLine {
                line: line_id,
                direction: MoveDirection::Down,
            });
        }
        Some(LineTool::DrawNewNumbers) => *pending = edit(SheetEdit::DrawNewNumbers { line: line_id }),
        Some(LineTool::ChangeToRate) => *pending = edit(SheetEdit::ChangeToRate { line: line_id }),
        Some(LineTool::ChangeToChance) => *pending = edit(SheetEdit::ChangeToChance { line: line_id }),
        Some(LineTool::Delete) => *pending = edit(SheetEdit::DeleteLine { line: line_id }),
        None => {}
    }
}

/// Returns whether line `line_id` can move up and whether it can move down in the outline of its card.
///
/// Only the card's first line stays at the top, and only a line at the top level with no line or block after it stays
/// at the bottom. A line inside a block moves out of it at either end.
fn can_move(relations: &CardRelations<'_>, line_id: &str) -> (bool, bool) {
    let up = relations.index(line_id).is_some_and(|index| index > 0);
    let down = relations.parent(line_id).is_some()
        || relations.index(line_id).is_some_and(|index| {
            relations.order()[index + 1..]
                .iter()
                .any(|later| relations.parent(later).is_none())
        });
    (up, down)
}

/// Draws the field that renames `line`, under it. Enter or leaving the field keeps the name, and Escape cancels.
fn rename_ui(ui: &mut Ui, line: &CardLine, state: &mut CardState, pending: &mut Option<CardEdit>) {
    let Some(renaming) = &mut state.renaming else {
        return;
    };
    let mut keep = None;
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.add_space(INDENT * line.depth as f32 + NUMBER_WIDTH);
        let field = ui.add(TextEdit::singleline(&mut renaming.text).desired_width(RENAME_WIDTH));
        set_name(ui, field.id, &rename_field_name(&line.prefix));
        if !renaming.focused {
            field.request_focus();
            renaming.focused = true;
        }
        if field.lost_focus() {
            if ui.input(|input| input.key_pressed(Key::Escape)) {
                cancel = true;
            } else {
                keep = Some(renaming.text.trim().to_owned());
            }
        }
    });
    if let Some(message) = &renaming.message {
        ui.horizontal(|ui| {
            ui.add_space(INDENT * line.depth as f32 + NUMBER_WIDTH);
            message_ui(ui, message);
        });
    }
    if cancel {
        state.renaming = None;
        return;
    }
    let Some(name) = keep else {
        return;
    };
    if name.is_empty() {
        state.renaming = None;
        return;
    }
    let mut edit = CardEdit::commands(
        vec![SheetEdit::RenameResult {
            line: line.id.clone(),
            name,
        }],
        After::Stay,
    );
    edit.rename = true;
    *pending = Some(edit);
}

// The New parameter dialog

/// Draws the New parameter dialog while it is open. Create adds the parameter and puts it in the place it was opened
/// from, as one edit.
fn new_parameter_dialog(ui: &Ui, document: &SheetDocument, state: &mut CardState, pending: &mut Option<CardEdit>) {
    let Some((target, dialog, drawn)) = &mut state.new_parameter else {
        return;
    };
    if !*drawn {
        // The click that chose "New parameter..." would land on the dialog's backdrop and close it.
        *drawn = true;
        ui.ctx().request_repaint();
        return;
    }
    let taken: Vec<&str> = document
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect();
    match new_parameter_modal(ui.ctx(), dialog, &taken) {
        DialogOutcome::Open => {}
        DialogOutcome::Canceled => state.new_parameter = None,
        DialogOutcome::Created(parameter) => {
            let target = target.clone();
            let draft = state.drafts.get(&target.line).cloned();
            let late = Rc::new(RefCell::new(None));
            let late_draft = Rc::clone(&late);
            let after = After::Next {
                line: target.line.clone(),
                target: target.place.target.clone(),
                want: target.place.want,
            };
            *pending = Some(CardEdit {
                run: Box::new(move |builder| {
                    let made = builder.apply(SheetEdit::AddParameter {
                        name: parameter.name,
                        parameter_type: parameter.parameter_type,
                        min: parameter.min,
                        max: parameter.max,
                        default: parameter.default,
                        unit: parameter.unit,
                    })?;
                    let id = made.first().cloned().unwrap_or_default();
                    let choice = Choice::Value(SheetValue::Chip(SheetChip::Param(id)));
                    let applied = apply_choice(builder.document(), draft, &target.line, &target.place, &choice);
                    for edit in applied.edits {
                        builder.apply(edit)?;
                    }
                    *late.borrow_mut() = applied.draft;
                    Ok(made)
                }),
                after,
                rename: false,
                draft: None,
                late_draft: Some(late_draft),
            });
            state.new_parameter = None;
        }
    }
}
