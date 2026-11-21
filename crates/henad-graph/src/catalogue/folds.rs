//! The stencil fold and the radius fold: regions whose iterations visit other cells or other agents.

use henad_core::authoring::primitives::space::{MOORE_COLUMN_MAJOR, MOORE_ROW_MAJOR, VON_NEUMANN};

use super::cases::pass_case;
use super::space::BOUNDARY;
use super::{
    BASE, BOOL_TYPE,
    CaseValue::{F32, U32, Vec2F},
    ConfigDefault, ConfigKey, ConfigPorts, ConfigType, ContextSet, Domain, Effect, F32_TYPE, InputDefault, InputSocket,
    NodeCost, NodeDef, Parity, PassCase, SocketType, TypeClass, U32_TYPE, WHEN, config, input, input_up_to, pinned,
};
use crate::node::Node;
use crate::types::Rate;
use crate::value::ConfigValue;

/// Returns the stencil of a `grid.fold` node in its table order: the centre first when `config.include_center` holds,
/// then the positions of `config.order`.
///
/// An order the file does not name gives the row-major Moore table.
pub fn stencil_offsets(node: &Node) -> Vec<(i32, i32)> {
    let order = node.config_str("order").unwrap_or("moore_row_major");
    let center = node
        .config
        .get("include_center")
        .and_then(ConfigValue::as_bool)
        .unwrap_or(false);
    let table: &[(i32, i32)] = match order {
        "moore_column_major" => &MOORE_COLUMN_MAJOR,
        "von_neumann" => &VON_NEUMANN,
        _ => &MOORE_ROW_MAJOR,
    };
    let mut offsets = Vec::with_capacity(table.len() + 1);
    if center {
        offsets.push((0, 0));
    }
    offsets.extend_from_slice(table);
    offsets
}

/// A grid of 3 by 3 cells whose fold of `$order` records the step code of table position 1, and counts positions.
macro_rules! second_position {
    ($order:literal, $center:literal, $boundary:literal) => {
        concat!(
            r#"[[lane]]
id = "second"
domain = "cells"
type = "u32"

[[lane]]
id = "count"
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
config.order = ""#,
            $order,
            r#""
config.include_center = "#,
            $center,
            r#"
config.boundary = ""#,
            $boundary,
            r#""
config.acc = [
  { name = "second", type = "u32", combine = "carry" },
  { name = "count", type = "u32", combine = "count" },
]
in.init.second = { u32 = 99 }

[[pass.node]]
id = "n2"
op = "space.step_code"
in.d = "n1.offset"
region = "n1"

[[pass.node]]
id = "n3"
op = "cmp.eq"
in.a = "n1.k"
in.b = { u32 = 1 }
region = "n1"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "second"
in.value = "n2"
in.when = "n3"
region = "n1"

[[pass.node]]
id = "n5"
op = "region.put"
config.acc = "count"
region = "n1"

[[pass.node]]
id = "n6"
op = "lane.set"
config.lane = "second"
in.value = "n1.second"

[[pass.node]]
id = "n7"
op = "lane.set"
config.lane = "count"
in.value = "n1.count"
"#
        )
    };
}

/// Agents that take the largest field value of the 3 by 3 cells around their cell, on a bounded field grid.
const FIELD_MAXIMUM: &str = r#"[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[lane]]
id = "best"
domain = "agents"
type = "f32"

[[field]]
id = "f"
type = "f32"
layers = ["value"]
static = true

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
op = "grid.fold"
config.order = "moore_row_major"
config.include_center = true
config.boundary = "bounded"
config.acc = [{ name = "best", type = "f32", combine = "max" }]
in.at = "n2.xy"

[[pass.node]]
id = "n4"
op = "field.get"
config.field = "f"
in.at = "n3.cell"
region = "n3"

[[pass.node]]
id = "n5"
op = "region.put"
config.acc = "best"
in.value = "n4"
region = "n3"

[[pass.node]]
id = "n6"
op = "lane.set"
config.lane = "best"
in.value = "n3.best"
"#;

/// Agents on a 10 by 10 torus that count the others within 2 world units, sum their `w`, their deltas and their squared
/// distances. `$exclude` sets `exclude_self`.
macro_rules! neighbours {
    ($exclude:literal) => {
        concat!(
            r#"[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[lane]]
id = "w"
domain = "agents"
type = "u32"

[[lane]]
id = "count"
domain = "agents"
type = "u32"

[[lane]]
id = "sum"
domain = "agents"
type = "u32"

[[lane]]
id = "delta"
domain = "agents"
type = "vec2f"

[[lane]]
id = "dist"
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
op = "agent.radius_fold"
config.exclude_self = "#,
            $exclude,
            r#"
config.acc = [
  { name = "count", type = "u32", combine = "count" },
  { name = "sum", type = "u32", combine = "sum" },
  { name = "deltas", type = "vec2f", combine = "sum" },
  { name = "dist", type = "f32", combine = "sum" },
]
in.center = "n1"
in.radius = { f32 = 2.0 }
in.cell_size = { f32 = 1.0 }

[[pass.node]]
id = "n3"
op = "lane.get_at"
config.lane = "w"
in.at = "n2.other"
region = "n2"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "count"
region = "n2"

[[pass.node]]
id = "n5"
op = "region.put"
config.acc = "sum"
in.value = "n3"
region = "n2"

[[pass.node]]
id = "n6"
op = "region.put"
config.acc = "deltas"
in.value = "n2.delta"
region = "n2"

[[pass.node]]
id = "n7"
op = "region.put"
config.acc = "dist"
in.value = "n2.dist_sq"
region = "n2"

[[pass.node]]
id = "n8"
op = "lane.set"
config.lane = "count"
in.value = "n2.count"

[[pass.node]]
id = "n9"
op = "lane.set"
config.lane = "sum"
in.value = "n2.sum"

[[pass.node]]
id = "n10"
op = "lane.set"
config.lane = "delta"
in.value = "n2.deltas"

[[pass.node]]
id = "n11"
op = "lane.set"
config.lane = "dist"
in.value = "n2.dist"
"#
        )
    };
}

/// Positions of the four agents of [`neighbours`].
const FOUR_AGENTS: &[super::CaseValue] = &[
    Vec2F([1.0, 1.0]),
    Vec2F([2.0, 1.0]),
    Vec2F([9.0, 1.0]),
    Vec2F([5.0, 5.0]),
];
/// Values of `w` of the four agents of [`neighbours`].
const FOUR_WEIGHTS: &[super::CaseValue] = &[U32(1), U32(10), U32(100), U32(1000)];

/// The input `at` of a stencil fold: a column and row, the element's own cell over cells, required over agents.
const AT_CELL: InputSocket = InputSocket {
    name: "at",
    value_type: SocketType::Fixed(TypeClass::Vec2I),
    max_rate: Rate::Pair,
    default: InputDefault::OwnCell,
};

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "grid.fold",
        group: "Grid",
        label: "Neighborhood fold",
        semantics: "Runs the body once per stencil position around the column and row `at` of the domain grid, in the table order of `order`: `moore_row_major` (rows outer, as `henad::space::MOORE_ROW_MAJOR`), `moore_column_major` (columns outer) or `von_neumann` (up, left, right, down), with the centre first when `include_center` holds. `at` is the element's own cell in a pass over cells, and is required in a pass over agents. `boundary` is `torus`, where a position wraps, `bounded`, where a position off the grid is skipped, or `domain`, the grid's `boundary` on a grid topology and the torus on an agents topology. Inner outputs: `k`, the table position, which counts skipped positions too; `offset`, the step; `xy`, the position after wrapping; `cell`, its cell; `diagonal`, whether both steps are nonzero; and `prev.<acc>`. Accumulators are those of `region.repeat`. With `when` false, or with every position skipped, every accumulator returns its initial value. A draw in the body reads `k` as its iteration word, and a fold nested in a Repeat combines them in mixed radix with the table length.",
        is_region: true,
        contexts: ContextSet::STENCILS,
        config: &[
            ConfigKey {
                name: "order",
                value: ConfigType::Word(&["moore_row_major", "moore_column_major", "von_neumann"]),
                default: Some(ConfigDefault::Word("moore_row_major")),
            },
            ConfigKey {
                name: "include_center",
                value: ConfigType::Bool,
                default: Some(ConfigDefault::Bool(false)),
            },
            BOUNDARY,
            config("acc", ConfigType::Accumulators),
        ],
        inputs: &[AT_CELL, WHEN],
        inner_outputs: &[
            pinned("k", U32_TYPE, Rate::Pair),
            pinned("offset", SocketType::Fixed(TypeClass::Vec2I), Rate::Pair),
            pinned("xy", SocketType::Fixed(TypeClass::Vec2I), Rate::Pair),
            pinned("cell", SocketType::Fixed(TypeClass::Cell), Rate::Pair),
            pinned("diagonal", BOOL_TYPE, Rate::Pair),
        ],
        config_ports: ConfigPorts::Accumulators,
        cost: NodeCost::Stencil,
        cases: &[
            pass_case(
                "row-major past a bounded edge",
                PassCase {
                    width: 3,
                    height: 3,
                    tables: second_position!("moore_row_major", "false", "bounded"),
                    after: &[
                        (
                            "second",
                            &[
                                U32(99),
                                U32(99),
                                U32(99),
                                U32(3),
                                U32(3),
                                U32(3),
                                U32(3),
                                U32(3),
                                U32(3),
                            ],
                        ),
                        (
                            "count",
                            &[U32(3), U32(5), U32(3), U32(5), U32(8), U32(5), U32(3), U32(5), U32(3)],
                        ),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "column-major past a bounded edge",
                PassCase {
                    width: 3,
                    height: 3,
                    tables: second_position!("moore_column_major", "false", "bounded"),
                    after: &[
                        (
                            "second",
                            &[
                                U32(99),
                                U32(1),
                                U32(1),
                                U32(99),
                                U32(1),
                                U32(1),
                                U32(99),
                                U32(1),
                                U32(1),
                            ],
                        ),
                        (
                            "count",
                            &[U32(3), U32(5), U32(3), U32(5), U32(8), U32(5), U32(3), U32(5), U32(3)],
                        ),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "the centre comes first",
                PassCase {
                    width: 3,
                    height: 3,
                    tables: second_position!("moore_row_major", "true", "torus"),
                    after: &[("second", &[U32(0); 9]), ("count", &[U32(9); 9])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "von neumann on the domain's torus",
                PassCase {
                    width: 3,
                    height: 3,
                    tables: second_position!("von_neumann", "true", "domain"),
                    after: &[("second", &[U32(3); 9]), ("count", &[U32(5); 9])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "agents fold over the field grid",
                PassCase {
                    width: 3,
                    height: 3,
                    agents: 3,
                    tables: FIELD_MAXIMUM,
                    before: &[
                        ("pos", &[Vec2F([0.5, 0.5]), Vec2F([1.5, 1.5]), Vec2F([2.9, 0.1])]),
                        (
                            "f.value",
                            &[
                                F32(0.0),
                                F32(1.0),
                                F32(2.0),
                                F32(3.0),
                                F32(4.0),
                                F32(5.0),
                                F32(6.0),
                                F32(7.0),
                                F32(8.0),
                            ],
                        ),
                    ],
                    after: &[("best", &[F32(4.0), F32(8.0), F32(5.0)])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "agent.radius_fold",
        group: "Agents",
        label: "Agents in radius",
        semantics: "Runs the body once per agent whose toroidal squared distance from `center` is at most `radius * radius`, the agent itself left out when `exclude_self` holds. The walk is `SpatialHash::for_each_within`'s over an index of cells about `cell_size` wide, built from the positions at the start of the tick: rows of cells, then columns, then ascending agent index within a cell on the host, and the order agents reached a cell on the GPU. Inner outputs: `k`, the number of agents visited before; `other`, the agent; `delta`, the toroidal difference from `center` to the agent; `dist_sq`, its squared length; and `prev.<acc>`. Accumulators are those of `region.repeat`. A visit order that differs between backends changes a float `sum`, any `carry`, any use of `k` and any read of a `prev.<acc>`. These then replay on the host alone. A draw in the body reads the other agent's index as its iteration word, so it does not depend on the order. `radius` and `cell_size` take constants or parameters, and every radius fold of a model reads `cell_size` from one source. An invocation whose walk meets more than 65536 candidates, inside the radius or not, faults, and so does one whose walk would cover more than 65536 cells of the index, before it starts. The fold reads the position lane at other agents. The read makes the lane two-sided in a tick pass that writes it.",
        is_region: true,
        contexts: ContextSet {
            tick: true,
            ..ContextSet::NONE
        },
        domain: Some(Domain::Agents),
        config: &[
            ConfigKey {
                name: "exclude_self",
                value: ConfigType::Bool,
                default: Some(ConfigDefault::Bool(true)),
            },
            config("acc", ConfigType::Accumulators),
        ],
        inputs: &[
            input("center", SocketType::Fixed(TypeClass::Vec2F)),
            input_up_to("radius", F32_TYPE, Rate::Parameter),
            input_up_to("cell_size", F32_TYPE, Rate::Parameter),
            WHEN,
        ],
        inner_outputs: &[
            pinned("k", U32_TYPE, Rate::Pair),
            pinned("other", SocketType::Fixed(TypeClass::Agent), Rate::Pair),
            pinned("delta", SocketType::Fixed(TypeClass::Vec2F), Rate::Pair),
            pinned("dist_sq", F32_TYPE, Rate::Pair),
        ],
        config_ports: ConfigPorts::Accumulators,
        effects: &[Effect::ReadsOtherPosition],
        parity: Parity::Approx,
        cost: NodeCost::RadiusFold,
        cases: &[
            pass_case(
                "others within the radius",
                PassCase {
                    width: 10,
                    height: 10,
                    agents: 4,
                    tables: neighbours!("true"),
                    before: &[("pos", FOUR_AGENTS), ("w", FOUR_WEIGHTS)],
                    after: &[
                        ("count", &[U32(2), U32(1), U32(1), U32(0)]),
                        ("sum", &[U32(110), U32(1), U32(1), U32(0)]),
                        (
                            "delta",
                            &[
                                Vec2F([-1.0, 0.0]),
                                Vec2F([-1.0, 0.0]),
                                Vec2F([2.0, 0.0]),
                                Vec2F([0.0, 0.0]),
                            ],
                        ),
                        ("dist", &[F32(5.0), F32(1.0), F32(4.0), F32(0.0)]),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "the agent itself when included",
                PassCase {
                    width: 10,
                    height: 10,
                    agents: 4,
                    tables: neighbours!("false"),
                    before: &[("pos", FOUR_AGENTS), ("w", FOUR_WEIGHTS)],
                    after: &[
                        ("count", &[U32(3), U32(2), U32(2), U32(1)]),
                        ("sum", &[U32(111), U32(11), U32(101), U32(1000)]),
                    ],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
];
