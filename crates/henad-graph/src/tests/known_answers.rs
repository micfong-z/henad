//! Every known-answer case, node and pass, held to the host evaluator.
//!
//! A node case runs its node through the evaluator's element block with the case's context. Its inputs go into the
//! document as they are, NaN and infinity included, which a file cannot write. A pass case runs the pass `case` over
//! the lanes the case gives, and compares the lanes, statistics and colours it leaves.

use super::case_files::{node_case_file, pass_case_file};
use crate::GraphFile;
use crate::catalogue::{CaseForm, CaseValue, NodeCase, NodeDef, PassCase, definitions};
use crate::document::{GraphDocument, PassStage};
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::ids::NodeId;
use crate::value::{InputValue, Literal, MemberName, PortRef};
use henad_core::authoring::primitives::space::Boundary;

/// Returns the literal of an input value, which may be a float a file cannot write.
pub(super) fn input_literal(value: CaseValue) -> Literal {
    match value {
        CaseValue::Bool(flag) => Literal::Bool(flag),
        CaseValue::U32(number) => Literal::U32(number),
        CaseValue::I32(number) => Literal::I32(number),
        CaseValue::F32(number) | CaseValue::Near(number, _) => Literal::F32(number),
        CaseValue::Nan => Literal::F32(f32::NAN),
        CaseValue::U64(number) => Literal::U64(number),
        CaseValue::F64(number) => Literal::F64(number),
        CaseValue::Vec2U(pair) => Literal::Vec2U(pair),
        CaseValue::Vec2I(pair) => Literal::Vec2I(pair),
        CaseValue::Vec2F(pair) => Literal::Vec2F(pair),
        CaseValue::Vec2D([x, _]) => Literal::F64(x),
        CaseValue::Variant(category, variant) => Literal::Category(MemberName {
            owner: category.to_owned(),
            member: variant.to_owned(),
        }),
    }
}

/// Returns whether `found` meets the expected value `expected`: floats bit for bit, except a NaN or a tolerance.
pub(super) fn meets(expected: CaseValue, found: HostValue, document: &GraphDocument) -> bool {
    match (expected, found) {
        (CaseValue::Nan, HostValue::F32(value)) => value.is_nan(),
        (CaseValue::Near(target, within), HostValue::F32(value)) => (value - target).abs() <= within,
        (CaseValue::F32(expected), HostValue::F32(value)) => expected.to_bits() == value.to_bits(),
        (CaseValue::Vec2F(expected), HostValue::Vec2F(value)) => expected
            .iter()
            .zip(value)
            .all(|(left, right)| left.to_bits() == right.to_bits()),
        (CaseValue::F64(expected), HostValue::F64(value)) => expected.to_bits() == value.to_bits(),
        (CaseValue::Vec2D(expected), HostValue::Vec2D(value)) => expected
            .iter()
            .zip(value)
            .all(|(left, right)| left.to_bits() == right.to_bits()),
        (expected, found) => {
            HostValue::from_literal(&input_literal(expected), document).is_some_and(|value| value == found)
        }
    }
}

/// Runs a node case and fails on the first output that differs.
fn run_node_case(def: &NodeDef, name: &str, case: &NodeCase) {
    let file = GraphFile::parse(&node_case_file(def, case)).expect("the case reads");
    assert!(
        !file.check().has_errors(),
        "{} '{name}': {:?}",
        def.id,
        file.check().diagnostics
    );
    let mut document = file.document().clone();
    let pass = document
        .passes
        .iter_mut()
        .find(|pass| pass.id == "case")
        .expect("the case pass");
    let stage = pass.stage;
    let node = &mut pass.body.nodes[0];
    for (port, value) in case.inputs {
        node.inputs
            .insert((*port).to_owned(), InputValue::Literal(input_literal(*value)));
    }
    let evaluator = HostEvaluator::from_document(&document).expect("the case compiles");
    let lanes = HostLanes::new(evaluator.document(), HostWorld::of(evaluator.document())).expect("the lanes");
    let context = &case.context;
    let run_context = RunContext {
        seed: context.seed,
        phase: context.phase,
        tick: context.tick,
        tick_word: if stage == PassStage::Action {
            context.press
        } else {
            context.tick
        },
        press: context.press,
    };
    let mut stream = context.stream;
    let values = evaluator
        .probe(
            "case",
            &lanes,
            &run_context,
            context.element,
            context.iteration,
            &mut stream,
        )
        .unwrap_or_else(|error| panic!("{} '{name}': {error}", def.id));
    let n1 = NodeId::new(1).expect("an id");
    for (port, expected) in case.outputs {
        let found = *values
            .get(&(n1, (*port).to_owned()))
            .unwrap_or_else(|| panic!("{} '{name}': no output {port}", def.id));
        assert!(
            meets(*expected, found, evaluator.document()),
            "{} '{name}': `{port}` is {found:?}, and the case expects {expected:?}",
            def.id
        );
    }
    let produced = values.keys().filter(|(node, _)| *node == n1).count();
    assert_eq!(
        produced,
        case.outputs.len(),
        "{} '{name}' leaves an output unchecked",
        def.id
    );
    if let Some(after) = case.stream_after {
        assert_eq!(stream, after, "{} '{name}'", def.id);
    }
}

/// Runs a pass case and fails on the first lane, statistic or colour that differs.
fn run_pass_case(def: &NodeDef, name: &str, case: &PassCase) {
    let file = GraphFile::parse(&pass_case_file(case)).expect("the case reads");
    let evaluator =
        HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{} '{name}': {:?}", def.id, error.diagnostics()));
    let document = evaluator.document();
    let world = case_world(case);
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    for (counter, value) in case.counters_before {
        assert!(
            lanes.set_counter(counter, *value),
            "{} '{name}': counter {counter}",
            def.id
        );
    }
    for (lane, values) in case.before {
        let column = lanes.column_mut(lane).expect("a lane of the case");
        for (index, value) in values.iter().enumerate() {
            let value = HostValue::from_literal(&input_literal(*value), document).expect("a value");
            assert!(
                column.set(index, value),
                "{} '{name}': lane {lane} takes {value:?}",
                def.id
            );
        }
    }
    let pass = document
        .passes
        .iter()
        .find(|pass| pass.id == "case")
        .expect("the case pass");
    let context = match pass.stage {
        PassStage::Init => RunContext::init(None),
        PassStage::Action => RunContext::action(None, 0, 1, 0),
        _ => RunContext::tick(None, 0),
    };
    let mut stream = 1;
    let outcome = evaluator
        .run_pass("case", &mut lanes, &[], &context, &mut stream)
        .unwrap_or_else(|error| panic!("{} '{name}': {error}", def.id));
    for (lane, expected) in case.after {
        let column = lanes.column(lane).expect("a lane of the case");
        for (index, value) in expected.iter().enumerate() {
            let found = column.get(index).expect("an element");
            assert!(
                meets(*value, found, document),
                "{} '{name}': {lane}[{index}] is {found:?}, and the case expects {value:?}",
                def.id
            );
        }
    }
    for (port, expected) in case.stats {
        let reference: PortRef = port.parse().expect("a port");
        let found = outcome.value(&reference).expect("a statistic");
        assert!(
            meets(*expected, found, document),
            "{} '{name}': {port} is {found:?}, and the case expects {expected:?}",
            def.id
        );
    }
    for (layer, expected) in case.deposits {
        let column = lanes.deposits(layer).expect("a layer that takes deposits");
        for (index, value) in expected.iter().enumerate() {
            let found = column.get(index).expect("a cell");
            assert!(
                meets(*value, found, document),
                "{} '{name}': deposits {layer}[{index}] are {found:?}, and the case expects {value:?}",
                def.id
            );
        }
    }
    for (counter, expected) in case.counters_after {
        assert_eq!(
            lanes.counter(counter),
            Some(*expected),
            "{} '{name}': counter {counter}",
            def.id
        );
    }
    if !case.colors.is_empty() {
        assert_eq!(
            outcome.colors.as_deref(),
            Some(case.colors),
            "{} '{name}': colours",
            def.id
        );
    }
}

/// Returns the world of a pass case: a grid, or agents in a world of the case's size.
pub(super) fn case_world(case: &PassCase) -> HostWorld {
    if case.agents > 0 {
        return HostWorld::Agents {
            count: case.agents,
            width: case.width as f32,
            height: case.height as f32,
        };
    }
    HostWorld::Grid {
        width: case.width,
        height: case.height,
        boundary: if case.bounded {
            Boundary::Bounded
        } else {
            Boundary::Torus
        },
    }
}

#[test]
fn every_known_answer_holds_on_the_host() {
    let (mut nodes, mut passes) = (0, 0);
    for def in definitions() {
        for case in def.cases {
            match &case.form {
                CaseForm::Node(node_case) => {
                    run_node_case(def, case.name, node_case);
                    nodes += 1;
                }
                CaseForm::Pass(pass_case) => {
                    run_pass_case(def, case.name, pass_case);
                    passes += 1;
                }
            }
        }
    }
    assert!(nodes > 150, "only {nodes} node cases");
    assert!(passes > 20, "only {passes} pass cases");
}
