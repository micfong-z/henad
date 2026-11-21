//! Values of a draft's rows: levels as typed and as resolved, the previews and examples of a row, and the estimates
//! of a design's configurations and of a search's generations and batches.

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{Factor, FactorLevel, FactorSpec, LevelSpec};
use henad_core::explore::plan::{ModelSchema, Plan};
use henad_core::explore::search::genetic::GeneticSettings;
use henad_core::explore::spec::{ActionSpec, SweepSpec};
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};

use crate::ui::params::display_value;

use super::messages::{factor_message, level_spec_message};
use super::{IssueKind, LevelPreview, LevelPreviews, MAX_DRAFT_LEVELS, SERIES_VALUE_BYTES, SweepDraft, TICKS_EXAMPLE};

impl SweepDraft {
    /// Returns the values of parameter row `index`, resolved against `schema` with the draft's `actions`, or the kind
    /// and message of its issue.
    pub(super) fn resolve_param(
        &self,
        schema: &ModelSchema<'_>,
        actions: &[ActionSpec],
        index: usize,
    ) -> Result<Factor, (IssueKind, String)> {
        let descriptor = &schema.params[index];
        let missing = || match descriptor.kind() {
            ParamKind::Bool { .. } | ParamKind::Choice { .. } => "Select at least one option".to_owned(),
            _ if self.draws_ranges() => format!("Enter values, such as {}", whole_range_text(descriptor.kind())),
            _ => format!("Enter values, such as {}", levels_example(descriptor.kind())),
        };
        let levels = row_levels(&self.factors[index].levels_text, self.draws_ranges(), missing)?;
        self.resolve(&FactorSpec::param(descriptor.id(), levels), schema, actions)
    }

    /// Returns the ticks of action row `position`, resolved with the draft's `actions`, or the kind and message of its
    /// issue.
    pub(super) fn resolve_ticks(
        &self,
        schema: &ModelSchema<'_>,
        actions: &[ActionSpec],
        position: usize,
    ) -> Result<Factor, (IssueKind, String)> {
        let action = &self.actions[position];
        let levels = row_levels(&action.ticks_text, self.draws_ranges(), || {
            format!("Enter ticks, such as {TICKS_EXAMPLE}")
        })?;
        self.resolve(&FactorSpec::action(action.name.clone(), levels), schema, actions)
    }

    /// Returns the values of `spec`, the factor of a parameter or action row, resolved against `schema` with the
    /// draft's `actions` under the draft's design, or the kind and message of its issue.
    pub(super) fn resolve(
        &self,
        spec: &FactorSpec,
        schema: &ModelSchema<'_>,
        actions: &[ActionSpec],
    ) -> Result<Factor, (IssueKind, String)> {
        spec.resolve(schema.params, actions, &self.factor_design())
            .map_err(|error| (IssueKind::Invalid, factor_message(&error)))
    }

    /// Returns the values each varied parameter takes, then those each varied action tick takes, `None` for a row that
    /// is not varied or has an issue.
    pub fn level_previews(&self, schema: &ModelSchema<'_>) -> LevelPreviews {
        let actions = self.declared_actions(schema);
        let params = schema
            .params
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let varied = self.factors.get(index).is_some_and(|factor| factor.vary);
                let factor = self.resolve_param(schema, &actions, index).ok().filter(|_| varied)?;
                Some(param_preview(descriptor, &factor))
            })
            .collect();
        let ticks = (0..self.actions.len())
            .map(|position| {
                let varied = self.actions[position].vary_tick;
                let factor = self.resolve_ticks(schema, &actions, position).ok().filter(|_| varied)?;
                Some(LevelPreview::of(&factor, tick_text, |bound| (bound as u64).to_string()))
            })
            .collect();
        (params, ticks)
    }
}

/// Returns the text a parameter's levels start as: `all` for a bool or a choice, and nothing for a number.
pub(super) fn default_levels_text(kind: &ParamKind) -> String {
    match kind {
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => "all".to_owned(),
        ParamKind::F32 { .. } | ParamKind::U32 { .. } => String::new(),
    }
}

/// Returns an example of the values a parameter of `kind` takes, as a hint and in the note of an empty row.
///
/// The example's range steps by about a quarter of the parameter's range, rounded to 1, 2 or 5 times a power of ten,
/// and to a whole number for an integer parameter.
pub fn levels_example(kind: &ParamKind) -> String {
    if matches!(kind, ParamKind::Bool { .. } | ParamKind::Choice { .. }) {
        return "all".to_owned();
    }
    let (min, max) = kind_bounds(kind);
    match quarter_step(kind, min, max).filter(|_| max > min) {
        Some(step) => format!(
            "{}:{}:{}",
            number_text(kind, min),
            number_text(kind, max),
            number_text(kind, step)
        ),
        None => both_ends_fill(kind),
    }
}

/// Returns about a quarter of the range from `min` to `max` of a parameter of `kind`, rounded to 1, 2 or 5 times a
/// power of ten, and to a whole number of at least 1 for an integer parameter.
///
/// Returns `None` for a step that is not finite and positive.
pub fn quarter_step(kind: &ParamKind, min: f64, max: f64) -> Option<f64> {
    let step = round_step((max - min) / 4.0);
    let step = match kind {
        ParamKind::U32 { .. } => step.round().max(1.0),
        _ => step,
    };
    (step.is_finite() && step > 0.0).then_some(step)
}

/// Returns the lowest and highest value of a parameter of `kind`, 0 and 1 for a parameter that is not a number.
pub fn kind_bounds(kind: &ParamKind) -> (f64, f64) {
    match *kind {
        ParamKind::F32 { min, max, .. } => (f64::from(min), f64::from(max)),
        ParamKind::U32 { min, max, .. } => (f64::from(min), f64::from(max)),
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => (0.0, 1.0),
    }
}

/// Returns the quick fill of a parameter's minimum and maximum only.
pub fn both_ends_fill(kind: &ParamKind) -> String {
    let (low, high) = kind_bounds(kind);
    format!("{},{}", number_text(kind, low), number_text(kind, high))
}

/// Returns `number` as a value of a parameter of `kind` is written: to the precision of an `f32`, or whole.
pub fn number_text(kind: &ParamKind, number: f64) -> String {
    match kind {
        ParamKind::U32 { .. } => format!("{}", number.round().max(0.0) as u64),
        _ => format!("{}", number as f32),
    }
}

/// Returns `value` rounded to 1, 2 or 5 times a power of ten.
pub(super) fn round_step(value: f64) -> f64 {
    let scale = 10_f64.powi(value.log10().floor() as i32);
    let multiple = match value / scale {
        fraction if fraction < 1.5 => 1.0,
        fraction if fraction < 3.5 => 2.0,
        fraction if fraction < 7.5 => 5.0,
        _ => 10.0,
    };
    multiple * scale
}

/// Returns a parameter's whole range as `min:max`, or `all` for a bool or a choice.
pub fn whole_range_text(kind: &ParamKind) -> String {
    match kind {
        ParamKind::F32 { min, max, .. } => format!("{min}:{max}"),
        ParamKind::U32 { min, max, .. } => format!("{min}:{max}"),
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => "all".to_owned(),
    }
}

/// Reads the levels of a row's `text`, or returns the kind and message of its issue. An empty row is missing input,
/// described by `missing`.
///
/// A range of more than [`MAX_DRAFT_LEVELS`] values is rejected, unless it has no step and `draws_ranges` is set. A
/// sampled design or a search draws its values from such a range and lists none of them.
pub(super) fn row_levels(
    text: &str,
    draws_ranges: bool,
    missing: impl FnOnce() -> String,
) -> Result<LevelSpec, (IssueKind, String)> {
    if text.trim().is_empty() {
        return Err((IssueKind::Missing, missing()));
    }
    let levels = parse_levels(text).map_err(|message| (IssueKind::Invalid, message))?;
    let enumerated = match levels {
        LevelSpec::Range { step: None, .. } => !draws_ranges,
        LevelSpec::Range { step: Some(_), .. } => true,
        LevelSpec::Values(_) | LevelSpec::All => false,
    };
    match level_estimate(&levels) {
        count if enumerated && count > MAX_DRAFT_LEVELS => Err((
            IssueKind::Invalid,
            format!("Range has {count} values, over the limit of {MAX_DRAFT_LEVELS}"),
        )),
        _ => Ok(levels),
    }
}

/// Returns the preview of parameter `descriptor`'s resolved `factor`, each value as the Parameters tab shows it.
pub(super) fn param_preview(descriptor: &ParamDescriptor, factor: &Factor) -> LevelPreview {
    LevelPreview::of(
        factor,
        |level| match level {
            FactorLevel::Param(value) => display_value(descriptor, value),
            FactorLevel::Tick(tick) => tick.to_string(),
        },
        |bound| {
            let value = match descriptor.kind() {
                ParamKind::U32 { .. } => ParamValue::U32(bound as u32),
                _ => ParamValue::F32(bound as f32),
            };
            display_value(descriptor, &value)
        },
    )
}

/// Returns the text of an action's tick `level`.
pub(super) fn tick_text(level: &FactorLevel) -> String {
    match level {
        FactorLevel::Tick(tick) => tick.to_string(),
        FactorLevel::Param(_) => String::new(),
    }
}

/// Returns the number of evaluations in each generation after the first of a genetic algorithm with `settings`:
/// the members it re-evaluates and a child for every member but the elites.
pub(super) fn later_generation_size(settings: &GeneticSettings) -> u64 {
    let children = settings.population.saturating_sub(settings.elite_count);
    (settings.reevaluation_count() + children) as u64
}

/// Returns the approximate number of generations a genetic algorithm with `settings` starts within `max_evaluations`, a
/// partial last one included, or `None` for a budget smaller than one generation.
///
/// Generation 0 evaluates the whole population. Note that a child with the same configuration as an earlier
/// candidate can leave a generation smaller than planned.
pub fn generation_estimate(max_evaluations: u64, settings: &GeneticSettings) -> Option<u64> {
    let population = settings.population as u64;
    if max_evaluations < population {
        return None;
    }
    match later_generation_size(settings) {
        0 => Some(1),
        later => Some(1 + (max_evaluations - population).div_ceil(later)),
    }
}

/// Returns the approximate number of batches of at most `batch_size` candidates that `max_evaluations` take, for a
/// genetic algorithm with `genetic` settings when set.
///
/// No batch of a genetic algorithm spans two generations.
pub fn batch_estimate(max_evaluations: u64, batch_size: usize, genetic: Option<&GeneticSettings>) -> u64 {
    let batch = (batch_size as u64).max(1);
    let Some(settings) = genetic else {
        return max_evaluations.div_ceil(batch);
    };
    let population = settings.population as u64;
    let later = later_generation_size(settings);
    if max_evaluations <= population || later == 0 {
        return max_evaluations.div_ceil(batch);
    }
    let rest = max_evaluations - population;
    population.div_ceil(batch) + rest / later * later.div_ceil(batch) + (rest % later).div_ceil(batch)
}

/// Reads levels as `--vary` accepts them, with spaces around each listed value ignored.
///
/// Note that a range of any length parses. The check rejects a range of more than [`MAX_DRAFT_LEVELS`] values where
/// the draft lists them.
///
/// # Errors
///
/// Returns a message for text with no values, or a range that does not parse.
pub fn parse_levels(text: &str) -> Result<LevelSpec, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter values".to_owned());
    }
    match LevelSpec::parse(text).map_err(|error| level_spec_message(&error))? {
        LevelSpec::Values(values) => {
            let values: Vec<String> = values
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
            if values.is_empty() {
                return Err("Enter values".to_owned());
            }
            Ok(LevelSpec::Values(values))
        }
        levels => Ok(levels),
    }
}

/// Returns `levels` in the text [`parse_levels`] reads.
pub fn levels_text(levels: &LevelSpec) -> String {
    match levels {
        LevelSpec::Values(values) => values.join(","),
        LevelSpec::Range {
            min,
            max,
            step: Some(step),
        } => format!("{min}:{max}:{step}"),
        LevelSpec::Range { min, max, step: None } => format!("{min}:{max}"),
        LevelSpec::All => "all".to_owned(),
    }
}

/// Returns the number of values `levels` lists, give or take one for a range, and 1 for levels a design draws from.
pub(super) fn level_estimate(levels: &LevelSpec) -> u64 {
    match levels {
        LevelSpec::Values(values) => values.len() as u64,
        LevelSpec::Range { min, max, step } => {
            let span = (max - min) / step.unwrap_or(1.0);
            if span.is_finite() && span >= 0.0 {
                (span as u64).saturating_add(1)
            } else {
                1
            }
        }
        LevelSpec::All => 1,
    }
}

/// Returns the approximate number of configurations of `spec`, cheap enough to count before planning.
pub(super) fn estimated_configs(spec: &SweepSpec) -> u64 {
    if spec.blocks.is_empty() {
        return 1;
    }
    spec.blocks
        .iter()
        .map(|block| {
            let counts = block.factors.iter().map(|factor| level_estimate(&factor.levels));
            match &block.design {
                DesignKind::Factorial => counts.fold(1, u64::saturating_mul),
                DesignKind::Zip => counts.max().unwrap_or(1),
                DesignKind::Random { samples } | DesignKind::LatinHypercube { samples } => *samples as u64,
                DesignKind::Table { text } => table_row_count(text) as u64,
            }
        })
        .fold(0, u64::saturating_add)
}

/// Returns the number of rows of the design table `text` after its header, blank lines left out.
pub(super) fn table_row_count(text: &str) -> usize {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .saturating_sub(1)
}

/// Returns the approximate size in bytes of `series.csv` that `run_count` runs with the settings of `plan` write, for a
/// model of `column_count` stat columns.
///
/// Every run counts as running to its last step.
pub(super) fn estimated_series_bytes(plan: &Plan, run_count: u64, column_count: usize) -> u64 {
    let series_every = plan.measure_settings().series_every;
    if series_every == 0 {
        return 0;
    }
    let rows = (plan.run_settings().steps / series_every).saturating_add(1);
    let row_bytes = (column_count as u64 + 2) * SERIES_VALUE_BYTES;
    run_count.saturating_mul(rows).saturating_mul(row_bytes)
}
