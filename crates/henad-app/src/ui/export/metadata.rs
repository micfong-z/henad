//! Exported metadata about a run.

use henad_explore::output::details::{ChoiceForm, params_by_id_json, scheduled_actions_json};
use henad_explore::output::manifest::RecordedBuild;
use serde_json::json;

use crate::state::AppState;

/// Exported metadata about a run as JSON.
pub fn run_details(app: &AppState) -> String {
    let entry = app.loaded_entry();
    let host = &app.runtime.host;
    let adapter = &app.runtime.adapter;

    let details = json!({
        "engine": "henad",
        "engine_version": henad_explore::ENGINE_BUILD.version(),
        "debug_build": cfg!(debug_assertions),
        "host": RecordedBuild::from(&app.product.host),
        "model": entry.map(|e| e.id()),
        "model_name": entry.map(|e| e.name()),
        "backend": entry.map(|e| e.metadata().backend.label()),
        "model_source": entry.map(|e| RecordedBuild::from(e.source())),
        "params": entry.map(|e| params_by_id_json(e.param_descriptors(), &app.loaded_values, ChoiceForm::Name)),
        // Kept from the files of 0.2. `params` holds the loaded model's own values.
        "params_match_running_model": entry.is_some(),
        // Null with a model loaded is the model's default seed.
        "seed": entry.and(app.loaded_seed),
        "scheduled_actions": entry.map(|_| scheduled_actions_json(&app.loaded_schedule)),
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
