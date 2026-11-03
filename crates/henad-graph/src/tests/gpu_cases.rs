//! Every known-answer case of scope `Both`, node and pass, held to the generated WGSL on a device.
//!
//! A pass case runs the shader of its pass `case` over the lanes the case gives. A node case becomes a pass: each input
//! a socket takes per element comes from a lane of its own, read with `lane.get`, so the node's WGSL form computes it
//! on the device, and each output is written to a lane of its own. An input of a socket that takes no element value,
//! such as a threshold's probability, stays a literal, and the host computes the value the shader reads, as it does in
//! a model. A case of a view kind colours its output from the palette `ramp`. A case whose pass runs in the init stage
//! runs on the host alone, since a model's init passes never run on the device.

use henad_core::authoring::primitives::space::Boundary;

use super::case_files::{node_case_file, pass_case_file};
use super::known_answers::{input_literal, meets};
use super::support::{ShaderRun, TestDevice, run_shader, test_device};
use crate::GraphFile;
use crate::catalogue::{CaseForm, CaseScope, CaseValue, NodeCase, NodeDef, PassCase, definitions};
use crate::document::{Domain, GraphDocument, Lane, PassStage, World};
use crate::host::value::HostValue;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext, run};
use crate::ids::NodeId;
use crate::node::Node;
use crate::types::{Rate, ValueType};
use crate::value::{ConfigValue, InputValue, PortRef};
use crate::wgsl::{Compiled, GeneratedShader, compile_shaders};

/// Returns the id `n<number>`.
fn id(number: u32) -> NodeId {
    NodeId::new(number).expect("a node id")
}

/// Returns the buffers and labels of `lanes`, then the tick buffer holding `tick` when `compiled` has one.
fn buffers(compiled: &Compiled, lanes: &HostLanes, tick: u32) -> (Vec<Vec<u32>>, Vec<String>) {
    let mut words: Vec<Vec<u32>> = lanes.columns().iter().map(|column| column.words().to_vec()).collect();
    let mut labels: Vec<String> = lanes
        .columns()
        .iter()
        .map(|column| format!("lane_{}_cells", column.id()))
        .collect();
    if compiled.tick_buffer {
        words.push(vec![tick]);
        labels.push("henad_tick".to_owned());
    }
    (words, labels)
}

/// Returns the shader that runs the pass `case`.
fn case_shader<'a>(compiled: &'a Compiled, what: &str) -> &'a GeneratedShader {
    compiled
        .shaders
        .iter()
        .find(|shader| shader.passes().iter().any(|pass| pass == "case"))
        .unwrap_or_else(|| panic!("{what}: no shader runs the pass `case`"))
}

/// Returns the palette index of `texel` in the palette `id` of `document`.
pub(super) fn color_index(document: &GraphDocument, id: &str, texel: [u8; 4]) -> Option<u32> {
    let colors: Vec<[u8; 4]> = document
        .palettes
        .iter()
        .find(|palette| palette.id == id)
        .map(|palette| palette.colors.clone())
        .or_else(|| {
            document
                .categories
                .iter()
                .find(|category| category.id == id)
                .map(|category| category.variants.iter().map(|variant| variant.color).collect())
        })?;
    u32::try_from(colors.iter().position(|color| *color == texel)?).ok()
}

/// Returns the palette the `view.color` node of the pass `case` names.
fn case_palette(document: &GraphDocument) -> Option<String> {
    let pass = document.passes.iter().find(|pass| pass.id == "case")?;
    pass.body
        .nodes
        .iter()
        .find(|node| node.op == "view.color")
        .and_then(|node| node.config.get("palette"))
        .and_then(ConfigValue::as_str)
        .map(str::to_owned)
}

/// Returns the value at `port` in the frame of `program` that `run::finish` left.
fn port_value(program: &crate::host::compile::Program, frame: &[HostValue], port: &PortRef) -> Option<HostValue> {
    let slot = if let Some(name) = &port.port {
        program.outputs.get(&(port.node, name.clone())).copied()
    } else {
        let mut slots = program
            .outputs
            .iter()
            .filter(|((node, _), _)| *node == port.node)
            .map(|(_, slot)| *slot);
        let only = slots.next();
        if slots.next().is_some() { None } else { only }
    }?;
    frame.get(slot).copied()
}

/// Runs a pass case on the device. Returns whether it ran, `false` for a pass of the init stage.
#[expect(
    clippy::too_many_lines,
    reason = "the lanes, statistics and colours of a case read in order"
)]
fn run_pass_case(device: &TestDevice, def: &NodeDef, name: &str, case: &PassCase) -> bool {
    let what = format!("{} '{name}'", def.id);
    let file = GraphFile::parse(&pass_case_file(case)).expect("the case reads");
    let report = file.check();
    let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{what}: {error}"));
    let document = evaluator.document();
    let position = document
        .passes
        .iter()
        .position(|pass| pass.id == "case")
        .expect("the case pass");
    let stage = document.passes[position].stage;
    if stage == PassStage::Init {
        return false;
    }
    let compiled =
        compile_shaders(document, &report, &evaluator, None).unwrap_or_else(|problems| panic!("{what}: {problems:#?}"));
    let shader = case_shader(&compiled, &what);
    let world = HostWorld::Grid {
        width: case.width,
        height: case.height,
        boundary: if case.bounded {
            Boundary::Bounded
        } else {
            Boundary::Torus
        },
    };
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    for (lane, values) in case.before {
        let column = lanes.column_mut(lane).expect("a lane of the case");
        for (index, value) in values.iter().enumerate() {
            let value = HostValue::from_literal(&input_literal(*value), document).expect("a value");
            assert!(column.set(index, value), "{what}: lane {lane} takes {value:?}");
        }
    }
    let context = if stage == PassStage::Action {
        RunContext::action(None, 0, 1, 0)
    } else {
        RunContext::tick(None, 0)
    };
    let params = evaluator.param_values(&[]);
    let uniform = shader
        .uniform_bytes(&evaluator, world, &params, &context)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    let (words, labels) = buffers(&compiled, &lanes, 0);
    let result = run_shader(
        device,
        shader,
        &ShaderRun {
            width: case.width,
            height: case.height,
            buffers: &words,
            labels: &labels,
            uniform: &uniform,
        },
    );
    for (lane, expected) in case.after {
        let lane_position = document
            .lanes
            .iter()
            .position(|known| known.id == *lane)
            .expect("a lane of the case");
        let column = lanes.columns()[lane_position].with_words(result.buffers[lane_position].clone());
        for (index, value) in expected.iter().enumerate() {
            let found = column.get(index).expect("an element");
            assert!(
                meets(*value, found, document),
                "{what}: {lane}[{index}] is {found:?} on the device, and the case expects {value:?}\n{}",
                shader.text()
            );
        }
    }
    if !case.stats.is_empty() {
        let mut first = 0;
        for pass in shader.passes() {
            if pass == "case" {
                break;
            }
            let (_, program) = evaluator.program(pass).expect("a stats pass");
            first += program.tallies;
        }
        let (_, program) = evaluator.program("case").expect("the case pass");
        let tallies: Vec<u64> = result.counters[first..first + program.tallies]
            .iter()
            .map(|&count| u64::from(count))
            .collect();
        let frame = run::finish(program, &params, &context, world, Some(&tallies))
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        for (port, expected) in case.stats {
            let reference: PortRef = port.parse().expect("a port");
            let found = port_value(program, &frame, &reference).expect("a statistic");
            assert!(
                meets(*expected, found, document),
                "{what}: {port} is {found:?} on the device, and the case expects {expected:?}"
            );
        }
    }
    if !case.colors.is_empty() {
        let palette = case_palette(document).expect("the case colours from a palette");
        let found: Vec<Option<u32>> = result
            .texels
            .iter()
            .map(|texel| color_index(document, &palette, *texel))
            .collect();
        let expected: Vec<Option<u32>> = case.colors.iter().map(|&index| Some(index)).collect();
        assert_eq!(found, expected, "{what}: colours\n{}", shader.text());
    }
    true
}

/// Returns the document of a node case as a pass: each input a socket takes per element read from a lane, and each
/// output written to a lane, or coloured from the palette `ramp` in a view pass. The node is `n1`, the input lanes are
/// `in_<k>` read by `n<200 + k>`, and the output lanes `out_<k>` written by `n<300 + k>`.
fn node_case_document(
    def: &NodeDef,
    case: &NodeCase,
    report_of: &dyn Fn(&GraphDocument) -> crate::check::CheckReport,
) -> GraphDocument {
    let file = GraphFile::parse(&node_case_file(def, case)).expect("the case reads");
    let mut document = file.document().clone();
    let position = document
        .passes
        .iter()
        .position(|pass| pass.id == "case")
        .expect("the case pass");
    let stage = document.passes[position].stage;
    let node = document.passes[position].body.nodes[0].clone();
    let inputs = def.resolved_inputs(&node, &document);
    let mut added_lanes = Vec::new();
    let mut added_nodes = Vec::new();
    let mut n1 = node.clone();
    for (k, (port, value)) in case.inputs.iter().enumerate() {
        let per_element = inputs
            .iter()
            .find(|input| input.name == *port)
            .is_some_and(|input| input.max_rate >= Rate::Element);
        let value_type = value.value_type();
        let on_device = !value_type.is_host_only();
        if per_element && on_device {
            let lane = format!("in_{k}");
            added_lanes.push(Lane::new(&lane, Domain::Cells, value_type));
            let mut get = Node::new(id(200 + k as u32), "lane.get");
            get.config.insert("lane".to_owned(), ConfigValue::String(lane));
            added_nodes.push(get);
            n1.inputs.insert(
                (*port).to_owned(),
                InputValue::Link(PortRef {
                    node: id(200 + k as u32),
                    port: None,
                }),
            );
        } else {
            n1.inputs
                .insert((*port).to_owned(), InputValue::Literal(input_literal(*value)));
        }
    }
    // The output types come from a check of the document with the inputs in place.
    let mut typed = document.clone();
    typed.lanes.extend(added_lanes.iter().cloned());
    typed.passes[position].body.nodes = std::iter::once(n1.clone()).chain(added_nodes.iter().cloned()).collect();
    let report = report_of(&typed);
    let outputs = report.passes[position].outputs.get(&id(1)).cloned().unwrap_or_default();
    for (k, (port, _)) in case.outputs.iter().enumerate() {
        let link = InputValue::Link(PortRef {
            node: id(1),
            port: Some((*port).to_owned()),
        });
        if stage == PassStage::View {
            let mut color = Node::new(id(300 + k as u32), "view.color");
            color
                .config
                .insert("palette".to_owned(), ConfigValue::String("ramp".to_owned()));
            color.inputs.insert("index".to_owned(), link);
            added_nodes.push(color);
            continue;
        }
        let value_type = outputs
            .get(*port)
            .and_then(|port_type| port_type.value_type.clone())
            .unwrap_or(ValueType::U32);
        let lane = format!("out_{k}");
        added_lanes.push(Lane::new(&lane, Domain::Cells, value_type));
        let mut set = Node::new(id(300 + k as u32), "lane.set");
        set.config.insert("lane".to_owned(), ConfigValue::String(lane));
        set.inputs.insert("value".to_owned(), link);
        added_nodes.push(set);
    }
    document.lanes.extend(added_lanes);
    document.passes[position].body.nodes = std::iter::once(n1).chain(added_nodes).collect();
    document
}

/// Returns the check report of `document`, through its text with every non-finite literal written as 0.
fn report_of(document: &GraphDocument) -> crate::check::CheckReport {
    let mut finite = document.clone();
    for pass in &mut finite.passes {
        for node in &mut pass.body.nodes {
            for value in node.inputs.values_mut() {
                if let InputValue::Literal(crate::value::Literal::F32(number)) = value
                    && !number.is_finite()
                {
                    *number = 0.0;
                }
            }
        }
    }
    GraphFile::from_document(&finite, None)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .check()
}

/// Runs a node case on the device. Returns whether it ran, `false` for a case in the init stage.
fn run_node_case(device: &TestDevice, def: &NodeDef, name: &str, case: &NodeCase) -> bool {
    let what = format!("{} '{name}'", def.id);
    let document = node_case_document(def, case, &report_of);
    let position = document
        .passes
        .iter()
        .position(|pass| pass.id == "case")
        .expect("the case pass");
    let stage = document.passes[position].stage;
    if stage == PassStage::Init {
        return false;
    }
    let report = report_of(&document);
    assert!(!report.has_errors(), "{what}: {:#?}", report.diagnostics);
    let evaluator = HostEvaluator::from_document(&document).unwrap_or_else(|error| panic!("{what}: {error}"));
    let document = evaluator.document();
    let compiled = compile_shaders(document, &report, &evaluator, case.context.iteration)
        .unwrap_or_else(|problems| panic!("{what}: {problems:#?}"));
    let shader = case_shader(&compiled, &what);
    let context = &case.context;
    let width = context.element + 1;
    let World::Grid(grid) = document.world else {
        unreachable!("a case file is a grid");
    };
    let world = HostWorld::Grid {
        width,
        height: 1,
        boundary: grid.boundary,
    };
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    for (k, (_, value)) in case.inputs.iter().enumerate() {
        let Some(column) = lanes.column_mut(&format!("in_{k}")) else {
            continue;
        };
        let value = match value {
            CaseValue::Nan => HostValue::F32(f32::NAN),
            other => HostValue::from_literal(&input_literal(*other), document).expect("a value"),
        };
        for element in 0..width as usize {
            assert!(column.set(element, value), "{what}: input {k} takes {value:?}");
        }
    }
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
    let params = evaluator.param_values(&[]);
    let uniform = shader
        .uniform_bytes(&evaluator, world, &params, &run_context)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    let (words, labels) = buffers(&compiled, &lanes, context.tick);
    let result = run_shader(
        device,
        shader,
        &ShaderRun {
            width,
            height: 1,
            buffers: &words,
            labels: &labels,
            uniform: &uniform,
        },
    );
    let element = context.element as usize;
    for (k, (port, expected)) in case.outputs.iter().enumerate() {
        let found = if stage == PassStage::View {
            color_index(document, "ramp", result.texels[element]).map(HostValue::U32)
        } else {
            let lane = document
                .lanes
                .iter()
                .position(|known| known.id == format!("out_{k}"))
                .expect("an output lane");
            lanes.columns()[lane]
                .with_words(result.buffers[lane].clone())
                .get(element)
        };
        let found = found.unwrap_or_else(|| panic!("{what}: no value of `{port}`"));
        assert!(
            meets(*expected, found, document),
            "{what}: `{port}` is {found:?} on the device, and the case expects {expected:?}\n{}",
            shader.text()
        );
    }
    true
}

/// Kinds of milestone A2 whose WGSL forms the compiler does not write yet. Their cases of scope `Both` hold on the host
/// today, and join this test as each form lands.
const WITHOUT_A_WGSL_FORM: [&str; 50] = [
    "ctx.world",
    "ctx.grid",
    "ctx.cell_xy",
    "lane.get_at",
    "field.get",
    "field.set",
    "field.deposit",
    "counter.add",
    "math.neg",
    "math.clamp",
    "math.sqrt",
    "math.recip",
    "math.log2",
    "math.log10",
    "math.floor",
    "math.midpoint",
    "math.expr",
    "conv.to_f64",
    "conv.to_vec2f",
    "conv.to_vec2i",
    "conv.to_vec2u",
    "vec.make",
    "vec.split",
    "vec.scale",
    "vec.dot",
    "vec.length",
    "vec.from_angle",
    "space.wrap",
    "space.cell_index",
    "space.cell_of",
    "space.heading_octant",
    "space.step_code",
    "space.step_offset",
    "space.delta",
    "space.dist_sq",
    "space.cell_xy",
    "space.offset",
    "rand.bits",
    "rand.choice3",
    "rng.reservoir",
    "rng.choice3",
    "stream.float",
    "grid.fold",
    "agent.radius_fold",
    "stat.sum",
    "stat.mean",
    "stat.counter",
    "fn.input",
    "fn.output",
    "fn.call",
];

/// Every known answer of scope `Both` holds on the device as it does on the host, except a case in the init stage,
/// which only the host runs, and the cases of [`WITHOUT_A_WGSL_FORM`].
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip and the cases left to the host are information for a reader of the test output"
)]
fn every_known_answer_of_both_scopes_holds_on_the_device() {
    let Some(device) = test_device() else {
        eprintln!("skipping every_known_answer_of_both_scopes_holds_on_the_device: no adapter");
        return;
    };
    let (mut ran, mut on_host) = (0, Vec::new());
    let mut awaiting = 0;
    for def in definitions() {
        if WITHOUT_A_WGSL_FORM.contains(&def.id) {
            awaiting += def.cases.iter().filter(|case| case.scope == CaseScope::Both).count();
            continue;
        }
        for case in def.cases {
            if case.scope != CaseScope::Both {
                continue;
            }
            let device_ran = match &case.form {
                CaseForm::Node(node_case) => run_node_case(&device, def, case.name, node_case),
                CaseForm::Pass(pass_case) => run_pass_case(&device, def, case.name, pass_case),
            };
            if device_ran {
                ran += 1;
            } else {
                on_host.push(format!("{} '{}'", def.id, case.name));
            }
        }
    }
    eprintln!(
        "{ran} cases ran on the device, these init cases on the host alone: {on_host:?}, and {awaiting} cases of kinds without a WGSL form on the host alone"
    );
    assert!(ran > 150, "only {ran} cases ran on the device");
}
