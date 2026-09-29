//! Genomes of a search, and the space that decodes them into configs.
//!
//! A genome holds one gene per factor of its space, each a fraction from 0 to 1. A continuous gene maps linearly
//! onto its range. A whole-number gene over `m` values takes the value at position `floor(gene * m)`, and so does a
//! categorical gene over `m` listed levels.

use std::collections::BTreeSet;
use std::fmt;

use crate::explore::design::{continuous_level, continuous_value, whole_number_level};
use crate::explore::design_rng::DesignRng;
use crate::explore::factor::{Factor, FactorDomain, FactorError, FactorLevel, FactorSpec, FactorTarget};
use crate::explore::plan::Config;
use crate::explore::spec::ActionSpec;
use crate::params::{ParamDescriptor, ParamValue};

/// A point in a search space, one gene per factor, each from 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Genome {
    genes: Vec<f64>,
}

impl Genome {
    pub fn genes(&self) -> &[f64] {
        &self.genes
    }

    /// Returns a child that takes each gene from `self` or `other` with even odds.
    ///
    /// # Panics
    ///
    /// Panics when the two genomes differ in length.
    pub fn crossover(&self, other: &Self, rng: &mut DesignRng) -> Self {
        assert_eq!(self.genes.len(), other.genes.len(), "both parents come from one space");
        let genes = self
            .genes
            .iter()
            .zip(&other.genes)
            .map(|(&first, &second)| if rng.unit_f64() < 0.5 { first } else { second })
            .collect();
        Self { genes }
    }
}

/// Key of the config a genome decodes to, one entry per factor.
///
/// Two genomes of one space have equal keys exactly when they decode to the same config.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigKey(Vec<u64>);

/// Factors a search varies, one gene each.
///
/// A factor over listed levels is a categorical gene. Any other factor is an ordered gene.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSpace {
    factors: Vec<Factor>,
}

impl SearchSpace {
    /// Returns the space over `factors`.
    ///
    /// A level listed twice is kept once, at its first place.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpaceError`] for no factors, or a factor with an empty list of levels.
    pub fn new(mut factors: Vec<Factor>) -> Result<Self, SearchSpaceError> {
        if factors.is_empty() {
            return Err(SearchSpaceError::NoFactors);
        }
        if let Some(factor_index) = factors
            .iter()
            .position(|factor| factor.levels().is_some_and(<[FactorLevel]>::is_empty))
        {
            return Err(SearchSpaceError::NoLevels { factor_index });
        }
        for factor in &mut factors {
            if let FactorDomain::Levels(levels) = &mut factor.domain {
                let mut seen = BTreeSet::new();
                levels.retain(|level| seen.insert(level_identity(level)));
            }
        }
        Ok(Self { factors })
    }

    /// Resolves `specs` against `params` and `actions` as a sampled design would, and returns their space.
    ///
    /// A range with no step spans every value from its `min` to its `max`. `fixed` lists the parameter values every
    /// config shares, as `(id, value)` pairs.
    ///
    /// # Errors
    ///
    /// Returns [`SearchSpaceError`] for no factors, a factor [`FactorSpec::resolve_sampled`] refuses, a target varied
    /// twice, or a parameter both fixed and varied.
    pub fn resolve(
        specs: &[FactorSpec],
        params: &[ParamDescriptor],
        actions: &[ActionSpec],
        fixed: &[(String, String)],
    ) -> Result<Self, SearchSpaceError> {
        let mut factors = Vec::with_capacity(specs.len());
        for (index, spec) in specs.iter().enumerate() {
            if specs[..index].iter().any(|earlier| earlier.target == spec.target) {
                return Err(SearchSpaceError::VariedTwice {
                    target: spec.target.clone(),
                });
            }
            if let FactorTarget::Param(id) = &spec.target
                && fixed.iter().any(|(fixed_id, _)| fixed_id == id)
            {
                return Err(SearchSpaceError::FixedAndVaried { id: id.clone() });
            }
            factors.push(
                spec.resolve_sampled(params, actions)
                    .map_err(SearchSpaceError::Factor)?,
            );
        }
        Self::new(factors)
    }

    /// Factors of the space, in gene order.
    pub fn factors(&self) -> &[Factor] {
        &self.factors
    }

    /// Returns a genome with every gene drawn uniformly.
    pub fn random_genome(&self, rng: &mut DesignRng) -> Genome {
        Genome {
            genes: self.factors.iter().map(|_| rng.unit_f64()).collect(),
        }
    }

    /// Returns a copy of `genome` in which each gene changes with probability `rate`.
    ///
    /// An ordered gene moves by `scale * (r1 + r2 - 1)` for two uniform draws, reflected back into `[0, 1]` at either
    /// end. A categorical gene is drawn again.
    ///
    /// # Panics
    ///
    /// Panics when `genome` has a gene count other than the space's.
    pub fn mutate(&self, genome: &Genome, rng: &mut DesignRng, rate: f64, scale: f64) -> Genome {
        assert_eq!(
            genome.genes.len(),
            self.factors.len(),
            "the genome comes from this space"
        );
        let genes = self
            .factors
            .iter()
            .zip(&genome.genes)
            .map(|(factor, &gene)| {
                if rng.unit_f64() >= rate {
                    gene
                } else if factor.levels().is_some() {
                    rng.unit_f64()
                } else {
                    reflect(gene + scale * rng.triangular())
                }
            })
            .collect();
        Genome { genes }
    }

    /// Returns `base` with the level of every gene of `genome` written into it.
    ///
    /// # Panics
    ///
    /// Panics when `genome` has a gene count other than the space's, or a factor's slot is past the end of `base`.
    pub fn decode(&self, genome: &Genome, base: &Config) -> Config {
        assert_eq!(
            genome.genes.len(),
            self.factors.len(),
            "the genome comes from this space"
        );
        let mut config = base.clone();
        for (factor, &gene) in self.factors.iter().zip(&genome.genes) {
            factor.apply(&level(factor, gene), &mut config);
        }
        config
    }

    /// Returns the key of the config `genome` decodes to.
    ///
    /// # Panics
    ///
    /// Panics when `genome` has a gene count other than the space's.
    pub fn config_key(&self, genome: &Genome) -> ConfigKey {
        assert_eq!(
            genome.genes.len(),
            self.factors.len(),
            "the genome comes from this space"
        );
        let entries = self
            .factors
            .iter()
            .zip(&genome.genes)
            .map(|(factor, &gene)| match &factor.domain {
                &FactorDomain::Continuous { min, max } => {
                    u64::from(continuous_value(min, max, gene.clamp(0.0, 1.0)).to_bits())
                }
                &FactorDomain::WholeNumbers { min, max } => level_index(gene, u128::from(max - min) + 1) as u64,
                FactorDomain::Levels(levels) => level_index(gene, levels.len() as u128) as u64,
            })
            .collect();
        ConfigKey(entries)
    }
}

/// Returns the level of `factor` at gene `gene`.
fn level(factor: &Factor, gene: f64) -> FactorLevel {
    match &factor.domain {
        &FactorDomain::Continuous { min, max } => continuous_level(min, max, gene.clamp(0.0, 1.0)),
        &FactorDomain::WholeNumbers { min, max } => {
            let offset = level_index(gene, u128::from(max - min) + 1) as u64;
            whole_number_level(factor.slot, min + offset)
        }
        FactorDomain::Levels(levels) => levels[level_index(gene, levels.len() as u128) as usize].clone(),
    }
}

/// Returns a key two levels share when they are equal, with zero and negative zero as one.
fn level_identity(level: &FactorLevel) -> (u8, u64) {
    match level {
        FactorLevel::Param(ParamValue::F32(value)) => (0, u64::from(if *value == 0.0 { 0 } else { value.to_bits() })),
        FactorLevel::Param(ParamValue::U32(value)) => (1, u64::from(*value)),
        FactorLevel::Param(ParamValue::Bool(value)) => (2, u64::from(*value)),
        FactorLevel::Param(ParamValue::Choice(index)) => (3, *index as u64),
        FactorLevel::Tick(tick) => (4, *tick),
    }
}

/// Returns `floor(gene * count)`, kept from 0 to `count - 1`.
fn level_index(gene: f64, count: u128) -> u128 {
    // The cast saturates, taking a negative product to 0.
    ((gene * count as f64) as u128).min(count - 1)
}

/// Returns `value` folded back into `[0, 1]`, as a reflection at each end would.
fn reflect(value: f64) -> f64 {
    let folded = value.rem_euclid(2.0);
    if folded > 1.0 { 2.0 - folded } else { folded }
}

/// A search space that cannot be built.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchSpaceError {
    /// A space with no factors.
    NoFactors,
    /// Factor `factor_index`, a list with no levels.
    NoLevels { factor_index: usize },
    /// A factor refused for the reason inside.
    Factor(FactorError),
    /// A target the space varies twice.
    VariedTwice { target: FactorTarget },
    /// Parameter `id`, varied by the search and fixed as well.
    FixedAndVaried { id: String },
}

impl fmt::Display for SearchSpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFactors => write!(f, "search space has no factors"),
            Self::NoLevels { factor_index } => write!(f, "factor {factor_index} of the search space has no levels"),
            Self::Factor(_) => write!(f, "search space"),
            Self::VariedTwice { target } => write!(f, "search space varies {target} twice"),
            Self::FixedAndVaried { id } => write!(f, "parameter '{id}' is both fixed and varied by the search"),
        }
    }
}

impl std::error::Error for SearchSpaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Factor(error) => Some(error),
            Self::NoFactors | Self::NoLevels { .. } | Self::VariedTwice { .. } | Self::FixedAndVaried { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Genome, SearchSpace, SearchSpaceError, reflect};
    use crate::explore::design_rng::DesignRng;
    use crate::explore::factor::{Factor, FactorDomain, FactorLevel, FactorSlot, FactorSpec, FactorTarget, LevelSpec};
    use crate::explore::plan::Config;
    use crate::explore::spec::ActionSpec;
    use crate::explore::value::check_value;
    use crate::helpers::{choice_param, f32_param, u32_param};
    use crate::params::{ParamDescriptor, ParamValue};

    const SHAPES: &[&str] = &["ring", "star", "grid"];

    fn params() -> Vec<ParamDescriptor> {
        vec![
            f32_param("rate", "Rate", 0.5, 0.0, 1.0, Some(0.01)),
            u32_param("size", "Size", 10, 1, 100),
            choice_param("shape", "Shape", SHAPES, 0),
        ]
    }

    fn actions() -> Vec<ActionSpec> {
        vec![ActionSpec::new("outbreak", 50)]
    }

    fn range(min: f64, max: f64) -> LevelSpec {
        LevelSpec::Range { min, max, step: None }
    }

    /// Returns a space over every kind of gene: continuous, whole numbers, ticks and a choice.
    fn space() -> SearchSpace {
        let specs = [
            FactorSpec::param("rate", range(0.05, 0.9)),
            FactorSpec::param("size", range(3.0, 17.0)),
            FactorSpec::action("outbreak", range(0.0, 400.0)),
            FactorSpec::param("shape", LevelSpec::All),
        ];
        SearchSpace::resolve(&specs, &params(), &actions(), &[]).expect("every factor resolves")
    }

    fn base() -> Config {
        Config {
            block: 0,
            params: vec![ParamValue::F32(0.5), ParamValue::U32(10), ParamValue::Choice(0)],
            action_ticks: vec![50],
        }
    }

    fn genome(genes: &[f64]) -> Genome {
        Genome { genes: genes.to_vec() }
    }

    #[test]
    fn a_decoded_point_respects_every_bound() {
        let space = space();
        let params = params();
        let mut rng = DesignRng::new(4);
        let mut genomes = vec![genome(&[0.0; 4]), genome(&[1.0; 4]), genome(&[0.999_999_999_999; 4])];
        for _ in 0..1000 {
            let random = space.random_genome(&mut rng);
            genomes.push(space.mutate(&random, &mut rng, 1.0, 0.5));
            genomes.push(random);
        }
        for genome in &genomes {
            let config = space.decode(genome, &base());
            for (descriptor, value) in params.iter().zip(&config.params) {
                assert!(
                    check_value(&descriptor.kind, value).is_ok(),
                    "{value:?} for {}",
                    descriptor.id
                );
            }
            let ParamValue::F32(rate) = config.params[0] else {
                panic!("rate is an f32");
            };
            assert!((0.05..=0.9).contains(&rate), "rate {rate}");
            let ParamValue::U32(size) = config.params[1] else {
                panic!("size is a u32");
            };
            assert!((3..=17).contains(&size), "size {size}");
            assert!(config.action_ticks[0] <= 400, "tick {}", config.action_ticks[0]);
        }
        assert_eq!(
            space.decode(&genome(&[0.0; 4]), &base()).params[0],
            ParamValue::F32(0.05)
        );
        assert_eq!(
            space.decode(&genome(&[1.0; 4]), &base()).params[0],
            ParamValue::F32(0.9)
        );
    }

    #[test]
    fn integer_and_categorical_genes_decode_to_valid_levels() {
        let space = space();
        let decode = |gene: f64| space.decode(&genome(&[0.5, gene, gene, gene]), &base());
        let ends = [decode(0.0), decode(1.0), decode(0.999_999_999_999)];
        assert_eq!(ends[0].params[1], ParamValue::U32(3));
        assert_eq!(ends[0].action_ticks[0], 0);
        assert_eq!(ends[0].params[2], ParamValue::Choice(0));
        for end in &ends[1..] {
            assert_eq!(end.params[1], ParamValue::U32(17));
            assert_eq!(end.action_ticks[0], 400);
            assert_eq!(end.params[2], ParamValue::Choice(2));
        }
        let mut sizes = BTreeSet::new();
        let mut ticks = BTreeSet::new();
        let mut shapes = BTreeSet::new();
        for step in 0..=4000 {
            let config = decode(f64::from(step) / 4000.0);
            let ParamValue::U32(size) = config.params[1] else {
                panic!("size is a u32");
            };
            let ParamValue::Choice(shape) = config.params[2] else {
                panic!("shape is a choice");
            };
            sizes.insert(size);
            ticks.insert(config.action_ticks[0]);
            shapes.insert(shape);
        }
        assert_eq!(sizes, (3..=17).collect(), "every whole number is reachable");
        assert_eq!(ticks, (0..=400).collect(), "every tick is reachable");
        assert_eq!(shapes, (0..3).collect(), "every option is reachable");
    }

    #[test]
    fn genomes_share_a_config_key_exactly_when_they_share_a_config() {
        let space = space();
        let first = genome(&[0.3, 0.50, 0.5, 0.1]);
        let same = [
            genome(&[0.3 + 1e-12, 0.52, 0.5001, 0.2]),
            genome(&[0.3, 0.51, 0.5, 0.3]),
        ];
        for other in &same {
            assert_eq!(space.decode(other, &base()), space.decode(&first, &base()));
            assert_eq!(space.config_key(other), space.config_key(&first), "{other:?}");
        }
        let different = [
            genome(&[0.31, 0.5, 0.5, 0.1]),
            genome(&[0.3, 0.6, 0.5, 0.1]),
            genome(&[0.3, 0.5, 0.6, 0.1]),
            genome(&[0.3, 0.5, 0.5, 0.9]),
        ];
        for other in &different {
            assert_ne!(space.decode(other, &base()), space.decode(&first, &base()));
            assert_ne!(space.config_key(other), space.config_key(&first), "{other:?}");
        }
    }

    #[test]
    fn mutation_keeps_every_gene_in_the_unit_interval() {
        let space = space();
        let mut rng = DesignRng::new(8);
        let mut genome = space.random_genome(&mut rng);
        for _ in 0..10_000 {
            genome = space.mutate(&genome, &mut rng, 0.7, 0.8);
            assert!(
                genome.genes().iter().all(|gene| (0.0..=1.0).contains(gene)),
                "{genome:?}"
            );
        }
        for (value, folded) in [(-0.25, 0.25), (1.25, 0.75), (0.5, 0.5), (2.5, 0.5), (-1.75, 0.25)] {
            assert!(
                (reflect(value) - folded).abs() < 1e-12,
                "{value} folds to {}",
                reflect(value)
            );
        }
    }

    #[test]
    fn a_zero_rate_changes_no_gene() {
        let space = space();
        let mut rng = DesignRng::new(2);
        let genome = space.random_genome(&mut rng);
        assert_eq!(space.mutate(&genome, &mut rng, 0.0, 0.5), genome);
    }

    #[test]
    fn crossover_takes_each_gene_from_a_parent() {
        let mut rng = DesignRng::new(6);
        let first = genome(&[0.1; 64]);
        let second = genome(&[0.9; 64]);
        let child = first.crossover(&second, &mut rng);
        assert!(child.genes().iter().all(|&gene| gene == 0.1 || gene == 0.9));
        assert!(
            child.genes().contains(&0.1) && child.genes().contains(&0.9),
            "{child:?}"
        );
    }

    #[test]
    fn a_search_space_refuses_repeated_and_fixed_targets() {
        let rate = FactorSpec::param("rate", range(0.1, 0.2));
        assert_eq!(
            SearchSpace::resolve(&[rate.clone(), rate.clone()], &params(), &actions(), &[]),
            Err(SearchSpaceError::VariedTwice {
                target: FactorTarget::Param("rate".to_owned()),
            })
        );
        let fixed = [("rate".to_owned(), "0.3".to_owned())];
        assert_eq!(
            SearchSpace::resolve(std::slice::from_ref(&rate), &params(), &actions(), &fixed),
            Err(SearchSpaceError::FixedAndVaried { id: "rate".to_owned() })
        );
        assert_eq!(
            SearchSpace::resolve(&[], &params(), &actions(), &[]),
            Err(SearchSpaceError::NoFactors)
        );
    }

    #[test]
    fn listed_levels_are_categorical() {
        let specs = [FactorSpec::param(
            "size",
            LevelSpec::Values(vec!["4".to_owned(), "8".to_owned()]),
        )];
        let space = SearchSpace::resolve(&specs, &params(), &actions(), &[]).expect("listed sizes resolve");
        assert_eq!(
            space.factors()[0].levels(),
            Some(
                &[
                    FactorLevel::Param(ParamValue::U32(4)),
                    FactorLevel::Param(ParamValue::U32(8))
                ][..]
            )
        );
        assert_eq!(space.decode(&genome(&[0.6]), &base()).params[1], ParamValue::U32(8));
    }

    #[test]
    fn a_repeated_level_is_kept_once() {
        // "2" names the third shape by its index.
        let shapes = ["star", "ring", "star", "2", "grid"].map(str::to_owned).to_vec();
        let specs = [FactorSpec::param("shape", LevelSpec::Values(shapes))];
        let space = SearchSpace::resolve(&specs, &params(), &actions(), &[]).expect("listed shapes resolve");
        let choices = [1, 0, 2].map(|index| FactorLevel::Param(ParamValue::Choice(index)));
        assert_eq!(space.factors()[0].levels(), Some(&choices[..]), "first places kept");
        for (first, second) in [(0.1, 0.3), (0.4, 0.6), (0.7, 0.9)] {
            assert_eq!(
                space.config_key(&genome(&[first])),
                space.config_key(&genome(&[second]))
            );
        }
        assert_ne!(space.config_key(&genome(&[0.1])), space.config_key(&genome(&[0.9])));

        // Steps below the spacing of f32 values near 0.5 round several levels to one value.
        let step = LevelSpec::Range {
            min: 0.5,
            max: 0.500_000_05,
            step: Some(1e-8),
        };
        let space = SearchSpace::resolve(&[FactorSpec::param("rate", step)], &params(), &actions(), &[])
            .expect("the rates resolve");
        let rates = [0.5, f32::from_bits(0.5_f32.to_bits() + 1)].map(|rate| FactorLevel::Param(ParamValue::F32(rate)));
        assert_eq!(space.factors()[0].levels(), Some(&rates[..]));

        let signed_zeros = [0.0, -0.0, 0.25].map(|rate| FactorLevel::Param(ParamValue::F32(rate)));
        let space = SearchSpace::new(vec![Factor {
            slot: FactorSlot::Param(0),
            domain: FactorDomain::Levels(signed_zeros.to_vec()),
        }])
        .expect("three levels");
        assert_eq!(
            space.factors()[0].levels().map(<[FactorLevel]>::len),
            Some(2),
            "zero is one level"
        );
    }
}
