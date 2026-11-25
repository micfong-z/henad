//! Self consistency check for SIR.
//!
//! See `tests/fixtures/docs/sir_fixture.md` for the cross-engine consistency checks.

use henad_compute::cpu::grid_engine::GridModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::StatValue;
use henad_models::sir::SirGridModel;

#[path = "gates/sir.rs"]
mod gate;

use gate::{BETA, GAMMA, I, INITIAL_INFECTED, R, S, SEED, SIDE};

fn params() -> Vec<ParamValue> {
    vec![
        ParamValue::U32(SIDE as u32),
        ParamValue::U32(SIDE as u32),
        ParamValue::F32(BETA as f32),
        ParamValue::F32(GAMMA as f32),
        ParamValue::F32(INITIAL_INFECTED as f32),
    ]
}

fn state() -> GridModelState<SirGridModel> {
    GridModelState::<SirGridModel>::from_params_seeded(&params(), Some(SEED))
}

/// Returns the cells of `state`, one word per cell as the bands read them.
fn cells(state: &GridModelState<SirGridModel>) -> Vec<u32> {
    state
        .grid_view()
        .expect("a grid model always has a grid view")
        .cells
        .iter()
        .map(|&cell| u32::from(cell))
        .collect()
}

/// The cell values that this file assumes must match the values that the model reports.
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
            .find(|e| e.label() == name)
            .unwrap_or_else(|| panic!("no `{name}` stat"));
        let StatValue::Scalar(reported) = entry.value() else {
            panic!("`{name}` is not a scalar: {:?}", entry.value())
        };
        assert!(
            (counted - reported).abs() < 0.5,
            "cell value {value} counted {counted} but the model reports {reported} `{name}`"
        );
    }
}

/// Checks `P(S -> I | k infected neighbours) = 1 - (1 - beta)^k` for every `k` with enough samples.
///
/// This is a self-consistency check.
#[test]
fn infection_rate_matches_the_closed_form() {
    let mut state = state();
    let before = cells(&state);
    state.step();
    let after = cells(&state);
    gate::check_infections(&before, &after, &format!("seed {SEED:#x}"));
}

/// Checks `P(I -> R) = gamma`, independent of the neighbourhood.
///
/// This is a self-consistency check.
#[test]
fn recovery_rate_matches_the_parameter() {
    let mut state = state();
    let before = cells(&state);
    state.step();
    let after = cells(&state);
    gate::check_recoveries(&before, &after, &format!("seed {SEED:#x}"));
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
