//! Words of the New model, unsaved changes, replace, delete and New parameter dialogs, the top bar, the world tabs and
//! the Diagram.

use super::card::line_count;
use super::grammar::{number_list, upper_first, word_list};

// The New model dialog

/// Heading of the New model dialog, and the top bar's button that opens it.
pub const NEW_MODEL: &str = "New model";

/// Label of the starters.
pub const START_FROM: &str = "Start from";

/// Starters with their descriptions, in the order the dialog shows them.
pub const STARTER_CHOICES: [(&str, &str); 9] = [
    ("Blank", "An empty model to build on."),
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

// The dialog that asks before Apply replaces a model applied from another sheet

/// Title of the dialog that asks before Apply replaces a model applied from another sheet under the same id.
pub const REPLACE_APPLIED_HEADING: &str = "Replace applied model?";

/// Returns the question of the dialog that asks before Apply replaces the model `name`, applied from another sheet
/// under the id `id`.
///
/// For example "Replace the applied model Chance SIR? It has the same id as this model, sir."
pub fn replace_applied(name: &str, id: &str) -> String {
    format!("Replace the applied model {name}? It has the same id as this model, {id}.")
}

/// Action of the dialog that asks before Apply replaces a model, which applies the model in its place.
pub const REPLACE: &str = "Replace";

// The dialog that asks before an edit takes away parts of the sheet besides the item it deletes or changes

/// Title of the dialog that asks before a parameter is deleted.
pub const DELETE_PARAMETER_HEADING: &str = "Delete parameter?";

/// Title of the dialog that asks before a property is deleted.
pub const DELETE_PROPERTY_HEADING: &str = "Delete property?";

/// Title of the dialog that asks before a line is deleted.
pub const DELETE_LINE_HEADING: &str = "Delete line?";

/// Title of the dialog that asks before a summary of a look at agents is removed.
pub const REMOVE_SUMMARY_HEADING: &str = "Remove summary?";

/// Title of the dialog that asks before an event is deleted.
pub const DELETE_EVENT_HEADING: &str = "Delete event?";

/// Title of the dialog that asks before a button is deleted.
pub const DELETE_BUTTON_HEADING: &str = "Delete button?";

/// Title of the dialog that asks before a measure is removed.
pub const REMOVE_MEASURE_HEADING: &str = "Remove measure?";

/// Title of the dialog that asks before Done of the property form deletes one state.
pub const DELETE_STATE_HEADING: &str = "Delete state?";

/// Title of the dialog that asks before Done of the property form deletes several states.
pub const DELETE_STATES_HEADING: &str = "Delete states?";

/// Title of the dialog that asks before Done of the property form gives a property another kind.
pub const CHANGE_KIND_HEADING: &str = "Change kind?";

/// Title of the dialog that asks before a property starts otherwise.
pub const REPLACE_START_HEADING: &str = "Replace start?";

/// Title of the dialog that asks before a kind of thing is coloured otherwise.
pub const CHANGE_COLORS_HEADING: &str = "Change colours?";

/// Title of the dialog that asks before Done of the button form changes what a button does.
pub const CHANGE_BUTTON_HEADING: &str = "Change button?";

/// Action of the dialog that asks before a delete, which deletes the item.
pub const DELETE: &str = "Delete";

/// Action of the dialog that asks before a summary or a measure is removed, which removes it.
pub const REMOVE: &str = "Remove";

/// Action of the dialog that asks before a property takes another kind, which gives it the kind.
pub const CHANGE_KIND: &str = "Change kind";

/// Action of the dialog that asks before a change of colours or of a button, which makes the change.
pub const CHANGE: &str = "Change";

/// Kind of thing a colour rule colours, as the dialog that asks before an edit names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColoredThing {
    /// The cells of a grid.
    Cells,
    /// The agents.
    Agents,
    /// The ground under agents.
    Ground,
}

impl ColoredThing {
    /// Returns the words of the kind of thing at the start of a sentence, such as "The cells".
    fn words(self) -> &'static str {
        match self {
            Self::Cells => "The cells",
            Self::Agents => "The agents",
            Self::Ground => "The ground",
        }
    }
}

/// Parts of a sheet that an edit takes away, named as the editor shows them, for the dialog that asks before the edit.
///
/// A line is the button of its card, `None` for a card that runs each tick, and its number on that card. The lines of
/// a button's card take its name, as in "Clear line 1".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LostParts<'a> {
    /// Lines that lose a value.
    pub lines: Vec<(Option<&'a str>, usize)>,
    /// Names of the properties whose start loses a value.
    pub starts: Vec<&'a str>,
    /// Starts of agents on a state of the ground that the ground will no longer paint on single cells, each as the
    /// name of the agents' property and the name of the state.
    pub start_cells: Vec<(&'a str, &'a str)>,
    /// Names of the ground properties whose share kept each tick loses its value.
    pub kept: Vec<&'a str>,
    /// Names of the buttons that lose a step.
    pub buttons: Vec<&'a str>,
    /// Names of the measures whose value loses a part.
    pub measures: Vec<&'a str>,
    /// Lines that go.
    pub deleted_lines: Vec<(Option<&'a str>, usize)>,
    /// Names of the measures that go.
    pub removed_measures: Vec<&'a str>,
    /// Kinds of thing no longer coloured by a property, each with the property's name.
    pub colors: Vec<(ColoredThing, &'a str)>,
    /// Kinds of thing whose colour rule loses shades, each with the names of the properties that no longer shade it.
    pub shades: Vec<(ColoredThing, Vec<&'a str>)>,
    /// Names of the properties whose painted start loses shapes, each with the number of shapes lost.
    pub shapes: Vec<(&'a str, usize)>,
}

/// Returns the question of the dialog that asks before an edit takes `lost` away from the item named `name`.
///
/// `name` is the name of what the edit deletes or changes. The question holds one sentence per kind of loss, such as
/// "Lines 1, 2 and 3 and New outbreak line 1 will lose Health. The measures Susceptible, Infected and Recovered will be
/// removed. The cells will no longer be coloured by Health."
pub fn lost_question(name: &str, lost: &LostParts<'_>) -> String {
    let mut sentences = Vec::new();
    let mut losers = line_groups(&lost.lines);
    losers.push(named_list("the start of", "the starts of", &lost.starts));
    if !lost.kept.is_empty() {
        let noun = if lost.kept.len() == 1 {
            "the share"
        } else {
            "the shares"
        };
        losers.push(format!("{noun} of {} kept each tick", names(&lost.kept)));
    }
    losers.push(named_list("the button", "the buttons", &lost.buttons));
    losers.push(named_list("the measure", "the measures", &lost.measures));
    losers.retain(|words| !words.is_empty());
    if !losers.is_empty() {
        sentences.push(format!("{} will lose {name}.", upper_first(&word_list(&losers))));
    }
    if !lost.deleted_lines.is_empty() {
        let lines = word_list(&line_groups(&lost.deleted_lines));
        sentences.push(format!("{} will be deleted.", upper_first(&lines)));
    }
    if !lost.removed_measures.is_empty() {
        let measures = named_list("the measure", "the measures", &lost.removed_measures);
        sentences.push(format!("{} will be removed.", upper_first(&measures)));
    }
    for (thing, property) in &lost.colors {
        sentences.push(format!("{} will no longer be coloured by {property}.", thing.words()));
    }
    for (thing, properties) in &lost.shades {
        sentences.push(format!(
            "{} will no longer be shaded by {}.",
            thing.words(),
            names(properties)
        ));
    }
    for (property, count) in &lost.shapes {
        sentences.push(match count {
            1 => format!("The shape painted on {property} will be lost."),
            _ => format!("The {count} shapes painted on {property} will be lost."),
        });
    }
    for (property, state) in &lost.start_cells {
        sentences.push(format!("{property} will have no {state} cell to start on."));
    }
    sentences.join(" ")
}

/// Returns the sentence of the dialog that asks before the measure `name` is removed while no control of the Sheet
/// pane makes it, such as "Total trail cannot be added again in the Sheet pane."
pub fn measure_not_added_again(name: &str) -> String {
    format!("{name} cannot be added again in the Sheet pane.")
}

/// Returns `lines` as one group of words per card, such as "lines 1, 2 and 3" and "New outbreak line 1", in the order
/// of each card's first line.
fn line_groups(lines: &[(Option<&str>, usize)]) -> Vec<String> {
    let mut cards: Vec<(Option<&str>, Vec<usize>)> = Vec::new();
    for &(button, number) in lines {
        match cards.iter_mut().find(|(known, _)| *known == button) {
            Some((_, numbers)) if numbers.contains(&number) => {}
            Some((_, numbers)) => numbers.push(number),
            None => cards.push((button, vec![number])),
        }
    }
    cards
        .iter()
        .map(|(button, numbers)| {
            let noun = if numbers.len() == 1 { "line" } else { "lines" };
            let numbers = number_list(numbers);
            match button {
                Some(button) => format!("{button} {noun} {numbers}"),
                None => format!("{noun} {numbers}"),
            }
        })
        .collect()
}

/// Returns `items` as a list after `one` or `several`, such as "the measure Total trail" or "the measures Susceptible
/// and Infected", and nothing for no item.
fn named_list(one: &str, several: &str, items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [item] => format!("{one} {item}"),
        _ => format!("{several} {}", names(items)),
    }
}

/// Returns `items` as a list ending in "and".
fn names(items: &[&str]) -> String {
    let items: Vec<String> = items.iter().map(|&item| item.to_owned()).collect();
    word_list(&items)
}

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

/// Placeholder of the New parameter dialog's Name field for the share of a mark kept each tick.
pub const SHARE_PARAMETER_PLACEHOLDER: &str = "Such as Trail kept per tick";

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
    "For a number of times, Default, Min and Max must be non-negative integers.";

/// Message on a minimum at or above the maximum.
pub const MIN_NOT_BELOW_MAX: &str = "Min must be less than Max.";

/// Message on a default outside the minimum and the maximum.
pub const DEFAULT_OUTSIDE_LIMITS: &str = "Default must be between Min and Max.";

/// Message on a parameter for a chance whose minimum or maximum lies outside 0 to 1.
pub const CHANCE_LIMITS_OUTSIDE: &str = "For a chance, Min and Max must be between 0 and 1.";

/// Message on a parameter for the share of a mark kept each tick whose minimum or maximum lies outside 0 to 1.
pub const SHARE_LIMITS_OUTSIDE: &str = "For a share, Min and Max must be between 0 and 1.";

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

/// Returns the tooltip of the button that shows the model's name `name` cut short to fit its row, such as "Rename
/// Spatial SIR epidemic with waning immunity".
pub fn rename_model_named(name: &str) -> String {
    format!("Rename {name}")
}

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
pub const PLOT_EMPTY: &str = "Turn on Plot beside a state under Measures to plot the number of cells in it.";

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
