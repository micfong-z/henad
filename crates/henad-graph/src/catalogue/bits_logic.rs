//! Integer bit kinds, comparisons, logic and choice.

use super::cases::{case, config_case, host_case};
use super::{
    BASE, BOOL_TYPE,
    CaseValue::{Bool, F32, I32, U32, U64, Variant, Vec2I, Vec2U},
    ConfigPorts, ConfigType, InputDefault, InputSocket, NodeCost, NodeDef, SocketType, TypeClass, U32_TYPE, config,
    input, output,
};
use crate::types::Rate;

const INTEGERS: &[TypeClass] = &[TypeClass::U32, TypeClass::I32, TypeClass::U64];
const SCALARS: &[TypeClass] = &[
    TypeClass::U32,
    TypeClass::I32,
    TypeClass::F32,
    TypeClass::U64,
    TypeClass::F64,
];
const ANY_VALUE: &[TypeClass] = &[
    TypeClass::Bool,
    TypeClass::U32,
    TypeClass::I32,
    TypeClass::F32,
    TypeClass::Vec2U,
    TypeClass::Vec2I,
    TypeClass::Vec2F,
    TypeClass::U64,
    TypeClass::F64,
    TypeClass::Vec2D,
    TypeClass::Category,
];
const COMPARED: &[TypeClass] = ANY_VALUE;

/// Bit kinds on two integers of one type.
const BITS: NodeDef = NodeDef {
    group: "Bits",
    type_var: INTEGERS,
    inputs: &[input("a", SocketType::Var), input("b", SocketType::Var)],
    outputs: &[output("out", SocketType::Var)],
    ..BASE
};

/// Shifts of an integer by a `u32`.
const SHIFT: NodeDef = NodeDef {
    inputs: &[input("a", SocketType::Var), input("b", U32_TYPE)],
    ..BITS
};

/// Comparisons of two values of one type.
const COMPARE: NodeDef = NodeDef {
    group: "Comparison",
    type_var: COMPARED,
    inputs: &[input("a", SocketType::Var), input("b", SocketType::Var)],
    outputs: &[output("out", BOOL_TYPE)],
    ..BASE
};

/// Orderings of two scalars of one type.
const ORDER: NodeDef = NodeDef {
    type_var: SCALARS,
    ..COMPARE
};

/// Logic on two truth values.
const LOGIC: NodeDef = NodeDef {
    group: "Logic",
    inputs: &[input("a", BOOL_TYPE), input("b", BOOL_TYPE)],
    outputs: &[output("out", BOOL_TYPE)],
    ..BASE
};

/// Rule list of SIR: two rules in one phase.
const SIR_RULES: &str = "config.category = \"health\"\nconfig.rules = [\n  { id = \"infect\", from = \"S\", to = \"I\", phase = 0 },\n  { id = \"recover\", from = \"I\", to = \"R\", phase = 0 },\n]\n";
/// Rule list of two phases, written out of order and with a gap.
const TWO_PHASES: &str = "config.category = \"health\"\nconfig.rules = [\n  { id = \"recover\", from = \"I\", to = \"R\", phase = 7 },\n  { id = \"infect\", from = \"S\", to = \"I\", phase = 2 },\n]\n";
/// Rule list whose first rule shadows the second within one phase.
const FIRST_WINS: &str = "config.category = \"health\"\nconfig.rules = [\n  { id = \"first\", from = \"S\", to = \"I\", phase = 0 },\n  { id = \"second\", from = \"S\", to = \"R\", phase = 0 },\n]\n";

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "bits.xor",
        label: "Bitwise xor",
        semantics: "Bitwise exclusive or of `a` and `b`.",
        cases: &[
            case(
                "u32",
                &[("a", U32(0b1100)), ("b", U32(0b1010))],
                &[("out", U32(0b0110))],
            ),
            case("i32", &[("a", I32(-1)), ("b", I32(1))], &[("out", I32(-2))]),
            host_case(
                "u64 on the host",
                &[("a", U64(1 << 40)), ("b", U64(1))],
                &[("out", U64((1 << 40) | 1))],
            ),
        ],
        ..BITS
    },
    NodeDef {
        id: "bits.and",
        label: "Bitwise and",
        semantics: "Bitwise and of `a` and `b`.",
        cases: &[
            case(
                "u32",
                &[("a", U32(0b1100)), ("b", U32(0b1010))],
                &[("out", U32(0b1000))],
            ),
            case("i32", &[("a", I32(-1)), ("b", I32(5))], &[("out", I32(5))]),
        ],
        ..BITS
    },
    NodeDef {
        id: "bits.or",
        label: "Bitwise or",
        semantics: "Bitwise or of `a` and `b`.",
        cases: &[
            case(
                "u32",
                &[("a", U32(0b1100)), ("b", U32(0b1010))],
                &[("out", U32(0b1110))],
            ),
            case(
                "i32",
                &[("a", I32(i32::MIN)), ("b", I32(1))],
                &[("out", I32(i32::MIN + 1))],
            ),
        ],
        ..BITS
    },
    NodeDef {
        id: "bits.not",
        label: "Bitwise not",
        semantics: "Every bit of `a` flipped.",
        inputs: &[input("a", SocketType::Var)],
        cases: &[
            case("u32", &[("a", U32(0))], &[("out", U32(u32::MAX))]),
            case("i32", &[("a", I32(0))], &[("out", I32(-1))]),
        ],
        ..BITS
    },
    NodeDef {
        id: "bits.shl",
        label: "Shift left",
        semantics: "`a` shifted left by `b`, with zeros shifted in. The amount is taken modulo the bit width of `a`, as WGSL takes it at run time, so a shift by 33 of a `u32` is a shift by 1.",
        cases: &[
            case("u32", &[("a", U32(1)), ("b", U32(4))], &[("out", U32(16))]),
            case(
                "bits leave at the top",
                &[("a", U32(0x8000_0001)), ("b", U32(1))],
                &[("out", U32(2))],
            ),
            case("amount modulo 32", &[("a", U32(1)), ("b", U32(33))], &[("out", U32(2))]),
            case("i32", &[("a", I32(-1)), ("b", U32(1))], &[("out", I32(-2))]),
            host_case(
                "u64 amount modulo 64",
                &[("a", U64(1)), ("b", U32(65))],
                &[("out", U64(2))],
            ),
        ],
        ..SHIFT
    },
    NodeDef {
        id: "bits.shr",
        label: "Shift right",
        semantics: "`a` shifted right by `b`: logical on `u32` and `u64`, arithmetic on `i32`. The amount is taken modulo the bit width of `a`, as WGSL takes it at run time.",
        cases: &[
            case(
                "u32 is logical",
                &[("a", U32(0x8000_0000)), ("b", U32(31))],
                &[("out", U32(1))],
            ),
            case(
                "i32 is arithmetic",
                &[("a", I32(-8)), ("b", U32(1))],
                &[("out", I32(-4))],
            ),
            case("amount modulo 32", &[("a", U32(4)), ("b", U32(33))], &[("out", U32(2))]),
        ],
        ..SHIFT
    },
    NodeDef {
        id: "cmp.eq",
        label: "Equal",
        semantics: "Whether `a` equals `b`: every component on a vector, the same variant on a category. On the host a NaN equals nothing, itself included, and a shader leaves that indeterminate.",
        cases: &[
            case("u32", &[("a", U32(3)), ("b", U32(3))], &[("out", Bool(true))]),
            case("f32", &[("a", F32(0.5)), ("b", F32(0.25))], &[("out", Bool(false))]),
            case(
                "vec2u needs both",
                &[("a", Vec2U([1, 2])), ("b", Vec2U([1, 3]))],
                &[("out", Bool(false))],
            ),
            case("bool", &[("a", Bool(true)), ("b", Bool(true))], &[("out", Bool(true))]),
            case(
                "category",
                &[("a", Variant("health", "I")), ("b", Variant("health", "I"))],
                &[("out", Bool(true))],
            ),
            host_case(
                "NaN",
                &[("a", F32(f32::NAN)), ("b", F32(f32::NAN))],
                &[("out", Bool(false))],
            ),
        ],
        ..COMPARE
    },
    NodeDef {
        id: "cmp.ne",
        label: "Not equal",
        semantics: "Negation of `cmp.eq`: any component differs on a vector. On the host a NaN differs from everything, itself included, and a shader leaves that indeterminate.",
        cases: &[
            case("u32", &[("a", U32(3)), ("b", U32(4))], &[("out", Bool(true))]),
            case(
                "vec2i needs one",
                &[("a", Vec2I([1, 2])), ("b", Vec2I([1, 3]))],
                &[("out", Bool(true))],
            ),
            case(
                "category",
                &[("a", Variant("health", "S")), ("b", Variant("health", "S"))],
                &[("out", Bool(false))],
            ),
            host_case(
                "NaN",
                &[("a", F32(f32::NAN)), ("b", F32(f32::NAN))],
                &[("out", Bool(true))],
            ),
        ],
        ..COMPARE
    },
    NodeDef {
        id: "cmp.lt",
        label: "Less",
        semantics: "Whether `a` is less than `b`. On the host a NaN compares false, and a shader leaves that indeterminate.",
        cases: &[
            case(
                "u32 is unsigned",
                &[("a", U32(1)), ("b", U32(u32::MAX))],
                &[("out", Bool(true))],
            ),
            case(
                "i32 is signed",
                &[("a", I32(1)), ("b", I32(-1))],
                &[("out", Bool(false))],
            ),
            case(
                "f32 equal",
                &[("a", F32(0.5)), ("b", F32(0.5))],
                &[("out", Bool(false))],
            ),
            host_case("NaN", &[("a", F32(f32::NAN)), ("b", F32(1.0))], &[("out", Bool(false))]),
        ],
        ..ORDER
    },
    NodeDef {
        id: "cmp.le",
        label: "Less or equal",
        semantics: "Whether `a` is less than or equal to `b`. On the host a NaN compares false, and a shader leaves that indeterminate.",
        cases: &[
            case("equal", &[("a", U32(3)), ("b", U32(3))], &[("out", Bool(true))]),
            case("greater", &[("a", I32(4)), ("b", I32(3))], &[("out", Bool(false))]),
            host_case(
                "NaN",
                &[("a", F32(f32::NAN)), ("b", F32(f32::NAN))],
                &[("out", Bool(false))],
            ),
        ],
        ..ORDER
    },
    NodeDef {
        id: "cmp.gt",
        label: "Greater",
        semantics: "Whether `a` is greater than `b`. On the host a NaN compares false, and a shader leaves that indeterminate.",
        cases: &[
            case("u32", &[("a", U32(4)), ("b", U32(3))], &[("out", Bool(true))]),
            case(
                "f32 equal",
                &[("a", F32(0.5)), ("b", F32(0.5))],
                &[("out", Bool(false))],
            ),
            host_case("NaN", &[("a", F32(1.0)), ("b", F32(f32::NAN))], &[("out", Bool(false))]),
        ],
        ..ORDER
    },
    NodeDef {
        id: "cmp.ge",
        label: "Greater or equal",
        semantics: "Whether `a` is greater than or equal to `b`. On the host a NaN compares false, and a shader leaves that indeterminate.",
        cases: &[
            case("equal", &[("a", U32(5)), ("b", U32(5))], &[("out", Bool(true))]),
            case("less", &[("a", F32(0.25)), ("b", F32(0.5))], &[("out", Bool(false))]),
            host_case("NaN", &[("a", F32(f32::NAN)), ("b", F32(0.0))], &[("out", Bool(false))]),
        ],
        ..ORDER
    },
    NodeDef {
        id: "logic.and",
        label: "And",
        semantics: "Whether `a` and `b` both hold. Both inputs are evaluated, with no short circuit.",
        cases: &[
            case("both", &[("a", Bool(true)), ("b", Bool(true))], &[("out", Bool(true))]),
            case("one", &[("a", Bool(true)), ("b", Bool(false))], &[("out", Bool(false))]),
        ],
        ..LOGIC
    },
    NodeDef {
        id: "logic.or",
        label: "Or",
        semantics: "Whether `a` or `b` holds. Both inputs are evaluated, with no short circuit.",
        cases: &[
            case("one", &[("a", Bool(false)), ("b", Bool(true))], &[("out", Bool(true))]),
            case(
                "neither",
                &[("a", Bool(false)), ("b", Bool(false))],
                &[("out", Bool(false))],
            ),
        ],
        ..LOGIC
    },
    NodeDef {
        id: "logic.xor",
        label: "Exclusive or",
        semantics: "Whether exactly one of `a` and `b` holds.",
        cases: &[
            case("one", &[("a", Bool(true)), ("b", Bool(false))], &[("out", Bool(true))]),
            case("both", &[("a", Bool(true)), ("b", Bool(true))], &[("out", Bool(false))]),
        ],
        ..LOGIC
    },
    NodeDef {
        id: "logic.not",
        label: "Not",
        semantics: "Negation of `a`.",
        inputs: &[input("a", BOOL_TYPE)],
        cases: &[
            case("true", &[("a", Bool(true))], &[("out", Bool(false))]),
            case("false", &[("a", Bool(false))], &[("out", Bool(true))]),
        ],
        ..LOGIC
    },
    NodeDef {
        id: "logic.select",
        label: "Select",
        semantics: "`if_true` when `cond` holds, otherwise `if_false`. Both arms are computed, and a draw in the arm not returned changes nothing, since a counter-based draw is a function of its key.",
        group: "Logic",
        type_var: ANY_VALUE,
        inputs: &[
            input("cond", BOOL_TYPE),
            input("if_true", SocketType::Var),
            input("if_false", SocketType::Var),
        ],
        outputs: &[output("out", SocketType::Var)],
        cases: &[
            case(
                "true arm",
                &[("cond", Bool(true)), ("if_true", U32(1)), ("if_false", U32(2))],
                &[("out", U32(1))],
            ),
            case(
                "false arm",
                &[
                    ("cond", Bool(false)),
                    ("if_true", Variant("health", "I")),
                    ("if_false", Variant("health", "S")),
                ],
                &[("out", Variant("health", "S"))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "logic.pick",
        label: "Pick",
        semantics: "`values[min(index, len - 1)]`, an entry of a constant list. An index past the end returns the last entry.",
        group: "Logic",
        type_var: ANY_VALUE,
        config: &[config("values", ConfigType::Literals)],
        inputs: &[input("index", U32_TYPE)],
        outputs: &[output("out", SocketType::Var)],
        cases: &[
            config_case(
                "in range",
                "config.values = [{ f32 = 0.5 }, { f32 = 0.25 }]\n",
                &[("index", U32(1))],
                &[("out", F32(0.25))],
            ),
            config_case(
                "past the end",
                "config.values = [{ u32 = 7 }, { u32 = 8 }]\n",
                &[("index", U32(99))],
                &[("out", U32(8))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "cat.switch",
        label: "Switch on category",
        semantics: "The input `case.<variant>` of the variant `select` holds, or `default` when that case is unwritten. Every variant needs its case unless `default` is written. Every input is evaluated, and only the choice depends on `select`.",
        group: "Logic",
        type_var: ANY_VALUE,
        config: &[config("category", ConfigType::CategoryId)],
        inputs: &[
            input("select", SocketType::ConfigCategory),
            InputSocket {
                name: "default",
                value_type: SocketType::Var,
                max_rate: Rate::Pair,
                default: InputDefault::Optional,
            },
        ],
        outputs: &[output("out", SocketType::Var)],
        config_ports: ConfigPorts::CategoryCases,
        cases: &[
            config_case(
                "named case",
                "config.category = \"health\"\n",
                &[
                    ("select", Variant("health", "I")),
                    ("case.S", U32(10)),
                    ("case.I", U32(20)),
                    ("case.R", U32(30)),
                ],
                &[("out", U32(20))],
            ),
            config_case(
                "default",
                "config.category = \"health\"\n",
                &[
                    ("select", Variant("health", "R")),
                    ("case.S", U32(10)),
                    ("default", U32(99)),
                ],
                &[("out", U32(99))],
            ),
        ],
        ..BASE
    },
    NodeDef {
        id: "cat.transition",
        label: "Transitions",
        semantics: "Rules over a category, each `{ id, from, to, phase }`, with `from` and `to` variant names and `phase` a `u32` compared as a number, gaps allowed. Phases run in ascending order, each on the state the previous phase left. Within a phase, the first rule in list order whose `from` equals the state and whose input `rule.<id>.when` holds fires, and sets the state to `to`, or to the input `rule.<id>.to` when that input is written. At most one rule fires per phase. `next` is the final state, and `fired.<id>` holds whether the rule fired.",
        group: "Logic",
        config: &[
            config("category", ConfigType::CategoryId),
            config("rules", ConfigType::TransitionRules),
        ],
        inputs: &[input("state", SocketType::ConfigCategory)],
        outputs: &[output("next", SocketType::ConfigCategory)],
        config_ports: ConfigPorts::TransitionRules,
        cost: NodeCost::PerRule(2),
        cases: &[
            config_case(
                "two rules in one phase",
                SIR_RULES,
                &[
                    ("state", Variant("health", "S")),
                    ("rule.infect.when", Bool(true)),
                    ("rule.recover.when", Bool(true)),
                ],
                &[
                    ("next", Variant("health", "I")),
                    ("fired.infect", Bool(true)),
                    ("fired.recover", Bool(false)),
                ],
            ),
            config_case(
                "no rule holds",
                SIR_RULES,
                &[
                    ("state", Variant("health", "I")),
                    ("rule.infect.when", Bool(true)),
                    ("rule.recover.when", Bool(false)),
                ],
                &[
                    ("next", Variant("health", "I")),
                    ("fired.infect", Bool(false)),
                    ("fired.recover", Bool(false)),
                ],
            ),
            config_case(
                "two phases run in ascending order",
                TWO_PHASES,
                &[
                    ("state", Variant("health", "S")),
                    ("rule.recover.when", Bool(true)),
                    ("rule.infect.when", Bool(true)),
                ],
                &[
                    ("next", Variant("health", "R")),
                    ("fired.recover", Bool(true)),
                    ("fired.infect", Bool(true)),
                ],
            ),
            config_case(
                "the first rule in a phase wins",
                FIRST_WINS,
                &[
                    ("state", Variant("health", "S")),
                    ("rule.first.when", Bool(true)),
                    ("rule.second.when", Bool(true)),
                ],
                &[
                    ("next", Variant("health", "I")),
                    ("fired.first", Bool(true)),
                    ("fired.second", Bool(false)),
                ],
            ),
            config_case(
                "a written target replaces `to`",
                SIR_RULES,
                &[
                    ("state", Variant("health", "S")),
                    ("rule.infect.when", Bool(true)),
                    ("rule.infect.to", Variant("health", "R")),
                    ("rule.recover.when", Bool(false)),
                ],
                &[
                    ("next", Variant("health", "R")),
                    ("fired.infect", Bool(true)),
                    ("fired.recover", Bool(false)),
                ],
            ),
        ],
        ..BASE
    },
];
