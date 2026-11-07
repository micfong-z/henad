//! Every known-answer case of scope `Both`, node and pass, held to the generated WGSL on a device.
//!
//! A pass case runs the shader of its pass `case` over the lanes the case gives. A node case becomes a pass: each input
//! a socket takes per element comes from a lane of its own, read with `lane.get`, so the node's WGSL form computes it
//! on the device, and each output is written to a lane of its own. An input of a socket that takes no element value,
//! such as a threshold's probability, stays a literal, and the host computes the value the shader reads, as it does in
//! a model. A case of a view kind colours its output from the palette `ramp`. A case whose pass runs in the init stage
//! runs on the host alone, since a model's init passes never run on the device.
//!
//! A counter that a million agents add to, over thousands of workgroups, is held to the host's total too, and so is
//! the word of a million deposits into one cell of a field that adds up.

use henad_core::authoring::primitives::space::Boundary;

use super::case_files::{node_case_file, pass_case_file};
use super::known_answers::{input_literal, meets};
use super::support::{AgentShape, ShaderRun, TestDevice, run_shader, test_device};
use crate::GraphFile;
use crate::catalogue::{CaseForm, CaseScope, CaseValue, NodeCase, NodeDef, PassCase, definitions};
use crate::document::{Domain, GraphDocument, Lane, PassStage, World};
use crate::host::value::HostValue;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext, run};
use crate::ids::NodeId;
use crate::node::Node;
use crate::types::{Rate, ValueType};
use crate::value::{ConfigValue, InputValue, PortRef};
use crate::wgsl::agent_hooks::AgentGraphHooks;
use crate::wgsl::emit::FIRST_COUNTER;
use crate::wgsl::uniform::Dispatch;
use crate::wgsl::{Compiled, GeneratedShader, Probes, ShaderRole, compile_shaders};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Domain as EngineDomain, Geometry};
use henad_core::spatial_hash::SpatialHash;

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
    let compiled = compile_shaders(document, &report, &evaluator, None, &Probes::default())
        .unwrap_or_else(|problems| panic!("{what}: {problems:#?}"));
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
    // A reduce shader that sums floats writes one partial per lane per workgroup of 16 by 16 cells.
    let groups = (case.width.div_ceil(16), case.height.div_ceil(16));
    let sum_lanes = shader.grid_sum_lanes();
    let named = [(
        "partials",
        vec![0; (groups.0 * groups.1) as usize * sum_lanes.len().max(1)],
    )];
    let result = run_shader(
        device,
        shader,
        &ShaderRun {
            width: case.width,
            height: case.height,
            buffers: &words,
            labels: &labels,
            uniform: &uniform,
            agents: (!sum_lanes.is_empty()).then_some(AgentShape {
                groups,
                texture: (case.width, case.height),
                named: &named,
            }),
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
        let lane_total = |lane: usize| -> f64 {
            (0..(groups.0 * groups.1) as usize)
                .map(|block| f64::from(f32::from_bits(result.named[0][block * sum_lanes.len() + lane])))
                .sum()
        };
        let sums = (0..program.sums.len())
            .map(|sum| {
                let mine: Vec<usize> = sum_lanes
                    .iter()
                    .enumerate()
                    .filter(|(_, (known, number))| *known == position && *number == sum)
                    .map(|(lane, _)| lane)
                    .collect();
                let total = |k: usize| mine.get(k).map_or(0.0, |&lane| lane_total(lane));
                ([total(0), total(1)], mine.len() > 1)
            })
            .collect();
        let readback = run::Readback {
            tallies,
            sums,
            ..run::Readback::default()
        };
        let frame = run::finish(program, &params, &context, world, Some(&readback))
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

/// Returns the invocations of a pass over `domain` in a world of `agents` agents and `cells` cells.
fn invocations(domain: EngineDomain, agents: u32, cells: u32) -> u32 {
    match domain {
        EngineDomain::Agents => agents,
        EngineDomain::Cells(layers) => cells * layers,
        EngineDomain::AgentsOrCells => agents.max(cells),
    }
}

/// Returns the neighbour index of the positions `lanes` hold in its position lane, with the cell size the radius
/// folds of the pass at `position` read, as the engine rebuilds it before a step.
fn case_index(
    evaluator: &HostEvaluator,
    position: usize,
    lanes: &HostLanes,
    params: &[HostValue],
    world: HostWorld,
) -> Option<SpatialHash> {
    let Some(Ok(program)) = evaluator.program_at(position) else {
        return None;
    };
    let fold = program
        .element
        .iter()
        .find_map(|instruction| match &instruction.operation {
            crate::host::compile::Operation::RadiusFold(fold) => Some(fold),
            _ => None,
        })?;
    let frame = run::prelude(program, params, &RunContext::tick(None, 0), world).ok()?;
    let cell_size = frame.get(fold.cell_size)?.as_f32()?;
    let column = lanes.columns().get(fold.position_lane)?;
    let positions: Vec<[f32; 2]> = (0..column.len())
        .filter_map(|index| column.get(index)?.as_vec2f())
        .collect();
    let xs: Vec<f32> = positions.iter().map(|position| position[0]).collect();
    let ys: Vec<f32> = positions.iter().map(|position| position[1]).collect();
    let [width, height] = world.extent();
    let mut index = SpatialHash::new(cell_size, width.max(1.0), height.max(1.0));
    index.build(&xs, &ys);
    Some(index)
}

/// Runs a pass case of an agents world of `agents` agents, written out as `text`, on the device. Returns whether it ran,
/// `false` for a pass of the init stage.
#[expect(
    clippy::too_many_lines,
    reason = "the buffers, lanes, deposits, counters, statistics and colours of a case read in order"
)]
fn run_agent_pass_case(
    device: &TestDevice,
    def: &NodeDef,
    name: &str,
    case: &PassCase,
    (text, agents): (&str, u32),
) -> bool {
    let what = format!("{} '{name}'", def.id);
    let file = GraphFile::parse(text).expect("the case reads");
    let report = file.check();
    assert!(!report.has_errors(), "{what}: {:#?}", report.diagnostics);
    let stage = file
        .document()
        .passes
        .iter()
        .find(|pass| pass.id == "case")
        .expect("the case pass")
        .stage;
    if stage == PassStage::Init {
        return false;
    }
    let (evaluator, _, compiled) = crate::wgsl::agents::compile_parts(&file, &Probes::default())
        .unwrap_or_else(|problems| panic!("{what}: {problems:#?}"));
    let document = evaluator.document();
    let position = document
        .passes
        .iter()
        .position(|pass| pass.id == "case")
        .expect("the case pass");
    let shader = compiled
        .shaders
        .iter()
        .find(|shader| shader.passes().iter().any(|pass| pass == "case"))
        .unwrap_or_else(|| panic!("{what}: no shader runs the pass `case`"));
    let agent = shader.agent().expect("a shader of an agents world");
    let world = HostWorld::Agents {
        count: agents,
        width: case.width as f32,
        height: case.height as f32,
    };
    let (grid_width, grid_height, _) = world.domain_grid();
    let cells = grid_width * grid_height;
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    for (lane, values) in case.before {
        let column = lanes.column_mut(lane).expect("a lane or layer of the case");
        for (index, value) in values.iter().enumerate() {
            let value = HostValue::from_literal(&input_literal(*value), document).expect("a value");
            assert!(column.set(index, value), "{what}: {lane} takes {value:?}");
        }
    }
    let params = evaluator.param_values(&[]);
    let context = if stage == PassStage::Action {
        RunContext::action(None, 0, 1, 0)
    } else {
        RunContext::tick(None, 0)
    };
    let index = case_index(&evaluator, position, &lanes, &params, world);
    let geometry = Geometry {
        num_agents: agents,
        extent: Extent {
            w: case.width as f32,
            h: case.height as f32,
        },
        width: grid_width,
        height: grid_height,
        n_cells: cells,
        display: (grid_width, grid_height),
        index: None,
    };
    let words: Vec<Vec<u32>> = compiled
        .sources
        .iter()
        .map(|source| AgentGraphHooks::words(source, &lanes, &geometry))
        .collect();
    let labels: Vec<String> = compiled.buffers.iter().map(|buffer| buffer.label.to_string()).collect();
    let display = shader.role() == &ShaderRole::Display;
    let count = invocations(agent.domain, agents, cells);
    let groups = if display {
        (grid_width.div_ceil(16), grid_height.div_ceil(16))
    } else {
        (count.div_ceil(256).max(1), 1)
    };
    let mut counters = vec![0u32; compiled.plan.counter_words.max(1)];
    for (id, value) in case.counters_before {
        let position = document
            .counters
            .iter()
            .position(|counter| counter.id == *id)
            .expect("a counter of the case");
        counters[FIRST_COUNTER + position] = *value;
    }
    let lanes_of_leaf = agent.sum_lanes.len().max(1);
    let mut named: Vec<(&str, Vec<u32>)> = vec![
        ("counters", counters),
        ("partials", vec![0; groups.0 as usize * lanes_of_leaf]),
    ];
    if let Some(index) = &index {
        let (cell_start, sorted) = index.buckets();
        named.push(("cell_start", cell_start.to_vec()));
        named.push(("sorted", sorted.to_vec()));
    }
    let hash = index.as_ref().map(|index| {
        let (grid_w, grid_h) = index.grid_dims();
        let (cell_w, cell_h) = index.cell_extents();
        henad_core::spatial_hash::HashGrid {
            grid_w,
            grid_h,
            cell_w,
            cell_h,
        }
    });
    let uniform = shader
        .uniform()
        .bytes(
            &evaluator,
            world,
            &params,
            &context,
            &Dispatch {
                groups_x: groups.0,
                hash,
                texture: [grid_width, grid_height],
                counting: false,
            },
        )
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    let result = run_shader(
        device,
        shader,
        &ShaderRun {
            width: grid_width,
            height: grid_height,
            buffers: &words,
            labels: &labels,
            uniform: &uniform,
            agents: Some(AgentShape {
                groups,
                texture: (grid_width, grid_height),
                named: &named,
            }),
        },
    );
    let buffer_of = |label: &str| labels.iter().position(|known| known == label);
    for (id, expected) in case.after {
        let (column, found) = if let Some((field, layer)) = id.split_once('.') {
            let field_position = compiled
                .plan
                .fields
                .iter()
                .position(|plan| plan.id == field)
                .expect("a field of the case");
            let plan = &compiled.plan.fields[field_position];
            let layer = document.fields[field_position]
                .layers
                .iter()
                .position(|known| known == layer)
                .expect("a layer of the case");
            let buffer = buffer_of(&plan.label()).expect("the field's buffer");
            let start = layer * cells as usize;
            let words = result.buffers[buffer][start..start + cells as usize].to_vec();
            (lanes.column(id).expect("the layer"), words)
        } else {
            let lane = document
                .lanes
                .iter()
                .position(|known| known.id == *id)
                .expect("a lane of the case");
            (&lanes.columns()[lane], result.buffers[lane].clone())
        };
        let column = column.with_words(found);
        for (index, value) in expected.iter().enumerate() {
            let found = column.get(index).expect("an element");
            assert!(
                meets(*value, found, document),
                "{what}: {id}[{index}] is {found:?} on the device, and the case expects {value:?}\n{}",
                shader.text()
            );
        }
    }
    for (id, expected) in case.deposits {
        let (field, layer) = id.split_once('.').expect("a field layer");
        let field_position = compiled
            .plan
            .fields
            .iter()
            .position(|plan| plan.id == field)
            .expect("a field of the case");
        let plan = &compiled.plan.fields[field_position];
        let layer = document.fields[field_position]
            .layers
            .iter()
            .position(|known| known == layer)
            .expect("a layer of the case");
        let buffer = buffer_of(&plan.accum_label()).expect("the deposits' buffer");
        let start = layer * cells as usize;
        let column = lanes
            .deposits(id)
            .expect("the deposits")
            .with_words(result.buffers[buffer][start..start + cells as usize].to_vec());
        for (index, value) in expected.iter().enumerate() {
            let found = column.get(index).expect("a cell");
            assert!(
                meets(*value, found, document),
                "{what}: the deposit at {id}[{index}] is {found:?} on the device, and the case expects {value:?}"
            );
        }
    }
    let counters = &result.named[0];
    for (id, expected) in case.counters_after {
        let position = document
            .counters
            .iter()
            .position(|counter| counter.id == *id)
            .expect("a counter of the case");
        assert_eq!(
            counters[FIRST_COUNTER + position],
            *expected,
            "{what}: the counter {id}"
        );
    }
    if !case.stats.is_empty() {
        let (_, program) = evaluator.program("case").expect("the case pass");
        let mut first = 0;
        for pass in shader.passes() {
            if pass == "case" {
                break;
            }
            let (_, earlier) = evaluator.program(pass).expect("a stats pass");
            first += earlier.tallies;
        }
        let tally_base = FIRST_COUNTER + compiled.plan.counters;
        let partials = &result.named[1];
        let lane_total = |lane: usize| -> f64 {
            (0..groups.0 as usize)
                .map(|block| f64::from(f32::from_bits(partials[block * lanes_of_leaf + lane])))
                .sum()
        };
        let sums = (0..program.sums.len())
            .map(|sum| {
                let mine: Vec<usize> = agent
                    .sum_lanes
                    .iter()
                    .enumerate()
                    .filter(|(_, (known, number))| *known == position && *number == sum)
                    .map(|(lane, _)| lane)
                    .collect();
                let total = |k: usize| mine.get(k).map_or(0.0, |&lane| lane_total(lane));
                ([total(0), total(1)], mine.len() > 1)
            })
            .collect();
        let readback = run::Readback {
            tallies: (0..program.tallies)
                .map(|k| u64::from(counters[tally_base + first + k]))
                .collect(),
            sums,
            counters: (0..compiled.plan.counters)
                .map(|k| counters[FIRST_COUNTER + k])
                .collect(),
        };
        let frame = run::finish(program, &params, &context, world, Some(&readback))
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
        let found: Vec<Option<u32>> = if display {
            result
                .texels
                .iter()
                .map(|texel| color_index(document, &palette, *texel))
                .collect()
        } else {
            let buffer = buffer_of(crate::wgsl::emit::AGENT_COLORS).expect("the colour buffer");
            result.buffers[buffer]
                .iter()
                .map(|word| color_index(document, &palette, word.to_le_bytes()))
                .collect()
        };
        let expected: Vec<Option<u32>> = case.colors.iter().map(|&index| Some(index)).collect();
        assert_eq!(found, expected, "{what}: colours\n{}", shader.text());
    }
    true
}

/// Returns the text of a grid pass case that sums floats as a case of an agents world, one agent per cell and its
/// lanes and passes over agents, with the number of agents.
fn as_agents_world(case: &PassCase) -> (String, u32) {
    let agents = case.width * case.height;
    let mut text = super::case_files::agents_header(agents, case.width, case.height);
    text.push_str("\n[[lane]]\nid = \"case_position\"\ndomain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\n");
    text.push_str(&case.tables.replace("domain = \"cells\"", "domain = \"agents\""));
    (text, agents)
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
    let compiled = compile_shaders(
        document,
        &report,
        &evaluator,
        case.context.iteration,
        &Probes::default(),
    )
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
            agents: None,
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

/// Returns whether the stats passes of `case`, a pass case of a grid, sum floats.
fn sums_floats(case: &PassCase) -> bool {
    case.tables.contains("op = \"stat.sum\"") || case.tables.contains("op = \"stat.mean\"")
}

/// Every known answer of scope `Both` holds on the device as it does on the host, except a case in the init stage,
/// which only the host runs. A pass case of a grid whose stats pass sums floats runs on the grid, and again in an agents
/// world over the same cells. Every kind with a WGSL form outside init passes runs at least one of its own
/// cases on the device.
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
    let mut without_device_case = Vec::new();
    for def in definitions() {
        let ran_before = ran;
        for case in def.cases {
            if case.scope != CaseScope::Both {
                continue;
            }
            let device_ran = match &case.form {
                CaseForm::Node(node_case) => run_node_case(&device, def, case.name, node_case),
                CaseForm::Pass(pass_case) if pass_case.agents > 0 => run_agent_pass_case(
                    &device,
                    def,
                    case.name,
                    pass_case,
                    (&pass_case_file(pass_case), pass_case.agents),
                ),
                CaseForm::Pass(pass_case) if sums_floats(pass_case) => {
                    let (text, agents) = as_agents_world(pass_case);
                    let on_agents = run_agent_pass_case(&device, def, case.name, pass_case, (&text, agents));
                    run_pass_case(&device, def, case.name, pass_case) && on_agents
                }
                CaseForm::Pass(pass_case) => run_pass_case(&device, def, case.name, pass_case),
            };
            if device_ran {
                ran += 1;
            } else {
                on_host.push(format!("{} '{}'", def.id, case.name));
            }
        }
        let contexts = def.contexts;
        let runs_on_device = contexts.tick || contexts.action || contexts.view || contexts.stats;
        if def.backends.wgsl && runs_on_device && ran == ran_before {
            without_device_case.push(def.id);
        }
    }
    eprintln!("{ran} cases ran on the device, and these init cases on the host alone: {on_host:?}");
    assert!(ran > 300, "only {ran} cases ran on the device");
    assert!(
        without_device_case.is_empty(),
        "these kinds have a WGSL form outside init passes and no case that ran on the device: {without_device_case:?}"
    );
}

/// Number of agents of [`a_counter_over_many_workgroups_equals_the_host_total`], 3907 workgroups of 256.
const MANY_AGENTS: u32 = 1_000_000;

/// Returns the amount agent `index` adds in [`a_counter_over_many_workgroups_equals_the_host_total`]: 0 for an even
/// index, and for an odd one a hash large enough that the total wraps past 2^32 many times.
fn counted_amount(index: u32) -> u32 {
    if index.is_multiple_of(2) {
        0
    } else {
        index.wrapping_mul(0x9e37_79b9) | 1
    }
}

/// Runs `shader`, a step of `compiled`, over the words of `lanes` on `device`: [`MANY_AGENTS`] agents in a world of
/// 100 by 100 units, the engine's `counters` starting at 0.
fn run_many_agents_step(
    device: &TestDevice,
    evaluator: &HostEvaluator,
    compiled: &crate::wgsl::agents::CompiledAgents,
    shader: &GeneratedShader,
    lanes: &HostLanes,
) -> super::support::ShaderResult {
    let world = *lanes.world();
    let (grid_width, grid_height, _) = world.domain_grid();
    let geometry = Geometry {
        num_agents: MANY_AGENTS,
        extent: Extent { w: 100.0, h: 100.0 },
        width: grid_width,
        height: grid_height,
        n_cells: grid_width * grid_height,
        display: (grid_width, grid_height),
        index: None,
    };
    let words: Vec<Vec<u32>> = compiled
        .sources
        .iter()
        .map(|source| AgentGraphHooks::words(source, lanes, &geometry))
        .collect();
    let labels: Vec<String> = compiled.buffers.iter().map(|buffer| buffer.label.to_string()).collect();
    let groups = (MANY_AGENTS.div_ceil(256), 1);
    let params = evaluator.param_values(&[]);
    let uniform = shader
        .uniform()
        .bytes(
            evaluator,
            world,
            &params,
            &RunContext::tick(None, 0),
            &Dispatch {
                groups_x: groups.0,
                hash: None,
                texture: [grid_width, grid_height],
                counting: false,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let named: Vec<(&str, Vec<u32>)> = vec![("counters", vec![0u32; compiled.plan.counter_words.max(1)])];
    run_shader(
        device,
        shader,
        &ShaderRun {
            width: grid_width,
            height: grid_height,
            buffers: &words,
            labels: &labels,
            uniform: &uniform,
            agents: Some(AgentShape {
                groups,
                texture: (grid_width, grid_height),
                named: &named,
            }),
        },
    )
}

/// A counter that every agent of a million adds to, half of them 0, ends at the host's total after one step on the
/// device, each workgroup's tally flushed with one global atomic.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn a_counter_over_many_workgroups_equals_the_host_total() {
    let Some(device) = test_device() else {
        eprintln!("skipping a_counter_over_many_workgroups_equals_the_host_total: no adapter");
        return;
    };
    let mut text = super::case_files::agents_header(MANY_AGENTS, 100, 100);
    text.push_str(
        "\n[[lane]]\nid = \"case_position\"\ndomain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\n\
         [[lane]]\nid = \"amount\"\ndomain = \"agents\"\ntype = \"u32\"\n\n[[counter]]\nid = \"events\"\n\n\
         [[pass]]\nid = \"count\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n\n\
         [[pass.node]]\nid = \"n1\"\nop = \"lane.get\"\nconfig.lane = \"amount\"\n\n\
         [[pass.node]]\nid = \"n2\"\nop = \"counter.add\"\nconfig.counter = \"events\"\nin.amount = \"n1\"\n",
    );
    let file = GraphFile::parse(&text).expect("the file reads");
    let report = file.check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    let (evaluator, _, compiled) = crate::wgsl::agents::compile_parts(&file, &Probes::default())
        .unwrap_or_else(|problems| panic!("{problems:#?}"));
    let document = evaluator.document();
    let shader = compiled
        .shaders
        .iter()
        .find(|shader| shader.passes().iter().any(|pass| pass == "count"))
        .expect("the step shader");
    let world = HostWorld::Agents {
        count: MANY_AGENTS,
        width: 100.0,
        height: 100.0,
    };
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    let column = lanes.column_mut("amount").expect("the lane");
    for index in 0..MANY_AGENTS {
        assert!(column.set(index as usize, HostValue::U32(counted_amount(index))));
    }
    let result = run_many_agents_step(&device, &evaluator, &compiled, shader, &lanes);

    let mut stream = 0;
    evaluator
        .run_pass("count", &mut lanes, &[], &RunContext::tick(None, 0), &mut stream)
        .unwrap_or_else(|error| panic!("{error}"));
    let host_total = lanes.counter("events").expect("the counter");
    let written_total = (0..MANY_AGENTS).fold(0u32, |total, index| total.wrapping_add(counted_amount(index)));
    assert_eq!(host_total, written_total, "the host adds every amount, wrapping");

    assert_eq!(
        result.named[0][FIRST_COUNTER],
        host_total,
        "the counter on the device\n{}",
        shader.text()
    );
}

/// Returns the amount agent `index` deposits in [`many_deposits_into_one_cell_add_up_exactly_on_device_and_host`]:
/// a negative value or a NaN for one index in sixteen each, an odd multiple of 2^-11 for one in four, which lies
/// halfway between two multiples of the resolution 2^-10, and below 2^-6 from a hash for the rest.
fn deposited_amount(index: u32) -> f32 {
    let hash = index.wrapping_mul(0x9e37_79b9).rotate_left(13);
    match index % 16 {
        0 => -(hash as f32),
        1 => f32::NAN,
        2 | 6 | 10 | 14 => ((hash % 16) * 2 + 1) as f32 / 2048.0,
        _ => (hash >> 8) as f32 / 16_777_216.0 / 64.0,
    }
}

/// A million deposits of varied amounts into one cell of a field that adds up at the resolution 2^-10 leave the same
/// word on the device and on the host after one step: the amounts each rounded half to even to a count of the
/// resolution and added up, a negative amount and a NaN adding 0.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn many_deposits_into_one_cell_add_up_exactly_on_device_and_host() {
    let Some(device) = test_device() else {
        eprintln!("skipping many_deposits_into_one_cell_add_up_exactly_on_device_and_host: no adapter");
        return;
    };
    let mut text = super::case_files::agents_header(MANY_AGENTS, 100, 100);
    text.push_str(
        "\n[[lane]]\nid = \"case_position\"\ndomain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\n\
         [[lane]]\nid = \"amount\"\ndomain = \"agents\"\ntype = \"f32\"\n\n\
         [[field]]\nid = \"f\"\ntype = \"f32\"\nlayers = [\"value\"]\nstatic = false\ncombine = \"sum\"\n\
         resolution = 0.0009765625\n\n\
         [[pass]]\nid = \"lay\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n\n\
         [[pass.node]]\nid = \"n1\"\nop = \"lane.get\"\nconfig.lane = \"case_position\"\n\n\
         [[pass.node]]\nid = \"n2\"\nop = \"space.cell_of\"\nin.p = \"n1\"\n\n\
         [[pass.node]]\nid = \"n3\"\nop = \"lane.get\"\nconfig.lane = \"amount\"\n\n\
         [[pass.node]]\nid = \"n4\"\nop = \"field.deposit\"\nconfig.field = \"f\"\n\
         in.layer = { layer = \"f.value\" }\nin.at = \"n2.cell\"\nin.value = \"n3\"\n",
    );
    let file = GraphFile::parse(&text).expect("the file reads");
    let report = file.check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    let (evaluator, _, compiled) = crate::wgsl::agents::compile_parts(&file, &Probes::default())
        .unwrap_or_else(|problems| panic!("{problems:#?}"));
    let document = evaluator.document();
    let shader = compiled
        .shaders
        .iter()
        .find(|shader| shader.passes().iter().any(|pass| pass == "lay"))
        .expect("the step shader");
    let world = HostWorld::Agents {
        count: MANY_AGENTS,
        width: 100.0,
        height: 100.0,
    };
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    let column = lanes.column_mut("case_position").expect("the lane");
    for index in 0..MANY_AGENTS {
        assert!(column.set(index as usize, HostValue::Vec2F([0.5, 0.5])));
    }
    let column = lanes.column_mut("amount").expect("the lane");
    for index in 0..MANY_AGENTS {
        assert!(column.set(index as usize, HostValue::F32(deposited_amount(index))));
    }
    let result = run_many_agents_step(&device, &evaluator, &compiled, shader, &lanes);

    let mut stream = 0;
    evaluator
        .run_pass("lay", &mut lanes, &[], &RunContext::tick(None, 0), &mut stream)
        .unwrap_or_else(|error| panic!("{error}"));
    let host_word = match lanes.deposits("f.value").and_then(|column| column.get(0)) {
        Some(HostValue::U32(word)) => word,
        other => panic!("the host's deposit word is {other:?}"),
    };
    let written_word = (0..MANY_AGENTS).fold(0u32, |total, index| {
        let amount = deposited_amount(index);
        let count = if amount > 0.0 {
            (amount * 1024.0).round_ties_even() as u32
        } else {
            0
        };
        total.wrapping_add(count)
    });
    assert_eq!(host_word, written_word, "the host adds every count of the resolution");
    assert!(
        written_word > 1 << 20 && written_word < 1 << 24,
        "the total {written_word} merges exactly"
    );

    let accum = compiled
        .buffers
        .iter()
        .position(|buffer| buffer.label == compiled.plan.fields[0].accum_label())
        .expect("the deposits' buffer");
    assert_eq!(
        result.buffers[accum][0],
        host_word,
        "the deposit word on the device\n{}",
        shader.text()
    );
    assert!(
        result.buffers[accum][1..].iter().all(|&word| word == 0),
        "no other cell takes a deposit"
    );
}
