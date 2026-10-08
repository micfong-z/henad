//! Generational genetic algorithm for a noisy objective.
//!
//! Generation 0 is drawn at random. Each later generation re-evaluates the best members of the previous generation,
//! keeps its elites and fills the rest with children. The children are bred as the generation starts, from the
//! fitness the members had before its re-evaluations. The elites are chosen once the re-evaluations are told.
//!
//! A child comes from a tournament winner, crossed with a second winner at the crossover rate, then mutated gene by
//! gene. Fitness is the objective over every replicate a member has, re-evaluations included.
//!
//! No two first evaluations share a config. A child is mutated again when an earlier candidate has the same config.
//! When no draw finds a new config, the generation re-evaluates that candidate, and the candidate joins it in the
//! child's place.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::explore::design_rng::DesignRng;
use crate::explore::search::evaluation_log::EvaluationLog;
use crate::explore::search::genome::{ConfigKey, Genome, SearchSpace};
use crate::explore::search::{
    Aggregate, Candidate, CandidateOrigin, CandidateTracker, ConfigDraw, Evaluation, GenerationSummary, Goal,
    Objective, Proposal, RankingEntry, SearchReport, SearchSpecError, Searcher, check_setting, draw_config,
};

/// Maximum number of members in a generation.
pub const MAX_POPULATION: usize = 1 << 16;

/// Maximum number of members that one tournament can draw.
pub const MAX_TOURNAMENT_SIZE: usize = 1 << 16;

/// Settings of a genetic algorithm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneticSettings {
    /// Number of members in each generation.
    pub population: usize,
    /// Number of best members carried unchanged into the next generation.
    pub elite_count: usize,
    /// Number of members drawn for each tournament, the best of whom becomes a parent.
    pub tournament_size: usize,
    /// Probability that a child has a second parent.
    pub crossover_rate: f64,
    /// Probability that each gene of a child changes.
    pub mutation_rate: f64,
    /// Largest step of a changed gene, as a fraction of its range.
    pub mutation_scale: f64,
    /// Share of the population re-evaluated each generation, rounded up, best members first.
    pub reevaluate_fraction: f64,
}

impl Default for GeneticSettings {
    fn default() -> Self {
        Self {
            population: 32,
            elite_count: 2,
            tournament_size: 3,
            crossover_rate: 0.9,
            mutation_rate: 0.2,
            mutation_scale: 0.1,
            reevaluate_fraction: 0.25,
        }
    }
}

impl GeneticSettings {
    /// Checks every setting against its range.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError::Setting`] for a population outside 1 to [`MAX_POPULATION`], as many elites as
    /// members, a tournament size outside 1 to [`MAX_TOURNAMENT_SIZE`], a rate or fraction outside `[0, 1]`, or a
    /// mutation scale that is not a positive number.
    pub fn check(&self) -> Result<(), SearchSpecError> {
        check_setting(
            (1..=MAX_POPULATION).contains(&self.population),
            "genetic.population",
            self.population,
            format!("from 1 to {MAX_POPULATION}"),
        )?;
        check_setting(
            self.elite_count < self.population,
            "genetic.elite_count",
            self.elite_count,
            format!("less than the population of {}", self.population),
        )?;
        check_setting(
            (1..=MAX_TOURNAMENT_SIZE).contains(&self.tournament_size),
            "genetic.tournament_size",
            self.tournament_size,
            format!("from 1 to {MAX_TOURNAMENT_SIZE}"),
        )?;
        for (key, rate) in [
            ("genetic.crossover_rate", self.crossover_rate),
            ("genetic.mutation_rate", self.mutation_rate),
            ("genetic.reevaluate_fraction", self.reevaluate_fraction),
        ] {
            check_setting((0.0..=1.0).contains(&rate), key, rate, "from 0 to 1")?;
        }
        check_setting(
            self.mutation_scale.is_finite() && self.mutation_scale > 0.0,
            "genetic.mutation_scale",
            self.mutation_scale,
            "a positive number",
        )
    }

    /// Returns the number of members re-evaluated each generation.
    pub fn reevaluation_count(&self) -> usize {
        let exact = self.reevaluate_fraction * self.population as f64;
        // A product such as 0.1 * 30 comes out a hair above the whole number it represents, and would round up past it.
        ((exact - 1e-9).ceil().max(0.0) as usize).min(self.population)
    }
}

/// A generational genetic algorithm.
#[derive(Debug, Clone)]
pub struct GeneticAlgorithm {
    space: SearchSpace,
    settings: GeneticSettings,
    rng: DesignRng,
    tracker: CandidateTracker,
    log: EvaluationLog,
    /// First candidate of each config, by config key.
    config_candidates: BTreeMap<ConfigKey, u64>,
    /// Candidates of the current generation not yet requested.
    queue: VecDeque<Proposal>,
    /// Members of the last finished generation.
    members: Vec<u64>,
    /// Children of the current generation requested so far.
    children: Vec<u64>,
    /// Earlier candidates that join the current generation instead of a child with the same config.
    revisited: BTreeSet<u64>,
    generations: Vec<GenerationSummary>,
}

impl GeneticAlgorithm {
    /// Returns a genetic algorithm of `max_evaluations` evaluations over `space`, drawing from `seed`.
    ///
    /// Note that generation 0 is smaller than the population when the space has fewer configs.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpecError`] when [`GeneticSettings::check`] rejects `settings`.
    pub fn new(
        space: SearchSpace,
        settings: GeneticSettings,
        objective: Objective,
        max_evaluations: u64,
        seed: u64,
    ) -> Result<Self, SearchSpecError> {
        settings.check()?;
        let mut rng = DesignRng::new(seed);
        let mut proposed = BTreeSet::new();
        let mut queue = VecDeque::with_capacity(settings.population);
        for _ in 0..settings.population {
            if let ConfigDraw::New(genome, key) =
                draw_config(&space, &BTreeMap::new(), &proposed, || space.random_genome(&mut rng))
            {
                proposed.insert(key);
                queue.push_back(Proposal::first_evaluation(genome, CandidateOrigin::Random));
            }
        }
        Ok(Self {
            space,
            settings,
            rng,
            tracker: CandidateTracker::new(max_evaluations),
            log: EvaluationLog::new(objective),
            config_candidates: BTreeMap::new(),
            queue,
            members: Vec::new(),
            children: Vec::new(),
            revisited: BTreeSet::new(),
            generations: Vec::new(),
        })
    }

    /// Ends the current generation, whose members are the elites of the last generation, the new children and the
    /// candidates revisited instead of a child.
    fn finish_generation(&mut self) {
        let mut members = self.log.rank(&self.members);
        members.truncate(self.settings.elite_count);
        members.append(&mut self.children);
        let mut present: BTreeSet<u64> = members.iter().copied().collect();
        for candidate_id in std::mem::take(&mut self.revisited) {
            if present.insert(candidate_id) {
                members.push(candidate_id);
            }
        }
        let members = self.log.rank(&members);
        let mut fitness: Vec<f64> = members
            .iter()
            .map(|&candidate_id| self.log.objective(candidate_id))
            .collect();
        let (best, worst) = (fitness[0], fitness[fitness.len() - 1]);
        self.generations.push(GenerationSummary {
            generation: self.generations.len() as u64,
            best,
            median: Aggregate::Median.combine(&mut fitness).unwrap_or(worst),
            worst,
        });
        self.members = members;
        self.breed();
    }

    /// Queues the next generation, re-evaluations of the best members first and the children after them.
    ///
    /// A child whose draws all produce a known config re-evaluates the candidate the first draw matched in its place,
    /// or is left out when the match is another child of the generation.
    fn breed(&mut self) {
        let reevaluations = self.settings.reevaluation_count().min(self.members.len());
        let mut reevaluated = BTreeSet::new();
        for &candidate_id in &self.members[..reevaluations] {
            let record = self.log.get(candidate_id).expect("every member was evaluated");
            self.queue.push_back(Proposal::reevaluation(candidate_id, record));
            reevaluated.insert(candidate_id);
        }
        let fitness: Vec<(u64, f64)> = self
            .members
            .iter()
            .map(|&candidate_id| (candidate_id, self.log.objective(candidate_id)))
            .collect();
        let mut proposed = BTreeSet::new();
        for _ in self.settings.elite_count..self.settings.population {
            let (genome, origin) = self.child_genome(&fitness);
            let (space, rng, settings) = (&self.space, &mut self.rng, &self.settings);
            let draw = draw_config(space, &self.config_candidates, &proposed, || {
                space.mutate(&genome, rng, settings.mutation_rate, settings.mutation_scale)
            });
            match draw {
                ConfigDraw::New(genome, key) => {
                    proposed.insert(key);
                    self.queue.push_back(Proposal::first_evaluation(genome, origin));
                }
                ConfigDraw::Known { candidate_id } => {
                    if reevaluated.insert(candidate_id) {
                        let record = self
                            .log
                            .get(candidate_id)
                            .expect("a candidate with a config was evaluated");
                        self.queue.push_back(Proposal::reevaluation(candidate_id, record));
                    }
                    self.revisited.insert(candidate_id);
                }
                ConfigDraw::Proposed => {}
            }
        }
    }

    /// Returns the genome of a child before mutation, and its origin.
    ///
    /// The first parent wins a tournament. At the crossover rate, the winner of a second tournament crosses with it.
    fn child_genome(&mut self, fitness: &[(u64, f64)]) -> (Genome, CandidateOrigin) {
        let (goal, size) = (self.log.goal(), self.settings.tournament_size);
        let first_parent_id = tournament(&mut self.rng, fitness, goal, size);
        let first = self
            .log
            .get(first_parent_id)
            .expect("every member was evaluated")
            .genome();
        if self.rng.unit_f64() < self.settings.crossover_rate {
            let second_parent_id = tournament(&mut self.rng, fitness, goal, size);
            let second = self
                .log
                .get(second_parent_id)
                .expect("every member was evaluated")
                .genome();
            let origin = CandidateOrigin::Crossover {
                first_parent_id,
                second_parent_id,
            };
            (first.crossover(second, &mut self.rng), origin)
        } else {
            let origin = CandidateOrigin::Mutation {
                parent_id: first_parent_id,
            };
            (first.clone(), origin)
        }
    }
}

/// Returns the best of `size` members drawn from `fitness` with replacement, the earliest drawn among equals.
fn tournament(rng: &mut DesignRng, fitness: &[(u64, f64)], goal: Goal, size: usize) -> u64 {
    let mut winner = fitness[rng.index(fitness.len() as u64) as usize];
    for _ in 1..size {
        let entrant = fitness[rng.index(fitness.len() as u64) as usize];
        if goal.is_better(entrant.1, winner.1) {
            winner = entrant;
        }
    }
    winner.0
}

impl Searcher for GeneticAlgorithm {
    fn ask(&mut self, max: usize) -> Vec<Candidate> {
        let count = self.tracker.capacity(max).min(self.queue.len());
        let asked = self.tracker.issue(self.queue.drain(..count).collect());
        self.children.extend(
            asked
                .iter()
                .filter(|candidate| candidate.origin.reevaluated_id().is_none())
                .map(|candidate| candidate.id),
        );
        asked
    }

    fn tell(&mut self, evaluations: &[Evaluation]) {
        for evaluation in evaluations {
            if let Some((candidate, batch)) = self.tracker.settle(evaluation.candidate_id) {
                if candidate.origin.reevaluated_id().is_none() {
                    self.config_candidates
                        .entry(self.space.config_key(&candidate.genome))
                        .or_insert(candidate.id);
                }
                self.log.record(&candidate, batch, evaluation);
            }
        }
        let started = !self.children.is_empty() || !self.revisited.is_empty();
        if self.queue.is_empty() && !self.tracker.is_waiting() && started {
            self.finish_generation();
        }
    }

    fn is_done(&self) -> bool {
        self.tracker.is_done()
    }

    fn report(&self) -> SearchReport {
        SearchReport {
            ranking: self.log.ranking(),
            generations: self.generations.clone(),
            archive: Vec::new(),
        }
    }

    fn best(&self) -> Option<RankingEntry> {
        self.log.best()
    }

    fn ranking_entry(&self, candidate_id: u64) -> Option<RankingEntry> {
        self.log.ranking_entry(candidate_id)
    }

    fn generations(&self) -> &[GenerationSummary] {
        &self.generations
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{GeneticAlgorithm, GeneticSettings};
    use crate::explore::search::tests::support::{drive, level_space, noise, unit_space};
    use crate::explore::search::{Aggregate, Candidate, CandidateOrigin, Evaluation, Goal, Objective, Searcher as _};

    fn settings() -> GeneticSettings {
        GeneticSettings {
            population: 20,
            elite_count: 2,
            tournament_size: 3,
            crossover_rate: 0.9,
            mutation_rate: 0.3,
            mutation_scale: 0.15,
            reevaluate_fraction: 0.25,
        }
    }

    /// Returns the height of a dome peaking at 0.6 in every gene.
    fn height(genes: &[f64]) -> f64 {
        -genes.iter().map(|gene| (gene - 0.6) * (gene - 0.6)).sum::<f64>()
    }

    #[test]
    fn the_genetic_algorithm_improves_on_a_noisy_objective() {
        let objective = Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Median,
        };
        let mut search = GeneticAlgorithm::new(unit_space(3), settings(), objective, 600, 11).expect("valid settings");
        let noisy = |candidate: &Candidate, replicate| {
            vec![Some(
                height(candidate.genome.genes()) + 0.1 * noise(candidate.id, replicate),
            )]
        };
        let asked = drive(&mut search, 16, 3, noisy);
        let report = search.report();
        let first = &report.generations[0];
        let last = report.generations.last().expect("generations finished");
        assert!(
            report.generations.len() >= 20,
            "{} generations",
            report.generations.len()
        );
        assert!(
            last.median > first.median,
            "median {} against {} at first",
            last.median,
            first.median
        );
        assert!(
            last.best > first.best,
            "best {} against {} at first",
            last.best,
            first.best
        );
        let best = report.best().expect("the search scored candidates");
        let genes = asked[best.candidate_id as usize].genome.genes();
        assert!(height(genes) > -0.02, "best point {genes:?}");
        assert!(
            report.ranking.iter().any(|entry| entry.replicate_count > 3),
            "some members gathered more replicates than one evaluation gives"
        );
    }

    #[test]
    fn elites_survive_into_the_next_generation() {
        let objective = Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Minimize,
            aggregate: Aggregate::Mean,
        };
        let settings = GeneticSettings {
            reevaluate_fraction: 0.0,
            ..settings()
        };
        let mut search = GeneticAlgorithm::new(unit_space(2), settings, objective, 38, 4).expect("valid settings");
        let sum = |candidate: &Candidate, _| vec![Some(candidate.genome.genes().iter().sum())];
        drive(&mut search, 7, 1, sum);
        let report = search.report();
        assert_eq!(report.generations.len(), 2, "20 random members, then 18 children");
        assert!(report.generations[1].best <= report.generations[0].best);
        let elites = search.log.rank(&(0..20).collect::<Vec<u64>>());
        for elite in &elites[..2] {
            assert!(search.members.contains(elite), "elite {elite} survives");
        }
        assert_eq!(search.members.len(), 20);
    }

    #[test]
    fn no_two_first_evaluations_share_a_config() {
        let objective = Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Median,
        };
        let settings = GeneticSettings {
            population: 12,
            ..GeneticSettings::default()
        };
        // Five whole numbers by three options: 15 configs, fewer than the budget evaluates.
        let space = level_space();
        let mut search = GeneticAlgorithm::new(space.clone(), settings, objective, 150, 3).expect("valid settings");
        let noisy = |candidate: &Candidate, replicate| {
            vec![Some(candidate.genome.genes()[0] + 0.1 * noise(candidate.id, replicate))]
        };
        let asked = drive(&mut search, 5, 2, noisy);
        assert_eq!(
            asked.len(),
            150,
            "the search spends its budget once every config is known"
        );
        let mut keys = BTreeSet::new();
        for candidate in asked
            .iter()
            .filter(|candidate| candidate.origin.reevaluated_id().is_none())
        {
            assert!(
                keys.insert(space.config_key(&candidate.genome)),
                "candidate {} repeats the config of an earlier one",
                candidate.id
            );
        }
        assert_eq!(keys.len(), 15, "the search finds every config");
        assert!(
            asked[100..]
                .iter()
                .all(|candidate| matches!(candidate.origin, CandidateOrigin::Reevaluation { .. })),
            "once every config is known, the search re-evaluates"
        );
    }

    #[test]
    fn no_batch_spans_two_generations() {
        let objective = Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Mean,
        };
        // 20 members, then 5 re-evaluations and 18 children a generation, in batches of at most 7.
        let mut search = GeneticAlgorithm::new(unit_space(2), settings(), objective, 200, 6).expect("valid settings");
        let mut sizes = Vec::new();
        while !search.is_done() {
            let (queued, finished) = (search.queue.len(), search.generations.len());
            let batch = search.ask(7);
            let evaluations: Vec<Evaluation> = batch
                .iter()
                .map(|candidate| Evaluation {
                    candidate_id: candidate.id,
                    outputs: vec![vec![Some(height(candidate.genome.genes()))]],
                })
                .collect();
            search.tell(&evaluations);
            let drained = batch.len() == queued;
            assert_eq!(
                search.generations.len(),
                finished + usize::from(drained),
                "a generation ends with the batch that asks for its last candidate"
            );
            sizes.push(batch.len());
        }
        assert_eq!(sizes[..7], [7, 7, 6, 7, 7, 7, 2]);
    }

    #[test]
    fn reevaluation_counts_round_up() {
        let count = |reevaluate_fraction, population| {
            GeneticSettings {
                population,
                reevaluate_fraction,
                ..settings()
            }
            .reevaluation_count()
        };
        assert_eq!(
            count(0.1, 30),
            3,
            "0.1 * 30 is 3, though the product lands a hair above"
        );
        assert_eq!(count(0.25, 10), 3);
        assert_eq!(count(0.0, 10), 0);
        assert_eq!(count(1.0, 10), 10);
    }

    #[test]
    fn a_population_of_elites_alone_is_refused() {
        let settings = GeneticSettings {
            elite_count: 20,
            ..settings()
        };
        let objective = Objective {
            column: "Infected:max".to_owned(),
            goal: Goal::Minimize,
            aggregate: Aggregate::Mean,
        };
        assert!(GeneticAlgorithm::new(unit_space(1), settings, objective, 10, 1).is_err());
    }
}
