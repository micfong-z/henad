//! Searches that run a spec's `[search]` table against a model, a batch of candidates at a time.
//!
//! A search asks its searcher for a batch of candidates, runs each candidate's config on the executor a sweep uses,
//! and tells the searcher the watched reducer values of every run. Run `r` of candidate `c` has run id
//! `c * replicates + r`, config id `c`, replicate index `c`'s replicate offset plus `r`, and the seed the spec's seed
//! scheme gives that replicate. Under common random numbers, replicate `k` of every candidate shares one seed.
//!
//! A search writes `runs.csv`, `series.csv` and `summary.csv` as a sweep does, with the candidate id as the config id,
//! and the tables of [`search_tables`](crate::output::search_tables). A resume replays the search from its seed, and
//! reads back every run the directory holds in place of running it again, a failed or timed-out run included. The
//! resumed search then follows the trajectory of the interrupted one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use web_time::Instant;

use henad_core::action::Schedule;
use henad_core::explore::factor::FactorSpec;
use henad_core::explore::fingerprint::{run_key, search_hash};
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::{PlannedRun, RunOutcome};
use henad_core::explore::plan::{Config, MAX_RUNS, ModelSchema, Plan, PlanError, Shard};
use henad_core::explore::replay::Replay;
use henad_core::explore::search::genome::{Genome, SearchSpace, SearchSpaceError};
use henad_core::explore::search::pse::{
    ArchiveEntry, PatternAxis, PatternCell, PatternPlacement, PatternSpaceSettings,
};
use henad_core::explore::search::{
    Candidate, CandidateOrigin, Evaluation, GenerationSummary, Objective, RankingEntry, SearchAlgorithm, SearchReport,
    SearchSpec, SearchSpecError, Searcher,
};
use henad_core::explore::seed::search_seed;
use henad_core::explore::spec::SweepSpec;

use crate::exec::{BatchEnd, RunRequest};
#[cfg(not(target_arch = "wasm32"))]
use crate::exec::{Executor, RunSink};
use crate::output::manifest::{
    Manifest, ManifestAxisRanges, ManifestMode, ManifestPlan, ManifestSearch, ResultCounts, now_unix_ms,
};
use crate::output::memory::{SweepFiles, memory_writer};
use crate::output::read::{ReadError, RunsCsv, SeriesScan, parse_one};
use crate::output::resume::{ResumeError, check_model};
use crate::output::runs_csv::{ID_COLUMNS, OUTCOME_COLUMNS, column_names};
use crate::output::search_tables::{ConfigColumns, SearchTablesWriter, write_archive, write_ranking};
use crate::output::{
    ARCHIVE_FILE, BATCHES_FILE, BEST_FILE, EVALUATIONS_FILE, GENERATIONS_FILE, MANIFEST_FILE, OutputDir, OutputError,
    OutputWriter, RUNS_FILE, SERIES_FILE, runs_csv, series_csv, table_paths,
};
use crate::probe::{ProbeReport, TimedProbe, check_capacity};
#[cfg(not(target_arch = "wasm32"))]
use crate::progress::ProgressMeter;
use crate::progress::{Progress, ProgressEvent};
use crate::sweep::{
    ExploreError, ManifestParts, SweepEnd, SweepInputs, SweepOutline, SweepRecord, SweepReport, SweepWarning,
    build_warnings, finish_manifest, hex, running_manifest, sized_layout,
};

/// A search spec checked against a model, with its space resolved.
#[derive(Debug, Clone)]
pub struct SearchPlan {
    /// Plan of the spec's fixed values and actions alone. Every candidate's config starts from its one config.
    base: Arc<Plan>,
    search: SearchSpec,
    space: SearchSpace,
    search_hash: u64,
    run_count: u64,
}

impl SearchPlan {
    /// Checks the search of `spec` against `schema`, and resolves its space.
    ///
    /// # Errors
    ///
    /// Returns [`SearchPlanError`] for a spec with no search or with blocks, search settings or a space the search
    /// refuses, fixed values or actions the model refuses, a batch of more than [`MAX_RUNS`] runs, or a budget whose
    /// runs overflow a 64-bit count.
    pub fn new(spec: &SweepSpec, schema: &ModelSchema<'_>) -> Result<Self, SearchPlanError> {
        let search = spec.search.as_ref().ok_or(SearchPlanError::NotASearch)?;
        if !spec.blocks.is_empty() {
            return Err(SearchPlanError::Blocks);
        }
        search.check().map_err(SearchPlanError::Settings)?;
        let base = spec.plan(schema).map_err(SearchPlanError::Plan)?;
        let space = SearchSpace::resolve(&search.space, schema.params, &spec.actions, &spec.fixed)
            .map_err(SearchPlanError::Space)?;
        if (search.batch_size as u64).saturating_mul(base.replicates()) > MAX_RUNS {
            return Err(SearchPlanError::BatchTooLarge {
                batch_size: search.batch_size,
                replicates: base.replicates(),
            });
        }
        let run_count = search
            .max_evaluations
            .checked_mul(base.replicates())
            .ok_or(SearchPlanError::TooManyRuns)?;
        Ok(Self {
            search_hash: search_hash(base.plan_hash(), base.replicates(), search),
            base: Arc::new(base),
            search: search.clone(),
            space,
            run_count,
        })
    }

    /// Plan of the spec's fixed values and actions, whose one config every candidate starts from.
    pub fn base(&self) -> &Arc<Plan> {
        &self.base
    }

    pub fn search(&self) -> &SearchSpec {
        &self.search
    }

    pub fn space(&self) -> &SearchSpace {
        &self.space
    }

    /// Hash of every setting that fixes the search's trajectory, from [`search_hash`].
    pub fn search_hash(&self) -> u64 {
        self.search_hash
    }

    /// Runs the whole budget takes, every evaluation times the replicates.
    pub fn run_count(&self) -> u64 {
        self.run_count
    }

    pub fn replicates(&self) -> u64 {
        self.base.replicates()
    }

    /// Returns the config `genome` decodes to, over the spec's fixed values and actions.
    ///
    /// # Panics
    ///
    /// Panics when `genome` has a gene count other than the space's.
    pub fn config(&self, genome: &Genome) -> Config {
        let base = self.base.config(0).expect("a plan with no blocks has one config");
        self.space.decode(genome, base)
    }

    /// Returns run `index` of `candidate`, counting from 0.
    ///
    /// Under independent seeds, a re-evaluation draws from the seeds of the candidate it repeats.
    pub fn run(&self, candidate: &Candidate, index: u64) -> PlannedRun {
        let replicates = self.replicates();
        let replicate = candidate.replicate_offset + index;
        let seeds = self.base.seed_settings();
        let seed_config_id = candidate.origin.reevaluated_id().unwrap_or(candidate.id);
        PlannedRun {
            run_id: candidate.id * replicates + index,
            config_id: candidate.id,
            rep: replicate,
            seed: seeds.scheme.seed(seeds.root, seed_config_id, replicate),
        }
    }

    /// Returns the key naming the results of `run`, a run of a candidate whose config is `config`.
    pub fn run_key(&self, run: &PlannedRun, config: &Config) -> u64 {
        run_key(
            self.base.results_fingerprint(),
            &config.params,
            &config.action_ticks,
            run.seed,
        )
    }

    /// Returns the [`Replay`] of `run`, a run of a candidate whose config is `config`.
    pub fn replay(&self, run: &PlannedRun, config: &Config) -> Replay {
        let settings = self.base.run_settings();
        Replay {
            model: self.base.model().to_owned(),
            params: config.params.clone(),
            seed: run.seed,
            schedule: self.base.schedule(config),
            ticks: settings.warmup + settings.steps,
            label: format!(
                "Search run {}: candidate {}, replicate {}",
                run.run_id, run.config_id, run.rep
            ),
        }
    }

    /// Returns the position of each watched column among the reducer columns of `measure`.
    ///
    /// # Errors
    ///
    /// Returns [`SearchPlanError::UnknownColumn`] for a watched column no reducer writes.
    pub fn watched_reducers(&self, measure: &MeasurePlan) -> Result<Vec<usize>, SearchPlanError> {
        let names = measure.reducers().names();
        self.search
            .watched_columns()
            .into_iter()
            .map(|column| {
                names
                    .iter()
                    .position(|name| name == column)
                    .ok_or_else(|| SearchPlanError::UnknownColumn {
                        column: column.to_owned(),
                        known: names.to_vec(),
                    })
            })
            .collect()
    }

    /// Returns the budget, objective and space of the search.
    pub fn outline(&self) -> SearchOutline {
        SearchOutline {
            algorithm: self.search.algorithm.as_str(),
            max_evaluations: self.search.max_evaluations,
            batch_size: self.search.batch_size,
            objective: self.search.objective.clone(),
            watched_columns: self.search.watched_columns().into_iter().map(str::to_owned).collect(),
            pattern_axes: match &self.search.algorithm {
                SearchAlgorithm::PatternSpaceExploration(settings) => {
                    Some([settings.x_axis.clone(), settings.y_axis.clone()])
                }
                SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => None,
            },
            space: self.search.space.clone(),
            search_seed: search_seed(self.base.seed_settings().root),
        }
    }
}

/// A search spec that cannot run.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchPlanError {
    /// A spec with no `[search]` table, handed to a search.
    NotASearch,
    /// A search spec that lists blocks as well.
    Blocks,
    /// Fixed values or actions the model refuses, for the reason inside.
    Plan(PlanError),
    /// Search settings refused for the reason inside.
    Settings(SearchSpecError),
    /// A search space refused for the reason inside.
    Space(SearchSpaceError),
    /// A watched column no reducer writes. `known` lists the reducer columns.
    UnknownColumn { column: String, known: Vec<String> },
    /// A batch of `batch_size` candidates at `replicates` runs each, more than [`MAX_RUNS`] runs in all.
    BatchTooLarge { batch_size: usize, replicates: u64 },
    /// More runs than a 64-bit count holds.
    TooManyRuns,
    /// A search run as one shard of several.
    Sharded,
    /// A resume asked to run a search's failed runs again.
    RetryFailed,
}

impl fmt::Display for SearchPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotASearch => f.write_str("the spec has no [search] table"),
            Self::Blocks => f.write_str("a search selects its own configs and takes no blocks"),
            Self::Plan(_) => f.write_str("cannot plan the search's fixed values and actions"),
            Self::Settings(_) => f.write_str("search settings"),
            Self::Space(_) => f.write_str("search space"),
            Self::UnknownColumn { column, known } if known.is_empty() => {
                write!(f, "search watches '{column}', and the runs have no reducer columns")
            }
            Self::UnknownColumn { column, known } => write!(
                f,
                "no reducer writes '{column}', the column the search watches. The reducer columns are {}",
                known.join(", ")
            ),
            Self::BatchTooLarge { batch_size, replicates } => write!(
                f,
                "a batch of {batch_size} candidates at {replicates} replicates each has more than {MAX_RUNS} runs"
            ),
            Self::TooManyRuns => f.write_str("search budget has more runs than a 64-bit integer can hold"),
            Self::Sharded => f.write_str("a search cannot run as a shard"),
            Self::RetryFailed => {
                f.write_str("a resumed search replays its failed runs as they were, and cannot run them again")
            }
        }
    }
}

impl std::error::Error for SearchPlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Settings(error) => Some(error),
            Self::Space(error) => Some(error),
            Self::NotASearch
            | Self::Blocks
            | Self::UnknownColumn { .. }
            | Self::BatchTooLarge { .. }
            | Self::TooManyRuns
            | Self::Sharded
            | Self::RetryFailed => None,
        }
    }
}

/// Budget, objective and space of a planned search.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOutline {
    /// Name a spec file gives the algorithm, as in `genetic`.
    pub algorithm: &'static str,
    pub max_evaluations: u64,
    pub batch_size: usize,
    /// Objective of a random search, hill climb or genetic algorithm, `None` for a Pattern Space Exploration.
    pub objective: Option<Objective>,
    /// Reducer columns each run reports to the searcher.
    pub watched_columns: Vec<String>,
    /// Axes of a Pattern Space Exploration as the spec gives them, the x axis first. `None` for any other search.
    pub pattern_axes: Option<[PatternAxis; 2]>,
    /// Factors of the space, in gene order.
    pub space: Vec<FactorSpec>,
    /// Seed the searcher draws from.
    pub search_seed: u64,
}

/// Standing of a search after it was told the evaluations of one batch.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchUpdate {
    /// Index of the batch, counting from 0.
    pub batch: u64,
    /// Evaluations told so far, the batch's included.
    pub evaluations: u64,
    /// Runs of those evaluations.
    pub runs: u64,
    /// Evaluations of the batch, in candidate order.
    pub evaluated: Vec<EvaluatedCandidate>,
    /// Best candidate after the batch, `None` for a Pattern Space Exploration.
    pub best: Option<RankingEntry>,
    /// Generations of a genetic algorithm that finished with the batch, in order.
    pub generations: Vec<GenerationSummary>,
    /// Archive entries of the cells of a Pattern Space Exploration that the batch landed in, as they stand after it,
    /// in cell order.
    pub landed_entries: Vec<ArchiveEntry>,
    /// Cells the archive fills after the batch, 0 for any other search.
    pub filled_cells: u64,
    /// Settings a Pattern Space Exploration places outputs with, each axis with its range. `None` for any other
    /// search, or while an automatic range waits for the initial samples.
    pub pattern_settings: Option<PatternSpaceSettings>,
}

/// One evaluation a search was told.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedCandidate {
    pub candidate_id: u64,
    /// Index of the batch that asked for the candidate.
    pub batch: u64,
    pub origin: CandidateOrigin,
    /// Replicate index of the evaluation's first run.
    pub replicate_offset: u64,
    /// Runs of the evaluation.
    pub replicates: u64,
    /// Config the candidate's genome decodes to.
    pub config: Config,
    /// Replicates with a watched value that is missing or not finite.
    pub failed_count: u64,
    pub reading: EvaluationReading,
}

/// Outcome of one evaluation, as its search reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvaluationReading {
    /// Objective of a random search, hill climb or genetic algorithm.
    Objective {
        /// Objective over the evaluation's own replicates.
        objective: f64,
        /// Objective over every replicate the candidate has so far. A re-evaluation reports the candidate it
        /// repeats.
        pooled_objective: f64,
        /// Replicates behind `pooled_objective`.
        pooled_replicates: u64,
    },
    /// Cell of a Pattern Space Exploration.
    Placement {
        /// Outputs of the evaluation and their cell, `None` when an axis has no finite value. The placement has no
        /// cell while an automatic range waits for the initial samples.
        placement: Option<PatternPlacement>,
        /// Whether the evaluation is the first to land in its cell, the cell's exemplar.
        new_cell: bool,
    },
}

/// A search running a batch at a time, and the bookkeeping its tables and events need.
pub(crate) struct SearchSession {
    plan: Arc<SearchPlan>,
    searcher: Box<dyn Searcher + Send>,
    /// Position of each watched column among the reducer columns.
    watched_reducers: Vec<usize>,
    /// Runs a resumed directory holds, by run id from 0.
    recorded_runs: Arc<[RecordedRun]>,
    /// Config of every candidate asked for its first evaluation, by candidate id.
    configs: BTreeMap<u64, Config>,
    batch_count: u64,
    evaluations: u64,
    runs: u64,
    /// Generations of a genetic algorithm reported so far.
    reported_generations: usize,
    /// Whether a Pattern Space Exploration has reported the range of each axis.
    reported_ranges: bool,
}

/// Candidates of one batch and their runs, in run order.
pub(crate) struct AskedBatch {
    index: u64,
    candidates: Vec<Candidate>,
    /// Config of each candidate.
    configs: Vec<Config>,
    /// Actions of each candidate's config, in the order they fire.
    schedules: Vec<Schedule>,
    runs: Vec<BatchRun>,
    /// Watched values of the runs a resumed directory holds, a prefix of `runs`.
    recorded_values: Vec<Vec<Option<f64>>>,
}

/// One run of a batch.
struct BatchRun {
    run: PlannedRun,
    run_key: u64,
    /// Position of the run's candidate in the batch.
    candidate_index: usize,
}

impl AskedBatch {
    /// Returns the requests of the runs left to run, in run order.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn requests(&self) -> Vec<RunRequest<'_>> {
        self.runs[self.recorded_values.len()..]
            .iter()
            .map(|batch_run| self.request_of(batch_run))
            .collect()
    }

    /// Returns the request of the run at `position` among the runs left to run, or `None` past the last.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn request(&self, position: usize) -> Option<RunRequest<'_>> {
        let batch_run = self.runs.get(self.recorded_values.len() + position)?;
        Some(self.request_of(batch_run))
    }

    fn request_of(&self, batch_run: &BatchRun) -> RunRequest<'_> {
        RunRequest {
            run: batch_run.run,
            run_key: batch_run.run_key,
            params: &self.configs[batch_run.candidate_index].params,
            schedule: self.schedules[batch_run.candidate_index].clone(),
        }
    }

    /// Returns the config of `run`, a run of the batch.
    ///
    /// # Panics
    ///
    /// Panics when no candidate of the batch has the run's config id.
    pub(crate) fn config_of(&self, run: &PlannedRun) -> &Config {
        let index = self
            .candidates
            .binary_search_by_key(&run.config_id, |candidate| candidate.id)
            .expect("a run of the batch belongs to one of its candidates");
        &self.configs[index]
    }
}

impl SearchSession {
    /// Returns a session of `plan` from its first batch, reading the runs `recorded_runs` holds in place of running
    /// them.
    ///
    /// # Errors
    ///
    /// Returns [`SearchPlanError::Settings`] when the searcher refuses the plan's settings.
    pub(crate) fn new(
        plan: Arc<SearchPlan>,
        watched_reducers: Vec<usize>,
        recorded_runs: Arc<[RecordedRun]>,
    ) -> Result<Self, SearchPlanError> {
        let root = plan.base.seed_settings().root;
        let searcher = plan
            .search
            .searcher(&plan.space, root)
            .map_err(SearchPlanError::Settings)?;
        Ok(Self {
            plan,
            searcher,
            watched_reducers,
            recorded_runs,
            configs: BTreeMap::new(),
            batch_count: 0,
            evaluations: 0,
            runs: 0,
            reported_generations: 0,
            reported_ranges: false,
        })
    }

    /// Position of each watched column among the reducer columns.
    pub(crate) fn watched_reducers(&self) -> &[usize] {
        &self.watched_reducers
    }

    /// Asks for the next batch, or returns `None` once the search is done.
    ///
    /// The runs a resumed directory holds come first, with their values read back.
    ///
    /// # Errors
    ///
    /// Returns [`ResumeError::SearchRunChanged`] for a held run whose key is not the key of the run asked for.
    pub(crate) fn ask(&mut self) -> Result<Option<AskedBatch>, ResumeError> {
        if self.searcher.is_done() {
            return Ok(None);
        }
        let candidates = self.searcher.ask(self.plan.search.batch_size);
        if candidates.is_empty() {
            return Ok(None);
        }
        let replicates = self.plan.replicates();
        let mut batch = AskedBatch {
            index: self.batch_count,
            configs: Vec::with_capacity(candidates.len()),
            schedules: Vec::with_capacity(candidates.len()),
            runs: Vec::with_capacity(candidates.len() * replicates as usize),
            recorded_values: Vec::new(),
            candidates: Vec::new(),
        };
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let config = self.plan.config(&candidate.genome);
            for index in 0..replicates {
                let run = self.plan.run(candidate, index);
                batch.runs.push(BatchRun {
                    run,
                    run_key: self.plan.run_key(&run, &config),
                    candidate_index,
                });
            }
            if candidate.origin.reevaluated_id().is_none() {
                self.configs.insert(candidate.id, config.clone());
            }
            batch.schedules.push(self.plan.base.schedule(&config));
            batch.configs.push(config);
        }
        for batch_run in &batch.runs {
            let Some(recorded) = usize::try_from(batch_run.run.run_id)
                .ok()
                .and_then(|run_id| self.recorded_runs.get(run_id))
            else {
                break;
            };
            if recorded.run_key != batch_run.run_key {
                return Err(ResumeError::SearchRunChanged {
                    run_id: batch_run.run.run_id,
                });
            }
            batch.recorded_values.push(recorded.values.clone());
        }
        batch.candidates = candidates;
        Ok(Some(batch))
    }

    /// Tells the searcher every batch whose runs a resumed directory holds in full, without running anything.
    ///
    /// The runs held of the batch after them are checked too, as [`Self::ask`] checks every run.
    ///
    /// # Errors
    ///
    /// Returns [`ResumeError::SearchRunChanged`] for a held run whose key is not the key of the run asked for.
    pub(crate) fn replay_recorded_runs(&mut self) -> Result<(), ResumeError> {
        while let Some(batch) = self.ask()? {
            if batch.recorded_values.len() < batch.runs.len() {
                break;
            }
            self.tell(batch, Vec::new());
        }
        Ok(())
    }

    /// Tells the searcher the evaluations of `batch`, whose runs gave `values` after the values it read back, and
    /// returns the standing after it.
    ///
    /// # Panics
    ///
    /// Panics when `values` does not hold one entry per run the batch ran.
    pub(crate) fn tell(&mut self, batch: AskedBatch, values: Vec<Vec<Option<f64>>>) -> SearchUpdate {
        let AskedBatch {
            index,
            candidates,
            configs,
            runs,
            mut recorded_values,
            ..
        } = batch;
        recorded_values.extend(values);
        let values = recorded_values;
        assert_eq!(values.len(), runs.len(), "every run of the batch has its values");
        let replicates = self.plan.replicates() as usize;
        let evaluations: Vec<Evaluation> = candidates
            .iter()
            .zip(values.chunks(replicates.max(1)))
            .map(|(candidate, outputs)| Evaluation {
                candidate_id: candidate.id,
                outputs: outputs.to_vec(),
            })
            .collect();
        self.searcher.tell(&evaluations);
        let searcher = &self.searcher;
        let pattern_settings = match &self.plan.search.algorithm {
            SearchAlgorithm::PatternSpaceExploration(settings) => Some(searcher.pattern_settings().unwrap_or(settings)),
            SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => None,
        };
        let mut touched = BTreeSet::new();
        let evaluated: Vec<EvaluatedCandidate> = candidates
            .into_iter()
            .zip(configs)
            .zip(&evaluations)
            .map(|((candidate, config), evaluation)| {
                let failed_count = evaluation
                    .outputs
                    .iter()
                    .filter(|row| row.iter().any(|value| !value.is_some_and(f64::is_finite)))
                    .count() as u64;
                let reading = if let Some(settings) = pattern_settings {
                    let placement = settings.place(evaluation);
                    let cell = placement.and_then(|placement| placement.cell);
                    if let Some(cell) = cell {
                        touched.insert(cell);
                    }
                    let new_cell = cell.is_some_and(|cell| {
                        searcher
                            .archive_entry(cell)
                            .is_some_and(|entry| entry.candidate_id == candidate.id)
                    });
                    EvaluationReading::Placement { placement, new_cell }
                } else {
                    let objective = self
                        .plan
                        .search
                        .objective
                        .as_ref()
                        .expect("a scored search has an objective");
                    let pooled_id = candidate.origin.reevaluated_id().unwrap_or(candidate.id);
                    let pooled = searcher.ranking_entry(pooled_id);
                    EvaluationReading::Objective {
                        objective: objective.score(evaluation),
                        pooled_objective: pooled.as_ref().map_or(objective.goal.worst(), |entry| entry.objective),
                        pooled_replicates: pooled.as_ref().map_or(0, |entry| entry.replicate_count),
                    }
                };
                EvaluatedCandidate {
                    candidate_id: candidate.id,
                    batch: index,
                    origin: candidate.origin,
                    replicate_offset: candidate.replicate_offset,
                    replicates: replicates as u64,
                    config,
                    failed_count,
                    reading,
                }
            })
            .collect();
        self.batch_count += 1;
        self.evaluations += evaluated.len() as u64;
        self.runs += runs.len() as u64;
        let generations = searcher.generations();
        let new_generations = generations[self.reported_generations.min(generations.len())..].to_vec();
        self.reported_generations = generations.len();
        let (best, filled_cells) = (searcher.best(), searcher.filled_cells());
        let pattern_settings = searcher.pattern_settings().cloned();
        SearchUpdate {
            batch: index,
            evaluations: self.evaluations,
            runs: self.runs,
            evaluated,
            best,
            generations: new_generations,
            landed_entries: self.landed_entries(touched),
            filled_cells,
            pattern_settings,
        }
    }

    /// Returns the archive entries of `touched`, the cells the last batch landed in, as they stand after it.
    ///
    /// The batch that fixes an automatic range files the evaluations held before it as well, and gets the whole
    /// archive. With both bounds given, the first batch's cells are the whole archive.
    fn landed_entries(&mut self, touched: BTreeSet<PatternCell>) -> Vec<ArchiveEntry> {
        if !self.reported_ranges && self.searcher.pattern_settings().is_some() {
            self.reported_ranges = true;
            return self.searcher.report().archive;
        }
        touched
            .into_iter()
            .filter_map(|cell| self.searcher.archive_entry(cell))
            .collect()
    }

    /// Returns every candidate the searcher ranks, or the whole archive, as the search stands.
    pub(crate) fn report(&self) -> SearchReport {
        self.searcher.report()
    }

    /// Returns the record of the search the manifest holds, as it stands.
    pub(crate) fn manifest_search(&self) -> ManifestSearch {
        let mut record = manifest_search(&self.plan);
        record.evaluations = self.evaluations;
        record.batch_count = self.batch_count;
        match &self.plan.search.algorithm {
            SearchAlgorithm::PatternSpaceExploration(_) => {
                record.filled_cells = Some(self.searcher.filled_cells());
                record.axis_ranges = self.searcher.pattern_settings().and_then(ManifestAxisRanges::of);
            }
            SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => {
                let best = self.searcher.best();
                record.best_candidate_id = best.as_ref().map(|entry| entry.candidate_id);
                record.best_objective = best
                    .map(|entry| entry.objective)
                    .filter(|objective| objective.is_finite());
            }
        }
        record
    }

    /// Writes the search's closing table from `report` to `dest`, `best.csv` or `archive.csv`, and hands `dest` back.
    ///
    /// # Errors
    ///
    /// Returns the error of a write.
    pub(crate) fn write_closing_table<W: Write>(
        &self,
        report: &SearchReport,
        dest: W,
        columns: &ConfigColumns,
    ) -> io::Result<W> {
        match &self.plan.search.algorithm {
            SearchAlgorithm::PatternSpaceExploration(settings) => {
                let settings = self.searcher.pattern_settings().unwrap_or(settings);
                write_archive(dest, settings, &report.archive, &self.configs, columns)
            }
            SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => {
                write_ranking(dest, &report.ranking, &self.configs, columns)
            }
        }
    }
}

/// Returns the record of a search of `plan` before its first batch.
fn manifest_search(plan: &SearchPlan) -> ManifestSearch {
    let pattern_settings = match &plan.search.algorithm {
        SearchAlgorithm::PatternSpaceExploration(settings) => Some(settings),
        SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => None,
    };
    ManifestSearch {
        algorithm: plan.search.algorithm.as_str().to_owned(),
        search_hash: hex(plan.search_hash),
        max_evaluations: plan.search.max_evaluations,
        batch_size: plan.search.batch_size,
        search_seed: search_seed(plan.base.seed_settings().root),
        watched_columns: plan.search.watched_columns().into_iter().map(str::to_owned).collect(),
        evaluations: 0,
        batch_count: 0,
        best_candidate_id: None,
        best_objective: None,
        filled_cells: pattern_settings.map(|_| 0),
        axis_ranges: pattern_settings.and_then(ManifestAxisRanges::of),
    }
}

/// Returns the name of the closing table a search of `plan` writes, `best.csv` or `archive.csv`.
fn closing_table_file(plan: &SearchPlan) -> &'static str {
    match plan.search.algorithm {
        SearchAlgorithm::PatternSpaceExploration(_) => ARCHIVE_FILE,
        SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => BEST_FILE,
    }
}

/// Returns whether a search of `plan` writes `generations.csv`.
fn writes_generations(plan: &SearchPlan) -> bool {
    matches!(plan.search.algorithm, SearchAlgorithm::Genetic(_))
}

/// Returns the values `outcome` reports for the reducers at positions `watched_reducers`, each `None` for a run that
/// failed.
///
/// A value that is not finite reads as `None`, as it does once written to `runs.csv` and read back.
pub(crate) fn watched_values(outcome: &RunOutcome, watched_reducers: &[usize]) -> Vec<Option<f64>> {
    watched_reducers
        .iter()
        .map(|&position| {
            if outcome.status.is_failure() {
                None
            } else {
                outcome
                    .reducers
                    .get(position)
                    .copied()
                    .flatten()
                    .filter(|value| value.is_finite())
            }
        })
        .collect()
}

/// One run a resumed directory holds, as a search reads it back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordedRun {
    run_key: u64,
    /// Values of the watched reducer columns, each `None` for a failed run or an empty cell.
    values: Vec<Option<f64>>,
}

/// Runs a directory holds for a resumed search, from run 0 in order, and the repair its tables need.
#[derive(Debug)]
#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "a browser resumes no folder"))]
pub(crate) struct RecordedSearch {
    /// Manifest the directory holds.
    pub(crate) recorded: Manifest,
    /// Runs the directory holds, by run id from 0.
    runs: Arc<[RecordedRun]>,
    counts: ResultCounts,
    /// Bytes `runs.csv` keeps, cutting a partial last record.
    runs_bytes: u64,
    /// Bytes `series.csv` keeps, cutting a partial last line and the rows of runs `runs.csv` never recorded.
    series_bytes: u64,
    /// Whether a table lacks its header, so both are written afresh and no run is kept.
    fresh_tables: bool,
}

impl RecordedSearch {
    /// Reads the directory at `path` for a resume of a search of `plan`, without changing it.
    ///
    /// `runs_header` and `series_header` are the header lines the search writes to `runs.csv` and `series.csv`,
    /// `watched_reducers` the position of each watched column among the reducer columns, and `config_column_count`
    /// the number of parameter and action columns.
    ///
    /// # Errors
    ///
    /// Returns [`ResumeError`] when the manifest or a table cannot be read, the directory holds a sweep, another
    /// search, another model, another model schema or another column layout, or `runs.csv` lists its runs out of
    /// order or more runs than the search's budget.
    pub(crate) fn read(
        path: &Path,
        plan: &SearchPlan,
        runs_header: &str,
        series_header: &str,
        watched_reducers: &[usize],
        config_column_count: usize,
    ) -> Result<Self, ResumeError> {
        let recorded = Manifest::read(&path.join(MANIFEST_FILE)).map_err(ResumeError::Manifest)?;
        if recorded.mode != ManifestMode::Search {
            return Err(ResumeError::ModeChanged {
                recorded: recorded.mode,
                current: ManifestMode::Search,
            });
        }
        check_model(&recorded, plan.base.model())?;
        let current_schema = hex(plan.base.schema_hash());
        if recorded.model.schema_hash != current_schema {
            return Err(ResumeError::SchemaChanged {
                recorded: recorded.model.schema_hash.clone(),
                current: current_schema,
            });
        }
        let current_search = hex(plan.search_hash);
        let recorded_search = recorded
            .search
            .as_ref()
            .map_or_else(String::new, |search| search.search_hash.clone());
        if recorded_search != current_search {
            return Err(ResumeError::SearchChanged {
                recorded: recorded_search,
                current: current_search,
            });
        }
        let (runs_path, series_path) = table_paths(path);
        let table = RunsCsv::read(&runs_path).map_err(ResumeError::Table)?;
        if let Some(header) = &table.header
            && runs_csv::header_line(header) != runs_header
        {
            return Err(ResumeError::ColumnsChanged { file: RUNS_FILE });
        }
        let reducers_start = ID_COLUMNS.len() + config_column_count + OUTCOME_COLUMNS.len();
        let mut runs = Vec::with_capacity(table.records.len());
        let mut counts = ResultCounts::default();
        for (position, record) in table.records.iter().enumerate() {
            if position as u64 >= plan.run_count() {
                return Err(ResumeError::UnknownRun { run_id: record.run_id });
            }
            if record.run_id != position as u64 {
                return Err(ResumeError::SearchRunChanged { run_id: record.run_id });
            }
            let fields =
                parse_one(&record.text, 0..record.text.len(), &runs_path, position + 2).map_err(ResumeError::Table)?;
            let values = watched_reducers
                .iter()
                .map(|&reducer| {
                    let column = reducers_start + reducer;
                    let field = fields.get(column).map_or("", String::as_str);
                    if record.status.is_failure() || field.is_empty() {
                        return Ok(None);
                    }
                    field.parse::<f64>().ok().map(Some).ok_or_else(|| {
                        ResumeError::Table(ReadError::BadField {
                            path: runs_path.clone(),
                            record_number: position + 2,
                            column: table
                                .header
                                .as_ref()
                                .and_then(|header| header.get(column))
                                .cloned()
                                .unwrap_or_default(),
                            text: field.to_owned(),
                        })
                    })
                })
                .collect::<Result<_, _>>()?;
            counts.count(record.status);
            runs.push(RecordedRun {
                run_key: record.run_key,
                values,
            });
        }
        let recorded_runs = runs.len() as u64;
        let series = SeriesScan::read(&series_path, |run_id| run_id < recorded_runs).map_err(ResumeError::Table)?;
        if series.header.as_deref().is_some_and(|header| header != series_header) {
            return Err(ResumeError::ColumnsChanged { file: SERIES_FILE });
        }
        if series.kept_after_dropped || series.segments.len() > 1 {
            return Err(ResumeError::SeriesOutOfOrder);
        }
        let fresh_tables = table.header.is_none() || series.header.is_none();
        if fresh_tables {
            runs.clear();
            counts = ResultCounts::default();
        }
        Ok(Self {
            recorded,
            runs: runs.into(),
            counts,
            runs_bytes: table.complete_bytes,
            series_bytes: series.first_dropped_offset.unwrap_or(series.complete_bytes),
            fresh_tables,
        })
    }

    /// Cuts the tables of `dir` to the runs kept, or removes both when they are written afresh.
    #[cfg(not(target_arch = "wasm32"))]
    fn repair(&self, dir: &OutputDir) -> Result<(), OutputError> {
        for (file, length) in [(RUNS_FILE, self.runs_bytes), (SERIES_FILE, self.series_bytes)] {
            let path = dir.path().join(file);
            let repaired = if self.fresh_tables {
                match std::fs::remove_file(&path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    removed => removed,
                }
            } else {
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|table| table.set_len(length))
            };
            repaired.map_err(|source| OutputError::Write { path, source })?;
        }
        Ok(())
    }
}

/// Runs the search of `inputs.spec` into the directory `output_dir`, and returns its record.
///
/// The search plans its fixed values and actions, checks their config against the device, probes it, and chooses a
/// layout for one batch of runs. It then writes the manifest with status `running`, and streams each run to
/// `runs.csv` and `series.csv` and each batch to the search tables. Once the budget is spent it writes `best.csv` or
/// `archive.csv`, rebuilds `summary.csv` and replaces the manifest with its final status.
///
/// `plan`, when given, is the search plan of `inputs.spec`, and the spec is planned here otherwise. With
/// `options.resume`, a directory holding runs of the same search is replayed, as the module documentation describes.
/// A search runs whole, so `options.shard` must be the whole plan, and it never retries failed runs.
///
/// # Errors
///
/// Returns [`ExploreError`] when the search cannot be planned, its config does not fit the device, the probe build
/// fails, a watched column names no reducer, the output directory holds results and is not resumed, the directory
/// cannot be resumed, the results cannot be written, or a batch cannot run.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_search_into_directory(
    inputs: &SweepInputs<'_>,
    plan: Option<Arc<SearchPlan>>,
    output_dir: &Path,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    let preparation = SearchPreparation::new(inputs, plan, None)?;
    preparation.announce(inputs, progress);
    let record = preparation.write_directory(inputs, output_dir, progress)?;
    progress.report(&ProgressEvent::Ended(&record.report));
    Ok(record)
}

/// Runs the search of `inputs.spec`, holding its files in memory, and returns its record.
///
/// `plan`, when given, is the search plan of `inputs.spec`, and the spec is planned here otherwise. The files hold the
/// bytes a directory would. The caller's `inputs` name no folder.
///
/// # Errors
///
/// Returns the errors of [`run_search_into_directory`] that do not come from a directory.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_search_in_memory(
    inputs: &SweepInputs<'_>,
    plan: Option<Arc<SearchPlan>>,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    let preparation = SearchPreparation::new(inputs, plan, None)?;
    preparation.announce(inputs, progress);
    let (mut output, manifest) = preparation.memory_output(inputs)?;
    let executor = preparation.executor(inputs)?;
    let end = preparation.run_all(&executor, &mut output, progress)?;
    let record = preparation.finish_in_memory(output, manifest, end)?;
    progress.report(&ProgressEvent::Ended(&record.report));
    Ok(record)
}

/// A search planned, checked and probed, with its layout chosen.
pub(crate) struct SearchPreparation {
    plan: Arc<SearchPlan>,
    probe: ProbeReport,
    measure: Arc<MeasurePlan>,
    /// Position of each watched column among the reducer columns.
    watched_reducers: Vec<usize>,
    /// Directory the search resumes, `None` for a search that starts afresh.
    resumed: Option<RecordedSearch>,
    /// Directory a resume holds locked from before its scan until its last write, `None` for a dry run or a search
    /// that starts afresh.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "a search in a browser writes no directory")
    )]
    locked: Option<OutputDir>,
    outline: SweepOutline,
    /// Clock reading when planning started.
    started: Instant,
    started_unix_ms: u64,
}

/// Session of a search and the writers of its tables while its batches run.
pub(crate) struct SearchWriters<W: Write> {
    pub(crate) session: SearchSession,
    pub(crate) writer: OutputWriter<W>,
    pub(crate) tables: SearchTablesWriter<W>,
    /// Column layout of a config in the search tables.
    pub(crate) columns: ConfigColumns,
}

impl SearchPreparation {
    /// Checks and probes `plan`, the search of `inputs.spec`, and chooses its layout. With no `plan`, the spec is
    /// planned first. `probe`, when given, takes the place of the report [`ProbeReport::for_plan`] gives for
    /// [`SearchPlan::base`], and its clock readings the ones taken on entry.
    pub(crate) fn new(
        inputs: &SweepInputs<'_>,
        plan: Option<Arc<SearchPlan>>,
        probe: Option<TimedProbe>,
    ) -> Result<Self, ExploreError> {
        let (entry, gpu, options) = (inputs.entry, inputs.gpu, inputs.options);
        if options.shard != Shard::WHOLE {
            return Err(SearchPlanError::Sharded.into());
        }
        if options.retry_failed {
            return Err(SearchPlanError::RetryFailed.into());
        }
        let (started, started_unix_ms, probe) = match probe {
            Some(timed) => (timed.started, timed.started_unix_ms, Some(timed.report)),
            None => (Instant::now(), now_unix_ms(), None),
        };
        let plan = match plan {
            Some(plan) => plan,
            None => Arc::new(SearchPlan::new(inputs.spec, &entry.schema())?),
        };
        let resume_dir = inputs
            .folder
            .filter(|output_dir| options.resume && OutputDir::holds_results(output_dir));
        let locked = match (resume_dir, inputs.folder) {
            (None, Some(output_dir)) => {
                OutputDir::check_free(output_dir)?;
                None
            }
            // Locked before the scan and held until the last write. Otherwise another writer could change the
            // tables between the two.
            (Some(output_dir), _) if !inputs.dry_run => Some(OutputDir::open(output_dir)?),
            _ => None,
        };
        let base = plan.base();
        if let Some(ctx) = gpu {
            check_capacity(entry, base, &ctx.device.limits())?;
        }
        let probe = match probe {
            Some(probe) => probe,
            None => ProbeReport::for_plan(entry, gpu, base)?,
        };
        let measure = MeasurePlan::new(base.run_settings(), base.measure_settings(), probe.columns.clone())?;
        let watched_reducers = plan.watched_reducers(&measure)?;
        let resumed = resume_dir
            .map(|output_dir| {
                let params = entry.param_descriptors();
                let columns = column_names(params, base.actions(), measure.reducers().names());
                RecordedSearch::read(
                    output_dir,
                    &plan,
                    &runs_csv::header_line(&columns),
                    &series_csv::header_line(measure.columns()),
                    &watched_reducers,
                    params.len() + base.actions().len(),
                )
            })
            .transpose()?;
        if let Some(recorded) = resumed.as_ref().filter(|recorded| !recorded.runs.is_empty()) {
            // Every held run is checked before anything is written. The search tables are emptied before the
            // batches replay, and a changed run found then would leave them holding their headers alone.
            SearchSession::new(Arc::clone(&plan), watched_reducers.clone(), Arc::clone(&recorded.runs))?
                .replay_recorded_runs()?;
        }
        let skipped = resumed.as_ref().map_or(0, |recorded| recorded.runs.len() as u64);
        let pending = plan.run_count().saturating_sub(skipped);
        let batch_runs = (plan.search.batch_size as u64).saturating_mul(plan.replicates());
        let (layout, projected_bytes) = sized_layout(entry, gpu, options, &probe, pending.min(batch_runs))?;
        let outline = SweepOutline {
            model: entry.id().to_owned(),
            backend: entry.metadata().backend,
            configs: None,
            replicates: plan.replicates(),
            runs: plan.run_count(),
            blocks: Vec::new(),
            shard: Shard::WHOLE,
            skipped,
            pending,
            layout,
            projected_bytes,
            series_rows: pending.saturating_mul(measure.series_row_count()),
            stat_columns: (0..measure.columns().len())
                .map(|column| measure.columns().name(column).to_owned())
                .collect(),
            reducer_columns: measure.reducers().names().to_vec(),
            dry_run: inputs.dry_run,
            search: Some(plan.outline()),
        };
        Ok(Self {
            plan,
            probe,
            measure: Arc::new(measure),
            watched_reducers,
            resumed,
            locked,
            outline,
            started,
            started_unix_ms,
        })
    }

    /// Reports the outline, then each warning of the plan and of a resume under another build than the engine or
    /// model build of `inputs`.
    pub(crate) fn announce(&self, inputs: &SweepInputs<'_>, progress: &mut dyn Progress) {
        progress.report(&ProgressEvent::Planned(&self.outline));
        let mut warnings: Vec<SweepWarning> = self
            .plan
            .base
            .warnings()
            .iter()
            .cloned()
            .map(SweepWarning::Plan)
            .collect();
        if let Some(resumed) = &self.resumed {
            warnings.extend(build_warnings(&resumed.recorded, inputs.provenance, inputs.entry));
        }
        for warning in &warnings {
            progress.report(&ProgressEvent::Warned(warning));
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn measure(&self) -> &Arc<MeasurePlan> {
        &self.measure
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn plan(&self) -> &Arc<SearchPlan> {
        &self.plan
    }

    /// Returns the manifest of the search while it runs.
    fn manifest(&self, inputs: &SweepInputs<'_>) -> Result<Manifest, OutputError> {
        running_manifest(
            inputs,
            &ManifestParts {
                mode: ManifestMode::Search,
                plan: &self.plan.base,
                probe: &self.probe,
                outline: &self.outline,
                manifest_plan: ManifestPlan {
                    plan_hash: hex(self.plan.base.plan_hash()),
                    results_fingerprint: hex(self.plan.base.results_fingerprint()),
                    configs: self.outline.configs,
                    replicates: self.outline.replicates,
                    runs: self.outline.runs,
                    blocks: Vec::new(),
                },
                started_unix_ms: self.started_unix_ms,
                recorded: self.resumed.as_ref().map(|resumed| &resumed.recorded),
                search: Some(manifest_search(&self.plan)),
            },
        )
    }

    /// Returns a session of the search, reading back the runs a resumed directory holds.
    fn session(&self) -> Result<SearchSession, ExploreError> {
        let recorded = self
            .resumed
            .as_ref()
            .map(|resumed| Arc::clone(&resumed.runs))
            .unwrap_or_default();
        Ok(SearchSession::new(
            Arc::clone(&self.plan),
            self.watched_reducers.clone(),
            recorded,
        )?)
    }

    /// Returns the session and writers over memory of a search held in memory, and its manifest while it runs.
    pub(crate) fn memory_output(
        &self,
        inputs: &SweepInputs<'_>,
    ) -> Result<(SearchWriters<Vec<u8>>, Manifest), ExploreError> {
        let params = inputs.entry.param_descriptors();
        let writer = memory_writer(&self.plan.base, params, &self.measure)?;
        let generations = writes_generations(&self.plan).then(Vec::new);
        let tables = SearchTablesWriter::new(
            Vec::new(),
            Vec::new(),
            generations,
            params,
            self.plan.base.actions(),
            self.is_pattern_search(),
        )
        .map_err(|source| table_error(Path::new(EVALUATIONS_FILE), source))?;
        let output = SearchWriters {
            session: self.session()?,
            writer,
            tables,
            columns: ConfigColumns::new(params, self.plan.base.actions()),
        };
        Ok((output, self.manifest(inputs)?))
    }

    fn is_pattern_search(&self) -> bool {
        matches!(self.plan.search.algorithm, SearchAlgorithm::PatternSpaceExploration(_))
    }

    /// Writes the manifest with status `running`, runs the search into `output_dir` and replaces the manifest with
    /// the search's final status.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_directory(
        &self,
        inputs: &SweepInputs<'_>,
        output_dir: &Path,
        progress: &mut dyn Progress,
    ) -> Result<SweepRecord, ExploreError> {
        let created;
        let dir = if let Some(dir) = &self.locked {
            dir
        } else {
            created = OutputDir::create(output_dir)?;
            &created
        };
        let mut manifest = self.manifest(inputs)?;
        dir.write_manifest(&manifest)?;
        let mut standing = None;
        let (end, counts, session, search_report) = match self.run_into(dir, inputs, progress, &mut standing) {
            Ok(finished) => finished,
            Err(error) => {
                manifest.fail(now_unix_ms());
                if let Some(standing) = standing {
                    manifest.search = Some(standing);
                }
                // The search's own error is the one to report. A manifest that cannot be written says `running`.
                drop(dir.write_manifest(&manifest));
                return Err(error);
            }
        };
        let end = finish_manifest(&mut manifest, end, counts);
        manifest.search = Some(session.manifest_search());
        dir.write_manifest(&manifest)?;
        Ok(SweepRecord {
            report: self.report(end, counts, Some(output_dir.to_owned())),
            manifest,
            files: None,
            search: Some(search_report),
        })
    }

    /// Repairs a resumed directory, runs the search into `dir`, writes its closing table and rebuilds the summary.
    ///
    /// Returns the end of the search, the counts of the rows `runs.csv` holds, the session and its final report.
    /// `standing` receives the manifest's record of the search once the batches stop, with an error or without.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_into(
        &self,
        dir: &OutputDir,
        inputs: &SweepInputs<'_>,
        progress: &mut dyn Progress,
        standing: &mut Option<ManifestSearch>,
    ) -> Result<(BatchEnd, ResultCounts, SearchSession, SearchReport), ExploreError> {
        let params = inputs.entry.param_descriptors();
        let executor = self.executor(inputs)?;
        let fresh_tables = match &self.resumed {
            Some(resumed) => {
                resumed.repair(dir)?;
                resumed.fresh_tables
            }
            None => true,
        };
        let writer = if fresh_tables {
            dir.open_writer(&self.plan.base, params, &self.measure)?
        } else {
            dir.append_writer(&self.plan.base, params)?
        };
        let generations = if writes_generations(&self.plan) {
            Some(dir.create_table(GENERATIONS_FILE)?)
        } else {
            None
        };
        let tables = SearchTablesWriter::new(
            dir.create_table(EVALUATIONS_FILE)?,
            dir.create_table(BATCHES_FILE)?,
            generations,
            params,
            self.plan.base.actions(),
            self.is_pattern_search(),
        )
        .map_err(|source| table_error(&dir.path().join(EVALUATIONS_FILE), source))?;
        let mut output = SearchWriters {
            session: self.session()?,
            writer,
            tables,
            columns: ConfigColumns::new(params, self.plan.base.actions()),
        };
        let end = self.run_all(&executor, &mut output, progress);
        *standing = Some(output.session.manifest_search());
        let end = end?;
        let SearchWriters {
            session,
            writer,
            tables,
            columns,
        } = output;
        let written = writer.counts();
        writer.finish().map_err(|source| OutputError::Write {
            path: dir.path().to_owned(),
            source,
        })?;
        tables
            .into_inner()
            .map_err(|source| table_error(&dir.path().join(EVALUATIONS_FILE), source))?;
        let closing_file = closing_table_file(&self.plan);
        let search_report = session.report();
        session
            .write_closing_table(&search_report, dir.create_table(closing_file)?, &columns)
            .map_err(|source| table_error(&dir.path().join(closing_file), source))?;
        dir.write_summary()?;
        let kept = self.resumed.as_ref().map(|resumed| resumed.counts).unwrap_or_default();
        Ok((end, kept + written, session, search_report))
    }

    /// Returns the executor that runs the search's batches.
    ///
    /// # Errors
    ///
    /// Returns the error of [`Executor::new`].
    #[cfg(not(target_arch = "wasm32"))]
    fn executor<'a>(&self, inputs: &SweepInputs<'a>) -> Result<Executor<'a>, ExploreError> {
        let options = inputs.options;
        let executor = Executor::new(
            inputs.entry,
            inputs.gpu,
            Arc::clone(&self.measure),
            self.outline.layout,
            options.control.clone(),
        )?;
        Ok(executor
            .with_timeout(self.plan.base.run_settings().timeout)
            .with_active_runs(options.active_runs.clone())
            .with_gpu_memory_budget(options.gpu_memory_budget))
    }

    /// Runs batches on `executor` until the budget is spent or its control aborts, writing each run and each batch to
    /// `output` and reporting both to `progress`.
    ///
    /// Returns the end of the batches.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_all<W: Write>(
        &self,
        executor: &Executor<'_>,
        output: &mut SearchWriters<W>,
        progress: &mut dyn Progress,
    ) -> Result<BatchEnd, ExploreError> {
        let mut meter = ProgressMeter::new(self.outline.pending);
        loop {
            if executor.control().is_aborted() {
                return Ok(BatchEnd::Aborted);
            }
            let Some(batch) = output.session.ask()? else {
                return Ok(BatchEnd::Complete);
            };
            let requests = batch.requests();
            let mut sink = SearchSink {
                batch: &batch,
                writer: &mut output.writer,
                watched_reducers: output.session.watched_reducers(),
                values: Vec::with_capacity(requests.len()),
                progress: &mut *progress,
                meter: &mut meter,
            };
            let end = if requests.is_empty() {
                BatchEnd::Complete
            } else {
                executor.run_batch(&requests, &mut sink)?
            };
            let values = sink.values;
            drop(requests);
            if end != BatchEnd::Complete {
                return Ok(end);
            }
            let update = output.session.tell(batch, values);
            output
                .tables
                .write_batch(&update)
                .map_err(|source| table_error(Path::new(EVALUATIONS_FILE), source))?;
            progress.report(&ProgressEvent::SearchBatchTold(&update));
        }
    }

    /// Returns the record of a search held in memory whose batches ended as `end`, with its files and `manifest` in
    /// its final status.
    pub(crate) fn finish_in_memory(
        &self,
        output: SearchWriters<Vec<u8>>,
        mut manifest: Manifest,
        end: BatchEnd,
    ) -> Result<SweepRecord, ExploreError> {
        let SearchWriters {
            session,
            writer,
            tables,
            columns,
        } = output;
        let counts = writer.counts();
        let end = finish_manifest(&mut manifest, end, counts);
        manifest.search = Some(session.manifest_search());
        let mut files = SweepFiles::assemble(writer, &manifest)?;
        let (evaluations, batches, generations) = tables
            .into_inner()
            .map_err(|source| table_error(Path::new(EVALUATIONS_FILE), source))?;
        let closing_file = closing_table_file(&self.plan);
        let search_report = session.report();
        let closing = session
            .write_closing_table(&search_report, Vec::new(), &columns)
            .map_err(|source| table_error(Path::new(closing_file), source))?;
        files.search_tables.push((EVALUATIONS_FILE, evaluations));
        files.search_tables.push((BATCHES_FILE, batches));
        if let Some(generations) = generations {
            files.search_tables.push((GENERATIONS_FILE, generations));
        }
        files.search_tables.push((closing_file, closing));
        Ok(SweepRecord {
            report: self.report(end, counts, None),
            manifest,
            files: Some(files),
            search: Some(search_report),
        })
    }

    pub(crate) fn report(&self, end: SweepEnd, counts: ResultCounts, output_dir: Option<PathBuf>) -> SweepReport {
        SweepReport {
            outline: self.outline.clone(),
            end,
            counts,
            elapsed: self.started.elapsed(),
            output_dir,
        }
    }
}

/// Returns an [`OutputError::Write`] for a search table at `path`.
fn table_error(path: &Path, source: io::Error) -> ExploreError {
    ExploreError::Output(OutputError::Write {
        path: path.to_owned(),
        source,
    })
}

/// Sink that writes each run of a batch and keeps its watched values, in request order.
#[cfg(not(target_arch = "wasm32"))]
struct SearchSink<'s, W: Write> {
    batch: &'s AskedBatch,
    writer: &'s mut OutputWriter<W>,
    /// Position of each watched column among the reducer columns.
    watched_reducers: &'s [usize],
    /// Watched values of each run committed, in run order.
    values: Vec<Vec<Option<f64>>>,
    progress: &'s mut dyn Progress,
    meter: &'s mut ProgressMeter,
}

#[cfg(not(target_arch = "wasm32"))]
impl<W: Write> RunSink for SearchSink<'_, W> {
    fn commit(&mut self, outcome: RunOutcome) -> io::Result<()> {
        self.writer
            .write_config_run(&outcome, self.batch.config_of(&outcome.run))?;
        self.values.push(watched_values(&outcome, self.watched_reducers));
        self.progress.report(&ProgressEvent::RunCommitted(&outcome));
        Ok(())
    }

    fn finished(&mut self, outcome: &RunOutcome) {
        if let Some(update) = self.meter.record_finished_run(outcome.status) {
            self.progress.report(&ProgressEvent::Progressed(update));
        }
    }
}
