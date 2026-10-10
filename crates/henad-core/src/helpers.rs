//! Builders for parameter descriptors and stat entries, parameter value readers, and
//! [`fmt_bytes`].

use crate::params::{ChoiceOptions, ParamDescriptor, ParamKind, ParamValue};
use crate::view::{StatEntry, StatValue};

/// Formats a byte count in B, KB, MB or GB, counting in powers of 1024, to one decimal place above B.
pub fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KB", bytes as f64 / (1u64 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
}

// Parameter descriptor builders.

/// Returns a descriptor for a live `f32` parameter, with `step` as its slider step.
pub fn f32_param(
    id: &'static str,
    label: &'static str,
    default: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
) -> ParamDescriptor {
    ParamDescriptor::new(
        id,
        label,
        ParamKind::F32 {
            min,
            max,
            default,
            step,
        },
    )
}

/// Returns a descriptor for a live `u32` parameter.
pub fn u32_param(id: &'static str, label: &'static str, default: u32, min: u32, max: u32) -> ParamDescriptor {
    ParamDescriptor::new(id, label, ParamKind::U32 { min, max, default })
}

/// Returns a descriptor for a live switch.
pub fn bool_param(id: &'static str, label: &'static str, default: bool) -> ParamDescriptor {
    ParamDescriptor::new(id, label, ParamKind::Bool { default })
}

/// Returns a descriptor for a choice among `options`, starting at option `default`.
///
/// # Panics
///
/// Panics when [`check_choice_options`] refuses `options`.
pub fn choice_param(
    id: &'static str,
    label: &'static str,
    options: &'static [&'static str],
    default: usize,
) -> ParamDescriptor {
    if let Err(reason) = check_choice_options(options) {
        panic!("choice '{id}': {reason}");
    }
    ParamDescriptor::new(
        id,
        label,
        ParamKind::Choice {
            options: ChoiceOptions::from_static(options),
            default,
        },
    )
}

/// Checks the option names of a choice.
///
/// Text that selects a choice is trimmed, then matched against the option names, then parsed as an index. The second
/// of two equal names would resolve to the first option, a name with spaces at an end would never match, and a name
/// that is an unsigned integer would shadow the index it spells.
///
/// The level text of `--vary` and the app's Sweep tab splits at each comma, reads as a range when it holds a colon, and
/// reads `all` as every option. A name holding a comma or a colon, or the name `all`, cannot be listed in it.
///
/// # Errors
///
/// Returns a sentence naming the first option that is empty, has spaces at either end, parses as an unsigned integer,
/// holds a comma or a colon, is `all`, or repeats an earlier name.
pub fn check_choice_options<S: AsRef<str>>(options: &[S]) -> Result<(), String> {
    for (position, option) in options.iter().enumerate() {
        let option = option.as_ref();
        if option.is_empty() || option.trim() != option {
            return Err(format!("option '{option}' is empty or has spaces at an end"));
        }
        if option.parse::<usize>().is_ok() {
            return Err(format!("option '{option}' reads as an index"));
        }
        if option.contains([',', ':']) || option == "all" {
            return Err(format!("option '{option}' cannot be listed in a sweep's levels"));
        }
        if options[..position].iter().any(|earlier| earlier.as_ref() == option) {
            return Err(format!("option '{option}' appears twice"));
        }
    }
    Ok(())
}

// Parameter value readers.

/// Returns the `f32` at `index`, or `default` when the index is missing or holds another kind.
pub fn extract_f32(params: &[ParamValue], index: usize, default: f32) -> f32 {
    match params.get(index) {
        Some(ParamValue::F32(v)) => *v,
        _ => default,
    }
}

/// Returns the `u32` at `index`, or `default` when the index is missing or holds another kind.
pub fn extract_u32(params: &[ParamValue], index: usize, default: u32) -> u32 {
    match params.get(index) {
        Some(ParamValue::U32(v)) => *v,
        _ => default,
    }
}

/// Returns the `bool` at `index`, or `default` when the index is missing or holds another kind.
pub fn extract_bool(params: &[ParamValue], index: usize, default: bool) -> bool {
    match params.get(index) {
        Some(ParamValue::Bool(v)) => *v,
        _ => default,
    }
}

/// Returns the index of the chosen option, or `default` if the value is not a choice.
pub fn extract_choice(params: &[ParamValue], index: usize, default: usize) -> usize {
    match params.get(index) {
        Some(ParamValue::Choice(v)) => *v,
        _ => default,
    }
}

// Stat entry builders.

/// Returns an entry holding a scalar.
pub fn stat(label: &'static str, value: f64, color: [u8; 4]) -> StatEntry {
    StatEntry::new(label, StatValue::Scalar(value), color)
}

/// Returns an entry holding a 2D vector.
pub fn stat_vec2(label: &'static str, x: f64, y: f64, color: [u8; 4]) -> StatEntry {
    StatEntry::new(label, StatValue::Vector2D { x, y }, color)
}

/// Returns an entry holding a histogram, in which `counts[i]` counts the values in `[edges[i], edges[i + 1])`.
pub fn stat_histogram(label: &'static str, edges: Vec<f64>, counts: Vec<u64>, color: [u8; 4]) -> StatEntry {
    StatEntry::new(label, StatValue::Histogram { edges, counts }, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_builders() {
        let p = f32_param("test", "Test", 0.5, 0.0, 1.0, Some(0.1));
        assert_eq!(p.id(), "test");
        assert_eq!(p.label(), "Test");
        assert_eq!(p.kind().default_value(), ParamValue::F32(0.5));

        let p = u32_param("count", "Count", 10, 1, 100);
        assert_eq!(p.kind().default_value(), ParamValue::U32(10));
    }

    #[test]
    fn choice_builder_takes_named_options() {
        let p = choice_param("shape", "Shape", &["square", "hexagon"], 1);
        assert_eq!(p.kind().default_value(), ParamValue::Choice(1));
    }

    #[test]
    #[should_panic(expected = "choice 'level': option '2' reads as an index")]
    fn choice_builder_refuses_an_option_named_as_an_index() {
        choice_param("level", "Level", &["low", "2"], 0);
    }

    #[test]
    #[should_panic(expected = "choice 'network': option 'Random ' is empty or has spaces at an end")]
    fn choice_builder_refuses_an_option_with_trailing_spaces() {
        choice_param("network", "Network", &["Random ", "Geometric"], 0);
    }

    #[test]
    #[should_panic(expected = "choice 'mode': option 'S,I' cannot be listed in a sweep's levels")]
    fn choice_builder_refuses_an_option_holding_a_comma() {
        choice_param("mode", "Mode", &["S,I", "R"], 0);
    }

    #[test]
    fn option_names_that_level_text_cannot_list_are_refused() {
        for options in [["S,I", "R"], ["1:2", "2:1"], ["all", "some"]] {
            let reason = check_choice_options(&options).err();
            assert_eq!(
                reason,
                Some(format!("option '{}' cannot be listed in a sweep's levels", options[0])),
                "{options:?}"
            );
        }
        assert_eq!(check_choice_options(&["All", "a.b", "a-b", "a b"]), Ok(()));
    }

    #[test]
    #[should_panic(expected = "choice 'shape': option 'square' appears twice")]
    fn choice_builder_refuses_a_repeated_option() {
        choice_param("shape", "Shape", &["square", "hexagon", "square"], 0);
    }

    #[test]
    fn extraction_defaults_on_mismatch() {
        let params = vec![ParamValue::U32(5)];
        assert_eq!(extract_f32(&params, 0, 1.0), 1.0);
        assert_eq!(extract_u32(&params, 0, 99), 5);
        assert_eq!(extract_f32(&params, 5, 2.0), 2.0);
    }
}
