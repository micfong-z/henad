//! Context and lane kinds.

use super::cases::{CaseContext, CaseScope, full_case, pass_case};
use super::{
    BASE, CaseValue::U32, ConfigType, ContextSet, Effect, NodeDef, PassCase, SocketType, U32_TYPE, config, input_up_to,
    pinned,
};
use crate::types::Rate;

/// A lane `x` and a pass that adds one to it.
const ADD_ONE: &str = r#"[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "math.add"
in.a = "n1"
in.b = { u32 = 1 }

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n2"
"#;

/// A pass that writes a neighbour's read: each cell takes its index, and the read of `x` sees the start of the pass.
const READ_THE_START: &str = r#"[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[lane]]
id = "y"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "ctx.index"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n2"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "y"
in.value = "n1"
"#;

const IN_AN_ACTION: ContextSet = ContextSet {
    action: true,
    region: true,
    ..ContextSet::NONE
};

/// Passes that write lanes: init, tick and action passes, outside region bodies.
const WRITES: ContextSet = ContextSet {
    init: true,
    tick: true,
    action: true,
    ..ContextSet::NONE
};

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "ctx.index",
        group: "Context",
        label: "Element index",
        semantics: "Index of the element in the pass domain: the row-major cell index or the agent slot.",
        outputs: &[pinned("index", U32_TYPE, Rate::Element)],
        cases: &[full_case(
            "the element",
            CaseScope::Both,
            "",
            &[],
            CaseContext {
                element: 5,
                ..CaseContext::NONE
            },
            &[("index", U32(5))],
        )],
        ..BASE
    },
    NodeDef {
        id: "ctx.tick",
        group: "Context",
        label: "Tick",
        semantics: "Tick count as a wrapping `u32`: the steps done before this one in a tick pass, the steps done in a view or stats pass and at the press in an action pass, and 0 in an init pass.",
        outputs: &[pinned("tick", U32_TYPE, Rate::Tick)],
        cases: &[full_case(
            "the tick",
            CaseScope::Both,
            "",
            &[],
            CaseContext {
                tick: 9,
                ..CaseContext::NONE
            },
            &[("tick", U32(9))],
        )],
        ..BASE
    },
    NodeDef {
        id: "ctx.press_seed",
        group: "Context",
        label: "Press seed",
        semantics: "The per-press word of an action. The word starts as `action_seed(seed) as u32` and advances by `x * 747796405 + 2891336453`, wrapping, before every press, so the first press reads the word after one advance.",
        contexts: IN_AN_ACTION,
        outputs: &[pinned("seed", U32_TYPE, Rate::Tick)],
        cases: &[
            full_case(
                "first press",
                CaseScope::Both,
                "",
                &[],
                CaseContext {
                    seed: Some(7),
                    press: 1,
                    ..CaseContext::NONE
                },
                &[("seed", U32(PRESS_ONE))],
            ),
            full_case(
                "second press",
                CaseScope::Both,
                "",
                &[],
                CaseContext {
                    seed: Some(7),
                    press: 2,
                    ..CaseContext::NONE
                },
                &[("seed", U32(PRESS_TWO))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "lane.get",
        group: "State",
        label: "Read lane",
        semantics: "The element's own value of `lane` at the start of the pass. A write in the same pass commits at its end, so the read does not see it.",
        config: &[config("lane", ConfigType::LaneId)],
        outputs: &[pinned("value", SocketType::Lane, Rate::Element)],
        effects: &[Effect::ReadsLane],
        cases: &[
            pass_case(
                "reads the element",
                PassCase {
                    width: 2,
                    tables: ADD_ONE,
                    before: &[("x", &[U32(1), U32(5)])],
                    after: &[("x", &[U32(2), U32(6)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "reads the start of the pass",
                PassCase {
                    width: 2,
                    tables: READ_THE_START,
                    before: &[("x", &[U32(7), U32(8)])],
                    after: &[("x", &[U32(0), U32(1)]), ("y", &[U32(7), U32(8)])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "lane.set",
        group: "State",
        label: "Write lane",
        semantics: "Sets the element's new value of `lane`, committed at the end of the pass. A pass writes a lane at most once per element, and an unwritten lane keeps its value.",
        contexts: WRITES,
        config: &[config("lane", ConfigType::LaneId)],
        inputs: &[input_up_to("value", SocketType::Lane, Rate::Element)],
        effects: &[Effect::WritesLane],
        cases: &[pass_case(
            "writes the element",
            PassCase {
                width: 2,
                tables: ADD_ONE,
                before: &[("x", &[U32(0), U32(u32::MAX)])],
                after: &[("x", &[U32(1), U32(0)])],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
];

/// Word of `ctx.press_seed` at seed 7 on the first press.
const PRESS_ONE: u32 = 0x923f_9d35;
/// Word of `ctx.press_seed` at seed 7 on the second press.
const PRESS_TWO: u32 = 0x699e_147e;
