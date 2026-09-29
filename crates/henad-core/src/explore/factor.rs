//! Factors of a sweep, as a spec writes them and as a plan resolves them.
//!
//! A factor is one parameter, or the tick of one action, that a sweep varies. Each value it takes is a level. A
//! random or Latin hypercube design samples its factors, and can draw a factor's value from a whole range instead
//! of a list of levels.

use std::fmt;
use std::num::ParseFloatError;

use crate::explore::design::DesignKind;
use crate::explore::plan::Config;
use crate::explore::spec::ActionSpec;
use crate::explore::value::{ValueError, check_value, parse_value};
use crate::params::{ParamDescriptor, ParamKind, ParamValue};

/// Most levels one factor can have.
pub const MAX_LEVELS: u64 = 1 << 24;

/// Relative amount by which a range may miss a whole number of steps and still end on its `max`.
const ROUNDING: f64 = 1e-9;

/// Levels of a factor, as a spec writes them.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelSpec {
    /// Values in the text [`parse_value`] reads, or ticks for an action.
    Values(Vec<String>),
    /// Every value from `min` to `max` inclusive, `step` apart.
    ///
    /// A whole-number factor takes a step of 1 when `step` is `None`. An `F32` parameter needs a step, except in a
    /// sampled design. A sampled design draws from the whole range.
    Range { min: f64, max: f64, step: Option<f64> },
    /// Every value of a `Bool` or `Choice` parameter.
    All,
}

impl LevelSpec {
    /// Reads levels as `--vary` writes them: `all`, a range `min:max:step` or `min:max`, or a list `v1,v2,...`.
    ///
    /// The model checks the levels when the sweep is planned.
    ///
    /// # Errors
    ///
    /// Returns [`LevelSpecError`] for a range with other than two or three parts, or a part that is not a number.
    pub fn parse(text: &str) -> Result<Self, LevelSpecError> {
        if text == "all" {
            return Ok(Self::All);
        }
        if !text.contains(':') {
            return Ok(Self::Values(text.split(',').map(str::to_owned).collect()));
        }
        let number = |part: &str| {
            part.trim().parse::<f64>().map_err(|source| LevelSpecError::NotANumber {
                part: part.to_owned(),
                range: text.to_owned(),
                source,
            })
        };
        match text.split(':').collect::<Vec<_>>().as_slice() {
            [min, max] => Ok(Self::Range {
                min: number(min)?,
                max: number(max)?,
                step: None,
            }),
            [min, max, step] => Ok(Self::Range {
                min: number(min)?,
                max: number(max)?,
                step: Some(number(step)?),
            }),
            _ => Err(LevelSpecError::BadRange { range: text.to_owned() }),
        }
    }
}

/// Text that does not read as a [`LevelSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelSpecError {
    /// A part of range `range` that is not a number, for the reason in `source`.
    NotANumber {
        part: String,
        range: String,
        source: ParseFloatError,
    },
    /// A range with other than two or three parts.
    BadRange { range: String },
}

impl fmt::Display for LevelSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotANumber { part, range, .. } => write!(f, "'{part}' in range '{range}' is not a number"),
            Self::BadRange { range } => write!(f, "invalid range '{range}', expected MIN:MAX:STEP or MIN:MAX"),
        }
    }
}

impl std::error::Error for LevelSpecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotANumber { source, .. } => Some(source),
            Self::BadRange { .. } => None,
        }
    }
}

/// Part of a config a factor varies, named as a spec names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactorTarget {
    /// Parameter with this id.
    Param(String),
    /// Tick of the action with this name, as [`ActionSpec::name`] gives it.
    Action(String),
}

impl fmt::Display for FactorTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Param(id) => write!(f, "parameter '{id}'"),
            Self::Action(name) => write!(f, "action '{name}'"),
        }
    }
}

/// A factor as a spec writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorSpec {
    pub target: FactorTarget,
    pub levels: LevelSpec,
}

impl FactorSpec {
    /// Returns a factor over parameter `id`.
    pub fn param(id: impl Into<String>, levels: LevelSpec) -> Self {
        Self {
            target: FactorTarget::Param(id.into()),
            levels,
        }
    }

    /// Returns a factor over the tick of the action named `name`.
    pub fn action(name: impl Into<String>, levels: LevelSpec) -> Self {
        Self {
            target: FactorTarget::Action(name.into()),
            levels,
        }
    }

    /// Resolves the target against `params` and `actions`, and checks every level against the target.
    ///
    /// In a random or Latin hypercube design, a range with no step stands for every value from `min` to `max`.
    /// Everywhere else the factor lists its levels.
    ///
    /// # Errors
    ///
    /// Returns [`FactorError`] for a parameter or action the sweep does not have, levels the target cannot take, or
    /// a level the target refuses.
    pub fn resolve(
        &self,
        params: &[ParamDescriptor],
        actions: &[ActionSpec],
        design: &DesignKind,
    ) -> Result<Factor, FactorError> {
        self.resolve_domain(params, actions, design.is_sampled())
    }

    /// Resolves the factor as [`Self::resolve`] does for a random or Latin hypercube design.
    ///
    /// # Errors
    ///
    /// Returns [`FactorError`] for a parameter or action the sweep does not have, levels the target cannot take, or
    /// a level the target refuses.
    pub fn resolve_sampled(&self, params: &[ParamDescriptor], actions: &[ActionSpec]) -> Result<Factor, FactorError> {
        self.resolve_domain(params, actions, true)
    }

    fn resolve_domain(
        &self,
        params: &[ParamDescriptor],
        actions: &[ActionSpec],
        sampled: bool,
    ) -> Result<Factor, FactorError> {
        match &self.target {
            FactorTarget::Param(id) => {
                let Some(index) = params.iter().position(|descriptor| descriptor.id == *id) else {
                    return Err(FactorError::UnknownParam {
                        id: id.clone(),
                        known: params.iter().map(|descriptor| descriptor.id).collect(),
                    });
                };
                let kind = &params[index].kind;
                let domain = match &self.levels {
                    LevelSpec::Values(raw) => FactorDomain::Levels(parsed_levels(id, kind, raw)?),
                    &LevelSpec::Range { min, max, step } => range_domain(id, kind, min, max, step, sampled)?,
                    LevelSpec::All => FactorDomain::Levels(every_level(id, kind)?),
                };
                Ok(Factor {
                    slot: FactorSlot::Param(index),
                    domain,
                })
            }
            FactorTarget::Action(name) => {
                let Some(index) = actions.iter().position(|action| action.name == *name) else {
                    return Err(FactorError::UnknownAction {
                        name: name.clone(),
                        known: actions.iter().map(|action| action.name.clone()).collect(),
                    });
                };
                let domain = match &self.levels {
                    LevelSpec::Values(raw) => FactorDomain::Levels(tick_levels(name, raw)?),
                    &LevelSpec::Range { min, max, step } => {
                        let min = whole_tick(name, min)?;
                        let max = whole_tick(name, max)?;
                        whole_domain(&self.target, min, max, step, sampled, FactorLevel::Tick)?
                    }
                    LevelSpec::All => {
                        return Err(FactorError::AllOverNumber {
                            target: self.target.clone(),
                        });
                    }
                };
                Ok(Factor {
                    slot: FactorSlot::Action(index),
                    domain,
                })
            }
        }
    }
}

/// Part of a config a resolved factor writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactorSlot {
    /// Index into [`Config::params`].
    Param(usize),
    /// Index into [`Config::action_ticks`].
    Action(usize),
}

/// One level of a resolved factor.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorLevel {
    Param(ParamValue),
    Tick(u64),
}

/// Values a resolved factor can take.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorDomain {
    /// Levels listed one by one.
    Levels(Vec<FactorLevel>),
    /// Every `f32` value from `min` to `max`, for a sampled design to draw from.
    Continuous { min: f64, max: f64 },
    /// Every whole number from `min` to `max` inclusive, for a sampled design to draw from.
    WholeNumbers { min: u64, max: u64 },
}

/// A factor resolved against a model's parameters and a spec's actions, with every level checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Factor {
    pub slot: FactorSlot,
    pub domain: FactorDomain,
}

impl Factor {
    /// Returns the listed levels, or `None` for a factor that takes a whole range.
    pub fn levels(&self) -> Option<&[FactorLevel]> {
        match &self.domain {
            FactorDomain::Levels(levels) => Some(levels),
            FactorDomain::Continuous { .. } | FactorDomain::WholeNumbers { .. } => None,
        }
    }

    /// Writes `level` into `config`.
    ///
    /// # Panics
    ///
    /// Panics when a tick meets a parameter slot or a parameter value meets an action slot, or the slot is past the
    /// end of `config`.
    pub fn apply(&self, level: &FactorLevel, config: &mut Config) {
        match (self.slot, level) {
            (FactorSlot::Param(index), FactorLevel::Param(value)) => config.params[index] = value.clone(),
            (FactorSlot::Action(index), &FactorLevel::Tick(tick)) => config.action_ticks[index] = tick,
            (slot, level) => panic!("level {level:?} does not fit slot {slot:?}"),
        }
    }
}

/// A range that gives no values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RangeError {
    /// A range with an end that is not finite.
    NotFinite { min: f64, max: f64 },
    /// A step that is zero, negative or not finite.
    BadStep { step: f64 },
    /// A `min` above `max`.
    Reversed { min: f64, max: f64 },
    /// A range of more than [`MAX_LEVELS`] values.
    TooManyLevels,
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { min, max } => write!(f, "range ends must be finite, got {min} and {max}"),
            Self::BadStep { step } => write!(f, "step {step} is not a positive number"),
            Self::Reversed { min, max } => write!(f, "min {min} is greater than max {max}"),
            Self::TooManyLevels => write!(f, "range has more than {MAX_LEVELS} values"),
        }
    }
}

impl std::error::Error for RangeError {}

/// A factor that cannot be resolved against a model's parameters and a spec's actions.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorError {
    /// An id no descriptor has. `known` lists the ids the descriptors do have.
    UnknownParam { id: String, known: Vec<&'static str> },
    /// A name no action of the spec has. `known` lists the names the actions do have.
    UnknownAction { name: String, known: Vec<String> },
    /// A level the descriptor of parameter `id` refuses, for the reason in `source`.
    Level { id: String, source: ValueError },
    /// A tick of action `name` that is not a whole number from 0.
    BadTick { name: String, raw: String },
    /// A range over `target` that gives no values, for the reason in `source`.
    Range { target: FactorTarget, source: RangeError },
    /// A range with no step over `F32` parameter `id`, in a design that lists levels.
    MissingStep { id: String },
    /// A range end or step for a whole-number `target` that is not a whole number.
    NotWhole { target: FactorTarget, value: f64 },
    /// `all` over a numeric `target`.
    AllOverNumber { target: FactorTarget },
    /// A range over `Bool` or `Choice` parameter `id`.
    RangeOverOptions { id: String },
    /// An empty list of values for `target`.
    NoLevels { target: FactorTarget },
}

impl fmt::Display for FactorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownParam { id, known } => {
                write!(f, "unknown parameter '{id}', expected one of {}", known.join(", "))
            }
            Self::UnknownAction { name, known } if known.is_empty() => {
                write!(f, "unknown action '{name}' (spec declares no actions)")
            }
            Self::UnknownAction { name, known } => {
                write!(f, "unknown action '{name}', expected one of {}", known.join(", "))
            }
            Self::Level { id, .. } => write!(f, "parameter '{id}'"),
            Self::BadTick { name, raw } => write!(f, "action '{name}' takes a non-negative integer tick, got '{raw}'"),
            Self::Range { target, .. } => write!(f, "range of {target}"),
            Self::MissingStep { id } => write!(
                f,
                "range of parameter '{id}' needs a step, except in a random or Latin hypercube design"
            ),
            Self::NotWhole { target, value } => write!(f, "{target} takes integers, got {value}"),
            Self::AllOverNumber { target } => write!(f, "{target} is a number, and 'all' needs a bool or choice"),
            Self::RangeOverOptions { id } => {
                write!(
                    f,
                    "parameter '{id}' is a bool or choice, and takes values or 'all' but no range"
                )
            }
            Self::NoLevels { target } => write!(f, "{target} has no values to vary over"),
        }
    }
}

impl std::error::Error for FactorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Level { source, .. } => Some(source),
            Self::Range { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Returns `min + i * step` for every `i` from 0 up to the last value not past `max`.
///
/// Each value is computed from its index, so no rounding error accumulates across steps. A last value within a
/// rounding error of `max`, on either side, is `max` itself.
///
/// # Errors
///
/// Returns [`RangeError`] for an end that is not finite, a step that is not positive, a `min` above `max`, or more
/// than [`MAX_LEVELS`] values.
pub fn inclusive_steps(min: f64, max: f64, step: f64) -> Result<Vec<f64>, RangeError> {
    if !min.is_finite() || !max.is_finite() {
        return Err(RangeError::NotFinite { min, max });
    }
    if !(step.is_finite() && step > 0.0) {
        return Err(RangeError::BadStep { step });
    }
    if min > max {
        return Err(RangeError::Reversed { min, max });
    }
    let span = (max - min) / step;
    let tolerance = span.max(1.0) * ROUNDING;
    let last = (span + tolerance).floor();
    if last >= MAX_LEVELS as f64 {
        return Err(RangeError::TooManyLevels);
    }
    let ends_on_max = span - last <= tolerance;
    let last = last as u64;
    Ok((0..=last)
        .map(|i| {
            if i == last && ends_on_max {
                max
            } else {
                min + i as f64 * step
            }
        })
        .collect())
}

fn parsed_levels(id: &str, kind: &ParamKind, raw: &[String]) -> Result<Vec<FactorLevel>, FactorError> {
    if raw.is_empty() {
        return Err(FactorError::NoLevels {
            target: FactorTarget::Param(id.to_owned()),
        });
    }
    raw.iter()
        .map(|text| {
            parse_value(kind, text)
                .map(FactorLevel::Param)
                .map_err(|source| FactorError::Level {
                    id: id.to_owned(),
                    source,
                })
        })
        .collect()
}

fn tick_levels(name: &str, raw: &[String]) -> Result<Vec<FactorLevel>, FactorError> {
    if raw.is_empty() {
        return Err(FactorError::NoLevels {
            target: FactorTarget::Action(name.to_owned()),
        });
    }
    raw.iter()
        .map(|text| {
            text.trim()
                .parse()
                .ok()
                .map(FactorLevel::Tick)
                .ok_or_else(|| FactorError::BadTick {
                    name: name.to_owned(),
                    raw: text.clone(),
                })
        })
        .collect()
}

fn range_domain(
    id: &str,
    kind: &ParamKind,
    min: f64,
    max: f64,
    step: Option<f64>,
    sampled: bool,
) -> Result<FactorDomain, FactorError> {
    let target = FactorTarget::Param(id.to_owned());
    match *kind {
        ParamKind::F32 { .. } => {
            // Every value lies between the two ends, and rounding to f32 keeps that order.
            for end in [min, max] {
                check_value(kind, &ParamValue::F32(end as f32)).map_err(|source| FactorError::Level {
                    id: id.to_owned(),
                    source,
                })?;
            }
            let Some(step) = step else {
                if !sampled {
                    return Err(FactorError::MissingStep { id: id.to_owned() });
                }
                if min > max {
                    return Err(FactorError::Range {
                        target,
                        source: RangeError::Reversed { min, max },
                    });
                }
                return Ok(FactorDomain::Continuous { min, max });
            };
            let values = inclusive_steps(min, max, step).map_err(|source| FactorError::Range { target, source })?;
            Ok(FactorDomain::Levels(
                values
                    .into_iter()
                    .map(|value| FactorLevel::Param(ParamValue::F32(value as f32)))
                    .collect(),
            ))
        }
        ParamKind::U32 {
            min: lowest,
            max: highest,
            ..
        } => {
            let min = whole_level(id, min, lowest, highest)?;
            let max = whole_level(id, max, lowest, highest)?;
            whole_domain(&target, min, max, step, sampled, |value| {
                FactorLevel::Param(ParamValue::U32(value as u32))
            })
        }
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => Err(FactorError::RangeOverOptions { id: id.to_owned() }),
    }
}

/// Returns the whole numbers from `min` to `max` inclusive, `step` apart, each made a level by `level`.
///
/// A sampled design with no step takes the whole range instead.
fn whole_domain(
    target: &FactorTarget,
    min: u64,
    max: u64,
    step: Option<f64>,
    sampled: bool,
    level: impl Fn(u64) -> FactorLevel,
) -> Result<FactorDomain, FactorError> {
    let range_error = |source| FactorError::Range {
        target: target.clone(),
        source,
    };
    if let Some(step) = step {
        if !(step.is_finite() && step > 0.0) {
            return Err(range_error(RangeError::BadStep { step }));
        }
        if step.fract() != 0.0 {
            return Err(FactorError::NotWhole {
                target: target.clone(),
                value: step,
            });
        }
    }
    if min > max {
        return Err(range_error(RangeError::Reversed {
            min: min as f64,
            max: max as f64,
        }));
    }
    let Some(step) = step else {
        if sampled {
            return Ok(FactorDomain::WholeNumbers { min, max });
        }
        return levels_by_step(min, max, 1, &level).map_err(range_error);
    };
    levels_by_step(min, max, step as u64, &level).map_err(range_error)
}

/// Returns `min + i * step` for every `i` up to the last value not past `max`, each made a level by `level`.
fn levels_by_step(
    min: u64,
    max: u64,
    step: u64,
    level: &impl Fn(u64) -> FactorLevel,
) -> Result<FactorDomain, RangeError> {
    let count = (max - min) / step + 1;
    if count > MAX_LEVELS {
        return Err(RangeError::TooManyLevels);
    }
    Ok(FactorDomain::Levels(
        (0..count).map(|i| level(min + i * step)).collect(),
    ))
}

/// Returns `value` as a level of a `U32` parameter bounded by `lowest` and `highest`.
fn whole_level(id: &str, value: f64, lowest: u32, highest: u32) -> Result<u64, FactorError> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(FactorError::NotWhole {
            target: FactorTarget::Param(id.to_owned()),
            value,
        });
    }
    if value < f64::from(lowest) || value > f64::from(highest) {
        return Err(FactorError::Level {
            id: id.to_owned(),
            source: ValueError::OutOfRange {
                value: value.to_string(),
                min: lowest.to_string(),
                max: highest.to_string(),
            },
        });
    }
    Ok(value as u64)
}

/// Returns `value` as a tick of action `name`.
fn whole_tick(name: &str, value: f64) -> Result<u64, FactorError> {
    // `u64::MAX` rounds up to 2^64, the first whole number a tick cannot hold.
    if value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value < u64::MAX as f64 {
        Ok(value as u64)
    } else {
        Err(FactorError::BadTick {
            name: name.to_owned(),
            raw: value.to_string(),
        })
    }
}

fn every_level(id: &str, kind: &ParamKind) -> Result<Vec<FactorLevel>, FactorError> {
    let values = match kind {
        ParamKind::Bool { .. } => vec![ParamValue::Bool(false), ParamValue::Bool(true)],
        ParamKind::Choice { options: [], .. } => {
            return Err(FactorError::NoLevels {
                target: FactorTarget::Param(id.to_owned()),
            });
        }
        ParamKind::Choice { options, .. } => (0..options.len()).map(ParamValue::Choice).collect(),
        ParamKind::F32 { .. } | ParamKind::U32 { .. } => {
            return Err(FactorError::AllOverNumber {
                target: FactorTarget::Param(id.to_owned()),
            });
        }
    };
    Ok(values.into_iter().map(FactorLevel::Param).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        Factor, FactorDomain, FactorError, FactorLevel, FactorSlot, FactorSpec, FactorTarget, LevelSpec,
        LevelSpecError, MAX_LEVELS, RangeError, inclusive_steps,
    };
    use crate::explore::design::DesignKind;
    use crate::explore::spec::ActionSpec;
    use crate::helpers::{bool_param, choice_param, f32_param, u32_param};
    use crate::params::{ParamDescriptor, ParamValue};

    const NEIGHBORHOODS: &[&str] = &["moore", "von_neumann", "hexagonal"];

    fn descriptors() -> Vec<ParamDescriptor> {
        vec![
            f32_param("rate", "Rate", 0.5, 0.0, 1.0, Some(0.01)),
            u32_param("size", "Size", 10, 1, 100),
            bool_param("wrap", "Wrap", true),
            choice_param("neighborhood", "Neighborhood", NEIGHBORHOODS, 0),
        ]
    }

    fn actions() -> Vec<ActionSpec> {
        vec![ActionSpec::new("seed_outbreak", 40), ActionSpec::new("clear", 0)]
    }

    fn resolve(factor: &FactorSpec, design: &DesignKind) -> Result<Factor, FactorError> {
        factor.resolve(&descriptors(), &actions(), design)
    }

    fn levels(id: &str, levels: LevelSpec) -> Result<Vec<ParamValue>, FactorError> {
        let factor = resolve(&FactorSpec::param(id, levels), &DesignKind::Factorial)?;
        Ok(factor
            .levels()
            .expect("a factorial lists its levels")
            .iter()
            .map(|level| match level {
                FactorLevel::Param(value) => value.clone(),
                FactorLevel::Tick(tick) => panic!("a parameter factor gave tick {tick}"),
            })
            .collect())
    }

    fn ticks(levels: LevelSpec) -> Result<Vec<u64>, FactorError> {
        let factor = resolve(&FactorSpec::action("clear", levels), &DesignKind::Zip)?;
        assert_eq!(factor.slot, FactorSlot::Action(1));
        Ok(factor
            .levels()
            .expect("a zip lists its levels")
            .iter()
            .map(|level| match level {
                FactorLevel::Tick(tick) => *tick,
                FactorLevel::Param(value) => panic!("an action factor gave value {value:?}"),
            })
            .collect())
    }

    fn range(min: f64, max: f64, step: Option<f64>) -> LevelSpec {
        LevelSpec::Range { min, max, step }
    }

    #[test]
    fn a_stepped_range_is_inclusive_and_does_not_drift() {
        let values = levels("rate", range(0.0, 1.0, Some(0.1))).expect("0 to 1 is in bounds");
        assert_eq!(values.len(), 11, "both ends are levels");
        for (i, value) in values.iter().enumerate() {
            assert_eq!(*value, ParamValue::F32((i as f64 * 0.1) as f32), "level {i}");
        }
        assert_eq!(values[10], ParamValue::F32(1.0), "the last level is exactly the max");

        assert_eq!(inclusive_steps(0.1, 0.5, 0.1).map(|values| values.len()), Ok(5));
        let ends = inclusive_steps(0.1, 0.7, 0.2).expect("a valid range");
        assert_eq!(ends.last(), Some(&0.7), "0.1 + 3 * 0.2 is a rounding error past 0.7");
        let short = inclusive_steps(0.0, 0.6, 0.2).expect("a valid range");
        assert_eq!(short.len(), 4, "0.6 / 0.2 lands a rounding error short of 3");
        assert_eq!(short.last(), Some(&0.6));
    }

    #[test]
    fn a_range_whose_step_overshoots_stops_before_the_end() {
        let values = levels("rate", range(0.0, 1.0, Some(0.3))).expect("0 to 1 is in bounds");
        let expected: Vec<ParamValue> = (0..4).map(|i| ParamValue::F32((f64::from(i) * 0.3) as f32)).collect();
        assert_eq!(values, expected);
    }

    #[test]
    fn a_single_point_range_has_one_level() {
        assert_eq!(
            levels("rate", range(0.25, 0.25, Some(0.1))),
            Ok(vec![ParamValue::F32(0.25)])
        );
        assert_eq!(levels("size", range(7.0, 7.0, None)), Ok(vec![ParamValue::U32(7)]));
    }

    #[test]
    fn an_integer_range_counts_every_value() {
        let u32_levels = |values: &[u32]| values.iter().map(|&value| ParamValue::U32(value)).collect::<Vec<_>>();
        assert_eq!(levels("size", range(1.0, 5.0, None)), Ok(u32_levels(&[1, 2, 3, 4, 5])));
        assert_eq!(levels("size", range(2.0, 10.0, Some(4.0))), Ok(u32_levels(&[2, 6, 10])));
        assert_eq!(
            levels("size", range(1.0, 10.0, Some(3.0))),
            Ok(u32_levels(&[1, 4, 7, 10]))
        );
        assert_eq!(levels("size", range(1.0, 9.0, Some(3.0))), Ok(u32_levels(&[1, 4, 7])));
        assert_eq!(
            levels("size", range(1.0, 100.0, None)).map(|values| values.len()),
            Ok(100)
        );
        assert!(matches!(
            levels("size", range(1.5, 5.0, None)),
            Err(FactorError::NotWhole { .. })
        ));
        assert!(matches!(
            levels("size", range(1.0, 5.0, Some(1.5))),
            Err(FactorError::NotWhole { .. })
        ));
    }

    #[test]
    fn a_range_outside_the_bounds_is_refused() {
        for spec in [
            range(0.0, 1.5, Some(0.5)),
            range(-0.5, 1.0, Some(0.5)),
            range(0.0, 1.05, Some(0.1)),
            range(0.0, f64::INFINITY, Some(0.1)),
        ] {
            let error = levels("rate", spec.clone()).expect_err("an end outside 0..=1");
            assert!(matches!(error, FactorError::Level { .. }), "{spec:?} gave {error:?}");
        }
        for spec in [range(0.0, 5.0, None), range(1.0, 101.0, None), range(1.0, 5e9, None)] {
            let error = levels("size", spec.clone()).expect_err("an end outside 1..=100");
            assert!(matches!(error, FactorError::Level { .. }), "{spec:?} gave {error:?}");
        }
        let error = levels("rate", LevelSpec::Values(vec!["0.5".to_owned(), "2".to_owned()])).expect_err("2 > 1");
        assert!(matches!(error, FactorError::Level { .. }), "{error:?}");
    }

    #[test]
    fn a_zero_or_negative_step_is_refused() {
        for step in [0.0, -0.1, f64::NAN, f64::INFINITY] {
            let error = levels("rate", range(0.0, 1.0, Some(step))).expect_err("no usable step");
            assert!(
                matches!(
                    error,
                    FactorError::Range {
                        source: RangeError::BadStep { .. },
                        ..
                    }
                ),
                "step {step} gave {error:?}"
            );
            let error = levels("size", range(1.0, 5.0, Some(step))).expect_err("no usable step");
            assert!(
                matches!(
                    error,
                    FactorError::Range {
                        source: RangeError::BadStep { .. },
                        ..
                    }
                ),
                "step {step} gave {error:?}"
            );
        }
    }

    #[test]
    fn a_reversed_or_oversized_range_is_refused() {
        assert_eq!(
            inclusive_steps(1.0, 0.0, 0.1),
            Err(RangeError::Reversed { min: 1.0, max: 0.0 })
        );
        assert_eq!(inclusive_steps(0.0, 1.0, 1e-9), Err(RangeError::TooManyLevels));
        assert_eq!(
            inclusive_steps(0.0, MAX_LEVELS as f64, 1.0),
            Err(RangeError::TooManyLevels),
            "one value past the limit"
        );
        assert!(matches!(
            levels("size", range(5.0, 1.0, None)),
            Err(FactorError::Range {
                source: RangeError::Reversed { .. },
                ..
            })
        ));
    }

    #[test]
    fn every_option_of_a_choice_is_a_level() {
        let choices = levels("neighborhood", LevelSpec::All).expect("a choice has options");
        assert_eq!(
            choices,
            [ParamValue::Choice(0), ParamValue::Choice(1), ParamValue::Choice(2)]
        );
        let flags = levels("wrap", LevelSpec::All).expect("a bool has two values");
        assert_eq!(flags, [ParamValue::Bool(false), ParamValue::Bool(true)]);
        let named = levels(
            "neighborhood",
            LevelSpec::Values(vec!["hexagonal".to_owned(), "0".to_owned()]),
        );
        assert_eq!(
            named,
            Ok(vec![ParamValue::Choice(2), ParamValue::Choice(0)]),
            "by name or index"
        );
    }

    #[test]
    fn levels_a_kind_cannot_take_are_refused() {
        assert!(matches!(
            levels("rate", range(0.0, 1.0, None)),
            Err(FactorError::MissingStep { .. })
        ));
        assert!(matches!(
            levels("rate", LevelSpec::All),
            Err(FactorError::AllOverNumber { .. })
        ));
        assert!(matches!(
            levels("wrap", range(0.0, 1.0, Some(1.0))),
            Err(FactorError::RangeOverOptions { .. })
        ));
        assert!(matches!(
            levels("rate", LevelSpec::Values(Vec::new())),
            Err(FactorError::NoLevels { .. })
        ));
        let error = levels("speed", LevelSpec::All).expect_err("no such parameter");
        assert_eq!(
            error.to_string(),
            "unknown parameter 'speed', expected one of rate, size, wrap, neighborhood"
        );
    }

    #[test]
    fn a_resolved_factor_names_its_slot() {
        let factor = resolve(&FactorSpec::param("wrap", LevelSpec::All), &DesignKind::Factorial).expect("a bool");
        assert_eq!(factor.slot, FactorSlot::Param(2));
    }

    #[test]
    fn level_text_parses_lists_ranges_and_all() {
        assert_eq!(LevelSpec::parse("0.1:0.5:0.1"), Ok(range(0.1, 0.5, Some(0.1))));
        assert_eq!(LevelSpec::parse("16:64"), Ok(range(16.0, 64.0, None)));
        assert_eq!(LevelSpec::parse(" -1 : 1 : 0.5"), Ok(range(-1.0, 1.0, Some(0.5))));
        assert_eq!(
            LevelSpec::parse("Random,Geometric"),
            Ok(LevelSpec::Values(vec!["Random".to_owned(), "Geometric".to_owned()]))
        );
        assert_eq!(LevelSpec::parse("0.05"), Ok(LevelSpec::Values(vec!["0.05".to_owned()])));
        assert_eq!(LevelSpec::parse("all"), Ok(LevelSpec::All));
        assert!(matches!(
            LevelSpec::parse("0.1:x:0.1"),
            Err(LevelSpecError::NotANumber { part, .. }) if part == "x"
        ));
        assert_eq!(
            LevelSpec::parse("0:1:0.1:2").map_err(|error| error.to_string()),
            Err("invalid range '0:1:0.1:2', expected MIN:MAX:STEP or MIN:MAX".to_owned())
        );
    }

    #[test]
    fn an_action_factor_takes_ticks() {
        let values = |raw: &[&str]| LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect());
        assert_eq!(ticks(values(&["200", "400", " 600"])), Ok(vec![200, 400, 600]));
        assert_eq!(ticks(range(0.0, 400.0, Some(200.0))), Ok(vec![0, 200, 400]));
        assert_eq!(ticks(range(3.0, 5.0, None)), Ok(vec![3, 4, 5]));
        for bad in [
            values(&["-1"]),
            values(&["1.5"]),
            values(&["soon"]),
            range(-2.0, 4.0, None),
        ] {
            assert!(
                matches!(ticks(bad.clone()), Err(FactorError::BadTick { .. })),
                "{bad:?}"
            );
        }
        assert!(matches!(
            ticks(range(0.0, 4.0, Some(0.5))),
            Err(FactorError::NotWhole { .. })
        ));
        assert!(matches!(ticks(LevelSpec::All), Err(FactorError::AllOverNumber { .. })));
        let error =
            resolve(&FactorSpec::action("wave", LevelSpec::All), &DesignKind::Factorial).expect_err("no such action");
        assert_eq!(
            error.to_string(),
            "unknown action 'wave', expected one of seed_outbreak, clear"
        );
        let error = FactorSpec::action("wave", range(0.0, 1.0, None))
            .resolve(&descriptors(), &[], &DesignKind::Factorial)
            .expect_err("no actions");
        assert_eq!(error.to_string(), "unknown action 'wave' (spec declares no actions)");
        assert_eq!(FactorTarget::Action("wave".to_owned()).to_string(), "action 'wave'");
    }

    #[test]
    fn a_sampled_design_takes_a_whole_range_without_a_step() {
        let sampled = DesignKind::LatinHypercube { samples: 10 };
        let domain = |factor: FactorSpec| resolve(&factor, &sampled).map(|factor| factor.domain);
        assert_eq!(
            domain(FactorSpec::param("rate", range(0.25, 0.75, None))),
            Ok(FactorDomain::Continuous { min: 0.25, max: 0.75 })
        );
        assert_eq!(
            domain(FactorSpec::param("size", range(1.0, 100.0, None))),
            Ok(FactorDomain::WholeNumbers { min: 1, max: 100 })
        );
        assert_eq!(
            domain(FactorSpec::action("seed_outbreak", range(0.0, 1e12, None))),
            Ok(FactorDomain::WholeNumbers {
                min: 0,
                max: 1_000_000_000_000
            }),
            "a whole range is not listed, so it can pass the level limit"
        );
        let stepped = domain(FactorSpec::param("size", range(1.0, 9.0, Some(4.0)))).expect("a stepped range");
        assert_eq!(
            stepped,
            FactorDomain::Levels(vec![
                FactorLevel::Param(ParamValue::U32(1)),
                FactorLevel::Param(ParamValue::U32(5)),
                FactorLevel::Param(ParamValue::U32(9)),
            ]),
            "a stepped range stays a list"
        );
        assert!(matches!(
            domain(FactorSpec::param("rate", range(0.75, 0.25, None))),
            Err(FactorError::Range {
                source: RangeError::Reversed { .. },
                ..
            })
        ));
        assert!(matches!(
            domain(FactorSpec::param("rate", range(0.5, 1.5, None))),
            Err(FactorError::Level { .. })
        ));
    }
}
