//! Words of the sheet: its sections, the property form, Starts as, parameters, measures, colours, buttons,
//! agents, the ground and events.

use henad_core::metadata::TimeUnit;

use super::card::OTHERWISE;
use super::editor::{MAX_LABEL, MIN_LABEL, UNIT_LABEL};
use super::grammar::word_list;
use crate::sheet::document::MadeOf;

// The sheet

/// One section of the sheet, with its heading and its help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetSection {
    /// Purpose of the model.
    Purpose,
    /// The grid, its edges and the length of one tick.
    World,
    /// The properties every cell has.
    Cells,
    /// The kind of agent of a world of agents: its name, how many there are and the properties every agent has.
    Agents,
    /// The cells under the agents: their properties, how marks left on them combine, and the cells agents cannot
    /// enter.
    Ground,
    /// The things that happen to agents that Record lines count.
    Events,
    /// The named numbers lines can use.
    Parameters,
    /// The numbers drawn on the plot.
    Measures,
    /// The property that colours the world.
    Colours,
    /// The buttons beside the world.
    Buttons,
}

impl SheetSection {
    /// Every section of a world of cells, in the order the sheet shows them.
    pub const ALL: [Self; 7] = [
        Self::Purpose,
        Self::World,
        Self::Cells,
        Self::Parameters,
        Self::Measures,
        Self::Colours,
        Self::Buttons,
    ];

    /// Every section of a world of agents, in the order the sheet shows them.
    pub const AGENT_WORLD: [Self; 9] = [
        Self::Purpose,
        Self::World,
        Self::Agents,
        Self::Ground,
        Self::Events,
        Self::Parameters,
        Self::Measures,
        Self::Colours,
        Self::Buttons,
    ];

    /// Returns the sections of a world made of `made_of`, in the order the sheet shows them.
    pub fn shown(made_of: MadeOf) -> &'static [Self] {
        match made_of {
            MadeOf::Agents => &Self::AGENT_WORLD,
            _ => &Self::ALL,
        }
    }

    /// Heading of the section, such as "Purpose".
    pub fn heading(self) -> &'static str {
        match self {
            Self::Purpose => "Purpose",
            Self::World => "World",
            Self::Cells => "Cells",
            Self::Agents => "Agents",
            Self::Ground => "Ground",
            Self::Events => "Events",
            Self::Parameters => "Parameters",
            Self::Measures => "Measures",
            Self::Colours => "Colours",
            Self::Buttons => "Buttons",
        }
    }

    /// Returns the accessible name of the "?" beside the heading, such as "Help: Purpose".
    pub fn help_name(self) -> String {
        format!("Help: {}", self.heading())
    }

    /// Text of the section's help tooltip.
    pub fn help(self) -> &'static str {
        match self {
            Self::Purpose => "Say in a sentence what the model is for and what to look for when it runs.",
            Self::World => {
                "The grid the cells live on: its size, what happens at its edges, and the length of one tick. A tick \
                 is one step of time, and every cell follows the card once per tick. Leave one tick as one step (no \
                 unit) when time has no unit, as in Game of Life. With a unit, the time so far also shows in the \
                 Playback and Charts tabs. With Wrap around, a cell on the left edge has neighbours on the right edge."
            }
            Self::Cells => {
                "A property is something every cell has. A States property is one of a few named states, such as Dead \
                 or Alive. A Number property holds a number. Under Starts as, set what every cell has when the run \
                 starts."
            }
            Self::Agents => {
                "The agents move in open space. Name them and say how many there are at the start. Every agent has \
                 each property listed here. Under Starts as, set what every agent has when the run starts."
            }
            Self::Ground => {
                "The ground is the grid of cells under the agents. Agents read it and leave marks on its Number \
                 properties. Beside Marks, choose how the marks left on one cell in one tick combine. A share of each \
                 mark can fade every tick. Agents never enter a blocked cell."
            }
            Self::Events => {
                "An event is something that happens to an agent. A Record line counts it each time, and a measure \
                 can plot the count."
            }
            Self::Parameters => {
                "A parameter is a named number with a slider. Lines can use it. The value here is the one the model \
                 keeps. Move the slider beside the world to try other values in the run without changing the model."
            }
            Self::Measures => {
                "A measure is a number worked out from all the cells every tick, drawn on the plot under the world. \
                 Turn on Plot beside a state to count the cells in it."
            }
            Self::Colours => {
                "Choose which property colours the cells in the world. A States property uses the colours of its \
                 states. A Number property is shaded from dark to light."
            }
            Self::Buttons => {
                "A button appears beside the world. Start again redraws how cells start. Run once changes every cell \
                 one time, with its own lines."
            }
        }
    }

    /// Example of the section's help tooltip, shown after [`HELP_EXAMPLE_LABEL`](super::HELP_EXAMPLE_LABEL).
    pub fn help_example(self) -> &'static str {
        match self {
            Self::Purpose => "Spread of a disease through a town. Look for waves of infection.",
            Self::World => "One tick is 1 day",
            Self::Cells => "Status: States Dead, Alive. Starts as Alive with chance 0.3, otherwise Dead",
            Self::Agents => "Walker: 10000 at the start. Position starts anywhere at random",
            Self::Ground => "To food: Number. Marks keep the largest. Each tick keeps 0.999 of it",
            Self::Events => "Delivery: an ant carrying food reaches the Nest",
            Self::Parameters => "Infection rate = 0.3 per day",
            Self::Measures => "Number of cells whose Status is Alive",
            Self::Colours => "Colour cells by Status",
            Self::Buttons => "Clear: Become Dead from any state, always",
        }
    }
}

/// Label of a name field, and the first column of the Cells and Parameters tables.
pub const NAME_LABEL: &str = "Name";

/// Label of the kind of a property.
pub const KIND_LABEL: &str = "Kind";

/// Button that keeps a form.
pub const DONE: &str = "Done";

/// Button that closes a form or a dialog without a change.
pub const CANCEL: &str = "Cancel";

/// Placeholder of the Purpose field, which takes the heading as its accessible name.
pub const PURPOSE_PLACEHOLDER: &str = "What is this model for?";

/// Name of a world of cells, in the World section and in the New model dialog.
pub const CELLS_ON_A_GRID: &str = "Cells on a grid";

/// Label of the size of the grid.
pub const SIZE_LABEL: &str = "Size";

/// Word between the width and the height of the grid.
pub const SIZE_BY: &str = "by";

/// Accessible name of the width field.
pub const WIDTH_NAME: &str = "Width in cells";

/// Accessible name of the height field.
pub const HEIGHT_NAME: &str = "Height in cells";

/// Label of the edges of the grid.
pub const EDGES_LABEL: &str = "Edges";

/// Edges that join the opposite edge.
pub const WRAP_AROUND: &str = "Wrap around";

/// Edges that end the grid.
pub const WALLS: &str = "Walls";

/// Sentence under the edges while they wrap around.
pub const WRAP_SENTENCE: &str = "The left edge joins the right edge, and the top edge joins the bottom.";

/// Sentence under the edges while they are walls, and the tooltip of [`WALLS`].
pub const WALLS_SENTENCE: &str = "Cells on an edge have fewer cells around them.";

/// Sentence under the edges of a world of agents while they are walls, and the tooltip of [`WALLS`] there.
pub const AGENT_WALLS_SENTENCE: &str = "An agent near an edge has fewer agents around it.";

/// Tooltip of [`WRAP_AROUND`] in a world of agents.
pub const AGENT_WRAP_TOOLTIP: &str =
    "An agent that leaves by one edge comes back at the opposite edge, and agents near opposite edges are neighbours.";

/// Help of the World section of a world of agents, in place of [`SheetSection::help`].
pub const WORLD_HELP_AGENTS: &str = "The open space the agents live in: its size, what happens at its edges, and the \
                                     length of one tick. A tick is one step of time, and every agent follows its card \
                                     once per tick. Leave one tick as one step (no unit) when time has no unit. With a \
                                     unit, the time so far also shows in the Playback and Charts tabs. With Wrap \
                                     around, an agent that leaves by one edge comes back at the opposite edge.";

/// Tooltip of [`WRAP_AROUND`].
pub const WRAP_TOOLTIP: &str =
    "Cells on the left edge are neighbours of cells on the right edge, and the top edge joins the bottom.";

/// Label before the length of one tick.
pub const ONE_TICK_IS: &str = "One tick is";

/// Returns the choice of the tick's unit: "one step (no unit)" for no unit, or the unit's plural, such as "days".
pub fn tick_unit_choice(unit: Option<TimeUnit>) -> &'static str {
    unit.map_or("one step (no unit)", TimeUnit::plural)
}

/// Accessible name of the length of one tick, hidden while the tick has no unit.
pub const TICK_LENGTH_NAME: &str = "Length of one tick";

/// Message under the length of one tick typed as 0 or less.
pub const TICK_LENGTH_NOT_POSITIVE: &str = "The length of one tick must be greater than 0.";

/// Accessible name of the unit of one tick.
pub const TICK_UNIT_NAME: &str = "Unit of one tick";

/// Columns of the Cells table.
pub const CELLS_COLUMNS: [&str; 4] = [NAME_LABEL, KIND_LABEL, "Starts as", "Changed by"];

/// Returns the Changed by column of a property that the lines `lines` change: "lines 2, 3", "line 2" or "no line
/// yet".
pub fn changed_by(lines: &[usize]) -> String {
    let numbers: Vec<String> = lines.iter().map(usize::to_string).collect();
    match lines.len() {
        0 => "no line yet".to_owned(),
        1 => format!("line {}", numbers.join(", ")),
        _ => format!("lines {}", numbers.join(", ")),
    }
}

/// Text of the Cells section of a sheet with no property, above [`ADD_PROPERTY`] as the primary action.
pub const SHEET_EMPTY: &str = "Every cell has the properties listed here. Add the first one to start.";

/// Notice that a change to the sheet after the first tick starts the world again.
///
/// The Parameters tab shows it as the tooltip of a slider whose tried value rebuilds the preview at tick 0.
pub const RESTART_NOTICE: &str = "Changes restart the world from tick 0.";

/// Notice while the world runs the sheet as it was before an edit that stops the sheet building.
pub const STALE_WORLD_NOTICE: &str =
    "The world shows the model before the last change. Fix the problems in red to update it.";

/// Text of a sheet's preview while it waits for the run of another model, which Offload in the Playback tab ends.
pub const PREVIEW_WAITS: &str =
    "Preview waits for the running model. Press Offload in the Playback tab to preview this model.";

// The property form

/// Button that opens the property form.
pub const ADD_PROPERTY: &str = "+ Add property";

/// Heading of the property form.
pub const NEW_PROPERTY: &str = "New property";

/// Placeholder of the property's name.
pub const PROPERTY_NAME_PLACEHOLDER: &str = "Such as Status or Health";

/// Accessible name of the property's name.
pub const PROPERTY_NAME_FIELD: &str = "Property name";

/// Kind of a property that holds one of a few named states.
pub const STATES_KIND: &str = "States";

/// Kind of a property that holds a number.
pub const NUMBER_KIND: &str = "Number";

/// Description of [`STATES_KIND`].
pub const STATES_KIND_DESCRIPTION: &str =
    "One of a few named states, such as Dead and Alive. Give it at least two states.";

/// Description of [`NUMBER_KIND`].
pub const NUMBER_KIND_DESCRIPTION: &str = "A number, such as an age or an amount of food.";

/// Kind of a Number property that holds whole numbers only.
pub const INTEGER_KIND: &str = "Integer";

/// Tooltip of [`INTEGER_KIND`].
pub const INTEGER_KIND_TOOLTIP: &str = "A number with no fraction, such as 3 or −2.";

/// Button that adds a state to the form.
pub const ADD_STATE: &str = "+ Add state";

/// Returns the accessible name of the colour button of state `number`, such as "State 1 colour: Dark grey".
pub fn state_color_name(number: usize, color: &str) -> String {
    format!("State {number} colour: {color}")
}

/// Returns the accessible name of the name field of state `number`, such as "State 1 name".
pub fn state_name_field(number: usize) -> String {
    format!("State {number} name")
}

/// Returns the accessible name of the button that removes state `number`, such as "Remove state 1".
pub fn remove_state_name(number: usize) -> String {
    format!("Remove state {number}")
}

/// Message on a property with no name.
pub const PROPERTY_NAME_MISSING: &str = "Type a name for the property.";

/// Returns the message on a property named `name`, which another property holds.
pub fn property_name_taken(name: &str) -> String {
    format!("A property called {name} already exists.")
}

/// Returns the message on a new property, parameter, event or button named `name`, a name Henad keeps for itself,
/// such as "Count is a reserved name. Choose another name."
pub fn name_reserved(name: &str) -> String {
    format!("{name} is a reserved name. Choose another name.")
}

/// Message on a refused edit whose name Henad keeps for itself, where the name is not at hand.
pub const NAME_RESERVED: &str = "That name is reserved. Choose another name.";

/// Message on a States property with fewer than two states.
pub const TOO_FEW_STATES: &str = "Give the property at least two states.";

/// Message on a States property with two states of one name.
pub const STATE_NAMES_REPEATED: &str = "Two states have the same name. Give each state its own name.";

/// Returns the heading of the form that edits the property `property`, such as "Edit property Status".
///
/// The words also name the Edit button of the property's row in a table of properties, and the Edit item of the
/// row's menu.
pub fn edit_property_heading(property: &str) -> String {
    format!("Edit property {property}")
}

/// Returns the accessible name of the button that deletes the property `property` from its row in a table of
/// properties, such as "Delete property Status".
///
/// The words are also the Delete item of the row's menu.
pub fn delete_property_name(property: &str) -> String {
    format!("Delete property {property}")
}

/// Returns the accessible name of the menu of the row of the property `property` in a table of properties too narrow
/// for its Edit and delete buttons, such as "Property Status actions".
pub fn property_actions_name(property: &str) -> String {
    format!("Property {property} actions")
}

/// Button that deletes the property being edited.
pub const DELETE_PROPERTY: &str = "Delete property";

// Starts as

/// Returns the choice of Starts as where every cell starts in `state`, such as "All Dead".
pub fn starts_all(state: &str) -> String {
    format!("All {state}")
}

/// Choice of Starts as where a state is drawn by chance.
pub const BY_CHANCE: &str = "By chance";

/// Words between the state and the chance of a row of Starts as.
pub const WITH_CHANCE: &str = "with chance";

/// Label of the button that removes a row of Starts as.
pub const REMOVE_ROW_LABEL: &str = "×";

/// Button that adds a row to Starts as.
pub const ADD_ROW: &str = "+ Add row";

/// Returns the accessible name of [`ADD_ROW`] for `property`, such as "Add row to Status starts as".
pub fn add_row_name(property: &str) -> String {
    format!("Add row to {property} starts as")
}

/// Returns the accessible name of the button that removes row `row`, such as "Remove row 1 of Status starts as".
pub fn remove_row_name(row: usize, property: &str) -> String {
    format!("Remove row {row} of {property} starts as")
}

/// Returns the accessible name of the state of the last row of Starts as, such as "Status starts as, otherwise".
pub fn starts_otherwise_name(property: &str) -> String {
    format!("{property} starts as, {OTHERWISE}")
}

/// Choice of Starts as where every cell starts at one number.
pub const A_VALUE: &str = "A value";

/// Choice of Starts as where every cell draws a number.
pub const A_RANDOM_NUMBER: &str = "A random number";

/// Words before the low end of [`A_RANDOM_NUMBER`].
pub const RANDOM_NUMBER_BETWEEN: &str = "A random number between";

/// Word between the low end and the high end of [`A_RANDOM_NUMBER`].
pub const RANDOM_NUMBER_AND: &str = "and";

/// One control of Starts as that takes an accessible name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartsField {
    /// The state of a row.
    State,
    /// The chance of a row.
    Chance,
    /// A number.
    Number,
    /// The "Use a parameter" button beside a field.
    UseParameter,
}

impl StartsField {
    /// Word of the control after its place, such as "chance".
    pub fn word(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Chance => "chance",
            Self::Number => "number",
            Self::UseParameter => "use a parameter",
        }
    }
}

/// Returns the place of row `row` of the Starts as of `property`, such as "Status starts as, row 1".
pub fn starts_row_place(property: &str, row: usize) -> String {
    format!("{property} starts as, row {row}")
}

/// Returns the place of the one number of the Starts as of `property`, such as "Food starts as".
pub fn starts_place(property: &str) -> String {
    format!("{property} starts as")
}

/// Returns the place of the low end of the Starts as of `property`, such as "Food starts as, low end".
pub fn starts_low_place(property: &str) -> String {
    format!("{property} starts as, low end")
}

/// Returns the place of the high end of the Starts as of `property`, such as "Food starts as, high end".
pub fn starts_high_place(property: &str) -> String {
    format!("{property} starts as, high end")
}

/// Returns the accessible name of a control of Starts as at `place`, such as "Status starts as, row 1: chance".
pub fn starts_field_name(place: &str, field: StartsField) -> String {
    format!("{place}: {}", field.word())
}

/// Message under a chance of Starts as that does not read.
pub const TYPE_A_CHANCE: &str = "Type a chance such as 0.3 or 30%.";

/// Button beside a field of Starts as that opens [`CHOOSE_A_PARAMETER`].
pub const USE_A_PARAMETER: &str = "Use a parameter";

/// Title of the picker of parameters for a field of Starts as.
pub const CHOOSE_A_PARAMETER: &str = "Choose a parameter";

/// Note on Starts as once the world is past its first tick. Playback's Build then builds the world again from tick 0
/// with the changed start.
pub const START_AGAIN_TO_SEE: &str = "Press Build in the Playback tab to see the change.";

// Parameters on the sheet

/// Columns of the Parameters table.
pub const PARAMETER_COLUMNS: [&str; 5] = [NAME_LABEL, "Value", MIN_LABEL, MAX_LABEL, UNIT_LABEL];

/// Returns the accessible name of the name field of the parameter `parameter`, such as "Name of Infection rate".
pub fn sheet_parameter_name_field(parameter: &str) -> String {
    format!("Name of {parameter}")
}

/// Returns the accessible name of the slider of `parameter`, such as "Infection rate value slider".
pub fn value_slider_name(parameter: &str) -> String {
    format!("{parameter} value slider")
}

/// Returns the accessible name of the value field of `parameter`, such as "Infection rate value".
pub fn value_field_name(parameter: &str) -> String {
    format!("{parameter} value")
}

/// Returns the accessible name of the button that deletes `parameter`, such as "Delete parameter Infection rate".
pub fn delete_parameter_name(parameter: &str) -> String {
    format!("Delete parameter {parameter}")
}

/// Returns the accessible name of the minimum of `parameter`, such as "Infection rate min".
pub fn parameter_min_name(parameter: &str) -> String {
    format!("{parameter} min")
}

/// Returns the accessible name of the maximum of `parameter`, such as "Infection rate max".
pub fn parameter_max_name(parameter: &str) -> String {
    format!("{parameter} max")
}

/// Returns the accessible name of the unit of `parameter`, such as "Infection rate unit".
pub fn parameter_unit_name(parameter: &str) -> String {
    format!("{parameter} unit")
}

/// Placeholder of a parameter's unit on the sheet.
pub const UNIT_FIELD_PLACEHOLDER: &str = "none";

/// Returns the suggestion for a parameter's unit in `unit`, such as "per day".
pub fn unit_suggestion(unit: TimeUnit) -> String {
    format!("per {}", unit.as_str())
}

/// Message on a parameter of the sheet with no name.
pub const PARAMETER_NAME_MISSING: &str = "Type a name for the parameter.";

/// Returns the message on a parameter named `name`, which another parameter holds.
pub fn parameter_name_taken(name: &str) -> String {
    format!("A parameter called {name} already exists.")
}

/// Returns the line of the sheet while the world tries the value `tried` and the model keeps `kept`.
pub fn sheet_trying_value(tried: &str, kept: &str) -> String {
    format!("The world is trying {tried}. The model keeps {kept}.")
}

/// Button that adds a parameter on the sheet.
pub const ADD_PARAMETER: &str = "+ Add parameter";

/// Returns the suggestions for a parameter's unit, "per second" to "per year".
pub fn unit_suggestions() -> Vec<String> {
    TimeUnit::ALL.iter().map(|unit| unit_suggestion(*unit)).collect()
}

// Measures

/// Returns the label of the box that plots the number of cells in `state`, such as "Plot Alive".
pub fn plot_state(state: &str) -> String {
    format!("Plot {state}")
}

/// Label of the box beside a state that plots the number of cells in it.
pub const PLOT: &str = "Plot";

/// Button that opens the measure form.
pub const ADD_MEASURE: &str = "+ Add measure";

/// Heading of the measure form.
pub const NEW_MEASURE: &str = "New measure";

/// Kind of a measure that counts the cells in a state.
pub const COUNT_MEASURE: &str = "Number of cells in a state";

/// Returns the words of a measure that counts the cells whose `property` is `state`, such as "Number of cells whose
/// Status is Alive".
pub fn count_measure(property: &str, state: &str) -> String {
    format!("Number of cells whose {property} is {state}")
}

/// Kind of a measure that averages a number property.
pub const AVERAGE_MEASURE: &str = "Average of a number";

/// Words before the property of a measure that counts the cells in a state.
pub const COUNT_MEASURE_LEAD: &str = "Number of cells whose";

/// Word between the property and the state of a measure that counts the cells in a state.
pub const COUNT_MEASURE_IS: &str = "is";

/// Words before the property of a measure that averages a number property.
pub const AVERAGE_MEASURE_LEAD: &str = "Average of";

/// Accessible name of the property of a new measure.
pub const MEASURE_PROPERTY_NAME: &str = "Measure property";

/// Accessible name of the state of a new measure.
pub const MEASURE_STATE_NAME: &str = "Measure state";

/// Words of the property select of a new measure before a property is chosen.
pub const MEASURE_PROPERTY_EMPTY: &str = "property";

/// Words of the state select of a new measure before a state is chosen.
pub const MEASURE_STATE_EMPTY: &str = "state";

/// Words of the property select of a new average before a number property is chosen.
pub const MEASURE_NUMBER_PROPERTY_EMPTY: &str = "number property";

/// Returns the words of a measure that averages `property`, such as "Average of Food".
pub fn average_measure(property: &str) -> String {
    format!("Average of {property}")
}

/// Heading of the list of measures.
pub const PLOTTED: &str = "Plotted";

/// Returns the accessible name of the colour button of the measure `measure`, such as "Colour of Alive: Blue".
pub fn measure_color_name(measure: &str, color: &str) -> String {
    color_button_name(measure, color)
}

/// Returns the accessible name of the button that sets the colour of `subject`, a measure or a state, to one of the
/// palette, such as "Colour of recovered: Green".
pub fn color_button_name(subject: &str, color: &str) -> String {
    format!("Colour of {subject}: {color}")
}

/// Returns the accessible name of the swatch under Colours that sets the colour of the state `state` of `property`,
/// such as "Colour of Status Alive: Green".
///
/// The property's name keeps it apart from a measure named as the state.
pub fn state_swatch_name(property: &str, state: &str, color: &str) -> String {
    color_button_name(&format!("{property} {state}"), color)
}

/// Returns the accessible name of the button that removes `measure`, such as "Remove measure Alive".
pub fn remove_measure_name(measure: &str) -> String {
    format!("Remove measure {measure}")
}

/// Message on a count measure with no property or no state.
pub const MEASURE_NEEDS_STATE: &str = "Choose a property and a state.";

/// Message on an average measure with no property.
pub const MEASURE_NEEDS_NUMBER_PROPERTY: &str = "Choose a Number property.";

/// Message on a measure that is already on the plot.
pub const MEASURE_ALREADY_PLOTTED: &str = "That measure is already plotted.";

/// Kind of a measure that counts the times an event happened, in a world of agents.
pub const EVENT_COUNT_MEASURE: &str = "Number of times an event happened";

/// Words before the event of a measure that counts the times an event happened.
pub const EVENT_COUNT_LEAD: &str = "Number of times";

/// Words after the event of a measure that counts the times an event happened.
pub const EVENT_COUNT_TAIL: &str = "happened";

/// Accessible name of the event of a new measure.
pub const MEASURE_EVENT_NAME: &str = "Measure event";

/// Words of the event select of a new measure before an event is chosen.
pub const MEASURE_EVENT_EMPTY: &str = "event";

/// Message on an event count measure with no event.
pub const MEASURE_NEEDS_EVENT: &str = "Choose an event.";

/// Returns the words of a measure that counts the times `event` happened since the start, such as "Number of times
/// Delivery happened".
pub fn event_count_measure(event: &str) -> String {
    format!("{EVENT_COUNT_LEAD} {event} {EVENT_COUNT_TAIL}")
}

/// Returns the name a new measure that counts the times `event` happened takes, such as "Delivery count".
pub fn event_count_name(event: &str) -> String {
    format!("{event} count")
}

// Colours

/// Words before the property that colours the world.
pub const COLOR_CELLS_BY: &str = "Colour cells by";

/// Label of the shade of a Number property.
pub const SHADE: &str = "Shade";

/// Shades of a Number property, in the order the select shows them.
pub const SHADE_COLORS: [&str; 7] = ["Grey", "Red", "Orange", "Yellow", "Green", "Blue", "Purple"];

/// Word before the value a Number property is drawn darkest at.
pub const SHADE_FROM: &str = "from";

/// Words between the value drawn darkest and the value drawn lightest.
pub const SHADE_DARK_TO: &str = "(dark) to";

/// Word after the value drawn lightest.
pub const SHADE_LIGHT: &str = "(light)";

/// Accessible name of the value drawn darkest.
pub const DARKEST_NAME: &str = "Darkest at value";

/// Accessible name of the value drawn lightest.
pub const LIGHTEST_NAME: &str = "Lightest at value";

/// Message on a value drawn darkest that equals the value drawn lightest.
pub const SHADE_ENDS_EQUAL: &str = "Darkest and lightest need different values.";

// Buttons on the sheet

/// Button that opens the button form.
pub const ADD_BUTTON: &str = "+ Add button";

/// Placeholder of a button's name.
pub const BUTTON_NAME_PLACEHOLDER: &str = "Such as Clear";

/// Accessible name of a button's name.
pub const BUTTON_NAME_FIELD: &str = "Button name";

/// Heading of the button form for a new button.
pub const NEW_BUTTON: &str = "New button";

/// Label of the button that opens the button form for a button.
pub const EDIT: &str = "Edit";

/// Label of what a button does.
pub const BUTTON_DOES: &str = "Does";

/// Action of a button that redraws how cells start, and the world's button of the same name.
pub const START_AGAIN: &str = "Start again";

/// Choice of [`START_AGAIN`] that redraws every property.
pub const EVERY_PROPERTY: &str = "every property";

/// Accessible name of the property [`START_AGAIN`] redraws.
pub const START_AGAIN_FOR: &str = "Start again for";

/// Returns what a button does that starts `target` again, such as "Start again: Status" or "Start again: every
/// property".
pub fn start_again_for(target: &str) -> String {
    format!("{START_AGAIN}: {target}")
}

/// Returns what a button does that runs its own lines once over `thing`, such as "Run once, every cell".
pub fn run_once(thing: &str) -> String {
    format!("Run once, every {thing}")
}

/// Returns the accessible name of the button that edits the button `button`, such as "Edit button Clear".
pub fn edit_button_name(button: &str) -> String {
    format!("Edit button {button}")
}

/// Returns the accessible name of the button that deletes the button `button`, such as "Delete button Clear".
pub fn delete_button_name(button: &str) -> String {
    format!("Delete button {button}")
}

/// Message on a button with no name.
pub const BUTTON_NAME_MISSING: &str = "Type a name for the button.";

/// Returns the message on a button named `name`, which another button holds.
pub fn button_name_taken(name: &str) -> String {
    format!("A button called {name} already exists.")
}

// Agents, the ground and events on the sheet

/// Accessible name of the name of the kind of agent.
pub const AGENT_NAME_FIELD: &str = "Name of the agents";

/// Label of the number of agents at the start.
pub const AGENT_COUNT_LABEL: &str = "Number at the start";

/// Accessible name of the number of agents at the start.
pub const AGENT_COUNT_NAME: &str = "Number of agents at the start";

/// Label of the largest number of agents the world holds.
pub const AGENT_MAX_LABEL: &str = "At most";

/// Accessible name of the largest number of agents the world holds.
pub const AGENT_MAX_NAME: &str = "Maximum number of agents";

/// Tooltip of the largest number of agents the world holds.
pub const AGENT_MAX_TOOLTIP: &str = "The world has room for this many agents.";

/// Message on a kind of agent with no name.
pub const AGENT_NAME_MISSING: &str = "Type a name for the agents.";

/// Returns the message on a number of agents that is no integer from 1 to `most`, such as "Type an integer from 1 to
/// 1000000.".
pub fn integer_up_to(most: u32) -> String {
    format!("Type an integer from 1 to {most}.")
}

/// Name of the kind of agent of a blank model of agents.
pub const BLANK_AGENT_NAME: &str = "Agent";

/// Returns the accessible name of [`ADD_PROPERTY`] under a kind of thing named `thing`, such as "Add property to
/// Walker".
pub fn add_property_name(thing: &str) -> String {
    format!("Add property to {thing}")
}

/// Accessible name of [`ADD_PROPERTY`] under the ground.
pub const ADD_GROUND_PROPERTY_NAME: &str = "Add property to the ground";

/// Kind of an agent's place in the world.
pub const POSITION_KIND: &str = "Position";

/// Kind of a value that is a cell of the grid or the ground, as a Move to takes.
pub const CELL_KIND: &str = "Cell";

/// Returns the accessible name of the symbol of the type `kind` before a name, such as "Type: Vector", where `kind`
/// is the type's name.
pub fn type_symbol_name(kind: &str) -> String {
    format!("Type: {kind}")
}

/// Kind of an agent's direction of travel.
pub const HEADING_KIND: &str = "Heading";

/// Kind of a property that holds yes or no, a Boolean.
pub const YES_NO_KIND: &str = "Boolean";

/// Kind of a property that holds a vector in the plane.
pub const ARROW_KIND: &str = "Vector";

/// Description of [`ARROW_KIND`].
pub const ARROW_KIND_DESCRIPTION: &str =
    "A direction with a length, such as a velocity. It starts in a random direction of length 1.";

/// Description of [`POSITION_KIND`].
pub const POSITION_KIND_DESCRIPTION: &str = "Where the agent is in the world.";

/// Description of [`HEADING_KIND`].
pub const HEADING_KIND_DESCRIPTION: &str = "The direction the agent last moved in.";

/// Description of [`YES_NO_KIND`].
pub const YES_NO_KIND_DESCRIPTION: &str = "A value that is yes or no, such as whether an agent is hungry.";

/// Value yes of a Boolean property.
pub const YES: &str = "Yes";

/// Value no of a Boolean property.
pub const NO: &str = "No";

/// Choice of Starts as where every agent starts at a random place.
pub const ANYWHERE_AT_RANDOM: &str = "Anywhere at random";

/// Returns the choice of Starts as where every agent starts on a cell whose ground `property` is `state`, such as "On
/// a cell whose Ground is Nest".
pub fn on_ground_start(property: &str, state: &str) -> String {
    format!("On a cell whose {property} is {state}")
}

/// Starts as of a Heading, which takes its first value from the first move.
pub const HEADING_START: &str = "None until the first move";

/// Choice of Starts as of a ground property painted with shapes in the file.
pub const PAINTED: &str = "Painted";

/// Returns the note under a painted start, painted `base` with `shapes` shapes on it, such as "Open, with 4 shapes
/// painted on it".
pub fn painted_shapes(base: &str, shapes: usize) -> String {
    match shapes {
        1 => format!("{base}, with 1 shape painted on it"),
        _ => format!("{base}, with {shapes} shapes painted on it"),
    }
}

/// Returns the words before the number of [`A_VALUE`] for a kind of thing called `thing`, such as "Every agent starts
/// at".
pub fn every_starts_at(thing: &str) -> String {
    format!("Every {thing} starts at")
}

/// Text of the Ground section with no property, above the button that adds one.
pub const GROUND_EMPTY: &str =
    "The cells under the agents. Add a property to give the agents a trail to leave or cells to avoid.";

/// Label of how marks left on one cell in one tick combine.
pub const MARKS_LABEL: &str = "Marks";

/// Tooltip of [`MARKS_LABEL`], with the bound of [`MARKS_ADD_UP`].
pub const MARKS_TOOLTIP: &str = "What a cell keeps when several agents leave marks on it in one tick. With Add up, a \
                                 cell adds up at most 65536 in a tick, in steps of 2^−16.";

/// Choice of [`MARKS_LABEL`] for a property agents leave nothing on.
pub const MARKS_NONE: &str = "Not marked";

/// Choice of [`MARKS_LABEL`] where a cell keeps the largest mark.
pub const MARKS_LARGEST: &str = "Keep the largest";

/// Choice of [`MARKS_LABEL`] where a cell adds up the marks.
pub const MARKS_ADD_UP: &str = "Add up";

/// Returns the accessible name of the marks rule of `property`, such as "To food marks".
pub fn marks_name(property: &str) -> String {
    format!("{property} marks")
}

/// Words before the share of a mark left after each tick.
pub const KEPT_LEAD: &str = "Each tick keeps";

/// Words after the share of a mark left after each tick.
pub const KEPT_TAIL: &str = "of it";

/// Returns the place of the share of `property` left after each tick, such as "To food, share kept each tick".
pub fn kept_place(property: &str) -> String {
    format!("{property}, share kept each tick")
}

/// Message on a share kept each tick outside 0 to 1.
pub const KEPT_OUT_OF_RANGE: &str = "Type a share from 0 to 1, such as 0.99.";

/// Returns the warning on the share `share` kept each tick of the ground property `property`.
///
/// `share` is greater than 1 when `grows` holds, and less than 0 otherwise. Each tick multiplies the property by the
/// share as it is. For example "Each tick multiplies To food by 5, a share greater than 1, so its marks grow every
/// tick."
pub fn kept_outside_note(property: &str, share: &str, grows: bool) -> String {
    if grows {
        format!("Each tick multiplies {property} by {share}, a share greater than 1, so its marks grow every tick.")
    } else {
        format!("Each tick multiplies {property} by {share}, a share less than 0.")
    }
}

/// Returns the warning on the parameter `parameter` used as the share kept each tick of the ground property `property`.
///
/// `limit` is the parameter's Max, greater than 1, when `grows` holds, and its Min, less than 0, otherwise. For example
/// "Trail kept per tick has a Max of 2, a share greater than 1. Each tick multiplies To food by it, so its marks can
/// grow every tick."
pub fn kept_limits_outside_note(parameter: &str, property: &str, limit: &str, grows: bool) -> String {
    if grows {
        format!(
            "{parameter} has a Max of {limit}, a share greater than 1. Each tick multiplies {property} by it, so its \
             marks can grow every tick."
        )
    } else {
        format!("{parameter} has a Min of {limit}, a share less than 0. Each tick multiplies {property} by it.")
    }
}

/// Returns the words before the states of `property` that agents cannot enter, such as "Agents cannot enter cells
/// whose Ground is".
pub fn blocked_lead(property: &str) -> String {
    format!("Agents cannot enter cells whose {property} is")
}

/// Returns the accessible name of the box that keeps agents out of `state`, such as "Agents cannot enter Obstacle".
pub fn blocked_state_name(state: &str) -> String {
    format!("Agents cannot enter {state}")
}

/// Text of the Events section with no event.
pub const EVENTS_EMPTY: &str = "No events yet. Add an event for a Record line to count.";

/// Button that opens the event form.
pub const ADD_EVENT: &str = "+ Add event";

/// Heading of the event form.
pub const NEW_EVENT: &str = "New event";

/// Placeholder of an event's name.
pub const EVENT_NAME_PLACEHOLDER: &str = "Such as Delivery";

/// Accessible name of a new event's name.
pub const EVENT_NAME_FIELD: &str = "Event name";

/// Message on an event with no name.
pub const EVENT_NAME_MISSING: &str = "Type a name for the event.";

/// Returns the message on an event named `name`, which another event holds.
pub fn event_name_taken(name: &str) -> String {
    format!("An event called {name} already exists.")
}

/// Returns the accessible name of the name field of the event `event`, such as "Name of event Delivery".
pub fn event_name_name(event: &str) -> String {
    format!("Name of event {event}")
}

/// Returns the accessible name of the button that deletes the event `event`, such as "Delete event Delivery".
pub fn delete_event_name(event: &str) -> String {
    format!("Delete event {event}")
}

/// Words before the property that colours the agents.
pub const COLOR_AGENTS_BY: &str = "Colour agents by";

/// Words before the property that colours the ground.
pub const COLOR_GROUND_BY: &str = "Colour ground by";

/// Note under agents coloured by their heading.
pub const BY_HEADING_NOTE: &str = "Each agent takes one of eight hues by the direction it last moved in.";

/// Option of a Colours select that colours by no property: the cells drawn black, the agents white, and the ground not
/// drawn.
pub const COLOR_BY_NOTHING: &str = "None";

/// Kind of a measure that counts the agents in a state.
pub const COUNT_AGENTS_MEASURE: &str = "Number of agents in a state";

/// Words before the property of a measure that counts the agents in a state.
pub const COUNT_AGENTS_LEAD: &str = "Number of agents whose";

/// Returns the words of a measure that counts the agents whose `property` is `state`, such as "Number of agents whose
/// Cargo is Searching".
pub fn count_agents_measure(property: &str, state: &str) -> String {
    format!("Number of agents whose {property} is {state}")
}

/// Returns what a button does that sets the ground `properties` to `value` everywhere, such as "Set To food and To
/// home to 0 everywhere".
pub fn set_everywhere(properties: &[String], value: &str) -> String {
    format!("Set {} to {value} everywhere", word_list(properties))
}
