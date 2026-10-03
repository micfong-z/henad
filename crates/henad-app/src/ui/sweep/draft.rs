//! Sweep drafts, the sweep or search the Sweep tab edits, and their conversion to and from a sweep spec.
//!
//! A draft keeps each field as typed, so text that does not parse yet survives the next frame. Every parameter a
//! draft does not vary takes its Parameters tab value. The draft reads those values each time it writes a spec.

use std::borrow::Cow;
use std::error::Error;
use std::time::Duration;

use henad_core::explore::design::{DesignError, DesignKind};
use henad_core::explore::factor::{
    Factor, FactorDomain, FactorError, FactorLevel, FactorSpec, FactorTarget, LevelSpec, LevelSpecError,
};
use henad_core::explore::measure::{MeasureError, MeasurePlan};
use henad_core::explore::plan::{ModelSchema, Plan, PlanError};
use henad_core::explore::reducer::{ReducerError, ReducerKind, ReducerPlan, ReducerSpec};
use henad_core::explore::search::genetic::GeneticSettings;
use henad_core::explore::search::genome::SearchSpaceError;
use henad_core::explore::search::hill_climb::HillClimbSettings;
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec, SearchSpecError};
use henad_core::explore::seed::SeedScheme;
use henad_core::explore::spec::{
    ACTION_COLUMN_PREFIX, ActionSpec, BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec,
};
use henad_core::explore::stop::{Comparator, Comparison, StopSpec};
use henad_core::explore::value::{ValueError, format_value, parse_value};
use henad_core::export::StatColumns;
use henad_core::export::csv::parse_records;
use henad_core::helpers::fmt_bytes;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_core::view::{StatEntry, StatValue};
use henad_explore::exec::Concurrency;
use henad_explore::search_run::{SearchPlan, SearchPlanError};
use henad_explore::spec_file::{ExecutionTable, LoadedSpec, SpecFile};
use henad_explore::sweep::SpecSource;

use crate::options::cli_phrase;
use crate::ui::params::display_value;
use crate::ui::plural;
use crate::ui::results::store::{action_label, output_label};

/// Most runs a sweep started from the app can have.
pub const MAX_DRAFT_RUNS: u64 = 1 << 20;

/// Most values one varied parameter or action tick can take in a draft.
pub const MAX_DRAFT_LEVELS: u64 = 1 << 20;

/// Most bytes of `series.csv` a sweep can hold in memory.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_MEMORY_SERIES_BYTES: u64 = 4 << 30;

/// Most bytes of `series.csv` a sweep can hold in memory. The web build has 4 GiB of memory in all.
#[cfg(target_arch = "wasm32")]
pub const MAX_MEMORY_SERIES_BYTES: u64 = 1 << 30;

/// Fewest seconds the Timeout field can be set to.
///
/// Note that a loaded spec can hold less, as `henad-cli` accepts it.
pub const MIN_TIMEOUT_SECONDS: f64 = 1.0;

/// Approximate bytes one value of `series.csv` takes as text.
const SERIES_VALUE_BYTES: u64 = 12;

/// Samples a sampled design draws until the user asks for another number.
const DEFAULT_SAMPLES: usize = 20;

/// Name a design table read from a spec file goes by.
const SPEC_TABLE_NAME: &str = "the spec";

/// Evaluations a search runs until the user asks for another number.
const DEFAULT_EVALUATIONS: u64 = 200;

/// Candidates a search evaluates at once until the user asks for another number.
const DEFAULT_BATCH_SIZE: usize = 16;

/// Cells of a Pattern Space Exploration axis until the user sets another number.
const DEFAULT_AXIS_CELLS: u32 = 20;

/// Most values of one parameter or action tick a preview formats.
pub const MAX_PREVIEW_VALUES: usize = 50;

/// Most times the check of a search method's settings repairs one setting and checks again, one per setting.
const MAX_SETTING_CHECKS: usize = 8;

/// Example of the ticks an action takes, as a hint and in the note of an empty row.
pub const TICKS_EXAMPLE: &str = "100, 200";

/// Key of the initial samples setting of a Pattern Space Exploration.
pub const INITIAL_SAMPLES_KEY: &str = "pse.initial_samples";

/// Issue of a grid axis with its own range that still spans 0 to 0, the range its fields start with.
pub const AXIS_RANGE_MISSING: &str = "Set Minimum and Maximum";

/// Issue of a stop condition or a crossing whose threshold is infinite or not a number.
const NOT_FINITE_THRESHOLD: &str = "Threshold must be a finite number";

/// Issue of a draft whose model has not reported its stat columns yet.
pub const COLUMNS_PENDING: &str = "Reading model outputs";

/// Output kinds the Objective and axis lists offer for each stat column, recorded or not.
const OFFERED_KINDS: [ReducerKind; 6] = [
    ReducerKind::Final,
    ReducerKind::Min,
    ReducerKind::Max,
    ReducerKind::Mean,
    ReducerKind::ArgMax,
    ReducerKind::ArgMin,
];

/// Kind of exploration a draft runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DraftMode {
    /// Every configuration of a design.
    #[default]
    Sweep,
    /// Configurations a search picks from the results of earlier ones.
    Search,
}

/// Search method of a draft.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DraftAlgorithm {
    #[default]
    Random,
    HillClimb,
    Genetic,
    PatternSpace,
}

impl DraftAlgorithm {
    /// Every method, in the order the Sweep tab lists them.
    pub const ALL: [Self; 4] = [Self::Random, Self::HillClimb, Self::Genetic, Self::PatternSpace];

    /// Returns whether the method scores candidates by an objective.
    ///
    /// A Pattern Space Exploration places them on a grid of two outputs instead.
    pub fn has_objective(self) -> bool {
        self != Self::PatternSpace
    }
}

impl From<&SearchAlgorithm> for DraftAlgorithm {
    /// Returns the method of `algorithm`, without its settings.
    fn from(algorithm: &SearchAlgorithm) -> Self {
        match algorithm {
            SearchAlgorithm::Random => Self::Random,
            SearchAlgorithm::HillClimb(_) => Self::HillClimb,
            SearchAlgorithm::Genetic(_) => Self::Genetic,
            SearchAlgorithm::PatternSpaceExploration(_) => Self::PatternSpace,
        }
    }
}

/// A search as the Sweep tab edits it.
///
/// The draft keeps the settings of every method, so switching the method loses none of them.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchDraft {
    pub algorithm: DraftAlgorithm,
    /// Evaluations the search runs, re-evaluations included.
    pub max_evaluations: u64,
    /// Most candidates evaluated at once.
    pub batch_size: usize,
    /// Output column the objective reads, as in `Infected:max`.
    pub objective_column: String,
    pub goal: Goal,
    /// Rule that folds the replicates of a candidate into its objective.
    pub aggregate: Aggregate,
    pub hill_climb: HillClimbSettings,
    pub genetic: GeneticSettings,
    pub pattern_space: PatternSpaceSettings,
    /// Minimum and maximum each axis held before its range was made automatic, the X axis's first.
    ///
    /// Unchecking Automatic range gives them back to the axis.
    pub remembered_ranges: [(f64, f64); 2],
}

impl SearchDraft {
    /// Returns a random search that maximizes the first stat's maximum, with every setting at its default.
    ///
    /// The axes of a Pattern Space Exploration start with an automatic range. Their bounds hold an empty range, from
    /// 0 to 0. The check refuses it once Automatic range is unchecked.
    pub fn new(schema: &ModelSchema<'_>) -> Self {
        let stat_output = |position: usize, kind: ReducerKind| {
            schema
                .stats
                .get(position)
                .or_else(|| schema.stats.first())
                .map(|stat| format!("{}:{kind}", stat.label))
                .unwrap_or_default()
        };
        let axis = |column: String| PatternAxis::automatic(column, DEFAULT_AXIS_CELLS);
        Self {
            algorithm: DraftAlgorithm::default(),
            max_evaluations: DEFAULT_EVALUATIONS,
            batch_size: DEFAULT_BATCH_SIZE,
            objective_column: stat_output(0, ReducerKind::Max),
            goal: Goal::Maximize,
            aggregate: Aggregate::default(),
            hill_climb: HillClimbSettings::default(),
            genetic: GeneticSettings::default(),
            pattern_space: PatternSpaceSettings::new(
                axis(stat_output(0, ReducerKind::Max)),
                axis(stat_output(1, ReducerKind::Max)),
            ),
            remembered_ranges: [(0.0, 0.0); 2],
        }
    }

    /// Returns grid axis `axis` of the Pattern Space Exploration, with the range it gets back from automatic.
    pub fn pattern_axis_mut(&mut self, axis: GridAxis) -> (&mut PatternAxis, &mut (f64, f64)) {
        let [x_range, y_range] = &mut self.remembered_ranges;
        match axis {
            GridAxis::X => (&mut self.pattern_space.x_axis, x_range),
            GridAxis::Y => (&mut self.pattern_space.y_axis, y_range),
        }
    }

    /// Makes the range of `axis` automatic, keeping the range its bounds held, or gives the axis that range back.
    pub fn set_automatic_range(&mut self, axis: GridAxis, automatic: bool) {
        let (pattern_axis, remembered) = self.pattern_axis_mut(axis);
        if automatic {
            if let Some(range) = pattern_axis.range() {
                *remembered = range;
            }
            pattern_axis.min = None;
            pattern_axis.max = None;
        } else if pattern_axis.is_automatic() {
            pattern_axis.min = Some(remembered.0);
            pattern_axis.max = Some(remembered.1);
        }
    }

    /// Returns the search spec of the draft over the factors `space`.
    fn to_spec(&self, space: Vec<FactorSpec>) -> SearchSpec {
        let algorithm = match self.algorithm {
            DraftAlgorithm::Random => SearchAlgorithm::Random,
            DraftAlgorithm::HillClimb => SearchAlgorithm::HillClimb(self.hill_climb),
            DraftAlgorithm::Genetic => SearchAlgorithm::Genetic(self.genetic),
            DraftAlgorithm::PatternSpace => SearchAlgorithm::PatternSpaceExploration(self.pattern_space.clone()),
        };
        SearchSpec {
            algorithm,
            max_evaluations: self.max_evaluations,
            batch_size: self.batch_size,
            objective: self.algorithm.has_objective().then(|| Objective {
                column: self.objective_column.clone(),
                goal: self.goal,
                aggregate: self.aggregate,
            }),
            space,
        }
    }

    /// Reads the method, budget, objective and settings of `search`. The settings of every other method keep their
    /// values.
    fn read_spec(&mut self, search: &SearchSpec) {
        self.max_evaluations = search.max_evaluations;
        self.batch_size = search.batch_size;
        if let Some(objective) = &search.objective {
            self.objective_column.clone_from(&objective.column);
            self.goal = objective.goal;
            self.aggregate = objective.aggregate;
        }
        self.algorithm = DraftAlgorithm::from(&search.algorithm);
        match &search.algorithm {
            SearchAlgorithm::Random => {}
            SearchAlgorithm::HillClimb(settings) => self.hill_climb = *settings,
            SearchAlgorithm::Genetic(settings) => self.genetic = *settings,
            SearchAlgorithm::PatternSpaceExploration(settings) => self.pattern_space = settings.clone(),
        }
    }

    /// Returns each output column the search reads, with the row that picks it.
    pub fn watched_columns(&self) -> Vec<(DraftSite, &str)> {
        if self.algorithm.has_objective() {
            vec![(DraftSite::Objective, self.objective_column.as_str())]
        } else {
            vec![
                (DraftSite::Axis(GridAxis::X), self.pattern_space.x_axis.column.as_str()),
                (DraftSite::Axis(GridAxis::Y), self.pattern_space.y_axis.column.as_str()),
            ]
        }
    }
}

/// Rule that combines the varied parameters of a draft into configurations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DraftDesign {
    /// Every combination of the listed values.
    #[default]
    EveryCombination,
    /// The first values of every parameter together, then the second, and so on.
    Zip,
    /// One block per varied parameter or action tick, every other varied one at its Parameters tab value or its
    /// action's tick.
    VaryEachAlone,
    LatinHypercube,
    UniformRandom,
    /// One configuration per row of a design table.
    Table,
}

impl DraftDesign {
    /// Every design, in the order the Sweep tab lists them.
    pub const ALL: [Self; 6] = [
        Self::EveryCombination,
        Self::Zip,
        Self::VaryEachAlone,
        Self::LatinHypercube,
        Self::UniformRandom,
        Self::Table,
    ];

    /// Returns whether the design draws its configurations from a design seed.
    pub fn is_sampled(self) -> bool {
        matches!(self, Self::LatinHypercube | Self::UniformRandom)
    }
}

/// One parameter of a draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactorDraft {
    pub vary: bool,
    /// Values as `--vary` takes them, read by [`LevelSpec::parse`]: `v1, v2`, `min:max:step`, `min:max` or `all`.
    pub levels_text: String,
}

/// An action a draft fires in every run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDraft {
    /// Index of the action in the model's declarations.
    pub action_index: usize,
    /// Name the spec gives the action, unique within the draft.
    pub name: String,
    /// Tick the action fires at, unless the tick varies.
    pub tick: u64,
    pub vary_tick: bool,
    /// Ticks the action fires at when its tick varies, in the text [`FactorDraft::levels_text`] takes.
    pub ticks_text: String,
}

impl ActionDraft {
    /// Returns the label `schema` declares for the action, with the number its name adds for a repeat, as in "Seed
    /// outbreak 2". An action the model does not declare goes by its name.
    pub fn label(&self, schema: &ModelSchema<'_>) -> String {
        schema.actions.get(self.action_index).map_or_else(
            || self.name.clone(),
            |declared| action_label(declared.label, declared.id, &self.name),
        )
    }
}

/// Condition that ends a run at the first sample where it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct StopDraft {
    /// Stat column the condition reads.
    pub column: String,
    pub comparator: Comparator,
    pub threshold: f64,
    /// First tick at which the condition can end a run.
    pub min_tick: u64,
}

/// A design table and the name of the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTableDraft {
    pub file_name: String,
    pub text: String,
}

impl DesignTableDraft {
    /// Returns the parameter ids and `action.<name>` columns the table's header names, or nothing when the header is
    /// not CSV.
    pub fn columns(&self) -> Vec<String> {
        let text = self.text.trim_start_matches('\u{feff}');
        let Some(header) = text.lines().find(|line| !line.trim().is_empty()) else {
            return Vec::new();
        };
        parse_records(header)
            .ok()
            .and_then(|records| records.into_iter().next())
            .map(|fields| fields.iter().map(|column| column.trim().to_owned()).collect())
            .unwrap_or_default()
    }

    /// Number of rows after the header, blank lines left out.
    pub fn row_count(&self) -> usize {
        table_row_count(&self.text)
    }
}

/// A sweep or a search as the Sweep tab edits it.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepDraft {
    /// Id of the model the draft sweeps.
    pub model_id: String,
    /// Program the advice on a sweep over the app's limit names, `None` for none.
    pub cli_command: Option<String>,
    pub mode: DraftMode,
    /// One entry per parameter, in descriptor order. A search varies the ticked ones over its space.
    pub factors: Vec<FactorDraft>,
    /// Search a draft in [`DraftMode::Search`] runs in place of the design.
    pub search: SearchDraft,
    pub design: DraftDesign,
    /// Configurations a sampled design draws.
    pub samples: usize,
    /// Seed of a sampled design's draws, empty to derive it from the root seed.
    pub design_seed_text: String,
    pub table: Option<DesignTableDraft>,
    pub actions: Vec<ActionDraft>,
    pub replicates: u64,
    pub root_seed_text: String,
    /// Whether replicate `r` gets the same seed in every configuration.
    pub common_random_numbers: bool,
    pub steps: u64,
    pub warmup: u64,
    /// Ticks between two samples.
    pub stats_every: u64,
    /// Ticks between two rows of the series, 0 for no series.
    pub series_every: u64,
    pub stop: Option<StopDraft>,
    /// Stat column of the stop condition last cleared. A spec never holds it.
    pub last_stop_column: Option<String>,
    /// Seconds of wall-clock time after which a run is abandoned.
    pub timeout_s: Option<f64>,
    /// Whether every stat gets its final, minimum, maximum and mean.
    pub default_reducers: bool,
    /// Outputs added after the defaults.
    pub reducers: Vec<ReducerSpec>,
    pub concurrency: Concurrency,
    /// Bytes of host memory the live runs can hold together, as a loaded spec's `[execution]` table sets it.
    ///
    /// The Execution section shows it read-only, Save spec writes it back unchanged, and a sweep the tab starts runs
    /// under it.
    pub memory_budget: Option<u64>,
    /// Bytes of device memory the live GPU runs can hold together, kept from a loaded spec as
    /// [`Self::memory_budget`] is.
    pub gpu_memory_budget: Option<u64>,
    /// Whether the results stream into [`Self::output_dir_text`] in place of staying in memory. A spec never holds it.
    pub results_in_folder: bool,
    /// Folder the results stream into while [`Self::results_in_folder`] is set. Only the desktop app writes one.
    pub output_dir_text: String,
    /// Stat columns a build of the model samples. They name the parts of a vector or histogram stat.
    ///
    /// Until [`Self::set_stat_columns`] gives them, they are `None` and every stat counts as one column. A spec never
    /// holds them.
    pub stat_columns: Option<StatColumns>,
    /// Whether a build of the model is still sampling [`Self::stat_columns`]. The draft cannot start until the build
    /// reports. A spec never holds it.
    pub columns_pending: bool,
}

/// Source of the tick an action fires at in each run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickSource {
    /// The action's own tick, the same in every run.
    Fixed,
    /// The ticks the draft varies or searches over.
    Varied,
    /// The action's column in the design table the draft runs.
    Table,
}

/// One of the two axes of a Pattern Space Exploration's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GridAxis {
    X,
    Y,
}

impl GridAxis {
    /// Returns the label of the axis's row, as in "X axis".
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X axis",
            Self::Y => "Y axis",
        }
    }
}

/// Part of the Sweep tab an issue belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DraftSite {
    /// The sweep as a whole.
    Sweep,
    /// Parameter row `i`.
    Factor(usize),
    /// The Parameters section as a whole, as for a search over no parameter.
    Parameters,
    Design,
    DesignSeed,
    /// Method, budget and every other search field without a site of its own.
    Search,
    Objective,
    /// Output and range of one axis of a Pattern Space Exploration.
    Axis(GridAxis),
    /// Setting of a search method, named by its key in the spec, as in `genetic.elite_count`.
    MethodSetting(&'static str),
    /// Action row `i`.
    Action(usize),
    Replicates,
    Seed,
    /// Steps and warm-up.
    RunLength,
    Stop,
    Timeout,
    /// Sample every and Series every.
    Sampling,
    /// Output row `i`.
    Output(usize),
    /// The Outputs section as a whole, as while the model's stat columns are still being read.
    Outputs,
    /// Results, the output folder and the concurrency.
    Execution,
}

/// Kind of a draft issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IssueKind {
    /// Input the plan refuses.
    Invalid,
    /// Input the user has not given yet.
    Missing,
}

/// A problem that stops a draft from starting, and the part of the tab it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftIssue {
    pub site: DraftSite,
    pub kind: IssueKind,
    pub message: String,
}

impl DraftIssue {
    /// Returns an issue of input the plan refuses.
    pub fn new(site: DraftSite, message: impl Into<String>) -> Self {
        Self {
            site,
            kind: IssueKind::Invalid,
            message: message.into(),
        }
    }

    /// Returns an issue of input the user has not given yet.
    pub fn missing(site: DraftSite, message: impl Into<String>) -> Self {
        Self {
            site,
            kind: IssueKind::Missing,
            message: message.into(),
        }
    }
}

/// Values one varied parameter or action tick takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelCount {
    /// This many values, listed or stepped.
    Listed(usize),
    /// A whole range for a sampled design to draw from.
    Drawn,
}

/// Values one varied parameter or action tick takes, as its row previews them.
///
/// A parameter's values read as the Parameters tab shows them, a fraction as a percentage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelPreview {
    /// Values listed or stepped: how many, the first [`MAX_PREVIEW_VALUES`] of them, and the last.
    Listed {
        count: usize,
        values: Vec<String>,
        last: String,
    },
    /// Every value from `min` to `max`, for a sampled design or a search to draw from.
    Drawn { min: String, max: String },
}

impl LevelPreview {
    pub fn count(&self) -> LevelCount {
        match self {
            Self::Listed { count, .. } => LevelCount::Listed(*count),
            Self::Drawn { .. } => LevelCount::Drawn,
        }
    }

    /// Returns the preview of `factor`, each level written by `level_text` and each end of a range by `bound_text`.
    fn of(factor: &Factor, level_text: impl Fn(&FactorLevel) -> String, bound_text: impl Fn(f64) -> String) -> Self {
        match &factor.domain {
            FactorDomain::Levels(levels) => Self::Listed {
                count: levels.len(),
                values: levels.iter().take(MAX_PREVIEW_VALUES).map(&level_text).collect(),
                last: levels.last().map(&level_text).unwrap_or_default(),
            },
            &FactorDomain::Continuous { min, max } => Self::Drawn {
                min: bound_text(min),
                max: bound_text(max),
            },
            &FactorDomain::WholeNumbers { min, max } => Self::Drawn {
                min: bound_text(min as f64),
                max: bound_text(max as f64),
            },
        }
    }
}

/// Values of each parameter row, then of each action row, `None` for a row that does not vary or has an issue.
pub type LevelPreviews = (Vec<Option<LevelPreview>>, Vec<Option<LevelPreview>>);

/// Spec a draft writes, and the plan of its runs.
#[derive(Debug, Clone)]
pub struct DraftPlan {
    pub spec: SweepSpec,
    /// Plan of a sweep's configs, or of a search's fixed values and actions alone.
    pub plan: Plan,
    /// Plan of a search, `None` for a sweep.
    pub search_plan: Option<SearchPlan>,
    /// Approximate bytes of `series.csv` once every run reaches its last step.
    pub series_bytes: u64,
}

impl DraftPlan {
    /// Returns the configurations of a sweep or the evaluations of a search, then the replicates of each, then the
    /// runs of all of them.
    pub fn counts(&self) -> (u64, u64, u64) {
        match &self.search_plan {
            Some(search_plan) => (
                search_plan.search().max_evaluations,
                search_plan.replicates(),
                search_plan.run_count(),
            ),
            None => (
                self.plan.configs().len() as u64,
                self.plan.replicates(),
                self.plan.run_count(),
            ),
        }
    }
}

impl SweepDraft {
    /// Returns a draft of `schema`'s model that varies nothing, with the settings a spec file takes by default.
    pub fn new(schema: &ModelSchema<'_>) -> Self {
        let run = RunSettings::default();
        let measure = MeasureSettings::default();
        let seeds = SeedSettings::default();
        Self {
            model_id: schema.id.to_owned(),
            cli_command: None,
            mode: DraftMode::default(),
            factors: schema
                .params
                .iter()
                .map(|descriptor| FactorDraft {
                    vary: false,
                    levels_text: default_levels_text(&descriptor.kind),
                })
                .collect(),
            search: SearchDraft::new(schema),
            design: DraftDesign::default(),
            samples: DEFAULT_SAMPLES,
            design_seed_text: String::new(),
            table: None,
            actions: Vec::new(),
            replicates: run.replicates,
            root_seed_text: seeds.root.to_string(),
            common_random_numbers: seeds.scheme == SeedScheme::Common,
            steps: run.steps,
            warmup: run.warmup,
            stats_every: measure.stats_every,
            series_every: measure.series_every,
            stop: None,
            last_stop_column: None,
            timeout_s: None,
            default_reducers: measure.default_reducers,
            reducers: Vec::new(),
            concurrency: Concurrency::Auto,
            memory_budget: None,
            gpu_memory_budget: None,
            results_in_folder: false,
            output_dir_text: String::new(),
            stat_columns: None,
            columns_pending: false,
        }
    }

    /// Gives the draft the stat columns a build of its model samples.
    ///
    /// Each output a search reads that names a vector or histogram stat by its label alone, as in `Velocity:max`,
    /// becomes the output of the column the label stands for, `Velocity.magnitude:max`. A reducer reads a bare label
    /// the same way.
    pub fn set_stat_columns(&mut self, columns: &StatColumns) {
        self.columns_pending = false;
        if self.stat_columns.as_ref() == Some(columns) {
            return;
        }
        self.stat_columns = Some(columns.clone());
        let search = &mut self.search;
        for output in [
            &mut search.objective_column,
            &mut search.pattern_space.x_axis.column,
            &mut search.pattern_space.y_axis.column,
        ] {
            let resolved = output.rsplit_once(':').and_then(|(stat_column, kind)| {
                let name = columns.name(columns.resolve(stat_column)?);
                (name != stat_column).then(|| format!("{name}:{kind}"))
            });
            if let Some(resolved) = resolved {
                *output = resolved;
            }
        }
    }

    /// Returns the stat columns of [`Self::stat_columns`], or one column per stat of `schema` until they are given.
    fn stat_layout(&self, schema: &ModelSchema<'_>) -> Cow<'_, StatColumns> {
        match &self.stat_columns {
            Some(columns) => Cow::Borrowed(columns),
            None => Cow::Owned(scalar_columns(schema)),
        }
    }

    /// Returns where the tick of `action`, one of the draft's actions, comes from in each run.
    ///
    /// A design table sets the tick of each action it has a column for. The action's ticks in the draft then go
    /// unused.
    pub fn tick_source(&self, action: &ActionDraft) -> TickSource {
        if !self.runs_table() {
            return if action.vary_tick {
                TickSource::Varied
            } else {
                TickSource::Fixed
            };
        }
        let column = format!("{ACTION_COLUMN_PREFIX}{}", action.name);
        if self
            .table
            .as_ref()
            .is_some_and(|table| table.columns().contains(&column))
        {
            TickSource::Table
        } else {
            TickSource::Fixed
        }
    }

    /// Adds action `action_index` of `schema` at `tick`, named by its id with a number after it when the draft has
    /// the name already.
    pub fn add_action(&mut self, schema: &ModelSchema<'_>, action_index: usize, tick: u64) {
        let Some(declared) = schema.actions.get(action_index) else {
            return;
        };
        let name_taken = |name: &str| self.actions.iter().any(|action| action.name == name);
        let name = (1..)
            .map(|count| match count {
                1 => declared.id.to_owned(),
                count => format!("{}_{count}", declared.id),
            })
            .find(|name| !name_taken(name))
            .unwrap_or_default();
        self.actions.push(ActionDraft {
            action_index,
            name,
            tick,
            vary_tick: false,
            ticks_text: tick.to_string(),
        });
    }

    /// Sets action row `position` to action `action_index` of `schema`, renamed after it.
    pub fn set_action(&mut self, schema: &ModelSchema<'_>, position: usize, action_index: usize) {
        if position >= self.actions.len()
            || self.actions[position].action_index == action_index
            || schema.actions.get(action_index).is_none()
        {
            return;
        }
        let row = self.actions.remove(position);
        self.add_action(schema, action_index, row.tick);
        if let Some(mut added) = self.actions.pop() {
            added.vary_tick = row.vary_tick;
            added.ticks_text = row.ticks_text;
            self.actions.insert(position, added);
        }
    }

    /// Returns whether the draft runs the rows of a design table. The rows set the parameters and ticks they name.
    pub fn runs_table(&self) -> bool {
        self.mode == DraftMode::Sweep && self.design == DraftDesign::Table
    }

    /// Returns whether the draft's factors take a whole range, as a sampled design or a search does.
    pub fn draws_ranges(&self) -> bool {
        self.mode == DraftMode::Search || self.design.is_sampled()
    }

    /// Returns the design the draft's factors resolve under. A search space resolves its factors as a sampled design
    /// does.
    fn factor_design(&self) -> DesignKind {
        match (self.mode, self.design) {
            (DraftMode::Search, _) => DesignKind::Random { samples: 1 },
            (DraftMode::Sweep, DraftDesign::LatinHypercube) => DesignKind::LatinHypercube { samples: self.samples },
            (DraftMode::Sweep, DraftDesign::UniformRandom) => DesignKind::Random { samples: self.samples },
            (DraftMode::Sweep, DraftDesign::Zip) => DesignKind::Zip,
            (DraftMode::Sweep, _) => DesignKind::Factorial,
        }
    }

    /// Returns each action of the draft that the model declares, as the spec writes it.
    fn declared_actions(&self, schema: &ModelSchema<'_>) -> Vec<ActionSpec> {
        self.actions
            .iter()
            .filter_map(|action| {
                let declared = schema.actions.get(action.action_index)?;
                Some(ActionSpec {
                    id: declared.id.to_owned(),
                    name: action.name.clone(),
                    tick: action.tick,
                })
            })
            .collect()
    }

    /// Returns the values of parameter row `index`, resolved against `schema` with the draft's `actions`, or the kind
    /// and message of its issue.
    fn resolve_param(
        &self,
        schema: &ModelSchema<'_>,
        actions: &[ActionSpec],
        index: usize,
    ) -> Result<Factor, (IssueKind, String)> {
        let descriptor = &schema.params[index];
        let missing = || match descriptor.kind {
            ParamKind::Bool { .. } | ParamKind::Choice { .. } => "Select at least one option".to_owned(),
            _ if self.draws_ranges() => format!("Enter values, such as {}", whole_range_text(&descriptor.kind)),
            _ => format!("Enter values, such as {}", levels_example(&descriptor.kind)),
        };
        let levels = row_levels(&self.factors[index].levels_text, self.draws_ranges(), missing)?;
        FactorSpec::param(descriptor.id, levels)
            .resolve(schema.params, actions, &self.factor_design())
            .map_err(|error| (IssueKind::Invalid, factor_message(&error)))
    }

    /// Returns the ticks of action row `position`, resolved with the draft's `actions`, or the kind and message of its
    /// issue.
    fn resolve_ticks(
        &self,
        schema: &ModelSchema<'_>,
        actions: &[ActionSpec],
        position: usize,
    ) -> Result<Factor, (IssueKind, String)> {
        let action = &self.actions[position];
        let levels = row_levels(&action.ticks_text, self.draws_ranges(), || {
            format!("Enter ticks, such as {TICKS_EXAMPLE}")
        })?;
        FactorSpec::action(action.name.clone(), levels)
            .resolve(schema.params, actions, &self.factor_design())
            .map_err(|error| (IssueKind::Invalid, factor_message(&error)))
    }

    /// Returns the values each varied parameter takes, then those each varied action tick takes, `None` for one that
    /// is not varied or has an issue.
    pub fn level_previews(&self, schema: &ModelSchema<'_>) -> LevelPreviews {
        let actions = self.declared_actions(schema);
        let params = schema
            .params
            .iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let varied = self.factors.get(index).is_some_and(|factor| factor.vary);
                let factor = self.resolve_param(schema, &actions, index).ok().filter(|_| varied)?;
                Some(param_preview(descriptor, &factor))
            })
            .collect();
        let ticks = (0..self.actions.len())
            .map(|position| {
                let varied = self.actions[position].vary_tick;
                let factor = self.resolve_ticks(schema, &actions, position).ok().filter(|_| varied)?;
                Some(LevelPreview::of(&factor, tick_text, |bound| (bound as u64).to_string()))
            })
            .collect();
        (params, ticks)
    }

    /// Returns every issue the draft's rows show before a plan is made, in the order the Sweep tab draws the rows.
    ///
    /// Each parameter row, action tick, seed and search field is checked, and every issue found is listed. The plan
    /// finds the rest one at a time. The issue of stat columns still being sampled comes last.
    pub fn issues(&self, schema: &ModelSchema<'_>) -> Vec<DraftIssue> {
        let mut issues = Vec::new();
        let mut row_issues = Vec::new();
        let actions = self.declared_actions(schema);
        let runs_table = self.runs_table();
        // Label and number of values of each varied row that lists its values, for a zip to compare.
        let mut listed: Vec<(String, usize)> = Vec::new();
        let mut varied_rows = 0;
        if !runs_table {
            for (index, descriptor) in schema.params.iter().enumerate() {
                if !self.factors.get(index).is_some_and(|factor| factor.vary) {
                    continue;
                }
                varied_rows += 1;
                match self.resolve_param(schema, &actions, index) {
                    Ok(factor) => {
                        if let Some(levels) = factor.levels() {
                            listed.push((descriptor.label.to_owned(), levels.len()));
                        }
                    }
                    Err((kind, message)) => row_issues.push(DraftIssue {
                        site: DraftSite::Factor(index),
                        kind,
                        message,
                    }),
                }
            }
        }
        if self.mode == DraftMode::Search && varied_rows == 0 && !self.actions.iter().any(|action| action.vary_tick) {
            row_issues.push(DraftIssue::missing(
                DraftSite::Parameters,
                "Select a parameter or an action's Search tick to search over it",
            ));
        }
        for (position, action) in self.actions.iter().enumerate() {
            let Some(declared) = schema.actions.get(action.action_index) else {
                row_issues.push(DraftIssue::new(DraftSite::Action(position), "Model has no such action"));
                continue;
            };
            if !action.vary_tick || runs_table {
                continue;
            }
            match self.resolve_ticks(schema, &actions, position) {
                Ok(factor) => {
                    if let Some(levels) = factor.levels() {
                        listed.push((format!("{} tick", declared.label), levels.len()));
                    }
                }
                Err((kind, message)) => row_issues.push(DraftIssue {
                    site: DraftSite::Action(position),
                    kind,
                    message,
                }),
            }
        }

        match self.mode {
            DraftMode::Sweep if runs_table && self.table.is_none() => {
                issues.push(DraftIssue::missing(DraftSite::Design, "No table loaded"));
            }
            DraftMode::Sweep if self.design == DraftDesign::Zip => issues.extend(zip_issue(&listed)),
            DraftMode::Sweep => {}
            DraftMode::Search => issues.extend(self.search_issues(schema)),
        }
        issues.append(&mut row_issues);

        if self.replicates == 0 {
            issues.push(DraftIssue::new(DraftSite::Replicates, "Replicates must be at least 1"));
        }
        if let Err(message) = parse_seed_text(&self.root_seed_text) {
            issues.push(DraftIssue::new(DraftSite::Seed, format!("Root seed: {message}")));
        }
        let design_seed = self.design_seed_text.trim();
        if self.mode == DraftMode::Sweep
            && self.design.is_sampled()
            && !design_seed.is_empty()
            && let Err(message) = parse_seed_text(design_seed)
        {
            issues.push(DraftIssue::new(
                DraftSite::DesignSeed,
                format!("Design seed: {message}"),
            ));
        }
        issues.extend(self.stop_issue());
        issues.extend(self.timeout().err());
        if self.stats_every == 0 {
            issues.push(DraftIssue::new(DraftSite::Sampling, "Sample every must be at least 1"));
        } else if !self.series_every.is_multiple_of(self.stats_every) {
            issues.push(DraftIssue::new(DraftSite::Sampling, self.series_cadence_message()));
        }
        issues.extend(self.output_issues());
        if self.results_in_folder && !cfg!(target_arch = "wasm32") && self.output_dir_text.trim().is_empty() {
            issues.push(DraftIssue::missing(DraftSite::Execution, "Select a folder"));
        }
        if self.columns_pending {
            issues.push(DraftIssue::missing(DraftSite::Outputs, COLUMNS_PENDING));
        }
        issues
    }

    /// Returns the issue of a stop condition whose threshold is not a finite number.
    ///
    /// The plan takes any threshold. A spec file refuses one that is not finite.
    fn stop_issue(&self) -> Option<DraftIssue> {
        self.stop
            .as_ref()
            .filter(|stop| !stop.threshold.is_finite())
            .map(|_| DraftIssue::new(DraftSite::Stop, NOT_FINITE_THRESHOLD))
    }

    /// Returns the time limit of each run, `None` for no limit.
    ///
    /// A limit under [`MIN_TIMEOUT_SECONDS`] is taken, 0 included, as `henad-cli` takes it.
    ///
    /// # Errors
    ///
    /// Returns the issue of a limit that is negative, not a number, or too large for a [`Duration`].
    fn timeout(&self) -> Result<Option<Duration>, DraftIssue> {
        let Some(seconds) = self.timeout_s else {
            return Ok(None);
        };
        if let Ok(timeout) = Duration::try_from_secs_f64(seconds) {
            return Ok(Some(timeout));
        }
        let message = if seconds.is_nan() {
            "Seconds per run must be a number"
        } else if seconds < 0.0 {
            "Seconds per run must be at least 0"
        } else {
            "Seconds per run is too large"
        };
        Err(DraftIssue::new(DraftSite::Timeout, message))
    }

    /// Returns an issue for each output row whose window ends before it starts, or whose crossing threshold is not a
    /// finite number.
    ///
    /// The plan takes a window in either order and any threshold. A spec file refuses both.
    fn output_issues(&self) -> impl Iterator<Item = DraftIssue> + '_ {
        self.reducers.iter().enumerate().filter_map(|(position, reducer)| {
            let message = match reducer.kind {
                ReducerKind::WindowMean { start, end } if start > end => "From tick must be at most To tick",
                ReducerKind::FirstCrossing(comparison) if !comparison.threshold.is_finite() => NOT_FINITE_THRESHOLD,
                _ => return None,
            };
            Some(DraftIssue::new(DraftSite::Output(position), message))
        })
    }

    /// Returns the issue of a series every [`Self::series_every`] ticks, off the cadence of the samples.
    fn series_cadence_message(&self) -> String {
        format!(
            "Series every must be 0 or a multiple of Sample every ({} {})",
            self.stats_every,
            plural(self.stats_every, "tick")
        )
    }

    /// Returns the issues of a search's objective or axes, and of its method's settings.
    fn search_issues(&self, schema: &ModelSchema<'_>) -> Vec<DraftIssue> {
        let search = &self.search;
        let mut issues = Vec::new();
        for (site, column) in search.watched_columns() {
            if column.is_empty() {
                issues.push(DraftIssue::missing(site, "Select output"));
            } else if !self.records_output(column, schema) {
                issues.push(DraftIssue::new(
                    site,
                    format!("{} is missing from Outputs", output_label(column)),
                ));
            }
            if let DraftSite::Axis(axis) = site {
                let settings = &search.pattern_space;
                let pattern_axis = match axis {
                    GridAxis::X => &settings.x_axis,
                    GridAxis::Y => &settings.y_axis,
                };
                issues.extend(axis_range_issue(pattern_axis).map(|(kind, message)| DraftIssue { site, kind, message }));
            }
        }
        issues.extend(self.setting_issues());
        issues
    }

    /// Returns an issue for each setting of the search's method that its check refuses.
    ///
    /// The check stops at the first setting it refuses. That setting is set to a value it takes, and the check runs
    /// again, until it passes. A grid axis is checked with the rows of the axes.
    fn setting_issues(&self) -> Vec<DraftIssue> {
        let search = &self.search;
        let refusals = match search.algorithm {
            DraftAlgorithm::Random => Vec::new(),
            DraftAlgorithm::HillClimb => every_refusal(search.hill_climb, HillClimbSettings::check, |settings, key| {
                let fallback = HillClimbSettings::default();
                match key {
                    "hill_climb.patience" => settings.patience = fallback.patience,
                    _ => settings.mutation_scale = fallback.mutation_scale,
                }
            }),
            DraftAlgorithm::Genetic => every_refusal(search.genetic, GeneticSettings::check, |settings, key| {
                let fallback = GeneticSettings::default();
                match key {
                    "genetic.population" => settings.population = fallback.population,
                    // No elite count below 1 is refused, whatever the population.
                    "genetic.elite_count" => settings.elite_count = 0,
                    "genetic.tournament_size" => settings.tournament_size = fallback.tournament_size,
                    "genetic.crossover_rate" => settings.crossover_rate = fallback.crossover_rate,
                    "genetic.mutation_rate" => settings.mutation_rate = fallback.mutation_rate,
                    "genetic.reevaluate_fraction" => settings.reevaluate_fraction = fallback.reevaluate_fraction,
                    _ => settings.mutation_scale = fallback.mutation_scale,
                }
            }),
            DraftAlgorithm::PatternSpace => {
                let mut settings = search.pattern_space.clone();
                // The rows of the axes report their ranges, so the check here meets only a range it takes. An
                // automatic range is left as it is. The check needs it to refuse zero initial samples.
                for axis in [&mut settings.x_axis, &mut settings.y_axis] {
                    if !axis.is_automatic() {
                        axis.min = Some(0.0);
                        axis.max = Some(1.0);
                    }
                    axis.cells = axis.cells.max(1);
                }
                every_refusal(settings, PatternSpaceSettings::check, |settings, key| {
                    let fallback = PatternSpaceSettings::new(settings.x_axis.clone(), settings.y_axis.clone());
                    match key {
                        INITIAL_SAMPLES_KEY => settings.initial_samples = fallback.initial_samples,
                        _ => settings.mutation_scale = fallback.mutation_scale,
                    }
                })
            }
        };
        refusals
            .into_iter()
            .map(|(key, message)| DraftIssue::new(DraftSite::MethodSetting(key), message))
            .collect()
    }

    /// Returns the site of the row the search setting `key` belongs to.
    fn setting_site(key: &'static str) -> DraftSite {
        if key.starts_with("pse.x_axis.") {
            DraftSite::Axis(GridAxis::X)
        } else if key.starts_with("pse.y_axis.") {
            DraftSite::Axis(GridAxis::Y)
        } else {
            DraftSite::MethodSetting(key)
        }
    }

    /// Returns the spec the draft writes, with each parameter it does not vary at its value in `panel_values`.
    ///
    /// # Errors
    ///
    /// Returns every [`DraftIssue`] of text that does not parse, a design table that is missing, an action the model
    /// does not declare, or a timeout that is not a [`Duration`]. Everything else is checked by [`Self::check`].
    pub fn to_spec(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Result<SweepSpec, Vec<DraftIssue>> {
        let mut issues = Vec::new();
        let mut spec = SweepSpec::new(self.model_id.clone());

        for (position, action) in self.actions.iter().enumerate() {
            match schema.actions.get(action.action_index) {
                Some(declared) => spec.actions.push(ActionSpec {
                    id: declared.id.to_owned(),
                    name: action.name.clone(),
                    tick: action.tick,
                }),
                None => issues.push(DraftIssue::new(DraftSite::Action(position), "Model has no such action")),
            }
        }

        let root = parse_seed_text(&self.root_seed_text).unwrap_or_else(|message| {
            issues.push(DraftIssue::new(DraftSite::Seed, format!("Root seed: {message}")));
            0
        });
        spec.seeds = SeedSettings {
            root,
            scheme: if self.common_random_numbers {
                SeedScheme::Common
            } else {
                SeedScheme::Independent
            },
        };

        match self.mode {
            DraftMode::Sweep => spec.blocks = self.blocks(schema, panel_values, &mut issues),
            DraftMode::Search => {
                let (params, actions) = self.varied_factors(schema, &mut issues);
                let space = params.into_iter().map(|(_, factor)| factor).chain(actions).collect();
                spec.search = Some(self.search.to_spec(space));
            }
        }
        spec.fixed = self.fixed_values(schema, panel_values);
        let timeout = self.timeout().unwrap_or_else(|issue| {
            issues.push(issue);
            None
        });
        spec.run = RunSettings {
            steps: self.steps,
            warmup: self.warmup,
            replicates: self.replicates,
            stop: self.stop.as_ref().map(|stop| StopSpec {
                column: stop.column.clone(),
                comparison: Comparison {
                    comparator: stop.comparator,
                    threshold: stop.threshold,
                },
                min_tick: stop.min_tick,
            }),
            timeout,
        };
        spec.measure = MeasureSettings {
            stats_every: self.stats_every,
            series_every: self.series_every,
            default_reducers: self.default_reducers,
            reducers: self.reducers.clone(),
        };

        if issues.is_empty() { Ok(spec) } else { Err(issues) }
    }

    /// Returns the spec the draft writes and its plan.
    ///
    /// # Errors
    ///
    /// Returns every issue of [`Self::issues`] when there is any. Otherwise returns the one issue of a sweep or search
    /// of more than [`MAX_DRAFT_RUNS`] runs, one held in memory whose series pass [`MAX_MEMORY_SERIES_BYTES`], or the
    /// reason the model refuses the spec.
    pub fn check(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Result<DraftPlan, Vec<DraftIssue>> {
        let issues = self.issues(schema);
        if !issues.is_empty() {
            return Err(issues);
        }
        let spec = self.to_spec(schema, panel_values)?;
        let noun = match self.mode {
            DraftMode::Sweep => "Sweep",
            DraftMode::Search => "Search",
        };
        let too_many = |runs: u64| {
            vec![DraftIssue::new(
                DraftSite::Sweep,
                format!(
                    "{noun} of {runs} runs is over the app's limit of {MAX_DRAFT_RUNS}. Run larger ones {}.",
                    cli_phrase(self.cli_command.as_deref())
                ),
            )]
        };
        let estimate = match self.mode {
            DraftMode::Sweep => estimated_configs(&spec),
            DraftMode::Search => self.search.max_evaluations,
        };
        let estimate = estimate.saturating_mul(spec.run.replicates);
        if estimate > MAX_DRAFT_RUNS {
            return Err(too_many(estimate));
        }
        let (plan, search_plan) = match self.mode {
            DraftMode::Sweep => match spec.plan(schema) {
                Ok(plan) => (plan, None),
                Err(error) => return Err(vec![self.plan_issue(&error, &spec, schema)]),
            },
            DraftMode::Search => match SearchPlan::new(&spec, schema) {
                Ok(search_plan) => ((**search_plan.base()).clone(), Some(search_plan)),
                Err(error) => return Err(vec![self.search_issue(&error, &spec, schema)]),
            },
        };
        // The plan checks each column against the stat labels alone. A sweep binds its reducers, its stop condition
        // and a search's outputs to the columns a build samples, and the check does the same once it has them.
        if let Some(columns) = &self.stat_columns {
            let measure = MeasurePlan::new(&spec.run, &spec.measure, columns.clone())
                .map_err(|error| vec![self.measure_issue(&error)])?;
            if let Some(search_plan) = &search_plan {
                search_plan
                    .watched_reducers(&measure)
                    .map_err(|error| vec![self.search_issue(&error, &spec, schema)])?;
            }
        }
        let run_count = search_plan.as_ref().map_or(plan.run_count(), SearchPlan::run_count);
        if run_count > MAX_DRAFT_RUNS {
            return Err(too_many(run_count));
        }
        let series_bytes = estimated_series_bytes(&plan, run_count, self.stat_layout(schema).len());
        if self.holds_results_in_memory() && series_bytes > MAX_MEMORY_SERIES_BYTES {
            return Err(vec![DraftIssue::new(
                DraftSite::Execution,
                memory_refusal(series_bytes),
            )]);
        }
        Ok(DraftPlan {
            spec,
            plan,
            search_plan,
            series_bytes,
        })
    }

    /// Returns the output columns the draft's runs record, as the reducers of a sweep name them: the final, minimum,
    /// maximum and mean of every stat column when the draft records them, then each added output.
    ///
    /// A scalar stat is one column, as in `Infected:max`. A vector stat records its parts, as in
    /// `Velocity.magnitude:max`, and a histogram stat its total. An added output whose column the model does not
    /// give is left out. Until [`Self::stat_columns`] are given, an added output over a part of a stat, as in
    /// `Velocity.x:argmax`, keeps the name it is written with.
    pub fn output_names(&self, schema: &ModelSchema<'_>) -> Vec<String> {
        let columns = self.stat_layout(schema);
        let bound: Vec<ReducerSpec> = self
            .reducers
            .iter()
            .filter(|reducer| columns.resolve(&reducer.column).is_some())
            .cloned()
            .collect();
        let mut names = ReducerPlan::bind(&columns, &bound, self.default_reducers)
            .map(|plan| plan.names().to_vec())
            .unwrap_or_default();
        if self.stat_columns.is_none() {
            let parts = self.reducers.iter().filter(|reducer| {
                columns.resolve(&reducer.column).is_none() && reducer.check_label(schema.stats).is_ok()
            });
            for reducer in parts {
                let name = written_name(reducer, None);
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    /// Returns each output column the Objective and axis lists offer that the draft's runs do not record yet, as in
    /// `Infected:argmax`: the final value, minimum, maximum, mean and ticks of the maximum and minimum of every stat
    /// column but a histogram's buckets.
    pub fn unrecorded_outputs(&self, schema: &ModelSchema<'_>) -> Vec<String> {
        let recorded = self.output_names(schema);
        let columns = self.stat_layout(schema);
        (0..columns.len())
            .filter(|&column| !columns.is_bucket(column))
            .flat_map(|column| {
                let name = columns.name(column);
                OFFERED_KINDS.iter().map(move |kind| format!("{name}:{kind}"))
            })
            .filter(|column| !recorded.contains(column))
            .collect()
    }

    /// Returns whether the draft's runs record the output column `column`, named as [`Self::output_names`] names it.
    ///
    /// Until [`Self::stat_columns`] are given, a column of a stat's part, as in `Velocity.x:max`, also counts when
    /// the draft records the defaults of every stat and the kind is one of them.
    pub fn records_output(&self, column: &str, schema: &ModelSchema<'_>) -> bool {
        if self.output_names(schema).iter().any(|name| name == column) {
            return true;
        }
        self.stat_columns.is_none()
            && self.default_reducers
            && column.parse::<ReducerSpec>().is_ok_and(|output| {
                ReducerKind::DEFAULTS.contains(&output.kind) && output.check_label(schema.stats).is_ok()
            })
    }

    /// Adds the output column `column` reads, as in `Infected:max`, as an output row of its own.
    ///
    /// Nothing is added when a row records it already, or when `column` does not read as a stat and a kind. Note that
    /// the defaults of every stat do not count. Returns whether a row was added.
    pub fn add_watched_output(&mut self, column: &str) -> bool {
        let Some((stat, kind)) = column.rsplit_once(':') else {
            return false;
        };
        let Ok(kind) = kind.parse::<ReducerKind>() else {
            return false;
        };
        let added = ReducerSpec {
            column: stat.to_owned(),
            kind,
        };
        let columns = self.stat_columns.as_ref();
        let name = written_name(&added, columns);
        let recorded = self
            .reducers
            .iter()
            .any(|reducer| written_name(reducer, columns) == name);
        if stat.is_empty() || recorded {
            return false;
        }
        self.reducers.push(added);
        true
    }

    /// Stops recording the final, minimum, maximum and mean of every stat.
    ///
    /// Each of those outputs that a search reads is first added as an output row of its own, so the search keeps it.
    pub fn drop_default_outputs(&mut self) {
        if self.mode == DraftMode::Search {
            let watched: Vec<String> = self
                .search
                .watched_columns()
                .into_iter()
                .map(|(_, column)| column.to_owned())
                .collect();
            for column in watched {
                let default_kind = column
                    .rsplit_once(':')
                    .and_then(|(_, kind)| kind.parse::<ReducerKind>().ok())
                    .is_some_and(|kind| ReducerKind::DEFAULTS.contains(&kind));
                if default_kind {
                    self.add_watched_output(&column);
                }
            }
        }
        self.default_reducers = false;
    }

    /// Returns the site of the search field that reads output row `position`, `None` for a row no field reads.
    ///
    /// Only a search reads outputs.
    pub fn output_reader(&self, position: usize) -> Option<DraftSite> {
        let reducer = self.reducers.get(position)?;
        if self.mode != DraftMode::Search {
            return None;
        }
        let column = written_name(reducer, self.stat_columns.as_ref());
        self.search
            .watched_columns()
            .into_iter()
            .find(|(_, watched)| *watched == column)
            .map(|(site, _)| site)
    }

    /// Returns the stat column a new stop condition reads.
    ///
    /// A search takes the stat its objective reads, or its X axis for a Pattern Space Exploration. A sweep takes the
    /// stat of its first output row. Without one, or when it names no stat of `schema`, the stat of the stop
    /// condition last cleared follows, then the model's first stat.
    pub fn default_stop_column(&self, schema: &ModelSchema<'_>) -> String {
        let watched = match self.mode {
            DraftMode::Search => self
                .search
                .watched_columns()
                .first()
                .and_then(|(_, column)| column.parse::<ReducerSpec>().ok())
                .map(|reducer| reducer.column),
            DraftMode::Sweep => self.reducers.first().map(|reducer| reducer.column.clone()),
        };
        let names_stat = |column: &String| {
            schema.stats.iter().any(|stat| {
                column
                    .strip_prefix(stat.label)
                    .is_some_and(|part| part.is_empty() || part.starts_with('.'))
            })
        };
        watched
            .into_iter()
            .chain(self.last_stop_column.clone())
            .find(names_stat)
            .or_else(|| schema.stats.first().map(|stat| stat.label.to_owned()))
            .unwrap_or_default()
    }

    /// Adds a stop condition on [`Self::default_stop_column`], at most 0 from tick 0. A draft with one keeps it.
    pub fn add_stop(&mut self, schema: &ModelSchema<'_>) {
        if self.stop.is_some() {
            return;
        }
        self.stop = Some(StopDraft {
            column: self.default_stop_column(schema),
            comparator: Comparator::LessOrEqual,
            threshold: 0.0,
            min_tick: 0,
        });
    }

    /// Clears the stop condition, keeping its stat for the next one.
    pub fn clear_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            self.last_stop_column = Some(stop.column);
        }
    }

    /// Returns whether the draft's results stay in memory, as they always do in a browser.
    pub fn holds_results_in_memory(&self) -> bool {
        cfg!(target_arch = "wasm32") || !self.results_in_folder
    }

    /// Returns the folder the draft's results go to, `None` for results held in memory or a folder not chosen yet.
    pub fn output_folder(&self) -> Option<&str> {
        Some(self.output_dir_text.trim()).filter(|folder| !self.holds_results_in_memory() && !folder.is_empty())
    }

    /// Returns the spec file of the draft as TOML, its concurrency and memory budgets included.
    ///
    /// # Errors
    ///
    /// Returns the issues of [`Self::to_spec`], and the issue of each value a spec file refuses: a window that ends
    /// before it starts, or a threshold that is not finite. Returns the reason [`Self::from_spec_file`] gives for
    /// any other file it would refuse to read back.
    pub fn to_toml(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Result<String, Vec<DraftIssue>> {
        let refused: Vec<DraftIssue> = self.stop_issue().into_iter().chain(self.output_issues()).collect();
        if !refused.is_empty() {
            return Err(refused);
        }
        let loaded = LoadedSpec {
            spec: self.to_spec(schema, panel_values)?,
            spec_source: SpecSource::default(),
            execution: ExecutionTable {
                concurrent: self.concurrency,
                memory: self.memory_budget,
                gpu_memory: self.gpu_memory_budget,
            },
        };
        let text = loaded
            .to_toml()
            .map_err(|error| vec![DraftIssue::new(DraftSite::Sweep, describe_error(&error))])?;
        SpecFile::parse(&text)
            .map_err(|error| describe_error(&error))
            .and_then(|file| Self::from_spec_file(file, schema))
            .map_err(|message| vec![DraftIssue::new(DraftSite::Sweep, message)])?;
        Ok(text)
    }

    /// Reads a draft from `file` over `schema`'s model, with the Parameters tab values the file sets.
    ///
    /// # Errors
    ///
    /// Returns the reason for a file that does not read as a spec, or a spec [`Self::from_spec`] refuses.
    pub fn from_spec_file(file: SpecFile, schema: &ModelSchema<'_>) -> Result<(Self, Vec<Option<ParamValue>>), String> {
        let execution = file.execution;
        let spec = file.into_spec().map_err(|error| describe_error(&error))?;
        let (mut draft, panel_values) = Self::from_spec(&spec, execution.concurrent, schema)?;
        draft.memory_budget = execution.memory;
        draft.gpu_memory_budget = execution.gpu_memory;
        Ok((draft, panel_values))
    }

    /// Reads a draft from `spec` over `schema`'s model.
    ///
    /// Returns the draft, and the value of each parameter the spec holds at one value, in descriptor order. A
    /// parameter the spec varies in every configuration is `None`, and keeps its Parameters tab value.
    ///
    /// # Errors
    ///
    /// Returns the reason for a spec of another model, a value or action the model refuses, or blocks the tab cannot
    /// edit. The tab edits one block, or the blocks [`DraftDesign::VaryEachAlone`] writes.
    pub fn from_spec(
        spec: &SweepSpec,
        concurrency: Concurrency,
        schema: &ModelSchema<'_>,
    ) -> Result<(Self, Vec<Option<ParamValue>>), String> {
        if spec.model != schema.id {
            return Err(format!(
                "Spec is for {}, but the selected model is {}",
                spec.model, schema.id
            ));
        }
        let mut draft = Self::new(schema);
        let mut panel_values = vec![None; schema.params.len()];
        for (id, text) in &spec.fixed {
            let index = param_index(schema, id)?;
            panel_values[index] = Some(read_value(schema, index, text)?);
        }
        for action in &spec.actions {
            let action_index = schema
                .actions
                .iter()
                .position(|declared| declared.id == action.id)
                .ok_or_else(|| format!("{} has no action {}", schema.id, action.id))?;
            draft.actions.push(ActionDraft {
                action_index,
                name: action.name.clone(),
                tick: action.tick,
                vary_tick: false,
                ticks_text: action.tick.to_string(),
            });
        }

        draft.replicates = spec.run.replicates;
        draft.root_seed_text = spec.seeds.root.to_string();
        draft.common_random_numbers = spec.seeds.scheme == SeedScheme::Common;
        draft.steps = spec.run.steps;
        draft.warmup = spec.run.warmup;
        draft.stats_every = spec.measure.stats_every;
        draft.series_every = spec.measure.series_every;
        draft.stop = spec.run.stop.as_ref().map(|stop| StopDraft {
            column: stop.column.clone(),
            comparator: stop.comparison.comparator,
            threshold: stop.comparison.threshold,
            min_tick: stop.min_tick,
        });
        draft.timeout_s = spec.run.timeout.map(|timeout| timeout.as_secs_f64());
        draft.default_reducers = spec.measure.default_reducers;
        draft.reducers.clone_from(&spec.measure.reducers);
        draft.concurrency = concurrency;

        match spec.blocks.as_slice() {
            [] => {}
            [block] => draft.read_block(block, schema)?,
            blocks => draft.read_vary_each_alone(blocks, schema, &mut panel_values)?,
        }
        if let Some(search) = &spec.search {
            draft.mode = DraftMode::Search;
            draft.search.read_spec(search);
            for factor in &search.space {
                draft.mark_varied(&factor.target, &factor.levels, schema)?;
            }
        }
        Ok((draft, panel_values))
    }

    /// Returns the blocks the draft's design writes, noting an issue for each varied value that does not parse.
    fn blocks(
        &self,
        schema: &ModelSchema<'_>,
        panel_values: &[ParamValue],
        issues: &mut Vec<DraftIssue>,
    ) -> Vec<BlockSpec> {
        let design = match self.design {
            DraftDesign::Table => return self.table_blocks(issues),
            DraftDesign::EveryCombination | DraftDesign::VaryEachAlone => DesignKind::Factorial,
            DraftDesign::Zip => DesignKind::Zip,
            DraftDesign::LatinHypercube => DesignKind::LatinHypercube { samples: self.samples },
            DraftDesign::UniformRandom => DesignKind::Random { samples: self.samples },
        };
        let (params, actions) = self.varied_factors(schema, issues);
        if self.design == DraftDesign::VaryEachAlone {
            return vary_each_alone_blocks(schema, panel_values, &params, actions);
        }
        if params.is_empty() && actions.is_empty() && !design.is_sampled() {
            return Vec::new();
        }
        let design_seed = match self.design_seed_text.trim() {
            "" => None,
            _ if !design.is_sampled() => None,
            text => match parse_seed_text(text) {
                Ok(seed) => Some(seed),
                Err(message) => {
                    issues.push(DraftIssue::new(
                        DraftSite::DesignSeed,
                        format!("Design seed: {message}"),
                    ));
                    None
                }
            },
        };
        vec![BlockSpec {
            design,
            factors: params.into_iter().map(|(_, factor)| factor).chain(actions).collect(),
            design_seed,
        }]
    }

    /// Returns the block of the draft's design table, or notes an issue when there is none.
    fn table_blocks(&self, issues: &mut Vec<DraftIssue>) -> Vec<BlockSpec> {
        let Some(table) = &self.table else {
            issues.push(DraftIssue::missing(DraftSite::Design, "No table loaded"));
            return Vec::new();
        };
        vec![BlockSpec {
            design: DesignKind::Table {
                text: table.text.clone(),
            },
            factors: Vec::new(),
            design_seed: None,
        }]
    }

    /// Returns a factor for each varied parameter with its index, and one for each varied action tick, noting an
    /// issue for each whose values do not parse.
    fn varied_factors(
        &self,
        schema: &ModelSchema<'_>,
        issues: &mut Vec<DraftIssue>,
    ) -> (Vec<(usize, FactorSpec)>, Vec<FactorSpec>) {
        let mut params = Vec::new();
        let draws_ranges = self.draws_ranges();
        for (index, (factor, descriptor)) in self.factors.iter().zip(schema.params).enumerate() {
            if factor.vary {
                match row_levels(&factor.levels_text, draws_ranges, || "Enter values".to_owned()) {
                    Ok(levels) => params.push((index, FactorSpec::param(descriptor.id, levels))),
                    Err((kind, message)) => issues.push(DraftIssue {
                        site: DraftSite::Factor(index),
                        kind,
                        message,
                    }),
                }
            }
        }
        let mut actions = Vec::new();
        for (position, action) in self.actions.iter().enumerate() {
            if action.vary_tick {
                match row_levels(&action.ticks_text, draws_ranges, || "Enter ticks".to_owned()) {
                    Ok(levels) => actions.push(FactorSpec::action(action.name.clone(), levels)),
                    Err((kind, message)) => issues.push(DraftIssue {
                        site: DraftSite::Action(position),
                        kind,
                        message,
                    }),
                }
            }
        }
        (params, actions)
    }

    /// Returns the Parameters tab value of each parameter no block sets, as `(id, text)` pairs.
    fn fixed_values(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Vec<(String, String)> {
        let table_design = self.mode == DraftMode::Sweep && self.design == DraftDesign::Table;
        let table_columns = match &self.table {
            Some(table) if table_design => table.columns(),
            _ => Vec::new(),
        };
        schema
            .params
            .iter()
            .zip(panel_values)
            .zip(&self.factors)
            .filter(|((descriptor, _), factor)| {
                if table_design {
                    !table_columns.iter().any(|column| column == descriptor.id)
                } else {
                    !factor.vary
                }
            })
            .map(|((descriptor, value), _)| (descriptor.id.to_owned(), format_value(&descriptor.kind, value)))
            .collect()
    }

    /// Reads the design and factors of a spec's only block.
    fn read_block(&mut self, block: &BlockSpec, schema: &ModelSchema<'_>) -> Result<(), String> {
        self.design = match &block.design {
            DesignKind::Factorial => DraftDesign::EveryCombination,
            DesignKind::Zip => DraftDesign::Zip,
            DesignKind::Random { samples } => {
                self.samples = *samples;
                DraftDesign::UniformRandom
            }
            DesignKind::LatinHypercube { samples } => {
                self.samples = *samples;
                DraftDesign::LatinHypercube
            }
            DesignKind::Table { text } => {
                self.table = Some(DesignTableDraft {
                    file_name: SPEC_TABLE_NAME.to_owned(),
                    text: text.clone(),
                });
                DraftDesign::Table
            }
        };
        if let Some(seed) = block.design_seed {
            self.design_seed_text = seed.to_string();
        }
        for factor in &block.factors {
            self.mark_varied(&factor.target, &factor.levels, schema)?;
        }
        Ok(())
    }

    /// Reads blocks that [`DraftDesign::VaryEachAlone`] writes, and the Parameters tab values they hold varied
    /// parameters at outside their own block.
    fn read_vary_each_alone(
        &mut self,
        blocks: &[BlockSpec],
        schema: &ModelSchema<'_>,
        panel_values: &mut [Option<ParamValue>],
    ) -> Result<(), String> {
        let refusal = || {
            format!(
                "Spec holds {} blocks. Load a spec with one block or a One at a time design.",
                blocks.len()
            )
        };
        let param_ids: Vec<&str> = blocks[0]
            .factors
            .iter()
            .map_while(|factor| match &factor.target {
                FactorTarget::Param(id) => Some(id.as_str()),
                FactorTarget::Action(_) => None,
            })
            .collect();
        if blocks.len() < param_ids.len() {
            return Err(refusal());
        }
        let mut baselines: Vec<Option<&str>> = vec![None; param_ids.len()];
        let mut action_names: Vec<&str> = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            if block.design != DesignKind::Factorial || block.design_seed.is_some() {
                return Err(refusal());
            }
            let own_action = usize::from(index >= param_ids.len());
            if block.factors.len() != param_ids.len() + own_action {
                return Err(refusal());
            }
            let (param_factors, action_factors) = block.factors.split_at(param_ids.len());
            for (position, (factor, id)) in param_factors.iter().zip(&param_ids).enumerate() {
                if !matches!(&factor.target, FactorTarget::Param(target) if target == id) {
                    return Err(refusal());
                }
                if position == index {
                    continue;
                }
                let LevelSpec::Values(values) = &factor.levels else {
                    return Err(refusal());
                };
                let [value] = values.as_slice() else {
                    return Err(refusal());
                };
                match baselines[position] {
                    Some(baseline) if baseline != value => return Err(refusal()),
                    _ => baselines[position] = Some(value),
                }
            }
            for factor in action_factors {
                match &factor.target {
                    FactorTarget::Action(name) if !action_names.contains(&name.as_str()) => {
                        action_names.push(name);
                    }
                    _ => return Err(refusal()),
                }
            }
        }

        self.design = DraftDesign::VaryEachAlone;
        for (index, block) in blocks.iter().enumerate() {
            // A parameter's own block varies it at its position, and an action's block ends with the action.
            let own_factor = &block.factors[index.min(param_ids.len())];
            self.mark_varied(&own_factor.target, &own_factor.levels, schema)?;
        }
        for (id, baseline) in param_ids.iter().zip(baselines) {
            if let Some(text) = baseline {
                let index = param_index(schema, id)?;
                panel_values[index] = Some(read_value(schema, index, text)?);
            }
        }
        Ok(())
    }

    /// Marks the parameter or action `target` varied over `levels`.
    fn mark_varied(
        &mut self,
        target: &FactorTarget,
        levels: &LevelSpec,
        schema: &ModelSchema<'_>,
    ) -> Result<(), String> {
        match target {
            FactorTarget::Param(id) => {
                let index = param_index(schema, id)?;
                self.factors[index] = FactorDraft {
                    vary: true,
                    levels_text: levels_text(levels),
                };
            }
            FactorTarget::Action(name) => {
                let action = self
                    .actions
                    .iter_mut()
                    .find(|action| action.name == *name)
                    .ok_or_else(|| format!("Spec varies the tick of action {name} but never runs it"))?;
                action.vary_tick = true;
                action.ticks_text = levels_text(levels);
            }
        }
        Ok(())
    }

    /// Returns the row of the parameter or action tick `target`.
    fn target_site(&self, target: &FactorTarget, schema: &ModelSchema<'_>) -> DraftSite {
        match target {
            FactorTarget::Param(id) => schema
                .params
                .iter()
                .position(|descriptor| descriptor.id == *id)
                .map_or(DraftSite::Sweep, DraftSite::Factor),
            FactorTarget::Action(name) => self.action_site(name),
        }
    }

    /// Returns the issue of a factor the plan or the search space refuses, placed on the row it names.
    fn factor_issue(&self, error: &FactorError, schema: &ModelSchema<'_>) -> DraftIssue {
        let site = match error {
            FactorError::Level { id, .. } | FactorError::MissingStep { id } | FactorError::RangeOverOptions { id } => {
                self.target_site(&FactorTarget::Param(id.clone()), schema)
            }
            FactorError::BadTick { name, .. } => self.action_site(name),
            FactorError::Range { target, .. }
            | FactorError::NotWhole { target, .. }
            | FactorError::AllOverNumber { target }
            | FactorError::NoLevels { target } => self.target_site(target, schema),
            FactorError::UnknownParam { .. } | FactorError::UnknownAction { .. } => DraftSite::Sweep,
        };
        DraftIssue::new(site, factor_message(error))
    }

    /// Returns the issue of a search the model or its settings refuse, placed on the row or section it names.
    fn search_issue(&self, error: &SearchPlanError, spec: &SweepSpec, schema: &ModelSchema<'_>) -> DraftIssue {
        match error {
            SearchPlanError::Plan(source) => self.plan_issue(source, spec, schema),
            SearchPlanError::Settings(source @ SearchSpecError::Setting { key, .. }) => {
                DraftIssue::new(Self::setting_site(key), setting_message(source))
            }
            SearchPlanError::Settings(SearchSpecError::MissingObjective { .. }) => {
                DraftIssue::missing(DraftSite::Objective, "Select output")
            }
            SearchPlanError::Settings(source) => DraftIssue::new(DraftSite::Search, setting_message(source)),
            SearchPlanError::Space(SearchSpaceError::NoFactors) => DraftIssue::missing(
                DraftSite::Parameters,
                "Select a parameter or an action's Search tick to search over it",
            ),
            SearchPlanError::Space(SearchSpaceError::NoLevels { factor_index }) => {
                let site = spec
                    .search
                    .as_ref()
                    .and_then(|search| search.space.get(*factor_index))
                    .map_or(DraftSite::Search, |factor| self.target_site(&factor.target, schema));
                DraftIssue::new(site, "No values")
            }
            SearchPlanError::Space(SearchSpaceError::Factor(source)) => self.factor_issue(source, schema),
            SearchPlanError::UnknownColumn { column, .. } => {
                let site = self
                    .search
                    .watched_columns()
                    .into_iter()
                    .find(|(_, watched)| watched == column)
                    .map_or(DraftSite::Search, |(site, _)| site);
                DraftIssue::new(site, format!("{} is missing from Outputs", output_label(column)))
            }
            _ => DraftIssue::new(DraftSite::Search, describe_error(error)),
        }
    }

    /// Returns the issue of a plan error, placed on the row of the factor, action or output it names.
    fn plan_issue(&self, error: &PlanError, spec: &SweepSpec, schema: &ModelSchema<'_>) -> DraftIssue {
        match error {
            PlanError::Factor { source, .. } => self.factor_issue(source, schema),
            PlanError::Design { block, source } => {
                let site = match source {
                    DesignError::UnlistedLevels { factor_index } => spec
                        .blocks
                        .get(*block)
                        .and_then(|block| block.factors.get(*factor_index))
                        .map_or(DraftSite::Design, |factor| self.target_site(&factor.target, schema)),
                    _ => DraftSite::Design,
                };
                DraftIssue::new(site, describe_error(source))
            }
            PlanError::Table { source, .. } => DraftIssue::new(DraftSite::Design, describe_error(source)),
            PlanError::Measure(source) => self.measure_issue(source),
            PlanError::Stop(source) => DraftIssue::new(DraftSite::Stop, describe_error(source)),
            PlanError::NoReplicates => DraftIssue::new(DraftSite::Replicates, "Replicates must be at least 1"),
            PlanError::DuplicateActionName { name } => DraftIssue::new(self.action_site(name), describe_error(error)),
            _ => DraftIssue::new(DraftSite::Sweep, describe_error(error)),
        }
    }

    /// Returns the issue of measurement settings the plan refuses, in the words of the tab's labels.
    fn measure_issue(&self, error: &MeasureError) -> DraftIssue {
        match error {
            MeasureError::ZeroStatsEvery => DraftIssue::new(DraftSite::Sampling, "Sample every must be at least 1"),
            MeasureError::SeriesOffCadence { .. } => {
                DraftIssue::new(DraftSite::Sampling, self.series_cadence_message())
            }
            MeasureError::Reducer(source) => {
                let position = match source {
                    ReducerError::UnknownColumn { column, .. } | ReducerError::MissingKind { raw: column } => {
                        self.reducers.iter().position(|reducer| reducer.column == *column)
                    }
                    ReducerError::UnknownKind { raw }
                    | ReducerError::Comparison { raw, .. }
                    | ReducerError::BadWindow { raw } => self
                        .reducers
                        .iter()
                        .position(|reducer| reducer.kind.to_string() == *raw),
                };
                DraftIssue::new(
                    position.map_or(DraftSite::Sweep, DraftSite::Output),
                    describe_error(source),
                )
            }
            MeasureError::Stop(source) => DraftIssue::new(DraftSite::Stop, describe_error(source)),
            MeasureError::TooManyTicks { .. } => DraftIssue::new(DraftSite::RunLength, describe_error(error)),
        }
    }

    fn action_site(&self, name: &str) -> DraftSite {
        self.actions
            .iter()
            .position(|action| action.name == name)
            .map_or(DraftSite::Sweep, DraftSite::Action)
    }
}

/// Returns the blocks of [`DraftDesign::VaryEachAlone`]: one per varied parameter, then one per varied action tick.
///
/// Every block lists every varied parameter. Outside its own block a parameter takes its value in `panel_values`, and
/// an action its fixed tick.
fn vary_each_alone_blocks(
    schema: &ModelSchema<'_>,
    panel_values: &[ParamValue],
    params: &[(usize, FactorSpec)],
    actions: Vec<FactorSpec>,
) -> Vec<BlockSpec> {
    let baseline = |index: usize| {
        let descriptor = &schema.params[index];
        let value = panel_values
            .get(index)
            .cloned()
            .unwrap_or_else(|| descriptor.kind.default_value());
        FactorSpec::param(
            descriptor.id,
            LevelSpec::Values(vec![format_value(&descriptor.kind, &value)]),
        )
    };
    let baselines = |own_index: Option<usize>| -> Vec<FactorSpec> {
        params
            .iter()
            .map(|(index, factor)| {
                if Some(*index) == own_index {
                    factor.clone()
                } else {
                    baseline(*index)
                }
            })
            .collect()
    };
    let factorial_block = |factors| BlockSpec {
        design: DesignKind::Factorial,
        factors,
        design_seed: None,
    };
    params
        .iter()
        .map(|(index, _)| factorial_block(baselines(Some(*index))))
        .chain(actions.into_iter().map(|action| {
            let mut factors = baselines(None);
            factors.push(action);
            factorial_block(factors)
        }))
        .collect()
}

/// Returns the text a parameter's levels start as: `all` for a bool or a choice, and nothing for a number.
fn default_levels_text(kind: &ParamKind) -> String {
    match kind {
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => "all".to_owned(),
        ParamKind::F32 { .. } | ParamKind::U32 { .. } => String::new(),
    }
}

/// Returns an example of the values a parameter of `kind` takes, as a hint and in the note of an empty row.
///
/// A range steps by about a quarter of it, rounded to one digit of 1, 2 or 5, and whole for a whole number.
pub fn levels_example(kind: &ParamKind) -> String {
    let (min, max) = match *kind {
        ParamKind::F32 { min, max, .. } => (f64::from(min), f64::from(max)),
        ParamKind::U32 { min, max, .. } => (f64::from(min), f64::from(max)),
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => return "all".to_owned(),
    };
    let step = match kind {
        ParamKind::U32 { .. } => round_step((max - min) / 4.0).round().max(1.0),
        _ => round_step((max - min) / 4.0),
    };
    if step.is_finite() && step > 0.0 && max > min {
        format!(
            "{}:{}:{}",
            number_text(kind, min),
            number_text(kind, max),
            number_text(kind, step)
        )
    } else {
        format!("{},{}", number_text(kind, min), number_text(kind, max))
    }
}

/// Returns `number` as a value of a parameter of `kind` is written: to the precision of an `f32`, or whole.
pub fn number_text(kind: &ParamKind, number: f64) -> String {
    match kind {
        ParamKind::U32 { .. } => format!("{}", number.round().max(0.0) as u64),
        _ => format!("{}", number as f32),
    }
}

/// Returns `value` rounded to 1, 2 or 5 times a power of ten.
pub fn round_step(value: f64) -> f64 {
    let scale = 10_f64.powi(value.log10().floor() as i32);
    let multiple = match value / scale {
        fraction if fraction < 1.5 => 1.0,
        fraction if fraction < 3.5 => 2.0,
        fraction if fraction < 7.5 => 5.0,
        _ => 10.0,
    };
    multiple * scale
}

/// Returns a parameter's whole range as `min:max`, or `all` for a bool or a choice.
pub fn whole_range_text(kind: &ParamKind) -> String {
    match kind {
        ParamKind::F32 { min, max, .. } => format!("{min}:{max}"),
        ParamKind::U32 { min, max, .. } => format!("{min}:{max}"),
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => "all".to_owned(),
    }
}

/// Reads the levels of a row's `text`, or returns the kind and message of its issue. An empty row is missing input,
/// described by `missing`.
///
/// A range of more than [`MAX_DRAFT_LEVELS`] values is refused, unless it has no step and `draws_ranges` is set. A
/// sampled design or a search draws its values from such a range and lists none of them.
fn row_levels(
    text: &str,
    draws_ranges: bool,
    missing: impl FnOnce() -> String,
) -> Result<LevelSpec, (IssueKind, String)> {
    if text.trim().is_empty() {
        return Err((IssueKind::Missing, missing()));
    }
    let levels = parse_levels(text).map_err(|message| (IssueKind::Invalid, message))?;
    let enumerated = match levels {
        LevelSpec::Range { step: None, .. } => !draws_ranges,
        LevelSpec::Range { step: Some(_), .. } => true,
        LevelSpec::Values(_) | LevelSpec::All => false,
    };
    match level_estimate(&levels) {
        count if enumerated && count > MAX_DRAFT_LEVELS => Err((
            IssueKind::Invalid,
            format!("Range gives {count} values, over the limit of {MAX_DRAFT_LEVELS}"),
        )),
        _ => Ok(levels),
    }
}

/// Returns the preview of parameter `descriptor`'s resolved `factor`, each value as the Parameters tab shows it.
fn param_preview(descriptor: &ParamDescriptor, factor: &Factor) -> LevelPreview {
    LevelPreview::of(
        factor,
        |level| match level {
            FactorLevel::Param(value) => display_value(descriptor, value),
            FactorLevel::Tick(tick) => tick.to_string(),
        },
        |bound| {
            let value = match descriptor.kind {
                ParamKind::U32 { .. } => ParamValue::U32(bound as u32),
                _ => ParamValue::F32(bound as f32),
            };
            display_value(descriptor, &value)
        },
    )
}

/// Returns the text of an action's tick `level`.
fn tick_text(level: &FactorLevel) -> String {
    match level {
        FactorLevel::Tick(tick) => tick.to_string(),
        FactorLevel::Param(_) => String::new(),
    }
}

/// Returns the issue of a zip over rows `listed`, each a label and a number of values, when the numbers differ.
fn zip_issue(listed: &[(String, usize)]) -> Option<DraftIssue> {
    let first = listed.first()?.1;
    if listed.iter().all(|(_, count)| *count == first) {
        return None;
    }
    let lists: Vec<String> = listed
        .iter()
        .map(|(label, count)| format!("{label} has {count}"))
        .collect();
    Some(DraftIssue::new(
        DraftSite::Design,
        format!("Use lists of equal length for Zip. {}.", lists.join(", ")),
    ))
}

/// Returns the kind and message of the issue of a grid axis's range, `None` for an automatic range or a range the
/// search takes.
///
/// A range from 0 to 0 is the one an axis's fields start with, and is missing input.
fn axis_range_issue(axis: &PatternAxis) -> Option<(IssueKind, String)> {
    let (min, max) = axis.range()?;
    if min == 0.0 && max == 0.0 {
        Some((IssueKind::Missing, AXIS_RANGE_MISSING.to_owned()))
    } else if !min.is_finite() {
        Some((IssueKind::Invalid, "Minimum must be a finite number".to_owned()))
    } else if !(max.is_finite() && max > min) {
        Some((IssueKind::Invalid, "Maximum must be greater than Minimum".to_owned()))
    } else {
        None
    }
}

/// Returns the evaluations of each generation after the first of a genetic algorithm with `settings`: the members it
/// re-evaluates and a child for every member but the elites.
fn later_generation_size(settings: &GeneticSettings) -> u64 {
    let children = settings.population.saturating_sub(settings.elite_count);
    (settings.reevaluation_count() + children) as u64
}

/// Returns about the number of generations a genetic algorithm with `settings` starts within `max_evaluations`, a
/// partial last one included, or `None` for a budget smaller than one generation.
///
/// Generation 0 evaluates the whole population. Note that a child whose configuration an earlier candidate has can
/// leave a generation smaller than planned.
pub fn generation_estimate(max_evaluations: u64, settings: &GeneticSettings) -> Option<u64> {
    let population = settings.population as u64;
    if max_evaluations < population {
        return None;
    }
    match later_generation_size(settings) {
        0 => Some(1),
        later => Some(1 + (max_evaluations - population).div_ceil(later)),
    }
}

/// Returns about the number of batches of at most `batch_size` candidates that `max_evaluations` take, for a genetic
/// algorithm with `genetic` settings when set.
///
/// No batch of a genetic algorithm spans two generations.
pub fn batch_estimate(max_evaluations: u64, batch_size: usize, genetic: Option<&GeneticSettings>) -> u64 {
    let batch = (batch_size as u64).max(1);
    let Some(settings) = genetic else {
        return max_evaluations.div_ceil(batch);
    };
    let population = settings.population as u64;
    let later = later_generation_size(settings);
    if max_evaluations <= population || later == 0 {
        return max_evaluations.div_ceil(batch);
    }
    let rest = max_evaluations - population;
    population.div_ceil(batch) + rest / later * later.div_ceil(batch) + (rest % later).div_ceil(batch)
}

/// Returns the key and message of every setting of `settings` that `check` refuses.
///
/// `repair` sets the refused setting named by its key to a value `check` takes, so the next check reaches the next
/// setting.
fn every_refusal<S>(
    mut settings: S,
    check: impl Fn(&S) -> Result<(), SearchSpecError>,
    repair: impl Fn(&mut S, &str),
) -> Vec<(&'static str, String)> {
    let mut refusals = Vec::new();
    for _ in 0..MAX_SETTING_CHECKS {
        match check(&settings) {
            Err(error @ SearchSpecError::Setting { key, .. }) => {
                if refusals.iter().any(|(refused, _)| *refused == key) {
                    break;
                }
                refusals.push((key, setting_message(&error)));
                repair(&mut settings, key);
            }
            Err(_) | Ok(()) => break,
        }
    }
    refusals
}

/// Reads levels as `--vary` takes them, with spaces around each listed value ignored.
///
/// Note that a range of any length reads. The check refuses one of more than [`MAX_DRAFT_LEVELS`] values where the
/// draft lists them.
///
/// # Errors
///
/// Returns a message for text with no values, or a range that does not read.
pub fn parse_levels(text: &str) -> Result<LevelSpec, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("No values".to_owned());
    }
    match LevelSpec::parse(text).map_err(|error| level_spec_message(&error))? {
        LevelSpec::Values(values) => {
            let values: Vec<String> = values
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
            if values.is_empty() {
                return Err("No values".to_owned());
            }
            Ok(LevelSpec::Values(values))
        }
        levels => Ok(levels),
    }
}

/// Returns the message of text that does not read as levels, in the words of the Sweep tab.
fn level_spec_message(error: &LevelSpecError) -> String {
    match error {
        LevelSpecError::NotANumber { part, .. } => format!("'{}' is not a number", part.trim()),
        LevelSpecError::BadRange { range } => format!("'{range}' is not a range. Enter min:max:step or min:max."),
    }
}

/// Returns the message of a factor the plan refuses, in the words of the Sweep tab.
///
/// The row the issue lands on names the parameter or the action, so the message leaves it out.
fn factor_message(error: &FactorError) -> String {
    match error {
        FactorError::Level { source, .. } => value_message(source),
        FactorError::BadTick { raw, .. } => format!("'{raw}' is not a non-negative integer"),
        FactorError::Range { source, .. } => capitalize(&source.to_string()),
        FactorError::MissingStep { .. } => {
            "Range needs a step, as in min:max:step. For min:max, select Latin hypercube or Uniform random under \
             Design."
                .to_owned()
        }
        FactorError::NotWhole { value, .. } => format!("{value} is not an integer"),
        FactorError::AllOverNumber { .. } => "Only checkbox and dropdown parameters accept 'all'".to_owned(),
        FactorError::RangeOverOptions { .. } => {
            "This parameter accepts listed values or 'all'. Ranges apply only to number parameters.".to_owned()
        }
        FactorError::NoLevels { .. } => "No values".to_owned(),
        FactorError::UnknownParam { .. } | FactorError::UnknownAction { .. } => describe_error(error),
    }
}

/// Returns the message of search settings the search refuses, in the words of the Sweep tab.
fn setting_message(error: &SearchSpecError) -> String {
    match error {
        SearchSpecError::NoEvaluations => "Evaluations must be at least 1".to_owned(),
        SearchSpecError::NoBatch => "Batch size must be at least 1".to_owned(),
        SearchSpecError::MissingObjective { .. } => "Objective: Select output".to_owned(),
        SearchSpecError::Setting { key, value, expected } => {
            format!("{} must be {expected}, got {value}", setting_label(key))
        }
        SearchSpecError::UnusedObjective => describe_error(error),
    }
}

/// Returns the label of the field that edits the search setting `key`, as in `genetic.elite_count`.
fn setting_label(key: &str) -> &str {
    match key {
        "hill_climb.mutation_scale" | "genetic.mutation_scale" | "pse.mutation_scale" => "Step size",
        "hill_climb.patience" => "Patience",
        "genetic.population" => "Population",
        "genetic.elite_count" => "Elites",
        "genetic.tournament_size" => "Tournament size",
        "genetic.crossover_rate" => "Crossover rate",
        "genetic.mutation_rate" => "Mutation rate",
        "genetic.reevaluate_fraction" => "Re-evaluation rate",
        INITIAL_SAMPLES_KEY => "Initial samples",
        "pse.x_axis.min" => "X axis Minimum",
        "pse.x_axis.max" => "X axis Maximum",
        "pse.x_axis.cells" => "X axis Cells",
        "pse.y_axis.min" => "Y axis Minimum",
        "pse.y_axis.max" => "Y axis Maximum",
        "pse.y_axis.cells" => "Y axis Cells",
        other => other,
    }
}

/// Returns the message of a value a parameter refuses, in the words of the Sweep tab.
fn value_message(error: &ValueError) -> String {
    match error {
        ValueError::NotANumber { raw, .. } => format!("'{raw}' is not a number"),
        ValueError::NotAnInteger { raw, .. } => format!("'{raw}' is not an integer"),
        ValueError::NotABool { raw, .. } => format!("'{raw}' is not true or false"),
        ValueError::OutOfRange { value, min, max } => format!("{value} is outside the range {min} to {max}"),
        ValueError::UnknownOption { raw, options } => format!("'{raw}' is not one of {}", options.join(", ")),
        ValueError::Param { source, .. } => value_message(source),
        ValueError::UnknownParam { .. } | ValueError::BadOverride { .. } => describe_error(error),
    }
}

/// Returns `levels` in the text [`parse_levels`] reads.
pub fn levels_text(levels: &LevelSpec) -> String {
    match levels {
        LevelSpec::Values(values) => values.join(","),
        LevelSpec::Range {
            min,
            max,
            step: Some(step),
        } => format!("{min}:{max}:{step}"),
        LevelSpec::Range { min, max, step: None } => format!("{min}:{max}"),
        LevelSpec::All => "all".to_owned(),
    }
}

/// Returns the number of values `levels` lists, give or take one for a range, and 1 for levels a design draws from.
fn level_estimate(levels: &LevelSpec) -> u64 {
    match levels {
        LevelSpec::Values(values) => values.len() as u64,
        LevelSpec::Range { min, max, step } => {
            let span = (max - min) / step.unwrap_or(1.0);
            if span.is_finite() && span >= 0.0 {
                (span as u64).saturating_add(1)
            } else {
                1
            }
        }
        LevelSpec::All => 1,
    }
}

/// Returns about the number of configurations of `spec`, cheap enough to count before planning.
fn estimated_configs(spec: &SweepSpec) -> u64 {
    if spec.blocks.is_empty() {
        return 1;
    }
    spec.blocks
        .iter()
        .map(|block| {
            let counts = block.factors.iter().map(|factor| level_estimate(&factor.levels));
            match &block.design {
                DesignKind::Factorial => counts.fold(1, u64::saturating_mul),
                DesignKind::Zip => counts.max().unwrap_or(1),
                DesignKind::Random { samples } | DesignKind::LatinHypercube { samples } => *samples as u64,
                DesignKind::Table { text } => table_row_count(text) as u64,
            }
        })
        .fold(0, u64::saturating_add)
}

/// Returns the rows of the design table `text` after its header, blank lines left out.
fn table_row_count(text: &str) -> usize {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .saturating_sub(1)
}

/// Returns about the bytes of `series.csv` that `run_count` runs with the settings of `plan` write, for a model of
/// `column_count` stat columns.
///
/// Every run counts as running to its last step.
fn estimated_series_bytes(plan: &Plan, run_count: u64, column_count: usize) -> u64 {
    let series_every = plan.measure_settings().series_every;
    if series_every == 0 {
        return 0;
    }
    let rows = (plan.run_settings().steps / series_every).saturating_add(1);
    let row_bytes = (column_count as u64 + 2) * SERIES_VALUE_BYTES;
    run_count.saturating_mul(rows).saturating_mul(row_bytes)
}

/// Returns the issue of a sweep held in memory whose series take about `bytes`.
fn memory_refusal(bytes: u64) -> String {
    let (size, limit) = (fmt_bytes(bytes), fmt_bytes(MAX_MEMORY_SERIES_BYTES));
    if cfg!(target_arch = "wasm32") {
        format!(
            "Series take about {size}, over the browser's limit of {limit}. Increase Series every, or run the sweep \
             in the desktop app."
        )
    } else {
        format!(
            "Series take about {size} in memory, over the limit of {limit}. Select In a folder under Results, or \
             increase Series every."
        )
    }
}

/// Returns the output column `reducer` writes among the stat columns `columns`, as in `Velocity.magnitude:max` for a
/// reducer over the bare label `Velocity`.
///
/// A column that `columns` lacks, or any column while `columns` is `None`, is written as the reducer names it.
pub fn written_name(reducer: &ReducerSpec, columns: Option<&StatColumns>) -> String {
    let column = columns
        .and_then(|columns| Some(columns.name(columns.resolve(&reducer.column)?)))
        .unwrap_or(&reducer.column);
    format!("{column}:{}", reducer.kind)
}

/// Returns the stat columns of `schema`'s stats, each one column as a scalar stat is.
fn scalar_columns(schema: &ModelSchema<'_>) -> StatColumns {
    let stats: Vec<StatEntry> = schema
        .stats
        .iter()
        .map(|stat| StatEntry {
            label: stat.label,
            value: StatValue::Scalar(0.0),
            color: stat.color,
        })
        .collect();
    StatColumns::plan(&stats)
}

/// Reads a seed typed as a whole number from 0 to `u64::MAX`.
fn parse_seed_text(text: &str) -> Result<u64, String> {
    let text = text.trim();
    text.parse()
        .ok()
        .ok_or_else(|| format!("'{text}' is not an integer from 0 to {}", u64::MAX))
}

fn param_index(schema: &ModelSchema<'_>, id: &str) -> Result<usize, String> {
    schema
        .params
        .iter()
        .position(|descriptor| descriptor.id == id)
        .ok_or_else(|| format!("{} has no parameter {id}", schema.id))
}

fn read_value(schema: &ModelSchema<'_>, index: usize, text: &str) -> Result<ParamValue, String> {
    parse_value(&schema.params[index].kind, text).map_err(|error| describe_error(&error))
}

/// Returns `error` and each of its causes, joined by colons, starting with a capital letter.
pub fn describe_error(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        text.push_str(": ");
        text.push_str(&source.to_string());
        cause = source.source();
    }
    capitalize(&text)
}

/// Returns the words the Sweep tab reads `comparator` as, as in "at most".
pub fn comparator_words(comparator: Comparator) -> &'static str {
    match comparator {
        Comparator::Less => "below",
        Comparator::LessOrEqual => "at most",
        Comparator::Equal => "equal to",
        Comparator::NotEqual => "not equal to",
        Comparator::GreaterOrEqual => "at least",
        Comparator::Greater => "above",
    }
}

/// Returns `text` with its first letter in upper case.
pub fn capitalize(text: &str) -> String {
    let mut characters = text.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::time::Duration;

    use henad_compute::entry::ModelEntry;
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::measure::MeasurePlan;
    use henad_core::explore::plan::ModelSchema;
    use henad_core::explore::reducer::{ReducerKind, ReducerSpec};
    use henad_core::explore::search::genetic::GeneticSettings;
    use henad_core::explore::search::pse::PatternAxis;
    use henad_core::explore::search::{Aggregate, Goal};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_core::explore::stop::{Comparator, Comparison};
    use henad_core::export::StatColumns;
    use henad_core::params::ParamValue;
    use henad_explore::exec::Concurrency;
    use henad_explore::probe::ProbeReport;
    use henad_explore::spec_file::SpecFile;
    use henad_models::example_models;

    use super::{
        COLUMNS_PENDING, DesignTableDraft, DraftAlgorithm, DraftDesign, DraftIssue, DraftMode, DraftSite, GridAxis,
        INITIAL_SAMPLES_KEY, IssueKind, LevelCount, LevelPreview, MAX_DRAFT_LEVELS, NOT_FINITE_THRESHOLD,
        SPEC_TABLE_NAME, StopDraft, SweepDraft, TickSource, batch_estimate, comparator_words, estimated_configs,
        generation_estimate, parse_levels, written_name,
    };
    use crate::ui::sweep::parameters::{EditorSegment, segment_text};

    // Indices of SIR's parameters.
    const INFECTION_RATE: usize = 2;
    const RECOVERY_RATE: usize = 3;

    fn sir() -> ModelEntry {
        example_models().get("sir").cloned().expect("SIR is registered")
    }

    fn boids() -> ModelEntry {
        example_models().get("boids").cloned().expect("boids is registered")
    }

    fn default_values(entry: &ModelEntry) -> Vec<ParamValue> {
        entry
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect()
    }

    /// Returns the stat columns of a build of `entry` at its default values, as a sweep's probe samples them.
    fn sampled_columns(entry: &ModelEntry) -> StatColumns {
        ProbeReport::build(entry, None, &default_values(entry), None)
            .unwrap_or_else(|error| panic!("{} does not build: {error}", entry.id()))
            .columns
    }

    /// Returns SIR's defaults on a 32 by 32 grid, with an infection rate of 0.3 and a recovery rate of 0.05.
    fn panel_values(entry: &ModelEntry) -> Vec<ParamValue> {
        let mut values: Vec<ParamValue> = entry
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        values[0] = ParamValue::U32(32);
        values[1] = ParamValue::U32(32);
        values[INFECTION_RATE] = ParamValue::F32(0.3);
        values[RECOVERY_RATE] = ParamValue::F32(0.05);
        values
    }

    /// Returns a draft of SIR that varies both rates and an action's tick, and sets every other field away from its
    /// default.
    fn varied_draft(entry: &ModelEntry, design: DraftDesign) -> SweepDraft {
        let schema = entry.schema();
        let mut draft = SweepDraft::new(&schema);
        draft.factors[INFECTION_RATE].vary = true;
        draft.factors[INFECTION_RATE].levels_text = "0.1,0.2,0.4".to_owned();
        draft.factors[RECOVERY_RATE].vary = true;
        draft.factors[RECOVERY_RATE].levels_text = "0.02:0.06:0.02".to_owned();
        draft.design = design;
        draft.add_action(&schema, 0, 50);
        draft.actions[0].vary_tick = true;
        draft.actions[0].ticks_text = "20,40,60".to_owned();
        draft.replicates = 3;
        draft.root_seed_text = "42".to_owned();
        draft.common_random_numbers = false;
        draft.steps = 200;
        draft.warmup = 10;
        draft.stats_every = 5;
        draft.series_every = 10;
        draft.stop = Some(StopDraft {
            column: "Infected".to_owned(),
            comparator: Comparator::LessOrEqual,
            threshold: 0.0,
            min_tick: 20,
        });
        draft.timeout_s = Some(30.0);
        draft.default_reducers = false;
        draft.reducers = vec![
            ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::ArgMax,
            },
            ReducerSpec {
                column: "Recovered".to_owned(),
                kind: ReducerKind::FirstCrossing(Comparison {
                    comparator: Comparator::GreaterOrEqual,
                    threshold: 100.0,
                }),
            },
        ];
        draft.concurrency = Concurrency::Fixed(NonZeroUsize::new(2).expect("2 is not zero"));
        if design.is_sampled() {
            draft.samples = 12;
            draft.design_seed_text = "7".to_owned();
            draft.factors[RECOVERY_RATE].levels_text = "0.02:0.06".to_owned();
        }
        draft
    }

    /// Writes `draft` as TOML, reads it back, and returns the draft and panel values it reads as.
    fn round_trip(entry: &ModelEntry, draft: &SweepDraft) -> (SweepDraft, Vec<Option<ParamValue>>) {
        let schema = entry.schema();
        let text = draft
            .to_toml(&schema, &panel_values(entry))
            .unwrap_or_else(|issues| panic!("the draft writes no spec: {issues:?}"));
        let file =
            SpecFile::parse(&text).unwrap_or_else(|error| panic!("the TOML does not read back: {error}\n{text}"));
        SweepDraft::from_spec_file(file, &schema).unwrap_or_else(|message| panic!("{message}\n{text}"))
    }

    #[test]
    fn a_draft_reads_back_from_its_toml_under_every_design() {
        let entry = sir();
        let schema = entry.schema();
        for design in DraftDesign::ALL {
            let mut draft = varied_draft(&entry, design);
            if design == DraftDesign::Table {
                draft.table = Some(DesignTableDraft {
                    file_name: SPEC_TABLE_NAME.to_owned(),
                    text: "infection_rate,action.seed_outbreak\n0.1,20\n0.4,60\n".to_owned(),
                });
            }
            let (reread, _) = round_trip(&entry, &draft);
            // A table design leaves the vary marks of the rows it ignores. The file cannot carry those marks.
            if design == DraftDesign::Table {
                assert_eq!(reread.table, draft.table, "the design table");
                assert_eq!(
                    reread.to_spec(&schema, &panel_values(&entry)),
                    draft.to_spec(&schema, &panel_values(&entry)),
                    "the table's spec"
                );
                continue;
            }
            assert_eq!(reread, draft, "{design:?} read back as another draft");
        }
    }

    #[test]
    fn a_read_back_spec_pins_the_values_it_holds_fixed() {
        let entry = sir();
        let (_, pinned) = round_trip(&entry, &varied_draft(&entry, DraftDesign::EveryCombination));
        let expected: Vec<Option<ParamValue>> = panel_values(&entry)
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index != INFECTION_RATE && index != RECOVERY_RATE).then_some(value))
            .collect();
        assert_eq!(pinned, expected, "the fixed values, and nothing for the varied rates");
    }

    #[test]
    fn vary_each_alone_writes_one_block_per_parameter() {
        let entry = sir();
        let schema = entry.schema();
        let draft = varied_draft(&entry, DraftDesign::VaryEachAlone);
        let planned = draft
            .check(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the draft does not plan: {issues:?}"));
        let blocks = &planned.spec.blocks;

        assert_eq!(
            blocks.len(),
            3,
            "one block per varied rate, and one for the varied tick"
        );
        let values = |raw: &[&str]| LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect());
        let range = LevelSpec::Range {
            min: 0.02,
            max: 0.06,
            step: Some(0.02),
        };
        assert_eq!(
            blocks[0].factors,
            [
                FactorSpec::param("infection_rate", values(&["0.1", "0.2", "0.4"])),
                FactorSpec::param("recovery_rate", values(&["0.05"])),
            ],
            "the infection rate varies, and the recovery rate stays at its panel value"
        );
        assert_eq!(
            blocks[1].factors,
            [
                FactorSpec::param("infection_rate", values(&["0.3"])),
                FactorSpec::param("recovery_rate", range),
            ],
            "the recovery rate varies, and the infection rate stays at its panel value"
        );
        assert_eq!(
            blocks[2].factors,
            [
                FactorSpec::param("infection_rate", values(&["0.3"])),
                FactorSpec::param("recovery_rate", values(&["0.05"])),
                FactorSpec::action("seed_outbreak", values(&["20", "40", "60"])),
            ],
            "the tick varies, and both rates stay at their panel values"
        );
        assert!(blocks.iter().all(|block| block.design == DesignKind::Factorial));
        assert!(
            planned
                .spec
                .fixed
                .iter()
                .all(|(id, _)| id != "infection_rate" && id != "recovery_rate"),
            "a varied parameter is also fixed: {:?}",
            planned.spec.fixed
        );
        assert_eq!(planned.plan.configs().len(), 3 + 3 + 3, "configurations");
        assert_eq!(planned.plan.run_count(), 9 * 3, "runs");

        let infection = |config: usize| planned.plan.configs()[config].params[INFECTION_RATE].clone();
        let ticks = |config: usize| planned.plan.configs()[config].action_ticks[0];
        assert_eq!(
            infection(4),
            ParamValue::F32(0.3),
            "the recovery block at the panel value"
        );
        assert_eq!(ticks(0), 50, "the infection block fires the action at its fixed tick");
        assert_eq!(ticks(8), 60, "the tick block's last configuration");

        let (reread, pinned) = round_trip(&entry, &draft);
        assert_eq!(
            reread.design,
            DraftDesign::VaryEachAlone,
            "the blocks read back as one design"
        );
        assert_eq!(reread, draft);
        assert_eq!(
            pinned[INFECTION_RATE],
            Some(ParamValue::F32(0.3)),
            "the panel values return"
        );
        assert_eq!(pinned[RECOVERY_RATE], Some(ParamValue::F32(0.05)));
    }

    #[test]
    fn the_run_count_is_the_plans() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.actions[0].vary_tick = false;
        let planned = draft
            .check(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the draft does not plan: {issues:?}"));
        assert_eq!(planned.plan.configs().len(), 3 * 3, "every combination of both rates");
        assert_eq!(planned.plan.run_count(), 9 * 3, "three replicates each");

        let (params, ticks) = draft.level_previews(&schema);
        let count = |preview: &Option<LevelPreview>| preview.as_ref().map(LevelPreview::count);
        assert_eq!(count(&params[INFECTION_RATE]), Some(LevelCount::Listed(3)));
        assert_eq!(
            params[RECOVERY_RATE],
            Some(LevelPreview::Listed {
                count: 3,
                values: vec!["0.02".to_owned(), "0.04".to_owned(), "0.06".to_owned()],
                last: "0.06".to_owned(),
            }),
            "a stepped range, both ends included"
        );
        assert_eq!(params[0], None, "a parameter that is not varied");
        assert_eq!(ticks, [None], "a tick that is not varied");

        draft.design = DraftDesign::LatinHypercube;
        draft.factors[RECOVERY_RATE].levels_text = "0.02:0.06".to_owned();
        assert_eq!(
            draft.level_previews(&schema).0[RECOVERY_RATE],
            Some(LevelPreview::Drawn {
                min: "0.02".to_owned(),
                max: "0.06".to_owned(),
            })
        );
    }

    #[test]
    fn a_sweep_held_in_memory_is_refused_past_the_series_limit() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = SweepDraft::new(&schema);
        draft.steps = 1 << 24;
        draft.stats_every = 1;
        draft.series_every = 1;
        draft.replicates = 64;
        let issues = draft
            .check(&schema, &panel_values(&entry))
            .expect_err("64 runs of 16 million series rows pass the limit");
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].site, DraftSite::Execution);
        assert!(
            issues[0].message.starts_with("Series take about 60.0 GB"),
            "{}",
            issues[0].message
        );

        draft.results_in_folder = true;
        draft.output_dir_text = "sweep-results".to_owned();
        assert!(
            draft.check(&schema, &panel_values(&entry)).is_ok(),
            "a folder holds any series"
        );
        draft.results_in_folder = false;
        draft.series_every = 1 << 20;
        assert!(
            draft.check(&schema, &panel_values(&entry)).is_ok(),
            "a sparse series fits in memory"
        );
    }

    #[test]
    fn levels_are_read_as_the_command_line_writes_them() {
        let values = |raw: &[&str]| LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect());
        assert_eq!(
            parse_levels(" 0.1 ,0.2, "),
            Ok(values(&["0.1", "0.2"])),
            "spaces and a trailing comma"
        );
        assert_eq!(parse_levels("all"), Ok(LevelSpec::All));
        assert_eq!(
            parse_levels("1:9:2"),
            Ok(LevelSpec::Range {
                min: 1.0,
                max: 9.0,
                step: Some(2.0)
            })
        );
        assert_eq!(parse_levels("  "), Err("No values".to_owned()));
        assert_eq!(parse_levels(" , "), Err("No values".to_owned()));
        assert_eq!(parse_levels("0:1: x"), Err("'x' is not a number".to_owned()));
        assert_eq!(
            parse_levels("0:1:2:3"),
            Err("'0:1:2:3' is not a range. Enter min:max:step or min:max.".to_owned())
        );
        assert_eq!(
            parse_levels("0:1:0.0000001"),
            Ok(LevelSpec::Range {
                min: 0.0,
                max: 1.0,
                step: Some(0.0000001)
            }),
            "a range of any length reads, and the check caps what it lists"
        );
    }

    #[test]
    fn an_issue_lands_on_the_row_it_names() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let sites = |draft: &SweepDraft| -> Vec<DraftSite> {
            draft
                .check(&schema, &panel_values)
                .err()
                .unwrap_or_default()
                .iter()
                .map(|issue| issue.site)
                .collect()
        };

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.factors[INFECTION_RATE].levels_text = "0.1, 5".to_owned();
        assert_eq!(
            sites(&draft),
            [DraftSite::Factor(INFECTION_RATE)],
            "a value past the bound"
        );

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.actions[0].ticks_text = "10:20:x".to_owned();
        draft.root_seed_text = "-1".to_owned();
        assert_eq!(
            sites(&draft),
            [DraftSite::Action(0), DraftSite::Seed],
            "text that does not parse, in the order of the rows"
        );

        let mut draft = varied_draft(&entry, DraftDesign::Zip);
        draft.factors[RECOVERY_RATE].levels_text = "0.02, 0.05".to_owned();
        assert_eq!(sites(&draft), [DraftSite::Design], "a zip of unequal lengths");

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.series_every = 7;
        assert_eq!(
            sites(&draft),
            [DraftSite::Sampling],
            "a series off the sampling cadence"
        );

        let mut draft = varied_draft(&entry, DraftDesign::Table);
        draft.table = None;
        assert_eq!(sites(&draft), [DraftSite::Design], "a table design with no table");

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.reducers[0].kind = ReducerKind::WindowMean { start: 500, end: 100 };
        assert_eq!(
            sites(&draft),
            [DraftSite::Output(0)],
            "a window that ends before it starts"
        );

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        "Nothing".clone_into(&mut draft.reducers[1].column);
        assert_eq!(
            sites(&draft),
            [DraftSite::Output(1)],
            "an output of a stat the model lacks"
        );
    }

    #[test]
    fn a_check_reports_every_row_at_once() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.factors[INFECTION_RATE].levels_text = "0.1, 0.x".to_owned();
        draft.factors[RECOVERY_RATE].levels_text = "0.02, 5".to_owned();
        draft.actions[0].ticks_text = "10, x".to_owned();
        draft.root_seed_text = "seed".to_owned();
        draft.series_every = 7;
        let issues = draft
            .check(&schema, &panel_values(&entry))
            .expect_err("five rows refuse");
        let found: Vec<(DraftSite, &str)> = issues
            .iter()
            .map(|issue| (issue.site, issue.message.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (DraftSite::Factor(INFECTION_RATE), "'0.x' is not a number"),
                (DraftSite::Factor(RECOVERY_RATE), "5 is outside the range 0 to 1"),
                (DraftSite::Action(0), "'x' is not a non-negative integer"),
                (
                    DraftSite::Seed,
                    "Root seed: 'seed' is not an integer from 0 to 18446744073709551615"
                ),
                (
                    DraftSite::Sampling,
                    "Series every must be 0 or a multiple of Sample every (5 ticks)"
                ),
            ]
        );
        assert!(issues.iter().all(|issue| issue.kind == IssueKind::Invalid));
    }

    #[test]
    fn input_not_given_yet_is_missing_and_not_invalid() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let issues = |draft: &SweepDraft| draft.check(&schema, &panel_values).expect_err("the draft waits");
        let only = |draft: &SweepDraft| {
            let issues = issues(draft);
            assert_eq!(issues.len(), 1, "{issues:?}");
            let issue = &issues[0];
            assert_eq!(issue.kind, IssueKind::Missing, "{issue:?}");
            (issue.site, issue.message.clone())
        };

        let mut draft = SweepDraft::new(&schema);
        draft.factors[INFECTION_RATE].vary = true;
        assert_eq!(
            only(&draft),
            (
                DraftSite::Factor(INFECTION_RATE),
                "Enter values, such as 0:1:0.2".to_owned()
            ),
            "a ticked row starts empty"
        );
        draft.design = DraftDesign::LatinHypercube;
        assert_eq!(
            only(&draft).1,
            "Enter values, such as 0:1",
            "a sampled design asks for a range"
        );

        let mut draft = SweepDraft::new(&schema);
        draft.add_action(&schema, 0, 10);
        draft.actions[0].vary_tick = true;
        draft.actions[0].ticks_text.clear();
        assert_eq!(
            only(&draft),
            (DraftSite::Action(0), "Enter ticks, such as 100, 200".to_owned())
        );

        let mut draft = SweepDraft::new(&schema);
        draft.design = DraftDesign::Table;
        assert_eq!(only(&draft), (DraftSite::Design, "No table loaded".to_owned()));

        let mut draft = SweepDraft::new(&schema);
        draft.results_in_folder = true;
        assert_eq!(only(&draft), (DraftSite::Execution, "Select a folder".to_owned()));

        let mut draft = SweepDraft::new(&schema);
        draft.mode = DraftMode::Search;
        assert_eq!(
            only(&draft),
            (
                DraftSite::Parameters,
                "Select a parameter or an action's Search tick to search over it".to_owned()
            ),
            "a search over nothing"
        );
        draft.factors[INFECTION_RATE].vary = true;
        draft.factors[INFECTION_RATE].levels_text = "0:1".to_owned();
        draft.search.objective_column.clear();
        assert_eq!(only(&draft), (DraftSite::Objective, "Select output".to_owned()));
        draft.search.algorithm = DraftAlgorithm::PatternSpace;
        assert!(draft.check(&schema, &panel_values).is_ok(), "both axes start automatic");
        draft.search.set_automatic_range(GridAxis::X, false);
        draft.search.set_automatic_range(GridAxis::Y, false);
        let sites: Vec<DraftSite> = issues(&draft).iter().map(|issue| issue.site).collect();
        assert_eq!(
            sites,
            [DraftSite::Axis(GridAxis::X), DraftSite::Axis(GridAxis::Y)],
            "both fields start with no range"
        );
    }

    #[test]
    fn a_zip_names_the_length_of_every_list() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = varied_draft(&entry, DraftDesign::Zip);
        draft.factors[RECOVERY_RATE].levels_text = "0.02, 0.05".to_owned();
        let issues = draft
            .check(&schema, &panel_values(&entry))
            .expect_err("the lists differ");
        assert_eq!(
            issues,
            [DraftIssue::new(
                DraftSite::Design,
                "Use lists of equal length for Zip. Infection Rate has 3, Recovery Rate has 2, Seed outbreak tick \
                 has 3."
            )]
        );
    }

    #[test]
    fn a_draft_never_writes_its_results_folder_into_a_spec() {
        let entry = sir();
        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.results_in_folder = true;
        draft.output_dir_text = "sweep-01".to_owned();
        let (reread, _) = round_trip(&entry, &draft);
        assert!(!reread.results_in_folder, "a loaded spec starts in memory");
        assert!(reread.output_dir_text.is_empty());
        assert_eq!(
            reread,
            SweepDraft {
                results_in_folder: false,
                output_dir_text: String::new(),
                ..draft
            }
        );
    }

    #[test]
    fn a_new_stop_condition_reads_the_stat_the_draft_watches() {
        let entry = sir();
        let schema = entry.schema();
        let stop_column = |draft: &mut SweepDraft| {
            draft.add_stop(&schema);
            let column = draft.stop.as_ref().expect("a stop condition").column.clone();
            draft.clear_stop();
            column
        };

        let mut draft = SweepDraft::new(&schema);
        assert_eq!(
            stop_column(&mut draft),
            "Susceptible",
            "nothing watched: the first stat"
        );
        draft.add_stop(&schema);
        draft.stop.as_mut().expect("a stop condition").column = "Recovered".to_owned();
        draft.clear_stop();
        assert_eq!(stop_column(&mut draft), "Recovered", "the stat last used");
        draft.reducers.push(ReducerSpec {
            column: "Infected".to_owned(),
            kind: ReducerKind::ArgMax,
        });
        assert_eq!(stop_column(&mut draft), "Infected", "a sweep's first output");

        draft.mode = DraftMode::Search;
        draft.search.objective_column = "Recovered:first<=10".to_owned();
        assert_eq!(stop_column(&mut draft), "Recovered", "the objective's stat");
        draft.search.algorithm = DraftAlgorithm::PatternSpace;
        draft.search.pattern_space.x_axis.column = "Infected:max".to_owned();
        assert_eq!(stop_column(&mut draft), "Infected", "the X axis's stat");
        draft.search.pattern_space.x_axis.column = "Exposed:max".to_owned();
        assert_eq!(
            stop_column(&mut draft),
            "Infected",
            "a stat SIR lacks: the stat last used"
        );

        draft.add_stop(&schema);
        draft.stop.as_mut().expect("a stop condition").threshold = 5.0;
        draft.add_stop(&schema);
        assert_eq!(
            draft.stop.as_ref().map(|stop| stop.threshold),
            Some(5.0),
            "a draft keeps the stop condition it has"
        );
    }

    #[test]
    fn a_row_previews_its_values_as_the_parameters_tab_shows_them() {
        let entry = sir();
        let schema = entry.schema();
        let initial = schema
            .params
            .iter()
            .position(|descriptor| descriptor.id == "initial_infected_pct")
            .expect("SIR has an initial share of infected cells");
        let mut draft = SweepDraft::new(&schema);
        draft.factors[initial].vary = true;
        draft.factors[initial].levels_text = "0.01, 0.02".to_owned();
        let (params, _) = draft.level_previews(&schema);
        let Some(LevelPreview::Listed { values, .. }) = &params[initial] else {
            panic!("the values are listed: {:?}", params[initial]);
        };
        assert!(
            values.iter().all(|value| value.ends_with('%')),
            "a percentage reads as one: {values:?}"
        );
    }

    #[test]
    fn a_search_keeps_the_outputs_it_reads() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = SweepDraft::new(&schema);
        draft.mode = DraftMode::Search;
        draft.search.objective_column = "Infected:max".to_owned();
        assert!(draft.add_watched_output("Recovered:argmax"));
        assert!(
            !draft.add_watched_output("Recovered:argmax"),
            "a row records it already"
        );
        assert!(!draft.add_watched_output("no kind"));

        draft.drop_default_outputs();
        assert!(!draft.default_reducers);
        let columns: Vec<String> = draft
            .reducers
            .iter()
            .map(|reducer| format!("{}:{}", reducer.column, reducer.kind))
            .collect();
        assert_eq!(
            columns,
            ["Recovered:argmax", "Infected:max"],
            "the objective keeps its default output as a row of its own"
        );
        assert_eq!(draft.output_reader(1), Some(DraftSite::Objective));
        assert_eq!(draft.output_reader(0), None);
        draft.mode = DraftMode::Sweep;
        assert_eq!(draft.output_reader(1), None, "a sweep reads no output");
    }

    #[test]
    fn an_issue_reads_as_the_panel_words_it() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let messages = |levels_text: &str| -> Vec<String> {
            let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
            levels_text.clone_into(&mut draft.factors[INFECTION_RATE].levels_text);
            draft
                .check(&schema, &panel_values)
                .err()
                .unwrap_or_default()
                .into_iter()
                .map(|issue| issue.message)
                .collect()
        };
        assert_eq!(messages("0.1, 0.2, abc"), ["'abc' is not a number"]);
        assert_eq!(messages("0.1, 5"), ["5 is outside the range 0 to 1"]);
        assert_eq!(
            messages("0:1"),
            [
                "Range needs a step, as in min:max:step. For min:max, select Latin hypercube or Uniform random under \
                 Design."
            ]
        );
    }

    #[test]
    fn a_table_design_holds_fixed_every_parameter_its_table_leaves_out() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = varied_draft(&entry, DraftDesign::Table);
        draft.table = Some(DesignTableDraft {
            file_name: "design.csv".to_owned(),
            text: "\u{feff}infection_rate , action.seed_outbreak\n0.1,20\n\n0.4,60\n".to_owned(),
        });
        let table = draft.table.as_ref().expect("the table was just set");
        assert_eq!(table.columns(), ["infection_rate", "action.seed_outbreak"]);
        assert_eq!(table.row_count(), 2, "blank lines are no rows");

        let planned = draft
            .check(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the draft does not plan: {issues:?}"));
        let fixed: Vec<&str> = planned.spec.fixed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            fixed,
            ["grid_width", "grid_height", "recovery_rate", "initial_infected_pct"],
            "the recovery rate is held fixed although it is ticked, and the infection rate is left to the table"
        );
        assert_eq!(planned.plan.configs().len(), 2);
    }

    /// Returns a search of SIR over both rates and an action's tick by `algorithm`, with that method's settings and
    /// the draft's run settings away from their defaults.
    fn search_draft(entry: &ModelEntry, algorithm: DraftAlgorithm) -> SweepDraft {
        let schema = entry.schema();
        let mut draft = SweepDraft::new(&schema);
        draft.mode = DraftMode::Search;
        draft.factors[INFECTION_RATE].vary = true;
        draft.factors[INFECTION_RATE].levels_text = "0.05:0.9".to_owned();
        draft.factors[RECOVERY_RATE].vary = true;
        draft.factors[RECOVERY_RATE].levels_text = "0.02,0.05,0.1".to_owned();
        draft.add_action(&schema, 0, 50);
        draft.actions[0].vary_tick = true;
        draft.actions[0].ticks_text = "0:400".to_owned();
        draft.replicates = 4;
        draft.root_seed_text = "9".to_owned();
        draft.steps = 300;
        draft.stats_every = 5;
        draft.series_every = 0;
        draft.default_reducers = false;
        draft.reducers = vec![
            ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::Max,
            },
            ReducerSpec {
                column: "Infected".to_owned(),
                kind: ReducerKind::ArgMax,
            },
        ];
        let search = &mut draft.search;
        search.algorithm = algorithm;
        search.max_evaluations = 96;
        search.batch_size = 12;
        match algorithm {
            DraftAlgorithm::Random => {}
            DraftAlgorithm::HillClimb => {
                search.hill_climb.mutation_scale = 0.25;
                search.hill_climb.patience = 3;
                search.hill_climb.reevaluate = true;
            }
            DraftAlgorithm::Genetic => {
                search.genetic.population = 24;
                search.genetic.elite_count = 3;
                search.genetic.crossover_rate = 0.75;
                search.genetic.reevaluate_fraction = 0.5;
            }
            DraftAlgorithm::PatternSpace => {
                // The x axis has its own range and the y axis an automatic one.
                search.pattern_space.x_axis = PatternAxis::bounded("Infected:max", 0.0, 1024.0, 16);
                search.pattern_space.y_axis.column = "Infected:argmax".to_owned();
                search.pattern_space.initial_samples = 24;
                search.pattern_space.aggregate = Aggregate::Mean;
            }
        }
        if algorithm.has_objective() {
            search.objective_column = "Infected:argmax".to_owned();
            search.goal = Goal::Minimize;
            search.aggregate = Aggregate::Mean;
        }
        draft
    }

    #[test]
    fn a_search_draft_reads_back_from_its_toml_under_every_method() {
        let entry = sir();
        for algorithm in DraftAlgorithm::ALL {
            let draft = search_draft(&entry, algorithm);
            let (reread, pinned) = round_trip(&entry, &draft);
            assert_eq!(reread, draft, "{algorithm:?} read back as another draft");
            assert_eq!(
                pinned[INFECTION_RATE], None,
                "a searched parameter keeps its Parameters tab value"
            );
            assert_eq!(pinned[0], Some(ParamValue::U32(32)), "the fixed values return");
        }
    }

    #[test]
    fn a_search_draft_plans_its_budget_and_space() {
        let entry = sir();
        let schema = entry.schema();
        let draft = search_draft(&entry, DraftAlgorithm::Genetic);
        let planned = draft
            .check(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the search does not plan: {issues:?}"));
        assert_eq!(planned.counts(), (96, 4, 96 * 4), "evaluations, replicates and runs");
        assert!(planned.spec.blocks.is_empty(), "a search writes no blocks");
        let search_plan = planned.search_plan.as_ref().expect("a search plans its search");
        assert_eq!(search_plan.space().factors().len(), 3, "both rates and the tick");
        let fixed: Vec<&str> = planned.spec.fixed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(fixed, ["grid_width", "grid_height", "initial_infected_pct"]);
    }

    #[test]
    fn a_search_reading_an_output_the_runs_do_not_record_is_refused() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let mut draft = search_draft(&entry, DraftAlgorithm::Random);
        draft.search.objective_column = "Recovered:max".to_owned();
        let issues = draft
            .check(&schema, &panel_values)
            .expect_err("no reducer records the objective");
        assert_eq!(
            issues,
            [DraftIssue {
                site: DraftSite::Objective,
                kind: IssueKind::Invalid,
                message: "Recovered, maximum is missing from Outputs".to_owned(),
            }]
        );

        draft.default_reducers = true;
        assert!(draft.check(&schema, &panel_values).is_ok(), "a default output");
        draft.search.objective_column = "Velocity.x:max".to_owned();
        assert!(
            draft.check(&schema, &panel_values).is_err(),
            "a part of a stat the model does not declare"
        );
    }

    #[test]
    fn a_search_issue_lands_on_the_row_it_names() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let sites = |draft: &SweepDraft| -> Vec<DraftSite> {
            let issues = draft.check(&schema, &panel_values).err().unwrap_or_default();
            issues.iter().map(|issue| issue.site).collect()
        };

        let mut draft = search_draft(&entry, DraftAlgorithm::HillClimb);
        draft.factors[INFECTION_RATE].levels_text = "0:5".to_owned();
        assert_eq!(
            sites(&draft),
            [DraftSite::Factor(INFECTION_RATE)],
            "a range past the bound"
        );

        let mut draft = search_draft(&entry, DraftAlgorithm::Genetic);
        draft.search.genetic.elite_count = 24;
        let issues = draft.check(&schema, &panel_values).err().unwrap_or_default();
        assert_eq!(
            issues,
            [DraftIssue::new(
                DraftSite::MethodSetting("genetic.elite_count"),
                "Elites must be less than the population of 24, got 24"
            )]
        );

        draft.search.genetic.crossover_rate = 1.5;
        draft.search.genetic.mutation_scale = 0.0;
        assert_eq!(
            sites(&draft),
            [
                DraftSite::MethodSetting("genetic.elite_count"),
                DraftSite::MethodSetting("genetic.crossover_rate"),
                DraftSite::MethodSetting("genetic.mutation_scale"),
            ],
            "every setting refused at once"
        );

        let mut draft = search_draft(&entry, DraftAlgorithm::Random);
        for factor in &mut draft.factors {
            factor.vary = false;
        }
        draft.actions[0].vary_tick = false;
        assert_eq!(sites(&draft), [DraftSite::Parameters], "a search over nothing");
    }

    #[test]
    fn a_pattern_space_draft_waits_for_the_range_of_each_axis() {
        let entry = sir();
        let schema = entry.schema();
        let mut fresh = SweepDraft::new(&schema);
        let axis = &fresh.search.pattern_space.x_axis;
        assert!(axis.is_automatic(), "an axis starts with an automatic range");
        assert_eq!(fresh.search.remembered_ranges, [(0.0, 0.0); 2], "its bounds start at 0");
        fresh.mode = DraftMode::Search;
        fresh.search.algorithm = DraftAlgorithm::PatternSpace;
        let issues = fresh.check(&schema, &panel_values(&entry)).err().unwrap_or_default();
        assert!(
            !issues.iter().any(|issue| matches!(issue.site, DraftSite::Axis(_))),
            "an automatic range needs no bounds: {issues:?}"
        );

        let mut draft = search_draft(&entry, DraftAlgorithm::PatternSpace);
        draft.search.set_automatic_range(GridAxis::Y, false);
        let issues = draft.check(&schema, &panel_values(&entry)).err().unwrap_or_default();
        assert_eq!(
            issues,
            [DraftIssue::missing(
                DraftSite::Axis(GridAxis::Y),
                "Set Minimum and Maximum"
            )]
        );

        draft.search.pattern_space.x_axis.min = Some(2000.0);
        draft.search.pattern_space.y_axis.min = Some(f64::NAN);
        let issues = draft.check(&schema, &panel_values(&entry)).err().unwrap_or_default();
        assert_eq!(
            issues,
            [
                DraftIssue::new(DraftSite::Axis(GridAxis::X), "Maximum must be greater than Minimum"),
                DraftIssue::new(DraftSite::Axis(GridAxis::Y), "Minimum must be a finite number"),
            ],
            "both axes at once"
        );
    }

    #[test]
    fn an_automatic_range_keeps_the_range_its_fields_held() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = search_draft(&entry, DraftAlgorithm::PatternSpace);
        draft.search.set_automatic_range(GridAxis::X, true);
        assert!(draft.search.pattern_space.x_axis.is_automatic());
        assert_eq!(draft.search.remembered_ranges[0], (0.0, 1024.0));
        draft.search.set_automatic_range(GridAxis::X, false);
        assert_eq!(draft.search.pattern_space.x_axis.range(), Some((0.0, 1024.0)));

        draft.search.pattern_space.initial_samples = 0;
        let issues = draft.check(&schema, &panel_values(&entry)).err().unwrap_or_default();
        assert_eq!(
            issues,
            [DraftIssue::new(
                DraftSite::MethodSetting(INITIAL_SAMPLES_KEY),
                "Initial samples must be at least 1 for an automatic range, got 0"
            )],
            "the y axis takes its range from the initial samples"
        );
        draft.search.set_automatic_range(GridAxis::Y, false);
        draft.search.pattern_space.y_axis.max = Some(300.0);
        assert!(
            draft.check(&schema, &panel_values(&entry)).is_ok(),
            "no initial sample is needed with both ranges set"
        );
    }

    #[test]
    fn blocks_the_panel_cannot_edit_are_refused() {
        let entry = sir();
        let schema = entry.schema();
        let mut spec = varied_draft(&entry, DraftDesign::VaryEachAlone)
            .to_spec(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the draft writes no spec: {issues:?}"));
        spec.blocks[1].design = DesignKind::Zip;
        let refused = SweepDraft::from_spec(&spec, Concurrency::Auto, &schema);
        assert!(
            refused.is_err_and(|message| message.starts_with("Spec holds 3 blocks")),
            "a zip among the blocks"
        );
    }

    #[test]
    fn a_genetic_budget_counts_its_generations_and_batches() {
        let settings = GeneticSettings::default();
        // A generation after the first re-evaluates 8 of 32 members and breeds 30 children.
        assert_eq!(generation_estimate(200, &settings), Some(6));
        assert_eq!(generation_estimate(100, &settings), Some(3));
        assert_eq!(generation_estimate(32, &settings), Some(1));
        assert_eq!(
            generation_estimate(33, &settings),
            Some(2),
            "a partial generation counts"
        );
        assert_eq!(generation_estimate(20, &settings), None, "less than one generation");

        assert_eq!(batch_estimate(200, 16, None), 13);
        assert_eq!(batch_estimate(0, 16, None), 0);
        assert_eq!(
            batch_estimate(200, 16, Some(&settings)),
            2 + 4 * 3 + 1,
            "2 batches for generation 0, 3 for each of 4 whole ones, then 1 for the last 16"
        );
        assert_eq!(batch_estimate(20, 16, Some(&settings)), 2);
    }

    #[test]
    fn the_output_lists_offer_each_unrecorded_output_of_every_stat() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = SweepDraft::new(&schema);
        let unrecorded = draft.unrecorded_outputs(&schema);
        assert_eq!(
            unrecorded.len(),
            2 * schema.stats.len(),
            "each stat's tick of maximum and minimum"
        );
        assert!(draft.records_output("Infected:max", &schema));
        draft.default_reducers = false;
        assert!(!draft.records_output("Infected:max", &schema));
        assert_eq!(draft.unrecorded_outputs(&schema).len(), 6 * schema.stats.len());
    }

    #[test]
    fn a_comparator_reads_as_words() {
        let words: Vec<&str> = [
            Comparator::Less,
            Comparator::LessOrEqual,
            Comparator::Equal,
            Comparator::NotEqual,
            Comparator::GreaterOrEqual,
            Comparator::Greater,
        ]
        .into_iter()
        .map(comparator_words)
        .collect();
        assert_eq!(
            words,
            ["below", "at most", "equal to", "not equal to", "at least", "above"]
        );
    }

    #[test]
    fn a_boids_pattern_space_search_watches_only_columns_a_reducer_writes() {
        let entry = boids();
        let schema = entry.schema();
        let values = default_values(&entry);
        let columns = sampled_columns(&entry);
        let mut draft = SweepDraft::new(&schema);
        draft.set_stat_columns(&columns);
        draft.mode = DraftMode::Search;
        draft.search.algorithm = DraftAlgorithm::PatternSpace;
        let separation = schema
            .params
            .iter()
            .position(|descriptor| descriptor.id == "separation")
            .expect("boids has a separation weight");
        draft.factors[separation].vary = true;
        draft.factors[separation].levels_text = "0:2".to_owned();
        assert_eq!(
            draft.search.pattern_space.y_axis.column, "Average Velocity.magnitude:max",
            "a vector's bare label stands for its magnitude"
        );

        let planned = draft
            .check(&schema, &values)
            .unwrap_or_else(|issues| panic!("the default search does not plan: {issues:?}"));
        let search_plan = planned.search_plan.as_ref().expect("a search plans its search");
        let measure = MeasurePlan::new(&planned.spec.run, &planned.spec.measure, columns.clone())
            .expect("the outputs bind to the sampled columns");
        assert!(
            search_plan.watched_reducers(&measure).is_ok(),
            "a reducer writes every column the search watches"
        );
        let written = measure.reducers().names();
        assert_eq!(
            draft.output_names(&schema),
            written,
            "the outputs are the ones the reducers write"
        );
        assert_eq!(
            written.len(),
            4 * 4,
            "the defaults of one scalar and of a vector's three parts"
        );
        let offered = draft.unrecorded_outputs(&schema);
        assert!(
            offered.contains(&"Average Velocity.x:argmax".to_owned()),
            "a vector's parts are offered: {offered:?}"
        );
        assert!(
            written
                .iter()
                .chain(&offered)
                .all(|name| !name.starts_with("Average Velocity:")),
            "no bare label of a vector is offered"
        );

        draft.search.pattern_space.y_axis.column = "Average Velocity:max".to_owned();
        let issues = draft
            .check(&schema, &values)
            .expect_err("no reducer writes a bare vector label");
        assert_eq!(
            issues,
            [DraftIssue::new(
                DraftSite::Axis(GridAxis::Y),
                "Average Velocity, maximum is missing from Outputs"
            )]
        );

        draft.search.pattern_space.y_axis.column = "Average Velocity.x:argmax".to_owned();
        assert!(draft.add_watched_output("Average Velocity.x:argmax"));
        assert!(
            draft.check(&schema, &values).is_ok(),
            "a part picked from the list is recorded as an output row"
        );
    }

    /// Returns a draft of a Pattern Space Exploration of boids over its separation weight.
    fn boids_pattern_space(schema: &ModelSchema<'_>) -> SweepDraft {
        let mut draft = SweepDraft::new(schema);
        draft.mode = DraftMode::Search;
        draft.search.algorithm = DraftAlgorithm::PatternSpace;
        let separation = schema
            .params
            .iter()
            .position(|descriptor| descriptor.id == "separation")
            .expect("boids has a separation weight");
        draft.factors[separation].vary = true;
        draft.factors[separation].levels_text = "0:2".to_owned();
        draft
    }

    #[test]
    fn a_draft_cannot_start_until_its_columns_are_sampled() {
        let entry = boids();
        let schema = entry.schema();
        let values = default_values(&entry);
        let mut draft = SweepDraft::new(&schema);
        draft.columns_pending = true;
        assert_eq!(
            draft
                .check(&schema, &values)
                .expect_err("a sweep waits for the columns"),
            [DraftIssue::missing(DraftSite::Outputs, COLUMNS_PENDING)]
        );

        let mut search = boids_pattern_space(&schema);
        search.columns_pending = true;
        search.search.pattern_space.y_axis.column.clear();
        assert_eq!(
            search
                .check(&schema, &values)
                .expect_err("a search waits for the columns"),
            [
                DraftIssue::missing(DraftSite::Axis(GridAxis::Y), "Select output"),
                DraftIssue::missing(DraftSite::Outputs, COLUMNS_PENDING),
            ],
            "every other issue comes first"
        );

        draft.set_stat_columns(&sampled_columns(&entry));
        assert!(!draft.columns_pending);
        assert!(
            draft.check(&schema, &values).is_ok(),
            "the issue clears once the build reports"
        );
    }

    #[test]
    fn a_draft_without_columns_accepts_a_part_of_a_stat() {
        let entry = boids();
        let schema = entry.schema();
        let values = default_values(&entry);
        let mut draft = boids_pattern_space(&schema);
        assert_eq!(draft.stat_columns, None);
        for column in ["Average Velocity:max", "Average Velocity.x:max"] {
            draft.search.pattern_space.y_axis.column = column.to_owned();
            assert!(draft.records_output(column, &schema), "{column}");
            assert!(
                draft.check(&schema, &values).is_ok(),
                "{column}: the sweep's probe checks the columns a build gives"
            );
        }

        let part = "Average Velocity.y:argmax";
        draft.search.pattern_space.y_axis.column = part.to_owned();
        assert!(
            !draft.records_output(part, &schema),
            "no default is a tick of a maximum"
        );
        assert!(draft.add_watched_output(part));
        assert!(draft.output_names(&schema).iter().any(|name| name == part));
        assert!(draft.check(&schema, &values).is_ok(), "an output row records the part");

        assert!(
            !draft.records_output("Average Heading.x:max", &schema),
            "the model has no such stat"
        );
        draft.default_reducers = false;
        assert!(
            !draft.records_output("Average Velocity.x:max", &schema),
            "no row records it without the defaults"
        );
    }

    #[test]
    fn an_output_row_over_a_bare_vector_label_writes_its_magnitude() {
        let entry = boids();
        let schema = entry.schema();
        let columns = sampled_columns(&entry);
        let mut draft = SweepDraft::new(&schema);
        let row = ReducerSpec {
            column: "Average Velocity".to_owned(),
            kind: ReducerKind::ArgMax,
        };
        assert_eq!(
            written_name(&row, None),
            "Average Velocity:argmax",
            "no columns sampled yet"
        );
        assert_eq!(written_name(&row, Some(&columns)), "Average Velocity.magnitude:argmax");

        draft.set_stat_columns(&columns);
        draft.mode = DraftMode::Search;
        draft.search.objective_column = "Average Velocity.magnitude:argmax".to_owned();
        draft.reducers.push(row);
        assert_eq!(
            draft.output_reader(0),
            Some(DraftSite::Objective),
            "the row writes the column the objective reads"
        );
        assert!(
            !draft.add_watched_output("Average Velocity.magnitude:argmax"),
            "a row records it already"
        );

        draft.mode = DraftMode::Sweep;
        draft.reducers[0].column = "Average Velocity.z".to_owned();
        let issues = draft
            .check(&schema, &default_values(&entry))
            .expect_err("the model gives no such column");
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].site, DraftSite::Output(0));
    }

    #[test]
    fn save_spec_refuses_what_the_loader_refuses() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let refusal = |draft: &SweepDraft| -> Vec<(DraftSite, String)> {
            draft
                .to_toml(&schema, &panel_values)
                .expect_err("the spec does not load back")
                .into_iter()
                .map(|issue| (issue.site, issue.message))
                .collect()
        };

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.reducers[0].kind = ReducerKind::WindowMean { start: 1000, end: 100 };
        assert_eq!(
            refusal(&draft),
            [(DraftSite::Output(0), "From tick must be at most To tick".to_owned())]
        );

        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        draft.stop.as_mut().expect("the draft stops early").threshold = f64::INFINITY;
        draft.reducers[1].kind = ReducerKind::FirstCrossing(Comparison {
            comparator: Comparator::GreaterOrEqual,
            threshold: f64::NAN,
        });
        let expected = [
            (DraftSite::Stop, NOT_FINITE_THRESHOLD.to_owned()),
            (DraftSite::Output(1), NOT_FINITE_THRESHOLD.to_owned()),
        ];
        assert_eq!(refusal(&draft), expected);
        let sites: Vec<DraftSite> = draft
            .check(&schema, &panel_values)
            .expect_err("the check refuses a threshold that is not finite")
            .iter()
            .map(|issue| issue.site)
            .collect();
        assert_eq!(sites, [DraftSite::Stop, DraftSite::Output(1)]);

        let mut outside = panel_values.clone();
        outside[0] = ParamValue::U32(0);
        let issues = varied_draft(&entry, DraftDesign::EveryCombination)
            .to_toml(&schema, &outside)
            .expect_err("a grid width of 0 does not load back");
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].site, DraftSite::Sweep);
    }

    #[test]
    fn a_timeout_that_is_not_a_duration_is_an_issue_of_its_row() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let mut draft = SweepDraft::new(&schema);
        for (seconds, message) in [
            (f64::NAN, "Seconds per run must be a number"),
            (-1.0, "Seconds per run must be at least 0"),
            (f64::INFINITY, "Seconds per run is too large"),
            (2.0_f64.powi(64), "Seconds per run is too large"),
        ] {
            draft.timeout_s = Some(seconds);
            let expected = [DraftIssue::new(DraftSite::Timeout, message)];
            assert_eq!(draft.issues(&schema), expected, "{seconds} s");
            let written = draft
                .to_spec(&schema, &panel_values)
                .expect_err("the spec takes no such timeout");
            assert_eq!(written, expected, "{seconds} s");
            let saved = draft
                .to_toml(&schema, &panel_values)
                .expect_err("the spec file takes no such timeout");
            assert_eq!(saved, expected, "{seconds} s");
        }
        for seconds in [0.0, 0.5, 600.0] {
            draft.timeout_s = Some(seconds);
            let spec = draft
                .to_spec(&schema, &panel_values)
                .unwrap_or_else(|issues| panic!("{seconds} s is refused: {issues:?}"));
            assert_eq!(
                spec.run.timeout,
                Some(Duration::from_secs_f64(seconds)),
                "a timeout under the field's minimum is kept, as henad-cli keeps it"
            );
            assert!(draft.check(&schema, &panel_values).is_ok(), "{seconds} s");
        }
    }

    #[test]
    fn a_loaded_memory_budget_is_saved_again() {
        let entry = sir();
        let schema = entry.schema();
        let text = varied_draft(&entry, DraftDesign::EveryCombination)
            .to_toml(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the draft writes no spec: {issues:?}"));
        let mut file = SpecFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
        file.execution.memory = Some(1_000_000);
        file.execution.gpu_memory = Some(2_000_000);
        let (loaded, _) = SweepDraft::from_spec_file(file, &schema).unwrap_or_else(|message| panic!("{message}"));
        assert_eq!(loaded.memory_budget, Some(1_000_000));
        assert_eq!(loaded.gpu_memory_budget, Some(2_000_000));

        let saved = loaded
            .to_toml(&schema, &panel_values(&entry))
            .unwrap_or_else(|issues| panic!("the loaded draft writes no spec: {issues:?}"));
        let execution = SpecFile::parse(&saved)
            .unwrap_or_else(|error| panic!("{error}"))
            .execution;
        assert_eq!(execution.memory, Some(1_000_000), "{saved}");
        assert_eq!(execution.gpu_memory, Some(2_000_000), "{saved}");
        assert_eq!(execution.concurrent, loaded.concurrency);
    }

    #[test]
    fn a_range_drawn_from_is_not_capped_at_the_listed_values() {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = panel_values(&entry);
        let mut draft = SweepDraft::new(&schema);
        draft.add_action(&schema, 0, 50);
        draft.actions[0].vary_tick = true;
        draft.actions[0].ticks_text = "0:2000000".to_owned();
        let over_limit = format!("Range gives 2000001 values, over the limit of {MAX_DRAFT_LEVELS}");

        let issues = draft.check(&schema, &panel_values).expect_err("every tick is listed");
        assert_eq!(issues, [DraftIssue::new(DraftSite::Action(0), over_limit.clone())]);
        assert_eq!(
            parse_levels("0:2000000"),
            Ok(LevelSpec::Range {
                min: 0.0,
                max: 2_000_000.0,
                step: None
            }),
            "the editor and the Plan read the range as typed"
        );
        assert_eq!(EditorSegment::of("1:5000000", true), EditorSegment::Drawn);
        let width = &schema.params[0].kind;
        assert_eq!(
            segment_text(EditorSegment::Drawn, "100:3000000", width),
            "100:3000000",
            "the editor keeps a typed range"
        );
        draft.design = DraftDesign::LatinHypercube;
        assert!(
            draft.check(&schema, &panel_values).is_ok(),
            "a sampled design draws its samples from the range"
        );
        draft.actions[0].ticks_text = "0:2000000:1".to_owned();
        let issues = draft
            .check(&schema, &panel_values)
            .expect_err("a step lists every tick");
        assert_eq!(issues, [DraftIssue::new(DraftSite::Action(0), over_limit)]);

        draft.design = DraftDesign::EveryCombination;
        draft.mode = DraftMode::Search;
        draft.actions[0].ticks_text = "0:2000000".to_owned();
        assert!(
            draft.check(&schema, &panel_values).is_ok(),
            "a search draws from the range"
        );
    }

    #[test]
    fn a_table_counts_its_rows_and_not_its_header() {
        let mut spec = SweepSpec::new("sir".to_owned());
        spec.blocks.push(BlockSpec {
            design: DesignKind::Table {
                text: "infection_rate\n0.1\n\n0.2\n0.3\n\n".to_owned(),
            },
            factors: Vec::new(),
            design_seed: None,
        });
        assert_eq!(estimated_configs(&spec), 3);
    }

    #[test]
    fn a_run_of_every_tick_does_not_overflow_the_series_estimate() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = SweepDraft::new(&schema);
        draft.steps = u64::MAX;
        draft.warmup = 0;
        draft.stats_every = 1;
        draft.series_every = 1;
        let issues = draft
            .check(&schema, &panel_values(&entry))
            .expect_err("the series pass the memory limit");
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].site, DraftSite::Execution);
    }

    #[test]
    fn a_design_table_sets_the_ticks_it_names() {
        let entry = sir();
        let schema = entry.schema();
        let mut draft = varied_draft(&entry, DraftDesign::EveryCombination);
        assert_eq!(draft.tick_source(&draft.actions[0]), TickSource::Varied);
        draft.design = DraftDesign::Table;
        assert_eq!(
            draft.tick_source(&draft.actions[0]),
            TickSource::Fixed,
            "no table yet, so the action keeps its own tick"
        );
        draft.table = Some(DesignTableDraft {
            file_name: "design.csv".to_owned(),
            text: "infection_rate,action.seed_outbreak\n0.1,20\n".to_owned(),
        });
        assert_eq!(draft.tick_source(&draft.actions[0]), TickSource::Table);
        draft.actions[0].vary_tick = false;
        assert_eq!(
            draft.tick_source(&draft.actions[0]),
            TickSource::Table,
            "the table sets it either way"
        );
        draft.add_action(&schema, 0, 70);
        assert_eq!(
            draft.tick_source(&draft.actions[1]),
            TickSource::Fixed,
            "a column the table lacks"
        );
    }

    #[test]
    fn an_integer_levels_example_resolves_as_vary_text() {
        for (min, max) in [(1, 2), (0, 1), (5, 5), (0, 100), (1, 7)] {
            let descriptor = henad_core::helpers::u32_param("count", "Count", min, min, max);
            let example = super::levels_example(&descriptor.kind);
            let factor = FactorSpec {
                target: henad_core::explore::factor::FactorTarget::Param("count".to_owned()),
                levels: LevelSpec::parse(&example).expect("the example parses"),
            };
            assert!(
                factor.resolve(&[descriptor], &[], &DesignKind::Factorial).is_ok(),
                "{min}..={max} gave {example}"
            );
        }
    }

    #[test]
    fn set_action_ignores_an_undeclared_action() {
        let sir = sir();
        let schema = sir.schema();
        let mut draft = SweepDraft::new(&schema);
        draft.add_action(&schema, 0, 10);
        draft.add_action(&schema, 0, 20);
        let rows = draft.actions.clone();
        draft.set_action(&schema, 0, schema.actions.len());
        assert_eq!(draft.actions, rows);
    }
}
