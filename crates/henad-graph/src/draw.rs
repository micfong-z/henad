//! Counter-based random draws: the words a `rand.*` node's bits are a pure function of.
//!
//! The bits of a draw are `core(k, e, t, i, s)`. `k` is the *key word*, which the host computes once per node per stage
//! from the node's `key`, the stage's phase and the run seed. `e` is the element, `t` the tick or press, `i` the region
//! iteration word, and `s` the sub-draw of a node that takes more than one word.
//!
//! The *phase* is a word that keeps the stages' draws apart: [`PHASE_INIT`] in the init stage, [`PHASE_TICK`] in the
//! tick stage, and [`action_phase`] of the action's position in an action, a rerun included. `t` is the tick in a tick
//! pass, the press count in an action, and 0 in an init pass. Inside a region, `i` is the iteration `k`, and nested
//! regions combine their iterations in mixed radix: `i = i_outer * max + k`, with `max` the inner region's bound.

use henad_core::authoring::primitives::rng::{mix_seed, pcg_hash};

/// Phase word of the init stage.
pub const PHASE_INIT: u32 = 0;
/// Phase word of the tick stage, and of the view and stats stages, which draw nothing.
pub const PHASE_TICK: u32 = 1;
/// Phase word of the first action. The action at position `j` of the file's `[[action]]` tables takes
/// `PHASE_FIRST_ACTION + j`.
pub const PHASE_FIRST_ACTION: u32 = 2;

/// Returns the phase word of the action at position `index` of the file's `[[action]]` tables.
pub fn action_phase(index: usize) -> u32 {
    PHASE_FIRST_ACTION.wrapping_add(index as u32)
}

/// Returns the iteration word of a draw at iteration `k` of a region with the bound `max`.
///
/// `outer` is the iteration word of the region around it, or `None` for a region at the top of its pass.
pub fn iteration_word(outer: Option<u32>, max: u32, k: u32) -> u32 {
    outer.map_or(k, |outer| outer.wrapping_mul(max).wrapping_add(k))
}

/// Run word of a graph with no seed.
pub const GRAPH_DEFAULT_SEED: u64 = 0x6A09_E667_F3BC_C908;
/// Salt that keeps a graph's run word apart from every other use of a seed.
pub const GRAPH_SEED_SALT: u64 = 0xBB67_AE85_84CA_A73B;
/// Odd multiplier that spreads a small counter across a word before it meets a hash.
const SPREAD: u32 = 0x9E37_79B1;
/// Multiplier of the per-press word.
const PRESS_MULTIPLIER: u32 = 747_796_405;
/// Increment of the per-press word.
const PRESS_INCREMENT: u32 = 2_891_336_453;

/// Returns the low and high halves of `word` folded into one `u32`.
pub fn fold32(word: u64) -> u32 {
    (word ^ (word >> 32)) as u32
}

/// Returns the word of the run a draw's key word folds in.
pub fn run_word(seed: Option<u64>) -> u64 {
    seed.map_or(GRAPH_DEFAULT_SEED, |seed| mix_seed(seed ^ GRAPH_SEED_SALT))
}

/// Returns the key word of a draw with the node key `key` in the stage with phase word `phase`.
///
/// For one phase and one run the word is a bijection of `key`, so two nodes of a stage never share a key word.
pub fn key_word(key: u32, phase: u32, seed: Option<u64>) -> u32 {
    pcg_hash(pcg_hash(key ^ pcg_hash(phase)) ^ fold32(run_word(seed)))
}

/// Returns the bits of a draw: sub-draw `sub_draw` at key word `key_word`, element `element` and tick `tick`.
///
/// `iteration` is `Some` when the draw sits in a region body, a static property of the draw's place. Another element,
/// tick, iteration or sub-draw gives another word.
pub fn draw_bits(key_word: u32, element: u32, tick: u32, iteration: Option<u32>, sub_draw: u32) -> u32 {
    let base = pcg_hash(key_word ^ pcg_hash(element.wrapping_mul(SPREAD) ^ pcg_hash(tick)));
    let word = match iteration {
        Some(iteration) => pcg_hash(base ^ iteration.wrapping_mul(SPREAD)),
        None => base,
    };
    if sub_draw == 0 { word } else { pcg_hash(word ^ sub_draw) }
}

/// Returns the per-press word of an action on press `press`, counted from 1.
pub fn press_word(seed: Option<u64>, press: u32) -> u32 {
    let mut word = henad_core::action::action_seed(seed) as u32;
    for _ in 0..press {
        word = word.wrapping_mul(PRESS_MULTIPLIER).wrapping_add(PRESS_INCREMENT);
    }
    word
}
