//! Run details as JSON, shared by the hosts that describe one run: the CLI's benchmark summary and the app's run
//! details.

use serde_json::{Map, Value, json};

use henad_core::action::Schedule;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};

use crate::schema::f32_json;

/// Form that a choice parameter's value takes in [`params_by_id_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceForm {
    /// The option's index, as the CLI's benchmark summary writes it.
    Index,
    /// The option's name, or its index when the descriptor has no option at that index.
    Name,
}

/// Returns `values` as one JSON object keyed by parameter id, so a record stays readable after a model's defaults
/// change.
///
/// `values` holds one value per entry of `descriptors`, in the same order. An `f32` value is written in the shortest
/// form that reads back as the same `f32`, as `--params --json` writes it.
pub fn params_by_id_json(descriptors: &[ParamDescriptor], values: &[ParamValue], choices: ChoiceForm) -> Value {
    let mut map = Map::new();
    for (descriptor, value) in descriptors.iter().zip(values) {
        let value = match *value {
            ParamValue::F32(number) => f32_json(number),
            ParamValue::U32(number) => json!(number),
            ParamValue::Bool(flag) => json!(flag),
            ParamValue::Choice(index) => match (&descriptor.kind, choices) {
                (ParamKind::Choice { options, .. }, ChoiceForm::Name) => {
                    options.get(index).map_or_else(|| json!(index), |option| json!(option))
                }
                _ => json!(index),
            },
        };
        map.insert(descriptor.id.to_owned(), value);
    }
    Value::Object(map)
}

/// Returns the entries of `schedule` as a JSON array of `{ "id", "tick" }` objects, in the order given.
pub fn scheduled_actions_json(schedule: &Schedule) -> Value {
    schedule
        .entries()
        .iter()
        .map(|entry| json!({ "id": entry.id, "tick": entry.tick }))
        .collect()
}

#[cfg(test)]
mod tests {
    use henad_core::helpers::f32_param;
    use henad_core::params::ParamValue;

    use super::{ChoiceForm, params_by_id_json};

    #[test]
    fn an_f32_value_is_written_in_its_shortest_form() {
        let rate = [f32_param("rate", "Rate", 0.3, 0.0, 1.0, None)];
        let json = params_by_id_json(&rate, &[ParamValue::F32(0.3)], ChoiceForm::Index);
        assert_eq!(json.to_string(), r#"{"rate":0.3}"#);
    }
}
