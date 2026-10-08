//! Random draws of sampled designs and searches, each stream from one seed.

use crate::authoring::primitives::rng::{mix_seed, next_bits, next_index};

/// Scale that turns 53 random bits into a fraction in `[0, 1)`.
const UNIT_SCALE: f64 = 1.0 / (1_u64 << 53) as f64;

/// Random number generator for a sampled design or a search, an xorshift64 stream started from one seed.
///
/// Every draw is built from whole 32-bit words with integer and basic float arithmetic and no library function, so a
/// seed produces the same draws on every platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignRng {
    state: u64,
}

impl DesignRng {
    /// Returns a generator whose stream starts from `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: mix_seed(seed) }
    }

    /// Returns a uniform fraction in `[0, 1)` with 53 random bits.
    pub fn unit_f64(&mut self) -> f64 {
        let high = u64::from(next_bits(&mut self.state));
        let low = u64::from(next_bits(&mut self.state));
        ((high << 21) | (low >> 11)) as f64 * UNIT_SCALE
    }

    /// Returns a draw from the triangular distribution on `[-1, 1)` with its mode at 0.
    ///
    /// The draw is the sum of two uniform fractions less 1.
    pub fn triangular(&mut self) -> f64 {
        let first = self.unit_f64();
        first + self.unit_f64() - 1.0
    }

    /// Returns a uniform integer in `[0, n)`, or 0 when `n` is 0.
    pub fn index(&mut self, n: u64) -> u64 {
        if let Ok(narrow) = u32::try_from(n) {
            return u64::from(next_index(&mut self.state, narrow));
        }
        // Lemire's method over 64-bit words, as `next_index` does over 32-bit ones.
        let mut wide = u128::from(self.word()) * u128::from(n);
        if (wide as u64) < n {
            let floor = n.wrapping_neg() % n;
            while (wide as u64) < floor {
                wide = u128::from(self.word()) * u128::from(n);
            }
        }
        (wide >> 64) as u64
    }

    /// Returns a uniform whole number from `min` to `max` inclusive.
    ///
    /// # Panics
    ///
    /// Panics when `min` is above `max`.
    pub fn whole_number(&mut self, min: u64, max: u64) -> u64 {
        let span = max.checked_sub(min).expect("min is at most max");
        match span.checked_add(1) {
            Some(count) => min + self.index(count),
            None => self.word(),
        }
    }

    /// Returns `0..n` in a uniformly random order, shuffled by Fisher-Yates.
    pub fn permutation(&mut self, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = self.index(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        order
    }

    /// Returns 64 random bits, from two draws.
    fn word(&mut self) -> u64 {
        let high = u64::from(next_bits(&mut self.state));
        (high << 32) | u64::from(next_bits(&mut self.state))
    }
}

#[cfg(test)]
mod tests {
    use super::DesignRng;

    #[test]
    fn a_permutation_holds_every_index_once() {
        let mut rng = DesignRng::new(5);
        let mut order = rng.permutation(100);
        assert_ne!(order, (0..100).collect::<Vec<_>>(), "the order is shuffled");
        order.sort_unstable();
        assert_eq!(order, (0..100).collect::<Vec<_>>());
        assert!(rng.permutation(0).is_empty());
        assert_eq!(rng.permutation(1), [0]);
    }

    #[test]
    fn draws_stay_within_their_bounds() {
        let mut rng = DesignRng::new(9);
        for _ in 0..10_000 {
            let unit = rng.unit_f64();
            assert!((0.0..1.0).contains(&unit), "{unit}");
            assert!(rng.index(3) < 3);
            assert!(rng.index(u64::from(u32::MAX) + 7) < u64::from(u32::MAX) + 7);
            assert!((10..=12).contains(&rng.whole_number(10, 12)));
            let step = rng.triangular();
            assert!((-1.0..1.0).contains(&step), "{step}");
        }
        assert_eq!(rng.whole_number(4, 4), 4);
        assert_eq!(rng.index(0), 0);
    }

    #[test]
    fn a_wide_span_reaches_past_32_bits() {
        let mut rng = DesignRng::new(3);
        let full: Vec<u64> = (0..64).map(|_| rng.whole_number(0, u64::MAX)).collect();
        assert!(full.iter().any(|&draw| draw > u64::from(u32::MAX)), "{full:?}");
        let wide: Vec<u64> = (0..64).map(|_| rng.whole_number(0, u64::from(u32::MAX))).collect();
        assert!(wide.iter().any(|&draw| draw > u64::from(u32::MAX / 2)), "{wide:?}");
    }

    #[test]
    fn a_seed_gives_one_stream() {
        let draws = |seed| {
            let mut rng = DesignRng::new(seed);
            (0..16).map(|_| rng.index(1000)).collect::<Vec<_>>()
        };
        assert_eq!(draws(1), draws(1));
        assert_ne!(draws(1), draws(2));
    }
}
