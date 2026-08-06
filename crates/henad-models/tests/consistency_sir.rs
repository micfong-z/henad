//! Cross-engine and self consistency check for SIR.
//!
//! See `tests/fixtures/docs/sir_fixture.md` for how a fixture is produced.

use henad_compute::grid_engine::GridModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::StatValue;
use henad_models::sir::SirGridModel;

/// Cell encoding. Private to the model, so `encoding_matches_the_models_own_stats` pins these
/// against what the model itself reports rather than trusting the order.
const S: u8 = 0;
const I: u8 = 1;
const R: u8 = 2;

const W: usize = 1024;
const H: usize = 1024;
const BETA: f64 = 0.3;
const GAMMA: f64 = 0.05;

const INITIAL_INFECTED: f64 = 0.5;

const SEED: u64 = 0x5152_0BEE_5EED_0001;

/// Buckets thinner than this are skipped to avoid false positives from small-number statistics.
const MIN_TRIALS: u64 = 1_000;

/// Half-width of the acceptance band for an observation, of a binomially distributed random variable.
const SIGMAS: f64 = 5.0;

fn params() -> Vec<ParamValue> {
    vec![
        ParamValue::U32(W as u32),
        ParamValue::U32(H as u32),
        ParamValue::F32(BETA as f32),
        ParamValue::F32(GAMMA as f32),
        ParamValue::F32(INITIAL_INFECTED as f32),
    ]
}

fn state() -> GridModelState<SirGridModel> {
    GridModelState::<SirGridModel>::from_params_seeded(&params(), Some(SEED))
}

fn cells(state: &GridModelState<SirGridModel>) -> Vec<u8> {
    state
        .grid_view()
        .expect("a grid model always has a grid view")
        .cells
        .to_vec()
}

/// Infected neighbours of `(x, y)` in a Moore neighbourhood, wrapping toroidally.
fn infected_neighbours(grid: &[u8], x: usize, y: usize) -> usize {
    let mut n = 0;
    for dy in [H - 1, 0, 1] {
        for dx in [W - 1, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (x + dx) % W;
            let ny = (y + dy) % H;
            if grid[ny * W + nx] == I {
                n += 1;
            }
        }
    }
    n
}

/// Half-width of the acceptance band for an observation, of a binomially distributed random variable.
fn band(expected: f64, trials: u64) -> f64 {
    SIGMAS * (expected * (1.0 - expected) / trials as f64).sqrt()
}

/// The cell values this file assumes must be the ones the model reports.
///
/// This is a self-consistency check.
#[test]
fn encoding_matches_the_models_own_stats() {
    let state = state();
    let grid = cells(&state);
    let stats = state.stats();

    for (value, name) in [(S, "Susceptible"), (I, "Infected"), (R, "Recovered")] {
        let counted = grid.iter().filter(|&&c| c == value).count() as f64;
        let entry = stats
            .iter()
            .find(|e| e.label == name)
            .unwrap_or_else(|| panic!("no `{name}` stat"));
        let StatValue::Scalar(reported) = entry.value else {
            panic!("`{name}` is not a scalar: {:?}", entry.value)
        };
        assert!(
            (counted - reported).abs() < 0.5,
            "cell value {value} counted {counted} but the model reports {reported} `{name}`"
        );
    }
}

/// `P(S -> I | k infected neighbours) = 1 - (1 - beta)^k`, for every `k` with enough samples.
///
/// This is a self-consistency check.
#[test]
fn infection_rate_matches_the_closed_form() {
    let mut state = state();
    let before = cells(&state);
    state.step();
    let after = cells(&state);

    let mut trials = [0u64; 9];
    let mut infections = [0u64; 9];
    for y in 0..H {
        for x in 0..W {
            let i = y * W + x;
            if before[i] != S {
                continue;
            }
            let k = infected_neighbours(&before, x, y);
            trials[k] += 1;
            if after[i] == I {
                infections[k] += 1;
            }
        }
    }

    let mut checked = 0;
    for (k, (&n, &hits)) in trials.iter().zip(&infections).enumerate() {
        if n < MIN_TRIALS {
            continue;
        }
        let expected = 1.0 - (1.0 - BETA).powi(k as i32);
        let observed = hits as f64 / n as f64;
        let allowed = band(expected, n);
        assert!(
            (observed - expected).abs() <= allowed,
            "k={k}: observed {observed:.6}, expected {expected:.6}, \
             off by {:.6} (allowed {allowed:.6}) over {n} cells",
            (observed - expected).abs()
        );
        checked += 1;
    }

    assert_eq!(infections[0], 0, "cells with no infected neighbour got infected");
    assert!(checked >= 8, "only {checked} buckets had enough samples to check");
}

/// `P(I -> R) = gamma`, independent of the neighbourhood.
///
/// This is a self-consistency check.
#[test]
fn recovery_rate_matches_the_parameter() {
    let mut state = state();
    let before = cells(&state);
    state.step();
    let after = cells(&state);

    let mut trials = 0u64;
    let mut recoveries = 0u64;
    for (b, a) in before.iter().zip(&after) {
        if *b == I {
            trials += 1;
            if *a == R {
                recoveries += 1;
            }
        }
    }

    assert!(trials >= MIN_TRIALS, "only {trials} infected cells to sample");
    let observed = recoveries as f64 / trials as f64;
    let allowed = band(GAMMA, trials);
    assert!(
        (observed - GAMMA).abs() <= allowed,
        "recovery rate {observed:.6}, expected {GAMMA:.6}, off by {:.6} (allowed {allowed:.6}) over {trials} cells",
        (observed - GAMMA).abs()
    );
}

/// The compartments only ever run S to I to R.
///
/// This is a self-consistency check.
#[test]
fn transitions_only_go_forwards() {
    let mut state = state();
    let mut before = cells(&state);
    for tick in 0..5 {
        state.step();
        let after = cells(&state);
        for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
            assert!(
                matches!((b, a), (S, S | I) | (I, I | R) | (R, R)),
                "cell {i} went from {b} to {a} on tick {tick}"
            );
        }
        before = after;
    }
}
