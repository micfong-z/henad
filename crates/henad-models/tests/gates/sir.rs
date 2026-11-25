//! The SIR bands: the size, rates and seed of the run, and the binomial bands its first step's infection and recovery
//! rates are held to.
//!
//! `tests/consistency_sir.rs` holds the stock CPU SIR to the bands, and `src/tests/graph_sir_parity.rs` the SIR
//! starter on the device. `tests/fixtures/docs/sir_fixture.md` holds the cross-engine checks.

// Cell states, as the stock models and the starter's categories order them. `encoding_matches_the_models_own_stats`
// of `consistency_sir.rs` pins them against what the CPU model reports.
pub const S: u32 = 0;
pub const I: u32 = 1;
pub const R: u32 = 2;

/// Side of the square world.
pub const SIDE: usize = 1024;
/// Infection rate.
pub const BETA: f64 = 0.3;
/// Recovery rate.
pub const GAMMA: f64 = 0.05;
/// Share of the cells infected at tick 0.
pub const INITIAL_INFECTED: f64 = 0.5;
/// Seed of the run.
pub const SEED: u64 = 0x5152_0BEE_5EED_0001;

/// Buckets thinner than this are skipped to avoid false positives from small-number statistics.
pub const MIN_TRIALS: u64 = 1_000;

/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
pub const SIGMAS: f64 = 5.0;

/// Returns the number of infected neighbours of `(x, y)` in a Moore neighbourhood, wrapping toroidally.
pub fn infected_neighbours(grid: &[u32], x: usize, y: usize) -> usize {
    let mut count = 0;
    for dy in [SIDE - 1, 0, 1] {
        for dx in [SIDE - 1, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }
            count += usize::from(grid[((y + dy) % SIDE) * SIDE + (x + dx) % SIDE] == I);
        }
    }
    count
}

/// Returns the half-width of the acceptance band for a binomial proportion `expected` over `trials` trials.
pub fn band(expected: f64, trials: u64) -> f64 {
    SIGMAS * (expected * (1.0 - expected) / trials as f64).sqrt()
}

/// Holds the step from the cells `before` to the cells `after` to `P(S -> I | k infected neighbours) = 1 - (1 -
/// beta)^k` for every `k` of at least [`MIN_TRIALS`] cells, at least eight of them, and to no infection without an
/// infected neighbour. `what` opens each message.
pub fn check_infections(before: &[u32], after: &[u32], what: &str) {
    let mut trials = [0u64; 9];
    let mut infections = [0u64; 9];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let index = y * SIDE + x;
            if before[index] != S {
                continue;
            }
            let k = infected_neighbours(before, x, y);
            trials[k] += 1;
            infections[k] += u64::from(after[index] == I);
        }
    }

    let mut checked = 0;
    for (k, (&count, &hits)) in trials.iter().zip(&infections).enumerate() {
        if count < MIN_TRIALS {
            continue;
        }
        let expected = 1.0 - (1.0 - BETA).powi(k as i32);
        let observed = hits as f64 / count as f64;
        let allowed = band(expected, count);
        assert!(
            (observed - expected).abs() <= allowed,
            "{what}, k={k}: observed {observed:.6}, expected {expected:.6}, off by {:.6} (allowed {allowed:.6}) over \
             {count} cells",
            (observed - expected).abs()
        );
        checked += 1;
    }

    assert_eq!(
        infections[0], 0,
        "{what}: a cell with no infected neighbour got infected"
    );
    assert!(
        checked >= 8,
        "{what}: only {checked} buckets had enough samples to check"
    );
}

/// Holds the step from the cells `before` to the cells `after` to `P(I -> R) = gamma`, over at least [`MIN_TRIALS`]
/// infected cells. `what` opens each message.
pub fn check_recoveries(before: &[u32], after: &[u32], what: &str) {
    let mut trials = 0u64;
    let mut recoveries = 0u64;
    for (&was, &now) in before.iter().zip(after) {
        if was == I {
            trials += 1;
            recoveries += u64::from(now == R);
        }
    }

    assert!(trials >= MIN_TRIALS, "{what}: only {trials} infected cells to sample");
    let observed = recoveries as f64 / trials as f64;
    let allowed = band(GAMMA, trials);
    assert!(
        (observed - GAMMA).abs() <= allowed,
        "{what}: recovery rate {observed:.6}, expected {GAMMA:.6}, off by {:.6} (allowed {allowed:.6}) over {trials} \
         cells",
        (observed - GAMMA).abs()
    );
}
