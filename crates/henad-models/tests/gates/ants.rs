//! The cross-engine gate of the ants: the twelve ants and the seeded field every engine starts from, and the
//! parameters and ticks it runs them for.
//!
//! Randomness in this model lives entirely inside `advect_agent`: a tie between two neighbours draws, and the momentum
//! and random-action branches draw. The gate sets both probabilities to zero and seeds a field whose 3x3
//! neighbourhoods hold no two equal values, so no draw can change the outcome and the run is the same under any
//! generator. Another engine's answer is comparable only because of this.
//!
//! `tests/consistency_ants.rs` holds the stock CPU ants to the gate, and `src/tests/graph_ants_parity.rs` the ants
//! starter on the device. `tests/fixtures/docs/ants_fixture.md` holds how a fixture is produced. The includer brings
//! the layer index `TO_FOOD` into scope.

use henad_core::params::ParamValue;

use super::TO_FOOD;

// Small enough to write out in full, large enough for both obstacle blobs and both sites.
pub const GATE_W: u32 = 32;
pub const GATE_H: u32 = 32;
/// Number of ticks the gate runs. Five is already too many.
///
/// Deposits put decayed copies of the same value into different cells, and by the fifth tick two
/// of them are exactly equal in a neighbourhood an ant is standing in. That tie draws, at one
/// chance in four, so a correct implementation reaches a different answer a quarter of the time.
/// Four ticks is tie free in both `f32` and `f64`.
pub const GATE_TICKS: u32 = 4;

/// One ant of the gate: `(x, y, last_step, has_food, reward)`, with `last_step` encoded `(dx + 1) * 3 + (dy + 1)` and
/// 255 meaning no step yet.
pub type GateAnt = (f32, f32, u8, u8, f32);

/// Twelve ants: the four corners, a top edge, both sites, two cells beside an obstacle, one ant
/// carrying momentum history, and two sharing a cell so the deposit combine has something to merge.
pub const GATE_AGENTS: [GateAnt; 12] = [
    (0.0, 0.0, 255, 0, 1.0),
    (31.0, 0.0, 255, 1, 1.0),
    (0.0, 31.0, 255, 0, 0.5),
    (31.0, 31.0, 255, 1, 0.5),
    (16.0, 0.0, 255, 0, 1.0),
    (4.0, 4.0, 255, 0, 1.0),    // on the food source
    (28.0, 28.0, 255, 1, 1.0),  // on the nest
    (8.0, 12.0, 255, 0, 1.0),   // (9, 13) is an obstacle
    (22.0, 20.0, 255, 1, 1.0),  // (21, 19) is an obstacle
    (15.0, 15.0, 7, 0, 1.0),    // last step (1, 0)
    (15.0, 15.0, 255, 0, 0.25), // shares the cell above, lower reward
    (15.0, 16.0, 2, 1, 1.0),    // last step (-1, 1)
];

/// The seeded field, stated as a formula so every engine builds the same field without a data file.
///
/// Two neighbours collide only when `7dx + 13dy` (or `11dx + 5dy`) is a multiple of the modulus.
/// Across a 3x3 window those sums stay well inside one period, so the only collision is with the
/// cell itself. Values sit in `(0, 1]`, keeping the best neighbour off zero and the momentum
/// branch out of reach.
pub fn gate_field(layer: usize) -> Vec<f32> {
    let (a, b, m) = if layer == TO_FOOD { (7, 13, 97) } else { (11, 5, 89) };
    (0..GATE_H)
        .flat_map(|y| (0..GATE_W).map(move |x| ((a * x + b * y) % m + 1) as f32 / (m + 1) as f32))
        .collect()
}

/// Returns the gate's parameters, in the order of the stock schema.
pub fn gate_params() -> Vec<ParamValue> {
    vec![
        ParamValue::U32(GATE_AGENTS.len() as u32),
        ParamValue::F32(GATE_W as f32),
        ParamValue::F32(GATE_H as f32),
        ParamValue::F32(0.9), // cutdown
        ParamValue::F32(1.0), // reward
        ParamValue::F32(0.0), // momentum, off so the branch cannot fire
        ParamValue::F32(0.0), // random action, off for the same reason
        ParamValue::F32(0.999),
    ]
}

/// Agent rows and both pheromone layers after the gate's ticks, as a fixture holds them.
pub type GateResult = (Vec<GateAnt>, Vec<f32>, Vec<f32>);
