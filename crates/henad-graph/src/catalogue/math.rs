//! Arithmetic kinds.
//!
//! Integer operations follow WGSL's total definitions on both backends. A shader may assume no NaN and no infinity, so
//! a case with one of those on either side of a float kind holds on the host alone.

use super::cases::{case, config_case, host_case};
use super::{
    BASE,
    CaseValue::{F32, I32, Nan, Near, U32, U64, Vec2F, Vec2I, Vec2U},
    ConfigType, F32_TYPE, NodeCost, NodeDef, Parity, SocketType, TypeClass, U32_TYPE, config, input, input_up_to,
    output,
};
use crate::limits::MAX_POWI_EXPONENT;
use crate::types::Rate;

const NUMERIC: &[TypeClass] = &[
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
        semantics: "Sum of `a` and `b`. Integers wrap and vectors add per component. A float NaN input gives NaN on the host.",
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
        semantics: "Difference `a - b`. Integers wrap and vectors subtract per component. A float NaN input gives NaN on the host.",
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
        semantics: "Product of `a` and `b`. Integers wrap and vectors multiply per component. A float NaN input gives NaN on the host.",
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
        semantics: "Truncated remainder of `a / b`, which takes the sign of `a`. Integer `x % 0` is 0 and `i32::MIN % -1` is 0. On floats the host computes `fmod`, a shader computes `a - b * trunc(a / b)`, and `x % 0` is NaN on the host and indeterminate in a shader, so parity is approximate.",
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
        semantics: "Absolute value of `a`, per component on vectors. Integers wrap, so `abs(i32::MIN)` is `i32::MIN`. On floats it clears the sign, so a NaN input stays NaN on the host and is indeterminate in a shader.",
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
        semantics: "Rounds `a` to the nearest whole number, per component on vectors. A tie goes to the even neighbour, as WGSL's `round` does, where Rust's `f32::round` goes away from zero. A NaN input stays NaN on the host and is indeterminate in a shader.",
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
        semantics: "`base` to the whole power `min(exponent, max_exponent)`, as Rust's `f32::powi`, with `x^0 = 1`. The host computes a table of `max_exponent + 1` entries when the model is built, and the element reads the table on both backends, so parity is exact.",
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
