//! Edit commands on a sheet and the builder that runs them: one command per edit of the sheet editor, with no user
//! interface.
//!
//! A [`SheetBuilder`] holds a [`SheetDocument`] and the [`KeyStream`] new draws take their keys from.
//! [`SheetBuilder::apply`] runs one [`SheetEdit`] on a copy. A refused command changes nothing. A command checks the
//! structure it changes (ids, names, the line a slot path reaches) and leaves the meaning to [`super::check`].
//!
//! - **Ids.** A new item's id is minted once from its name by [`mint_id`]: lower case, each run of other characters
//!   as one `_`, a leading `x` before a digit, `_2`, `_3` and so on against a clash, and a reserved id refused. A rename
//!   changes the name alone, and [`SheetEdit::ChangeId`] rewrites every reference. A card that runs each tick is
//!   `cell_tick` or `<agent kind id>_tick`, and the card of a button that runs once takes the button's id.
//! - **Lines.** A new line takes the id `l<next_line>`, and `next_line` grows past it. A deleted line's number is never
//!   minted again. A line that draws takes a fresh key, distinct from every key of the sheet, and keeps it for its
//!   life, also while an edit takes its draw away. [`SheetEdit::DrawNewNumbers`] mints a new one.
//! - **Deletes.** A delete never refuses. It clears every reference to what it deletes: a slot that named it turns
//!   blank, a field turns empty, and a table about it, such as a measure of a deleted state, goes with it. A start
//!   that named a deleted state takes the first state its property still holds. Note that the checker reads most
//!   blanks of a card line as a skip of the line, and a blank outside the card lines, such as a start's chance, as an
//!   error. Deleting a When keeps its lines one level up.
//! - **Names.** After every command, each result line that follows its automatic name takes it again, made unique, as
//!   [`automatic_name`] gives it.
//! - **Rates.** A new rate takes the unit of one tick, marked as taken from the tick. A parameter put into a rate whose
//!   unit is empty or marked gives the rate its own unit and clears the mark. A unit chosen by hand clears the mark
//!   too, and gives a parameter with no unit that the rate reads the same unit. [`SheetEdit::ChangeToRate`] and
//!   [`SheetEdit::ChangeToChance`] turn a typed chance into a rate and back, keeping the chance per tick.
//!
//! ```
//! use henad_graph::sheet::ParsedSheet;
//! use henad_graph::sheet::document::ParameterType;
//! use henad_graph::sheet::edit::{SheetBuilder, SheetEdit};
//!
//! let sheet = ParsedSheet::parse(henad_graph::sheet::starters()[0].text).expect("the starter reads");
//! let mut builder = SheetBuilder::new(sheet.document().clone()).with_key_seed(1);
//! let made = builder
//!     .apply(SheetEdit::AddParameter {
//!         name: "Growth rate".to_owned(),
//!         parameter_type: ParameterType::Number,
//!         min: 0.0,
//!         max: 1.0,
//!         default: 0.1,
//!         unit: None,
//!     })
//!     .expect("the parameter is added");
//! assert_eq!(made, ["growth_rate"]);
//! let written = builder.document().to_text().expect("the sheet writes");
//! assert!(written.contains("id = \"growth_rate\""));
//! ```

pub(crate) mod lines;
mod names;
mod references;

pub use names::automatic_name;

use std::collections::BTreeSet;
use std::fmt;

use henad_core::metadata::TimeUnit;

use self::lines::{PathStep, is_block};
use self::references::{Change, Target, all_properties, all_properties_mut, change_references};
use super::check::{RESERVED_IDS, RESERVED_PARAMETER_IDS, RESERVED_PREFIX, holds_draw, is_result_kind};
use super::document::{
    AgentKind, Around, BlockedState, ButtonStep, ButtonStepRule, CardRuns, CellWhich, ColorRule, ColorRuleKind,
    CompareTest, Edges, LineKind, LineMode, LookSummary, MadeOf, Marks, MeasureRule, ParameterType, PickRule,
    PropertyType, SheetButton, SheetCard, SheetChip, SheetDocument, SheetEvent, SheetLine, SheetLiteral, SheetMeasure,
    SheetParameter, SheetProperty, SheetState, SheetTime, SheetValue, Slot, StartRule, StartsAs,
};
use super::limits::{MAX_LINES_PER_CARD, MAX_NESTING, MAX_SHEET_ID_BYTES};
use super::rates::{chance_from_rate, rate_from_chance, shortest_f32, tick_in};
use super::sentence::{ANY_STATE_ID, ConditionItem, rate_unit_word, unit_suggestion};
use crate::ids::is_id;
use crate::keys::KeyStream;
use crate::value::Color;

/// Colours new states take in order, so the defaults carry no meaning: dark grey, blue, orange, purple, yellow, grey,
/// dark green, brown, white, green, red and black.
pub const STATE_COLORS: [Color; 12] = [
    [0x42, 0x42, 0x42, 0xFF],
    [0x00, 0x7A, 0xF5, 0xFF],
    [0xEC, 0x6F, 0x27, 0xFF],
    [0x91, 0x54, 0xFF, 0xFF],
    [0xFF, 0xC1, 0x07, 0xFF],
    [0x80, 0x80, 0x80, 0xFF],
    [0x08, 0x68, 0x2F, 0xFF],
    [0x62, 0x43, 0x09, 0xFF],
    [0xF5, 0xF5, 0xF5, 0xFF],
    [0x14, 0xAE, 0x52, 0xFF],
    [0xE4, 0x37, 0x48, 0xFF],
    [0x13, 0x13, 0x13, 0xFF],
];

/// Colours of the palette a state's colour is chosen from, in the order of [`super::sentence::COLOR_NAMES`].
///
/// They are the colours of [`STATE_COLORS`] in another order: black, dark grey, grey, white, red, orange, yellow,
/// brown, green, dark green, blue and purple.
pub const PALETTE_COLORS: [Color; 12] = [
    STATE_COLORS[11],
    STATE_COLORS[0],
    STATE_COLORS[5],
    STATE_COLORS[8],
    STATE_COLORS[10],
    STATE_COLORS[2],
    STATE_COLORS[4],
    STATE_COLORS[7],
    STATE_COLORS[9],
    STATE_COLORS[6],
    STATE_COLORS[1],
    STATE_COLORS[3],
];

/// Colours a measure that counts no state takes in order: blue, red, green, purple, orange, yellow and grey.
pub const MEASURE_COLORS: [Color; 7] = [
    [0x00, 0x7A, 0xF5, 0xFF],
    [0xE4, 0x37, 0x48, 0xFF],
    [0x14, 0xAE, 0x52, 0xFF],
    [0x91, 0x54, 0xFF, 0xFF],
    [0xEC, 0x6F, 0x27, 0xFF],
    [0xFF, 0xC1, 0x07, 0xFF],
    [0x80, 0x80, 0x80, 0xFF],
];

/// Returns the usual colour of a state named `name`, whole name and case aside, such as red for "Infected", or `None`
/// for a name with no usual colour.
pub fn suggested_state_color(name: &str) -> Option<Color> {
    let name = name.trim().to_lowercase();
    let usual: [(&[&str], Color); 6] = [
        (&["infected", "infectious", "sick", "ill"], STATE_COLORS[10]),
        (&["burning", "fire", "on fire"], STATE_COLORS[2]),
        (
            &["tree", "trees", "forest", "grass", "plant", "plants", "vegetation"],
            STATE_COLORS[9],
        ),
        (&["recovered", "removed", "immune"], STATE_COLORS[5]),
        (&["susceptible", "healthy"], STATE_COLORS[1]),
        (&["dead"], STATE_COLORS[11]),
    ];
    usual
        .iter()
        .find(|(names, _)| names.contains(&name.as_str()))
        .map(|(_, color)| *color)
}

/// A namespace of sheet ids, with the ids reserved in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdSpace {
    /// Properties, parameters, measures and events, which the lowered model holds side by side.
    Item,
    /// Kinds of thing.
    Thing,
    /// Buttons.
    Button,
    /// Cards.
    Card,
    /// The states of one property.
    State,
}

impl IdSpace {
    /// Returns whether `id` is reserved in the namespace.
    pub fn is_reserved(self, id: &str) -> bool {
        if self == Self::State {
            return id == ANY_STATE_ID;
        }
        RESERVED_IDS.contains(&id)
            || references::is_line_id(id)
            || self == Self::Item && (RESERVED_PARAMETER_IDS.contains(&id) || id.starts_with(RESERVED_PREFIX))
            || self == Self::Thing && matches!(id, "cells" | "ground")
    }
}

/// Returns the id minted from `name` in `space`, where `is_taken` returns whether an id is already held.
///
/// The id is `name` in lower case with each run of other characters than `a-z` and `0-9` as one `_`, an `x` before a
/// leading digit, and at most [`MAX_SHEET_ID_BYTES`] bytes. A name with no letter or digit mints `x`. Against a clash
/// it takes `_2`, `_3` and so on.
///
/// ```
/// use henad_graph::sheet::edit::{IdSpace, mint_id};
///
/// assert_eq!(mint_id("Infection rate", IdSpace::Item, |_| false).as_deref(), Ok("infection_rate"));
/// assert_eq!(mint_id("Infection rate", IdSpace::Item, |id| id == "infection_rate").as_deref(), Ok("infection_rate_2"));
/// assert!(mint_id("Grid width", IdSpace::Item, |_| false).is_err());
/// ```
///
/// # Errors
///
/// Returns [`SheetEditError::Reserved`] when the id minted from the name, or a suffixed id tried against a clash, is
/// reserved in `space`. "Henad" mints `henad_2` when `henad` is taken, and `henad_` starts every reserved item id.
pub fn mint_id(name: &str, space: IdSpace, is_taken: impl Fn(&str) -> bool) -> Result<String, SheetEditError> {
    let mut base = String::new();
    let mut gap = false;
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if gap && !base.is_empty() {
                base.push('_');
            }
            gap = false;
            base.push(character);
        } else {
            gap = true;
        }
    }
    if base.is_empty() || base.starts_with(|character: char| character.is_ascii_digit()) {
        base.insert(0, 'x');
    }
    let base = shortened(&base, MAX_SHEET_ID_BYTES);
    if space.is_reserved(&base) {
        return Err(SheetEditError::Reserved(base));
    }
    let mut id = base.clone();
    let mut count = 2_u32;
    while is_taken(&id) {
        let suffix = format!("_{count}");
        id = format!("{}{suffix}", shortened(&base, MAX_SHEET_ID_BYTES - suffix.len()));
        count += 1;
        if space.is_reserved(&id) {
            return Err(SheetEditError::Reserved(id));
        }
    }
    Ok(id)
}

/// Returns `id`, an ASCII id, cut to at most `bytes` bytes with no `_` at its end.
fn shortened(id: &str, bytes: usize) -> String {
    id.get(..bytes.min(id.len()))
        .unwrap_or(id)
        .trim_end_matches('_')
        .to_owned()
}

/// An item of a sheet that a command renames, gives a new id, recolours or deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetItem {
    /// A property of any kind of thing, by id.
    Property(String),
    /// A state of a property.
    State {
        /// Id of the property.
        property: String,
        /// Id of the state.
        state: String,
    },
    /// A parameter, by id.
    Parameter(String),
    /// A measure, by id.
    Measure(String),
    /// An event, by id.
    Event(String),
    /// A button, by id.
    Button(String),
    /// A card, by id.
    Card(String),
    /// A kind of agent, by id.
    AgentKind(String),
}

/// A direction of a move in the outline of a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoveDirection {
    /// One step towards the top of the card.
    Up,
    /// One step towards the bottom of the card.
    Down,
}

/// A field of a line or a phrase that holds no value of a slot, with its new content.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LineField {
    /// The property the line changes or reads. A Become loses a new state and an old state of another property.
    Property(Option<String>),
    /// The new state of a Become, with its property, as `(property, state)`. An old state of another property is
    /// cleared.
    NewState(Option<(String, String)>),
    /// The old state of a Become, a state id or `any_state`.
    StartState(Option<String>),
    /// The test of a comparison. A change between a single value and a list keeps the first value or makes a list of
    /// one.
    Test(Option<CompareTest>),
    /// The neighbourhood of a Count, Look around or Pick a cell line.
    Around(Option<Around>),
    /// The unit of a rate, chosen by hand. The choice clears the mark of a unit taken from the tick.
    ///
    /// With no length of one tick on the sheet, one tick becomes 1 of the unit. A parameter with no unit in the
    /// rate's `rate` slot takes the unit, as in `per day`.
    RateUnit(Option<TimeUnit>),
    /// The formula of a Calculate line or phrase.
    Formula(Option<String>),
    /// The mode of a Steer or Move line.
    Mode(Option<LineMode>),
    /// What a Pick a cell line picks.
    Pick(Option<PickRule>),
    /// The property a Pick a cell line ranks cells by.
    RankedBy(Option<String>),
    /// The cell a Look at a cell line looks at.
    Which(Option<CellWhich>),
    /// The ground property a Leave line marks.
    GroundProperty(Option<String>),
    /// The event a Record line records.
    Event(Option<String>),
    /// The kind of agent an agents-within neighbourhood looks at.
    AgentKind(Option<String>),
    /// Whether an agents-within neighbourhood counts the agent itself.
    IncludeMe(Option<bool>),
    /// Whether a Pick a cell line picks only open cells.
    OpenOnly(Option<bool>),
    /// The summaries of a Look around line.
    Summaries(Vec<LookSummary>),
}

/// One edit of a sheet.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SheetEdit {
    /// Sets the model's name.
    SetSheetName {
        /// New name.
        name: String,
    },
    /// Sets or clears what the model is for.
    SetPurpose {
        /// New purpose, `None` for none.
        purpose: Option<String>,
    },
    /// Sets the size of the world and what happens at its edges.
    SetWorld {
        /// Width in cells.
        width: u32,
        /// Height in cells.
        height: u32,
        /// What happens at the edges.
        edges: Edges,
    },
    /// Sets or clears the length of one tick.
    SetTime {
        /// New length of one tick, `None` for a tick with no unit.
        time: Option<SheetTime>,
    },
    /// Adds a property to a kind of thing, with an id minted from its name, and returns the id.
    ///
    /// The first States property of a kind of thing with no colour rule colours it by state.
    AddProperty {
        /// Kind of thing: `cells`, `ground` or an agent kind's id.
        thing: String,
        /// Name of the property.
        name: String,
        /// Type of the property's value.
        property_type: PropertyType,
    },
    /// Gives a property another type. Every reference to the property and its states is cleared, and the property
    /// loses its states and its start.
    ///
    /// A property that becomes a States property colours its kind of thing by state while the thing has no colour
    /// rule. A type the property already has changes nothing.
    SetPropertyType {
        /// Id of the property.
        property: String,
        /// New type of the property's value.
        property_type: PropertyType,
    },
    /// Adds a state to a States property, with an id minted from its name, and returns the id.
    AddState {
        /// Id of the property.
        property: String,
        /// Name of the state.
        name: String,
        /// Colour of the state, `None` for the next of [`STATE_COLORS`] after the property's states.
        color: Option<Color>,
    },
    /// Adds a parameter, with an id minted from its name, and returns the id.
    AddParameter {
        /// Name of the parameter.
        name: String,
        /// Type of the parameter's value.
        parameter_type: ParameterType,
        /// Smallest value of the slider.
        min: f64,
        /// Largest value of the slider.
        max: f64,
        /// Value the model keeps.
        default: f64,
        /// Unit of the value, free text such as `per day`.
        unit: Option<String>,
    },
    /// Sets the values of a parameter's row on the sheet, as one edit.
    SetParameter {
        /// Id of the parameter.
        parameter: String,
        /// Smallest value of the slider.
        min: f64,
        /// Largest value of the slider.
        max: f64,
        /// Value the model keeps.
        default: f64,
        /// Step of the slider.
        step: Option<f64>,
        /// Unit of the value.
        unit: Option<String>,
        /// What the parameter means.
        means: Option<String>,
    },
    /// Adds a measure, with an id minted from its name, and returns the id.
    AddMeasure {
        /// Name of the measure, which is also its column in results.
        name: String,
        /// What the measure counts or sums.
        rule: MeasureRule,
        /// Kind of thing the measure is over.
        over: Option<String>,
        /// Property a count in a state reads.
        property: Option<String>,
        /// State a count in a state counts.
        state: Option<String>,
        /// Event an event count counts.
        event: Option<String>,
        /// Value an average, a total or a spread reads.
        value: Slot,
        /// Colour of the plot, `None` for the state's colour of a count in a state, or the next of
        /// [`MEASURE_COLORS`].
        color: Option<Color>,
    },
    /// Adds an event, with an id minted from its name, and returns the id.
    AddEvent {
        /// Name of the event.
        name: String,
    },
    /// Adds a button, with an id minted from its name, and returns the id.
    ///
    /// A button that starts again starts every property of `thing` again. A button that runs once gets a card of its
    /// own over `thing`, with the button's id.
    AddButton {
        /// Name of the button.
        name: String,
        /// What the button does: [`ButtonStepRule::StartAgain`] or [`ButtonStepRule::RunOnce`].
        rule: ButtonStepRule,
        /// Kind of thing the button acts on: `cells` or an agent kind's id.
        thing: String,
    },
    /// Replaces the steps of a button.
    SetButtonSteps {
        /// Id of the button.
        button: String,
        /// New steps, in order. A step that runs a card names a card of the sheet.
        does: Vec<ButtonStep>,
    },
    /// Adds the card a kind of thing runs each tick, `cell_tick` or `<agent kind id>_tick`, and returns its id.
    AddCard {
        /// Kind of thing: `cells` or an agent kind's id.
        every: String,
    },
    /// Renames an item. Every reference keeps the id.
    ///
    /// A result line whose name follows its automatic name takes the new words, as a count of a renamed state does.
    Rename {
        /// The item.
        item: SheetItem,
        /// New name.
        name: String,
    },
    /// Gives an item a new id and rewrites every reference to it.
    ///
    /// A card that holds a button's id and that the button runs takes the new id too, and so does the tick card of a
    /// kind of agent.
    ChangeId {
        /// The item.
        item: SheetItem,
        /// New id.
        id: String,
    },
    /// Recolours a state or a measure. A measure drawn in a state's colour follows the state.
    Recolor {
        /// The state or the measure.
        item: SheetItem,
        /// New colour.
        color: Color,
    },
    /// Deletes an item and clears every reference to it. A line that used it does not run.
    ///
    /// A deleted property takes its measures and its colour rule with it, a deleted state its measures and its start
    /// rows, and a deleted button the cards it alone runs.
    Delete {
        /// The item.
        item: SheetItem,
    },
    /// Sets how a property starts, as one edit. A start that draws keeps the property's key or takes a fresh one.
    SetStartsAs {
        /// Id of the property.
        property: String,
        /// New start, `None` for none.
        starts_as: Option<Box<StartsAs>>,
    },
    /// Sets or clears the colour rule of a kind of thing.
    SetColorRule {
        /// Kind of thing: `cells`, `ground` or an agent kind's id.
        thing: String,
        /// New rule, `None` for none.
        rule: Option<ColorRule>,
    },
    /// Sets the number of agents of a kind at the start and the largest number the world holds.
    SetAgentKind {
        /// Id of the kind of agent.
        kind: String,
        /// Number of agents at the start, from 1 to `max_count`.
        count: u32,
        /// Largest number of agents the world holds.
        max_count: u32,
    },
    /// Sets how agents' marks on a Number property of the ground combine, and the share of it left after a tick's
    /// fading.
    ///
    /// No marks rule takes the fading away too, since a property that fades names its marks rule.
    SetMarks {
        /// Id of the Number property of the ground.
        property: String,
        /// How marks combine, `None` for a property that takes no marks.
        marks: Option<Marks>,
        /// Share left after a tick's fading, blank for a property that does not fade.
        kept_per_tick: Slot,
    },
    /// Lets agents enter the cells of a state of a ground property, or keeps them out.
    ///
    /// A state blocked twice is held once, and a state let in that was not blocked changes nothing.
    SetBlocked {
        /// Id of the States property of the ground.
        property: String,
        /// Id of the state.
        state: String,
        /// Whether agents are kept out of the state's cells.
        blocked: bool,
    },
    /// Adds a line at the end of a card or of a When or Repeat line, and returns its id.
    ///
    /// The line takes the next line id, and a key when it draws: its own when no other draw holds it, or a fresh one.
    /// Inside a When, a change with no condition starts as always, and a Become with no old state takes the state the
    /// nearest When tests as "my X is S". A rate with no unit takes the unit of one tick, marked.
    AddLine {
        /// Id of the card.
        card: String,
        /// Id of the When or Repeat line to add inside, `None` for the end of the card.
        inside: Option<String>,
        /// The new line. Its lines, if any, are dropped.
        line: Box<SheetLine>,
    },
    /// Puts a value in a slot of a line or of a phrase inside it, or leaves the slot blank.
    ///
    /// A rate put in with no unit takes the unit of one tick, marked. A parameter put into a rate whose unit is empty
    /// or marked gives the rate the first unit its own unit names ("per day per infected neighbour" names day) and
    /// clears the mark.
    SetSlot {
        /// Id of the line.
        line: String,
        /// Slot path, such as `condition.rate` or `whose.right`.
        path: String,
        /// New value, [`Slot::Blank`] to empty the slot.
        value: Slot,
    },
    /// Sets a field of a line or of a phrase inside it.
    SetField {
        /// Id of the line.
        line: String,
        /// Slot path of the phrase that holds the field, empty for the line itself.
        phrase: String,
        /// The field and its new content.
        field: LineField,
    },
    /// Chooses the form of the condition of a change or a When, from "Choose when".
    ///
    /// A form the condition already has keeps its values, and "is", "is at least" and "is at most" on a comparison set
    /// its test. A When cannot be always.
    ChooseCondition {
        /// Id of the line.
        line: String,
        /// The form.
        item: ConditionItem,
    },
    /// Replaces a condition "is less than ... or more than ..." with one side filled by the plain comparison it
    /// means: "is less than" the lower number or "is more than" the upper number.
    UseOneSide {
        /// Id of the line.
        line: String,
    },
    /// Moves the count that a chance or rate holds as its value into "for each", and empties the chance or rate.
    UseCountForEach {
        /// Id of the line, a chance or rate line or a change whose condition is one.
        line: String,
    },
    /// Copies the condition of a When into the one change inside it, and removes the When.
    ///
    /// The change's own condition is blank or always. Any other condition refuses the edit.
    MoveConditionInto {
        /// Id of the When line.
        when: String,
    },
    /// Renames the result of a line. The name no longer follows the automatic name, until it equals it again.
    RenameResult {
        /// Id of the line.
        line: String,
        /// New name, unique among the sheet's result names, case aside.
        name: String,
    },
    /// Sets or clears the author's note under a line.
    SetNote {
        /// Id of the line.
        line: String,
        /// New note, `None` for none.
        note: Option<String>,
    },
    /// Moves a line, with the lines inside it, one step up or down the outline of its card.
    MoveLine {
        /// Id of the line.
        line: String,
        /// Direction of the move.
        direction: MoveDirection,
    },
    /// Deletes a line. A When or Repeat leaves its lines one level up, and a slot that used its result turns blank.
    DeleteLine {
        /// Id of the line.
        line: String,
    },
    /// Gives a line that draws a fresh key, so it draws new numbers.
    DrawNewNumbers {
        /// Id of the line.
        line: String,
    },
    /// Turns the chance of a line into the rate that gives the same chance per tick, `-ln(1 - p) / dt` per the tick's
    /// unit.
    ///
    /// The line is a chance line or a change whose condition is a chance, and the chance is a typed number below 1.
    /// The rate takes the tick's unit, marked as taken from the tick, keeps the line's "for each" and its key, and is
    /// written as the shortest number that reads back as the same `f32`. The sheet's tick needs a length.
    ChangeToRate {
        /// Id of the line.
        line: String,
    },
    /// Turns the rate of a line into the chance per tick it gives, `1 - e^(-r dt)` with `dt` one tick in the rate's
    /// unit.
    ///
    /// The line is a rate line or a change whose condition is a rate, and the rate is a typed number with a unit. The
    /// chance keeps the line's "for each" and its key, and is written as the shortest number that reads back as the
    /// same `f32`. The sheet's tick needs a length.
    ChangeToChance {
        /// Id of the line.
        line: String,
    },
}

/// Reason a sheet edit is refused. A refused edit changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetEditError {
    /// The edit names something the sheet does not hold.
    Missing(String),
    /// The edit breaks a rule of the sheet's structure, such as an empty name.
    Invalid(String),
    /// The name or id is already held by another item of the same kind.
    Taken(String),
    /// The id, minted from a name or given, is reserved.
    Reserved(String),
}

impl fmt::Display for SheetEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(message) | Self::Invalid(message) | Self::Taken(message) => f.write_str(message),
            Self::Reserved(id) => write!(f, "the id '{id}' is reserved"),
        }
    }
}

impl std::error::Error for SheetEditError {}

/// A sheet under edit: its document and the stream of keys new draws take.
#[derive(Clone)]
pub struct SheetBuilder {
    /// The sheet.
    document: SheetDocument,
    /// Stream new keys come from.
    keys: KeyStream,
}

/// Prints the sheet's id and the number of its cards.
impl fmt::Debug for SheetBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SheetBuilder")
            .field("id", &self.document.sheet.id)
            .field("cards", &self.document.cards.len())
            .finish_non_exhaustive()
    }
}

impl SheetBuilder {
    /// Returns a builder of `document`, with keys seeded from entropy.
    pub fn new(document: SheetDocument) -> Self {
        Self {
            document,
            keys: KeyStream::from_entropy(),
        }
    }

    /// Returns the builder with its keys drawn from `seed`, so a test mints the same keys every run.
    pub fn with_key_seed(mut self, seed: u64) -> Self {
        self.keys = KeyStream::seeded(seed);
        self
    }

    /// Sheet under edit.
    pub fn document(&self) -> &SheetDocument {
        &self.document
    }

    /// Returns the sheet under edit.
    pub fn into_document(self) -> SheetDocument {
        self.document
    }

    /// Runs `edit` and returns the ids of the items it made, empty for an edit that makes none.
    ///
    /// # Errors
    ///
    /// Returns the reason the edit is refused. On an error nothing changes.
    pub fn apply(&mut self, edit: SheetEdit) -> Result<Vec<String>, SheetEditError> {
        let mut draft = Draft {
            document: self.document.clone(),
            keys: self.keys.clone(),
            touched: Vec::new(),
            touched_starts: Vec::new(),
        };
        let made = draft.run(edit)?;
        draft.finish();
        names::update_names(&self.document, &mut draft.document);
        self.document = draft.document;
        self.keys = draft.keys;
        Ok(made)
    }
}

/// A copy of the sheet that one edit changes, with the lines it touched.
struct Draft {
    /// The sheet.
    document: SheetDocument,
    /// Stream new keys come from.
    keys: KeyStream,
    /// Ids of the lines the edit added or changed, whose keys are checked when it ends.
    touched: Vec<String>,
    /// Ids of the properties whose starts the edit set, whose keys are checked when it ends.
    touched_starts: Vec<String>,
}

/// Returns an error for a missing `what` named `id`.
fn missing(what: &str, id: &str) -> SheetEditError {
    SheetEditError::Missing(format!("the sheet has no {what} '{id}'"))
}

/// Returns `name` trimmed, or an error naming `what` for an empty name.
fn clean_name(name: &str, what: &str) -> Result<String, SheetEditError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SheetEditError::Invalid(format!("a {what} needs a name")));
    }
    Ok(name.to_owned())
}

/// Returns an error when `name` equals one of `names`, case aside.
fn refuse_taken<'a>(name: &str, mut names: impl Iterator<Item = &'a str>, what: &str) -> Result<(), SheetEditError> {
    let lower = name.to_lowercase();
    if names.any(|other| other.to_lowercase() == lower) {
        return Err(SheetEditError::Taken(format!(
            "a {what} called '{name}' already exists"
        )));
    }
    Ok(())
}

/// Returns an error for a number that is not finite.
fn refuse_infinite(numbers: &[f64]) -> Result<(), SheetEditError> {
    if numbers.iter().all(|number| number.is_finite()) {
        Ok(())
    } else {
        Err(SheetEditError::Invalid("every number of a sheet is finite".to_owned()))
    }
}

/// Returns the first unit of time that `text` names as "per second" to "per year", `None` when it names none.
///
/// Case is ignored, one or more spaces may follow "per", the unit may take a plural "s", and both ends fall on a word
/// boundary. "Per days" names day, and "per daylight" names none.
pub(crate) fn unit_in_text(text: &str) -> Option<TimeUnit> {
    let is_word = |character: char| character.is_ascii_alphanumeric() || character == '_';
    let lower = text.to_ascii_lowercase();
    let boundary_before = |at: usize| !lower[..at].chars().next_back().is_some_and(is_word);
    lower.match_indices("per").find_map(|(at, _)| {
        if !boundary_before(at) {
            return None;
        }
        let after = &lower[at + "per".len()..];
        let rest = after.trim_start_matches(char::is_whitespace);
        if rest.len() == after.len() {
            return None;
        }
        TimeUnit::ALL.into_iter().find(|unit| {
            rest.strip_prefix(rate_unit_word(*unit)).is_some_and(|tail| {
                let tail = tail.strip_prefix('s').unwrap_or(tail);
                !tail.chars().next().is_some_and(is_word)
            })
        })
    })
}

/// Returns the id of the parameter in the `rate` slot of `line`, when `line` is a rate.
fn rate_parameter(line: &SheetLine) -> Option<String> {
    match line.rate.value() {
        Some(SheetValue::Chip(SheetChip::Param(parameter))) if line.kind == LineKind::Rate => Some(parameter.clone()),
        _ => None,
    }
}

/// Returns whether a start of `starts` draws random numbers.
fn starts_draws(starts: &StartsAs) -> bool {
    matches!(starts.rule, StartRule::ByChance | StartRule::Anywhere)
        || matches!(starts.value.value(), Some(SheetValue::Phrase(phrase)) if holds_draw(phrase))
}

/// Gives every rate phrase in `value` with no unit the unit of one tick, marked as taken from the tick.
fn rate_from_tick(value: &mut SheetValue, tick: Option<TimeUnit>) {
    match value {
        SheetValue::Phrase(phrase) => line_rates_from_tick(phrase, tick),
        SheetValue::List(items) => {
            for item in items {
                rate_from_tick(item, tick);
            }
        }
        _ => {}
    }
}

/// Gives `line`, when it is a rate with no unit, and every rate phrase in its slots, the unit of one tick, marked.
fn line_rates_from_tick(line: &mut SheetLine, tick: Option<TimeUnit>) {
    let Some(tick) = tick else {
        return;
    };
    if line.kind == LineKind::Rate && line.rate_unit.is_none() {
        line.rate_unit = Some(tick);
        line.rate_unit_from_tick = true;
    }
    for slot in [
        &mut line.chance,
        &mut line.rate,
        &mut line.left,
        &mut line.right,
        &mut line.value,
        &mut line.condition,
        &mut line.whose,
        &mut line.only,
    ] {
        if let Slot::Filled(value) = slot {
            rate_from_tick(value, Some(tick));
        }
    }
    for item in &mut line.items {
        rate_from_tick(item, Some(tick));
    }
}

/// Depth of a line table in a sheet's text: the cards, the card, `line` and the line's id.
const LINE_TABLE_DEPTH: usize = 4;

impl Draft {
    /// Runs one edit on the draft.
    #[expect(clippy::too_many_lines, reason = "one arm per edit reads best as one table")]
    fn run(&mut self, edit: SheetEdit) -> Result<Vec<String>, SheetEditError> {
        match edit {
            SheetEdit::SetSheetName { name } => {
                self.document.sheet.name = clean_name(&name, "model")?;
            }
            SheetEdit::SetPurpose { purpose } => {
                self.document.sheet.purpose = purpose.filter(|purpose| !purpose.trim().is_empty());
            }
            SheetEdit::SetWorld { width, height, edges } => {
                if width == 0 || height == 0 {
                    return Err(SheetEditError::Invalid(
                        "the world is at least 1 cell wide and 1 cell high".to_owned(),
                    ));
                }
                self.document.world.width = width;
                self.document.world.height = height;
                self.document.world.edges = edges;
            }
            SheetEdit::SetTime { time } => {
                if let Some(time) = time
                    && !(time.tick_length.is_finite() && time.tick_length > 0.0)
                {
                    return Err(SheetEditError::Invalid(
                        "one tick lasts a number of units above 0".to_owned(),
                    ));
                }
                self.document.time = time;
            }
            SheetEdit::AddProperty {
                thing,
                name,
                property_type,
            } => return self.add_property(&thing, &name, property_type),
            SheetEdit::SetPropertyType {
                property,
                property_type,
            } => self.set_property_type(&property, property_type)?,
            SheetEdit::AddState { property, name, color } => return self.add_state(&property, &name, color),
            SheetEdit::AddParameter {
                name,
                parameter_type,
                min,
                max,
                default,
                unit,
            } => {
                refuse_infinite(&[min, max, default])?;
                let name = clean_name(&name, "parameter")?;
                refuse_taken(
                    &name,
                    self.document.parameters.iter().map(|parameter| parameter.name.as_str()),
                    "parameter",
                )?;
                let id = self.mint_item(&name)?;
                let mut parameter = SheetParameter::new(id.clone(), name, parameter_type, min, max, default);
                parameter.unit = unit.filter(|unit| !unit.trim().is_empty());
                self.document.parameters.push(parameter);
                return Ok(vec![id]);
            }
            SheetEdit::SetParameter {
                parameter,
                min,
                max,
                default,
                step,
                unit,
                means,
            } => {
                refuse_infinite(&[min, max, default, step.unwrap_or(0.0)])?;
                let parameter = self
                    .document
                    .parameters
                    .iter_mut()
                    .find(|known| known.id == parameter)
                    .ok_or_else(|| missing("parameter", &parameter))?;
                parameter.min = min;
                parameter.max = max;
                parameter.default = default;
                parameter.step = step;
                parameter.unit = unit.filter(|unit| !unit.trim().is_empty());
                parameter.means = means.filter(|means| !means.trim().is_empty());
            }
            SheetEdit::AddMeasure {
                name,
                rule,
                over,
                property,
                state,
                event,
                value,
                color,
            } => {
                let name = clean_name(&name, "measure")?;
                refuse_taken(
                    &name,
                    self.document.measures.iter().map(|measure| measure.name.as_str()),
                    "measure",
                )?;
                let state_color = match (&property, &state) {
                    (Some(property), Some(state)) if rule == MeasureRule::CountInState => self
                        .property(property)
                        .and_then(|property| property.states.iter().find(|known| known.id == *state))
                        .map(|state| state.color),
                    _ => None,
                };
                let color = color
                    .or(state_color)
                    .unwrap_or_else(|| MEASURE_COLORS[self.document.measures.len() % MEASURE_COLORS.len()]);
                let id = self.mint_item(&name)?;
                let mut measure = SheetMeasure::new(id.clone(), name, color, rule);
                measure.over = over;
                measure.property = property;
                measure.state = state;
                measure.event = event;
                measure.value = value;
                self.document.measures.push(measure);
                return Ok(vec![id]);
            }
            SheetEdit::AddEvent { name } => {
                let name = clean_name(&name, "event")?;
                refuse_taken(
                    &name,
                    self.document.events.iter().map(|event| event.name.as_str()),
                    "event",
                )?;
                let id = self.mint_item(&name)?;
                self.document.events.push(SheetEvent::new(id.clone(), name));
                return Ok(vec![id]);
            }
            SheetEdit::AddButton { name, rule, thing } => return self.add_button(&name, rule, &thing),
            SheetEdit::SetButtonSteps { button, does } => {
                for step in &does {
                    if let Some(card) = &step.card
                        && !self.document.cards.iter().any(|known| known.id == *card)
                    {
                        return Err(missing("card", card));
                    }
                }
                self.document
                    .buttons
                    .iter_mut()
                    .find(|known| known.id == button)
                    .ok_or_else(|| missing("button", &button))?
                    .does = does;
            }
            SheetEdit::AddCard { every } => return self.add_card(&every),
            SheetEdit::Rename { item, name } => self.rename(&item, &name)?,
            SheetEdit::ChangeId { item, id } => self.change_id(&item, &id)?,
            SheetEdit::Recolor { item, color } => self.recolor(&item, color)?,
            SheetEdit::Delete { item } => {
                self.delete(&item)?;
                // A ground with nothing left on it goes, as it came with its first property.
                if self
                    .document
                    .ground
                    .as_ref()
                    .is_some_and(|ground| ground.properties.is_empty() && ground.blocked.is_empty())
                {
                    self.document.ground = None;
                }
            }
            SheetEdit::SetStartsAs { property, starts_as } => {
                self.touched_starts.push(property.clone());
                let property = self.property_mut(&property)?;
                let old_key = property.starts_as.as_ref().and_then(|starts| starts.key);
                property.starts_as = starts_as.map(|starts| {
                    let mut starts = *starts;
                    starts.key = starts.key.or(old_key);
                    starts
                });
            }
            SheetEdit::SetColorRule { thing, rule } => {
                self.require_thing(&thing)?;
                match rule {
                    Some(rule) => {
                        self.document.colors.insert(thing, rule);
                    }
                    None => {
                        self.document.colors.remove(&thing);
                    }
                }
            }
            SheetEdit::SetAgentKind { kind, count, max_count } => {
                if count == 0 || count > max_count {
                    return Err(SheetEditError::Invalid(format!(
                        "the agents '{kind}' need a count from 1 to the largest number the world holds"
                    )));
                }
                let kind = self.agent_kind_mut(&kind)?;
                kind.count = count;
                kind.max_count = max_count;
            }
            SheetEdit::SetMarks {
                property,
                marks,
                kept_per_tick,
            } => self.set_marks(&property, marks, kept_per_tick)?,
            SheetEdit::SetBlocked {
                property,
                state,
                blocked,
            } => self.set_blocked(&property, &state, blocked)?,
            SheetEdit::AddLine { card, inside, line } => return self.add_line(&card, inside.as_deref(), *line),
            SheetEdit::SetSlot { line, path, value } => self.set_slot(&line, &path, value)?,
            SheetEdit::SetField { line, phrase, field } => self.set_field(&line, &phrase, field)?,
            SheetEdit::ChooseCondition { line, item } => self.choose_condition(&line, item)?,
            SheetEdit::UseOneSide { line } => self.use_one_side(&line)?,
            SheetEdit::UseCountForEach { line } => self.use_count_for_each(&line)?,
            SheetEdit::MoveConditionInto { when } => self.move_condition_into(&when)?,
            SheetEdit::RenameResult { line, name } => {
                let name = clean_name(&name, "result")?;
                let others = self
                    .document
                    .cards
                    .iter()
                    .flat_map(|card| &card.line_tables)
                    .filter(|(id, other)| **id != line && is_result_kind(other.kind))
                    .filter_map(|(_, other)| other.name.as_deref());
                refuse_taken(&name, others, "result")?;
                let target = self.line_mut(&line)?;
                if !is_result_kind(target.kind) {
                    return Err(SheetEditError::Invalid(format!("line '{line}' works out no result")));
                }
                target.name = Some(name);
            }
            SheetEdit::SetNote { line, note } => {
                self.line_mut(&line)?.note = note.filter(|note| !note.trim().is_empty());
            }
            SheetEdit::MoveLine { line, direction } => {
                let card = self.card_of_line(&line)?;
                lines::move_line(&mut self.document.cards[card], &line, direction)?;
            }
            SheetEdit::DeleteLine { line } => self.delete_line(&line)?,
            SheetEdit::ChangeToRate { line } => self.change_draw(&line, LineKind::Rate)?,
            SheetEdit::ChangeToChance { line } => self.change_draw(&line, LineKind::Chance)?,
            SheetEdit::DrawNewNumbers { line } => {
                let target = self.line(&line)?;
                if !holds_draw(target) {
                    return Err(SheetEditError::Invalid(format!("line '{line}' draws no numbers")));
                }
                let key = self.mint_key();
                self.line_mut(&line)?.key = Some(key);
            }
        }
        Ok(Vec::new())
    }

    /// Gives every touched line that draws a key no other draw holds, and every touched start that draws or shares its
    /// key one too.
    fn finish(&mut self) {
        let touched = std::mem::take(&mut self.touched);
        for id in touched {
            let Some((card, line)) = self.document.line(&id) else {
                continue;
            };
            if !holds_draw(line) {
                continue;
            }
            let shared = line.key.is_some_and(|key| self.key_count(key) > 1);
            if line.key.is_none() || shared {
                let key = self.mint_key();
                if let Some(line) = self.document.cards[card].line_tables.get_mut(&id) {
                    line.key = Some(key);
                }
            }
        }
        let touched_starts = std::mem::take(&mut self.touched_starts);
        let rekeyed: Vec<String> = all_properties(&self.document)
            .filter(|property| {
                touched_starts.contains(&property.id)
                    && property.starts_as.as_ref().is_some_and(|starts| match starts.key {
                        None => starts_draws(starts),
                        Some(key) => self.key_count(key) > 1,
                    })
            })
            .map(|property| property.id.clone())
            .collect();
        for id in rekeyed {
            let key = self.mint_key();
            if let Ok(property) = self.property_mut(&id)
                && let Some(starts) = property.starts_as.as_mut()
            {
                starts.key = Some(key);
            }
        }
    }

    /// Returns every key of the sheet: those of the lines and those of the starts.
    fn keys_held(&self) -> Vec<u32> {
        self.document
            .cards
            .iter()
            .flat_map(|card| card.line_tables.values())
            .filter_map(|line| line.key)
            .chain(all_properties(&self.document).filter_map(|property| property.starts_as.as_ref()?.key))
            .collect()
    }

    /// Returns the number of draws that hold `key`.
    fn key_count(&self, key: u32) -> usize {
        self.keys_held().into_iter().filter(|held| *held == key).count()
    }

    /// Returns a fresh key, distinct from every key of the sheet.
    fn mint_key(&mut self) -> u32 {
        let taken: BTreeSet<u32> = self.keys_held().into_iter().collect();
        self.keys.next_key_not_in(|key| taken.contains(&key))
    }

    /// Returns every id of the item namespace: properties, parameters, measures and events.
    fn item_ids(&self) -> BTreeSet<String> {
        all_properties(&self.document)
            .map(|property| property.id.clone())
            .chain(self.document.parameters.iter().map(|parameter| parameter.id.clone()))
            .chain(self.document.measures.iter().map(|measure| measure.id.clone()))
            .chain(self.document.events.iter().map(|event| event.id.clone()))
            .collect()
    }

    /// Returns an id minted from `name` in the item namespace.
    fn mint_item(&self, name: &str) -> Result<String, SheetEditError> {
        let taken = self.item_ids();
        mint_id(name, IdSpace::Item, |id| taken.contains(id))
    }

    /// Returns the property `id` of any kind of thing.
    fn property(&self, id: &str) -> Option<&SheetProperty> {
        all_properties(&self.document).find(|property| property.id == id)
    }

    /// Returns the property `id` of any kind of thing, mutable.
    fn property_mut(&mut self, id: &str) -> Result<&mut SheetProperty, SheetEditError> {
        all_properties_mut(&mut self.document)
            .find(|property| property.id == id)
            .ok_or_else(|| missing("property", id))
    }

    /// Returns an error unless `thing` is a kind of thing of the sheet's world.
    fn require_thing(&self, thing: &str) -> Result<(), SheetEditError> {
        let known = match thing {
            "cells" => self.document.world.made_of == MadeOf::Cells,
            "ground" => self.document.world.made_of == MadeOf::Agents,
            _ => self.document.agents.iter().any(|kind| kind.id == thing),
        };
        if known {
            Ok(())
        } else {
            Err(missing("kind of thing", thing))
        }
    }

    /// Returns the properties of the kind of thing `thing`, made empty when the sheet writes none yet.
    fn thing_properties_mut(&mut self, thing: &str) -> Result<&mut Vec<SheetProperty>, SheetEditError> {
        self.require_thing(thing)?;
        match thing {
            "cells" => Ok(self.document.cells.get_or_insert_with(Vec::new)),
            "ground" => Ok(&mut self.document.ground.get_or_insert_with(Default::default).properties),
            _ => self
                .document
                .agents
                .iter_mut()
                .find(|kind| kind.id == thing)
                .map(|kind| &mut kind.properties)
                .ok_or_else(|| missing("kind of thing", thing)),
        }
    }

    /// Returns the index of the card that holds line `line_id`.
    fn card_of_line(&self, line_id: &str) -> Result<usize, SheetEditError> {
        self.document
            .line(line_id)
            .map(|(card, _)| card)
            .ok_or_else(|| missing("line", line_id))
    }

    /// Returns line `line_id`.
    fn line(&self, line_id: &str) -> Result<&SheetLine, SheetEditError> {
        self.document
            .line(line_id)
            .map(|(_, line)| line)
            .ok_or_else(|| missing("line", line_id))
    }

    /// Returns line `line_id`, mutable, and records it as touched.
    fn line_mut(&mut self, line_id: &str) -> Result<&mut SheetLine, SheetEditError> {
        let card = self.card_of_line(line_id)?;
        self.touched.push(line_id.to_owned());
        self.document.cards[card]
            .line_tables
            .get_mut(line_id)
            .ok_or_else(|| missing("line", line_id))
    }

    /// Returns an error when the tables and lists of line `line_id` nest past what a sheet reads.
    fn refuse_deep(&self, line_id: &str) -> Result<(), SheetEditError> {
        let line = self.line(line_id)?;
        if LINE_TABLE_DEPTH + lines::line_depth(line) > MAX_NESTING {
            return Err(SheetEditError::Invalid(format!(
                "line '{line_id}' would nest tables and lists past {MAX_NESTING}"
            )));
        }
        Ok(())
    }

    /// Adds a property named `name` of `property_type` to `thing`.
    fn add_property(
        &mut self,
        thing: &str,
        name: &str,
        property_type: PropertyType,
    ) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "property")?;
        let id = self.mint_item(&name)?;
        let properties = self.thing_properties_mut(thing)?;
        refuse_taken(
            &name,
            properties.iter().map(|property| property.name.as_str()),
            "property",
        )?;
        properties.push(SheetProperty::new(id.clone(), name, property_type));
        if property_type == PropertyType::States && !self.document.colors.contains_key(thing) {
            let mut rule = ColorRule::new(ColorRuleKind::ByState);
            rule.property = Some(id.clone());
            self.document.colors.insert(thing.to_owned(), rule);
        }
        Ok(vec![id])
    }

    /// Gives the property `id` the type `property_type`, clearing every reference to it and to its states.
    fn set_property_type(&mut self, id: &str, property_type: PropertyType) -> Result<(), SheetEditError> {
        let property = self.property(id).ok_or_else(|| missing("property", id))?;
        if property.property_type == property_type {
            return Ok(());
        }
        // The states go first, so every chip of a state of the property is cleared with them.
        let states: Vec<String> = property.states.iter().map(|state| state.id.clone()).collect();
        for state in states {
            change_references(
                &mut self.document,
                &Target::State {
                    property: id.to_owned(),
                    state,
                },
                Change::Clear,
            );
        }
        change_references(&mut self.document, &Target::Property(id.to_owned()), Change::Clear);
        // A colour rule over the property no longer fits its type.
        self.document
            .colors
            .retain(|_, rule| rule.property.as_deref() != Some(id));
        let thing = self.thing_of_property(id);
        let property = self.property_mut(id)?;
        property.property_type = property_type;
        property.states.clear();
        property.starts_as = None;
        property.marks = None;
        if property_type == PropertyType::States
            && let Some(thing) = thing
            && !self.document.colors.contains_key(&thing)
        {
            let mut rule = ColorRule::new(ColorRuleKind::ByState);
            rule.property = Some(id.to_owned());
            self.document.colors.insert(thing, rule);
        }
        Ok(())
    }

    /// Adds a state named `name` to the States property `property`.
    fn add_state(&mut self, property: &str, name: &str, color: Option<Color>) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "state")?;
        let property = self.property_mut(property)?;
        if property.property_type != PropertyType::States {
            return Err(SheetEditError::Invalid(format!(
                "'{}' is no States property",
                property.id
            )));
        }
        refuse_taken(&name, property.states.iter().map(|state| state.name.as_str()), "state")?;
        let id = mint_id(&name, IdSpace::State, |id| {
            property.states.iter().any(|state| state.id == id)
        })?;
        let color = color.unwrap_or(STATE_COLORS[property.states.len() % STATE_COLORS.len()]);
        property.states.push(SheetState::new(id.clone(), name, color));
        Ok(vec![id])
    }

    /// Adds a button named `name` that does `rule` to `thing`.
    fn add_button(&mut self, name: &str, rule: ButtonStepRule, thing: &str) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "button")?;
        self.require_thing(thing)?;
        refuse_taken(
            &name,
            self.document.buttons.iter().map(|button| button.name.as_str()),
            "button",
        )?;
        let document = &self.document;
        let id = mint_id(&name, IdSpace::Button, |id| {
            document.buttons.iter().any(|button| button.id == id)
                || rule == ButtonStepRule::RunOnce && document.cards.iter().any(|card| card.id == id)
        })?;
        let mut button = SheetButton::new(id.clone(), name);
        let mut step = ButtonStep::new(rule);
        match rule {
            ButtonStepRule::StartAgain => step.thing = Some(thing.to_owned()),
            ButtonStepRule::RunOnce => {
                step.card = Some(id.clone());
                self.document
                    .cards
                    .push(SheetCard::new(id.clone(), thing, CardRuns::Once));
            }
            _ => {
                return Err(SheetEditError::Invalid(
                    "a new button starts again or runs once".to_owned(),
                ));
            }
        }
        button.does.push(step);
        self.document.buttons.push(button);
        Ok(vec![id])
    }

    /// Returns the id of the card the kind of thing `every` runs each tick.
    fn tick_card_id(every: &str) -> String {
        match every {
            "cells" => "cell_tick".to_owned(),
            _ => format!("{every}_tick"),
        }
    }

    /// Adds the card `every` runs each tick, before every card that runs once.
    fn add_card(&mut self, every: &str) -> Result<Vec<String>, SheetEditError> {
        if every == "ground" {
            return Err(SheetEditError::Invalid("the ground has no card".to_owned()));
        }
        self.require_thing(every)?;
        let id = Self::tick_card_id(every);
        if id.len() > MAX_SHEET_ID_BYTES {
            return Err(SheetEditError::Invalid(format!(
                "the card id '{id}' holds more than {MAX_SHEET_ID_BYTES} bytes"
            )));
        }
        if self.document.cards.iter().any(|card| card.id == id) {
            return Err(SheetEditError::Taken(format!("the sheet already has the card '{id}'")));
        }
        let at = self
            .document
            .cards
            .iter()
            .position(|card| card.runs == CardRuns::Once)
            .unwrap_or(self.document.cards.len());
        self.document
            .cards
            .insert(at, SheetCard::new(id.clone(), every, CardRuns::EachTick));
        Ok(vec![id])
    }

    /// Renames `item` to `name`.
    #[expect(clippy::too_many_lines, reason = "one arm per kind of item reads best as one table")]
    fn rename(&mut self, item: &SheetItem, name: &str) -> Result<(), SheetEditError> {
        match item {
            SheetItem::Property(id) => {
                let name = clean_name(name, "property")?;
                let siblings: Vec<String> = self
                    .thing_of_property(id)
                    .and_then(|thing| self.properties_of(&thing))
                    .into_iter()
                    .flatten()
                    .filter(|property| property.id != *id)
                    .map(|property| property.name.clone())
                    .collect();
                refuse_taken(&name, siblings.iter().map(String::as_str), "property")?;
                self.property_mut(id)?.name = name;
            }
            SheetItem::State { property, state } => {
                let name = clean_name(name, "state")?;
                let property = self.property_mut(property)?;
                refuse_taken(
                    &name,
                    property
                        .states
                        .iter()
                        .filter(|known| known.id != *state)
                        .map(|known| known.name.as_str()),
                    "state",
                )?;
                property
                    .states
                    .iter_mut()
                    .find(|known| known.id == *state)
                    .ok_or_else(|| missing("state", state))?
                    .name = name;
            }
            SheetItem::Parameter(id) => {
                let name = clean_name(name, "parameter")?;
                let parameters = &mut self.document.parameters;
                refuse_taken(
                    &name,
                    parameters
                        .iter()
                        .filter(|known| known.id != *id)
                        .map(|known| known.name.as_str()),
                    "parameter",
                )?;
                parameters
                    .iter_mut()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("parameter", id))?
                    .name = name;
            }
            SheetItem::Measure(id) => {
                let name = clean_name(name, "measure")?;
                let measures = &mut self.document.measures;
                refuse_taken(
                    &name,
                    measures
                        .iter()
                        .filter(|known| known.id != *id)
                        .map(|known| known.name.as_str()),
                    "measure",
                )?;
                measures
                    .iter_mut()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("measure", id))?
                    .name = name;
            }
            SheetItem::Event(id) => {
                let name = clean_name(name, "event")?;
                let events = &mut self.document.events;
                refuse_taken(
                    &name,
                    events
                        .iter()
                        .filter(|known| known.id != *id)
                        .map(|known| known.name.as_str()),
                    "event",
                )?;
                events
                    .iter_mut()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("event", id))?
                    .name = name;
            }
            SheetItem::Button(id) => {
                let name = clean_name(name, "button")?;
                let buttons = &mut self.document.buttons;
                refuse_taken(
                    &name,
                    buttons
                        .iter()
                        .filter(|known| known.id != *id)
                        .map(|known| known.name.as_str()),
                    "button",
                )?;
                buttons
                    .iter_mut()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("button", id))?
                    .name = name;
            }
            SheetItem::AgentKind(id) => {
                let name = clean_name(name, "kind of agent")?;
                self.agent_kind_mut(id)?.name = name;
            }
            SheetItem::Card(id) => {
                return Err(SheetEditError::Invalid(format!("card '{id}' has no name of its own")));
            }
        }
        Ok(())
    }

    /// Returns the kind of thing that holds property `id`.
    fn thing_of_property(&self, id: &str) -> Option<String> {
        let has = |properties: &[SheetProperty]| properties.iter().any(|property| property.id == id);
        if self.document.cells.as_deref().is_some_and(has) {
            return Some("cells".to_owned());
        }
        if let Some(kind) = self.document.agents.iter().find(|kind| has(&kind.properties)) {
            return Some(kind.id.clone());
        }
        self.document
            .ground
            .as_ref()
            .filter(|ground| has(&ground.properties))
            .map(|_| "ground".to_owned())
    }

    /// Returns the properties of the kind of thing `thing`.
    fn properties_of(&self, thing: &str) -> Option<&[SheetProperty]> {
        match thing {
            "cells" => self.document.cells.as_deref(),
            "ground" => self.document.ground.as_ref().map(|ground| ground.properties.as_slice()),
            _ => self
                .document
                .agents
                .iter()
                .find(|kind| kind.id == thing)
                .map(|kind| kind.properties.as_slice()),
        }
    }

    /// Returns the property `id` of the ground, mutable, and an error naming `what` when the ground holds no such
    /// property or it is no property of type `property_type`.
    fn ground_property_mut(
        &mut self,
        id: &str,
        property_type: PropertyType,
        what: &str,
    ) -> Result<&mut SheetProperty, SheetEditError> {
        let property = self
            .document
            .ground
            .as_mut()
            .and_then(|ground| ground.properties.iter_mut().find(|property| property.id == id))
            .ok_or_else(|| missing("ground property", id))?;
        if property.property_type != property_type {
            return Err(SheetEditError::Invalid(format!("'{id}' is no {what} of the ground")));
        }
        Ok(property)
    }

    /// Sets the marks rule and the share kept per tick of the Number property `id` of the ground. No marks rule clears
    /// the share kept.
    fn set_marks(&mut self, id: &str, marks: Option<Marks>, kept_per_tick: Slot) -> Result<(), SheetEditError> {
        let property = self.ground_property_mut(id, PropertyType::Number, "Number property")?;
        property.marks = marks;
        property.kept_per_tick = if marks.is_some() { kept_per_tick } else { Slot::Blank };
        Ok(())
    }

    /// Keeps agents out of the cells whose ground property `property` is `state`, or lets them in.
    fn set_blocked(&mut self, property: &str, state: &str, blocked: bool) -> Result<(), SheetEditError> {
        let holder = self.ground_property_mut(property, PropertyType::States, "States property")?;
        if !holder.states.iter().any(|known| known.id == state) {
            return Err(missing("state", state));
        }
        let ground = self.document.ground.get_or_insert_with(Default::default);
        let held = ground
            .blocked
            .iter()
            .any(|known| known.property == property && known.state == state);
        if blocked && !held {
            ground.blocked.push(BlockedState::new(property, state));
        } else if !blocked {
            ground
                .blocked
                .retain(|known| !(known.property == property && known.state == state));
        }
        Ok(())
    }

    /// Returns the kind of agent `id`, mutable.
    fn agent_kind_mut(&mut self, id: &str) -> Result<&mut AgentKind, SheetEditError> {
        self.document
            .agents
            .iter_mut()
            .find(|kind| kind.id == id)
            .ok_or_else(|| missing("kind of agent", id))
    }

    /// Gives `item` the id `id` and rewrites every reference to it.
    #[expect(clippy::too_many_lines, reason = "one arm per kind of item reads best as one table")]
    fn change_id(&mut self, item: &SheetItem, id: &str) -> Result<(), SheetEditError> {
        if !is_id(id) || id.len() > MAX_SHEET_ID_BYTES {
            return Err(SheetEditError::Invalid(format!(
                "'{id}' is no sheet id: lower case letters, digits and single '_', a letter first, at most \
                 {MAX_SHEET_ID_BYTES} bytes"
            )));
        }
        let space = match item {
            SheetItem::Property(_) | SheetItem::Parameter(_) | SheetItem::Measure(_) | SheetItem::Event(_) => {
                IdSpace::Item
            }
            SheetItem::State { .. } => IdSpace::State,
            SheetItem::Button(_) => IdSpace::Button,
            SheetItem::Card(_) => IdSpace::Card,
            SheetItem::AgentKind(_) => IdSpace::Thing,
        };
        if space.is_reserved(id) {
            return Err(SheetEditError::Reserved(id.to_owned()));
        }
        let taken = |held: bool| {
            if held {
                Err(SheetEditError::Taken(format!("the id '{id}' is already held")))
            } else {
                Ok(())
            }
        };
        match item {
            SheetItem::Property(old) | SheetItem::Parameter(old) | SheetItem::Measure(old) | SheetItem::Event(old) => {
                if old == id {
                    return Ok(());
                }
                let items = self.item_ids();
                if !items.contains(old.as_str()) {
                    return Err(missing("item", old));
                }
                taken(items.contains(id))?;
                match item {
                    SheetItem::Property(_) => {
                        change_references(&mut self.document, &Target::Property(old.clone()), Change::Rename(id));
                        self.property_mut(old)?.id = id.to_owned();
                    }
                    SheetItem::Parameter(_) => {
                        change_references(&mut self.document, &Target::Parameter(old.clone()), Change::Rename(id));
                        if let Some(parameter) = self.document.parameters.iter_mut().find(|known| known.id == *old) {
                            id.clone_into(&mut parameter.id);
                        }
                    }
                    SheetItem::Event(_) => {
                        change_references(&mut self.document, &Target::Event(old.clone()), Change::Rename(id));
                        if let Some(event) = self.document.events.iter_mut().find(|known| known.id == *old) {
                            id.clone_into(&mut event.id);
                        }
                    }
                    _ => {
                        let measure = self
                            .document
                            .measures
                            .iter_mut()
                            .find(|known| known.id == *old)
                            .ok_or_else(|| missing("measure", old))?;
                        id.clone_into(&mut measure.id);
                    }
                }
            }
            SheetItem::State { property, state } => {
                if state == id {
                    return Ok(());
                }
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                if !holder.states.iter().any(|known| known.id == *state) {
                    return Err(missing("state", state));
                }
                taken(holder.states.iter().any(|known| known.id == id))?;
                change_references(
                    &mut self.document,
                    &Target::State {
                        property: property.clone(),
                        state: state.clone(),
                    },
                    Change::Rename(id),
                );
                if let Some(known) = self
                    .property_mut(property)?
                    .states
                    .iter_mut()
                    .find(|known| known.id == *state)
                {
                    id.clone_into(&mut known.id);
                }
            }
            SheetItem::Button(old) => {
                if old == id {
                    return Ok(());
                }
                if !self.document.buttons.iter().any(|button| button.id == *old) {
                    return Err(missing("button", old));
                }
                taken(self.document.buttons.iter().any(|button| button.id == id))?;
                let card_follows = self.document.cards.iter().any(|card| card.id == *old)
                    && self.document.buttons.iter().any(|button| {
                        button.id == *old
                            && button
                                .does
                                .iter()
                                .any(|step| step.card.as_deref() == Some(old.as_str()))
                    });
                if card_follows {
                    taken(self.document.cards.iter().any(|card| card.id == id))?;
                    self.rename_card(old, id);
                }
                if let Some(button) = self.document.buttons.iter_mut().find(|button| button.id == *old) {
                    id.clone_into(&mut button.id);
                }
            }
            SheetItem::Card(old) => {
                if old == id {
                    return Ok(());
                }
                if !self.document.cards.iter().any(|card| card.id == *old) {
                    return Err(missing("card", old));
                }
                taken(self.document.cards.iter().any(|card| card.id == id))?;
                self.rename_card(old, id);
            }
            SheetItem::AgentKind(old) => {
                if old == id {
                    return Ok(());
                }
                if !self.document.agents.iter().any(|kind| kind.id == *old) {
                    return Err(missing("kind of agent", old));
                }
                taken(self.document.agents.iter().any(|kind| kind.id == id))?;
                let old_tick = Self::tick_card_id(old);
                let new_tick = Self::tick_card_id(id);
                if self.document.cards.iter().any(|card| card.id == old_tick) {
                    taken(self.document.cards.iter().any(|card| card.id == new_tick))?;
                    self.rename_card(&old_tick, &new_tick);
                }
                change_references(&mut self.document, &Target::Thing(old.clone()), Change::Rename(id));
                self.agent_kind_mut(old)?.id = id.to_owned();
            }
        }
        Ok(())
    }

    /// Gives card `old` the id `new`, and every button step that runs it.
    fn rename_card(&mut self, old: &str, new: &str) {
        change_references(&mut self.document, &Target::Card(old.to_owned()), Change::Rename(new));
        if let Some(card) = self.document.cards.iter_mut().find(|card| card.id == old) {
            new.clone_into(&mut card.id);
        }
    }

    /// Recolours the state or measure `item`.
    fn recolor(&mut self, item: &SheetItem, color: Color) -> Result<(), SheetEditError> {
        match item {
            SheetItem::State { property, state } => {
                let holder = self.property_mut(property)?;
                let known = holder
                    .states
                    .iter_mut()
                    .find(|known| known.id == *state)
                    .ok_or_else(|| missing("state", state))?;
                let old = known.color;
                known.color = color;
                for measure in &mut self.document.measures {
                    if measure.rule == MeasureRule::CountInState
                        && measure.property.as_deref() == Some(property.as_str())
                        && measure.state.as_deref() == Some(state.as_str())
                        && measure.color == old
                    {
                        measure.color = color;
                    }
                }
            }
            SheetItem::Measure(id) => {
                self.document
                    .measures
                    .iter_mut()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("measure", id))?
                    .color = color;
            }
            _ => {
                return Err(SheetEditError::Invalid(
                    "only a state or a measure has a colour".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Deletes `item` and clears every reference to it.
    fn delete(&mut self, item: &SheetItem) -> Result<(), SheetEditError> {
        match item {
            SheetItem::Property(id) => {
                self.property(id).ok_or_else(|| missing("property", id))?;
                // The states go first, so every chip of a state of the property is cleared with them.
                let states: Vec<String> = self
                    .property(id)
                    .map(|property| property.states.iter().map(|state| state.id.clone()).collect())
                    .unwrap_or_default();
                for state in states {
                    change_references(
                        &mut self.document,
                        &Target::State {
                            property: id.clone(),
                            state,
                        },
                        Change::Clear,
                    );
                }
                change_references(&mut self.document, &Target::Property(id.clone()), Change::Clear);
                if let Some(cells) = self.document.cells.as_mut() {
                    cells.retain(|property| property.id != *id);
                }
                for kind in &mut self.document.agents {
                    kind.properties.retain(|property| property.id != *id);
                }
                if let Some(ground) = self.document.ground.as_mut() {
                    ground.properties.retain(|property| property.id != *id);
                }
            }
            SheetItem::State { property, state } => {
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                if !holder.states.iter().any(|known| known.id == *state) {
                    return Err(missing("state", state));
                }
                change_references(
                    &mut self.document,
                    &Target::State {
                        property: property.clone(),
                        state: state.clone(),
                    },
                    Change::Clear,
                );
                self.property_mut(property)?.states.retain(|known| known.id != *state);
            }
            SheetItem::Parameter(id) => {
                if !self.document.parameters.iter().any(|known| known.id == *id) {
                    return Err(missing("parameter", id));
                }
                change_references(&mut self.document, &Target::Parameter(id.clone()), Change::Clear);
                self.document.parameters.retain(|known| known.id != *id);
            }
            SheetItem::Measure(id) => {
                if !self.document.measures.iter().any(|known| known.id == *id) {
                    return Err(missing("measure", id));
                }
                self.document.measures.retain(|known| known.id != *id);
            }
            SheetItem::Event(id) => {
                if !self.document.events.iter().any(|known| known.id == *id) {
                    return Err(missing("event", id));
                }
                change_references(&mut self.document, &Target::Event(id.clone()), Change::Clear);
                self.document.events.retain(|known| known.id != *id);
            }
            SheetItem::Button(id) => {
                let button = self
                    .document
                    .buttons
                    .iter()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("button", id))?;
                let cards: Vec<String> = button.does.iter().filter_map(|step| step.card.clone()).collect();
                self.document.buttons.retain(|known| known.id != *id);
                for card in cards {
                    let still_run = self.document.buttons.iter().any(|other| {
                        other
                            .does
                            .iter()
                            .any(|step| step.card.as_deref() == Some(card.as_str()))
                    });
                    let runs_once = self
                        .document
                        .cards
                        .iter()
                        .any(|known| known.id == card && known.runs == CardRuns::Once);
                    if !still_run && runs_once {
                        self.delete_card(&card)?;
                    }
                }
            }
            SheetItem::Card(id) => self.delete_card(id)?,
            SheetItem::AgentKind(id) => {
                return Err(SheetEditError::Invalid(format!(
                    "the kind of agent '{id}' is the world's only one and stays"
                )));
            }
        }
        Ok(())
    }

    /// Deletes card `id` with its lines, and clears every reference to them and to it.
    fn delete_card(&mut self, id: &str) -> Result<(), SheetEditError> {
        let index = self
            .document
            .cards
            .iter()
            .position(|card| card.id == id)
            .ok_or_else(|| missing("card", id))?;
        let card = self.document.cards.remove(index);
        for line in card.line_tables.keys() {
            change_references(&mut self.document, &Target::Line(line.clone()), Change::Clear);
        }
        change_references(&mut self.document, &Target::Card(id.to_owned()), Change::Clear);
        Ok(())
    }

    /// Returns the next line id and moves `next_line` past it.
    fn mint_line_id(&mut self) -> Result<String, SheetEditError> {
        loop {
            let number = self.document.sheet.next_line.max(1);
            let next = number
                .checked_add(1)
                .ok_or_else(|| SheetEditError::Invalid("the sheet has minted every line number".to_owned()))?;
            self.document.sheet.next_line = next;
            let id = format!("l{number}");
            if self.document.line(&id).is_none() {
                return Ok(id);
            }
        }
    }

    /// Returns the state the nearest When around `parent` tests as "my X is S", with its property.
    fn tested_state(&self, card: usize, mut parent: Option<String>) -> Option<(String, String)> {
        let card = &self.document.cards[card];
        while let Some(id) = parent {
            let line = card.line_tables.get(&id)?;
            if line.kind == LineKind::When
                && let Some(SheetValue::Phrase(test)) = line.condition.value()
                && test.kind == LineKind::Compare
                && test.test == Some(CompareTest::Is)
                && let Some(SheetValue::Chip(SheetChip::My(property))) = test.left.value()
                && let Some(SheetValue::Chip(SheetChip::State(state))) = test.right.value()
            {
                return Some((property.clone(), state.clone()));
            }
            parent = lines::parent_of(card, &id);
        }
        None
    }

    /// Returns whether a line inside `parent` of card `card` sits inside a When, directly or deeper.
    fn inside_when(&self, card: usize, mut parent: Option<String>) -> bool {
        let card = &self.document.cards[card];
        while let Some(id) = parent {
            if card
                .line_tables
                .get(&id)
                .is_some_and(|line| line.kind == LineKind::When)
            {
                return true;
            }
            parent = lines::parent_of(card, &id);
        }
        false
    }

    /// Adds `line` at the end of card `card_id`, or of the block line `inside`.
    fn add_line(
        &mut self,
        card_id: &str,
        inside: Option<&str>,
        mut line: SheetLine,
    ) -> Result<Vec<String>, SheetEditError> {
        let card = self
            .document
            .cards
            .iter()
            .position(|card| card.id == card_id)
            .ok_or_else(|| missing("card", card_id))?;
        if let Some(block) = inside {
            let holder = self.document.cards[card]
                .line_tables
                .get(block)
                .ok_or_else(|| SheetEditError::Missing(format!("card '{card_id}' has no line '{block}'")))?;
            if !is_block(holder.kind) {
                return Err(SheetEditError::Invalid(format!(
                    "line '{block}' holds no lines: it is no When or Repeat"
                )));
            }
        }
        if self.document.cards[card].line_tables.len() >= MAX_LINES_PER_CARD {
            return Err(SheetEditError::Invalid(format!(
                "card '{card_id}' holds {MAX_LINES_PER_CARD} lines, the most a card holds"
            )));
        }
        line.lines.clear();
        let parent = inside.map(str::to_owned);
        if self.inside_when(card, parent.clone()) {
            if matches!(line.kind, LineKind::Become | LineKind::Set) && line.condition.is_blank() {
                line.condition = Slot::Filled(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::Always))));
            }
            if line.kind == LineKind::Become
                && line.start_state.is_none()
                && let Some((property, state)) = self.tested_state(card, parent.clone())
                && line.property.as_ref().is_none_or(|own| *own == property)
            {
                line.property = Some(property);
                line.start_state = Some(state);
            }
        }
        line_rates_from_tick(&mut line, self.document.time.map(|time| time.tick_unit));
        let id = self.mint_line_id()?;
        let target = &mut self.document.cards[card];
        target.line_tables.insert(id.clone(), line);
        match inside {
            Some(block) => {
                if let Some(holder) = target.line_tables.get_mut(block) {
                    holder.lines.push(id.clone());
                }
            }
            None => target.lines.push(id.clone()),
        }
        self.touched.push(id.clone());
        self.refuse_deep(&id)?;
        Ok(vec![id])
    }

    /// Puts `value` in the slot at `path` of line `line_id`.
    fn set_slot(&mut self, line_id: &str, path: &str, mut value: Slot) -> Result<(), SheetEditError> {
        let steps = lines::parse_path(path)?;
        let tick = self.document.time.map(|time| time.tick_unit);
        if let Slot::Filled(inner) = &mut value {
            rate_from_tick(inner, tick);
        }
        // A parameter put into a rate gives the rate its unit.
        let parameter = match (&value, steps.last()) {
            (Slot::Filled(SheetValue::Chip(SheetChip::Param(id))), Some(PathStep::Slot(slot))) if slot == "rate" => {
                Some(id.clone())
            }
            _ => None,
        };
        let line = self.line_mut(line_id)?;
        lines::set_value(line, &steps, value)?;
        if let Some(parameter) = parameter {
            self.rate_takes_parameter(line_id, &steps, &parameter)?;
        }
        self.refuse_deep(line_id)
    }

    /// Gives the rate at `steps` of line `line_id` the unit of `parameter`, when the rate's unit is empty or marked.
    fn rate_takes_parameter(
        &mut self,
        line_id: &str,
        steps: &[PathStep],
        parameter: &str,
    ) -> Result<(), SheetEditError> {
        let parameter_unit = self
            .document
            .parameters
            .iter()
            .find(|known| known.id == parameter)
            .and_then(|known| known.unit.as_deref())
            .and_then(unit_in_text);
        let inner = steps.split_last().map_or(&[][..], |(_, inner)| inner);
        let line = self.line_mut(line_id)?;
        let rate = lines::phrase_mut(line, inner)?;
        if rate.kind != LineKind::Rate {
            return Ok(());
        }
        let chosen = parameter_unit.filter(|_| rate.rate_unit.is_none() || rate.rate_unit_from_tick);
        if let Some(unit) = chosen {
            rate.rate_unit = Some(unit);
            rate.rate_unit_from_tick = false;
        }
        if let Some(unit) = chosen
            && self.document.time.is_none()
        {
            self.document.time = Some(SheetTime::new(1.0, unit));
        }
        Ok(())
    }

    /// Sets `field` of the phrase at `phrase` of line `line_id`.
    fn set_field(&mut self, line_id: &str, phrase: &str, field: LineField) -> Result<(), SheetEditError> {
        let steps = if phrase.is_empty() {
            Vec::new()
        } else {
            lines::parse_path(phrase)?
        };
        let state_owner = match &field {
            LineField::NewState(Some((property, state))) => {
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                if !holder.states.iter().any(|known| known.id == *state) {
                    return Err(missing("state", state));
                }
                Some(holder.states.iter().map(|known| known.id.clone()).collect::<Vec<_>>())
            }
            LineField::Property(Some(property)) => Some(
                self.property(property)
                    .map(|holder| holder.states.iter().map(|known| known.id.clone()).collect())
                    .unwrap_or_default(),
            ),
            _ => None,
        };
        let mut chosen_unit = None;
        let line = self.line_mut(line_id)?;
        let target = lines::phrase_mut(line, &steps)?;
        match field {
            LineField::Property(property) => {
                let states = state_owner.unwrap_or_default();
                let keep = |state: &Option<String>| {
                    state
                        .as_deref()
                        .is_some_and(|state| state == ANY_STATE_ID || states.iter().any(|known| known == state))
                };
                if target.property != property {
                    if !keep(&target.new_state) {
                        target.new_state = None;
                    }
                    if !keep(&target.start_state) {
                        target.start_state = None;
                    }
                }
                target.property = property;
            }
            LineField::NewState(new_state) => match new_state {
                Some((property, state)) => {
                    let states = state_owner.unwrap_or_default();
                    let old_fits = target
                        .start_state
                        .as_deref()
                        .is_some_and(|old| old == ANY_STATE_ID || states.iter().any(|known| known == old));
                    if !old_fits {
                        target.start_state = None;
                    }
                    target.property = Some(property);
                    target.new_state = Some(state);
                }
                None => target.new_state = None,
            },
            LineField::StartState(state) => target.start_state = state,
            LineField::Test(test) => {
                let list = |test: Option<CompareTest>| matches!(test, Some(CompareTest::OneOf | CompareTest::NotOneOf));
                match (list(target.test), list(test)) {
                    (false, true) => {
                        if let Slot::Filled(value) = &target.right
                            && !matches!(value, SheetValue::List(_))
                        {
                            target.right = Slot::Filled(SheetValue::List(vec![value.clone()]));
                        }
                    }
                    (true, false) => {
                        if let Slot::Filled(SheetValue::List(items)) = &target.right {
                            target.right = items.first().cloned().map_or(Slot::Blank, Slot::Filled);
                        }
                    }
                    _ => {}
                }
                target.test = test;
            }
            LineField::Around(around) => target.around = around,
            LineField::RateUnit(unit) => {
                target.rate_unit = unit;
                target.rate_unit_from_tick = false;
                chosen_unit = unit.map(|unit| (unit, rate_parameter(target)));
            }
            LineField::Formula(formula) => target.formula = formula.filter(|formula| !formula.trim().is_empty()),
            LineField::Mode(mode) => target.mode = mode,
            LineField::Pick(pick) => target.pick = pick,
            LineField::RankedBy(property) => target.ranked_by = property,
            LineField::Which(which) => target.which = which,
            LineField::GroundProperty(property) => target.ground_property = property,
            LineField::Event(event) => target.event = event,
            LineField::AgentKind(kind) => target.agent_kind = kind,
            LineField::IncludeMe(include) => target.include_me = include,
            LineField::OpenOnly(open) => target.open_only = open,
            LineField::Summaries(summaries) => target.summaries = summaries,
        }
        if let Some((unit, parameter)) = chosen_unit {
            self.rate_unit_chosen(unit, parameter.as_deref());
        }
        self.refuse_deep(line_id)
    }

    /// Sets one tick to 1 `unit` when the sheet has no length of one tick, and gives `parameter`, the parameter a rate
    /// reads, the unit when it has none.
    fn rate_unit_chosen(&mut self, unit: TimeUnit, parameter: Option<&str>) {
        if self.document.time.is_none() {
            self.document.time = Some(SheetTime::new(1.0, unit));
        }
        if let Some(parameter) = parameter
            && let Some(known) = self.document.parameters.iter_mut().find(|known| known.id == parameter)
            && known.unit.is_none()
        {
            known.unit = Some(unit_suggestion(unit));
        }
    }

    /// Chooses the form `item` for the condition of line `line_id`.
    fn choose_condition(&mut self, line_id: &str, item: ConditionItem) -> Result<(), SheetEditError> {
        let tick = self.document.time.map(|time| time.tick_unit);
        let line = self.line_mut(line_id)?;
        let current = match line.condition.value() {
            Some(SheetValue::Phrase(phrase)) => Some(phrase.as_ref().clone()),
            _ => None,
        };
        let phrase = match item {
            ConditionItem::Always => {
                if line.kind == LineKind::When {
                    return Err(SheetEditError::Invalid("a When cannot be always".to_owned()));
                }
                SheetLine::new(LineKind::Always)
            }
            ConditionItem::Is | ConditionItem::AtLeast | ConditionItem::AtMost => {
                let test = match item {
                    ConditionItem::Is => CompareTest::Is,
                    ConditionItem::AtLeast => CompareTest::AtLeast,
                    _ => CompareTest::AtMost,
                };
                let mut compare = match current {
                    Some(compare)
                        if compare.kind == LineKind::Compare && compare.test != Some(CompareTest::Outside) =>
                    {
                        compare
                    }
                    _ => SheetLine::new(LineKind::Compare),
                };
                if let Slot::Filled(SheetValue::List(items)) = &compare.right {
                    compare.right = items.first().cloned().map_or(Slot::Blank, Slot::Filled);
                }
                compare.test = Some(test);
                compare
            }
            ConditionItem::Outside => match current {
                Some(compare) if compare.kind == LineKind::Compare && compare.test == Some(CompareTest::Outside) => {
                    compare
                }
                _ => {
                    let mut compare = SheetLine::new(LineKind::Compare);
                    compare.test = Some(CompareTest::Outside);
                    compare
                }
            },
            ConditionItem::WithChance => match current {
                Some(chance) if chance.kind == LineKind::Chance => chance,
                _ => SheetLine::new(LineKind::Chance),
            },
            ConditionItem::AtRate => match current {
                Some(rate) if rate.kind == LineKind::Rate => rate,
                _ => {
                    let mut rate = SheetLine::new(LineKind::Rate);
                    line_rates_from_tick(&mut rate, tick);
                    rate
                }
            },
        };
        line.condition = Slot::Filled(SheetValue::Phrase(Box::new(phrase)));
        Ok(())
    }

    /// Replaces the one-sided outside condition of line `line_id` with the plain comparison it means.
    fn use_one_side(&mut self, line_id: &str) -> Result<(), SheetEditError> {
        let line = self.line_mut(line_id)?;
        let not_one_sided =
            || SheetEditError::Invalid(format!("line '{line_id}' has no condition with one side filled"));
        let Some(SheetValue::Phrase(condition)) = line.condition.value() else {
            return Err(not_one_sided());
        };
        if condition.kind != LineKind::Compare || condition.test != Some(CompareTest::Outside) {
            return Err(not_one_sided());
        }
        let (test, right) = match (&condition.low, &condition.high) {
            (Slot::Filled(low), Slot::Blank) => (CompareTest::LessThan, low.clone()),
            (Slot::Blank, Slot::Filled(high)) => (CompareTest::MoreThan, high.clone()),
            _ => return Err(not_one_sided()),
        };
        let mut compare = SheetLine::new(LineKind::Compare);
        compare.left = condition.left.clone();
        compare.test = Some(test);
        compare.right = Slot::Filled(right);
        line.condition = Slot::Filled(SheetValue::Phrase(Box::new(compare)));
        Ok(())
    }

    /// Moves the count that the chance or rate of line `line_id` holds as its value into "for each".
    fn use_count_for_each(&mut self, line_id: &str) -> Result<(), SheetEditError> {
        let counts: BTreeSet<String> = self
            .document
            .cards
            .iter()
            .flat_map(|card| &card.line_tables)
            .filter(|(_, line)| line.kind == LineKind::Count)
            .map(|(id, _)| id.clone())
            .collect();
        let line = self.line_mut(line_id)?;
        let holder = if matches!(line.kind, LineKind::Chance | LineKind::Rate) {
            line
        } else {
            match &mut line.condition {
                Slot::Filled(SheetValue::Phrase(phrase))
                    if matches!(phrase.kind, LineKind::Chance | LineKind::Rate) =>
                {
                    phrase.as_mut()
                }
                _ => {
                    return Err(SheetEditError::Invalid(format!(
                        "line '{line_id}' holds no chance or rate"
                    )));
                }
            }
        };
        let slot = if holder.kind == LineKind::Rate {
            &mut holder.rate
        } else {
            &mut holder.chance
        };
        let count = match slot.value() {
            Some(
                chip @ SheetValue::Chip(SheetChip::Result {
                    line: count_line,
                    part: None,
                    property: None,
                }),
            ) if counts.contains(count_line) => chip.clone(),
            _ => {
                return Err(SheetEditError::Invalid(format!(
                    "the chance or rate of line '{line_id}' is no count"
                )));
            }
        };
        *slot = Slot::Blank;
        holder.tries = Slot::Filled(count);
        Ok(())
    }

    /// Turns the chance of line `line_id` into a rate for `into` [`LineKind::Rate`], or its rate into a chance for
    /// `into` [`LineKind::Chance`], keeping its chance per tick.
    fn change_draw(&mut self, line_id: &str, into: LineKind) -> Result<(), SheetEditError> {
        let time = self
            .document
            .time
            .ok_or_else(|| SheetEditError::Invalid("the sheet sets no length of one tick".to_owned()))?;
        let from = if into == LineKind::Rate {
            LineKind::Chance
        } else {
            LineKind::Rate
        };
        let line = self.line_mut(line_id)?;
        let holder = if line.kind == from {
            line
        } else {
            match &mut line.condition {
                Slot::Filled(SheetValue::Phrase(phrase)) if phrase.kind == from => phrase.as_mut(),
                _ => {
                    return Err(SheetEditError::Invalid(format!(
                        "line '{line_id}' holds no {}",
                        from.as_str()
                    )));
                }
            }
        };
        let slot = if from == LineKind::Rate {
            &holder.rate
        } else {
            &holder.chance
        };
        let typed = match slot.value() {
            Some(SheetValue::Literal(literal @ (SheetLiteral::Number(_) | SheetLiteral::Percent(_)))) => {
                literal.number()
            }
            _ => None,
        };
        let typed = typed.ok_or_else(|| {
            SheetEditError::Invalid(format!("the {} of line '{line_id}' is no typed number", from.as_str()))
        })?;
        let converted = if into == LineKind::Rate {
            if !(0.0..1.0).contains(&typed) {
                return Err(SheetEditError::Invalid(format!(
                    "a chance of {typed} gives no rate, which needs a chance from 0 to below 1"
                )));
            }
            rate_from_chance(typed, time.tick_length)
        } else {
            let unit = holder
                .rate_unit
                .ok_or_else(|| SheetEditError::Invalid(format!("the rate of line '{line_id}' has no unit")))?;
            chance_from_rate(typed, tick_in(time, unit))
        };
        let converted = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(shortest_f32(converted))));
        holder.kind = into;
        if into == LineKind::Rate {
            holder.rate = converted;
            holder.chance = Slot::Blank;
            holder.rate_unit = Some(time.tick_unit);
            holder.rate_unit_from_tick = true;
        } else {
            holder.chance = converted;
            holder.rate = Slot::Blank;
            holder.rate_unit = None;
            holder.rate_unit_from_tick = false;
        }
        Ok(())
    }

    /// Copies the condition of the When `when_id` into the one change inside it and removes the When.
    fn move_condition_into(&mut self, when_id: &str) -> Result<(), SheetEditError> {
        let card = self.card_of_line(when_id)?;
        let when = self.line(when_id)?;
        let refused = || SheetEditError::Invalid(format!("line '{when_id}' is no When around exactly one change"));
        if when.kind != LineKind::When {
            return Err(refused());
        }
        let [inner] = when.lines.as_slice() else {
            return Err(refused());
        };
        let inner = inner.clone();
        let condition = when.condition.clone();
        let change = self.line(&inner)?;
        if !matches!(change.kind, LineKind::Become | LineKind::Set) {
            return Err(refused());
        }
        let always =
            matches!(change.condition.value(), Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Always);
        if !(change.condition.is_blank() || always) {
            return Err(SheetEditError::Invalid(format!(
                "line '{inner}' has a condition of its own, and the When's condition would replace it"
            )));
        }
        self.line_mut(&inner)?.condition = condition;
        lines::remove_line(&mut self.document.cards[card], when_id)?;
        Ok(())
    }

    /// Deletes line `line_id`, keeps its lines one level up, and clears every reference to its result.
    fn delete_line(&mut self, line_id: &str) -> Result<(), SheetEditError> {
        let card = self.card_of_line(line_id)?;
        lines::remove_line(&mut self.document.cards[card], line_id)?;
        change_references(&mut self.document, &Target::Line(line_id.to_owned()), Change::Clear);
        Ok(())
    }
}
