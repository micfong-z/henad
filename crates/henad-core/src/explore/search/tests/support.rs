//! A driver that runs a searcher over pure functions instead of model runs.

use crate::authoring::primitives::rng::mix_seed;
use crate::explore::design_rng::DesignRng;
use crate::explore::factor::{Factor, FactorDomain, FactorLevel, FactorSlot};
use crate::explore::search::genome::SearchSpace;
use crate::explore::search::{Aggregate, Candidate, Evaluation, Goal, Objective, Searcher};
use crate::fnv::fnv1a64;
use crate::params::ParamValue;

/// Maximum number of asks that one drive makes before it gives up on the searcher finishing.
const MAX_ASKS: usize = 100_000;

/// Runs `searcher` until it is done, requesting `batch_size` candidates at a time.
///
/// Each candidate runs `replicates` replicates, and the replicate with index `replicate` reports
/// `outputs(candidate, replicate)`. Returns every candidate requested, in order, so candidate `id` sits at index `id`.
///
/// # Panics
///
/// Panics when the searcher is still not done after [`MAX_ASKS`] asks.
pub fn drive(
    searcher: &mut dyn Searcher,
    batch_size: usize,
    replicates: u64,
    outputs: impl Fn(&Candidate, u64) -> Vec<Option<f64>>,
) -> Vec<Candidate> {
    drive_batches(searcher, batch_size, replicates, outputs).concat()
}

/// Runs `searcher` as [`drive`] does, and returns the candidates of each ask as a separate batch.
///
/// # Panics
///
/// Panics when the searcher is still not done after [`MAX_ASKS`] asks.
pub fn drive_batches(
    searcher: &mut dyn Searcher,
    batch_size: usize,
    replicates: u64,
    outputs: impl Fn(&Candidate, u64) -> Vec<Option<f64>>,
) -> Vec<Vec<Candidate>> {
    let mut asked = Vec::new();
    for _ in 0..MAX_ASKS {
        if searcher.is_done() {
            return asked;
        }
        let batch = searcher.ask(batch_size);
        if batch.is_empty() {
            return asked;
        }
        searcher.tell(&evaluate(&batch, replicates, &outputs));
        asked.push(batch);
    }
    panic!("the searcher was not done after {MAX_ASKS} asks");
}

/// Returns the evaluations of `batch`, each candidate with `replicates` replicates.
///
/// The replicate with index `replicate` reports `outputs(candidate, replicate)`, counting from the candidate's first
/// replicate.
pub fn evaluate(
    batch: &[Candidate],
    replicates: u64,
    outputs: impl Fn(&Candidate, u64) -> Vec<Option<f64>>,
) -> Vec<Evaluation> {
    batch
        .iter()
        .map(|candidate| Evaluation {
            candidate_id: candidate.id,
            outputs: (0..replicates)
                .map(|index| outputs(candidate, candidate.replicate_offset + index))
                .collect(),
        })
        .collect()
}

/// Returns an objective over the column `Infected:max` with `goal` and `aggregate`.
pub fn objective(goal: Goal, aggregate: Aggregate) -> Objective {
    Objective {
        column: "Infected:max".to_owned(),
        goal,
        aggregate,
    }
}

/// Returns a hash of every candidate in `asked`: its id, its first replicate, its genes and its origin.
pub fn trajectory_hash(asked: &[Candidate]) -> u64 {
    let mut bytes = Vec::new();
    for candidate in asked {
        bytes.extend(candidate.id.to_le_bytes());
        bytes.extend(candidate.replicate_offset.to_le_bytes());
        for gene in candidate.genome.genes() {
            bytes.extend(gene.to_bits().to_le_bytes());
        }
        bytes.extend(format!("{:?}", candidate.origin).bytes());
    }
    fnv1a64(&bytes)
}

/// Returns uniform noise in `[-0.5, 0.5)` for replicate `replicate` of candidate `candidate_id`.
pub fn noise(candidate_id: u64, replicate: u64) -> f64 {
    DesignRng::new(mix_seed(candidate_id) ^ replicate).unit_f64() - 0.5
}

/// Returns a space of `genes` continuous factors from 0 to 1, over parameter slots `0..genes`.
pub fn unit_space(genes: usize) -> SearchSpace {
    let factors = (0..genes)
        .map(|index| Factor {
            slot: FactorSlot::Param(index),
            domain: FactorDomain::Continuous { min: 0.0, max: 1.0 },
        })
        .collect();
    SearchSpace::new(factors).expect("at least one gene")
}

/// Returns a space of 15 configs: whole numbers 1 to 5 over parameter slot 0, and three options over slot 1.
pub fn level_space() -> SearchSpace {
    let options = (0..3)
        .map(|option| FactorLevel::Param(ParamValue::Choice(option)))
        .collect();
    SearchSpace::new(vec![
        Factor {
            slot: FactorSlot::Param(0),
            domain: FactorDomain::WholeNumbers { min: 1, max: 5 },
        },
        Factor {
            slot: FactorSlot::Param(1),
            domain: FactorDomain::Levels(options),
        },
    ])
    .expect("two factors with levels")
}
