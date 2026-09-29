//! Random search. Every candidate is drawn uniformly from the space, and the best is kept.

use crate::explore::design_rng::DesignRng;
use crate::explore::search::evaluation_log::EvaluationLog;
use crate::explore::search::genome::SearchSpace;
use crate::explore::search::{
    Candidate, CandidateOrigin, CandidateTracker, Evaluation, Objective, Proposal, RankingEntry, SearchReport, Searcher,
};

/// A search that draws every candidate uniformly.
#[derive(Debug, Clone)]
pub struct RandomSearch {
    space: SearchSpace,
    rng: DesignRng,
    tracker: CandidateTracker,
    log: EvaluationLog,
}

impl RandomSearch {
    /// Returns a search of `max_evaluations` evaluations over `space`, drawing from `seed`.
    pub fn new(space: SearchSpace, objective: Objective, max_evaluations: u64, seed: u64) -> Self {
        Self {
            space,
            rng: DesignRng::new(seed),
            tracker: CandidateTracker::new(max_evaluations),
            log: EvaluationLog::new(objective),
        }
    }
}

impl Searcher for RandomSearch {
    fn ask(&mut self, max: usize) -> Vec<Candidate> {
        let proposals = (0..self.tracker.capacity(max))
            .map(|_| Proposal::first_evaluation(self.space.random_genome(&mut self.rng), CandidateOrigin::Random))
            .collect();
        self.tracker.issue(proposals)
    }

    fn tell(&mut self, evaluations: &[Evaluation]) {
        for evaluation in evaluations {
            if let Some((candidate, batch)) = self.tracker.settle(evaluation.candidate_id) {
                self.log.record(&candidate, batch, evaluation);
            }
        }
    }

    fn is_done(&self) -> bool {
        self.tracker.is_done()
    }

    fn report(&self) -> SearchReport {
        SearchReport {
            ranking: self.log.ranking(),
            ..SearchReport::default()
        }
    }

    fn best(&self) -> Option<RankingEntry> {
        self.log.best()
    }

    fn ranking_entry(&self, candidate_id: u64) -> Option<RankingEntry> {
        self.log.ranking_entry(candidate_id)
    }
}

#[cfg(test)]
mod tests {
    use super::RandomSearch;
    use crate::explore::search::tests::support::{drive, unit_space};
    use crate::explore::search::{Aggregate, Candidate, Goal, Objective, Searcher as _};

    fn search(seed: u64) -> RandomSearch {
        let objective = Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Mean,
        };
        RandomSearch::new(unit_space(3), objective, 50, seed)
    }

    #[test]
    fn random_search_is_reproducible_from_its_seed() {
        let sum = |candidate: &Candidate, _| vec![Some(candidate.genome.genes().iter().sum())];
        let (mut first, mut again, mut other) = (search(7), search(7), search(8));
        let first_asked = drive(&mut first, 6, 2, sum);
        assert_eq!(first_asked.len(), 50);
        assert_eq!(drive(&mut again, 6, 2, sum), first_asked);
        assert_eq!(first.report(), again.report());
        assert_ne!(drive(&mut other, 6, 2, sum), first_asked);

        let report = first.report();
        let best = report.best().expect("the search scored candidates");
        let best_sum: f64 = first_asked[best.candidate_id as usize].genome.genes().iter().sum();
        let top = first_asked
            .iter()
            .map(|candidate| candidate.genome.genes().iter().sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(best_sum, top, "the best candidate has the largest sum");
        assert_eq!((best.replicate_count, best.evaluations), (2, 1));
    }
}
