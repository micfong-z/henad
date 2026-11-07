//! The sheet document: every table of a model file of kind `sheet`, as plain data with public fields.
//!
//! The tables follow the order of the sheet: the `[sheet]` header, the world, the time scale, the kinds of thing with
//! their properties, events, parameters, measures, colour rules, buttons and cards. Items refer to each other by id,
//! and display names are data, so a rename changes one `name`.
//!
//! A line is a [`SheetLine`]: a [`LineKind`] and every field and slot any kind can hold. A field the file leaves out is
//! `None`, an empty list, or [`Slot::Blank`] for a slot. A half-built line is stored this way. The reader takes any
//! field on a line of any kind.

use std::collections::BTreeMap;
use std::fmt;

use henad_core::metadata::TimeUnit;

use crate::value::Color;

/// A sheet: the author document of a model, from its header to its cards.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetDocument {
    /// The `[sheet]` table.
    pub sheet: SheetHeader,
    /// The `[world]` table.
    pub world: SheetWorld,
    /// The `[time]` table, `None` when the file writes none.
    pub time: Option<SheetTime>,
    /// The properties of the cells of a grid world, `None` when the file writes no `[cells]` table.
    pub cells: Option<Vec<SheetProperty>>,
    /// The kinds of agent, each an `[[agents]]` table.
    pub agents: Vec<AgentKind>,
    /// The ground under agents, `None` when the file writes no `[ground]` table.
    pub ground: Option<SheetGround>,
    /// Each `[[event]]` table.
    pub events: Vec<SheetEvent>,
    /// Each `[[parameter]]` table.
    pub parameters: Vec<SheetParameter>,
    /// Each `[[measure]]` table.
    pub measures: Vec<SheetMeasure>,
    /// The colour rule of each kind of thing, by the thing's id, each a `[color.<thing>]` table.
    pub colors: BTreeMap<String, ColorRule>,
    /// Each `[[button]]` table.
    pub buttons: Vec<SheetButton>,
    /// Each `[[card]]` table.
    pub cards: Vec<SheetCard>,
}

/// Prints the sheet's id and the number of its cards and lines.
impl fmt::Debug for SheetDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SheetDocument")
            .field("id", &self.sheet.id)
            .field("cards", &self.cards.len())
            .field(
                "lines",
                &self.cards.iter().map(|card| card.line_tables.len()).sum::<usize>(),
            )
            .finish_non_exhaustive()
    }
}

impl SheetDocument {
    /// Creates a sheet with `sheet` and `world` and no other table.
    pub fn new(sheet: SheetHeader, world: SheetWorld) -> Self {
        Self {
            sheet,
            world,
            time: None,
            cells: None,
            agents: Vec::new(),
            ground: None,
            events: Vec::new(),
            parameters: Vec::new(),
            measures: Vec::new(),
            colors: BTreeMap::new(),
            buttons: Vec::new(),
            cards: Vec::new(),
        }
    }

    /// Returns the line `line_id` and the index of its card, or `None` when no card holds it.
    pub fn line(&self, line_id: &str) -> Option<(usize, &SheetLine)> {
        self.cards
            .iter()
            .enumerate()
            .find_map(|(index, card)| card.line_tables.get(line_id).map(|line| (index, line)))
    }
}

/// The `[sheet]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetHeader {
    /// Version of the author format the file is written in.
    pub version: u32,
    /// Id of the model.
    pub id: String,
    /// Name of the model.
    pub name: String,
    /// What the model is for, in the author's words.
    pub purpose: Option<String>,
    /// Number of the next line id to mint. Every line id is below it.
    pub next_line: u32,
}

impl SheetHeader {
    /// Creates the header of sheet `id` named `name`, in [`super::SHEET_VERSION`], with no purpose.
    pub fn new(id: impl Into<String>, name: impl Into<String>, next_line: u32) -> Self {
        Self {
            version: super::SHEET_VERSION,
            id: id.into(),
            name: name.into(),
            purpose: None,
            next_line,
        }
    }
}

/// The `[world]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetWorld {
    /// What the world is made of.
    pub made_of: MadeOf,
    /// Width in cells.
    pub width: u32,
    /// Height in cells.
    pub height: u32,
    /// What happens at the world's edges.
    pub edges: Edges,
}

impl SheetWorld {
    /// Creates a world of `made_of`, `width` by `height` cells, with `edges`.
    pub fn new(made_of: MadeOf, width: u32, height: u32, edges: Edges) -> Self {
        Self {
            made_of,
            width,
            height,
            edges,
        }
    }
}

/// Value of the world's `made_of` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MadeOf {
    /// Cells on a grid, `cells`.
    Cells,
    /// Agents in open space over a ground of cells, `agents`.
    Agents,
}

/// Value of the world's `edges` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Edges {
    /// Each edge joins the opposite edge, `wrap`.
    Wrap,
    /// The world ends at its edges, `walls`.
    Walls,
}

/// The `[time]` table: the length of one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SheetTime {
    /// Length of one tick in [`Self::tick_unit`].
    pub tick_length: f64,
    /// Unit of [`Self::tick_length`].
    pub tick_unit: TimeUnit,
}

impl SheetTime {
    /// Creates the time scale of a tick `tick_length` units of `tick_unit` long.
    pub fn new(tick_length: f64, tick_unit: TimeUnit) -> Self {
        Self { tick_length, tick_unit }
    }
}

/// A property of a kind of thing, a `[[cells.property]]`, `[[agents.property]]` or `[[ground.property]]` table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetProperty {
    /// Id of the property.
    pub id: String,
    /// Name of the property.
    pub name: String,
    /// Type of the property's value.
    pub property_type: PropertyType,
    /// States of a States property, in order.
    pub states: Vec<SheetState>,
    /// How agents' marks on a Number property of the ground combine.
    pub marks: Option<Marks>,
    /// Share of a Number property of the ground left after a tick's fading.
    pub kept_per_tick: Slot,
    /// How the property starts, `None` for a property with no start, such as a heading.
    pub starts_as: Option<StartsAs>,
    /// The author's comment on the property.
    pub note: Option<String>,
}

impl SheetProperty {
    /// Creates property `id` named `name` of `property_type`, with no states and no start.
    pub fn new(id: impl Into<String>, name: impl Into<String>, property_type: PropertyType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            property_type,
            states: Vec::new(),
            marks: None,
            kept_per_tick: Slot::Blank,
            starts_as: None,
            note: None,
        }
    }
}

/// Value of a property's `type` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PropertyType {
    /// One of a few named states, `states`.
    States,
    /// A number, `number`.
    Number,
    /// A number with no fraction, `whole_number`.
    WholeNumber,
    /// Yes or no, `yes_no`.
    YesNo,
    /// An arrow in the plane, `arrow`.
    Arrow,
    /// An agent's place, `position`.
    Position,
    /// An agent's direction of travel, `heading`.
    Heading,
}

/// One state of a States property, an element of its `states` list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetState {
    /// Id of the state, unique within its property.
    pub id: String,
    /// Name of the state.
    pub name: String,
    /// Colour of the state as red, green, blue and alpha bytes.
    pub color: Color,
}

impl SheetState {
    /// Creates state `id` named `name` drawn in `color`.
    pub fn new(id: impl Into<String>, name: impl Into<String>, color: Color) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            color,
        }
    }
}

/// Value of a ground property's `marks` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Marks {
    /// A cell keeps the largest mark left on it, `largest`.
    Largest,
    /// A cell adds up the marks left on it, `add_up`.
    AddUp,
}

/// A property's `starts_as` table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StartsAs {
    /// How the property starts.
    pub rule: StartRule,
    /// Key of the start's draws.
    pub key: Option<u32>,
    /// State the ground is painted with before its shapes, for [`StartRule::Painted`].
    pub base: Option<String>,
    /// Ground property an agent starts on, for [`StartRule::OnGround`].
    pub property: Option<String>,
    /// State of [`Self::property`] an agent starts on, for [`StartRule::OnGround`].
    pub state: Option<String>,
    /// Value every element starts with, for [`StartRule::Value`].
    pub value: Slot,
    /// State an element takes when no row of a [`StartRule::ByChance`] start applies.
    pub otherwise: Option<String>,
    /// Rows of a [`StartRule::ByChance`] start, in order.
    pub rows: Vec<StartsAsRow>,
    /// Shapes of a [`StartRule::Painted`] start, painted in order.
    pub shapes: Vec<PaintedShape>,
}

impl StartsAs {
    /// Creates a start of `rule` with no key and nothing filled in.
    pub fn new(rule: StartRule) -> Self {
        Self {
            rule,
            key: None,
            base: None,
            property: None,
            state: None,
            value: Slot::Blank,
            otherwise: None,
            rows: Vec::new(),
            shapes: Vec::new(),
        }
    }
}

/// Value of a start's `rule` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartRule {
    /// Every element starts with one value, `value`.
    Value,
    /// Each element takes a state by chance, `by_chance`.
    ByChance,
    /// The ground is painted with a base state and shapes, `painted`.
    Painted,
    /// Each agent starts anywhere at random, `anywhere`.
    Anywhere,
    /// Each agent starts on a cell of one state of the ground, `on_ground`.
    OnGround,
}

/// One row of a by-chance start, a `[[...starts_as.row]]` table.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct StartsAsRow {
    /// State the row gives.
    pub state: Option<String>,
    /// Chance that an element takes [`Self::state`].
    pub chance: Slot,
}

impl StartsAsRow {
    /// Creates a row giving `state` with chance `chance`.
    pub fn new(state: impl Into<String>, chance: Slot) -> Self {
        Self {
            state: Some(state.into()),
            chance,
        }
    }
}

/// One shape of a painted start, a `[[...starts_as.shape]]` table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PaintedShape {
    /// Kind of shape.
    pub shape: ShapeKind,
    /// State the shape paints.
    pub state: String,
    /// Centre of the shape as fractions of the world's width and height.
    pub center: Option<[f64; 2]>,
    /// Size of the shape as fractions of the world's width and height.
    pub size: Option<[f64; 2]>,
    /// Angle of the shape in degrees.
    pub angle: Option<f64>,
}

impl PaintedShape {
    /// Creates a shape of `shape` painting `state`, with no centre, size or angle.
    pub fn new(shape: ShapeKind, state: impl Into<String>) -> Self {
        Self {
            shape,
            state: state.into(),
            center: None,
            size: None,
            angle: None,
        }
    }
}

/// Value of a painted shape's `shape` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShapeKind {
    /// An ellipse, `ellipse`.
    Ellipse,
    /// A rectangle, `rectangle`.
    Rectangle,
    /// One cell, `cell`.
    Cell,
}

/// One kind of agent, an `[[agents]]` table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AgentKind {
    /// Id of the kind.
    pub id: String,
    /// Name of one agent of the kind.
    pub name: String,
    /// Number of agents at the start.
    pub count: u32,
    /// Largest number of agents the world holds.
    pub max_count: u32,
    /// Each `[[agents.property]]` table.
    pub properties: Vec<SheetProperty>,
}

impl AgentKind {
    /// Creates kind `id` named `name`, with `count` agents and room for `max_count`, and no properties.
    pub fn new(id: impl Into<String>, name: impl Into<String>, count: u32, max_count: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            count,
            max_count,
            properties: Vec::new(),
        }
    }
}

/// The `[ground]` table: the cells under agents.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct SheetGround {
    /// States of ground properties that agents cannot enter.
    pub blocked: Vec<BlockedState>,
    /// Each `[[ground.property]]` table.
    pub properties: Vec<SheetProperty>,
}

/// One state agents cannot enter, an element of the ground's `blocked` list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockedState {
    /// Ground property the state belongs to.
    pub property: String,
    /// State of [`Self::property`].
    pub state: String,
}

impl BlockedState {
    /// Creates the blocked state `state` of ground property `property`.
    pub fn new(property: impl Into<String>, state: impl Into<String>) -> Self {
        Self {
            property: property.into(),
            state: state.into(),
        }
    }
}

/// An `[[event]]` table: something a Record line counts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetEvent {
    /// Id of the event.
    pub id: String,
    /// Name of the event.
    pub name: String,
    /// What the event means, in the author's words.
    pub means: Option<String>,
}

impl SheetEvent {
    /// Creates event `id` named `name`.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            means: None,
        }
    }
}

/// A `[[parameter]]` table: a named number with a slider.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetParameter {
    /// Id of the parameter.
    pub id: String,
    /// Name of the parameter.
    pub name: String,
    /// Type of the parameter's value.
    pub parameter_type: ParameterType,
    /// Smallest value of the slider.
    pub min: f64,
    /// Largest value of the slider.
    pub max: f64,
    /// Value the model keeps.
    pub default: f64,
    /// Step of the slider.
    pub step: Option<f64>,
    /// Unit of the value, free text such as `per day`.
    pub unit: Option<String>,
    /// How the value is shown.
    pub format: Option<ParameterFormat>,
    /// What the parameter means, in the author's words.
    pub means: Option<String>,
}

impl SheetParameter {
    /// Creates parameter `id` named `name` of `parameter_type`, from `min` to `max` and keeping `default`.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        parameter_type: ParameterType,
        min: f64,
        max: f64,
        default: f64,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            parameter_type,
            min,
            max,
            default,
            step: None,
            unit: None,
            format: None,
            means: None,
        }
    }
}

/// Value of a parameter's `type` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParameterType {
    /// A number, `number`.
    Number,
    /// A number with no fraction, `whole_number`.
    WholeNumber,
}

/// Value of a parameter's `format` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParameterFormat {
    /// The value shown as a percentage, `percent`.
    Percent,
}

/// A `[[measure]]` table: a number plotted each tick.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetMeasure {
    /// Id of the measure.
    pub id: String,
    /// Name of the measure, which is also its column in results.
    pub name: String,
    /// Colour of the measure's plot as red, green, blue and alpha bytes.
    pub color: Color,
    /// What the measure counts or sums.
    pub rule: MeasureRule,
    /// Kind of thing the measure is over.
    pub over: Option<String>,
    /// Property a [`MeasureRule::CountInState`] measure reads.
    pub property: Option<String>,
    /// State a [`MeasureRule::CountInState`] measure counts.
    pub state: Option<String>,
    /// Event an [`MeasureRule::EventCount`] measure counts.
    pub event: Option<String>,
    /// Value read for each thing by an average, a total or a spread.
    pub value: Slot,
}

impl SheetMeasure {
    /// Creates measure `id` named `name`, plotted in `color`, of `rule`, with nothing filled in.
    pub fn new(id: impl Into<String>, name: impl Into<String>, color: Color, rule: MeasureRule) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            color,
            rule,
            over: None,
            property: None,
            state: None,
            event: None,
            value: Slot::Blank,
        }
    }
}

/// Value of a measure's `rule` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MeasureRule {
    /// Number of things in one state, `count_in_state`.
    CountInState,
    /// Average of a value over the things, `average`.
    Average,
    /// Total of a value over the things, `total`.
    Total,
    /// Spread of a value over the things, `spread`.
    Spread,
    /// Number of times an event was recorded, `event_count`.
    EventCount,
}

/// A `[color.<thing>]` table: how one kind of thing is coloured.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ColorRule {
    /// How the things are coloured.
    pub rule: ColorRuleKind,
    /// States property whose state colours a thing.
    pub property: Option<String>,
    /// State whose cells are shaded by [`Self::shades`].
    pub shade_state: Option<String>,
    /// Number properties that shade cells of [`Self::shade_state`], each with its colour.
    pub shades: Vec<ColorShade>,
    /// Scale of the shading.
    pub scale: Option<ShadeScale>,
    /// Number of powers of ten a [`ShadeScale::Log`] shading spans.
    pub decades: Option<u32>,
}

impl ColorRule {
    /// Creates a colour rule of `rule` with nothing filled in.
    pub fn new(rule: ColorRuleKind) -> Self {
        Self {
            rule,
            property: None,
            shade_state: None,
            shades: Vec::new(),
            scale: None,
            decades: None,
        }
    }
}

/// Value of a colour rule's `rule` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorRuleKind {
    /// Each thing in the colour of its state, `by_state`.
    ByState,
    /// Each agent in a colour of its heading, `by_heading`.
    ByHeading,
    /// Each thing shaded by a number, `shaded`.
    Shaded,
}

/// One shade of a colour rule, an element of its `shades` list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ColorShade {
    /// Number property the shade reads.
    pub property: String,
    /// Colour at the shade's strongest, as red, green, blue and alpha bytes.
    pub color: Color,
}

impl ColorShade {
    /// Creates a shade of `property` in `color`.
    pub fn new(property: impl Into<String>, color: Color) -> Self {
        Self {
            property: property.into(),
            color,
        }
    }
}

/// Value of a colour rule's `scale` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShadeScale {
    /// Shade in proportion to the value, `linear`.
    Linear,
    /// Shade in proportion to the value's logarithm, `log`.
    Log,
}

/// A `[[button]]` table: a control that runs its steps when pressed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetButton {
    /// Id of the button.
    pub id: String,
    /// Name of the button.
    pub name: String,
    /// Steps the button runs, in order, each a `[[button.does]]` table.
    pub does: Vec<ButtonStep>,
}

impl SheetButton {
    /// Creates button `id` named `name` with no steps.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            does: Vec::new(),
        }
    }
}

/// One step of a button, a `[[button.does]]` table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ButtonStep {
    /// What the step does.
    pub rule: ButtonStepRule,
    /// Kind of thing whose every property starts again.
    pub thing: Option<String>,
    /// Properties that start again.
    pub properties: Vec<String>,
    /// Property a [`ButtonStepRule::SetEverywhere`] step sets.
    pub property: Option<String>,
    /// Card a [`ButtonStepRule::RunOnce`] step runs.
    pub card: Option<String>,
    /// Value a [`ButtonStepRule::SetEverywhere`] step sets.
    pub value: Slot,
}

impl ButtonStep {
    /// Creates a step of `rule` with nothing filled in.
    pub fn new(rule: ButtonStepRule) -> Self {
        Self {
            rule,
            thing: None,
            properties: Vec::new(),
            property: None,
            card: None,
            value: Slot::Blank,
        }
    }
}

/// Value of a button step's `rule` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ButtonStepRule {
    /// Properties start again from their starts, `start_again`.
    StartAgain,
    /// One property takes one value everywhere, `set_everywhere`.
    SetEverywhere,
    /// A card that runs once runs over every thing, `run_once`.
    RunOnce,
}

/// A `[[card]]` table: the lines one kind of thing runs.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetCard {
    /// Id of the card.
    pub id: String,
    /// Kind of thing the card runs over: `cells`, or an agent kind's id.
    pub every: String,
    /// When the card runs.
    pub runs: CardRuns,
    /// Ids of the card's top-level lines, in display order.
    pub lines: Vec<String>,
    /// Every line of the card by line id, the lines inside When and Repeat lines included, each a
    /// `[card.line.<id>]` table.
    pub line_tables: BTreeMap<String, SheetLine>,
}

impl SheetCard {
    /// Creates card `id` over `every` that runs `runs`, with no lines.
    pub fn new(id: impl Into<String>, every: impl Into<String>, runs: CardRuns) -> Self {
        Self {
            id: id.into(),
            every: every.into(),
            runs,
            lines: Vec::new(),
            line_tables: BTreeMap::new(),
        }
    }
}

/// Value of a card's `runs` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CardRuns {
    /// Every tick, `each_tick`.
    EachTick,
    /// Once, when a button runs it, `once`.
    Once,
}

/// A line of a card, or a phrase: an unnamed line inside a slot.
///
/// Every field and slot that any [`LineKind`] holds sits here, and a kind uses some of them. A field the file leaves
/// out is `None` or empty, and a slot it leaves out is [`Slot::Blank`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetLine {
    /// Kind of the line.
    pub kind: LineKind,
    /// Name of the result the line works out.
    pub name: Option<String>,
    /// Key of the line's draws.
    pub key: Option<u32>,
    /// Property the line changes or reads.
    pub property: Option<String>,
    /// State a Become line changes to.
    pub new_state: Option<String>,
    /// State a Become line changes from, `any_state` for any state.
    pub start_state: Option<String>,
    /// Mode of a Steer or Move line.
    pub mode: Option<LineMode>,
    /// Neighbourhood a Count, Look around or Pick a cell line looks at.
    pub around: Option<Around>,
    /// Kind of agent an [`Around::AgentsWithin`] neighbourhood looks at.
    pub agent_kind: Option<String>,
    /// Distance of an [`Around::AgentsWithin`] neighbourhood.
    pub distance: Slot,
    /// Whether an [`Around::AgentsWithin`] neighbourhood counts the agent itself.
    pub include_me: Option<bool>,
    /// Whether a Pick a cell line picks only open cells.
    pub open_only: Option<bool>,
    /// What a Pick a cell line picks.
    pub pick: Option<PickRule>,
    /// Property a Pick a cell line ranks cells by.
    pub ranked_by: Option<String>,
    /// Cell a Look at a cell line looks at.
    pub which: Option<CellWhich>,
    /// Place a Look at a cell line of [`CellWhich::At`] looks at.
    pub place: Slot,
    /// Ground property a Leave line marks.
    pub ground_property: Option<String>,
    /// Event a Record line records.
    pub event: Option<String>,
    /// Test of a Compare line or phrase.
    pub test: Option<CompareTest>,
    /// Left side of a comparison.
    pub left: Slot,
    /// Right side of a comparison.
    pub right: Slot,
    /// Tolerance of a [`CompareTest::Within`] comparison.
    pub tolerance: Slot,
    /// Chance of a Chance line or phrase.
    pub chance: Slot,
    /// Rate of a Rate line or phrase.
    pub rate: Slot,
    /// Unit of time of [`Self::rate`].
    pub rate_unit: Option<TimeUnit>,
    /// Whether [`Self::rate_unit`] came from the length of a tick and was not chosen by hand.
    pub rate_unit_from_tick: bool,
    /// Number of tries of a chance or a rate, "for each" of a count.
    pub tries: Slot,
    /// Lower bound of a random number, or of a [`CompareTest::Outside`] comparison.
    pub low: Slot,
    /// Upper bound of a random number, or of a [`CompareTest::Outside`] comparison.
    pub high: Slot,
    /// Length of a random direction.
    pub length: Slot,
    /// Formula of a Calculate line or phrase.
    pub formula: Option<String>,
    /// Number of times a Repeat line runs its lines.
    pub times: Slot,
    /// Value a line sets, steers by or moves by.
    pub value: Slot,
    /// Strength of a Steer line.
    pub strength: Slot,
    /// Lower bound of a Keep between line.
    pub minimum: Slot,
    /// Upper bound of a Keep between line.
    pub maximum: Slot,
    /// Amount a Leave line leaves.
    pub amount: Slot,
    /// Filter of a Look around or Pick a cell line.
    pub only: Slot,
    /// Test of each neighbour a Count line counts.
    pub whose: Slot,
    /// Condition under which the line applies, `always` for always.
    pub condition: Slot,
    /// Value of a Depending on line for each state, by state id.
    pub branches: BTreeMap<String, SheetValue>,
    /// Summaries of a Look around line.
    pub summaries: Vec<LookSummary>,
    /// Items of an all of, any of or none of.
    pub items: Vec<SheetValue>,
    /// Rows of a Choose line.
    pub rows: Vec<ChooseRow>,
    /// Value of a Choose line when no row applies.
    pub otherwise: Slot,
    /// Ids of the lines inside a When or Repeat line, in display order.
    pub lines: Vec<String>,
    /// The author's comment drawn under the line.
    pub note: Option<String>,
}

impl SheetLine {
    /// Creates a line of `kind` with every field empty and every slot blank.
    pub fn new(kind: LineKind) -> Self {
        Self {
            kind,
            name: None,
            key: None,
            property: None,
            new_state: None,
            start_state: None,
            mode: None,
            around: None,
            agent_kind: None,
            distance: Slot::Blank,
            include_me: None,
            open_only: None,
            pick: None,
            ranked_by: None,
            which: None,
            place: Slot::Blank,
            ground_property: None,
            event: None,
            test: None,
            left: Slot::Blank,
            right: Slot::Blank,
            tolerance: Slot::Blank,
            chance: Slot::Blank,
            rate: Slot::Blank,
            rate_unit: None,
            rate_unit_from_tick: false,
            tries: Slot::Blank,
            low: Slot::Blank,
            high: Slot::Blank,
            length: Slot::Blank,
            formula: None,
            times: Slot::Blank,
            value: Slot::Blank,
            strength: Slot::Blank,
            minimum: Slot::Blank,
            maximum: Slot::Blank,
            amount: Slot::Blank,
            only: Slot::Blank,
            whose: Slot::Blank,
            condition: Slot::Blank,
            branches: BTreeMap::new(),
            summaries: Vec::new(),
            items: Vec::new(),
            rows: Vec::new(),
            otherwise: Slot::Blank,
            lines: Vec::new(),
            note: None,
        }
    }

    /// Returns every value in the slots of the line, the lines inside a block left out.
    pub(crate) fn slot_values(&self) -> impl Iterator<Item = &SheetValue> {
        [
            &self.distance,
            &self.place,
            &self.left,
            &self.right,
            &self.tolerance,
            &self.chance,
            &self.rate,
            &self.tries,
            &self.low,
            &self.high,
            &self.length,
            &self.times,
            &self.value,
            &self.strength,
            &self.minimum,
            &self.maximum,
            &self.amount,
            &self.only,
            &self.whose,
            &self.condition,
            &self.otherwise,
        ]
        .into_iter()
        .filter_map(Slot::value)
        .chain(self.branches.values())
        .chain(self.summaries.iter().filter_map(|summary| summary.value.value()))
        .chain(&self.items)
        .chain(
            self.rows
                .iter()
                .flat_map(|row| [&row.value, &row.condition])
                .filter_map(Slot::value),
        )
    }
}

/// Value of a line's `kind` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineKind {
    /// Counts the neighbours that pass a test, `count`.
    Count,
    /// Summarises the neighbours, `look_around`.
    LookAround,
    /// Picks a neighbouring cell, `pick_cell`.
    PickCell,
    /// Looks at one cell, `look_at_cell`.
    LookAtCell,
    /// Yes with a chance, `chance`.
    Chance,
    /// Yes at a rate per unit of time, `rate`.
    Rate,
    /// A random number between two bounds, `random_number`.
    RandomNumber,
    /// A random direction of a length, `random_direction`.
    RandomDirection,
    /// A comparison, `compare`.
    Compare,
    /// Yes when every item is yes, `all_of`.
    AllOf,
    /// Yes when some item is yes, `any_of`.
    AnyOf,
    /// Yes when no item is yes, `none_of`.
    NoneOf,
    /// The value of the first row that applies, `choose`.
    Choose,
    /// A value for each state of a property, `depending_on`.
    DependingOn,
    /// A formula, `calculate`.
    Calculate,
    /// Changes a States property to a state, `become`.
    Become,
    /// Sets a property to a value, `set`.
    Set,
    /// Steers an arrow or a number, `steer`.
    Steer,
    /// Keeps a property between two bounds, `keep_between`.
    KeepBetween,
    /// Moves an agent, `move`.
    Move,
    /// Leaves a mark on the ground, `leave`.
    Leave,
    /// Records an event, `record`.
    Record,
    /// Runs its lines when a condition holds, `when`.
    When,
    /// Runs its lines a number of times, `repeat`.
    Repeat,
    /// The condition that always holds, `always`.
    Always,
}

/// Value of a Steer or Move line's `mode` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineMode {
    /// Steer or move by a value, `by`.
    By,
    /// Steer towards a value, `towards`.
    Towards,
    /// Move to a place, `to`.
    To,
}

/// Value of a line's `around` key: the neighbourhood it looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Around {
    /// The 8 cells around, `cells_8`.
    Cells8,
    /// The 4 cells beside, `cells_4`.
    Cells4,
    /// The 3 by 3 block, the cell itself included, `block_3x3`.
    Block3x3,
    /// The agents within a distance, `agents_within`.
    AgentsWithin,
}

/// Value of a Pick a cell line's `pick` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PickRule {
    /// The cell with the most of a property, ties at random, `most`.
    Most,
    /// The cell with the least of a property, ties at random, `least`.
    Least,
    /// A cell at random, `random`.
    Random,
}

/// Value of a Look at a cell line's `which` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CellWhich {
    /// The cell an agent is on, `here`.
    Here,
    /// The cell ahead of an agent, `ahead`.
    Ahead,
    /// The cell at a place, `at`.
    At,
}

/// Value of a comparison's `test` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompareTest {
    /// Equal, `is`.
    Is,
    /// Not equal, `is_not`.
    IsNot,
    /// Greater than or equal, `at_least`.
    AtLeast,
    /// Less than or equal, `at_most`.
    AtMost,
    /// Greater than, `more_than`.
    MoreThan,
    /// Less than, `less_than`.
    LessThan,
    /// Equal to one value of a list, `one_of`.
    OneOf,
    /// Equal to no value of a list, `not_one_of`.
    NotOneOf,
    /// Within a tolerance of a value, `within`.
    Within,
    /// Less than a low bound or more than a high bound, `outside`. Only a condition slot holds this test.
    Outside,
}

/// One summary of a Look around line, an element of its `summaries` list.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LookSummary {
    /// Id of the summary, unique within its line.
    pub id: String,
    /// Name of the summary.
    pub name: String,
    /// How the values are summarised.
    pub summary: Option<SummaryKind>,
    /// Value read from each neighbour.
    pub value: Slot,
}

impl LookSummary {
    /// Creates summary `id` named `name`, with nothing filled in.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            summary: None,
            value: Slot::Blank,
        }
    }
}

/// Value of a summary's `summary` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SummaryKind {
    /// Number of neighbours, `count`.
    Count,
    /// Total of the values, `total`.
    Total,
    /// Average of the values, `average`.
    Average,
    /// Largest value, `largest`.
    Largest,
    /// Smallest value, `smallest`.
    Smallest,
    /// Whether some value is yes, `any`.
    Any,
    /// Whether every value is yes, `all`.
    All,
}

/// One row of a Choose line, a `[[...row]]` table.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ChooseRow {
    /// Value the row gives.
    pub value: Slot,
    /// Condition under which the row applies.
    pub condition: Slot,
}

/// A place in a line that takes a value, empty while the author has not filled it.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub enum Slot {
    /// A slot the author has not filled, a key the file leaves out.
    #[default]
    Blank,
    /// A slot holding a value.
    Filled(SheetValue),
}

impl Slot {
    /// Returns whether the slot is blank.
    pub fn is_blank(&self) -> bool {
        matches!(self, Self::Blank)
    }

    /// Value the slot holds, `None` for a blank slot.
    pub fn value(&self) -> Option<&SheetValue> {
        match self {
            Self::Blank => None,
            Self::Filled(value) => Some(value),
        }
    }
}

/// A value in a slot: a literal, a chip, a phrase, or a list of literals and chips.
///
/// A formula is the `formula` field of a [`LineKind::Calculate`] phrase.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SheetValue {
    /// A number, a percent, or yes or no.
    Literal(SheetLiteral),
    /// One thing named by id.
    Chip(SheetChip),
    /// An unnamed line drawn inside the line that holds it.
    Phrase(Box<SheetLine>),
    /// A list of literals and chips, such as the values of [`CompareTest::OneOf`].
    List(Vec<Self>),
}

/// A literal value.
///
/// A percent can stand wherever a number can, and [`Self::number`] returns the number either one stands for.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SheetLiteral {
    /// A number.
    Number(f64),
    /// A number typed as a percent, `{ percent = 30 }`, holding the number as typed (30).
    Percent(f64),
    /// Yes, `true`, or no, `false`.
    Bool(bool),
}

impl SheetLiteral {
    /// Returns the number the literal stands for, `None` for yes or no.
    ///
    /// A percent stands for its number divided by 100, so `{ percent = 30 }` and `0.3` stand for the same number.
    pub fn number(self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(number),
            Self::Percent(percent) => Some(percent / 100.0),
            Self::Bool(_) => None,
        }
    }
}

/// A chip: one thing that a value names by id, an inline table such as `{ param = "infection_rate" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetChip {
    /// A parameter, `{ param = "..." }`.
    Param(String),
    /// The result of a line, `{ result = "l1" }`, one part of it, or a property of the place it names.
    Result {
        /// Id of the line.
        line: String,
        /// Part of the result, such as a summary's id or `amount`.
        part: Option<String>,
        /// Property of the place the line names.
        property: Option<String>,
    },
    /// A property of mine at the start of the tick, `{ my = "..." }`.
    My(String),
    /// A property of mine as the lines above have changed it, `{ my_new = "..." }`.
    MyNew(String),
    /// A property of the neighbour a Count or Look around looks at, `{ their = "..." }`.
    Their(String),
    /// A relation to the neighbour a Look around looks at, `{ relation = "..." }`.
    Relation(ChipRelation),
    /// A state of the property the slot is about, `{ state = "..." }`.
    State(String),
}

/// Value of a relation chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChipRelation {
    /// The offset from me to them, `offset_to_them`.
    OffsetToThem,
    /// The offset from them to me, `offset_away_from_them`.
    OffsetAwayFromThem,
    /// The distance between us, `distance_to_them`.
    DistanceToThem,
}

/// Implements `ALL`, `as_str` and `parse` for an enum whose values a sheet stores by name, and [`StoredName`] with
/// `$what`, the enum's name in a message.
macro_rules! stored_names {
    ($kind:ident, $what:literal { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $kind {
            /// Every value, in the order of the author format.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Returns the value's name in a file.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            /// Returns the value that [`Self::as_str`] names `name`, or `None` for any other text.
            pub fn parse(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.as_str() == name)
            }
        }

        impl StoredName for $kind {
            const WHAT: &'static str = $what;

            fn all() -> &'static [Self] {
                Self::ALL
            }

            fn name(self) -> &'static str {
                self.as_str()
            }
        }
    };
}

/// An enum whose values a sheet stores by name, read by the sheet reader.
pub(crate) trait StoredName: Copy + 'static {
    /// Name of the enum in a message, such as `pick rule`.
    const WHAT: &'static str;

    /// Every value.
    fn all() -> &'static [Self];

    /// Returns the value's name in a file.
    fn name(self) -> &'static str;
}

impl StoredName for TimeUnit {
    const WHAT: &'static str = "time unit";

    fn all() -> &'static [Self] {
        &Self::ALL
    }

    fn name(self) -> &'static str {
        self.as_str()
    }
}

stored_names!(MadeOf, "world kind" { Cells => "cells", Agents => "agents" });
stored_names!(Edges, "edge rule" { Wrap => "wrap", Walls => "walls" });
stored_names!(PropertyType, "property type" {
    States => "states",
    Number => "number",
    WholeNumber => "whole_number",
    YesNo => "yes_no",
    Arrow => "arrow",
    Position => "position",
    Heading => "heading",
});
stored_names!(Marks, "marks rule" { Largest => "largest", AddUp => "add_up" });
stored_names!(StartRule, "start rule" {
    Value => "value",
    ByChance => "by_chance",
    Painted => "painted",
    Anywhere => "anywhere",
    OnGround => "on_ground",
});
stored_names!(ShapeKind, "shape" { Ellipse => "ellipse", Rectangle => "rectangle", Cell => "cell" });
stored_names!(ParameterType, "parameter type" { Number => "number", WholeNumber => "whole_number" });
stored_names!(ParameterFormat, "parameter format" { Percent => "percent" });
stored_names!(MeasureRule, "measure rule" {
    CountInState => "count_in_state",
    Average => "average",
    Total => "total",
    Spread => "spread",
    EventCount => "event_count",
});
stored_names!(ColorRuleKind, "colour rule" { ByState => "by_state", ByHeading => "by_heading", Shaded => "shaded" });
stored_names!(ShadeScale, "shade scale" { Linear => "linear", Log => "log" });
stored_names!(ButtonStepRule, "button step" {
    StartAgain => "start_again",
    SetEverywhere => "set_everywhere",
    RunOnce => "run_once",
});
stored_names!(CardRuns, "card schedule" { EachTick => "each_tick", Once => "once" });
stored_names!(LineKind, "line kind" {
    Count => "count",
    LookAround => "look_around",
    PickCell => "pick_cell",
    LookAtCell => "look_at_cell",
    Chance => "chance",
    Rate => "rate",
    RandomNumber => "random_number",
    RandomDirection => "random_direction",
    Compare => "compare",
    AllOf => "all_of",
    AnyOf => "any_of",
    NoneOf => "none_of",
    Choose => "choose",
    DependingOn => "depending_on",
    Calculate => "calculate",
    Become => "become",
    Set => "set",
    Steer => "steer",
    KeepBetween => "keep_between",
    Move => "move",
    Leave => "leave",
    Record => "record",
    When => "when",
    Repeat => "repeat",
    Always => "always",
});
stored_names!(LineMode, "line mode" { By => "by", Towards => "towards", To => "to" });
stored_names!(Around, "neighbourhood" {
    Cells8 => "cells_8",
    Cells4 => "cells_4",
    Block3x3 => "block_3x3",
    AgentsWithin => "agents_within",
});
stored_names!(PickRule, "pick rule" { Most => "most", Least => "least", Random => "random" });
stored_names!(CellWhich, "cell" { Here => "here", Ahead => "ahead", At => "at" });
stored_names!(CompareTest, "comparison" {
    Is => "is",
    IsNot => "is_not",
    AtLeast => "at_least",
    AtMost => "at_most",
    MoreThan => "more_than",
    LessThan => "less_than",
    OneOf => "one_of",
    NotOneOf => "not_one_of",
    Within => "within",
    Outside => "outside",
});
stored_names!(SummaryKind, "summary" {
    Count => "count",
    Total => "total",
    Average => "average",
    Largest => "largest",
    Smallest => "smallest",
    Any => "any",
    All => "all",
});
stored_names!(ChipRelation, "relation" {
    OffsetToThem => "offset_to_them",
    OffsetAwayFromThem => "offset_away_from_them",
    DistanceToThem => "distance_to_them",
});
