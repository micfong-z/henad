//! Conversion kinds.

use super::cases::{case, config_case, host_case};
use super::{
    BASE,
    CaseValue::{Bool, F32, F64, I32, U32, U64, Variant},
    ConfigType, F32_TYPE, NodeDef, SocketType, TypeClass, U32_TYPE, config, input, output,
};

/// Conversions of one input `a` to one output `out`.
const CONVERSION: NodeDef = NodeDef {
    group: "Conversions",
    inputs: &[input("a", SocketType::Var)],
    ..BASE
};

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "conv.to_f32",
        label: "To f32",
        semantics: "`a` as the nearest `f32`, exact for integers below 2^24, and 0 or 1 from a `bool`.",
        type_var: &[
            TypeClass::U32,
            TypeClass::I32,
            TypeClass::Bool,
            TypeClass::U64,
            TypeClass::F64,
        ],
        outputs: &[output("out", F32_TYPE)],
        cases: &[
            case("u32", &[("a", U32(3))], &[("out", F32(3.0))]),
            case("i32", &[("a", I32(-2))], &[("out", F32(-2.0))]),
            case(
                "past 2^24 ties to even",
                &[("a", U32(16_777_217))],
                &[("out", F32(16_777_216.0))],
            ),
            case("bool", &[("a", Bool(true))], &[("out", F32(1.0))]),
            case("f64 on the host", &[("a", F64(0.5))], &[("out", F32(0.5))]),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.to_u32",
        label: "To u32",
        semantics: "`a` as a `u32`. From a float it truncates toward zero and clamps to the values of the float's type that a `u32` holds exactly, as WGSL converts: 0 to 4294967040 (`0x4f7fffff`, the largest `f32` below 2^32) from an `f32`, and 0 to `u32::MAX` from an `f64`. A NaN gives 0 on the host and is indeterminate in a shader, since a shader may assume no NaN. From an `i32` it reinterprets the bits, from a `bool` it gives 0 or 1, and from a `u64` it keeps the low 32 bits.",
        type_var: &[
            TypeClass::F32,
            TypeClass::I32,
            TypeClass::Bool,
            TypeClass::U64,
            TypeClass::F64,
        ],
        outputs: &[output("out", U32_TYPE)],
        cases: &[
            case("truncates", &[("a", F32(3.9))], &[("out", U32(3))]),
            case("saturates below", &[("a", F32(-1.0))], &[("out", U32(0))]),
            case(
                "clamps above to the largest f32 below 2^32",
                &[("a", F32(5e9))],
                &[("out", U32(4_294_967_040))],
            ),
            case(
                "the largest f32 below 2^32 is exact",
                &[("a", F32(f32::from_bits(0x4f7f_ffff)))],
                &[("out", U32(4_294_967_040))],
            ),
            host_case(
                "f64 clamps above to u32::MAX",
                &[("a", F64(5e9))],
                &[("out", U32(u32::MAX))],
            ),
            case("i32 keeps the bits", &[("a", I32(-1))], &[("out", U32(u32::MAX))]),
            case("bool", &[("a", Bool(true))], &[("out", U32(1))]),
            case(
                "u64 keeps the low bits",
                &[("a", U64(0x1_0000_0002))],
                &[("out", U32(2))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", U32(0))]),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.to_i32",
        label: "To i32",
        semantics: "`a` as an `i32`. From an `f32` it truncates toward zero and clamps to the `f32` values an `i32` holds exactly, as WGSL converts: `i32::MIN` to 2147483520 (`0x4effffff`, the largest `f32` below 2^31). A NaN gives 0 on the host and is indeterminate in a shader, since a shader may assume no NaN. From a `u32` it reinterprets the bits, and from a `bool` it gives 0 or 1.",
        type_var: &[TypeClass::F32, TypeClass::U32, TypeClass::Bool],
        outputs: &[output("out", SocketType::Fixed(TypeClass::I32))],
        cases: &[
            case("truncates toward zero", &[("a", F32(-3.9))], &[("out", I32(-3))]),
            case(
                "clamps above to the largest f32 below 2^31",
                &[("a", F32(3e9))],
                &[("out", I32(2_147_483_520))],
            ),
            case("saturates below", &[("a", F32(-3e9))], &[("out", I32(i32::MIN))]),
            case("u32 keeps the bits", &[("a", U32(u32::MAX))], &[("out", I32(-1))]),
            case("bool", &[("a", Bool(true))], &[("out", I32(1))]),
            host_case("NaN", &[("a", F32(f32::NAN))], &[("out", I32(0))]),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.u32_to_cat",
        label: "Number to category",
        semantics: "The variant of `category` numbered `min(a, variants - 1)`. A number past the last variant gives the last variant.",
        config: &[config("category", ConfigType::CategoryId)],
        inputs: &[input("a", U32_TYPE)],
        outputs: &[output("out", SocketType::ConfigCategory)],
        cases: &[
            config_case(
                "in range",
                "config.category = \"health\"\n",
                &[("a", U32(1))],
                &[("out", Variant("health", "I"))],
            ),
            config_case(
                "past the last",
                "config.category = \"health\"\n",
                &[("a", U32(99))],
                &[("out", Variant("health", "R"))],
            ),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.cat_to_u32",
        label: "Category to number",
        semantics: "The number of the variant `a` holds, counted from 0 in the order the category declares them.",
        type_var: &[TypeClass::Category],
        outputs: &[output("out", U32_TYPE)],
        cases: &[
            case("first", &[("a", Variant("health", "S"))], &[("out", U32(0))]),
            case("last", &[("a", Variant("health", "R"))], &[("out", U32(2))]),
        ],
        ..CONVERSION
    },
];
