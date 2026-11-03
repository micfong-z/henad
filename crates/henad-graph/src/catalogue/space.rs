//! World, vector and space kinds.
//!
//! The *domain grid* is the grid of cells a pass over cells runs on: the grid of a grid topology, or the field grid
//! of an agents topology, which tiles the world at one cell per world unit, `max(width, 1)` by `max(height, 1)` cells
//! truncated toward zero. A *cell reference* holds the row-major index `y * width + x` of a cell of the domain grid,
//! and a reference to no cell holds `u32::MAX`, past every cell.

use super::cases::{case, host_case, pass_case};
use super::{
    BASE, BOOL_TYPE,
    CaseValue::{Bool, F32, F64, I32, NearVec2F, U32, Vec2D, Vec2F, Vec2I, Vec2U},
    ConfigDefault, ConfigKey, ConfigType, Domain, F32_TYPE, NodeCost, NodeDef, Parity, PassCase, SocketType, TypeClass,
    U32_TYPE, input, output, pinned,
};
use crate::types::Rate;

const VEC2F: SocketType = SocketType::Fixed(TypeClass::Vec2F);
const VEC2I: SocketType = SocketType::Fixed(TypeClass::Vec2I);
const VEC2U: SocketType = SocketType::Fixed(TypeClass::Vec2U);
const CELL: SocketType = SocketType::Fixed(TypeClass::Cell);

/// A lane `out` of `vec2f` over agents, a position lane, and the head of the pass `case` over agents.
macro_rules! agents_head {
    ($out:literal) => {
        concat!(
            r#"[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[lane]]
id = "out"
domain = "agents"
type = ""#,
            $out,
            r#""

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "agents"
"#
        )
    };
}

/// A lane `out` of cells of type `$out` and the head of the pass `case` over cells, on a grid.
macro_rules! cells_head {
    ($out:literal) => {
        concat!(
            r#"[[lane]]
id = "out"
domain = "cells"
type = ""#,
            $out,
            r#""

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"
"#
        )
    };
}

/// One node `n1` of kind `$op` with no input, written to the lane `out`.
macro_rules! write_one {
    ($op:literal) => {
        concat!(
            "\n[[pass.node]]\nid = \"n1\"\nop = \"",
            $op,
            "\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"lane.set\"\nconfig.lane = \"out\"\nin.value = \"n1\"\n"
        )
    };
}

const WORLD_OF_AGENTS: &str = concat!(agents_head!("vec2f"), write_one!("ctx.world"));
const GRID_OF_AGENTS: &str = concat!(agents_head!("vec2u"), write_one!("ctx.grid"));
const CELL_XY: &str = concat!(cells_head!("vec2u"), write_one!("ctx.cell_xy"));

/// Writes `space.cell_index` of each cell's `xy` lane as a number, and its validity.
const CELL_INDEX: &str = r#"[[lane]]
id = "xy"
domain = "cells"
type = "vec2i"

[[lane]]
id = "out"
domain = "cells"
type = "u32"

[[lane]]
id = "valid"
domain = "cells"
type = "bool"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "xy"

[[pass.node]]
id = "n2"
op = "space.cell_index"
in.xy = "n1"

[[pass.node]]
id = "n3"
op = "conv.to_u32"
in.a = "n2.cell"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "out"
in.value = "n3"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "valid"
in.value = "n2.valid"
"#;

/// Writes `space.cell_of` of each cell's `p` lane: the cell as a number, the column and row, and the validity.
const CELL_OF: &str = r#"[[lane]]
id = "p"
domain = "cells"
type = "vec2f"

[[lane]]
id = "out"
domain = "cells"
type = "u32"

[[lane]]
id = "xy"
domain = "cells"
type = "vec2i"

[[lane]]
id = "valid"
domain = "cells"
type = "bool"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "p"

[[pass.node]]
id = "n2"
op = "space.cell_of"
in.p = "n1"

[[pass.node]]
id = "n3"
op = "conv.to_u32"
in.a = "n2.cell"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "out"
in.value = "n3"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "xy"
in.value = "n2.xy"

[[pass.node]]
id = "n6"
op = "lane.set"
config.lane = "valid"
in.value = "n2.valid"
"#;

/// Writes the column and row of each cell's index, found through `space.cell_index` and `space.cell_xy`.
const CELL_XY_OF_A_CELL: &str = r#"[[lane]]
id = "out"
domain = "cells"
type = "vec2u"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "ctx.cell_xy"

[[pass.node]]
id = "n2"
op = "space.cell_index"
in.xy = "n1"

[[pass.node]]
id = "n3"
op = "space.cell_xy"
in.cell = "n2.cell"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "out"
in.value = "n3"
"#;

/// Writes `space.offset` of each cell by `(1, 1)` under the boundary `$boundary`: the cell as a number and the
/// validity.
macro_rules! offset_case {
    ($boundary:literal) => {
        concat!(
            r#"[[lane]]
id = "out"
domain = "cells"
type = "u32"

[[lane]]
id = "valid"
domain = "cells"
type = "bool"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "ctx.cell_xy"

[[pass.node]]
id = "n2"
op = "conv.to_vec2i"
in.a = "n1"

[[pass.node]]
id = "n3"
op = "space.offset"
config.boundary = ""#,
            $boundary,
            r#""
in.xy = "n2"
in.by = { vec2i = [1, 1] }

[[pass.node]]
id = "n4"
op = "conv.to_u32"
in.a = "n3.cell"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "out"
in.value = "n4"

[[pass.node]]
id = "n6"
op = "lane.set"
config.lane = "valid"
in.value = "n3.valid"
"#
        )
    };
}

/// Number a cell reference to no cell converts to.
const NONE_MISSING: u32 = u32::MAX;

/// Configuration of a kind that steps past the edge of the domain grid.
pub(super) const BOUNDARY: ConfigKey = ConfigKey {
    name: "boundary",
    value: ConfigType::Word(&["torus", "bounded", "domain"]),
    default: Some(ConfigDefault::Word("domain")),
};

const SCALARS: &[TypeClass] = &[TypeClass::U32, TypeClass::I32, TypeClass::F32, TypeClass::F64];
const VECTORS: &[TypeClass] = &[TypeClass::Vec2U, TypeClass::Vec2I, TypeClass::Vec2F, TypeClass::Vec2D];

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "ctx.world",
        group: "Context",
        label: "World size",
        semantics: "Size of the world: `world_width` and `world_height` in world units on the agents topology, and the grid's width and height in cells, as `f32`, on a grid. It is a parameter of the model, the same for every element.",
        outputs: &[pinned("size", VEC2F, Rate::Parameter)],
        cases: &[
            case("a grid of one cell", &[], &[("size", Vec2F([1.0, 1.0]))]),
            pass_case(
                "the world of agents",
                PassCase {
                    width: 300,
                    height: 200,
                    agents: 2,
                    tables: WORLD_OF_AGENTS,
                    after: &[("out", &[Vec2F([300.0, 200.0]), Vec2F([300.0, 200.0])])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "ctx.grid",
        group: "Context",
        label: "Grid size",
        semantics: "Size in cells of the domain grid: the grid of a grid topology, or the field grid of an agents topology, which tiles the world at one cell per world unit, `max(world, 1)` truncated toward zero on each axis.",
        outputs: &[pinned("size", VEC2U, Rate::Parameter)],
        cases: &[
            case("a grid of one cell", &[], &[("size", Vec2U([1, 1]))]),
            pass_case(
                "the field grid of agents",
                PassCase {
                    width: 300,
                    height: 200,
                    agents: 1,
                    tables: GRID_OF_AGENTS,
                    after: &[("out", &[Vec2U([300, 200])])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "ctx.cell_xy",
        group: "Context",
        label: "Cell position",
        semantics: "Column and row of the element in a pass over the cells of the domain grid.",
        domain: Some(Domain::Cells),
        outputs: &[pinned("xy", VEC2U, Rate::Element)],
        cases: &[pass_case(
            "row-major",
            PassCase {
                width: 3,
                height: 2,
                tables: CELL_XY,
                after: &[(
                    "out",
                    &[
                        Vec2U([0, 0]),
                        Vec2U([1, 0]),
                        Vec2U([2, 0]),
                        Vec2U([0, 1]),
                        Vec2U([1, 1]),
                        Vec2U([2, 1]),
                    ],
                )],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "vec.make",
        group: "Vectors",
        label: "Make vector",
        semantics: "The vector `(x, y)`: a `vec2u` of two `u32`, a `vec2i` of two `i32`, a `vec2f` of two `f32`, and a `vec2d` of two `f64` on the host.",
        type_var: SCALARS,
        inputs: &[input("x", SocketType::Var), input("y", SocketType::Var)],
        outputs: &[output("out", SocketType::Vector)],
        cases: &[
            case(
                "f32",
                &[("x", F32(1.5)), ("y", F32(-2.0))],
                &[("out", Vec2F([1.5, -2.0]))],
            ),
            case("u32", &[("x", U32(3)), ("y", U32(4))], &[("out", Vec2U([3, 4]))]),
            case("i32", &[("x", I32(-3)), ("y", I32(4))], &[("out", Vec2I([-3, 4]))]),
            host_case(
                "f64 on the host",
                &[("x", F64(0.5)), ("y", F64(2.0))],
                &[("out", Vec2D([0.5, 2.0]))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "vec.split",
        group: "Vectors",
        label: "Split vector",
        semantics: "The components `x` and `y` of a vector.",
        type_var: VECTORS,
        inputs: &[input("v", SocketType::Var)],
        outputs: &[output("x", SocketType::Scalar), output("y", SocketType::Scalar)],
        cases: &[
            case(
                "vec2f",
                &[("v", Vec2F([1.5, -2.0]))],
                &[("x", F32(1.5)), ("y", F32(-2.0))],
            ),
            case("vec2i", &[("v", Vec2I([-3, 4]))], &[("x", I32(-3)), ("y", I32(4))]),
            case("vec2u", &[("v", Vec2U([7, 8]))], &[("x", U32(7)), ("y", U32(8))]),
        ],
        ..BASE
    },
    NodeDef {
        id: "vec.scale",
        group: "Vectors",
        label: "Scale",
        semantics: "`(v.x * s, v.y * s)`.",
        inputs: &[input("v", VEC2F), input("s", F32_TYPE)],
        outputs: &[output("out", VEC2F)],
        cases: &[case(
            "both components",
            &[("v", Vec2F([1.0, -2.5])), ("s", F32(2.0))],
            &[("out", Vec2F([2.0, -5.0]))],
        )],
        ..BASE
    },
    NodeDef {
        id: "vec.dot",
        group: "Vectors",
        label: "Dot product",
        semantics: "`a.x * b.x + a.y * b.y`, the two products added in that order. A shader writes it out, not as `dot()`, and may still contract a product into the addition, so parity is approximate.",
        inputs: &[input("a", VEC2F), input("b", VEC2F)],
        outputs: &[output("out", F32_TYPE)],
        parity: Parity::Approx,
        cost: NodeCost::Units(2),
        cases: &[case(
            "whole numbers",
            &[("a", Vec2F([1.0, 2.0])), ("b", Vec2F([3.0, 4.0]))],
            &[("out", F32(11.0))],
        )],
        ..BASE
    },
    NodeDef {
        id: "vec.length",
        group: "Vectors",
        label: "Length",
        semantics: "`sqrt(v.x * v.x + v.y * v.y)`, written out. Parity is approximate, since a shader's square root is within 2.5 ULP.",
        inputs: &[input("v", VEC2F)],
        outputs: &[output("out", F32_TYPE)],
        parity: Parity::Approx,
        cost: NodeCost::Units(4),
        cases: &[
            case(
                "three four five",
                &[("v", Vec2F([3.0, 4.0]))],
                &[("out", super::CaseValue::Near(5.0, 1e-5))],
            ),
            case("zero", &[("v", Vec2F([0.0, 0.0]))], &[("out", F32(0.0))]),
        ],
        ..BASE
    },
    NodeDef {
        id: "vec.from_angle",
        group: "Vectors",
        label: "From angle",
        semantics: "`(cos(angle) * length, sin(angle) * length)`, the angle in radians. Parity is approximate, since sine and cosine differ in the last bits across backends.",
        inputs: &[input("angle", F32_TYPE), input("length", F32_TYPE)],
        outputs: &[output("out", VEC2F)],
        parity: Parity::Approx,
        cost: NodeCost::Units(8),
        cases: &[
            case(
                "east",
                &[("angle", F32(0.0)), ("length", F32(2.0))],
                &[("out", NearVec2F([2.0, 0.0], 1e-5))],
            ),
            case(
                "north",
                &[("angle", F32(core::f32::consts::FRAC_PI_2)), ("length", F32(3.0))],
                &[("out", NearVec2F([0.0, 3.0], 1e-5))],
            ),
            case(
                "south west",
                &[
                    ("angle", F32(-3.0 * core::f32::consts::FRAC_PI_4)),
                    ("length", F32(1.0)),
                ],
                &[(
                    "out",
                    NearVec2F(
                        [-core::f32::consts::FRAC_1_SQRT_2, -core::f32::consts::FRAC_1_SQRT_2],
                        1e-5,
                    ),
                )],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "space.wrap",
        group: "Space",
        label: "Wrap",
        semantics: "`v` wrapped into `[0, world)` per component, as `henad::space::wrap_coord`: the Euclidean remainder of `v` by `world`, and 0 where that rounds up to `world`.",
        type_var: &[TypeClass::F32, TypeClass::Vec2F],
        inputs: &[input("v", SocketType::Var), input("world", SocketType::Var)],
        outputs: &[output("out", SocketType::Var)],
        parity: Parity::Approx,
        cost: NodeCost::Units(3),
        cases: &[
            case(
                "past the end",
                &[("v", F32(5.5)), ("world", F32(5.0))],
                &[("out", F32(0.5))],
            ),
            case(
                "below zero",
                &[("v", F32(-0.5)), ("world", F32(5.0))],
                &[("out", F32(4.5))],
            ),
            case(
                "at the end",
                &[("v", F32(5.0)), ("world", F32(5.0))],
                &[("out", F32(0.0))],
            ),
            host_case(
                "a remainder that rounds up to the world",
                &[("v", F32(-1e-8)), ("world", F32(5.0))],
                &[("out", F32(0.0))],
            ),
            case(
                "vec2f per component",
                &[("v", Vec2F([-1.0, 11.0])), ("world", Vec2F([10.0, 10.0]))],
                &[("out", Vec2F([9.0, 1.0]))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "space.delta",
        group: "Space",
        label: "Toroidal difference",
        semantics: "`b - a` per component on a torus of size `world`, as `henad::space::axis_delta`: the difference brought into `[-world / 2, world / 2)`. It is the `delta` a radius fold gives its body.",
        inputs: &[input("a", VEC2F), input("b", VEC2F), input("world", VEC2F)],
        outputs: &[output("out", VEC2F)],
        parity: Parity::Approx,
        cost: NodeCost::Units(4),
        cases: &[
            case(
                "across the seam",
                &[
                    ("a", Vec2F([1.0, 1.0])),
                    ("b", Vec2F([9.0, 2.0])),
                    ("world", Vec2F([10.0, 10.0])),
                ],
                &[("out", Vec2F([-2.0, 1.0]))],
            ),
            case(
                "half the world goes back",
                &[
                    ("a", Vec2F([0.0, 0.0])),
                    ("b", Vec2F([5.0, 0.0])),
                    ("world", Vec2F([10.0, 10.0])),
                ],
                &[("out", Vec2F([-5.0, 0.0]))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "space.dist_sq",
        group: "Space",
        label: "Toroidal distance squared",
        semantics: "`d.x * d.x + d.y * d.y` of `d = space.delta(a, b, world)`, the squared distance on a torus. It is the `dist_sq` a radius fold gives its body.",
        inputs: &[input("a", VEC2F), input("b", VEC2F), input("world", VEC2F)],
        outputs: &[output("out", F32_TYPE)],
        parity: Parity::Approx,
        cost: NodeCost::Units(6),
        cases: &[case(
            "across the seam",
            &[
                ("a", Vec2F([1.0, 1.0])),
                ("b", Vec2F([9.0, 2.0])),
                ("world", Vec2F([10.0, 10.0])),
            ],
            &[("out", F32(5.0))],
        )],
        ..BASE
    },
    NodeDef {
        id: "space.cell_index",
        group: "Space",
        label: "Cell at",
        semantics: "The cell at column and row `xy` of the domain grid, and whether it lies on the grid. A position off the grid gives the reference to no cell.",
        type_var: &[TypeClass::Vec2U, TypeClass::Vec2I],
        inputs: &[input("xy", SocketType::Var)],
        outputs: &[output("cell", CELL), output("valid", BOOL_TYPE)],
        cases: &[pass_case(
            "on and off the grid",
            PassCase {
                width: 3,
                height: 2,
                tables: CELL_INDEX,
                before: &[(
                    "xy",
                    &[
                        Vec2I([2, 1]),
                        Vec2I([0, 0]),
                        Vec2I([3, 0]),
                        Vec2I([-1, 0]),
                        Vec2I([0, 2]),
                        Vec2I([1, 1]),
                    ],
                )],
                after: &[
                    (
                        "out",
                        &[
                            U32(5),
                            U32(0),
                            U32(NONE_MISSING),
                            U32(NONE_MISSING),
                            U32(NONE_MISSING),
                            U32(4),
                        ],
                    ),
                    (
                        "valid",
                        &[
                            Bool(true),
                            Bool(true),
                            Bool(false),
                            Bool(false),
                            Bool(false),
                            Bool(true),
                        ],
                    ),
                ],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "space.cell_of",
        group: "Space",
        label: "Cell under",
        semantics: "The cell of the domain grid under the position `p`: each component truncated toward zero to an `i32`, then `space.cell_index`. A component past the `i32` range clamps, as WGSL converts. `xy` is the truncated position, on the grid or not.",
        inputs: &[input("p", VEC2F)],
        outputs: &[output("cell", CELL), output("xy", VEC2I), output("valid", BOOL_TYPE)],
        cost: NodeCost::Units(3),
        cases: &[pass_case(
            "truncates toward zero",
            PassCase {
                width: 3,
                height: 2,
                tables: CELL_OF,
                before: &[(
                    "p",
                    &[
                        Vec2F([2.7, 1.2]),
                        Vec2F([-0.5, 0.0]),
                        Vec2F([3.0, 0.0]),
                        Vec2F([-1.5, 1.0]),
                        Vec2F([0.0, 0.0]),
                        Vec2F([1.9, 0.9]),
                    ],
                )],
                after: &[
                    (
                        "out",
                        &[U32(5), U32(0), U32(NONE_MISSING), U32(NONE_MISSING), U32(0), U32(1)],
                    ),
                    (
                        "xy",
                        &[
                            Vec2I([2, 1]),
                            Vec2I([0, 0]),
                            Vec2I([3, 0]),
                            Vec2I([-1, 1]),
                            Vec2I([0, 0]),
                            Vec2I([1, 0]),
                        ],
                    ),
                    (
                        "valid",
                        &[Bool(true), Bool(true), Bool(false), Bool(false), Bool(true), Bool(true)],
                    ),
                ],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "space.cell_xy",
        group: "Space",
        label: "Cell position of",
        semantics: "Column and row of the cell `cell` of the domain grid: `(cell % width, cell / width)`. The reference to no cell gives the same formula over `u32::MAX`.",
        inputs: &[input("cell", CELL)],
        outputs: &[output("xy", VEC2U)],
        cost: NodeCost::Units(2),
        cases: &[pass_case(
            "round trip",
            PassCase {
                width: 3,
                height: 2,
                tables: CELL_XY_OF_A_CELL,
                after: &[(
                    "out",
                    &[
                        Vec2U([0, 0]),
                        Vec2U([1, 0]),
                        Vec2U([2, 0]),
                        Vec2U([0, 1]),
                        Vec2U([1, 1]),
                        Vec2U([2, 1]),
                    ],
                )],
                ..PassCase::EMPTY
            },
        )],
        ..BASE
    },
    NodeDef {
        id: "space.offset",
        group: "Space",
        label: "Offset cell",
        semantics: "The column and row `xy + by` of the domain grid, its cell and whether it lies on the grid. Under `boundary = \"torus\"` the position wraps on both axes and is always on the grid. Under `\"bounded\"` it does not wrap, and a position off the grid gives the reference to no cell. `\"domain\"` takes the grid's `boundary` on a grid topology, and the torus on an agents topology. It is the step a `grid.fold` takes to each stencil position.",
        config: &[BOUNDARY],
        inputs: &[input("xy", VEC2I), input("by", VEC2I)],
        outputs: &[output("xy", VEC2I), output("cell", CELL), output("valid", BOOL_TYPE)],
        cost: NodeCost::Units(3),
        cases: &[
            pass_case(
                "a torus wraps",
                PassCase {
                    width: 3,
                    height: 2,
                    tables: offset_case!("torus"),
                    after: &[
                        ("out", &[U32(4), U32(5), U32(3), U32(1), U32(2), U32(0)]),
                        ("valid", &[Bool(true); 6]),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a bounded grid has an edge",
                PassCase {
                    width: 3,
                    height: 2,
                    tables: offset_case!("bounded"),
                    after: &[
                        (
                            "out",
                            &[
                                U32(4),
                                U32(5),
                                U32(NONE_MISSING),
                                U32(NONE_MISSING),
                                U32(NONE_MISSING),
                                U32(NONE_MISSING),
                            ],
                        ),
                        (
                            "valid",
                            &[
                                Bool(true),
                                Bool(true),
                                Bool(false),
                                Bool(false),
                                Bool(false),
                                Bool(false),
                            ],
                        ),
                    ],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "the domain's boundary",
                PassCase {
                    width: 3,
                    height: 2,
                    bounded: true,
                    tables: offset_case!("domain"),
                    after: &[(
                        "valid",
                        &[
                            Bool(true),
                            Bool(true),
                            Bool(false),
                            Bool(false),
                            Bool(false),
                            Bool(false),
                        ],
                    )],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "space.heading_octant",
        group: "Space",
        label: "Heading octant",
        semantics: "Octant 0 to 7 of the direction `v`, as `henad::space::heading_octant`: east and south are a component of at least 0, and steep is `|v.y| > |v.x|`. Octant 0 is east to south-east, and the octants run clockwise on the screen, whose y axis points down.",
        inputs: &[input("v", VEC2F)],
        outputs: &[output("out", U32_TYPE)],
        cost: NodeCost::Units(3),
        cases: &[
            case("east", &[("v", Vec2F([1.0, 0.5]))], &[("out", U32(0))]),
            case("south", &[("v", Vec2F([0.5, 1.0]))], &[("out", U32(1))]),
            case("south-west", &[("v", Vec2F([-0.5, 1.0]))], &[("out", U32(2))]),
            case("west", &[("v", Vec2F([-1.0, -0.5]))], &[("out", U32(4))]),
            case("north-east", &[("v", Vec2F([1.0, -0.5]))], &[("out", U32(7))]),
            case("still", &[("v", Vec2F([0.0, 0.0]))], &[("out", U32(0))]),
        ],
        ..BASE
    },
    NodeDef {
        id: "space.step_code",
        group: "Space",
        label: "Step code",
        semantics: "`(d.x + 1) * 3 + (d.y + 1)` as a `u32`, wrapping, which numbers the nine steps of a Moore neighbourhood 0 to 8.",
        inputs: &[input("d", VEC2I)],
        outputs: &[output("out", U32_TYPE)],
        cost: NodeCost::Units(2),
        cases: &[
            case("up-left", &[("d", Vec2I([-1, -1]))], &[("out", U32(0))]),
            case("still", &[("d", Vec2I([0, 0]))], &[("out", U32(4))]),
            case("down-right", &[("d", Vec2I([1, 1]))], &[("out", U32(8))]),
            case("right", &[("d", Vec2I([1, 0]))], &[("out", U32(7))]),
        ],
        ..BASE
    },
    NodeDef {
        id: "space.step_offset",
        group: "Space",
        label: "Step offset",
        semantics: "`(code / 3 - 1, code % 3 - 1)` as an `i32` vector, the inverse of `space.step_code` on 0 to 8. A code past 8 follows the same formula, the `u32` quotient reinterpreted as an `i32`.",
        inputs: &[input("code", U32_TYPE)],
        outputs: &[output("out", VEC2I)],
        cost: NodeCost::Units(2),
        cases: &[
            case("up-left", &[("code", U32(0))], &[("out", Vec2I([-1, -1]))]),
            case("still", &[("code", U32(4))], &[("out", Vec2I([0, 0]))]),
            case("right", &[("code", U32(7))], &[("out", Vec2I([1, 0]))]),
            case("no step", &[("code", U32(255))], &[("out", Vec2I([84, -1]))]),
        ],
        ..BASE
    },
];
