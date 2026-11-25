//! The edit commands of a sheet, the items and fields they name, and the reasons a command is refused.

use std::fmt;

use henad_core::metadata::TimeUnit;

use crate::sheet::document::{
    Around, ButtonStep, ButtonStepRule, CellWhich, ColorRule, CompareTest, Edges, LineMode, LookSummary, Marks,
    MeasureRule, ParameterType, PickRule, PropertyType, SheetLine, SheetTime, Slot, StartsAs, SummaryKind,
};
use crate::sheet::sentence::ConditionItem;
use crate::value::Color;

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
    /// Rule a Pick a cell line picks by.
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
        /// Behaviour of the edges.
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
        /// Colour of the state, `None` for the next of [`STATE_COLORS`](super::STATE_COLORS) after the property's states.
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
        /// Meaning of the parameter.
        means: Option<String>,
    },
    /// Adds a measure, with an id minted from its name, and returns the id.
    AddMeasure {
        /// Name of the measure, which is also its column in results.
        name: String,
        /// Quantity the measure counts or sums.
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
        /// [`MEASURE_COLORS`](super::MEASURE_COLORS).
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
        /// Action of the button: [`ButtonStepRule::StartAgain`] or [`ButtonStepRule::RunOnce`].
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
    /// kind of agent. An item the sheet does not hold under its kind is [`SheetEditError::Missing`], even when another
    /// kind of item holds its id.
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
        /// Rule by which marks combine, `None` for a property that takes no marks.
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
    /// Chooses the form of the condition of a row of a Choose line, as [`Self::ChooseCondition`] chooses a change's.
    ChooseRowCondition {
        /// Id of the line.
        line: String,
        /// Index of the row, from 0.
        row: usize,
        /// The form.
        item: ConditionItem,
    },
    /// Replaces a condition "is less than … or more than …" with one side filled by the plain comparison it
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
    /// Adds a summary of the kind `summary` at the end of the summaries of a look at agents, with its value empty.
    ///
    /// The summary takes its automatic name, such as "average", and the id minted from it, and both follow its value
    /// until it is renamed.
    AddSummary {
        /// Id of the line.
        line: String,
        /// Kind of the summary over the agents.
        summary: SummaryKind,
    },
    /// Removes the summary at an index of a look at agents. A slot that used it turns blank.
    DeleteSummary {
        /// Id of the line.
        line: String,
        /// Index of the summary, from 0.
        index: usize,
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
    /// written as the shortest number that reads back as the same `f32`. The sheet's tick needs a length. A card that
    /// runs once takes no rate, and a chance there is refused in the words of
    /// [`RATE_IN_BUTTON_CARD`](crate::sheet::sentence::RATE_IN_BUTTON_CARD). A chance on a line inside a Repeat is
    /// refused in the words of [`RATE_IN_REPEAT`](crate::sheet::sentence::RATE_IN_REPEAT).
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
