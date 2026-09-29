//! Results the Results tab holds, from the sweep or search in progress or from the files of an ended one, and the
//! statistics its views plot.
//!
//! A value column is a parameter or an action tick of `runs.csv`. An axis is a value column that takes more than one
//! value across the configs, and a level is one of those values. Runs are held in full, and their series within a
//! byte budget. The configs of a search are its candidates, known once the search is told their evaluations.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use henad_core::explore::factor::FactorTarget;
use henad_core::explore::measure::SeriesBuffer;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::{Config, Plan};
use henad_core::explore::reducer::ReducerKind;
use henad_core::explore::replay::Replay;
use henad_core::explore::search::SearchSpec;
use henad_core::explore::spec::{ACTION_COLUMN_PREFIX, ActionSpec};
use henad_core::explore::summary::{ReplicateSummary, RunningMoments};
use henad_core::explore::value::{format_value, parse_value};
use henad_core::params::ParamDescriptor;
use henad_explore::output::search_tables::SearchHistory;
use henad_explore::result_set::ResultSet;
use henad_explore::schema::model_schema;
use henad_explore::search_run::{SearchPlan, SearchUpdate};
use henad_models::registry::ModelEntry;

use crate::ui::sweep::draft::describe_error;

/// Level of a config on an axis whose column the config lacks.
const NO_LEVEL: usize = usize::MAX;

/// Place results come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsSource {
    /// The sweep or search started from the Sweep tab.
    Sweep,
    /// A folder a sweep wrote.
    Folder(PathBuf),
    /// Files picked one by one, by name.
    Files(Vec<String>),
}

/// A value column that takes more than one value across the configs.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultsAxis {
    /// Position of the axis among the value columns.
    pub column: usize,
    /// Name of the axis as the views show it.
    pub label: String,
    /// Values of the axis as `runs.csv` writes them. Numbers ascend, and other values keep the order of the first
    /// config that takes them.
    pub levels: Vec<String>,
    /// Place of each level on a plot axis, its value on a numeric axis and its index otherwise.
    pub positions: Vec<f64>,
    /// Whether every level is a number.
    pub numeric: bool,
}

/// Block and values of one config.
#[derive(Debug, Clone, PartialEq)]
struct ConfigTexts {
    /// Index of the block the config comes from.
    block: usize,
    /// Value of each value column, as `runs.csv` writes it.
    texts: Vec<String>,
}

/// One config of the results.
#[derive(Debug, Clone, PartialEq)]
struct ConfigEntry {
    /// Index of the block the config comes from.
    block: usize,
    /// Value of each value column, as `runs.csv` writes it.
    texts: Vec<String>,
    /// Level of each axis, [`NO_LEVEL`] where the config lacks the axis's column.
    levels: Vec<usize>,
    /// Positions in the store's runs of the config's runs, in order of run id.
    runs: Vec<usize>,
}

/// Series of runs, held while they fit a byte budget.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesCache {
    /// Bytes the cache holds at most.
    budget: usize,
    /// Bytes the held series take.
    used: usize,
    runs: BTreeMap<u64, SeriesBuffer>,
}

impl SeriesCache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            runs: BTreeMap::new(),
        }
    }

    /// Holds `series` as the series of run `run_id`, and returns whether it fits the budget.
    ///
    /// A series that does not fit is dropped.
    pub fn insert(&mut self, run_id: u64, series: SeriesBuffer) -> bool {
        self.remove(run_id);
        let bytes = series_bytes(&series);
        if self.used + bytes > self.budget {
            return false;
        }
        self.used += bytes;
        self.runs.insert(run_id, series);
        true
    }

    pub fn get(&self, run_id: u64) -> Option<&SeriesBuffer> {
        self.runs.get(&run_id)
    }

    fn remove(&mut self, run_id: u64) {
        if let Some(series) = self.runs.remove(&run_id) {
            self.used -= series_bytes(&series);
        }
    }

    /// Drops held series of runs outside `kept_runs`, highest run id first, until `bytes` more fit the budget.
    fn make_room(&mut self, bytes: usize, kept_runs: &BTreeSet<u64>) {
        let evictable: Vec<u64> = self
            .runs
            .keys()
            .rev()
            .copied()
            .filter(|id| !kept_runs.contains(id))
            .collect();
        for run_id in evictable {
            if self.used + bytes <= self.budget {
                return;
            }
            self.remove(run_id);
        }
    }

    /// Number of runs whose series is held.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Bytes the held series take.
    pub fn used_bytes(&self) -> usize {
        self.used
    }
}

/// Plan the runs of a store replay through.
#[derive(Debug, Clone)]
enum ReplayPlan {
    Sweep(Arc<Plan>),
    /// A search, whose runs replay from the configs of their candidates.
    Search(Arc<SearchPlan>),
}

impl ReplayPlan {
    /// Plan of the settings every run shares, and of a sweep's configs.
    fn base(&self) -> &Plan {
        match self {
            Self::Sweep(plan) => plan,
            Self::Search(search_plan) => search_plan.base(),
        }
    }
}

/// Settings and course of the search that produced the results.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchLog {
    pub spec: SearchSpec,
    pub history: SearchHistory,
    /// Reason the search tables of an opened folder cannot be read. `history` is then empty.
    pub table_error: Option<String>,
}

/// Returns the bytes `series` takes, counted as a sweep's events count them.
fn series_bytes(series: &SeriesBuffer) -> usize {
    series.len() * (series.width() + 1) * size_of::<f64>()
}

/// Spread a series band shows around its centre line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BandKind {
    /// Mean, with one sample standard deviation either side.
    #[default]
    StandardDeviation,
    /// Mean, with the 95% confidence interval of the mean.
    ConfidenceInterval,
    /// Median, from the 10th to the 90th percentile.
    Percentiles,
}

/// Centre line and spread of one stat over the replicates of a config, tick by tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Band {
    pub ticks: Vec<f64>,
    pub center: Vec<f64>,
    pub low: Vec<f64>,
    pub high: Vec<f64>,
    /// Number of runs whose series the band reads.
    pub runs: usize,
}

/// Output plotted against one axis, pooled over every config the pins let through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseQuery {
    pub x_axis: usize,
    /// Index of the reducer column.
    pub output: usize,
    /// Axis whose levels each get a line of their own.
    pub group_axis: Option<usize>,
    /// Level each axis is held at, `None` for any level. The x and group axes are free whatever their pin.
    pub pins: Vec<Option<usize>>,
}

/// Statistics of an output at one level of the x axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResponsePoint {
    pub x: f64,
    /// Level of the x axis.
    pub level: usize,
    pub summary: ReplicateSummary,
}

/// Points of one line of a response plot.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseLine {
    /// Level of the group axis, `None` without one.
    pub group_level: Option<usize>,
    /// Points in order of x level.
    pub points: Vec<ResponsePoint>,
}

/// Statistic a heatmap cell is coloured by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeatColor {
    #[default]
    Mean,
    StandardDeviation,
    /// Standard deviation over the magnitude of the mean.
    CoefficientOfVariation,
}

/// Output over the levels of two axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatQuery {
    pub x_axis: usize,
    pub y_axis: usize,
    /// Index of the reducer column.
    pub output: usize,
    pub color_by: HeatColor,
    /// Level each axis is held at, `None` for any level. The x and y axes are free whatever their pin.
    pub pins: Vec<Option<usize>>,
}

/// Cells of a heatmap, row by row from the lowest y level.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatGrid {
    /// Number of x levels.
    pub columns: usize,
    /// Number of y levels.
    pub rows: usize,
    /// Value of each cell, not finite for a cell with no value.
    pub values: Vec<f64>,
    /// Values each cell pools.
    pub counts: Vec<u64>,
    /// Configs each cell pools.
    pub configs: Vec<Vec<u64>>,
}

impl HeatGrid {
    /// Returns the index of the cell at x level `column` and y level `row`.
    pub fn cell(&self, column: usize, row: usize) -> usize {
        row * self.columns + column
    }

    /// Returns the lowest and highest finite value, or `None` when no cell holds one.
    pub fn range(&self) -> Option<(f64, f64)> {
        let finite = self.values.iter().copied().filter(|value| value.is_finite());
        finite.fold(None, |range, value| match range {
            None => Some((value, value)),
            Some((low, high)) => Some((f64::min(low, value), f64::max(high, value))),
        })
    }
}

/// Runs the runs table lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunsFilter {
    #[default]
    All,
    /// Runs that ended on a fault or a timeout.
    Failed,
    /// Runs of the selected configs.
    SelectedConfigs,
}

/// Column of the runs table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunsColumn {
    Run,
    Config,
    Replicate,
    Seed,
    Status,
    Ticks,
    /// Wall time spent stepping and sampling.
    Time,
    /// Level of the axis at this index.
    Axis(usize),
    /// Value of the reducer column at this index.
    Output(usize),
}

/// Order of the runs table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunsSort {
    pub column: RunsColumn,
    pub descending: bool,
}

impl Default for RunsSort {
    fn default() -> Self {
        Self {
            column: RunsColumn::Run,
            descending: false,
        }
    }
}

/// Value a run sorts by in one column of the runs table.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SortValue {
    Whole(u64),
    Real(f64),
}

impl SortValue {
    /// Returns the order of `self` and `other`, equal for two values of different kinds.
    fn compare(self, other: Self) -> Ordering {
        match (self, other) {
            (Self::Whole(a), Self::Whole(b)) => a.cmp(&b),
            (Self::Real(a), Self::Real(b)) => a.total_cmp(&b),
            (Self::Whole(_), Self::Real(_)) | (Self::Real(_), Self::Whole(_)) => Ordering::Equal,
        }
    }
}

/// Runs, configs and series of one sweep.
#[derive(Debug)]
pub struct ResultsStore {
    pub source: ResultsSource,
    pub model_id: String,
    /// Name of the model as the sweep recorded it.
    pub model_name: String,
    /// Index of the model in the app's registry, `None` when this device lacks it.
    pub model_index: Option<usize>,
    /// Whether the model declares the parameters, stats and actions the sweep ran with.
    pub schema_matches: bool,
    /// Whether every run of the sweep is held.
    pub complete: bool,
    /// Folder that holds the sweep's files, `None` for a sweep held in memory or picked files.
    folder: Option<PathBuf>,
    /// Plan the runs replay through, or the reason they cannot: this device lacks the model, or the model refuses the
    /// spec.
    plan: Result<ReplayPlan, String>,
    /// Search the results come from, `None` for a sweep.
    search: Option<SearchLog>,
    /// Parameters of the model, empty when this device lacks it.
    descriptors: Vec<ParamDescriptor>,
    /// Label of each action the sweep fires, by the name its spec gives the action. Empty when this device lacks the
    /// model.
    action_labels: BTreeMap<String, String>,
    value_columns: Vec<String>,
    stat_columns: Vec<String>,
    reducer_columns: Vec<String>,
    axes: Vec<ResultsAxis>,
    /// Whether each axis takes more than one level within a block, by block index.
    block_axes: BTreeMap<usize, Vec<bool>>,
    configs: BTreeMap<u64, ConfigEntry>,
    /// Runs in the order they arrived, each with its series left empty.
    runs: Vec<RunOutcome>,
    /// Position in `runs` of each run, by run id.
    positions: BTreeMap<u64, usize>,
    series: SeriesCache,
    /// Runs that recorded no series rows, which a load cannot find either.
    empty_series_runs: BTreeSet<u64>,
    /// Count of changes. A view keeps its cache while the count stays the same.
    revision: u64,
}

impl ResultsStore {
    /// Returns an empty store for the sweep of `plan` over `entry`, the model at `model_index` in the registry, whose
    /// files go to `folder`.
    pub fn for_sweep(
        plan: Arc<Plan>,
        entry: &ModelEntry,
        model_index: usize,
        folder: Option<PathBuf>,
        series_budget: usize,
    ) -> Self {
        let value_columns = plan_value_columns(&plan, &entry.param_descriptors);
        let texts = plan_texts(&plan, &entry.param_descriptors);
        let mut store = Self {
            source: ResultsSource::Sweep,
            model_id: entry.id.clone(),
            model_name: entry.name.clone(),
            model_index: Some(model_index),
            schema_matches: true,
            complete: false,
            folder,
            action_labels: action_labels(plan.actions(), entry),
            plan: Ok(ReplayPlan::Sweep(plan)),
            search: None,
            descriptors: entry.param_descriptors.clone(),
            value_columns,
            stat_columns: Vec::new(),
            reducer_columns: Vec::new(),
            axes: Vec::new(),
            block_axes: BTreeMap::new(),
            configs: BTreeMap::new(),
            runs: Vec::new(),
            positions: BTreeMap::new(),
            series: SeriesCache::new(series_budget),
            empty_series_runs: BTreeSet::new(),
            revision: 0,
        };
        store.set_configs(texts);
        store
    }

    /// Returns an empty store for the search of `search_plan` over `entry`, the model at `model_index` in the
    /// registry, whose files go to `folder`.
    pub fn for_search(
        search_plan: Arc<SearchPlan>,
        entry: &ModelEntry,
        model_index: usize,
        folder: Option<PathBuf>,
        series_budget: usize,
    ) -> Self {
        let base = Arc::clone(search_plan.base());
        let search = SearchLog {
            spec: search_plan.search().clone(),
            history: SearchHistory::default(),
            table_error: None,
        };
        let mut store = Self::for_sweep(base, entry, model_index, folder, series_budget);
        store.set_configs(BTreeMap::new());
        store.plan = Ok(ReplayPlan::Search(search_plan));
        store.search = Some(search);
        store
    }

    /// Returns a store of the runs `set` read from `source`, holding at most `series_budget` bytes of series.
    ///
    /// The runs replay through the model of the same id in `registry`, when there is one.
    pub fn from_result_set(
        set: ResultSet,
        source: ResultsSource,
        registry: &[ModelEntry],
        series_budget: usize,
    ) -> Self {
        let model = &set.manifest().model;
        let (model_id, model_name) = (model.id.clone(), model.name.clone());
        let model_index = registry.iter().position(|entry| entry.id == model_id);
        let entry = model_index.map(|index| &registry[index]);
        let schema_matches = entry.is_some_and(|entry| set.schema_matches(entry));
        let search = set
            .spec()
            .search
            .clone()
            .filter(|_| set.is_search())
            .map(|spec| match set.search_history() {
                Ok(history) => SearchLog {
                    spec,
                    history: history.unwrap_or_default(),
                    table_error: None,
                },
                Err(error) => SearchLog {
                    spec,
                    history: SearchHistory::default(),
                    table_error: Some(describe_error(&error)),
                },
            });
        let plan = match entry {
            None => Err(format!("{model_name} is unavailable on this device")),
            Some(entry) if search.is_some() => SearchPlan::new(set.spec(), &model_schema(entry))
                .map(|search_plan| ReplayPlan::Search(Arc::new(search_plan)))
                .map_err(|error| format!("{} refuses this search's spec: {}", entry.name, describe_error(&error))),
            Some(entry) => set
                .plan(entry)
                .map(|plan| ReplayPlan::Sweep(Arc::new(plan)))
                .map_err(|error| format!("{} refuses this sweep's spec: {}", entry.name, describe_error(&error))),
        };
        let mut texts = match (&plan, entry) {
            (Ok(ReplayPlan::Sweep(plan)), Some(entry)) if schema_matches => plan_texts(plan, &entry.param_descriptors),
            _ => BTreeMap::new(),
        };
        let mut store = Self {
            source,
            model_id,
            model_name,
            model_index,
            schema_matches,
            complete: set.is_complete(),
            folder: set.dir().map(Path::to_path_buf),
            plan,
            search,
            descriptors: entry.map(|entry| entry.param_descriptors.clone()).unwrap_or_default(),
            action_labels: entry.map_or_else(BTreeMap::new, |entry| action_labels(&set.spec().actions, entry)),
            value_columns: set.value_columns().to_vec(),
            stat_columns: set.stat_columns().to_vec(),
            reducer_columns: set.reducer_columns().to_vec(),
            axes: Vec::new(),
            block_axes: BTreeMap::new(),
            configs: BTreeMap::new(),
            runs: Vec::new(),
            positions: BTreeMap::new(),
            series: SeriesCache::new(series_budget),
            empty_series_runs: BTreeSet::new(),
            revision: 0,
        };
        let rows = set.into_runs();
        for row in &rows {
            let config = ConfigTexts {
                block: row.block,
                texts: row.values.clone(),
            };
            texts.insert(row.outcome.run.config_id, config);
        }
        store.set_configs(texts);
        for row in rows {
            store.insert_run(row.outcome, !row.series_held);
        }
        store
    }

    /// Replaces the configs with `texts`, the block and values of each config by id, and finds the axes they vary.
    fn set_configs(&mut self, texts: BTreeMap<u64, ConfigTexts>) {
        let axes: Vec<ResultsAxis> = (0..self.value_columns.len())
            .filter_map(|column| self.axis(column, &texts))
            .collect();
        let lookups: Vec<BTreeMap<&str, usize>> = axes
            .iter()
            .map(|axis| {
                let levels = axis.levels.iter().enumerate();
                levels.map(|(level, text)| (text.as_str(), level)).collect()
            })
            .collect();
        let configs: BTreeMap<u64, ConfigEntry> = texts
            .into_iter()
            .map(|(config_id, ConfigTexts { block, texts })| {
                let levels = axes
                    .iter()
                    .zip(&lookups)
                    .map(|(axis, lookup)| {
                        let text = texts.get(axis.column).map(String::as_str);
                        text.and_then(|text| lookup.get(text)).copied().unwrap_or(NO_LEVEL)
                    })
                    .collect();
                let entry = ConfigEntry {
                    block,
                    texts,
                    levels,
                    runs: Vec::new(),
                };
                (config_id, entry)
            })
            .collect();
        self.block_axes = block_axes(&configs, axes.len());
        self.configs = configs;
        self.axes = axes;
        self.revision += 1;
    }

    /// Returns whether axis `axis` takes more than one level within block `block`.
    fn block_varies(&self, block: usize, axis: usize) -> bool {
        self.block_axes
            .get(&block)
            .and_then(|varies| varies.get(axis))
            .copied()
            .unwrap_or(false)
    }

    /// Returns the axis of value column `column` across the configs `texts`, or `None` when the column holds one
    /// value.
    fn axis(&self, column: usize, texts: &BTreeMap<u64, ConfigTexts>) -> Option<ResultsAxis> {
        let mut seen = BTreeSet::new();
        let mut distinct: Vec<&str> = Vec::new();
        for config in texts.values() {
            if let Some(text) = config.texts.get(column)
                && seen.insert(text.as_str())
            {
                distinct.push(text);
            }
        }
        if distinct.len() < 2 {
            return None;
        }
        let numbers: Option<Vec<f64>> = distinct
            .iter()
            .map(|text| text.parse::<f64>().ok().filter(|value| value.is_finite()))
            .collect();
        let (levels, positions, numeric) = if let Some(numbers) = numbers {
            let mut pairs: Vec<(f64, &str)> = numbers.into_iter().zip(distinct).collect();
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let levels = pairs.iter().map(|&(_, text)| text.to_owned()).collect();
            (levels, pairs.iter().map(|&(value, _)| value).collect(), true)
        } else {
            let positions = (0..distinct.len()).map(|level| level as f64).collect();
            (distinct.into_iter().map(str::to_owned).collect(), positions, false)
        };
        Some(ResultsAxis {
            column,
            label: self.column_label(column),
            levels,
            positions,
            numeric,
        })
    }

    /// Returns the name the views give value column `column`: a parameter's label, or an action's label and "tick".
    ///
    /// An action or a parameter this device's model does not declare goes by the name its column gives it.
    fn column_label(&self, column: usize) -> String {
        let name = &self.value_columns[column];
        if let Some(action) = name.strip_prefix(ACTION_COLUMN_PREFIX) {
            let label = self.action_labels.get(action).map_or(action, String::as_str);
            return format!("{label} tick");
        }
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == name)
            .map_or_else(|| name.clone(), |descriptor| descriptor.label.to_owned())
    }

    /// Sets the stat and reducer columns of a sweep that has planned its runs, unless the store knows them already.
    pub fn set_columns(&mut self, stat_columns: &[String], reducer_columns: &[String]) {
        if self.stat_columns.is_empty() && self.reducer_columns.is_empty() {
            self.stat_columns = stat_columns.to_vec();
            self.reducer_columns = reducer_columns.to_vec();
            self.revision += 1;
        }
    }

    /// Adds a finished run, or replaces the run of the same id, holding its series while it fits the budget.
    ///
    /// `series_dropped` says whether the run's series was left out of `outcome` for a budget. An empty series left
    /// in marks a run that recorded none.
    pub fn push_run(&mut self, outcome: RunOutcome, series_dropped: bool) {
        self.insert_run(outcome, series_dropped);
        self.revision += 1;
    }

    fn insert_run(&mut self, mut outcome: RunOutcome, series_dropped: bool) {
        let width = outcome.series.width();
        let series = std::mem::replace(&mut outcome.series, SeriesBuffer::new(width));
        let run_id = outcome.run.run_id;
        if series.is_empty() && !series_dropped {
            self.empty_series_runs.insert(run_id);
        } else {
            self.empty_series_runs.remove(&run_id);
        }
        if series.is_empty() {
            self.series.remove(run_id);
        } else {
            self.series.insert(run_id, series);
        }
        if let Some(&position) = self.positions.get(&run_id) {
            self.runs[position] = outcome;
            return;
        }
        let position = self.runs.len();
        let config_id = outcome.run.config_id;
        self.runs.push(outcome);
        self.positions.insert(run_id, position);
        if let Some(config) = self.configs.get_mut(&config_id) {
            let runs = &self.runs;
            let slot = config.runs.partition_point(|&held| runs[held].run.run_id < run_id);
            config.runs.insert(slot, position);
        }
    }

    /// Adds the configs of the candidates `update` reports, and the batch to the search's course.
    ///
    /// A batch the course holds already, as a resumed search reports again, adds no second entry.
    pub fn push_search_update(&mut self, update: &SearchUpdate) {
        let Some(search) = &mut self.search else {
            return;
        };
        let known = search
            .history
            .batches
            .last()
            .is_some_and(|last| last.batch >= update.batch);
        if !known {
            search.history.push(update);
        }
        let added: BTreeMap<u64, ConfigTexts> = update
            .evaluated
            .iter()
            .filter(|evaluated| !self.configs.contains_key(&evaluated.candidate_id))
            .map(|evaluated| {
                let texts = config_texts(&evaluated.config, &self.descriptors);
                (evaluated.candidate_id, texts)
            })
            .collect();
        if !added.is_empty() && !self.descriptors.is_empty() {
            self.add_configs(added);
        }
        self.revision += 1;
    }

    /// Adds the configs `added` beside the configs held, finds the axes again and gives each config its runs.
    fn add_configs(&mut self, added: BTreeMap<u64, ConfigTexts>) {
        let mut texts: BTreeMap<u64, ConfigTexts> = self
            .configs
            .iter()
            .map(|(&config_id, config)| {
                let texts = ConfigTexts {
                    block: config.block,
                    texts: config.texts.clone(),
                };
                (config_id, texts)
            })
            .collect();
        for (config_id, config) in added {
            texts.entry(config_id).or_insert(config);
        }
        self.set_configs(texts);
        let mut positions: Vec<usize> = (0..self.runs.len()).collect();
        positions.sort_by_key(|&position| self.runs[position].run.run_id);
        for position in positions {
            let config_id = self.runs[position].run.config_id;
            if let Some(config) = self.configs.get_mut(&config_id) {
                config.runs.push(position);
            }
        }
    }

    /// Search the results come from, `None` for a sweep.
    pub fn search_log(&self) -> Option<&SearchLog> {
        self.search.as_ref()
    }

    /// Returns whether the results come from a search, whose configs are its candidates.
    pub fn is_search(&self) -> bool {
        self.search.is_some()
    }

    /// Marks whether every run of the sweep is held.
    pub fn set_complete(&mut self, complete: bool) {
        self.complete = complete;
        self.revision += 1;
    }

    /// Count of changes to the store.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Runs in the order they arrived.
    pub fn runs(&self) -> &[RunOutcome] {
        &self.runs
    }

    pub fn run(&self, run_id: u64) -> Option<&RunOutcome> {
        self.runs.get(*self.positions.get(&run_id)?)
    }

    /// Number of runs that ended on a fault or a timeout.
    pub fn failed_count(&self) -> usize {
        self.runs.iter().filter(|outcome| outcome.status.is_failure()).count()
    }

    pub fn series(&self, run_id: u64) -> Option<&SeriesBuffer> {
        self.series.get(run_id)
    }

    pub fn series_cache(&self) -> &SeriesCache {
        &self.series
    }

    /// Number of runs that recorded no series rows.
    pub fn empty_series_count(&self) -> usize {
        self.empty_series_runs.len()
    }

    pub fn axes(&self) -> &[ResultsAxis] {
        &self.axes
    }

    pub fn stat_columns(&self) -> &[String] {
        &self.stat_columns
    }

    pub fn reducer_columns(&self) -> &[String] {
        &self.reducer_columns
    }

    /// Returns the name the views give reducer column `output`, as in `Infected, max`.
    pub fn output_label(&self, output: usize) -> String {
        output_label(self.reducer_columns.get(output).map_or("", String::as_str))
    }

    /// Ids of every config, planned or seen in a run.
    pub fn config_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.configs.keys().copied()
    }

    /// Number of configs, planned or seen in a run.
    pub fn config_count(&self) -> usize {
        self.configs.len()
    }

    /// Returns the level of config `config_id` on axis `axis`.
    pub fn config_level(&self, config_id: u64, axis: usize) -> Option<usize> {
        let level = *self.configs.get(&config_id)?.levels.get(axis)?;
        (level != NO_LEVEL).then_some(level)
    }

    /// Returns the word the views give a config, "Candidate" for the results of a search.
    pub fn config_noun(&self) -> &'static str {
        if self.is_search() { "Candidate" } else { "Config" }
    }

    /// Returns a name for config `config_id` that gives its level on every axis.
    pub fn config_label(&self, config_id: u64) -> String {
        let noun = self.config_noun();
        let Some(config) = self.configs.get(&config_id) else {
            return format!("{noun} {config_id}");
        };
        let levels: Vec<String> = self
            .axes
            .iter()
            .filter_map(|axis| Some(format!("{} {}", axis.label, config.texts.get(axis.column)?)))
            .collect();
        if levels.is_empty() {
            format!("{noun} {config_id}")
        } else {
            format!("{noun} {config_id}: {}", levels.join(", "))
        }
    }

    /// Returns the label and value of each column config `config_id` varies, the value as `runs.csv` writes it.
    ///
    /// A search's config lists every parameter and tick the search picks, and a sweep's config its level on every
    /// axis. Returns `None` for a config the store does not hold, as a search candidate before its batch ends.
    pub fn config_values(&self, config_id: u64) -> Option<Vec<(String, String)>> {
        let config = self.configs.get(&config_id)?;
        let columns: Vec<usize> = match &self.search {
            Some(search) => search
                .spec
                .space
                .iter()
                .filter_map(|factor| self.target_column(&factor.target))
                .collect(),
            None => self.axes.iter().map(|axis| axis.column).collect(),
        };
        let values = columns
            .into_iter()
            .filter_map(|column| Some((self.column_label(column), config.texts.get(column)?.clone())))
            .collect();
        Some(values)
    }

    /// Returns the values of config `config_id` on one line, as in "Infection Rate 0.2, Recovery Rate 0.05", or
    /// `None` for a config the store does not hold.
    pub fn config_values_text(&self, config_id: u64) -> Option<String> {
        let values = self.config_values(config_id)?;
        let parts: Vec<String> = values.iter().map(|(label, value)| format!("{label} {value}")).collect();
        Some(parts.join(", "))
    }

    /// Returns the value column that `target` sets, `None` for a parameter or action the columns lack.
    fn target_column(&self, target: &FactorTarget) -> Option<usize> {
        let name = match target {
            FactorTarget::Param(id) => id.clone(),
            FactorTarget::Action(name) => format!("{ACTION_COLUMN_PREFIX}{name}"),
        };
        self.value_columns.iter().position(|column| *column == name)
    }

    /// Returns the lowest and highest finite value of the output column `column`, as in `Infected:max`, over the
    /// runs that did not fail. Returns `None` for a column the runs do not record, or record no value of.
    pub fn output_range(&self, column: &str) -> Option<(f64, f64)> {
        let output = self.reducer_columns.iter().position(|name| name == column)?;
        self.runs
            .iter()
            .filter(|outcome| !outcome.status.is_failure())
            .filter_map(|outcome| outcome.reducers.get(output).copied().flatten())
            .filter(|value| value.is_finite())
            .fold(None, |range, value| match range {
                None => Some((value, value)),
                Some((low, high)) => Some((f64::min(low, value), f64::max(high, value))),
            })
    }

    /// Returns the runs of config `config_id` in order of run id.
    pub fn config_runs(&self, config_id: u64) -> impl Iterator<Item = &RunOutcome> + '_ {
        let positions = self.configs.get(&config_id).map_or(&[][..], |config| &config.runs[..]);
        positions.iter().map(|&position| &self.runs[position])
    }

    /// Returns the values of reducer column `output` over the runs of `config` that did not fail, in order of run id.
    fn output_values<'a>(&'a self, config: &'a ConfigEntry, output: usize) -> impl Iterator<Item = f64> + 'a {
        config.runs.iter().filter_map(move |&position| {
            let outcome = &self.runs[position];
            if outcome.status.is_failure() {
                return None;
            }
            outcome.reducers.get(output).copied().flatten()
        })
    }

    /// Returns whether `config` sits at every pinned level of `pins`, the axes in `free` left out.
    fn matches_pins(config: &ConfigEntry, pins: &[Option<usize>], free: &[Option<usize>]) -> bool {
        pins.iter().enumerate().all(|(axis, pin)| {
            free.contains(&Some(axis)) || pin.is_none_or(|level| config.levels.get(axis) == Some(&level))
        })
    }

    /// Returns the band of stat column `stat` over the runs of config `config_id` whose series is held and that did
    /// not fail, or `None` when there is no such run.
    ///
    /// A run that ended early adds nothing to the ticks past its end.
    pub fn band(&self, config_id: u64, stat: usize, kind: BandKind) -> Option<Band> {
        let config = self.configs.get(&config_id)?;
        let mut samples: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
        let mut runs = 0;
        for &position in &config.runs {
            let outcome = &self.runs[position];
            let Some(series) = self.series.get(outcome.run.run_id) else {
                continue;
            };
            if outcome.status.is_failure() || stat >= series.width() {
                continue;
            }
            runs += 1;
            for (tick, row) in series.rows() {
                if row[stat].is_finite() {
                    samples.entry(tick).or_default().push(row[stat]);
                }
            }
        }
        if runs == 0 {
            return None;
        }
        let mut band = Band {
            runs,
            ..Band::default()
        };
        for (tick, mut values) in samples {
            let (center, low, high) = spread(&mut values, kind);
            band.ticks.push(tick as f64);
            band.center.push(center);
            band.low.push(low);
            band.high.push(high);
        }
        Some(band)
    }

    /// Returns the response `query` asks for, one line per level of its group axis.
    ///
    /// A point pools the runs of every config at its x level that the pins let through, failed runs left out. Only a
    /// block that varies the x axis counts. A block that holds it at one level, as each block of a One at a time
    /// design does for every parameter but its own, would pool its variation into that one point.
    pub fn response(&self, query: &ResponseQuery) -> Vec<ResponseLine> {
        let free = [Some(query.x_axis), query.group_axis];
        let mut cells: BTreeMap<(Option<usize>, usize), RunningMoments> = BTreeMap::new();
        for config in self.configs.values() {
            if !self.block_varies(config.block, query.x_axis) || !Self::matches_pins(config, &query.pins, &free) {
                continue;
            }
            let Some(&x_level) = config.levels.get(query.x_axis).filter(|&&level| level != NO_LEVEL) else {
                continue;
            };
            let group_level = match query.group_axis {
                None => None,
                Some(axis) => match config.levels.get(axis) {
                    Some(&level) if level != NO_LEVEL => Some(level),
                    _ => continue,
                },
            };
            let moments = cells.entry((group_level, x_level)).or_default();
            for value in self.output_values(config, query.output) {
                moments.push(value);
            }
        }
        let Some(axis) = self.axes.get(query.x_axis) else {
            return Vec::new();
        };
        let mut lines: Vec<ResponseLine> = Vec::new();
        for ((group_level, level), moments) in cells {
            if moments.count() == 0 {
                continue;
            }
            let point = ResponsePoint {
                x: axis.positions[level],
                level,
                summary: moments.summary(),
            };
            match lines.last_mut() {
                Some(last) if last.group_level == group_level => last.points.push(point),
                _ => lines.push(ResponseLine {
                    group_level,
                    points: vec![point],
                }),
            }
        }
        lines
    }

    /// Returns the heatmap `query` asks for, or `None` when its axes are the same or out of range.
    ///
    /// A cell pools the runs of every config at its levels that the pins let through, failed runs left out. Only a
    /// block that varies the x or the y axis counts, and a config whose values an earlier config of the cell repeats
    /// counts once. A cell with no value, or too few values for its statistic, holds a value that is not finite.
    pub fn heat_grid(&self, query: &HeatQuery) -> Option<HeatGrid> {
        if query.x_axis == query.y_axis {
            return None;
        }
        let (columns, rows) = (
            self.axes.get(query.x_axis)?.levels.len(),
            self.axes.get(query.y_axis)?.levels.len(),
        );
        let free = [Some(query.x_axis), Some(query.y_axis)];
        let mut moments = vec![RunningMoments::default(); columns * rows];
        let mut configs = vec![Vec::new(); columns * rows];
        let mut pooled_texts: Vec<BTreeSet<&[String]>> = vec![BTreeSet::new(); columns * rows];
        for (&config_id, config) in &self.configs {
            let varies = self.block_varies(config.block, query.x_axis) || self.block_varies(config.block, query.y_axis);
            if !varies || !Self::matches_pins(config, &query.pins, &free) {
                continue;
            }
            let (Some(&column), Some(&row)) = (config.levels.get(query.x_axis), config.levels.get(query.y_axis)) else {
                continue;
            };
            if column == NO_LEVEL || row == NO_LEVEL {
                continue;
            }
            let cell = row * columns + column;
            if !pooled_texts[cell].insert(config.texts.as_slice()) {
                continue;
            }
            configs[cell].push(config_id);
            for value in self.output_values(config, query.output) {
                moments[cell].push(value);
            }
        }
        let values = moments
            .iter()
            .map(|moments| {
                let (mean, deviation) = (moments.mean(), moments.standard_deviation());
                let value = match query.color_by {
                    HeatColor::Mean => mean,
                    HeatColor::StandardDeviation => deviation,
                    HeatColor::CoefficientOfVariation => mean
                        .zip(deviation)
                        .filter(|&(mean, _)| mean != 0.0)
                        .map(|(mean, deviation)| deviation / mean.abs()),
                };
                value.unwrap_or(f64::NAN)
            })
            .collect();
        Some(HeatGrid {
            columns,
            rows,
            values,
            counts: moments.iter().map(RunningMoments::count).collect(),
            configs,
        })
    }

    /// Returns the positions of the runs `filter` lets through, in the order `sort` asks for, ties by run id.
    ///
    /// A missing value sorts last in either direction.
    pub fn row_order(&self, filter: RunsFilter, selected: &BTreeSet<u64>, sort: RunsSort) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.runs.len())
            .filter(|&position| {
                let outcome = &self.runs[position];
                match filter {
                    RunsFilter::All => true,
                    RunsFilter::Failed => outcome.status.is_failure(),
                    RunsFilter::SelectedConfigs => selected.contains(&outcome.run.config_id),
                }
            })
            .collect();
        order.sort_by(|&a, &b| {
            let (first, second) = (&self.runs[a], &self.runs[b]);
            let ordering = match (self.sort_key(first, sort.column), self.sort_key(second, sort.column)) {
                (Some(x), Some(y)) if sort.descending => y.compare(x),
                (Some(x), Some(y)) => x.compare(y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            ordering.then(first.run.run_id.cmp(&second.run.run_id))
        });
        order
    }

    /// Returns the value `outcome` sorts by in `column`, `None` when it has none.
    fn sort_key(&self, outcome: &RunOutcome, column: RunsColumn) -> Option<SortValue> {
        let run = &outcome.run;
        match column {
            RunsColumn::Run => Some(SortValue::Whole(run.run_id)),
            RunsColumn::Config => Some(SortValue::Whole(run.config_id)),
            RunsColumn::Replicate => Some(SortValue::Whole(run.rep)),
            RunsColumn::Seed => Some(SortValue::Whole(run.seed)),
            RunsColumn::Status => Some(SortValue::Whole(outcome.status as u64)),
            RunsColumn::Ticks => Some(SortValue::Whole(outcome.ticks)),
            RunsColumn::Time => Some(outcome.wall_ms)
                .filter(|time| time.is_finite())
                .map(SortValue::Real),
            RunsColumn::Axis(axis) => self
                .config_level(run.config_id, axis)
                .map(|level| SortValue::Whole(level as u64)),
            RunsColumn::Output(output) => outcome.reducers.get(output).copied().flatten().map(SortValue::Real),
        }
    }

    /// Reason the runs cannot replay, `None` when they can.
    pub fn replay_refusal(&self) -> Option<&str> {
        self.plan.as_ref().err().map(String::as_str)
    }

    /// Returns the replay of run `run_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Self::replay_refusal`], or a message when the store holds no run `run_id` or the plan gives the run
    /// another config, replicate or seed than its record. A search's run fails when its candidate's values do not
    /// give the key its record holds.
    pub fn replay(&self, run_id: u64) -> Result<Replay, String> {
        let plan = self.plan.as_ref().map_err(Clone::clone)?;
        let outcome = self
            .run(run_id)
            .ok_or_else(|| format!("No run {run_id} in these results"))?;
        let plan = match plan {
            ReplayPlan::Sweep(plan) => plan,
            ReplayPlan::Search(search_plan) => return self.search_replay(search_plan, outcome),
        };
        if plan.run(run_id) != Some(outcome.run) {
            return Err(format!("Run {run_id} does not match the plan of its sweep"));
        }
        plan.replay(run_id)
            .ok_or_else(|| format!("Run {run_id} does not match the plan of its sweep"))
    }

    /// Returns the replay of `outcome`, a run of the search of `search_plan`, from the values of its candidate.
    fn search_replay(&self, search_plan: &SearchPlan, outcome: &RunOutcome) -> Result<Replay, String> {
        let run_id = outcome.run.run_id;
        let mismatch = || format!("Run {run_id} does not match the search it comes from");
        let config = self.configs.get(&outcome.run.config_id).ok_or_else(mismatch)?;
        let param_count = self.descriptors.len();
        if config.texts.len() != param_count + search_plan.base().actions().len() {
            return Err(mismatch());
        }
        let (param_texts, tick_texts) = config.texts.split_at(param_count);
        let params = self
            .descriptors
            .iter()
            .zip(param_texts)
            .map(|(descriptor, text)| parse_value(&descriptor.kind, text).ok())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(mismatch)?;
        let action_ticks = tick_texts
            .iter()
            .map(|text| text.parse().ok())
            .collect::<Option<Vec<u64>>>()
            .ok_or_else(mismatch)?;
        let config = Config {
            block: config.block,
            params,
            action_ticks,
        };
        if search_plan.run_key(&outcome.run, &config) != outcome.run_key {
            return Err(mismatch());
        }
        Ok(search_plan.replay(&outcome.run, &config))
    }

    /// Returns a `henad-cli` command that steps run `run_id` to the tick it ended on and writes its stats.
    ///
    /// The command sets each parameter that differs from the model's default and fires each action due by the end.
    ///
    /// # Errors
    ///
    /// Returns the message of [`Self::replay`] for a run that does not replay.
    pub fn cli_command(&self, run_id: u64) -> Result<String, String> {
        let replay = self.replay(run_id)?;
        let (Ok(plan), Some(outcome)) = (&self.plan, self.run(run_id)) else {
            return Err(format!("No run {run_id} in these results"));
        };
        let plan = plan.base();
        let mut words = vec![
            "henad-cli".to_owned(),
            shell_word(&replay.model),
            "--seed".to_owned(),
            replay.seed.to_string(),
        ];
        for (descriptor, value) in self.descriptors.iter().zip(&replay.params) {
            if *value != descriptor.kind.default_value() {
                let assignment = format!("{}={}", descriptor.id, format_value(&descriptor.kind, value));
                words.extend(["--set".to_owned(), shell_word(&assignment)]);
            }
        }
        for scheduled in replay.schedule.entries() {
            if scheduled.tick <= outcome.ticks {
                words.extend([
                    "--act".to_owned(),
                    shell_word(&format!("{}@{}", scheduled.id, scheduled.tick)),
                ]);
            }
        }
        let warmup = plan.run_settings().warmup.min(outcome.ticks);
        words.extend([
            "--warmup".to_owned(),
            warmup.to_string(),
            "--steps".to_owned(),
            (outcome.ticks - warmup).to_string(),
        ]);
        let stats_every = plan.measure_settings().stats_every;
        if stats_every != 1 {
            words.extend(["--stats-every".to_owned(), stats_every.to_string()]);
        }
        words.extend(["--export-stats".to_owned(), format!("run-{run_id}.csv")]);
        Ok(words.join(" "))
    }

    /// Folder that holds the sweep's files, `None` for a sweep held in memory or picked files.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// Returns the ids of the runs of the configs `config_ids` whose series is not held, runs that recorded none
    /// left out.
    pub fn runs_without_series(&self, config_ids: &BTreeSet<u64>) -> BTreeSet<u64> {
        config_ids
            .iter()
            .flat_map(|&config_id| self.config_runs(config_id))
            .map(|outcome| outcome.run.run_id)
            .filter(|&run_id| self.series.get(run_id).is_none() && !self.empty_series_runs.contains(&run_id))
            .collect()
    }

    /// Records that the runs `run_ids` have no series rows, so [`Self::runs_without_series`] leaves them out.
    pub fn record_empty_series(&mut self, run_ids: &BTreeSet<u64>) {
        let known = run_ids.iter().filter(|run_id| self.positions.contains_key(run_id));
        self.empty_series_runs.extend(known);
        self.revision += 1;
    }

    /// Returns the bytes of series a load can add while the series of `kept_runs` stay held.
    pub fn series_room(&self, kept_runs: &BTreeSet<u64>) -> usize {
        let kept_bytes: usize = kept_runs
            .iter()
            .filter_map(|&run_id| self.series.get(run_id))
            .map(series_bytes)
            .sum();
        self.series.budget.saturating_sub(kept_bytes)
    }

    /// Holds `series`, series of runs by id, dropping held series of runs outside `kept_runs` to make room, and
    /// returns the number that fit.
    pub fn insert_series(&mut self, series: BTreeMap<u64, SeriesBuffer>, kept_runs: &BTreeSet<u64>) -> usize {
        let needed = series.values().map(series_bytes).sum();
        self.series.make_room(needed, kept_runs);
        let mut held = 0;
        for (run_id, buffer) in series {
            if self.positions.contains_key(&run_id) && self.series.insert(run_id, buffer) {
                held += 1;
            }
        }
        self.revision += 1;
        held
    }
}

/// Returns the name the Outputs section gives an output kind, as in `Tick of maximum`.
pub fn reducer_label(kind: ReducerKind) -> &'static str {
    match kind {
        ReducerKind::Final => "Final value",
        ReducerKind::Min => "Minimum",
        ReducerKind::Max => "Maximum",
        ReducerKind::Mean => "Mean",
        ReducerKind::ArgMax => "Tick of maximum",
        ReducerKind::ArgMin => "Tick of minimum",
        ReducerKind::FirstCrossing(_) => "First tick crossing",
        ReducerKind::WindowMean { .. } => "Mean over window",
    }
}

/// Returns the name the views give the output column `name`, as in `Infected, tick of maximum` for
/// `Infected:argmax`.
///
/// A threshold or a window stays in the name, as in `Infected, first tick <= 10`. A kind that does not parse shows as
/// written.
pub fn output_label(name: &str) -> String {
    let Some((column, written)) = name.rsplit_once(':') else {
        return name.to_owned();
    };
    match written.parse::<ReducerKind>() {
        Ok(kind) => column_output_label(column, kind),
        Err(_) => format!("{column}, {written}"),
    }
}

/// Returns the name the views give output `kind` of the stat column `column`, as [`output_label`] does for its
/// written name.
pub fn column_output_label(column: &str, kind: ReducerKind) -> String {
    match kind {
        ReducerKind::FirstCrossing(comparison) => format!(
            "{column}, first tick {} {}",
            comparison.comparator.as_str(),
            comparison.threshold
        ),
        ReducerKind::WindowMean { start, end } => format!("{column}, mean over ticks {start} to {end}"),
        kind => format!("{column}, {}", reducer_label(kind).to_lowercase()),
    }
}

/// Returns the label `entry` declares for each action of `actions`, by the name the spec gives the action.
fn action_labels(actions: &[ActionSpec], entry: &ModelEntry) -> BTreeMap<String, String> {
    actions
        .iter()
        .filter_map(|action| {
            let declared = entry
                .action_descriptors
                .iter()
                .find(|declared| declared.id == action.id)?;
            let label = action_label(declared.label, &action.id, &action.name);
            Some((action.name.clone(), label))
        })
        .collect()
}

/// Returns the label of the action `id`, declared as `declared_label`, fired under the name `name`.
///
/// An action fired under another name carries it after the label, so two firings of one action stay apart. A name
/// the Sweep tab numbers, such as `seed_outbreak_2`, adds its number alone, as in "Seed outbreak 2".
pub fn action_label(declared_label: &str, id: &str, name: &str) -> String {
    let number = name
        .strip_prefix(id)
        .and_then(|rest| rest.strip_prefix('_'))
        .filter(|rest| rest.parse::<u64>().is_ok());
    if name == id {
        declared_label.to_owned()
    } else if let Some(number) = number {
        format!("{declared_label} {number}")
    } else {
        format!("{declared_label} ({name})")
    }
}

/// Returns the names of the value columns of `plan`: each parameter of `params`, then each action's tick.
fn plan_value_columns(plan: &Plan, params: &[ParamDescriptor]) -> Vec<String> {
    let actions = plan.actions().iter().map(|action| action.column_name());
    params.iter().map(|param| param.id.to_owned()).chain(actions).collect()
}

/// Returns the block and values of every config of `plan` by id, as `runs.csv` writes them for the parameters
/// `params`.
fn plan_texts(plan: &Plan, params: &[ParamDescriptor]) -> BTreeMap<u64, ConfigTexts> {
    plan.configs()
        .iter()
        .enumerate()
        .map(|(config_id, config)| (config_id as u64, config_texts(config, params)))
        .collect()
}

/// Returns the block and values of `config`, as `runs.csv` writes them for the parameters `params`.
fn config_texts(config: &Config, params: &[ParamDescriptor]) -> ConfigTexts {
    let values = params
        .iter()
        .zip(&config.params)
        .map(|(param, value)| format_value(&param.kind, value));
    let ticks = config.action_ticks.iter().map(u64::to_string);
    ConfigTexts {
        block: config.block,
        texts: values.chain(ticks).collect(),
    }
}

/// Returns whether each of `axis_count` axes takes more than one level within a block of `configs`, by block index.
fn block_axes(configs: &BTreeMap<u64, ConfigEntry>, axis_count: usize) -> BTreeMap<usize, Vec<bool>> {
    let mut first_levels: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut varied: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
    for config in configs.values() {
        let first = first_levels
            .entry(config.block)
            .or_insert_with(|| vec![NO_LEVEL; axis_count]);
        let varies = varied.entry(config.block).or_insert_with(|| vec![false; axis_count]);
        for (axis, &level) in config.levels.iter().enumerate() {
            if level == NO_LEVEL {
                continue;
            }
            if first[axis] == NO_LEVEL {
                first[axis] = level;
            } else if first[axis] != level {
                varies[axis] = true;
            }
        }
    }
    varied
}

/// Returns the centre and the low and high ends of the spread `kind` over `values`.
///
/// # Panics
///
/// Panics when `values` is empty and `kind` is [`BandKind::Percentiles`].
fn spread(values: &mut [f64], kind: BandKind) -> (f64, f64, f64) {
    let mut moments = RunningMoments::default();
    for &value in values.iter() {
        moments.push(value);
    }
    let mean = moments.mean().unwrap_or(f64::NAN);
    match kind {
        BandKind::StandardDeviation => {
            let deviation = moments.standard_deviation().unwrap_or(0.0);
            (mean, mean - deviation, mean + deviation)
        }
        BandKind::ConfidenceInterval => {
            let (low, high) = moments.summary().ci95.unwrap_or((mean, mean));
            (mean, low, high)
        }
        BandKind::Percentiles => {
            values.sort_by(f64::total_cmp);
            (
                percentile(values, 0.5),
                percentile(values, 0.1),
                percentile(values, 0.9),
            )
        }
    }
}

/// Returns the `fraction` percentile of the sorted `values`, interpolated linearly between the two nearest ranks.
fn percentile(values: &[f64], fraction: f64) -> f64 {
    let rank = (values.len() - 1) as f64 * fraction;
    let below = rank.floor() as usize;
    let above = (below + 1).min(values.len() - 1);
    values[below] + (rank - below as f64) * (values[above] - values[below])
}

/// Returns `text` as one word of a shell command, quoted when it holds anything but plain characters.
fn shell_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.,:=@/+%".contains(character));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::measure::SeriesBuffer;
    use henad_core::explore::outcome::{RunOutcome, RunStatus, StopReason};
    use henad_core::explore::plan::{Config, Plan};
    use henad_core::explore::search::{Aggregate, CandidateOrigin, Goal, Objective, SearchAlgorithm, SearchSpec};
    use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
    use henad_core::params::ParamValue;
    use henad_explore::schema::model_schema;
    use henad_explore::search_run::{EvaluatedCandidate, EvaluationReading, SearchPlan, SearchUpdate};
    use henad_models::registry::{ModelEntry, model_registry};

    use super::{
        BandKind, HeatColor, HeatQuery, ResponseQuery, ResultsStore, RunsColumn, RunsFilter, RunsSort, output_label,
        series_bytes, shell_word,
    };

    const STATS: [&str; 3] = ["Susceptible", "Infected", "Recovered"];

    fn sir() -> ModelEntry {
        model_registry(None)
            .into_iter()
            .find(|entry| entry.id == "sir")
            .expect("SIR is registered")
    }

    fn values(raw: &[&str]) -> LevelSpec {
        LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
    }

    /// Returns the plan of an SIR factorial over `factors` with `replicates` runs per config.
    fn plan(entry: &ModelEntry, factors: Vec<FactorSpec>, replicates: u64) -> Arc<Plan> {
        let mut spec = SweepSpec::new("sir");
        spec.run.steps = 20;
        spec.run.replicates = replicates;
        spec.measure.stats_every = 10;
        spec.measure.series_every = 10;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors,
            design_seed: None,
        }];
        Arc::new(spec.plan(&model_schema(entry)).expect("a valid spec"))
    }

    /// Returns a store for `plan` whose runs record one reducer, `Infected:max`.
    fn store(entry: &ModelEntry, plan: Arc<Plan>, series_budget: usize) -> ResultsStore {
        let mut store = ResultsStore::for_sweep(plan, entry, 0, None, series_budget);
        let stats = STATS.map(str::to_owned);
        store.set_columns(&stats, &["Infected:max".to_owned()]);
        store
    }

    /// Returns run `run_id` of `plan` with `status`, the reducer value `reducer`, and the Infected values `infected`
    /// sampled at ticks 0, 10, 20 and so on, ending at the last of them.
    fn outcome(plan: &Plan, run_id: u64, status: RunStatus, reducer: f64, infected: &[f64]) -> RunOutcome {
        let run = plan.run(run_id).expect("a planned run");
        let mut series = SeriesBuffer::new(STATS.len());
        for (sample, &value) in infected.iter().enumerate() {
            series.push(sample as u64 * 10, &[0.0, value, 0.0]);
        }
        RunOutcome {
            run,
            run_key: plan.run_key(&run),
            status,
            stop_reason: StopReason::Steps,
            ticks: (infected.len() as u64).saturating_sub(1) * 10,
            population: 256,
            build_ms: 1.0,
            wall_ms: 2.0,
            reducers: vec![Some(reducer)],
            series,
            note: None,
        }
    }

    fn close(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-9
    }

    fn assert_band(actual: &[f64], expected: &[f64], label: &str) {
        assert_eq!(actual.len(), expected.len(), "{label}: {actual:?}");
        for (&actual, &expected) in actual.iter().zip(expected) {
            assert!(close(actual, expected), "{label}: {actual} is not {expected}");
        }
    }

    #[test]
    fn a_band_matches_hand_computed_statistics() {
        let sir = sir();
        let plan = plan(
            &sir,
            vec![FactorSpec::param("infection_rate", values(&["0.1", "0.2"]))],
            4,
        );
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        store.push_run(outcome(&plan, 0, RunStatus::Ok, 7.0, &[1.0, 4.0, 7.0]), false);
        store.push_run(outcome(&plan, 1, RunStatus::Ok, 9.0, &[1.0, 6.0, 9.0]), false);
        // A run stopped at tick 10 adds nothing later, and a failed run adds nothing at all.
        store.push_run(outcome(&plan, 2, RunStatus::Ok, 8.0, &[1.0, 8.0]), false);
        store.push_run(
            outcome(&plan, 3, RunStatus::Panicked, 0.0, &[100.0, 100.0, 100.0]),
            false,
        );

        let band = store
            .band(0, 1, BandKind::StandardDeviation)
            .expect("config 0 has series");
        assert_eq!(band.runs, 3);
        assert_band(&band.ticks, &[0.0, 10.0, 20.0], "ticks");
        assert_band(&band.center, &[1.0, 6.0, 8.0], "means");
        assert_band(&band.low, &[1.0, 4.0, 8.0 - 2.0_f64.sqrt()], "mean minus SD");
        assert_band(&band.high, &[1.0, 8.0, 8.0 + 2.0_f64.sqrt()], "mean plus SD");

        let band = store
            .band(0, 1, BandKind::ConfidenceInterval)
            .expect("config 0 has series");
        assert_band(&band.low, &[1.0, 1.031_724_576_210_043, -4.706_204_736], "CI low");
        assert_band(&band.high, &[1.0, 10.968_275_423_789_958, 20.706_204_736], "CI high");

        let band = store.band(0, 1, BandKind::Percentiles).expect("config 0 has series");
        assert_band(&band.center, &[1.0, 6.0, 8.0], "medians");
        assert_band(&band.low, &[1.0, 4.4, 7.2], "10th percentiles");
        assert_band(&band.high, &[1.0, 7.6, 8.8], "90th percentiles");

        assert!(
            store.band(1, 1, BandKind::StandardDeviation).is_none(),
            "config 1 has no runs"
        );
    }

    /// Returns a store of a 2 by 2 factorial of infection and recovery rates with 2 replicates, holding `Infected:max`
    /// values of 10 and 14, 4 and 6, 30 and 34, and a failed 99 and 12, config by config.
    fn factorial() -> (ResultsStore, usize, usize) {
        let sir = sir();
        let factors = vec![
            FactorSpec::param("infection_rate", values(&["0.1", "0.2"])),
            FactorSpec::param("recovery_rate", values(&["0.05", "0.1"])),
        ];
        let plan = plan(&sir, factors, 2);
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        let recorded = [
            (RunStatus::Ok, 10.0),
            (RunStatus::Ok, 14.0),
            (RunStatus::Ok, 4.0),
            (RunStatus::Ok, 6.0),
            (RunStatus::Ok, 30.0),
            (RunStatus::Ok, 34.0),
            (RunStatus::Panicked, 99.0),
            (RunStatus::Ok, 12.0),
        ];
        for (run_id, (status, value)) in recorded.into_iter().enumerate() {
            store.push_run(outcome(&plan, run_id as u64, status, value, &[0.0, 1.0, 2.0]), false);
        }
        let axis = |label: &str| {
            store
                .axes()
                .iter()
                .position(|axis| axis.label == label)
                .expect("the rate is varied")
        };
        let (infection, recovery) = (axis("Infection Rate"), axis("Recovery Rate"));
        (store, infection, recovery)
    }

    #[test]
    fn axes_list_the_varied_values_in_order() {
        let (store, infection, recovery) = factorial();
        assert_eq!(store.axes().len(), 2, "fixed parameters are no axis");
        assert_eq!(store.axes()[infection].levels, ["0.1", "0.2"]);
        assert_eq!(store.axes()[recovery].positions, [0.05, 0.1]);
        assert!(store.axes()[recovery].numeric);
        assert_eq!(store.config_level(3, infection), Some(1));
        assert_eq!(
            store.config_label(1),
            "Config 1: Infection Rate 0.1, Recovery Rate 0.1".to_owned()
        );
        assert_eq!(store.output_label(0), "Infected, maximum");
    }

    #[test]
    fn a_config_lists_the_values_it_varies_and_an_output_its_range() {
        let (store, _, _) = factorial();
        let values = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|&(label, value)| (label.to_owned(), value.to_owned()))
                .collect()
        };
        assert_eq!(
            store.config_values(1),
            Some(values(&[("Infection Rate", "0.1"), ("Recovery Rate", "0.1")]))
        );
        assert_eq!(
            store.config_values_text(1).as_deref(),
            Some("Infection Rate 0.1, Recovery Rate 0.1")
        );
        assert_eq!(store.config_values(99), None, "a config the plan lacks");
        assert_eq!(
            store.output_range("Infected:max"),
            Some((4.0, 34.0)),
            "the failed run's 99 is left out"
        );
        assert_eq!(store.output_range("Infected:min"), None, "a column the runs lack");
    }

    #[test]
    fn a_search_candidate_lists_its_searched_values_once_its_batch_ends() {
        let sir = sir();
        let schema = model_schema(&sir);
        let mut spec = SweepSpec::new("sir");
        spec.search = Some(SearchSpec {
            algorithm: SearchAlgorithm::Random,
            max_evaluations: 4,
            batch_size: 2,
            objective: Some(Objective {
                column: "Infected:max".to_owned(),
                goal: Goal::Maximize,
                aggregate: Aggregate::Median,
            }),
            space: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.0,
                    max: 1.0,
                    step: None,
                },
            )],
        });
        let search_plan = SearchPlan::new(&spec, &schema).expect("a valid search");
        let mut store = ResultsStore::for_search(Arc::new(search_plan), &sir, 0, None, usize::MAX);
        assert_eq!(store.config_values(0), None, "no batch has ended");

        let mut params: Vec<ParamValue> = sir
            .param_descriptors
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        params[2] = ParamValue::F32(0.25);
        let evaluated = EvaluatedCandidate {
            candidate_id: 0,
            batch: 0,
            origin: CandidateOrigin::Random,
            replicate_offset: 0,
            replicates: 1,
            config: Config {
                block: 0,
                params,
                action_ticks: Vec::new(),
            },
            failed_count: 0,
            reading: EvaluationReading::Objective {
                objective: 1.0,
                pooled_objective: 1.0,
                pooled_replicates: 1,
            },
        };
        store.push_search_update(&SearchUpdate {
            batch: 0,
            evaluations: 1,
            runs: 1,
            evaluated: vec![evaluated],
            best: None,
            generations: Vec::new(),
            landed_entries: Vec::new(),
            filled_cells: 0,
            pattern_settings: None,
        });
        assert_eq!(
            store.config_values_text(0).as_deref(),
            Some("Infection Rate 0.25"),
            "one candidate varies no axis, and still names what the search picks"
        );
    }

    #[test]
    fn a_response_pools_the_runs_at_each_level() {
        let (store, infection, recovery) = factorial();
        let mut query = ResponseQuery {
            x_axis: infection,
            output: 0,
            group_axis: None,
            pins: vec![None; 2],
        };
        let pooled = store.response(&query);
        assert_eq!(pooled.len(), 1);
        let points = &pooled[0].points;
        assert_eq!(points.iter().map(|point| point.x).collect::<Vec<_>>(), [0.1, 0.2]);
        let (low, high) = (&points[0].summary, &points[1].summary);
        assert_eq!((low.n, high.n), (4, 3), "the failed run is left out");
        assert!(close(low.mean.unwrap_or_default(), 8.5));
        assert!(close(low.standard_deviation.unwrap_or_default(), 4.434_711_565_216_69));
        assert!(close(high.mean.unwrap_or_default(), 76.0 / 3.0));
        let (ci_low, ci_high) = high.ci95.expect("three values have an interval");
        assert!(close(ci_high - 76.0 / 3.0, 29.111_437_332_678_68), "{ci_high}");
        assert!(close(76.0 / 3.0 - ci_low, ci_high - 76.0 / 3.0));

        query.group_axis = Some(recovery);
        let grouped = store.response(&query);
        let means: Vec<Vec<f64>> = grouped
            .iter()
            .map(|line| {
                line.points
                    .iter()
                    .map(|point| point.summary.mean.unwrap_or_default())
                    .collect()
            })
            .collect();
        assert_eq!(means, [vec![12.0, 32.0], vec![5.0, 12.0]]);
        assert_eq!(grouped[1].points[1].summary.ci95, None, "one value has no interval");

        query.group_axis = None;
        query.pins[recovery] = Some(1);
        let pinned = store.response(&query);
        let means: Vec<f64> = pinned[0]
            .points
            .iter()
            .map(|point| point.summary.mean.unwrap_or_default())
            .collect();
        assert_eq!(means, [5.0, 12.0]);
    }

    #[test]
    fn a_heatmap_matches_hand_computed_cells() {
        let (store, infection, recovery) = factorial();
        let mut query = HeatQuery {
            x_axis: infection,
            y_axis: recovery,
            output: 0,
            color_by: HeatColor::Mean,
            pins: vec![None; 2],
        };
        let grid = store.heat_grid(&query).expect("two axes");
        assert_eq!((grid.columns, grid.rows), (2, 2));
        assert_eq!(grid.values, [12.0, 32.0, 5.0, 12.0]);
        assert_eq!(grid.counts, [2, 2, 2, 1]);
        assert_eq!(grid.configs[grid.cell(1, 0)], [2]);
        assert_eq!(grid.range(), Some((5.0, 32.0)));

        query.color_by = HeatColor::StandardDeviation;
        let grid = store.heat_grid(&query).expect("two axes");
        let root_eight = 8.0_f64.sqrt();
        assert!(close(grid.values[0], root_eight) && close(grid.values[1], root_eight));
        assert!(close(grid.values[2], 2.0_f64.sqrt()));
        assert!(grid.values[3].is_nan(), "one value has no spread");

        query.color_by = HeatColor::CoefficientOfVariation;
        let grid = store.heat_grid(&query).expect("two axes");
        assert!(close(grid.values[0], root_eight / 12.0));
        assert!(close(grid.values[1], root_eight / 32.0));

        query.y_axis = infection;
        assert!(store.heat_grid(&query).is_none(), "an axis against itself");
    }

    #[test]
    fn vary_each_alone_pools_only_the_block_that_varies_an_axis() {
        let sir = sir();
        let block = |infection: &[&str], recovery: &[&str]| BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![
                FactorSpec::param("infection_rate", values(infection)),
                FactorSpec::param("recovery_rate", values(recovery)),
            ],
            design_seed: None,
        };
        let mut spec = SweepSpec::new("sir");
        spec.run.steps = 20;
        spec.run.replicates = 2;
        spec.measure.stats_every = 10;
        spec.measure.series_every = 10;
        // Configs 0 and 1 vary the infection rate, and configs 2 and 3 the recovery rate. Configs 1 and 2 are the
        // baseline both blocks share.
        spec.blocks = vec![block(&["0.1", "0.3"], &["0.05"]), block(&["0.3"], &["0.05", "0.1"])];
        let plan = Arc::new(spec.plan(&model_schema(&sir)).expect("a valid spec"));
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        for (run_id, value) in [10.0, 14.0, 20.0, 22.0, 20.0, 22.0, 40.0, 44.0].into_iter().enumerate() {
            store.push_run(outcome(&plan, run_id as u64, RunStatus::Ok, value, &[0.0]), false);
        }
        let axis = |label: &str| {
            store
                .axes()
                .iter()
                .position(|axis| axis.label == label)
                .expect("the rate is varied")
        };
        let (infection, recovery) = (axis("Infection Rate"), axis("Recovery Rate"));
        let points = |x_axis| -> Vec<(f64, f64, u64)> {
            let query = ResponseQuery {
                x_axis,
                output: 0,
                group_axis: None,
                pins: vec![None; 2],
            };
            let response = store.response(&query);
            assert_eq!(response.len(), 1);
            response[0]
                .points
                .iter()
                .map(|point| (point.x, point.summary.mean.unwrap_or_default(), point.summary.n))
                .collect()
        };
        assert_eq!(
            points(infection),
            [(0.1, 12.0, 2), (0.3, 21.0, 2)],
            "the baseline is the infection block's alone"
        );
        assert_eq!(points(recovery), [(0.05, 21.0, 2), (0.1, 42.0, 2)]);

        let query = HeatQuery {
            x_axis: infection,
            y_axis: recovery,
            output: 0,
            color_by: HeatColor::Mean,
            pins: vec![None; 2],
        };
        let grid = store.heat_grid(&query).expect("two axes");
        assert_eq!(grid.counts, [2, 2, 0, 2], "the shared baseline counts once");
        assert_eq!(grid.values[grid.cell(1, 0)], 21.0);
        assert_eq!(grid.configs[grid.cell(1, 0)], [1]);
        assert!(grid.values[grid.cell(0, 1)].is_nan(), "no block runs this pair");
    }

    #[test]
    fn a_config_without_runs_is_a_cell_with_no_data() {
        let sir = sir();
        let factors = vec![
            FactorSpec::param("infection_rate", values(&["0.1", "0.2"])),
            FactorSpec::param("recovery_rate", values(&["0.05", "0.1"])),
        ];
        let plan = plan(&sir, factors, 1);
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        for run_id in 0..3 {
            store.push_run(
                outcome(&plan, run_id, RunStatus::Ok, 1.0 + run_id as f64, &[0.0]),
                false,
            );
        }
        let query = HeatQuery {
            x_axis: 0,
            y_axis: 1,
            output: 0,
            color_by: HeatColor::Mean,
            pins: vec![None; 2],
        };
        let grid = store.heat_grid(&query).expect("two axes");
        assert!(grid.values[3].is_nan(), "config 3 is still to run");
        assert_eq!(grid.counts[3], 0);
        assert_eq!(grid.configs[3], [3], "the cell still names its config");
    }

    #[test]
    fn the_series_budget_stops_holding_series_past_it() {
        let sir = sir();
        let plan = plan(
            &sir,
            vec![FactorSpec::param("infection_rate", values(&["0.1", "0.2"]))],
            2,
        );
        let first = outcome(&plan, 0, RunStatus::Ok, 1.0, &[1.0, 2.0, 3.0]);
        let first_series = first.series.clone();
        let budget = 2 * series_bytes(&first.series);
        let mut store = store(&sir, Arc::clone(&plan), budget);
        store.push_run(first, false);
        for run_id in 1..4 {
            store.push_run(outcome(&plan, run_id, RunStatus::Ok, 1.0, &[1.0, 2.0, 3.0]), false);
        }
        assert_eq!(store.runs().len(), 4, "every run is held");
        assert_eq!(store.series_cache().len(), 2);
        assert_eq!(store.series_cache().used_bytes(), budget);
        assert!(store.series(2).is_none() && store.series(3).is_none());

        // Loading config 1's series makes room by dropping config 0's.
        let kept_runs: BTreeSet<u64> = store.config_runs(1).map(|outcome| outcome.run.run_id).collect();
        let missing = store.runs_without_series(&[1].into());
        assert_eq!(missing, kept_runs);
        assert_eq!(store.series_room(&kept_runs), budget, "config 1 holds no series yet");
        assert_eq!(
            store.series_room(&[0].into()),
            budget - series_bytes(&first_series),
            "run 0's series stays"
        );
        let loaded = missing
            .iter()
            .map(|&run_id| {
                (
                    run_id,
                    outcome(&plan, run_id, RunStatus::Ok, 1.0, &[4.0, 5.0, 6.0]).series,
                )
            })
            .collect();
        assert_eq!(store.insert_series(loaded, &kept_runs), 2);
        assert!(store.series(0).is_none() && store.series(3).is_some());
        assert_eq!(store.series_cache().used_bytes(), budget);
    }

    #[test]
    fn runs_sort_by_a_column_and_filter_by_status_or_config() {
        let (store, infection, _) = factorial();
        let descending = RunsSort {
            column: RunsColumn::Output(0),
            descending: true,
        };
        let order = store.row_order(RunsFilter::All, &BTreeSet::new(), descending);
        let run_ids: Vec<u64> = order
            .iter()
            .map(|&position| store.runs()[position].run.run_id)
            .collect();
        assert_eq!(run_ids, [6, 5, 4, 1, 7, 0, 3, 2]);

        let failed = store.row_order(RunsFilter::Failed, &BTreeSet::new(), RunsSort::default());
        assert_eq!(failed, [6]);
        let by_level = RunsSort {
            column: RunsColumn::Axis(infection),
            descending: true,
        };
        let selected = store.row_order(RunsFilter::SelectedConfigs, &[0, 3].into(), by_level);
        let run_ids: Vec<u64> = selected
            .iter()
            .map(|&position| store.runs()[position].run.run_id)
            .collect();
        assert_eq!(run_ids, [6, 7, 0, 1], "ties keep run order");
    }

    #[test]
    fn an_output_label_names_its_kind_in_words() {
        assert_eq!(output_label("Infected:argmax"), "Infected, tick of maximum");
        assert_eq!(output_label("Infected:final"), "Infected, final value");
        assert_eq!(output_label("Infected:first<=10"), "Infected, first tick <= 10");
        assert_eq!(
            output_label("Recovered:mean@200..600"),
            "Recovered, mean over ticks 200 to 600"
        );
        assert_eq!(
            output_label("Infected:median"),
            "Infected, median",
            "an unknown kind shows as written"
        );
        assert_eq!(output_label("Infected"), "Infected");
    }

    #[test]
    fn an_action_axis_takes_the_label_the_model_declares() {
        let sir = sir();
        let names = ["seed_outbreak", "seed_outbreak_2", "late"];
        let mut spec = SweepSpec::new("sir");
        spec.run.steps = 20;
        spec.actions = names
            .map(|name| ActionSpec {
                name: name.to_owned(),
                ..ActionSpec::new("seed_outbreak", 0)
            })
            .to_vec();
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: names.map(|name| FactorSpec::action(name, values(&["0", "5"]))).to_vec(),
            design_seed: None,
        }];
        let plan = Arc::new(spec.plan(&model_schema(&sir)).expect("a valid spec"));
        let store = store(&sir, plan, usize::MAX);
        let labels: Vec<&str> = store.axes().iter().map(|axis| axis.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Seed outbreak tick",
                "Seed outbreak 2 tick",
                "Seed outbreak (late) tick"
            ]
        );
    }

    #[test]
    fn a_run_that_recorded_no_series_is_never_missing_one() {
        let sir = sir();
        let plan = plan(
            &sir,
            vec![FactorSpec::param("infection_rate", values(&["0.1", "0.2"]))],
            2,
        );
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        store.push_run(outcome(&plan, 0, RunStatus::Ok, 1.0, &[1.0, 2.0]), false);
        store.push_run(outcome(&plan, 1, RunStatus::Ok, 1.0, &[]), false);
        store.push_run(outcome(&plan, 2, RunStatus::Ok, 1.0, &[]), true);
        store.push_run(outcome(&plan, 3, RunStatus::Ok, 1.0, &[]), true);
        assert_eq!(store.empty_series_count(), 1);
        assert_eq!(store.runs_without_series(&[0, 1].into()), [2, 3].into());

        store.record_empty_series(&[3, 99].into());
        assert_eq!(store.empty_series_count(), 2, "run 99 is not in the results");
        assert_eq!(store.runs_without_series(&[0, 1].into()), [2].into());
    }

    #[test]
    fn a_command_sets_what_differs_and_steps_to_the_end() {
        let sir = sir();
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![("grid_width".to_owned(), "32".to_owned())];
        spec.run.warmup = 5;
        spec.run.steps = 40;
        spec.measure.stats_every = 5;
        spec.measure.series_every = 5;
        spec.seeds.root = 3;
        spec.actions = vec![
            ActionSpec::new("seed_outbreak", 10),
            ActionSpec {
                name: "late".to_owned(),
                ..ActionSpec::new("seed_outbreak", 40)
            },
        ];
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param("infection_rate", values(&["0.3", "0.45"]))],
            design_seed: None,
        }];
        let plan = Arc::new(spec.plan(&model_schema(&sir)).expect("a valid spec"));
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        let mut stopped = outcome(&plan, 1, RunStatus::Ok, 1.0, &[1.0]);
        stopped.ticks = 25;
        stopped.stop_reason = StopReason::Condition;
        store.push_run(stopped, false);

        let seed = plan.run(1).expect("a planned run").seed;
        assert_eq!(
            store.cli_command(1).expect("the run replays"),
            format!(
                "henad-cli sir --seed {seed} --set grid_width=32 --set infection_rate=0.45 --act seed_outbreak@10 \
                 --warmup 5 --steps 20 --stats-every 5 --export-stats run-1.csv"
            ),
            "the default infection rate is left out, and so is the action past the end"
        );
        assert!(store.cli_command(0).is_err(), "run 0 is not in the results");
        assert_eq!(shell_word("network=Small world"), "'network=Small world'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
    }

    /// Folder under the system's temporary folder, removed when dropped.
    #[cfg(not(target_arch = "wasm32"))]
    struct ScratchFolder(std::path::PathBuf);

    #[cfg(not(target_arch = "wasm32"))]
    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// Takes snapshots from `thread` until one reports `tick`, or gives up after ten seconds.
    #[cfg(not(target_arch = "wasm32"))]
    fn snapshot_at(
        thread: &mut henad_compute::cpu::sim_thread::SimThread,
        tick: u64,
    ) -> Option<henad_compute::snapshot::Snapshot> {
        for _ in 0..1000 {
            if let Some(snapshot) = thread.take_snapshot()
                && snapshot.tick == tick
            {
                return Some(snapshot);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_run_opened_from_a_folder_replays_to_its_row() {
        use henad_compute::cpu::sim_thread::SimThread;
        use henad_compute::fault::FaultSink;
        use henad_core::explore::stop::StopSpec;
        use henad_core::export::stats_csv::StatColumns;
        use henad_explore::progress::NoProgress;
        use henad_explore::result_set::ResultSet;
        use henad_explore::sweep::{SweepOptions, run_sweep};
        use henad_models::registry::ModelState;

        use crate::ui::results::store::ResultsSource;

        let registry = model_registry(None);
        let mut spec = SweepSpec::new("sir");
        spec.fixed = [("grid_width", "32"), ("grid_height", "32"), ("recovery_rate", "0.1")]
            .map(|(id, value)| (id.to_owned(), value.to_owned()))
            .to_vec();
        spec.run.warmup = 4;
        spec.run.steps = 36;
        spec.run.replicates = 2;
        spec.run.stop = Some(StopSpec::parse("Recovered >= 12", 0).expect("a valid condition"));
        spec.measure.stats_every = 5;
        spec.measure.series_every = 5;
        spec.seeds.root = 21;
        spec.actions = vec![ActionSpec::new("seed_outbreak", 9)];
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![
                FactorSpec::param("infection_rate", values(&["0.04", "0.3"])),
                FactorSpec::action("seed_outbreak", values(&["0", "9"])),
            ],
            design_seed: None,
        }];
        let folder = ScratchFolder(std::env::temp_dir().join(format!("henad-app-results-{}", std::process::id())));
        drop(std::fs::remove_dir_all(&folder.0));
        let options = SweepOptions {
            output_dir: Some(folder.0.clone()),
            ..SweepOptions::default()
        };
        let sir = registry
            .iter()
            .find(|entry| entry.id == "sir")
            .expect("SIR is registered");
        let source = henad_explore::sweep::SpecSource::default();
        let provenance = henad_explore::sweep::Provenance::default();
        run_sweep(sir, None, None, &spec, &source, &provenance, &options, &mut NoProgress).expect("the sweep runs");

        let set = ResultSet::open_dir(&folder.0, usize::MAX).expect("the folder reads");
        let store = ResultsStore::from_result_set(set, ResultsSource::Folder(folder.0.clone()), &registry, usize::MAX);
        assert_eq!(store.runs().len(), 8);
        assert!(store.complete && store.schema_matches);
        assert!(
            store.axes().iter().any(|axis| axis.label == "Seed outbreak tick"),
            "an action axis read from a folder takes the model's label"
        );
        let infected = store
            .stat_columns()
            .iter()
            .position(|name| name == "Infected")
            .expect("SIR counts its infected");
        let final_infected = store
            .reducer_columns()
            .iter()
            .position(|name| name == "Infected:final")
            .expect("the default reducers include the final value");
        assert!(
            store
                .runs()
                .iter()
                .any(|outcome| outcome.stop_reason == StopReason::Condition),
            "no run stopped early, so the recorded end is never tested"
        );

        for outcome in store.runs() {
            let run_id = outcome.run.run_id;
            let replay = store.replay(run_id).expect("the run replays");
            let Ok(ModelState::Cpu(state)) = (sir.create)(&replay.params, Some(replay.seed)) else {
                panic!("SIR builds on the CPU");
            };
            // Open at end steps to the tick the run ended on. A stop condition can end it before the plan's last tick.
            let mut thread = SimThread::new(state, 60.0, None, FaultSink::new());
            thread.set_schedule(replay.schedule.clone());
            thread.run_to(outcome.ticks);
            let reached = snapshot_at(&mut thread, outcome.ticks).expect("the replay reaches the run's last tick");
            let mut replayed = Vec::new();
            StatColumns::plan(&reached.stats)
                .extract(reached.tick, &reached.stats, &mut replayed)
                .expect("the stats fit their own columns");
            let (last_tick, last_row) = store
                .series(run_id)
                .and_then(|series| series.rows().last())
                .expect("the run's series is held");
            assert_eq!(last_tick, outcome.ticks, "run {run_id}");
            assert_eq!(
                replayed, last_row,
                "run {run_id}: the replay ends on the run's last row"
            );
            assert_eq!(
                Some(replayed[infected]),
                outcome.reducers[final_infected],
                "run {run_id}"
            );
        }
    }
}
