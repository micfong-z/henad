//! Seeds of the runs in a sweep, each derived from one root seed.

use crate::authoring::primitives::rng::mix_seed;

/// Domain separator for run seeds.
pub const SWEEP_SALT: u64 = 0x005E_E900_5EED_0001;

/// Domain separator for the per-config offset of [`independent_run_seed`].
pub const CONFIG_SALT: u64 = 0x00C0_4F16_5EED_0001;

/// Domain separator for [`design_seed`].
pub const DESIGN_SALT: u64 = 0x00DE_5160_5EED_0001;

/// Domain separator for [`search_seed`].
pub const SEARCH_SALT: u64 = 0x005E_A6C4_5EED_0001;

/// Rule that assigns a seed to each run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SeedScheme {
    /// Replicate `r` of every config shares one seed, so configs are compared on common random numbers.
    #[default]
    Common,
    /// Every run gets its own seed.
    Independent,
}

impl SeedScheme {
    /// Returns the scheme's name in a spec file, as in `common`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Independent => "independent",
        }
    }

    /// Returns the formula behind the seeds, in terms of [`mix_seed`], [`SWEEP_SALT`] and [`CONFIG_SALT`].
    pub fn formula(self) -> &'static str {
        match self {
            Self::Common => "mix_seed(mix_seed(root ^ SWEEP_SALT) + rep)",
            Self::Independent => "mix_seed(mix_seed((mix_seed(root ^ CONFIG_SALT) + config_id) ^ SWEEP_SALT) + rep)",
        }
    }

    /// Returns the seed of replicate `rep` of config `config_id`.
    pub fn seed(self, root: u64, config_id: u64, rep: u64) -> u64 {
        match self {
            Self::Common => run_seed(root, rep),
            Self::Independent => independent_run_seed(root, config_id, rep),
        }
    }
}

/// Returns the seed of replicate `rep` under [`SeedScheme::Common`], the same in every config.
///
/// Note that the result is never 0.
pub fn run_seed(root: u64, rep: u64) -> u64 {
    mix_seed(mix_seed(root ^ SWEEP_SALT).wrapping_add(rep))
}

/// Returns the seed of replicate `rep` of config `config_id` under [`SeedScheme::Independent`].
///
/// Note that the result is never 0.
pub fn independent_run_seed(root: u64, config_id: u64, rep: u64) -> u64 {
    run_seed(mix_seed(root ^ CONFIG_SALT).wrapping_add(config_id), rep)
}

/// Returns the draw seed of block `block`, used when the spec sets no seed for the block.
pub fn design_seed(root: u64, block: usize) -> u64 {
    mix_seed(mix_seed(root ^ DESIGN_SALT).wrapping_add(block as u64))
}

/// Returns the draw seed of a search.
pub fn search_seed(root: u64) -> u64 {
    mix_seed(mix_seed(root ^ SEARCH_SALT))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{SWEEP_SALT, SeedScheme, design_seed, independent_run_seed, run_seed, search_seed};

    /// Root `r` and replicate `k` must not produce the seed of root `r + 1` and replicate `k - 1`.
    #[test]
    fn run_seeds_do_not_overlap_across_nearby_roots() {
        let seeds: BTreeSet<u64> = (0..64)
            .flat_map(|root| (0..64).map(move |rep| run_seed(root, rep)))
            .collect();
        assert_eq!(seeds.len(), 64 * 64);
    }

    #[test]
    fn common_seeds_depend_on_the_replicate_only() {
        let scheme = SeedScheme::Common;
        assert_eq!(scheme.seed(7, 0, 3), scheme.seed(7, 5, 3));
        assert_eq!(scheme.seed(7, 0, 3), run_seed(7, 3));
        assert_ne!(scheme.seed(7, 0, 3), scheme.seed(7, 0, 4));
        assert_ne!(scheme.seed(7, 0, 3), scheme.seed(8, 0, 3));
    }

    #[test]
    fn independent_seeds_differ_by_config() {
        let scheme = SeedScheme::Independent;
        assert_ne!(scheme.seed(7, 0, 3), scheme.seed(7, 1, 3));
        let seeds: BTreeSet<u64> = (0..64)
            .flat_map(|config_id| (0..64).map(move |rep| independent_run_seed(7, config_id, rep)))
            .collect();
        assert_eq!(seeds.len(), 64 * 64);
        let common: BTreeSet<u64> = (0..64).map(|rep| run_seed(7, rep)).collect();
        assert!(seeds.is_disjoint(&common), "the two schemes do not share seeds");
    }

    #[test]
    fn design_seeds_differ_by_block_and_from_run_seeds() {
        let designs: BTreeSet<u64> = (0..64)
            .flat_map(|root| (0..64).map(move |block| design_seed(root, block)))
            .collect();
        assert_eq!(designs.len(), 64 * 64);
        let runs: BTreeSet<u64> = (0..64)
            .flat_map(|root| (0..64).map(move |rep| run_seed(root, rep)))
            .collect();
        assert!(designs.is_disjoint(&runs), "a design draws apart from every run");
    }

    #[test]
    fn search_seeds_differ_by_root_and_from_other_seeds() {
        let searches: BTreeSet<u64> = (0..4096).map(search_seed).collect();
        assert_eq!(searches.len(), 4096);
        let others: BTreeSet<u64> = (0..64)
            .flat_map(|root| (0..64).flat_map(move |index| [run_seed(root, index), design_seed(root, index as usize)]))
            .collect();
        assert!(
            searches.is_disjoint(&others),
            "a search draws apart from every run and design"
        );
    }

    #[test]
    fn no_run_seed_is_zero() {
        for root in [0, 1, u64::MAX, SWEEP_SALT] {
            for rep in 0..1024 {
                assert_ne!(run_seed(root, rep), 0, "root {root}, replicate {rep}");
                assert_ne!(independent_run_seed(root, rep, rep), 0, "root {root}, replicate {rep}");
            }
        }
    }
}
