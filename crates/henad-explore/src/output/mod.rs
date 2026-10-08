//! Output directories of a sweep, and the writer that streams each finished run into one.
//!
//! A directory holds four files. `runs.csv` has one row per run, `series.csv` the sampled stat rows of every run,
//! `summary.csv` statistics over the replicates of each config, and `manifest.json` the settings, provenance and
//! progress of the sweep. Both CSV tables list runs in order of their ids once a sweep ends. A search adds the
//! tables [`search_tables`] writes.
//!
//! A resume that drops, renumbers or reorders rows replaces `runs.csv` and `series.csv` together, and a merge writes
//! them the same way. Each is written in full beside the original with a `.staged` suffix, then a marker file says
//! both are complete, then both are renamed into place and the marker is removed. A process that ends between the
//! marker and its removal leaves the replacement for [`OutputDir::open`] to finish.
//!
//! A writer holds the operating system's advisory lock on the file [`LOCK_FILE`] while it writes, and a second writer
//! is refused. A resume takes the lock before it reads the tables and holds it until its last write. The file stays
//! in the directory when the writer ends, and the next writer locks it again. The lock goes with the process that
//! holds it, a killed one included.
//!
//! A writer creates afresh each file it replaces. It appends to `runs.csv` and `series.csv`, and cuts them, only after
//! [`OutputDir::open`] has refused either as a symbolic link.

pub mod details;
pub mod manifest;
pub mod memory;
pub mod read;
pub mod resume;
pub mod runs_csv;
pub mod search_tables;
pub mod series_csv;
pub mod summary_csv;

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::{Config, Plan};
use henad_core::params::ParamDescriptor;

use crate::exec::RunSink;
use crate::output::manifest::{Manifest, ResultCounts};
use crate::output::read::{ReadError, RunsCsv, SeriesScan, SeriesSegment, merge_series};
use crate::output::runs_csv::RunsWriter;
use crate::output::series_csv::SeriesWriter;
use crate::output::summary_csv::{SummaryError, write_summary};

pub const RUNS_FILE: &str = "runs.csv";
pub const SERIES_FILE: &str = "series.csv";
pub const SUMMARY_FILE: &str = "summary.csv";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const EVALUATIONS_FILE: &str = "evaluations.csv";
pub const BATCHES_FILE: &str = "batches.csv";
pub const BEST_FILE: &str = "best.csv";
pub const ARCHIVE_FILE: &str = "archive.csv";
pub const GENERATIONS_FILE: &str = "generations.csv";

/// Files a sweep or search writes, any one of which marks a directory as holding results.
const RESULT_FILES: [&str; 9] = [
    RUNS_FILE,
    SERIES_FILE,
    SUMMARY_FILE,
    MANIFEST_FILE,
    EVALUATIONS_FILE,
    BATCHES_FILE,
    BEST_FILE,
    ARCHIVE_FILE,
    GENERATIONS_FILE,
];

/// File a writer locks while it writes to an output directory.
pub const LOCK_FILE: &str = ".lock";

/// Suffix of a table written in full to replace the one it names.
const STAGED_SUFFIX: &str = ".staged";

/// File whose presence says both staged tables are complete.
const STAGED_MARKER: &str = "tables.staged";

/// Returns the path of the staged table that replaces `file` in `dir`.
fn staged_path(dir: &Path, file: &str) -> PathBuf {
    dir.join(format!("{file}{STAGED_SUFFIX}"))
}

/// Returns the paths of `runs.csv` and `series.csv` in the directory at `path`, as they stand.
///
/// A staged table stands in for its original while a replacement waits to be finished.
pub(crate) fn table_paths(path: &Path) -> (PathBuf, PathBuf) {
    let complete = path.join(STAGED_MARKER).exists();
    let current = |file: &str| {
        let staged = staged_path(path, file);
        if complete && staged.exists() {
            staged
        } else {
            path.join(file)
        }
    };
    (current(RUNS_FILE), current(SERIES_FILE))
}

/// A directory that holds, or is about to hold, the results of one sweep, locked against other writers.
///
/// The lock is released when the value is dropped.
#[derive(Debug)]
pub struct OutputDir {
    path: PathBuf,
    #[expect(dead_code, reason = "held for its drop, which releases the lock")]
    lock: DirLock,
}

impl OutputDir {
    /// Checks that the directory at `path` holds no results, without creating it.
    ///
    /// A symbolic link under the name of a file counts as that file, even one that points nowhere.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::HoldsResults`] when the directory holds a file a sweep or a search writes, a staged
    /// table or the marker of staged tables included.
    pub fn check_free(path: &Path) -> Result<(), OutputError> {
        let holds_results = |file: String| {
            Err(OutputError::HoldsResults {
                dir: path.to_owned(),
                file,
            })
        };
        if let Some(&file) = RESULT_FILES
            .iter()
            .find(|&&file| std::fs::symlink_metadata(path.join(file)).is_ok())
        {
            return holds_results(file.to_owned());
        }
        // A staged table can stand in for its original, as `table_paths` reads the directory.
        let staged = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name.ends_with(STAGED_SUFFIX));
        match staged {
            Some(file) => holds_results(file),
            None => Ok(()),
        }
    }

    /// Creates the directory at `path` and its parents, or takes an existing directory that holds no results, and
    /// locks it.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::HoldsResults`] when the directory holds a file a sweep or a search writes,
    /// [`OutputError::Locked`] when another writer holds its lock, and [`OutputError::Write`] when it cannot be
    /// created.
    pub fn create(path: &Path) -> Result<Self, OutputError> {
        Self::check_free(path)?;
        std::fs::create_dir_all(path).map_err(write_error_at(path))?;
        let dir = Self {
            path: path.to_owned(),
            lock: DirLock::acquire(path)?,
        };
        // Checked again under the lock. Another writer could have finished between the first check and the lock.
        Self::check_free(path)?;
        Ok(dir)
    }

    /// Returns whether the directory at `path` holds a file a sweep or a search writes.
    pub fn holds_results(path: &Path) -> bool {
        Self::check_free(path).is_err()
    }

    /// Takes and locks the existing directory at `path`, finishing a replacement of its tables that a process left
    /// behind.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Locked`] when another writer holds the directory's lock, [`OutputError::Link`] when
    /// `runs.csv` or `series.csv` is a symbolic link, and [`OutputError::Write`] when the replacement cannot be
    /// finished or discarded.
    pub fn open(path: &Path) -> Result<Self, OutputError> {
        let dir = Self {
            path: path.to_owned(),
            lock: DirLock::acquire(path)?,
        };
        dir.finish_staged()?;
        // A resume appends to both tables and cuts them, and either would follow a link.
        for file in [RUNS_FILE, SERIES_FILE] {
            let table = path.join(file);
            if std::fs::symlink_metadata(&table).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return Err(OutputError::Link { path: table });
            }
        }
        Ok(dir)
    }

    /// Checks that no writer holds the lock of the directory at `path`, without taking it.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Locked`] when another writer holds the lock.
    pub fn check_unlocked(path: &Path) -> Result<(), OutputError> {
        // A directory or a lock file that is missing is unlocked. The check never creates either.
        let Ok(file) = File::open(path.join(LOCK_FILE)) else {
            return Ok(());
        };
        match file.try_lock_shared() {
            Err(TryLockError::WouldBlock) => Err(OutputError::Locked { dir: path.to_owned() }),
            Ok(()) | Err(TryLockError::Error(_)) => Ok(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Creates `runs.csv` and `series.csv`, and returns a writer for the runs of `plan`.
    ///
    /// `params` are the model's parameters, and `measure` fixes the stat and reducer columns.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Write`] when a file exists already or cannot be created, or its header cannot be written.
    pub fn open_writer(
        &self,
        plan: &Arc<Plan>,
        params: &[ParamDescriptor],
        measure: &MeasurePlan,
    ) -> Result<OutputWriter<BufWriter<File>>, OutputError> {
        let runs_path = self.path.join(RUNS_FILE);
        let runs = File::create_new(&runs_path).map_err(write_error_at(&runs_path))?;
        let runs = RunsWriter::new(BufWriter::new(runs), params, plan.actions(), measure.reducers().names())
            .map_err(write_error_at(&runs_path))?;
        let series_path = self.path.join(SERIES_FILE);
        let series = File::create_new(&series_path).map_err(write_error_at(&series_path))?;
        let series =
            SeriesWriter::new(BufWriter::new(series), measure.columns()).map_err(write_error_at(&series_path))?;
        Ok(OutputWriter::new(Arc::clone(plan), runs, series))
    }

    /// Opens `runs.csv` and `series.csv`, whose headers are already written, and returns a writer that adds the runs
    /// of `plan` to them.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Write`] when a file cannot be opened.
    pub fn append_writer(
        &self,
        plan: &Arc<Plan>,
        params: &[ParamDescriptor],
    ) -> Result<OutputWriter<BufWriter<File>>, OutputError> {
        let append = |file: &str| {
            let path = self.path.join(file);
            OpenOptions::new()
                .append(true)
                .open(&path)
                .map(BufWriter::new)
                .map_err(write_error_at(&path))
        };
        let runs = RunsWriter::appending(append(RUNS_FILE)?, params);
        let series = SeriesWriter::appending(append(SERIES_FILE)?);
        Ok(OutputWriter::new(Arc::clone(plan), runs, series))
    }

    /// Replaces `runs.csv` and `series.csv` with the text `write_runs` and `write_series` write, as the module
    /// documentation describes.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Write`] when a write, a rename or a removal fails. The originals stay in place until
    /// both new tables are complete.
    pub fn replace_tables(
        &self,
        write_runs: impl FnOnce(&mut dyn Write) -> io::Result<()>,
        write_series: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> Result<(), OutputError> {
        self.write_staged(RUNS_FILE, write_runs)?;
        self.write_staged(SERIES_FILE, write_series)?;
        let marker = self.path.join(STAGED_MARKER);
        create_fresh(&marker)
            .and_then(|file| file.sync_all())
            .map_err(write_error_at(&marker))?;
        self.finish_staged()
    }

    /// Writes the staged table replacing `file` with `write`, and flushes it to the disk.
    fn write_staged(
        &self,
        file: &str,
        write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    ) -> Result<(), OutputError> {
        let path = staged_path(&self.path, file);
        let staged = create_fresh(&path).map_err(write_error_at(&path))?;
        let mut writer = BufWriter::new(staged);
        write(&mut writer).map_err(write_error_at(&path))?;
        let staged = writer
            .into_inner()
            .map_err(|error| write_error_at(&path)(error.into_error()))?;
        staged.sync_all().map_err(write_error_at(&path))
    }

    /// Moves both staged tables into place when the marker says they are complete, and removes staged tables left
    /// incomplete otherwise.
    fn finish_staged(&self) -> Result<(), OutputError> {
        let marker = self.path.join(STAGED_MARKER);
        let complete = marker.exists();
        for file in [RUNS_FILE, SERIES_FILE] {
            let staged = staged_path(&self.path, file);
            // A link that points nowhere is a staged table to remove as well.
            if std::fs::symlink_metadata(&staged).is_err() {
                continue;
            }
            if complete {
                let installed = self.path.join(file);
                std::fs::rename(&staged, &installed).map_err(write_error_at(&installed))?;
            } else {
                std::fs::remove_file(&staged).map_err(write_error_at(&staged))?;
            }
        }
        if complete {
            std::fs::remove_file(&marker).map_err(write_error_at(&marker))?;
        }
        Ok(())
    }

    /// Rewrites `runs.csv` and `series.csv` with their runs in order of their ids, leaving tables already in order
    /// alone.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Table`] when a table cannot be read back, and [`OutputError::Write`] when the
    /// replacement fails.
    pub fn order_tables(&self) -> Result<(), OutputError> {
        let runs_path = self.path.join(RUNS_FILE);
        let series_path = self.path.join(SERIES_FILE);
        let runs = RunsCsv::read(&runs_path).map_err(OutputError::Table)?;
        let series = SeriesScan::read(&series_path, |_| true).map_err(OutputError::Table)?;
        let in_order = runs.records.is_sorted_by_key(|record| record.run_id) && series.segments.len() <= 1;
        if in_order {
            return Ok(());
        }
        let (Some(runs_header), Some(series_header)) = (runs.header.as_ref(), series.header.as_ref()) else {
            return Ok(());
        };
        let mut records: Vec<_> = runs.records.iter().collect();
        records.sort_by_key(|record| record.run_id);
        let segments: Vec<SeriesSegment> = series
            .segments
            .iter()
            .map(|range| SeriesSegment {
                path: series_path.clone(),
                range: range.clone(),
                input_index: 0,
            })
            .collect();
        let runs_header_line = runs_csv::header_line(runs_header);
        self.replace_tables(
            |dest| {
                dest.write_all(runs_header_line.as_bytes())?;
                records
                    .iter()
                    .try_for_each(|record| dest.write_all(record.text.as_bytes()))
            },
            |dest| merge_series(dest, series_header, &segments, |_, run_id| Some(run_id)),
        )
    }

    /// Creates the table `file` afresh, removing one that exists, and returns a buffered writer over it.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Write`] when the file cannot be removed or created.
    pub fn create_table(&self, file: &str) -> Result<BufWriter<File>, OutputError> {
        let path = self.path.join(file);
        create_fresh(&path).map(BufWriter::new).map_err(write_error_at(&path))
    }

    /// Writes `manifest` to `manifest.json`. An earlier manifest is replaced by a single rename, so a reader sees
    /// one or the other whole.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Manifest`] when the manifest cannot be serialized, and [`OutputError::Write`] when a
    /// write or the rename fails.
    pub fn write_manifest(&self, manifest: &Manifest) -> Result<(), OutputError> {
        let text = manifest_text(manifest)?;
        let partial = self.path.join(format!("{MANIFEST_FILE}.partial"));
        // The file closes at the end of the block, before the rename.
        {
            let mut file = create_fresh(&partial).map_err(write_error_at(&partial))?;
            file.write_all(text.as_bytes()).map_err(write_error_at(&partial))?;
            file.sync_all().map_err(write_error_at(&partial))?;
        }
        let manifest_path = self.path.join(MANIFEST_FILE);
        std::fs::rename(&partial, &manifest_path).map_err(write_error_at(&manifest_path))
    }

    /// Rebuilds `summary.csv` from `runs.csv`.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Read`] when `runs.csv` cannot be read, [`OutputError::Write`] when `summary.csv`
    /// cannot be written, and [`OutputError::Summary`] when `runs.csv` cannot be summarized.
    pub fn write_summary(&self) -> Result<(), OutputError> {
        let runs_path = self.path.join(RUNS_FILE);
        let read_error = |source| OutputError::Read {
            path: runs_path.clone(),
            source,
        };
        let runs = File::open(&runs_path).map_err(read_error)?;
        let summary_path = self.path.join(SUMMARY_FILE);
        let summary = create_fresh(&summary_path).map_err(write_error_at(&summary_path))?;
        match write_summary(BufReader::new(runs), BufWriter::new(summary)) {
            Ok(_) => Ok(()),
            Err(SummaryError::Read(source)) => Err(read_error(source)),
            Err(SummaryError::Io(source)) => Err(write_error_at(&summary_path)(source)),
            Err(error) => Err(OutputError::Summary(error)),
        }
    }
}

/// Returns `manifest` as the text of `manifest.json`, pretty-printed JSON ending in a line feed.
///
/// # Errors
///
/// Returns [`OutputError::Manifest`] when the manifest cannot be serialized.
pub(crate) fn manifest_text(manifest: &Manifest) -> Result<String, OutputError> {
    let mut text = serde_json::to_string_pretty(manifest).map_err(OutputError::Manifest)?;
    text.push('\n');
    Ok(text)
}

/// Returns a function that turns an I/O error on `path` into an [`OutputError::Write`].
fn write_error_at(path: &Path) -> impl FnOnce(io::Error) -> OutputError + use<> {
    let path = path.to_owned();
    move |source| OutputError::Write { path, source }
}

/// Creates the file at `path` afresh. A file or a symbolic link already there is removed first, never followed.
///
/// # Errors
///
/// Returns the error of the removal or the creation.
fn create_fresh(path: &Path) -> io::Result<File> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    File::create_new(path)
}

/// Advisory lock a writer holds on [`LOCK_FILE`] of an output directory, released when dropped.
///
/// Note that the file stays in the directory. Were it removed, a writer that had it open could lock the removed file
/// while another writer locks a new one.
#[derive(Debug)]
struct DirLock {
    /// File the lock is held on.
    file: File,
}

impl DirLock {
    /// Locks the directory at `dir`, creating its lock file when missing.
    ///
    /// Note that a filesystem without file locks, as some network filesystems are, leaves the directory unguarded.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Locked`] when another writer holds the lock, and [`OutputError::Write`] when the lock
    /// file cannot be opened.
    fn acquire(dir: &Path) -> Result<Self, OutputError> {
        let path = dir.join(LOCK_FILE);
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            std::fs::remove_file(&path).map_err(write_error_at(&path))?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(write_error_at(&path))?;
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => Err(OutputError::Locked { dir: dir.to_owned() }),
            Ok(()) | Err(TryLockError::Error(_)) => Ok(Self { file }),
        }
    }
}

impl Drop for DirLock {
    /// Releases the lock before the file closes.
    fn drop(&mut self) {
        drop(self.file.unlock());
    }
}

/// Sink that streams each committed run to `runs.csv` and `series.csv`, and counts the rows by status.
///
/// A run's series is written and flushed before its row in `runs.csv`, so every run in `runs.csv` has its whole
/// series written.
#[derive(Debug)]
pub struct OutputWriter<W: Write> {
    plan: Arc<Plan>,
    runs: RunsWriter<W>,
    series: SeriesWriter<W>,
    counts: ResultCounts,
}

impl<W: Write> OutputWriter<W> {
    /// Returns a writer for the runs of `plan` over writers whose headers are written.
    pub fn new(plan: Arc<Plan>, runs: RunsWriter<W>, series: SeriesWriter<W>) -> Self {
        Self {
            plan,
            runs,
            series,
            counts: ResultCounts::default(),
        }
    }

    /// Writes the series of `outcome`, then its row, flushing each.
    ///
    /// # Errors
    ///
    /// Returns the error of a write or a flush.
    ///
    /// # Panics
    ///
    /// Panics when the run's config is not in the plan.
    pub fn write_run(&mut self, outcome: &RunOutcome) -> io::Result<()> {
        let Self {
            plan,
            runs,
            series,
            counts,
        } = self;
        let config = plan
            .config(outcome.run.config_id)
            .expect("a committed run's config is in its plan");
        write_both(runs, series, counts, outcome, config)
    }

    /// Writes the series of `outcome`, a run of `config`, then its row, flushing each.
    ///
    /// A search writes its runs this way. Its configs are in no plan.
    ///
    /// # Errors
    ///
    /// Returns the error of a write or a flush.
    pub fn write_config_run(&mut self, outcome: &RunOutcome, config: &Config) -> io::Result<()> {
        write_both(&mut self.runs, &mut self.series, &mut self.counts, outcome, config)
    }

    /// Counts of the rows written so far.
    pub fn counts(&self) -> ResultCounts {
        self.counts
    }

    /// Flushes both writers, and hands them back as the writers of `runs.csv` and `series.csv`.
    ///
    /// # Errors
    ///
    /// Returns the error of a flush.
    pub fn finish(self) -> io::Result<(W, W)> {
        let series = self.series.into_inner()?;
        let runs = self.runs.into_inner()?;
        Ok((runs, series))
    }
}

/// Writes the series of `outcome`, a run of `config`, to `series`, then its row to `runs`, flushing each, and counts
/// the row in `counts`.
fn write_both<W: Write>(
    runs: &mut RunsWriter<W>,
    series: &mut SeriesWriter<W>,
    counts: &mut ResultCounts,
    outcome: &RunOutcome,
    config: &Config,
) -> io::Result<()> {
    series.write_run(outcome.run.run_id, &outcome.series)?;
    series.flush()?;
    runs.write_run(outcome, config)?;
    runs.flush()?;
    counts.count(outcome.status);
    Ok(())
}

impl<W: Write> RunSink for OutputWriter<W> {
    fn commit(&mut self, outcome: RunOutcome) -> io::Result<()> {
        self.write_run(&outcome)
    }
}

/// Results that cannot be written.
#[derive(Debug)]
pub enum OutputError {
    /// Directory `dir` holds `file` from an earlier sweep or search.
    HoldsResults { dir: PathBuf, file: String },
    /// Another sweep, search or merge holds the lock of directory `dir` and writes to it.
    Locked { dir: PathBuf },
    /// The table at `path` is a symbolic link. A writer never follows one.
    Link { path: PathBuf },
    /// Reading `path` failed.
    Read { path: PathBuf, source: io::Error },
    /// Creating or writing `path` failed.
    Write { path: PathBuf, source: io::Error },
    /// A table that cannot be read back, for the reason inside.
    Table(ReadError),
    /// `runs.csv` cannot be summarized, for the reason inside.
    Summary(SummaryError),
    /// Serializing the manifest failed.
    Manifest(serde_json::Error),
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HoldsResults { dir, file } => {
                write!(f, "'{}' already holds results ({file})", dir.display())
            }
            Self::Locked { dir } => write!(
                f,
                "another sweep, search or merge is writing to '{}'. Wait for it to end, or stop it",
                dir.display()
            ),
            Self::Link { path } => write!(
                f,
                "'{}' is a symbolic link. A sweep writes to no table through a link",
                path.display()
            ),
            Self::Read { path, .. } => write!(f, "cannot read '{}'", path.display()),
            Self::Write { path, .. } => write!(f, "cannot create or write '{}'", path.display()),
            Self::Table(_) => f.write_str("cannot read a table back"),
            Self::Summary(_) => f.write_str("cannot summarize the runs"),
            Self::Manifest(_) => f.write_str("cannot serialize the manifest"),
        }
    }
}

impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HoldsResults { .. } | Self::Locked { .. } | Self::Link { .. } => None,
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Table(error) => Some(error),
            Self::Summary(error) => Some(error),
            Self::Manifest(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        LOCK_FILE, MANIFEST_FILE, OutputDir, OutputError, RUNS_FILE, SERIES_FILE, STAGED_MARKER, staged_path,
        table_paths,
    };
    use crate::tests::support::ScratchDir;

    #[test]
    fn a_replacement_cut_short_is_finished_once_both_tables_are_staged() {
        let scratch = ScratchDir::new("staged-tables");
        drop(OutputDir::create(scratch.path()).expect("a scratch directory"));
        let path = scratch.path();
        for file in [RUNS_FILE, SERIES_FILE] {
            fs::write(path.join(file), "old\n").expect("a table writes");
            fs::write(staged_path(path, file), "new\n").expect("a staged table writes");
        }
        assert_eq!(table_paths(path).0, path.join(RUNS_FILE), "no marker, no replacement");
        drop(OutputDir::open(path).expect("the directory opens"));
        for file in [RUNS_FILE, SERIES_FILE] {
            assert_eq!(fs::read_to_string(path.join(file)).expect("a table reads"), "old\n");
            assert!(
                !staged_path(path, file).exists(),
                "a staged table without the marker is dropped"
            );
        }

        // The process ended after moving runs.csv into place and before series.csv.
        fs::write(staged_path(path, SERIES_FILE), "new\n").expect("a staged table writes");
        fs::write(path.join(RUNS_FILE), "new\n").expect("a table writes");
        fs::write(path.join(STAGED_MARKER), "").expect("the marker writes");
        assert_eq!(
            table_paths(path),
            (path.join(RUNS_FILE), staged_path(path, SERIES_FILE))
        );
        drop(OutputDir::open(path).expect("the directory opens"));
        for file in [RUNS_FILE, SERIES_FILE] {
            assert_eq!(fs::read_to_string(path.join(file)).expect("a table reads"), "new\n");
        }
        assert!(!path.join(STAGED_MARKER).exists());
    }

    #[test]
    fn a_second_writer_is_refused_while_the_first_holds_the_lock() {
        let scratch = ScratchDir::new("locked-directory");
        let path = scratch.path();
        let first = OutputDir::create(path).expect("a scratch directory");
        assert!(path.join(LOCK_FILE).exists(), "the writer holds the lock file");
        for second in [OutputDir::open(path), OutputDir::create(path)] {
            assert!(matches!(second, Err(OutputError::Locked { .. })), "{second:?}");
        }
        assert!(matches!(
            OutputDir::check_unlocked(path),
            Err(OutputError::Locked { .. })
        ));
        drop(first);
        assert!(path.join(LOCK_FILE).exists(), "the writer leaves its lock file");
        assert!(OutputDir::check_unlocked(path).is_ok());
        let next = OutputDir::open(path).expect("the next writer locks the file again");
        assert!(matches!(OutputDir::open(path), Err(OutputError::Locked { .. })));
        drop(next);

        // A killed process leaves its lock file behind, and the lock goes with the process.
        fs::write(path.join(LOCK_FILE), "").expect("a stale lock file writes");
        assert!(OutputDir::check_unlocked(path).is_ok());
        drop(OutputDir::open(path).expect("a stale lock file is taken over"));
    }

    #[cfg(unix)]
    #[test]
    fn a_writer_follows_no_link_in_its_directory() {
        use std::os::unix::fs::symlink;

        use henad_core::explore::spec::SweepSpec;

        use super::SUMMARY_FILE;
        use crate::exec::Concurrency;
        use crate::tests::support::{entry, sweep};

        let scratch = ScratchDir::new("planted-links");
        fs::create_dir_all(scratch.path()).expect("a scratch directory");
        let victim = scratch.path().join("victim.txt");
        fs::write(&victim, "kept\n").expect("a scratch file");
        let path = scratch.path().join("out");
        fs::create_dir(&path).expect("a scratch directory");

        symlink(&victim, path.join(format!("{MANIFEST_FILE}.partial"))).expect("a link");
        symlink(&victim, path.join(LOCK_FILE)).expect("a link");
        let mut spec = SweepSpec::new("game_of_life");
        spec.fixed = vec![
            ("grid_width".to_owned(), "8".to_owned()),
            ("grid_height".to_owned(), "8".to_owned()),
        ];
        spec.run.steps = 2;
        sweep(&entry("game_of_life", None), None, &spec, &path, Concurrency::Auto);
        assert_eq!(fs::read_to_string(&victim).expect("the victim reads"), "kept\n");
        let manifest = fs::symlink_metadata(path.join(MANIFEST_FILE)).expect("the manifest is written");
        assert!(manifest.file_type().is_file());

        let dangling = scratch.path().join("dangling");
        fs::create_dir(&dangling).expect("a scratch directory");
        symlink(scratch.path().join("nowhere.csv"), dangling.join(SUMMARY_FILE)).expect("a link");
        let refused = OutputDir::create(&dangling);
        assert!(
            matches!(&refused, Err(OutputError::HoldsResults { file, .. }) if file == SUMMARY_FILE),
            "{refused:?}"
        );

        let linked = scratch.path().join("linked");
        fs::create_dir(&linked).expect("a scratch directory");
        symlink(&victim, linked.join(RUNS_FILE)).expect("a link");
        let refused = OutputDir::open(&linked);
        assert!(matches!(refused, Err(OutputError::Link { .. })), "{refused:?}");
        assert_eq!(fs::read_to_string(&victim).expect("the victim reads"), "kept\n");
    }

    #[test]
    fn a_directory_holding_results_is_refused() {
        let scratch = ScratchDir::new("holding-results");
        let nested = scratch.path().join("a").join("b");
        let dir = OutputDir::create(&nested).expect("a missing directory is created");
        assert!(dir.path().is_dir());
        drop(dir);
        assert!(OutputDir::create(&nested).is_ok(), "an empty directory is taken");

        fs::write(nested.join("notes.txt"), "kept").expect("a scratch file");
        assert!(
            OutputDir::create(&nested).is_ok(),
            "a file a sweep does not write is left alone"
        );

        fs::write(nested.join(MANIFEST_FILE), "{}").expect("a scratch file");
        let error = OutputDir::create(&nested).expect_err("a manifest marks results");
        assert!(
            matches!(&error, OutputError::HoldsResults { file, .. } if file == MANIFEST_FILE),
            "{error:?}"
        );
        fs::remove_file(nested.join(MANIFEST_FILE)).expect("the manifest is removed");
        fs::write(nested.join(RUNS_FILE), "").expect("a scratch file");
        assert!(OutputDir::check_free(&nested).is_err(), "so does an empty runs.csv");
        fs::remove_file(nested.join(RUNS_FILE)).expect("the table is removed");

        for staged in [staged_path(&nested, SERIES_FILE), nested.join(STAGED_MARKER)] {
            fs::write(&staged, "").expect("a scratch file");
            let error = OutputDir::create(&nested).expect_err("a staged file marks results");
            let name = staged.file_name().map(|name| name.to_string_lossy().into_owned());
            assert!(
                matches!(&error, OutputError::HoldsResults { file, .. } if Some(file) == name.as_ref()),
                "{error:?}"
            );
            fs::remove_file(&staged).expect("the staged file is removed");
        }
        assert!(OutputDir::check_free(&nested).is_ok());
    }
}
