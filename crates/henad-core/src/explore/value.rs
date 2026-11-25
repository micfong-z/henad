//! Parameter values written as text, read and checked against their descriptors.
//!
//! [`parse_param`] and [`check_param`] apply every rule of a descriptor. A parameter in the
//! [`ParamFormat::Percent`] format also reads text with a trailing `%`, as `2.5%` for `0.025`, and an `F32`
//! parameter marked [`ParamDescriptor::integer`] rejects a value with a fractional part. [`parse_value`] and
//! [`check_value`] apply the rules of the kind alone, and suit a value read back from results.

use std::fmt;
use std::num::{ParseFloatError, ParseIntError};
use std::str::ParseBoolError;

use crate::params::{ParamDescriptor, ParamFormat, ParamKind, ParamValue};

/// A parameter value that cannot be read, or does not fit its descriptor.
///
/// The `source` of text that cannot be parsed as its kind is the parser's error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// Text that cannot be parsed as an `f32`.
    NotANumber {
        /// Text as given.
        raw: String,
        /// Error of the `f32` parser.
        source: ParseFloatError,
    },
    /// Text that cannot be parsed as a `u32`.
    NotAnInteger {
        /// Text as given.
        raw: String,
        /// Error of the `u32` parser.
        source: ParseIntError,
    },
    /// Text that cannot be parsed as a `bool`.
    NotABool {
        /// Text as given.
        raw: String,
        /// Error of the `bool` parser.
        source: ParseBoolError,
    },
    /// A number outside the descriptor's inclusive bounds. A non-finite number is outside every bound.
    OutOfRange {
        /// Number as text.
        value: String,
        /// Lower bound as text.
        min: String,
        /// Upper bound as text.
        max: String,
    },
    /// Neither the index nor the name of an option.
    UnknownOption {
        /// Text as given, or the index as text.
        raw: String,
        /// Options of the parameter.
        options: Vec<String>,
    },
    /// An id no descriptor has.
    UnknownParam {
        /// Id as given.
        id: String,
        /// Ids the descriptors have.
        known: Vec<String>,
    },
    /// An override with no `=` between the id and the value.
    BadOverride {
        /// Override as given.
        raw: String,
    },
    /// A value that parameter `id` rejects.
    Param {
        /// Id of the parameter.
        id: String,
        /// Reason the descriptor rejects the value.
        source: Box<Self>,
    },
    /// A value with a fractional part, for an `F32` parameter marked [`ParamDescriptor::integer`].
    NotWhole {
        /// Number as text.
        value: String,
    },
    /// Text with a trailing `%` for a parameter that is not in the [`ParamFormat::Percent`] format.
    UnexpectedPercent {
        /// Text as given.
        raw: String,
    },
    /// A value passed to [`check_value`] whose kind differs from the descriptor's kind.
    WrongKind {
        /// Kind that the descriptor accepts.
        expected: ValueKind,
        /// Kind of the value.
        found: ValueKind,
        /// Value as text.
        value: String,
    },
}

/// Kind of a parameter value, as [`ValueError::WrongKind`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// An `f32`.
    F32,
    /// A `u32`.
    U32,
    /// A `bool`.
    Bool,
    /// An index into a choice's options.
    Choice,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::F32 => "f32",
            Self::U32 => "u32",
            Self::Bool => "bool",
            Self::Choice => "choice",
        })
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotANumber { raw, .. } => write!(f, "'{raw}' is not a number"),
            Self::NotAnInteger { raw, .. } => write!(f, "'{raw}' is not an integer"),
            Self::NotABool { raw, .. } => write!(f, "'{raw}' is not a bool"),
            Self::OutOfRange { value, min, max } => write!(f, "{value} is outside the range {min} to {max}"),
            Self::UnknownOption { raw, options } if raw.parse::<usize>().is_ok() => {
                write!(f, "index {raw} is not one of {options:?}")
            }
            Self::UnknownOption { raw, options } => write!(f, "'{raw}' is not one of {options:?}"),
            Self::UnknownParam { id, .. } => write!(f, "unknown parameter '{id}'"),
            Self::BadOverride { raw } => write!(f, "invalid --set '{raw}', expected ID=VALUE"),
            Self::Param { id, .. } => write!(f, "parameter '{id}'"),
            Self::NotWhole { value } => write!(f, "{value} is not an integer"),
            Self::UnexpectedPercent { raw } => write!(f, "'{raw}' is a percentage, expected a plain number"),
            Self::WrongKind { expected, found, value } => {
                let example = match expected {
                    ValueKind::F32 => "an f32 value such as 1.0f32",
                    ValueKind::U32 => "a u32 value such as 1u32",
                    ValueKind::Bool => "a bool value",
                    ValueKind::Choice => "a choice such as ParamValue::Choice(0)",
                };
                write!(f, "expected {example}, got the {found} {value}")
            }
        }
    }
}

impl std::error::Error for ValueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotANumber { source, .. } => Some(source),
            Self::NotAnInteger { source, .. } => Some(source),
            Self::NotABool { source, .. } => Some(source),
            Self::Param { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Splits each `--set ID=VALUE` string into an `(id, value)` pair at its first `=`.
///
/// # Errors
///
/// Returns [`ValueError::BadOverride`] for a string with no `=`.
pub fn parse_overrides(raw: &[String]) -> Result<Vec<(String, String)>, ValueError> {
    raw.iter()
        .map(|pair| {
            let (id, value) = pair
                .split_once('=')
                .ok_or_else(|| ValueError::BadOverride { raw: pair.clone() })?;
            Ok((id.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Returns every descriptor's default value, with `overrides` applied in order by id.
///
/// Each override is read by [`parse_param`].
///
/// # Errors
///
/// Returns [`ValueError::UnknownParam`] for an id no descriptor has, and [`ValueError::Param`] for a
/// value that its descriptor rejects.
pub fn resolve_params(
    descriptors: &[ParamDescriptor],
    overrides: &[(String, String)],
) -> Result<Vec<ParamValue>, ValueError> {
    let mut values: Vec<ParamValue> = descriptors
        .iter()
        .map(|descriptor| descriptor.kind().default_value())
        .collect();

    for (id, raw) in overrides {
        let index = descriptors
            .iter()
            .position(|descriptor| descriptor.id() == id)
            .ok_or_else(|| ValueError::UnknownParam {
                id: id.clone(),
                known: descriptors
                    .iter()
                    .map(|descriptor| descriptor.id().to_owned())
                    .collect(),
            })?;
        values[index] = parse_param(&descriptors[index], raw).map_err(|error| ValueError::Param {
            id: id.clone(),
            source: Box::new(error),
        })?;
    }

    Ok(values)
}

/// Reads `raw` as a value of the parameter `descriptor`, and checks it with [`check_param`].
///
/// A parameter in the [`ParamFormat::Percent`] format reads a number with a trailing `%` as that number divided by
/// 100, so `2.5%` reads as `0.025` does, and reads a number without one as the fraction itself. For a choice, `raw` is
/// read as an option name first, then as an option index.
///
/// # Errors
///
/// Returns [`ValueError::UnexpectedPercent`] for a number with a trailing `%` when the parameter is not a percentage,
/// [`ValueError`] when `raw` cannot be parsed as the kind, and the error of [`check_param`] for a value it rejects.
pub fn parse_param(descriptor: &ParamDescriptor, raw: &str) -> Result<ParamValue, ValueError> {
    let kind = descriptor.kind();
    let value = match (kind, raw.strip_suffix('%')) {
        (ParamKind::F32 { .. }, Some(number)) if descriptor.format() == ParamFormat::Percent => {
            ParamValue::F32(read_percent(raw, number.trim_end())?)
        }
        (ParamKind::F32 { .. } | ParamKind::U32 { .. }, Some(_)) => {
            return Err(ValueError::UnexpectedPercent { raw: raw.to_owned() });
        }
        _ => read_value(kind, raw)?,
    };
    check_param(descriptor, &value)?;
    Ok(value)
}

/// Reads `raw` as a value of `kind`, and checks it with [`check_value`].
///
/// For a choice, `raw` is read as an option name first, then as an option index. Note that the rules of a descriptor
/// beyond its kind, a trailing `%` and a whole number, are left to [`parse_param`].
///
/// # Errors
///
/// Returns [`ValueError`] when `raw` cannot be parsed as the kind, or [`check_value`] rejects it.
pub fn parse_value(kind: &ParamKind, raw: &str) -> Result<ParamValue, ValueError> {
    let value = read_value(kind, raw)?;
    check_value(kind, &value)?;
    Ok(value)
}

/// Reads `raw` as a value of `kind`, without checking it against the bounds or options.
fn read_value(kind: &ParamKind, raw: &str) -> Result<ParamValue, ValueError> {
    Ok(match kind {
        ParamKind::F32 { .. } => ParamValue::F32(raw.parse().map_err(|source| ValueError::NotANumber {
            raw: raw.to_owned(),
            source,
        })?),
        ParamKind::U32 { .. } => ParamValue::U32(raw.parse().map_err(|source| ValueError::NotAnInteger {
            raw: raw.to_owned(),
            source,
        })?),
        ParamKind::Bool { .. } => ParamValue::Bool(raw.parse().map_err(|source| ValueError::NotABool {
            raw: raw.to_owned(),
            source,
        })?),
        ParamKind::Choice { options, .. } => {
            let index = options
                .position(raw)
                .or_else(|| raw.parse::<usize>().ok())
                .ok_or_else(|| ValueError::UnknownOption {
                    raw: raw.to_owned(),
                    options: options.iter().map(str::to_owned).collect(),
                })?;
            ParamValue::Choice(index)
        }
    })
}

/// Returns `number`, the text of percentage `raw` without its `%` sign, as a fraction.
///
/// The decimal exponent moves two places, so the fraction rounds to `f32` once, as the fraction written out would.
fn read_percent(raw: &str, number: &str) -> Result<f32, ValueError> {
    let percent: f32 = number.parse().map_err(|source| ValueError::NotANumber {
        raw: raw.to_owned(),
        source,
    })?;
    Ok(fraction_text(number)
        .and_then(|text| text.parse().ok())
        .unwrap_or(percent / 100.0))
}

/// Returns `percent` as a fraction, with the decimal exponent of its shortest text moved two places.
///
/// A range written in percent reaches the same `f64` as the range written as fractions.
pub(crate) fn percent_fraction(percent: f64) -> f64 {
    fraction_text(&percent.to_string())
        .and_then(|text| text.parse().ok())
        .unwrap_or(percent / 100.0)
}

/// Returns the decimal number `number` divided by 100, as text with its exponent lowered by two, or `None` for text
/// whose exponent does not read.
fn fraction_text(number: &str) -> Option<String> {
    match number.find(['e', 'E']) {
        Some(at) => {
            let exponent: i32 = number[at + 1..].parse().ok()?;
            Some(format!("{}e{}", &number[..at], exponent.checked_sub(2)?))
        }
        None => Some(format!("{number}e-2")),
    }
}

/// Checks `value` against the type, bounds and options of `descriptor`, and against its [`integer`] mark.
///
/// # Errors
///
/// Returns the error of [`check_value`], and [`ValueError::NotWhole`] for a value with a fractional part when the
/// parameter is marked [`integer`].
///
/// [`integer`]: ParamDescriptor::integer
pub fn check_param(descriptor: &ParamDescriptor, value: &ParamValue) -> Result<(), ValueError> {
    check_value(descriptor.kind(), value)?;
    match value {
        ParamValue::F32(number) if descriptor.is_integer() && number.fract() != 0.0 => Err(ValueError::NotWhole {
            value: number.to_string(),
        }),
        _ => Ok(()),
    }
}

/// Checks `value` against the type, bounds and options of `kind`.
///
/// Note that the [`integer`](ParamDescriptor::integer) mark of a descriptor is left to [`check_param`].
///
/// # Errors
///
/// Returns [`ValueError::OutOfRange`] for a number outside the bounds or not finite,
/// [`ValueError::UnknownOption`] for an index past the options, and [`ValueError::WrongKind`] for a value of another
/// kind. A `u32` is never accepted as a choice index.
pub fn check_value(kind: &ParamKind, value: &ParamValue) -> Result<(), ValueError> {
    let out_of_range = |value: &dyn fmt::Display, min: &dyn fmt::Display, max: &dyn fmt::Display| {
        Err(ValueError::OutOfRange {
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        })
    };
    match (kind, value) {
        (ParamKind::F32 { min, max, .. }, ParamValue::F32(number)) => {
            if number.is_finite() && number >= min && number <= max {
                Ok(())
            } else {
                out_of_range(number, min, max)
            }
        }
        (ParamKind::U32 { min, max, .. }, ParamValue::U32(number)) => {
            if number >= min && number <= max {
                Ok(())
            } else {
                out_of_range(number, min, max)
            }
        }
        (ParamKind::Bool { .. }, ParamValue::Bool(_)) => Ok(()),
        (ParamKind::Choice { options, .. }, ParamValue::Choice(index)) if *index < options.len() => Ok(()),
        (ParamKind::Choice { options, .. }, ParamValue::Choice(index)) => Err(ValueError::UnknownOption {
            raw: index.to_string(),
            options: options.iter().map(str::to_owned).collect(),
        }),
        (kind, other) => Err(ValueError::WrongKind {
            expected: param_kind(kind),
            found: value_kind(other),
            value: plain_text(other),
        }),
    }
}

/// Returns the kind of value that `kind` accepts.
fn param_kind(kind: &ParamKind) -> ValueKind {
    match kind {
        ParamKind::F32 { .. } => ValueKind::F32,
        ParamKind::U32 { .. } => ValueKind::U32,
        ParamKind::Bool { .. } => ValueKind::Bool,
        ParamKind::Choice { .. } => ValueKind::Choice,
    }
}

/// Returns the kind of `value`.
fn value_kind(value: &ParamValue) -> ValueKind {
    match value {
        ParamValue::F32(_) => ValueKind::F32,
        ParamValue::U32(_) => ValueKind::U32,
        ParamValue::Bool(_) => ValueKind::Bool,
        ParamValue::Choice(_) => ValueKind::Choice,
    }
}

/// Returns `value` as text that [`parse_value`] reads back unchanged.
///
/// A number is written in its shortest round-trip form, and a choice as its option name.
pub fn format_value(kind: &ParamKind, value: &ParamValue) -> String {
    match (kind, value) {
        (ParamKind::Choice { options, .. }, ParamValue::Choice(index)) => {
            options.get(*index).map_or_else(|| index.to_string(), str::to_owned)
        }
        _ => plain_text(value),
    }
}

/// Returns `value` as text without its descriptor, so a choice is written as its index.
fn plain_text(value: &ParamValue) -> String {
    match value {
        ParamValue::F32(number) => number.to_string(),
        ParamValue::U32(number) => number.to_string(),
        ParamValue::Bool(flag) => flag.to_string(),
        ParamValue::Choice(index) => index.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ValueError, ValueKind, check_param, check_value, format_value, parse_overrides, parse_param, parse_value,
        resolve_params,
    };
    use crate::helpers::{bool_param, f32_param, u32_param};
    use crate::params::{ChoiceOptions, ParamDescriptor, ParamKind, ParamValue};

    const OPTIONS: &[&str] = &["moore", "von_neumann"];

    fn descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("num_agents", "Agents", 1000, 1, 5_000_000),
            f32_param("cohesion", "Cohesion", 0.5, 0.0, 1.0, None),
            ParamDescriptor::new(
                "neighborhood",
                "Neighborhood",
                ParamKind::Choice {
                    options: ChoiceOptions::from_static(OPTIONS),
                    default: 0,
                },
            ),
        ]
    }

    fn resolve(raw: &str) -> Result<Vec<ParamValue>, ValueError> {
        let overrides = parse_overrides(&[raw.to_owned()])?;
        resolve_params(&descriptors(), &overrides)
    }

    /// Returns the error's message followed by each source's message, joined by ": ".
    fn message(error: &ValueError) -> String {
        let mut text = error.to_string();
        let mut source = std::error::Error::source(error);
        while let Some(inner) = source {
            text = format!("{text}: {inner}");
            source = inner.source();
        }
        text
    }

    #[test]
    fn an_override_replaces_one_default_and_leaves_the_rest() {
        let values = resolve("num_agents=2500").expect("2500 agents is in range");
        assert_eq!(values[0], ParamValue::U32(2500));
        assert_eq!(values[1], ParamValue::F32(0.5));
        assert_eq!(values[2], ParamValue::Choice(0));
    }

    /// A slider cannot request four billion agents, and `--set` cannot either. Unchecked, the number would go
    /// straight to `init`.
    #[test]
    fn a_value_outside_the_descriptor_range_is_refused() {
        let error = resolve("num_agents=4000000000").expect_err("4e9 agents is over the maximum");
        assert!(message(&error).contains("5000000"), "{}", message(&error));

        let error = resolve("num_agents=0").expect_err("0 agents is under the minimum");
        assert!(message(&error).contains("num_agents"), "{}", message(&error));

        assert!(resolve("cohesion=1.5").is_err(), "1.5 is over the maximum");
        assert!(resolve("cohesion=-0.5").is_err(), "-0.5 is under the minimum");
        assert!(resolve("cohesion=nan").is_err(), "NaN is in no range");
    }

    /// Both ends of the range are allowed.
    #[test]
    fn the_limits_themselves_are_accepted() {
        assert_eq!(resolve("cohesion=0").expect("the minimum")[1], ParamValue::F32(0.0));
        assert_eq!(resolve("cohesion=1").expect("the maximum")[1], ParamValue::F32(1.0));
        assert_eq!(
            resolve("num_agents=5000000").expect("the maximum")[0],
            ParamValue::U32(5_000_000)
        );
    }

    /// A choice is an index into the option list, by number or by label.
    #[test]
    fn a_choice_index_is_checked_against_the_options() {
        assert_eq!(
            resolve("neighborhood=1").expect("index 1 exists")[2],
            ParamValue::Choice(1)
        );
        assert_eq!(
            resolve("neighborhood=von_neumann").expect("a label")[2],
            ParamValue::Choice(1)
        );
        assert!(resolve("neighborhood=2").is_err(), "there is no third option");
        assert!(resolve("neighborhood=hexagonal").is_err(), "no such label");
    }

    #[test]
    fn an_unknown_parameter_is_refused() {
        assert!(resolve("no_such_param=1").is_err());
        assert!(
            parse_overrides(&["num_agents".to_owned()]).is_err(),
            "no '=' in the pair"
        );
    }

    #[test]
    fn text_of_the_wrong_kind_keeps_the_parser_error() {
        let error = resolve("num_agents=abc").expect_err("not an integer");
        assert_eq!(
            message(&error),
            "parameter 'num_agents': 'abc' is not an integer: invalid digit found in string",
            "the parser's error is the last source"
        );
        let error = resolve("cohesion=high").expect_err("not a number");
        assert!(
            message(&error).ends_with("invalid float literal"),
            "{}",
            message(&error)
        );
    }

    #[test]
    fn a_formatted_value_parses_back_to_itself() {
        let mut descriptors = descriptors();
        descriptors.push(bool_param("wrap", "Wrap", true));
        let cases = [
            (0, ParamValue::U32(1)),
            (0, ParamValue::U32(5_000_000)),
            (1, ParamValue::F32(0.0)),
            (1, ParamValue::F32(0.1)),
            (1, ParamValue::F32(1.0 / 3.0)),
            (1, ParamValue::F32(f32::from_bits(1))),
            (1, ParamValue::F32(1.0)),
            (2, ParamValue::Choice(0)),
            (2, ParamValue::Choice(1)),
            (3, ParamValue::Bool(false)),
            (3, ParamValue::Bool(true)),
        ];
        for (index, value) in cases {
            let kind = descriptors[index].kind();
            let text = format_value(kind, &value);
            assert_eq!(parse_value(kind, &text), Ok(value), "{text}");
        }
        assert_eq!(
            format_value(descriptors[2].kind(), &ParamValue::Choice(1)),
            "von_neumann"
        );
    }

    /// A name that looks like a number is matched as a name first. Read as an index, option "0" at index 1 would
    /// come back as option 0.
    #[test]
    fn a_choice_reads_an_option_name_before_an_index() {
        let kind = ParamKind::Choice {
            options: ChoiceOptions::from_static(&["low", "0"]),
            default: 0,
        };
        assert_eq!(parse_value(&kind, "0"), Ok(ParamValue::Choice(1)), "the name wins");
        assert_eq!(
            parse_value(&kind, "1"),
            Ok(ParamValue::Choice(1)),
            "an index still reads"
        );
        let kind = ParamKind::Choice {
            options: ChoiceOptions::from_static(&["low", "2"]),
            default: 0,
        };
        for index in 0..2 {
            let value = ParamValue::Choice(index);
            assert_eq!(parse_value(&kind, &format_value(&kind, &value)), Ok(value));
        }
    }

    /// A choice that owns its option names reads and writes them as a borrowed one does.
    #[test]
    fn a_choice_with_owned_options_parses_by_name() {
        let names = ["moore", "von_neumann", "hexagonal"].map(str::to_owned);
        let owned = ParamDescriptor::new(
            "neighborhood".to_owned(),
            "Neighborhood".to_owned(),
            ParamKind::Choice {
                options: ChoiceOptions::new(names.to_vec()),
                default: 0,
            },
        );
        let kind = owned.kind();
        assert_eq!(parse_value(kind, "hexagonal"), Ok(ParamValue::Choice(2)));
        assert_eq!(
            parse_value(kind, "1"),
            Ok(ParamValue::Choice(1)),
            "an index still reads"
        );
        assert_eq!(format_value(kind, &ParamValue::Choice(1)), "von_neumann");
        let error = parse_value(kind, "triangle").expect_err("no such option");
        assert_eq!(
            error,
            ValueError::UnknownOption {
                raw: "triangle".to_owned(),
                options: names.to_vec(),
            }
        );
        assert_eq!(
            error.to_string(),
            r#"'triangle' is not one of ["moore", "von_neumann", "hexagonal"]"#
        );

        let overrides = parse_overrides(&["neighborhood=von_neumann".to_owned()]).expect("one pair");
        let values = resolve_params(std::slice::from_ref(&owned), &overrides).expect("a declared option");
        assert_eq!(values, [ParamValue::Choice(1)]);
        let error = resolve_params(&[owned], &[("shape".to_owned(), "1".to_owned())]).expect_err("no such id");
        assert_eq!(
            error,
            ValueError::UnknownParam {
                id: "shape".to_owned(),
                known: vec!["neighborhood".to_owned()],
            }
        );
    }

    #[test]
    fn a_value_that_does_not_fit_its_kind_is_refused() {
        let descriptors = descriptors();
        assert!(check_value(descriptors[0].kind(), &ParamValue::F32(10.0)).is_err());
        assert!(check_value(descriptors[1].kind(), &ParamValue::U32(0)).is_err());
        assert!(check_value(descriptors[1].kind(), &ParamValue::F32(f32::INFINITY)).is_err());
        assert!(check_value(descriptors[2].kind(), &ParamValue::Choice(2)).is_err());
        assert_eq!(check_value(descriptors[2].kind(), &ParamValue::Choice(1)), Ok(()));
    }

    /// The message for a value of another kind includes the literal that fits.
    #[test]
    fn a_value_of_another_kind_names_both_kinds() {
        let descriptors = descriptors();
        let error = check_value(descriptors[1].kind(), &ParamValue::U32(1)).expect_err("a u32 for an f32");
        assert_eq!(
            error,
            ValueError::WrongKind {
                expected: ValueKind::F32,
                found: ValueKind::U32,
                value: "1".to_owned(),
            }
        );
        assert_eq!(error.to_string(), "expected an f32 value such as 1.0f32, got the u32 1");
        let error = check_value(descriptors[0].kind(), &ParamValue::F32(256.0)).expect_err("an f32 for a u32");
        assert_eq!(error.to_string(), "expected a u32 value such as 1u32, got the f32 256");
        let error = check_value(descriptors[2].kind(), &ParamValue::U32(1)).expect_err("a u32 for a choice");
        assert!(
            matches!(
                error,
                ValueError::WrongKind {
                    expected: ValueKind::Choice,
                    found: ValueKind::U32,
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "expected a choice such as ParamValue::Choice(0), got the u32 1"
        );
        let wrap = bool_param("wrap", "Wrap", true);
        let error = check_value(wrap.kind(), &ParamValue::Choice(0)).expect_err("a choice for a bool");
        assert_eq!(error.to_string(), "expected a bool value, got the choice 0");
    }

    /// A percentage reads with its `%` sign as the fraction written out does, and without it as the fraction itself.
    #[test]
    fn a_percentage_reads_with_or_without_its_sign() {
        let share = f32_param("share", "Share", 0.01, 0.0, 1.0, None).percent();
        for (text, fraction) in [
            ("1%", "0.01"),
            ("2.5%", "0.025"),
            ("0.3%", "0.003"),
            ("1e1%", "0.1"),
            ("100%", "1"),
            ("7 %", "0.07"),
            ("0.025", "0.025"),
        ] {
            assert_eq!(
                parse_param(&share, text),
                parse_param(&share, fraction),
                "{text} reads as {fraction}"
            );
        }
        assert!(matches!(
            parse_param(&share, "150%"),
            Err(ValueError::OutOfRange { .. })
        ));
        let error = parse_param(&share, "half%").expect_err("not a number");
        assert!(
            matches!(&error, ValueError::NotANumber { raw, .. } if raw == "half%"),
            "{error:?}"
        );
        assert!(
            parse_value(share.kind(), "1%").is_err(),
            "the kind alone does not know the format"
        );
    }

    /// A percentage reads an exponent, a negative number and spaces before its sign, and refuses a sign alone or twice.
    #[test]
    fn a_percentage_reads_its_edge_cases() {
        let shift = f32_param("shift", "Shift", 0.0, -1.0, 1.0, None).percent();
        for (text, fraction) in [
            ("1e2%", "1"),
            ("-2.5%", "-0.025"),
            ("-100%", "-1"),
            ("33.3%", "0.333"),
            ("0.1 %", "0.001"),
        ] {
            assert_eq!(parse_param(&shift, text), parse_param(&shift, fraction), "{text}");
            assert!(parse_param(&shift, text).is_ok(), "{text}");
        }
        for text in ["%", "2.5%%", "e2%", "1e%"] {
            let error = parse_param(&shift, text).expect_err("not a number");
            assert!(
                matches!(&error, ValueError::NotANumber { raw, .. } if raw == text),
                "{text}: {error:?}"
            );
        }
        assert!(matches!(
            parse_param(&shift, "-101%"),
            Err(ValueError::OutOfRange { .. })
        ));
    }

    /// A `%` sign on a parameter that is not a percentage is refused, and `--set` names the parameter.
    #[test]
    fn a_percent_sign_on_a_plain_parameter_is_refused() {
        for raw in ["num_agents=5%", "cohesion=50%"] {
            let error = resolve(raw).expect_err("neither parameter is a percentage");
            let id = raw.split('=').next().unwrap_or_default();
            let value = raw.split('=').nth(1).unwrap_or_default();
            assert_eq!(
                message(&error),
                format!("parameter '{id}': '{value}' is a percentage, expected a plain number")
            );
        }
    }

    /// A parameter marked integer rejects a fraction from text and from a value, and takes a whole number in either
    /// form. The kind alone accepts the fraction.
    #[test]
    fn an_integer_parameter_rejects_a_fraction() {
        let birth = f32_param("birth", "Birth", 3.0, -1.0, 8.0, Some(1.0)).integer();
        assert!(birth.is_integer());
        for (text, value) in [("-1", -1.0), ("3", 3.0), ("3.0", 3.0), ("8", 8.0)] {
            assert_eq!(parse_param(&birth, text), Ok(ParamValue::F32(value)), "{text}");
        }
        assert_eq!(
            parse_param(&birth, "2.5"),
            Err(ValueError::NotWhole {
                value: "2.5".to_owned()
            })
        );
        assert_eq!(
            check_param(&birth, &ParamValue::F32(-0.5)).map_err(|error| error.to_string()),
            Err("-0.5 is not an integer".to_owned())
        );
        assert!(matches!(
            check_param(&birth, &ParamValue::F32(9.0)),
            Err(ValueError::OutOfRange { .. })
        ));
        assert_eq!(check_value(birth.kind(), &ParamValue::F32(2.5)), Ok(()));
        let overrides = parse_overrides(&["birth=2.5".to_owned()]).expect("one pair");
        let error = resolve_params(std::slice::from_ref(&birth), &overrides).expect_err("2.5 is not whole");
        assert_eq!(message(&error), "parameter 'birth': 2.5 is not an integer");

        let plain = f32_param("birth", "Birth", 3.0, -1.0, 8.0, Some(1.0));
        assert!(!plain.is_integer());
        assert_eq!(parse_param(&plain, "2.5"), Ok(ParamValue::F32(2.5)));
        assert!(
            !u32_param("size", "Size", 1, 0, 9).integer().is_integer(),
            "only an f32 takes the mark"
        );
    }
}
