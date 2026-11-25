//! Machine-readable output, one JSON object per line on stdout, each with a `kind` field.
//!
//! A benchmark prints `info`, then a `rep` line per repetition and a `summary`, and `--info` adds a `runtime` line.
//! The cross-engine driver reads these lines from every engine's harness, and `benchmarks/protocol.md` fixes their
//! shape. `--params --json` prints a `params` line. A sweep or a search prints `explore_plan`, an `explore_run` line
//! per run, `explore_progress`, `explore_search_batch` after each batch of a search, `explore_warning` and
//! `explore_end`, and a merge prints `explore_warning` and `explore_merge`. `docs/reference/cli.md` lists their fields.
//! The human report writes to the same stream, so exactly one of the two runs.

use std::time::Duration;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_compute::runtime_info::RuntimeInfo;
use henad_compute::simulation::RunSetup;
use henad_core::explore::outcome::RunOutcome;
use henad_core::params::ParamValue;
use henad_explore::benchmark::RepetitionReport;
use henad_explore::merge::MergeReport;
use henad_explore::output::details::{ChoiceForm, params_by_id_json, scheduled_actions_json};
use henad_explore::output::manifest::ManifestRuntime;
use henad_explore::probe::ProbeReport;
use henad_explore::progress::ProgressUpdate;
use henad_explore::schema::{backend_name, schema_json};
use henad_explore::search_run::{SearchOutline, SearchUpdate};
use henad_explore::sweep::{SweepEnd, SweepOutline, SweepReport, SweepWarning};
use serde_json::{Value, json};

use crate::explore::report::{SearchStanding, axis_text, factor_text};
use crate::single::Timings;

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
///
/// The line holds the fields of a manifest's runtime record but the driver and the limits.
pub fn runtime(runtime: Option<&RuntimeInfo>) {
    let record = ManifestRuntime::new(runtime);
    let line = json!({
        "kind": "runtime",
        "os": record.os,
        "arch": record.arch,
        "logical_cpus": record.logical_cpus,
        "worker_threads": record.worker_threads,
        "adapter": record.adapter,
        "adapter_backend": record.adapter_backend,
        "adapter_type": record.adapter_type,
    });
    emit(&line);
}

/// Prints the `rep` line of one timed repetition of `steps` steps after `warmup` untimed ones.
///
/// The repetition's `heap_bytes` is `None` for a GPU model, whose state lives on the device.
pub fn rep(repetition: &RepetitionReport, steps: u64, warmup: u64) {
    let line = json!({
        "kind": "rep",
        "rep": repetition.index,
        "seed": repetition.seed,
        "steps": steps,
        "warmup": warmup,
        "elapsed_s": repetition.elapsed.as_secs_f64(),
        "population": repetition.population_after_warmup,
        "heap_bytes": repetition.heap_bytes,
    });
    emit(&line);
}

/// Prints the `summary` line, the statistics of the human report with the parameters and actions of `setup`.
///
/// Each repetition of `timings` covers `steps_per_rep` steps. The driver computes its own statistics from the `rep`
/// lines, and reads this line for provenance alone.
pub fn summary(timings: &Timings, steps_per_rep: u64, setup: &RunSetup) {
    let mean = timings.mean_secs();
    let rate = |count: f64| (mean > 0.0).then(|| count / mean);
    let line = json!({
        "kind": "summary",
        "reps": timings.samples.len(),
        "min_s": timings.min().map(|min| min.as_secs_f64()),
        "median_s": timings.median().map(|median| median.as_secs_f64()),
        "max_s": timings.max().map(|max| max.as_secs_f64()),
        "mean_s": mean,
        "std_dev_s": timings.std_dev_secs(mean),
        "steps_per_sec": rate(steps_per_rep as f64),
        "updates_per_sec": rate(steps_per_rep as f64 * timings.population as f64),
        "grid_w": timings.grid_size.map(|(width, _)| width),
        "grid_h": timings.grid_size.map(|(_, height)| height),
        "params": params_by_id_json(setup.entry().param_descriptors(), setup.values(), ChoiceForm::Index),
        "actions": scheduled_actions_json(setup.schedule()),
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
        .map(|descriptor| descriptor.kind().default_value())
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

/// Returns the `explore_plan` line of a sweep or a search planned as `outline`.
pub fn explore_plan(outline: &SweepOutline) -> Value {
    let blocks: Vec<Value> = outline
        .blocks
        .iter()
        .map(|block| {
            json!({
                "design": block.design.as_str(),
                "configs": block.configs.end - block.configs.start,
                "design_seed": block.design_seed,
            })
        })
        .collect();
    json!({
        "kind": "explore_plan",
        "model": outline.model,
        "backend": backend_name(outline.backend),
        "configs": outline.configs,
        "replicates": outline.replicates,
        "runs": outline.runs,
        "blocks": blocks,
        "shard": { "index": outline.shard.index(), "count": outline.shard.count() },
        "skipped": outline.skipped,
        "pending": outline.pending,
        "cpu_lanes": outline.layout.cpu_lanes,
        "threads_per_lane": outline.layout.threads_per_lane,
        "gpu_tracks": outline.layout.gpu_tracks,
        "projected_bytes": outline.projected_bytes,
        "series_rows": outline.series_rows,
        "dry_run": outline.dry_run,
        "search": outline.search.as_ref().map(search_outline),
    })
}

/// Returns the `search` field of an `explore_plan` line.
fn search_outline(search: &SearchOutline) -> Value {
    json!({
        "algorithm": search.algorithm,
        "max_evaluations": search.max_evaluations,
        "batch_size": search.batch_size,
        "objective": search.objective.as_ref().map(|objective| json!({
            "column": objective.column,
            "goal": objective.goal.as_str(),
            "aggregate": objective.aggregate.as_str(),
        })),
        "watched_columns": search.watched_columns,
        "axes": search
            .pattern_axes
            .as_ref()
            .map(|axes| axes.iter().map(axis_text).collect::<Vec<_>>()),
        "space": search.space.iter().map(factor_text).collect::<Vec<_>>(),
        "search_seed": search.search_seed,
    })
}

/// Returns the `explore_search_batch` line of one batch of `search`.
///
/// The line holds the best candidate of a scored search, the number of generations a genetic algorithm has finished,
/// or the cells a Pattern Space Exploration fills, as `standing` holds them after the batch.
pub fn explore_search_batch(search: &SearchOutline, update: &SearchUpdate, standing: &SearchStanding) -> Value {
    let mut batch = json!({
        "kind": "explore_search_batch",
        "batch": update.batch,
        "evaluations": standing.evaluations,
        "runs": update.runs,
    });
    if search.objective.is_some() {
        let best = standing.best.as_ref();
        batch["best_candidate_id"] = json!(best.map(|best| best.candidate_id));
        batch["best_objective"] = json!(
            best.map(|best| best.objective)
                .filter(|objective| objective.is_finite())
        );
        batch["best_replicates"] = json!(best.map(|best| best.replicate_count));
    } else {
        batch["filled_cells"] = json!(standing.filled_cells);
    }
    if search.algorithm == "genetic" {
        batch["generation_count"] = json!(standing.generation_count);
    }
    batch
}

/// Returns the `explore_run` line of a run that ended as `outcome`.
pub fn explore_run(outcome: &RunOutcome) -> Value {
    let run = &outcome.run;
    json!({
        "kind": "explore_run",
        "run_id": run.run_id,
        "config_id": run.config_id,
        "rep": run.rep,
        "seed": run.seed,
        "status": outcome.status.as_str(),
        "stop_reason": outcome.stop_reason.as_str(),
        "ticks": outcome.ticks,
        "wall_ms": outcome.wall_ms,
    })
}

/// Returns the `explore_progress` line of `update`, with the `skipped` runs that the output directory already held.
pub fn explore_progress(update: &ProgressUpdate, skipped: u64) -> Value {
    json!({
        "kind": "explore_progress",
        "done": update.done,
        "total": update.total,
        "skipped": skipped,
        "failed": update.failed,
        "elapsed_s": update.elapsed.as_secs_f64(),
        "remaining_s": update.remaining.as_ref().map(Duration::as_secs_f64),
    })
}

/// Returns the `explore_warning` line of `warning`.
///
/// The `warning` field holds the warning's kind, `message` holds the text that the warning prints, and a
/// `build_changed` warning adds its `role`, the `recorded` and `current` builds as the manifest records them, and
/// `between_shards`, true when a merge found `current` in another shard.
pub fn explore_warning(warning: &SweepWarning) -> Value {
    let mut line = json!({
        "kind": "explore_warning",
        "message": warning.to_string(),
    });
    let kind = match warning {
        SweepWarning::Plan(_) => "plan",
        SweepWarning::MissingRuns { .. } => "missing_runs",
        SweepWarning::BuildChanged {
            role,
            recorded,
            current,
            between_shards,
        } => {
            line["role"] = json!(role.as_str());
            line["between_shards"] = json!(between_shards);
            line["recorded"] = serde_json::to_value(recorded.as_ref()).unwrap_or_default();
            line["current"] = serde_json::to_value(current.as_ref()).unwrap_or_default();
            "build_changed"
        }
    };
    line["warning"] = json!(kind);
    line
}

/// Returns the `explore_end` line of `report`, with the evaluations and the best candidate or filled cells of a
/// search's last `standing`.
pub fn explore_end(report: &SweepReport, standing: Option<&SearchStanding>) -> Value {
    let counts = &report.counts;
    let mut end = json!({
        "kind": "explore_end",
        "end": end_name(report.end),
        "rows": counts.rows,
        "skipped": report.outline.skipped,
        "ok": counts.ok,
        "non_finite": counts.non_finite,
        "failed": counts.failed,
        "elapsed_s": report.elapsed.as_secs_f64(),
        "output_dir": report.output_dir.as_ref().map(|dir| dir.display().to_string()),
    });
    if let Some(search) = &report.outline.search {
        let best = standing.and_then(|standing| standing.best.as_ref());
        end["evaluations"] = json!(standing.map_or(0, |standing| standing.evaluations));
        if search.objective.is_some() {
            end["best_candidate_id"] = json!(best.map(|best| best.candidate_id));
            end["best_objective"] = json!(
                best.map(|best| best.objective)
                    .filter(|objective| objective.is_finite())
            );
        } else {
            end["filled_cells"] = json!(standing.map_or(0, |standing| standing.filled_cells));
        }
    }
    end
}

/// Returns the `end` field of an `explore_end` line.
fn end_name(end: SweepEnd) -> &'static str {
    match end {
        SweepEnd::Planned => "planned",
        SweepEnd::Complete => "complete",
        SweepEnd::Aborted => "aborted",
        SweepEnd::DeviceLost => "device_lost",
    }
}

/// Returns the `explore_merge` line of a merge of `inputs` shard directories.
pub fn explore_merge(report: &MergeReport, inputs: usize) -> Value {
    let counts = &report.counts;
    json!({
        "kind": "explore_merge",
        "inputs": inputs,
        "rows": counts.rows,
        "ok": counts.ok,
        "non_finite": counts.non_finite,
        "failed": counts.failed,
        "missing": report.missing,
        "output_dir": report.output_dir.display().to_string(),
    })
}

/// Returns the `explore_warning` line of a plan whose `series.csv` would hold more rows than
/// [`SERIES_ROWS_WARNING`](crate::explore::report::SERIES_ROWS_WARNING), with the warning's `message`.
pub fn explore_series_rows_warning(message: &str) -> Value {
    json!({
        "kind": "explore_warning",
        "warning": "series_rows",
        "message": message,
    })
}

/// Writes `line` to stdout as one line.
pub fn emit(line: &Value) {
    println!("{line}");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use henad_models::example_models;
    use serde_json::json;

    use super::params;

    #[test]
    fn params_json_lists_every_descriptor() {
        let models = example_models();
        for entry in models.runnable(None) {
            let line = params(entry, None);
            assert_eq!(line["kind"], json!("params"), "{}", entry.id());
            assert_eq!(line["model"], json!(entry.id()));
            let params = line["params"].as_array().expect("params is a list");
            let ids: Vec<&str> = params.iter().filter_map(|param| param["id"].as_str()).collect();
            let declared: Vec<&str> = entry.param_descriptors().iter().map(|d| d.id()).collect();
            assert_eq!(ids, declared, "{}", entry.id());
            for param in params {
                assert!(param["kind"].is_string() && !param["default"].is_null(), "{param}");
            }
            let columns = line["stat_columns"]
                .as_array()
                .expect("the model builds at its defaults");
            assert!(columns.len() >= entry.stat_descriptors().len(), "{}", entry.id());
            let actions = line["actions"].as_array().expect("actions is a list");
            assert_eq!(actions.len(), entry.action_descriptors().len(), "{}", entry.id());
        }
    }

    /// Checks that an `explore_progress` line holds the fields `docs/reference/cli.md` lists. A sweep quick enough for
    /// the integration tests sends no such line.
    #[test]
    fn a_progress_line_holds_the_documented_fields() {
        let update = henad_explore::progress::ProgressUpdate::new(3, 8, 1, Duration::from_secs(2));
        let line = super::explore_progress(&update, 2);
        let mut fields: Vec<&str> = line
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        let mut expected = ["kind", "done", "total", "skipped", "failed", "elapsed_s", "remaining_s"];
        expected.sort_unstable();
        assert_eq!(fields, expected);
        assert_eq!(
            (&line["kind"], &line["skipped"]),
            (&"explore_progress".into(), &2.into())
        );
    }

    /// Checks that a `SweepWarning::BuildChanged` prints as an `explore_warning` line with both builds and its message.
    #[test]
    fn a_build_change_prints_as_an_explore_warning_line() {
        use henad_explore::output::manifest::{BuildRole, RecordedBuild};
        use henad_explore::sweep::SweepWarning;

        let current = RecordedBuild::engine();
        let mut recorded = current.clone();
        recorded.version = "0.1.0".to_owned();
        let warning = SweepWarning::BuildChanged {
            role: BuildRole::Engine,
            recorded: Box::new(recorded),
            current: Box::new(current),
            between_shards: false,
        };
        let line = super::explore_warning(&warning);
        assert_eq!(line["kind"], "explore_warning");
        assert_eq!(line["warning"], "build_changed");
        assert_eq!(line["role"], "engine");
        assert_eq!(line["recorded"]["version"], "0.1.0");
        assert_eq!(line["current"]["name"], "henad");
        assert_eq!(line["between_shards"], false);
        assert_eq!(line["message"], warning.to_string());
    }
}
