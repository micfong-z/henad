//! Statistics and view kinds.

use super::cases::{config_case, host_case_with_config, pass_case};
use super::{
    BASE, BOOL_TYPE,
    CaseValue::{F32, U32, U64, Variant},
    ConfigPorts, ConfigType, ContextSet, Effect, F32_TYPE, NodeDef, PassCase, SocketType, TypeClass, U32_TYPE, config,
    input, input_up_to, output, pinned,
};
use crate::types::Rate;

/// A tick pass that leaves a lane as it is, for a case of another stage. A grid model has exactly one tick pass.
macro_rules! leave_lane {
    ($lane:literal) => {
        concat!(
            r#"
[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n20"
op = "lane.get"
config.lane = ""#,
            $lane,
            r#""

[[pass.node]]
id = "n21"
op = "lane.set"
config.lane = ""#,
            $lane,
            r#""
in.value = "n20"
"#
        )
    };
}

const COUNT_WHERE: &str = concat!(
    r#"[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "cmp.gt"
in.a = "n1"
in.b = { u32 = 2 }

[[pass.node]]
id = "n3"
op = "stat.count_where"
in.pred = "n2"
"#,
    leave_lane!("x")
);

const COUNT_BY: &str = concat!(
    r#"[[lane]]
id = "c"
domain = "cells"
type = "cat:health"

[[pass]]
id = "case"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "c"

[[pass.node]]
id = "n2"
op = "stat.count_by"
config.category = "health"
in.value = "n1"
"#,
    leave_lane!("c")
);

const COLOR_BY_CATEGORY: &str = concat!(
    r#"[[lane]]
id = "c"
domain = "cells"
type = "cat:health"

[[pass]]
id = "case"
stage = "view"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "c"

[[pass.node]]
id = "n2"
op = "view.color"
config.palette = "health"
in.index = "n1"
"#,
    leave_lane!("c")
);

const COLOR_BY_INDEX: &str = concat!(
    r#"[[palette]]
id = "steps"
colors = [[0, 0, 0], [85, 85, 85], [170, 170, 170], [255, 255, 255]]

[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[pass]]
id = "case"
stage = "view"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "view.color"
config.palette = "steps"
in.index = "n1"
"#,
    leave_lane!("x")
);

const IN_VIEW: ContextSet = ContextSet {
    view: true,
    ..ContextSet::NONE
};

/// Configuration of the ramp's cases: a palette of four colours.
const FOUR_COLORS: &str = "config.palette = \"ramp\"\n";

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "stat.count_where",
        group: "Statistics",
        label: "Count where",
        semantics: "Number of elements of the pass where `pred` holds, as a `u64` at the tick rate.",
        contexts: ContextSet::STATS,
        inputs: &[input_up_to("pred", BOOL_TYPE, Rate::Element)],
        outputs: &[pinned("count", SocketType::Fixed(TypeClass::U64), Rate::Tick)],
        effects: &[Effect::Reduce],
        cases: &[pass_case(
            "counts",
            PassCase {
                width: 2,
                height: 2,
                tables: COUNT_WHERE,
                before: &[("x", &[U32(1), U32(2), U32(3), U32(4)])],
                stats: &[("n3", U64(2))],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "stat.count_by",
        group: "Statistics",
        label: "Count by category",
        semantics: "Number of elements per variant of `category`, one `u64` output per variant at the tick rate, named after the variant. An output no `[[stat]]` table binds is pruned.",
        contexts: ContextSet::STATS,
        config: &[config("category", ConfigType::CategoryId)],
        inputs: &[input_up_to("value", SocketType::ConfigCategory, Rate::Element)],
        config_ports: ConfigPorts::VariantCounts,
        effects: &[Effect::Reduce],
        cases: &[pass_case(
            "counts each variant",
            PassCase {
                width: 2,
                height: 2,
                tables: COUNT_BY,
                before: &[(
                    "c",
                    &[
                        Variant("health", "S"),
                        Variant("health", "I"),
                        Variant("health", "I"),
                        Variant("health", "R"),
                    ],
                )],
                stats: &[("n2.S", U64(1)), ("n2.I", U64(2)), ("n2.R", U64(1))],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "view.color",
        group: "View",
        label: "Color",
        semantics: "Sets the element's colour to entry `min(index, len - 1)` of `palette`. `index` is a `u32`, or a category value with `palette` naming that category, whose variant colours form its palette.",
        contexts: IN_VIEW,
        type_var: &[TypeClass::U32, TypeClass::Category],
        config: &[config("palette", ConfigType::PaletteId)],
        inputs: &[input_up_to("index", SocketType::Var, Rate::Element)],
        effects: &[Effect::View],
        cases: &[
            pass_case(
                "a category colours by variant",
                PassCase {
                    width: 3,
                    tables: COLOR_BY_CATEGORY,
                    before: &[(
                        "c",
                        &[Variant("health", "R"), Variant("health", "S"), Variant("health", "I")],
                    )],
                    colors: &[2, 0, 1],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "an index past the end takes the last entry",
                PassCase {
                    width: 2,
                    tables: COLOR_BY_INDEX,
                    before: &[("x", &[U32(1), U32(99)])],
                    colors: &[1, 3],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "view.ramp",
        group: "View",
        label: "Color ramp",
        semantics: "Entry of `palette` a value falls on when `low..high` is cut into one equal band per entry: `floor((value - low) / (high - low) * len)`, clamped to `0..len - 1`. With `high <= low` the result is 0. A NaN `value` returns 0 on the host and is indeterminate in a shader. A shader can assume no NaN. Parity is approximate, since the bands come from float division.",
        contexts: ContextSet {
            region: true,
            ..IN_VIEW
        },
        config: &[config("palette", ConfigType::PaletteId)],
        inputs: &[
            input("value", F32_TYPE),
            input("low", F32_TYPE),
            input("high", F32_TYPE),
        ],
        outputs: &[output("index", U32_TYPE)],
        parity: super::Parity::Approx,
        cost: super::NodeCost::Units(4),
        cases: &[
            config_case(
                "the band a value falls on",
                FOUR_COLORS,
                &[("value", F32(0.5)), ("low", F32(0.0)), ("high", F32(1.0))],
                &[("index", U32(2))],
            ),
            config_case(
                "the top of the range takes the last entry",
                FOUR_COLORS,
                &[("value", F32(1.0)), ("low", F32(0.0)), ("high", F32(1.0))],
                &[("index", U32(3))],
            ),
            config_case(
                "below the range",
                FOUR_COLORS,
                &[("value", F32(-1.0)), ("low", F32(0.0)), ("high", F32(1.0))],
                &[("index", U32(0))],
            ),
            config_case(
                "an empty range",
                FOUR_COLORS,
                &[("value", F32(0.5)), ("low", F32(1.0)), ("high", F32(1.0))],
                &[("index", U32(0))],
            ),
            host_case_with_config(
                "NaN",
                FOUR_COLORS,
                &[("value", F32(f32::NAN)), ("low", F32(0.0)), ("high", F32(1.0))],
                &[("index", U32(0))],
            ),
        ],
        ..BASE
    },
];
