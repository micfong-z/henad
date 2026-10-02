//! `manifest.json`, the settings, provenance and progress of a sweep.
//!
//! A sweep writes the manifest with status `running` before its first run, and replaces it once the runs end.
//! Timestamps are RFC 3339 in UTC.

use std::collections::BTreeMap;
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
use henad_core::provenance::{BuildInfo, ModelSource};

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
    /// Build of Henad that wrote the manifest last, under the package name `henad`.
    pub engine: RecordedBuild,
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

    /// Returns the build that `session` records for `role`.
    ///
    /// A session that records no engine build, as every session of a 0.2 manifest is, reads as the engine build of
    /// its own `commit` and the version of the manifest's `engine` block. Its model build is `None`.
    pub fn session_build(&self, session: &ManifestSession, role: BuildRole) -> Option<RecordedBuild> {
        match role {
            BuildRole::Engine => Some(session.engine.clone().unwrap_or_else(|| RecordedBuild {
                package: self.engine.package.clone(),
                version: self.engine.version.clone(),
                commit: session.commit.clone(),
                commit_date: if session.commit == self.engine.commit {
                    self.engine.commit_date.clone()
                } else {
                    String::new()
                },
                dirty: None,
                source_hash: None,
                debug_build: self.engine.debug_build,
                type_path: None,
                crate_hashes: BTreeMap::new(),
                crate_versions: BTreeMap::new(),
            })),
            BuildRole::Model => session.model_source.clone(),
        }
    }

    /// Returns every distinct build the sessions record for `role`, in session order.
    ///
    /// Builds that [`RecordedBuild::same_build`] finds the same are listed once, as the first session records it.
    pub fn recorded_builds(&self, role: BuildRole) -> Vec<RecordedBuild> {
        let mut builds: Vec<RecordedBuild> = Vec::new();
        for build in self
            .sessions
            .iter()
            .filter_map(|session| self.session_build(session, role))
        {
            if !builds.iter().any(|known| known.same_build(&build)) {
                builds.push(build);
            }
        }
        builds
    }

    /// Writes into each session that records no engine build the build [`Self::session_build`] reads for it.
    ///
    /// A resume or a merge calls this before the manifest's `engine` block changes. A 0.2 session then keeps the
    /// version it ran.
    pub(crate) fn record_session_engines(&mut self) {
        let builds: Vec<Option<RecordedBuild>> = self
            .sessions
            .iter()
            .map(|session| self.session_build(session, BuildRole::Engine))
            .collect();
        for (session, build) in self.sessions.iter_mut().zip(builds) {
            if session.engine.is_none() {
                session.engine = build;
            }
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

/// One build as a manifest records it: Henad's engine, a host binary or the crate that registered a model.
///
/// A 0.2 manifest's `engine` block reads as one, with the fields 0.2 did not write at their defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedBuild {
    /// Package name, under the key `name` that 0.2's engine block uses.
    #[serde(rename = "name")]
    pub package: String,
    pub version: String,
    /// Short commit hash, empty when the build did not know it.
    pub commit: String,
    pub commit_date: String,
    /// Whether the build's sources differed from its commit, `None` when the build could not tell.
    #[serde(default)]
    pub dirty: Option<bool>,
    /// Hash of the build's sources, written as 16 hexadecimal digits as `schema_hash` is.
    #[serde(default, with = "hex_hash")]
    pub source_hash: Option<u64>,
    pub debug_build: bool,
    /// Type path of a model's registered type. `None` for the engine and the host.
    #[serde(default)]
    pub type_path: Option<String>,
    /// Hash of each engine crate's own sources, by package name. Empty for a host or a model.
    #[serde(default, with = "hex_hash_map")]
    pub crate_hashes: BTreeMap<String, u64>,
    /// Version of each engine crate, by package name. Empty for a host or a model, and in a 0.2 folder.
    #[serde(default)]
    pub crate_versions: BTreeMap<String, String>,
}

impl From<&BuildInfo> for RecordedBuild {
    fn from(build: &BuildInfo) -> Self {
        Self {
            package: build.package().to_owned(),
            version: build.version().to_owned(),
            commit: build.commit().to_owned(),
            commit_date: build.commit_date().to_owned(),
            dirty: build.dirty(),
            source_hash: build.source_hash(),
            debug_build: build.debug_build(),
            type_path: None,
            crate_hashes: BTreeMap::new(),
            crate_versions: BTreeMap::new(),
        }
    }
}

impl From<&ModelSource> for RecordedBuild {
    /// Records the registering crate's build and the type path. A source whose set recorded no build reads as an
    /// empty package that cannot be identified.
    fn from(source: &ModelSource) -> Self {
        let mut build = source.build().map_or_else(
            || Self {
                package: String::new(),
                version: String::new(),
                commit: String::new(),
                commit_date: String::new(),
                dirty: None,
                source_hash: None,
                debug_build: cfg!(debug_assertions),
                type_path: None,
                crate_hashes: BTreeMap::new(),
                crate_versions: BTreeMap::new(),
            },
            Self::from,
        );
        build.type_path = Some(source.type_path().to_owned());
        build
    }
}

/// Package name the `engine` block of a manifest records for Henad.
pub const ENGINE_PACKAGE: &str = "henad";

impl RecordedBuild {
    /// Returns the record of [`ENGINE_BUILD`](crate::ENGINE_BUILD) under the package name `henad`, with the version of
    /// each engine crate and the source hashes of henad-compute and henad-explore.
    pub fn engine() -> Self {
        let engine = crate::ENGINE_BUILD;
        let compute = henad_compute::__COMPUTE_BUILD;
        let mut build = Self::from(&engine);
        build.package = ENGINE_PACKAGE.to_owned();
        let versions = [
            ("henad-core", henad_core::__VERSION),
            ("henad-build", crate::STAMP_VERSION),
            ("henad-compute", compute.version()),
            ("henad-explore", engine.version()),
        ];
        build.crate_versions = versions
            .into_iter()
            .filter(|(_, version)| !version.is_empty())
            .map(|(package, version)| (package.to_owned(), version.to_owned()))
            .collect();
        let hashes = [
            ("henad-compute", compute.source_hash()),
            ("henad-explore", u64::from_str_radix(crate::CRATE_HASH, 16).ok()),
        ];
        build.crate_hashes = hashes
            .into_iter()
            .filter_map(|(package, hash)| Some((package.to_owned(), hash?)))
            .collect();
        build
    }

    /// Returns whether `self` and `other` are the same build.
    ///
    /// The checks run in order. A different package or version is a change, as is a different version or source hash
    /// of an engine crate both sides record. Then two builds that each record a clean commit compare by commit alone.
    /// Otherwise the source hashes decide.
    ///
    /// A commit counts as clean when the build records `dirty` as `false`. A build whose dirty flag is unknown counts
    /// as clean only when it records no source hash either, as a session of a 0.2 manifest does. Any other build with
    /// an unknown flag goes to the source hashes.
    ///
    /// Two builds that both record neither a commit nor a source hash are never the same.
    pub fn same_build(&self, other: &Self) -> bool {
        if self.package != other.package || self.version != other.version {
            return false;
        }
        if differs_in(&self.crate_versions, &other.crate_versions)
            || differs_in(&self.crate_hashes, &other.crate_hashes)
        {
            return false;
        }
        if self.is_clean_commit() && other.is_clean_commit() {
            return self.commit == other.commit;
        }
        matches!((self.source_hash, other.source_hash), (Some(left), Some(right)) if left == right)
    }

    /// Returns whether the build records a commit known to be clean, or a commit alone, with neither a dirty flag nor
    /// a source hash.
    fn is_clean_commit(&self) -> bool {
        !self.commit.is_empty()
            && match self.dirty {
                Some(dirty) => !dirty,
                None => self.source_hash.is_none(),
            }
    }

    /// Returns whether the build records a commit or a source hash.
    pub fn is_identified(&self) -> bool {
        !self.commit.is_empty() || self.source_hash.is_some()
    }

    /// Returns the package, the version and what identifies the build, as in `henad 0.2.0 (52fb2ab3, modified,
    /// sources 0123456789abcdef)`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.commit.is_empty() {
            parts.push(self.commit.clone());
        }
        if self.dirty == Some(true) {
            parts.push("modified".to_owned());
        }
        if (self.commit.is_empty() || self.dirty == Some(true))
            && let Some(hash) = self.source_hash
        {
            parts.push(format!("sources {hash:016x}"));
        }
        let package = if self.package.is_empty() {
            "an unknown package"
        } else {
            &self.package
        };
        let mut text = format!("{package} {}", self.version).trim_end().to_owned();
        if parts.is_empty() {
            text.push_str(" (unidentified)");
        } else {
            text.push_str(&format!(" ({})", parts.join(", ")));
        }
        text
    }

    /// Returns the first engine crate whose version or source hash differs between `self` and `other`, as text.
    pub(crate) fn crate_difference(&self, other: &Self) -> Option<String> {
        for (package, version) in &self.crate_versions {
            if let Some(other_version) = other.crate_versions.get(package)
                && other_version != version
            {
                return Some(format!("{package} {version} against {other_version}"));
            }
        }
        self.crate_hashes.iter().find_map(|(package, hash)| {
            other
                .crate_hashes
                .get(package)
                .filter(|other_hash| *other_hash != hash)
                .map(|_| format!("the sources of {package}"))
        })
    }
}

/// Returns whether a package both maps hold has another value in each.
fn differs_in<T: PartialEq>(left: &BTreeMap<String, T>, right: &BTreeMap<String, T>) -> bool {
    left.iter()
        .any(|(package, value)| right.get(package).is_some_and(|other_value| other_value != value))
}

/// Part of a sweep whose build a manifest records for each session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildRole {
    /// Henad itself.
    Engine,
    /// Crate that registered the model.
    Model,
}

impl BuildRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Model => "model",
        }
    }
}

/// Serde form of an optional hash, as 16 hexadecimal digits or null.
mod hex_hash {
    use serde::{Deserialize as _, Deserializer, Serializer};

    #[expect(clippy::ref_option, reason = "serde hands a field to its writer by reference")]
    pub(super) fn serialize<S: Serializer>(hash: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match hash {
            Some(hash) => serializer.serialize_str(&format!("{hash:016x}")),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| u64::from_str_radix(&text, 16).map_err(serde::de::Error::custom))
            .transpose()
    }
}

/// Serde form of a map of hashes, each as 16 hexadecimal digits.
mod hex_hash_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize as _, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(hashes: &BTreeMap<String, u64>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(hashes.iter().map(|(package, hash)| (package, format!("{hash:016x}"))))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, u64>, D::Error> {
        BTreeMap::<String, String>::deserialize(deserializer)?
            .into_iter()
            .map(|(package, text)| {
                let hash = u64::from_str_radix(&text, 16).map_err(serde::de::Error::custom)?;
                Ok((package, hash))
            })
            .collect()
    }
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
    /// Whether two builds of the model on one seed step through identical states. A 0.2 manifest reads as `true`.
    #[serde(default = "default_true")]
    pub replays_exactly: bool,
}

/// Default of [`ManifestModel::replays_exactly`]. Serde takes a default only as a path.
fn default_true() -> bool {
    true
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
    /// Commit of the engine build that ran the session.
    pub commit: String,
    /// Runs the session found written and kept.
    pub skipped: u64,
    /// Runs the session wrote.
    pub ran: u64,
    /// Build of Henad that ran the session, `None` in a 0.2 manifest.
    #[serde(default)]
    pub engine: Option<RecordedBuild>,
    /// Build of the host binary that ran the session, `None` in a 0.2 manifest.
    #[serde(default)]
    pub host: Option<RecordedBuild>,
    /// Build of the crate that registered the model, with the model's type path, `None` in a 0.2 manifest.
    #[serde(default)]
    pub model_source: Option<RecordedBuild>,
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
pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since_epoch| {
        u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
    })
}

/// Returns `unix_ms` as an RFC 3339 timestamp in UTC with milliseconds, as in `2026-09-27T08:30:00.250Z`.
pub(crate) fn rfc3339(unix_ms: u64) -> String {
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

    use super::{Manifest, RecordedBuild, ResultCounts, rfc3339};
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
    fn two_unidentified_builds_are_never_the_same() {
        let mut build = RecordedBuild::engine();
        build.commit = String::new();
        build.dirty = None;
        build.source_hash = None;
        assert!(!build.same_build(&build.clone()));
        build.source_hash = Some(7);
        assert!(build.same_build(&build.clone()));
        build.commit = "abc12345".to_owned();
        build.dirty = Some(false);
        let mut other = build.clone();
        other.source_hash = Some(8);
        assert!(
            build.same_build(&other),
            "two clean builds of one commit compare by commit"
        );
        other.dirty = Some(true);
        assert!(!build.same_build(&other), "a dirty side compares by source hash");
        other.dirty = None;
        assert!(
            !build.same_build(&other),
            "a commit with an unknown flag and a source hash compares by source hash"
        );
        other.source_hash = Some(7);
        assert!(build.same_build(&other));
        let mut commit_alone = build.clone();
        (commit_alone.dirty, commit_alone.source_hash) = (None, None);
        assert!(
            commit_alone.same_build(&build),
            "a 0.2 session's commit alone compares by commit"
        );
    }

    #[test]
    fn a_recorded_build_writes_its_hashes_in_hexadecimal() {
        let mut build = RecordedBuild::engine();
        build.source_hash = Some(0xAB);
        build.crate_hashes.insert("henad-compute".to_owned(), 0xCD);
        let json = serde_json::to_value(&build).expect("a build writes as JSON");
        assert_eq!(json["source_hash"], "00000000000000ab");
        assert_eq!(json["crate_hashes"]["henad-compute"], "00000000000000cd");
        assert_eq!(json["name"], "henad");
        let read: RecordedBuild = serde_json::from_value(json).expect("a build reads back");
        assert_eq!(read, build);

        let block_0_2 = serde_json::json!({
            "name": "henad",
            "version": "0.2.0",
            "commit": "773a7a5b",
            "commit_date": "2026-09-29",
            "debug_build": false,
        });
        let read: RecordedBuild = serde_json::from_value(block_0_2).expect("a 0.2 engine block reads");
        assert_eq!((read.dirty, read.source_hash, read.type_path), (None, None, None));
        assert!(read.crate_hashes.is_empty() && read.crate_versions.is_empty());
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
