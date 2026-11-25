//! Wording of the errors a draft's check reports, in the words of the Sweep tab.

use henad_core::explore::design::MAX_CONFIGS;
use henad_core::explore::factor::{FactorError, LevelSpecError};
use henad_core::explore::search::SearchSpecError;
use henad_core::explore::stop::Comparator;
use henad_core::explore::value::ValueError;

use crate::ui::results::labels::output_label;
use crate::ui::text::{capitalize, describe_error};

use super::{
    CROSSOVER_RATE_KEY, ELITE_COUNT_KEY, GENETIC_MUTATION_SCALE_KEY, HILL_CLIMB_MUTATION_SCALE_KEY,
    INITIAL_SAMPLES_KEY, MUTATION_RATE_KEY, PATIENCE_KEY, POPULATION_KEY, PSE_MUTATION_SCALE_KEY,
    REEVALUATE_FRACTION_KEY, TOURNAMENT_SIZE_KEY,
};

/// Returns the message of text that does not parse as levels, in the words of the Sweep tab.
pub(super) fn level_spec_message(error: &LevelSpecError) -> String {
    match error {
        LevelSpecError::NotANumber { part, .. } => format!("'{}' is not a number", part.trim()),
        LevelSpecError::BadRange { range } => format!("'{range}' is not a range. Enter min:max:step or min:max."),
        LevelSpecError::MixedPercent { range } => {
            format!("'{range}' has % after some parts only. Enter % after every part or no part.")
        }
    }
}

/// Returns the message of a factor the plan rejects, in the words of the Sweep tab.
///
/// The row that the issue is placed on already identifies the parameter or the action, so the message leaves it out.
pub(super) fn factor_message(error: &FactorError) -> String {
    match error {
        FactorError::Level { source, .. } => value_message(source),
        FactorError::BadTick { raw, .. } => format!("'{raw}' is not a non-negative integer"),
        FactorError::Range { source, .. } => capitalize(&source.to_string()),
        FactorError::MissingStep { .. } => {
            "Range needs a step, as in min:max:step. For min:max, select Latin hypercube or Uniform random under \
             Design."
                .to_owned()
        }
        FactorError::NotWhole { value, .. } => format!("{value} is not an integer"),
        FactorError::AllOverNumber { .. } => "Only checkbox and dropdown parameters accept 'all'".to_owned(),
        FactorError::RangeOverOptions { .. } => {
            "This parameter accepts listed values or 'all'. Ranges apply only to number parameters.".to_owned()
        }
        FactorError::NoLevels { .. } => "Enter values".to_owned(),
        FactorError::UnknownParam { .. } | FactorError::UnknownAction { .. } => describe_error(error),
    }
}

/// Returns the message of search settings the search rejects, in the words of the Sweep tab.
pub(super) fn setting_message(error: &SearchSpecError) -> String {
    match error {
        SearchSpecError::NoEvaluations => "Evaluations must be at least 1".to_owned(),
        SearchSpecError::NoBatch => "Batch size must be at least 1".to_owned(),
        SearchSpecError::BatchTooLarge { batch_size } => {
            format!("Batch size must be at most {MAX_CONFIGS}, got {batch_size}")
        }
        SearchSpecError::MissingObjective { .. } => "Objective: Select output".to_owned(),
        SearchSpecError::Setting { key, value, expected } => {
            format!("{} must be {expected}, got {value}", setting_label(key))
        }
        SearchSpecError::UnusedObjective => describe_error(error),
    }
}

/// Returns the label of the field that edits the search setting `key`, as in `genetic.elite_count`.
pub(in crate::ui::sweep) fn setting_label(key: &str) -> &str {
    match key {
        HILL_CLIMB_MUTATION_SCALE_KEY | GENETIC_MUTATION_SCALE_KEY | PSE_MUTATION_SCALE_KEY => "Step size",
        PATIENCE_KEY => "Patience",
        POPULATION_KEY => "Population",
        ELITE_COUNT_KEY => "Elites",
        TOURNAMENT_SIZE_KEY => "Tournament size",
        CROSSOVER_RATE_KEY => "Crossover rate",
        MUTATION_RATE_KEY => "Mutation rate",
        REEVALUATE_FRACTION_KEY => "Re-evaluation rate",
        INITIAL_SAMPLES_KEY => "Initial samples",
        "pse.x_axis.min" => "X axis Minimum",
        "pse.x_axis.max" => "X axis Maximum",
        "pse.x_axis.cells" => "X axis Cells",
        "pse.y_axis.min" => "Y axis Minimum",
        "pse.y_axis.max" => "Y axis Maximum",
        "pse.y_axis.cells" => "Y axis Cells",
        other => other,
    }
}

/// Returns the issue of a root seed that does not read, for `reason`.
pub(super) fn root_seed_message(reason: &str) -> String {
    format!("Root seed: {reason}")
}

/// Returns the issue of a design seed that does not read, for `reason`.
pub(super) fn design_seed_message(reason: &str) -> String {
    format!("Design seed: {reason}")
}

/// Returns the issue of an output `column` that a search reads and the runs do not record.
pub(super) fn missing_output_message(column: &str) -> String {
    format!("{} is missing from Outputs", output_label(column))
}

/// Returns the message of a value a parameter rejects, in the words of the Sweep tab.
pub(super) fn value_message(error: &ValueError) -> String {
    match error {
        ValueError::NotANumber { raw, .. } => format!("'{raw}' is not a number"),
        ValueError::NotAnInteger { raw, .. } => format!("'{raw}' is not an integer"),
        ValueError::NotABool { raw, .. } => format!("'{raw}' is not true or false"),
        ValueError::OutOfRange { value, min, max } => format!("{value} is outside the range {min} to {max}"),
        ValueError::NotWhole { value } => format!("{value} is not an integer"),
        ValueError::UnexpectedPercent { raw } => format!("'{raw}' is a percentage, but this parameter is not"),
        ValueError::UnknownOption { raw, options } => format!("'{raw}' is not one of {}", options.join(", ")),
        ValueError::Param { source, .. } => value_message(source),
        ValueError::UnknownParam { .. } | ValueError::BadOverride { .. } | ValueError::WrongKind { .. } => {
            describe_error(error)
        }
    }
}

/// Returns the words that the Sweep tab uses for `comparator`, as in "at most".
pub fn comparator_words(comparator: Comparator) -> &'static str {
    match comparator {
        Comparator::Less => "less than",
        Comparator::LessOrEqual => "at most",
        Comparator::Equal => "equal to",
        Comparator::NotEqual => "not equal to",
        Comparator::GreaterOrEqual => "at least",
        Comparator::Greater => "greater than",
    }
}
