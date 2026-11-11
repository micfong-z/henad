//! Names that the Sweep and Results tabs give an output kind, an output column and an action.

use henad_core::explore::reducer::ReducerKind;

/// Returns the name that the Outputs section uses for an output kind, as in `Tick of maximum`.
pub fn reducer_label(kind: ReducerKind) -> &'static str {
    match kind {
        ReducerKind::Final => "Final value",
        ReducerKind::Min => "Minimum",
        ReducerKind::Max => "Maximum",
        ReducerKind::Mean => "Mean",
        ReducerKind::ArgMax => "Tick of maximum",
        ReducerKind::ArgMin => "Tick of minimum",
        ReducerKind::FirstCrossing(_) => "First tick crossing",
        ReducerKind::WindowMean { .. } => "Mean over window",
    }
}

/// Returns the name that the views use for the output column `name`, as in `Infected, tick of maximum` for
/// `Infected:argmax`.
///
/// A threshold or a window stays in the name, as in `Infected, first tick <= 10`. A kind that does not parse shows as
/// written.
pub fn output_label(name: &str) -> String {
    let Some((column, written)) = name.rsplit_once(':') else {
        return name.to_owned();
    };
    match written.parse::<ReducerKind>() {
        Ok(kind) => column_output_label(column, kind),
        Err(_) => format!("{column}, {written}"),
    }
}

/// Returns the name that the views use for output `kind` of the stat column `column`, as [`output_label`] does for its
/// written name.
pub fn column_output_label(column: &str, kind: ReducerKind) -> String {
    match kind {
        ReducerKind::FirstCrossing(comparison) => format!(
            "{column}, first tick {} {}",
            comparison.comparator.as_str(),
            comparison.threshold
        ),
        ReducerKind::WindowMean { start, end } => format!("{column}, mean over ticks {start} to {end}"),
        kind => format!("{column}, {}", reducer_label(kind).to_lowercase()),
    }
}

/// Returns the label of the action `id`, declared as `declared_label`, fired under the name `name`.
///
/// An action fired under another name carries that name after the label, so two firings of one action stay apart.
/// A name that the Sweep tab numbered, such as `seed_outbreak_2`, adds only its number, as in "Seed outbreak 2".
pub fn action_label(declared_label: &str, id: &str, name: &str) -> String {
    let number = name
        .strip_prefix(id)
        .and_then(|rest| rest.strip_prefix('_'))
        .filter(|rest| rest.parse::<u64>().is_ok());
    if name == id {
        declared_label.to_owned()
    } else if let Some(number) = number {
        format!("{declared_label} {number}")
    } else {
        format!("{declared_label} ({name})")
    }
}

#[cfg(test)]
mod tests {
    use super::output_label;

    #[test]
    fn an_output_label_names_its_kind_in_words() {
        assert_eq!(output_label("Infected:argmax"), "Infected, tick of maximum");
        assert_eq!(output_label("Infected:final"), "Infected, final value");
        assert_eq!(output_label("Infected:first<=10"), "Infected, first tick <= 10");
        assert_eq!(
            output_label("Recovered:mean@200..600"),
            "Recovered, mean over ticks 200 to 600"
        );
        assert_eq!(
            output_label("Infected:median"),
            "Infected, median",
            "an unknown kind shows as written"
        );
        assert_eq!(output_label("Infected"), "Infected");
    }
}
