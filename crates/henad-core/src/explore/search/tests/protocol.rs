//! Tests of the ask and tell protocol every searcher follows.

use std::collections::BTreeMap;

use crate::explore::search::genetic::{GeneticSettings, MAX_POPULATION, MAX_TOURNAMENT_SIZE};
use crate::explore::search::hill_climb::HillClimbSettings;
use crate::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use crate::explore::search::tests::support::{drive, noise, unit_space};
use crate::explore::search::{Aggregate, Candidate, Goal, Objective, SearchAlgorithm, SearchSpec, SearchSpecError};

fn objective() -> Objective {
    Objective {
        column: "Infected:max".to_owned(),
        goal: Goal::Minimize,
        aggregate: Aggregate::Median,
    }
}

fn pattern_settings() -> PatternSpaceSettings {
    let axis = |column: &str| PatternAxis::bounded(column, 0.0, 2.0, 10);
    PatternSpaceSettings {
        initial_samples: 8,
        ..PatternSpaceSettings::new(axis("Infected:max"), axis("Infected:argmax"))
    }
}

/// Returns the settings of [`pattern_settings`] with an automatic range on each axis.
fn automatic_pattern_settings() -> PatternSpaceSettings {
    let axis = |column: &str| PatternAxis::automatic(column, 10);
    PatternSpaceSettings {
        initial_samples: 8,
        ..PatternSpaceSettings::new(axis("Infected:max"), axis("Infected:argmax"))
    }
}

fn genetic_settings() -> GeneticSettings {
    GeneticSettings {
        population: 10,
        ..GeneticSettings::default()
    }
}

/// Returns a spec for `algorithm` with a budget of `max_evaluations`, and an objective where the algorithm takes one.
fn spec(algorithm: SearchAlgorithm, max_evaluations: u64) -> SearchSpec {
    let objective = match algorithm {
        SearchAlgorithm::PatternSpaceExploration(_) => None,
        SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => Some(objective()),
    };
    SearchSpec {
        algorithm,
        max_evaluations,
        batch_size: 5,
        objective,
        space: Vec::new(),
    }
}

fn every_algorithm() -> [SearchAlgorithm; 5] {
    [
        SearchAlgorithm::Random,
        SearchAlgorithm::HillClimb(HillClimbSettings {
            reevaluate: true,
            ..HillClimbSettings::default()
        }),
        SearchAlgorithm::Genetic(genetic_settings()),
        SearchAlgorithm::PatternSpaceExploration(pattern_settings()),
        SearchAlgorithm::PatternSpaceExploration(automatic_pattern_settings()),
    ]
}

/// Returns noisy outputs that both an objective and a pattern grid can read.
fn outputs(candidate: &Candidate, replicate: u64) -> Vec<Option<f64>> {
    let genes = candidate.genome.genes();
    vec![
        Some(genes[0] + genes[1] + 0.1 * noise(candidate.id, replicate)),
        Some(2.0 * genes[1]),
    ]
}

#[test]
fn no_searcher_asks_past_its_budget() {
    for algorithm in every_algorithm() {
        let name = algorithm.as_str();
        let spec = spec(algorithm, 37);
        let mut searcher = spec.searcher(&unit_space(2), 42).expect("a valid spec");
        let asked = drive(searcher.as_mut(), spec.batch_size, 2, outputs);
        let ids: Vec<u64> = asked.iter().map(|candidate| candidate.id).collect();
        assert_eq!(
            ids,
            (0..37).collect::<Vec<_>>(),
            "{name} numbers its candidates in ask order"
        );
        assert!(searcher.is_done(), "{name} is done");
        assert!(searcher.ask(5).is_empty(), "{name} asks for nothing past its budget");
    }
}

#[test]
fn every_searcher_repeats_its_trajectory_from_its_seed() {
    for algorithm in every_algorithm() {
        let name = algorithm.as_str();
        let spec = spec(algorithm, 60);
        let run = |root| {
            let mut searcher = spec.searcher(&unit_space(2), root).expect("a valid spec");
            let asked = drive(searcher.as_mut(), spec.batch_size, 3, outputs);
            (asked, searcher.report())
        };
        let first = run(1);
        assert_eq!(run(1), first, "{name} repeats itself from one root");
        assert_ne!(run(2).0, first.0, "{name} draws differently from another root");
    }
}

#[test]
fn the_standing_of_a_searcher_matches_its_report() {
    for algorithm in every_algorithm() {
        let name = algorithm.as_str();
        let spec = spec(algorithm, 45);
        let mut searcher = spec.searcher(&unit_space(2), 8).expect("a valid spec");
        drive(searcher.as_mut(), spec.batch_size, 2, outputs);
        let report = searcher.report();
        assert_eq!(searcher.best().as_ref(), report.best(), "{name}");
        for entry in &report.ranking {
            assert_eq!(
                searcher.ranking_entry(entry.candidate_id).as_ref(),
                Some(entry),
                "{name}"
            );
        }
        for entry in &report.archive {
            assert_eq!(searcher.archive_entry(entry.cell).as_ref(), Some(entry), "{name}");
        }
        assert_eq!(searcher.filled_cells(), report.archive.len() as u64, "{name}");
        assert_eq!(searcher.generations(), report.generations.as_slice(), "{name}");
    }
}

#[test]
fn reevaluation_uses_fresh_replicate_indices() {
    let replicates = 3;
    let algorithms = [
        SearchAlgorithm::Genetic(genetic_settings()),
        SearchAlgorithm::HillClimb(HillClimbSettings {
            reevaluate: true,
            patience: 50,
            ..HillClimbSettings::default()
        }),
    ];
    for algorithm in algorithms {
        let name = algorithm.as_str();
        let spec = spec(algorithm, 120);
        let mut searcher = spec.searcher(&unit_space(2), 5).expect("a valid spec");
        let asked = drive(searcher.as_mut(), spec.batch_size, replicates, outputs);
        let mut offsets: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for candidate in &asked {
            let candidate_id = candidate.origin.reevaluated_id().unwrap_or(candidate.id);
            offsets
                .entry(candidate_id)
                .or_default()
                .push(candidate.replicate_offset);
        }
        for (candidate_id, offsets) in &offsets {
            let expected: Vec<u64> = (0..offsets.len() as u64).map(|index| index * replicates).collect();
            assert_eq!(
                offsets, &expected,
                "{name}: every evaluation of {candidate_id} starts past the last"
            );
        }
        assert!(
            offsets.values().any(|offsets| offsets.len() >= 3),
            "{name} re-evaluates some candidate more than once"
        );
        let report = searcher.report();
        for entry in &report.ranking {
            assert_eq!(
                entry.replicate_count,
                entry.evaluations * replicates,
                "{name}: {entry:?}"
            );
        }
    }
}

#[test]
fn a_search_spec_checks_its_objective_and_settings() {
    let mut random = spec(SearchAlgorithm::Random, 10);
    assert_eq!(random.check(), Ok(()));
    assert_eq!(random.watched_columns(), ["Infected:max"]);
    random.objective = None;
    assert_eq!(
        random.check(),
        Err(SearchSpecError::MissingObjective { algorithm: "random" })
    );

    let mut pattern = spec(SearchAlgorithm::PatternSpaceExploration(pattern_settings()), 10);
    assert_eq!(pattern.watched_columns(), ["Infected:max", "Infected:argmax"]);
    pattern.objective = Some(objective());
    assert_eq!(pattern.check(), Err(SearchSpecError::UnusedObjective));

    let empty = spec(SearchAlgorithm::Random, 0);
    assert_eq!(empty.check(), Err(SearchSpecError::NoEvaluations));

    let genetic = spec(
        SearchAlgorithm::Genetic(GeneticSettings {
            mutation_rate: 1.5,
            ..genetic_settings()
        }),
        10,
    );
    let Err(SearchSpecError::Setting { key, .. }) = genetic.check() else {
        panic!("a rate above 1 is refused");
    };
    assert_eq!(key, "genetic.mutation_rate");
    assert!(genetic.searcher(&unit_space(1), 0).is_err());

    let largest = GeneticSettings {
        population: MAX_POPULATION,
        tournament_size: MAX_TOURNAMENT_SIZE,
        ..genetic_settings()
    };
    assert_eq!(spec(SearchAlgorithm::Genetic(largest), 10).check(), Ok(()));
    for (settings, refused) in [
        (
            GeneticSettings {
                population: MAX_POPULATION + 1,
                ..genetic_settings()
            },
            "genetic.population",
        ),
        (
            GeneticSettings {
                population: usize::MAX,
                ..genetic_settings()
            },
            "genetic.population",
        ),
        (
            GeneticSettings {
                tournament_size: MAX_TOURNAMENT_SIZE + 1,
                ..genetic_settings()
            },
            "genetic.tournament_size",
        ),
    ] {
        let genetic = spec(SearchAlgorithm::Genetic(settings), 10);
        let Err(SearchSpecError::Setting { key, .. }) = genetic.check() else {
            panic!("{settings:?} is refused");
        };
        assert_eq!(key, refused);
        assert!(
            genetic.searcher(&unit_space(1), 0).is_err(),
            "{key} is refused before the searcher allocates"
        );
    }
}

#[test]
fn goals_and_aggregates_read_back_their_names() {
    for goal in [Goal::Minimize, Goal::Maximize] {
        assert_eq!(goal.as_str().parse::<Goal>(), Ok(goal));
    }
    for aggregate in [Aggregate::Mean, Aggregate::Median] {
        assert_eq!(aggregate.as_str().parse::<Aggregate>(), Ok(aggregate));
    }
    assert!("largest".parse::<Goal>().is_err());
    assert!(Goal::Maximize.is_better(2.0, 1.0));
    assert!(!Goal::Minimize.is_better(1.0, 1.0));
}
