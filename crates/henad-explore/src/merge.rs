//! Merges of shard directories into one directory, holding the files one sweep of the whole plan would have written.
//!
//! The inputs are shards of one plan run with one replicate count, each with a shard index of its own. `runs.csv`
//! and `series.csv` are merged by run id and written together, as a resume replaces them. `summary.csv` is rebuilt,
//! and the manifest lists the merged directories.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use henad_core::explore::plan::Shard;

use crate::output::manifest::{
    BuildRole, Manifest, ManifestError, ManifestMode, ManifestStatus, ManifestTimestamps, RecordedBuild, ResultCounts,
    now_unix_ms, rfc3339,
};
use crate::output::read::{ReadError, RunRecord, RunsCsv, SeriesScan, SeriesSegment, merge_series};
use crate::output::runs_csv::header_line;
use crate::output::{MANIFEST_FILE, OutputDir, OutputError, RUNS_FILE, SERIES_FILE, table_paths};
use crate::progress::{Progress, ProgressEvent};
use crate::sweep::SweepWarning;

/// Most missing run ids a [`SweepWarning::MissingRuns`] lists.
pub const MAX_LISTED_RUNS: usize = 5;

/// Result of a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    /// Rows of the merged `runs.csv`, by status.
    pub counts: ResultCounts,
    /// Runs of the plan that no input holds.
    pub missing: u64,
    pub output_dir: PathBuf,
}

/// One shard directory read for a merge.
struct ShardInput {
    dir: PathBuf,
    manifest: Manifest,
    shard: Shard,
    runs: RunsCsv,
    series: SeriesScan,
    /// Ids of the runs `runs.csv` holds.
    run_ids: BTreeSet<u64>,
}

/// Merges the shard directories `shard_dirs` into `output_dir`. The output directory must hold no results.
///
/// Rows of a run whose `runs.csv` row is missing, and a partial last record, are left out. A plan run that no input
/// holds is reported as a [`ProgressEvent::Warned`] and marks the merged manifest `incomplete`. Shards whose
/// sessions ran different engine or model builds are reported as a [`SweepWarning::BuildChanged`] for each build
/// that differs from the lowest shard's. A resume of the
/// merged directory runs it. A merge that fails once its manifest is written marks the manifest `failed` when it can.
///
/// # Errors
///
/// Returns [`MergeError`] when there are no inputs, an input cannot be read, the inputs hold different plans,
/// replicate counts, shard counts or columns, two inputs hold one shard, a run's id does not match its config and
/// replicate, or the merged directory cannot be written.
pub fn merge(
    shard_dirs: &[PathBuf],
    output_dir: &Path,
    progress: &mut dyn Progress,
) -> Result<MergeReport, MergeError> {
    let mut shards = shard_dirs
        .iter()
        .map(|dir| read_input(dir))
        .collect::<Result<Vec<_>, _>>()?;
    shards.sort_by_key(|input| input.shard.index());
    let Some(first) = shards.first() else {
        return Err(MergeError::NoInputs);
    };
    check_shards(&shards)?;
    for warning in build_warnings(&shards) {
        progress.report(&ProgressEvent::Warned(&warning));
    }
    let (runs_header, series_header) = common_headers(&shards)?;
    let plan_runs = first.manifest.plan.runs;
    let (records, counts) = merged_records(&shards, plan_runs, first.manifest.plan.replicates)?;

    let dir = OutputDir::create(output_dir).map_err(MergeError::Output)?;
    let mut manifest = merged_manifest(&shards, shard_dirs);
    dir.write_manifest(&manifest).map_err(MergeError::Output)?;
    if let Err(error) = write_tables(&dir, &shards, &records, &runs_header, &series_header) {
        manifest.fail(now_unix_ms());
        // The merge's own error is the one to report. A manifest that cannot be written says `running`.
        drop(dir.write_manifest(&manifest));
        return Err(MergeError::Output(error));
    }

    let missing = plan_runs - records.len() as u64;
    if missing > 0 {
        let first_missing = (0..plan_runs)
            .filter(|run_id| !records.contains_key(run_id))
            .take(MAX_LISTED_RUNS)
            .collect();
        progress.report(&ProgressEvent::Warned(&SweepWarning::MissingRuns {
            count: missing,
            first: first_missing,
        }));
    }
    let status = if missing == 0 {
        ManifestStatus::Complete
    } else {
        ManifestStatus::Incomplete
    };
    // Every session of every shard is credited already, so the counts go in without `Manifest::finish`.
    let finished = now_unix_ms();
    manifest.status = status;
    manifest.results = Some(counts);
    manifest.timestamps.finished_unix_ms = Some(finished);
    manifest.timestamps.finished = Some(rfc3339(finished));
    dir.write_manifest(&manifest).map_err(MergeError::Output)?;
    Ok(MergeReport {
        counts,
        missing,
        output_dir: output_dir.to_owned(),
    })
}

/// Writes the merged `runs.csv` and `series.csv` into `dir` through [`OutputDir::replace_tables`], then rebuilds
/// `summary.csv`.
fn write_tables(
    dir: &OutputDir,
    shards: &[ShardInput],
    records: &BTreeMap<u64, &RunRecord>,
    runs_header: &str,
    series_header: &str,
) -> Result<(), OutputError> {
    let segments: Vec<SeriesSegment> = shards
        .iter()
        .enumerate()
        .flat_map(|(input_index, input)| {
            input.series.segments.iter().map(move |range| SeriesSegment {
                path: input.series.path.clone(),
                range: range.clone(),
                input_index,
            })
        })
        .collect();
    dir.replace_tables(
        |dest| {
            dest.write_all(runs_header.as_bytes())?;
            records
                .values()
                .try_for_each(|record| dest.write_all(record.text.as_bytes()))
        },
        |dest| {
            merge_series(dest, series_header, &segments, |input_index, run_id| {
                shards[input_index].run_ids.contains(&run_id).then_some(run_id)
            })
        },
    )?;
    dir.write_summary()
}

/// Returns the header lines of `runs.csv` and `series.csv` every input that has them shares.
fn common_headers(shards: &[ShardInput]) -> Result<(String, String), MergeError> {
    let runs_header = shards
        .iter()
        .find_map(|input| input.runs.header.as_ref())
        .map(|header| header_line(header));
    let series_header = shards.iter().find_map(|input| input.series.header.clone());
    let (Some(runs_header), Some(series_header)) = (runs_header, series_header) else {
        return Err(MergeError::NoTables);
    };
    for input in shards {
        let differs = |file| MergeError::ColumnsDiffer {
            dir: input.dir.clone(),
            file,
        };
        if input
            .runs
            .header
            .as_ref()
            .is_some_and(|header| header_line(header) != runs_header)
        {
            return Err(differs(RUNS_FILE));
        }
        if input
            .series
            .header
            .as_ref()
            .is_some_and(|header| *header != series_header)
        {
            return Err(differs(SERIES_FILE));
        }
    }
    Ok((runs_header, series_header))
}

/// Returns the records of every input by run id, and their counts by status.
///
/// A record whose run id is not `config_id * replicates + rep` is refused.
fn merged_records(
    shards: &[ShardInput],
    plan_runs: u64,
    replicates: u64,
) -> Result<(BTreeMap<u64, &RunRecord>, ResultCounts), MergeError> {
    let mut records = BTreeMap::new();
    let mut counts = ResultCounts::default();
    for input in shards {
        for record in &input.runs.records {
            let numbered = record
                .config_id
                .checked_mul(replicates)
                .and_then(|first| first.checked_add(record.rep));
            if record.rep >= replicates || numbered != Some(record.run_id) {
                return Err(MergeError::RunIdDiffers {
                    dir: input.dir.clone(),
                    run_id: record.run_id,
                    config_id: record.config_id,
                    rep: record.rep,
                });
            }
            if !input.shard.contains(record.run_id) || record.run_id >= plan_runs {
                return Err(MergeError::OutsideShard {
                    dir: input.dir.clone(),
                    run_id: record.run_id,
                });
            }
            if records.insert(record.run_id, record).is_some() {
                return Err(MergeError::DuplicateRun {
                    dir: input.dir.clone(),
                    run_id: record.run_id,
                });
            }
            counts.count(record.status);
        }
    }
    Ok((records, counts))
}

/// Reads the manifest and tables of the shard directory `dir`.
fn read_input(dir: &Path) -> Result<ShardInput, MergeError> {
    let manifest = Manifest::read(&dir.join(MANIFEST_FILE)).map_err(MergeError::Manifest)?;
    if manifest.mode != ManifestMode::Sweep {
        return Err(MergeError::NotASweep { dir: dir.to_owned() });
    }
    let shard = manifest
        .shard
        .to_shard()
        .ok_or_else(|| MergeError::BadShard { dir: dir.to_owned() })?;
    let (runs_path, series_path) = table_paths(dir);
    let runs = RunsCsv::read(&runs_path).map_err(MergeError::Table)?;
    let run_ids: BTreeSet<u64> = runs.records.iter().map(|record| record.run_id).collect();
    let series = SeriesScan::read(&series_path, |run_id| run_ids.contains(&run_id)).map_err(MergeError::Table)?;
    Ok(ShardInput {
        dir: dir.to_owned(),
        manifest,
        shard,
        runs,
        series,
        run_ids,
    })
}

/// Checks that `shards`, sorted by shard index, are distinct shards of one plan with one replicate count.
fn check_shards(shards: &[ShardInput]) -> Result<(), MergeError> {
    let Some(first) = shards.first() else {
        return Err(MergeError::NoInputs);
    };
    for pair in shards.windows(2) {
        if pair[0].shard == pair[1].shard {
            return Err(MergeError::SameShard {
                shard: pair[0].shard,
                first: pair[0].dir.clone(),
                second: pair[1].dir.clone(),
            });
        }
    }
    for input in shards {
        let (plan, expected) = (&input.manifest.plan, &first.manifest.plan);
        if plan.plan_hash != expected.plan_hash
            || input.manifest.model.schema_hash != first.manifest.model.schema_hash
            || plan.replicates != expected.replicates
            || plan.runs != expected.runs
        {
            return Err(MergeError::PlanDiffers {
                dir: input.dir.clone(),
                first: first.dir.clone(),
            });
        }
        if input.shard.count() != first.shard.count() {
            return Err(MergeError::ShardCountDiffers {
                dir: input.dir.clone(),
                first: first.dir.clone(),
            });
        }
    }
    Ok(())
}

/// Returns a [`SweepWarning::BuildChanged`] for each engine or model build the shards' sessions record that differs
/// from the first build recorded for its role, the lowest shard's.
fn build_warnings(shards: &[ShardInput]) -> Vec<SweepWarning> {
    let mut warnings = Vec::new();
    for role in [BuildRole::Engine, BuildRole::Model] {
        let mut builds: Vec<RecordedBuild> = Vec::new();
        for build in shards.iter().flat_map(|input| input.manifest.recorded_builds(role)) {
            if !builds.iter().any(|known| known.same_build(&build)) {
                builds.push(build);
            }
        }
        if let Some((first, others)) = builds.split_first() {
            warnings.extend(others.iter().map(|other| SweepWarning::BuildChanged {
                role,
                recorded: Box::new(first.clone()),
                current: Box::new(other.clone()),
                between_shards: true,
            }));
        }
    }
    warnings
}

/// Returns the manifest of the merged directory while it is written, based on the manifest of the lowest shard.
///
/// The merge holds the whole plan, lists every session of every shard with the builds it ran, and keeps the
/// execution and runtime of the lowest shard. A shard's session that records no engine build takes the one its
/// shard's manifest reads for it. The engine block is the build that merges.
fn merged_manifest(shards: &[ShardInput], shard_dirs: &[PathBuf]) -> Manifest {
    let mut manifest = shards[0].manifest.clone();
    manifest.status = ManifestStatus::Running;
    manifest.shard = Shard::WHOLE.into();
    manifest.engine = RecordedBuild::engine();
    manifest.sessions = shards
        .iter()
        .flat_map(|input| {
            let mut recorded = input.manifest.clone();
            recorded.record_session_engines();
            recorded.sessions
        })
        .collect();
    let started = shards
        .iter()
        .map(|input| input.manifest.timestamps.started_unix_ms)
        .min()
        .unwrap_or_default();
    manifest.timestamps = ManifestTimestamps::started_at(started);
    manifest.results = None;
    manifest.merged_shards = Some(shard_dirs.iter().map(|dir| dir.display().to_string()).collect());
    manifest
}

/// Shard directories that cannot be merged.
#[derive(Debug)]
pub enum MergeError {
    /// A merge with no input directories.
    NoInputs,
    /// A manifest that cannot be read, for the reason inside.
    Manifest(ManifestError),
    /// A table that cannot be read back, for the reason inside.
    Table(ReadError),
    /// A manifest in `dir` whose shard no plan can have.
    BadShard { dir: PathBuf },
    /// Inputs with no `runs.csv` or `series.csv` header between them.
    NoTables,
    /// Input `dir` holds another plan, model schema or replicate count than input `first`.
    PlanDiffers { dir: PathBuf, first: PathBuf },
    /// Input `dir` splits the plan into another number of shards than input `first`.
    ShardCountDiffers { dir: PathBuf, first: PathBuf },
    /// Inputs `first` and `second` both hold `shard`.
    SameShard {
        shard: Shard,
        first: PathBuf,
        second: PathBuf,
    },
    /// `file` of input `dir` has other columns than the other inputs.
    ColumnsDiffer { dir: PathBuf, file: &'static str },
    /// Run `run_id` of input `dir`, written as replicate `rep` of config `config_id` under another id than the
    /// manifest's replicate count gives it.
    RunIdDiffers {
        dir: PathBuf,
        run_id: u64,
        config_id: u64,
        rep: u64,
    },
    /// Run `run_id` of input `dir`, outside its shard or past the plan's runs.
    OutsideShard { dir: PathBuf, run_id: u64 },
    /// Run `run_id`, listed twice in `runs.csv` of input `dir`.
    DuplicateRun { dir: PathBuf, run_id: u64 },
    /// The merged directory cannot be written, for the reason inside.
    Output(OutputError),
    /// Input `dir` holds a search, which runs whole and has no shards.
    NotASweep { dir: PathBuf },
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputs => f.write_str("a merge needs at least one shard directory"),
            Self::Manifest(_) => f.write_str("cannot read the manifest of a shard"),
            Self::Table(_) => f.write_str("cannot read the tables of a shard"),
            Self::BadShard { dir } => write!(f, "'{}' records an invalid shard", dir.display()),
            Self::NoTables => f.write_str("no shard holds a runs.csv and a series.csv with a header"),
            Self::PlanDiffers { dir, first } => write!(
                f,
                "'{}' has a different plan, model schema or replicate count from '{}'",
                dir.display(),
                first.display()
            ),
            Self::ShardCountDiffers { dir, first } => write!(
                f,
                "'{}' has a different shard count from '{}'",
                dir.display(),
                first.display()
            ),
            Self::SameShard { shard, first, second } => write!(
                f,
                "'{}' and '{}' both hold shard {shard}",
                first.display(),
                second.display()
            ),
            Self::ColumnsDiffer { dir, file } => {
                write!(
                    f,
                    "{file} of '{}' has different columns from the other shards",
                    dir.display()
                )
            }
            Self::RunIdDiffers {
                dir,
                run_id,
                config_id,
                rep,
            } => write!(
                f,
                "'{}' holds replicate {rep} of config {config_id} as run {run_id}, and its manifest gives a \
                 different run id. Resume that shard before merging it",
                dir.display()
            ),
            Self::OutsideShard { dir, run_id } => {
                write!(f, "'{}' holds run {run_id}, outside its shard", dir.display())
            }
            Self::DuplicateRun { dir, run_id } => {
                write!(f, "'{}' lists run {run_id} twice", dir.display())
            }
            Self::Output(_) => f.write_str("cannot write the merged results"),
            Self::NotASweep { dir } => write!(f, "'{}' holds a search. A search has no shards to merge", dir.display()),
        }
    }
}

impl std::error::Error for MergeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(error) => Some(error),
            Self::Table(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::NoInputs
            | Self::BadShard { .. }
            | Self::NoTables
            | Self::PlanDiffers { .. }
            | Self::ShardCountDiffers { .. }
            | Self::SameShard { .. }
            | Self::ColumnsDiffer { .. }
            | Self::RunIdDiffers { .. }
            | Self::OutsideShard { .. }
            | Self::DuplicateRun { .. }
            | Self::NotASweep { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use henad_core::explore::spec::SweepSpec;

    use super::{MergeError, merge};
    use crate::exec::Concurrency;
    use crate::output::RUNS_FILE;
    use crate::progress::NoProgress;
    use crate::tests::support::{ScratchDir, entry, sweep};

    #[test]
    fn a_shard_listing_a_run_twice_is_refused() {
        let scratch = ScratchDir::new("merge-duplicate");
        let shard_dir = scratch.path().join("shard");
        let mut spec = SweepSpec::new("game_of_life");
        spec.fixed = vec![
            ("grid_width".to_owned(), "8".to_owned()),
            ("grid_height".to_owned(), "8".to_owned()),
        ];
        spec.run.steps = 2;
        spec.run.replicates = 2;
        sweep(&entry("game_of_life", None), None, &spec, &shard_dir, Concurrency::Auto);
        let runs_path = shard_dir.join(RUNS_FILE);
        let runs = fs::read_to_string(&runs_path).expect("runs.csv is written");
        let last_row = runs.lines().last().expect("runs.csv holds a run");
        fs::write(&runs_path, format!("{runs}{last_row}\n")).expect("runs.csv is written again");

        let error = merge(
            std::slice::from_ref(&shard_dir),
            &scratch.path().join("merged"),
            &mut NoProgress,
        )
        .expect_err("a run listed twice");
        assert!(matches!(error, MergeError::DuplicateRun { run_id: 1, .. }), "{error:?}");
        assert_eq!(
            error.to_string(),
            format!("'{}' lists run 1 twice", shard_dir.display())
        );
    }
}
