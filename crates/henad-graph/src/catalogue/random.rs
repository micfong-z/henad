//! Counter-based draws, random primitives on a given word, and the serial stream.
//!
//! The bits of a draw are [`draw_bits`](crate::draw::draw_bits) of the key word, the element, the tick and the region
//! iteration. See [`crate::draw`].

use super::cases::{CaseContext, CaseForm, CaseScope, KnownAnswer, NodeCase, case, full_case};
use super::{
    BASE, BOOL_TYPE,
    CaseValue::{Bool, F32, U32},
    ConfigType, ContextSet, Effect, F32_TYPE, NodeDef, U32_TYPE, WHEN, config, input, input_up_to, output, pinned,
};
use crate::types::Rate;

/// Configuration key of a draw: a random `u32` minted when the node is created.
const DRAW_KEY: &[super::ConfigKey] = &[config(
    "key",
    ConfigType::Integer {
        min: 0,
        max: u32::MAX as i64,
    },
)];
/// Configuration lines of the draws' cases, with their key.
const KEY_LINE: &str = "config.key = 1234\n";
/// Context of a draw at seed 7 in the tick phase.
const AT_ELEMENT_THREE: CaseContext = CaseContext {
    seed: Some(7),
    phase: 1,
    element: 3,
    tick: 5,
    ..CaseContext::NONE
};
/// [`AT_ELEMENT_THREE`] inside a region, on iteration 2.
const ON_ITERATION_TWO: CaseContext = CaseContext {
    iteration: Some(2),
    ..AT_ELEMENT_THREE
};

/// Kinds that draw from the counter-based family.
const DRAW: NodeDef = NodeDef {
    group: "Random",
    contexts: ContextSet::DRAWS,
    config: DRAW_KEY,
    effects: &[Effect::DrawsCounter],
    cost: super::NodeCost::Units(5),
    ..BASE
};

/// Kinds that work on a given word.
const PRIMITIVE: NodeDef = NodeDef {
    group: "Random primitives",
    ..BASE
};

/// Returns `random_float(bits, 1.0)`: the top 24 bits of `bits` over 2^24.
const fn unit_float(bits: u32) -> f32 {
    (bits >> 8) as f32 / 16_777_216.0
}

/// Returns a draw's case in `context`.
const fn draw_case(
    name: &'static str,
    inputs: &'static [(&'static str, super::CaseValue)],
    context: CaseContext,
    outputs: &'static [(&'static str, super::CaseValue)],
) -> KnownAnswer {
    full_case(name, CaseScope::Both, KEY_LINE, inputs, context, outputs)
}

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "rand.float",
        label: "Random number",
        semantics: "`random_float(bits, max)` of one draw: the top 24 bits over 2^24, times `max`, in `[0, max)`. The result is a pure function of the run seed, `key`, the phase, the element, the tick or press, and the region iteration.",
        inputs: &[input("max", F32_TYPE)],
        outputs: &[pinned("out", F32_TYPE, Rate::Element)],
        cases: &[
            draw_case(
                "one",
                &[("max", F32(1.0))],
                AT_ELEMENT_THREE,
                &[("out", F32(unit_float(0x6e1c_8791)))],
            ),
            draw_case(
                "scaled",
                &[("max", F32(2.0))],
                AT_ELEMENT_THREE,
                &[("out", F32(unit_float(0x6e1c_8791) * 2.0))],
            ),
            draw_case(
                "in a region",
                &[("max", F32(1.0))],
                ON_ITERATION_TWO,
                &[("out", F32(unit_float(0x3401_275b)))],
            ),
            draw_case(
                "another tick",
                &[("max", F32(1.0))],
                CaseContext {
                    tick: 6,
                    ..AT_ELEMENT_THREE
                },
                &[("out", F32(unit_float(0xae90_7a07)))],
            ),
            draw_case(
                "another element",
                &[("max", F32(1.0))],
                CaseContext {
                    element: 4,
                    ..AT_ELEMENT_THREE
                },
                &[("out", F32(unit_float(0x6295_c096)))],
            ),
            draw_case(
                "no seed",
                &[("max", F32(1.0))],
                CaseContext {
                    seed: None,
                    ..AT_ELEMENT_THREE
                },
                &[("out", F32(unit_float(0x8f52_273a)))],
            ),
        ],
        ..DRAW
    },
    NodeDef {
        id: "rand.below",
        label: "Below threshold",
        semantics: "Whether the bits of one draw are below `threshold`, compared as integers.",
        inputs: &[input("threshold", U32_TYPE)],
        outputs: &[pinned("out", BOOL_TYPE, Rate::Element)],
        cases: &[
            draw_case(
                "just above the bits",
                &[("threshold", U32(0x6e1c_8792))],
                AT_ELEMENT_THREE,
                &[("out", Bool(true))],
            ),
            draw_case(
                "at the bits",
                &[("threshold", U32(0x6e1c_8791))],
                AT_ELEMENT_THREE,
                &[("out", Bool(false))],
            ),
            draw_case(
                "in a region",
                &[("threshold", U32(0x3401_275c))],
                ON_ITERATION_TWO,
                &[("out", Bool(true))],
            ),
        ],
        ..DRAW
    },
    NodeDef {
        id: "rand.chance",
        label: "Chance",
        semantics: "`random_float(bits, 1.0) < p` of one draw, the float form of a probability. A `p` of 1 always holds, and a `p` of 0 never does.",
        inputs: &[input("p", F32_TYPE)],
        outputs: &[pinned("out", BOOL_TYPE, Rate::Element)],
        cases: &[
            draw_case("certain", &[("p", F32(1.0))], AT_ELEMENT_THREE, &[("out", Bool(true))]),
            draw_case("never", &[("p", F32(0.0))], AT_ELEMENT_THREE, &[("out", Bool(false))]),
            draw_case(
                "above the float",
                &[("p", F32(0.44))],
                AT_ELEMENT_THREE,
                &[("out", Bool(true))],
            ),
            draw_case(
                "below the float",
                &[("p", F32(0.42))],
                AT_ELEMENT_THREE,
                &[("out", Bool(false))],
            ),
        ],
        ..DRAW
    },
    NodeDef {
        id: "rng.threshold",
        label: "Probability threshold",
        semantics: "`(p * u32::MAX as f32) as u32`, computed on the host when the model is built: truncated toward zero and saturating, so 1 gives `u32::MAX`, and a negative or NaN `p` gives 0.",
        inputs: &[input_up_to("p", F32_TYPE, Rate::Parameter)],
        outputs: &[output("out", U32_TYPE)],
        cases: &[
            case("zero", &[("p", F32(0.0))], &[("out", U32(0))]),
            case("half", &[("p", F32(0.5))], &[("out", U32(0x8000_0000))]),
            case("one saturates", &[("p", F32(1.0))], &[("out", U32(u32::MAX))]),
            case("negative", &[("p", F32(-1.0))], &[("out", U32(0))]),
            case("NaN", &[("p", F32(f32::NAN))], &[("out", U32(0))]),
        ],
        ..PRIMITIVE
    },
    NodeDef {
        id: "rng.hash",
        label: "Hash",
        semantics: "`pcg_hash(x)`, the 32-bit PCG hash, with every operation wrapping.",
        inputs: &[input("x", U32_TYPE)],
        outputs: &[output("out", U32_TYPE)],
        cost: super::NodeCost::Units(2),
        cases: &[
            case("zero", &[("x", U32(0))], &[("out", U32(129_708_002))]),
            case("one", &[("x", U32(1))], &[("out", U32(2_831_084_092))]),
        ],
        ..PRIMITIVE
    },
    NodeDef {
        id: "rng.to_float",
        label: "Bits to number",
        semantics: "`random_float(bits, max)` on a given word: the top 24 bits over 2^24, times `max`, in `[0, max)`.",
        inputs: &[input("bits", U32_TYPE), input("max", F32_TYPE)],
        outputs: &[output("out", F32_TYPE)],
        cases: &[
            case("zero", &[("bits", U32(0)), ("max", F32(1.0))], &[("out", F32(0.0))]),
            case(
                "half",
                &[("bits", U32(0x8000_0000)), ("max", F32(2.0))],
                &[("out", F32(1.0))],
            ),
            case(
                "all bits stay below max",
                &[("bits", U32(u32::MAX)), ("max", F32(1.0))],
                &[("out", F32(0.999_999_94))],
            ),
            case(
                "low bits are unused",
                &[("bits", U32(0xFF)), ("max", F32(1.0))],
                &[("out", F32(0.0))],
            ),
        ],
        ..PRIMITIVE
    },
    NodeDef {
        id: "rng.below",
        label: "Bits below",
        semantics: "Whether `bits` is below `threshold`, compared as integers.",
        inputs: &[input("bits", U32_TYPE), input("threshold", U32_TYPE)],
        outputs: &[output("out", BOOL_TYPE)],
        cases: &[
            case(
                "below",
                &[("bits", U32(3)), ("threshold", U32(4))],
                &[("out", Bool(true))],
            ),
            case(
                "at",
                &[("bits", U32(4)), ("threshold", U32(4))],
                &[("out", Bool(false))],
            ),
            case(
                "zero never holds",
                &[("bits", U32(0)), ("threshold", U32(0))],
                &[("out", Bool(false))],
            ),
        ],
        ..PRIMITIVE
    },
    NodeDef {
        id: "rng.lane_seed",
        label: "Lane seed",
        semantics: "`fold32(seed.map_or(salt, |s| mix_seed(s ^ salt)))` with `fold32(x) = (x ^ (x >> 32)) as u32`, the word a stock GPU port seeds a per-element state lane from. `salt` is a `u64`, written as a string.",
        config: &[config("salt", ConfigType::U64Text)],
        outputs: &[pinned("out", U32_TYPE, Rate::Parameter)],
        cases: &[
            full_case(
                "no seed",
                CaseScope::Both,
                "config.salt = \"0x8040e00294134cd9\"\n",
                &[],
                CaseContext::NONE,
                &[("out", U32(0x1453_acdb))],
            ),
            full_case(
                "a seed",
                CaseScope::Both,
                "config.salt = \"0x8040e00294134cd9\"\n",
                &[],
                CaseContext {
                    seed: Some(7),
                    ..CaseContext::NONE
                },
                &[("out", U32(0xdd7e_0488))],
            ),
        ],
        ..PRIMITIVE
    },
    NodeDef {
        id: "stream.bits",
        group: "Random",
        label: "Next bits",
        semantics: "The next word of the stage's serial stream: advance `xorshift64`, then take the top 32 bits. With `when` false the draw consumes nothing and returns 0. A pass lists each stream node once in `draw_order`, the order each element draws them.",
        contexts: ContextSet {
            init: true,
            action: true,
            ..ContextSet::NONE
        },
        inputs: &[WHEN],
        outputs: &[pinned("bits", U32_TYPE, Rate::Element)],
        effects: &[Effect::DrawsStream],
        backends: super::Backends::HOST,
        cases: &[
            KnownAnswer {
                name: "draws",
                scope: CaseScope::Host,
                form: CaseForm::Node(NodeCase {
                    config: "",
                    inputs: &[("when", Bool(true))],
                    context: CaseContext {
                        stream: 0x1234_5678_9ABC_DEF0,
                        ..CaseContext::NONE
                    },
                    outputs: &[("bits", U32(0xfe80_0d65))],
                    stream_after: Some(0xfe80_0d65_69fa_1b4d),
                }),
            },
            KnownAnswer {
                name: "when false consumes nothing",
                scope: CaseScope::Host,
                form: CaseForm::Node(NodeCase {
                    config: "",
                    inputs: &[("when", Bool(false))],
                    context: CaseContext {
                        stream: 0x1234_5678_9ABC_DEF0,
                        ..CaseContext::NONE
                    },
                    outputs: &[("bits", U32(0))],
                    stream_after: Some(0x1234_5678_9ABC_DEF0),
                }),
            },
        ],
        ..BASE
    },
];
