//! Counter-based random draws: the words a `rand.*` node's bits are a pure function of.
//!
//! The bits of a draw are `core(k, e, t, i, s)`. `k` is the *key word*, which the host computes once per node per stage
//! from the node's `key`, the stage's phase and the run seed. `e` is the element, `t` the tick or press, `i` the region
//! iteration word, and `s` the sub-draw of a node that takes more than one word.
//!
//! The core is [`draw_core`], two rounds of [`pcg4d`] over `[k, e, t, (i << 4) | (s >> 2)]`, of which sub-draw `s`
//! takes word `s & 3`. [`pcg4d`] is the hash of henad-core's random primitives, and the WGSL twin imports it from
//! `henad::rng`.
//!
//! The *phase* is a word that keeps the stages' draws apart: [`PHASE_INIT`] in the init stage, [`PHASE_TICK`] in the
//! tick stage, and [`action_phase`] of the action's position in an action, a rerun included. `t` is the tick in a tick
//! pass, the press count in an action, and 0 in an init pass. Inside a region, `i` is the iteration `k`, and nested
//! regions combine their iterations in mixed radix: `i = i_outer * max + k`, with `max` the inner region's bound.

pub use henad_core::authoring::primitives::rng::pcg4d;
use henad_core::authoring::primitives::rng::{mix_seed, pcg_hash};

/// WGSL module `henad_graph::draw`, the twin of [`draw_core`] that a generated shader imports.
pub const DRAW_WGSL: &str = include_str!("draw.wgsl");

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

/// Returns the bits of sub-draw `sub_draw` at key word `key_word`, element `element`, tick `tick` and iteration word
/// `iteration`.
///
/// This is `pcg4d(pcg4d([k, e, t, (i << 4) | (s >> 2)]))[s & 3]`, so one pair of rounds serves four sub-draws. The
/// fourth word is injective for an `iteration` below 2^28 and a `sub_draw` below 64, and the cost limit keeps
/// `iteration` below 2^16.
pub fn draw_core(key_word: u32, element: u32, tick: u32, iteration: u32, sub_draw: u32) -> u32 {
    let words = pcg4d(pcg4d([key_word, element, tick, (iteration << 4) | (sub_draw >> 2)]));
    words[(sub_draw & 3) as usize]
}

/// Returns the bits of a draw: sub-draw `sub_draw` at key word `key_word`, element `element` and tick `tick`.
///
/// `iteration` is `Some` when the draw sits in a region body, a static property of the draw's place, and a draw outside
/// every region reads iteration word 0. Another element, tick, iteration or sub-draw gives another word.
pub fn draw_bits(key_word: u32, element: u32, tick: u32, iteration: Option<u32>, sub_draw: u32) -> u32 {
    draw_core(key_word, element, tick, iteration.unwrap_or(0), sub_draw)
}

/// Returns the per-press word of an action on press `press`, counted from 1.
pub fn press_word(seed: Option<u64>, press: u32) -> u32 {
    let mut word = henad_core::action::action_seed(seed) as u32;
    for _ in 0..press {
        word = word.wrapping_mul(PRESS_MULTIPLIER).wrapping_add(PRESS_INCREMENT);
    }
    word
}

#[cfg(test)]
mod tests {
    use super::{draw_core, pcg4d};

    /// The core's words are pinned, since the `rand.*` case words and the library lock are drawn from them.
    #[test]
    fn the_draw_core_keeps_its_words() {
        assert_eq!(
            pcg4d([0, 0, 0, 0]),
            [251_852_841, 760_645_481, 850_445_371, 3_542_436_074]
        );
        assert_eq!(
            pcg4d([1, 2, 3, 4]),
            [908_250_390, 4_044_648_920, 3_775_961_919, 45_698_095]
        );
        assert_eq!(draw_core(0, 0, 0, 0, 0), 629_284_576);
        assert_eq!(draw_core(1, 2, 3, 4, 5), 1_535_565_428);
        assert_eq!(draw_core(0xDEAD_BEEF, 1023, 77, 6, 3), 1_008_266_312);
        assert_eq!(draw_core(u32::MAX, u32::MAX, u32::MAX, 0, 63), 904_119_187);
    }
}
