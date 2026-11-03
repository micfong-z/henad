//! Node cases held to a reference evaluation written from the definitions, on the host.
//!
//! The reference is test code alone. It holds the written cases to the written semantics while no host evaluator
//! exists. Pass cases need a pass to run and wait for the evaluator.

use std::collections::BTreeMap;

use henad_core::authoring::primitives::rng::{pcg_hash, random_float, xorshift64};

use super::case_files::node_case_file;
use crate::GraphFile;
use crate::catalogue::{CaseForm, CaseValue, NodeCase, NodeDef, definitions, effective_config};
use crate::document::GraphDocument;
use crate::draw::{draw_bits, fold32, key_word, press_word};
use crate::value::{ConfigValue, Literal};

use CaseValue::{Bool, F32, F64, I32, U32, U64, Variant, Vec2F, Vec2I, Vec2U};

/// Result of a reference evaluation: the outputs, and the stream state after it.
struct Evaluation {
    outputs: BTreeMap<String, CaseValue>,
    stream: u64,
}

/// Applies `op` per component of numbers of one type.
fn numeric(
    a: CaseValue,
    b: CaseValue,
    unsigned: fn(u32, u32) -> u32,
    signed: fn(i32, i32) -> i32,
    float: fn(f32, f32) -> f32,
    wide: fn(u64, u64) -> u64,
    double: fn(f64, f64) -> f64,
) -> CaseValue {
    match (a, b) {
        (U32(a), U32(b)) => U32(unsigned(a, b)),
        (I32(a), I32(b)) => I32(signed(a, b)),
        (F32(a), F32(b)) => F32(float(a, b)),
        (U64(a), U64(b)) => U64(wide(a, b)),
        (F64(a), F64(b)) => F64(double(a, b)),
        (Vec2U(a), Vec2U(b)) => Vec2U([unsigned(a[0], b[0]), unsigned(a[1], b[1])]),
        (Vec2I(a), Vec2I(b)) => Vec2I([signed(a[0], b[0]), signed(a[1], b[1])]),
        (Vec2F(a), Vec2F(b)) => Vec2F([float(a[0], b[0]), float(a[1], b[1])]),
        _ => panic!("no numeric pair: {a:?} {b:?}"),
    }
}

/// Applies `op` per component of one float value.
fn float_unary(a: CaseValue, op: fn(f32) -> f32) -> CaseValue {
    match a {
        F32(a) => F32(op(a)),
        Vec2F([x, y]) => Vec2F([op(x), op(y)]),
        _ => panic!("no float: {a:?}"),
    }
}

/// Returns the ordering comparison of two scalars.
fn order(a: CaseValue, b: CaseValue) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (U32(a), U32(b)) => a.partial_cmp(&b),
        (I32(a), I32(b)) => a.partial_cmp(&b),
        (F32(a), F32(b)) => a.partial_cmp(&b),
        (U64(a), U64(b)) => a.partial_cmp(&b),
        (F64(a), F64(b)) => a.partial_cmp(&b),
        _ => panic!("no scalar pair: {a:?} {b:?}"),
    }
}

/// Returns the value of a configuration literal.
fn literal_value(literal: &Literal) -> CaseValue {
    match literal {
        Literal::Bool(flag) => Bool(*flag),
        Literal::U32(number) => U32(*number),
        Literal::I32(number) => I32(*number),
        Literal::F32(number) => F32(*number),
        _ => panic!("no case value for {literal:?}"),
    }
}

/// Returns the variant names of `category`.
fn variants<'a>(document: &'a GraphDocument, category: &str) -> Vec<&'a str> {
    document
        .categories
        .iter()
        .find(|known| known.id == category)
        .map(|known| known.variants.iter().map(|variant| variant.name.as_str()).collect())
        .unwrap_or_default()
}

/// Returns the case world's variant name equal to `name`.
fn static_name(name: &str) -> &'static str {
    ["S", "I", "R", "Dead", "Alive"]
        .into_iter()
        .find(|known| *known == name)
        .unwrap_or_else(|| panic!("no variant {name} in the case world"))
}

/// Evaluates one node case by the written semantics.
#[expect(clippy::too_many_lines, reason = "one arm per kind reads best as one table")]
fn evaluate(def: &NodeDef, case: &NodeCase, document: &GraphDocument) -> Evaluation {
    let node = &document
        .passes
        .iter()
        .find(|pass| pass.id == "case")
        .expect("the case pass")
        .body
        .nodes[0];
    let config = effective_config(def, node);
    let text = |key: &str| {
        config
            .get(key)
            .and_then(ConfigValue::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let integer = |key: &str| config.get(key).and_then(ConfigValue::as_integer).unwrap_or_default();
    let inputs: BTreeMap<&str, CaseValue> = case.inputs.iter().copied().collect();
    let input = |name: &str| {
        *inputs
            .get(name)
            .unwrap_or_else(|| panic!("{}: no input {name}", def.id))
    };
    let context = case.context;
    let mut stream = context.stream;
    let mut outputs = BTreeMap::new();
    let mut put = |name: &str, value: CaseValue| {
        outputs.insert(name.to_owned(), value);
    };
    let draw = || {
        draw_bits(
            key_word(integer("key") as u32, context.phase, context.seed),
            context.element,
            context.tick,
            context.iteration,
            0,
        )
    };
    match def.id {
        "ctx.index" => put("index", U32(context.element)),
        "ctx.tick" => put("tick", U32(context.tick)),
        "ctx.press_seed" => put("seed", U32(press_word(context.seed, context.press))),
        "math.add" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                u32::wrapping_add,
                i32::wrapping_add,
                |a, b| a + b,
                u64::wrapping_add,
                |a, b| a + b,
            ),
        ),
        "math.sub" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                u32::wrapping_sub,
                i32::wrapping_sub,
                |a, b| a - b,
                u64::wrapping_sub,
                |a, b| a - b,
            ),
        ),
        "math.mul" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                u32::wrapping_mul,
                i32::wrapping_mul,
                |a, b| a * b,
                u64::wrapping_mul,
                |a, b| a * b,
            ),
        ),
        "math.div" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                |a, b| a.checked_div(b).unwrap_or(a),
                |a, b| if b == 0 { a } else { a.wrapping_div(b) },
                |a, b| a / b,
                |a, b| a.checked_div(b).unwrap_or(a),
                |a, b| a / b,
            ),
        ),
        "math.rem" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                |a, b| a.checked_rem(b).unwrap_or(0),
                |a, b| if b == 0 { 0 } else { a.wrapping_rem(b) },
                |a, b| a % b,
                |a, b| a.checked_rem(b).unwrap_or(0),
                |a, b| a % b,
            ),
        ),
        "math.min" => put(
            "out",
            numeric(input("a"), input("b"), u32::min, i32::min, f32::min, u64::min, f64::min),
        ),
        "math.max" => put(
            "out",
            numeric(input("a"), input("b"), u32::max, i32::max, f32::max, u64::max, f64::max),
        ),
        "math.abs" => put(
            "out",
            match input("a") {
                I32(a) => I32(a.wrapping_abs()),
                value => float_unary(value, f32::abs),
            },
        ),
        "math.ceil" => put("out", float_unary(input("a"), f32::ceil)),
        "math.trunc" => put("out", float_unary(input("a"), f32::trunc)),
        "math.round" => put("out", float_unary(input("a"), f32::round_ties_even)),
        "math.exp" => put("out", float_unary(input("a"), f32::exp)),
        "math.log" => put("out", float_unary(input("a"), f32::ln)),
        "math.sin" => put("out", float_unary(input("a"), f32::sin)),
        "math.cos" => put("out", float_unary(input("a"), f32::cos)),
        "math.powi" => {
            let (F32(base), U32(exponent)) = (input("base"), input("exponent")) else {
                panic!("powi inputs")
            };
            let exponent = exponent.min(integer("max_exponent") as u32);
            put("out", F32(base.powi(exponent as i32)));
        }
        "math.pow" => {
            let (F32(base), F32(exponent)) = (input("base"), input("exponent")) else {
                panic!("pow inputs")
            };
            put("out", F32(base.powf(exponent)));
        }
        "bits.xor" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                |a, b| a ^ b,
                |a, b| a ^ b,
                |_, _| f32::NAN,
                |a, b| a ^ b,
                |_, _| f64::NAN,
            ),
        ),
        "bits.and" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                |a, b| a & b,
                |a, b| a & b,
                |_, _| f32::NAN,
                |a, b| a & b,
                |_, _| f64::NAN,
            ),
        ),
        "bits.or" => put(
            "out",
            numeric(
                input("a"),
                input("b"),
                |a, b| a | b,
                |a, b| a | b,
                |_, _| f32::NAN,
                |a, b| a | b,
                |_, _| f64::NAN,
            ),
        ),
        "bits.not" => put(
            "out",
            match input("a") {
                U32(a) => U32(!a),
                I32(a) => I32(!a),
                U64(a) => U64(!a),
                value => panic!("not: {value:?}"),
            },
        ),
        "bits.shl" | "bits.shr" => {
            let U32(amount) = input("b") else {
                panic!("shift amount")
            };
            let left = def.id == "bits.shl";
            put(
                "out",
                match input("a") {
                    U32(a) => U32(if left {
                        a.wrapping_shl(amount)
                    } else {
                        a.wrapping_shr(amount)
                    }),
                    I32(a) => I32(if left {
                        a.wrapping_shl(amount)
                    } else {
                        a.wrapping_shr(amount)
                    }),
                    U64(a) => U64(if left {
                        a.wrapping_shl(amount)
                    } else {
                        a.wrapping_shr(amount)
                    }),
                    value => panic!("shift: {value:?}"),
                },
            );
        }
        "cmp.eq" => put("out", Bool(input("a") == input("b"))),
        "cmp.ne" => put("out", Bool(input("a") != input("b"))),
        "cmp.lt" => put(
            "out",
            Bool(order(input("a"), input("b")) == Some(std::cmp::Ordering::Less)),
        ),
        "cmp.le" => put(
            "out",
            Bool(order(input("a"), input("b")).is_some_and(std::cmp::Ordering::is_le)),
        ),
        "cmp.gt" => put(
            "out",
            Bool(order(input("a"), input("b")) == Some(std::cmp::Ordering::Greater)),
        ),
        "cmp.ge" => put(
            "out",
            Bool(order(input("a"), input("b")).is_some_and(std::cmp::Ordering::is_ge)),
        ),
        "logic.and" | "logic.or" | "logic.xor" | "logic.not" => {
            let a = input("a") == Bool(true);
            let b = || input("b") == Bool(true);
            put(
                "out",
                Bool(match def.id {
                    "logic.and" => a && b(),
                    "logic.or" => a || b(),
                    "logic.xor" => a != b(),
                    _ => !a,
                }),
            );
        }
        "logic.select" => put(
            "out",
            if input("cond") == Bool(true) {
                input("if_true")
            } else {
                input("if_false")
            },
        ),
        "logic.pick" => {
            let U32(index) = input("index") else {
                panic!("pick index")
            };
            let values = config.get("values").and_then(ConfigValue::as_array).expect("values");
            let chosen = &values[(index as usize).min(values.len() - 1)];
            put(
                "out",
                literal_value(&crate::check::literal_for_tests(chosen).expect("a literal")),
            );
        }
        "cat.switch" => {
            let Variant(_, current) = input("select") else {
                panic!("select")
            };
            let chosen = inputs
                .get(format!("case.{current}").as_str())
                .copied()
                .unwrap_or_else(|| input("default"));
            put("out", chosen);
        }
        "cat.transition" => {
            let category = text("category");
            let Variant(_, mut state) = input("state") else {
                panic!("state")
            };
            let rules = config.get("rules").and_then(ConfigValue::as_array).expect("rules");
            let rules: Vec<(String, String, Option<String>, i64)> = rules
                .iter()
                .map(|rule| {
                    let table = rule.as_table().expect("a rule");
                    let field = |key: &str| table.get(key).and_then(ConfigValue::as_str).map(str::to_owned);
                    (
                        field("id").expect("id"),
                        field("from").expect("from"),
                        field("to"),
                        table["phase"].as_integer().expect("phase"),
                    )
                })
                .collect();
            let mut phases: Vec<i64> = rules.iter().map(|rule| rule.3).collect();
            phases.sort_unstable();
            phases.dedup();
            let mut fired = BTreeMap::new();
            let names = variants(document, &category);
            let mut state_name = state.to_owned();
            for phase in phases {
                let start = state_name.clone();
                for (id, from, to, rule_phase) in &rules {
                    if *rule_phase != phase
                        || *from != start
                        || inputs.get(format!("rule.{id}.when").as_str()) != Some(&Bool(true))
                    {
                        continue;
                    }
                    state_name = match inputs.get(format!("rule.{id}.to").as_str()) {
                        Some(Variant(_, target)) => (*target).to_owned(),
                        _ => to.clone().expect("a target"),
                    };
                    fired.insert(id.clone(), true);
                    break;
                }
            }
            assert!(names.contains(&state_name.as_str()), "{state_name} is no variant");
            state = static_name(&state_name);
            put("next", Variant("health", state));
            for (id, ..) in &rules {
                put(&format!("fired.{id}"), Bool(fired.contains_key(id)));
            }
        }
        "conv.to_f32" => put(
            "out",
            F32(match input("a") {
                U32(a) => a as f32,
                I32(a) => a as f32,
                Bool(a) => f32::from(u8::from(a)),
                U64(a) => a as f32,
                F64(a) => a as f32,
                value => panic!("to_f32: {value:?}"),
            }),
        ),
        "conv.to_u32" => put(
            "out",
            U32(match input("a") {
                F32(a) => a as u32,
                I32(a) => a as u32,
                Bool(a) => u32::from(a),
                U64(a) => a as u32,
                F64(a) => a as u32,
                value => panic!("to_u32: {value:?}"),
            }),
        ),
        "conv.to_i32" => put(
            "out",
            I32(match input("a") {
                F32(a) => a as i32,
                U32(a) => a as i32,
                Bool(a) => i32::from(a),
                value => panic!("to_i32: {value:?}"),
            }),
        ),
        "conv.u32_to_cat" => {
            let U32(a) = input("a") else { panic!("a") };
            let names = variants(document, &text("category"));
            let name = names[(a as usize).min(names.len() - 1)];
            put("out", Variant("health", static_name(name)));
        }
        "conv.cat_to_u32" => {
            let Variant(category, name) = input("a") else {
                panic!("a")
            };
            let names = variants(document, category);
            put(
                "out",
                U32(names.iter().position(|known| *known == name).expect("a variant") as u32),
            );
        }
        "rand.float" => {
            let F32(max) = input("max") else { panic!("max") };
            put("out", F32(random_float(draw(), max)));
        }
        "rand.below" => {
            let U32(threshold) = input("threshold") else {
                panic!("threshold")
            };
            put("out", Bool(draw() < threshold));
        }
        "rand.chance" => {
            let F32(p) = input("p") else { panic!("p") };
            put("out", Bool(random_float(draw(), 1.0) < p));
        }
        "rng.threshold" => {
            let F32(p) = input("p") else { panic!("p") };
            put("out", U32((p * u32::MAX as f32) as u32));
        }
        "rng.hash" => {
            let U32(x) = input("x") else { panic!("x") };
            put("out", U32(pcg_hash(x)));
        }
        "rng.to_float" => {
            let (U32(bits), F32(max)) = (input("bits"), input("max")) else {
                panic!("to_float")
            };
            put("out", F32(random_float(bits, max)));
        }
        "rng.below" => {
            let (U32(bits), U32(threshold)) = (input("bits"), input("threshold")) else {
                panic!("below")
            };
            put("out", Bool(bits < threshold));
        }
        "rng.lane_seed" => {
            let salt = crate::reader::read_u64(&text("salt")).expect("a salt");
            let word = context.seed.map_or(salt, |seed| {
                henad_core::authoring::primitives::rng::mix_seed(seed ^ salt)
            });
            put("out", U32(fold32(word)));
        }
        "stream.bits" => {
            if input("when") == Bool(true) {
                stream = xorshift64(stream);
                put("bits", U32((stream >> 32) as u32));
            } else {
                put("bits", U32(0));
            }
        }
        "grid.life_like" => {
            let (Variant(_, cell), U32(count)) = (input("cell"), input("count")) else {
                panic!("life_like")
            };
            let listed = |key: &str| {
                config
                    .get(key)
                    .and_then(ConfigValue::as_array)
                    .expect("a list")
                    .iter()
                    .any(|item| item.as_integer() == Some(i64::from(count)))
            };
            let alive = text("alive");
            let lives = if cell == alive {
                listed("survive")
            } else {
                listed("birth")
            };
            put("next", Variant("life", if lives { "Alive" } else { "Dead" }));
        }
        "view.ramp" => {
            let (F32(value), F32(low), F32(high)) = (input("value"), input("low"), input("high")) else {
                panic!("ramp")
            };
            let palette = text("palette");
            let len = document
                .palettes
                .iter()
                .find(|known| known.id == palette)
                .expect("a palette")
                .colors
                .len();
            let index = if high > low && !value.is_nan() {
                (((value - low) / (high - low) * len as f32).floor().max(0.0) as usize).min(len - 1)
            } else {
                0
            };
            put("index", U32(index as u32));
        }
        other => panic!("no reference for {other}"),
    }
    Evaluation { outputs, stream }
}

/// Returns whether `found` meets the expected value `expected`.
fn meets(expected: CaseValue, found: CaseValue) -> bool {
    match (expected, found) {
        (CaseValue::Nan, F32(value)) => value.is_nan(),
        (CaseValue::Near(target, within), F32(value)) => (value - target).abs() <= within,
        (F32(expected), F32(value)) => expected.to_bits() == value.to_bits(),
        (Vec2F(expected), Vec2F(value)) => expected
            .iter()
            .zip(value)
            .all(|(left, right)| left.to_bits() == right.to_bits()),
        (F64(expected), F64(value)) => expected.to_bits() == value.to_bits(),
        (expected, value) => expected == value,
    }
}

#[test]
fn every_node_case_meets_the_reference_evaluation() {
    let mut checked = 0;
    for def in definitions() {
        for case in def.cases {
            let CaseForm::Node(node_case) = &case.form else {
                continue;
            };
            let file = GraphFile::parse(&node_case_file(def, node_case)).expect("the case reads");
            let evaluation = evaluate(def, node_case, file.document());
            for (port, expected) in node_case.outputs {
                let found = *evaluation
                    .outputs
                    .get(*port)
                    .unwrap_or_else(|| panic!("{} '{}': no output {port}", def.id, case.name));
                assert!(
                    meets(*expected, found),
                    "{} '{}': `{port}` is {found:?}, and the case expects {expected:?}",
                    def.id,
                    case.name
                );
            }
            assert_eq!(
                evaluation.outputs.len(),
                node_case.outputs.len(),
                "{} '{}' leaves an output unchecked",
                def.id,
                case.name
            );
            if let Some(after) = node_case.stream_after {
                assert_eq!(evaluation.stream, after, "{} '{}'", def.id, case.name);
            }
            checked += 1;
        }
    }
    assert!(checked > 150, "only {checked} node cases");
}
