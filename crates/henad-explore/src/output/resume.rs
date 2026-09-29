//! Resumes of a sweep into a directory that holds some of its runs.
//!
//! A resume takes a directory only when its manifest names the same plan, model schema and shard, and no more
//! replicates than the sweep runs. It keeps every run that ended `ok` or `non_finite`, and every failed run unless
//! asked to retry failures. A run that timed out is always run again. Kept runs take their ids in the current plan.
//! A changed replicate count gives them new ids.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::OpenOptions;
use std::path::Path;

use henad_core::explore::outcome::RunStatus;
use henad_core::explore::plan::{Plan, Shard};

use crate::output::manifest::{Manifest, ManifestError, ManifestMode, ManifestShard, ResultCounts};
use crate::output::read::{ReadError, RunRecord, RunsCsv, SeriesScan, SeriesSegment, merge_series};
use crate::output::runs_csv::header_line;
use crate::output::{MANIFEST_FILE, OutputDir, OutputError, RUNS_FILE, SERIES_FILE, table_paths};
use crate::sweep::hex;

/// Runs a directory holds for a resume, and the repair its tables need.
#[derive(Debug)]
pub struct ResumeScan {
    /// Manifest the directory holds.
    pub recorded: Manifest,
    runs: RunsCsv,
    series: SeriesScan,
    /// Id in the current plan of each record of `runs`, in record order. `None` for a record left out.
    kept: Vec<Option<u64>>,
    finished: BTreeSet<u64>,
    counts: ResultCounts,
    repair: Repair,
    /// Header line of `runs.csv` the sweep writes.
    runs_header: String,
    /// Header line of `series.csv` the sweep writes.
    series_header: String,
}

/// Change a resumed directory's tables need before new runs are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repair {
    None,
    /// Each table is cut to the given length, dropping a partial last record and the rows of runs never committed.
    Truncate {
        runs_bytes: u64,
        series_bytes: u64,
    },
    /// Both tables are written again from their kept runs.
    Rewrite,
}

impl ResumeScan {
    /// Reads the directory at `path` for a resume of the runs of `plan` in `shard`, without changing it.
    ///
    /// `runs_header` and `series_header` are the header lines the sweep writes to `runs.csv` and `series.csv`. With
    /// `retry_failed`, a run that ended on a fault is run again.
    ///
    /// # Errors
    ///
    /// Returns [`ResumeError`] when the manifest or a table cannot be read, the directory holds another plan, model
    /// schema, shard or column layout or more replicates, or a row names a run the plan does not have.
    pub fn read(
        path: &Path,
        plan: &Plan,
        shard: Shard,
        retry_failed: bool,
        runs_header: &str,
        series_header: &str,
    ) -> Result<Self, ResumeError> {
        let recorded = Manifest::read(&path.join(MANIFEST_FILE)).map_err(ResumeError::Manifest)?;
        check_recorded(&recorded, plan, shard)?;

        let (runs_path, series_path) = table_paths(path);
        let runs = RunsCsv::read(&runs_path).map_err(ResumeError::Table)?;
        if let Some(header) = &runs.header
            && header_line(header) != runs_header
        {
            return Err(ResumeError::ColumnsChanged { file: RUNS_FILE });
        }
        let mut kept = Vec::with_capacity(runs.records.len());
        let mut finished = BTreeSet::new();
        // Current id of each kept run, by the id it was written with.
        let mut written_ids = BTreeMap::new();
        // Ids of every record, as written and in the current plan. Series rows name a run by its written id alone.
        let (mut seen_written, mut seen_current) = (BTreeSet::new(), BTreeSet::new());
        let mut counts = ResultCounts::default();
        for record in &runs.records {
            let run_id = current_id(plan, shard, record.config_id, record.rep, record.run_key, record.run_id)?;
            if !seen_written.insert(record.run_id) || !seen_current.insert(run_id) {
                return Err(ResumeError::DuplicateRun { run_id: record.run_id });
            }
            let keep = match record.status {
                RunStatus::Ok | RunStatus::NonFinite => true,
                RunStatus::TimedOut => false,
                RunStatus::Panicked | RunStatus::GpuError | RunStatus::Refused | RunStatus::ShapeError => !retry_failed,
            };
            if keep {
                counts.count(record.status);
                finished.insert(run_id);
                written_ids.insert(record.run_id, run_id);
                kept.push(Some(run_id));
            } else {
                kept.push(None);
            }
        }

        let series =
            SeriesScan::read(&series_path, |run_id| written_ids.contains_key(&run_id)).map_err(ResumeError::Table)?;
        if series.header.as_deref().is_some_and(|header| header != series_header) {
            return Err(ResumeError::ColumnsChanged { file: SERIES_FILE });
        }
        let untouched = kept
            .iter()
            .zip(&runs.records)
            .all(|(run_id, record)| *run_id == Some(record.run_id));
        let rewrite = !untouched
            || runs.header.is_none()
            || series.header.is_none()
            || !runs.records.is_sorted_by_key(|record| record.run_id)
            || series.segments.len() > 1
            || series.kept_after_dropped;
        let repair = if rewrite {
            Repair::Rewrite
        } else {
            let series_bytes = series.first_dropped_offset.unwrap_or(series.complete_bytes);
            if runs.complete_bytes < runs.file_bytes || series_bytes < series.file_bytes {
                Repair::Truncate {
                    runs_bytes: runs.complete_bytes,
                    series_bytes,
                }
            } else {
                Repair::None
            }
        };
        Ok(Self {
            recorded,
            runs,
            series,
            kept,
            finished,
            counts,
            repair,
            runs_header: runs_header.to_owned(),
            series_header: series_header.to_owned(),
        })
    }

    /// Ids in the current plan of the runs the directory keeps.
    pub fn finished(&self) -> &BTreeSet<u64> {
        &self.finished
    }

    /// Counts of the kept rows, by status.
    pub fn counts(&self) -> ResultCounts {
        self.counts
    }

    /// Cuts or rewrites the tables of `dir` to hold the kept runs alone, with the ids the current plan gives them.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Write`] when a table cannot be cut or replaced.
    pub fn repair(&self, dir: &OutputDir) -> Result<(), OutputError> {
        match self.repair {
            Repair::None => Ok(()),
            Repair::Truncate {
                runs_bytes,
                series_bytes,
            } => {
                for (file, length) in [(RUNS_FILE, runs_bytes), (SERIES_FILE, series_bytes)] {
                    let path = dir.path().join(file);
                    OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .and_then(|table| table.set_len(length))
                        .map_err(|source| OutputError::Write { path, source })?;
                }
                Ok(())
            }
            Repair::Rewrite => self.rewrite(dir),
        }
    }

    /// Writes both tables again from the kept runs, in order of their ids in the current plan.
    fn rewrite(&self, dir: &OutputDir) -> Result<(), OutputError> {
        let mut records: Vec<(u64, &RunRecord)> = Vec::with_capacity(self.finished.len());
        let mut renumbering = BTreeMap::new();
        for (record, run_id) in self.runs.records.iter().zip(&self.kept) {
            if let Some(run_id) = *run_id {
                renumbering.insert(record.run_id, run_id);
                records.push((run_id, record));
            }
        }
        records.sort_by_key(|&(run_id, _)| run_id);
        // Opening the directory moved any staged tables into place, so the scanned series is at its own name.
        let series_path = dir.path().join(SERIES_FILE);
        let segments: Vec<SeriesSegment> = self
            .series
            .segments
            .iter()
            .map(|range| SeriesSegment {
                path: series_path.clone(),
                range: range.clone(),
                input_index: 0,
            })
            .collect();
        dir.replace_tables(
            |dest| {
                dest.write_all(self.runs_header.as_bytes())?;
                records
                    .iter()
                    .try_for_each(|&(run_id, record)| dest.write_all(record.renumbered(run_id).as_bytes()))
            },
            |dest| {
                merge_series(dest, &self.series_header, &segments, |_, run_id| {
                    renumbering.get(&run_id).copied()
                })
            },
        )
    }
}

/// Checks that `recorded`, the manifest of a directory to resume, holds a sweep of `plan` in `shard` with no more
/// replicates than `plan`.
fn check_recorded(recorded: &Manifest, plan: &Plan, shard: Shard) -> Result<(), ResumeError> {
    if recorded.mode != ManifestMode::Sweep {
        return Err(ResumeError::ModeChanged {
            recorded: recorded.mode,
            current: ManifestMode::Sweep,
        });
    }
    let current_plan = hex(plan.plan_hash());
    if recorded.plan.plan_hash != current_plan {
        return Err(ResumeError::PlanChanged {
            recorded: recorded.plan.plan_hash.clone(),
            current: current_plan,
        });
    }
    let current_schema = hex(plan.schema_hash());
    if recorded.model.schema_hash != current_schema {
        return Err(ResumeError::SchemaChanged {
            recorded: recorded.model.schema_hash.clone(),
            current: current_schema,
        });
    }
    if recorded.shard != ManifestShard::from(shard) {
        return Err(ResumeError::ShardChanged {
            recorded: recorded.shard,
            current: shard,
        });
    }
    if plan.replicates() < recorded.plan.replicates {
        return Err(ResumeError::ReplicatesLowered {
            recorded: recorded.plan.replicates,
            current: plan.replicates(),
        });
    }
    Ok(())
}

/// Returns the id in `plan` of the run written with id `written_id` as replicate `rep` of config `config_id`, after
/// checking that `plan` has that run with key `run_key` in `shard`.
fn current_id(
    plan: &Plan,
    shard: Shard,
    config_id: u64,
    rep: u64,
    run_key: u64,
    written_id: u64,
) -> Result<u64, ResumeError> {
    let replicates = plan.replicates();
    let run = config_id
        .checked_mul(replicates)
        .filter(|_| rep < replicates)
        .and_then(|first| first.checked_add(rep))
        .and_then(|run_id| plan.run(run_id))
        .filter(|run| plan.run_key(run) == run_key)
        .ok_or(ResumeError::UnknownRun { run_id: written_id })?;
    if !shard.contains(run.run_id) {
        return Err(ResumeError::OutsideShard {
            run_id: written_id,
            shard,
        });
    }
    Ok(run.run_id)
}

/// A directory a sweep cannot resume into.
#[derive(Debug)]
pub enum ResumeError {
    /// The manifest cannot be read, for the reason inside.
    Manifest(ManifestError),
    /// A table cannot be read back, for the reason inside.
    Table(ReadError),
    /// A directory holding another plan, each hash as 16 hexadecimal digits.
    PlanChanged { recorded: String, current: String },
    /// A directory holding runs of another model schema, each hash as 16 hexadecimal digits.
    SchemaChanged { recorded: String, current: String },
    /// A directory holding another shard of the plan.
    ShardChanged { recorded: ManifestShard, current: Shard },
    /// A table whose columns differ from those the sweep writes.
    ColumnsChanged { file: &'static str },
    /// Run `run_id` of `runs.csv`, whose config, replicate or key the plan does not have.
    UnknownRun { run_id: u64 },
    /// A directory holding `recorded` replicates per config, more than the `current` count.
    ReplicatesLowered { recorded: u64, current: u64 },
    /// Run `run_id` of `runs.csv`, outside `shard` at the plan's replicate count.
    OutsideShard { run_id: u64, shard: Shard },
    /// Run `run_id`, written twice in `runs.csv`.
    DuplicateRun { run_id: u64 },
    /// A directory holding a `recorded` kind of exploration, resumed as the `current` kind.
    ModeChanged {
        recorded: ManifestMode,
        current: ManifestMode,
    },
    /// A directory holding another search, each hash as 16 hexadecimal digits.
    SearchChanged { recorded: String, current: String },
    /// Run `run_id` of `runs.csv`, other than the run the resumed search asks for at that position.
    SearchRunChanged { run_id: u64 },
    /// A `series.csv` whose rows are not in order of their run ids.
    SeriesOutOfOrder,
}

impl fmt::Display for ResumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(_) => f.write_str("cannot read the manifest of the sweep to resume"),
            Self::Table(_) => f.write_str("cannot read the tables of the sweep to resume"),
            Self::PlanChanged { recorded, current } => write!(
                f,
                "the directory holds a different plan (plan hash {recorded}, expected {current}). Only the \
                 replicate count and the timeout can change on a resume"
            ),
            Self::SchemaChanged { recorded, current } => write!(
                f,
                "the directory holds runs of a different model version (schema hash {recorded}, expected \
                 {current})"
            ),
            Self::ShardChanged { recorded, current } => write!(
                f,
                "the directory holds shard {}/{}, expected shard {current}",
                recorded.index, recorded.count
            ),
            Self::ColumnsChanged { file } => write!(f, "{file} does not match the columns this sweep writes"),
            Self::UnknownRun { run_id } => write!(f, "run {run_id} of runs.csv is not a run of this plan"),
            Self::ReplicatesLowered { recorded, current } => write!(
                f,
                "the directory holds {recorded} replicates per config, more than this sweep's {current}. A \
                 resume can only increase the replicate count"
            ),
            Self::OutsideShard { run_id, shard } => write!(
                f,
                "run {run_id} of runs.csv falls outside shard {shard} at this replicate count. A resume of a \
                 sharded sweep cannot change the replicate count"
            ),
            Self::DuplicateRun { run_id } => write!(f, "run {run_id} is written twice in runs.csv"),
            Self::ModeChanged { recorded, current } => write!(
                f,
                "the directory holds a {}, expected a {}",
                recorded.as_str(),
                current.as_str()
            ),
            Self::SearchChanged { recorded, current } => write!(
                f,
                "the directory holds a different search (search hash {recorded}, expected {current}). Only \
                 the timeout and the execution settings can change on a resume of a search"
            ),
            Self::SearchRunChanged { run_id } => {
                write!(f, "run {run_id} of runs.csv does not match the run this search expects")
            }
            Self::SeriesOutOfOrder => f.write_str("series.csv lists its runs out of order"),
        }
    }
}

impl std::error::Error for ResumeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(error) => Some(error),
            Self::Table(error) => Some(error),
            Self::PlanChanged { .. }
            | Self::SchemaChanged { .. }
            | Self::ShardChanged { .. }
            | Self::ColumnsChanged { .. }
            | Self::UnknownRun { .. }
            | Self::ReplicatesLowered { .. }
            | Self::OutsideShard { .. }
            | Self::DuplicateRun { .. }
            | Self::ModeChanged { .. }
            | Self::SearchChanged { .. }
            | Self::SearchRunChanged { .. }
            | Self::SeriesOutOfOrder => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::plan::Shard;
    use henad_core::explore::spec::{BlockSpec, SweepSpec};

    use super::{ResumeError, current_id};
    use crate::exec::Concurrency;
    use crate::output::RUNS_FILE;
    use crate::progress::NoProgress;
    use crate::sweep::ExploreError;
    use crate::tests::support::{ScratchDir, entry, planned, sweep, sweep_options, sweep_with};

    /// Returns 2 configs of Game of Life, 8 and 12 cells wide, with `replicates` replicates of 4 steps.
    fn life_spec(replicates: u64) -> SweepSpec {
        let mut spec = SweepSpec::new("game_of_life");
        spec.fixed = vec![("grid_height".to_owned(), "8".to_owned())];
        spec.run.steps = 4;
        spec.run.replicates = replicates;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "grid_width",
                LevelSpec::Values(vec!["8".to_owned(), "12".to_owned()]),
            )],
            design_seed: None,
        }];
        spec
    }

    #[test]
    fn a_run_past_the_largest_id_is_unknown() {
        let (plan, _) = planned(&entry("game_of_life", None), None, &life_spec(3));
        let config_id = u64::MAX / 3;
        assert_eq!(
            config_id.checked_mul(3),
            Some(u64::MAX),
            "the product fits, and adding a replicate does not"
        );
        let result = current_id(&plan, Shard::WHOLE, config_id, 1, 0, 9);
        assert!(
            matches!(result, Err(ResumeError::UnknownRun { run_id: 9 })),
            "{result:?}"
        );
    }

    #[test]
    fn a_run_id_written_twice_is_refused() {
        let life = entry("game_of_life", None);
        let spec = life_spec(2);
        let scratch = ScratchDir::new("duplicate-run-id");
        sweep(&life, None, &spec, scratch.path(), Concurrency::Auto);
        let runs_path = scratch.path().join(RUNS_FILE);
        let runs = fs::read_to_string(&runs_path).expect("runs.csv is written");
        // Run 1 is replicate 1 of config 0. Written as run 0, it shares its id with replicate 0.
        let duplicated = runs.replacen("\n1,0,0,1,", "\n0,0,0,1,", 1);
        assert_ne!(duplicated, runs);
        fs::write(&runs_path, duplicated).expect("runs.csv is written again");
        let error = sweep_with(
            &life,
            None,
            &spec,
            &sweep_options(scratch.path(), true),
            &mut NoProgress,
        )
        .expect_err("a run id written twice");
        assert!(
            matches!(error, ExploreError::Resume(ResumeError::DuplicateRun { run_id: 0 })),
            "{error:?}"
        );
    }
}
