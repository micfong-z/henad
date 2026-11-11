//! Words of the New model, unsaved changes and New parameter dialogs, the top bar, the world tabs and the Diagram.

use super::card::line_count;
use super::grammar::word_list;

// The New model dialog

/// Heading of the New model dialog, and the top bar's button that opens it.
pub const NEW_MODEL: &str = "New model";

/// Label of the starters.
pub const START_FROM: &str = "Start from";

/// Starters with their descriptions, in the order the dialog shows them.
pub const STARTER_CHOICES: [(&str, &str); 9] = [
    ("Blank", "An empty grid of cells to build on."),
    (
        "Game of Life",
        "Conway's Life. A dead cell with 3 live neighbours comes alive.",
    ),
    (
        "SIR epidemic",
        "An epidemic that spreads to neighbours at a rate per day.",
    ),
    ("Majority vote", "Each cell takes the opinion of most of its block."),
    (
        "Forest fire",
        "Trees grow, lightning strikes, and fire spreads to neighbours.",
    ),
    (
        "Random walk",
        "Walkers take random steps and spread evenly across the plane.",
    ),
    ("Boids", "Birds steer by the birds around them and form flocks."),
    (
        "Bounded confidence",
        "People move their opinion towards the opinions close to it.",
    ),
    (
        "Ant foraging",
        "Ants lay trails and find a short path from the nest to food.",
    ),
];

/// Question under Blank only.
pub const WORLD_QUESTION: &str = "What is the world made of?";

/// Answer to [`WORLD_QUESTION`] beside [`CELLS_ON_A_GRID`](super::CELLS_ON_A_GRID).
pub const AGENTS_IN_OPEN_SPACE: &str = "Agents in open space";

/// Name of a new model before the author types one.
pub const UNTITLED_MODEL: &str = "Untitled model";

/// Primary action of the New model dialog.
pub const CREATE: &str = "Create";

// The dialog that asks before a model with unsaved changes is replaced

/// Returns the sentence of the dialog that asks before New model or Open model replaces the models `names`, which
/// hold unsaved changes, such as "Game of Life has unsaved changes."
pub fn unsaved_changes(names: &[String]) -> String {
    let verb = if names.len() == 1 { "has" } else { "have" };
    format!("{} {verb} unsaved changes.", word_list(names))
}

/// Primary action of the unsaved changes dialog, which saves each model before it is replaced.
pub const SAVE_MODEL: &str = "Save model";

/// Action of the unsaved changes dialog that replaces the models without saving them.
pub const DISCARD_CHANGES: &str = "Discard changes";

/// Title of the dialog that asks before a model with unsaved changes is replaced.
pub const UNSAVED_HEADING: &str = "Unsaved changes";

/// Accessible name of the mark beside the name of a model with unsaved changes.
pub const UNSAVED_MARK: &str = "Unsaved changes";

/// Tooltip of the mark beside the name of a model with unsaved changes.
pub const UNSAVED_MARK_TOOLTIP: &str = "Unsaved changes. Press Save model to keep them.";

// The New parameter dialog

/// Heading of the New parameter dialog.
pub const NEW_PARAMETER_HEADING: &str = "New parameter";

/// Lead of the New parameter dialog.
pub const NEW_PARAMETER_LEAD: &str = "A named number with a slider. Lines that use it change as the slider moves.";

/// Accessible name of the parameter's name.
pub const PARAMETER_NAME_FIELD: &str = "Parameter name";

/// Placeholder of the parameter's name when it is for a rate.
pub const RATE_PARAMETER_PLACEHOLDER: &str = "Such as Infection rate";

/// Placeholder of the parameter's name when it is for a chance.
pub const CHANCE_PARAMETER_PLACEHOLDER: &str = "Such as Initial density";

/// Placeholder of the parameter's name when it is for any other number.
pub const NUMBER_PARAMETER_PLACEHOLDER: &str = "Such as Step size";

/// Placeholder of the New parameter dialog's Name field for a number of times.
pub const TIMES_PARAMETER_PLACEHOLDER: &str = "Such as Steps per tick";

/// Label of the parameter's default.
pub const DEFAULT_LABEL: &str = "Default";

/// Accessible name of the parameter's default.
pub const DEFAULT_NAME: &str = "Default value";

/// Label of a parameter's minimum.
pub const MIN_LABEL: &str = "Min";

/// Label of a parameter's maximum.
pub const MAX_LABEL: &str = "Max";

/// Label of a parameter's unit.
pub const UNIT_LABEL: &str = "Unit";

/// Placeholder of the parameter's unit in the dialog.
pub const DIALOG_UNIT_PLACEHOLDER: &str = "Optional, such as per day";

/// Message on a new parameter with no name.
pub const DIALOG_NAME_MISSING: &str = "Type a name.";

/// Message on a default, a minimum or a maximum that does not read.
pub const LIMITS_NOT_NUMBERS: &str = "Default, Min and Max must be numbers.";

/// Check of the New parameter dialog for a number of times whose Default, Min or Max is no integer from 0.
pub const TIMES_LIMITS_NOT_INTEGERS: &str =
    "A number of times is an integer. Make Default, Min and Max integers from 0.";

/// Message on a minimum at or above the maximum.
pub const MIN_NOT_BELOW_MAX: &str = "Min must be below Max.";

/// Message on a default outside the minimum and the maximum.
pub const DEFAULT_OUTSIDE_LIMITS: &str = "Default must be between Min and Max.";

/// Message on a parameter for a chance whose minimum or maximum lies outside 0 to 1.
pub const CHANCE_LIMITS_OUTSIDE: &str = "A chance stays between 0 and 1. Set Min and Max inside that range.";

/// Primary action of the New parameter dialog.
pub const CREATE_PARAMETER: &str = "Create parameter";

// The top bar

/// Returns the accessible name of the button that shows the model's name `name`, such as "Model name: Game of Life.
/// Rename".
pub fn model_name_button(name: &str) -> String {
    format!("Model name: {name}. Rename")
}

/// Accessible name of the field that renames the model.
pub const MODEL_NAME_FIELD: &str = "Model name";

/// Tooltip of the button that shows the model's name.
pub const RENAME_MODEL_TOOLTIP: &str = "Rename model";

/// Button that undoes the last edit.
pub const UNDO: &str = "Undo";

/// Tooltip of [`UNDO`].
pub const UNDO_TOOLTIP: &str = "Undo last edit";

/// Button that redoes the last undone edit.
pub const REDO: &str = "Redo";

/// Tooltip of [`REDO`].
pub const REDO_TOOLTIP: &str = "Redo last undone edit";

/// Segment beside [`DIAGRAM`] that draws the card as its lines, the view that is on while the diagram is off.
pub const LINES_VIEW: &str = "Lines";

/// Toggle that draws the card as a diagram.
pub const DIAGRAM: &str = "Diagram";

/// Tooltip of [`DIAGRAM`].
pub const DIAGRAM_TOOLTIP: &str = "Draw the lines as boxes and arrows";

// The world tabs

/// Returns the tick counter at tick `tick`: "Tick 4 (a tick is one step of time)", or "Tick 4 (4 days)" when the ticks
/// so far last `elapsed`.
pub fn tick_counter(tick: u64, elapsed: Option<&str>) -> String {
    format!("Tick {tick} ({})", elapsed.unwrap_or("a tick is one step of time"))
}

/// Returns the note beside a parameter's slider while the world tries a value and the model keeps `kept`.
pub fn trying_value(kept: &str) -> String {
    format!("Trying this value. The model keeps {kept}.")
}

/// Button that returns a tried parameter to the value the model keeps.
pub const RESET: &str = "Reset";

/// Returns the accessible name of [`RESET`] for `parameter`, such as "Reset Lightning f to 0.0001".
pub fn reset_name(parameter: &str, kept: &str) -> String {
    format!("Reset {parameter} to {kept}")
}

/// Button that makes a tried value the one the model keeps.
pub const KEEP: &str = "Keep";

/// Returns the accessible name of [`KEEP`], such as "Keep 0.0005 for Lightning f".
pub fn keep_name(tried: &str, parameter: &str) -> String {
    format!("Keep {tried} for {parameter}")
}

/// Tooltip of [`KEEP`].
pub const KEEP_TOOLTIP: &str = "Make it the value the model keeps";

/// Text of the plot while the sheet holds no measure.
pub const PLOT_EMPTY: &str = "Tick Plot beside a state under Measures to plot the number of cells in it.";

/// Text of the world while the sheet holds no property.
pub const WORLD_EMPTY: &str = "Add a property on the sheet to see the world.";

// The Diagram

/// Note above the Diagram.
pub const DIAGRAM_NOTE: &str = "Read-only. Edit the lines to change it.";

/// Returns the accessible name of the Diagram of a card holding `count` lines, such as "Diagram of the card's 3
/// lines, read-only".
pub fn diagram_name(count: usize) -> String {
    format!("Diagram of the card's {}, read-only", line_count(count))
}

/// Returns the label of the frame around the lines inside line `number` in the Diagram, such as "Inside line 4".
pub fn inside_line_frame(number: usize) -> String {
    format!("Inside line {number}")
}
