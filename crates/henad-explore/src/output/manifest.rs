//! `manifest.json`, the settings, provenance and progress of a sweep.
//!
//! A sweep writes the manifest with status `running` before its first run, and replaces it once the runs end.
//! Timestamps are RFC 3339 in UTC.

use std::fmt;
use std::io;
use std::ops::Add;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use web_time::{SystemTime, UNIX_EPOCH};

use henad_compute::runtime_info::{HostInfo, RuntimeInfo};
use henad_core::explore::outcome::RunStatus;
use henad_core::explore::plan::Shard;
use henad_core::explore::search::pse::PatternSpaceSettings;

/// Value of the `format` field, naming the kind of file.
pub const FORMAT: &str = "henad-explore";

/// Version of the manifest's layout.
pub const FORMAT_VERSION: u64 = 1;

/// Record of one sweep or search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always [`FORMAT`].
    pub format: String,
    /// Always [`FORMAT_VERSION`] for a manifest this build writes.
    pub format_version: u64,
    pub mode: ManifestMode,
    pub status: ManifestStatus,
    pub engine: ManifestEngine,
    pub model: ManifestModel,
    /// Spec the sweep ran, resolved, in the form of a spec file.
    pub spec: Value,
    pub spec_source: ManifestSpecSource,
    /// Command line of the host binary.
    pub argv: Vec<String>,
    pub plan: ManifestPlan,
    pub seeds: ManifestSeeds,
    pub columns: ManifestColumns,
    pub shard: ManifestShard,
    pub execution: ManifestExecution,
    pub runtime: ManifestRuntime,
    pub timestamps: ManifestTimestamps,
    /// One entry per session that wrote runs into the directory.
    pub sessions: Vec<ManifestSession>,
    /// Counts of the rows in `runs.csv`, `None` while the sweep is running.
    pub results: Option<ResultCounts>,
    /// Directories whose shards were merged into this one, `None` for a sweep that ran here.
    pub merged_shards: Option<Vec<String>>,
    /// Search the directory holds, `None` for a sweep.
    #[serde(default)]
    pub search: Option<ManifestSearch>,
}

impl Manifest {
    /// Reads the manifest at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the file cannot be read, is not a manifest, or has another format or version.
    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text, path)
    }

    /// Reads a manifest from `text`, the contents of the file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the text is not a manifest, or has another format or version.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_str(text).map_err(|source| ManifestError::Parse {
            path: path.to_owned(),
            source,
        })?;
        if manifest.format != FORMAT || manifest.format_version != FORMAT_VERSION {
            return Err(ManifestError::Format {
                path: path.to_owned(),
                format: manifest.format,
                format_version: manifest.format_version,
            });
        }
        Ok(manifest)
    }

    /// Marks the sweep as ended with `status` at `finished_unix_ms`, and records `results`.
    ///
    /// The latest session is credited with every row in `results` it did not skip.
    pub fn finish(&mut self, status: ManifestStatus, results: ResultCounts, finished_unix_ms: u64) {
        self.status = status;
        self.results = Some(results);
        self.timestamps.finished_unix_ms = Some(finished_unix_ms);
        self.timestamps.finished = Some(rfc3339(finished_unix_ms));
        if let Some(session) = self.sessions.last_mut() {
            session.ran = results.rows.saturating_sub(session.skipped);
        }
    }

    /// Marks the sweep as failed at `finished_unix_ms`, leaving the counts unknown.
    pub fn fail(&mut self, finished_unix_ms: u64) {
        self.status = ManifestStatus::Failed;
        self.results = None;
        self.timestamps.finished_unix_ms = Some(finished_unix_ms);
        self.timestamps.finished = Some(rfc3339(finished_unix_ms));
    }
}

/// A manifest that cannot be read.
#[derive(Debug)]
pub enum ManifestError {
    /// Reading `path` failed.
    Read { path: PathBuf, source: io::Error },
    /// The file at `path` is not a manifest, for the reason in `source`.
    Parse { path: PathBuf, source: serde_json::Error },
    /// A manifest of another kind or version than this build writes.
    Format {
        path: PathBuf,
        format: String,
        format_version: u64,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, .. } => write!(f, "cannot read '{}'", path.display()),
            Self::Parse { path, .. } => write!(f, "'{}' is not a sweep manifest", path.display()),
            Self::Format {
                path,
                format,
                format_version,
            } => write!(
                f,
                "'{}' has format '{format}' version {format_version}, expected '{FORMAT}' version \
                 {FORMAT_VERSION}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Format { .. } => None,
        }
    }
}

/// Kind of exploration a directory holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestMode {
    Sweep,
    Search,
}

impl ManifestMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sweep => "sweep",
            Self::Search => "search",
        }
    }
}

/// Search a directory holds, and its standing when the manifest was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestSearch {
    /// `random`, `hill_climb`, `genetic` or `pse`.
    pub algorithm: String,
    /// Hash of every setting that fixes the search's trajectory, from
    /// [`search_hash`](henad_core::explore::fingerprint::search_hash), as 16 hexadecimal digits.
    pub search_hash: String,
    pub max_evaluations: u64,
    pub batch_size: usize,
    /// Seed the searcher draws from, derived from the root seed.
    pub search_seed: u64,
    /// Reducer columns each run reports to the searcher, the objective's alone or the x axis's then the y axis's.
    pub watched_columns: Vec<String>,
    /// Evaluations told to the searcher so far.
    pub evaluations: u64,
    /// Number of batches those evaluations came in.
    pub batch_count: u64,
    /// Best candidate so far, `None` for a Pattern Space Exploration or before the first batch.
    pub best_candidate_id: Option<u64>,
    /// Objective of the best candidate, `None` when it has no finite value.
    pub best_objective: Option<f64>,
    /// Cells of a Pattern Space Exploration's grid that its archive fills, `None` for any other search.
    pub filled_cells: Option<u64>,
    /// Range of each axis of a Pattern Space Exploration's grid, an automatic range once the initial samples fixed
    /// it. `None` for any other search, or while an automatic range waits for the initial samples.
    #[serde(default)]
    pub axis_ranges: Option<ManifestAxisRanges>,
}

/// Range of each axis of a Pattern Space Exploration's grid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ManifestAxisRanges {
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
}

impl ManifestAxisRanges {
    /// Returns the range of each axis of `settings`, or `None` while an axis lacks its range.
    pub fn of(settings: &PatternSpaceSettings) -> Option<Self> {
        let ((x_min, x_max), (y_min, y_max)) = settings.x_axis.range().zip(settings.y_axis.range())?;
        Some(Self {
            x_min,
            x_max,
            y_min,
            y_max,
        })
    }
}

/// Progress of a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestStatus {
    /// The sweep has runs left, or its process ended without replacing the manifest.
    Running,
    /// Every run of the directory's shard is in `runs.csv`.
    Complete,
    /// The sweep was stopped before its last run. A resume runs the rest.
    Aborted,
    /// The sweep ended on an error of its own, outside any run. A resume runs the rest.
    Failed,
    /// Some planned runs are missing, as from shards left out of a merge. A resume runs them.
    Incomplete,
}

/// Build of Henad that ran the sweep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEngine {
    pub name: String,
    pub version: String,
    /// Short hash of the commit, empty when the build did not know it.
    pub commit: String,
    pub commit_date: String,
    pub debug_build: bool,
}

/// Model a sweep ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestModel {
    pub id: String,
    pub name: String,
    /// `cpu` or `gpu`.
    pub backend: String,
    /// Hash of the model's declarations, as 16 hexadecimal digits.
    pub schema_hash: String,
    /// Parameters, stats and actions, as `--params --json` writes them.
    pub schema: Value,
}

/// Spec file a sweep was read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSpecSource {
    /// Path as given, `None` for a spec built from flags.
    pub path: Option<String>,
    /// Text of the spec file as written.
    pub toml: Option<String>,
    /// Design tables the spec reads, each with its hash.
    pub tables: Vec<ManifestDesignTable>,
}

/// Design table a spec reads, as a path and the hash of its contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDesignTable {
    pub path: String,
    /// FNV-1a hash of the file, as 16 hexadecimal digits.
    pub fnv1a64: String,
}

/// Configs and runs of a sweep's plan, or the runs of a search's budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPlan {
    /// Hash of every setting that fixes the configs and their seeds, as 16 hexadecimal digits.
    ///
    /// A search records the hash of its fixed values and actions. [`ManifestSearch::search_hash`] covers the rest.
    pub plan_hash: String,
    /// Hash of every setting that shapes the results of one run, as 16 hexadecimal digits.
    pub results_fingerprint: String,
    /// Configs of a sweep's plan, `None` for a search, whose budget [`ManifestSearch::max_evaluations`] gives.
    pub configs: Option<u64>,
    pub replicates: u64,
    pub runs: u64,
    /// Blocks of the plan, none for a search.
    pub blocks: Vec<ManifestBlock>,
}

/// Design and config count of one block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestBlock {
    pub design: String,
    pub configs: u64,
    /// Seed of a design that draws its configs, `None` for one that lists them.
    pub design_seed: Option<u64>,
}

/// Root seed and the rule deriving each run's seed from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSeeds {
    pub root: u64,
    pub scheme: String,
    pub formula: String,
}

/// Names of the stat and reducer columns, before CSV escaping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestColumns {
    pub stats: Vec<String>,
    pub reducers: Vec<String>,
}

/// Share of the plan a directory holds, the runs whose id leaves remainder `index` when divided by `count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestShard {
    pub index: u64,
    pub count: u64,
}

impl From<Shard> for ManifestShard {
    fn from(shard: Shard) -> Self {
        Self {
            index: shard.index(),
            count: shard.count(),
        }
    }
}

impl ManifestShard {
    /// Returns the shard the record names, or `None` for a record no shard can have.
    pub fn to_shard(self) -> Option<Shard> {
        Shard::new(self.index, self.count).ok()
    }
}

/// Lanes, tracks and budgets a sweep ran with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestExecution {
    /// `cpu` or `gpu`.
    pub backend: String,
    /// `auto`, or the count of lanes or tracks asked for.
    pub concurrency: String,
    pub cpu_lanes: usize,
    pub threads_per_lane: usize,
    pub gpu_tracks: usize,
    /// Bytes the live runs were projected to hold together.
    pub projected_bytes: u64,
    /// Bytes of host memory the live runs could hold together, `None` for no limit.
    pub memory_budget: Option<u64>,
    /// Bytes of device memory the live GPU runs could hold together, `None` for the device's largest buffer.
    pub gpu_memory_budget: Option<u64>,
}

/// Host and device a sweep ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRuntime {
    pub os: String,
    pub arch: String,
    pub logical_cpus: Option<usize>,
    /// Width of rayon's global pool.
    pub worker_threads: Option<usize>,
    /// Name of the GPU adapter, `None` when the sweep had no device.
    pub adapter: Option<String>,
    pub adapter_backend: Option<String>,
    pub adapter_type: Option<String>,
    pub driver: Option<String>,
    /// Limits the device was created with.
    pub limits: Option<ManifestLimits>,
}

impl ManifestRuntime {
    /// Returns the record of `runtime`, or of this host alone when there is no device.
    pub fn new(runtime: Option<&RuntimeInfo>) -> Self {
        let collected;
        let host = if let Some(runtime) = runtime {
            &runtime.host
        } else {
            collected = HostInfo::collect();
            &collected
        };
        Self {
            os: host.os.to_owned(),
            arch: host.arch.to_owned(),
            logical_cpus: host.logical_cpus,
            worker_threads: host.worker_threads,
            adapter: runtime.map(|runtime| runtime.adapter.name.clone()),
            adapter_backend: runtime.map(|runtime| runtime.adapter.backend.to_string()),
            adapter_type: runtime.map(|runtime| format!("{:?}", runtime.adapter.device_type)),
            driver: runtime.map(|runtime| runtime.adapter.driver.clone()),
            limits: runtime.map(|runtime| ManifestLimits {
                max_buffer_size: runtime.granted.max_buffer_size,
                max_storage_buffer_binding_size: runtime.granted.max_storage_buffer_binding_size,
                max_texture_dimension_2d: runtime.granted.max_texture_dimension_2d,
            }),
        }
    }
}

/// Device limits that bound the size of a GPU run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestLimits {
    pub max_buffer_size: u64,
    pub max_storage_buffer_binding_size: u64,
    pub max_texture_dimension_2d: u32,
}

/// Start and end of a sweep, each in milliseconds since the Unix epoch and as RFC 3339 text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestTimestamps {
    pub started_unix_ms: u64,
    pub started: String,
    pub finished_unix_ms: Option<u64>,
    pub finished: Option<String>,
}

impl ManifestTimestamps {
    /// Returns the timestamps of a sweep started at `started_unix_ms` and still running.
    pub fn started_at(started_unix_ms: u64) -> Self {
        Self {
            started_unix_ms,
            started: rfc3339(started_unix_ms),
            finished_unix_ms: None,
            finished: None,
        }
    }
}

/// One process that wrote runs into the directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSession {
    /// Start of the session, as RFC 3339 text.
    pub started: String,
    /// Commit of the build that ran the session.
    pub commit: String,
    /// Runs the session found written and kept.
    pub skipped: u64,
    /// Runs the session wrote.
    pub ran: u64,
}

/// Counts of the rows of `runs.csv` by status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultCounts {
    pub rows: u64,
    pub ok: u64,
    pub non_finite: u64,
    /// Runs that ended on a fault or a timeout.
    pub failed: u64,
}

impl ResultCounts {
    /// Counts one row with `status`.
    pub fn count(&mut self, status: RunStatus) {
        self.rows += 1;
        match status {
            RunStatus::Ok => self.ok += 1,
            RunStatus::NonFinite => self.non_finite += 1,
            RunStatus::Panicked
            | RunStatus::GpuError
            | RunStatus::Refused
            | RunStatus::ShapeError
            | RunStatus::TimedOut => {
                self.failed += 1;
            }
        }
    }
}

impl Add for ResultCounts {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            rows: self.rows + other.rows,
            ok: self.ok + other.ok,
            non_finite: self.non_finite + other.non_finite,
            failed: self.failed + other.failed,
        }
    }
}

/// Returns the system clock in milliseconds since the Unix epoch. A clock set before the epoch reads as 0.
pub fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since_epoch| {
        u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
    })
}

/// Returns `unix_ms` as an RFC 3339 timestamp in UTC with milliseconds, as in `2026-09-27T08:30:00.250Z`.
pub fn rfc3339(unix_ms: u64) -> String {
    let seconds = unix_ms / 1000;
    let (year, month, day) = civil_date(seconds / 86_400);
    let second_of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60,
        unix_ms % 1000
    )
}

/// Returns the year, month and day of the date `days` after 1970-01-01 in the proleptic Gregorian calendar.
///
/// The algorithm is Howard Hinnant's `civil_from_days`, over eras of 400 years that start on 1 March.
fn civil_date(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use henad_core::explore::outcome::RunStatus;
    use henad_core::explore::spec::SweepSpec;
    use serde_json::Value;

    use super::{Manifest, ResultCounts, rfc3339};
    use crate::exec::Concurrency;
    use crate::output::MANIFEST_FILE;
    use crate::tests::support::{ScratchDir, entry, sweep};

    #[test]
    fn a_sweep_manifest_writes_its_search_as_null() {
        let scratch = ScratchDir::new("sweep-manifest");
        let mut spec = SweepSpec::new("game_of_life");
        spec.fixed = vec![
            ("grid_width".to_owned(), "8".to_owned()),
            ("grid_height".to_owned(), "8".to_owned()),
        ];
        spec.run.steps = 2;
        sweep(
            &entry("game_of_life", None),
            None,
            &spec,
            scratch.path(),
            Concurrency::Auto,
        );
        let text = std::fs::read_to_string(scratch.path().join(MANIFEST_FILE)).expect("the manifest is written");
        let mut json: Value = serde_json::from_str(&text).expect("the manifest is JSON");
        assert_eq!(json.get("search"), Some(&Value::Null));
        assert_eq!(json.get("merged_shards"), Some(&Value::Null));
        let manifest = Manifest::parse(&text, Path::new(MANIFEST_FILE)).expect("the manifest reads back");
        assert_eq!(manifest.search, None);

        json.as_object_mut().expect("a JSON object").remove("search");
        let without_key: Manifest = serde_json::from_value(json).expect("a manifest without the key reads");
        assert_eq!(without_key, manifest);
    }

    #[test]
    fn timestamps_are_written_in_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z", "a leap day");
        assert_eq!(rfc3339(951_868_799_999), "2000-02-29T23:59:59.999Z");
        assert_eq!(rfc3339(1_798_704_000_000), "2026-12-31T08:00:00.000Z");
        assert_eq!(
            rfc3339(4_107_542_400_000),
            "2100-03-01T00:00:00.000Z",
            "2100 is not a leap year"
        );
    }

    #[test]
    fn counts_split_the_rows_by_status() {
        let mut counts = ResultCounts::default();
        for status in [
            RunStatus::Ok,
            RunStatus::Ok,
            RunStatus::NonFinite,
            RunStatus::Panicked,
            RunStatus::ShapeError,
        ] {
            counts.count(status);
        }
        assert_eq!(
            counts,
            ResultCounts {
                rows: 5,
                ok: 2,
                non_finite: 1,
                failed: 2
            }
        );
    }
}
