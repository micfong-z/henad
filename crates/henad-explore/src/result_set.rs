//! Results of a sweep or search read back from its files, for a host to show and replay.
//!
//! A result set reads `manifest.json` and `runs.csv`, and `series.csv` and a search's tables when there are any.
//! `summary.csv` is left unread. The runs rebuild it. Only complete records are read, so a directory a stopped sweep
//! left behind opens with the runs it wrote. Series are held run by run in file order while they fit a byte budget.
//! From the first run past it, runs are held without their series.

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::measure::SeriesBuffer;
use henad_core::explore::outcome::{PlannedRun, RunOutcome};
use henad_core::explore::plan::{Config, ModelSchema, Plan, PlanError};
use henad_core::explore::replay::Replay;
use henad_core::explore::search::SearchSpecError;
use henad_core::explore::spec::SweepSpec;

use henad_core::explore::value::parse_value;

use crate::output::manifest::{BuildRole, Manifest, ManifestError, ManifestMode, ManifestStatus, RecordedBuild};
use crate::output::read::{ReadError, parse_one, record_ends};
use crate::output::runs_csv::{ID_COLUMNS, NOTE_COLUMN, OUTCOME_COLUMNS};
use crate::output::search_tables::{
    ARCHIVE_ID_COLUMNS, EVALUATION_ID_COLUMNS, GENERATION_COLUMNS, OBJECTIVE_BATCH_COLUMNS, PATTERN_BATCH_COLUMNS,
    RANKING_ID_COLUMNS, SearchHistory,
};
use crate::output::series_csv::SERIES_ID_COLUMNS;
use crate::output::{
    ARCHIVE_FILE, BATCHES_FILE, BEST_FILE, EVALUATIONS_FILE, GENERATIONS_FILE, MANIFEST_FILE, RUNS_FILE, SERIES_FILE,
};
use crate::search_run::{SearchPlan, SearchPlanError};
use crate::spec_file::{SpecFile, SpecFileError};
use crate::sweep::hex;

/// Results of one sweep, read from an output directory or from its files' bytes.
#[derive(Debug, Clone)]
pub struct ResultSet {
    manifest: Manifest,
    spec: SweepSpec,
    /// Directory the files were read from, `None` for files handed over as bytes.
    dir: Option<PathBuf>,
    /// Names of the parameter and action columns of `runs.csv`, in order.
    value_columns: Vec<String>,
    reducer_columns: Vec<String>,
    stat_columns: Vec<String>,
    runs: Vec<RunRow>,
    /// Position in `runs` of each run, by run id.
    positions: BTreeMap<u64, usize>,
    /// Bytes the held series take.
    series_bytes: usize,
    /// Text of each search table read, by file name. Empty for a sweep.
    search_tables: BTreeMap<&'static str, String>,
}

/// Tables a search writes beside `runs.csv`, in the order a result set lists them.
const SEARCH_FILES: [&str; 5] = [
    EVALUATIONS_FILE,
    BATCHES_FILE,
    GENERATIONS_FILE,
    BEST_FILE,
    ARCHIVE_FILE,
];

/// One run as `runs.csv` records it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// The run's row, with its series when [`Self::series_held`] is set.
    ///
    /// A timing written as an empty cell reads as a value that is not finite.
    pub outcome: RunOutcome,
    /// Index of the block the run's config comes from.
    pub block: usize,
    /// Parameter values and action ticks as `runs.csv` writes them, one per [`ResultSet::value_columns`] entry.
    pub values: Vec<String>,
    /// Whether `outcome` holds the run's whole series.
    ///
    /// A run past the series budget, or read without a `series.csv`, holds none.
    pub series_held: bool,
}

impl ResultSet {
    /// Reads the output directory at `dir`, holding at most `series_budget` bytes of series.
    ///
    /// # Errors
    ///
    /// Returns [`ResultSetError`] when the manifest or `runs.csv` is missing or cannot be read, the manifest's spec
    /// cannot be read back or the search it records has a setting out of range, or a complete record of a table is
    /// not one the sweep writes.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_dir(dir: &Path, series_budget: usize) -> Result<Self, ResultSetError> {
        let manifest_path = dir.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            return Err(ResultSetError::Missing { file: MANIFEST_FILE });
        }
        let manifest = Manifest::read(&manifest_path).map_err(ResultSetError::Manifest)?;
        let (runs_path, series_path) = crate::output::table_paths(dir);
        let runs = match std::fs::read(&runs_path) {
            Ok(runs) => runs,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ResultSetError::Missing { file: RUNS_FILE });
            }
            Err(source) => {
                return Err(ResultSetError::Table(ReadError::Io {
                    path: runs_path,
                    source,
                }));
            }
        };
        let mut set = Self::from_tables(manifest, &runs, &runs_path)?;
        set.dir = Some(dir.to_owned());
        for file in SEARCH_FILES {
            let path = dir.join(file);
            match std::fs::read(&path) {
                // A table without a complete header line is one a stopped search had only begun, and reads as none,
                // as `from_files` reads it.
                Ok(bytes) if !bytes.contains(&b'\n') => {}
                Ok(bytes) => {
                    set.search_tables
                        .insert(file, String::from_utf8_lossy(&bytes).into_owned());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(ResultSetError::Table(ReadError::Io { path, source })),
            }
        }
        match std::fs::File::open(&series_path) {
            Ok(file) => set.read_series(io::BufReader::new(file), &series_path, series_budget)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => set.mark_series_missing(),
            Err(source) => {
                return Err(ResultSetError::Table(ReadError::Io {
                    path: series_path,
                    source,
                }));
            }
        }
        Ok(set)
    }

    /// Reads the files `files` of one output directory, each as its name and bytes, holding at most `series_budget`
    /// bytes of series.
    ///
    /// A file is known by its contents, so a browser's renamed download such as `runs (1).csv` still reads. The one
    /// JSON file is the manifest, and a CSV file is `runs.csv`, `series.csv` or a search table by its header. The
    /// manifest and `runs.csv` are needed, `series.csv` and the search tables are read when present, and every other
    /// file is left unread.
    ///
    /// # Errors
    ///
    /// Returns [`ResultSetError`] when the manifest or `runs.csv` is missing or cannot be read, two files hold the
    /// same table, the manifest's spec cannot be read back or the search it records has a setting out of range, or a
    /// complete record of a table is not one the sweep writes.
    pub fn from_files(files: Vec<(String, Vec<u8>)>, series_budget: usize) -> Result<Self, ResultSetError> {
        let mut tables: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
        for (name, bytes) in files {
            let Some(table) = picked_table(&name, &bytes) else {
                continue;
            };
            if tables.insert(table, bytes).is_some() {
                return Err(ResultSetError::Duplicate { file: table });
            }
        }
        let manifest_bytes = tables
            .get(MANIFEST_FILE)
            .ok_or(ResultSetError::Missing { file: MANIFEST_FILE })?;
        let manifest = Manifest::parse(&String::from_utf8_lossy(manifest_bytes), Path::new(MANIFEST_FILE))
            .map_err(ResultSetError::Manifest)?;
        let runs = tables
            .get(RUNS_FILE)
            .ok_or(ResultSetError::Missing { file: RUNS_FILE })?;
        let mut set = Self::from_tables(manifest, runs, Path::new(RUNS_FILE))?;
        match tables.get(SERIES_FILE) {
            Some(series) => set.read_series(series.as_slice(), Path::new(SERIES_FILE), series_budget)?,
            None => set.mark_series_missing(),
        }
        for file in SEARCH_FILES {
            if let Some(bytes) = tables.get(file) {
                set.search_tables
                    .insert(file, String::from_utf8_lossy(bytes).into_owned());
            }
        }
        Ok(set)
    }

    /// Returns the set of `manifest` and `runs`, the bytes of the `runs.csv` at `runs_path`, with no series read.
    fn from_tables(manifest: Manifest, runs: &[u8], runs_path: &Path) -> Result<Self, ResultSetError> {
        let spec = SpecFile::from_json(&manifest.spec)
            .and_then(SpecFile::into_spec)
            .map_err(ResultSetError::Spec)?;
        if let Some(search) = &spec.search {
            search.check().map_err(ResultSetError::Search)?;
        }
        let stat_columns = manifest.columns.stats.clone();
        let (column_names, records) = read_runs(runs, runs_path, stat_columns.len())?;
        let positions = records
            .iter()
            .enumerate()
            .map(|(position, run)| (run.outcome.run.run_id, position))
            .collect();
        Ok(Self {
            manifest,
            spec,
            dir: None,
            value_columns: column_names.value_columns,
            reducer_columns: column_names.reducer_columns,
            stat_columns,
            runs: records,
            positions,
            series_bytes: 0,
            search_tables: BTreeMap::new(),
        })
    }

    /// Reads `series`, the lines of the `series.csv` at `path`, into the runs, while the series fit `series_budget`
    /// bytes.
    ///
    /// Rows of a run that `runs.csv` does not hold are left out, and so is a partial last line. A run with any row
    /// past the budget is held without its series, even when some of its rows came before.
    ///
    /// # Errors
    ///
    /// Returns [`ResultSetError::Table`] when `series` cannot be read, its header does not name the manifest's stat
    /// columns, or a row is not one the sweep writes.
    fn read_series(
        &mut self,
        mut series: impl BufRead,
        path: &Path,
        series_budget: usize,
    ) -> Result<(), ResultSetError> {
        let width = self.stat_columns.len();
        let row_bytes = (width + 1) * size_of::<f64>();
        let mut line = Vec::new();
        if !read_line(&mut series, &mut line, path)? {
            self.mark_series_missing();
            return Ok(());
        }
        check_series_header(&line, &self.stat_columns, path)?;
        let mut budget_spent = false;
        let mut values = Vec::with_capacity(width);
        let mut record_number = 1;
        // Position of the run the last row went to. Its series is trimmed to its rows once another run's rows start.
        let mut last_position = None;
        while read_line(&mut series, &mut line, path)? {
            record_number += 1;
            let (run_id, tick) = parse_series_row(&line, width, &mut values, path, record_number)?;
            let Some(&position) = self.positions.get(&run_id) else {
                continue;
            };
            if let Some(last) = last_position.replace(position)
                && last != position
            {
                self.runs[last].outcome.series.shrink_to_fit();
            }
            let run = &mut self.runs[position];
            if !run.series_held {
                continue;
            }
            // Past the budget a run drops the rows it holds. Otherwise a run with rows on both sides of the crossing
            // keeps part of its series.
            if budget_spent || self.series_bytes + row_bytes > series_budget {
                budget_spent = true;
                self.series_bytes -= run.outcome.series.len() * row_bytes;
                run.outcome.series = SeriesBuffer::new(width);
                run.series_held = false;
                continue;
            }
            self.series_bytes += row_bytes;
            run.outcome.series.push(tick, &values);
        }
        if let Some(last) = last_position {
            self.runs[last].outcome.series.shrink_to_fit();
        }
        Ok(())
    }

    /// Marks every run as held without its series.
    fn mark_series_missing(&mut self) {
        for run in &mut self.runs {
            run.series_held = false;
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Returns every distinct build the manifest's sessions that wrote runs record for `role`, in session order, as
    /// [`Manifest::recorded_builds`] lists them.
    ///
    /// A session of a 0.2 manifest records no builds, and reads as the engine build of its own commit and the
    /// manifest's engine version, with no model build.
    pub fn recorded_builds(&self, role: BuildRole) -> Vec<RecordedBuild> {
        self.manifest.recorded_builds(role)
    }

    /// Spec the sweep ran, as its manifest records it.
    pub fn spec(&self) -> &SweepSpec {
        &self.spec
    }

    /// Directory the files were read from, `None` for files handed over as bytes.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Names of the parameter and action columns of `runs.csv`, in order.
    ///
    /// Empty when `runs.csv` has no header yet, as in a directory whose sweep has written no run.
    pub fn value_columns(&self) -> &[String] {
        &self.value_columns
    }

    /// Names of the reducer columns of `runs.csv`, one per value of a run's [`RunOutcome::reducers`].
    ///
    /// Empty when `runs.csv` has no header yet.
    pub fn reducer_columns(&self) -> &[String] {
        &self.reducer_columns
    }

    /// Names of the stat columns of `series.csv`, one per value of a series row.
    pub fn stat_columns(&self) -> &[String] {
        &self.stat_columns
    }

    /// Runs in the order `runs.csv` lists them.
    pub fn runs(&self) -> &[RunRow] {
        &self.runs
    }

    pub fn run(&self, run_id: u64) -> Option<&RunRow> {
        self.runs.get(*self.positions.get(&run_id)?)
    }

    /// Number of runs held with their series.
    pub fn held_series_count(&self) -> usize {
        self.runs.iter().filter(|run| run.series_held).count()
    }

    /// Returns whether the sweep ran every run of its shard.
    ///
    /// A directory a sweep left `running`, `aborted` or `failed`, or a merge left `incomplete`, can be resumed.
    pub fn is_complete(&self) -> bool {
        self.manifest.status == ManifestStatus::Complete
    }

    /// Returns whether `schema` declares the parameters, stats and actions the sweep ran with.
    ///
    /// A replay of a run through a model that does not match might differ from the run. A host passes
    /// `entry.schema()` for a [`ModelEntry`](henad_compute::entry::ModelEntry).
    pub fn schema_matches(&self, schema: ModelSchema<'_>) -> bool {
        schema.id == self.manifest.model.id && hex(schema_hash(&schema)) == self.manifest.model.schema_hash
    }

    /// Plans the recorded spec against `schema`.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] when the model refuses the spec.
    pub fn plan(&self, schema: ModelSchema<'_>) -> Result<Plan, PlanError> {
        self.spec.plan(&schema)
    }

    /// Returns the [`Replay`] of run `run_id` through the model `schema` declares.
    ///
    /// The spec is planned afresh on each call. A host that replays several runs of a sweep plans once with
    /// [`Self::plan`] and calls [`Plan::replay`]. A search's run is rebuilt from the config its row records.
    ///
    /// # Errors
    ///
    /// Returns [`ResultReplayError`] when the model refuses the spec, `runs.csv` holds no run `run_id`, or the plan
    /// gives the run another config, replicate, seed or run key than its row. The run key covers the config's
    /// parameter values and action ticks. It also hashes the model's declarations, so it is compared only while
    /// [`Self::schema_matches`] holds for `schema`.
    pub fn replay(&self, schema: ModelSchema<'_>, run_id: u64) -> Result<Replay, ResultReplayError> {
        let recorded = self.run(run_id).ok_or(ResultReplayError::UnknownRun { run_id })?;
        if self.is_search() {
            return self.search_replay(schema, recorded);
        }
        let plan = self.plan(schema).map_err(ResultReplayError::Plan)?;
        match plan.run(run_id) {
            Some(planned)
                if planned == recorded.outcome.run && self.key_matches(schema, plan.run_key(&planned), recorded) =>
            {
                plan.replay(run_id).ok_or(ResultReplayError::Mismatch { run_id })
            }
            _ => Err(ResultReplayError::Mismatch { run_id }),
        }
    }

    /// Returns the [`Replay`] of `recorded`, a run of a search, from the config its row records.
    ///
    /// Each value is found by its column's name, a parameter's id or an action's `action.<name>`, so a model that
    /// reorders its parameters still reads each value under its own id.
    fn search_replay(&self, schema: ModelSchema<'_>, recorded: &RunRow) -> Result<Replay, ResultReplayError> {
        let run = recorded.outcome.run;
        let mismatch = || ResultReplayError::Mismatch { run_id: run.run_id };
        let plan = SearchPlan::new(&self.spec, &schema).map_err(ResultReplayError::Search)?;
        let actions = plan.base().actions();
        if recorded.values.len() != schema.params.len() + actions.len()
            || self.value_columns.len() != recorded.values.len()
        {
            return Err(mismatch());
        }
        let value_of = |column: &str| {
            let position = self.value_columns.iter().position(|name| name == column)?;
            recorded.values.get(position)
        };
        let config = Config {
            block: recorded.block,
            params: schema
                .params
                .iter()
                .map(|descriptor| parse_value(&descriptor.kind, value_of(descriptor.id)?).ok())
                .collect::<Option<_>>()
                .ok_or_else(mismatch)?,
            action_ticks: actions
                .iter()
                .map(|action| value_of(&action.column_name())?.parse().ok())
                .collect::<Option<_>>()
                .ok_or_else(mismatch)?,
        };
        if !self.key_matches(schema, plan.run_key(&run, &config), recorded) {
            return Err(mismatch());
        }
        Ok(plan.replay(&run, &config))
    }

    /// Returns whether `key`, the key a plan through `schema` gives `recorded`, matches the key its row holds. Any key
    /// matches once `schema` no longer declares what the sweep ran with.
    fn key_matches(&self, schema: ModelSchema<'_>, key: u64, recorded: &RunRow) -> bool {
        !self.schema_matches(schema) || key == recorded.outcome.run_key
    }

    /// Returns whether the results are a search's.
    pub fn is_search(&self) -> bool {
        self.manifest.mode == ManifestMode::Search
    }

    /// Returns the text of the search table `file`, such as `evaluations.csv`, or `None` when the results hold none.
    pub fn search_table(&self, file: &str) -> Option<&str> {
        self.search_tables.get(file).map(String::as_str)
    }

    /// Returns the course of a search batch by batch, from its tables, or `None` for a sweep.
    ///
    /// # Errors
    ///
    /// Returns [`ResultSetError::Table`] when a search table cannot be read.
    pub fn search_history(&self) -> Result<Option<SearchHistory>, ResultSetError> {
        if !self.is_search() {
            return Ok(None);
        }
        SearchHistory::read(
            self.search_table(BATCHES_FILE),
            self.search_table(GENERATIONS_FILE),
            self.search_table(EVALUATIONS_FILE),
            self.spec.search.as_ref(),
        )
        .map(Some)
        .map_err(ResultSetError::Table)
    }

    /// Reads the whole series of each run of `run_ids` from the directory the set was read from, while the series
    /// fit `series_budget` bytes, as [`read_directory_series`] does.
    ///
    /// # Errors
    ///
    /// Returns [`ResultSetError::Missing`] for a set read from bytes, and [`ResultSetError::Table`] when `series.csv`
    /// cannot be read or its header does not name the stat columns.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_run_series(
        &self,
        run_ids: &BTreeSet<u64>,
        series_budget: usize,
    ) -> Result<DirectorySeries, ResultSetError> {
        let dir = self
            .dir
            .as_deref()
            .ok_or(ResultSetError::Missing { file: SERIES_FILE })?;
        let held: BTreeSet<u64> = run_ids
            .iter()
            .copied()
            .filter(|run_id| self.positions.contains_key(run_id))
            .collect();
        read_directory_series(dir, &self.stat_columns, &held, series_budget)
    }

    /// Returns the runs in the order `runs.csv` lists them, and drops the rest of the set.
    pub fn into_runs(self) -> Vec<RunRow> {
        self.runs
    }
}

/// Series of runs read from the `series.csv` of an output directory.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirectorySeries {
    /// Whole series of each run read, by run id.
    pub series: BTreeMap<u64, SeriesBuffer>,
    /// Runs left out once the series passed the budget.
    ///
    /// A run asked for and found in neither map has no rows in `series.csv`.
    pub dropped_runs: BTreeSet<u64>,
}

/// Reads the whole series of each run of `run_ids` from the `series.csv` of the output directory `dir`, whose stat
/// columns are `stat_columns`, while the series fit `series_budget` bytes.
///
/// A series counts its bytes as [`ResultSet`] does. From the first row past the budget, every run not yet read in
/// full is left out.
///
/// # Errors
///
/// Returns [`ResultSetError::Table`] when `series.csv` cannot be read, its header does not name `stat_columns`, or it
/// holds a row that is not as wide as the header.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_directory_series(
    dir: &Path,
    stat_columns: &[String],
    run_ids: &BTreeSet<u64>,
    series_budget: usize,
) -> Result<DirectorySeries, ResultSetError> {
    let (_, path) = crate::output::table_paths(dir);
    let file = std::fs::File::open(&path).map_err(|source| {
        ResultSetError::Table(ReadError::Io {
            path: path.clone(),
            source,
        })
    })?;
    let mut lines = io::BufReader::new(file);
    let mut read = DirectorySeries::default();
    let stat_width = stat_columns.len();
    let row_bytes = (stat_width + 1) * size_of::<f64>();
    let mut series_bytes = 0;
    let mut budget_spent = false;
    let mut line = Vec::new();
    let mut values = Vec::with_capacity(stat_width);
    let mut record_number = 1;
    // Run the last row went to. Its series is trimmed to its rows once another run's rows start.
    let mut last_run = None;
    if !read_line(&mut lines, &mut line, &path)? {
        return Ok(read);
    }
    check_series_header(&line, stat_columns, &path)?;
    while read_line(&mut lines, &mut line, &path)? {
        record_number += 1;
        let (run_id, tick) = parse_series_row(&line, stat_width, &mut values, &path, record_number)?;
        if !run_ids.contains(&run_id) || read.dropped_runs.contains(&run_id) {
            continue;
        }
        if let Some(last) = last_run.replace(run_id)
            && last != run_id
            && let Some(series) = read.series.get_mut(&last)
        {
            series.shrink_to_fit();
        }
        budget_spent |= series_bytes + row_bytes > series_budget;
        if budget_spent {
            if let Some(partial) = read.series.remove(&run_id) {
                series_bytes -= partial.len() * row_bytes;
            }
            read.dropped_runs.insert(run_id);
            continue;
        }
        series_bytes += row_bytes;
        read.series
            .entry(run_id)
            .or_insert_with(|| SeriesBuffer::new(stat_width))
            .push(tick, &values);
    }
    if let Some(series) = last_run.and_then(|last| read.series.get_mut(&last)) {
        series.shrink_to_fit();
    }
    Ok(read)
}

/// Returns the table of an output directory the picked file `name` holds, from its `bytes`, or `None` for any other
/// file.
fn picked_table(name: &str, bytes: &[u8]) -> Option<&'static str> {
    let extension = Path::new(name)
        .extension()
        .map(|extension| extension.to_ascii_lowercase());
    if extension.as_ref().is_some_and(|extension| extension == "json") {
        return Some(MANIFEST_FILE);
    }
    let header_end = bytes.iter().position(|&byte| byte == b'\n').unwrap_or(bytes.len());
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let header: Vec<&str> = header.trim_end_matches('\r').split(',').collect();
    if header.starts_with(&ID_COLUMNS) {
        Some(RUNS_FILE)
    } else if header.starts_with(&SERIES_ID_COLUMNS) {
        Some(SERIES_FILE)
    } else if header.starts_with(&EVALUATION_ID_COLUMNS) {
        Some(EVALUATIONS_FILE)
    } else if header == OBJECTIVE_BATCH_COLUMNS || header == PATTERN_BATCH_COLUMNS {
        Some(BATCHES_FILE)
    } else if header == GENERATION_COLUMNS {
        Some(GENERATIONS_FILE)
    } else if header.starts_with(&RANKING_ID_COLUMNS) {
        Some(BEST_FILE)
    } else if header.starts_with(&ARCHIVE_ID_COLUMNS) {
        Some(ARCHIVE_FILE)
    } else {
        None
    }
}

/// Checks that `line`, the header line of the `series.csv` at `path`, names the id columns and then `stat_columns`.
fn check_series_header(line: &[u8], stat_columns: &[String], path: &Path) -> Result<(), ResultSetError> {
    let table_error = ResultSetError::Table;
    let text = String::from_utf8_lossy(line);
    let header = parse_one(&text, 0..text.len(), path, 1).map_err(table_error)?;
    let expected = SERIES_ID_COLUMNS.len() + stat_columns.len();
    if header.len() != expected {
        return Err(table_error(ReadError::FieldCount {
            path: path.to_owned(),
            record_number: 1,
            found: header.len(),
            expected,
        }));
    }
    let (ids, stats) = header.split_at(SERIES_ID_COLUMNS.len());
    for (&column, name) in SERIES_ID_COLUMNS.iter().zip(ids) {
        if name != column {
            return Err(table_error(ReadError::MissingColumn {
                path: path.to_owned(),
                column,
            }));
        }
    }
    for (column, name) in stat_columns.iter().zip(stats) {
        if name != column {
            return Err(table_error(ReadError::BadField {
                path: path.to_owned(),
                record_number: 1,
                column: column.clone(),
                text: name.clone(),
            }));
        }
    }
    Ok(())
}

/// Reads the next complete line of `lines` into `line`, and returns whether there was one.
///
/// A last line without a line feed is partial, and reads as none.
fn read_line(lines: &mut impl BufRead, line: &mut Vec<u8>, path: &Path) -> Result<bool, ResultSetError> {
    line.clear();
    let read = lines.read_until(b'\n', line).map_err(|source| {
        ResultSetError::Table(ReadError::Io {
            path: path.to_owned(),
            source,
        })
    })?;
    Ok(read > 0 && line.ends_with(b"\n"))
}

/// Reads the run id, tick and `width` values of the series row `line` into `values`, an empty cell as a value that is
/// not finite.
///
/// Returns the run id and the tick.
fn parse_series_row(
    line: &[u8],
    width: usize,
    values: &mut Vec<f64>,
    path: &Path,
    record_number: usize,
) -> Result<(u64, u64), ResultSetError> {
    let text = String::from_utf8_lossy(line);
    let text = text.trim_end_matches(['\n', '\r']);
    let bad_field = |column: &str, field: &str| {
        ResultSetError::Table(ReadError::BadField {
            path: path.to_owned(),
            record_number,
            column: column.to_owned(),
            text: field.to_owned(),
        })
    };
    let mut fields = text.split(',');
    let mut number = |column: &str| {
        let field = fields.next().unwrap_or_default();
        field.parse::<u64>().ok().ok_or_else(|| bad_field(column, field))
    };
    let run_id = number("run_id")?;
    let tick = number("tick")?;
    values.clear();
    for field in fields {
        let value = if field.is_empty() {
            f64::NAN
        } else {
            field.parse().ok().ok_or_else(|| bad_field("value", field))?
        };
        values.push(value);
    }
    if values.len() != width {
        return Err(ResultSetError::Table(ReadError::FieldCount {
            path: path.to_owned(),
            record_number,
            found: values.len() + 2,
            expected: width + 2,
        }));
    }
    Ok((run_id, tick))
}

/// Names of the columns of a `runs.csv` that hold a config's values and a run's reducers.
struct RunsColumnNames {
    value_columns: Vec<String>,
    reducer_columns: Vec<String>,
}

/// Reads the complete records of `bytes`, a `runs.csv` read from `path`, as runs whose series are `width` wide.
///
/// A file without a complete header line holds no runs and no columns. A sweep's header reaches the file with its
/// first run.
fn read_runs(bytes: &[u8], path: &Path, width: usize) -> Result<(RunsColumnNames, Vec<RunRow>), ResultSetError> {
    let table_error = ResultSetError::Table;
    let ends = record_ends(bytes);
    let complete = ends.last().copied().unwrap_or(0);
    let text = std::str::from_utf8(&bytes[..complete]).map_err(|error| {
        table_error(ReadError::Io {
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidData, error),
        })
    })?;
    let Some((&header_end, record_ends)) = ends.split_first() else {
        let columns = RunsColumnNames {
            value_columns: Vec::new(),
            reducer_columns: Vec::new(),
        };
        return Ok((columns, Vec::new()));
    };
    let header = parse_one(text, 0..header_end, path, 1).map_err(table_error)?;
    let column_positions = RunsColumnPositions::find(&header, path).map_err(table_error)?;
    let mut records = Vec::with_capacity(record_ends.len());
    let mut start = header_end;
    for (index, &end) in record_ends.iter().enumerate() {
        let record_number = index + 2;
        let fields = parse_one(text, start..end, path, record_number).map_err(table_error)?;
        if fields.len() != header.len() {
            return Err(table_error(ReadError::FieldCount {
                path: path.to_owned(),
                record_number,
                found: fields.len(),
                expected: header.len(),
            }));
        }
        records.push(
            column_positions
                .record(&header, &fields, width, path, record_number)
                .map_err(table_error)?,
        );
        start = end;
    }
    let columns = RunsColumnNames {
        value_columns: header[column_positions.values.clone()].to_vec(),
        reducer_columns: header[column_positions.reducers.clone()].to_vec(),
    };
    Ok((columns, records))
}

/// Positions of the columns of a `runs.csv` header.
struct RunsColumnPositions {
    /// Parameter and action columns, the ones between `run_key` and `status`.
    values: std::ops::Range<usize>,
    /// Position of `status`, the first of the outcome columns.
    status: usize,
    /// Reducer columns, the ones between `steps_per_s` and `note`.
    reducers: std::ops::Range<usize>,
}

impl RunsColumnPositions {
    /// Finds the columns in `header`. The ids come first in their own order, and `status` is found from the end,
    /// since a parameter can share its name.
    fn find(header: &[String], path: &Path) -> Result<Self, ReadError> {
        let missing = |column: &'static str| ReadError::MissingColumn {
            path: path.to_owned(),
            column,
        };
        for (position, &column) in ID_COLUMNS.iter().enumerate() {
            if header.get(position).is_none_or(|name| name != column) {
                return Err(missing(column));
            }
        }
        let status = header
            .iter()
            .rposition(|name| name == OUTCOME_COLUMNS[0])
            .ok_or_else(|| missing(OUTCOME_COLUMNS[0]))?;
        for (offset, &column) in OUTCOME_COLUMNS.iter().enumerate() {
            if header.get(status + offset).is_none_or(|name| name != column) {
                return Err(missing(column));
            }
        }
        if header.last().is_none_or(|name| name != NOTE_COLUMN) {
            return Err(missing(NOTE_COLUMN));
        }
        Ok(Self {
            values: ID_COLUMNS.len()..status,
            status,
            reducers: status + OUTCOME_COLUMNS.len()..header.len() - 1,
        })
    }

    /// Returns the run of `fields`, record `record_number` of the file at `path`, with an empty series `width` wide.
    fn record(
        &self,
        header: &[String],
        fields: &[String],
        width: usize,
        path: &Path,
        record_number: usize,
    ) -> Result<RunRow, ReadError> {
        let bad_field = |column: usize| ReadError::BadField {
            path: path.to_owned(),
            record_number,
            column: header[column].clone(),
            text: fields[column].clone(),
        };
        let integer = |column: usize| fields[column].parse::<u64>().ok().ok_or_else(|| bad_field(column));
        let real = |column: usize| match fields[column].as_str() {
            "" => Ok(f64::NAN),
            text => text.parse::<f64>().ok().ok_or_else(|| bad_field(column)),
        };
        let status = self.status;
        let reducers = self
            .reducers
            .clone()
            .map(|column| match fields[column].as_str() {
                "" => Ok(None),
                text => text.parse::<f64>().map(Some).ok().ok_or_else(|| bad_field(column)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let note = &fields[fields.len() - 1];
        let outcome = RunOutcome {
            run: PlannedRun {
                run_id: integer(0)?,
                config_id: integer(1)?,
                rep: integer(3)?,
                seed: integer(4)?,
            },
            run_key: u64::from_str_radix(&fields[5], 16).ok().ok_or_else(|| bad_field(5))?,
            status: fields[status].parse().ok().ok_or_else(|| bad_field(status))?,
            stop_reason: fields[status + 1].parse().ok().ok_or_else(|| bad_field(status + 1))?,
            ticks: integer(status + 2)?,
            population: integer(status + 3)?,
            build_ms: real(status + 4)?,
            wall_ms: real(status + 5)?,
            reducers,
            series: SeriesBuffer::new(width),
            note: (!note.is_empty()).then(|| note.clone()),
        };
        Ok(RunRow {
            outcome,
            block: fields[2].parse().ok().ok_or_else(|| bad_field(2))?,
            values: fields[self.values.clone()].to_vec(),
            series_held: true,
        })
    }
}

/// Results that cannot be read.
#[derive(Debug)]
pub enum ResultSetError {
    /// The directory or the files handed over lack `file`.
    Missing { file: &'static str },
    /// Two of the files handed over hold `file`.
    Duplicate { file: &'static str },
    /// The manifest cannot be read, for the reason inside.
    Manifest(ManifestError),
    /// The manifest's spec cannot be read back, for the reason inside.
    Spec(SpecFileError),
    /// The search the manifest records has a setting [`SearchSpec::check`] refuses, for the reason inside.
    ///
    /// [`SearchSpec::check`]: henad_core::explore::search::SearchSpec::check
    Search(SearchSpecError),
    /// A table cannot be read, for the reason inside.
    Table(ReadError),
}

impl fmt::Display for ResultSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { file } => write!(f, "the results are missing {file}"),
            Self::Duplicate { file } => write!(f, "two selected files hold {file}"),
            Self::Manifest(_) => f.write_str("cannot read the manifest"),
            Self::Spec(_) => f.write_str("cannot read the spec the manifest records"),
            Self::Search(_) => f.write_str("the search the manifest records has an invalid setting"),
            Self::Table(_) => f.write_str("cannot read the results"),
        }
    }
}

impl std::error::Error for ResultSetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Missing { .. } | Self::Duplicate { .. } => None,
            Self::Manifest(error) => Some(error),
            Self::Spec(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::Table(error) => Some(error),
        }
    }
}

/// A run of a result set that cannot be replayed.
#[derive(Debug)]
pub enum ResultReplayError {
    /// The model refuses the recorded spec, for the reason inside.
    Plan(PlanError),
    /// The model refuses the recorded search, for the reason inside.
    Search(SearchPlanError),
    /// A run id `runs.csv` does not hold.
    UnknownRun { run_id: u64 },
    /// A run whose config, replicate, seed or run key differs between its row and the plan.
    Mismatch { run_id: u64 },
}

impl fmt::Display for ResultReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(_) | Self::Search(_) => f.write_str("the model refuses the spec of these results"),
            Self::UnknownRun { run_id } => write!(f, "the results hold no run {run_id}"),
            Self::Mismatch { run_id } => write!(f, "run {run_id} of the results does not match its plan"),
        }
    }
}

impl std::error::Error for ResultReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::UnknownRun { .. } | Self::Mismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::plan::ModelSchema;
    use henad_core::explore::replay::Replay;
    use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec};
    use henad_core::explore::spec::SweepSpec;
    use henad_core::params::{ParamDescriptor, ParamValue};

    use super::ResultSet;
    use crate::progress::NoProgress;
    use crate::tests::support::{ScratchDir, entry, sweep_options, sweep_with};

    /// Returns the value `replay` gives the parameter `id`, whose schema lists `params`.
    fn value_of(replay: &Replay, params: &[ParamDescriptor], id: &str) -> ParamValue {
        let position = params
            .iter()
            .position(|param| param.id == id)
            .expect("the schema has the parameter");
        replay.params[position].clone()
    }

    #[test]
    fn a_search_run_replays_each_value_under_its_parameter_id() {
        let sir = entry("sir", None);
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![
            ("grid_width".to_owned(), "8".to_owned()),
            ("grid_height".to_owned(), "8".to_owned()),
        ];
        spec.run.steps = 4;
        spec.measure.default_reducers = false;
        spec.measure.reducers = vec!["Infected:max".parse().expect("a valid reducer")];
        let rate = |min, max| LevelSpec::Range { min, max, step: None };
        spec.search = Some(SearchSpec {
            algorithm: SearchAlgorithm::Random,
            max_evaluations: 2,
            batch_size: 2,
            objective: Some(Objective {
                column: "Infected:max".to_owned(),
                goal: Goal::Minimize,
                aggregate: Aggregate::Median,
            }),
            space: vec![
                FactorSpec::param("infection_rate", rate(0.5, 0.9)),
                FactorSpec::param("recovery_rate", rate(0.02, 0.3)),
            ],
        });
        let scratch = ScratchDir::new("search-replay-by-id");
        sweep_with(
            &sir,
            None,
            &spec,
            scratch.path(),
            &sweep_options(false),
            &mut NoProgress,
        )
        .expect("the search runs");
        let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the folder reads back");
        let schema = sir.schema();
        let replay = set.replay(schema, 0).expect("the run replays");

        // A later version of the model lists its two rates the other way round.
        let mut params = schema.params.to_vec();
        let position = |id: &str| params.iter().position(|param| param.id == id).expect("a SIR parameter");
        let (infection, recovery) = (position("infection_rate"), position("recovery_rate"));
        params.swap(infection, recovery);
        let reordered = ModelSchema {
            params: &params,
            ..schema
        };
        let swapped = set.replay(reordered, 0).expect("the run replays");
        for id in ["infection_rate", "recovery_rate", "grid_width"] {
            assert_eq!(
                value_of(&swapped, &params, id),
                value_of(&replay, schema.params, id),
                "{id}"
            );
        }
    }
}
