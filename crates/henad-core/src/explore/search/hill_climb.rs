//! Hill climbing with restarts, a (1 + lambda) climb whose lambda is the batch size.
//!
//! A climb starts from the best of a batch of random candidates. Each later batch holds neighbors of the incumbent,
//! and the climb moves to the best of them when it strictly beats the incumbent. After `patience` batches without a
//! move, the climb starts over from a new random batch.
//!
//! No two first evaluations share a config. A starting point or a neighbor is drawn again when an earlier candidate
//! has the same config. When no draw finds a new config, the batch re-evaluates that candidate in its place.
//! A re-evaluated starting point competes for the start like any other, and a re-evaluated neighbor never moves the
//! climb.

use std::collections::{BTreeMap, BTreeSet};

use crate::explore::design_rng::DesignRng;
use crate::explore::search::evaluation_log::EvaluationLog;
use crate::explore::search::genome::{ConfigKey, SearchSpace};
use crate::explore::search::{
    Candidate, CandidateOrigin, CandidateTracker, ConfigDraw, Evaluation, Objective, Proposal, RankingEntry,
    SearchReport, SearchSpecError, Searcher, check_setting, draw_config,
};

/// Settings of a hill climb.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HillClimbSettings {
    /// Largest step of a gene toward a neighbor, as a fraction of its range.
    pub mutation_scale: f64,
    /// Number of batches without a move after which the climb starts over.
    pub patience: u64,
    /// Whether each batch also re-evaluates the incumbent, instead of one neighbor.
    ///
    /// Note that a batch of one candidate holds a neighbor alone.
    pub reevaluate: bool,
}

impl Default for HillClimbSettings {
    fn default() -> Self {
        Self {
            mutation_scale: 0.1,
            patience: 5,
            reevaluate: false,
        }
    }
}

impl HillClimbSettings {
    /// Checks the mutation scale and the patience.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError::Setting`] for a mutation scale that is not a positive number, or a patience of 0.
    pub fn check(&self) -> Result<(), SearchSpecError> {
        check_setting(
            self.mutation_scale.is_finite() && self.mutation_scale > 0.0,
            "hill_climb.mutation_scale",
            self.mutation_scale,
            "a positive number",
        )?;
        check_setting(self.patience >= 1, "hill_climb.patience", self.patience, "at least 1")
    }
}

/// A hill climb with restarts.
#[derive(Debug, Clone)]
pub struct HillClimb {
    space: SearchSpace,
    settings: HillClimbSettings,
    rng: DesignRng,
    tracker: CandidateTracker,
    log: EvaluationLog,
    /// First candidate of each config, by config key.
    config_candidates: BTreeMap<ConfigKey, u64>,
    /// Candidate the climb moves from, `None` while the next ask draws starting points.
    incumbent: Option<u64>,
    /// Known candidates the pending start batch re-evaluates instead of a starting point.
    revisited: BTreeSet<u64>,
    /// Number of batches since the incumbent last moved.
    stalled: u64,
    /// Number of climbs begun, the first one and each restart.
    climbs: u64,
}

impl HillClimb {
    /// Returns a climb of `max_evaluations` evaluations over `space`, drawing from `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError`] when [`HillClimbSettings::check`] rejects `settings`.
    pub fn new(
        space: SearchSpace,
        settings: HillClimbSettings,
        objective: Objective,
        max_evaluations: u64,
        seed: u64,
    ) -> Result<Self, SearchSpecError> {
        settings.check()?;
        Ok(Self {
            space,
            settings,
            rng: DesignRng::new(seed),
            tracker: CandidateTracker::new(max_evaluations),
            log: EvaluationLog::new(objective),
            config_candidates: BTreeMap::new(),
            incumbent: None,
            revisited: BTreeSet::new(),
            stalled: 0,
            climbs: 0,
        })
    }

    /// Returns up to `count` random starting points.
    ///
    /// A starting point whose draws all produce a known config becomes a re-evaluation of the candidate the first draw
    /// matched, unless the batch re-evaluates that candidate already or the match is another starting point of the
    /// batch. Note that the batch is never empty. Its first draw meets no other proposal of the batch.
    fn starting_points(&mut self, count: usize) -> Vec<Proposal> {
        let origin = if self.climbs == 0 {
            CandidateOrigin::Random
        } else {
            CandidateOrigin::Restart
        };
        self.climbs += 1;
        let mut proposals = Vec::with_capacity(count);
        let mut proposed = BTreeSet::new();
        for _ in 0..count {
            let (space, rng) = (&self.space, &mut self.rng);
            match draw_config(space, &self.config_candidates, &proposed, || space.random_genome(rng)) {
                ConfigDraw::New(genome, key) => {
                    proposed.insert(key);
                    proposals.push(Proposal::first_evaluation(genome, origin));
                }
                ConfigDraw::Known { candidate_id } => {
                    if self.revisited.insert(candidate_id) {
                        let known = self
                            .log
                            .get(candidate_id)
                            .expect("a candidate with a config was evaluated");
                        proposals.push(Proposal::reevaluation(candidate_id, known));
                    }
                }
                ConfigDraw::Proposed => {}
            }
        }
        proposals
    }

    /// Returns up to `count` candidates around `incumbent`, starting with a re-evaluation of `incumbent` when the
    /// settings enable re-evaluation.
    ///
    /// A neighbor whose draws all produce a known config becomes a re-evaluation of the candidate the first draw
    /// matched, unless the batch re-evaluates that candidate already or the match is another neighbor of the batch.
    fn neighbors(&mut self, incumbent: u64, count: usize) -> Vec<Proposal> {
        let record = self.log.get(incumbent).expect("the incumbent was evaluated");
        let mut proposals = Vec::with_capacity(count);
        let mut reevaluated = BTreeSet::new();
        if self.settings.reevaluate && count >= 2 {
            proposals.push(Proposal::reevaluation(incumbent, record));
            reevaluated.insert(incumbent);
        }
        let mut proposed = BTreeSet::new();
        for _ in proposals.len()..count {
            let (space, rng, scale) = (&self.space, &mut self.rng, self.settings.mutation_scale);
            let draw = draw_config(space, &self.config_candidates, &proposed, || {
                space.mutate(record.genome(), rng, 1.0, scale)
            });
            match draw {
                ConfigDraw::New(genome, key) => {
                    proposed.insert(key);
                    proposals.push(Proposal::first_evaluation(
                        genome,
                        CandidateOrigin::Neighbor { parent_id: incumbent },
                    ));
                }
                ConfigDraw::Known { candidate_id } => {
                    if reevaluated.insert(candidate_id) {
                        let known = self
                            .log
                            .get(candidate_id)
                            .expect("a candidate with a config was evaluated");
                        proposals.push(Proposal::reevaluation(candidate_id, known));
                    }
                }
                ConfigDraw::Proposed => {}
            }
        }
        proposals
    }
}

impl Searcher for HillClimb {
    fn ask(&mut self, max: usize) -> Vec<Candidate> {
        let count = self.tracker.capacity(max);
        if count == 0 {
            return Vec::new();
        }
        let proposals = match self.incumbent {
            None => self.starting_points(count),
            Some(incumbent) => self.neighbors(incumbent, count),
        };
        self.tracker.issue(proposals)
    }

    fn tell(&mut self, evaluations: &[Evaluation]) {
        let mut starting_points = Vec::new();
        let mut neighbors = Vec::new();
        let revisited = std::mem::take(&mut self.revisited);
        for evaluation in evaluations {
            let Some((candidate, batch)) = self.tracker.settle(evaluation.candidate_id) else {
                continue;
            };
            if candidate.origin.reevaluated_id().is_none() {
                self.config_candidates
                    .entry(self.space.config_key(&candidate.genome))
                    .or_insert(candidate.id);
            }
            self.log.record(&candidate, batch, evaluation);
            match candidate.origin {
                CandidateOrigin::Random | CandidateOrigin::Restart => starting_points.push(candidate.id),
                CandidateOrigin::Neighbor { .. } => neighbors.push(candidate.id),
                CandidateOrigin::Reevaluation { candidate_id } if revisited.contains(&candidate_id) => {
                    starting_points.push(candidate_id);
                }
                CandidateOrigin::Mutation { .. }
                | CandidateOrigin::Crossover { .. }
                | CandidateOrigin::Reevaluation { .. } => {}
            }
        }
        if !starting_points.is_empty() {
            self.incumbent = self.log.rank(&starting_points).first().copied();
            self.stalled = 0;
            return;
        }
        let Some(incumbent) = self.incumbent else {
            return;
        };
        let reference = self.log.objective(incumbent);
        let goal = self.log.goal();
        let improves = |candidate_id: &u64| goal.is_better(self.log.objective(*candidate_id), reference);
        if let Some(best) = self.log.rank(&neighbors).into_iter().next().filter(improves) {
            self.incumbent = Some(best);
            self.stalled = 0;
        } else {
            self.stalled += 1;
            if self.stalled >= self.settings.patience {
                self.incumbent = None;
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
    use std::collections::BTreeSet;

    use super::{HillClimb, HillClimbSettings};
    use crate::explore::search::tests::support::{drive, level_space, noise, unit_space};
    use crate::explore::search::{Aggregate, Candidate, CandidateOrigin, Evaluation, Goal, Objective, Searcher as _};

    fn objective() -> Objective {
        Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Mean,
        }
    }

    /// Returns a smooth hill peaking at `(0.3, 0.7)`, with a little seeded noise.
    fn hill(candidate: &Candidate, replicate: u64) -> Vec<Option<f64>> {
        let genes = candidate.genome.genes();
        let (across, along) = (genes[0] - 0.3, genes[1] - 0.7);
        vec![Some(
            1e-4 * noise(candidate.id, replicate) - across * across - along * along,
        )]
    }

    #[test]
    fn hill_climbing_finds_the_peak_of_a_smooth_function() {
        let settings = HillClimbSettings {
            mutation_scale: 0.1,
            patience: 4,
            reevaluate: false,
        };
        let mut climb = HillClimb::new(unit_space(2), settings, objective(), 400, 3).expect("valid settings");
        let asked = drive(&mut climb, 8, 2, hill);
        let report = climb.report();
        let best = report.best().expect("the climb scored candidates");
        let genes = asked[best.candidate_id as usize].genome.genes();
        assert!(
            (genes[0] - 0.3).abs() < 0.05 && (genes[1] - 0.7).abs() < 0.05,
            "best point {genes:?}"
        );
        assert!(
            asked
                .iter()
                .any(|candidate| matches!(candidate.origin, CandidateOrigin::Neighbor { .. })),
            "the climb moved through neighbors"
        );
    }

    #[test]
    fn a_climb_that_stalls_starts_over() {
        let settings = HillClimbSettings {
            mutation_scale: 0.1,
            patience: 2,
            reevaluate: true,
        };
        let mut climb = HillClimb::new(unit_space(2), settings, objective(), 60, 5).expect("valid settings");
        let flat = |_: &Candidate, _| vec![Some(1.0)];
        let asked = drive(&mut climb, 4, 1, flat);
        let origins: Vec<&str> = asked.iter().map(|candidate| candidate.origin.as_str()).collect();
        let first_restart = origins
            .iter()
            .position(|&origin| origin == "restart")
            .expect("a flat hill never improves");
        assert_eq!(first_restart, 12, "one start batch and two batches without a move");
        assert_eq!(&origins[4..8], ["reevaluation", "neighbor", "neighbor", "neighbor"]);
    }

    #[test]
    fn no_two_first_evaluations_share_a_config() {
        let settings = HillClimbSettings {
            mutation_scale: 0.1,
            patience: 3,
            reevaluate: true,
        };
        let space = level_space();
        let mut climb = HillClimb::new(space.clone(), settings, objective(), 200, 7).expect("valid settings");
        let value = |candidate: &Candidate, replicate| {
            vec![Some(candidate.genome.genes()[0] + 0.1 * noise(candidate.id, replicate))]
        };
        let mut asked = Vec::new();
        let mut revisited_starts = 0;
        while !climb.is_done() {
            let starting = climb.incumbent.is_none();
            let batch = climb.ask(4);
            assert!(!batch.is_empty(), "an ask within the budget returns a candidate");
            let evaluations: Vec<Evaluation> = batch
                .iter()
                .map(|candidate| Evaluation {
                    candidate_id: candidate.id,
                    outputs: (0..2)
                        .map(|index| value(candidate, candidate.replicate_offset + index))
                        .collect(),
                })
                .collect();
            climb.tell(&evaluations);
            if starting {
                assert!(climb.incumbent.is_some(), "every start gives the climb an incumbent");
                if batch
                    .iter()
                    .all(|candidate| candidate.origin.reevaluated_id().is_some())
                {
                    revisited_starts += 1;
                }
            }
            asked.extend(batch);
        }
        assert_eq!(asked.len(), 200);

        let mut keys = BTreeSet::new();
        for candidate in &asked {
            if let CandidateOrigin::Neighbor { parent_id } = candidate.origin {
                assert!(
                    asked[parent_id as usize].origin.reevaluated_id().is_none(),
                    "a re-evaluation never becomes the incumbent"
                );
            }
            if candidate.origin.reevaluated_id().is_none() {
                assert!(
                    keys.insert(space.config_key(&candidate.genome)),
                    "candidate {} repeats the config of an earlier candidate",
                    candidate.id
                );
            }
        }
        assert_eq!(keys.len(), 15, "the climb finds every config");
        assert!(
            asked
                .iter()
                .any(|candidate| candidate.origin == CandidateOrigin::Restart),
            "the climb runs out of new neighbors and starts over"
        );
        assert!(revisited_starts > 0, "a start over known configs re-evaluates them");
    }

    #[test]
    fn a_climb_with_no_patience_is_refused() {
        let settings = HillClimbSettings {
            patience: 0,
            ..HillClimbSettings::default()
        };
        assert!(HillClimb::new(unit_space(1), settings, objective(), 10, 1).is_err());
    }
}
