//! Function kinds: the call of an embedded function and the two ends of its body.

use super::cases::pass_case;
use super::{
    BASE,
    CaseValue::{F32, U32},
    ConfigPorts, ConfigType, InputSocket, NodeCost, NodeDef, PassCase, SocketType, config, input, output,
};
use crate::types::Rate;

/// A function `twice` of `x`, and a function `quad` that calls it twice, called from a pass on the lane `a`.
const TWICE: &str = r#"[[lane]]
id = "a"
domain = "cells"
type = "f32"

[[lane]]
id = "two"
domain = "cells"
type = "f32"

[[lane]]
id = "four"
domain = "cells"
type = "f32"

[[function]]
id = "twice"
kind = "pure"

[[function.input]]
port = "x"
type = "f32"

[[function.output]]
port = "y"
type = "f32"

[[function.node]]
id = "n101"
op = "fn.input"
config.port = "x"

[[function.node]]
id = "n102"
op = "math.add"
in.a = "n101"
in.b = "n101"

[[function.node]]
id = "n103"
op = "fn.output"
config.port = "y"
in.value = "n102"

[[function]]
id = "quad"
kind = "pure"

[[function.input]]
port = "x"
type = "f32"

[[function.output]]
port = "y"
type = "f32"

[[function.node]]
id = "n111"
op = "fn.input"
config.port = "x"

[[function.node]]
id = "n112"
op = "fn.call"
config.function = "twice"
config.key = 7
in.x = "n111"

[[function.node]]
id = "n113"
op = "fn.call"
config.function = "twice"
config.key = 8
in.x = "n112"

[[function.node]]
id = "n114"
op = "fn.output"
config.port = "y"
in.value = "n113"

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
op = "fn.call"
config.function = "twice"
config.key = 1
in.x = "n1"

[[pass.node]]
id = "n3"
op = "fn.call"
config.function = "quad"
config.key = 2
in.x = "n1"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "two"
in.value = "n2"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "four"
in.value = "n3.y"
"#;

/// A function whose output is a literal and whose input goes straight to an output, called on a parameter.
const PASS_THROUGH: &str = r#"[[lane]]
id = "same"
domain = "cells"
type = "u32"

[[lane]]
id = "seven"
domain = "cells"
type = "u32"

[[param]]
id = "p"
label = "P"
kind = "u32"
min = 0
max = 10
default = 3

[[function]]
id = "through"
kind = "pure"

[[function.input]]
port = "value"
type = "u32"

[[function.output]]
port = "value"
type = "u32"

[[function.output]]
port = "seven"
type = "u32"

[[function.node]]
id = "n101"
op = "fn.input"
config.port = "value"

[[function.node]]
id = "n102"
op = "fn.output"
config.port = "value"
in.value = "n101"

[[function.node]]
id = "n103"
op = "fn.output"
config.port = "seven"
in.value = { u32 = 7 }

[[pass]]
id = "case"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "fn.call"
config.function = "through"
config.key = 1
in.value = { param = "p" }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "same"
in.value = "n1.value"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "seven"
in.value = "n1.seven"
"#;

/// The case that calls `twice` and `quad`.
const TWICE_CASE: PassCase = PassCase {
    width: 2,
    tables: TWICE,
    before: &[("a", &[F32(1.5), F32(-0.25)])],
    after: &[("two", &[F32(3.0), F32(-0.5)]), ("four", &[F32(6.0), F32(-1.0)])],
    ..PassCase::EMPTY
};

/// The case of a function whose outputs need no node of its body.
const PASS_THROUGH_CASE: PassCase = PassCase {
    tables: PASS_THROUGH,
    after: &[("same", &[U32(3)]), ("seven", &[U32(7)])],
    ..PassCase::EMPTY
};

pub(super) static KINDS: &[NodeDef] = &[
    NodeDef {
        id: "fn.input",
        group: "Functions",
        label: "Function input",
        semantics: "Inside a function's body, the value the call gives the function's input `port`. It has the port's type and the rate of the call's input.",
        config: &[config("port", ConfigType::InterfacePort)],
        outputs: &[output("value", SocketType::InterfacePort)],
        cost: NodeCost::Units(0),
        cases: &[
            pass_case("reads the call's input", TWICE_CASE),
            pass_case("reaches an output directly", PASS_THROUGH_CASE),
        ],
        ..BASE
    },
    NodeDef {
        id: "fn.output",
        group: "Functions",
        label: "Function output",
        semantics: "Inside a function's body, sets the function's output `port` to `value`, of the port's type. Each output of a function takes exactly one `fn.output`.",
        config: &[config("port", ConfigType::InterfacePort)],
        inputs: &[InputSocket {
            max_rate: Rate::Pair,
            ..input("value", SocketType::InterfacePort)
        }],
        cost: NodeCost::Units(0),
        cases: &[
            pass_case("sets the call's output", TWICE_CASE),
            pass_case("a literal output", PASS_THROUGH_CASE),
        ],
        ..BASE
    },
    NodeDef {
        id: "fn.call",
        group: "Functions",
        label: "Call function",
        semantics: "An inlined copy of the body of `function`, whose inputs and outputs are the function's ports. Every random key `k` of a draw inside the copy becomes `fold32(mix_seed((key << 32) | k))` with the call's `key`, once per enclosing call, so two calls of one function draw apart. A function holds no lane write, deposit, counter, colour, reducer or stream draw, and its body is checked as a pure graph. The places a call may sit are those its body's kinds share, and its cost is its body's, plus one.",
        config: &[
            config("function", ConfigType::FunctionId),
            config(
                "key",
                ConfigType::Integer {
                    min: 0,
                    max: u32::MAX as i64,
                },
            ),
        ],
        config_ports: ConfigPorts::FunctionInterface,
        cases: &[
            pass_case("inlines the body, and calls nest", TWICE_CASE),
            pass_case("passes values through", PASS_THROUGH_CASE),
        ],
        ..BASE
    },
];
