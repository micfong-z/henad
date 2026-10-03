//! Run details as JSON, shared by the hosts that describe one run: the CLI's benchmark summary and the app's run
//! details.

use serde_json::{Map, Value, json};

use henad_core::action::Schedule;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};

/// Form a choice parameter's value takes in [`params_by_id_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceForm {
    /// The option's index, as the CLI's benchmark summary writes it.
    Index,
    /// The option's name, or its index when the descriptor names no option there.
    Name,
}

/// Returns `values` as one JSON object keyed by parameter id, so a record stays readable after a model's defaults
/// change.
///
/// `values` holds one value per entry of `descriptors`, in the same order.
pub fn params_by_id_json(descriptors: &[ParamDescriptor], values: &[ParamValue], choices: ChoiceForm) -> Value {
    let mut map = Map::new();
    for (descriptor, value) in descriptors.iter().zip(values) {
        let value = match *value {
            ParamValue::F32(number) => json!(number),
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
