//! Region kinds, and the grid kinds that read a stencil.

use super::cases::{config_case, pass_case};
use super::{
    BASE,
    CaseValue::{Bool, U32, Variant},
    ConfigDefault, ConfigKey, ConfigPorts, ConfigType, ContextSet, Effect, InputDefault, InputSocket, NodeCost,
    NodeDef, PassCase, SocketType, TypeClass, U32_TYPE, WHEN, config, input, input_up_to, output, pinned,
};
use crate::document::Domain;
use crate::limits::MAX_REPEAT_ITERATIONS;
use crate::types::Rate;

/// Prefix of the Repeat cases: a lane `x` and the head of a pass.
macro_rules! repeat_head {
    () => {
        r#"[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"
"#
    };
}

/// A Repeat with one `sum` accumulator that adds `k`, whose result lands in the lane `x`. `$inputs` writes the Repeat's
/// inputs and `$max` its bound, and `$extra` adds nodes numbered from `n10`.
macro_rules! sum_of_k {
    ($max:literal, $inputs:literal, $extra:literal) => {
        concat!(
            repeat_head!(),
            r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = "#,
            $max,
            r#"
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
"#,
            $inputs,
            r#"

[[pass.node]]
id = "n2"
op = "region.put"
config.acc = "sum"
in.value = "n1.k"
region = "n1"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n1.sum"
"#,
            $extra
        )
    };
}

/// A Repeat with one accumulator of each of the combines `min`, `max`, `or` and `and`. `$count` sets its count.
macro_rules! every_combine {
    ($count:literal) => {
        concat!(
            r#"[[lane]]
id = "low"
domain = "cells"
type = "u32"

[[lane]]
id = "high"
domain = "cells"
type = "u32"

[[lane]]
id = "any"
domain = "cells"
type = "bool"

[[lane]]
id = "all"
domain = "cells"
type = "bool"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [
  { name = "low", type = "u32", combine = "min" },
  { name = "high", type = "u32", combine = "max" },
  { name = "any", type = "bool", combine = "or" },
  { name = "all", type = "bool", combine = "and" },
]
in.count = { u32 = "#,
            $count,
            r#" }

[[pass.node]]
id = "n2"
op = "math.add"
in.a = "n1.k"
in.b = { u32 = 1 }
region = "n1"

[[pass.node]]
id = "n3"
op = "cmp.eq"
in.a = "n1.k"
in.b = { u32 = 2 }
region = "n1"

[[pass.node]]
id = "n4"
op = "cmp.lt"
in.a = "n1.k"
in.b = { u32 = 2 }
region = "n1"

[[pass.node]]
id = "n5"
op = "region.put"
config.acc = "low"
in.value = "n2"
region = "n1"

[[pass.node]]
id = "n6"
op = "region.put"
config.acc = "high"
in.value = "n2"
region = "n1"

[[pass.node]]
id = "n7"
op = "region.put"
config.acc = "any"
in.value = "n3"
region = "n1"

[[pass.node]]
id = "n8"
op = "region.put"
config.acc = "all"
in.value = "n4"
region = "n1"

[[pass.node]]
id = "n11"
op = "lane.set"
config.lane = "low"
in.value = "n1.low"

[[pass.node]]
id = "n12"
op = "lane.set"
config.lane = "high"
in.value = "n1.high"

[[pass.node]]
id = "n13"
op = "lane.set"
config.lane = "any"
in.value = "n1.any"

[[pass.node]]
id = "n14"
op = "lane.set"
config.lane = "all"
in.value = "n1.all"
"#
        )
    };
}

const CARRY_DOUBLES: &str = concat!(
    repeat_head!(),
    r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "doubled", type = "u32", combine = "carry" }]
in.count = { u32 = 5 }
in.init.doubled = { u32 = 1 }

[[pass.node]]
id = "n2"
op = "math.mul"
in.a = "n1.prev.doubled"
in.b = { u32 = 2 }
region = "n1"

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "doubled"
in.value = "n2"
region = "n1"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "x"
in.value = "n1.doubled"
"#
);

const EARLY_EXIT: &str = concat!(
    repeat_head!(),
    r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [
  { name = "going", type = "bool", combine = "carry" },
  { name = "steps", type = "u32", combine = "count" },
]
in.count = { u32 = 6 }
in.init.going = { bool = true }

[[pass.node]]
id = "n2"
op = "cmp.lt"
in.a = "n1.k"
in.b = { u32 = 2 }
region = "n1"

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "going"
in.value = "n2"
in.when = "n1.prev.going"
region = "n1"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "steps"
in.when = "n1.prev.going"
region = "n1"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "x"
in.value = "n1.steps"
"#
);

const NESTED_COUNT: &str = concat!(
    repeat_head!(),
    r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 3
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = { u32 = 3 }

[[pass.node]]
id = "n2"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "steps", type = "u32", combine = "count" }]
in.count = "n1.k"
region = "n1"

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "steps"
region = "n2"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "sum"
in.value = "n2.steps"
region = "n1"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "x"
in.value = "n1.sum"
"#
);

const TOTAL_OF_ELEMENT: &str = concat!(
    repeat_head!(),
    r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = "n4"

[[pass.node]]
id = "n2"
op = "region.put"
config.acc = "sum"
in.value = "n1.total"
region = "n1"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n1.sum"

[[pass.node]]
id = "n4"
op = "ctx.index"
"#
);

const PUT_WHEN: &str = concat!(
    repeat_head!(),
    r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = { u32 = 4 }

[[pass.node]]
id = "n2"
op = "bits.and"
in.a = "n1.k"
in.b = { u32 = 1 }
region = "n1"

[[pass.node]]
id = "n3"
op = "cmp.eq"
in.a = "n2"
in.b = { u32 = 1 }
region = "n1"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "sum"
in.value = "n1.k"
in.when = "n3"
region = "n1"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "x"
in.value = "n1.sum"
"#
);

const PUT_COUNT: &str = concat!(
    repeat_head!(),
    r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "steps", type = "u32", combine = "count" }]
in.count = { u32 = 3 }
in.init.steps = { u32 = 10 }

[[pass.node]]
id = "n2"
op = "region.put"
config.acc = "steps"
region = "n1"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n1.steps"
"#
);

const REPEAT_IN_INIT: &str = r#"[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "init"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "sum", type = "u32", combine = "sum" }]
in.count = { u32 = 3 }

[[pass.node]]
id = "n2"
op = "region.put"
config.acc = "sum"
in.value = "n1.k"
region = "n1"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n1.sum"

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n20"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n21"
op = "lane.set"
config.lane = "x"
in.value = "n20"
"#;

/// Returns tables holding a lane `c` of `health`, a lane `n` of `u32`, and a pass that writes one `grid.count` to `n`.
macro_rules! count_grid {
    ($config:literal, $when:literal) => {
        concat!(
            r#"[[lane]]
id = "c"
domain = "cells"
type = "cat:health"

[[lane]]
id = "n"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "grid.count"
config.lane = "c"
"#,
            $config,
            r#"
in.value = { cat = "health.I" }
in.when = { bool = "#,
            $when,
            r#" }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "n"
in.value = "n1"
"#
        )
    };
}

/// Two infected cells on a 3 by 3 grid, at (0, 0) and (1, 1).
const TWO_INFECTED: &[crate::catalogue::CaseValue] = &[
    Variant("health", "I"),
    Variant("health", "S"),
    Variant("health", "S"),
    Variant("health", "S"),
    Variant("health", "I"),
    Variant("health", "S"),
    Variant("health", "S"),
    Variant("health", "S"),
    Variant("health", "S"),
];

/// Returns a 3 by 3 case of `grid.count` over [`TWO_INFECTED`], on a bounded grid when `$bounded` holds.
macro_rules! count_case {
    ($tables:expr, $bounded:literal, $after:expr $(,)?) => {
        PassCase {
            width: 3,
            height: 3,
            bounded: $bounded,
            tables: $tables,
            before: &[("c", TWO_INFECTED)],
            after: &[("n", $after)],
            ..PassCase::EMPTY
        }
    };
}

const STENCIL_CONTEXTS: ContextSet = ContextSet {
    tick: true,
    view: true,
    stats: true,
    region: true,
    ..ContextSet::NONE
};

const COUNT_CONFIG: &[ConfigKey] = &[
    config("lane", ConfigType::LaneId),
    ConfigKey {
        name: "neighborhood",
        value: ConfigType::Word(&["moore", "von_neumann"]),
        default: Some(ConfigDefault::Word("moore")),
    },
    ConfigKey {
        name: "include_center",
        value: ConfigType::Bool,
        default: Some(ConfigDefault::Bool(false)),
    },
];

/// Configuration of Conway's rule, as the life-like rule's cases write it.
const B3S23: &str = "config.category = \"life\"\nconfig.birth = [3]\nconfig.survive = [2, 3]\nconfig.dead = \"Dead\"\nconfig.alive = \"Alive\"\n";

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "region.repeat",
        group: "Regions",
        label: "Repeat",
        semantics: "Runs the body `min(count, max)` times with `k` ascending from 0. The inner output `total` holds the iterations this element runs, and `prev.<acc>` the accumulator's value before this iteration. With zero iterations, or with `when` false, every accumulator returns its initial value: `init.<acc>` when written, otherwise the identity of its combine, which is 0 for `sum` and `count`, the largest value for `min`, the smallest for `max`, false for `or` and true for `and`. `carry` needs `init.<acc>`. The body reads no other element through `k`. Writes, deposits and counters stay outside the body, whose only effect is its accumulators. Float results are deterministic on one device, since the order is fixed.",
        is_region: true,
        config: &[
            config(
                "max",
                ConfigType::Integer {
                    min: 1,
                    max: MAX_REPEAT_ITERATIONS as i64,
                },
            ),
            config("acc", ConfigType::Accumulators),
        ],
        inputs: &[input("count", U32_TYPE), WHEN],
        inner_outputs: &[pinned("k", U32_TYPE, Rate::Pair), pinned("total", U32_TYPE, Rate::Pair)],
        config_ports: ConfigPorts::Accumulators,
        cost: NodeCost::Repeat,
        cases: &[
            pass_case(
                "sums k",
                PassCase {
                    tables: sum_of_k!("8", "in.count = { u32 = 4 }", ""),
                    after: &[("x", &[U32(6)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "zero iterations return the initial value",
                PassCase {
                    tables: sum_of_k!("8", "in.count = { u32 = 0 }\nin.init.sum = { u32 = 5 }", ""),
                    after: &[("x", &[U32(5)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "when false returns the initial value",
                PassCase {
                    tables: sum_of_k!(
                        "8",
                        "in.count = { u32 = 4 }\nin.when = { bool = false }\nin.init.sum = { u32 = 5 }",
                        ""
                    ),
                    after: &[("x", &[U32(5)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a count past max clamps",
                PassCase {
                    tables: sum_of_k!("3", "in.count = { u32 = 100 }", ""),
                    after: &[("x", &[U32(3)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "the count varies per element",
                PassCase {
                    width: 4,
                    tables: sum_of_k!(
                        "8",
                        "in.count = \"n10\"",
                        "\n[[pass.node]]\nid = \"n10\"\nop = \"ctx.index\"\n"
                    ),
                    after: &[("x", &[U32(0), U32(0), U32(1), U32(3)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a nested count reads the outer k",
                PassCase {
                    tables: NESTED_COUNT,
                    after: &[("x", &[U32(3)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "carry feeds the next iteration",
                PassCase {
                    tables: CARRY_DOUBLES,
                    after: &[("x", &[U32(32)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a carry flag exits early",
                PassCase {
                    tables: EARLY_EXIT,
                    after: &[("x", &[U32(3)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "total is the element's count",
                PassCase {
                    width: 3,
                    tables: TOTAL_OF_ELEMENT,
                    after: &[("x", &[U32(0), U32(1), U32(4)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "every combine",
                PassCase {
                    tables: every_combine!("3"),
                    after: &[
                        ("low", &[U32(1)]),
                        ("high", &[U32(3)]),
                        ("any", &[Bool(true)]),
                        ("all", &[Bool(false)]),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "identities with zero iterations",
                PassCase {
                    tables: every_combine!("0"),
                    after: &[
                        ("low", &[U32(u32::MAX)]),
                        ("high", &[U32(0)]),
                        ("any", &[Bool(false)]),
                        ("all", &[Bool(true)]),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "in an init pass",
                PassCase {
                    tables: REPEAT_IN_INIT,
                    after: &[("x", &[U32(3)])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "region.put",
        group: "Regions",
        label: "Accumulate",
        semantics: "Feeds the accumulator `acc` of the enclosing region, once per iteration, when `when` holds. `sum` adds `value`, `count` adds `value`, which defaults to 1, `min` and `max` keep the smaller and the larger value per component, `or` and `and` combine truth values, and `carry` replaces the value. With `when` false the accumulator keeps its value.",
        config: &[config("acc", ConfigType::AccumulatorName)],
        inputs: &[
            InputSocket {
                name: "value",
                value_type: SocketType::Accumulator,
                max_rate: Rate::Pair,
                default: InputDefault::Optional,
            },
            WHEN,
        ],
        effects: &[Effect::RegionSink],
        cases: &[
            pass_case(
                "when skips an iteration",
                PassCase {
                    tables: PUT_WHEN,
                    after: &[("x", &[U32(4)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "count adds one from its start",
                PassCase {
                    tables: PUT_COUNT,
                    after: &[("x", &[U32(13)])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "grid.count",
        group: "Grid",
        label: "Count neighbors",
        semantics: "Number of stencil positions around the element whose `lane` value equals `value`. `neighborhood` is `moore`, 8 positions, or `von_neumann`, 4, and `include_center` adds the element itself. On a torus each position counts, so a grid narrower than the stencil reaches one cell twice. Past a bounded edge a position is skipped. With `when` false the count is 0. The read sees the start of the pass, and makes the lane dual when its pass also writes it.",
        contexts: STENCIL_CONTEXTS,
        domain: Some(Domain::Cells),
        type_var: &[TypeClass::Category, TypeClass::U32],
        config: COUNT_CONFIG,
        inputs: &[input_up_to("value", SocketType::Var, Rate::Tick), WHEN],
        outputs: &[pinned("count", U32_TYPE, Rate::Element)],
        effects: &[Effect::ReadsOtherLane],
        cost: NodeCost::Units(9),
        cases: &[
            pass_case(
                "moore on a torus",
                count_case!(
                    count_grid!("config.neighborhood = \"moore\"\nconfig.include_center = false", "true"),
                    false,
                    &[U32(1), U32(2), U32(2), U32(2), U32(1), U32(2), U32(2), U32(2), U32(2)],
                ),
            ),
            pass_case(
                "von neumann on a torus",
                count_case!(
                    count_grid!(
                        "config.neighborhood = \"von_neumann\"\nconfig.include_center = false",
                        "true"
                    ),
                    false,
                    &[U32(0), U32(2), U32(1), U32(2), U32(0), U32(1), U32(1), U32(1), U32(0)],
                ),
            ),
            pass_case(
                "the center counts when included",
                count_case!(
                    count_grid!("config.neighborhood = \"moore\"\nconfig.include_center = true", "true"),
                    false,
                    &[U32(2), U32(2), U32(2), U32(2), U32(2), U32(2), U32(2), U32(2), U32(2)],
                ),
            ),
            pass_case(
                "when false counts nothing",
                count_case!(
                    count_grid!(
                        "config.neighborhood = \"moore\"\nconfig.include_center = false",
                        "false"
                    ),
                    false,
                    &[U32(0), U32(0), U32(0), U32(0), U32(0), U32(0), U32(0), U32(0), U32(0)],
                ),
            ),
            pass_case(
                "a bounded edge skips positions",
                count_case!(
                    count_grid!("config.neighborhood = \"moore\"\nconfig.include_center = false", "true"),
                    true,
                    &[U32(1), U32(2), U32(1), U32(2), U32(1), U32(1), U32(1), U32(1), U32(1)],
                ),
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "grid.life_like",
        group: "Grid",
        label: "Life-like rule",
        semantics: "Two-state outer-totalistic rule: `alive` when `cell` is `alive` and `count` is in `survive`, or when `cell` is anything else and `count` is in `birth`, otherwise `dead`. `birth` and `survive` hold counts from 0 to 9, and a `count` past 9 is in neither.",
        config: &[
            config("category", ConfigType::CategoryId),
            config("birth", ConfigType::CountList { max: 9 }),
            config("survive", ConfigType::CountList { max: 9 }),
            config("dead", ConfigType::VariantName),
            config("alive", ConfigType::VariantName),
        ],
        inputs: &[input("cell", SocketType::ConfigCategory), input("count", U32_TYPE)],
        outputs: &[output("next", SocketType::ConfigCategory)],
        cost: NodeCost::Units(2),
        cases: &[
            config_case(
                "survives",
                B3S23,
                &[("cell", Variant("life", "Alive")), ("count", U32(2))],
                &[("next", Variant("life", "Alive"))],
            ),
            config_case(
                "dies",
                B3S23,
                &[("cell", Variant("life", "Alive")), ("count", U32(4))],
                &[("next", Variant("life", "Dead"))],
            ),
            config_case(
                "born",
                B3S23,
                &[("cell", Variant("life", "Dead")), ("count", U32(3))],
                &[("next", Variant("life", "Alive"))],
            ),
            config_case(
                "stays dead",
                B3S23,
                &[("cell", Variant("life", "Dead")), ("count", U32(2))],
                &[("next", Variant("life", "Dead"))],
            ),
            config_case(
                "a count past the masks",
                "config.category = \"life\"\nconfig.birth = [3]\nconfig.survive = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]\nconfig.dead = \"Dead\"\nconfig.alive = \"Alive\"\n",
                &[("cell", Variant("life", "Alive")), ("count", U32(35))],
                &[("next", Variant("life", "Dead"))],
            ),
        ],
        ..BASE
    },
];
