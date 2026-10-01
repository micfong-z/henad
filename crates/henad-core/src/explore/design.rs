//! Designs that combine the factors of a block into configs.
//!
//! A factorial or zip design combines listed levels. A random or Latin hypercube design samples, drawing each level
//! from a design seed, and a table design reads every config from a table of values.

use std::fmt;

use crate::explore::design_rng::DesignRng;
use crate::explore::factor::{Factor, FactorDomain, FactorLevel, FactorSlot};
use crate::explore::plan::Config;
use crate::params::ParamValue;

/// Most configs one block can have.
pub const MAX_CONFIGS: usize = 1 << 24;

/// Rule that combines the factors of a block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DesignKind {
    /// Every combination of levels. The first factor varies slowest and the last fastest.
    #[default]
    Factorial,
    /// Levels paired by position, so config `i` takes level `i` of every factor.
    Zip,
    /// `samples` configs, each factor's value drawn on its own from its whole domain.
    Random { samples: usize },
    /// `samples` configs in a Latin hypercube, each factor's domain split into `samples` strata.
    ///
    /// Every stratum of a continuous factor holds one sample, at a random point inside it. A stratum of a factor
    /// with `m` levels takes the lowest level it covers, and each level is then taken `samples / m` times, rounded
    /// down or up. The levels such a factor takes therefore depend on `samples` alone, and the design seed decides
    /// only which config takes each one.
    LatinHypercube { samples: usize },
    /// One config per row of a comma-separated table, whose header names parameter ids or `action.<name>`.
    ///
    /// [`crate::explore::design_csv`] reads the table.
    Table { text: String },
}

impl DesignKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Factorial => "factorial",
            Self::Zip => "zip",
            Self::Random { .. } => "random",
            Self::LatinHypercube { .. } => "lhs",
            Self::Table { .. } => "table",
        }
    }

    /// Returns whether the design draws its configs from a design seed.
    pub fn is_sampled(&self) -> bool {
        matches!(self, Self::Random { .. } | Self::LatinHypercube { .. })
    }
}

/// Resolved factors combined under one design.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub design: DesignKind,
    /// Factors of the block, or the columns of its table.
    pub factors: Vec<Factor>,
    /// Seed of a sampled design's draws. Other designs draw nothing.
    pub design_seed: u64,
}

/// A block whose design cannot combine its factors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignError {
    /// A zip over factors with different numbers of levels, given in factor order.
    UnequalLengths { lengths: Vec<usize> },
    /// A design with more than [`MAX_CONFIGS`] configs.
    TooManyConfigs,
    /// A random or Latin hypercube design of 0 samples.
    NoSamples,
    /// A random or Latin hypercube design with no factor to draw.
    NoFactors,
    /// Factor `factor_index` of a factorial or zip design, a range with no listed levels.
    UnlistedLevels { factor_index: usize },
    /// Factor `factor_index`, a list of no levels.
    NoLevels { factor_index: usize },
}

impl fmt::Display for DesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnequalLengths { lengths } => {
                let lengths: Vec<String> = lengths.iter().map(ToString::to_string).collect();
                write!(
                    f,
                    "zip needs every factor to have the same number of levels, got {}",
                    lengths.join(", ")
                )
            }
            Self::TooManyConfigs => write!(f, "design has more than {MAX_CONFIGS} configs"),
            Self::NoSamples => write!(f, "a random or Latin hypercube design needs at least 1 sample"),
            Self::NoFactors => write!(f, "a random or Latin hypercube design needs at least 1 factor"),
            Self::UnlistedLevels { factor_index } => {
                write!(
                    f,
                    "range of factor {factor_index} needs a step, except in a random or Latin hypercube design"
                )
            }
            Self::NoLevels { factor_index } => write!(f, "factor {factor_index} has no levels"),
        }
    }
}

impl std::error::Error for DesignError {}

/// Returns the configs of `block`, each a copy of `base` with one level of every factor written into it.
///
/// A factorial or zip block with no factors gives `base` alone. Note that when two factors write one slot, the later
/// factor's level is the one kept.
///
/// # Errors
///
/// Returns [`DesignError`] for a factor with an empty list of levels, a zip over factors of unequal length, a
/// sampled design with no samples or no factors, a whole range in a design that lists levels, or more than
/// [`MAX_CONFIGS`] configs.
pub fn generate(block: &Block, base: &Config) -> Result<Vec<Config>, DesignError> {
    if let Some(factor_index) = block
        .factors
        .iter()
        .position(|factor| factor.levels().is_some_and(<[FactorLevel]>::is_empty))
    {
        return Err(DesignError::NoLevels { factor_index });
    }
    match block.design {
        DesignKind::Factorial => factorial(&listed_levels(&block.factors)?, &block.factors, base),
        DesignKind::Zip | DesignKind::Table { .. } => zip(&listed_levels(&block.factors)?, &block.factors, base),
        DesignKind::Random { samples } => {
            check_samples(samples, &block.factors)?;
            Ok(random(
                &block.factors,
                base,
                samples,
                &mut DesignRng::new(block.design_seed),
            ))
        }
        DesignKind::LatinHypercube { samples } => {
            check_samples(samples, &block.factors)?;
            Ok(latin_hypercube(
                &block.factors,
                base,
                samples,
                &mut DesignRng::new(block.design_seed),
            ))
        }
    }
}

/// Returns the listed levels of every factor.
fn listed_levels(factors: &[Factor]) -> Result<Vec<&[FactorLevel]>, DesignError> {
    factors
        .iter()
        .enumerate()
        .map(|(factor_index, factor)| factor.levels().ok_or(DesignError::UnlistedLevels { factor_index }))
        .collect()
}

fn check_samples(samples: usize, factors: &[Factor]) -> Result<(), DesignError> {
    if samples == 0 {
        return Err(DesignError::NoSamples);
    }
    if samples > MAX_CONFIGS {
        return Err(DesignError::TooManyConfigs);
    }
    if factors.is_empty() {
        return Err(DesignError::NoFactors);
    }
    Ok(())
}

fn factorial(levels: &[&[FactorLevel]], factors: &[Factor], base: &Config) -> Result<Vec<Config>, DesignError> {
    let count = levels
        .iter()
        .try_fold(1_usize, |product, levels| product.checked_mul(levels.len()))
        .filter(|&count| count <= MAX_CONFIGS)
        .ok_or(DesignError::TooManyConfigs)?;
    let mut configs = Vec::with_capacity(count);
    let mut positions = vec![0; factors.len()];
    for _ in 0..count {
        let mut config = base.clone();
        for ((factor, levels), &position) in factors.iter().zip(levels).zip(&positions) {
            factor.apply(&levels[position], &mut config);
        }
        configs.push(config);
        for (position, levels) in positions.iter_mut().zip(levels).rev() {
            *position += 1;
            if *position < levels.len() {
                break;
            }
            *position = 0;
        }
    }
    Ok(configs)
}

fn zip(levels: &[&[FactorLevel]], factors: &[Factor], base: &Config) -> Result<Vec<Config>, DesignError> {
    let lengths: Vec<usize> = levels.iter().map(|levels| levels.len()).collect();
    let Some(&count) = lengths.first() else {
        return Ok(vec![base.clone()]);
    };
    if lengths.iter().any(|&length| length != count) {
        return Err(DesignError::UnequalLengths { lengths });
    }
    if count > MAX_CONFIGS {
        return Err(DesignError::TooManyConfigs);
    }
    Ok((0..count)
        .map(|position| {
            let mut config = base.clone();
            for (factor, levels) in factors.iter().zip(levels) {
                factor.apply(&levels[position], &mut config);
            }
            config
        })
        .collect())
}

/// Returns `samples` configs, drawing every factor's value on its own.
fn random(factors: &[Factor], base: &Config, samples: usize, rng: &mut DesignRng) -> Vec<Config> {
    (0..samples)
        .map(|_| {
            let mut config = base.clone();
            for factor in factors {
                let level = match &factor.domain {
                    FactorDomain::Levels(levels) => levels[rng.index(levels.len() as u64) as usize].clone(),
                    &FactorDomain::Continuous { min, max } => continuous_level(min, max, rng.unit_f64()),
                    &FactorDomain::WholeNumbers { min, max } => {
                        whole_number_level(factor.slot, rng.whole_number(min, max))
                    }
                };
                factor.apply(&level, &mut config);
            }
            config
        })
        .collect()
}

/// Returns `samples` configs, where each factor takes its strata in an order of its own.
///
/// The draws go factor by factor, a permutation of the strata first, then one position within each stratum for a
/// continuous factor.
fn latin_hypercube(factors: &[Factor], base: &Config, samples: usize, rng: &mut DesignRng) -> Vec<Config> {
    let mut configs = vec![base.clone(); samples];
    let strata = samples as f64;
    for factor in factors {
        let order = rng.permutation(samples);
        for (config, &stratum) in configs.iter_mut().zip(&order) {
            let level = match &factor.domain {
                FactorDomain::Levels(levels) => {
                    levels[balanced_level(stratum, samples, levels.len() as u128) as usize].clone()
                }
                &FactorDomain::Continuous { min, max } => {
                    continuous_level(min, max, (stratum as f64 + rng.unit_f64()) / strata)
                }
                &FactorDomain::WholeNumbers { min, max } => {
                    let count = u128::from(max - min) + 1;
                    whole_number_level(factor.slot, min + balanced_level(stratum, samples, count) as u64)
                }
            };
            factor.apply(&level, config);
        }
    }
    configs
}

/// Returns the level of `count` levels that stratum `stratum` of `samples` falls on.
///
/// Each level is the landing place of `samples / count` strata, rounded down or up.
fn balanced_level(stratum: usize, samples: usize, count: u128) -> u128 {
    stratum as u128 * count / samples as u128
}

/// Returns the `f32` value at fraction `fraction` of the way from `min` to `max`, clamped to the rounded ends.
pub(crate) fn continuous_value(min: f64, max: f64, fraction: f64) -> f32 {
    let value = (min + fraction * (max - min)) as f32;
    value.clamp(min as f32, max as f32)
}

/// Returns [`continuous_value`] as a level.
pub(crate) fn continuous_level(min: f64, max: f64, fraction: f64) -> FactorLevel {
    FactorLevel::Param(ParamValue::F32(continuous_value(min, max, fraction)))
}

/// Returns whole number `value` as a level of the factor writing `slot`.
pub(crate) fn whole_number_level(slot: FactorSlot, value: u64) -> FactorLevel {
    match slot {
        FactorSlot::Param(_) => FactorLevel::Param(ParamValue::U32(value as u32)),
        FactorSlot::Action(_) => FactorLevel::Tick(value),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Block, DesignError, DesignKind, generate};
    use crate::explore::factor::{Factor, FactorDomain, FactorLevel, FactorSlot};
    use crate::explore::plan::Config;
    use crate::params::ParamValue;

    fn factor(slot: usize, values: &[u32]) -> Factor {
        Factor {
            slot: FactorSlot::Param(slot),
            domain: FactorDomain::Levels(
                values
                    .iter()
                    .map(|&value| FactorLevel::Param(ParamValue::U32(value)))
                    .collect(),
            ),
        }
    }

    fn continuous(slot: usize, min: f64, max: f64) -> Factor {
        Factor {
            slot: FactorSlot::Param(slot),
            domain: FactorDomain::Continuous { min, max },
        }
    }

    fn whole_numbers(slot: FactorSlot, min: u64, max: u64) -> Factor {
        Factor {
            slot,
            domain: FactorDomain::WholeNumbers { min, max },
        }
    }

    fn base() -> Config {
        Config {
            block: 0,
            params: vec![ParamValue::U32(0); 3],
            action_ticks: vec![0],
        }
    }

    fn block(design: DesignKind, factors: Vec<Factor>) -> Block {
        Block {
            design,
            factors,
            design_seed: 11,
        }
    }

    /// Returns the configs of a block as rows of `u32` values.
    fn rows(design: DesignKind, factors: Vec<Factor>) -> Result<Vec<Vec<u32>>, DesignError> {
        let configs = generate(&block(design, factors), &base())?;
        Ok(configs
            .into_iter()
            .map(|config| {
                config
                    .params
                    .into_iter()
                    .map(|value| match value {
                        ParamValue::U32(number) => number,
                        other => panic!("every test value is a u32, got {other:?}"),
                    })
                    .collect()
            })
            .collect())
    }

    /// Returns parameter `slot` of every config as an `f64`.
    fn column(configs: &[Config], slot: usize) -> Vec<f64> {
        configs
            .iter()
            .map(|config| match &config.params[slot] {
                &ParamValue::F32(value) => f64::from(value),
                &ParamValue::U32(value) => f64::from(value),
                other => panic!("every test value is a number, got {other:?}"),
            })
            .collect()
    }

    /// Returns the number of configs holding each value of parameter `slot`.
    fn counts(configs: &[Config], slot: usize) -> BTreeMap<u32, usize> {
        let mut counts = BTreeMap::new();
        for value in column(configs, slot) {
            *counts.entry(value as u32).or_default() += 1;
        }
        counts
    }

    fn lhs(samples: usize, factors: Vec<Factor>, design_seed: u64) -> Vec<Config> {
        let block = Block {
            design_seed,
            ..block(DesignKind::LatinHypercube { samples }, factors)
        };
        generate(&block, &base()).expect("a valid design")
    }

    #[test]
    fn a_factorial_varies_the_last_factor_fastest() {
        let configs = rows(
            DesignKind::Factorial,
            vec![factor(0, &[1, 2]), factor(2, &[10, 20, 30])],
        );
        assert_eq!(
            configs,
            Ok(vec![
                vec![1, 0, 10],
                vec![1, 0, 20],
                vec![1, 0, 30],
                vec![2, 0, 10],
                vec![2, 0, 20],
                vec![2, 0, 30],
            ])
        );
    }

    #[test]
    fn a_factorial_has_the_product_of_the_level_counts() {
        let configs = rows(
            DesignKind::Factorial,
            vec![factor(0, &[1, 2]), factor(1, &[1, 2, 3]), factor(2, &[1, 2, 3, 4])],
        )
        .expect("24 configs");
        assert_eq!(configs.len(), 24);
        let mut distinct = configs.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), 24, "every combination appears once");
    }

    #[test]
    fn a_block_with_no_factors_is_its_base() {
        assert_eq!(rows(DesignKind::Factorial, Vec::new()), Ok(vec![vec![0, 0, 0]]));
        assert_eq!(rows(DesignKind::Zip, Vec::new()), Ok(vec![vec![0, 0, 0]]));
    }

    #[test]
    fn a_zip_pairs_levels_by_position() {
        let configs = rows(DesignKind::Zip, vec![factor(0, &[1, 2, 3]), factor(1, &[10, 20, 30])]);
        assert_eq!(configs, Ok(vec![vec![1, 10, 0], vec![2, 20, 0], vec![3, 30, 0]]));
    }

    #[test]
    fn a_zip_of_unequal_lengths_is_refused() {
        let error = rows(DesignKind::Zip, vec![factor(0, &[1, 2, 3]), factor(1, &[10, 20])]);
        assert_eq!(error, Err(DesignError::UnequalLengths { lengths: vec![3, 2] }));
    }

    #[test]
    fn a_design_past_the_limit_is_refused() {
        let wide: Vec<u32> = (0..4096).collect();
        let error = rows(
            DesignKind::Factorial,
            vec![factor(0, &wide), factor(1, &wide), factor(2, &[1, 2])],
        );
        assert_eq!(error, Err(DesignError::TooManyConfigs));
    }

    #[test]
    fn a_latin_hypercube_puts_one_sample_in_each_stratum() {
        let samples = 50;
        let configs = lhs(samples, vec![continuous(0, 0.0, 50.0), continuous(2, 10.0, 20.0)], 7);
        assert_eq!(configs.len(), samples);
        for (slot, min, max) in [(0, 0.0, 50.0), (2, 10.0, 20.0)] {
            let mut strata: Vec<usize> = column(&configs, slot)
                .into_iter()
                .map(|value| ((value - min) / (max - min) * samples as f64).floor() as usize)
                .collect();
            strata.sort_unstable();
            assert_eq!(strata, (0..samples).collect::<Vec<_>>(), "parameter {slot}");
        }
        assert!(
            configs.iter().all(|config| config.params[1] == ParamValue::U32(0)),
            "a parameter no factor varies keeps its base value"
        );
    }

    #[test]
    fn a_latin_hypercube_balances_discrete_levels() {
        let options = Factor {
            slot: FactorSlot::Param(0),
            domain: FactorDomain::Levels(
                (0..3)
                    .map(|option| FactorLevel::Param(ParamValue::Choice(option)))
                    .collect(),
            ),
        };
        let configs = lhs(10, vec![options], 3);
        let mut chosen = [0; 3];
        for config in &configs {
            let ParamValue::Choice(option) = config.params[0] else {
                panic!("the factor writes a choice");
            };
            chosen[option] += 1;
        }
        chosen.sort_unstable();
        assert_eq!(chosen, [3, 3, 4], "ten samples over three options");

        let configs = lhs(8, vec![whole_numbers(FactorSlot::Param(1), 1, 4)], 3);
        assert_eq!(counts(&configs, 1), BTreeMap::from([(1, 2), (2, 2), (3, 2), (4, 2)]));

        let configs = lhs(8, vec![whole_numbers(FactorSlot::Action(0), 100, 103)], 3);
        let mut ticks: Vec<u64> = configs.iter().map(|config| config.action_ticks[0]).collect();
        ticks.sort_unstable();
        assert_eq!(
            ticks,
            [100, 100, 101, 101, 102, 102, 103, 103],
            "a tick range is discrete too"
        );
    }

    #[test]
    fn a_latin_hypercube_is_reproducible_from_its_seed() {
        let factors = || vec![continuous(0, 0.0, 1.0), whole_numbers(FactorSlot::Param(1), 1, 1000)];
        assert_eq!(lhs(20, factors(), 42), lhs(20, factors(), 42));
    }

    #[test]
    fn different_design_seeds_give_different_designs() {
        let factors = || vec![continuous(0, 0.0, 1.0), whole_numbers(FactorSlot::Param(1), 1, 1000)];
        assert_ne!(lhs(20, factors(), 42), lhs(20, factors(), 43));
        let random = |design_seed| {
            let block = Block {
                design_seed,
                ..block(DesignKind::Random { samples: 20 }, factors())
            };
            generate(&block, &base()).expect("a valid design")
        };
        assert_eq!(random(42), random(42));
        assert_ne!(random(42), random(43));
    }

    #[test]
    fn random_samples_stay_within_bounds() {
        let block = block(
            DesignKind::Random { samples: 2000 },
            vec![
                continuous(0, 0.1, 0.2),
                whole_numbers(FactorSlot::Param(1), 0, u64::from(u32::MAX)),
                factor(2, &[5, 6]),
                whole_numbers(FactorSlot::Action(0), 10, 20),
            ],
        );
        let configs = generate(&block, &base()).expect("a valid design");
        assert_eq!(configs.len(), 2000);
        let lowest = f64::from(0.1_f32);
        let highest = f64::from(0.2_f32);
        assert!(
            column(&configs, 0)
                .iter()
                .all(|&value| (lowest..=highest).contains(&value))
        );
        assert!(
            column(&configs, 1).iter().any(|&value| value > f64::from(u32::MAX / 2)),
            "a full span reaches its upper half"
        );
        assert_eq!(counts(&configs, 2).keys().copied().collect::<Vec<_>>(), [5, 6]);
        assert!(configs.iter().all(|config| (10..=20).contains(&config.action_ticks[0])));
    }

    #[test]
    fn a_design_that_cannot_draw_is_refused() {
        let unlisted = || vec![continuous(0, 0.0, 1.0)];
        assert_eq!(
            generate(&block(DesignKind::LatinHypercube { samples: 0 }, unlisted()), &base()),
            Err(DesignError::NoSamples)
        );
        assert_eq!(
            generate(&block(DesignKind::Random { samples: 4 }, Vec::new()), &base()),
            Err(DesignError::NoFactors)
        );
        assert_eq!(
            generate(&block(DesignKind::Factorial, unlisted()), &base()),
            Err(DesignError::UnlistedLevels { factor_index: 0 })
        );
    }

    /// The regression. A sampled design indexed into an empty list and panicked, and a listed one gave no configs.
    #[test]
    fn a_factor_with_no_levels_is_refused_by_every_design() {
        for design in [
            DesignKind::Factorial,
            DesignKind::Zip,
            DesignKind::Random { samples: 4 },
            DesignKind::LatinHypercube { samples: 4 },
        ] {
            assert_eq!(
                rows(design.clone(), vec![factor(0, &[1, 2]), factor(1, &[])]),
                Err(DesignError::NoLevels { factor_index: 1 }),
                "{design:?}"
            );
        }
    }

    /// A stratum of a discrete factor takes the lowest level it covers, whatever the design seed.
    #[test]
    fn a_latin_hypercube_takes_the_lowest_level_of_each_stratum() {
        let levels = || vec![whole_numbers(FactorSlot::Param(1), 1, 100)];
        let expected: BTreeMap<u32, usize> = (0..40).map(|stratum| (1 + stratum * 100 / 40, 1)).collect();
        for design_seed in [3, 42] {
            assert_eq!(counts(&lhs(40, levels(), design_seed), 1), expected);
        }
    }
}
