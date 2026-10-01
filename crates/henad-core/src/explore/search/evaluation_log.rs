//! Log of the candidates a search scored, with the objective value of every replicate of each.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::explore::search::genome::Genome;
use crate::explore::search::{Candidate, Evaluation, Goal, Objective, RankingEntry};

/// Evaluations a searcher was told, by candidate id, each re-evaluation filed under the candidate it repeats.
///
/// The log keeps its candidates in ranking order as they are told, so the best one and each candidate's standing
/// read without a sort.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationLog {
    objective: Objective,
    records: BTreeMap<u64, CandidateRecord>,
    /// Place of every candidate of `records` in the ranking.
    places: BTreeSet<RankingPlace>,
}

/// Evaluations of one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRecord {
    genome: Genome,
    first_batch: u64,
    evaluations: u64,
    /// Objective value of each replicate in the order told, `None` for a replicate told no value.
    ///
    /// A failed replicate holds `None` or a value that is not finite.
    values: Vec<Option<f64>>,
    /// Objective over every value of `values`.
    objective: f64,
}

impl CandidateRecord {
    pub fn genome(&self) -> &Genome {
        &self.genome
    }

    /// Index of the ask that first returned the candidate.
    pub fn first_batch(&self) -> u64 {
        self.first_batch
    }

    /// Number of evaluations, the first one and each re-evaluation.
    pub fn evaluations(&self) -> u64 {
        self.evaluations
    }

    /// Number of replicates, re-evaluations included.
    pub fn replicate_count(&self) -> u64 {
        self.values.len() as u64
    }

    /// Number of replicates whose value was missing or not finite.
    pub fn failed_count(&self) -> u64 {
        self.values
            .iter()
            .filter(|value| !value.is_some_and(f64::is_finite))
            .count() as u64
    }

    /// Objective over every replicate, [`Goal::worst`] standing in for a failed one.
    pub fn objective(&self) -> f64 {
        self.objective
    }
}

/// Place of a candidate in a ranking, the better objective first and the lower id first among equals.
#[derive(Debug, Clone, Copy)]
struct RankingPlace {
    /// Objective turned so that a lower score is better, with `NaN` as the worst and no negative zero.
    score: f64,
    candidate_id: u64,
}

impl RankingPlace {
    fn new(goal: Goal, objective: f64, candidate_id: u64) -> Self {
        let turned = match goal {
            Goal::Minimize => objective,
            Goal::Maximize => -objective,
        };
        let score = if turned.is_nan() {
            f64::INFINITY
        } else if turned == 0.0 {
            0.0
        } else {
            turned
        };
        Self { score, candidate_id }
    }
}

impl PartialEq for RankingPlace {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RankingPlace {}

impl PartialOrd for RankingPlace {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankingPlace {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then(self.candidate_id.cmp(&other.candidate_id))
    }
}

impl EvaluationLog {
    /// Returns an empty log that scores its candidates by `objective`.
    pub fn new(objective: Objective) -> Self {
        Self {
            objective,
            records: BTreeMap::new(),
            places: BTreeSet::new(),
        }
    }

    pub fn goal(&self) -> Goal {
        self.objective.goal
    }

    /// Files `evaluation` of `candidate`, a candidate the ask with index `batch` returned.
    ///
    /// Only the first watched column of each replicate is kept, the one the objective scores. A re-evaluation of a
    /// candidate the log does not hold starts a record of its own.
    pub fn record(&mut self, candidate: &Candidate, batch: u64, evaluation: &Evaluation) {
        let candidate_id = candidate.origin.reevaluated_id().unwrap_or(candidate.id);
        let goal = self.objective.goal;
        let record = self.records.entry(candidate_id).or_insert_with(|| CandidateRecord {
            genome: candidate.genome.clone(),
            first_batch: batch,
            evaluations: 0,
            values: Vec::new(),
            objective: goal.worst(),
        });
        if record.evaluations > 0 {
            self.places
                .remove(&RankingPlace::new(goal, record.objective, candidate_id));
        }
        record.evaluations += 1;
        record
            .values
            .extend(evaluation.outputs.iter().map(|row| row.first().copied().flatten()));
        record.objective = self.objective.score_values(record.values.iter().copied());
        self.places
            .insert(RankingPlace::new(goal, record.objective, candidate_id));
    }

    pub fn get(&self, candidate_id: u64) -> Option<&CandidateRecord> {
        self.records.get(&candidate_id)
    }

    /// Returns the objective of candidate `candidate_id` over every replicate it has.
    ///
    /// A candidate the log does not hold scores [`Goal::worst`].
    pub fn objective(&self, candidate_id: u64) -> f64 {
        self.records
            .get(&candidate_id)
            .map_or(self.objective.goal.worst(), CandidateRecord::objective)
    }

    /// Returns `candidate_ids` best first, the lower id first among equal objectives.
    pub fn rank(&self, candidate_ids: &[u64]) -> Vec<u64> {
        let goal = self.objective.goal;
        let mut places: Vec<RankingPlace> = candidate_ids
            .iter()
            .map(|&candidate_id| RankingPlace::new(goal, self.objective(candidate_id), candidate_id))
            .collect();
        places.sort_unstable();
        places.into_iter().map(|place| place.candidate_id).collect()
    }

    /// Returns every candidate of the log best first, the lower id first among equal objectives.
    pub fn ranking(&self) -> Vec<RankingEntry> {
        self.places
            .iter()
            .filter_map(|place| self.ranking_entry(place.candidate_id))
            .collect()
    }

    /// Returns the best candidate of the log, or `None` for an empty log.
    pub fn best(&self) -> Option<RankingEntry> {
        let place = self.places.first()?;
        self.ranking_entry(place.candidate_id)
    }

    /// Returns candidate `candidate_id` with its objective over every replicate, or `None` for a candidate the log
    /// does not hold.
    pub fn ranking_entry(&self, candidate_id: u64) -> Option<RankingEntry> {
        self.records.get(&candidate_id).map(|record| RankingEntry {
            candidate_id,
            objective: record.objective,
            replicate_count: record.replicate_count(),
            failed_count: record.failed_count(),
            evaluations: record.evaluations,
            first_batch: record.first_batch,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::EvaluationLog;
    use crate::explore::design_rng::DesignRng;
    use crate::explore::factor::{Factor, FactorDomain, FactorSlot};
    use crate::explore::search::genome::SearchSpace;
    use crate::explore::search::{Aggregate, Candidate, CandidateOrigin, Evaluation, Goal, Objective};

    fn objective(goal: Goal, aggregate: Aggregate) -> Objective {
        Objective {
            column: "Infected:max".to_owned(),
            goal,
            aggregate,
        }
    }

    fn candidate(id: u64, replicate_offset: u64, origin: CandidateOrigin) -> Candidate {
        let space = SearchSpace::new(vec![Factor {
            slot: FactorSlot::Param(0),
            domain: FactorDomain::Continuous { min: 0.0, max: 1.0 },
        }])
        .expect("one factor");
        Candidate {
            id,
            genome: space.random_genome(&mut DesignRng::new(id)),
            replicate_offset,
            origin,
        }
    }

    fn evaluation(candidate_id: u64, values: &[Option<f64>]) -> Evaluation {
        Evaluation {
            candidate_id,
            outputs: values.iter().map(|&value| vec![value, Some(-1.0)]).collect(),
        }
    }

    /// Returns a log under `objective` that holds candidate 0, told `values`.
    fn log_of(objective: Objective, values: &[Option<f64>]) -> EvaluationLog {
        let mut log = EvaluationLog::new(objective);
        log.record(&candidate(0, 0, CandidateOrigin::Random), 0, &evaluation(0, values));
        log
    }

    #[test]
    fn a_failed_replicate_counts_as_the_worst_value() {
        let values = [Some(1.0), None, Some(f64::NAN), Some(3.0)];
        let log = log_of(objective(Goal::Minimize, Aggregate::Mean), &values);
        let record = log.get(0).expect("candidate 0 was told");
        assert_eq!(record.replicate_count(), 4);
        assert_eq!(record.failed_count(), 2);
        assert_eq!(log.objective(0), f64::INFINITY);
        let maximize = log_of(objective(Goal::Maximize, Aggregate::Median), &values);
        assert_eq!(
            maximize.objective(0),
            f64::NEG_INFINITY,
            "the two failures sit below the rest"
        );
        let median = log_of(objective(Goal::Minimize, Aggregate::Median), &values);
        assert_eq!(median.objective(0), f64::INFINITY);
        assert_eq!(
            median.objective(9),
            f64::INFINITY,
            "an unknown candidate scores the worst value"
        );
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_two() {
        assert_eq!(Aggregate::Median.combine(&mut [4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(Aggregate::Median.combine(&mut [5.0, 1.0, 3.0]), Some(3.0));
        assert_eq!(Aggregate::Mean.combine(&mut [1.0, 2.0, 6.0]), Some(3.0));
        assert_eq!(Aggregate::Median.combine(&mut []), None);
    }

    #[test]
    fn a_reevaluation_adds_to_the_candidate_it_repeats() {
        let mut log = EvaluationLog::new(objective(Goal::Maximize, Aggregate::Mean));
        log.record(
            &candidate(0, 0, CandidateOrigin::Random),
            0,
            &evaluation(0, &[Some(1.0), Some(3.0)]),
        );
        log.record(
            &candidate(1, 0, CandidateOrigin::Random),
            0,
            &evaluation(1, &[Some(2.5), Some(2.5)]),
        );
        assert_eq!(log.rank(&[0, 1]), [1, 0]);
        assert_eq!(log.best().map(|entry| entry.candidate_id), Some(1));
        let repeat = candidate(2, 2, CandidateOrigin::Reevaluation { candidate_id: 0 });
        log.record(&repeat, 1, &evaluation(2, &[Some(5.0), Some(7.0)]));
        assert!(log.get(2).is_none(), "a re-evaluation has no record of its own");
        let record = log.get(0).expect("candidate 0 was told");
        assert_eq!(
            (record.replicate_count(), record.evaluations(), record.first_batch()),
            (4, 2, 0)
        );
        assert_eq!(log.objective(0), 4.0);
        let ranking = log.ranking();
        assert_eq!(
            ranking.iter().map(|entry| entry.candidate_id).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!((ranking[0].replicate_count, ranking[0].evaluations), (4, 2));
        assert_eq!(
            log.best().as_ref(),
            ranking.first(),
            "the best moves with the re-evaluation"
        );
        assert_eq!(log.ranking_entry(1).as_ref(), ranking.get(1));
    }

    #[test]
    fn a_reevaluation_can_move_the_best_down_the_ranking() {
        let mut log = EvaluationLog::new(objective(Goal::Minimize, Aggregate::Median));
        for (id, value) in [(0, 1.0), (1, 2.0), (2, 3.0)] {
            log.record(
                &candidate(id, 0, CandidateOrigin::Random),
                0,
                &evaluation(id, &[Some(value)]),
            );
        }
        let repeat = candidate(3, 1, CandidateOrigin::Reevaluation { candidate_id: 0 });
        log.record(&repeat, 1, &evaluation(3, &[Some(9.0), Some(9.0)]));
        let ids = |log: &EvaluationLog| log.ranking().iter().map(|entry| entry.candidate_id).collect::<Vec<_>>();
        assert_eq!(ids(&log), [1, 2, 0]);
        assert_eq!(log.best().map(|entry| entry.candidate_id), Some(1));
    }

    #[test]
    fn equal_objectives_rank_by_candidate_id() {
        let mut log = EvaluationLog::new(objective(Goal::Minimize, Aggregate::Median));
        for id in [3, 1, 2] {
            log.record(
                &candidate(id, 0, CandidateOrigin::Random),
                0,
                &evaluation(id, &[Some(1.0)]),
            );
        }
        assert_eq!(log.rank(&[3, 2, 1]), [1, 2, 3]);
        let zeros = |goal| {
            let mut log = EvaluationLog::new(objective(goal, Aggregate::Mean));
            for (id, value) in [(0, 0.0), (1, -0.0)] {
                log.record(
                    &candidate(id, 0, CandidateOrigin::Random),
                    0,
                    &evaluation(id, &[Some(value)]),
                );
            }
            log.rank(&[1, 0])
        };
        assert_eq!(zeros(Goal::Minimize), [0, 1], "zero and negative zero tie");
        assert_eq!(zeros(Goal::Maximize), [0, 1], "zero and negative zero tie");
    }
}
