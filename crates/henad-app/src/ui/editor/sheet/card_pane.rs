//! The Card pane: each card of the sheet under its heading, with its lines numbered from 1 in display order, written
//! and edited on the card.
//!
//! A card is a framed box: its heading, the law of a card that runs each tick, one row per line and a footer of add
//! buttons. A row holds a bar in the hue of the line's *kind*, the group of the add button that adds such a line, then
//! the line's number, its sentence with the notes of the sheet's check under it, and its menu. The add buttons carry
//! the same hues, so they double as the legend.
//!
//! A line reads as its sentence from [`henad_graph::sheet::sentence`], drawn part by part: words with its lead words in
//! the semibold face, a chosen value as a *chip* whose ground and border tell what it refers to, an empty slot as a
//! *blank*, a select inside the line, the grey "always" and the muted "otherwise" words. A line inside a When
//! or a Repeat is indented one step per level, with a guide down each level.
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
//! A card over agents offers the lines of agents under the same add buttons, and "+ Repeat…" after them: looks
//! at the agents within a distance with their summaries, picks and looks at cells of the ground, random numbers and
//! directions, Choose and Depending on, Steer, Keep between, Move, Leave and Record. A summary of a look at agents
//! names itself after what it sums up, and "+" after the summaries adds another. A problem the lowering finds in a
//! line, such as a Set of a property another line steers, shows under that line.
//!
//! Each line is a group named "Line 2: " and its sentence, and each control inside it is named by its visible words,
//! a blank or a select by what it wants and where it is ("Line 2: choose the old state"). Hovering or focusing
//! "otherwise" or a result chip highlights the line it names.

use std::collections::{BTreeMap, BTreeSet};

use egui::accesskit::Role;
use egui::{
    Align, Color32, CornerRadius, FontId, Frame, Id, Key, Label, Layout, Margin, Popup, Pos2, Rect, Response, RichText,
    Sense, Shape, Sides, Stroke, TextEdit, TextStyle, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use henad_graph::sheet::check::{SheetReport, holds_draw};
use henad_graph::sheet::document::{
    Around, CELLS_ID, CardRuns, CellWhich, ChooseRow, CompareTest, GROUND_ID, LineKind, LineMode, LookSummary, MadeOf,
    PickRule, PropertyType, SheetCard, SheetChip, SheetDocument, SheetLine, SheetValue, Slot, SummaryKind,
};
use henad_graph::sheet::edit::{
    LineField, MoveDirection, SheetBuilder, SheetEdit, SheetEditError, automatic_name, slot_value,
};
use henad_graph::sheet::rates::{chance_note, conversion, count_as_draw, grow_count, parameter_defaults, rate_note};
use henad_graph::sheet::relations::{CardRelations, RelationNote, RelationNoteKind};
use henad_graph::sheet::sentence::{
    ADD_AT_END_NOTE, ALWAYS, ALWAYS_TOOLTIP, AddButton, AddItem, BLANK_MODEL_CARD, BUTTON_CARD_EMPTY, Blank,
    CONDITION_ARROW_TOOLTIP, ChipKind, DELETE_WHEN_TOOLTIP, DRAW_NEW_NUMBERS, DrawKind, FOR_EACH_BLANK_WORDS,
    FOR_EACH_WORDS, FROM_WORD, FieldPlace, LAW, LineSelect, MOVE_DOWN_LAST, MOVE_UP_FIRST, ONE_SIDED_RANGE, PartPlace,
    PlaceTarget, PlacedPart, REMOVE_THIS_WHEN, SHARED_DRAW_GROUP, SentenceChip, SentencePart, Sentences,
    add_inside_label, add_inside_name, add_value_name, card_header, condition_arrow_name, count_used_as_draw,
    delete_line, for_each_optional_tooltip, for_each_words, from_tooltip, line_actions_name, line_count,
    line_list_name, move_into_label, move_into_name, move_line_down, move_line_up, operation_words, otherwise_words,
    remove_when_name, rename_field_name, rename_line, rename_result_name, result_name_taken, use_for_each_label,
    use_for_each_name, use_instead_label, use_instead_name,
};
use henad_graph::sheet::sentence::{
    CHANGE_TO_CHANCE, CHANGE_TO_RATE, COMPETING_WORD, DELETE_LINE_HEADING, LOOK_IN_REPEAT, RANDOM_VALUE_GROUP,
    REMOVE_SUMMARY_HEADING, add_choose_row_name, add_for_each_label, add_for_each_name, add_summary_name,
    comparison_words, drag_line_tooltip, fire_count, fire_count_description, include_me_tool, leave_me_out_tool,
    long_tick_note, rate_grow_prompt, tick_set_notice,
};

use super::card_drag::{DragFrame, DrawnCard, DrawnRow, LineDrag, RowOf, faded, handle_ui};
#[cfg(test)]
use super::card_lines::LineControl;
pub use super::card_lines::{CardLine, LineNote, NoteColor, card_lines, draws_diagnostic};
use super::card_lines::{NoteAction, button_of, select_words};
use super::delete_question::{DeleteAnswer, DeleteQuestion};
use super::help::{Help, help_mark};
use super::picker::{
    CalcDraft, CardPicker, Choice, DraftUpdate, LineContext, SelectOption, apply_choice, around_options,
    calculate_edit, choose_when, comparison_options, list_len, look_summary_options, looked_at_picker, mode_options,
    operation_options, phrase_of, pick_options, picker_for, read_property, summary_options, summary_picker,
    takes_ground_states, typed_text, unit_options, value_at, which_options,
};
use super::sheet_pane::SheetTrial;
use super::widgets::{
    ARROW_WIDTH, DialogOutcome, FocusTooltip as _, KIND_BAR_WIDTH, NewParameterDialog, NumberUse, OverlayItem,
    REGION_GAP, add_row_button, arrow_control, blank_shapes, blank_ui, caption, chip_with_id, escape_to_anchor,
    hairline, kind_bar, menu_button, menu_group, menu_item, message_ui, new_parameter_modal, note_button, notice_ui,
    overlay, overlay_group, overlay_items, overlay_title, parameter_names, placed, refusal_text, region,
    select_control, select_list, set_name, slot_outline, small_label, text_control, title_text,
};
use super::{LineFires, SheetEditor, accessible_group, find_property, over_agents, properties_of};
use crate::init::semibold_font;
use crate::ui::mcs;

// The layout of a card

/// Width in points of the gutter of line numbers, between the kind bar and the body of a row.
const GUTTER_WIDTH: f32 = 30.0;

/// Distance in points from the right edge of a line number to the body of its row. The bracket of a competing group
/// runs in it.
const NUMBER_GAP: f32 = 9.0;

/// Indent in points of a line's body for each block around it.
const INDENT: f32 = 20.0;

/// Distance in points from the start of a level's indent to its guide.
const GUIDE_OFFSET: f32 = 5.0;

/// Padding in points above and below the contents of a row.
const ROW_PADDING: f32 = 4.0;

/// Height in points of one row of a sentence. A chip is [`CHIP_HEIGHT`](super::widgets::CHIP_HEIGHT) high. Two rows
/// of chips never touch.
const SENTENCE_ROW: f32 = 24.0;

/// Start and length in points of the dash in the indent of an alternative of a Choose or a Depending on, level with its
/// first row.
const ALTERNATIVE_DASH: (f32, f32) = (6.0, 7.0);

/// Size in points of the words and chips of a sentence.
const SENTENCE_SIZE: f32 = 13.0;

/// Size in points of the law.
const LAW_SIZE: f32 = 12.0;

/// Inset in points of the title and the law from the card's left edge.
const CARD_INSET: f32 = 10.0;

/// Width in points of the column at the right of a row that holds the line's menu.
const MENU_WIDTH: f32 = 28.0;

/// Gap in points between the body of a row and the column at its right.
const TAIL_GAP: f32 = 6.0;

/// Width in points of the column at the right of a card that runs each tick, which holds the fire count of each line.
const COUNT_WIDTH: f32 = 72.0;

/// Width in points of the field that renames a result.
const RENAME_WIDTH: f32 = 200.0;

/// Thickness in points of the mark in the gutter of the row that holds the keyboard focus.
const FOCUS_MARK_WIDTH: f32 = 2.0;

/// Length in points of the mark in the gutter of the row that holds the keyboard focus.
const FOCUS_MARK_LENGTH: f32 = 14.0;

/// Size in points of a note under a line.
const NOTE_SIZE: f32 = 11.5;

/// Distance in points from the kind bar to the centre of the grip that shows in the gutter of a row under the pointer.
const GRIP_OFFSET: f32 = 5.0;

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
    /// Ids of the lines whose "is less than … or more than …" is being filled. The one-side offer waits on them.
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
    /// Delete of a line that waits on the dialog that asks before it empties formulas the card cannot write again,
    /// `None` while the dialog is closed.
    delete_question: Option<DeleteQuestion>,
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
            delete_question: None,
        }
    }

    /// Keeps the draft that `update` leaves for its Calculate line, or drops the line's draft when it leaves none.
    fn keep_draft(&mut self, (line, draft): DraftUpdate) {
        match draft {
            Some(draft) => self.drafts.insert(line, draft),
            None => self.drafts.remove(&line),
        };
    }

    /// Closes the rename field, the New parameter dialog and the dialog that asks before a delete, drops every draft
    /// and message, and ends every range being filled, as an undo or a redo does.
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
        self.delete_question = None;
    }

    /// Returns whether the New parameter dialog or the dialog that asks before a delete is open.
    pub fn dialog_open(&self) -> bool {
        self.new_parameter.is_some() || self.delete_question.is_some()
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

/// Move of the focus after an edit of the card.
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
        /// Kind of value the place wanted.
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

/// The commands of one edit of the card, as a closure that returns the ids of what they made and the draft of a
/// Calculate line they leave, `None` for commands that leave the drafts alone.
type CardRun = Box<dyn FnOnce(&mut SheetBuilder) -> Result<(Vec<String>, Option<DraftUpdate>), SheetEditError>>;

/// An edit collected while the card is drawn, run once the card is drawn.
struct CardEdit {
    /// The commands.
    run: CardRun,
    /// Move of the focus after it.
    after: After,
    /// Whether a refusal belongs under the rename field.
    rename: bool,
}

impl CardEdit {
    /// Returns the edit that runs `edits` in order, then moves the focus as `after` says.
    fn commands(edits: Vec<SheetEdit>, after: After) -> Self {
        Self::with_draft(edits, after, None)
    }

    /// Returns the edit that runs `edits` in order and leaves `draft`, then moves the focus as `after` says.
    fn with_draft(edits: Vec<SheetEdit>, after: After, draft: Option<DraftUpdate>) -> Self {
        Self {
            run: Box::new(move |builder| {
                let mut made = Vec::new();
                for edit in edits {
                    made.extend(builder.apply(edit)?);
                }
                Ok((made, draft))
            }),
            after,
            rename: false,
        }
    }
}

/// Draws every card of `sheet` and runs each edit made on it. `past_start` is whether the preview has passed tick 0 or
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
    let mut pending = Vec::new();
    // A delete held in the frame before asks first. It ran on the sheet as it is drawn now.
    if let Some(question) = &sheet.card.delete_question
        && let Some(answer) = question.show(ui.ctx())
        && let Some(question) = sheet.card.delete_question.take()
        && answer == DeleteAnswer::Delete
    {
        pending.push(CardEdit::commands(vec![question.edit], After::Stay));
    }
    {
        let document = sheet.builder.document();
        let state = &mut sheet.card;
        for notice in [&state.tick_notice, &state.otherwise_notice].into_iter().flatten() {
            notice_ui(ui, notice);
            ui.add_space(4.0);
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
        ui.scope(|ui| {
            ui.style_mut()
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(SENTENCE_SIZE));
            cards_ui(ui, document, &shown, state, &mut pending);
        });
        if let Some(refusal) = &state.refusal {
            message_ui(ui, refusal);
        }
        new_parameter_dialog(ui, document, state, &mut pending);
    }
    // A field that loses the focus to a click commits in the frame of the click, and each edit runs on the sheet as
    // the edit before it left it.
    for edit in pending {
        run_card_edit(sheet, past_start, edit);
    }
}

/// Runs `edit` on `sheet`, then moves the focus and keeps the drafts and marks it leaves.
fn run_card_edit(sheet: &mut SheetEditor, past_start: bool, edit: CardEdit) {
    let CardEdit { run, after, rename } = edit;
    let mut made = Vec::new();
    let mut draft = None;
    let timed = sheet.document().time.is_some();
    let result = sheet.edit(past_start, false, |builder| {
        (made, draft) = run(builder)?;
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
                _ => state.refusal = Some(refusal_text(&error)),
            }
            return;
        }
    }
    if let Some(update) = draft {
        state.keep_draft(update);
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
/// "is less than … or more than …".
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
    pending: &mut Vec<CardEdit>,
) {
    let report = shown.report;
    let values = shown.values;
    let sentences = Sentences::new(document);
    let highlight_id = ui.id().with("henad_card_highlight");
    let highlight: Vec<String> = ui.data(|data| data.get_temp(highlight_id)).unwrap_or_default();
    let mut hovered: Vec<String> = Vec::new();
    let every = match document.world.made_of {
        MadeOf::Agents => document.agents.first().map(|kind| kind.id.as_str()),
        _ => Some(CELLS_ID),
    };
    let mut drag_frame = DragFrame::new(ui, document, state.widgets);
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
        region(ui, |ui| {
            title_bar_ui(ui, &card_header(&sentences.thing_word(every)), None, None);
            empty_card_ui(ui, BLANK_MODEL_CARD);
            footer_ui(ui, None, |ui| add_buttons_ui(ui, document, state, None, every, pending));
        });
        ui.add_space(REGION_GAP);
    }
    let mut overridden: Vec<(String, String)> = Vec::new();
    for (index, card) in document.cards.iter().enumerate() {
        if index > 0 {
            ui.add_space(REGION_GAP);
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
            counted: shown.counted,
        };
        card_ui(ui, &drawing, &lines, state, &mut hovered, &mut drag_frame, pending);
    }
    drag_frame.finish(ui);
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

/// Values every line of one card reads while the card is drawn.
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
    /// Ids of the lines with a fire count in the last lowering of the sheet. Such a line on a card that runs each tick
    /// keeps a column at its right for its count, whether or not the run has counted a tick.
    counted: &'a BTreeSet<String>,
}

/// Draws the card of `drawing` in its frame: the title bar, the law under a card that runs every tick, a row for each
/// of the numbered `lines` and the footer of add buttons.
///
/// The ids of the lines that a hovered or focused control names are added to `hovered`. While a line of the card is
/// dragged, its rows fade, and `drag_frame` marks the place under the pointer and drops the line there.
fn card_ui(
    ui: &mut Ui,
    drawing: &CardDrawing<'_>,
    lines: &[CardLine],
    state: &mut CardState,
    hovered: &mut Vec<String>,
    drag_frame: &mut DragFrame,
    pending: &mut Vec<CardEdit>,
) {
    let card = drawing.card;
    let each_tick = card.runs == CardRuns::EachTick;
    let heading = drawing.sentences.card_heading(card);
    let agents = !matches!(card.every.as_str(), CELLS_ID | GROUND_ID);
    let help = each_tick.then_some(if agents { Help::AgentLines } else { Help::Lines });
    let list_name = line_list_name(button_of(drawing.document, card));
    let relations = CardRelations::new(drawing.document, card);
    let report = drawing.report;
    let notes = relations.notes(drawing.sentences, |line| !report.is_skipped(line));
    let closing = closing_blocks(&relations, lines);
    region(ui, |ui| {
        title_bar_ui(ui, &heading, Some(lines.len()), help);
        if each_tick {
            law_ui(ui);
        }
        if lines.is_empty() {
            empty_card_ui(ui, if each_tick { BLANK_MODEL_CARD } else { BUTTON_CARD_EMPTY });
        }
        let body_left = |rect: Rect, depth: usize| rect.left() + KIND_BAR_WIDTH + GUTTER_WIDTH + INDENT * depth as f32;
        let list = ui.scope(|ui| {
            let mut rows = Vec::with_capacity(lines.len());
            let mut rows_drawn = Vec::with_capacity(lines.len());
            for (index, line) in lines.iter().enumerate() {
                let row = RowPlace {
                    first: index == 0,
                    depth: line.depth,
                };
                let rect = faded(ui, drag_frame.moves(&relations, &line.id), |ui| {
                    line_ui(ui, drawing, &relations, &notes, line, &row, state, hovered, pending)
                });
                rows.push(rect);
                rows_drawn.push(DrawnRow {
                    rect,
                    row: RowOf::Line(line.id.clone()),
                    body_left: body_left(rect, line.depth),
                });
                for block in closing.get(&index).into_iter().flatten() {
                    let rect = faded(ui, drag_frame.moves(&relations, &block.id), |ui| {
                        add_inside_ui(ui, drawing, block, state, pending)
                    });
                    rows_drawn.push(DrawnRow {
                        rect,
                        row: RowOf::BlockEnd(block.id.clone()),
                        body_left: body_left(rect, block.depth),
                    });
                }
            }
            for bracket in brackets(&relations, lines) {
                bracket_ui(ui, &rows, &bracket);
            }
            (ui.unique_id(), rows_drawn)
        });
        let (list_id, rows_drawn) = list.inner;
        accessible_group(ui.ctx(), list_id, Role::List, &list_name, list.response.rect);
        let note = card
            .line_tables
            .values()
            .any(|line| line.kind.is_block())
            .then_some(ADD_AT_END_NOTE);
        footer_ui(ui, note, |ui| {
            add_buttons_ui(ui, drawing.document, state, Some(card), &card.every, pending);
        });
        let drawn_card = DrawnCard {
            relations: &relations,
            lines,
            rows: &rows_drawn,
            rect: ui.min_rect(),
            top_body_left: body_left(ui.min_rect(), 0),
        };
        if let Some(drop) = drag_frame.place_ui(ui, &drawn_card) {
            pending.push(CardEdit {
                run: Box::new(move |builder| {
                    drop.run(builder)?;
                    Ok((Vec::new(), None))
                }),
                after: After::Stay,
                rename: false,
            });
        }
    });
}

/// Draws a card's title row, with a hairline under it: the `heading` in gray 100 and the semibold face, the help `help`
/// beside it where the card has one, and the number of lines `count` as a [`caption`] at the right edge.
fn title_bar_ui(ui: &mut Ui, heading: &str, count: Option<usize>, help: Option<Help>) {
    Frame::NONE
        .inner_margin(Margin {
            left: CARD_INSET as i8,
            right: CARD_INSET as i8,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            Sides::new().shrink_left().wrap().show(
                ui,
                |ui| {
                    ui.add(Label::new(title_text(ui, heading)));
                    if let Some(help) = help {
                        help_mark(ui, help);
                    }
                },
                |ui| {
                    if let Some(count) = count {
                        caption(ui, &line_count(count));
                    }
                },
            );
        });
    hairline(ui);
}

/// Draws the law of a card that runs each tick in gray 400, with a hairline under it.
fn law_ui(ui: &mut Ui) {
    Frame::NONE
        .inner_margin(Margin {
            left: CARD_INSET as i8,
            right: CARD_INSET as i8,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let words = RichText::new(LAW).size(LAW_SIZE).color(mcs::GRAY_400);
            ui.add(Label::new(words).wrap());
        });
    hairline(ui);
}

/// Draws `words` in an empty slot where the lines of an empty card would be: weak words inside a faded outline.
fn empty_card_ui(ui: &mut Ui, words: &str) {
    Frame::NONE.inner_margin(Margin::same(8)).show(ui, |ui| {
        Frame::NONE
            .inner_margin(Margin::symmetric(8, 6))
            .stroke(slot_outline(false))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(Label::new(RichText::new(words).weak()).wrap());
            });
    });
}

/// Draws a card's footer under a hairline: `note` in the weak text colour where one is given, then what `add` draws,
/// the add buttons.
fn footer_ui(ui: &mut Ui, note: Option<&str>, add: impl FnOnce(&mut Ui)) {
    hairline(ui);
    Frame::NONE.inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(note) = note {
            let words = RichText::new(note)
                .size(NOTE_SIZE)
                .color(ui.visuals().weak_text_color());
            ui.add(Label::new(words).wrap());
            ui.add_space(6.0);
        }
        add(ui);
    });
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

/// Draws the thin bracket that joins the rows `rows` of the lines of `bracket`, indices in `lines`, in the gutter
/// between the line numbers and the bodies of the rows.
fn bracket_ui(ui: &Ui, rows: &[Rect], bracket: &[usize]) {
    let (Some(&first), Some(&last)) = (bracket.first(), bracket.last()) else {
        return;
    };
    let (Some(top), Some(bottom)) = (rows.get(first), rows.get(last)) else {
        return;
    };
    let x = top.left() + KIND_BAR_WIDTH + GUTTER_WIDTH - NUMBER_GAP / 2.0;
    let (y0, y1) = (
        top.top() + ROW_PADDING + 4.0,
        bottom.top() + ROW_PADDING + SENTENCE_ROW - 4.0,
    );
    ui.painter().add(Shape::line(
        vec![
            Pos2::new(x + 3.0, y0),
            Pos2::new(x, y0),
            Pos2::new(x, y1),
            Pos2::new(x + 3.0, y1),
        ],
        Stroke::new(1.0, mcs::GRAY_500),
    ));
}

/// A When or Repeat block whose add row follows a line.
#[derive(Debug, Clone)]
struct ClosingBlock {
    /// Id of the block's line.
    id: String,
    /// Kind of the block's line, a When or a Repeat.
    kind: LineKind,
    /// Number of the block's line on its card.
    number: usize,
    /// Number of blocks around the lines inside the block.
    depth: usize,
    /// Whether a line added inside the block sits inside a Repeat: the block is a Repeat, or a block inside one.
    in_repeat: bool,
}

/// Returns the When and Repeat blocks of `lines` that close after each line, by the line's index, inner blocks first.
///
/// A block closes after the last line inside it, or after its When or Repeat line while it holds none.
fn closing_blocks(relations: &CardRelations<'_>, lines: &[CardLine]) -> BTreeMap<usize, Vec<ClosingBlock>> {
    let mut closing: BTreeMap<usize, Vec<ClosingBlock>> = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(kind) = relations
            .line(&line.id)
            .map(|data| data.kind)
            .filter(|kind| kind.is_block())
        else {
            continue;
        };
        let last = lines
            .iter()
            .enumerate()
            .skip(index + 1)
            .take_while(|(_, inner)| relations.is_within(&inner.id, &line.id))
            .last()
            .map_or(index, |(last, _)| last);
        closing.entry(last).or_default().push(ClosingBlock {
            id: line.id.clone(),
            kind,
            number: line.number,
            depth: line.depth + 1,
            in_repeat: kind == LineKind::Repeat || relations.repeat_of(&line.id).is_some(),
        });
    }
    for blocks in closing.values_mut() {
        blocks.sort_by_key(|block| std::cmp::Reverse(block.depth));
    }
    closing
}

/// Draws the row that ends `block`, "+ Add inside line 4", indented to the block's inner level. Its menu, "Add a line
/// inside line 4", lists every kind of line in groups, and an item adds its line at the end of the block.
///
/// On a card that runs once the menu holds the Change group alone, as the card's own add buttons offer "+ Change"
/// alone. Returns the row.
fn add_inside_ui(
    ui: &mut Ui,
    drawing: &CardDrawing<'_>,
    block: &ClosingBlock,
    state: &CardState,
    pending: &mut Vec<CardEdit>,
) -> Rect {
    let states = pattern_states(drawing.document, &drawing.card.every);
    let states = states.as_ref().map(|(new, old)| (new.as_str(), old.as_str()));
    let label = add_inside_label(block.number);
    let title = add_inside_name(block.number);
    let groups = add_inside_groups(drawing.card);
    let agents = over_agents(&drawing.card.every);
    let id = state.widgets.with(("add inside", &block.id));
    let menu_id = id.with("menu");
    let ground = ui.painter().add(Shape::Noop);
    let top = ui.cursor().top();
    let (left, right) = (ui.max_rect().left(), ui.max_rect().right());
    let body_left = left + KIND_BAR_WIDTH + GUTTER_WIDTH + INDENT * block.depth as f32;
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(body_left, top + ROW_PADDING),
                pos2(right - TAIL_GAP, top + ROW_PADDING + SENTENCE_ROW),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let open = Popup::is_id_open(row.ctx(), menu_id);
    let response = add_row_button(&mut row, &label, &label, None, open);
    let (popup, _) = placed(row.ctx(), Popup::menu(&response).id(menu_id).frame(Frame::NONE));
    let chosen = popup
        .show(|ui| {
            menu_dialog(ui, &title, |ui, _| {
                overlay_items(ui, |ui| {
                    let mut chosen = None;
                    for (index, &button) in groups.iter().enumerate() {
                        let group = overlay_group(ui, button.group(), index == 0).id;
                        if let Some(item) = add_items_ui(ui, group, button, agents, states, block.in_repeat) {
                            chosen = Some(item);
                        }
                    }
                    chosen
                })
            })
        })
        .and_then(|popup| popup.inner);
    escape_to_anchor(&row, &response, menu_id, open);
    let rect = Rect::from_min_max(
        pos2(left, top),
        pos2(
            right,
            row.min_rect().bottom().max(top + ROW_PADDING + SENTENCE_ROW) + ROW_PADDING,
        ),
    );
    ui.advance_cursor_after_rect(rect);
    paint_row(
        ui,
        rect,
        ground,
        &RowPaint {
            depth: block.depth,
            separator: true,
            highlight: false,
        },
    );
    // The row closes the block. Its bar takes the hue of the block's line.
    kind_bar(ui.painter(), rect, AddButton::for_kind(block.kind));
    if response.has_focus() {
        focus_mark(ui, rect);
    }
    if let Some(item) = chosen {
        pending.push(add_line_edit(
            Some(drawing.card.id.clone()),
            Some(block.id.clone()),
            item,
            &drawing.card.every,
        ));
    }
    rect
}

/// Where a row sits on its card.
struct RowPlace {
    /// Whether the row is the first of the card, with no separator above it.
    first: bool,
    /// Number of blocks around the row.
    depth: usize,
}

/// How a row is painted.
struct RowPaint {
    /// Number of blocks around the row. Each draws a guide down the row.
    depth: usize,
    /// Whether a separator runs along the row's top.
    separator: bool,
    /// Whether the row is highlighted as a line that a hovered or focused control names.
    highlight: bool,
}

/// Paints the row `rect` as `paint` says: the blue 950 tint of a highlighted line into the empty shape `ground`, the
/// hairline at its top and the guide of each block around it.
///
/// The guides of consecutive rows meet, so each runs unbroken down its block.
fn paint_row(ui: &Ui, rect: Rect, ground: egui::layers::ShapeIdx, paint: &RowPaint) {
    if paint.highlight {
        ui.painter()
            .set(ground, Shape::rect_filled(rect, CornerRadius::ZERO, mcs::BLUE_950));
    }
    let painter = ui.painter();
    let hairline = Stroke::new(1.0, mcs::GRAY_800);
    if paint.separator {
        painter.hline(rect.x_range(), rect.top() + 0.5, hairline);
    }
    let base = rect.left() + KIND_BAR_WIDTH + GUTTER_WIDTH + GUIDE_OFFSET;
    for level in 0..paint.depth {
        let x = base + INDENT * level as f32 + 0.5;
        painter.vline(x, rect.y_range(), hairline);
    }
}

/// Paints the mark of the row `rect` that holds the keyboard focus: a line of 2 points of blue 500 under its number in
/// the gutter.
fn focus_mark(ui: &Ui, rect: Rect) {
    let right = rect.left() + KIND_BAR_WIDTH + GUTTER_WIDTH - NUMBER_GAP;
    let bottom = rect.top() + ROW_PADDING + SENTENCE_ROW - 3.0;
    let mark = Rect::from_min_max(
        pos2(right - FOCUS_MARK_LENGTH, bottom - FOCUS_MARK_WIDTH),
        pos2(right, bottom),
    );
    ui.painter().rect_filled(mark, CornerRadius::ZERO, mcs::BLUE_500);
}

/// Returns the groups of the menu of a block's add row on `card`: the Change group alone on a card that runs once, and
/// otherwise every group of the card's kind of thing, Repeat among them on a card over agents.
fn add_inside_groups(card: &SheetCard) -> &'static [AddButton] {
    if card.runs == CardRuns::Once {
        &[AddButton::Change]
    } else if over_agents(&card.every) {
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
        counted: &BTreeSet::new(),
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

// The add buttons

/// Draws the add buttons under `card`, or under the heading of the card that runs each tick over `every` while there is
/// none. A card that runs once offers "+ Change" alone, and a card over agents adds "+ Repeat…".
///
/// Each button shows a square in the hue of the lines it adds. The buttons are the legend of the rows' bars.
fn add_buttons_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    state: &CardState,
    card: Option<&SheetCard>,
    every: &str,
    pending: &mut Vec<CardEdit>,
) {
    let once = card.is_some_and(|card| card.runs == CardRuns::Once);
    let agents = over_agents(every);
    let buttons: &[AddButton] = match (once, agents) {
        (true, _) => &[AddButton::Change],
        (false, true) => &AddButton::AGENTS,
        (false, false) => &AddButton::ALL,
    };
    let states = pattern_states(document, every);
    let card_id = card.map(|card| card.id.clone());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for button in buttons {
            let id = state.widgets.with(("add", card_id.as_deref(), button.label()));
            let menu_id = id.with("menu");
            let open = Popup::is_id_open(ui.ctx(), menu_id);
            let response = add_row_button(ui, button.label(), button.label(), Some(*button), open)
                .on_hover_or_focus_text(button.tooltip_for(agents));
            let (popup, _) = placed(ui.ctx(), Popup::menu(&response).id(menu_id).frame(Frame::NONE));
            let chosen = popup
                .show(|ui| {
                    add_menu_ui(
                        ui,
                        *button,
                        agents,
                        states.as_ref().map(|(new, old)| (new.as_str(), old.as_str())),
                    )
                })
                .and_then(|popup| popup.inner);
            escape_to_anchor(ui, &response, menu_id, open);
            if let Some(item) = chosen {
                pending.push(add_line_edit(card_id.clone(), None, item, every));
            }
        }
    });
}

/// Draws `add` inside the overlay of a menu titled `title`, with `title` as its heading, and returns what `add`
/// returns. `add` takes the id of the heading.
///
/// The overlay is a group of role `Dialog` named `title`.
fn menu_dialog<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui, Id) -> R) -> R {
    let menu = overlay(ui, |ui| {
        ui.scope(|ui| {
            let heading = overlay_title(ui, title).id;
            (add(ui, heading), ui.unique_id())
        })
    });
    let (inner, id) = menu.inner;
    accessible_group(ui.ctx(), id, Role::Dialog, title, menu.response.rect);
    inner
}

/// Draws the menu of the add button `button`, on a card over agents when `agents` holds: its heading, and each item
/// with its pattern in grey under it. Returns the item chosen.
///
/// The pattern is painted, so it stays out of the accessibility tree and the item is named by its label alone.
fn add_menu_ui(ui: &mut Ui, button: AddButton, agents: bool, states: Option<(&str, &str)>) -> Option<AddItem> {
    menu_dialog(ui, button.heading_for(agents), |ui, heading| {
        overlay_items(ui, |ui| add_items_ui(ui, heading, button, agents, states, false))
    })
}

/// Returns the heading of the group of an add menu that `item` opens, `None` for an item that opens none.
fn opens_group(item: AddItem) -> Option<&'static str> {
    match item {
        AddItem::WithChance => Some(SHARED_DRAW_GROUP),
        AddItem::RandomNumber => Some(RANDOM_VALUE_GROUP),
        _ => None,
    }
}

/// Draws each item of the menu of the add button `button`, on a card over agents when `agents` holds, with its pattern
/// in grey under it, and returns the item chosen.
///
/// The items until the first group of their own form a menu labelled by the node `label`, and each later group a menu
/// labelled by its heading. Inside a Repeat (`in_repeat`), an item whose line looks around, counts or picks a cell is
/// weak, with [`LOOK_IN_REPEAT`] as its tooltip, and adds nothing.
fn add_items_ui(
    ui: &mut Ui,
    label: Id,
    button: AddButton,
    agents: bool,
    states: Option<(&str, &str)>,
    in_repeat: bool,
) -> Option<AddItem> {
    let mut chosen = None;
    let mut label = label;
    let mut rest = button.items_for(agents);
    while let Some(first) = rest.first() {
        let end = rest
            .iter()
            .skip(1)
            .position(|item| opens_group(*item).is_some())
            .map_or(rest.len(), |index| index + 1);
        let (group, tail) = rest.split_at(end);
        rest = tail;
        if let Some(heading) = opens_group(*first) {
            label = overlay_group(ui, heading, false).id;
        }
        menu_group(ui, label, |ui| {
            for item in group {
                let pattern = item.pattern(states);
                let row = OverlayItem {
                    label: item.label(),
                    detail: Some(&pattern),
                    ..OverlayItem::default()
                };
                let refused = in_repeat && looks_in_place(new_line(*item, "").kind);
                let response = ui.add_enabled_ui(!refused, |ui| menu_item(ui, &row)).inner;
                if refused {
                    response.on_disabled_hover_text(LOOK_IN_REPEAT);
                } else if response.clicked() {
                    chosen = Some(*item);
                }
            }
        });
    }
    chosen
}

/// Returns whether a line of `kind` looks at the cells or agents around its place, which a Repeat cannot hold: a Count,
/// a Look around or a Pick a cell.
pub(super) fn looks_in_place(kind: LineKind) -> bool {
    matches!(kind, LineKind::Count | LineKind::LookAround | LineKind::PickCell)
}

/// Returns the second and first states of the first States property with two or more states of the kind of thing
/// `every`, as `(new, old)`, which the Become patterns of the add menus name.
fn pattern_states(document: &SheetDocument, every: &str) -> Option<(String, String)> {
    let property = properties_of(document, every)
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
            let made = builder.apply(SheetEdit::AddLine {
                card,
                inside,
                line: Box::new(line),
            })?;
            Ok((made, None))
        }),
        after: After::NewLine,
        rename: false,
    }
}

// The lines

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

/// Results the parts of one line gather while they are drawn.
struct LineDrawing<'a> {
    /// Ids of the lines that a hovered or focused "otherwise" or result chip names.
    hovered: &'a mut Vec<String>,
    /// Shape and place of each empty "for each …" blank, painted once the line is known to be hovered or focused.
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

/// Draws the row of `line` at `place`: the bar in the hue of its kind, its number, its sentence part by part with its
/// rename field and its notes under it, the relation notes of the card among them, then its fire count and its menu at
/// the row's top. Returns the row.
///
/// The body of the row is a column of its own, so a wrapped sentence and the notes start at the body's left edge, right
/// of the number. A line inside a block moves its body right one indent per level, and its number and bar stay.
#[expect(
    clippy::too_many_arguments,
    reason = "the card, its relations and notes, the line and the pane's state each take a part"
)]
#[expect(clippy::too_many_lines, reason = "the columns of one row read best in one place")]
fn line_ui(
    ui: &mut Ui,
    drawing: &CardDrawing<'_>,
    relations: &CardRelations<'_>,
    relation_notes: &[RelationNote],
    line: &CardLine,
    place: &RowPlace,
    state: &mut CardState,
    hovered: &mut Vec<String>,
    pending: &mut Vec<CardEdit>,
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
    let ground = ui.painter().add(Shape::Noop);
    let mut gathered = LineDrawing {
        hovered,
        for_each_shapes: Vec::new(),
        focused: false,
        picker_open: false,
        closed_empty: false,
        arrow_drawn: false,
    };
    let top = ui.cursor().top();
    let (left, right) = (ui.max_rect().left(), ui.max_rect().right());
    let gutter_right = left + KIND_BAR_WIDTH + GUTTER_WIDTH;
    let body_left = gutter_right + INDENT * place.depth as f32;
    let counted = drawing.card.runs == CardRuns::EachTick && drawing.counted.contains(&line.id);
    let column = if counted { COUNT_WIDTH } else { 0.0 };
    let tail_left = right - MENU_WIDTH - column;
    let body_right = (tail_left - TAIL_GAP).max(body_left + INDENT);
    let mut row = ui.new_child(
        UiBuilder::new()
            .id_salt(("henad_card_row", &line.id))
            .max_rect(Rect::from_min_max(pos2(left, top), pos2(right, f32::INFINITY)))
            .layout(Layout::top_down(Align::Min)),
    );
    let mut body = row.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(body_left, top + ROW_PADDING),
                pos2(body_right, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    body.spacing_mut().item_spacing.y = 0.0;
    sentence_ui(&mut body, &context, line, state, &mut gathered, pending);
    let sentence_bottom = body.min_rect().bottom().max(top + ROW_PADDING + SENTENCE_ROW);
    // The menu follows the sentence on the keyboard path, before the notes' buttons. Its glyph is painted once the
    // row's size is known.
    let menu_rect = Rect::from_min_size(
        pos2(right - MENU_WIDTH, top + ROW_PADDING),
        vec2(MENU_WIDTH - 4.0, SENTENCE_ROW),
    );
    let tail = row.new_child(
        UiBuilder::new()
            .max_rect(menu_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let menu = line_menu_ui(&tail, drawing.document, relations, line, line_data, state, pending);
    let sentence_rect = Rect::from_min_max(pos2(left, top), pos2(right, sentence_bottom));
    end_filling(ui, state, &line.id, sentence_rect, &gathered);
    // The rename field and the notes sit under the sentence, where the columns of the fire count and the menu end, so
    // they take the width up to the row's right padding.
    let mut under = row.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(body_left, sentence_bottom),
                pos2(right - TAIL_GAP - 4.0, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    under.spacing_mut().item_spacing.y = 0.0;
    if state.renaming.as_ref().is_some_and(|renaming| renaming.line == line.id) {
        gathered.focused |= rename_ui(&mut under, line, state, pending);
    }
    let filling = state.filling.contains(&line.id);
    gathered.focused |= notes_ui(
        &mut under,
        line,
        line_data,
        card_notes(drawing, line, line_data, relation_notes, filling),
        pending,
    );
    let bottom = under.min_rect().bottom().max(sentence_bottom) + ROW_PADDING;
    let rect = Rect::from_min_max(pos2(left, top), pos2(right, bottom));
    number_ui(
        &mut row,
        line.number,
        Rect::from_min_max(pos2(left, top), pos2(gutter_right, bottom)),
    );
    fire_count_ui(
        &mut row,
        drawing,
        line,
        Rect::from_min_size(
            pos2(tail_left, top + ROW_PADDING),
            vec2(column - TAIL_GAP, SENTENCE_ROW),
        ),
    );
    accessible_group(ui.ctx(), row.unique_id(), Role::Group, &line.label(), rect);
    ui.advance_cursor_after_rect(rect);
    paint_row(
        ui,
        rect,
        ground,
        &RowPaint {
            depth: place.depth,
            separator: !place.first,
            highlight: drawing.highlight.contains(&line.id),
        },
    );
    let handle = state.widgets.with((&line.id, "drag"));
    let payload = LineDrag {
        card: drawing.card.id.clone(),
        line: line.id.clone(),
    };
    kind_bar_ui(ui, line_data.kind, rect, handle, payload);
    let focused = gathered.focused || menu.has_focus();
    if focused {
        focus_mark(ui, rect);
    }
    // The empty "for each …" blank is quiet until its line is hovered or focused.
    let quiet = !(focused || ui.rect_contains_pointer(rect));
    for (shape, blank) in gathered.for_each_shapes.drain(..) {
        ui.painter()
            .set(shape, Shape::Vec(blank_shapes(ui, blank, FOR_EACH_BLANK_WORDS, quiet)));
    }
    rect
}

/// Draws the sentence of `line` part by part in rows of [`SENTENCE_ROW`], each alternative of a Choose or a Depending
/// on in rows of its own, indented once, with a dash in the indent of its first row.
fn sentence_ui(
    ui: &mut Ui,
    context: &LineContext<'_>,
    line: &CardLine,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Vec<CardEdit>,
) {
    let line_data = context.line;
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for (index, part) in line.parts.iter().enumerate() {
        if *part == SentencePart::Alternative {
            segments.push((start, index));
            start = index + 1;
        }
    }
    segments.push((start, line.parts.len()));
    let last = segments.len() - 1;
    for (segment, &(from, until)) in segments.iter().enumerate() {
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().interact_size.y = SENTENCE_ROW;
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal_wrapped(|ui| {
                for index in from..until {
                    let Some(part) = line.parts.get(index) else {
                        continue;
                    };
                    let place = line.places.get(index).cloned().flatten();
                    let parts = PartDrawing {
                        context,
                        line,
                        index,
                        place,
                    };
                    part_ui(ui, &parts, part, state, gathered, pending);
                    if let Some(place) = parts.place.as_ref().and_then(|place| list_end(line_data, place)) {
                        let plus = PartDrawing {
                            place: Some(place),
                            ..parts
                        };
                        add_value_ui(ui, &plus, state, gathered, pending);
                    }
                }
                if segment != last {
                    return;
                }
                if line_data.kind == LineKind::Choose {
                    let parts = PartDrawing {
                        context,
                        line,
                        index: line.parts.len(),
                        place: Some(PartPlace {
                            target: PlaceTarget::Slot(format!("row.{}.value", line_data.rows.len())),
                            want: Some(Blank::Value),
                        }),
                    };
                    add_row_ui(ui, &parts, state, gathered, pending);
                }
                if line_data.around == Some(Around::AgentsWithin) && line_data.kind == LineKind::LookAround {
                    let parts = PartDrawing {
                        context,
                        line,
                        index: line.parts.len(),
                        place: None,
                    };
                    add_summary_ui(ui, &parts, state, gathered, pending);
                }
            });
        };
        let indent = if segment == 0 { 0.0 } else { INDENT };
        let mut place = ui.available_rect_before_wrap();
        place.min.x += indent;
        if segment > 0 {
            // A dash in the indent marks the row an alternative starts on, so the rows it wraps onto read as its own.
            let y = place.top() + SENTENCE_ROW / 2.0;
            let dash = pos2(place.left() - INDENT + ALTERNATIVE_DASH.0, y);
            ui.painter()
                .hline(dash.x..=dash.x + ALTERNATIVE_DASH.1, y, Stroke::new(1.0, mcs::GRAY_500));
        }
        let mut column = ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)));
        draw(&mut column);
        ui.advance_cursor_after_rect(column.min_rect());
    }
}

/// Draws the number `number` of a line, right-aligned in the gutter of the row `rect` and level with its first row.
fn number_ui(ui: &mut Ui, number: usize, rect: Rect) {
    let place = Rect::from_min_max(
        pos2(rect.left() + KIND_BAR_WIDTH, rect.top() + ROW_PADDING),
        pos2(rect.right() - NUMBER_GAP, rect.top() + ROW_PADDING + SENTENCE_ROW),
    );
    let mut column = ui.new_child(
        UiBuilder::new()
            .max_rect(place)
            .layout(Layout::right_to_left(Align::Center)),
    );
    column.add(Label::new(small_label(number.to_string())).selectable(false));
}

/// Paints the bar in the hue of the kind of a line of `kind` at the left edge of its row `rect`. The bar and the
/// gutter beside it are the handle, under the id `handle`, that drags the line `payload` names, and show the kind's
/// word with how to drag the line as their hover text.
fn kind_bar_ui(ui: &Ui, kind: LineKind, rect: Rect, handle: Id, payload: LineDrag) {
    let group = AddButton::for_kind(kind);
    kind_bar(ui.painter(), rect, group);
    let gutter = Rect::from_min_size(
        rect.min,
        vec2(KIND_BAR_WIDTH + GUTTER_WIDTH - NUMBER_GAP, rect.height()),
    );
    // The grip sits left of the number, level with the row's first line of words.
    let grip = pos2(
        rect.left() + KIND_BAR_WIDTH + GRIP_OFFSET,
        rect.top() + ROW_PADDING + SENTENCE_ROW / 2.0,
    );
    handle_ui(ui, gutter, rect, handle, payload, grip).on_hover_text(drag_line_tooltip(group.group()));
}

/// Draws the fire count of `line` in grey, right-aligned in `place`, the column of [`COUNT_WIDTH`] at the row's top,
/// when the card's counts hold one for the line. Its hover text and accessible description name the line, the number
/// of things the card runs on and the tick.
fn fire_count_ui(ui: &mut Ui, drawing: &CardDrawing<'_>, line: &CardLine, place: Rect) {
    let Some(fires) = drawing.fires else {
        return;
    };
    let Some(&count) = fires.counts.get(&line.id) else {
        return;
    };
    let thing = drawing.sentences.thing_word(&drawing.card.every);
    let description = fire_count_description(&line.prefix, count, &thing, fires.tick);
    // A child placed over the row's top takes no space, so the notes under the row keep their place.
    let mut column = ui.new_child(
        UiBuilder::new()
            .max_rect(place)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let label = column.add(Label::new(small_label(fire_count(count))).selectable(false));
    ui.ctx()
        .accesskit_node_builder(label.id, |node| node.set_description(description.as_str()));
    label.on_hover_text(description);
}

/// Draws `notes` under `line`, each note's button running its edit, and returns whether a note's button holds the
/// keyboard focus.
fn notes_ui(
    ui: &mut Ui,
    line: &CardLine,
    line_data: &SheetLine,
    notes: Vec<LineNote>,
    pending: &mut Vec<CardEdit>,
) -> bool {
    let mut focused = false;
    for note in notes {
        let color = match note.color {
            NoteColor::Amber => ui.visuals().warn_fg_color,
            NoteColor::Red => ui.visuals().error_fg_color,
            NoteColor::Grey => ui.visuals().weak_text_color(),
        };
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
            let words = RichText::new(note.text.as_str()).size(NOTE_SIZE).color(color);
            let label = ui.add(Label::new(words).wrap());
            if let Some(description) = &note.description {
                ui.ctx()
                    .accesskit_node_builder(label.id, |node| node.set_description(description.as_str()));
                label.on_hover_text(description.as_str());
            }
            if let Some(action) = note.action {
                let button = note_button(ui, &action.label, &action.name);
                focused |= button.has_focus();
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
                    pending.push(CardEdit::commands(vec![action.edit], after));
                }
            }
        });
    }
    focused
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
    /// Values the picker of the part reads.
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
    pending: &mut Vec<CardEdit>,
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
            text_ui(ui, words);
            return;
        }
        SentencePart::Keyword(words) => {
            words_ui(ui, words, &Words::lead(ui), Sense::hover());
            return;
        }
        SentencePart::ResultName(name) => {
            let response = result_name_ui(ui, drawing, name, state);
            gathered.focused |= response.has_focus();
            return;
        }
        // A word that does not wrap moves to the next row whole when the row has no room left for it.
        SentencePart::ConditionWord(words) => {
            condition_word_ui(ui, drawing, words, state, gathered, pending);
            return;
        }
        SentencePart::From => {
            let context = drawing.context;
            let thing = context.sentences.thing_word(&context.relations.card().every);
            let look = Words {
                dotted: true,
                whole: true,
                ..Words::plain(ui)
            };
            words_ui(ui, FROM_WORD, &look, Sense::hover()).on_hover_text(from_tooltip(&thing));
            return;
        }
        SentencePart::Chip(chip) => chip_part_ui(ui, drawing, id, chip, state, gathered),
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
            select_control(ui, id, select_words(*select, &words), &name, words.is_empty())
        }
        SentencePart::Always => {
            text_control(ui, id, ALWAYS, &name, SENTENCE_ROW).on_hover_or_focus_text(ALWAYS_TOOLTIP)
        }
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
            response.on_hover_or_focus_text(&for_each_optional_tooltip(*draw))
        }
        SentencePart::ForEach(_) => text_control(ui, id, FOR_EACH_WORDS, &name, SENTENCE_ROW),
        _ => return,
    };
    gathered.focused |= response.has_focus();
    let (Some(place), Some(id)) = (drawing.place.as_ref(), id) else {
        return;
    };
    control_picker(ui, drawing, place, part, &response, id, state, gathered, pending);
}

/// Adds the chip `chip` of the part being drawn under the widget id `id`, red with its tooltip when its line cannot use
/// it, and returns its response. Hovering or focusing a result chip highlights the result's line on the next frame.
fn chip_part_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    id: Option<Id>,
    chip: &SentenceChip,
    state: &CardState,
    gathered: &mut LineDrawing<'_>,
) -> Response {
    let line = drawing.line;
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
        Some(tooltip) => response.on_hover_or_focus_text(tooltip.as_str()),
        None => response,
    };
    if let ChipKind::Result(result) = &shown.kind
        && (response.contains_pointer() || response.has_focus())
    {
        gathered.hovered.push(result.clone());
    }
    response
}

/// How the words of a sentence are set.
struct Words {
    /// Font of the words.
    font: FontId,
    /// Colour of the words.
    color: Color32,
    /// Whether the words are set in italics.
    italics: bool,
    /// Whether a dotted line runs under the words, as under words that show a tooltip.
    dotted: bool,
    /// Whether the words stay on one row, and move to the next row whole when the row has no room left for them.
    whole: bool,
}

impl Words {
    /// Returns the look of a sentence's plain words.
    fn plain(ui: &Ui) -> Self {
        Self {
            font: TextStyle::Body.resolve(ui.style()),
            color: ui.visuals().text_color(),
            italics: false,
            dotted: false,
            whole: false,
        }
    }

    /// Returns the look of a line's lead words, gray 100 in the semibold face.
    fn lead(ui: &Ui) -> Self {
        Self {
            font: semibold_font(ui.ctx(), TextStyle::Body.resolve(ui.style()).size),
            color: mcs::GRAY_100,
            ..Self::plain(ui)
        }
    }
}

/// Adds `words` set as `look` says, a label sensing `sense`, and returns its response.
///
/// Each row of the words is [`SENTENCE_ROW`] high with the words in its middle, level with the chips beside them. egui
/// puts the extra height of a row under its words and draws no dotted underline, so the label is laid out here and
/// painted lower by half the extra height.
fn words_ui(ui: &mut Ui, words: &str, look: &Words, sense: Sense) -> Response {
    let mut text = RichText::new(words)
        .font(look.font.clone())
        .color(look.color)
        .line_height(Some(SENTENCE_ROW));
    if look.italics {
        text = text.italics();
    }
    let mut label = Label::new(text).selectable(false).sense(sense);
    if look.whole {
        label = label.extend();
    }
    let (position, galley, response) = label.layout_in_ui(ui);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), galley.text()));
    if ui.is_rect_visible(response.rect) {
        let height = ui.fonts_mut(|fonts| fonts.row_height(&look.font));
        let position = position + vec2(0.0, ((SENTENCE_ROW - height) / 2.0).round());
        let painter = ui.painter();
        if look.dotted {
            for row in &galley.rows {
                let rect = row.rect_without_leading_space().translate(position.to_vec2());
                let y = rect.top() + height - 1.0;
                painter.extend(Shape::dotted_line(
                    &[pos2(rect.left() + 1.0, y), pos2(rect.right() - 1.0, y)],
                    look.color,
                    3.0,
                    0.6,
                ));
            }
        }
        painter.galley(position, galley, look.color);
    }
    response
}

/// Draws the plain words `words` of a sentence. Closing punctuation at their start stays on the row of the part before
/// it, so a row never starts with ")" or ";", and a space that ends a full row takes no row of its own.
fn text_ui(ui: &mut Ui, words: &str) {
    let space = ui.fonts_mut(|fonts| fonts.glyph_width(&TextStyle::Body.resolve(ui.style()), ' '));
    if words.trim().is_empty() && ui.available_size_before_wrap().x < space * words.len() as f32 {
        // A space at the end of a full row takes no row of its own.
        let rect = Rect::from_min_size(ui.cursor().min, vec2(0.0, SENTENCE_ROW));
        let response = ui.allocate_rect(rect, Sense::hover());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), words));
        return;
    }
    let rest = words.trim_start_matches([',', ';', ':', ')', '.']);
    let attached = &words[..words.len() - rest.len()];
    if attached.is_empty() {
        words_ui(ui, words, &Words::plain(ui), Sense::hover());
        return;
    }
    let look = Words::plain(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(attached.to_owned(), look.font.clone(), look.color);
    // Placed at the cursor without the wrap check of the layout. The punctuation may reach a few points into the
    // gap at the row's right.
    let rect = Rect::from_min_size(ui.cursor().min, vec2(galley.size().x, SENTENCE_ROW));
    let response = ui.allocate_rect(rect, Sense::hover());
    ui.painter().galley(
        pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        look.color,
    );
    // One label keeps all of `words`, as before the punctuation was set apart.
    let label = if rest.is_empty() {
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), words));
        response
    } else {
        words_ui(ui, rest, &look, Sense::hover())
    };
    ui.ctx().accesskit_node_builder(label.id, |node| node.set_value(words));
}

/// Draws the name `name` that a result line works out as the line's result chip, and returns its response. Pressing it
/// opens the field that renames the line, as the line's menu does.
fn result_name_ui(ui: &mut Ui, drawing: &PartDrawing<'_>, name: &str, state: &mut CardState) -> Response {
    let line = drawing.line;
    let id = state.widgets.with((&line.id, "result name"));
    let chip = SentenceChip {
        kind: ChipKind::Result(line.id.clone()),
        lead: "",
        name: name.to_owned(),
    };
    let response = chip_with_id(ui, Some(id), &chip, false, None).on_hover_or_focus_text(&rename_line(&line.word));
    set_name(ui, response.id, &rename_result_name(&line.prefix, name));
    if response.clicked() {
        state.renaming = Some(Renaming {
            line: line.id.clone(),
            text: name.to_owned(),
            message: None,
            focused: false,
        });
    }
    response
}

/// Draws the word `words` that leads a change, "otherwise (when line 2 does not apply)" or "or", in the weak text colour
/// and in italics with a dotted line under it, focusable and described by `tooltip`. Hovering or focusing it highlights
/// the lines `lines` on the next frame.
fn lead_word_ui(ui: &mut Ui, words: &str, tooltip: &str, lines: &[String], gathered: &mut LineDrawing<'_>) -> Response {
    let look = Words {
        color: ui.visuals().weak_text_color(),
        italics: true,
        dotted: true,
        ..Words::plain(ui)
    };
    let response = words_ui(ui, words, &look, Sense::click()).on_hover_or_focus_text(tooltip);
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
    pending: &mut Vec<CardEdit>,
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
    if let SentencePart::Select(_, words) = part {
        let chosen = select_popup(ui, drawing, place, response, popup_id, words);
        gathered.picker_open |= Popup::is_id_open(ui.ctx(), popup_id);
        if let Some(option) = chosen {
            pending.extend(select_edit(drawing, state, place, option));
        }
        return;
    }
    let mut picker = place_picker(drawing, place, part);
    if let SentencePart::Chip(chip) = part {
        picker.current = Some(chip.name.clone());
    }
    let mut open = false;
    let choice = picker.show(ui, response, popup_id, &mut open);
    gathered.picker_open |= open;
    gathered.closed_empty |= was_open && !open && choice.is_none();
    if let Some(choice) = choice {
        pending.extend(choice_edit(drawing, state, place, &choice));
    }
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
                Some(phrase) => match slot_value(line, phrase)? {
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
    pending: &mut Vec<CardEdit>,
) {
    let Some(place) = drawing.place.as_ref() else {
        return;
    };
    let id = state.control_id(&drawing.line.id, &place.target, 0);
    ui.add_space(4.0);
    let response = text_control(ui, Some(id), "+", &add_value_name(&drawing.line.prefix), SENTENCE_ROW);
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
    pending: &mut Vec<CardEdit>,
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
        &add_choose_row_name(&drawing.line.prefix),
        SENTENCE_ROW,
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
    pending: &mut Vec<CardEdit>,
) {
    let line = drawing.line;
    let target = PlaceTarget::Slot("summaries".to_owned());
    let id = state.control_id(&line.id, &target, 3);
    ui.add_space(4.0);
    let response = text_control(ui, Some(id), "+", &add_summary_name(&line.prefix), SENTENCE_ROW);
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
        pending.push(CardEdit::commands(
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
    pending: &mut Vec<CardEdit>,
) {
    let line = drawing.line;
    let target = PlaceTarget::Slot("condition".to_owned());
    let id = state.control_id(&line.id, &target, 2);
    let name = condition_arrow_name(&line.prefix);
    let response = arrow_control(ui, id, &name, SENTENCE_ROW).on_hover_or_focus_text(CONDITION_ARROW_TOOLTIP);
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
        pending.extend(choice_edit(drawing, state, &place, &choice));
    }
}

/// Draws the condition word `words` in the semibold face, and after the first condition word of a line that takes a
/// condition its arrow, on the word's row.
fn condition_word_ui(
    ui: &mut Ui,
    drawing: &PartDrawing<'_>,
    words: &str,
    state: &mut CardState,
    gathered: &mut LineDrawing<'_>,
    pending: &mut Vec<CardEdit>,
) {
    let look = Words {
        whole: true,
        ..Words::lead(ui)
    };
    let arrow = !gathered.arrow_drawn && takes_condition(drawing.context.line.kind);
    if arrow {
        // Both move to the next row when the row has no room for the word and its arrow.
        let width = ui
            .painter()
            .layout_no_wrap(words.to_owned(), look.font.clone(), look.color)
            .size()
            .x;
        let room = ui.available_size_before_wrap().x;
        if room < width + ARROW_WIDTH && room < ui.available_width() {
            ui.end_row();
        }
    }
    words_ui(ui, words, &look, Sense::hover());
    if arrow {
        gathered.arrow_drawn = true;
        condition_arrow_ui(ui, drawing, state, gathered, pending);
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
    let ground = takes_ground_states(line, &place.target);
    let mut picker = picker_for(context, want, property, ground, filled, &typed_text(value), draw);
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
        if let Some(update) = applied.draft {
            state.keep_draft(update);
        }
        return None;
    }
    Some(CardEdit::with_draft(
        applied.edits,
        After::Next {
            line: line_id,
            target: place.target.clone(),
            want: place.want,
        },
        applied.draft,
    ))
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

/// Draws the options of the select at `place` under `anchor`, the option that shows `current` marked, and returns the
/// option chosen.
fn select_popup(
    ui: &Ui,
    drawing: &PartDrawing<'_>,
    place: &PartPlace,
    anchor: &Response,
    popup_id: Id,
    current: &str,
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
                slot_value(line, phrase).and_then(|value| match value {
                    SheetValue::Phrase(phrase) => Some(phrase.as_ref()),
                    _ => None,
                })
            };
            let of_states = phrase
                .and_then(|phrase| read_property(phrase.left.value()))
                .and_then(|id| find_property(drawing.context.document, id))
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
    let items: Vec<OverlayItem<'_>> = options
        .iter()
        .map(|(words, _)| OverlayItem {
            label: words,
            current: words.as_str() == current,
            ..OverlayItem::default()
        })
        .collect();
    let chosen = select_list(ui, anchor, popup_id, &items)?;
    options.get(chosen).map(|(_, option)| *option)
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
            let remove = SheetEdit::DeleteSummary { line: line_id, index };
            // A summary is named as its chip reads, by its id while it has no name.
            let name = line.summaries.get(index).map(|summary| {
                if summary.name.is_empty() {
                    &summary.id
                } else {
                    &summary.name
                }
            })?;
            let document = drawing.context.document;
            if let Some(question) = DeleteQuestion::before(document, &remove, REMOVE_SUMMARY_HEADING, name) {
                state.delete_question = Some(question);
                return None;
            }
            return Some(CardEdit::commands(vec![remove], After::Stay));
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
                if let Some(update) = applied.draft {
                    state.keep_draft(update);
                }
                return None;
            }
            return Some(CardEdit::with_draft(applied.edits, After::Stay, applied.draft));
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

/// Draws the menu of `line`, "Line 2 actions": Rename on a result line, Move up and Move down, Draw new numbers on a
/// line that draws, Convert to a rate or Convert to a chance on a line whose typed chance or rate can change, and
/// Delete.
/// Returns the response of its button. The button fills the place `ui` gives it.
///
/// The tools form a menu labelled by the button. Move up and Move down stay in the menu where the outline keeps the
/// line in place, weak, with the reason as their tooltip.
fn line_menu_ui(
    ui: &Ui,
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line: &CardLine,
    line_data: &SheetLine,
    state: &mut CardState,
    pending: &mut Vec<CardEdit>,
) -> Response {
    let id = state.widgets.with((&line.id, "menu"));
    let popup_id = id.with("popup");
    let response = menu_button(ui, id, popup_id, &line_actions_name(&line.prefix), ui.max_rect());
    let includes_me = line_data.include_me == Some(true);
    let button = response.id;
    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    let (popup, _) = placed(ui.ctx(), Popup::menu(&response).id(popup_id).frame(Frame::NONE));
    let chosen = popup
        .show(|ui| {
            overlay(ui, |ui| {
                menu_group(ui, button, |ui| line_tools_ui(ui, document, relations, line, line_data))
            })
        })
        .and_then(|popup| popup.inner);
    escape_to_anchor(ui, &response, popup_id, was_open);
    let edit = |edit: SheetEdit| CardEdit::commands(vec![edit], After::Stay);
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
            pending.push(edit(SheetEdit::SetField {
                line: line_id,
                phrase: String::new(),
                field: LineField::IncludeMe(Some(!includes_me)),
            }));
        }
        Some(LineTool::MoveUp) => {
            pending.push(edit(SheetEdit::MoveLine {
                line: line_id,
                direction: MoveDirection::Up,
            }));
        }
        Some(LineTool::MoveDown) => {
            pending.push(edit(SheetEdit::MoveLine {
                line: line_id,
                direction: MoveDirection::Down,
            }));
        }
        Some(LineTool::DrawNewNumbers) => pending.push(edit(SheetEdit::DrawNewNumbers { line: line_id })),
        Some(LineTool::ChangeToRate) => pending.push(edit(SheetEdit::ChangeToRate { line: line_id })),
        Some(LineTool::ChangeToChance) => pending.push(edit(SheetEdit::ChangeToChance { line: line_id })),
        Some(LineTool::Delete) => {
            let delete = SheetEdit::DeleteLine { line: line_id };
            // The line is named as a chip of its result reads: by its name, an unnamed Count by its automatic name, and
            // any other line by its number. A line that a formula names works out a result.
            let name = line_data
                .name
                .clone()
                .or_else(|| {
                    (line_data.kind == LineKind::Count)
                        .then(|| automatic_name(document, line_data))
                        .flatten()
                })
                .unwrap_or_else(|| line.prefix.clone());
            match DeleteQuestion::before(document, &delete, DELETE_LINE_HEADING, &name) {
                Some(question) => state.delete_question = Some(question),
                None => pending.push(edit(delete)),
            }
        }
        None => {}
    }
    response
}

/// Draws the tools of the menu of `line`, and returns the tool chosen.
fn line_tools_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line: &CardLine,
    line_data: &SheetLine,
) -> Option<LineTool> {
    let (up, down) = can_move(relations, &line.id);
    let change = conversion(document, &line.id);
    let mut chosen = None;
    let mut tool = |ui: &mut Ui, shown: bool, refusal: Option<&str>, words: String, tool: LineTool| {
        if !shown {
            return;
        }
        let row = OverlayItem {
            label: &words,
            ..OverlayItem::default()
        };
        let response = ui.add_enabled_ui(refusal.is_none(), |ui| menu_item(ui, &row)).inner;
        if let Some(refusal) = refusal {
            response.on_disabled_hover_text(refusal);
        } else if response.clicked() {
            chosen = Some(tool);
        }
    };
    tool(
        ui,
        line_data.kind.works_out_result(),
        None,
        rename_line(&line.word),
        LineTool::Rename,
    );
    let include = if line_data.include_me == Some(true) {
        leave_me_out_tool(&line.word)
    } else {
        include_me_tool(&line.word)
    };
    let looks_at_agents = line_data.around == Some(Around::AgentsWithin);
    tool(ui, looks_at_agents, None, include, LineTool::IncludeMe);
    let refusal = (!up).then_some(MOVE_UP_FIRST);
    tool(ui, true, refusal, move_line_up(&line.word), LineTool::MoveUp);
    let refusal = (!down).then_some(MOVE_DOWN_LAST);
    tool(ui, true, refusal, move_line_down(&line.word), LineTool::MoveDown);
    let draws = holds_draw(line_data);
    tool(ui, draws, None, DRAW_NEW_NUMBERS.to_owned(), LineTool::DrawNewNumbers);
    let to_rate = change == Some(DrawKind::Rate);
    tool(ui, to_rate, None, CHANGE_TO_RATE.to_owned(), LineTool::ChangeToRate);
    let to_chance = change == Some(DrawKind::Chance);
    tool(
        ui,
        to_chance,
        None,
        CHANGE_TO_CHANCE.to_owned(),
        LineTool::ChangeToChance,
    );
    ui.add_space(4.0);
    let words = delete_line(&line.word);
    let row = OverlayItem {
        label: &words,
        ..OverlayItem::default()
    };
    let delete = menu_item(ui, &row);
    let delete = if line_data.kind == LineKind::When {
        delete.on_hover_text(DELETE_WHEN_TOOLTIP)
    } else {
        delete
    };
    if delete.clicked() {
        chosen = Some(LineTool::Delete);
    }
    chosen
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

/// Draws the field that renames `line`, under it, and returns whether the field holds the keyboard focus. Enter or
/// leaving the field keeps the name, and Escape cancels.
fn rename_ui(ui: &mut Ui, line: &CardLine, state: &mut CardState, pending: &mut Vec<CardEdit>) -> bool {
    let Some(renaming) = &mut state.renaming else {
        return false;
    };
    let mut keep = None;
    let mut cancel = false;
    let mut focused = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let width = RENAME_WIDTH.min(ui.available_width());
        let field = ui.add(TextEdit::singleline(&mut renaming.text).desired_width(width));
        focused = field.has_focus();
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
        message_ui(ui, message);
    }
    if cancel {
        state.renaming = None;
        return focused;
    }
    let Some(name) = keep else {
        return focused;
    };
    if name.is_empty() {
        state.renaming = None;
        return focused;
    }
    let mut edit = CardEdit::commands(
        vec![SheetEdit::RenameResult {
            line: line.id.clone(),
            name,
        }],
        After::Stay,
    );
    edit.rename = true;
    pending.push(edit);
    focused
}

// The New parameter dialog

/// Draws the New parameter dialog while it is open. Create adds the parameter and puts it in the place it was opened
/// from, as one edit.
fn new_parameter_dialog(ui: &Ui, document: &SheetDocument, state: &mut CardState, pending: &mut Vec<CardEdit>) {
    let Some((target, dialog, drawn)) = &mut state.new_parameter else {
        return;
    };
    if !*drawn {
        // The click that chose "New parameter…" would land on the dialog's backdrop and close it.
        *drawn = true;
        ui.ctx().request_repaint();
        return;
    }
    match new_parameter_modal(ui.ctx(), dialog, &parameter_names(document)) {
        DialogOutcome::Open => {}
        DialogOutcome::Canceled => state.new_parameter = None,
        DialogOutcome::Created(parameter) => {
            let target = target.clone();
            let draft = state.drafts.get(&target.line).cloned();
            let after = After::Next {
                line: target.line.clone(),
                target: target.place.target.clone(),
                want: target.place.want,
            };
            pending.push(CardEdit {
                run: Box::new(move |builder| {
                    let made = builder.apply(parameter.add_edit())?;
                    let id = made.first().cloned().unwrap_or_default();
                    let choice = Choice::Value(SheetValue::Chip(SheetChip::Param(id)));
                    let applied = apply_choice(builder.document(), draft, &target.line, &target.place, &choice);
                    for edit in applied.edits {
                        builder.apply(edit)?;
                    }
                    Ok((made, applied.draft))
                }),
                after,
                rename: false,
            });
            state.new_parameter = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use henad_graph::sheet::document::CELLS_ID;
    use henad_graph::sheet::sentence::{AddButton, AddItem};

    use super::new_line;
    use crate::ui::editor::sheet::widgets::kind_hue;

    /// Every item of an add button's menus adds a line whose bar takes the button's hue. A Become from "+ Chance" is a
    /// change and takes the Change hue. The five groups of lines that are no block take five hues.
    #[test]
    fn each_add_button_adds_lines_in_its_own_hue() {
        for agents in [false, true] {
            let (every, buttons): (&str, &[AddButton]) = if agents {
                ("ant", &AddButton::AGENTS)
            } else {
                (CELLS_ID, &AddButton::ALL)
            };
            for &button in buttons {
                for &item in button.items_for(agents) {
                    let group = AddButton::for_kind(new_line(item, every).kind);
                    let expected = match item {
                        AddItem::BecomeWithChance | AddItem::BecomeAtRate => AddButton::Change,
                        _ => button,
                    };
                    assert_eq!(group, expected, "{item:?}");
                }
            }
        }
        let hues: Vec<_> = AddButton::ALL[..5].iter().map(|button| kind_hue(*button)).collect();
        for (index, hue) in hues.iter().enumerate() {
            assert!(!hues[index + 1..].contains(hue), "{hues:?}");
        }
        assert_eq!(kind_hue(AddButton::When), kind_hue(AddButton::Repeat));
    }
}
