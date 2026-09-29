//! Searches that pick the configs to run from the results of the configs run before.
//!
//! A search varies the factors of its space. A candidate is one point of that space, held as a [`Genome`] and decoded
//! into a config. Evaluating a candidate runs its config for a fixed number of replicates and reports the watched
//! output columns of each run. A searcher asks for candidates a batch at a time, and is told their evaluations
//! before its next ask. A re-evaluation runs more replicates of a candidate already evaluated, and its values count
//! toward that candidate. The budget counts evaluations, re-evaluations included.

pub mod evaluation_log;
pub mod genetic;
pub mod genome;
pub mod hill_climb;
pub mod pse;
pub mod random;

#[cfg(test)]
mod tests;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use crate::explore::factor::FactorSpec;
use crate::explore::search::evaluation_log::CandidateRecord;
use crate::explore::search::genetic::{GeneticAlgorithm, GeneticSettings};
use crate::explore::search::genome::{ConfigKey, Genome, SearchSpace};
use crate::explore::search::hill_climb::{HillClimb, HillClimbSettings};
use crate::explore::search::pse::{ArchiveEntry, PatternCell, PatternSpaceExploration, PatternSpaceSettings};
use crate::explore::search::random::RandomSearch;
use crate::explore::seed::search_seed;

/// A search method, driven by asking it for candidates and telling it their evaluations.
///
/// Every [`Searcher::ask`] is followed by one [`Searcher::tell`] with the evaluations of the candidates it returned.
/// A searcher draws only from its seed, so one sequence of asks and tells always gives the same candidates.
pub trait Searcher {
    /// Returns up to `max` candidates to evaluate next.
    ///
    /// Note that an ask returns fewer at the end of a generation or of the budget, and none once the budget is spent.
    fn ask(&mut self, max: usize) -> Vec<Candidate>;

    /// Takes the evaluations of the candidates the last ask returned, sorted by candidate id.
    ///
    /// An evaluation of a candidate the searcher is not waiting for is ignored.
    fn tell(&mut self, evaluations: &[Evaluation]);

    /// Returns whether the search has asked for its whole budget and been told every evaluation.
    fn is_done(&self) -> bool;

    /// Returns every candidate scored so far, best first, with the finished generations and the archive.
    ///
    /// Note that the report copies every candidate. [`Self::best`], [`Self::ranking_entry`] and
    /// [`Self::archive_entry`] read one candidate or cell at a time.
    fn report(&self) -> SearchReport;

    /// Returns the best candidate so far, or `None` before any evaluation or for a Pattern Space Exploration.
    fn best(&self) -> Option<RankingEntry> {
        None
    }

    /// Returns candidate `candidate_id` with its objective over every replicate it has, or `None` for a candidate
    /// never told or a Pattern Space Exploration.
    fn ranking_entry(&self, _candidate_id: u64) -> Option<RankingEntry> {
        None
    }

    /// Returns the archive entry of `cell`, or `None` for an empty cell or a search other than a Pattern Space
    /// Exploration.
    fn archive_entry(&self, _cell: PatternCell) -> Option<ArchiveEntry> {
        None
    }

    /// Number of filled cells in the archive of a Pattern Space Exploration, 0 for any other search.
    fn filled_cells(&self) -> u64 {
        0
    }

    /// Finished generations of a genetic algorithm, in order, none for any other search.
    fn generations(&self) -> &[GenerationSummary] {
        &[]
    }

    /// Returns the settings a Pattern Space Exploration places outputs with, each axis with its range, or `None`
    /// while an automatic range waits for the initial samples or for any other search.
    fn pattern_settings(&self) -> Option<&PatternSpaceSettings> {
        None
    }
}

/// A point of a search space to evaluate.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Position of the candidate among every candidate of the search, counting from 0.
    pub id: u64,
    pub genome: Genome,
    /// Replicate index of the candidate's first run.
    ///
    /// A first evaluation starts at 0. A re-evaluation starts past every replicate its candidate has.
    pub replicate_offset: u64,
    pub origin: CandidateOrigin,
}

/// Source of a candidate's genome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateOrigin {
    /// Drawn uniformly from the space.
    Random,
    /// Drawn uniformly from the space when a hill climb starts over.
    Restart,
    /// A step away from hill climb incumbent `parent_id`.
    Neighbor { parent_id: u64 },
    /// Candidate `parent_id` with some genes changed.
    Mutation { parent_id: u64 },
    /// Genes taken from two parents, then mutated.
    Crossover {
        first_parent_id: u64,
        second_parent_id: u64,
    },
    /// More replicates of candidate `candidate_id`.
    Reevaluation { candidate_id: u64 },
}

impl CandidateOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Restart => "restart",
            Self::Neighbor { .. } => "neighbor",
            Self::Mutation { .. } => "mutation",
            Self::Crossover { .. } => "crossover",
            Self::Reevaluation { .. } => "reevaluation",
        }
    }

    /// Returns the ids of the parents the genome was bred from, the first parent first.
    ///
    /// A re-evaluation has no parents. [`Self::reevaluated_id`] names the candidate it repeats.
    pub fn parent_ids(self) -> [Option<u64>; 2] {
        match self {
            Self::Random | Self::Restart | Self::Reevaluation { .. } => [None, None],
            Self::Neighbor { parent_id } | Self::Mutation { parent_id } => [Some(parent_id), None],
            Self::Crossover {
                first_parent_id,
                second_parent_id,
            } => [Some(first_parent_id), Some(second_parent_id)],
        }
    }

    /// Returns the candidate a re-evaluation adds replicates to, or `None` for any other origin.
    pub fn reevaluated_id(self) -> Option<u64> {
        match self {
            Self::Reevaluation { candidate_id } => Some(candidate_id),
            _ => None,
        }
    }
}

/// Values the runs of a candidate reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub candidate_id: u64,
    /// Values of the watched columns, one row per replicate in replicate order.
    ///
    /// A row holds the columns [`SearchSpec::watched_columns`] names, in that order. `None` marks a failed run or a
    /// value the run never reached.
    pub outputs: Vec<Vec<Option<f64>>>,
}

/// Output a search scores candidates by, and the direction it prefers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    /// Output column, a reducer column such as `Infected:max`.
    pub column: String,
    pub goal: Goal,
    /// Rule that folds the replicates of a candidate into one value.
    pub aggregate: Aggregate,
}

impl Objective {
    /// Returns the objective over the replicates of `evaluation`, from the first watched column.
    ///
    /// A failed replicate, missing or not finite, counts as [`Goal::worst`].
    pub fn score(&self, evaluation: &Evaluation) -> f64 {
        self.score_values(evaluation.outputs.iter().map(|row| row.first().copied().flatten()))
    }

    /// Returns the objective over replicate values `values`, [`Goal::worst`] standing in for a failed one.
    fn score_values(&self, values: impl Iterator<Item = Option<f64>>) -> f64 {
        let worst = self.goal.worst();
        let mut values: Vec<f64> = values
            .map(|value| value.filter(|value| value.is_finite()).unwrap_or(worst))
            .collect();
        self.aggregate.combine(&mut values).unwrap_or(worst)
    }
}

/// Direction of an objective.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Goal {
    #[default]
    Minimize,
    Maximize,
}

impl Goal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
        }
    }

    /// Value a failed replicate counts as, infinitely far from the goal.
    pub fn worst(self) -> f64 {
        match self {
            Self::Minimize => f64::INFINITY,
            Self::Maximize => f64::NEG_INFINITY,
        }
    }

    /// Returns whether `value` is strictly better than `reference`.
    pub fn is_better(self, value: f64, reference: f64) -> bool {
        self.compare(value, reference) == Ordering::Less
    }

    /// Orders two values, the better one first.
    ///
    /// Note that `NaN` ties with every value.
    pub fn compare(self, first: f64, second: f64) -> Ordering {
        let ordering = first.partial_cmp(&second).unwrap_or(Ordering::Equal);
        match self {
            Self::Minimize => ordering,
            Self::Maximize => ordering.reverse(),
        }
    }
}

impl FromStr for Goal {
    type Err = SearchKeywordError;

    /// Reads a goal as [`Goal::as_str`] writes it.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "minimize" => Ok(Self::Minimize),
            "maximize" => Ok(Self::Maximize),
            _ => Err(SearchKeywordError {
                raw: raw.to_owned(),
                expected: &["minimize", "maximize"],
            }),
        }
    }
}

/// Rule that folds the replicate values of a candidate into one value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Aggregate {
    Mean,
    /// Middle value, or the mean of the middle two for an even count.
    #[default]
    Median,
}

impl Aggregate {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Median => "median",
        }
    }

    /// Returns the mean or median of `values`, or `None` when there are none.
    ///
    /// The median sorts `values` in place.
    pub fn combine(self, values: &mut [f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }
        match self {
            Self::Mean => Some(values.iter().sum::<f64>() / values.len() as f64),
            Self::Median => {
                values.sort_unstable_by(f64::total_cmp);
                let middle = values.len() / 2;
                if values.len() % 2 == 1 {
                    Some(values[middle])
                } else {
                    Some(f64::midpoint(values[middle - 1], values[middle]))
                }
            }
        }
    }
}

impl FromStr for Aggregate {
    type Err = SearchKeywordError;

    /// Reads an aggregate as [`Aggregate::as_str`] writes it.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "mean" => Ok(Self::Mean),
            "median" => Ok(Self::Median),
            _ => Err(SearchKeywordError {
                raw: raw.to_owned(),
                expected: &["mean", "median"],
            }),
        }
    }
}

/// Text that names no goal or aggregate. `expected` lists the names there are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchKeywordError {
    pub raw: String,
    pub expected: &'static [&'static str],
}

impl fmt::Display for SearchKeywordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown value '{}', expected {}",
            self.raw,
            self.expected.join(" or ")
        )
    }
}

impl std::error::Error for SearchKeywordError {}

/// Standing of a search, for its output tables and views.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchReport {
    /// Every candidate a random search, hill climb or genetic algorithm has scored, best first.
    ///
    /// Re-evaluations appear under the candidate they repeat. Among equal objectives the lower id comes first.
    pub ranking: Vec<RankingEntry>,
    /// Every finished generation of a genetic algorithm, in order.
    pub generations: Vec<GenerationSummary>,
    /// Every filled cell of a Pattern Space Exploration archive, in cell order.
    pub archive: Vec<ArchiveEntry>,
}

impl SearchReport {
    /// Returns the best candidate so far, or `None` before any evaluation or for a Pattern Space Exploration.
    pub fn best(&self) -> Option<&RankingEntry> {
        self.ranking.first()
    }

    pub fn ranking_entry(&self, candidate_id: u64) -> Option<&RankingEntry> {
        self.ranking.iter().find(|entry| entry.candidate_id == candidate_id)
    }

    pub fn archive_entry(&self, cell: PatternCell) -> Option<&ArchiveEntry> {
        self.archive
            .binary_search_by(|entry| entry.cell.cmp(&cell))
            .ok()
            .map(|index| &self.archive[index])
    }
}

/// A candidate of a ranking, with its objective over every replicate it has.
#[derive(Debug, Clone, PartialEq)]
pub struct RankingEntry {
    pub candidate_id: u64,
    /// Objective over every replicate, [`Goal::worst`] standing in for a failed one.
    pub objective: f64,
    /// Replicates behind the objective, re-evaluations included.
    pub replicate_count: u64,
    /// Replicates whose value was missing or not finite.
    pub failed_count: u64,
    /// Evaluations of the candidate, the first one and each re-evaluation.
    pub evaluations: u64,
    /// Index of the ask that first returned the candidate, counting from 0.
    pub first_batch: u64,
}

/// Objectives of the members of one generation of a genetic algorithm.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationSummary {
    /// Index of the generation, counting from 0.
    pub generation: u64,
    pub best: f64,
    pub median: f64,
    pub worst: f64,
}

/// A search as written, with its space still in the form a spec gives.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSpec {
    pub algorithm: SearchAlgorithm,
    /// Evaluations the search can ask for, re-evaluations included.
    pub max_evaluations: u64,
    /// Most candidates one ask returns.
    pub batch_size: usize,
    /// Output a random search, hill climb or genetic algorithm scores. A Pattern Space Exploration takes none.
    pub objective: Option<Objective>,
    /// Factors the search varies. A range with no step spans every value from its `min` to its `max`.
    pub space: Vec<FactorSpec>,
}

impl SearchSpec {
    /// Checks the budget, the batch size, the objective and the settings of the algorithm.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError`] for no evaluations, a batch size of 0, an objective missing or given where none
    /// is taken, or a setting outside its range.
    pub fn check(&self) -> Result<(), SearchSpecError> {
        if self.max_evaluations == 0 {
            return Err(SearchSpecError::NoEvaluations);
        }
        if self.batch_size == 0 {
            return Err(SearchSpecError::NoBatch);
        }
        match (&self.algorithm, &self.objective) {
            (SearchAlgorithm::PatternSpaceExploration(settings), None) => settings.check(),
            (SearchAlgorithm::PatternSpaceExploration(_), Some(_)) => Err(SearchSpecError::UnusedObjective),
            (algorithm, None) => Err(SearchSpecError::MissingObjective {
                algorithm: algorithm.as_str(),
            }),
            (SearchAlgorithm::Random, Some(_)) => Ok(()),
            (SearchAlgorithm::HillClimb(settings), Some(_)) => settings.check(),
            (SearchAlgorithm::Genetic(settings), Some(_)) => settings.check(),
        }
    }

    /// Returns the output columns each replicate of an evaluation reports, in the order [`Evaluation::outputs`]
    /// holds them.
    pub fn watched_columns(&self) -> Vec<&str> {
        match &self.algorithm {
            SearchAlgorithm::PatternSpaceExploration(settings) => {
                vec![settings.x_axis.column.as_str(), settings.y_axis.column.as_str()]
            }
            SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => self
                .objective
                .iter()
                .map(|objective| objective.column.as_str())
                .collect(),
        }
    }

    /// Returns a searcher over `space`, drawing from [`search_seed`] of `root`.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError`] when [`Self::check`] refuses the spec.
    pub fn searcher(&self, space: &SearchSpace, root: u64) -> Result<Box<dyn Searcher + Send>, SearchSpecError> {
        self.check()?;
        let space = space.clone();
        let seed = search_seed(root);
        let budget = self.max_evaluations;
        let objective = || {
            self.objective.clone().ok_or(SearchSpecError::MissingObjective {
                algorithm: self.algorithm.as_str(),
            })
        };
        Ok(match &self.algorithm {
            SearchAlgorithm::Random => Box::new(RandomSearch::new(space, objective()?, budget, seed)),
            SearchAlgorithm::HillClimb(settings) => {
                Box::new(HillClimb::new(space, *settings, objective()?, budget, seed)?)
            }
            SearchAlgorithm::Genetic(settings) => {
                Box::new(GeneticAlgorithm::new(space, *settings, objective()?, budget, seed)?)
            }
            SearchAlgorithm::PatternSpaceExploration(settings) => {
                Box::new(PatternSpaceExploration::new(space, settings.clone(), budget, seed)?)
            }
        })
    }
}

/// Search method, with the settings it alone takes.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchAlgorithm {
    Random,
    HillClimb(HillClimbSettings),
    Genetic(GeneticSettings),
    PatternSpaceExploration(PatternSpaceSettings),
}

impl SearchAlgorithm {
    /// Returns the name a spec file gives the algorithm.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::HillClimb(_) => "hill_climb",
            Self::Genetic(_) => "genetic",
            Self::PatternSpaceExploration(_) => "pse",
        }
    }
}

/// A search spec that cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchSpecError {
    /// A budget of 0 evaluations.
    NoEvaluations,
    /// A batch size of 0.
    NoBatch,
    /// A random search, hill climb or genetic algorithm with no objective.
    MissingObjective { algorithm: &'static str },
    /// An objective given to a Pattern Space Exploration. Pattern Space Exploration scores none.
    UnusedObjective,
    /// Setting `key`, set to `value` where it takes `expected`.
    Setting {
        key: &'static str,
        value: String,
        expected: String,
    },
}

impl fmt::Display for SearchSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEvaluations => write!(f, "max_evaluations must be at least 1"),
            Self::NoBatch => write!(f, "batch_size must be at least 1"),
            Self::MissingObjective { algorithm } => write!(f, "a {algorithm} search needs an objective"),
            Self::UnusedObjective => write!(f, "a pse search takes x_axis and y_axis, and no objective"),
            Self::Setting { key, value, expected } => write!(f, "{key} must be {expected}, got {value}"),
        }
    }
}

impl std::error::Error for SearchSpecError {}

/// Returns an error for setting `key` unless `holds`.
fn check_setting(
    holds: bool,
    key: &'static str,
    value: impl fmt::Display,
    expected: impl Into<String>,
) -> Result<(), SearchSpecError> {
    if holds {
        Ok(())
    } else {
        Err(SearchSpecError::Setting {
            key,
            value: value.to_string(),
            expected: expected.into(),
        })
    }
}

/// A candidate before it has an id.
#[derive(Debug, Clone, PartialEq)]
struct Proposal {
    genome: Genome,
    replicate_offset: u64,
    origin: CandidateOrigin,
}

impl Proposal {
    /// Returns a proposal for a first evaluation of `genome`.
    fn first_evaluation(genome: Genome, origin: CandidateOrigin) -> Self {
        Self {
            genome,
            replicate_offset: 0,
            origin,
        }
    }

    /// Returns a proposal for more replicates of candidate `candidate_id`, whose evaluations `record` holds.
    fn reevaluation(candidate_id: u64, record: &CandidateRecord) -> Self {
        Self {
            genome: record.genome().clone(),
            replicate_offset: record.replicate_count(),
            origin: CandidateOrigin::Reevaluation { candidate_id },
        }
    }
}

/// Most genomes drawn for one proposal while each decodes to a config some candidate has.
const MAX_CONFIG_DRAWS: usize = 16;

/// Result of [`draw_config`].
#[derive(Debug, Clone, PartialEq)]
enum ConfigDraw {
    /// A genome whose config no candidate and no proposal has, with its key.
    New(Genome, ConfigKey),
    /// Every draw gave a known config. The first gave the config of candidate `candidate_id`.
    Known { candidate_id: u64 },
    /// Every draw gave a known config. The first gave the config of a proposal not yet asked for.
    Proposed,
}

/// Draws genomes with `draw`, at most [`MAX_CONFIG_DRAWS`] times, until one decodes to a config that no candidate of
/// `config_candidates` and no key of `proposed` has.
///
/// `config_candidates` holds the first candidate of each config, by key, and `proposed` the keys of the proposals
/// not yet asked for.
fn draw_config(
    space: &SearchSpace,
    config_candidates: &BTreeMap<ConfigKey, u64>,
    proposed: &BTreeSet<ConfigKey>,
    mut draw: impl FnMut() -> Genome,
) -> ConfigDraw {
    let mut first = None;
    for _ in 0..MAX_CONFIG_DRAWS {
        let genome = draw();
        let key = space.config_key(&genome);
        let candidate_id = config_candidates.get(&key).copied();
        if candidate_id.is_none() && !proposed.contains(&key) {
            return ConfigDraw::New(genome, key);
        }
        first.get_or_insert(candidate_id);
    }
    match first.flatten() {
        Some(candidate_id) => ConfigDraw::Known { candidate_id },
        None => ConfigDraw::Proposed,
    }
}

/// Ids, budget and pending candidates of a searcher.
#[derive(Debug, Clone, PartialEq)]
struct CandidateTracker {
    max_evaluations: u64,
    /// Candidates issued so far, and so the id of the next one.
    issued: u64,
    /// Asks that issued at least one candidate.
    batches: u64,
    /// Candidates issued and not yet told, each with the index of its batch.
    pending: BTreeMap<u64, (Candidate, u64)>,
}

impl CandidateTracker {
    fn new(max_evaluations: u64) -> Self {
        Self {
            max_evaluations,
            issued: 0,
            batches: 0,
            pending: BTreeMap::new(),
        }
    }

    /// Returns the number of candidates an ask for `max` can issue within the budget.
    fn capacity(&self, max: usize) -> usize {
        let remaining = self.max_evaluations.saturating_sub(self.issued);
        usize::try_from(remaining).map_or(max, |remaining| remaining.min(max))
    }

    /// Returns `proposals` as the candidates of one batch, numbered from the next id.
    fn issue(&mut self, proposals: Vec<Proposal>) -> Vec<Candidate> {
        if proposals.is_empty() {
            return Vec::new();
        }
        let batch = self.batches;
        self.batches += 1;
        proposals
            .into_iter()
            .map(|proposal| {
                let candidate = Candidate {
                    id: self.issued,
                    genome: proposal.genome,
                    replicate_offset: proposal.replicate_offset,
                    origin: proposal.origin,
                };
                self.issued += 1;
                self.pending.insert(candidate.id, (candidate.clone(), batch));
                candidate
            })
            .collect()
    }

    /// Removes candidate `candidate_id` from the pending ones, and returns it with the index of its batch.
    fn settle(&mut self, candidate_id: u64) -> Option<(Candidate, u64)> {
        self.pending.remove(&candidate_id)
    }

    fn is_waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    fn is_done(&self) -> bool {
        self.issued >= self.max_evaluations && self.pending.is_empty()
    }
}
