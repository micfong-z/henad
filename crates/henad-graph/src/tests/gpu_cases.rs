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
//! the word of a million deposits into one cell of a field that adds up. So are an integer division and remainder by
//! a constant 0, a fold that carries a cell, and a difference on a torus between points several worlds apart.

use super::case_files::{agents_header_with_position, node_case_file, pass_case_file};
use super::gpu_repeat::{on_device, on_host};
use super::known_answers::{case_world, input_literal, meets, node_case_context, pass_case_context, plant};
use super::support::{
    AgentShape, ShaderRun, TestDevice, device_for, first_tally, lane_buffers, lane_words, node_id, run_shader,
    shader_of_pass,
};
use crate::GraphFile;
use crate::catalogue::{CaseForm, CaseScope, CaseValue, NodeCase, NodeDef, PassCase, definitions};
use crate::document::{Domain, GraphDocument, Lane, PassStage, World};
use crate::host::program::Program;
use crate::host::value::HostValue;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext, run};
use crate::node::Node;
use crate::types::{Rate, ValueType};
use crate::value::{ConfigValue, InputValue, PortRef};
use crate::wgsl::agent_hooks::AgentGraphHooks;
use crate::wgsl::agents::CompiledAgents;
use crate::wgsl::emit::{FieldPlan, SumLane, counter_total, counter_word, counters_end};
use crate::wgsl::uniform::Dispatch;
use crate::wgsl::{GeneratedShader, Probes, ShaderRole, compile_shaders};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Domain as EngineDomain, Geometry};
use henad_core::spatial_hash::SpatialHash;

/// Returns the shader among `shaders` that runs the pass `case` of the case `what`.
fn case_shader<'a>(shaders: &'a [GeneratedShader], what: &str) -> &'a GeneratedShader {
    shader_of_pass(shaders, "case").unwrap_or_else(|| panic!("{what}: no shader runs the pass `case`"))
}

/// Returns the sum of each of the `sums` sums of the stats pass at `position`, with whether it takes two lanes, from
/// the sum lanes `sum_lanes` of its shader and the total `lane_total` returns for each lane.
fn sum_totals(
    sums: usize,
    sum_lanes: &[SumLane],
    position: usize,
    lane_total: impl Fn(usize) -> f64,
) -> Vec<([f64; 2], bool)> {
    (0..sums)
        .map(|sum| {
            let mine: Vec<usize> = sum_lanes
                .iter()
                .enumerate()
                .filter(|(_, lane)| lane.pass == position && lane.sum == sum)
                .map(|(lane, _)| lane)
                .collect();
            let total = |k: usize| mine.get(k).map_or(0.0, |&lane| lane_total(lane));
            ([total(0), total(1)], mine.len() > 1)
        })
        .collect()
}

/// Checks each statistic of `stats` of the case `what` against the value at its port in `frame`, the frame of
/// `program` that `run::finish` left.
fn assert_stats(
    what: &str,
    stats: &[(&str, CaseValue)],
    program: &Program,
    frame: &[HostValue],
    document: &GraphDocument,
) {
    for (port, expected) in stats {
        let reference: PortRef = port.parse().expect("a port");
        let found = port_value(program, frame, &reference).expect("a statistic");
        assert!(
            meets(*expected, found, document),
            "{what}: {port} is {found:?} on the device, and the case expects {expected:?}"
        );
    }
}

/// Returns the plan of the field `field` of `compiled` and the position of its layer `layer`.
fn field_layer<'a>(
    compiled: &'a CompiledAgents,
    document: &GraphDocument,
    field: &str,
    layer: &str,
) -> (&'a FieldPlan, usize) {
    let position = compiled
        .plan
        .fields
        .iter()
        .position(|plan| plan.id == field)
        .expect("a field of the case");
    let layer = document.fields[position]
        .layers
        .iter()
        .position(|known| known == layer)
        .expect("a layer of the case");
    (&compiled.plan.fields[position], layer)
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
fn port_value(program: &Program, frame: &[HostValue], port: &PortRef) -> Option<HostValue> {
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
    let shader = case_shader(&compiled.shaders, &what);
    let world = case_world(case);
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    plant(&mut lanes, case.before, document, &what);
    let context = pass_case_context(stage);
    let params = evaluator.param_values(&[]);
    let uniform = shader
        .uniform_bytes(&evaluator, world, &params, &context)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    let (words, labels) = lane_buffers(&compiled, &lanes, 0);
    // A reduce shader that sums floats writes one partial per lane per workgroup of 16 by 16 cells.
    let groups = (case.width.div_ceil(16), case.height.div_ceil(16));
    let sum_lanes = shader.sum_lanes();
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
        let column = lanes.columns()[lane_position].with_words(lane_words(&compiled, &result.buffers, lane_position));
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
        let first = first_tally(&evaluator, shader, "case");
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
        let readback = run::Readback {
            tallies,
            sums: sum_totals(program.sums.len(), sum_lanes, position, lane_total),
            ..run::Readback::default()
        };
        let frame = run::finish(program, &params, &context, world, Some(&readback))
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_stats(&what, case.stats, program, &frame, document);
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
            crate::host::program::Operation::RadiusFold(fold) => Some(fold),
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
    let shader = case_shader(&compiled.shaders, &what);
    let domain = shader.domain().expect("a shader of an agents world");
    let world = HostWorld::Agents {
        count: agents,
        width: case.width as f32,
        height: case.height as f32,
    };
    let (grid_width, grid_height, _) = world.domain_grid();
    let cells = grid_width * grid_height;
    let mut lanes = HostLanes::new(document, world).expect("the lanes");
    plant(&mut lanes, case.before, document, &what);
    let params = evaluator.param_values(&[]);
    let context = pass_case_context(stage);
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
    let count = invocations(domain, agents, cells);
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
        // The low word, then the high word.
        counters[counter_word(position)] = *value as u32;
        counters[counter_word(position) + 1] = (*value >> 32) as u32;
    }
    let lanes_of_leaf = shader.sum_lanes().len().max(1);
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
            let (plan, layer) = field_layer(&compiled, document, field, layer);
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
            let place = compiled.layout.place(lane).expect("a lane on the device");
            (
                &lanes.columns()[lane],
                place.segment(&result.buffers[place.buffer()]).to_vec(),
            )
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
        let (plan, layer) = field_layer(&compiled, document, field, layer);
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
        assert_eq!(counter_total(counters, position), *expected, "{what}: the counter {id}");
    }
    if !case.stats.is_empty() {
        let (_, program) = evaluator.program("case").expect("the case pass");
        let first = first_tally(&evaluator, shader, "case");
        let tally_base = counters_end(compiled.plan.counters);
        let partials = &result.named[1];
        let lane_total = |lane: usize| -> f64 {
            (0..groups.0 as usize)
                .map(|block| f64::from(f32::from_bits(partials[block * lanes_of_leaf + lane])))
                .sum()
        };
        let readback = run::Readback {
            tallies: (0..program.tallies)
                .map(|k| u64::from(counters[tally_base + first + k]))
                .collect(),
            sums: sum_totals(program.sums.len(), shader.sum_lanes(), position, lane_total),
            counters: (0..compiled.plan.counters)
                .map(|k| counter_total(counters, k))
                .collect(),
        };
        let frame = run::finish(program, &params, &context, world, Some(&readback))
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_stats(&what, case.stats, program, &frame, document);
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
    let mut text = agents_header_with_position(agents, case.width, case.height);
    text.push_str(&case.tables.replace("domain = \"cells\"", "domain = \"agents\""));
    (text, agents)
}

/// Returns the document of a node case as a pass: each input a socket takes per element read from a lane, and each
/// output written to a lane, or coloured from the palette `ramp` in a view pass. The node is `n1`, the input lanes are
/// `in_<k>` read by `n<200 + k>`, and the output lanes `out_<k>` written by `n<300 + k>`.
fn node_case_document(def: &NodeDef, case: &NodeCase) -> GraphDocument {
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
            let mut get = Node::new(node_id(200 + k as u32), "lane.get");
            get.config.insert("lane".to_owned(), ConfigValue::String(lane));
            added_nodes.push(get);
            n1.inputs.insert(
                (*port).to_owned(),
                InputValue::Link(PortRef {
                    node: node_id(200 + k as u32),
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
    let outputs = report.passes[position]
        .outputs
        .get(&node_id(1))
        .cloned()
        .unwrap_or_default();
    for (k, (port, _)) in case.outputs.iter().enumerate() {
        let link = InputValue::Link(PortRef {
            node: node_id(1),
            port: Some((*port).to_owned()),
        });
        if stage == PassStage::View {
            let mut color = Node::new(node_id(300 + k as u32), "view.color");
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
        let mut set = Node::new(node_id(300 + k as u32), "lane.set");
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
    let document = node_case_document(def, case);
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
    let shader = case_shader(&compiled.shaders, &what);
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
    let run_context = node_case_context(context, stage);
    let params = evaluator.param_values(&[]);
    let uniform = shader
        .uniform_bytes(&evaluator, world, &params, &run_context)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    let (words, labels) = lane_buffers(&compiled, &lanes, context.tick);
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
                .with_words(lane_words(&compiled, &result.buffers, lane))
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
    reason = "the cases left to the host are information for a reader of the test output"
)]
fn every_known_answer_of_both_scopes_holds_on_the_device() {
    let Some(device) = device_for("every_known_answer_of_both_scopes_holds_on_the_device") else {
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
/// device, each workgroup's tally flushed with one global atomic on each word.
///
/// The amounts are hashed words. A workgroup's tally carries past 2^32, and the total does too, near 2^50.
#[test]
fn a_counter_over_many_workgroups_equals_the_host_total() {
    let Some(device) = device_for("a_counter_over_many_workgroups_equals_the_host_total") else {
        return;
    };
    let mut text = agents_header_with_position(MANY_AGENTS, 100, 100);
    text.push_str(
        "[[lane]]\nid = \"amount\"\ndomain = \"agents\"\ntype = \"u32\"\n\n[[counter]]\nid = \"events\"\n\n\
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
    let shader = shader_of_pass(&compiled.shaders, "count").expect("the step shader");
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
    let written_total: u64 = (0..MANY_AGENTS).map(|index| u64::from(counted_amount(index))).sum();
    assert_eq!(host_total, written_total, "the host adds every amount");
    assert!(
        written_total > 1 << 48,
        "the total is {written_total}, and should pass 2^32 by far"
    );

    assert_eq!(
        counter_total(&result.named[0], 0),
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
fn many_deposits_into_one_cell_add_up_exactly_on_device_and_host() {
    let Some(device) = device_for("many_deposits_into_one_cell_add_up_exactly_on_device_and_host") else {
        return;
    };
    let mut text = agents_header_with_position(MANY_AGENTS, 100, 100);
    text.push_str(
        "[[lane]]\nid = \"amount\"\ndomain = \"agents\"\ntype = \"f32\"\n\n\
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
    let shader = shader_of_pass(&compiled.shaders, "lay").expect("the step shader");
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

/// Returns a pass node `id` of the kind `op`, with the inputs `inputs` as TOML lines.
fn pass_node(id: &str, op: &str, inputs: &str) -> String {
    format!("\n[[pass.node]]\nid = \"{id}\"\nop = \"{op}\"\n{inputs}")
}

/// Returns the nodes of a tick pass that writes to the lane `x` the `op`, `math.div` or `math.rem`, of a value of
/// `value_type` made from the cell's index by the constant `divisor`, folded to one `u32`.
///
/// A signed value runs negative below the 30th cell, and the two components of a vector land in the two halves of the
/// word.
fn division_by_constant(op: &str, value_type: &str, divisor: &str) -> String {
    let mut nodes = pass_node("n1", "ctx.index", "");
    nodes += &pass_node("n2", "conv.to_i32", "in.a = \"n1\"\n");
    nodes += &pass_node("n3", "math.sub", "in.a = \"n2\"\nin.b = { i32 = 30 }\n");
    let dividend = match value_type {
        "u32" => "n1",
        "i32" => "n3",
        "vec2u" => {
            nodes += &pass_node("n4", "vec.make", "in.x = \"n1\"\nin.y = \"n1\"\n");
            "n4"
        }
        _ => {
            nodes += &pass_node("n4", "vec.make", "in.x = \"n3\"\nin.y = \"n2\"\n");
            "n4"
        }
    };
    nodes += &pass_node(
        "n10",
        op,
        &format!("in.a = \"{dividend}\"\nin.b = {{ {value_type} = {divisor} }}\n"),
    );
    let written = match value_type {
        "u32" => "n10",
        "i32" => {
            nodes += &pass_node("n11", "conv.to_u32", "in.a = \"n10\"\n");
            "n11"
        }
        _ => {
            let unsigned = if value_type == "vec2u" {
                "n10"
            } else {
                nodes += &pass_node("n11", "conv.to_vec2u", "in.a = \"n10\"\n");
                "n11"
            };
            nodes += &pass_node("n12", "vec.split", &format!("in.v = \"{unsigned}\"\n"));
            nodes += &pass_node("n13", "math.mul", "in.a = \"n12.x\"\nin.b = { u32 = 65536 }\n");
            nodes += &pass_node("n14", "math.add", "in.a = \"n13\"\nin.b = \"n12.y\"\n");
            "n14"
        }
    };
    nodes += &pass_node(
        "n20",
        "lane.set",
        &format!("config.lane = \"x\"\nin.value = \"{written}\"\n"),
    );
    nodes
}

/// An integer division by a constant with a component of 0, and an integer remainder by a constant 0, build and give
/// the documented results on the device as on the host: the dividend's component for a quotient and 0 for a remainder.
#[test]
fn an_integer_division_by_a_constant_zero_runs_as_the_host_does() {
    let Some(device) = device_for("an_integer_division_by_a_constant_zero_runs_as_the_host_does") else {
        return;
    };
    for (op, value_type, divisor) in [
        ("math.div", "u32", "0"),
        ("math.rem", "u32", "0"),
        ("math.div", "i32", "0"),
        ("math.rem", "i32", "0"),
        ("math.div", "vec2u", "[3, 0]"),
        ("math.div", "vec2i", "[0, -2]"),
        ("math.div", "vec2u", "[0, 0]"),
    ] {
        let nodes = division_by_constant(op, value_type, divisor);
        let host = on_host(&nodes, None, 0);
        assert_eq!(
            on_device(&device, &nodes, None, 0)[0],
            host[0],
            "{op} of a {value_type} by {divisor}"
        );
        if (op, value_type) == ("math.div", "u32") {
            assert_eq!(
                host[0],
                (0..64).collect::<Vec<u32>>(),
                "a quotient by 0 is the dividend"
            );
        }
        if (op, value_type) == ("math.rem", "u32") {
            assert!(host[0].iter().all(|word| *word == 0), "a remainder by 0 is 0");
        }
    }
}

/// A grid fold that carries a cell, from the cell itself to each position it visits, ends at the last position of its
/// table on the device as on the host.
#[test]
fn a_fold_carries_a_cell_on_the_device_as_on_the_host() {
    let Some(device) = device_for("a_fold_carries_a_cell_on_the_device_as_on_the_host") else {
        return;
    };
    let mut nodes = pass_node("n1", "ctx.cell_xy", "");
    nodes += &pass_node("n2", "space.cell_index", "in.xy = \"n1.xy\"\n");
    nodes += &pass_node(
        "n3",
        "grid.fold",
        "config.order = \"von_neumann\"\nconfig.acc = [{ name = \"best\", type = \"cell\", combine = \"carry\" }]\n\
         in.when = { bool = true }\nin.init.best = \"n2.cell\"\n",
    );
    nodes += &pass_node(
        "n4",
        "region.put",
        "region = \"n3\"\nconfig.acc = \"best\"\nin.value = \"n3.cell\"\nin.when = { bool = true }\n",
    );
    nodes += &pass_node("n5", "conv.to_u32", "in.a = \"n3.best\"\n");
    nodes += &pass_node("n6", "lane.set", "config.lane = \"x\"\nin.value = \"n5\"\n");
    let host = on_host(&nodes, None, 0);
    // The last position of the von Neumann table is the cell below, on a torus of 8 by 8.
    let below: Vec<u32> = (0..64).map(|cell| (cell + 8) % 64).collect();
    assert_eq!(host[0], below, "the carry ends at the cell below");
    assert_eq!(
        on_device(&device, &nodes, None, 0)[0],
        host[0],
        "the device and the host"
    );
}

/// Returns the nodes of a tick pass that writes to the lane `y` the largest component of the difference on a torus of
/// 100 by 100 from points up to six worlds below a far point to the far point, or its squared distance when
/// `squared` holds.
fn far_difference(squared: bool) -> String {
    let mut nodes = pass_node("n1", "ctx.index", "");
    nodes += &pass_node("n2", "conv.to_f32", "in.a = \"n1\"\n");
    nodes += &pass_node("n3", "math.mul", "in.a = \"n2\"\nin.b = { f32 = 13.7 }\n");
    nodes += &pass_node("n4", "math.sub", "in.a = \"n3\"\nin.b = { f32 = 600.0 }\n");
    nodes += &pass_node("n5", "vec.make", "in.x = \"n4\"\nin.y = \"n4\"\n");
    let ends = "in.a = \"n5\"\nin.b = { vec2f = [51.0, -260.0] }\nin.world = { vec2f = [100.0, 100.0] }\n";
    let written = if squared {
        nodes += &pass_node("n6", "space.dist_sq", ends);
        "n6"
    } else {
        nodes += &pass_node("n6", "space.delta", ends);
        nodes += &pass_node("n7", "vec.split", "in.v = \"n6\"\n");
        nodes += &pass_node("n8", "math.abs", "in.a = \"n7.x\"\n");
        nodes += &pass_node("n9", "math.abs", "in.a = \"n7.y\"\n");
        nodes += &pass_node("n10", "math.max", "in.a = \"n8\"\nin.b = \"n9\"\n");
        "n10"
    };
    nodes += &pass_node(
        "n20",
        "lane.set",
        &format!("config.lane = \"y\"\nin.value = \"{written}\"\n"),
    );
    nodes
}

/// The difference and the squared distance on a torus between points up to several worlds apart agree on the device
/// and on the host, each component of the difference within half a world.
#[test]
fn a_difference_between_far_points_is_reduced_on_the_device_as_on_the_host() {
    let Some(device) = device_for("a_difference_between_far_points_is_reduced_on_the_device_as_on_the_host") else {
        return;
    };
    for squared in [false, true] {
        let nodes = far_difference(squared);
        let host = on_host(&nodes, None, 0);
        let on_device = on_device(&device, &nodes, None, 0);
        for (cell, (device_word, host_word)) in on_device[1].iter().zip(&host[1]).enumerate() {
            let (device_value, host_value) = (f32::from_bits(*device_word), f32::from_bits(*host_word));
            if !squared {
                assert!(host_value <= 50.0, "cell {cell}: {host_value} is past half a world");
            }
            assert!(
                (device_value - host_value).abs() <= 1e-3 * host_value.max(1.0),
                "cell {cell}, squared {squared}: {device_value} on the device, {host_value} on the host"
            );
        }
    }
}
