//! Results the Results tab holds, from the sweep or search in progress or from the files of an ended one, and the
//! statistics its views plot.
//!
//! A value column is a parameter or an action tick of `runs.csv`. An axis is a value column that takes more than one
//! value across the configs, and a level is one of those values. Runs are held in full, and their series within a
//! byte budget. The configs of a search are its candidates, known once the search is told their evaluations.
//!
//! Every statistic pools the runs of a config in order of run id. Note that `summary.csv` pools them in the order of
//! `runs.csv`, and a resumed sweep can write that file out of run-id order. The last digits of a statistic can then
//! differ between the two.

mod axes;
mod recorded;
mod replay;
mod series_cache;
pub mod statistics;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use henad_compute::entry::{ModelEntry, ModelLookupError};
use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::measure::SeriesBuffer;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::Plan;
use henad_core::explore::search::SearchSpec;
use henad_core::metadata::TimeScale;
use henad_core::params::ParamDescriptor;
use henad_explore::output::manifest::BuildRole;
use henad_explore::output::search_tables::SearchHistory;
use henad_explore::result_set::ResultSet;
use henad_explore::search_run::SearchPlan;

use crate::state::lookup_message;
use crate::ui::results::labels::output_label;
use crate::ui::sweep::draft::ExploreMode;
use crate::ui::sweep::session::SessionExecution;
use crate::ui::text::describe_error;
pub use axes::ResultsAxis;
use axes::{BlockAxes, ConfigEntry, ConfigTexts, plan_texts, plan_value_columns};
use recorded::{RecordedModel, RecordedSpec, action_labels};
use replay::{ReplayPlan, planned_texts, replay_plan};
pub use series_cache::SeriesCache;
pub use statistics::{
    BandKind, HeatColor, HeatGrid, HeatQuery, ResponseLine, ResponseQuery, RunsColumn, RunsFilter, RunsSort, SeriesBand,
};

/// Maximum number of configs a folder's manifest can record for the app to plan its sweep. The configs of a larger
/// sweep are listed from `runs.csv` alone.
const MAX_PLANNED_CONFIGS: u64 = 1 << 20;

/// Source the results come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsSource {
    /// The sweep or search started from the Sweep tab.
    Sweep,
    /// A folder a sweep wrote.
    #[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "a browser reads no folder"))]
    Folder(PathBuf),
    /// Files picked one by one, by name.
    Files(Vec<String>),
}

/// Settings and history of the search that produced the results.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchLog {
    pub spec: SearchSpec,
    pub history: SearchHistory,
    /// Reason the search tables of an opened folder cannot be read. `history` is then empty.
    pub table_error: Option<String>,
}

/// Runs, configs and series of one sweep.
#[derive(Debug)]
pub struct ResultsStore {
    pub source: ResultsSource,
    pub model_id: String,
    /// Name of the model as the sweep recorded it.
    pub model_name: String,
    /// Whether the model declares the parameters, stats and actions the sweep ran with.
    pub schema_matches: bool,
    /// Roles whose current build differs from a build some session of the sweep recorded, the engine role
    /// before the model role.
    pub changed_builds: Vec<BuildRole>,
    /// Roles outside `changed_builds` whose current or recorded build records neither a commit nor a source hash, and
    /// so cannot be compared, the engine role before the model role.
    pub unidentified_builds: Vec<BuildRole>,
    /// Whether two builds of the model on one seed step through identical states, as the model and the sweep both
    /// declare.
    pub replays_exactly: bool,
    /// Whether the app's entry for the model runs the code that every session of a folder's sweep ran its runs with,
    /// as a resume of the folder requires.
    ///
    /// `false` without an entry. For a sweep run in this app, `true` whenever the app has an entry.
    pub code_matches: bool,
    /// Model as the sweep recorded it. [`Self::model_changed`] compares it with a replacement.
    recorded: RecordedModel,
    /// Whether every run of the sweep is held.
    pub complete: bool,
    /// Settings a resume runs with: the memory budgets the folder's manifest records, and automatic concurrency.
    /// `None` for a sweep the app runs.
    pub recorded_execution: Option<SessionExecution>,
    /// Folder that holds the sweep's files, `None` for a sweep held in memory or picked files.
    folder: Option<PathBuf>,
    /// Plan used to replay the runs, or the reason they cannot replay: this device lacks the model, or the model
    /// rejects the spec.
    plan: Result<ReplayPlan, String>,
    /// Spec of a sweep read from files, `None` for a sweep run in this app. The plan of a sweep run in this app was
    /// made against the entry that ran it.
    recorded_spec: Option<RecordedSpec>,
    /// Search the results come from, `None` for a sweep.
    search: Option<SearchLog>,
    /// Parameters of the model, empty when this device lacks it.
    descriptors: Vec<ParamDescriptor>,
    /// Time scale of the model, `None` for a model with no time scale or when this device lacks it.
    time: Option<TimeScale>,
    /// Label of each action the sweep fires, by the name that its spec assigns to the action.
    /// Empty when this device lacks the model.
    action_labels: BTreeMap<String, String>,
    value_columns: Vec<String>,
    stat_columns: Vec<String>,
    reducer_columns: Vec<String>,
    axes: Vec<ResultsAxis>,
    /// Levels the configs of each block take on the axes, by block index.
    block_axes: BTreeMap<usize, BlockAxes>,
    configs: BTreeMap<u64, ConfigEntry>,
    /// Runs in the order they arrived, each with its series left empty.
    runs: Vec<RunOutcome>,
    /// Position in `runs` of each run, by run id.
    positions: BTreeMap<u64, usize>,
    /// Positions in `runs` of the runs whose config the store does not hold yet, by config id and in order of run id.
    unassigned_runs: BTreeMap<u64, Vec<usize>>,
    /// Number of runs that ended on a fault or a timeout.
    failed_runs: usize,
    /// Number of runs replaced by a later run of the same id.
    replaced_runs: u64,
    series: SeriesCache,
    /// Runs that recorded no series rows. A load cannot find rows for them either.
    empty_series_runs: BTreeSet<u64>,
    /// Revision counter, raised on every change. A view keeps its cache while the revision stays the same.
    revision: u64,
}

impl ResultsStore {
    /// Returns an empty store for the sweep of `plan` over `entry`, whose files go to `folder`.
    pub fn for_sweep(plan: Arc<Plan>, entry: &ModelEntry, folder: Option<PathBuf>, series_budget: usize) -> Self {
        let value_columns = plan_value_columns(&plan, entry.param_descriptors());
        let texts = plan_texts(&plan, entry.param_descriptors());
        let mut store = Self {
            source: ResultsSource::Sweep,
            model_id: entry.id().to_owned(),
            model_name: entry.name().to_owned(),
            schema_matches: true,
            changed_builds: Vec::new(),
            unidentified_builds: Vec::new(),
            replays_exactly: entry.metadata().replays_exactly,
            code_matches: true,
            recorded: RecordedModel::of_entry(entry),
            complete: false,
            recorded_execution: None,
            folder,
            action_labels: action_labels(plan.actions(), entry),
            plan: Ok(ReplayPlan::Sweep(plan)),
            recorded_spec: None,
            search: None,
            descriptors: entry.param_descriptors().to_vec(),
            time: entry.metadata().time.clone(),
            value_columns,
            stat_columns: Vec::new(),
            reducer_columns: Vec::new(),
            axes: Vec::new(),
            block_axes: BTreeMap::new(),
            configs: BTreeMap::new(),
            runs: Vec::new(),
            positions: BTreeMap::new(),
            unassigned_runs: BTreeMap::new(),
            failed_runs: 0,
            replaced_runs: 0,
            series: SeriesCache::new(series_budget),
            empty_series_runs: BTreeSet::new(),
            revision: 0,
        };
        store.set_configs(texts);
        store
    }

    /// Returns an empty store for the search of `search_plan` over `entry`, whose files go to `folder`.
    pub fn for_search(
        search_plan: Arc<SearchPlan>,
        entry: &ModelEntry,
        folder: Option<PathBuf>,
        series_budget: usize,
    ) -> Self {
        let base = Arc::clone(search_plan.base());
        let search = SearchLog {
            spec: search_plan.search().clone(),
            history: SearchHistory::default(),
            table_error: None,
        };
        let mut store = Self::for_sweep(base, entry, folder, series_budget);
        store.set_configs(BTreeMap::new());
        store.plan = Ok(ReplayPlan::Search(search_plan));
        store.search = Some(search);
        store
    }

    /// Returns a store of the runs `set` read from `source`, holding at most `series_budget` bytes of series.
    ///
    /// The runs replay through `model`, the app's lookup of the sweep's model. A sweep whose manifest records more than
    /// [`MAX_PLANNED_CONFIGS`] configs is never planned. Its configs come from `runs.csv` alone, and its runs do not
    /// replay.
    pub fn from_result_set(
        set: ResultSet,
        source: ResultsSource,
        model: Result<&ModelEntry, ModelLookupError>,
        series_budget: usize,
    ) -> Self {
        Self::listing_planned_configs(set, source, model, series_budget, MAX_PLANNED_CONFIGS)
    }

    /// Returns the store that [`Self::from_result_set`] returns, planning a sweep only while its manifest records at
    /// most `max_planned` configs, and listing configs missing from `runs.csv` only from a plan of at most that many.
    fn listing_planned_configs(
        set: ResultSet,
        source: ResultsSource,
        model: Result<&ModelEntry, ModelLookupError>,
        series_budget: usize,
        max_planned: u64,
    ) -> Self {
        let recorded = &set.manifest().model;
        let (model_id, model_name) = (recorded.id.clone(), recorded.name.clone());
        let model = model.map_err(|error| lookup_message(&error));
        let entry = model.as_ref().ok().copied();
        let schema_matches = entry.is_some_and(|entry| set.schema_matches(entry.schema()));
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
        let recorded_spec = RecordedSpec {
            spec: set.spec().clone(),
            is_search: search.is_some(),
            configs: set.manifest().plan.configs.unwrap_or(0),
            max_planned,
            planned_with: entry.map(|entry| schema_hash(&entry.schema())),
        };
        let plan = replay_plan(&recorded_spec, model);
        let mut texts = planned_texts(&plan, entry, schema_matches, max_planned);
        let mut store = Self {
            source,
            model_id,
            model_name,
            schema_matches,
            // The four fields compared with the entry are set by `compare_entry` below.
            changed_builds: Vec::new(),
            unidentified_builds: Vec::new(),
            replays_exactly: false,
            code_matches: false,
            recorded: RecordedModel::of_result_set(&set),
            complete: set.is_complete(),
            recorded_execution: Some(SessionExecution::recorded(&set.manifest().execution)),
            folder: set.dir().map(Path::to_path_buf),
            plan,
            recorded_spec: Some(recorded_spec),
            search,
            // The parameters and action labels of the entry are set by `take_declarations` below.
            descriptors: Vec::new(),
            time: entry.and_then(|entry| entry.metadata().time.clone()),
            action_labels: BTreeMap::new(),
            value_columns: set.value_columns().to_vec(),
            stat_columns: set.stat_columns().to_vec(),
            reducer_columns: set.reducer_columns().to_vec(),
            axes: Vec::new(),
            block_axes: BTreeMap::new(),
            configs: BTreeMap::new(),
            runs: Vec::new(),
            positions: BTreeMap::new(),
            unassigned_runs: BTreeMap::new(),
            failed_runs: 0,
            replaced_runs: 0,
            series: SeriesCache::new(series_budget),
            empty_series_runs: BTreeSet::new(),
            revision: 0,
        };
        store.compare_entry(entry);
        store.take_declarations(entry, &set.spec().actions);
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
    /// in marks a run that recorded no series.
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
        let failed = usize::from(outcome.status.is_failure());
        if let Some(&position) = self.positions.get(&run_id) {
            let replaced = std::mem::replace(&mut self.runs[position], outcome);
            self.failed_runs -= usize::from(replaced.status.is_failure());
            self.failed_runs += failed;
            self.replaced_runs += 1;
            return;
        }
        let position = self.runs.len();
        let config_id = outcome.run.config_id;
        self.runs.push(outcome);
        self.positions.insert(run_id, position);
        self.failed_runs += failed;
        let positions = match self.configs.get_mut(&config_id) {
            Some(config) => &mut config.runs,
            None => self.unassigned_runs.entry(config_id).or_default(),
        };
        insert_by_run_id(positions, &self.runs, position);
    }

    /// Search the results come from, `None` for a sweep.
    pub fn search_log(&self) -> Option<&SearchLog> {
        self.search.as_ref()
    }

    /// Returns whether the results come from a search, whose configs are its candidates.
    pub fn is_search(&self) -> bool {
        self.search.is_some()
    }

    /// Returns whether the results come from a sweep or a search.
    pub fn mode(&self) -> ExploreMode {
        if self.is_search() {
            ExploreMode::Search
        } else {
            ExploreMode::Sweep
        }
    }

    /// Marks whether every run of the sweep is held.
    pub fn set_complete(&mut self, complete: bool) {
        self.complete = complete;
        self.revision += 1;
    }

    /// Revision counter of the store, raised on every change.
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
        self.failed_runs
    }

    /// Number of runs replaced by a later run of the same id, as a resumed sweep replaces a run that timed out.
    pub fn replaced_count(&self) -> u64 {
        self.replaced_runs
    }

    pub fn stat_columns(&self) -> &[String] {
        &self.stat_columns
    }

    pub fn reducer_columns(&self) -> &[String] {
        &self.reducer_columns
    }

    /// Returns the name that the views use for reducer column `output`, as in `Infected, max`.
    pub fn output_label(&self, output: usize) -> String {
        output_label(self.reducer_columns.get(output).map_or("", String::as_str))
    }

    /// Folder that holds the sweep's files, `None` for a sweep held in memory or picked files.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }
}

/// Inserts `position` into `positions`, positions in `runs` in order of run id, keeping that order.
fn insert_by_run_id(positions: &mut Vec<usize>, runs: &[RunOutcome], position: usize) {
    let run_id = runs[position].run.run_id;
    let slot = positions.partition_point(|&held| runs[held].run.run_id < run_id);
    positions.insert(slot, position);
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use henad_compute::entry::{ModelEntry, ModelLookupError};
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::measure::SeriesBuffer;
    use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus, StopReason};
    use henad_core::explore::plan::{Config, ModelSchema, Plan};
    use henad_core::explore::search::{Aggregate, CandidateOrigin, Goal, Objective, SearchAlgorithm, SearchSpec};
    use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
    use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
    use henad_explore::search_run::{EvaluatedCandidate, EvaluationReading, SearchPlan, SearchUpdate};
    use henad_models::example_models;

    use super::axes::config_texts;
    use super::replay::shell_word;
    use super::series_cache::series_bytes;
    use super::{
        BandKind, HeatColor, HeatQuery, ResponseQuery, ResultsAxis, ResultsStore, RunsColumn, RunsFilter, RunsSort,
    };
    use crate::state::default_values;
    use crate::tests::support::{ScratchFolder, finished_run, sir};
    use crate::ui::sweep::draft::ExploreMode;

    const STATS: [&str; 3] = ["Susceptible", "Infected", "Recovered"];

    fn values(raw: &[&str]) -> LevelSpec {
        LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
    }

    /// Returns the spec of an SIR factorial over `factors` with `replicates` runs per config.
    fn factorial_spec(factors: Vec<FactorSpec>, replicates: u64) -> SweepSpec {
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
        spec
    }

    /// Returns the plan of an SIR factorial over `factors` with `replicates` runs per config.
    fn plan(entry: &ModelEntry, factors: Vec<FactorSpec>, replicates: u64) -> Arc<Plan> {
        let spec = factorial_spec(factors, replicates);
        Arc::new(spec.plan(&entry.schema()).expect("a valid spec"))
    }

    /// Returns a store for `plan` whose runs record one reducer, `Infected:max`.
    fn store(entry: &ModelEntry, plan: Arc<Plan>, series_budget: usize) -> ResultsStore {
        let mut store = ResultsStore::for_sweep(plan, entry, None, series_budget);
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
            status,
            ticks: (infected.len() as u64).saturating_sub(1) * 10,
            population: 256,
            wall_ms: 2.0,
            reducers: vec![Some(reducer)],
            series,
            ..finished_run(run, plan.run_key(&run))
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

    /// Returns a random search of 4096 evaluations in batches of 16 over `space`, maximising the median of
    /// `Infected:max`.
    fn random_search(space: Vec<FactorSpec>) -> SearchSpec {
        SearchSpec {
            algorithm: SearchAlgorithm::Random,
            max_evaluations: 4096,
            batch_size: 16,
            objective: Some(Objective {
                column: "Infected:max".to_owned(),
                goal: Goal::Maximize,
                aggregate: Aggregate::Median,
            }),
            space,
        }
    }

    /// Returns the evaluation of candidate `candidate_id`, requested in batch `batch`, whose genome decodes to
    /// `config`.
    fn evaluated_candidate(candidate_id: u64, batch: u64, config: Config) -> EvaluatedCandidate {
        EvaluatedCandidate {
            candidate_id,
            batch,
            origin: CandidateOrigin::Random,
            replicate_offset: 0,
            replicates: 1,
            config,
            failed_count: 0,
            reading: EvaluationReading::Objective {
                objective: 1.0,
                pooled_objective: 1.0,
                pooled_replicates: 1,
            },
        }
    }

    /// Returns the update of batch `batch`, which tells the search `evaluated`.
    fn search_update(batch: u64, evaluated: Vec<EvaluatedCandidate>) -> SearchUpdate {
        SearchUpdate {
            batch,
            evaluations: evaluated.len() as u64,
            runs: evaluated.len() as u64,
            evaluated,
            best: None,
            generations: Vec::new(),
            landed_entries: Vec::new(),
            filled_cells: 0,
            pattern_settings: None,
        }
    }

    #[test]
    fn a_search_candidate_lists_its_searched_values_once_its_batch_ends() {
        let sir = sir();
        let schema = sir.schema();
        let mut spec = SweepSpec::new("sir");
        spec.search = Some(random_search(vec![FactorSpec::param(
            "infection_rate",
            LevelSpec::Range {
                min: 0.0,
                max: 1.0,
                step: None,
            },
        )]));
        let search_plan = SearchPlan::new(&spec, &schema).expect("a valid search");
        let mut store = ResultsStore::for_search(Arc::new(search_plan), &sir, None, usize::MAX);
        assert_eq!(store.config_values(0), None, "no batch has ended");

        let mut params = default_values(&sir);
        params[2] = ParamValue::F32(0.25);
        let config = Config {
            block: 0,
            params,
            action_ticks: Vec::new(),
        };
        // The run of a batch arrives before the batch is told.
        store.push_run(search_outcome(0, 0, 1, RunStatus::Ok), false);
        let untold = "Candidate 0 will be known once its batch ends";
        assert_eq!(store.untold_candidate(0).as_deref(), Some(untold));
        assert_eq!(
            store.replay(0),
            Err(untold.to_owned()),
            "a run of a candidate not told yet"
        );
        store.push_search_updates([&search_update(0, vec![evaluated_candidate(0, 0, config)])]);
        assert_eq!(
            store.config_values_text(0).as_deref(),
            Some("Infection Rate 0.25"),
            "one candidate varies no axis, and still names what the search picks"
        );
        assert_eq!(store.untold_candidate(0), None);
        assert_ne!(store.replay(0), Err(untold.to_owned()));
    }

    /// Advances `state` by one xorshift step and returns a draw below `count`.
    fn draw(state: &mut u64, count: u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state % count
    }

    /// Returns a config of `entry` for candidate `candidate_id`, drawn from `state`.
    ///
    /// `gain_resistance_chance` stays at its default, and `initial_outbreak_size` stays there before candidate 2000.
    /// Candidates from 3000 on come from block 1. A search has one block, and the store accepts any block.
    fn drawn_config(entry: &ModelEntry, candidate_id: u64, state: &mut u64) -> Config {
        let params = entry
            .param_descriptors()
            .iter()
            .map(|descriptor| {
                let fixed = descriptor.id() == "gain_resistance_chance"
                    || (descriptor.id() == "initial_outbreak_size" && candidate_id < 2000);
                match descriptor.kind() {
                    _ if fixed => descriptor.kind().default_value(),
                    ParamKind::F32 { min, max, .. } => {
                        ParamValue::F32(min + (max - min) * draw(state, 1000) as f32 / 1000.0)
                    }
                    ParamKind::U32 { min, max, .. } => {
                        ParamValue::U32(min + draw(state, u64::from((max - min).min(2)) + 1) as u32)
                    }
                    ParamKind::Bool { .. } => ParamValue::Bool(draw(state, 2) == 1),
                    ParamKind::Choice { options, .. } => ParamValue::Choice(draw(state, options.len() as u64) as usize),
                }
            })
            .collect();
        Config {
            block: usize::from(candidate_id >= 3000),
            params,
            action_ticks: vec![draw(state, 3) * 10],
        }
    }

    /// Returns replicate `rep` of candidate `candidate_id`, a run of `replicates` per candidate, ended with `status`.
    fn search_outcome(candidate_id: u64, rep: u64, replicates: u64, status: RunStatus) -> RunOutcome {
        let run = PlannedRun {
            run_id: candidate_id * replicates + rep,
            config_id: candidate_id,
            rep,
            seed: rep,
        };
        RunOutcome {
            status,
            ticks: 10,
            reducers: vec![Some(candidate_id as f64)],
            series: SeriesBuffer::new(1),
            ..finished_run(run, 0)
        }
    }

    /// Returns a copy of each axis of `store`, with the level ids that a new axis assigns to its levels.
    fn axis_levels(store: &ResultsStore) -> Vec<ResultsAxis> {
        let axes = store.axes.iter().cloned();
        axes.map(|axis| ResultsAxis::new(axis.column, axis.label, axis.levels, axis.positions, axis.numeric))
            .collect()
    }

    /// Returns the level of each config of `store` on each axis, by config id.
    fn config_levels(store: &ResultsStore) -> Vec<(u64, Vec<Option<usize>>)> {
        let levels = |config| {
            (0..store.axes.len())
                .map(|axis| store.entry_level(config, axis))
                .collect()
        };
        store
            .configs
            .iter()
            .map(|(&config_id, config)| (config_id, levels(config)))
            .collect()
    }

    /// Returns the level of the first config of each block of `store` on each axis, and whether the block varies
    /// each axis, by block index.
    fn block_levels(store: &ResultsStore) -> Vec<(usize, Vec<Option<usize>>, Vec<bool>)> {
        let blocks = store.block_axes.iter();
        blocks
            .map(|(&block, block_axes)| {
                let first_ids = store.axes.iter().zip(&block_axes.first_level_ids);
                let first_levels = first_ids.map(|(axis, &level_id)| axis.level_of_id(level_id)).collect();
                (block, first_levels, block_axes.varies.clone())
            })
            .collect()
    }

    /// Asserts that `store` holds the configs, axes and runs that `expected` holds, at `checkpoint`.
    fn assert_same_configs(store: &ResultsStore, expected: &ResultsStore, checkpoint: &str) {
        assert_eq!(axis_levels(store), axis_levels(expected), "the axes {checkpoint}");
        for axis in &store.axes {
            let places = axis.level_ids.iter().map(|&level_id| axis.id_levels[level_id]);
            assert!(
                places.eq(0..axis.levels.len()),
                "the ids of axis {} {checkpoint}",
                axis.label
            );
        }
        assert_eq!(store.configs.len(), expected.configs.len(), "the configs {checkpoint}");
        for ((config_id, config), expected_config) in store.configs.iter().zip(expected.configs.values()) {
            assert_eq!(
                (config.block, &config.texts, &config.runs),
                (expected_config.block, &expected_config.texts, &expected_config.runs),
                "config {config_id} {checkpoint}"
            );
        }
        assert_eq!(
            config_levels(store),
            config_levels(expected),
            "the levels of each config {checkpoint}"
        );
        assert_eq!(
            block_levels(store),
            block_levels(expected),
            "the levels of each block {checkpoint}"
        );
        assert_eq!(
            store.unassigned_runs, expected.unassigned_runs,
            "the runs without a config {checkpoint}"
        );
        assert_eq!(
            store.failed_count(),
            expected.failed_count(),
            "the failed runs {checkpoint}"
        );
    }

    /// Tells `store` the batches `updates` together. Returns whether they extend the axes in place, and whether they
    /// move the level of a config held.
    ///
    /// Asserts that batches that extend the axes in place leave the level id of every config held alone.
    fn tell_batches(store: &mut ResultsStore, updates: &[SearchUpdate]) -> (bool, bool) {
        let mut added = BTreeMap::new();
        for candidate in updates.iter().flat_map(|update| &update.evaluated) {
            if !store.configs.contains_key(&candidate.candidate_id) {
                let texts = config_texts(&candidate.config, &store.descriptors);
                added.entry(candidate.candidate_id).or_insert(texts);
            }
        }
        let fits_axes = store.fits_axes(&added);
        let held_ids: Vec<Vec<usize>> = store.configs.values().map(|config| config.level_ids.clone()).collect();
        let held_levels = config_levels(store);
        store.push_search_updates(updates);
        if !fits_axes {
            return (false, false);
        }
        let batches: Vec<u64> = updates.iter().map(|update| update.batch).collect();
        let ids = store.configs.values().map(|config| &config.level_ids);
        assert!(
            ids.take(held_ids.len()).eq(&held_ids),
            "batches {batches:?} keep the level ids held"
        );
        let levels = config_levels(store);
        (true, levels[..held_levels.len()] != held_levels[..])
    }

    // Shape of the search that `told_batches` tells.
    const SEARCH_BATCHES: u64 = 300;
    const SEARCH_BATCH_SIZE: u64 = 16;
    const SEARCH_REPLICATES: u64 = 2;

    /// Batch a search tells, with the runs that finish before it.
    struct ToldBatch {
        runs: Vec<RunOutcome>,
        update: SearchUpdate,
    }

    /// Returns Virus on a Network and a random search of it over the virus spread chance, which fires the rewire
    /// action.
    fn virus_search() -> (ModelEntry, Arc<SearchPlan>) {
        let entry = example_models()
            .get("virus_network")
            .cloned()
            .expect("Virus on a Network is registered");
        let mut spec = SweepSpec::new("virus_network");
        spec.actions = vec![ActionSpec::new("rewire", 0)];
        spec.search = Some(random_search(vec![FactorSpec::param(
            "virus_spread_chance",
            LevelSpec::Range {
                min: 0.0,
                max: 1.0,
                step: None,
            },
        )]));
        let search_plan = SearchPlan::new(&spec, &entry.schema()).expect("a valid search");
        (entry, Arc::new(search_plan))
    }

    /// Returns the batches of a search of `entry` over configs [`drawn_config`] draws, [`SEARCH_BATCH_SIZE`]
    /// candidates of [`SEARCH_REPLICATES`] runs each.
    ///
    /// Replicates arrive out of order, and a few fail. Candidate 50 is told in batch 200, after candidates of higher
    /// ids. Each batch also re-evaluates a candidate told before, which adds no config.
    fn told_batches(entry: &ModelEntry) -> Vec<ToldBatch> {
        const LATE_CANDIDATE: u64 = 50;
        const LATE_BATCH: u64 = 200;

        let mut state = 0x9e37_79b9_7f4a_7c15;
        let mut told: Vec<EvaluatedCandidate> = Vec::new();
        let mut late = None;
        let mut batches = Vec::new();
        for batch in 0..SEARCH_BATCHES {
            let mut runs = Vec::new();
            let mut evaluated = Vec::new();
            for candidate_id in batch * SEARCH_BATCH_SIZE..(batch + 1) * SEARCH_BATCH_SIZE {
                for rep in (0..SEARCH_REPLICATES).rev() {
                    let status = if (candidate_id + rep) % 7 == 0 {
                        RunStatus::Panicked
                    } else {
                        RunStatus::Ok
                    };
                    runs.push(search_outcome(candidate_id, rep, SEARCH_REPLICATES, status));
                }
                let candidate = evaluated_candidate(candidate_id, batch, drawn_config(entry, candidate_id, &mut state));
                if candidate_id == LATE_CANDIDATE {
                    late = Some(candidate);
                } else {
                    evaluated.push(candidate);
                }
            }
            if batch == LATE_BATCH {
                evaluated.extend(late.take());
            }
            told.extend(evaluated.iter().cloned());
            if let Some(repeated) = told.get(told.len().saturating_sub(40)) {
                evaluated.push(EvaluatedCandidate {
                    batch,
                    ..repeated.clone()
                });
            }
            let update = search_update(batch, evaluated);
            batches.push(ToldBatch { runs, update });
        }
        batches
    }

    /// Returns a store of `entry` for `search_plan` that holds the runs of `batches`, then `reruns`, and is told every
    /// candidate of `batches` in one batch.
    fn one_pass(
        entry: &ModelEntry,
        search_plan: &Arc<SearchPlan>,
        batches: &[ToldBatch],
        reruns: &[RunOutcome],
    ) -> ResultsStore {
        let mut store = ResultsStore::for_search(Arc::clone(search_plan), entry, None, usize::MAX);
        for outcome in batches.iter().flat_map(|told| &told.runs).chain(reruns) {
            store.push_run(outcome.clone(), false);
        }
        let mut seen = BTreeSet::new();
        let candidates = batches.iter().flat_map(|told| &told.update.evaluated);
        let told = candidates.filter(|candidate| seen.insert(candidate.candidate_id));
        store.push_search_updates([&search_update(0, told.cloned().collect())]);
        store
    }

    #[test]
    fn search_batches_extend_the_store_as_one_pass_builds_it() {
        let (entry, search_plan) = virus_search();
        let batches = told_batches(&entry);
        let mut store = ResultsStore::for_search(Arc::clone(&search_plan), &entry, None, usize::MAX);
        let (mut in_place, mut moved_levels) = (0, 0);
        for (index, told) in batches.iter().enumerate() {
            for outcome in &told.runs {
                store.push_run(outcome.clone(), false);
            }
            let (extended, moved) = tell_batches(&mut store, std::slice::from_ref(&told.update));
            in_place += usize::from(extended);
            moved_levels += usize::from(moved);
            if index % 50 == 49 {
                let expected = one_pass(&entry, &search_plan, &batches[..=index], &[]);
                assert_same_configs(&store, &expected, &format!("after batch {index}"));
            }
        }
        // A resumed search reruns a failed run and a run that ended well.
        let reruns = [(7, 0, RunStatus::Ok), (8, 1, RunStatus::TimedOut)]
            .map(|(candidate_id, rep, status)| search_outcome(candidate_id, rep, SEARCH_REPLICATES, status));
        for outcome in &reruns {
            store.push_run(outcome.clone(), false);
        }
        let expected = one_pass(&entry, &search_plan, &batches, &reruns);
        assert_same_configs(&store, &expected, "after the reruns");

        let batches = SEARCH_BATCHES as usize;
        assert!(
            (batches - 10..=batches - 3).contains(&in_place),
            "{in_place} of {batches} batches extend the axes in place. The first batch, the batch that adds an axis \
             and the batch of the late candidate build the configs again."
        );
        assert!(
            moved_levels > batches / 2,
            "{moved_levels} of {batches} batches move a level held"
        );
        assert!(store.unassigned_runs.is_empty(), "every run has its config");
        let failed = store
            .runs()
            .iter()
            .filter(|outcome| outcome.status.is_failure())
            .count();
        assert_eq!(store.failed_count(), failed, "the failed runs");
    }

    #[test]
    fn search_batches_told_together_extend_the_store_as_batches_told_one_at_a_time() {
        const LARGEST_GROUP: usize = 5;

        let (entry, search_plan) = virus_search();
        let batches = told_batches(&entry);
        let new_store = || ResultsStore::for_search(Arc::clone(&search_plan), &entry, None, usize::MAX);
        let (mut together, mut one_at_a_time) = (new_store(), new_store());
        let (mut start, mut group_size, mut group_count) = (0, 1, 0);
        let (mut joined_groups, mut in_place, mut moved_levels) = (0, 0, 0);
        while start < batches.len() {
            let group = &batches[start..batches.len().min(start + group_size)];
            let end = start + group.len();
            for told in group {
                for outcome in &told.runs {
                    one_at_a_time.push_run(outcome.clone(), false);
                    together.push_run(outcome.clone(), false);
                }
                one_at_a_time.push_search_updates([&told.update]);
            }
            let updates: Vec<SearchUpdate> = group.iter().map(|told| told.update.clone()).collect();
            let (extended, moved) = tell_batches(&mut together, &updates);
            if group.len() > 1 {
                joined_groups += 1;
                in_place += usize::from(extended);
                moved_levels += usize::from(moved);
            }
            let checkpoint = format!("after batches {start} to {}", end - 1);
            assert_same_configs(&together, &one_at_a_time, &checkpoint);
            assert_eq!(
                together.search_log(),
                one_at_a_time.search_log(),
                "the search's course {checkpoint}"
            );
            group_count += 1;
            if group_count % 10 == 0 || end == batches.len() {
                let expected = one_pass(&entry, &search_plan, &batches[..end], &[]);
                assert_same_configs(&together, &expected, &checkpoint);
            }
            start = end;
            group_size = group_size % LARGEST_GROUP + 1;
        }

        assert!(
            (joined_groups - 4..joined_groups).contains(&in_place),
            "{in_place} of {joined_groups} groups of batches extend the axes in place. The group that adds an axis and \
             the group of the late candidate build the configs again."
        );
        assert!(
            moved_levels > joined_groups / 2,
            "{moved_levels} of {joined_groups} groups of batches move a level held"
        );
    }

    #[test]
    fn a_numeric_axis_places_new_levels_among_its_numbers() {
        let texts = |texts: &[&str]| texts.iter().map(|&text| text.to_owned()).collect();
        let mut axis = ResultsAxis::new(0, "Rate".to_owned(), texts(&["1", "2"]), vec![1.0, 2.0], true);
        let (one, two) = (axis.level_id("1"), axis.level_id("2"));
        axis.extend(["3", "2.5", "3", "1"].into_iter());
        assert_eq!(axis.levels, ["1", "2", "2.5", "3"]);
        // A new text of an old value goes after the old level, as a config of a higher id does in one pass.
        axis.extend(["1.0", "0.5", "2", "not a number"].into_iter());
        assert_eq!(axis.levels, ["0.5", "1", "1.0", "2", "2.5", "3"]);
        assert_eq!(axis.positions, [0.5, 1.0, 1.0, 2.0, 2.5, 3.0]);
        assert_eq!(
            (axis.level("1.0"), axis.level("1"), axis.level("4")),
            (Some(2), Some(1), None)
        );
        assert_eq!(
            (axis.level_of_id(one), axis.level_of_id(two)),
            (Some(1), Some(3)),
            "a level keeps its id as levels are added below it"
        );
        let places = axis.level_ids.iter().map(|&level_id| axis.level_of_id(level_id));
        assert!(places.eq((0..6).map(Some)), "each level's id leads back to it");
    }

    #[test]
    fn a_text_axis_adds_new_levels_at_the_end() {
        let texts = |texts: &[&str]| texts.iter().map(|&text| text.to_owned()).collect();
        let mut axis = ResultsAxis::new(0, "Mode".to_owned(), texts(&["b", "a"]), vec![0.0, 1.0], false);
        axis.extend(["c", "a", "0.5", "c"].into_iter());
        assert_eq!(axis.levels, ["b", "a", "c", "0.5"]);
        assert_eq!(axis.positions, [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(axis.level_of_id(axis.level_id("0.5")), Some(3));
    }

    #[test]
    fn the_failed_count_follows_a_replaced_run() {
        let sir = sir();
        let plan = plan(
            &sir,
            vec![FactorSpec::param("infection_rate", values(&["0.1", "0.2"]))],
            2,
        );
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        store.push_run(outcome(&plan, 0, RunStatus::TimedOut, 1.0, &[1.0]), false);
        store.push_run(outcome(&plan, 1, RunStatus::Panicked, 1.0, &[1.0]), false);
        store.push_run(outcome(&plan, 2, RunStatus::Ok, 1.0, &[1.0]), false);
        assert_eq!((store.failed_count(), store.replaced_count()), (2, 0));

        // A resumed sweep reruns the run that timed out.
        store.push_run(outcome(&plan, 0, RunStatus::Ok, 1.0, &[1.0]), false);
        assert_eq!((store.failed_count(), store.replaced_count()), (1, 1));
        store.push_run(outcome(&plan, 2, RunStatus::Panicked, 1.0, &[1.0]), false);
        assert_eq!((store.failed_count(), store.replaced_count()), (2, 2));
        assert_eq!(
            store.row_order(RunsFilter::Failed, &BTreeSet::new(), RunsSort::default()),
            [1, 2]
        );
    }

    /// Returns the parameters of `entry` with a default Recovery Rate of 0.1, as an earlier version of SIR declared.
    fn params_before_change(entry: &ModelEntry) -> Vec<ParamDescriptor> {
        let mut params = entry.param_descriptors().to_vec();
        let recovery = params
            .iter_mut()
            .find(|param| param.id() == "recovery_rate")
            .expect("SIR declares a recovery rate");
        if let ParamKind::F32 { min, max, step, .. } = *recovery.kind() {
            let kind = ParamKind::F32 {
                min,
                max,
                default: 0.1,
                step,
            };
            *recovery = ParamDescriptor::new(recovery.id().to_owned(), recovery.label().to_owned(), kind);
        }
        params
    }

    /// Returns the schema of `entry` with `params` instead of the parameters it declares.
    fn schema_with<'a>(entry: &'a ModelEntry, params: &'a [ParamDescriptor]) -> ModelSchema<'a> {
        ModelSchema {
            params,
            ..entry.schema()
        }
    }

    #[test]
    fn a_sweep_run_replays_after_the_model_changes() {
        let sir = sir();
        let factors = vec![FactorSpec::param("infection_rate", values(&["0.1", "0.2"]))];
        let params = params_before_change(&sir);
        let spec = factorial_spec(factors.clone(), 1);
        let recorded_plan = spec.plan(&schema_with(&sir, &params)).expect("a valid spec");
        let mut store = store(&sir, plan(&sir, factors, 1), usize::MAX);
        // The run used the earlier model, whose declarations give every run a different key.
        store.push_run(outcome(&recorded_plan, 1, RunStatus::Ok, 1.0, &[1.0]), false);
        assert_eq!(
            store.replay(1).err().as_deref(),
            Some("Run 1 does not match the plan of its sweep"),
            "a key that differs under the same model"
        );
        store.schema_matches = false;
        let replay = store.replay(1).expect("the run replays with a warning");
        assert_eq!(replay.params[2], ParamValue::F32(0.2));
    }

    #[test]
    fn a_search_run_replays_after_the_model_changes() {
        let sir = sir();
        let mut spec = SweepSpec::new("sir");
        spec.search = Some(random_search(vec![FactorSpec::param(
            "infection_rate",
            LevelSpec::Range {
                min: 0.0,
                max: 1.0,
                step: None,
            },
        )]));
        let params = params_before_change(&sir);
        let recorded_plan = SearchPlan::new(&spec, &schema_with(&sir, &params)).expect("a valid search");
        let search_plan = SearchPlan::new(&spec, &sir.schema()).expect("a valid search");
        let mut store = ResultsStore::for_search(Arc::new(search_plan), &sir, None, usize::MAX);
        let mut values: Vec<ParamValue> = params.iter().map(|param| param.kind().default_value()).collect();
        values[2] = ParamValue::F32(0.25);
        let config = Config {
            block: 0,
            params: values,
            action_ticks: Vec::new(),
        };
        let mut recorded = search_outcome(0, 0, 1, RunStatus::Ok);
        recorded.run_key = recorded_plan.run_key(&recorded.run, &config);
        store.push_run(recorded, false);
        store.push_search_updates([&search_update(0, vec![evaluated_candidate(0, 0, config.clone())])]);
        let refusal = Some("Run 0 does not match the search it comes from");
        assert_eq!(
            store.replay(0).err().as_deref(),
            refusal,
            "a key that differs under the same model"
        );

        store.schema_matches = false;
        let replay = store.replay(0).expect("the run replays with a warning");
        assert_eq!(
            replay.params, config.params,
            "the replay takes the values the run recorded"
        );
        store.value_columns.swap(2, 3);
        assert_eq!(
            store.replay(0).err().as_deref(),
            refusal,
            "columns that name other parameters"
        );
    }

    #[test]
    fn a_sweep_run_replays_only_with_the_values_of_its_record() {
        let sir = sir();
        let factor = |levels: &[&str]| vec![FactorSpec::param("infection_rate", values(levels))];
        let recorded_plan = plan(&sir, factor(&["0.1", "0.2"]), 1);
        let other_plan = plan(&sir, factor(&["0.1", "0.3"]), 1);
        let mut store = store(&sir, recorded_plan, usize::MAX);
        // Both runs have the ids and seeds that the store's plan assigns to them. Run 1 ran a different infection rate.
        store.push_run(outcome(&other_plan, 0, RunStatus::Ok, 1.0, &[1.0]), false);
        store.push_run(outcome(&other_plan, 1, RunStatus::Ok, 1.0, &[1.0]), false);
        assert!(store.replay(0).is_ok(), "run 0 ran the values of the plan");
        assert_eq!(
            store.replay(1).err().as_deref(),
            Some("Run 1 does not match the plan of its sweep")
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
        let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
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
        let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
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
        let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
        let mut store = store(&sir, Arc::clone(&plan), usize::MAX);
        let mut stopped = outcome(&plan, 1, RunStatus::Ok, 1.0, &[1.0]);
        stopped.ticks = 25;
        stopped.stop_reason = StopReason::Condition;
        store.push_run(stopped, false);

        let seed = plan.run(1).expect("a planned run").seed;
        assert_eq!(
            store.cli_command("henad-cli", 1).expect("the run replays"),
            format!(
                "henad-cli sir --seed {seed} --set grid_width=32 --set infection_rate=0.45 --act seed_outbreak@10 \
                 --warmup 5 --steps 20 --stats-every 5 --export-stats run-1.csv"
            ),
            "the default infection rate is left out, and so is the action past the end"
        );
        assert!(
            store.cli_command("henad-cli", 0).is_err(),
            "run 0 is not in the results"
        );
        assert_eq!(shell_word("network=Small world"), "'network=Small world'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
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
    fn a_replay_warns_on_a_build_from_any_session() {
        use henad_explore::handle::SweepOutput;
        use henad_explore::output::manifest::BuildRole;
        use henad_explore::progress::NoProgress;
        use henad_explore::result_set::ResultSet;
        use henad_explore::sweep::{Provenance, SweepOptions, run_spec};
        use serde_json::Value;

        use crate::ui::results::store::ResultsSource;

        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let mut spec = SweepSpec::new("sir");
        spec.fixed = [("grid_width", "8"), ("grid_height", "8")]
            .map(|(id, value)| (id.to_owned(), value.to_owned()))
            .to_vec();
        spec.run.steps = 3;
        let folder = ScratchFolder::new("builds");
        let mut options = SweepOptions::new(Provenance::new(henad_core::build_info!(), Vec::new()));
        for replicates in [1, 2] {
            spec.run.replicates = replicates;
            let output = SweepOutput::Directory(folder.0.clone());
            run_spec(sir, None, &spec, output, &options, &mut NoProgress).expect("the sweep runs");
            options.resume = true;
        }
        let open = || {
            let set = ResultSet::open_dir(&folder.0, usize::MAX).expect("the folder reads");
            ResultsStore::from_result_set(
                set,
                ResultsSource::Folder(folder.0.clone()),
                models.lookup("sir", None),
                usize::MAX,
            )
        };
        let store = open();
        assert_eq!(store.changed_builds, []);
        assert!(store.replays_exactly);

        // The first of two sessions ran a different engine, and the second ran this one.
        let path = folder.0.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the manifest is written")).expect("JSON");
        manifest["sessions"][0]["engine"]["version"] = Value::from("0.1.0");
        manifest["model"]["replays_exactly"] = Value::from(false);
        std::fs::write(&path, manifest.to_string()).expect("the manifest can be written");
        let store = open();
        assert_eq!(store.changed_builds, [BuildRole::Engine]);
        assert!(
            !store.replays_exactly,
            "the manifest declares the model does not replay exactly"
        );
        assert_eq!(store.unidentified_builds, []);

        // A model crate whose build script stamps nothing records an unidentified build. The store cannot compare
        // it, so the model is listed as unidentified instead of changed.
        for session in manifest["sessions"].as_array_mut().expect("a list of sessions") {
            let model_source = &mut session["model_source"];
            model_source["commit"] = Value::from("");
            model_source["dirty"] = Value::Null;
            model_source["source_hash"] = Value::Null;
        }
        std::fs::write(&path, manifest.to_string()).expect("the manifest can be written");
        let store = open();
        assert_eq!(store.changed_builds, [BuildRole::Engine]);
        assert_eq!(store.unidentified_builds, [BuildRole::Model]);
    }

    /// Results read from files compare their recorded builds and declarations with the entry that replaced the
    /// sweep's model, as Open on a run of other code warns.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_folder_store_compares_its_runs_with_a_replaced_entry() {
        use henad_core::provenance::ModelCode;
        use henad_explore::handle::SweepOutput;
        use henad_explore::output::manifest::BuildRole;
        use henad_explore::progress::NoProgress;
        use henad_explore::result_set::ResultSet;
        use henad_explore::sweep::{Provenance, SweepOptions, run_spec};

        use crate::tests::support::edited_sir;
        use crate::ui::results::store::ResultsSource;

        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let mut spec = SweepSpec::new("sir");
        spec.fixed = [("grid_width", "8"), ("grid_height", "8")]
            .map(|(id, value)| (id.to_owned(), value.to_owned()))
            .to_vec();
        spec.run.steps = 3;
        let options = SweepOptions::new(Provenance::new(henad_core::build_info!(), Vec::new()));
        let record =
            run_spec(sir, None, &spec, SweepOutput::Memory, &options, &mut NoProgress).expect("the sweep runs");
        let files = record.files.expect("a sweep in memory returns its files");
        let named = files
            .entries()
            .into_iter()
            .map(|(name, bytes)| (name.to_owned(), bytes.to_vec()))
            .collect();
        let named: Vec<(String, Vec<u8>)> = named;
        let read = |model: Result<&ModelEntry, ModelLookupError>| {
            let set = ResultSet::from_files(named.clone(), usize::MAX).expect("the files read");
            ResultsStore::from_result_set(set, ResultsSource::Files(Vec::new()), model, usize::MAX)
        };
        let mut store = read(models.lookup("sir", None));
        assert!(store.schema_matches);
        assert_eq!(store.changed_builds, []);
        assert!(store.code_matches);

        let recoded = sir.clone().__with_code(ModelCode::new("henad-app-test", "edited sir"));
        store.model_changed(Ok(&recoded));
        assert!(store.schema_matches);
        assert_eq!(store.changed_builds, [BuildRole::Model]);
        assert!(
            !store.code_matches,
            "a resume refuses runtime code the sweep did not run"
        );
        assert_eq!(
            super::super::code_refusal(Some(&store), ExploreMode::Sweep).as_deref(),
            Some("Model code changed since this sweep ran")
        );

        store.model_changed(Ok(&edited_sir()));
        assert!(!store.schema_matches, "the edit declares other parameters");
        assert!(store.code_matches, "a compiled change is a warning alone");

        let missing = ModelLookupError::NotInSet { id: "sir".to_owned() };
        store.model_changed(Err(&missing));
        assert!(!store.schema_matches);
        assert_eq!(store.changed_builds, [], "no entry to compare with");
        assert!(!store.code_matches);
        assert_eq!(store.replay_refusal(), Some("This build does not include model 'sir'"));

        store.model_changed(Ok(sir));
        assert!(store.schema_matches);
        assert_eq!(store.changed_builds, [], "the sweep's own code again");
        assert!(store.replays_exactly);
        assert!(store.code_matches);
        assert_eq!(super::super::code_refusal(Some(&store), ExploreMode::Sweep), None);
        assert_eq!(store.replay_refusal(), None);
        assert!(store.replay(0).is_ok(), "the run replays under its own code");

        // Files read while the set held other declarations, or no entry, plan their spec again for the sweep's own
        // code. A plan of the other declarations would give the run another key than its row.
        let mut edited = read(Ok(&edited_sir()));
        assert!(!edited.schema_matches);
        edited.model_changed(Ok(sir));
        assert!(edited.schema_matches);
        assert!(edited.replay(0).is_ok(), "the run replays under its own code");
        let mut absent = read(Err(missing));
        assert!(absent.replay_refusal().is_some() && !absent.code_matches);
        absent.model_changed(Ok(sir));
        assert_eq!(absent.replay_refusal(), None);
        assert!(absent.code_matches);
        assert!(absent.replay(0).is_ok(), "the run replays once the model joins the set");
    }

    #[test]
    fn a_model_file_run_copies_no_command() {
        use henad_core::provenance::ModelCode;
        use henad_explore::model_file::ModelFile;

        use super::replay::MODEL_FILE_COMMAND;

        let starter = crate::ui::editor::life_starter();
        assert!(starter.contains("id = \"life_rule\""));
        for id in ["life_rule", "gpu_game_of_life"] {
            let text = starter.replace("id = \"life_rule\"", &format!("id = \"{id}\""));
            let entry = ModelFile::parse(&text)
                .and_then(|file| file.entry())
                .expect("the rule compiles");
            assert_eq!(entry.id(), id);
            let mut spec = SweepSpec::new(id);
            spec.run.steps = 3;
            let plan = Arc::new(spec.plan(&entry.schema()).expect("a valid spec"));
            let store = ResultsStore::for_sweep(plan, &entry, None, usize::MAX);
            assert_eq!(store.command_refusal(), Some(MODEL_FILE_COMMAND), "{id}");
            assert_eq!(
                store.cli_command("henad-cli", 0),
                Err(MODEL_FILE_COMMAND.to_owned()),
                "{id}"
            );
        }
        let sir = sir();
        let recoded = sir
            .clone()
            .__with_code(ModelCode::new("henad-app-test", "a rule under SIR's id"));
        let runtime = store(&recoded, plan(&sir, Vec::new(), 1), usize::MAX);
        assert_eq!(runtime.command_refusal(), Some(MODEL_FILE_COMMAND));
        let compiled = store(&sir, plan(&sir, Vec::new(), 1), usize::MAX);
        assert_eq!(compiled.command_refusal(), None);
    }

    /// A sweep whose manifest records more configs than the bound is never planned. It lists the configs `runs.csv`
    /// holds, and its runs do not replay. Planning a manifest that records a huge design would take gigabytes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_folder_plans_its_sweep_only_within_a_bounded_size() {
        use henad_core::explore::factor::{FactorSpec, LevelSpec};
        use henad_core::explore::spec::BlockSpec;
        use henad_explore::handle::SweepOutput;
        use henad_explore::progress::NoProgress;
        use henad_explore::result_set::ResultSet;
        use henad_explore::sweep::{Provenance, SweepOptions, run_spec};
        use serde_json::Value;

        use super::MAX_PLANNED_CONFIGS;
        use crate::ui::results::store::ResultsSource;

        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let mut spec = SweepSpec::new("sir");
        spec.fixed = [("grid_width", "8"), ("grid_height", "8")]
            .map(|(id, value)| (id.to_owned(), value.to_owned()))
            .to_vec();
        spec.blocks = vec![BlockSpec {
            factors: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Values(["0.1", "0.2", "0.3", "0.4"].map(str::to_owned).to_vec()),
            )],
            ..BlockSpec::default()
        }];
        spec.run.steps = 3;
        let folder = ScratchFolder::new("planned");
        let options = SweepOptions::new(Provenance::new(henad_core::build_info!(), Vec::new()));
        run_spec(
            sir,
            None,
            &spec,
            SweepOutput::Directory(folder.0.clone()),
            &options,
            &mut NoProgress,
        )
        .expect("the sweep runs");
        // A sweep stopped after its first run.
        let runs_path = folder.0.join("runs.csv");
        let runs = std::fs::read_to_string(&runs_path).expect("runs.csv is written");
        let first_run: String = runs.lines().take(2).map(|line| format!("{line}\n")).collect();
        std::fs::write(&runs_path, first_run).expect("runs.csv can be written");

        let open = |max_planned| {
            let set = ResultSet::open_dir(&folder.0, usize::MAX).expect("the folder reads");
            let source = ResultsSource::Folder(folder.0.clone());
            ResultsStore::listing_planned_configs(set, source, models.lookup("sir", None), usize::MAX, max_planned)
        };
        let within = open(4);
        assert_eq!(
            within.config_ids().count(),
            4,
            "a plan within the bound lists every config"
        );
        assert!(within.replay(0).is_ok(), "the run replays");
        let past = open(3);
        assert_eq!(
            past.replay_refusal(),
            Some("Sweep has 4 configurations, too many to replay in the app")
        );
        assert_eq!(past.config_ids().collect::<Vec<_>>(), [0], "the config runs.csv holds");
        assert_eq!(past.runs().len(), 1);

        // A spec that the model rejects. If the store planned it, the reason would differ.
        let path = folder.0.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the manifest is written")).expect("JSON");
        manifest["spec"]["block"][0]["factors"][0]["param"] = Value::from("no_such_parameter");
        manifest["plan"]["configs"] = Value::from(MAX_PLANNED_CONFIGS + 1);
        std::fs::write(&path, manifest.to_string()).expect("the manifest can be written");
        assert!(
            open(MAX_PLANNED_CONFIGS + 1)
                .replay_refusal()
                .is_some_and(|refusal| refusal.contains("refuses this sweep's spec")),
            "a count within the bound plans the spec"
        );
        let set = ResultSet::open_dir(&folder.0, usize::MAX).expect("the folder reads");
        let source = ResultsSource::Folder(folder.0.clone());
        let mut huge = ResultsStore::from_result_set(set, source, models.lookup("sir", None), usize::MAX);
        let refusal = format!(
            "Sweep has {} configurations, too many to replay in the app",
            MAX_PLANNED_CONFIGS + 1
        );
        assert_eq!(huge.replay_refusal(), Some(refusal.as_str()), "the plan is never built");
        assert_eq!(huge.config_ids().collect::<Vec<_>>(), [0]);

        // A browser reads the picked files through the same store.
        let names = ["manifest.json", "runs.csv"];
        let files = names
            .map(|name| {
                (
                    name.to_owned(),
                    std::fs::read(folder.0.join(name)).expect("the file reads"),
                )
            })
            .to_vec();
        let set = ResultSet::from_files(files, usize::MAX).expect("the files read");
        let source = ResultsSource::Files(names.map(str::to_owned).to_vec());
        let picked = ResultsStore::from_result_set(set, source, models.lookup("sir", None), usize::MAX);
        assert_eq!(picked.replay_refusal(), Some(refusal.as_str()));
        assert_eq!(picked.config_ids().collect::<Vec<_>>(), [0]);

        // A resume's run of a config the store does not list. A sweep has no candidates to wait for.
        let mut resumed = huge.runs()[0].clone();
        resumed.run.run_id = 2;
        resumed.run.config_id = 2;
        huge.push_run(resumed, true);
        assert_eq!(huge.untold_candidate(2), None);
        assert_eq!(huge.replay(2), Err(refusal));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_run_opened_from_a_folder_replays_to_its_row() {
        use henad_compute::cpu::sim_thread::SimThread;
        use henad_compute::entry::ModelState;
        use henad_compute::fault::FaultSink;
        use henad_core::explore::stop::StopSpec;
        use henad_core::export::stats_csv::StatColumns;
        use henad_explore::handle::SweepOutput;
        use henad_explore::progress::NoProgress;
        use henad_explore::result_set::ResultSet;
        use henad_explore::sweep::{SweepOptions, run_spec};

        use crate::ui::results::store::ResultsSource;

        let models = example_models();
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
        let folder = ScratchFolder::new("results");
        let options = SweepOptions::new(henad_explore::sweep::Provenance::new(
            henad_core::build_info!(),
            Vec::new(),
        ));
        let sir = models.get("sir").expect("SIR is registered");
        let output = SweepOutput::Directory(folder.0.clone());
        run_spec(sir, None, &spec, output, &options, &mut NoProgress).expect("the sweep runs");

        let set = ResultSet::open_dir(&folder.0, usize::MAX).expect("the folder reads");
        let store = ResultsStore::from_result_set(
            set,
            ResultsSource::Folder(folder.0.clone()),
            models.lookup("sir", None),
            usize::MAX,
        );
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
            let Ok(ModelState::Cpu(state)) = sir.build(&replay.params, Some(replay.seed), None) else {
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

    /// A store whose value columns hold no tick length reads in ticks, as the bands and run lines then draw at ticks.
    #[test]
    fn a_store_without_a_tick_length_column_reads_in_ticks() {
        let entry = crate::tests::support::sir_rates();
        let plan = Arc::new(SweepSpec::new(entry.id()).plan(&entry.schema()).expect("a valid spec"));
        let mut store = ResultsStore::for_sweep(plan, &entry, None, usize::MAX);
        assert_eq!(store.time_unit(), Some(henad_core::metadata::TimeUnit::Day));
        assert!(store.config_time(0).is_some());

        // A folder written before the model declared a time scale records no tick length among its values.
        store.value_columns.retain(|column| column != "tick_length");
        assert_eq!((store.time_unit(), store.config_time(0)), (None, None));
        assert_eq!(crate::ui::time_axis::axis_label(store.time_unit()), "Tick");
    }
}
