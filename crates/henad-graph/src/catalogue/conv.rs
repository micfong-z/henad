//! Conversion kinds.

use super::cases::{case, config_case, host_case};
use super::{
    BASE,
    CaseValue::{Bool, F32, F64, I32, U32, U64, Variant, Vec2F, Vec2I, Vec2U},
    ConfigType, ContextSet, F32_TYPE, NodeDef, SocketType, TypeClass, U32_TYPE, config, input, output,
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
            host_case("f64 on the host", &[("a", F64(0.5))], &[("out", F32(0.5))]),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.to_u32",
        label: "To u32",
        semantics: "`a` as a `u32`. From a float it truncates toward zero and clamps to the values of the float's type that a `u32` holds exactly, as WGSL converts: 0 to 4294967040 (`0x4f7fffff`, the largest `f32` below 2^32) from an `f32`, and 0 to `u32::MAX` from an `f64`. A NaN returns 0 on the host and is indeterminate in a shader, since a shader can assume no NaN. From an `i32` it reinterprets the bits, from a `bool` it returns 0 or 1, from a `u64` it keeps the low 32 bits, and from a cell or agent reference it returns the index, `u32::MAX` for the reference to no cell.",
        type_var: &[
            TypeClass::F32,
            TypeClass::I32,
            TypeClass::Bool,
            TypeClass::U64,
            TypeClass::F64,
            TypeClass::Cell,
            TypeClass::Agent,
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
            host_case(
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
        semantics: "`a` as an `i32`. From an `f32` it truncates toward zero and clamps to the `f32` values an `i32` holds exactly, as WGSL converts: `i32::MIN` to 2147483520 (`0x4effffff`, the largest `f32` below 2^31). A NaN returns 0 on the host and is indeterminate in a shader, since a shader can assume no NaN. From a `u32` it reinterprets the bits, and from a `bool` it returns 0 or 1.",
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
        semantics: "The variant of `category` numbered `min(a, variants - 1)`. A number past the last variant returns the last variant.",
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

/// 2^64, the nearest `f64` to `u64::MAX`.
const TWO_POW_64: f64 = 65_536.0 * 65_536.0 * 65_536.0 * 65_536.0;

/// Kinds of the group that the palette lists after [`KINDS`].
pub(super) static MORE_KINDS: &[NodeDef] = &[
    NodeDef {
        id: "conv.to_f64",
        label: "To f64",
        semantics: "`a` as an `f64`: exact from an `f32`, a `u32` and an `i32`, and the nearest `f64` from a `u64`. An `f64` exists on the host alone, so the kind runs in an init pass, where it is a value per element, and in the post block of a stats pass.",
        type_var: &[TypeClass::F32, TypeClass::U32, TypeClass::I32, TypeClass::U64],
        outputs: &[output("out", SocketType::Fixed(TypeClass::F64))],
        backends: super::Backends::HOST,
        contexts: ContextSet {
            init: true,
            stats: true,
            region: true,
            ..ContextSet::NONE
        },
        cases: &[
            super::cases::host_case("f32 exactly", &[("a", F32(0.1))], &[("out", F64(0.1f32 as f64))]),
            super::cases::host_case("i32", &[("a", I32(-2))], &[("out", F64(-2.0))]),
            super::cases::host_case("u32", &[("a", U32(u32::MAX))], &[("out", F64(4_294_967_295.0))]),
            super::cases::host_case("u64 nearest", &[("a", U64(u64::MAX))], &[("out", F64(TWO_POW_64))]),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.to_vec2f",
        label: "To vec2f",
        semantics: "`a` as a `vec2f`, each component the nearest `f32`, exact below 2^24.",
        type_var: &[TypeClass::Vec2I, TypeClass::Vec2U],
        outputs: &[output("out", SocketType::Fixed(TypeClass::Vec2F))],
        cases: &[
            case("vec2i", &[("a", Vec2I([-1, 2]))], &[("out", Vec2F([-1.0, 2.0]))]),
            case(
                "vec2u",
                &[("a", Vec2U([3, 16_777_217]))],
                &[("out", Vec2F([3.0, 16_777_216.0]))],
            ),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.to_vec2i",
        label: "To vec2i",
        semantics: "`a` as a `vec2i`, per component as `conv.to_i32`: a float truncates toward zero and clamps to `i32::MIN` to 2147483520, and a `u32` reinterprets the bits. A NaN component returns 0 on the host and is indeterminate in a shader.",
        type_var: &[TypeClass::Vec2F, TypeClass::Vec2U],
        outputs: &[output("out", SocketType::Fixed(TypeClass::Vec2I))],
        cases: &[
            case(
                "truncates toward zero",
                &[("a", Vec2F([-1.5, 2.5]))],
                &[("out", Vec2I([-1, 2]))],
            ),
            case(
                "clamps",
                &[("a", Vec2F([3e9, -3e9]))],
                &[("out", Vec2I([2_147_483_520, i32::MIN]))],
            ),
            case(
                "vec2u keeps the bits",
                &[("a", Vec2U([u32::MAX, 1]))],
                &[("out", Vec2I([-1, 1]))],
            ),
        ],
        ..CONVERSION
    },
    NodeDef {
        id: "conv.to_vec2u",
        label: "To vec2u",
        semantics: "`a` as a `vec2u`, per component as `conv.to_u32`: a float truncates toward zero and clamps to 0 to 4294967040, and an `i32` reinterprets the bits. A NaN component returns 0 on the host and is indeterminate in a shader.",
        type_var: &[TypeClass::Vec2F, TypeClass::Vec2I],
        outputs: &[output("out", SocketType::Fixed(TypeClass::Vec2U))],
        cases: &[
            case("truncates", &[("a", Vec2F([-1.0, 3.9]))], &[("out", Vec2U([0, 3]))]),
            case(
                "vec2i keeps the bits",
                &[("a", Vec2I([-1, 2]))],
                &[("out", Vec2U([u32::MAX, 2]))],
            ),
        ],
        ..CONVERSION
    },
];
