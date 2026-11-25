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
use crate::explore::value::{ValueError, check_param, parse_param, percent_fraction};
use crate::params::{ParamDescriptor, ParamFormat, ParamKind, ParamValue};

/// Maximum number of levels in one factor.
pub const MAX_LEVELS: u64 = 1 << 24;

/// Relative amount by which a range may miss a whole number of steps and still end on its `max`.
const ROUNDING: f64 = 1e-9;

/// Levels of a factor, as a spec writes them.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelSpec {
    /// Values in the text form that [`parse_param`] accepts, or ticks for an action.
    ///
    /// A plan trims the spaces around each value before it reads it.
    Values(Vec<String>),
    /// Every value from `min` to `max` inclusive, `step` apart.
    ///
    /// A whole-number factor takes a step of 1 when `step` is `None`. An `F32` parameter needs a step, except in a
    /// sampled design. A sampled design draws from the whole range.
    Range {
        /// Lowest value of the range.
        min: f64,
        /// Highest value the range can reach.
        max: f64,
        /// Distance between two consecutive values.
        step: Option<f64>,
    },
    /// Every value from `min` to `max` percent inclusive, `step` percent apart, for a parameter in the
    /// [`ParamFormat::Percent`] format.
    ///
    /// A plan reads it as the [`Self::Range`] of the same numbers divided by 100, so `1%:5%:1%` plans as
    /// `0.01:0.05:0.01` does.
    PercentRange {
        /// Lowest value of the range, in percent.
        min: f64,
        /// Highest value the range can reach, in percent.
        max: f64,
        /// Distance between two consecutive values, in percent.
        step: Option<f64>,
    },
    /// Every value of a `Bool` or `Choice` parameter.
    All,
}

impl LevelSpec {
    /// Reads levels as `--vary` writes them: `all`, a range `min:max:step` or `min:max`, or a list `v1,v2,...`.
    ///
    /// Spaces around the text, a listed value or a part of a range are ignored. A range with a `%` sign after every
    /// part, as in `1%:5%:1%`, is a [`Self::PercentRange`]. The model checks the levels when the sweep is planned.
    ///
    /// # Errors
    ///
    /// Returns [`LevelSpecError`] for a range with other than two or three parts, a part that is not a number, or a
    /// `%` sign after some parts of a range but not all.
    pub fn parse(text: &str) -> Result<Self, LevelSpecError> {
        let text = text.trim();
        if text == "all" {
            return Ok(Self::All);
        }
        if !text.contains(':') {
            return Ok(Self::Values(
                text.split(',').map(|value| value.trim().to_owned()).collect(),
            ));
        }
        let parts: Vec<&str> = text.split(':').collect();
        if !matches!(parts.len(), 2 | 3) {
            return Err(LevelSpecError::BadRange { range: text.to_owned() });
        }
        let percents = parts.iter().filter(|part| part.trim().ends_with('%')).count();
        if percents != 0 && percents != parts.len() {
            return Err(LevelSpecError::MixedPercent { range: text.to_owned() });
        }
        let numbers = parts
            .iter()
            .map(|part| {
                let trimmed = part.trim();
                let number = trimmed.strip_suffix('%').map_or(trimmed, str::trim_end);
                number.parse::<f64>().map_err(|source| LevelSpecError::NotANumber {
                    part: (*part).to_owned(),
                    range: text.to_owned(),
                    source,
                })
            })
            .collect::<Result<Vec<f64>, _>>()?;
        let (min, max, step) = (numbers[0], numbers[1], numbers.get(2).copied());
        Ok(if percents == 0 {
            Self::Range { min, max, step }
        } else {
            Self::PercentRange { min, max, step }
        })
    }

    /// Returns the levels with a [`Self::PercentRange`] written as the [`Self::Range`] of its fractions, and any
    /// other levels unchanged.
    ///
    /// A spec file writes a range in percent this way, and a plan reads both forms to the same levels.
    pub fn to_fractions(&self) -> Self {
        match *self {
            Self::PercentRange { min, max, step } => Self::Range {
                min: percent_fraction(min),
                max: percent_fraction(max),
                step: step.map(percent_fraction),
            },
            _ => self.clone(),
        }
    }
}

/// Returns the range `min:max:step` as `--vary` writes it, with `suffix` after each part.
fn range_text(min: f64, max: f64, step: Option<f64>, suffix: &str) -> String {
    match step {
        Some(step) => format!("{min}{suffix}:{max}{suffix}:{step}{suffix}"),
        None => format!("{min}{suffix}:{max}{suffix}"),
    }
}

/// Text that cannot be parsed as a [`LevelSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelSpecError {
    /// A part of range `range` that is not a number.
    NotANumber {
        /// Part that is not a number, as written.
        part: String,
        /// Range the part belongs to, as written, after trimming.
        range: String,
        /// Reason the part cannot be parsed as an `f64`.
        source: ParseFloatError,
    },
    /// A range with other than two or three parts.
    BadRange {
        /// Range as written, after trimming.
        range: String,
    },
    /// A range with a `%` sign after some of its parts but not all.
    MixedPercent {
        /// Range as written, after trimming.
        range: String,
    },
}

impl fmt::Display for LevelSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotANumber { part, range, .. } => write!(f, "'{part}' in range '{range}' is not a number"),
            Self::BadRange { range } => write!(f, "invalid range '{range}', expected MIN:MAX:STEP or MIN:MAX"),
            Self::MixedPercent { range } => {
                write!(
                    f,
                    "invalid range '{range}', expected a % sign after every part or no part"
                )
            }
        }
    }
}

impl std::error::Error for LevelSpecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotANumber { source, .. } => Some(source),
            Self::BadRange { .. } | Self::MixedPercent { .. } => None,
        }
    }
}

/// Part of a config that a factor varies, identified by the id or name that a spec uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactorTarget {
    /// Parameter with this id.
    Param(String),
    /// Tick of the action with this [`ActionSpec::name`].
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
    /// Parameter or action tick the factor varies.
    pub target: FactorTarget,
    /// Levels the factor takes, as written.
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
    /// Everywhere else the factor lists its levels. A parameter marked [`ParamDescriptor::integer`] reads its range
    /// as a `U32` parameter does: whole ends and steps, a step of 1 when none is given, and whole numbers alone in a
    /// sampled design.
    ///
    /// # Errors
    ///
    /// Returns [`FactorError`] for a parameter or action the sweep does not have, levels the target cannot take, or
    /// a level the target rejects.
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
    /// a level the target rejects.
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
                let Some(index) = params.iter().position(|descriptor| descriptor.id() == id) else {
                    return Err(FactorError::UnknownParam {
                        id: id.clone(),
                        known: params.iter().map(|descriptor| descriptor.id().to_owned()).collect(),
                    });
                };
                let descriptor = &params[index];
                let domain = match &self.levels {
                    LevelSpec::Values(raw) => FactorDomain::Levels(parsed_levels(descriptor, raw)?),
                    &LevelSpec::Range { min, max, step } => range_domain(descriptor, min, max, step, sampled)?,
                    &LevelSpec::PercentRange { min, max, step } if descriptor.format() == ParamFormat::Percent => {
                        let fraction = step.map(percent_fraction);
                        range_domain(
                            descriptor,
                            percent_fraction(min),
                            percent_fraction(max),
                            fraction,
                            sampled,
                        )?
                    }
                    &LevelSpec::PercentRange { min, max, step } => {
                        return Err(FactorError::Level {
                            id: id.clone(),
                            source: ValueError::UnexpectedPercent {
                                raw: range_text(min, max, step, "%"),
                            },
                        });
                    }
                    LevelSpec::All => FactorDomain::Levels(every_level(id, descriptor.kind())?),
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
                    &LevelSpec::PercentRange { min, max, step } => {
                        return Err(FactorError::BadTick {
                            name: name.clone(),
                            raw: range_text(min, max, step, "%"),
                        });
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
    /// Value of a parameter.
    Param(ParamValue),
    /// Tick of an action.
    Tick(u64),
}

/// Values a resolved factor can take.
#[derive(Debug, Clone, PartialEq)]
pub enum FactorDomain {
    /// Levels listed one by one.
    Levels(Vec<FactorLevel>),
    /// Every `f32` value from `min` to `max`, for a sampled design to draw from.
    Continuous {
        /// Lower bound of the draws.
        min: f64,
        /// Upper bound of the draws.
        max: f64,
    },
    /// Every whole number from `min` to `max` inclusive, for a sampled design to draw from.
    WholeNumbers {
        /// Lowest whole number a draw can take.
        min: u64,
        /// Highest whole number a draw can take.
        max: u64,
    },
    /// Every whole number from `min` to `max` inclusive, as the value of an `F32` parameter marked
    /// [`ParamDescriptor::integer`], for a sampled design to draw from.
    IntegerF32 {
        /// Lowest whole number a draw can take.
        min: i64,
        /// Highest whole number a draw can take.
        max: i64,
    },
}

impl FactorDomain {
    /// Returns the level of the whole number `offset` places above `min`, in the domain
    /// [`FactorDomain::IntegerF32`] starting at `min`.
    pub(crate) fn integer_f32_level(min: i64, offset: u64) -> FactorLevel {
        FactorLevel::Param(ParamValue::F32((i128::from(min) + i128::from(offset)) as f32))
    }

    /// Returns the number of whole numbers from `min` to `max`, less one, for `min` not above `max`.
    pub(crate) fn integer_span(min: i64, max: i64) -> u64 {
        (i128::from(max) - i128::from(min)) as u64
    }
}

/// A factor resolved against a model's parameters and a spec's actions, with every level checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Factor {
    /// Part of a config the factor writes.
    pub slot: FactorSlot,
    /// Values the factor takes.
    pub domain: FactorDomain,
}

impl Factor {
    /// Returns the listed levels, or `None` for a factor that takes a whole range.
    pub fn levels(&self) -> Option<&[FactorLevel]> {
        match &self.domain {
            FactorDomain::Levels(levels) => Some(levels),
            FactorDomain::Continuous { .. } | FactorDomain::WholeNumbers { .. } | FactorDomain::IntegerF32 { .. } => {
                None
            }
        }
    }

    /// Writes `level` into `config`.
    ///
    /// # Panics
    ///
    /// Panics when the kind of `level` does not match the slot, or the slot is past the end of `config`.
    pub fn apply(&self, level: &FactorLevel, config: &mut Config) {
        match (self.slot, level) {
            (FactorSlot::Param(index), FactorLevel::Param(value)) => config.params[index] = value.clone(),
            (FactorSlot::Action(index), &FactorLevel::Tick(tick)) => config.action_ticks[index] = tick,
            (slot, level) => panic!("level {level:?} does not fit slot {slot:?}"),
        }
    }
}

/// A range that yields no values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RangeError {
    /// A range with an end that is not finite.
    NotFinite {
        /// End the range starts from, as given.
        min: f64,
        /// End the range runs to, as given.
        max: f64,
    },
    /// A step that is zero, negative or not finite.
    BadStep {
        /// Step as given.
        step: f64,
    },
    /// A `min` above `max`.
    Reversed {
        /// End the range starts from, as given.
        min: f64,
        /// End the range runs to, as given.
        max: f64,
    },
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
    /// An id no descriptor has.
    UnknownParam {
        /// Id as given.
        id: String,
        /// Ids the descriptors have.
        known: Vec<String>,
    },
    /// A name that no action of the spec has.
    UnknownAction {
        /// Name as given.
        name: String,
        /// Names the spec's actions have.
        known: Vec<String>,
    },
    /// A level that parameter `id` rejects.
    Level {
        /// Id of the parameter.
        id: String,
        /// Reason the descriptor rejects the level.
        source: ValueError,
    },
    /// A tick of action `name` that is not a non-negative whole number.
    BadTick {
        /// Name of the action.
        name: String,
        /// Tick as listed, or a range end as `f64`'s `Display` writes it.
        raw: String,
    },
    /// A range over `target` that yields no values.
    Range {
        /// Parameter or action tick the range varies.
        target: FactorTarget,
        /// Reason the range yields no values.
        source: RangeError,
    },
    /// A range with no step over `F32` parameter `id`, in a design that lists levels.
    MissingStep {
        /// Id of the parameter.
        id: String,
    },
    /// A range end or step for a whole-number `target` that is not a whole number.
    NotWhole {
        /// Parameter or action tick the range varies.
        target: FactorTarget,
        /// Range end or step as given.
        value: f64,
    },
    /// `all` over a numeric `target`.
    AllOverNumber {
        /// Parameter or action tick `all` was given for.
        target: FactorTarget,
    },
    /// A range over `Bool` or `Choice` parameter `id`.
    RangeOverOptions {
        /// Id of the parameter.
        id: String,
    },
    /// An empty list of values for `target`.
    NoLevels {
        /// Parameter or action tick with no values.
        target: FactorTarget,
    },
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
            Self::AllOverNumber { target } => write!(f, "{target} is a number, expected a bool or choice for 'all'"),
            Self::RangeOverOptions { id } => {
                write!(
                    f,
                    "parameter '{id}' is a bool or choice and takes values or 'all', got a range"
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

fn parsed_levels(descriptor: &ParamDescriptor, raw: &[String]) -> Result<Vec<FactorLevel>, FactorError> {
    let id = descriptor.id();
    if raw.is_empty() {
        return Err(FactorError::NoLevels {
            target: FactorTarget::Param(id.to_owned()),
        });
    }
    raw.iter()
        .map(|text| {
            parse_param(descriptor, text.trim())
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
    descriptor: &ParamDescriptor,
    min: f64,
    max: f64,
    step: Option<f64>,
    sampled: bool,
) -> Result<FactorDomain, FactorError> {
    let id = descriptor.id();
    let target = FactorTarget::Param(id.to_owned());
    match *descriptor.kind() {
        ParamKind::F32 { .. } if descriptor.is_integer() => {
            let min = integer_end(descriptor, min)?;
            let max = integer_end(descriptor, max)?;
            integer_domain(&target, min, max, step, sampled)
        }
        ParamKind::F32 { .. } => {
            // Every value lies between the two ends, and rounding to f32 keeps that order.
            for end in [min, max] {
                check_param(descriptor, &ParamValue::F32(end as f32)).map_err(|source| FactorError::Level {
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
    if sampled && step.is_none() {
        return Ok(FactorDomain::WholeNumbers { min, max });
    }
    levels_by_step(min, max, step.map_or(1, |step| step as u64), &level).map_err(range_error)
}

/// Returns the whole numbers from `min` to `max` inclusive, `step` apart, as levels of an `F32` parameter marked
/// integer.
///
/// A sampled design with no step takes the whole range instead.
fn integer_domain(
    target: &FactorTarget,
    min: i64,
    max: i64,
    step: Option<f64>,
    sampled: bool,
) -> Result<FactorDomain, FactorError> {
    if min > max {
        return Err(FactorError::Range {
            target: target.clone(),
            source: RangeError::Reversed {
                min: min as f64,
                max: max as f64,
            },
        });
    }
    if sampled && step.is_none() {
        return Ok(FactorDomain::IntegerF32 { min, max });
    }
    let span = FactorDomain::integer_span(min, max);
    whole_domain(target, 0, span, step, false, |offset| {
        FactorDomain::integer_f32_level(min, offset)
    })
}

/// Returns `value`, an end of a range over the `F32` parameter `descriptor` marked integer, as a whole number.
///
/// An end past the range of `i64` is clamped to it.
fn integer_end(descriptor: &ParamDescriptor, value: f64) -> Result<i64, FactorError> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(FactorError::NotWhole {
            target: FactorTarget::Param(descriptor.id().to_owned()),
            value,
        });
    }
    check_param(descriptor, &ParamValue::F32(value as f32)).map_err(|source| FactorError::Level {
        id: descriptor.id().to_owned(),
        source,
    })?;
    Ok(value as i64)
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
        ParamKind::Choice { options, .. } if options.is_empty() => {
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
            f32_param("share", "Share", 0.01, 0.0, 1.0, None).percent(),
            f32_param("offset", "Offset", 3.0, -4.0, 8.0, Some(1.0)).integer(),
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
            "unknown parameter 'speed', expected one of rate, size, wrap, neighborhood, share, offset"
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

    /// A listed value drops its surrounding spaces, as a range and a tick do.
    #[test]
    fn spaces_around_a_listed_value_are_ignored() {
        assert_eq!(
            LevelSpec::parse(" moore, von_neumann "),
            Ok(LevelSpec::Values(vec!["moore".to_owned(), "von_neumann".to_owned()]))
        );
        assert_eq!(LevelSpec::parse(" all "), Ok(LevelSpec::All));
        let spaced = |raw: &[&str]| LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect());
        assert_eq!(
            levels("neighborhood", spaced(&["moore", " hexagonal "])),
            Ok(vec![ParamValue::Choice(0), ParamValue::Choice(2)]),
            "a spec file's values are trimmed when planned"
        );
        assert_eq!(
            levels("rate", spaced(&[" 0.1", "0.2 "])),
            Ok(vec![ParamValue::F32(0.1), ParamValue::F32(0.2)])
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

    /// Returns the message of `error` followed by each source's message, joined by ": ".
    fn message(error: &dyn std::error::Error) -> String {
        let mut text = error.to_string();
        let mut source = error.source();
        while let Some(inner) = source {
            text = format!("{text}: {inner}");
            source = inner.source();
        }
        text
    }

    /// A range written in percent plans the levels of the same range written as fractions, and only a percentage
    /// takes one.
    #[test]
    fn a_percent_range_plans_as_the_range_of_its_fractions() {
        let percent = |min, max, step| LevelSpec::PercentRange { min, max, step };
        assert_eq!(LevelSpec::parse("1%:5%:1%"), Ok(percent(1.0, 5.0, Some(1.0))));
        assert_eq!(LevelSpec::parse(" 0.5 % : 2.5% "), Ok(percent(0.5, 2.5, None)));
        assert_eq!(
            LevelSpec::parse("1%:5:1%").map_err(|error| error.to_string()),
            Err("invalid range '1%:5:1%', expected a % sign after every part or no part".to_owned())
        );
        assert!(matches!(
            LevelSpec::parse("1%:x%"),
            Err(LevelSpecError::NotANumber { part, .. }) if part == "x%"
        ));
        assert_eq!(
            levels("share", percent(1.0, 5.0, Some(1.0))),
            levels("share", range(0.01, 0.05, Some(0.01)))
        );
        assert_eq!(
            levels("share", percent(0.3, 0.9, Some(0.3))),
            levels("share", range(0.003, 0.009, Some(0.003)))
        );
        let uneven = levels("share", percent(1.0, 5.0, Some(1.5)));
        assert_eq!(uneven, levels("share", range(0.01, 0.05, Some(0.015))));
        assert_eq!(
            uneven.map(|levels| levels.len()),
            Ok(3),
            "a step that does not divide stops short of the end"
        );
        let listed = |raw: &[&str]| LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect());
        assert_eq!(
            levels("share", listed(&["1%", "2.5%", "0.05"])),
            levels("share", listed(&["0.01", "0.025", "0.05"]))
        );
        let sampled = DesignKind::LatinHypercube { samples: 4 };
        assert_eq!(
            resolve(&FactorSpec::param("share", percent(1.0, 5.0, None)), &sampled).map(|factor| factor.domain),
            Ok(FactorDomain::Continuous { min: 0.01, max: 0.05 })
        );

        let error = levels("rate", percent(1.0, 5.0, Some(1.0))).expect_err("rate is not a percentage");
        assert_eq!(
            message(&error),
            "parameter 'rate': '1%:5%:1%' is a percentage, expected a plain number"
        );
        let error = levels("size", listed(&["5%"])).expect_err("size is not a percentage");
        assert_eq!(
            message(&error),
            "parameter 'size': '5%' is a percentage, expected a plain number"
        );
        assert!(matches!(
            ticks(percent(1.0, 5.0, None)),
            Err(FactorError::BadTick { raw, .. }) if raw == "1%:5%"
        ));
    }

    /// A parameter marked integer takes whole levels: listed, stepped by 1 or by a whole step, and drawn from the
    /// whole numbers alone in a sampled design. A fraction is refused wherever it is typed.
    #[test]
    fn an_integer_parameter_takes_whole_levels() {
        let whole = |values: &[f32]| values.iter().map(|&value| ParamValue::F32(value)).collect::<Vec<_>>();
        assert_eq!(
            levels("offset", range(-2.0, 2.0, None)),
            Ok(whole(&[-2.0, -1.0, 0.0, 1.0, 2.0]))
        );
        assert_eq!(
            levels("offset", range(-4.0, 8.0, Some(4.0))),
            Ok(whole(&[-4.0, 0.0, 4.0, 8.0]))
        );
        let listed = LevelSpec::Values(vec!["-3".to_owned(), "7".to_owned()]);
        assert_eq!(levels("offset", listed), Ok(whole(&[-3.0, 7.0])));

        for spec in [range(-1.5, 2.0, None), range(-1.0, 2.0, Some(0.5))] {
            let error = levels("offset", spec.clone()).expect_err("not whole");
            assert!(matches!(error, FactorError::NotWhole { .. }), "{spec:?} gave {error:?}");
        }
        let error = levels("offset", LevelSpec::Values(vec!["2.5".to_owned()])).expect_err("not whole");
        assert_eq!(message(&error), "parameter 'offset': 2.5 is not an integer");
        assert!(matches!(
            levels("offset", range(-5.0, 2.0, None)),
            Err(FactorError::Level { .. })
        ));
        assert!(matches!(
            levels("offset", range(2.0, -2.0, None)),
            Err(FactorError::Range {
                source: RangeError::Reversed { .. },
                ..
            })
        ));

        let sampled = DesignKind::Random { samples: 4 };
        let domain =
            |spec: LevelSpec| resolve(&FactorSpec::param("offset", spec), &sampled).map(|factor| factor.domain);
        assert_eq!(
            domain(range(-4.0, 8.0, None)),
            Ok(FactorDomain::IntegerF32 { min: -4, max: 8 })
        );
        assert_eq!(
            domain(range(-4.0, 4.0, Some(4.0))),
            Ok(FactorDomain::Levels(
                whole(&[-4.0, 0.0, 4.0]).into_iter().map(FactorLevel::Param).collect()
            )),
            "a stepped range stays a list"
        );
    }
}
