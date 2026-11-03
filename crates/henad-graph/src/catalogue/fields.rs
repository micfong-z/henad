//! Reads at other elements, fields, counters and the reducers of agent models.

use super::cases::pass_case;
use super::{
    BASE,
    CaseValue::{F32, F64, U32, U64, Vec2D, Vec2F},
    ConfigDefault, ConfigKey, ConfigType, ContextSet, Domain, Effect, InputDefault, InputSocket, NodeDef, Parity,
    PassCase, SocketType, TypeClass, U32_TYPE, config, input, input_up_to, pinned,
};
use crate::types::Rate;

/// The head of an agents case: a position lane and a field `f` of two layers that takes deposits.
macro_rules! field_head {
    () => {
        r#"[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[field]]
id = "f"
type = "f32"
layers = ["a", "b"]
static = false
combine = "max"
"#
    };
}

/// A tick pass over a grid lane `$lane` that leaves it as it is, for a case of another stage.
macro_rules! leave {
    ($lane:literal) => {
        concat!(
            "\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\nid = \"n20\"\nop = \"lane.get\"\nconfig.lane = \"",
            $lane,
            "\"\n\n[[pass.node]]\nid = \"n21\"\nop = \"lane.set\"\nconfig.lane = \"",
            $lane,
            "\"\nin.value = \"n20\"\n"
        )
    };
}

const GET_AT_NEIGHBOURS: &str = r#"[[lane]]
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
op = "grid.fold"
config.order = "von_neumann"
config.boundary = "bounded"
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]

[[pass.node]]
id = "n2"
op = "lane.get_at"
config.lane = "x"
in.at = "n1.cell"
region = "n1"

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "sum"
in.value = "n2"
region = "n1"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "y"
in.value = "n1.sum"
"#;

const FIELD_AT_AGENTS: &str = concat!(
    field_head!(),
    r#"
[[lane]]
id = "out"
domain = "agents"
type = "f32"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n2"
op = "space.cell_of"
in.p = "n1"

[[pass.node]]
id = "n3"
op = "field.get"
config.field = "f"
in.layer = { layer = "f.a" }
in.at = "n2.cell"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "out"
in.value = "n3"
"#
);

const FIELD_OF_OWN_CELL: &str = concat!(
    field_head!(),
    r#"
[[pass]]
id = "case"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "field.get"
config.field = "f"

[[pass.node]]
id = "n2"
op = "stat.sum"
config.chunk_acc = "f64"
in.value = "n1"
"#
);

const FIELD_SET: &str = concat!(
    field_head!(),
    r#"
[[action]]
id = "press"
label = "Press"

[[pass]]
id = "case"
stage = "action"
action = "press"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "ctx.cell_xy"

[[pass.node]]
id = "n2"
op = "vec.split"
in.v = "n1"

[[pass.node]]
id = "n3"
op = "conv.to_f32"
in.a = "n2.x"

[[pass.node]]
id = "n4"
op = "field.set"
config.field = "f"
in.layer = { layer = "f.b" }
in.value = "n3"
"#
);

const DEPOSIT: &str = concat!(
    field_head!(),
    r#"
[[lane]]
id = "r"
domain = "agents"
type = "f32"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n2"
op = "space.cell_of"
in.p = "n1"

[[pass.node]]
id = "n3"
op = "lane.get"
config.lane = "r"

[[pass.node]]
id = "n4"
op = "field.deposit"
config.field = "f"
in.layer = { layer = "f.b" }
in.at = "n2.cell"
in.value = "n3"
"#
);

const COUNT_EVENTS: &str = r#"[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[lane]]
id = "n"
domain = "agents"
type = "u32"

[[counter]]
id = "c"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "n"

[[pass.node]]
id = "n2"
op = "counter.add"
config.counter = "c"
in.amount = "n1"
"#;

const COUNTER_TOTAL: &str = r#"[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[counter]]
id = "c"

[[pass]]
id = "case"
stage = "stats"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "stat.counter"
config.counter = "c"
"#;

/// A stats pass over a grid lane `x` of `$type`, reduced by `$op` with `chunk_acc = $acc`.
macro_rules! reduce {
    ($type:literal, $op:literal, $acc:literal) => {
        concat!(
            "[[lane]]\nid = \"x\"\ndomain = \"cells\"\ntype = \"",
            $type,
            "\"\n\n[[pass]]\nid = \"case\"\nstage = \"stats\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"lane.get\"\nconfig.lane = \"x\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"",
            $op,
            "\"\nconfig.chunk_acc = \"",
            $acc,
            "\"\nin.value = \"n1\"\n",
            leave!("x")
        )
    };
}

/// The configuration key that sets the precision of a sum within a chunk.
const CHUNK_ACC: ConfigKey = ConfigKey {
    name: "chunk_acc",
    value: ConfigType::Word(&["f32", "f64"]),
    default: Some(ConfigDefault::Word("f64")),
};

/// The `layer` input of a field kind: a layer of `config.field`, its first layer when unwritten.
const LAYER: InputSocket = InputSocket {
    name: "layer",
    value_type: SocketType::FieldLayer,
    max_rate: Rate::Pair,
    default: InputDefault::FirstLayer,
};

const IN_STATS: ContextSet = ContextSet {
    stats: true,
    ..ContextSet::NONE
};

/// Sum of four `f32` values that is exact in either precision.
const FOUR_VALUES: &[super::CaseValue] = &[F32(1.5), F32(2.25), F32(0.25), F32(4.0)];

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "lane.get_at",
        group: "State",
        label: "Read lane at",
        semantics: "The value of `lane` at the element `at`, a cell of a lane of cells or an agent of a lane of agents, as it was at the start of the pass. `at` is an inner output of an enclosing region, `cell` of a `grid.fold` or `other` of a radius fold, so another element is reached only through a region. The read makes the lane two-sided in a tick pass that writes it.",
        contexts: ContextSet {
            tick: true,
            view: true,
            stats: true,
            region: true,
            ..ContextSet::NONE
        },
        config: &[config("lane", ConfigType::LaneId)],
        inputs: &[input("at", SocketType::LaneElement)],
        outputs: &[pinned("value", SocketType::Lane, Rate::Element)],
        effects: &[Effect::ReadsOtherLane],
        cases: &[pass_case(
            "a neighbour's value",
            PassCase {
                width: 3,
                tables: GET_AT_NEIGHBOURS,
                before: &[("x", &[U32(1), U32(2), U32(4)])],
                after: &[("y", &[U32(2), U32(5), U32(2)])],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "field.get",
        group: "Fields",
        label: "Read field",
        semantics: "The value of the layer `layer` of `field` at the cell `at`, as it was at the start of the tick: an `f32`, or a variant of the field's category. `layer` is the field's first layer when unwritten. `at` is the element's own cell in a pass over cells, and is required in a pass over agents. The reference to no cell reads 0, or the first variant.",
        contexts: ContextSet {
            tick: true,
            view: true,
            stats: true,
            region: true,
            ..ContextSet::NONE
        },
        config: &[config("field", ConfigType::FieldId)],
        inputs: &[
            LAYER,
            InputSocket {
                name: "at",
                value_type: SocketType::Fixed(TypeClass::Cell),
                max_rate: Rate::Pair,
                default: InputDefault::OwnCell,
            },
        ],
        outputs: &[pinned("value", SocketType::Field, Rate::Element)],
        effects: &[Effect::ReadsField],
        cases: &[
            pass_case(
                "at a cell, and no cell",
                PassCase {
                    width: 3,
                    height: 2,
                    agents: 3,
                    tables: FIELD_AT_AGENTS,
                    before: &[
                        ("pos", &[Vec2F([2.5, 1.5]), Vec2F([0.2, 0.9]), Vec2F([5.0, 0.0])]),
                        ("f.a", &[F32(0.0), F32(1.0), F32(2.0), F32(3.0), F32(4.0), F32(5.0)]),
                    ],
                    after: &[("out", &[F32(5.0), F32(0.0), F32(0.0)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "the own cell and the first layer",
                PassCase {
                    width: 3,
                    height: 2,
                    agents: 1,
                    tables: FIELD_OF_OWN_CELL,
                    before: &[
                        ("f.a", &[F32(0.0), F32(1.0), F32(2.0), F32(3.0), F32(4.0), F32(5.0)]),
                        ("f.b", &[F32(9.0); 6]),
                    ],
                    stats: &[("n2", F64(15.0))],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "field.set",
        group: "Fields",
        label: "Write field",
        semantics: "Sets the element's own cell of the layer `layer` of `field`, committed at the end of the pass, in a pass over cells. A static field is set in init passes alone, and a field that takes deposits in init and action passes. `layer` is the field's first layer when unwritten. A pass writes each layer once per cell.",
        contexts: ContextSet {
            init: true,
            action: true,
            ..ContextSet::NONE
        },
        domain: Some(Domain::Cells),
        config: &[config("field", ConfigType::FieldId)],
        inputs: &[LAYER, input_up_to("value", SocketType::Field, Rate::Element)],
        effects: &[Effect::WritesField],
        cases: &[pass_case(
            "a layer of the own cell",
            PassCase {
                width: 3,
                height: 2,
                agents: 1,
                tables: FIELD_SET,
                before: &[("f.a", &[F32(7.0); 6])],
                after: &[
                    ("f.a", &[F32(7.0); 6]),
                    ("f.b", &[F32(0.0), F32(1.0), F32(2.0), F32(0.0), F32(1.0), F32(2.0)]),
                ],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "field.deposit",
        group: "Fields",
        label: "Deposit",
        semantics: "Contributes `max(value, 0)` to the cell `at` of the layer `layer` of `field`, a field that takes deposits. Deposits merge with the field's `combine` from 0, its identity, and land at the field update after the last tick pass, which sets each cell to the field's `finish` of the larger of the cell and its merged deposit. A deposit at the reference to no cell is dropped, and the deposit returns nothing.",
        contexts: ContextSet {
            tick: true,
            ..ContextSet::NONE
        },
        domain: Some(Domain::Agents),
        config: &[config("field", ConfigType::FieldId)],
        inputs: &[
            InputSocket {
                default: InputDefault::Required,
                ..LAYER
            },
            input("at", SocketType::Fixed(TypeClass::Cell)),
            input_up_to("value", SocketType::Fixed(TypeClass::F32), Rate::Element),
        ],
        effects: &[Effect::Deposits],
        cases: &[pass_case(
            "the larger of two, clamped at zero",
            PassCase {
                width: 3,
                height: 2,
                agents: 4,
                tables: DEPOSIT,
                before: &[
                    (
                        "pos",
                        &[
                            Vec2F([0.5, 0.5]),
                            Vec2F([0.7, 0.2]),
                            Vec2F([1.5, 0.5]),
                            Vec2F([2.5, 1.5]),
                        ],
                    ),
                    ("r", &[F32(0.5), F32(2.0), F32(0.25), F32(-1.0)]),
                ],
                deposits: &[("f.b", &[F32(2.0), F32(0.25), F32(0.0), F32(0.0), F32(0.0), F32(0.0)])],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "counter.add",
        group: "Fields",
        label: "Count event",
        semantics: "Adds `amount` to the run-long counter `counter`, wrapping at 2^32. No action resets a counter.",
        contexts: ContextSet {
            tick: true,
            ..ContextSet::NONE
        },
        config: &[config("counter", ConfigType::CounterId)],
        inputs: &[input_up_to("amount", U32_TYPE, Rate::Element)],
        effects: &[Effect::Counts],
        cases: &[pass_case(
            "wraps",
            PassCase {
                width: 10,
                height: 10,
                agents: 3,
                tables: COUNT_EVENTS,
                before: &[("n", &[U32(1), U32(2), U32(u32::MAX)])],
                counters_before: &[("c", 5)],
                counters_after: &[("c", 7)],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "stat.counter",
        group: "Statistics",
        label: "Counter total",
        semantics: "The run total of the counter `counter`, as a `u64` at the tick rate.",
        contexts: IN_STATS,
        config: &[config("counter", ConfigType::CounterId)],
        outputs: &[pinned("total", SocketType::Fixed(TypeClass::U64), Rate::Tick)],
        cases: &[pass_case(
            "reads the total",
            PassCase {
                width: 10,
                height: 10,
                agents: 1,
                tables: COUNTER_TOTAL,
                counters_before: &[("c", 9)],
                stats: &[("n1", U64(9))],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "stat.sum",
        group: "Statistics",
        label: "Sum",
        semantics: "Sum of `value` over the elements of the pass, an `f64` from an `f32` or a `vec2d` from a `vec2f`. The elements fold in index order in chunks of 8192: each chunk sums from `+0.0` in `chunk_acc`, a value widened first for `f64`, and the chunk sums fold left in `f64` from `+0.0`. The GPU folds a tree of `f32` partial sums, so parity is approximate.",
        contexts: IN_STATS,
        type_var: &[TypeClass::F32, TypeClass::Vec2F],
        config: &[CHUNK_ACC],
        inputs: &[input_up_to("value", SocketType::Var, Rate::Element)],
        outputs: &[pinned("sum", SocketType::Wide, Rate::Tick)],
        effects: &[Effect::Reduce],
        parity: Parity::Approx,
        cases: &[
            pass_case(
                "in f32",
                PassCase {
                    width: 2,
                    height: 2,
                    tables: reduce!("f32", "stat.sum", "f32"),
                    before: &[("x", FOUR_VALUES)],
                    stats: &[("n2", F64(8.0))],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "in f64",
                PassCase {
                    width: 2,
                    height: 2,
                    tables: reduce!("f32", "stat.sum", "f64"),
                    before: &[("x", FOUR_VALUES)],
                    stats: &[("n2", F64(8.0))],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "starts at positive zero",
                PassCase {
                    width: 2,
                    tables: reduce!("f32", "stat.sum", "f32"),
                    before: &[("x", &[F32(-0.0), F32(-0.0)])],
                    stats: &[("n2", F64(0.0))],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a vector",
                PassCase {
                    width: 2,
                    tables: reduce!("vec2f", "stat.sum", "f64"),
                    before: &[("x", &[Vec2F([1.0, 2.0]), Vec2F([3.0, -4.0])])],
                    stats: &[("n2", Vec2D([4.0, -2.0]))],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "stat.mean",
        group: "Statistics",
        label: "Mean",
        semantics: "`stat.sum` of `value` times `1.0 / max(n, 1)` in `f64`, where `n` is the number of elements of the pass, the form the stock statistics use.",
        contexts: IN_STATS,
        type_var: &[TypeClass::F32, TypeClass::Vec2F],
        config: &[CHUNK_ACC],
        inputs: &[input_up_to("value", SocketType::Var, Rate::Element)],
        outputs: &[pinned("mean", SocketType::Wide, Rate::Tick)],
        effects: &[Effect::Reduce],
        parity: Parity::Approx,
        cases: &[
            pass_case(
                "a scalar",
                PassCase {
                    width: 2,
                    height: 2,
                    tables: reduce!("f32", "stat.mean", "f32"),
                    before: &[("x", FOUR_VALUES)],
                    stats: &[("n2", F64(2.0))],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a vector",
                PassCase {
                    width: 2,
                    tables: reduce!("vec2f", "stat.mean", "f64"),
                    before: &[("x", &[Vec2F([1.0, 2.0]), Vec2F([3.0, -4.0])])],
                    stats: &[("n2", Vec2D([2.0, -1.0]))],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
];
