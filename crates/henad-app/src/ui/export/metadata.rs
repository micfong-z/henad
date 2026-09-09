//! Exported metadata about a run.

use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use serde_json::{Map, Value, json};

use crate::state::AppState;

/// Exported metadata about a run as JSON.
pub fn run_details(app: &AppState) -> String {
    let entry = app.loaded_model.and_then(|index| app.registry.get(index));
    let host = &app.runtime.host;
    let adapter = &app.runtime.adapter;

    let details = json!({
        "engine": "henad",
        "engine_version": env!("CARGO_PKG_VERSION"),
        "debug_build": cfg!(debug_assertions),
        "model": entry.map(|e| e.id.as_str()),
        "model_name": entry.map(|e| e.name.as_str()),
        "backend": entry.map(|e| e.metadata.backend.label()),
        "params": entry.map(|e| params_object(&e.param_descriptors, &app.param_values)),
        "params_match_running_model": app.selection_is_loaded() && !app.pending_reload.iter().any(|p| *p),
        "tick": app.snapshot.as_ref().map(|snap| snap.tick),
        "population": app.snapshot.as_ref().map(|snap| snap.population),
        "ticks_per_snapshot": app.ticks_per_snapshot,
        "history_samples": app.stats_history.as_ref().map(henad_core::view::StatsHistory::len),
        "history_capacity": app.history_capacity,
        "os": host.os,
        "arch": host.arch,
        "logical_cpus": host.logical_cpus,
        "worker_threads": host.worker_threads,
        "adapter": adapter.name,
        "adapter_backend": adapter.backend.to_string(),
        "adapter_type": format!("{:?}", adapter.device_type),
    });

    format!("{details:#}\n")
}

fn params_object(descriptors: &[ParamDescriptor], params: &[ParamValue]) -> Value {
    let mut map = Map::new();
    for (desc, value) in descriptors.iter().zip(params) {
        let value = match *value {
            ParamValue::F32(v) => json!(v),
            ParamValue::U32(v) => json!(v),
            ParamValue::Bool(v) => json!(v),
            ParamValue::Choice(index) => match &desc.kind {
                ParamKind::Choice { options, .. } => options.get(index).map_or_else(|| json!(index), |o| json!(o)),
                _ => json!(index),
            },
        };
        map.insert(desc.id.to_owned(), value);
    }
    Value::Object(map)
}
