//! Random draws, and the generator behind them.
//!
//! The draws take raw bits so the same function serves both backends. Only they have WGSL twins:
//! the generators differ, since WGSL has no 64-bit integers and uses `pcg_hash` over `u32`.
//!
//! Every draw needs its own [`next_bits`]. Two draws off one word are correlated, and nothing will
//! say so.

/// Fast xorshift64 PRNG. The state must never be 0.
#[inline]
pub fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

/// Scrambles a user-supplied seed into a usable RNG state.
///
/// Guards against the absorbing `xorshift64(0) == 0`, and decorrelates adjacent seeds like 1, 2, 3.
pub fn mix_seed(seed: u64) -> u64 {
    const GOLDEN: u64 = 0x7347_5CB4_0A56_8E8D;
    let mut z = seed.wrapping_add(GOLDEN);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    if z == 0 { GOLDEN } else { z }
}

/// Advances `rng` and returns 32 fresh random bits.
///
/// The top half of the state rather than the low one, since xorshift64's low bits are the weaker
/// of the two.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::rng::next_bits;
///
/// let mut rng = 0x1234_5678_9ABC_DEF0;
/// let first = next_bits(&mut rng);
/// assert_ne!(first, next_bits(&mut rng));
/// ```
///
/// See also: [`next_float`], [`xorshift64`].
#[inline]
pub fn next_bits(rng: &mut u64) -> u32 {
    *rng = xorshift64(*rng);
    (*rng >> 32) as u32
}

/// A uniform float in `[0, max)`, as NetLogo's `random-float`.
///
/// Built from the top 24 bits, which is the f32 mantissa width, so every value it can produce is
/// exact and the range really is half-open. Using all 32 bits would round the largest word up to
/// exactly `max`, and a closed range breaks any caller comparing against a probability of 1.
///
/// The same 24-bit form runs on both backends, so this is held to bit equality rather than a
/// tolerance.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::rng::random_float;
///
/// assert_eq!(random_float(0, 1.0), 0.0);
/// // The largest draw stays strictly under `max`.
/// assert!(random_float(u32::MAX, 1.0) < 1.0);
/// assert!(random_float(u32::MAX, 10.0) < 10.0);
/// ```
///
/// See also: [`next_float`], [`below`], [`reservoir_accept`].
#[inline]
pub fn random_float(bits: u32, max: f32) -> f32 {
    (bits >> 8) as f32 / 16_777_216.0 * max
}

/// Advances `rng`, then draws with [`random_float`].
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::rng::next_float;
///
/// let mut rng = 0x1234_5678_9ABC_DEF0;
/// let draw = next_float(&mut rng, 1.0);
/// assert!((0.0..1.0).contains(&draw));
/// ```
///
/// # WGSL counterpart
///
/// `rng::next_float` takes a `ptr<function, u32>` and advances it with `pcg_hash`, so it draws
/// from a different stream. Only [`random_float`] is held to bit equality.
///
/// See also: [`random_float`], [`next_bits`].
#[inline]
pub fn next_float(rng: &mut u64, max: f32) -> f32 {
    random_float(next_bits(rng), max)
}

/// A Bernoulli trial, true for `threshold` of the 2^32 possible words.
///
/// Pass `(p * u32::MAX as f32) as u32` for probability `p`. Integer comparison rather than a float
/// one, so a seeded run cannot drift on a machine that rounds differently.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::rng::below;
///
/// assert!(below(0, 1));
/// assert!(!below(u32::MAX, u32::MAX));
/// ```
///
/// See also: [`random_float`], [`next_bits`].
#[inline]
pub fn below(bits: u32, threshold: u32) -> bool {
    bits < threshold
}

/// One of `-1`, `0` or `+1`.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::rng::choice3;
///
/// assert_eq!(choice3(0), -1);
/// assert_eq!(choice3(1), 0);
/// assert_eq!(choice3(2), 1);
/// ```
///
/// See also: [`reservoir_accept`].
#[inline]
pub fn choice3(bits: u32) -> i32 {
    (bits % 3) as i32 - 1
}

/// Accepts the `count`-th of a run of equally good candidates, with probability `1 / count`.
///
/// Reservoir sampling over ties, so once `n` equal candidates have been seen each has been picked
/// with probability `1 / n`. `count` is the candidate's 1-based position, so the first of a run is
/// always accepted and a `count` of 0 accepts too.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::rng::reservoir_accept;
///
/// // The first candidate of a run always wins, whatever the draw.
/// assert!(reservoir_accept(0, 1));
/// assert!(reservoir_accept(u32::MAX, 1));
/// // The second wins half the time.
/// assert!(reservoir_accept(0, 2));
/// assert!(!reservoir_accept(u32::MAX, 2));
/// ```
///
/// See also: [`random_float`], [`choice3`].
#[inline]
pub fn reservoir_accept(bits: u32, count: u32) -> bool {
    random_float(bits, 1.0) < 1.0 / count as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift64_no_zero() {
        let mut s = 1u64;
        for _ in 0..1000 {
            s = xorshift64(s);
            assert_ne!(s, 0, "xorshift64 should not produce 0");
        }
    }

    #[test]
    fn mix_seed_rescues_the_absorbing_zero_state() {
        assert_ne!(mix_seed(0), 0, "seed 0 must not stay in xorshift64's absorbing state");
        for s in 0..8u64 {
            assert_ne!(xorshift64(mix_seed(s)), 0);
        }
    }

    #[test]
    fn seed_that_would_mix_to_zero_is_guarded() {
        const PREIMAGE_OF_ZERO: u64 = 0x61C8_8646_80B5_83EB;
        assert_ne!(mix_seed(PREIMAGE_OF_ZERO), 0);
        assert_ne!(xorshift64(mix_seed(PREIMAGE_OF_ZERO)), 0);
        // Neighbours are unaffected, so the guard has not perturbed the surrounding range.
        assert_ne!(mix_seed(PREIMAGE_OF_ZERO - 1), mix_seed(PREIMAGE_OF_ZERO));
        assert_ne!(mix_seed(PREIMAGE_OF_ZERO + 1), mix_seed(PREIMAGE_OF_ZERO));
    }

    #[test]
    fn mix_seed_separates_adjacent_seeds() {
        for s in 0..64u64 {
            let (a, b) = (mix_seed(s), mix_seed(s + 1));
            assert_ne!(a, b);
            assert!(
                (a ^ b).count_ones() >= 8,
                "seeds {s} and {} differ in only {} bits after mixing",
                s + 1,
                (a ^ b).count_ones()
            );
        }
    }

    #[test]
    fn xorshift64_deterministic() {
        let a = xorshift64(42);
        let b = xorshift64(42);
        assert_eq!(a, b);
    }

    /// The half-open range is the point. A closed one rejects the first of a tie run, which is the
    /// bug that took the divisor from `u32::MAX` to the 24-bit form.
    #[test]
    fn random_float_never_reaches_max() {
        for max in [1.0f32, 0.5, 10.0, 1e-3] {
            for bits in [0u32, 1, u32::MAX / 2, u32::MAX - 1, u32::MAX] {
                let v = random_float(bits, max);
                assert!(v >= 0.0 && v < max, "random_float({bits}, {max}) was {v}");
            }
        }
    }

    /// Every value is a 24-bit numerator over a power of two, so nothing rounds.
    #[test]
    fn random_float_is_exact() {
        for bits in [0u32, 256, 1 << 16, u32::MAX] {
            let v = random_float(bits, 1.0);
            assert_eq!(
                v * 16_777_216.0,
                (bits >> 8) as f32,
                "random_float({bits}, 1.0) lost precision"
            );
        }
    }

    /// A drifting mean would bias every model that draws through this.
    #[test]
    fn random_float_averages_near_half_of_max() {
        let mut rng = 0xC0FF_EE00_1234_5678;
        let n = 100_000;
        let total: f64 = (0..n).map(|_| f64::from(next_float(&mut rng, 1.0))).sum();
        let mean = total / f64::from(n);
        assert!((mean - 0.5).abs() < 0.01, "mean of {n} draws was {mean}");
    }

    #[test]
    fn choice3_covers_all_three_and_nothing_else() {
        let mut seen = [false; 3];
        let mut rng = 0x0BAD_C0DE_0BAD_C0DE;
        for _ in 0..1000 {
            let d = choice3(next_bits(&mut rng));
            assert!((-1..=1).contains(&d), "choice3 gave {d}");
            seen[(d + 1) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s), "not every direction came up: {seen:?}");
    }

    /// The first of a run must always be accepted, or a tie-break silently drops candidates.
    #[test]
    fn reservoir_accept_always_takes_the_first_of_a_run() {
        let mut rng = 0xABCD_1234_ABCD_1234;
        for _ in 0..10_000 {
            assert!(
                reservoir_accept(next_bits(&mut rng), 1),
                "the first candidate was rejected"
            );
        }
        assert!(
            reservoir_accept(u32::MAX, 1),
            "the largest draw rejected the first candidate"
        );
    }

    /// The property the ants tie-break relies on. Each of `n` equal candidates must end up chosen
    /// about `1 / n` of the time.
    #[test]
    fn reservoir_accept_is_uniform_over_a_run_of_ties() {
        const RUN: u32 = 8;
        const TRIALS: u32 = 200_000;
        let mut rng = 0xFEED_FACE_CAFE_BEEF;
        let mut wins = [0u32; RUN as usize];

        for _ in 0..TRIALS {
            let mut chosen = 0usize;
            for k in 1..=RUN {
                if reservoir_accept(next_bits(&mut rng), k) {
                    chosen = (k - 1) as usize;
                }
            }
            wins[chosen] += 1;
        }

        let expected = f64::from(TRIALS) / f64::from(RUN);
        for (k, &w) in wins.iter().enumerate() {
            let ratio = f64::from(w) / expected;
            assert!(
                (ratio - 1.0).abs() < 0.05,
                "candidate {k} won {w} times, expected about {expected}"
            );
        }
    }

    #[test]
    fn below_is_a_plain_comparison() {
        assert!(below(0, 1));
        assert!(!below(1, 1));
        assert!(!below(u32::MAX, u32::MAX));
        assert!(below(u32::MAX - 1, u32::MAX));
    }

    /// A threshold of 0 must never fire, or a probability of 0 would still happen sometimes.
    #[test]
    fn a_zero_threshold_never_fires() {
        let mut rng = 0x5EED_5EED_5EED_5EED;
        for _ in 0..10_000 {
            assert!(!below(next_bits(&mut rng), 0), "a zero threshold fired");
        }
    }
}
