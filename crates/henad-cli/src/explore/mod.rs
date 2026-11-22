//! Sweeps and searches from the command line, selected by `--out`, `--spec` or `--dry-run`, and shard merges selected
//! by `--merge`.
//!
//! Flags build a sweep of one block: a factorial over every `--vary`, a zip with `--zip`, a sample with `--sample`,
//! or a table with `--design`. A spec file describes the whole sweep instead, and conflicts with every flag that
//! changes a result. A spec file with a `[search]` table runs a search. Flags cannot describe a search.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::spec::SweepSpec;
use henad_explore::handle::SweepOutput;
use henad_explore::merge::{MergeError, merge};
use henad_explore::model_file::ModelFile;
use henad_explore::output::MANIFEST_FILE;
use henad_explore::output::manifest::{Manifest, ResultCounts};
use henad_explore::output::resume::ResumeError;
use henad_explore::probe::ShortfallWords;
use henad_explore::spec_file::LoadedSpec;
use henad_explore::sweep::{ExploreError, Provenance, SweepEnd, SweepOptions, plan_spec, run_spec};

use crate::json_report;
use crate::{Args, SOME_RUNS_NOT_OK};

mod args;
pub(crate) mod report;

pub use args::ExploreArgs;
use args::{flags_source, spec_from_flags};
use report::{Reporter, WarningPrinter, merge_text};

/// Runs a sweep of `entry` from the flags or `spec`, and returns the exit status.
///
/// `model_file` is the path and the model file that `entry` came from, if any. The configs of a sheet past the device,
/// and the candidates a search draws past it, are then refused in the sheet's lines, as a single run of the sheet is. A
/// resume refused for changed model code then names the file.
///
/// The status is success when every run is `ok`, and [`SOME_RUNS_NOT_OK`] when the sweep ran to its end with some
/// run in another status.
///
/// # Errors
///
/// Returns an error when the flags or the spec cannot make a sweep, the sweep cannot be planned, or its results
/// cannot be written.
pub fn run(
    args: &Args,
    entry: &ModelEntry,
    model_file: Option<(&Path, &ModelFile)>,
    gpu: Option<&GpuContext>,
    spec: Option<LoadedSpec>,
    provenance: Provenance,
) -> Result<u8> {
    let (sweep, mut options) = sweep_and_options(args, entry, spec, provenance)?;
    if let Some(sheet) = model_file.and_then(|(_, file)| file.sheet()) {
        let sheet = sheet.clone();
        options.shortfall_words = Some(ShortfallWords::new(move |entry, params, limits| {
            sheet.shortfalls(entry, params, limits)
        }));
    }
    if cfg!(debug_assertions) && !args.explore.dry_run {
        eprintln!("!!! warning: debug build. Runs will step slowly and timings will be unreliable. Use --release !!!");
    }
    let mut reporter = Reporter::new(entry.name(), args, options.spec_source.path.is_some());
    let path = model_file.map(|(path, _)| path);
    let report = match &args.explore.out {
        _ if args.explore.dry_run => {
            plan_spec(entry, gpu, &sweep, args.explore.out.as_deref(), &options, &mut reporter)
                .map_err(|error| changed_file_context(error, path))?
        }
        Some(output_dir) => {
            let output = SweepOutput::Directory(output_dir.clone());
            run_spec(entry, gpu, &sweep, output, &options, &mut reporter)
                .map_err(|error| changed_file_context(error, path))?
                .report
        }
        None => bail!("a sweep needs an output directory unless it is a dry run"),
    };
    exit_status(report.end, &report.counts)
}

/// Returns `error` as an error of the command, with a first line naming the model file at `path` when the error is a
/// resume refused for the changed code of the model that file holds.
fn changed_file_context(error: ExploreError, path: Option<&Path>) -> anyhow::Error {
    let changed_code = matches!(error, ExploreError::Resume(ResumeError::CodeChanged { .. }));
    let error = anyhow::Error::new(error);
    match path {
        Some(path) if changed_code => error.context(format!(
            "model file '{}' holds different code from the runs in the directory",
            path.display()
        )),
        _ => error,
    }
}

/// Returns the sweep the flags or `spec` describe, and its options.
///
/// A spec's `[execution]` table is applied first, and then each of `--concurrent`, `--memory` and `--gpu-memory` given
/// on the command line replaces its setting.
///
/// # Errors
///
/// Returns an error when the flags cannot make a sweep.
fn sweep_and_options(
    args: &Args,
    entry: &ModelEntry,
    spec: Option<LoadedSpec>,
    provenance: Provenance,
) -> Result<(SweepSpec, SweepOptions)> {
    let mut options = SweepOptions::new(provenance);
    let sweep = if let Some(loaded) = spec {
        options.apply_execution(&loaded.execution);
        options.spec_source = loaded.spec_source;
        loaded.spec
    } else {
        let sweep = spec_from_flags(args, entry.id())?;
        options.spec_source = flags_source(args, &sweep);
        sweep
    };
    if let Some(concurrency) = args.explore.concurrent {
        options.concurrency = concurrency;
    }
    if let Some(memory) = args.explore.memory {
        options.memory_budget = Some(memory);
    }
    if let Some(gpu_memory) = args.explore.gpu_memory {
        options.gpu_memory_budget = Some(gpu_memory);
    }
    options.shard = args.explore.shard.unwrap_or_default();
    options.resume = args.explore.resume;
    options.retry_failed = args.explore.retry_failed;
    Ok((sweep, options))
}

/// Merges the shard directories that `--merge` lists into the `--out` directory, and returns the exit status.
///
/// The status is success when the merged directory holds every run of the plan and every run is `ok`, and
/// [`SOME_RUNS_NOT_OK`] otherwise.
///
/// # Errors
///
/// Returns an error when the directories cannot be merged.
pub fn merge_shards(args: &Args) -> Result<u8> {
    let output_dir = args.explore.out.as_deref().context("--merge requires --out")?;
    let report = merge(&args.explore.merge, output_dir, &mut WarningPrinter { json: args.json }).map_err(|error| {
        let context = match &error {
            MergeError::CodeChanged { dir, lower_dir, .. } => changed_files_text(lower_dir, dir),
            _ => None,
        };
        let error = anyhow::Error::new(error);
        match context {
            Some(context) => error.context(context),
            None => error,
        }
    })?;
    if args.json {
        json_report::emit(&json_report::explore_merge(&report, args.explore.merge.len()));
    } else {
        eprintln!("{}", merge_text(&report, args.explore.merge.len()));
    }
    let counts = &report.counts;
    if report.missing == 0 && counts.ok == counts.rows {
        Ok(0)
    } else {
        Ok(SOME_RUNS_NOT_OK)
    }
}

/// Returns the paths that the command line recorded in the manifest of `dir` gives with `--model-file`, or no path
/// when the manifest cannot be read.
fn recorded_model_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(manifest) = Manifest::read(&dir.join(MANIFEST_FILE)) else {
        return Vec::new();
    };
    let mut arguments = manifest.argv.iter();
    let mut paths = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--model-file" {
            paths.extend(arguments.next().map(PathBuf::from));
        } else if let Some(path) = argument.strip_prefix("--model-file=") {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}

/// Returns a line naming the model files that the shards in `lower_dir` and `dir` ran, for a merge refused because the
/// two ran different model code, or `None` when neither ran a model file.
fn changed_files_text(lower_dir: &Path, dir: &Path) -> Option<String> {
    let (lower, other) = (recorded_model_files(lower_dir), recorded_model_files(dir));
    let ran = |dir: &Path, paths: &[PathBuf]| format!("'{}' ran with {}", dir.display(), model_file_words(paths));
    match (lower.is_empty(), other.is_empty()) {
        (true, true) => None,
        _ if lower == other => Some(format!(
            "the shards ran different versions of {}",
            model_file_words(&lower)
        )),
        (false, false) => Some(format!("{}, and {}", ran(lower_dir, &lower), ran(dir, &other))),
        (false, true) => Some(ran(lower_dir, &lower)),
        (true, false) => Some(ran(dir, &other)),
    }
}

/// Returns `paths` as `model file 'a'` or `model files 'a', 'b'`.
fn model_file_words(paths: &[PathBuf]) -> String {
    let quoted: Vec<String> = paths.iter().map(|path| format!("'{}'", path.display())).collect();
    let noun = if paths.len() == 1 { "model file" } else { "model files" };
    format!("{noun} {}", quoted.join(", "))
}

/// Returns the exit status of a sweep or search that ended as `end` with `counts` rows written.
///
/// # Errors
///
/// Returns an error for a sweep that was aborted or lost its GPU device.
fn exit_status(end: SweepEnd, counts: &ResultCounts) -> Result<u8> {
    match end {
        SweepEnd::Planned => Ok(0),
        SweepEnd::Complete if counts.ok == counts.rows => Ok(0),
        SweepEnd::Complete => Ok(SOME_RUNS_NOT_OK),
        SweepEnd::Aborted => bail!("the sweep was aborted after {} runs", counts.rows),
        SweepEnd::DeviceLost => bail!(
            "the GPU device was lost after {} runs. Use --resume to run the rest",
            counts.rows
        ),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use henad_explore::exec::Concurrency;
    use henad_explore::output::manifest::ResultCounts;
    use henad_explore::sweep::SweepEnd;
    use henad_models::example_models;

    use super::{LoadedSpec, SOME_RUNS_NOT_OK, exit_status, sweep_and_options};
    use crate::{Args, test_provenance};

    #[test]
    fn a_sweep_exits_with_success_only_when_every_run_is_ok() {
        let counts = |ok, non_finite, failed| ResultCounts {
            rows: ok + non_finite + failed,
            ok,
            non_finite,
            failed,
        };
        let status = |end, counts: ResultCounts| exit_status(end, &counts).ok();
        assert_eq!(status(SweepEnd::Complete, counts(4, 0, 0)), Some(0));
        assert_eq!(status(SweepEnd::Planned, counts(0, 0, 0)), Some(0));
        let some_not_ok = Some(SOME_RUNS_NOT_OK);
        assert_eq!(status(SweepEnd::Complete, counts(3, 0, 1)), some_not_ok);
        assert_eq!(status(SweepEnd::Complete, counts(3, 1, 0)), some_not_ok);
        assert_eq!(status(SweepEnd::Aborted, counts(2, 0, 0)), None, "an abort is an error");
        assert_eq!(
            status(SweepEnd::DeviceLost, counts(2, 0, 0)),
            None,
            "a lost device is an error"
        );
    }

    /// Checks that a flag given on the command line replaces the spec table's setting, `--concurrent auto` included,
    /// and that a setting with no flag keeps the table's value.
    #[test]
    fn an_explicit_concurrent_auto_overrides_the_spec_table() {
        let dir = std::env::temp_dir().join(format!("henad-cli-execution-table-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
        let path = dir.join("spec.toml");
        std::fs::write(
            &path,
            "model = \"sir\"\n[execution]\nconcurrent = 4\nmemory = 1000\ngpu_memory = 2000\n",
        )
        .expect("the spec can be written");
        let spec_arg = path.to_str().expect("the temporary directory is UTF-8");
        let entry = example_models().get("sir").cloned().expect("sir is registered");
        let options = |extra: &[&str]| {
            let mut line = vec!["henad-cli", "--spec", spec_arg, "--dry-run"];
            line.extend(extra);
            let args = Args::try_parse_from(line).expect("the line parses");
            let loaded = LoadedSpec::read(&path).expect("the spec reads");
            sweep_and_options(&args, &entry, Some(loaded), test_provenance())
                .expect("a sweep")
                .1
        };

        let from_table = options(&[]);
        assert_eq!(from_table.concurrency.to_string(), "4");
        assert_eq!(
            (from_table.memory_budget, from_table.gpu_memory_budget),
            (Some(1000), Some(2000))
        );

        let overridden = options(&["--concurrent", "auto", "--memory", "500"]);
        assert_eq!(overridden.concurrency, Concurrency::Auto);
        assert_eq!(
            (overridden.memory_budget, overridden.gpu_memory_budget),
            (Some(500), Some(2000))
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
