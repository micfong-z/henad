//! Pickers of the Card pane: what a blank, a chip or a select of a line can take, and the commands a choice runs.
//!
//! A *picker* lists only what fits its place, in groups under headings: the results of the lines above ("From lines
//! above"), the thing's properties ("This cell's properties", "This bird's properties"), the parameters with "New
//! parameter...", the states, or the properties of one kind. On a card over agents a value also takes the summaries of
//! the looks above, the cells picked above, "my new" values below the last change to a property, and new random
//! values. A Number box takes a typed number, a list box several numbers at once. A group with no
//! item is left out, and a picker with nothing to offer says what to make first.
//!
//! A *result above* a line is the result of a line higher on its card. A result worked out inside a When is open only
//! to the lines after it inside that When.

use egui::{Color32, Frame, Id, Key, Popup, PopupCloseBehavior, Response, RichText, ScrollArea, TextEdit, Ui};
use henad_core::metadata::TimeUnit;
use henad_graph::sheet::check::changes_property;
use henad_graph::sheet::document::{
    Around, CardRuns, CellWhich, ChipRelation, CompareTest, LineKind, LineMode, ParameterType, PickRule, PropertyType,
    SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetProperty, SheetValue, Slot, SummaryKind,
};
use henad_graph::sheet::edit::{LineField, SheetEdit, automatic_name};
use henad_graph::sheet::format_number;
use henad_graph::sheet::rates::parameter_rate_unit;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    ANY_STATE, ANY_STATE_ID, Blank, CHANCE_BOX_HINT, CHOOSE_WHEN, COMPARISONS, CONDITIONS, ConditionItem, DrawKind,
    FROM_LINES_ABOVE, FieldPlace, LIST_GROUP, LIST_HINT, LIST_IN_ONE_NUMBER, NEW_PARAMETER,
    NO_NUMBER_PROPERTY_ON_SHEET, NO_NUMBER_RESULT, NO_STATES, NO_STATES_PROPERTY, NO_YES_OR_NO_RESULT, NUMBER_BOX_HINT,
    OR_A_NEW_CONDITION, OTHERWISE, PARAMETERS, PROPERTIES, PartPlace, PlaceTarget, REMOVE_FOR_EACH, REMOVE_FROM_LIST,
    STATES, Sentences, THIS_CELLS_PROPERTIES, TYPE_ONE_NUMBER, USE_BUTTON, around_words, binary_formula,
    comparison_words, for_each_lead, literal_words, my_property_item, operation_words, otherwise_description,
    otherwise_item_name, rate_unit_word, read_typed_number, result_condition_item, result_description,
    shared_state_name, this_cells_property, when_lead,
};
use henad_graph::sheet::sentence::{
    AddItem, CHOOSE_A_SUMMARY, EVENTS, GROUND_PROPERTIES, LOOK_AT_AHEAD, LOOK_AT_HERE, LOOK_AT_PLACE, NO_EVENT,
    NO_GROUND_PROPERTY, NO_PLACE, NO_PROPERTY, OR_A_NEW_VALUE, REMOVE_SUMMARY, SUMMARIES, each_looked_at,
    length_of_item, my_new_detail, my_new_property_item, relation_words, summary_item, their_property_item,
    this_things_properties, this_things_property,
};

use super::accessible_group;
use super::widgets::{
    NumberUse, OVERLAY_PADDING, OverlayItem, PICKER_WIDTH, color32, message_ui, overlay, overlay_group, overlay_item,
    overlay_title, set_name,
};

/// Tallest a picker grows in points before it scrolls.
const PICKER_HEIGHT: f32 = 420.0;

/// Number of items past which a picker scrolls.
const LONG_PICKER: usize = 12;

/// Salt of the id the Number box's text is kept under, below the picker's id.
const NUMBER_BOX_ID: &str = "henad_card_number_box";

/// Salt of the id the list box's text is kept under, below the picker's id.
const LIST_BOX_ID: &str = "henad_card_list_box";

/// Value a choice in a picker puts in its place.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    /// A value: a result, a property, a parameter or a typed number.
    Value(SheetValue),
    /// A state of a property.
    State {
        /// Id of the property.
        property: String,
        /// Id of the state.
        state: String,
    },
    /// Any state, the old state of a Become line.
    AnyState,
    /// A property, by id.
    Property(String),
    /// A form of condition from "Choose when".
    Condition(ConditionItem),
    /// "New parameter...", the item that opens the New parameter dialog.
    NewParameter,
    /// Numbers typed in the list box.
    Numbers(Vec<SheetLiteral>),
    /// "Remove from list" or "Remove for each".
    Remove,
    /// An event, by id.
    Event(String),
    /// A new summary of a look at agents, of this kind.
    Summary(SummaryKind),
}

/// One item of a picker.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerItem {
    /// Words of the item. They also name it, unless `name` gives a name of its own.
    pub label: String,
    /// Accessible name of the item when it differs from its words, `None` otherwise.
    pub name: Option<String>,
    /// Grey words under the item, its accessible description.
    pub detail: Option<String>,
    /// Colour of a state's swatch before the words, `None` for an item that is no state.
    pub swatch: Option<Color32>,
    /// Value choosing the item puts in the place.
    pub choice: Choice,
}

/// One group of a picker: a heading and its items.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerGroup {
    /// Heading of the group.
    pub heading: String,
    /// Items of the group, in order.
    pub items: Vec<PickerItem>,
}

/// The Number box of a picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberBox {
    /// Range the box reads numbers in.
    pub number_use: NumberUse,
    /// Whether the box comes first and takes the focus. Otherwise it comes last.
    pub first: bool,
    /// Text the box shows when the picker opens, a typed number as it was typed.
    pub typed: String,
}

/// A picker of the Card pane: its title, what it offers, and what it says when it offers nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct CardPicker {
    /// Title of the picker, such as "Choose a state".
    pub title: String,
    /// Words under the title, `None` for a picker with none.
    pub lead: Option<String>,
    /// Groups of items. A group with no item is left out.
    pub groups: Vec<PickerGroup>,
    /// Whether the picker offers "New parameter...".
    pub new_parameter: bool,
    /// The Number box, `None` for a picker with none.
    pub number_box: Option<NumberBox>,
    /// Whether the picker offers the list box of numbers.
    pub list_box: bool,
    /// Last item, "Remove from list" or "Remove for each", `None` for a picker with none.
    pub remove: Option<&'static str>,
    /// Text the picker shows when it offers nothing.
    pub empty: &'static str,
    /// Words of the item that names the value the place holds, `None` while it holds none. That item takes the tinted
    /// fill and a check.
    pub current: Option<String>,
}

impl CardPicker {
    /// Returns a picker titled `title` that offers nothing yet and says `empty`.
    fn titled(title: String, empty: &'static str) -> Self {
        Self {
            title,
            lead: None,
            groups: Vec::new(),
            new_parameter: false,
            number_box: None,
            list_box: false,
            remove: None,
            empty,
            current: None,
        }
    }

    /// Draws the picker under `anchor` while it is open, and returns the choice made in it.
    ///
    /// The picker's open state lives in egui memory under `popup_id`. A click on the anchor opens it and keeps it
    /// open, and a choice, Escape or a click outside both the picker and the anchor closes it. `open` holds whether
    /// it was open when drawn, for the card's focus rules.
    pub fn show(&self, ui: &Ui, anchor: &Response, popup_id: Id, open: &mut bool) -> Option<Choice> {
        if anchor.clicked() {
            Popup::open_id(ui.ctx(), popup_id);
        }
        let was_open = Popup::is_id_open(ui.ctx(), popup_id);
        let popup = Popup::from_response(anchor)
            .id(popup_id)
            .open_memory(None)
            .close_behavior(PopupCloseBehavior::IgnoreClicks)
            .width(PICKER_WIDTH)
            .frame(Frame::NONE)
            .show(|ui| {
                overlay(ui, |ui| {
                    ui.set_min_width(PICKER_WIDTH - 2.0 * OVERLAY_PADDING);
                    // A long picker scrolls. A short one takes its own height. A scroll area lags a frame behind it.
                    let choice = if self.groups.iter().map(|group| group.items.len()).sum::<usize>() > LONG_PICKER {
                        ScrollArea::vertical()
                            .max_height(PICKER_HEIGHT)
                            .show(ui, |ui| self.contents(ui, popup_id))
                            .inner
                    } else {
                        self.contents(ui, popup_id)
                    };
                    (choice, ui.unique_id())
                })
            });
        let Some(popup) = popup else {
            *open = false;
            return None;
        };
        let (choice, id) = popup.inner;
        accessible_group(
            ui.ctx(),
            id,
            egui::accesskit::Role::Dialog,
            &self.title,
            popup.response.rect,
        );
        let clicked_outside = ui.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|position| !popup.response.rect.contains(position) && !anchor.rect.contains(position))
        });
        if choice.is_some() || (was_open && clicked_outside) {
            Popup::close_id(ui.ctx(), popup_id);
        }
        *open = Popup::is_id_open(ui.ctx(), popup_id);
        if !*open {
            // The boxes start afresh, and take the focus, when the picker opens again.
            ui.data_mut(|data| {
                data.remove::<BoxDraft>(popup_id.with(NUMBER_BOX_ID));
                data.remove::<BoxDraft>(popup_id.with(LIST_BOX_ID));
            });
            if choice.is_none() && ui.input(|input| input.key_pressed(Key::Escape)) {
                anchor.request_focus();
            }
        }
        choice
    }

    /// Draws what the picker offers, and returns the choice made.
    fn contents(&self, ui: &mut Ui, popup_id: Id) -> Option<Choice> {
        let mut choice = None;
        overlay_title(ui, &self.title);
        if let Some(lead) = &self.lead {
            ui.add(egui::Label::new(RichText::new(lead).weak()).wrap());
            ui.add_space(4.0);
        }
        let mut offered = false;
        if let Some(number_box) = self.number_box.as_ref().filter(|number_box| number_box.first) {
            offered = true;
            if let Some(literal) = number_box_ui(ui, popup_id, number_box) {
                choice = Some(Choice::Value(SheetValue::Literal(literal)));
            }
        }
        if self.list_box {
            offered = true;
            if let Some(numbers) = list_box_ui(ui, popup_id) {
                choice = Some(Choice::Numbers(numbers));
            }
        }
        for group in &self.groups {
            if group.items.is_empty() {
                continue;
            }
            offered = true;
            overlay_group(ui, &group.heading);
            for item in &group.items {
                let row = OverlayItem {
                    label: &item.label,
                    name: item.name.as_deref(),
                    detail: item.detail.as_deref(),
                    described: true,
                    current: self.current.as_deref() == Some(item.label.as_str()),
                    swatch: item.swatch,
                };
                if overlay_item(ui, &row).clicked() {
                    choice = Some(item.choice.clone());
                }
            }
        }
        if self.new_parameter || self.number_box.as_ref().is_some_and(|number_box| !number_box.first) {
            ui.add_space(4.0);
        }
        if self.new_parameter {
            offered = true;
            let row = OverlayItem {
                label: NEW_PARAMETER,
                ..OverlayItem::default()
            };
            if overlay_item(ui, &row).clicked() {
                choice = Some(Choice::NewParameter);
            }
        }
        if let Some(number_box) = self.number_box.as_ref().filter(|number_box| !number_box.first) {
            offered = true;
            ui.add_space(4.0);
            if let Some(literal) = number_box_ui(ui, popup_id, number_box) {
                choice = Some(Choice::Value(SheetValue::Literal(literal)));
            }
        }
        if let Some(remove) = self.remove {
            offered = true;
            ui.add_space(4.0);
            let row = OverlayItem {
                label: remove,
                ..OverlayItem::default()
            };
            if overlay_item(ui, &row).clicked() {
                choice = Some(Choice::Remove);
            }
        }
        if !offered {
            ui.add(egui::Label::new(RichText::new(self.empty).weak()).wrap());
        }
        choice
    }
}

/// Text typed in a box of a picker and the messages under it.
#[derive(Debug, Clone, Default)]
struct BoxDraft {
    /// Text in the box.
    text: String,
    /// Messages under the box after its text was refused.
    messages: Vec<&'static str>,
    /// Whether the box has been drawn since the picker opened.
    shown: bool,
}

/// Draws the Number box of the picker `popup_id`, and returns the number typed once Enter or Use is pressed on text
/// that reads in its range.
///
/// A list such as "2, 3" says to choose "is one of" first.
fn number_box_ui(ui: &mut Ui, popup_id: Id, number_box: &NumberBox) -> Option<SheetLiteral> {
    let hint = match number_box.number_use {
        NumberUse::Chance => CHANCE_BOX_HINT,
        NumberUse::Rate | NumberUse::Number | NumberUse::Times => NUMBER_BOX_HINT,
    };
    ui.weak(hint);
    let id = popup_id.with(NUMBER_BOX_ID);
    let mut draft: BoxDraft = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let first_frame = !draft.shown;
    if first_frame {
        draft.text.clone_from(&number_box.typed);
        draft.shown = true;
    }
    let mut typed = None;
    ui.horizontal(|ui| {
        let field = ui.add(TextEdit::singleline(&mut draft.text).desired_width(140.0));
        set_name(ui, field.id, hint);
        if first_frame && number_box.first {
            field.request_focus();
        }
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        if ui.button(USE_BUTTON).clicked() || entered {
            match number_box.number_use.read(&draft.text) {
                Ok(literal) => {
                    typed = Some(literal);
                    draft = BoxDraft::default();
                }
                Err(message) => {
                    draft.messages = vec![message];
                    if message == TYPE_ONE_NUMBER && read_numbers(&draft.text).is_some_and(|numbers| numbers.len() > 1)
                    {
                        draft.messages.push(LIST_IN_ONE_NUMBER);
                    }
                    // The refused text stays in the box. The box keeps the focus for another try.
                    field.request_focus();
                }
            }
        }
    });
    for message in &draft.messages {
        message_ui(ui, message);
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
    typed
}

/// Draws the list box of the picker `popup_id`, and returns the numbers typed once Enter or Use is pressed on text
/// that reads as one or more numbers.
fn list_box_ui(ui: &mut Ui, popup_id: Id) -> Option<Vec<SheetLiteral>> {
    overlay_group(ui, LIST_GROUP);
    ui.weak(LIST_HINT);
    let id = popup_id.with(LIST_BOX_ID);
    let mut draft: BoxDraft = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let first_frame = !draft.shown;
    draft.shown = true;
    let mut typed = None;
    ui.horizontal(|ui| {
        let field = ui.add(TextEdit::singleline(&mut draft.text).desired_width(140.0));
        set_name(ui, field.id, LIST_HINT);
        if first_frame {
            field.request_focus();
        }
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        if ui.button(USE_BUTTON).clicked() || entered {
            if let Some(numbers) = read_numbers(&draft.text) {
                typed = Some(numbers);
                draft = BoxDraft::default();
            } else {
                draft.messages = vec![TYPE_ONE_NUMBER];
                field.request_focus();
            }
        }
    });
    for message in &draft.messages {
        message_ui(ui, message);
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
    typed
}

/// Returns the numbers typed as `text`, separated by commas, or `None` when a part is not one number or none is typed.
pub fn read_numbers(text: &str) -> Option<Vec<SheetLiteral>> {
    let numbers: Option<Vec<SheetLiteral>> = text
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .map(read_typed_number)
        .collect();
    numbers.filter(|numbers| !numbers.is_empty())
}

// Things a line can use

/// Place of a picker: the sheet, the card and the line of the place it fills.
#[derive(Debug)]
pub struct LineContext<'a> {
    /// The sheet.
    pub document: &'a SheetDocument,
    /// Writer of the sheet's sentences.
    pub sentences: &'a Sentences<'a>,
    /// Lines of the card that holds the line, in display order.
    pub relations: &'a CardRelations<'a>,
    /// Id of the line.
    pub line_id: &'a str,
    /// The line.
    pub line: &'a SheetLine,
}

/// Kind of result a line works out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultKind {
    /// A number: a count, a total or average, a calculation, a random number, or a value chosen from others.
    Number,
    /// A yes or no: a comparison, an all of or any of, a chance or a rate.
    YesOrNo,
    /// An arrow: a random direction.
    Arrow,
    /// A cell of the ground: a picked cell, or the cell here or ahead.
    Place,
}

/// Returns the kinds of result `line` can work out, empty for a change, a block, or a look at agents, whose summaries
/// are its results. A Choose or a Depending on gives the kind of its values, a number, an arrow or a place.
fn result_kinds(line: &SheetLine) -> &'static [ResultKind] {
    match line.kind {
        LineKind::LookAround if line.around == Some(Around::AgentsWithin) => &[],
        LineKind::Count | LineKind::LookAround | LineKind::Calculate | LineKind::RandomNumber => &[ResultKind::Number],
        LineKind::Choose | LineKind::DependingOn => &[ResultKind::Number, ResultKind::Arrow, ResultKind::Place],
        LineKind::Compare
        | LineKind::AllOf
        | LineKind::AnyOf
        | LineKind::NoneOf
        | LineKind::Chance
        | LineKind::Rate => &[ResultKind::YesOrNo],
        LineKind::RandomDirection => &[ResultKind::Arrow],
        LineKind::PickCell | LineKind::LookAtCell => &[ResultKind::Place],
        _ => &[],
    }
}

/// Returns whether a line of `kind` works out a result, so that it has a name to rename.
pub fn is_result_line(kind: LineKind) -> bool {
    !matches!(
        kind,
        LineKind::Become
            | LineKind::Set
            | LineKind::Steer
            | LineKind::KeepBetween
            | LineKind::Move
            | LineKind::Leave
            | LineKind::Record
            | LineKind::When
            | LineKind::Repeat
            | LineKind::Always
    )
}

impl LineContext<'_> {
    /// Returns the ids of the lines above the line whose results it can use, in display order: every line above it
    /// on its card, a line inside a When or Repeat only to the lines after it inside that block.
    fn lines_above(&self) -> Vec<&str> {
        let Some(position) = self.relations.index(self.line_id) else {
            return Vec::new();
        };
        self.relations
            .order()
            .iter()
            .take(position)
            .copied()
            .filter(|id| {
                self.relations
                    .parent(id)
                    .is_none_or(|block| self.relations.is_within(self.line_id, block))
            })
            .collect()
    }

    /// Returns the result lines above the line that it can use, of the kind `kind`, as items that put the result in
    /// the place, each named by its result's name with "line N: [its sentence]" under it.
    ///
    /// `condition` is whether the items read "when [name]", as in "Choose when".
    fn results_above(&self, kind: ResultKind, condition: bool) -> Vec<PickerItem> {
        self.results_above_of(&[kind], condition)
    }

    /// Returns the result lines above the line that it can use, of any of the kinds `kinds`, once each, as
    /// [`Self::results_above`] does.
    fn results_above_of(&self, kinds: &[ResultKind], condition: bool) -> Vec<PickerItem> {
        self.lines_above()
            .into_iter()
            .filter_map(|id| {
                let line = self.relations.line(id)?;
                if !result_kinds(line).iter().any(|kind| kinds.contains(kind)) {
                    return None;
                }
                let name = line
                    .name
                    .clone()
                    .or_else(|| automatic_name(self.document, line))
                    .unwrap_or_else(|| id.to_owned());
                let number = self.relations.number(id).unwrap_or(0);
                Some(PickerItem {
                    label: if condition {
                        result_condition_item(&name, self.line.kind == LineKind::When)
                    } else {
                        name
                    },
                    name: None,
                    detail: Some(result_description(number, &self.sentences.result_words(line))),
                    swatch: None,
                    choice: Choice::Value(SheetValue::Chip(SheetChip::Result {
                        line: id.to_owned(),
                        part: None,
                        property: None,
                    })),
                })
            })
            .collect()
    }

    /// Returns the summaries of the looks at agents above the line that it can use, and the properties and amounts
    /// of the cells picked above it, as items that put the part in the place, each with "line N: [its sentence]" under
    /// it.
    fn parts_above(&self) -> Vec<PickerItem> {
        let mut items = Vec::new();
        for id in self.lines_above() {
            let Some(line) = self.relations.line(id) else {
                continue;
            };
            let number = self.relations.number(id).unwrap_or(0);
            let detail = result_description(number, &self.sentences.result_words(line));
            let mut chips: Vec<SheetChip> = Vec::new();
            if line.kind == LineKind::LookAround && line.around == Some(Around::AgentsWithin) {
                chips.extend(line.summaries.iter().map(|summary| SheetChip::Result {
                    line: id.to_owned(),
                    part: Some(summary.id.clone()),
                    property: None,
                }));
            }
            if matches!(line.kind, LineKind::PickCell | LineKind::LookAtCell) {
                if line.kind == LineKind::PickCell && line.pick.is_some_and(|pick| pick != PickRule::Random) {
                    chips.push(SheetChip::Result {
                        line: id.to_owned(),
                        part: Some("amount".to_owned()),
                        property: None,
                    });
                }
                chips.extend(self.ground_properties().iter().map(|property| SheetChip::Result {
                    line: id.to_owned(),
                    part: None,
                    property: Some(property.id.clone()),
                }));
            }
            items.extend(chips.into_iter().map(|chip| {
                let value = SheetValue::Chip(chip);
                PickerItem {
                    label: self.sentences.value(&value, None),
                    name: None,
                    detail: Some(detail.clone()),
                    swatch: None,
                    choice: Choice::Value(value),
                }
            }));
        }
        items
    }

    /// Returns the id of the kind of thing the line's card runs over: `cells` or a kind of agent's id.
    fn every(&self) -> &str {
        self.document
            .line(self.line_id)
            .and_then(|(card, _)| self.document.cards.get(card))
            .map_or("cells", |card| card.every.as_str())
    }

    /// Returns whether the line's card runs over agents.
    fn agents(&self) -> bool {
        self.every() != "cells"
    }

    /// Returns the properties of the kind of thing the line's card runs over.
    fn properties(&self) -> &[SheetProperty] {
        let every = self.every();
        if every == "cells" {
            return self.document.cells.as_deref().unwrap_or_default();
        }
        self.document
            .agents
            .iter()
            .find(|kind| kind.id == every)
            .map_or(&[], |kind| kind.properties.as_slice())
    }

    /// Returns the properties of the ground under agents.
    fn ground_properties(&self) -> &[SheetProperty] {
        self.document
            .ground
            .as_ref()
            .map_or(&[], |ground| ground.properties.as_slice())
    }

    /// Returns the heading of the group of the properties of the thing the line's card runs over, "This cell's
    /// properties" or "This bird's properties".
    fn my_heading(&self) -> String {
        if self.agents() {
            this_things_properties(&self.sentences.thing_word(self.every()))
        } else {
            THIS_CELLS_PROPERTIES.to_owned()
        }
    }

    /// Returns the thing's properties as items "my Food", with "this cell's Food" beside each, keeping those for which
    /// `keep` holds. On a card over agents, the value a property takes from the changes above the line follows each
    /// property that a line above changes for the last time, as "my new Velocity".
    fn my_properties(&self, keep: impl Fn(&SheetProperty) -> bool) -> Vec<PickerItem> {
        let thing = self.sentences.thing_word(self.every());
        let agents = self.agents();
        let mut items = Vec::new();
        for property in self.properties().iter().filter(|property| keep(property)) {
            items.push(PickerItem {
                label: my_property_item(&property.name),
                name: None,
                detail: Some(if agents {
                    this_things_property(&thing, &property.name)
                } else {
                    this_cells_property(&property.name)
                }),
                swatch: None,
                choice: Choice::Value(SheetValue::Chip(SheetChip::My(property.id.clone()))),
            });
            if agents && let Some(last) = self.last_change_above(property) {
                items.push(PickerItem {
                    label: my_new_property_item(&property.name),
                    name: None,
                    detail: Some(my_new_detail(&thing, &property.name, last)),
                    swatch: None,
                    choice: Choice::Value(SheetValue::Chip(SheetChip::MyNew(property.id.clone()))),
                });
            }
        }
        items
    }

    /// Returns the number of the last line of the card that changes `property` when that line sits above the line,
    /// `None` when no line changes it or a line at or below the line does.
    fn last_change_above(&self, property: &SheetProperty) -> Option<usize> {
        let position = self.relations.index(self.line_id)?;
        let order = self.relations.order();
        let last = order.iter().rposition(|id| {
            self.relations
                .line(id)
                .is_some_and(|line| changes_property(line, &property.id, Some(property.property_type)))
        })?;
        (last < position).then(|| self.relations.number(order[last]).unwrap_or(last + 1))
    }

    /// Returns the length of each arrow property of the thing, as items "length of my Velocity" whose value is the
    /// formula that works it out.
    fn lengths(&self) -> Vec<PickerItem> {
        let thing = self.sentences.thing_word(self.every());
        self.properties()
            .iter()
            .filter(|property| property.property_type == PropertyType::Arrow)
            .map(|property| {
                let mut formula = SheetLine::new(LineKind::Calculate);
                formula.formula = Some(format!("length(my.{})", property.id));
                PickerItem {
                    label: length_of_item(&property.name),
                    name: None,
                    detail: Some(this_things_property(&thing, &property.name)),
                    swatch: None,
                    choice: Choice::Value(SheetValue::Phrase(Box::new(formula))),
                }
            })
            .collect()
    }

    /// Returns what a look at agents reads from each agent it looks at: their properties and the relations to them.
    fn looked_at(&self) -> PickerGroup {
        let mut items: Vec<PickerItem> = self
            .properties()
            .iter()
            .filter(|property| property.property_type != PropertyType::States)
            .map(|property| PickerItem {
                label: their_property_item(&property.name),
                name: None,
                detail: None,
                swatch: None,
                choice: Choice::Value(SheetValue::Chip(SheetChip::Their(property.id.clone()))),
            })
            .collect();
        items.extend(
            [
                ChipRelation::OffsetToThem,
                ChipRelation::OffsetAwayFromThem,
                ChipRelation::DistanceToThem,
            ]
            .into_iter()
            .map(|relation| PickerItem {
                label: relation_words(relation).to_owned(),
                name: None,
                detail: None,
                swatch: None,
                choice: Choice::Value(SheetValue::Chip(SheetChip::Relation(relation))),
            }),
        );
        PickerGroup {
            heading: each_looked_at(&self.sentences.thing_word(self.every())),
            items,
        }
    }

    /// Returns the parameters as items, each with its value and unit beside it.
    ///
    /// A rate's picker leaves out a parameter whose Unit is set and names no unit of time, and a chance's picker leaves
    /// out a rate parameter, one whose Unit names a unit of time, as "per day" does. Any other number offers every
    /// parameter.
    fn parameters(&self, number_use: NumberUse) -> Vec<PickerItem> {
        self.document
            .parameters
            .iter()
            .filter(|parameter| match number_use {
                NumberUse::Chance => parameter_rate_unit(parameter).is_none(),
                NumberUse::Rate => parameter.unit.is_none() || parameter_rate_unit(parameter).is_some(),
                NumberUse::Number => true,
                NumberUse::Times => parameter.parameter_type == ParameterType::WholeNumber,
            })
            .map(|parameter| {
                let value = format_number(parameter.default).unwrap_or_default();
                PickerItem {
                    label: parameter.name.clone(),
                    name: None,
                    detail: Some(match &parameter.unit {
                        Some(unit) => format!("{value} {unit}"),
                        None => value,
                    }),
                    swatch: None,
                    choice: Choice::Value(SheetValue::Chip(SheetChip::Param(parameter.id.clone()))),
                }
            })
            .collect()
    }

    /// Returns the states of `property`, or of every States property when `property` is `None`, as items named as chips
    /// name them. A state of the ground is offered too on a card over agents.
    fn states(&self, property: Option<&str>) -> Vec<PickerItem> {
        let states_properties: Vec<&SheetProperty> = self
            .properties()
            .iter()
            .chain(self.ground_properties().iter().filter(|_| self.agents()))
            .filter(|candidate| candidate.property_type == PropertyType::States)
            .collect();
        states_properties
            .iter()
            .filter(|candidate| property.is_none_or(|id| candidate.id == id))
            .flat_map(|owner| {
                owner.states.iter().map(|state| {
                    let shared = states_properties
                        .iter()
                        .any(|other| other.id != owner.id && other.states.iter().any(|known| known.name == state.name));
                    PickerItem {
                        label: shared_state_name(&owner.name, &state.name, shared),
                        name: None,
                        detail: None,
                        swatch: Some(color32(state.color)),
                        choice: Choice::State {
                            property: owner.id.clone(),
                            state: state.id.clone(),
                        },
                    }
                })
            })
            .collect()
    }
}

/// Returns whether `property` holds a number.
fn is_number(property: &SheetProperty) -> bool {
    matches!(property.property_type, PropertyType::Number | PropertyType::WholeNumber)
}

/// Returns whether the property `property` fits the property of a line of `kind` on a card over agents when `agents`
/// holds: a Number or an Arrow to steer, a number or an Arrow to keep between bounds, anything but a States property, a
/// Position or a Heading to set on a card over agents, a number to set on a card over cells, and a States property
/// otherwise.
fn fits_property(kind: LineKind, agents: bool, property: &SheetProperty) -> bool {
    let arrow = property.property_type == PropertyType::Arrow;
    match kind {
        LineKind::Steer => arrow || property.property_type == PropertyType::Number,
        LineKind::KeepBetween => arrow || is_number(property),
        LineKind::Set if agents => !matches!(
            property.property_type,
            PropertyType::States | PropertyType::Position | PropertyType::Heading
        ),
        LineKind::Set => is_number(property),
        _ => property.property_type == PropertyType::States,
    }
}

/// Returns the property that a value in a slot reads, `None` for any other value.
pub fn read_property(value: Option<&SheetValue>) -> Option<&str> {
    match value? {
        SheetValue::Chip(SheetChip::My(id) | SheetChip::Their(id) | SheetChip::MyNew(id)) => Some(id),
        _ => None,
    }
}

/// Returns the text a Number box shows for a value already in the place: a typed number as it was typed.
pub fn typed_text(value: Option<&SheetValue>) -> String {
    match value {
        Some(SheetValue::Literal(literal)) if literal.number().is_some() => literal_words(*literal),
        _ => String::new(),
    }
}

/// Returns the picker of a place that wants `want` on the line of `context`.
///
/// `property` is the property whose states the place offers, `None` for every States property. `filled` is whether
/// the place holds a value, a filled item of a list offering "Remove from list". `typed` is the text the Number box
/// shows, and `draw` the kind of draw a "for each" grows.
pub fn picker_for(
    context: &LineContext<'_>,
    want: Blank,
    property: Option<&str>,
    filled: bool,
    typed: &str,
    draw: DrawKind,
) -> CardPicker {
    let title = want.picker_title();
    let one_group = |empty: &'static str, heading: &str, items: Vec<PickerItem>| {
        let mut picker = CardPicker::titled(title.clone(), empty);
        picker.groups = vec![PickerGroup {
            heading: heading.to_owned(),
            items,
        }];
        picker
    };
    let numbers = |first: bool, number_use: NumberUse| number_picker(context, title.clone(), first, number_use, typed);
    let agents = context.agents();
    let kind = context.line.kind;
    match want {
        Blank::State => one_group(NO_STATES, STATES, context.states(property)),
        Blank::OldState => {
            let mut items = vec![PickerItem {
                label: ANY_STATE.to_owned(),
                name: None,
                detail: None,
                swatch: None,
                choice: Choice::AnyState,
            }];
            items.extend(context.states(property));
            one_group(NO_STATES, STATES, items)
        }
        Blank::StateList => {
            let mut picker = one_group(NO_STATES, STATES, context.states(property));
            picker.remove = filled.then_some(REMOVE_FROM_LIST);
            picker
        }
        Blank::Property => one_group(
            if agents { NO_PROPERTY } else { NO_STATES_PROPERTY },
            PROPERTIES,
            property_items(context, |property| fits_property(kind, agents, property)),
        ),
        Blank::NumberProperty => one_group(
            NO_NUMBER_PROPERTY_ON_SHEET,
            PROPERTIES,
            property_items(context, is_number),
        ),
        Blank::GroundProperty => one_group(NO_GROUND_PROPERTY, GROUND_PROPERTIES, ground_number_items(context)),
        Blank::Event => one_group(NO_EVENT, EVENTS, event_items(context)),
        Blank::Value if agents && kind == LineKind::Move && context.line.mode == Some(LineMode::To) => one_group(
            NO_PLACE,
            FROM_LINES_ABOVE,
            context.results_above(ResultKind::Place, false),
        ),
        Blank::Value => value_picker(context, numbers(false, NumberUse::Number)),
        Blank::NumberList => {
            let mut picker = CardPicker::titled(title, NO_NUMBER_RESULT);
            picker.list_box = true;
            picker.remove = filled.then_some(REMOVE_FROM_LIST);
            picker
        }
        Blank::Chance => numbers(true, NumberUse::Chance),
        Blank::Rate => numbers(true, NumberUse::Rate),
        Blank::Times => numbers(true, NumberUse::Times),
        Blank::Count => {
            let mut picker = numbers(false, NumberUse::Number);
            picker.lead = Some(for_each_lead(draw).to_owned());
            picker.groups.retain(|group| group.heading == FROM_LINES_ABOVE);
            picker.groups.truncate(1);
            picker.new_parameter = false;
            picker.remove = filled.then_some(REMOVE_FOR_EACH);
            picker
        }
        Blank::YesOrNo | Blank::YesOrNoList => {
            let mut picker = one_group(
                NO_YES_OR_NO_RESULT,
                FROM_LINES_ABOVE,
                context.results_above(ResultKind::YesOrNo, false),
            );
            picker.remove = (filled && want == Blank::YesOrNoList).then_some(REMOVE_FROM_LIST);
            picker
        }
        _ => numbers(true, NumberUse::Number),
    }
}

/// Returns `picker`, a picker of a number, as the picker of any value on the line of `context`: every result above, and
/// every property of the thing. On a card over agents the results add the summaries and cells above, and the new
/// values add a random direction.
fn value_picker(context: &LineContext<'_>, mut picker: CardPicker) -> CardPicker {
    let agents = context.agents();
    let mut results = if agents {
        context.results_above_of(&[ResultKind::Number, ResultKind::YesOrNo, ResultKind::Arrow], false)
    } else {
        let mut results = context.results_above(ResultKind::Number, false);
        results.extend(context.results_above(ResultKind::YesOrNo, false));
        results
    };
    if agents {
        results.extend(context.parts_above());
    }
    picker.groups[0].items = results;
    picker.groups[1].items = context.my_properties(|_| true);
    if agents {
        // The new values come before the parameters, as a long picker scrolls.
        picker.groups.retain(|group| group.heading != OR_A_NEW_VALUE);
        picker.groups.insert(
            2,
            PickerGroup {
                heading: OR_A_NEW_VALUE.to_owned(),
                items: new_values(true),
            },
        );
    }
    picker
}

/// Returns the Number properties of the ground under agents as items that choose the property.
fn ground_number_items(context: &LineContext<'_>) -> Vec<PickerItem> {
    context
        .ground_properties()
        .iter()
        .filter(|property| property.property_type == PropertyType::Number)
        .map(|property| PickerItem {
            label: property.name.clone(),
            name: None,
            detail: None,
            swatch: None,
            choice: Choice::Property(property.id.clone()),
        })
        .collect()
}

/// Returns the sheet's events as items that choose the event.
fn event_items(context: &LineContext<'_>) -> Vec<PickerItem> {
    context
        .document
        .events
        .iter()
        .map(|event| PickerItem {
            label: event.name.clone(),
            name: None,
            detail: None,
            swatch: None,
            choice: Choice::Event(event.id.clone()),
        })
        .collect()
}

/// Returns the picker of a number titled `title`: the number results above, the thing's Number properties, the
/// parameters with "New parameter...", and the Number box reading in `number_use`, first or last, showing `typed`.
///
/// On a card over agents the results add the summaries and amounts above, the properties add the length of each
/// arrow, and a random number between two values comes last. A number of times takes integer parameters alone, with
/// no results or properties, since a Repeat runs a typed number or an integer parameter of times.
fn number_picker(
    context: &LineContext<'_>,
    title: String,
    first: bool,
    number_use: NumberUse,
    typed: &str,
) -> CardPicker {
    let agents = context.agents() && number_use != NumberUse::Times;
    let mut picker = CardPicker::titled(title, NO_NUMBER_RESULT);
    if number_use != NumberUse::Times {
        let mut results = context.results_above(ResultKind::Number, false);
        let mut properties = context.my_properties(is_number);
        if agents {
            results.extend(context.parts_above());
            properties.extend(context.lengths());
        }
        picker.groups = vec![
            PickerGroup {
                heading: FROM_LINES_ABOVE.to_owned(),
                items: results,
            },
            PickerGroup {
                heading: context.my_heading(),
                items: properties,
            },
        ];
    }
    picker.groups.push(PickerGroup {
        heading: PARAMETERS.to_owned(),
        items: context.parameters(number_use),
    });
    if agents && number_use == NumberUse::Number {
        // The new values come before the parameters, as a long picker scrolls.
        picker.groups.insert(
            2,
            PickerGroup {
                heading: OR_A_NEW_VALUE.to_owned(),
                items: new_values(false),
            },
        );
    }
    picker.new_parameter = true;
    picker.number_box = Some(NumberBox {
        number_use,
        first,
        typed: typed.to_owned(),
    });
    picker
}

/// Returns the new values a picker of a value offers on a card over agents: a random number between two values, and
/// when `arrow` holds a random direction of a length. Each is a phrase whose own values start blank.
fn new_values(arrow: bool) -> Vec<PickerItem> {
    let mut items = vec![PickerItem {
        label: AddItem::RandomNumber.pattern(None),
        name: None,
        detail: None,
        swatch: None,
        choice: Choice::Value(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::RandomNumber)))),
    }];
    if arrow {
        items.push(PickerItem {
            label: AddItem::RandomDirection.pattern(None),
            name: None,
            detail: None,
            swatch: None,
            choice: Choice::Value(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::RandomDirection)))),
        });
    }
    items
}

/// Returns the properties of the thing for which `keep` holds, as items that choose the property.
fn property_items(context: &LineContext<'_>, keep: impl Fn(&SheetProperty) -> bool) -> Vec<PickerItem> {
    context
        .properties()
        .iter()
        .filter(|property| keep(property))
        .map(|property| PickerItem {
            label: property.name.clone(),
            name: None,
            detail: None,
            swatch: None,
            choice: Choice::Property(property.id.clone()),
        })
        .collect()
}

/// Returns "Choose when" for the line of `context`: inside a When the lead that names it, then its yes or no results
/// above as "when [name]", then the forms of a condition.
///
/// `otherwise` holds the numbers of the higher lines that would override the line were its condition always. While it
/// is not empty, "otherwise" stands in place of "always". On a When line the items start with "When", and "always"
/// is left out, since a When cannot be always.
pub fn choose_when(context: &LineContext<'_>, otherwise: &[usize]) -> CardPicker {
    let mut picker = CardPicker::titled(CHOOSE_WHEN.to_owned(), NO_YES_OR_NO_RESULT);
    let on_when = context.line.kind == LineKind::When;
    if !on_when
        && let Some(parent) = context.relations.parent(context.line_id)
        && context
            .relations
            .line(parent)
            .is_some_and(|block| block.kind == LineKind::When)
    {
        picker.lead = context.relations.number(parent).map(when_lead);
    }
    let results = context.results_above(ResultKind::YesOrNo, true);
    let heading = if results.is_empty() {
        CONDITIONS
    } else {
        OR_A_NEW_CONDITION
    };
    let forms = ConditionItem::ALL
        .into_iter()
        .filter(|item| offers_condition(context, *item))
        .map(|item| match item {
            ConditionItem::Always if !otherwise.is_empty() => PickerItem {
                label: OTHERWISE.to_owned(),
                name: Some(otherwise_item_name(otherwise)),
                detail: Some(otherwise_description(otherwise)),
                swatch: None,
                choice: Choice::Condition(item),
            },
            _ => PickerItem {
                label: item.label(on_when),
                name: None,
                detail: item.description().map(str::to_owned),
                swatch: None,
                choice: Choice::Condition(item),
            },
        })
        .collect();
    picker.groups = vec![
        PickerGroup {
            heading: FROM_LINES_ABOVE.to_owned(),
            items: results,
        },
        PickerGroup {
            heading: heading.to_owned(),
            items: forms,
        },
    ];
    picker
}

/// Returns whether "Choose when" offers the form `item` on the line of `context`.
///
/// A When cannot be always. "at rate ... per ..." is offered on a card that runs each tick. A button's card has no
/// duration and takes chances only.
fn offers_condition(context: &LineContext<'_>, item: ConditionItem) -> bool {
    match item {
        ConditionItem::Always => context.line.kind != LineKind::When,
        ConditionItem::AtRate => runs_each_tick(context),
        _ => true,
    }
}

/// Returns whether the card that holds the line of `context` runs each tick.
fn runs_each_tick(context: &LineContext<'_>) -> bool {
    context
        .document
        .line(context.line_id)
        .and_then(|(card, _)| context.document.cards.get(card))
        .is_some_and(|card| card.runs == CardRuns::EachTick)
}

// The selects inside a line

/// One option of a select inside a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOption {
    /// A comparison.
    Test(CompareTest),
    /// A neighbourhood of cells.
    Around(Around),
    /// Total or average.
    Summary(SummaryKind),
    /// An operation of a Calculate line: `+`, `-`, `*` or `/`.
    Operation(char),
    /// The unit of time of a rate.
    Unit(TimeUnit),
    /// The mode of a Steer or Move line.
    Mode(LineMode),
    /// Rule a Pick a cell line picks by.
    Pick(PickRule),
    /// The cell a Look at a cell line looks at.
    Which(CellWhich),
    /// Kind of the summary at an index of a look at agents, as `(index, kind)`.
    LookSummary(usize, SummaryKind),
    /// "Remove this summary" in the select of the summary at an index.
    RemoveSummary(usize),
}

/// Returns the options of the select of a Steer line's mode, "by" and "towards", or of a Move line's, "by" and "to",
/// as `steer` says.
pub fn mode_options(steer: bool) -> Vec<(String, SelectOption)> {
    let second = if steer { LineMode::Towards } else { LineMode::To };
    [LineMode::By, second]
        .into_iter()
        .map(|mode| (mode.as_str().to_owned(), SelectOption::Mode(mode)))
        .collect()
}

/// Returns the options of the select of what a Pick a cell line picks: "most", "least" and "random".
pub fn pick_options() -> Vec<(String, SelectOption)> {
    PickRule::ALL
        .iter()
        .map(|pick| (pick.as_str().to_owned(), SelectOption::Pick(*pick)))
        .collect()
}

/// Returns the options of the select of the cell a Look at a cell line looks at.
pub fn which_options() -> Vec<(String, SelectOption)> {
    [
        (LOOK_AT_HERE, CellWhich::Here),
        (LOOK_AT_AHEAD, CellWhich::Ahead),
        (LOOK_AT_PLACE, CellWhich::At),
    ]
    .into_iter()
    .map(|(words, which)| (words.to_owned(), SelectOption::Which(which)))
    .collect()
}

/// Returns the options of the select of the summary at `index` of a look at agents: each way to sum the agents up,
/// then "Remove this summary" when `removable`.
pub fn look_summary_options(index: usize, removable: bool) -> Vec<(String, SelectOption)> {
    let mut options: Vec<(String, SelectOption)> = LOOK_SUMMARIES
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), SelectOption::LookSummary(index, kind)))
        .collect();
    if removable {
        options.push((REMOVE_SUMMARY.to_owned(), SelectOption::RemoveSummary(index)));
    }
    options
}

/// Ways a look at agents sums them up, in the order the editor offers them.
pub const LOOK_SUMMARIES: [SummaryKind; 5] = [
    SummaryKind::Average,
    SummaryKind::Total,
    SummaryKind::Largest,
    SummaryKind::Smallest,
    SummaryKind::Count,
];

/// Returns the picker of a new summary of a look at agents: each way to sum the agents up.
pub fn summary_picker() -> CardPicker {
    let mut picker = CardPicker::titled(CHOOSE_A_SUMMARY.to_owned(), NO_PROPERTY);
    picker.groups = vec![PickerGroup {
        heading: SUMMARIES.to_owned(),
        items: LOOK_SUMMARIES
            .into_iter()
            .map(|kind| PickerItem {
                label: summary_item(kind),
                name: None,
                detail: None,
                swatch: None,
                choice: Choice::Summary(kind),
            })
            .collect(),
    }];
    picker
}

/// Returns the picker of what a summary of a look at agents reads from each agent: their properties and the relations
/// to them.
pub fn looked_at_picker(context: &LineContext<'_>) -> CardPicker {
    let mut picker = CardPicker::titled(Blank::Value.picker_title(), NO_PROPERTY);
    picker.groups = vec![context.looked_at()];
    picker
}

/// Returns the options of the select of a comparison, with their words, where `of_states` is whether the value on its
/// left is a States property.
pub fn comparison_options(of_states: bool) -> Vec<(String, SelectOption)> {
    COMPARISONS
        .iter()
        .map(|test| (comparison_words(*test, of_states).to_owned(), SelectOption::Test(*test)))
        .collect()
}

/// Returns the options of the select of a neighbourhood of cells, with their words.
pub fn around_options() -> Vec<(String, SelectOption)> {
    [Around::Cells8, Around::Cells4, Around::Block3x3]
        .into_iter()
        .filter_map(|around| around_words(around).map(|words| (words.to_owned(), SelectOption::Around(around))))
        .collect()
}

/// Returns the options of the select of total or average, with their words.
pub fn summary_options() -> Vec<(String, SelectOption)> {
    [SummaryKind::Total, SummaryKind::Average]
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), SelectOption::Summary(kind)))
        .collect()
}

/// Returns the options of the select of an operation, with their words.
pub fn operation_options() -> Vec<(String, SelectOption)> {
    ['+', '-', '*', '/']
        .into_iter()
        .filter_map(|operator| {
            operation_words(&operator.to_string()).map(|words| (words.to_owned(), SelectOption::Operation(operator)))
        })
        .collect()
}

/// Returns the options of the select of a rate's unit, with their words: "second" to "year".
pub fn unit_options() -> Vec<(String, SelectOption)> {
    TimeUnit::ALL
        .into_iter()
        .map(|unit| (rate_unit_word(unit).to_owned(), SelectOption::Unit(unit)))
        .collect()
}

// The commands a choice runs

/// Values chosen for a Calculate line of one operation while its formula is not written yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalcDraft {
    /// Value on the left of the operation, as the formula writes it.
    pub first: Option<String>,
    /// The operator: `+`, `-`, `*` or `/`.
    pub operator: char,
    /// Value on the right of the operation, as the formula writes it.
    pub second: Option<String>,
}

impl Default for CalcDraft {
    fn default() -> Self {
        Self {
            first: None,
            operator: '+',
            second: None,
        }
    }
}

/// Draft of a Calculate line that an edit leaves, as `(line, draft)`, with `None` for a draft removed.
pub type DraftUpdate = (String, Option<CalcDraft>);

/// Commands that a choice runs, and the draft of a Calculate line it leaves.
#[derive(Debug, Default)]
pub struct Applied {
    /// The commands, empty for a choice that only changes a draft.
    pub edits: Vec<SheetEdit>,
    /// Draft of a Calculate line the choice leaves, `None` for a choice that leaves the drafts alone.
    pub draft: Option<DraftUpdate>,
}

/// Returns the commands that put `choice` at `place` of line `line_id` of `document`, where `draft` is the line's draft
/// when it is an empty Calculate line.
pub fn apply_choice(
    document: &SheetDocument,
    draft: Option<CalcDraft>,
    line_id: &str,
    place: &PartPlace,
    choice: &Choice,
) -> Applied {
    let Some((_, line)) = document.line(line_id) else {
        return Applied::default();
    };
    match &place.target {
        PlaceTarget::Slot(path) => Applied {
            edits: slot_choice(document, line_id, line, path, place.want, choice),
            draft: None,
        },
        PlaceTarget::ListItem { slot, index } => Applied {
            edits: list_choice(line_id, line, slot, *index, choice),
            draft: None,
        },
        PlaceTarget::Field { phrase, field } => field_choice(line_id, line, draft, phrase, *field, choice),
        _ => Applied::default(),
    }
}

/// Returns the command that puts `value` in the slot at `path` of line `line_id`.
fn set_slot(line_id: &str, path: &str, value: Slot) -> SheetEdit {
    SheetEdit::SetSlot {
        line: line_id.to_owned(),
        path: path.to_owned(),
        value,
    }
}

/// Returns the commands that put `choice` in the slot at `path` of `line`, the line `line_id` of `document`.
///
/// `want` is what the slot takes. A state put in a slot that takes a list of states starts the list. A Count's state
/// also sets its property, and a Count's new property clears a state of another property.
fn slot_choice(
    document: &SheetDocument,
    line_id: &str,
    line: &SheetLine,
    path: &str,
    want: Option<Blank>,
    choice: &Choice,
) -> Vec<SheetEdit> {
    let filled = Slot::Filled;
    match choice {
        Choice::Condition(item) => match row_of_condition(path) {
            Some(row) => vec![SheetEdit::ChooseRowCondition {
                line: line_id.to_owned(),
                row,
                item: *item,
            }],
            None => vec![SheetEdit::ChooseCondition {
                line: line_id.to_owned(),
                item: *item,
            }],
        },
        Choice::Remove => vec![set_slot(line_id, path, Slot::Blank)],
        Choice::Numbers(numbers) => {
            let items = numbers.iter().map(|number| SheetValue::Literal(*number)).collect();
            vec![set_slot(line_id, path, filled(SheetValue::List(items)))]
        }
        Choice::State { property, state } => {
            let chip = SheetValue::Chip(SheetChip::State(state.clone()));
            let value = if want == Some(Blank::StateList) {
                SheetValue::List(vec![chip])
            } else {
                chip
            };
            let mut edits = vec![set_slot(line_id, path, filled(value))];
            if let Some(outer) = path.strip_suffix(".right")
                && outer.ends_with("whose")
            {
                let left = format!("{outer}.left");
                let their = SheetValue::Chip(SheetChip::Their(property.clone()));
                if slot_at(line, &left).and_then(Slot::value) != Some(&their) {
                    edits.insert(0, set_slot(line_id, &left, filled(their)));
                }
            }
            edits
        }
        Choice::Property(property) => {
            let mut edits = vec![set_slot(
                line_id,
                path,
                filled(SheetValue::Chip(SheetChip::Their(property.clone()))),
            )];
            if let Some(outer) = path.strip_suffix(".left") {
                let right = format!("{outer}.right");
                let owns = |state: &str| {
                    document
                        .cells
                        .iter()
                        .flatten()
                        .find(|known| known.id == *property)
                        .is_some_and(|owner| owner.states.iter().any(|known| known.id == state))
                };
                if let Some(SheetValue::Chip(SheetChip::State(state))) = slot_at(line, &right).and_then(Slot::value)
                    && !owns(state)
                {
                    edits.push(set_slot(line_id, &right, Slot::Blank));
                }
            }
            edits
        }
        Choice::Value(value) => vec![set_slot(line_id, path, filled(value.clone()))],
        Choice::Summary(summary) => vec![SheetEdit::AddSummary {
            line: line_id.to_owned(),
            summary: *summary,
        }],
        Choice::AnyState | Choice::NewParameter | Choice::Event(_) => Vec::new(),
    }
}

/// Returns the index of the row of a Choose line whose condition `path` names, as `row.1.condition`, `None` for any
/// other path.
fn row_of_condition(path: &str) -> Option<usize> {
    path.strip_prefix("row.")?.strip_suffix(".condition")?.parse().ok()
}

/// Returns the commands that put `choice` at item `index` of the list in the slot `slot` of `line`, the line
/// `line_id`. The index one past the last item adds an item, and a list left empty turns blank.
fn list_choice(line_id: &str, line: &SheetLine, slot: &str, index: usize, choice: &Choice) -> Vec<SheetEdit> {
    let mut items = match slot_at(line, slot).and_then(Slot::value) {
        Some(SheetValue::List(items)) => items.clone(),
        _ => Vec::new(),
    };
    let new_items: Vec<SheetValue> = match choice {
        Choice::Value(value) => vec![value.clone()],
        Choice::State { state, .. } => vec![SheetValue::Chip(SheetChip::State(state.clone()))],
        Choice::Numbers(numbers) => numbers.iter().map(|number| SheetValue::Literal(*number)).collect(),
        Choice::Remove => Vec::new(),
        _ => return Vec::new(),
    };
    let start = index.min(items.len());
    let end = (index + 1).min(items.len());
    items.splice(start..end, new_items);
    let value = if items.is_empty() {
        Slot::Blank
    } else {
        Slot::Filled(SheetValue::List(items))
    };
    vec![set_slot(line_id, slot, value)]
}

/// Returns the commands that put `choice` in the field `field` of the phrase at `phrase` of `line`, the line `line_id`,
/// where `draft` is the line's draft when it is an empty Calculate line.
///
/// An old state of another property also sets the line's property.
fn field_choice(
    line_id: &str,
    line: &SheetLine,
    draft: Option<CalcDraft>,
    phrase: &str,
    field: FieldPlace,
    choice: &Choice,
) -> Applied {
    let set_field = |field: LineField| SheetEdit::SetField {
        line: line_id.to_owned(),
        phrase: phrase.to_owned(),
        field,
    };
    let edits = match (field, choice) {
        (FieldPlace::NewState, Choice::State { property, state }) => {
            vec![set_field(LineField::NewState(Some((property.clone(), state.clone()))))]
        }
        (FieldPlace::StartState, Choice::AnyState) => {
            vec![set_field(LineField::StartState(Some(ANY_STATE_ID.to_owned())))]
        }
        (FieldPlace::StartState, Choice::State { property, state }) => {
            let mut edits = Vec::new();
            if line.property.as_deref() != Some(property.as_str()) {
                edits.push(set_field(LineField::Property(Some(property.clone()))));
            }
            edits.push(set_field(LineField::StartState(Some(state.clone()))));
            edits
        }
        (FieldPlace::Property, Choice::Property(property)) => {
            vec![set_field(LineField::Property(Some(property.clone())))]
        }
        (FieldPlace::GroundProperty, Choice::Property(property)) => {
            vec![set_field(LineField::GroundProperty(Some(property.clone())))]
        }
        (FieldPlace::RankedBy, Choice::Property(property)) => {
            vec![set_field(LineField::RankedBy(Some(property.clone())))]
        }
        (FieldPlace::Event, Choice::Event(event)) => vec![set_field(LineField::Event(Some(event.clone())))],
        (FieldPlace::SummaryValue, Choice::Property(property)) => match line.summaries.first() {
            Some(summary) => {
                let mut summary = summary.clone();
                summary.value = Slot::Filled(SheetValue::Chip(SheetChip::Their(property.clone())));
                vec![set_field(LineField::Summaries(vec![summary]))]
            }
            None => Vec::new(),
        },
        (FieldPlace::FirstOperand | FieldPlace::SecondOperand, Choice::Value(value)) if phrase.is_empty() => {
            let Some(token) = value_token(value) else {
                return Applied::default();
            };
            return calculate_edit(line_id, line, draft, |draft| {
                if field == FieldPlace::FirstOperand {
                    draft.first = Some(token);
                } else {
                    draft.second = Some(token);
                }
            });
        }
        _ => Vec::new(),
    };
    Applied { edits, draft: None }
}

/// Returns the commands that change one part of the Calculate line `line`, the line `line_id`, by `change`, or the
/// draft it leaves while a value is still missing. `draft` is the line's draft, read while it has no formula.
///
/// A line whose formula is not one operation between two values takes no change.
pub fn calculate_edit(
    line_id: &str,
    line: &SheetLine,
    draft: Option<CalcDraft>,
    change: impl FnOnce(&mut CalcDraft),
) -> Applied {
    let mut current = match line.formula.as_deref().map(binary_formula) {
        Some(Some((first, operator, second))) => CalcDraft {
            first: Some(first),
            operator,
            second: Some(second),
        },
        Some(None) => return Applied::default(),
        None => draft.unwrap_or_default(),
    };
    change(&mut current);
    if let (Some(first), Some(second)) = (&current.first, &current.second) {
        return Applied {
            edits: vec![SheetEdit::SetField {
                line: line_id.to_owned(),
                phrase: String::new(),
                field: LineField::Formula(Some(format!("{first} {} {second}", current.operator))),
            }],
            draft: Some((line_id.to_owned(), None)),
        };
    }
    Applied {
        edits: Vec::new(),
        draft: Some((line_id.to_owned(), Some(current))),
    }
}

/// Returns `value` as a formula writes it: a parameter's id, `l2` for a result, `my.food` for a property of mine, or a
/// number. A percent is written as its decimal.
fn value_token(value: &SheetValue) -> Option<String> {
    match value {
        SheetValue::Chip(SheetChip::Param(id)) => Some(id.clone()),
        SheetValue::Chip(SheetChip::Result {
            line,
            part: None,
            property: None,
        }) => Some(line.clone()),
        SheetValue::Chip(SheetChip::My(id)) => Some(format!("my.{id}")),
        SheetValue::Literal(literal @ (SheetLiteral::Number(_) | SheetLiteral::Percent(_))) => {
            literal.number().and_then(format_number)
        }
        _ => None,
    }
}

// Slots by path

/// Returns the slot of `line` at the slot path `path`, `None` when the line holds no such slot.
pub fn slot_at<'a>(line: &'a SheetLine, path: &str) -> Option<&'a Slot> {
    let (head, rest) = match path.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (path, None),
    };
    let slot = match head {
        "condition" => &line.condition,
        "whose" => &line.whose,
        "only" => &line.only,
        "left" => &line.left,
        "right" => &line.right,
        "low" => &line.low,
        "high" => &line.high,
        "chance" => &line.chance,
        "rate" => &line.rate,
        "tries" => &line.tries,
        "value" => &line.value,
        "tolerance" => &line.tolerance,
        _ => return None,
    };
    match rest {
        None => Some(slot),
        Some(rest) => match slot.value()? {
            SheetValue::Phrase(phrase) => slot_at(phrase, rest),
            _ => None,
        },
    }
}

/// Returns the phrase that holds the last slot of `path` in `line`, the line itself for a slot of its own.
pub fn phrase_of<'a>(line: &'a SheetLine, path: &str) -> Option<&'a SheetLine> {
    match path.rsplit_once('.') {
        None => Some(line),
        Some((outer, _)) => match slot_at(line, outer)?.value()? {
            SheetValue::Phrase(phrase) => Some(phrase),
            _ => None,
        },
    }
}

/// Returns the value at `target` of `line`, `None` for an empty place or a field.
pub fn value_at<'a>(line: &'a SheetLine, target: &PlaceTarget) -> Option<&'a SheetValue> {
    match target {
        PlaceTarget::Slot(path) => slot_at(line, path)?.value(),
        PlaceTarget::ListItem { slot, index } => match slot_at(line, slot)?.value()? {
            SheetValue::List(items) => items.get(*index),
            _ => None,
        },
        _ => None,
    }
}

/// Returns the length of the list in the slot at `path` of `line`, 0 for a slot that holds no list.
pub fn list_len(line: &SheetLine, path: &str) -> usize {
    match slot_at(line, path).and_then(Slot::value) {
        Some(SheetValue::List(items)) => items.len(),
        _ => 0,
    }
}
