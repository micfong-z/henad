//! Machine-readable benchmark output, one JSON object per line on stdout.
//!
//! The cross-engine driver reads these lines from every engine's harness, and `benchmarks/protocol.md` fixes their
//! shape. The human report in the crate root writes to the same stream, so exactly one of the two runs.

use std::time::Duration;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_compute::runtime_info::{HostInfo, RuntimeInfo};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_explore::output::details::{ChoiceForm, params_by_id_json, scheduled_actions_json};
use henad_explore::probe::ProbeReport;
use henad_explore::schema::schema_json;
use serde_json::{Value, json};

/// Prints the `info` line, once before any repetition.
pub fn info(model: &str, variant: &str, threads: usize, parallel_jobs: Option<usize>, adapter: Option<&str>) {
    let line = json!({
        "kind": "info",
        "engine": "henad",
        "engine_version": henad_explore::ENGINE_BUILD.version(),
        "model": model,
        "variant": variant,
        "threads": threads,
        "parallel_jobs": parallel_jobs,
        "adapter": adapter,
        "debug_build": cfg!(debug_assertions),
    });
    emit(&line);
}

/// Prints the `runtime` line, the host and adapter `--info` reports.
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

/// Prints the `rep` line of one timed repetition.
///
/// `heap_bytes` is `None` for a GPU model, whose state lives on the device.
pub fn rep(
    index: u64,
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

/// Prints the `summary` line, the statistics of the human report with the parameters and actions of the run.
///
/// The driver computes its own statistics from the `rep` lines, and reads this line for provenance alone.
pub fn summary(
    samples: &[Duration],
    steps_per_rep: u64,
    population: u64,
    grid_dims: Option<(u32, u32)>,
    descriptors: &[ParamDescriptor],
    params: &[ParamValue],
    schedule: &henad_core::action::Schedule,
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
        "median_s": (!sorted.is_empty()).then(|| crate::median_of(&sorted).as_secs_f64()),
        "max_s": sorted.last().map(Duration::as_secs_f64),
        "mean_s": mean,
        "std_dev_s": variance.sqrt(),
        "steps_per_sec": if mean > 0.0 { Some(steps_per_rep as f64 / mean) } else { None },
        "updates_per_sec": if mean > 0.0 { Some(steps_per_rep as f64 * population as f64 / mean) } else { None },
        "grid_w": grid_dims.map(|(w, _)| w),
        "grid_h": grid_dims.map(|(_, h)| h),
        "params": params_by_id_json(descriptors, params, ChoiceForm::Index),
        "actions": scheduled_actions_json(schedule),
    });
    emit(&line);
}

/// Returns the `--params --json` line: the parameters, stats and actions of `entry`.
///
/// The line carries `stat_columns` from a build at the defaults, and leaves them out when that build fails.
pub fn params(entry: &ModelEntry, gpu: Option<&GpuContext>) -> Value {
    let defaults: Vec<ParamValue> = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind.default_value())
        .collect();
    let probe = match ProbeReport::build(entry, gpu, &defaults, None) {
        Ok(probe) => Some(probe),
        Err(error) => {
            let error = anyhow::Error::new(error);
            eprintln!(
                "note: '{}' failed to build with default parameters ({error:#}), so stat_columns is omitted",
                entry.id()
            );
            None
        }
    };
    let mut line = schema_json(entry, probe.as_ref());
    line["kind"] = json!("params");
    line
}

/// Writes `line` to stdout as one line.
pub fn emit(line: &Value) {
    println!("{line}");
}
