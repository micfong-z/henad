//! Machine-readable benchmark output, one JSON object per line on stdout.
//!
//! The cross-engine harness reads these lines from every engine it drives, so the shape is a
//! contract rather than a convenience. `benchmarks/protocol.md` states it. The human report in
//! `main` writes to the same stream, so exactly one of the two runs.

use std::time::Duration;

use henad_compute::runtime_info::{HostInfo, RuntimeInfo};
use henad_core::params::{ParamDescriptor, ParamValue};
use serde_json::{Map, Value, json};

/// Emitted once, before any rep.
pub fn info(model: &str, variant: &str, threads: usize, adapter: Option<&str>) {
    let line = json!({
        "kind": "info",
        "engine": "henad",
        "engine_version": env!("CARGO_PKG_VERSION"),
        "model": model,
        "variant": variant,
        "threads": threads,
        "adapter": adapter,
        "debug_build": cfg!(debug_assertions),
    });
    emit(&line);
}

/// Host and adapter provenance, the `--info` output in the machine-readable stream.
pub fn runtime(runtime: Option<&RuntimeInfo>) {
    let collected;
    let host = if let Some(runtime) = runtime {
        &runtime.host
    } else {
        collected = HostInfo::collect();
        &collected
    };
    let line = json!({
        "kind": "runtime",
        "os": host.os,
        "arch": host.arch,
        "logical_cpus": host.logical_cpus,
        "worker_threads": host.worker_threads,
        "adapter": runtime.map(|r| r.adapter.name.clone()),
        "adapter_backend": runtime.map(|r| r.adapter.backend.to_string()),
        "adapter_type": runtime.map(|r| format!("{:?}", r.adapter.device_type)),
    });
    emit(&line);
}

/// One line per timed rep. `heap_bytes` is `None` for a GPU model, whose state lives on the device.
pub fn rep(
    index: usize,
    seed: Option<u64>,
    steps: u64,
    warmup: u64,
    elapsed: Duration,
    population: u64,
    heap_bytes: Option<usize>,
) {
    let line = json!({
        "kind": "rep",
        "rep": index,
        "seed": seed,
        "steps": steps,
        "warmup": warmup,
        "elapsed_s": elapsed.as_secs_f64(),
        "population": population,
        "heap_bytes": heap_bytes,
    });
    emit(&line);
}

/// The machine-readable twin of the human report, plus what the harness needs for provenance.
///
/// The harness derives its own statistics from the `rep` lines, so these are a convenience and a
/// cross-check, not the source of truth.
pub fn summary(
    samples: &[Duration],
    steps_per_rep: u64,
    population: u64,
    grid_dims: Option<(u32, u32)>,
    descriptors: &[ParamDescriptor],
    params: &[ParamValue],
) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let mean = if samples.is_empty() {
        0.0
    } else {
        samples.iter().map(Duration::as_secs_f64).sum::<f64>() / samples.len() as f64
    };
    let variance = if samples.is_empty() {
        0.0
    } else {
        samples.iter().map(|s| (s.as_secs_f64() - mean).powi(2)).sum::<f64>() / samples.len() as f64
    };
    let line = json!({
        "kind": "summary",
        "reps": samples.len(),
        "min_s": sorted.first().map(Duration::as_secs_f64),
        "median_s": sorted.get(sorted.len() / 2).map(Duration::as_secs_f64),
        "max_s": sorted.last().map(Duration::as_secs_f64),
        "mean_s": mean,
        "std_dev_s": variance.sqrt(),
        "steps_per_sec": if mean > 0.0 { Some(steps_per_rep as f64 / mean) } else { None },
        "updates_per_sec": if mean > 0.0 { Some(steps_per_rep as f64 * population as f64 / mean) } else { None },
        "grid_w": grid_dims.map(|(w, _)| w),
        "grid_h": grid_dims.map(|(_, h)| h),
        "params": params_object(descriptors, params),
    });
    emit(&line);
}

/// Resolved parameters keyed by id, so a row stays interpretable after a model's defaults change.
fn params_object(descriptors: &[ParamDescriptor], params: &[ParamValue]) -> Value {
    let mut map = Map::new();
    for (desc, value) in descriptors.iter().zip(params) {
        let value = match *value {
            ParamValue::F32(v) => json!(v),
            ParamValue::U32(v) => json!(v),
            ParamValue::Bool(v) => json!(v),
            ParamValue::Choice(v) => json!(v),
        };
        map.insert(desc.id.to_owned(), value);
    }
    Value::Object(map)
}

fn emit(line: &Value) {
    println!("{line}");
}
