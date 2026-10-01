//! A model's parameters, stats and actions as JSON, for tools that build sweeps from outside.

use serde_json::{Map, Value, json};

use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::plan::ModelSchema;
use henad_core::metadata::Backend;
use henad_core::params::{ParamApply, ParamDescriptor, ParamFormat, ParamKind};
use henad_models::registry::ModelEntry;

use crate::probe::ProbeReport;

/// Version of the object [`schema_json`] returns.
pub const SCHEMA_VERSION: u64 = 1;

/// Returns the declarations of `entry`, as a plan checks a spec against them.
pub fn model_schema(entry: &ModelEntry) -> ModelSchema<'_> {
    ModelSchema {
        id: &entry.id,
        params: &entry.param_descriptors,
        stats: &entry.stat_descriptors,
        actions: &entry.action_descriptors,
    }
}

/// Returns the parameters, stats and actions of `entry` as one JSON object.
///
/// The object carries `stat_columns`, the columns a sweep writes, only when `probe` is given. A choice's default is
/// its option name, with its index beside it as `default_index`.
pub fn schema_json(entry: &ModelEntry, probe: Option<&ProbeReport>) -> Value {
    let params: Vec<Value> = entry
        .param_descriptors
        .iter()
        .enumerate()
        .map(|(index, descriptor)| param_json(index, descriptor))
        .collect();
    let stats: Vec<Value> = entry
        .stat_descriptors
        .iter()
        .map(|stat| json!({ "label": stat.label, "color": stat.color }))
        .collect();
    let actions: Vec<Value> = entry
        .action_descriptors
        .iter()
        .enumerate()
        .map(|(index, action)| json!({ "index": index, "id": action.id, "label": action.label }))
        .collect();
    let mut schema = json!({
        "schema_version": SCHEMA_VERSION,
        "model": entry.id,
        "name": entry.name,
        "backend": backend_name(entry.metadata.backend),
        "schema_hash": format!("{:016x}", schema_hash(&model_schema(entry))),
        "params": params,
        "stats": stats,
        "actions": actions,
        "seed": { "type": "u64" },
    });
    if let Some(probe) = probe {
        let columns: Vec<&str> = (0..probe.columns.len()).map(|i| probe.columns.name(i)).collect();
        schema["stat_columns"] = json!(columns);
    }
    schema
}

/// Returns `backend` as written in JSON and CSV, `cpu` or `gpu`.
pub fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Cpu => "cpu",
        Backend::Gpu => "gpu",
    }
}

fn param_json(index: usize, descriptor: &ParamDescriptor) -> Value {
    let mut param = Map::new();
    param.insert("index".to_owned(), json!(index));
    param.insert("id".to_owned(), json!(descriptor.id));
    param.insert("label".to_owned(), json!(descriptor.label));
    let apply = match descriptor.apply {
        ParamApply::Live => "live",
        ParamApply::OnReload => "reload",
    };
    param.insert("apply".to_owned(), json!(apply));
    let format = match descriptor.format {
        ParamFormat::Plain => "plain",
        ParamFormat::Percent => "percent",
    };
    param.insert("format".to_owned(), json!(format));
    let fields = match descriptor.kind {
        ParamKind::F32 {
            min,
            max,
            default,
            step,
        } => {
            let mut fields = json!({
                "kind": "f32",
                "min": f32_json(min),
                "max": f32_json(max),
                "default": f32_json(default),
            });
            if let Some(step) = step {
                fields["step"] = f32_json(step);
            }
            fields
        }
        ParamKind::U32 { min, max, default } => {
            json!({ "kind": "u32", "min": min, "max": max, "default": default })
        }
        ParamKind::Bool { default } => json!({ "kind": "bool", "default": default }),
        ParamKind::Choice { options, default } => json!({
            "kind": "choice",
            "options": options,
            "default": options.get(default),
            "default_index": default,
        }),
    };
    if let Value::Object(fields) = fields {
        param.extend(fields);
    }
    Value::Object(param)
}

/// Returns `value` as the JSON number with the shortest decimal form that reads back as `value`.
///
/// A plain conversion widens to `f64` first and writes `0.025` as `0.02500000037252903`.
fn f32_json(value: f32) -> Value {
    value
        .to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use henad_models::registry::model_registry;
    use serde_json::json;

    use henad_core::explore::fingerprint::schema_hash;

    use super::{f32_json, model_schema, schema_json};
    use crate::probe::ProbeReport;

    /// Each example model's `schema_hash` as Henad 0.2.0 wrote it, at commit 773a7a5.
    ///
    /// `crates/henad-models/tests/fixtures/docs/schema-hashes-0.2.0.md` gives the procedure that recorded them.
    const SCHEMA_HASHES_0_2_0: [(&str, &str); 10] = [
        ("sir", "6ff1dc3971fd0a96"),
        ("boids", "ae99f3e0d37c8d3d"),
        ("game_of_life", "aba7303a467b06bf"),
        ("ants", "f6eb31e76efdf4cf"),
        ("virus_network", "77993c7b4047b8bd"),
        ("team_assembly", "d1724ce65dada5d6"),
        ("gpu_game_of_life", "461ad8e0d063a398"),
        ("gpu_sir", "99f14d30367e3743"),
        ("gpu_boids", "f7328729019009c0"),
        ("gpu_ants", "b58e8a5a5b6a829e"),
    ];

    /// Checks that every example model hashes as it did in 0.2.0, so every folder written since still resumes.
    ///
    /// Without a device, the GPU models are left out. `HENAD_REQUIRE_GPU` turns that into a failure.
    #[test]
    fn schema_hashes_are_unchanged_since_0_2_0() {
        let gpu = crate::tests::support::headless_device();
        let has_gpu = gpu.is_some();
        let registry = model_registry(gpu);
        for (id, recorded) in SCHEMA_HASHES_0_2_0 {
            let Some(entry) = registry.iter().find(|entry| entry.id == id) else {
                assert!(id.starts_with("gpu_") && !has_gpu, "{id} is registered");
                continue;
            };
            let current = format!("{:016x}", schema_hash(&model_schema(entry)));
            assert_eq!(current, recorded, "{id}'s schema hash moved since 0.2.0");
        }
    }

    #[test]
    fn every_descriptor_is_listed_with_its_kind_and_bounds() {
        for entry in model_registry(None) {
            let schema = schema_json(&entry, None);
            let params = schema["params"].as_array().expect("params is a list");
            assert_eq!(params.len(), entry.param_descriptors.len(), "{}", entry.id);
            for (index, param) in params.iter().enumerate() {
                assert_eq!(param["index"], json!(index));
                assert_eq!(param["id"], json!(entry.param_descriptors[index].id));
                assert!(param["kind"].is_string() && param["default"] != json!(null), "{param}");
            }
            assert_eq!(schema["model"], json!(entry.id));
            assert_eq!(schema["schema_hash"].as_str().map(str::len), Some(16));
            assert!(schema.get("stat_columns").is_none(), "no probe, no columns");
        }
    }

    #[test]
    fn a_probe_adds_the_stat_columns() {
        let entry = model_registry(None)
            .into_iter()
            .find(|entry| entry.id == "sir")
            .expect("sir is registered");
        let defaults: Vec<_> = entry
            .param_descriptors
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        let probe = ProbeReport::build(&entry, None, &defaults, None).expect("sir builds at its defaults");
        let schema = schema_json(&entry, Some(&probe));
        assert_eq!(schema["stat_columns"], json!(["Susceptible", "Infected", "Recovered"]));
        let grid_width = &schema["params"][0];
        assert_eq!(grid_width["id"], json!("grid_width"));
        assert_eq!(grid_width["kind"], json!("u32"));
        assert_eq!(grid_width["apply"], json!("reload"));
    }

    #[test]
    fn an_f32_is_written_in_its_shortest_form() {
        assert_eq!(f32_json(0.025).to_string(), "0.025");
        assert_eq!(f32_json(1.0).to_string(), "1.0");
        assert_eq!(f32_json(f32::NAN), json!(null));
    }
}
