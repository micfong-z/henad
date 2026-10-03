//! Spec files, the TOML form of a sweep spec.
//!
//! Every table refuses a key it does not know. A parameter value is kept as the text `--set` and `--vary` take, so a
//! spec file and a command line hand [`parse_value`] the same text.
//!
//! [`parse_value`]: henad_core::explore::value::parse_value

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use henad_core::explore::fingerprint::fnv1a64;
use henad_core::explore::reducer::{ReducerError, ReducerSpec};
use henad_core::explore::search::genetic::GeneticSettings;
use henad_core::explore::search::hill_climb::HillClimbSettings;
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec};
use henad_core::explore::seed::SeedScheme;
use henad_core::explore::spec::{ActionSpec, BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec};
use henad_core::explore::stop::{StopError, StopSpec};

use crate::exec::Concurrency;
use crate::sweep::SpecSource;

/// A sweep spec as a TOML file writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecFile {
    /// Id of the model to run.
    pub model: String,
    /// Values every config shares, by parameter id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub set: BTreeMap<String, SpecValue>,
    #[serde(default)]
    pub run: RunTable,
    #[serde(default)]
    pub measure: MeasureTable,
    #[serde(default)]
    pub seeds: SeedsTable,
    /// Actions every run fires, each an `[[action]]` table.
    #[serde(default, rename = "action", skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionTable>,
    /// Blocks in order, each a `[[block]]` table.
    #[serde(default, rename = "block", skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<BlockTable>,
    /// Search that picks the configs in place of blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchTable>,
    /// Settings that spread the runs over the machine and never change a result.
    #[serde(default, skip_serializing_if = "ExecutionTable::is_default")]
    pub execution: ExecutionTable,
}

/// Length and end of each run, and the number of runs per config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RunTable {
    pub steps: u64,
    pub warmup: u64,
    pub replicates: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopTable>,
    /// Seconds of wall-clock time after which a run is abandoned. On a GPU track, a run's clock counts its share of the
    /// time the sweep spends on the tracks, so the run can take longer than this in real time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<f64>,
}

impl Default for RunTable {
    fn default() -> Self {
        (&RunSettings::default()).into()
    }
}

impl From<&RunSettings> for RunTable {
    fn from(run: &RunSettings) -> Self {
        Self {
            steps: run.steps,
            warmup: run.warmup,
            replicates: run.replicates,
            stop: run.stop.as_ref().map(|stop| StopTable {
                condition: stop.to_string(),
                min_tick: stop.min_tick,
            }),
            timeout_s: run.timeout.map(|timeout| timeout.as_secs_f64()),
        }
    }
}

impl RunTable {
    /// Returns the run settings the table writes.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Stop`] for a stop condition that cannot be read, and [`SpecFileError::Timeout`] for
    /// a timeout that is negative or not a number.
    fn into_settings(self) -> Result<RunSettings, SpecFileError> {
        let stop = self
            .stop
            .map(|stop| StopSpec::parse(&stop.condition, stop.min_tick))
            .transpose()
            .map_err(SpecFileError::Stop)?;
        let timeout = self
            .timeout_s
            .map(|seconds| {
                Duration::try_from_secs_f64(seconds)
                    .ok()
                    .ok_or(SpecFileError::Timeout { seconds })
            })
            .transpose()?;
        Ok(RunSettings {
            steps: self.steps,
            warmup: self.warmup,
            replicates: self.replicates,
            stop,
            timeout,
        })
    }
}

/// Condition that ends a run at the first sample where it holds, as in `Infected <= 0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopTable {
    pub condition: String,
    /// First tick at which the condition can end a run.
    #[serde(default)]
    pub min_tick: u64,
}

/// A model action every run fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionTable {
    /// Id of the action the model declares.
    pub id: String,
    /// Name factors and output columns give the action, the id when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub tick: u64,
}

impl From<&ActionSpec> for ActionTable {
    fn from(action: &ActionSpec) -> Self {
        Self {
            id: action.id.clone(),
            name: (action.name != action.id).then(|| action.name.clone()),
            tick: action.tick,
        }
    }
}

impl From<ActionTable> for ActionSpec {
    fn from(action: ActionTable) -> Self {
        Self {
            name: action.name.unwrap_or_else(|| action.id.clone()),
            id: action.id,
            tick: action.tick,
        }
    }
}

/// Sampling cadence and reducers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MeasureTable {
    pub stats_every: u64,
    /// Ticks between two rows of the series, `stats_every` when left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_every: Option<u64>,
    pub default_reducers: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reducers: Vec<ReducerTable>,
}

impl Default for MeasureTable {
    fn default() -> Self {
        let measure = MeasureSettings::default();
        Self {
            stats_every: measure.stats_every,
            series_every: None,
            default_reducers: measure.default_reducers,
            reducers: Vec::new(),
        }
    }
}

/// Reducers over one stat column, one per kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReducerTable {
    pub column: String,
    pub kinds: Vec<String>,
}

/// Root seed and seed scheme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SeedsTable {
    pub root: TomlSeed,
    pub scheme: SeedSchemeFile,
}

/// Seed written as an integer or, past the largest TOML integer, as a decimal string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TomlSeed(pub u64);

impl Serialize for TomlSeed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match i64::try_from(self.0) {
            Ok(seed) => serializer.serialize_i64(seed),
            Err(_) => serializer.serialize_str(&self.0.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for TomlSeed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SeedVisitor;

        impl Visitor<'_> for SeedVisitor {
            type Value = TomlSeed;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a seed from 0 to 18446744073709551615, as an integer or a decimal string")
            }

            fn visit_i64<E: de::Error>(self, seed: i64) -> Result<TomlSeed, E> {
                u64::try_from(seed)
                    .ok()
                    .map(TomlSeed)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Signed(seed), &self))
            }

            fn visit_u64<E: de::Error>(self, seed: u64) -> Result<TomlSeed, E> {
                Ok(TomlSeed(seed))
            }

            fn visit_str<E: de::Error>(self, seed: &str) -> Result<TomlSeed, E> {
                seed.parse()
                    .ok()
                    .map(TomlSeed)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Str(seed), &self))
            }
        }

        deserializer.deserialize_any(SeedVisitor)
    }
}

/// Seed scheme, as [`SeedScheme::as_str`] names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedSchemeFile {
    #[default]
    Common,
    Independent,
}

impl From<SeedScheme> for SeedSchemeFile {
    fn from(scheme: SeedScheme) -> Self {
        match scheme {
            SeedScheme::Common => Self::Common,
            SeedScheme::Independent => Self::Independent,
        }
    }
}

impl From<SeedSchemeFile> for SeedScheme {
    fn from(scheme: SeedSchemeFile) -> Self {
        match scheme {
            SeedSchemeFile::Common => Self::Common,
            SeedSchemeFile::Independent => Self::Independent,
        }
    }
}

/// Factors combined under one design.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockTable {
    #[serde(default)]
    pub design: DesignKindFile,
    /// Configs a random or Latin hypercube design draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<usize>,
    /// Seed of a random or Latin hypercube design's draws, derived from the root seed when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_seed: Option<TomlSeed>,
    /// Path of a table design's CSV file, relative to the spec file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// Text of a table design's CSV, written in the spec in place of `file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_text: Option<String>,
    #[serde(default)]
    pub factors: Vec<FactorTable>,
    /// Text of the table `file` names, read by [`SpecFile::load`].
    #[serde(skip)]
    pub file_text: Option<String>,
}

/// Design table a spec file reads, as its path and the hash of its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTableFile {
    /// Path as the spec file writes it.
    pub path: PathBuf,
    /// FNV-1a hash of the table's text.
    pub fnv1a64: u64,
}

/// Design of a block, as [`DesignKind::as_str`] names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignKindFile {
    #[default]
    Factorial,
    Zip,
    Random,
    Lhs,
    Table,
}

impl From<&DesignKind> for DesignKindFile {
    fn from(design: &DesignKind) -> Self {
        match design {
            DesignKind::Factorial => Self::Factorial,
            DesignKind::Zip => Self::Zip,
            DesignKind::Random { .. } => Self::Random,
            DesignKind::LatinHypercube { .. } => Self::Lhs,
            DesignKind::Table { .. } => Self::Table,
        }
    }
}

/// A factor over one parameter or one action's tick, with exactly one of `values`, `range` and `levels`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactorTable {
    /// Id of the parameter the factor varies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Name of the action whose tick the factor varies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<SpecValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<LevelsKeyword>,
}

/// Every value from `min` to `max` inclusive, `step` apart, as [`LevelSpec::Range`] reads them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RangeTable {
    pub min: f64,
    pub max: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
}

/// Levels named by a keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelsKeyword {
    /// Every value of a `Bool` or `Choice` parameter.
    All,
}

/// A factor table without exactly one target or one form of levels.
enum FactorTableError {
    /// None or both of `param` and `action`.
    Target,
    /// None or several of `values`, `range` and `levels`, on the parameter or action `target_name`.
    Levels { target_name: String },
}

impl FactorTable {
    fn into_spec(self) -> Result<FactorSpec, FactorTableError> {
        let target = match (self.param, self.action) {
            (Some(id), None) => FactorTarget::Param(id),
            (None, Some(name)) => FactorTarget::Action(name),
            _ => return Err(FactorTableError::Target),
        };
        let levels = match (self.values, self.range, self.levels) {
            (Some(values), None, None) => LevelSpec::Values(values.iter().map(SpecValue::to_text).collect()),
            (None, Some(range), None) => LevelSpec::Range {
                min: range.min,
                max: range.max,
                step: range.step,
            },
            (None, None, Some(LevelsKeyword::All)) => LevelSpec::All,
            _ => {
                let (FactorTarget::Param(target_name) | FactorTarget::Action(target_name)) = target;
                return Err(FactorTableError::Levels { target_name });
            }
        };
        Ok(FactorSpec { target, levels })
    }
}

impl From<&FactorSpec> for FactorTable {
    /// Writes each value as [`SpecValue::from_text`] reads it.
    fn from(factor: &FactorSpec) -> Self {
        let (param, action) = match &factor.target {
            FactorTarget::Param(id) => (Some(id.clone()), None),
            FactorTarget::Action(name) => (None, Some(name.clone())),
        };
        let mut table = Self {
            param,
            action,
            values: None,
            range: None,
            levels: None,
        };
        match &factor.levels {
            LevelSpec::Values(values) => {
                table.values = Some(values.iter().map(|value| SpecValue::from_text(value)).collect());
            }
            &LevelSpec::Range { min, max, step } => table.range = Some(RangeTable { min, max, step }),
            LevelSpec::All => table.levels = Some(LevelsKeyword::All),
        }
        table
    }
}

/// A search, the `[search]` table.
///
/// The table of the chosen algorithm, `[search.hill_climb]`, `[search.genetic]` or `[search.pse]`, holds its
/// settings. A random search takes none, and a left-out table of another algorithm takes its defaults, except for
/// `[search.pse]`, whose axes are needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchTable {
    pub algorithm: SearchAlgorithmFile,
    /// Evaluations the search can ask for, re-evaluations included.
    pub max_evaluations: u64,
    /// Most candidates one batch evaluates.
    pub batch_size: usize,
    /// Output a random search, hill climb or genetic algorithm scores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<ObjectiveTable>,
    /// Factors the search varies, in the form a block's factors take.
    pub space: Vec<FactorTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hill_climb: Option<HillClimbTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genetic: Option<GeneticTable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pse: Option<PatternSpaceTable>,
}

/// Search method, as [`SearchAlgorithm::as_str`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchAlgorithmFile {
    Random,
    HillClimb,
    Genetic,
    Pse,
}

/// Output a search scores candidates by, as `{ column, goal, aggregate }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectiveTable {
    /// Reducer column, as in `Infected:max`.
    pub column: String,
    pub goal: GoalFile,
    /// Rule that folds a candidate's replicates into one value, the median when left out.
    #[serde(default)]
    pub aggregate: AggregateFile,
}

/// Direction of an objective, as [`Goal::as_str`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalFile {
    Minimize,
    Maximize,
}

/// Rule that folds replicates into one value, as [`Aggregate::as_str`] names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateFile {
    Mean,
    #[default]
    Median,
}

/// Settings of a hill climb, each defaulting to [`HillClimbSettings::default`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HillClimbTable {
    /// Largest step of a gene toward a neighbor, as a fraction of its range.
    pub mutation_scale: f64,
    /// Batches without a move after which the climb starts over.
    pub patience: u64,
    /// Whether each batch also re-evaluates the incumbent.
    pub reevaluate: bool,
}

impl Default for HillClimbTable {
    fn default() -> Self {
        (&HillClimbSettings::default()).into()
    }
}

/// Settings of a genetic algorithm, each defaulting to [`GeneticSettings::default`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GeneticTable {
    pub population: usize,
    pub elite_count: usize,
    pub tournament_size: usize,
    pub crossover_rate: f64,
    /// Probability that each gene of a child changes.
    pub mutation_rate: f64,
    /// Largest step of a changed gene, as a fraction of its range.
    pub mutation_scale: f64,
    /// Share of the population re-evaluated each generation, rounded up.
    pub reevaluate_fraction: f64,
}

impl Default for GeneticTable {
    fn default() -> Self {
        (&GeneticSettings::default()).into()
    }
}

/// Settings of a Pattern Space Exploration.
///
/// The axes are needed, and the rest default as [`PatternSpaceSettings::new`] sets them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSpaceTable {
    pub x_axis: PatternAxisTable,
    pub y_axis: PatternAxisTable,
    /// Candidates drawn at random before any is bred from the archive.
    #[serde(default = "default_initial_samples")]
    pub initial_samples: u64,
    /// Largest step of a mutated gene, as a fraction of its range.
    #[serde(default = "default_pattern_mutation_scale")]
    pub mutation_scale: f64,
    #[serde(default)]
    pub aggregate: AggregateFile,
}

/// One axis of a Pattern Space Exploration's grid, as `{ column, min, max, cells }`, or as `{ column, cells }` for a
/// range taken from the initial samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternAxisTable {
    /// Reducer column, as in `Infected:max`.
    pub column: String,
    /// Lower bound, left out with `max` for an automatic range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Upper bound, left out with `min` for an automatic range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Number of cells from `min` to `max`.
    pub cells: u32,
}

fn default_pattern_settings() -> PatternSpaceSettings {
    let axis = PatternAxis::automatic(String::new(), 1);
    PatternSpaceSettings::new(axis.clone(), axis)
}

fn default_initial_samples() -> u64 {
    default_pattern_settings().initial_samples
}

fn default_pattern_mutation_scale() -> f64 {
    default_pattern_settings().mutation_scale
}

impl From<&HillClimbSettings> for HillClimbTable {
    fn from(settings: &HillClimbSettings) -> Self {
        Self {
            mutation_scale: settings.mutation_scale,
            patience: settings.patience,
            reevaluate: settings.reevaluate,
        }
    }
}

impl From<HillClimbTable> for HillClimbSettings {
    fn from(table: HillClimbTable) -> Self {
        Self {
            mutation_scale: table.mutation_scale,
            patience: table.patience,
            reevaluate: table.reevaluate,
        }
    }
}

impl From<&GeneticSettings> for GeneticTable {
    fn from(settings: &GeneticSettings) -> Self {
        Self {
            population: settings.population,
            elite_count: settings.elite_count,
            tournament_size: settings.tournament_size,
            crossover_rate: settings.crossover_rate,
            mutation_rate: settings.mutation_rate,
            mutation_scale: settings.mutation_scale,
            reevaluate_fraction: settings.reevaluate_fraction,
        }
    }
}

impl From<GeneticTable> for GeneticSettings {
    fn from(table: GeneticTable) -> Self {
        Self {
            population: table.population,
            elite_count: table.elite_count,
            tournament_size: table.tournament_size,
            crossover_rate: table.crossover_rate,
            mutation_rate: table.mutation_rate,
            mutation_scale: table.mutation_scale,
            reevaluate_fraction: table.reevaluate_fraction,
        }
    }
}

impl From<&PatternAxis> for PatternAxisTable {
    fn from(axis: &PatternAxis) -> Self {
        Self {
            column: axis.column.clone(),
            min: axis.min,
            max: axis.max,
            cells: axis.cells,
        }
    }
}

impl PatternAxisTable {
    /// Returns the axis the table describes.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Search`] with the reason `lone_bound` for a table with one bound and not the other.
    fn into_axis(self, lone_bound: &'static str) -> Result<PatternAxis, SpecFileError> {
        if self.min.is_some() != self.max.is_some() {
            return Err(SpecFileError::Search { reason: lone_bound });
        }
        Ok(PatternAxis {
            column: self.column,
            min: self.min,
            max: self.max,
            cells: self.cells,
        })
    }
}

impl From<&PatternSpaceSettings> for PatternSpaceTable {
    fn from(settings: &PatternSpaceSettings) -> Self {
        Self {
            x_axis: (&settings.x_axis).into(),
            y_axis: (&settings.y_axis).into(),
            initial_samples: settings.initial_samples,
            mutation_scale: settings.mutation_scale,
            aggregate: settings.aggregate.into(),
        }
    }
}

impl PatternSpaceTable {
    /// Returns the settings the table describes.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Search`] for an axis with one bound and not the other.
    fn into_settings(self) -> Result<PatternSpaceSettings, SpecFileError> {
        Ok(PatternSpaceSettings {
            x_axis: self
                .x_axis
                .into_axis("x_axis needs both min and max, or neither for an automatic range")?,
            y_axis: self
                .y_axis
                .into_axis("y_axis needs both min and max, or neither for an automatic range")?,
            initial_samples: self.initial_samples,
            mutation_scale: self.mutation_scale,
            aggregate: self.aggregate.into(),
        })
    }
}

impl From<Goal> for GoalFile {
    fn from(goal: Goal) -> Self {
        match goal {
            Goal::Minimize => Self::Minimize,
            Goal::Maximize => Self::Maximize,
        }
    }
}

impl From<GoalFile> for Goal {
    fn from(goal: GoalFile) -> Self {
        match goal {
            GoalFile::Minimize => Self::Minimize,
            GoalFile::Maximize => Self::Maximize,
        }
    }
}

impl From<Aggregate> for AggregateFile {
    fn from(aggregate: Aggregate) -> Self {
        match aggregate {
            Aggregate::Mean => Self::Mean,
            Aggregate::Median => Self::Median,
        }
    }
}

impl From<AggregateFile> for Aggregate {
    fn from(aggregate: AggregateFile) -> Self {
        match aggregate {
            AggregateFile::Mean => Self::Mean,
            AggregateFile::Median => Self::Median,
        }
    }
}

impl SearchTable {
    /// Returns the search spec the table writes.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError`] for a factor without exactly one target or one form of levels, a settings table of
    /// another algorithm than the chosen one, or a Pattern Space Exploration with no `[search.pse]` table. The
    /// settings themselves are checked when the search is planned.
    fn into_spec(self) -> Result<SearchSpec, SpecFileError> {
        let refuse = |reason| Err(SpecFileError::Search { reason });
        if self.hill_climb.is_some() && self.algorithm != SearchAlgorithmFile::HillClimb {
            return refuse("[search.hill_climb] applies to a hill_climb search");
        }
        if self.genetic.is_some() && self.algorithm != SearchAlgorithmFile::Genetic {
            return refuse("[search.genetic] applies to a genetic search");
        }
        if self.pse.is_some() && self.algorithm != SearchAlgorithmFile::Pse {
            return refuse("[search.pse] applies to a pse search");
        }
        let algorithm = match self.algorithm {
            SearchAlgorithmFile::Random => SearchAlgorithm::Random,
            SearchAlgorithmFile::HillClimb => SearchAlgorithm::HillClimb(self.hill_climb.unwrap_or_default().into()),
            SearchAlgorithmFile::Genetic => SearchAlgorithm::Genetic(self.genetic.unwrap_or_default().into()),
            SearchAlgorithmFile::Pse => match self.pse {
                Some(settings) => SearchAlgorithm::PatternSpaceExploration(settings.into_settings()?),
                None => return refuse("a pse search needs a [search.pse] table with x_axis and y_axis"),
            },
        };
        let space = self
            .space
            .into_iter()
            .map(|factor| {
                factor.into_spec().map_err(|error| match error {
                    FactorTableError::Target => SpecFileError::SearchFactorTarget,
                    FactorTableError::Levels { target_name } => SpecFileError::SearchFactorLevels { target_name },
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(SearchSpec {
            algorithm,
            max_evaluations: self.max_evaluations,
            batch_size: self.batch_size,
            objective: self.objective.map(|objective| Objective {
                column: objective.column,
                goal: objective.goal.into(),
                aggregate: objective.aggregate.into(),
            }),
            space,
        })
    }
}

impl From<&SearchSpec> for SearchTable {
    /// Writes the settings of the chosen algorithm in full, in its own table.
    fn from(search: &SearchSpec) -> Self {
        let mut table = Self {
            algorithm: SearchAlgorithmFile::Random,
            max_evaluations: search.max_evaluations,
            batch_size: search.batch_size,
            objective: search.objective.as_ref().map(|objective| ObjectiveTable {
                column: objective.column.clone(),
                goal: objective.goal.into(),
                aggregate: objective.aggregate.into(),
            }),
            space: search.space.iter().map(FactorTable::from).collect(),
            hill_climb: None,
            genetic: None,
            pse: None,
        };
        match &search.algorithm {
            SearchAlgorithm::Random => {}
            SearchAlgorithm::HillClimb(settings) => {
                table.algorithm = SearchAlgorithmFile::HillClimb;
                table.hill_climb = Some(settings.into());
            }
            SearchAlgorithm::Genetic(settings) => {
                table.algorithm = SearchAlgorithmFile::Genetic;
                table.genetic = Some(settings.into());
            }
            SearchAlgorithm::PatternSpaceExploration(settings) => {
                table.algorithm = SearchAlgorithmFile::Pse;
                table.pse = Some(settings.into());
            }
        }
        table
    }
}

/// Settings that spread the runs over the machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExecutionTable {
    /// `"auto"`, or a number of CPU lanes or GPU tracks.
    #[serde(with = "concurrent_field")]
    pub concurrent: Concurrency,
    /// Bytes of host memory the live runs can hold together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
    /// Bytes of device memory the live GPU runs can hold together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_memory: Option<u64>,
}

impl ExecutionTable {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Serde form of [`ExecutionTable::concurrent`], `"auto"` or a positive integer.
mod concurrent_field {
    use std::fmt;
    use std::num::NonZeroUsize;

    use serde::de::{self, Deserializer, Visitor};
    use serde::ser::Serializer;

    use crate::exec::Concurrency;

    pub(super) fn serialize<S: Serializer>(concurrency: &Concurrency, serializer: S) -> Result<S::Ok, S::Error> {
        match concurrency {
            Concurrency::Auto => serializer.serialize_str("auto"),
            Concurrency::Fixed(count) => serializer.serialize_u64(count.get() as u64),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Concurrency, D::Error> {
        struct ConcurrencyVisitor;

        impl Visitor<'_> for ConcurrencyVisitor {
            type Value = Concurrency;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("\"auto\" or a count of at least 1")
            }

            fn visit_i64<E: de::Error>(self, count: i64) -> Result<Concurrency, E> {
                usize::try_from(count)
                    .ok()
                    .and_then(NonZeroUsize::new)
                    .map(Concurrency::Fixed)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Signed(count), &self))
            }

            fn visit_u64<E: de::Error>(self, count: u64) -> Result<Concurrency, E> {
                usize::try_from(count)
                    .ok()
                    .and_then(NonZeroUsize::new)
                    .map(Concurrency::Fixed)
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Unsigned(count), &self))
            }

            fn visit_str<E: de::Error>(self, raw: &str) -> Result<Concurrency, E> {
                raw.parse()
                    .ok()
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Str(raw), &self))
            }
        }

        deserializer.deserialize_any(ConcurrencyVisitor)
    }
}

/// A parameter value as a spec file writes it.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecValue {
    Integer(i64),
    Float(f64),
    Bool(bool),
    Text(String),
}

impl SpecValue {
    /// Returns the value as text in the form `--set` takes.
    ///
    /// A float takes its shortest round-trip form, as in `0.1`.
    pub fn to_text(&self) -> String {
        match self {
            Self::Integer(number) => number.to_string(),
            Self::Float(number) => number.to_string(),
            Self::Bool(flag) => flag.to_string(),
            Self::Text(text) => text.clone(),
        }
    }

    /// Returns `text` as the value [`Self::to_text`] writes back as `text`.
    ///
    /// Text that reads as an integer, a finite float or a bool, and is written back unchanged, becomes one.
    /// Anything else stays text.
    pub fn from_text(text: &str) -> Self {
        if let Ok(number) = text.parse::<i64>()
            && number.to_string() == text
        {
            return Self::Integer(number);
        }
        if let Ok(number) = text.parse::<f64>()
            && number.is_finite()
            && number.to_string() == text
        {
            return Self::Float(number);
        }
        match text {
            "true" => Self::Bool(true),
            "false" => Self::Bool(false),
            _ => Self::Text(text.to_owned()),
        }
    }
}

impl Serialize for SpecValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Integer(number) => serializer.serialize_i64(*number),
            Self::Float(number) => serializer.serialize_f64(*number),
            Self::Bool(flag) => serializer.serialize_bool(*flag),
            Self::Text(text) => serializer.serialize_str(text),
        }
    }
}

impl<'de> Deserialize<'de> for SpecValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;

        impl Visitor<'_> for ValueVisitor {
            type Value = SpecValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a number, a bool or a string")
            }

            fn visit_bool<E: de::Error>(self, flag: bool) -> Result<SpecValue, E> {
                Ok(SpecValue::Bool(flag))
            }

            fn visit_i64<E: de::Error>(self, number: i64) -> Result<SpecValue, E> {
                Ok(SpecValue::Integer(number))
            }

            fn visit_u64<E: de::Error>(self, number: u64) -> Result<SpecValue, E> {
                Ok(i64::try_from(number).map_or_else(|_| SpecValue::Text(number.to_string()), SpecValue::Integer))
            }

            fn visit_f64<E: de::Error>(self, number: f64) -> Result<SpecValue, E> {
                Ok(SpecValue::Float(number))
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<SpecValue, E> {
                Ok(SpecValue::Text(text.to_owned()))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

/// A spec file that cannot be read or turned into a sweep spec.
#[derive(Debug)]
pub enum SpecFileError {
    /// Reading the file at `path` failed.
    Read { path: PathBuf, source: io::Error },
    /// Table path `path` of block `block`, absolute or holding a component other than a name, such as `..`.
    TablePath { block: usize, path: PathBuf },
    /// Text that is not a spec file, for the reason in `source`.
    Parse { source: Box<toml::de::Error> },
    /// JSON that is not a spec file, for the reason in `source`.
    Json { source: serde_json::Error },
    /// A factor of block `block` with none or both of `param` and `action`.
    FactorTarget { block: usize },
    /// Factor of block `block` on the parameter or action `target_name`, with none or several of `values`, `range`
    /// and `levels`.
    FactorLevels { block: usize, target_name: String },
    /// Block `block`, whose keys do not fit its design for the reason in `reason`.
    Design { block: usize, reason: &'static str },
    /// A reducer kind that cannot be read.
    Reducer(ReducerError),
    /// A stop condition that cannot be read.
    Stop(StopError),
    /// A timeout that is negative or not a number.
    Timeout { seconds: f64 },
    /// A `[search]` table whose keys do not fit its algorithm or the rest of the spec, for the reason in `reason`.
    Search { reason: &'static str },
    /// A factor of the search space with none or both of `param` and `action`.
    SearchFactorTarget,
    /// A factor of the search space on the parameter or action `target_name`, with none or several of `values`,
    /// `range` and `levels`.
    SearchFactorLevels { target_name: String },
    /// A spec that cannot be written as TOML, for the reason in `source`.
    Write { source: toml::ser::Error },
}

impl fmt::Display for SpecFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, .. } => write!(f, "cannot read '{}'", path.display()),
            Self::TablePath { block, path } => write!(
                f,
                "block {block}: table file '{}' must be a relative path without '..'",
                path.display()
            ),
            Self::Parse { .. } | Self::Json { .. } => f.write_str("not a valid spec file"),
            Self::FactorTarget { block } => {
                write!(f, "a factor of block {block} needs exactly one of param and action")
            }
            Self::FactorLevels { block, target_name } => write!(
                f,
                "factor '{target_name}' of block {block} needs exactly one of values, range and levels"
            ),
            Self::Design { block, reason } => write!(f, "block {block}: {reason}"),
            Self::Reducer(_) => f.write_str("reducer"),
            Self::Stop(_) => f.write_str("stop condition"),
            Self::Timeout { seconds } => write!(f, "timeout_s must be a non-negative number of seconds, got {seconds}"),
            Self::Search { reason } => write!(f, "search: {reason}"),
            Self::SearchFactorTarget => {
                f.write_str("a factor of the search space needs exactly one of param and action")
            }
            Self::SearchFactorLevels { target_name } => write!(
                f,
                "factor '{target_name}' of the search space needs exactly one of values, range and levels"
            ),
            Self::Write { .. } => f.write_str("cannot write the spec as TOML"),
        }
    }
}

impl std::error::Error for SpecFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source } => Some(source),
            Self::Json { source } => Some(source),
            Self::Reducer(error) => Some(error),
            Self::Stop(error) => Some(error),
            Self::Write { source } => Some(source),
            Self::TablePath { .. }
            | Self::FactorTarget { .. }
            | Self::FactorLevels { .. }
            | Self::Design { .. }
            | Self::Timeout { .. }
            | Self::Search { .. }
            | Self::SearchFactorTarget
            | Self::SearchFactorLevels { .. } => None,
        }
    }
}

/// A spec file read whole: the spec, the text it was read from, and its execution settings.
#[derive(Debug, Clone)]
pub struct LoadedSpec {
    pub spec: SweepSpec,
    /// Path, text and design tables of the file, as a manifest records them.
    pub spec_source: SpecSource,
    /// The file's `[execution]` table, which [`SweepOptions::apply_execution`] applies.
    ///
    /// [`SweepOptions::apply_execution`]: crate::sweep::SweepOptions::apply_execution
    pub execution: ExecutionTable,
}

impl LoadedSpec {
    /// Reads the file at `path`, reading a table design's file relative to it.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`SpecFile::load`] and [`SpecFile::into_spec`].
    pub fn read(path: &Path) -> Result<Self, SpecFileError> {
        let (file, text) = SpecFile::load(path)?;
        let spec_source = SpecSource::loaded(path, text, &file);
        Self::from_file(file, spec_source)
    }

    /// Reads spec text that names no table file.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`SpecFile::parse`] and [`SpecFile::into_spec`]. A block that names a table `file` is
    /// refused, since no file is read. A table design given inline as `table_text` is read.
    pub fn parse(text: &str) -> Result<Self, SpecFileError> {
        let file = SpecFile::parse(text)?;
        let spec_source = SpecSource {
            path: None,
            toml: Some(text.to_owned()),
            tables: Vec::new(),
        };
        Self::from_file(file, spec_source)
    }

    /// Writes the spec and its execution table as spec-file text, as the app's Save spec does.
    ///
    /// A table design's rows are written inline, as a manifest records them.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Write`] when the spec cannot be written as TOML.
    pub fn to_toml(&self) -> Result<String, SpecFileError> {
        let mut file = SpecFile::from(&self.spec);
        file.execution = self.execution;
        file.to_toml().map_err(|source| SpecFileError::Write { source })
    }

    fn from_file(file: SpecFile, spec_source: SpecSource) -> Result<Self, SpecFileError> {
        let execution = file.execution;
        Ok(Self {
            spec: file.into_spec()?,
            spec_source,
            execution,
        })
    }
}

impl SpecFile {
    /// Reads the spec file at `path`, and the design tables its blocks name, and returns it with its text as
    /// written.
    ///
    /// A table's path is relative to the directory of `path` and cannot leave it. Only a table design's `file` is read.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Read`] when the file or a table cannot be read, [`SpecFileError::TablePath`] for a
    /// table path that is absolute or holds a component other than a name, and the errors of [`Self::parse`].
    pub fn load(path: &Path) -> Result<(Self, String), SpecFileError> {
        let read = |path: &Path| {
            std::fs::read_to_string(path).map_err(|source| SpecFileError::Read {
                path: path.to_owned(),
                source,
            })
        };
        let text = read(path)?;
        let mut file = Self::parse(&text)?;
        let directory = path.parent().unwrap_or_else(|| Path::new(""));
        for (index, block) in file.blocks.iter_mut().enumerate() {
            // Any other design refuses a file in `into_spec`. Read here, a missing file would be reported in its place.
            let Some(table) = block.file.as_ref().filter(|_| block.design == DesignKindFile::Table) else {
                continue;
            };
            if table
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
            {
                return Err(SpecFileError::TablePath {
                    block: index,
                    path: table.clone(),
                });
            }
            block.file_text = Some(read(&directory.join(table))?);
        }
        Ok((file, text))
    }

    /// Returns the design tables [`Self::load`] read, in block order.
    pub fn tables(&self) -> Vec<DesignTableFile> {
        self.blocks
            .iter()
            .filter_map(|block| {
                Some(DesignTableFile {
                    path: block.file.clone()?,
                    fnv1a64: fnv1a64(block.file_text.as_ref()?.as_bytes()),
                })
            })
            .collect()
    }

    /// Reads a spec file from its text.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Parse`] for text that is not TOML, or a key or value no table takes.
    pub fn parse(text: &str) -> Result<Self, SpecFileError> {
        toml::from_str(text).map_err(|source| SpecFileError::Parse {
            source: Box::new(source),
        })
    }

    /// Reads a spec file from `value`, the JSON form a manifest records.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError::Json`] for a value no table takes.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, SpecFileError> {
        Self::deserialize(value).map_err(|source| SpecFileError::Json { source })
    }

    /// Returns the file as TOML text.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error. Every field of a spec file has a TOML form, so none is expected.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }

    /// Returns the sweep spec the file writes, leaving its execution settings behind.
    ///
    /// # Errors
    ///
    /// Returns [`SpecFileError`] for a factor without exactly one target or one form of levels, block keys that do
    /// not fit the design, a table that was never read, a reducer kind, stop condition or timeout that cannot be
    /// read, or a `[search]` table beside blocks or with settings of another algorithm. Everything else is checked
    /// when the spec is planned.
    pub fn into_spec(self) -> Result<SweepSpec, SpecFileError> {
        let mut reducers = Vec::new();
        for reducer in self.measure.reducers {
            for kind in &reducer.kinds {
                reducers.push(ReducerSpec {
                    column: reducer.column.clone(),
                    kind: kind.parse().map_err(SpecFileError::Reducer)?,
                });
            }
        }
        if self.search.is_some() && !self.blocks.is_empty() {
            return Err(SpecFileError::Search {
                reason: "a search selects its own configs and takes no [[block]] tables",
            });
        }
        let blocks = self
            .blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| block.into_spec(index))
            .collect::<Result<_, _>>()?;
        let search = self.search.map(SearchTable::into_spec).transpose()?;
        let stats_every = self.measure.stats_every;
        Ok(SweepSpec {
            model: self.model,
            fixed: self.set.into_iter().map(|(id, value)| (id, value.to_text())).collect(),
            run: self.run.into_settings()?,
            measure: MeasureSettings {
                stats_every,
                series_every: self.measure.series_every.unwrap_or(stats_every),
                default_reducers: self.measure.default_reducers,
                reducers,
            },
            seeds: SeedSettings {
                root: self.seeds.root.0,
                scheme: self.seeds.scheme.into(),
            },
            actions: self.actions.into_iter().map(ActionSpec::from).collect(),
            blocks,
            search,
        })
    }
}

impl BlockTable {
    fn into_spec(self, index: usize) -> Result<BlockSpec, SpecFileError> {
        let refuse = |reason| Err(SpecFileError::Design { block: index, reason });
        let sampled = matches!(self.design, DesignKindFile::Random | DesignKindFile::Lhs);
        if !sampled && self.samples.is_some() {
            return refuse("samples applies to a random or lhs design");
        }
        if !sampled && self.design_seed.is_some() {
            return refuse("design_seed applies to a random or lhs design");
        }
        if self.design != DesignKindFile::Table && (self.file.is_some() || self.table_text.is_some()) {
            return refuse("file and table_text apply to a table design");
        }
        let design = match (self.design, self.samples) {
            (DesignKindFile::Factorial, _) => DesignKind::Factorial,
            (DesignKindFile::Zip, _) => DesignKind::Zip,
            (DesignKindFile::Random, Some(samples)) => DesignKind::Random { samples },
            (DesignKindFile::Lhs, Some(samples)) => DesignKind::LatinHypercube { samples },
            (DesignKindFile::Random | DesignKindFile::Lhs, None) => {
                return refuse("a random or lhs design needs samples");
            }
            (DesignKindFile::Table, _) => match (self.file, self.file_text, self.table_text) {
                (None, _, Some(text)) | (Some(_), Some(text), None) => DesignKind::Table { text },
                (Some(_), _, Some(_)) => return refuse("a table design takes one of file and table_text"),
                (Some(_), None, None) => {
                    return refuse("a table file needs a spec file loaded from disk");
                }
                (None, _, None) => return refuse("a table design needs a file or a table_text"),
            },
        };
        let factors = self
            .factors
            .into_iter()
            .map(|factor| {
                factor.into_spec().map_err(|error| match error {
                    FactorTableError::Target => SpecFileError::FactorTarget { block: index },
                    FactorTableError::Levels { target_name } => SpecFileError::FactorLevels {
                        block: index,
                        target_name,
                    },
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(BlockSpec {
            design,
            factors,
            design_seed: self.design_seed.map(|seed| seed.0),
        })
    }
}

impl From<&SweepSpec> for SpecFile {
    /// Writes each value as [`SpecValue::from_text`] reads it, and each consecutive group of reducers over one
    /// column as one entry.
    fn from(spec: &SweepSpec) -> Self {
        let mut reducers: Vec<ReducerTable> = Vec::new();
        for reducer in &spec.measure.reducers {
            match reducers.last_mut() {
                Some(last) if last.column == reducer.column => last.kinds.push(reducer.kind.to_string()),
                _ => reducers.push(ReducerTable {
                    column: reducer.column.clone(),
                    kinds: vec![reducer.kind.to_string()],
                }),
            }
        }
        Self {
            model: spec.model.clone(),
            set: spec
                .fixed
                .iter()
                .map(|(id, value)| (id.clone(), SpecValue::from_text(value)))
                .collect(),
            run: (&spec.run).into(),
            measure: MeasureTable {
                stats_every: spec.measure.stats_every,
                series_every: Some(spec.measure.series_every),
                default_reducers: spec.measure.default_reducers,
                reducers,
            },
            seeds: SeedsTable {
                root: TomlSeed(spec.seeds.root),
                scheme: spec.seeds.scheme.into(),
            },
            actions: spec.actions.iter().map(ActionTable::from).collect(),
            blocks: spec.blocks.iter().map(BlockTable::from).collect(),
            search: spec.search.as_ref().map(SearchTable::from),
            execution: ExecutionTable::default(),
        }
    }
}

impl From<&BlockSpec> for BlockTable {
    /// Writes a table design's text in [`BlockTable::table_text`], and a design seed for a random or Latin hypercube
    /// design alone.
    fn from(block: &BlockSpec) -> Self {
        let factors = block.factors.iter().map(FactorTable::from).collect();
        let (samples, table_text) = match &block.design {
            DesignKind::Random { samples } | DesignKind::LatinHypercube { samples } => (Some(*samples), None),
            DesignKind::Table { text } => (None, Some(text.clone())),
            DesignKind::Factorial | DesignKind::Zip => (None, None),
        };
        Self {
            design: (&block.design).into(),
            samples,
            design_seed: block.design_seed.filter(|_| block.design.is_sampled()).map(TomlSeed),
            file: None,
            table_text,
            factors,
            file_text: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::Path;

    use std::time::Duration;

    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::fingerprint::fnv1a64;
    use henad_core::explore::measure::MeasureError;
    use henad_core::explore::plan::PlanError;
    use henad_core::explore::reducer::{ReducerError, ReducerKind, ReducerSpec};
    use henad_core::explore::seed::SeedScheme;
    use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
    use henad_core::explore::stop::StopSpec;

    use super::{DesignTableFile, ExecutionTable, SpecFile, SpecFileError, SpecValue};
    use crate::exec::Concurrency;
    use crate::tests::support::ScratchDir;

    const SWEEP: &str = r#"
model = "sir"

[set]
grid_width = 128
grid_height = 64
neighborhood = "von_neumann"

[run]
steps = 500
warmup = 20
replicates = 8

[measure]
stats_every = 5
series_every = 25
default_reducers = false
reducers = [{ column = "Infected", kinds = ["max", "mean"] }, { column = "Recovered", kinds = ["final"] }]

[seeds]
root = 42
scheme = "independent"

[[block]]
design = "factorial"
factors = [
  { param = "infection_rate", range = { min = 0.1, max = 0.5, step = 0.1 } },
  { param = "recovery_rate", values = [0.02, 0.05, 1] },
]

[[block]]
design = "zip"
factors = [
  { param = "wrap", levels = "all" },
  { param = "initial_infected", range = { min = 1, max = 2 } },
]

[execution]
concurrent = 4
memory = 1000000
"#;

    fn text(values: &[&str]) -> LevelSpec {
        LevelSpec::Values(values.iter().map(|&value| value.to_owned()).collect())
    }

    fn sweep() -> SweepSpec {
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![
            ("grid_height".to_owned(), "64".to_owned()),
            ("grid_width".to_owned(), "128".to_owned()),
            ("neighborhood".to_owned(), "von_neumann".to_owned()),
        ];
        spec.run.steps = 500;
        spec.run.warmup = 20;
        spec.run.replicates = 8;
        spec.measure.stats_every = 5;
        spec.measure.series_every = 25;
        spec.measure.default_reducers = false;
        spec.measure.reducers = vec![
            ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::Max,
            },
            ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::Mean,
            },
            ReducerSpec {
                column: "Recovered".to_owned(),
                kind: ReducerKind::Final,
            },
        ];
        spec.seeds.root = 42;
        spec.seeds.scheme = SeedScheme::Independent;
        spec.blocks = vec![
            BlockSpec {
                design: DesignKind::Factorial,
                factors: vec![
                    FactorSpec::param(
                        "infection_rate",
                        LevelSpec::Range {
                            min: 0.1,
                            max: 0.5,
                            step: Some(0.1),
                        },
                    ),
                    FactorSpec::param("recovery_rate", text(&["0.02", "0.05", "1"])),
                ],
                design_seed: None,
            },
            BlockSpec {
                design: DesignKind::Zip,
                factors: vec![
                    FactorSpec::param("wrap", LevelSpec::All),
                    FactorSpec::param(
                        "initial_infected",
                        LevelSpec::Range {
                            min: 1.0,
                            max: 2.0,
                            step: None,
                        },
                    ),
                ],
                design_seed: None,
            },
        ];
        spec
    }

    fn parse(text: &str) -> Result<SpecFile, SpecFileError> {
        SpecFile::parse(text)
    }

    #[test]
    fn a_spec_file_reads_as_the_sweep_it_writes() {
        let file = parse(SWEEP).expect("a valid spec file");
        assert_eq!(
            file.execution.concurrent,
            Concurrency::Fixed(NonZeroUsize::new(4).expect("4 is above 0"))
        );
        assert_eq!(file.execution.memory, Some(1_000_000));
        assert_eq!(file.into_spec().expect("every factor has one form of levels"), sweep());
    }

    #[test]
    fn list_values_reach_the_parser_as_command_line_text() {
        let file = parse(
            r#"
model = "sir"
[set]
a = 0.1
b = 3
c = true
d = "moore"
e = 1e-3
"#,
        )
        .expect("a valid spec file");
        let spec = file.into_spec().expect("a valid spec file");
        let values: Vec<&str> = spec.fixed.iter().map(|(_, value)| value.as_str()).collect();
        assert_eq!(values, ["0.1", "3", "true", "moore", "0.001"]);
    }

    #[test]
    fn a_missing_table_takes_the_defaults() {
        let spec = parse("model = \"sir\"\n[measure]\nstats_every = 4\n")
            .and_then(SpecFile::into_spec)
            .expect("a valid spec file");
        let mut expected = SweepSpec::new("sir");
        expected.measure.stats_every = 4;
        expected.measure.series_every = 4;
        assert_eq!(spec, expected, "series_every follows stats_every");
    }

    #[test]
    fn an_unknown_key_is_refused() {
        for text in [
            "model = \"sir\"\nstep = 3\n",
            "model = \"sir\"\n[run]\nstep = 3\n",
            "model = \"sir\"\n[seeds]\nseed = 3\n",
            "model = \"sir\"\n[measure]\nreducers = [{ column = \"Infected\", kind = \"max\" }]\n",
            "model = \"sir\"\n[[block]]\nfactors = [{ param = \"a\", values = [1], step = 2 }]\n",
            "model = \"sir\"\n[[block]]\nfactors = [{ param = \"a\", range = { from = 0, to = 1 } }]\n",
            "model = \"sir\"\n[execution]\nthreads = 3\n",
        ] {
            let error = parse(text).expect_err("an unknown key");
            assert!(matches!(error, SpecFileError::Parse { .. }), "{text}");
        }
    }

    #[test]
    fn a_bad_value_is_refused() {
        for text in [
            "model = \"sir\"\n[[block]]\ndesign = \"latin\"\n",
            "model = \"sir\"\n[[block]]\nfactors = [{ param = \"a\", levels = \"some\" }]\n",
            "model = \"sir\"\n[execution]\nconcurrent = 0\n",
            "model = \"sir\"\n[seeds]\nroot = -1\n",
            "model = \"sir\"\n[set]\na = [1, 2]\n",
            "model = \"sir\"\n[run]\nsteps = -5\n",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_factor_needs_exactly_one_form_of_levels() {
        for factor in [
            "{ param = \"a\" }",
            "{ param = \"a\", values = [1], levels = \"all\" }",
            "{ param = \"a\", values = [1], range = { min = 0, max = 1 } }",
        ] {
            let text = format!("model = \"sir\"\n[[block]]\nfactors = [{factor}]\n");
            let error = parse(&text)
                .and_then(SpecFile::into_spec)
                .expect_err("not exactly one form");
            assert!(
                matches!(error, SpecFileError::FactorLevels { block: 0, .. }),
                "{factor} gave {error:?}"
            );
        }
        let error = parse("model = \"sir\"\n[measure]\nreducers = [{ column = \"Infected\", kinds = [\"median\"] }]\n")
            .and_then(SpecFile::into_spec)
            .expect_err("no such reducer kind");
        assert!(matches!(error, SpecFileError::Reducer(_)), "{error:?}");
    }

    #[test]
    fn every_example_spec_parses_and_plans() {
        let specs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("specs");
        let models = henad_models::example_models();
        let mut planned = Vec::new();
        for file in std::fs::read_dir(&specs).expect("the specs directory exists") {
            let path = file.expect("the directory lists").path();
            if path.extension().is_none_or(|extension| extension != "toml") {
                continue;
            }
            let (file, _) = SpecFile::load(&path).expect("an example spec parses");
            let spec = file.into_spec().expect("an example spec is a sweep spec");
            // A search plans through its own test, every_example_search_spec_parses.
            if spec.search.is_some() {
                continue;
            }
            let entry = models
                .get(&spec.model)
                .expect("an example spec names a registered model");
            let plan = spec
                .plan(&entry.schema())
                .unwrap_or_else(|error| panic!("{} does not plan: {error:?}", path.display()));
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
            planned.push((name.unwrap_or_default(), plan.configs().len(), plan.run_count()));
        }
        for counts in [
            ("sir_sweep.toml".to_owned(), 88, 704),
            ("sir_table.toml".to_owned(), 4, 16),
        ] {
            assert!(
                planned.contains(&counts),
                "the counts its header comment gives: {planned:?}"
            );
        }
    }

    #[test]
    fn a_seed_past_the_largest_toml_integer_is_a_string() {
        let spec = parse("model = \"sir\"\n[seeds]\nroot = \"18446744073709551615\"\n")
            .and_then(SpecFile::into_spec)
            .expect("a valid seed");
        assert_eq!(spec.seeds.root, u64::MAX);
        let file = SpecFile::from(&spec);
        let text = file.to_toml().expect("a spec file serializes");
        assert!(text.contains("root = \"18446744073709551615\""), "{text}");
        let small = parse("model = \"sir\"\n[seeds]\nroot = \"7\"\n")
            .and_then(SpecFile::into_spec)
            .expect("a valid seed");
        assert_eq!(small.seeds.root, 7);
    }

    #[test]
    fn a_spec_survives_a_round_trip_through_toml_and_json() {
        let spec = sweep();
        let text = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let file = parse(&text).expect("the written file reads back");
        assert_eq!(file.clone().into_spec().expect("a valid spec file"), spec, "{text}");
        let json = serde_json::to_value(&file).expect("a spec file serializes");
        assert_eq!(
            json["set"]["grid_width"],
            serde_json::json!(128),
            "a number stays a number"
        );
        assert_eq!(json["block"][0]["factors"][1]["values"][0], serde_json::json!(0.02));
        let back: SpecFile = serde_json::from_value(json).expect("the JSON reads back");
        assert_eq!(back, file);
    }

    /// Checks that every spec the plan accepts reads back from the file its manifest records, and that the plan
    /// refuses the two that cannot.
    #[test]
    fn a_spec_that_plans_reads_back_from_its_file() {
        let models = henad_models::example_models();
        let schema = models.get("sir").expect("the example set holds SIR").schema();

        let mut stop = sweep();
        stop.run.stop = Some(StopSpec::parse("Agents (k=3) <= 0.5", 4).expect("a well-formed condition"));
        let text = SpecFile::from(&stop).to_toml().expect("a spec file serializes");
        let back = parse(&text).and_then(SpecFile::into_spec).expect("the stop reads back");
        assert_eq!(back, stop, "{text}");

        let mut window = SweepSpec::new("sir");
        window.measure.reducers.push(ReducerSpec {
            column: "Infected".to_owned(),
            kind: ReducerKind::WindowMean { start: 600, end: 200 },
        });
        assert!(
            SpecFile::from(&window).into_spec().is_err(),
            "a reversed window does not read back"
        );
        assert!(
            matches!(
                window.plan(&schema),
                Err(PlanError::Measure(MeasureError::Reducer(
                    ReducerError::BadWindow { .. }
                )))
            ),
            "the plan refuses a reversed window"
        );

        let mut timeout = SweepSpec::new("sir");
        timeout.run.timeout = Some(Duration::MAX);
        assert!(
            SpecFile::from(&timeout).into_spec().is_err(),
            "the longest timeout does not read back"
        );
        assert!(
            matches!(
                timeout.plan(&schema),
                Err(PlanError::Measure(MeasureError::TimeoutTooLong { .. }))
            ),
            "the plan refuses the longest timeout"
        );
        timeout.run.timeout = Some(Duration::from_secs(1 << 62));
        let back = SpecFile::from(&timeout).into_spec().expect("a long timeout reads back");
        assert_eq!(back.run.timeout, timeout.run.timeout);
        let planned = timeout.plan(&schema);
        assert!(planned.is_ok(), "the plan takes a timeout that reads back: {planned:?}");
    }

    #[test]
    fn text_becomes_a_typed_value_only_when_it_writes_back_unchanged() {
        assert_eq!(SpecValue::from_text("128"), SpecValue::Integer(128));
        assert_eq!(SpecValue::from_text("0.25"), SpecValue::Float(0.25));
        assert_eq!(SpecValue::from_text("true"), SpecValue::Bool(true));
        for text in ["007", "0.10", "1e-3", "inf", "NaN", "moore", "+5"] {
            assert_eq!(SpecValue::from_text(text), SpecValue::Text(text.to_owned()), "{text}");
            assert_eq!(SpecValue::from_text(text).to_text(), text);
        }
        assert_eq!(ExecutionTable::default().concurrent, Concurrency::Auto);
    }

    const ACTIONS_AND_SAMPLED_DESIGNS: &str = r#"
model = "sir"

[run]
steps = 800
timeout_s = 1.5
stop = { condition = "Infected <= 0", min_tick = 20 }

[measure]
reducers = [{ column = "Infected", kinds = ["argmax", "first<=10", "mean@200..600"] }]

[[action]]
id = "seed_outbreak"
tick = 400

[[action]]
id = "seed_outbreak"
name = "second_wave"
tick = 500

[[block]]
design = "lhs"
samples = 40
design_seed = 7
factors = [
  { param = "infection_rate", range = { min = 0.05, max = 0.95 } },
  { action = "second_wave", range = { min = 200, max = 600 } },
]

[[block]]
design = "random"
samples = 3
factors = [{ action = "seed_outbreak", values = [100, 200] }]
"#;

    #[test]
    fn a_spec_file_reads_actions_stops_and_sampled_designs() {
        let spec = parse(ACTIONS_AND_SAMPLED_DESIGNS)
            .and_then(SpecFile::into_spec)
            .expect("a valid spec file");
        let stop = spec.run.stop.as_ref().expect("the run stops");
        assert_eq!((stop.to_string().as_str(), stop.min_tick), ("Infected <= 0", 20));
        assert_eq!(spec.run.timeout, Some(Duration::from_millis(1500)));
        let kinds: Vec<String> = spec
            .measure
            .reducers
            .iter()
            .map(|reducer| reducer.kind.to_string())
            .collect();
        assert_eq!(kinds, ["argmax", "first<=10", "mean@200..600"]);
        assert_eq!(
            spec.actions,
            [
                ActionSpec::new("seed_outbreak", 400),
                ActionSpec {
                    name: "second_wave".to_owned(),
                    ..ActionSpec::new("seed_outbreak", 500)
                },
            ]
        );
        assert_eq!(spec.blocks[0].design, DesignKind::LatinHypercube { samples: 40 });
        assert_eq!(spec.blocks[0].design_seed, Some(7));
        assert_eq!(
            spec.blocks[0].factors[1],
            FactorSpec::action(
                "second_wave",
                LevelSpec::Range {
                    min: 200.0,
                    max: 600.0,
                    step: None
                }
            )
        );
        assert_eq!(spec.blocks[1].design, DesignKind::Random { samples: 3 });

        let text = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let back = parse(&text)
            .and_then(SpecFile::into_spec)
            .expect("the written file reads back");
        assert_eq!(back, spec, "{text}");
        let models = henad_models::example_models();
        let sir = models.get("sir").expect("sir is registered");
        let plan = spec.plan(&sir.schema()).expect("the spec plans");
        assert_eq!(plan.configs().len(), 43);
    }

    #[test]
    fn a_table_design_is_read_beside_its_spec_file() {
        let directory = std::env::temp_dir().join(format!("henad-spec-table-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let spec_path = directory.join("sweep.toml");
        std::fs::write(
            &spec_path,
            "model = \"sir\"\n[[block]]\ndesign = \"table\"\nfile = \"design.csv\"\n",
        )
        .expect("the spec writes");
        std::fs::write(directory.join("design.csv"), "infection_rate\n0.2\n0.4\n").expect("the table writes");
        let (file, _) = SpecFile::load(&spec_path).expect("the spec and its table read");
        std::fs::remove_dir_all(&directory).expect("the scratch directory goes");
        assert_eq!(
            file.tables(),
            [DesignTableFile {
                path: "design.csv".into(),
                fnv1a64: fnv1a64(b"infection_rate\n0.2\n0.4\n"),
            }]
        );
        let spec = file.into_spec().expect("a valid spec file");
        assert_eq!(
            spec.blocks[0].design,
            DesignKind::Table {
                text: "infection_rate\n0.2\n0.4\n".to_owned()
            }
        );
        let text = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let inline = parse(&text)
            .and_then(SpecFile::into_spec)
            .expect("the table is written in the spec");
        assert_eq!(inline, spec, "{text}");
        let unread = parse("model = \"sir\"\n[[block]]\ndesign = \"table\"\nfile = \"design.csv\"\n")
            .and_then(SpecFile::into_spec)
            .expect_err("a table read from nowhere");
        assert!(matches!(unread, SpecFileError::Design { block: 0, .. }), "{unread:?}");
    }

    #[test]
    fn a_table_path_cannot_leave_the_spec_directory() {
        let scratch = ScratchDir::new("spec-table-path");
        let specs = scratch.path().join("specs");
        std::fs::create_dir_all(&specs).expect("a scratch directory");
        let table = scratch.path().join("design.csv");
        std::fs::write(&table, "infection_rate\n0.2\n").expect("the table writes");
        let spec_path = specs.join("sweep.toml");
        std::fs::write(specs.join("design.csv"), "infection_rate\n0.2\n").expect("the table writes");
        std::fs::write(
            &spec_path,
            "model = \"sir\"\n[[block]]\ndesign = \"table\"\nfile = './design.csv'\n",
        )
        .expect("the spec writes");
        SpecFile::load(&spec_path).expect("a table beside the spec, written with a leading './'");
        for path in [
            "../design.csv",
            table.to_str().expect("a UTF-8 path"),
            "tables/../../design.csv",
        ] {
            let text = format!("model = \"sir\"\n[[block]]\ndesign = \"table\"\nfile = '{path}'\n");
            std::fs::write(&spec_path, text).expect("the spec writes");
            let error = SpecFile::load(&spec_path).expect_err("a table outside the spec's directory");
            let SpecFileError::TablePath {
                block: 0,
                path: refused,
            } = &error
            else {
                panic!("{path} gave {error:?}");
            };
            assert_eq!(refused, Path::new(path));
            assert!(error.to_string().contains(path), "{error}");
        }
    }

    #[test]
    fn a_file_on_another_design_is_refused_without_reading_it() {
        let scratch = ScratchDir::new("spec-stray-file");
        std::fs::create_dir_all(scratch.path()).expect("a scratch directory");
        let spec_path = scratch.path().join("sweep.toml");
        for path in ["missing.csv", "../missing.csv"] {
            let text = format!("model = \"sir\"\n[[block]]\ndesign = \"factorial\"\nfile = '{path}'\n");
            std::fs::write(&spec_path, text).expect("the spec writes");
            let (file, _) = SpecFile::load(&spec_path).expect("a factorial design reads no file");
            assert_eq!(file.tables(), [], "{path}");
            let error = file.into_spec().expect_err("a file on a factorial design");
            assert!(
                matches!(error, SpecFileError::Design { block: 0, .. }),
                "{path} gave {error:?}"
            );
        }
    }

    #[test]
    fn a_design_seed_is_written_for_a_sampled_design_alone() {
        let mut spec = SweepSpec::new("sir");
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param("infection_rate", text(&["0.2", "0.4"]))],
            design_seed: Some(9),
        }];
        let text = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let back = parse(&text)
            .and_then(SpecFile::into_spec)
            .expect("the written file reads back");
        assert_eq!(back.blocks[0].design_seed, None, "{text}");
        let models = henad_models::example_models();
        let sir = models.get("sir").expect("sir is registered");
        let plan_hash = |spec: &SweepSpec| spec.plan(&sir.schema()).expect("the spec plans").plan_hash();
        assert_eq!(plan_hash(&back), plan_hash(&spec), "a factorial design draws nothing");
    }

    #[test]
    fn block_keys_must_fit_the_design() {
        for block in [
            "design = \"lhs\"",
            "design = \"random\"",
            "design = \"table\"",
            "design = \"zip\"\nsamples = 4",
            "design = \"factorial\"\ndesign_seed = 4",
            "design = \"lhs\"\nsamples = 4\nfile = \"design.csv\"",
            "design = \"zip\"\ntable_text = \"a\\n1\\n\"",
            "design = \"table\"\nfile = \"design.csv\"\ntable_text = \"a\\n1\\n\"",
        ] {
            let text = format!("model = \"sir\"\n[[block]]\n{block}\n");
            let error = parse(&text)
                .and_then(SpecFile::into_spec)
                .expect_err("keys that do not fit");
            assert!(
                matches!(error, SpecFileError::Design { block: 0, .. }),
                "{block} gave {error:?}"
            );
        }
        for factor in ["{ values = [1] }", "{ param = \"a\", action = \"b\", values = [1] }"] {
            let text = format!("model = \"sir\"\n[[block]]\nfactors = [{factor}]\n");
            let error = parse(&text)
                .and_then(SpecFile::into_spec)
                .expect_err("no single target");
            assert!(
                matches!(error, SpecFileError::FactorTarget { block: 0 }),
                "{factor} gave {error:?}"
            );
        }
        for run in ["stop = { condition = \"Infected\" }", "timeout_s = -1.0"] {
            let text = format!("model = \"sir\"\n[run]\n{run}\n");
            let error = parse(&text)
                .and_then(SpecFile::into_spec)
                .expect_err("an unreadable run table");
            assert!(
                matches!(error, SpecFileError::Stop(_) | SpecFileError::Timeout { .. }),
                "{run} gave {error:?}"
            );
        }
    }

    const SEARCH: &str = r#"
model = "sir"

[[action]]
id = "seed_outbreak"
tick = 50

[search]
algorithm = "hill_climb"
max_evaluations = 40
batch_size = 8
objective = { column = "Infected:max", goal = "maximize" }
space = [
  { param = "infection_rate", range = { min = 0.1, max = 0.9 } },
  { action = "seed_outbreak", values = [10, 20, 30] },
]

[search.hill_climb]
patience = 3
"#;

    #[test]
    fn a_search_table_reads_as_the_search_it_writes() {
        use henad_core::explore::search::hill_climb::HillClimbSettings;
        use henad_core::explore::search::{Aggregate, Goal, SearchAlgorithm};

        let spec = parse(SEARCH).and_then(SpecFile::into_spec).expect("a valid search");
        let search = spec.search.as_ref().expect("the spec is a search");
        assert_eq!(
            search.algorithm,
            SearchAlgorithm::HillClimb(HillClimbSettings {
                patience: 3,
                ..HillClimbSettings::default()
            }),
            "a left-out setting takes its default"
        );
        assert_eq!((search.max_evaluations, search.batch_size), (40, 8));
        let objective = search.objective.as_ref().expect("a hill climb has an objective");
        assert_eq!(
            (objective.goal, objective.aggregate),
            (Goal::Maximize, Aggregate::Median)
        );
        assert_eq!(
            search.space[1],
            FactorSpec::action("seed_outbreak", text(&["10", "20", "30"]))
        );
        let written = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let back = parse(&written)
            .and_then(SpecFile::into_spec)
            .expect("the written file reads back");
        assert_eq!(back, spec, "{written}");
    }

    #[test]
    fn search_keys_must_fit_the_algorithm() {
        let with = |extra: &str| format!("{SEARCH}{extra}");
        for (text, reason) in [
            (
                with("[search.genetic]\npopulation = 4\n"),
                "a genetic table on a hill climb",
            ),
            (
                SEARCH
                    .replace("hill_climb\"\n", "pse\"\n")
                    .replace("[search.hill_climb]\npatience = 3\n", ""),
                "a pse search without its axes",
            ),
            (with("[[block]]\nfactors = []\n"), "blocks beside a search"),
        ] {
            let error = parse(&text).and_then(SpecFile::into_spec).expect_err(reason);
            assert!(matches!(error, SpecFileError::Search { .. }), "{reason}: {error:?}");
        }
        for text in [
            with("[search.pse]\nx_axis = { column = \"a\", min = 0, max = 1, cells = 2, width = 3 }\n"),
            SEARCH.replace("patience = 3", "patient = 3"),
            SEARCH.replace("batch_size = 8", "batch = 8"),
            SEARCH.replace("goal = \"maximize\"", "goal = \"max\""),
        ] {
            let error = parse(&text).expect_err("an unknown key or value");
            assert!(matches!(error, SpecFileError::Parse { .. }), "{text}");
        }
        let error = parse(&SEARCH.replace("{ action = \"seed_outbreak\", values", "{ values"))
            .and_then(SpecFile::into_spec)
            .expect_err("a factor with no target");
        assert!(matches!(error, SpecFileError::SearchFactorTarget), "{error:?}");
    }

    #[test]
    fn a_pse_axis_takes_both_bounds_or_neither() {
        use henad_core::explore::search::SearchAlgorithm;
        use henad_core::explore::search::pse::PatternAxis;

        let pse = |x_axis: &str| {
            SEARCH
                .replace("hill_climb\"\n", "pse\"\n")
                .replace("objective = { column = \"Infected:max\", goal = \"maximize\" }\n", "")
                .replace(
                    "[search.hill_climb]\npatience = 3\n",
                    &format!(
                        "[search.pse]\nx_axis = {x_axis}\ny_axis = {{ column = \"Infected:argmax\", min = 0, max = 50, cells = 10 }}\n"
                    ),
                )
        };
        let spec = parse(&pse("{ column = \"Infected:max\", cells = 16 }"))
            .and_then(SpecFile::into_spec)
            .expect("an automatic range reads");
        let search = spec.search.as_ref().expect("the spec is a search");
        let SearchAlgorithm::PatternSpaceExploration(settings) = &search.algorithm else {
            panic!("a pse search reads as one: {:?}", search.algorithm);
        };
        assert_eq!(settings.x_axis, PatternAxis::automatic("Infected:max", 16));
        assert_eq!(settings.y_axis, PatternAxis::bounded("Infected:argmax", 0.0, 50.0, 10));
        let written = SpecFile::from(&spec).to_toml().expect("a spec file serializes");
        let back = parse(&written)
            .and_then(SpecFile::into_spec)
            .expect("the written file reads back");
        assert_eq!(back, spec, "{written}");

        for (x_axis, bound) in [
            ("{ column = \"Infected:max\", min = 0, cells = 16 }", "min"),
            ("{ column = \"Infected:max\", max = 900, cells = 16 }", "max"),
        ] {
            let error = parse(&pse(x_axis))
                .and_then(SpecFile::into_spec)
                .expect_err("one bound alone");
            assert!(matches!(error, SpecFileError::Search { .. }), "{bound}: {error:?}");
            assert_eq!(
                error.to_string(),
                "search: x_axis needs both min and max, or neither for an automatic range",
                "{bound}"
            );
        }
    }
}
