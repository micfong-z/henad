//! Arithmetic kinds.
//!
//! Integer operations follow WGSL's total definitions on both backends. A shader may assume no NaN and no infinity, so
//! a case with one of those on either side of a float kind holds on the host alone.

use super::cases::{case, config_case, host_case, pass_case};
use super::{
    BASE,
    CaseValue::{Bool, F32, I32, Nan, Near, U32, U64, Vec2F, Vec2I, Vec2U},
    ConfigPorts, ConfigType, F32_TYPE, NodeCost, NodeDef, Parity, PassCase, SocketType, TypeClass, U32_TYPE, config,
    input, input_up_to, output,
};
use crate::limits::MAX_POWI_EXPONENT;
use crate::types::Rate;

/// Classes of the numbers and vectors.
pub(crate) const NUMERIC: &[TypeClass] = &[
    TypeClass::U32,
    TypeClass::I32,
    TypeClass::F32,
    TypeClass::Vec2U,
    TypeClass::Vec2I,
    TypeClass::Vec2F,
    TypeClass::U64,
    TypeClass::F64,
];
const REMAINDER: &[TypeClass] = &[
    TypeClass::U32,
    TypeClass::I32,
    TypeClass::U64,
    TypeClass::F32,
    TypeClass::F64,
];
const SIGNED: &[TypeClass] = &[
    TypeClass::I32,
    TypeClass::F32,
    TypeClass::Vec2I,
    TypeClass::Vec2F,
    TypeClass::F64,
];
const FLOATS: &[TypeClass] = &[TypeClass::F32, TypeClass::Vec2F, TypeClass::F64, TypeClass::Vec2D];

/// Numeric kinds with the inputs `a` and `b` and the output `out`, all one type.
const BINARY: NodeDef = NodeDef {
    group: "Arithmetic",
    type_var: NUMERIC,
    inputs: &[input("a", SocketType::Var), input("b", SocketType::Var)],
    outputs: &[output("out", SocketType::Var)],
    ..BASE
};

/// Kinds with the input `a` and the output `out`, both one type.
const UNARY: NodeDef = NodeDef {
    group: "Arithmetic",
    inputs: &[input("a", SocketType::Var)],
    outputs: &[output("out", SocketType::Var)],
    ..BASE
};

/// `f32` kinds with the input `a` and the output `out`. Their floats differ across backends.
const FLOAT_FUNCTION: NodeDef = NodeDef {
    group: "Arithmetic",
    inputs: &[input("a", F32_TYPE)],
    outputs: &[output("out", F32_TYPE)],
    parity: Parity::Approx,
    cost: NodeCost::Units(4),
    ..BASE
};

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "math.add",
        label: "Add",
        semantics: "Sum of `a` and `b`. Integers wrap and vectors add per component. A float NaN input returns NaN on the host.",
        cases: &[
            case("u32 wraps", &[("a", U32(u32::MAX)), ("b", U32(2))], &[("out", U32(1))]),
            case(
                "i32 wraps",
                &[("a", I32(i32::MAX)), ("b", I32(1))],
                &[("out", I32(i32::MIN))],
            ),
            case("f32", &[("a", F32(1.5)), ("b", F32(0.25))], &[("out", F32(1.75))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([1.0, 2.0])), ("b", Vec2F([0.5, -2.0]))],
                &[("out", Vec2F([1.5, 0.0]))],
            ),
            host_case(
                "u64 on the host",
                &[("a", U64(u64::MAX)), ("b", U64(1))],
                &[("out", U64(0))],
            ),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.sub",
        label: "Subtract",
        semantics: "Difference `a - b`. Integers wrap and vectors subtract per component. A float NaN input returns NaN on the host.",
        cases: &[
            case("u32 wraps", &[("a", U32(0)), ("b", U32(1))], &[("out", U32(u32::MAX))]),
            case("i32", &[("a", I32(-3)), ("b", I32(4))], &[("out", I32(-7))]),
            case("f32", &[("a", F32(1.0)), ("b", F32(0.3))], &[("out", F32(0.7))]),
            case(
                "vec2i per component",
                &[("a", Vec2I([1, 2])), ("b", Vec2I([3, 1]))],
                &[("out", Vec2I([-2, 1]))],
            ),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.mul",
        label: "Multiply",
        semantics: "Product of `a` and `b`. Integers wrap and vectors multiply per component. A float NaN input returns NaN on the host.",
        cases: &[
            case(
                "u32 wraps",
                &[("a", U32(0x8000_0000)), ("b", U32(3))],
                &[("out", U32(0x8000_0000))],
            ),
            case("i32", &[("a", I32(-3)), ("b", I32(4))], &[("out", I32(-12))]),
            case("f32", &[("a", F32(1.5)), ("b", F32(0.5))], &[("out", F32(0.75))]),
            case(
                "vec2u per component",
                &[("a", Vec2U([2, 3])), ("b", Vec2U([4, 5]))],
                &[("out", Vec2U([8, 15]))],
            ),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.div",
        label: "Divide",
        semantics: "Quotient `a / b`, per component on vectors. Integers truncate toward zero, `x / 0` is `x`, and `i32::MIN / -1` is `i32::MIN`. Floats follow IEEE 754 on the host, where `1 / 0` is infinite and `0 / 0` is NaN, and a shader leaves those two indeterminate. Parity is approximate, since WGSL float division is within 2.5 ULP.",
        parity: Parity::Approx,
        cases: &[
            case("u32 truncates", &[("a", U32(7)), ("b", U32(2))], &[("out", U32(3))]),
            case(
                "i32 truncates toward zero",
                &[("a", I32(-7)), ("b", I32(2))],
                &[("out", I32(-3))],
            ),
            case("u32 by zero", &[("a", U32(7)), ("b", U32(0))], &[("out", U32(7))]),
            case("i32 by zero", &[("a", I32(-7)), ("b", I32(0))], &[("out", I32(-7))]),
            case(
                "i32 overflow",
                &[("a", I32(i32::MIN)), ("b", I32(-1))],
                &[("out", I32(i32::MIN))],
            ),
            case("f32", &[("a", F32(1.0)), ("b", F32(4.0))], &[("out", Near(0.25, 1e-6))]),
            host_case(
                "f32 by zero",
                &[("a", F32(1.0)), ("b", F32(0.0))],
                &[("out", F32(f32::INFINITY))],
            ),
            host_case("f32 zero by zero", &[("a", F32(0.0)), ("b", F32(0.0))], &[("out", Nan)]),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.rem",
        label: "Remainder",
        semantics: "Truncated remainder of `a / b`, which takes the sign of `a`. Integer `x % 0` is 0 and `i32::MIN % -1` is 0. On floats the host computes `fmod`, a shader computes `a - b * trunc(a / b)`, and `x % 0` is NaN on the host and indeterminate in a shader. Parity is approximate.",
        type_var: REMAINDER,
        parity: Parity::Approx,
        cases: &[
            case("u32", &[("a", U32(7)), ("b", U32(3))], &[("out", U32(1))]),
            case(
                "i32 takes the sign of a",
                &[("a", I32(-7)), ("b", I32(3))],
                &[("out", I32(-1))],
            ),
            case("u32 by zero", &[("a", U32(7)), ("b", U32(0))], &[("out", U32(0))]),
            case("i32 by zero", &[("a", I32(-7)), ("b", I32(0))], &[("out", I32(0))]),
            case(
                "i32 overflow",
                &[("a", I32(i32::MIN)), ("b", I32(-1))],
                &[("out", I32(0))],
            ),
            case("f32", &[("a", F32(5.5)), ("b", F32(2.0))], &[("out", Near(1.5, 1e-5))]),
            case(
                "f32 takes the sign of a",
                &[("a", F32(-5.5)), ("b", F32(2.0))],
                &[("out", Near(-1.5, 1e-5))],
            ),
            host_case("f32 by zero", &[("a", F32(1.0)), ("b", F32(0.0))], &[("out", Nan)]),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.min",
        label: "Minimum",
        semantics: "Smaller of `a` and `b`, per component on vectors. On the host a NaN input returns the other operand, and a shader leaves it indeterminate.",
        cases: &[
            case("u32", &[("a", U32(3)), ("b", U32(5))], &[("out", U32(3))]),
            case("i32", &[("a", I32(-3)), ("b", I32(5))], &[("out", I32(-3))]),
            case("f32", &[("a", F32(0.5)), ("b", F32(0.25))], &[("out", F32(0.25))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([1.0, 4.0])), ("b", Vec2F([2.0, 3.0]))],
                &[("out", Vec2F([1.0, 3.0]))],
            ),
            host_case(
                "NaN returns the other",
                &[("a", F32(f32::NAN)), ("b", F32(2.0))],
                &[("out", F32(2.0))],
            ),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.max",
        label: "Maximum",
        semantics: "Larger of `a` and `b`, per component on vectors. On the host a NaN input returns the other operand, and a shader leaves it indeterminate.",
        cases: &[
            case("u32", &[("a", U32(3)), ("b", U32(5))], &[("out", U32(5))]),
            case("i32", &[("a", I32(-3)), ("b", I32(5))], &[("out", I32(5))]),
            case("f32", &[("a", F32(0.5)), ("b", F32(0.25))], &[("out", F32(0.5))]),
            case(
                "vec2i per component",
                &[("a", Vec2I([1, 4])), ("b", Vec2I([2, 3]))],
                &[("out", Vec2I([2, 4]))],
            ),
            host_case(
                "NaN returns the other",
                &[("a", F32(2.0)), ("b", F32(f32::NAN))],
                &[("out", F32(2.0))],
            ),
        ],
        ..BINARY
    },
    NodeDef {
        id: "math.abs",
        label: "Absolute value",
        semantics: "Absolute value of `a`, per component on vectors. Integers wrap, so `abs(i32::MIN)` is `i32::MIN`. On floats it clears the sign. A NaN input stays NaN on the host and is indeterminate in a shader.",
        type_var: SIGNED,
        cases: &[
            case("i32", &[("a", I32(-3))], &[("out", I32(3))]),
            case("i32 wraps", &[("a", I32(i32::MIN))], &[("out", I32(i32::MIN))]),
            case("f32", &[("a", F32(-1.5))], &[("out", F32(1.5))]),
            case("negative zero", &[("a", F32(-0.0))], &[("out", F32(0.0))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([-1.0, 2.0]))],
                &[("out", Vec2F([1.0, 2.0]))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.ceil",
        label: "Ceiling",
        semantics: "Rounds `a` toward plus infinity, per component on vectors. A NaN input stays NaN on the host and is indeterminate in a shader.",
        type_var: FLOATS,
        cases: &[
            case("up", &[("a", F32(1.25))], &[("out", F32(2.0))]),
            case("negative", &[("a", F32(-1.25))], &[("out", F32(-1.0))]),
            case("whole", &[("a", F32(3.0))], &[("out", F32(3.0))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([0.5, -0.5]))],
                &[("out", Vec2F([1.0, -0.0]))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.trunc",
        label: "Truncate",
        semantics: "Rounds `a` toward zero, per component on vectors. A NaN input stays NaN on the host and is indeterminate in a shader.",
        type_var: FLOATS,
        cases: &[
            case("positive", &[("a", F32(1.75))], &[("out", F32(1.0))]),
            case("negative", &[("a", F32(-1.75))], &[("out", F32(-1.0))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([0.5, 2.5]))],
                &[("out", Vec2F([0.0, 2.0]))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.round",
        label: "Round",
        semantics: "Rounds `a` to the nearest integer, per component on vectors. A tie goes to the even neighbour, as WGSL's `round` does, where Rust's `f32::round` goes away from zero. A NaN input stays NaN on the host and is indeterminate in a shader.",
        type_var: FLOATS,
        cases: &[
            case("nearest", &[("a", F32(1.25))], &[("out", F32(1.0))]),
            case("tie up to even", &[("a", F32(1.5))], &[("out", F32(2.0))]),
            case("tie down to even", &[("a", F32(2.5))], &[("out", F32(2.0))]),
            case("negative tie", &[("a", F32(-0.5))], &[("out", F32(-0.0))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([0.75, 3.5]))],
                &[("out", Vec2F([1.0, 4.0]))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.exp",
        label: "Exponential",
        semantics: "`e` to the power `a`. An overflow is infinite on the host and indeterminate in a shader, and a NaN input stays NaN on the host and is indeterminate in a shader.",
        cases: &[
            case("zero", &[("a", F32(0.0))], &[("out", Near(1.0, 1e-5))]),
            case("one", &[("a", F32(1.0))], &[("out", Near(core::f32::consts::E, 1e-5))]),
            host_case("overflow", &[("a", F32(1000.0))], &[("out", F32(f32::INFINITY))]),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..FLOAT_FUNCTION
    },
    NodeDef {
        id: "math.log",
        label: "Natural log",
        semantics: "Natural logarithm of `a`. On the host `log(0)` is minus infinity and a negative or NaN input is NaN, and a shader leaves all three indeterminate. A model that can meet them clamps `a` first.",
        cases: &[
            case("one", &[("a", F32(1.0))], &[("out", Near(0.0, 1e-5))]),
            case("e", &[("a", F32(core::f32::consts::E))], &[("out", Near(1.0, 1e-5))]),
            host_case("zero", &[("a", F32(0.0))], &[("out", F32(f32::NEG_INFINITY))]),
            host_case("negative", &[("a", F32(-1.0))], &[("out", Nan)]),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..FLOAT_FUNCTION
    },
    NodeDef {
        id: "math.sin",
        label: "Sine",
        semantics: "Sine of `a` in radians. An infinite or NaN input is NaN on the host and indeterminate in a shader.",
        cases: &[
            case("zero", &[("a", F32(0.0))], &[("out", Near(0.0, 1e-5))]),
            case(
                "quarter turn",
                &[("a", F32(core::f32::consts::FRAC_PI_2))],
                &[("out", Near(1.0, 1e-5))],
            ),
            host_case("infinite", &[("a", F32(f32::INFINITY))], &[("out", Nan)]),
        ],
        ..FLOAT_FUNCTION
    },
    NodeDef {
        id: "math.cos",
        label: "Cosine",
        semantics: "Cosine of `a` in radians. An infinite or NaN input is NaN on the host and indeterminate in a shader.",
        cases: &[
            case("zero", &[("a", F32(0.0))], &[("out", Near(1.0, 1e-5))]),
            case(
                "half turn",
                &[("a", F32(core::f32::consts::PI))],
                &[("out", Near(-1.0, 1e-5))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..FLOAT_FUNCTION
    },
    NodeDef {
        id: "math.powi",
        label: "Integer power",
        semantics: "`base` to the integer power `min(exponent, max_exponent)`, as Rust's `f32::powi`, with `x^0 = 1`. The host computes a table of `max_exponent + 1` entries when the model is built, and the element reads the table on both backends, so parity is exact.",
        config: &[config(
            "max_exponent",
            ConfigType::Integer {
                min: 1,
                max: MAX_POWI_EXPONENT as i64,
            },
        )],
        inputs: &[
            input_up_to("base", F32_TYPE, Rate::Parameter),
            input("exponent", U32_TYPE),
        ],
        outputs: &[output("out", F32_TYPE)],
        group: "Arithmetic",
        cost: NodeCost::Units(1),
        cases: &[
            config_case(
                "cube",
                "config.max_exponent = 8\n",
                &[("base", F32(0.5)), ("exponent", U32(3))],
                &[("out", F32(0.125))],
            ),
            config_case(
                "exponent clamps",
                "config.max_exponent = 2\n",
                &[("base", F32(0.5)), ("exponent", U32(5))],
                &[("out", F32(0.25))],
            ),
            config_case(
                "zero exponent",
                "config.max_exponent = 8\n",
                &[("base", F32(0.0)), ("exponent", U32(0))],
                &[("out", F32(1.0))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "math.pow",
        label: "Power",
        semantics: "`base` to the power `exponent`. With both inputs uniform the host computes it when the model is built. On the host a negative base with a fractional exponent is NaN and `0^0` is 1, and a shader leaves both indeterminate, as it does a NaN input.",
        inputs: &[input("base", F32_TYPE), input("exponent", F32_TYPE)],
        cases: &[
            case(
                "whole",
                &[("base", F32(2.0)), ("exponent", F32(10.0))],
                &[("out", Near(1024.0, 1e-2))],
            ),
            case(
                "root",
                &[("base", F32(4.0)), ("exponent", F32(0.5))],
                &[("out", Near(2.0, 1e-4))],
            ),
            host_case(
                "zero to zero",
                &[("base", F32(0.0)), ("exponent", F32(0.0))],
                &[("out", F32(1.0))],
            ),
            host_case(
                "negative base",
                &[("base", F32(-1.0)), ("exponent", F32(0.5))],
                &[("out", Nan)],
            ),
        ],
        ..FLOAT_FUNCTION
    },
];

/// A lane `out` of `$type`, lanes `a` and `b` of `f32`, and a pass writing the expression `$text` of `a` and `b` to
/// `out`.
macro_rules! expression_case {
    ($type:literal, $text:literal) => {
        concat!(
            r#"[[lane]]
id = "a"
domain = "cells"
type = "f32"

[[lane]]
id = "b"
domain = "cells"
type = "f32"

[[lane]]
id = "out"
domain = "cells"
type = ""#,
            $type,
            r#""

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "a"

[[pass.node]]
id = "n2"
op = "lane.get"
config.lane = "b"

[[pass.node]]
id = "n3"
op = "math.expr"
config.text = '"#,
            $text,
            r#"'
config.inputs = [{ name = "a", type = "f32" }, { name = "b", type = "f32" }]
config.outputs = ["out"]
in.a = "n1"
in.b = "n2"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "out"
in.value = "n3.out"
"#
        )
    };
}

/// Two outputs of one expression, one reading the other.
const TWO_OUTPUTS: &str = r#"[[lane]]
id = "a"
domain = "cells"
type = "vec2f"

[[lane]]
id = "x"
domain = "cells"
type = "f32"

[[lane]]
id = "v"
domain = "cells"
type = "vec2f"

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "a"

[[pass.node]]
id = "n2"
op = "math.expr"
config.text = "x = a.x * 2.0 + 1; v = vec(x, a.y) * 0.5"
config.inputs = [{ name = "a", type = "vec2f" }]
config.outputs = ["x", "v"]
in.a = "n1"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n2.x"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "v"
in.value = "n2.v"
"#;

/// Kinds of the group that the palette lists after [`KINDS`].
pub(super) static MORE_KINDS: &[NodeDef] = &[
    NodeDef {
        id: "math.neg",
        label: "Negate",
        semantics: "`-a`, per component on vectors. Integers wrap, so `-i32::MIN` is `i32::MIN`. On floats it flips the sign, so `-0.0` is `0.0`.",
        type_var: SIGNED,
        cases: &[
            case("i32", &[("a", I32(3))], &[("out", I32(-3))]),
            case("i32 wraps", &[("a", I32(i32::MIN))], &[("out", I32(i32::MIN))]),
            case("f32", &[("a", F32(1.5))], &[("out", F32(-1.5))]),
            case("zero flips its sign", &[("a", F32(0.0))], &[("out", F32(-0.0))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([1.0, -2.0]))],
                &[("out", Vec2F([-1.0, 2.0]))],
            ),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.clamp",
        label: "Clamp",
        semantics: "`min(max(x, lo), hi)` per component, composed of `math.max` and `math.min`, so `hi` wins when `lo > hi`. On the host a NaN `x` returns `lo`, where `f32::clamp` returns NaN, and a shader leaves it indeterminate.",
        group: "Arithmetic",
        type_var: NUMERIC,
        inputs: &[
            input("x", SocketType::Var),
            input("lo", SocketType::Var),
            input("hi", SocketType::Var),
        ],
        outputs: &[output("out", SocketType::Var)],
        cost: NodeCost::Units(2),
        cases: &[
            case(
                "inside",
                &[("x", U32(4)), ("lo", U32(1)), ("hi", U32(6))],
                &[("out", U32(4))],
            ),
            case(
                "below",
                &[("x", F32(-0.5)), ("lo", F32(0.0)), ("hi", F32(1.0))],
                &[("out", F32(0.0))],
            ),
            case(
                "above",
                &[("x", F32(1.5)), ("lo", F32(0.0)), ("hi", F32(1.0))],
                &[("out", F32(1.0))],
            ),
            case(
                "lo past hi",
                &[("x", I32(5)), ("lo", I32(3)), ("hi", I32(1))],
                &[("out", I32(1))],
            ),
            case(
                "vec2i per component",
                &[("x", Vec2I([-5, 5])), ("lo", Vec2I([0, 0])), ("hi", Vec2I([3, 3]))],
                &[("out", Vec2I([0, 3]))],
            ),
            host_case(
                "NaN gives lo",
                &[("x", F32(f32::NAN)), ("lo", F32(0.0)), ("hi", F32(1.0))],
                &[("out", F32(0.0))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "math.floor",
        label: "Floor",
        semantics: "Rounds `a` toward minus infinity, per component on vectors. A NaN input stays NaN on the host and is indeterminate in a shader.",
        type_var: FLOATS,
        cases: &[
            case("down", &[("a", F32(1.75))], &[("out", F32(1.0))]),
            case("negative", &[("a", F32(-1.25))], &[("out", F32(-2.0))]),
            case(
                "vec2f per component",
                &[("a", Vec2F([0.5, -0.5]))],
                &[("out", Vec2F([0.0, -1.0]))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", Nan)]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.sqrt",
        label: "Square root",
        semantics: "Square root of `a`, per component on vectors. The host rounds correctly, and a shader is within 2.5 ULP, so parity is approximate. A negative input is NaN on the host and indeterminate in a shader.",
        type_var: FLOATS,
        parity: Parity::Approx,
        cost: NodeCost::Units(4),
        cases: &[
            case("a square", &[("a", F32(4.0))], &[("out", Near(2.0, 1e-6))]),
            case(
                "two",
                &[("a", F32(2.0))],
                &[("out", Near(core::f32::consts::SQRT_2, 1e-6))],
            ),
            host_case(
                "exact on the host",
                &[("a", F32(2.0))],
                &[("out", F32(core::f32::consts::SQRT_2))],
            ),
            host_case("negative", &[("a", F32(-1.0))], &[("out", Nan)]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.recip",
        label: "Reciprocal",
        semantics: "`1 / a`, per component on vectors. Parity is approximate, since WGSL float division is within 2.5 ULP. On the host `1 / 0` is infinite, and a shader leaves it indeterminate.",
        type_var: FLOATS,
        parity: Parity::Approx,
        cost: NodeCost::Units(2),
        cases: &[
            case("a quarter", &[("a", F32(4.0))], &[("out", Near(0.25, 1e-7))]),
            host_case("exact on the host", &[("a", F32(3.0))], &[("out", F32(1.0 / 3.0))]),
            host_case("zero", &[("a", F32(0.0))], &[("out", F32(f32::INFINITY))]),
        ],
        ..UNARY
    },
    NodeDef {
        id: "math.log2",
        label: "Log base 2",
        semantics: "Logarithm of `a` to base 2. On the host `log2(0)` is minus infinity and a negative or NaN input is NaN, and a shader leaves all three indeterminate.",
        cases: &[
            case("eight", &[("a", F32(8.0))], &[("out", Near(3.0, 1e-5))]),
            case("a half", &[("a", F32(0.5))], &[("out", Near(-1.0, 1e-5))]),
            host_case("zero", &[("a", F32(0.0))], &[("out", F32(f32::NEG_INFINITY))]),
            host_case("negative", &[("a", F32(-1.0))], &[("out", Nan)]),
        ],
        ..FLOAT_FUNCTION
    },
    NodeDef {
        id: "math.log10",
        label: "Log base 10",
        semantics: "Logarithm of `a` to base 10. The host computes `f32::log10`, and a shader `log2(a) * 0.30102999566398120`, so parity is approximate. On the host `log10(0)` is minus infinity and a negative or NaN input is NaN, and a shader leaves all three indeterminate.",
        cases: &[
            case("a thousand", &[("a", F32(1000.0))], &[("out", Near(3.0, 1e-5))]),
            case("a tenth", &[("a", F32(0.1))], &[("out", Near(-1.0, 1e-5))]),
            host_case("zero", &[("a", F32(0.0))], &[("out", F32(f32::NEG_INFINITY))]),
            host_case("negative", &[("a", F32(-1.0))], &[("out", Nan)]),
        ],
        ..FLOAT_FUNCTION
    },
    NodeDef {
        id: "math.midpoint",
        label: "Midpoint",
        semantics: "`f32::midpoint(a, b)`, the point halfway between `a` and `b`. It does not overflow for large inputs and is not `(a + b) * 0.5` written out. The host computes it when the model is built, so both inputs take a constant or a parameter, and parity is exact.",
        group: "Arithmetic",
        inputs: &[
            input_up_to("a", F32_TYPE, Rate::Parameter),
            input_up_to("b", F32_TYPE, Rate::Parameter),
        ],
        outputs: &[output("out", F32_TYPE)],
        cases: &[
            case("between", &[("a", F32(3.0)), ("b", F32(15.0))], &[("out", F32(9.0))]),
            case(
                "no overflow",
                &[("a", F32(f32::MAX)), ("b", F32(f32::MAX))],
                &[("out", F32(f32::MAX))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "math.expr",
        label: "Expression",
        semantics: "An expression over the inputs `config.inputs`, each `{ name, type }`, that sets the outputs `config.outputs`. The text is a list of assignments `name = expression`, separated by `;` or a new line, each setting an output once, in an order where an assignment reads only the inputs and the outputs set before it. An expression holds number literals, input and output names, `+ - * / %`, unary `-`, the comparisons `< <= > >= == !=`, `and`, `or`, `not`, `true`, `false`, the components `.x` and `.y` of a vector, `if(cond, a, b)` and calls of `min`, `max`, `clamp`, `abs`, `floor`, `ceil`, `trunc`, `round`, `sqrt`, `recip`, `exp`, `log`, `log2`, `log10`, `sin`, `cos`, `pow`, `vec`, `dot` and `length`. Operators bind as in Rust, `not` above the comparisons, and `and` above `or`. An integer literal takes the type of the other operand, `u32` when that is not a number, and a literal with a point is `f32`, or `f64` beside an `f64`. A vector times or by a scalar scales it. The node expands into the arithmetic, comparison, logic and vector kinds it names, operand order kept, before the file is checked, and has their semantics.",
        group: "Arithmetic",
        config: &[
            config("text", ConfigType::ExpressionText),
            config("inputs", ConfigType::ExpressionInputs),
            config("outputs", ConfigType::NameList),
        ],
        config_ports: ConfigPorts::Expression,
        cost: NodeCost::Expression,
        cases: &[
            pass_case(
                "operators bind as in Rust",
                PassCase {
                    width: 2,
                    tables: expression_case!("f32", "out = a + b * 2.0 - 1"),
                    before: &[("a", &[F32(1.0), F32(0.5)]), ("b", &[F32(3.0), F32(0.25)])],
                    after: &[("out", &[F32(6.0), F32(0.0)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a choice and a call",
                PassCase {
                    width: 2,
                    tables: expression_case!("f32", "out = if(a > b and not (a == 0), max(a, b), -b)"),
                    before: &[("a", &[F32(4.0), F32(0.5)]), ("b", &[F32(3.0), F32(2.0)])],
                    after: &[("out", &[F32(4.0), F32(-2.0)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "a comparison",
                PassCase {
                    width: 2,
                    tables: expression_case!("bool", "out = a <= b or a % 2 == 1"),
                    before: &[("a", &[F32(5.0), F32(4.0)]), ("b", &[F32(3.0), F32(3.0)])],
                    after: &[("out", &[Bool(true), Bool(false)])],
                    ..PassCase::EMPTY
                },
            ),
            pass_case(
                "two outputs and a vector",
                PassCase {
                    tables: TWO_OUTPUTS,
                    before: &[("a", &[Vec2F([1.5, 4.0])])],
                    after: &[("x", &[F32(4.0)]), ("v", &[Vec2F([2.0, 2.0])])],
                    ..PassCase::EMPTY
                },
            ),
        ],
        ..BASE
    },
];
