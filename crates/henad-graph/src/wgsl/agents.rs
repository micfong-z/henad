//! WGSL compiler of graph models of agents: the shaders of every pass the GPU runs, the [`GpuAgentSpec`] that runs
//! them, and the [`ModelEntry`] a host builds.
//!
//! Each shader but the display is a *linear pass*, one invocation per element of its domain. A tick pass is a step,
//! and so is the *field update* of each field that takes deposits, which runs after the tick passes and merges the
//! deposits into the field one invocation per cell of each layer. The view passes over agents run in the snapshot
//! colour pass, which writes each agent's packed RGBA, and the view passes over cells in the display pass. The stats
//! passes run in one reduce leaf over the longest stats domain.
//!
//! *Buffers.* Each lane has a buffer of its own, `lane_<id>_<domain>`, a `vec2` lane as one interleaved `vec2` per
//! element. A lane that a tick pass writes and another agent reads is dual, its sides swapped at the end of the step.
//! A field holds its layers one after another in `field_<id>_cells`, and a field that takes deposits holds their merged
//! values in `accum_<id>_cells`. The engine's `counters` hold the fault word, then the document's counters, then the
//! tallies of the reduce leaf, which the engine clears before each leaf.

use std::fmt;
use std::sync::Arc;

use henad_compute::entry::{EntryDeclarations, ModelEntry};
use henad_compute::gpu::compose::MAX_FILE_BYTES;
use henad_compute::gpu::{
    AgentActionSpec, AgentBufferSpec, AgentDisplaySpec, AgentPassId, AgentPassSpec, AgentReduceSpec, BufferSide,
    GpuAgentSpec, ShaderSpec, SideRow, SideTable,
};
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::gpu_agent_model::Domain as EngineDomain;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::{f32_param, u32_param};
use henad_core::params::ParamDescriptor;
use henad_core::provenance::ModelSource;
use henad_core::view::StatDescriptor;

use super::agent_hooks::{AgentGraphHooks, BufferSource, ReducePass, StatSlot, WalkCharge};
use super::emit::{self, AGENT_COLORS, AgentPlan, EmitInput, Emitted, FIRST_COUNTER, FieldPlan, LanePlan, Role};
use super::uniform::WgslType;
use super::{
    GeneratedShader, ShaderRole, WORKGROUP_SIZE, any_instruction, compose, oversized, place, uniform::UniformBlock,
};
use crate::check::CheckReport;
use crate::document::{Domain, GraphDocument, Lane, Pass, PassEnabled, PassStage, World};
use crate::file::GraphFile;
use crate::host::HostEvaluator;
use crate::host::compile::{Instruction, Operation, Program};
use crate::ids::NodeId;
use crate::node::{Combine, Node};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, PortRef};

/// What the agents engine reads of one shader of an agents world.
#[derive(Debug, Clone)]
pub(crate) struct AgentShader {
    /// Pass of the spec the shader is, for its uniform block.
    pub pass: AgentPassId,
    /// Invocation domain of the pass.
    pub domain: EngineDomain,
    /// Bytes of the shader's `var<workgroup>` declarations.
    pub workgroup_bytes: u64,
    /// Lanes of a reduce leaf, each by the document position of its pass and the number of its sum.
    pub sum_lanes: Vec<(usize, usize)>,
}

/// A graph model of agents compiled to WGSL: its shaders, its host evaluator and its declarations.
///
/// Cheap to clone. Clones share everything.
#[derive(Clone)]
pub struct AgentProgram {
    parts: Arc<AgentParts>,
}

struct AgentParts {
    id: String,
    name: String,
    description: String,
    source: ModelSource,
    params: Vec<ParamDescriptor>,
    stats: Vec<StatDescriptor>,
    actions: Vec<ActionDescriptor>,
    buffers: Vec<AgentBufferSpec>,
    pos_buffer: usize,
    color_buffer: usize,
    shaders: Vec<GeneratedShader>,
    hooks: Arc<AgentGraphHooks>,
    index: bool,
    counter_words: usize,
    cleared: std::ops::Range<usize>,
    sum_lanes: usize,
    reduce_domain: EngineDomain,
    init_cost: u64,
    rerun_costs: Vec<u64>,
    replays_exactly: bool,
}

/// Prints the id and the shaders.
impl fmt::Debug for AgentProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentProgram")
            .field("id", &self.parts.id)
            .field("shaders", &self.parts.shaders)
            .finish_non_exhaustive()
    }
}

/// Hooks that forward every call to a shared [`AgentGraphHooks`], since a spec owns its hooks.
struct SharedHooks(Arc<AgentGraphHooks>);

/// Number of agents parameters the engine puts in front of a graph's own: the population and the world's two sides.
pub(crate) const AGENT_PARAMS: usize = 3;

/// One shader the compiler writes, before it writes it.
struct ShaderPlan {
    role: ShaderRole,
    label: String,
    /// Document positions of the passes it runs.
    positions: Vec<usize>,
    emit_role: Role,
    sides: SideTable,
    pass: AgentPassId,
    domain: EngineDomain,
    cost: u64,
}

/// Returns the engine's domain of a pass over `domain`.
fn engine_domain(domain: Option<Domain>) -> EngineDomain {
    match domain {
        Some(Domain::Cells) => EngineDomain::Cells(1),
        _ => EngineDomain::Agents,
    }
}

/// Returns the radius folds at the top of `block`, each with its node.
fn radius_folds(block: &[Instruction]) -> impl Iterator<Item = (&Instruction, &crate::host::compile::RadiusFold)> {
    block.iter().filter_map(|instruction| match &instruction.operation {
        Operation::RadiusFold(fold) => Some((instruction, &**fold)),
        _ => None,
    })
}

/// Returns whether a radius fold of `program` visits in an order its results depend on: one that holds a `carry`,
/// sums floats, or reads its inner `k` or the `prev.<acc>` of any accumulator.
///
/// The GPU's index orders the agents of a cell by the order its atomics resolve, which can differ between two runs.
fn fold_order_matters(program: &Program) -> bool {
    radius_folds(&program.element).any(|(_, fold)| {
        let float_sum = |accumulator: &crate::host::compile::RepeatAccumulator| {
            matches!(
                accumulator.identity,
                crate::host::HostValue::F32(_) | crate::host::HostValue::Vec2F(_)
            )
        };
        let combines = fold.body.iter().filter_map(|instruction| match &instruction.operation {
            Operation::Put { running, combine, .. } => Some((*running, *combine)),
            _ => None,
        });
        let mut order_dependent = false;
        for (running, combine) in combines {
            let accumulator = fold
                .accumulators
                .iter()
                .find(|accumulator| accumulator.running == running);
            match combine {
                Combine::Carry => order_dependent = true,
                Combine::Sum if accumulator.is_some_and(float_sum) => order_dependent = true,
                _ => {}
            }
        }
        // A running value read before the last visit, such as a count read as "first neighbour", depends on which
        // agents came first.
        let order_slots: Vec<usize> = std::iter::once(fold.inner[0])
            .chain(fold.accumulators.iter().map(|accumulator| accumulator.prev))
            .collect();
        order_dependent
            || any_instruction(&fold.body, &|operation| {
                operation.operands().iter().any(|operand| order_slots.contains(operand))
            })
    })
}

/// Id of the lane that stands for the merged value of the field `field` in its update.
fn merged_lane(field: &str) -> String {
    format!("henad_merged_{field}")
}

/// Returns `document` with a lane and a tick pass for the update of each field that takes deposits and has a `finish`
/// function: the pass reads the lane, calls the function and writes the lane, which the update's shader binds to the
/// merged value and to the field. The passes follow the last tick pass, where the reader would sort them, and each
/// comes with its field's id and its position.
fn with_field_updates(document: &GraphDocument) -> (GraphDocument, Vec<(String, usize)>) {
    let mut augmented = document.clone();
    let mut at = document
        .passes
        .iter()
        .rposition(|pass| pass.stage <= PassStage::Tick)
        .map_or(0, |position| position + 1);
    let mut next = crate::normal::first_free_id(document);
    let mut updates = Vec::new();
    for field in &document.fields {
        let Some(finish) = field.finish.as_ref().filter(|_| !field.is_static) else {
            continue;
        };
        let lane = merged_lane(&field.id);
        augmented.lanes.push(Lane::new(&lane, Domain::Cells, ValueType::F32));
        let ids: Vec<NodeId> = (0..3)
            .map(|offset| NodeId::new(next + offset).unwrap_or_else(|| unreachable!("a node id past the file's")))
            .collect();
        next += 3;
        let mut get = Node::new(ids[0], "lane.get");
        get.config.insert("lane".to_owned(), ConfigValue::String(lane.clone()));
        let mut call = Node::new(ids[1], "fn.call");
        call.config
            .insert("function".to_owned(), ConfigValue::String(finish.clone()));
        call.config.insert("key".to_owned(), ConfigValue::Integer(1));
        call.inputs.insert(
            "value".to_owned(),
            InputValue::Link(PortRef {
                node: ids[0],
                port: None,
            }),
        );
        let mut set = Node::new(ids[2], "lane.set");
        set.config.insert("lane".to_owned(), ConfigValue::String(lane));
        set.inputs.insert(
            "value".to_owned(),
            InputValue::Link(PortRef {
                node: ids[1],
                port: Some("value".to_owned()),
            }),
        );
        let mut pass = Pass::new(
            &format!("henad_update_{}", field.id),
            PassStage::Tick,
            Some(Domain::Cells),
        );
        pass.body.nodes = vec![get, call, set];
        updates.push((field.id.clone(), at));
        augmented.passes.insert(at, pass);
        at += 1;
    }
    (augmented, updates)
}

/// Returns the number of instructions of `block`, region bodies included, as the cost of a field update per cell.
fn instruction_count(block: &[Instruction]) -> u64 {
    block
        .iter()
        .map(|instruction| {
            1 + match &instruction.operation {
                Operation::Repeat(repeat) => instruction_count(&repeat.body) * u64::from(repeat.max),
                Operation::GridFold(fold) => instruction_count(&fold.body) * fold.offsets.len() as u64,
                _ => 0,
            }
        })
        .fold(0, u64::saturating_add)
}

impl AgentProgram {
    /// Checks `file` and compiles every pass the GPU runs.
    ///
    /// # Errors
    ///
    /// Returns the checker's errors, then the host evaluator's, then a diagnostic for each node no shader can run, with
    /// its pass and node, and a diagnostic for a graph that models no agents. A shader past the composer's size limit
    /// is reported against the pass whose nodes wrote most of it. A shader that does not compose is a fault of the
    /// compiler, reported against the node that wrote the line where it can be placed.
    #[expect(clippy::too_many_lines, reason = "every declaration of the entry is read here once")]
    pub fn compile(file: &GraphFile) -> Result<Self, Vec<CodeDiagnostic>> {
        let report = file.check();
        if report.has_errors() {
            return Err(report.errors().cloned().collect());
        }
        HostEvaluator::new(file).map_err(|error| error.diagnostics())?;
        let document = file.document();
        let World::Agents(world) = document.world else {
            return Err(vec![CodeDiagnostic::error(
                "the graph models a grid, and the compiler of agents compiles graphs of agents alone",
            )]);
        };
        let (evaluator, report, compiled) = compile_parts(file)?;
        let evaluator = Arc::new(evaluator);
        let CompiledAgents {
            shaders,
            plan,
            buffers,
            sources,
            index,
            tallies,
        } = compiled;
        let document = evaluator.document();
        let pos_buffer = plan.position_lane.unwrap_or_default();
        let color_buffer = buffers
            .iter()
            .position(|buffer| buffer.label == AGENT_COLORS)
            .unwrap_or_default();

        let uniforms: Vec<(AgentPassId, UniformBlock)> = shaders
            .iter()
            .filter_map(|shader| Some((shader.agent.as_ref()?.pass, shader.uniform.clone())))
            .collect();
        // The radius folds of each step, in the order of the spec's steps.
        let walks: Vec<Vec<WalkCharge>> = shaders
            .iter()
            .filter(|shader| matches!(shader.role, ShaderRole::Step | ShaderRole::Merge(_)))
            .map(|shader| {
                let mut charges = Vec::new();
                for id in shader.passes() {
                    let Some(position) = document.passes.iter().position(|pass| pass.id == *id) else {
                        continue;
                    };
                    if let Some(Ok(program)) = evaluator.program_at(position) {
                        charges.extend(radius_folds(&program.element).map(|(_, fold)| WalkCharge {
                            position,
                            radius: fold.radius,
                            body: instruction_count(&fold.body),
                        }));
                    }
                }
                charges
            })
            .collect();
        let reduce = shaders.iter().find(|shader| shader.role == ShaderRole::Reduce);
        let sum_lanes = reduce
            .and_then(|shader| shader.agent.as_ref())
            .map(|agent| agent.sum_lanes.clone())
            .unwrap_or_default();
        let mut reduce_passes = Vec::new();
        let mut first_tally = 0;
        for id in reduce.map(GeneratedShader::passes).unwrap_or_default() {
            if let Some(position) = document.passes.iter().position(|pass| pass.id == *id) {
                reduce_passes.push(ReducePass { position, first_tally });
                if let Some(Ok(program)) = evaluator.program_at(position) {
                    first_tally += program.tallies;
                }
            }
        }
        let stats = document
            .stats
            .iter()
            .map(|stat| {
                reduce_passes.iter().find_map(|pass| {
                    let Some(Ok(program)) = evaluator.program_at(pass.position) else {
                        return None;
                    };
                    let slot = if let Some(port) = &stat.from.port {
                        program.outputs.get(&(stat.from.node, port.clone())).copied()
                    } else {
                        let mut ports = program.outputs.iter().filter(|((node, _), _)| *node == stat.from.node);
                        let only = ports.next().map(|(_, slot)| *slot);
                        if ports.next().is_some() { None } else { only }
                    }?;
                    Some(StatSlot {
                        position: pass.position,
                        slot,
                    })
                })
            })
            .collect();
        let cell_size = document
            .passes
            .iter()
            .enumerate()
            .filter(|(_, pass)| pass.stage == PassStage::Tick)
            .find_map(|(position, _)| {
                let Some(Ok(program)) = evaluator.program_at(position) else {
                    return None;
                };
                radius_folds(&program.element)
                    .next()
                    .map(|(_, fold)| (position, fold.cell_size))
            });
        let faults = plan
            .faults
            .iter()
            .flat_map(|(pass, node)| {
                // The node as written: the outermost call of a copy, or the node itself.
                let written = evaluator
                    .origin(*node)
                    .map_or(*node, |origin| origin.calls.first().copied().unwrap_or(origin.node));
                let located = match evaluator.origin(*node) {
                    Some(origin) if !origin.calls.is_empty() => format!(
                        "node {written} (node {} of the function '{}')",
                        origin.node,
                        origin.function.as_deref().unwrap_or_default()
                    ),
                    _ => format!("node {written}"),
                };
                let line = place(
                    file,
                    CodeDiagnostic::error(String::new())
                        .with_pass(pass.clone())
                        .with_node(written.to_string()),
                )
                .line
                .map_or_else(String::new, |line| format!(" on line {line}"));
                [
                    format!(
                        "the pass '{pass}', {located}{line}: Agents in radius visited more than {} agents around one \
                         agent, and the run stops",
                        crate::limits::MAX_FOLD_CANDIDATES
                    ),
                    format!(
                        "the pass '{pass}', {located}{line}: Agents in radius would walk more than {} cells of the \
                         neighbour index around one agent, and the run stops. A smaller radius or a larger cell size \
                         walks fewer",
                        crate::limits::MAX_WALK_CELLS
                    ),
                ]
            })
            .collect();
        let actions_plan = document
            .actions
            .iter()
            .map(|action| super::hooks::ActionPlan {
                id: action.id.clone(),
                reruns: action.runs.iter().any(|run| {
                    document
                        .passes
                        .iter()
                        .any(|pass| pass.id == *run && pass.stage == PassStage::Init)
                }),
            })
            .collect();
        let hooks = Arc::new(AgentGraphHooks {
            evaluator: Arc::clone(&evaluator),
            count: world.count.clamp(1, world.max_count.max(1)),
            width: world.width,
            height: world.height,
            buffers: sources,
            uniforms,
            stats,
            reduce: reduce_passes,
            sum_lanes: sum_lanes.clone(),
            tally_base: FIRST_COUNTER + plan.counters,
            counters: plan.counters,
            faults,
            cell_size,
            walks,
            actions: actions_plan,
        });

        let cost_of = |id: &str| -> u64 {
            document
                .passes
                .iter()
                .zip(&report.passes)
                .find(|(pass, _)| pass.id == id)
                .map_or(0, |(_, pass)| pass.cost)
        };
        let init_cost = document
            .passes
            .iter()
            .zip(&report.passes)
            .filter(|(pass, _)| pass.stage == PassStage::Init && pass.enabled != PassEnabled::Literal(false))
            .map(|(_, pass)| pass.cost)
            .fold(0u64, u64::saturating_add);
        let rerun_costs = document
            .actions
            .iter()
            .map(|action| {
                action
                    .runs
                    .iter()
                    .filter(|run| {
                        document
                            .passes
                            .iter()
                            .any(|pass| pass.id == **run && pass.stage == PassStage::Init)
                    })
                    .map(|run| cost_of(run))
                    .fold(0u64, u64::saturating_add)
            })
            .collect();
        let replays_exactly = !document
            .passes
            .iter()
            .enumerate()
            .filter(|(_, pass)| pass.stage == PassStage::Tick)
            .any(|(position, _)| matches!(evaluator.program_at(position), Some(Ok(program)) if fold_order_matters(program)));

        let mut params = vec![
            u32_param(
                "num_agents",
                "Number of Agents",
                world.count.clamp(1, world.max_count.max(1)),
                1,
                world.max_count.max(1),
            )
            .on_reload(),
            f32_param("world_width", "World Width", world.width, 1.0, 10_000.0, Some(50.0)).on_reload(),
            f32_param("world_height", "World Height", world.height, 1.0, 10_000.0, Some(50.0)).on_reload(),
        ];
        params.extend(document.params.iter().cloned().map(ParamDescriptor::on_reload));
        let target = &document.target;
        let name = if target.name.is_empty() {
            &document.name
        } else {
            &target.name
        };
        let description = if target.description.is_empty() {
            &document.description
        } else {
            &target.description
        };
        let reduce_domain = reduce
            .and_then(|shader| shader.agent.as_ref())
            .map_or(EngineDomain::Agents, |agent| agent.domain);
        let cleared = FIRST_COUNTER + plan.counters..FIRST_COUNTER + plan.counters + tallies;
        Ok(Self {
            parts: Arc::new(AgentParts {
                id: target.id.clone(),
                name: name.clone(),
                description: description.clone(),
                source: file.source(),
                params,
                stats: document
                    .stats
                    .iter()
                    .map(|stat| StatDescriptor::owned(stat.label.clone(), stat.color))
                    .collect(),
                actions: document
                    .actions
                    .iter()
                    .map(|action| ActionDescriptor::owned(action.id.clone(), action.label.clone()))
                    .collect(),
                buffers,
                pos_buffer,
                color_buffer,
                shaders,
                hooks,
                index,
                counter_words: plan.counter_words,
                cleared,
                sum_lanes: sum_lanes.len(),
                reduce_domain,
                init_cost,
                rerun_costs,
                replays_exactly,
            }),
        })
    }

    /// Id of the model, the id of the graph's target.
    pub fn id(&self) -> &str {
        &self.parts.id
    }

    /// Every shader: the step passes, the field updates, then the colour, display and reduce passes, then each
    /// action's pass.
    pub fn shaders(&self) -> &[GeneratedShader] {
        &self.parts.shaders
    }

    /// Whether two builds on one seed step through identical states.
    ///
    /// It is false when a radius fold sums floats, holds a `carry` or reads its inner `k`, since the GPU's index
    /// orders the agents of a cell as its atomics resolve.
    pub fn replays_exactly(&self) -> bool {
        self.parts.replays_exactly
    }

    /// Returns the spec the GPU agent engine runs.
    pub fn spec(&self) -> GpuAgentSpec {
        let parts = &*self.parts;
        let of_role = |role: ShaderRole| parts.shaders.iter().filter(move |shader| shader.role == role);
        let linear = |shader: &GeneratedShader| {
            let agent = shader
                .agent
                .as_ref()
                .unwrap_or_else(|| unreachable!("every shader of an agents world has its facts"));
            let mut pass = AgentPassSpec::new(
                shader.label.clone(),
                ShaderSpec::new(shader.composed.clone(), shader.bindings.clone()),
                agent.domain,
                shader.sides.clone(),
            );
            pass.cost = shader.cost;
            pass.uniform_bytes = shader.uniform.size as u64;
            pass.workgroup_bytes = agent.workgroup_bytes;
            pass
        };
        let steps: Vec<AgentPassSpec> = parts
            .shaders
            .iter()
            .filter(|shader| matches!(shader.role, ShaderRole::Step | ShaderRole::Merge(_)))
            .map(linear)
            .collect();
        let reduce_shader = of_role(ShaderRole::Reduce)
            .next()
            .unwrap_or_else(|| unreachable!("the compiler writes one reduce shader"));
        let mut reduce = AgentReduceSpec::new(
            ShaderSpec::new(reduce_shader.composed.clone(), reduce_shader.bindings.clone()),
            parts.sum_lanes.max(1),
            parts.reduce_domain,
            reduce_shader.sides.clone(),
        );
        reduce.cost = reduce_shader.cost;
        reduce.uniform_bytes = reduce_shader.uniform.size as u64;
        reduce.workgroup_bytes = reduce_shader.agent.as_ref().map_or(0, |agent| agent.workgroup_bytes);
        let mut spec = GpuAgentSpec::new(
            parts.id.clone(),
            parts.buffers.clone(),
            parts.pos_buffer,
            parts.color_buffer,
            reduce,
            parts.stats.clone(),
            Box::new(SharedHooks(Arc::clone(&parts.hooks))),
        );
        spec.index = parts.index;
        spec.counters = parts.counter_words;
        spec.cleared_counters = parts.cleared.clone();
        spec.steps = steps;
        spec.color = of_role(ShaderRole::Color).next().map(linear);
        spec.display = of_role(ShaderRole::Display).next().map(|shader| {
            let mut display = AgentDisplaySpec::new(
                ShaderSpec::new(shader.composed.clone(), shader.bindings.clone()),
                WORKGROUP_SIZE,
                shader.sides.clone(),
            );
            display.cost = shader.cost;
            display.uniform_bytes = shader.uniform.size as u64;
            display
        });
        spec.actions = parts
            .actions
            .iter()
            .zip(&parts.rerun_costs)
            .map(|(action, rerun_cost)| {
                let id = action.id().to_owned();
                let shader = of_role(ShaderRole::Action(id.clone()))
                    .next()
                    .unwrap_or_else(|| unreachable!("the compiler writes one shader per action"));
                let pass = linear(shader);
                let mut spec = AgentActionSpec::new(id, pass.shader, pass.domain, pass.sides);
                spec.cost = pass.cost;
                spec.uniform_bytes = pass.uniform_bytes;
                spec.workgroup_bytes = pass.workgroup_bytes;
                spec.rerun_cost = *rerun_cost;
                spec
            })
            .collect();
        spec.init_cost = parts.init_cost;
        spec
    }

    /// Returns the entry that builds the model on any device passed to its build. Its source records the graph's
    /// semantic text as its code.
    pub fn entry(&self) -> ModelEntry {
        let parts = &*self.parts;
        let spec = Arc::new(self.spec());
        let mut declarations = EntryDeclarations::new(
            parts.id.clone(),
            parts.name.clone(),
            parts.description.clone(),
            parts.params.clone(),
            spec.topology_hint(),
            spec.structure(),
        );
        declarations.stats = parts.stats.clone();
        declarations.actions = parts.actions.clone();
        declarations.replays_exactly = parts.replays_exactly;
        ModelEntry::from_gpu_agent_spec(declarations, parts.source.clone(), spec)
    }
}

/// Returns the host evaluator of `file`, a checked graph of agents, with a lane and a pass for each field update, the
/// check of its normalised document, and its shaders.
///
/// # Errors
///
/// Returns the problems [`AgentProgram::compile`] returns after the check, each placed in `file`.
pub(crate) fn compile_parts(
    file: &GraphFile,
) -> Result<(HostEvaluator, CheckReport, CompiledAgents), Vec<CodeDiagnostic>> {
    let document = file.document();
    let (augmented, updates) = with_field_updates(document);
    let evaluator = HostEvaluator::from_document(&augmented).map_err(|error| error.diagnostics())?;
    // The normalised document holds the copies of every call and expression, and its check types them.
    let report = GraphFile::from_normalised(evaluator.document())
        .map_err(|error| {
            vec![CodeDiagnostic::error(format!(
                "the normalised graph does not read: {error}"
            ))]
        })?
        .check();
    let compiled = compile_agent_shaders(document, &report, &evaluator, &updates).map_err(|problems| {
        problems
            .into_iter()
            .map(|problem| place(file, problem))
            .collect::<Vec<_>>()
    })?;
    Ok((evaluator, report, compiled))
}

/// Shaders and the buffer plan of one graph of agents.
pub(crate) struct CompiledAgents {
    pub shaders: Vec<GeneratedShader>,
    pub plan: AgentPlan,
    pub buffers: Vec<AgentBufferSpec>,
    /// What each buffer holds, in the order of `buffers`.
    pub sources: Vec<BufferSource>,
    /// Whether a tick pass walks the neighbour index.
    pub index: bool,
    /// Number of tallies of the reduce leaf.
    pub tallies: usize,
}

/// Writes and composes every shader of `document`, a graph of agents, whose host evaluator `evaluator` holds the
/// passes of `updates` after the document's own.
///
/// # Errors
///
/// Returns a diagnostic for each node no shader can run, and each problem of composition.
#[expect(
    clippy::too_many_lines,
    reason = "the buffer plan, the side tables and the shader list read in order"
)]
pub(crate) fn compile_agent_shaders(
    document: &GraphDocument,
    report: &CheckReport,
    evaluator: &HostEvaluator,
    updates: &[(String, usize)],
) -> Result<CompiledAgents, Vec<CodeDiagnostic>> {
    let mut problems = Vec::new();
    let augmented = evaluator.document();
    let doc_lanes = document.lanes.len();
    let is_update = |position: usize| updates.iter().any(|(_, known)| *known == position);
    let lanes: Vec<LanePlan> = augmented
        .lanes
        .iter()
        .enumerate()
        .map(|(position, lane)| {
            let merged = (position >= doc_lanes).then(|| {
                augmented
                    .fields
                    .iter()
                    .position(|field| merged_lane(&field.id) == lane.id)
                    .unwrap_or_default()
            });
            let wgsl = WgslType::of(&lane.value_type);
            if wgsl.is_none() {
                problems.push(CodeDiagnostic::error(format!(
                    "the lane '{}' holds a type only the host holds, and every lane of an agents world lives on the GPU",
                    lane.id
                )));
            }
            LanePlan {
                id: lane.id.clone(),
                label: format!("lane_{}_{}", lane.id, lane.domain.as_str()),
                wgsl,
                merged,
            }
        })
        .collect();
    let position_lane = document
        .lanes
        .iter()
        .position(|lane| lane.role == Some(crate::document::LaneRole::Position));
    if position_lane.is_none() {
        problems.push(CodeDiagnostic::error(
            "the world of agents has no lane with `role = \"position\"`, which the GPU draws and indexes",
        ));
    }
    let mut fields = Vec::new();
    let (mut column, mut deposit) = (augmented.lanes.len(), 0);
    for field in &augmented.fields {
        let layers = u32::try_from(field.layers.len()).unwrap_or(u32::MAX);
        fields.push(FieldPlan {
            id: field.id.clone(),
            layers,
            wgsl: if matches!(field.value_type, ValueType::Category(_)) {
                WgslType::U32
            } else {
                WgslType::F32
            },
            first: column,
            dynamic: !field.is_static,
            first_deposit: deposit,
        });
        column += field.layers.len();
        if !field.is_static {
            deposit += field.layers.len();
        }
    }
    let runs = |stage: PassStage| {
        augmented
            .passes
            .iter()
            .enumerate()
            .filter(move |(position, pass)| {
                !is_update(*position) && pass.stage == stage && pass.enabled != PassEnabled::Literal(false)
            })
            .map(|(position, _)| position)
    };
    let program = |position: usize| evaluator.program_at(position)?.as_ref().ok();
    for action in &document.actions {
        for run in &action.runs {
            if document
                .passes
                .iter()
                .any(|pass| pass.id == *run && pass.stage == PassStage::Tick)
            {
                problems.push(
                    CodeDiagnostic::error(format!(
                        "the action '{}' reruns the tick pass '{run}', and the GPU runs an action's reruns on the \
                         host, which runs init passes alone",
                        action.id
                    ))
                    .with_pass(run.clone()),
                );
            }
        }
    }
    let ticks: Vec<usize> = runs(PassStage::Tick).collect();
    let mut faults = Vec::new();
    let mut index = false;
    for &position in &ticks {
        if let Some(program) = program(position) {
            for (instruction, _) in radius_folds(&program.element) {
                faults.push((augmented.passes[position].id.clone(), instruction.node));
                index = true;
            }
        }
    }
    let gpu_passes: Vec<usize> = [PassStage::Tick, PassStage::View, PassStage::Stats, PassStage::Action]
        .into_iter()
        .flat_map(runs)
        .chain(updates.iter().map(|(_, position)| *position))
        .collect();
    let tick_buffer = gpu_passes.iter().any(|&position| {
        program(position).is_some_and(|program| {
            let on_tick = augmented.passes[position].stage == PassStage::Tick;
            [&program.prelude, &program.element].into_iter().any(|block| {
                any_instruction(block, &|operation| {
                    matches!(operation, Operation::Tick) || (on_tick && matches!(operation, Operation::Draw { .. }))
                })
            })
        })
    });
    let dual: Vec<(usize, usize)> = document
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| report.dual_lanes.contains(&lane.id))
        .filter_map(|(lane, _)| {
            ticks
                .iter()
                .position(|&position| program(position).is_some_and(|program| program.written.contains(&lane)))
                .map(|step| (lane, step))
        })
        .collect();

    // Buffers: the document's lanes at their own positions, then each field, each field's deposits, the colours and
    // the tick.
    let mut buffers: Vec<AgentBufferSpec> = Vec::new();
    let mut sources: Vec<BufferSource> = Vec::new();
    for (position, lane) in document.lanes.iter().enumerate() {
        let is_dual = dual.iter().any(|(known, _)| *known == position);
        buffers.push(AgentBufferSpec::new(
            lanes[position].label.clone(),
            is_dual,
            Some(position) == position_lane,
        ));
        sources.push(BufferSource::Lane {
            column: position,
            domain: lane.domain,
            words: crate::host::value::words_per_value(&lane.value_type).unwrap_or(1),
        });
    }
    for field in &fields {
        buffers.push(AgentBufferSpec::new(field.label(), false, false));
        sources.push(BufferSource::Field {
            first: field.first,
            layers: field.layers as usize,
        });
    }
    for field in fields.iter().filter(|field| field.dynamic) {
        buffers.push(AgentBufferSpec::new(field.accum_label(), false, false));
        sources.push(BufferSource::Cleared {
            per_cell: field.layers as usize,
        });
    }
    buffers.push(AgentBufferSpec::new(AGENT_COLORS, false, true));
    sources.push(BufferSource::Colors);
    let buffer_count = buffers.len();

    let update_positions: Vec<(usize, usize)> = updates
        .iter()
        .filter_map(|(id, position)| Some((fields.iter().position(|field| field.id == *id)?, *position)))
        .collect();
    let dynamic: Vec<usize> = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.dynamic)
        .map(|(index, _)| index)
        .collect();
    let step_sides = |step: usize| {
        let mut rows = vec![SideRow::new(BufferSide::Current, BufferSide::Current); buffer_count];
        for &(lane, writer) in &dual {
            rows[lane] = match step.cmp(&writer) {
                std::cmp::Ordering::Less => SideRow::new(BufferSide::Current, BufferSide::Current),
                std::cmp::Ordering::Equal => SideRow::new(BufferSide::Current, BufferSide::Next),
                std::cmp::Ordering::Greater => SideRow::new(BufferSide::Next, BufferSide::Next),
            };
        }
        SideTable::new(rows)
    };
    let counters = document.counters.len();
    let cost_of = |positions: &[usize]| {
        positions
            .iter()
            .filter_map(|&position| report.passes.get(position))
            .map(|pass| pass.cost)
            .fold(0u64, u64::saturating_add)
    };
    let mut plans: Vec<ShaderPlan> = Vec::new();
    for (step, &position) in ticks.iter().enumerate() {
        plans.push(ShaderPlan {
            role: ShaderRole::Step,
            label: augmented.passes[position].id.clone(),
            positions: vec![position],
            emit_role: Role::Step { writes_tick: false },
            sides: step_sides(step),
            pass: AgentPassId::Step(step),
            domain: engine_domain(augmented.passes[position].domain),
            cost: cost_of(&[position]),
        });
    }
    for (k, &field) in dynamic.iter().enumerate() {
        let step = ticks.len() + k;
        let position = update_positions
            .iter()
            .find(|(known, _)| *known == field)
            .map(|(_, position)| *position);
        let cost = 4 + position
            .and_then(program)
            .map_or(0, |program| instruction_count(&program.element));
        plans.push(ShaderPlan {
            role: ShaderRole::Merge(fields[field].id.clone()),
            label: format!("update_{}", fields[field].id),
            positions: position.into_iter().collect(),
            emit_role: Role::Merge(field),
            sides: step_sides(step),
            pass: AgentPassId::Step(step),
            domain: EngineDomain::Cells(fields[field].layers.max(1)),
            cost,
        });
    }
    // A pass reads the tick from the counters, and a pass of its own advances it after the step's last pass, since an
    // invocation that advanced it would race the others reading it.
    if tick_buffer {
        let step = ticks.len() + dynamic.len();
        plans.push(ShaderPlan {
            role: ShaderRole::Step,
            label: "advance_tick".to_owned(),
            positions: Vec::new(),
            emit_role: Role::Step { writes_tick: true },
            sides: step_sides(step),
            pass: AgentPassId::Step(step),
            domain: EngineDomain::Agents,
            cost: 1,
        });
    }
    let views: Vec<usize> = runs(PassStage::View).collect();
    let (agent_views, cell_views): (Vec<usize>, Vec<usize>) = views
        .iter()
        .partition(|&&position| augmented.passes[position].domain != Some(Domain::Cells));
    plans.push(ShaderPlan {
        role: ShaderRole::Color,
        label: "color".to_owned(),
        positions: agent_views.clone(),
        emit_role: Role::Color,
        sides: SideTable::in_place(buffer_count),
        pass: AgentPassId::Color,
        domain: EngineDomain::Agents,
        cost: cost_of(&agent_views).max(1),
    });
    if !cell_views.is_empty() {
        plans.push(ShaderPlan {
            role: ShaderRole::Display,
            label: "display".to_owned(),
            positions: cell_views.clone(),
            emit_role: Role::Display,
            sides: SideTable::in_place(buffer_count),
            pass: AgentPassId::Display,
            domain: EngineDomain::Cells(1),
            cost: cost_of(&cell_views).max(1),
        });
    }
    let stats: Vec<usize> = runs(PassStage::Stats).collect();
    let over_agents = stats
        .iter()
        .any(|&position| augmented.passes[position].domain != Some(Domain::Cells));
    let over_cells = stats
        .iter()
        .any(|&position| augmented.passes[position].domain == Some(Domain::Cells));
    plans.push(ShaderPlan {
        role: ShaderRole::Reduce,
        label: "reduce".to_owned(),
        positions: stats.clone(),
        emit_role: Role::Reduce,
        sides: SideTable::in_place(buffer_count),
        pass: AgentPassId::Reduce,
        domain: match (over_agents, over_cells) {
            (true, true) => EngineDomain::AgentsOrCells,
            (false, true) => EngineDomain::Cells(1),
            _ => EngineDomain::Agents,
        },
        cost: cost_of(&stats).max(1),
    });
    for (action_index, action) in document.actions.iter().enumerate() {
        let passes: Vec<usize> = runs(PassStage::Action)
            .filter(|&position| augmented.passes[position].action.as_deref() == Some(action.id.as_str()))
            .collect();
        if passes.len() > 1 {
            problems.push(
                CodeDiagnostic::error(format!(
                    "the action '{}' runs {} action passes, and the GPU runs one action pass per action in an agents \
                     world",
                    action.id,
                    passes.len()
                ))
                .with_pass(augmented.passes[passes[1]].id.clone()),
            );
            continue;
        }
        let domain = passes.first().map_or(EngineDomain::Agents, |&position| {
            engine_domain(augmented.passes[position].domain)
        });
        plans.push(ShaderPlan {
            role: ShaderRole::Action(action.id.clone()),
            label: passes.first().map_or_else(
                || format!("{}_noop", action.id),
                |&position| augmented.passes[position].id.clone(),
            ),
            positions: passes.clone(),
            emit_role: Role::Action,
            sides: SideTable::in_place(buffer_count),
            pass: AgentPassId::Action(action_index),
            domain,
            cost: cost_of(&passes).max(1),
        });
    }
    for field in &fields {
        let written_partially = document.actions.iter().any(|action| {
            let mut written: Vec<usize> = Vec::new();
            for run in &action.runs {
                if let Some(position) = augmented
                    .passes
                    .iter()
                    .position(|pass| pass.id == *run && pass.stage == PassStage::Init)
                    && let Some(program) = program(position)
                {
                    written.extend(program.written.iter().copied());
                }
            }
            let layers = (field.first..field.first + field.layers as usize)
                .filter(|layer| written.contains(layer))
                .count();
            layers > 0 && layers < field.layers as usize
        });
        if written_partially {
            problems.push(CodeDiagnostic::error(format!(
                "an action reruns an init pass that writes some layers of the field '{}', and the GPU uploads a \
                 field whole: write every layer of it in the rerun",
                field.id
            )));
        }
    }
    let plan = AgentPlan {
        fields,
        counters,
        counter_words: FIRST_COUNTER + counters,
        faults,
        position_lane,
    };
    if !problems.is_empty() {
        return Err(problems);
    }
    let mut shaders = Vec::with_capacity(plans.len());
    let mut tallies = 0;
    let mut first_plan = plan.clone();
    // The reduce leaf's tallies size the counters, which every shader declares, so the leaf is written first.
    let reduce_tallies = plans
        .iter()
        .find(|shader| shader.emit_role == Role::Reduce)
        .map_or(0, |shader| {
            shader
                .positions
                .iter()
                .filter_map(|&position| program(position))
                .map(|program| program.tallies)
                .sum::<usize>()
        });
    first_plan.counter_words = FIRST_COUNTER + counters + reduce_tallies;
    let input = EmitInput {
        document: augmented,
        report,
        evaluator,
        lanes: &lanes,
        tick_buffer,
        outer_iteration: None,
        agents: Some(&first_plan),
    };
    for ShaderPlan {
        role,
        label,
        positions,
        emit_role,
        sides,
        pass,
        domain,
        cost,
    } in plans
    {
        let emitted = match emit::emit(&input, emit_role, &positions, &sides) {
            Ok(emitted) => emitted,
            Err(found) => {
                problems.extend(found);
                continue;
            }
        };
        let Emitted {
            text,
            ranges,
            bindings,
            uniform,
            counters: shader_tallies,
            sum_lanes,
            workgroup_bytes,
        } = emitted;
        if role == ShaderRole::Reduce {
            tallies = shader_tallies;
        }
        if text.len() > MAX_FILE_BYTES {
            problems.push(oversized(&label, text.len(), &ranges));
            continue;
        }
        let composed = match compose(&label, &text, &ranges) {
            Ok(composed) => composed,
            Err(found) => {
                problems.extend(found);
                continue;
            }
        };
        shaders.push(GeneratedShader {
            role,
            passes: positions
                .iter()
                .map(|&position| augmented.passes[position].id.clone())
                .collect(),
            label,
            text,
            composed,
            ranges,
            bindings,
            uniform,
            sides,
            cost,
            counters: shader_tallies,
            grid_sum_lanes: Vec::new(),
            pass_id: henad_compute::gpu::GridPassId::Display,
            agent: Some(AgentShader {
                pass,
                domain,
                workgroup_bytes,
                sum_lanes,
            }),
        });
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(CompiledAgents {
        shaders,
        plan: first_plan,
        buffers,
        sources,
        index,
        tallies,
    })
}

/// Forwards every hook to the shared plan.
impl henad_compute::gpu::AgentHooks for SharedHooks {
    fn dims(&self, params: &[henad_core::params::ParamValue]) -> (u32, henad_core::authoring::model::field::Extent) {
        self.0.dims(params)
    }

    fn buffer_lens(&self, geom: &henad_core::authoring::model::gpu_agent_model::Geometry) -> Vec<usize> {
        self.0.buffer_lens(geom)
    }

    fn seed_buffers(
        &self,
        geom: &henad_core::authoring::model::gpu_agent_model::Geometry,
        params: &[henad_core::params::ParamValue],
        seed: Option<u64>,
    ) -> Vec<Vec<u8>> {
        self.0.seed_buffers(geom, params, seed)
    }

    fn index_cell_size(&self, params: &[henad_core::params::ParamValue]) -> f32 {
        self.0.index_cell_size(params)
    }

    fn pass_params_bytes(
        &self,
        pass: henad_core::authoring::model::gpu_agent_model::PassId,
        ctx: henad_core::authoring::model::gpu_agent_model::PassCtx<'_>,
        params: &[henad_core::params::ParamValue],
    ) -> Vec<u8> {
        self.0.pass_params_bytes(pass, ctx, params)
    }

    fn stats(
        &self,
        sums: &[f32],
        counters: &[u32],
        geom: &henad_core::authoring::model::gpu_agent_model::Geometry,
    ) -> Vec<henad_core::view::StatValue> {
        self.0.stats(sums, counters, geom)
    }

    fn run_params_bytes(
        &self,
        pass: AgentPassId,
        ctx: henad_core::authoring::model::gpu_agent_model::PassCtx<'_>,
        run: henad_compute::gpu::AgentRun,
        params: &[henad_core::params::ParamValue],
    ) -> Result<Vec<u8>, String> {
        self.0.run_params_bytes(pass, ctx, run, params)
    }

    fn run_stats(
        &self,
        sums: &[f32],
        counters: &[u32],
        geom: &henad_core::authoring::model::gpu_agent_model::Geometry,
        run: henad_compute::gpu::AgentRun,
        params: &[henad_core::params::ParamValue],
    ) -> Vec<henad_core::view::StatValue> {
        self.0.run_stats(sums, counters, geom, run, params)
    }

    fn rerun(
        &self,
        action: usize,
        geom: &henad_core::authoring::model::gpu_agent_model::Geometry,
        run: henad_compute::gpu::AgentRun,
        params: &[henad_core::params::ParamValue],
        stream: &mut u64,
    ) -> Result<henad_compute::gpu::AgentUploads, String> {
        self.0.rerun(action, geom, run, params, stream)
    }

    fn fault(&self, counters: &[u32]) -> Option<String> {
        self.0.fault(counters)
    }

    fn reports_faults(&self) -> bool {
        self.0.reports_faults()
    }

    fn step_cost(
        &self,
        step: usize,
        geom: &henad_core::authoring::model::gpu_agent_model::Geometry,
        params: &[henad_core::params::ParamValue],
    ) -> u64 {
        self.0.step_cost(step, geom, params)
    }
}
