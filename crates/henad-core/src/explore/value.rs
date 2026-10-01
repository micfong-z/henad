//! Parameter values written as text, read and checked against their descriptors.

use std::fmt;
use std::num::{ParseFloatError, ParseIntError};
use std::str::ParseBoolError;

use crate::params::{ParamDescriptor, ParamKind, ParamValue};

/// A parameter value that cannot be read, or does not fit its descriptor.
///
/// The `source` of text that does not read as its kind is the parser's error. It is `None` when
/// [`check_value`] was given a value of another kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    NotANumber {
        raw: String,
        source: Option<ParseFloatError>,
    },
    NotAnInteger {
        raw: String,
        source: Option<ParseIntError>,
    },
    NotABool {
        raw: String,
        source: Option<ParseBoolError>,
    },
    /// A number outside the descriptor's inclusive bounds. A non-finite number is outside every bound.
    OutOfRange {
        value: String,
        min: String,
        max: String,
    },
    /// Neither the index nor the name of an option.
    UnknownOption {
        raw: String,
        options: &'static [&'static str],
    },
    /// An id no descriptor has. `known` lists the ids the descriptors do have.
    UnknownParam {
        id: String,
        known: Vec<&'static str>,
    },
    /// An override with no `=` between the id and the value.
    BadOverride {
        raw: String,
    },
    /// A value refused by the descriptor of parameter `id`, for the reason in `source`.
    Param {
        id: String,
        source: Box<Self>,
    },
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotANumber { raw, .. } => write!(f, "'{raw}' is not a number"),
            Self::NotAnInteger { raw, .. } => write!(f, "'{raw}' is not an integer"),
            Self::NotABool { raw, .. } => write!(f, "'{raw}' is not a bool"),
            Self::OutOfRange { value, min, max } => write!(f, "{value} is outside {min}..={max}"),
            Self::UnknownOption { raw, options } if raw.parse::<usize>().is_ok() => {
                write!(f, "index {raw} is not one of {options:?}")
            }
            Self::UnknownOption { raw, options } => write!(f, "'{raw}' is not one of {options:?}"),
            Self::UnknownParam { id, .. } => write!(f, "model has no parameter '{id}'"),
            Self::BadOverride { raw } => write!(f, "bad --set '{raw}', expected ID=VALUE"),
            Self::Param { id, .. } => write!(f, "parameter '{id}'"),
        }
    }
}

impl std::error::Error for ValueError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotANumber {
                source: Some(error), ..
            } => Some(error),
            Self::NotAnInteger {
                source: Some(error), ..
            } => Some(error),
            Self::NotABool {
                source: Some(error), ..
            } => Some(error),
            Self::Param { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Split each raw `--set ID=VALUE` string into an `(id, value)` pair.
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

/// Start from every parameter's default, then apply the overrides by id.
///
/// # Errors
///
/// Returns [`ValueError::UnknownParam`] for an id no descriptor has, and [`ValueError::Param`] for a
/// value its descriptor refuses.
pub fn resolve_params(
    descriptors: &[ParamDescriptor],
    overrides: &[(String, String)],
) -> Result<Vec<ParamValue>, ValueError> {
    let mut values: Vec<ParamValue> = descriptors
        .iter()
        .map(|descriptor| descriptor.kind.default_value())
        .collect();

    for (id, raw) in overrides {
        let index = descriptors
            .iter()
            .position(|descriptor| descriptor.id == *id)
            .ok_or_else(|| ValueError::UnknownParam {
                id: id.clone(),
                known: descriptors.iter().map(|descriptor| descriptor.id).collect(),
            })?;
        values[index] = parse_value(&descriptors[index].kind, raw).map_err(|error| ValueError::Param {
            id: id.clone(),
            source: Box::new(error),
        })?;
    }

    Ok(values)
}

/// Parse a raw string into a [`ParamValue`] matching the descriptor's kind, and refuse whatever the
/// descriptor's own range does not allow.
///
/// The GUI cannot produce an out-of-range value, since it edits every parameter through a widget
/// built from this range. `--set` reaches the same parameter with nothing between it and `init`,
/// where a model sizes its buffers from the number it is given.
///
/// A choice reads an option's name first, then an option's index.
///
/// # Errors
///
/// Returns [`ValueError`] when `raw` does not read as the kind, or [`check_value`] refuses it.
pub fn parse_value(kind: &ParamKind, raw: &str) -> Result<ParamValue, ValueError> {
    let value = match kind {
        ParamKind::F32 { .. } => ParamValue::F32(raw.parse().map_err(|error| ValueError::NotANumber {
            raw: raw.to_owned(),
            source: Some(error),
        })?),
        ParamKind::U32 { .. } => ParamValue::U32(raw.parse().map_err(|error| ValueError::NotAnInteger {
            raw: raw.to_owned(),
            source: Some(error),
        })?),
        ParamKind::Bool { .. } => ParamValue::Bool(raw.parse().map_err(|error| ValueError::NotABool {
            raw: raw.to_owned(),
            source: Some(error),
        })?),
        ParamKind::Choice { options, .. } => {
            let index = options
                .iter()
                .position(|option| *option == raw)
                .or_else(|| raw.parse::<usize>().ok())
                .ok_or_else(|| ValueError::UnknownOption {
                    raw: raw.to_owned(),
                    options,
                })?;
            ParamValue::Choice(index)
        }
    };
    check_value(kind, &value)?;
    Ok(value)
}

/// Checks `value` against the type, bounds and options of `kind`.
///
/// # Errors
///
/// Returns [`ValueError::OutOfRange`] for a number outside the bounds or not finite, and
/// [`ValueError::UnknownOption`] for an index past the options. A value of another type gets the
/// error [`parse_value`] gives text of the wrong type.
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
        (ParamKind::F32 { .. }, other) => Err(ValueError::NotANumber {
            raw: plain_text(other),
            source: None,
        }),
        (ParamKind::U32 { .. }, other) => Err(ValueError::NotAnInteger {
            raw: plain_text(other),
            source: None,
        }),
        (ParamKind::Bool { .. }, other) => Err(ValueError::NotABool {
            raw: plain_text(other),
            source: None,
        }),
        (ParamKind::Choice { options, .. }, other) => Err(ValueError::UnknownOption {
            raw: plain_text(other),
            options,
        }),
    }
}

/// Returns `value` as text that [`parse_value`] reads back unchanged.
///
/// A number takes its shortest round-trip form, and a choice its option name.
pub fn format_value(kind: &ParamKind, value: &ParamValue) -> String {
    match (kind, value) {
        (ParamKind::Choice { options, .. }, ParamValue::Choice(index)) => options
            .get(*index)
            .map_or_else(|| index.to_string(), |&name| name.to_owned()),
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
    use super::{ValueError, check_value, format_value, parse_overrides, parse_value, resolve_params};
    use crate::helpers::{bool_param, f32_param, u32_param};
    use crate::params::{ParamApply, ParamDescriptor, ParamFormat, ParamKind, ParamValue};

    const OPTIONS: &[&str] = &["moore", "von_neumann"];

    fn descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("num_agents", "Agents", 1000, 1, 5_000_000),
            f32_param("cohesion", "Cohesion", 0.5, 0.0, 1.0, None),
            ParamDescriptor {
                id: "neighborhood",
                label: "Neighborhood",
                kind: ParamKind::Choice {
                    options: OPTIONS,
                    default: 0,
                },
                apply: ParamApply::Live,
                format: ParamFormat::Plain,
            },
        ]
    }

    fn resolve(raw: &str) -> Result<Vec<ParamValue>, ValueError> {
        let overrides = parse_overrides(&[raw.to_owned()])?;
        resolve_params(&descriptors(), &overrides)
    }

    /// Returns the error's message followed by the message of each source, joined by ": ".
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

    /// The regression. A slider cannot ask for four billion agents, `--set` used to be able to, and
    /// the number went straight to `init`.
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
            let kind = &descriptors[index].kind;
            let text = format_value(kind, &value);
            assert_eq!(parse_value(kind, &text), Ok(value), "{text}");
        }
        assert_eq!(
            format_value(&descriptors[2].kind, &ParamValue::Choice(1)),
            "von_neumann"
        );
    }

    /// The regression. A name that reads as a number used to be read as an index, so option "0" at index 1 came
    /// back as option 0.
    #[test]
    fn a_choice_reads_an_option_name_before_an_index() {
        let kind = ParamKind::Choice {
            options: &["low", "0"],
            default: 0,
        };
        assert_eq!(parse_value(&kind, "0"), Ok(ParamValue::Choice(1)), "the name wins");
        assert_eq!(
            parse_value(&kind, "1"),
            Ok(ParamValue::Choice(1)),
            "an index still reads"
        );
        let kind = ParamKind::Choice {
            options: &["low", "2"],
            default: 0,
        };
        for index in 0..2 {
            let value = ParamValue::Choice(index);
            assert_eq!(parse_value(&kind, &format_value(&kind, &value)), Ok(value));
        }
    }

    #[test]
    fn a_value_that_does_not_fit_its_kind_is_refused() {
        let descriptors = descriptors();
        assert!(check_value(&descriptors[0].kind, &ParamValue::F32(10.0)).is_err());
        assert!(check_value(&descriptors[1].kind, &ParamValue::U32(0)).is_err());
        assert!(check_value(&descriptors[1].kind, &ParamValue::F32(f32::INFINITY)).is_err());
        assert!(check_value(&descriptors[2].kind, &ParamValue::Choice(2)).is_err());
        assert_eq!(check_value(&descriptors[2].kind, &ParamValue::Choice(1)), Ok(()));
    }
}
