//! WGSL compiler of graph models: the shaders of every pass the GPU runs, the [`GpuGridSpec`] or
//! [`GpuAgentSpec`](henad_compute::gpu::GpuAgentSpec) that runs them, and the [`ModelEntry`] a host builds.
//!
//! [`GridProgram`] compiles a graph on a grid, and [`AgentProgram`] a graph of agents. What follows describes the
//! grid's compiler, and the agents' extends it in `agents.rs`.
//!
//! [`GridProgram::compile`] checks a graph and writes one shader per *role*: one per tick pass, a display shader for
//! the view passes, a reduce shader for the stats passes, and one per action pass. Each shader is the engine's template
//! around the body of each pass it runs, and the body is the pass's host program written as WGSL, so a node means the
//! same on both backends. Init passes and the reruns of an action run on the host when the model is built and when the
//! action is pressed, and their lanes are uploaded.
//!
//! *Names.* Every value is a `let` named `n<id>_<port>`, with each `.` of a dotted port written `__`, so a line of a
//! shader names its node. [`GeneratedShader::ranges`] holds the byte range of the text each node wrote, for a line that
//! holds several. A lane binds as `lane_<id>_cells`, and as `lane_<id>_cells_in` and `lane_<id>_cells_out` in the tick
//! pass that writes a *dual* lane, one another element reads. Names the template owns start with `henad_`.
//!
//! *Uniform block.* A pass's prelude, the values the same for every element, runs on the host when the model is built,
//! and the shader reads the values in the uniform block `params`, with the grid's size, each draw's key word and the
//! press count of an action. A prelude value that reads the tick is computed in the shader, which reads the tick from a
//! one-word buffer that the last tick pass advances.
//!
//! *Regions.* A Repeat is a counted `for` loop that the compiler writes, with its accumulators as `var`s before it. A
//! grid count is unrolled over its stencil, one test per neighbour.

pub(crate) mod agent_hooks;
pub(crate) mod agents;
pub(crate) mod emit;
mod hooks;
pub(crate) mod uniform;

pub use agents::AgentProgram;

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use henad_compute::entry::{EntryDeclarations, ModelEntry};
use henad_compute::gpu::compose::{MAX_FILE_BYTES, WgslFile, compose_wgsl};
use henad_compute::gpu::{
    BufferSide, GpuGridSpec, GridActionSpec, GridBufferSpec, GridPassId, GridPassSpec, ShaderBinding, ShaderSpec,
    SideRow, SideTable,
};
use henad_core::action::ActionDescriptor;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::u32_param;
use henad_core::params::ParamDescriptor;
use henad_core::provenance::ModelSource;
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

use crate::check::CheckReport;
use crate::document::{Domain, GraphDocument, PassEnabled, PassStage, World};
use crate::draw::DRAW_WGSL;
use crate::file::GraphFile;
use crate::host::HostEvaluator;
use crate::host::compile::{Instruction, Operation};
#[cfg(test)]
use crate::host::{HostWorld, RunContext};
use crate::ids::NodeId;

use emit::{EmitInput, Emitted, LanePlan, Role};
use hooks::{ActionPlan, GraphHooks, PassUniform, StatSource};
use uniform::{UniformBlock, WgslType};

/// Side `N` of the `@workgroup_size(N, N)` of every generated grid shader.
pub const WORKGROUP_SIZE: u32 = 16;

/// Largest side of a graph's grid, in cells, the bound of its `grid_width` and `grid_height`.
pub const MAX_GRID_SIDE: u32 = 16_384;

/// Byte range of a generated shader's text that one node wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NodeRange {
    /// Bytes of [`GeneratedShader::text`].
    pub range: Range<usize>,
    /// Id of the node's pass.
    pub pass: String,
    /// The node.
    pub node: NodeId,
}

/// Role of a generated shader.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShaderRole {
    /// A step pass, in the order of the step. Every tick pass is one, and a graph that reads the tick with no tick pass
    /// has one that only advances it.
    Step,
    /// The display pass, which runs the view passes.
    Display,
    /// The reduce pass, which runs the stats passes.
    Reduce,
    /// A pass of the action with this id.
    Action(String),
    /// The snapshot colour pass of an agents world, which runs the view passes over agents.
    Color,
    /// The field update of the field with this id, in an agents world.
    Merge(String),
}

/// One shader the compiler wrote, before and after composition.
#[derive(Clone)]
pub struct GeneratedShader {
    role: ShaderRole,
    label: String,
    passes: Vec<String>,
    text: String,
    composed: String,
    ranges: Vec<NodeRange>,
    bindings: Vec<ShaderBinding>,
    uniform: UniformBlock,
    sides: SideTable,
    cost: u64,
    counters: usize,
    /// Pass of the spec the shader is, for its uniform block.
    pass_id: GridPassId,
    /// What the agents engine reads of a shader of an agents world, `None` for a grid.
    agent: Option<agents::AgentShader>,
}

/// Prints the role, the label, the passes and the byte lengths of the texts.
impl fmt::Debug for GeneratedShader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedShader")
            .field("role", &self.role)
            .field("label", &self.label)
            .field("passes", &self.passes)
            .field("text_bytes", &self.text.len())
            .field("composed_bytes", &self.composed.len())
            .finish_non_exhaustive()
    }
}

impl GeneratedShader {
    /// Role of the shader.
    pub fn role(&self) -> &ShaderRole {
        &self.role
    }

    /// Label of the shader's pass in the engine: a pass id, `display`, `reduce` or `advance_tick`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Ids of the passes the shader runs, in order.
    pub fn passes(&self) -> &[String] {
        &self.passes
    }

    /// Text the compiler wrote, with its `#import` lines.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Text after composition with the shared modules, as naga writes it back and the device compiles it.
    pub fn composed(&self) -> &str {
        &self.composed
    }

    /// Byte ranges of [`Self::text`] that each node wrote, in text order.
    pub fn ranges(&self) -> &[NodeRange] {
        &self.ranges
    }

    /// Returns the pass and node that wrote the byte at `offset` of [`Self::text`], or `None` for the template's text.
    pub fn node_at(&self, offset: usize) -> Option<(&str, NodeId)> {
        self.ranges
            .iter()
            .filter(|range| range.range.contains(&offset))
            .min_by_key(|range| range.range.len())
            .map(|range| (range.pass.as_str(), range.node))
    }

    /// Cost of one invocation in work units, the checker's cost of the passes it runs.
    pub fn cost(&self) -> u64 {
        self.cost
    }

    /// What the agents engine reads of the shader, `None` for a shader of a grid.
    #[cfg(test)]
    pub(crate) fn agent(&self) -> Option<&agents::AgentShader> {
        self.agent.as_ref()
    }

    /// Uniform block of the shader.
    #[cfg(test)]
    pub(crate) fn uniform(&self) -> &UniformBlock {
        &self.uniform
    }

    /// Bindings of the shader in `@binding` order.
    #[cfg(test)]
    pub(crate) fn bindings(&self) -> &[ShaderBinding] {
        &self.bindings
    }

    /// Side table the shader binds through, one row per buffer.
    #[cfg(test)]
    pub(crate) fn sides(&self) -> &SideTable {
        &self.sides
    }

    /// Number of counters of a reduce shader.
    pub(crate) fn counters(&self) -> usize {
        self.counters
    }

    /// Returns the shader's uniform block for a grid of `world` with the document's parameters `params` in `context`.
    ///
    /// # Errors
    ///
    /// Returns the fault of a prelude the host cannot evaluate.
    #[cfg(test)]
    pub(crate) fn uniform_bytes(
        &self,
        evaluator: &HostEvaluator,
        world: HostWorld,
        params: &[crate::host::HostValue],
        context: &RunContext,
    ) -> Result<Vec<u8>, String> {
        self.uniform
            .bytes(evaluator, world, params, context, &uniform::Dispatch::default())
    }

    /// Returns the spec of the pass.
    fn pass_spec(&self) -> GridPassSpec {
        let mut pass = GridPassSpec::new(
            self.label.clone(),
            ShaderSpec::new(self.composed.clone(), self.bindings.clone()),
            self.sides.clone(),
        );
        pass.cost = self.cost;
        pass.uniform_bytes = self.uniform.size as u64;
        if self.role == ShaderRole::Reduce {
            pass.workgroup_bytes = self.counters as u64 * std::mem::size_of::<u32>() as u64;
        }
        pass
    }
}

/// A graph model on a grid compiled to WGSL: its shaders, its host evaluator and its declarations.
///
/// Cheap to clone. Clones share everything.
#[derive(Clone)]
pub struct GridProgram {
    parts: Arc<GridParts>,
}

struct GridParts {
    id: String,
    name: String,
    description: String,
    source: ModelSource,
    params: Vec<ParamDescriptor>,
    stats: Vec<StatDescriptor>,
    actions: Vec<ActionDescriptor>,
    palette: Option<Vec<[u8; 4]>>,
    buffers: Vec<GridBufferSpec>,
    shaders: Vec<GeneratedShader>,
    hooks: Arc<GraphHooks>,
    counters: usize,
    init_cost: u64,
    rerun_costs: Vec<u64>,
}

/// Prints the id and the shaders.
impl fmt::Debug for GridProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GridProgram")
            .field("id", &self.parts.id)
            .field("shaders", &self.parts.shaders)
            .finish_non_exhaustive()
    }
}

/// Hooks that forward every call to a shared [`GraphHooks`], since a spec owns its hooks.
struct SharedHooks(Arc<GraphHooks>);

impl henad_compute::gpu::GridHooks for SharedHooks {
    fn dims(&self, params: &[henad_core::params::ParamValue]) -> (u32, u32) {
        self.0.dims(params)
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        self.0.buffer_lens(width, height)
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        self.0.step_dims(width, height)
    }

    fn seed_buffers(
        &self,
        width: u32,
        height: u32,
        params: &[henad_core::params::ParamValue],
        seed: Option<u64>,
    ) -> Vec<Vec<u32>> {
        self.0.seed_buffers(width, height, params, seed)
    }

    fn pass_params_bytes(
        &self,
        pass: GridPassId,
        ctx: henad_compute::gpu::GridPassCtx,
        params: &[henad_core::params::ParamValue],
    ) -> Vec<u8> {
        self.0.pass_params_bytes(pass, ctx, params)
    }

    fn rerun(
        &self,
        action: usize,
        ctx: henad_compute::gpu::GridPassCtx,
        params: &[henad_core::params::ParamValue],
        stream: &mut u64,
    ) -> Result<henad_compute::gpu::RerunUploads, String> {
        self.0.rerun(action, ctx, params, stream)
    }

    fn stats(
        &self,
        counts: &[u32],
        ctx: henad_compute::gpu::GridPassCtx,
        params: &[henad_core::params::ParamValue],
    ) -> Vec<henad_core::view::StatValue> {
        self.0.stats(counts, ctx, params)
    }
}

impl GridProgram {
    /// Checks `file` and compiles every pass the GPU runs.
    ///
    /// # Errors
    ///
    /// Returns the checker's errors, then the host evaluator's, then a diagnostic for each node no shader can run, with
    /// its pass and node, and a diagnostic for a graph that is no grid. A shader past the composer's size limit is
    /// reported against the pass whose nodes wrote most of it. A shader that does not compose is a fault of the
    /// compiler, reported against the node that wrote the line where it can be placed.
    #[expect(clippy::too_many_lines, reason = "every declaration of the entry is read here once")]
    pub fn compile(file: &GraphFile) -> Result<Self, Vec<CodeDiagnostic>> {
        let report = file.check();
        if report.has_errors() {
            return Err(report.errors().cloned().collect());
        }
        let evaluator = HostEvaluator::new(file).map_err(|error| error.diagnostics())?;
        let document = file.document();
        let World::Grid(grid) = document.world else {
            return Err(vec![CodeDiagnostic::error(
                "the graph models agents, and `GridProgram` compiles graphs on a grid. Compile it with \
                 `AgentProgram`",
            )]);
        };
        let compiled = compile_shaders(document, &report, &evaluator, None).map_err(|problems| {
            problems
                .into_iter()
                .map(|problem| place(file, problem))
                .collect::<Vec<_>>()
        })?;
        let evaluator = Arc::new(evaluator);
        let Compiled {
            shaders,
            lanes,
            tick_buffer,
            dual,
        } = compiled;

        let mut buffers: Vec<GridBufferSpec> = lanes
            .iter()
            .enumerate()
            .map(|(position, lane)| GridBufferSpec::new(format!("lane_{}_cells", lane.id), dual.contains(&position)))
            .collect();
        if tick_buffer {
            buffers.push(GridBufferSpec::new("henad_tick", true));
        }

        let uniforms = shaders
            .iter()
            .map(|shader| PassUniform {
                id: shader.pass_id,
                block: shader.uniform.clone(),
            })
            .collect();
        let reduce = shaders.iter().find(|shader| shader.role == ShaderRole::Reduce);
        let counters = reduce.map_or(1, GeneratedShader::counters);
        let stats = stat_sources(document, &evaluator, reduce);
        let actions = document
            .actions
            .iter()
            .map(|action| ActionPlan {
                id: action.id.clone(),
                reruns: action.runs.iter().any(|run| {
                    document
                        .passes
                        .iter()
                        .any(|pass| pass.id == *run && pass.stage == PassStage::Init)
                }),
            })
            .collect();
        let hooks = Arc::new(GraphHooks {
            evaluator: Arc::clone(&evaluator),
            width: grid.width.clamp(1, MAX_GRID_SIDE),
            height: grid.height.clamp(1, MAX_GRID_SIDE),
            boundary: grid.boundary,
            tick_buffer,
            uniforms,
            stats,
            actions,
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

        let mut params = vec![
            u32_param(
                "grid_width",
                "Grid Width",
                grid.width.clamp(1, MAX_GRID_SIDE),
                1,
                MAX_GRID_SIDE,
            )
            .on_reload(),
            u32_param(
                "grid_height",
                "Grid Height",
                grid.height.clamp(1, MAX_GRID_SIDE),
                1,
                MAX_GRID_SIDE,
            )
            .on_reload(),
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
        Ok(Self {
            parts: Arc::new(GridParts {
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
                palette: display_palette(document),
                buffers,
                shaders,
                hooks,
                counters,
                init_cost,
                rerun_costs,
            }),
        })
    }

    /// Id of the model, the id of the graph's target.
    pub fn id(&self) -> &str {
        &self.parts.id
    }

    /// Every shader, step passes first, then the display and reduce passes, then each action's passes.
    pub fn shaders(&self) -> &[GeneratedShader] {
        &self.parts.shaders
    }

    /// Returns the spec the GPU grid engine runs.
    pub fn spec(&self) -> GpuGridSpec {
        let parts = &*self.parts;
        let of_role = |role: ShaderRole| parts.shaders.iter().filter(move |shader| shader.role == role);
        let steps: Vec<GridPassSpec> = of_role(ShaderRole::Step).map(GeneratedShader::pass_spec).collect();
        let display = of_role(ShaderRole::Display)
            .next()
            .map(GeneratedShader::pass_spec)
            .unwrap_or_else(|| unreachable!("the compiler writes one display shader"));
        let reduce = of_role(ShaderRole::Reduce)
            .next()
            .map(GeneratedShader::pass_spec)
            .unwrap_or_else(|| unreachable!("the compiler writes one reduce shader"));
        let mut spec = GpuGridSpec::new(
            parts.id.clone(),
            WORKGROUP_SIZE,
            parts.buffers.clone(),
            steps,
            display,
            reduce,
            parts.stats.clone(),
            Box::new(SharedHooks(Arc::clone(&parts.hooks))),
        );
        spec.actions = parts
            .actions
            .iter()
            .zip(&parts.rerun_costs)
            .map(|(action, rerun_cost)| {
                let id = action.id().to_owned();
                let passes = of_role(ShaderRole::Action(id.clone()))
                    .map(GeneratedShader::pass_spec)
                    .collect();
                let mut spec = GridActionSpec::with_passes(id, passes);
                spec.rerun_cost = *rerun_cost;
                spec
            })
            .collect();
        spec.counters = parts.counters;
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
            TopologyHint::GRID,
            spec.structure(),
        );
        declarations.stats = parts.stats.clone();
        declarations.actions = parts.actions.clone();
        declarations.palette = parts.palette.clone().map(Cow::Owned);
        ModelEntry::from_gpu_grid_spec(declarations, parts.source.clone(), spec)
    }
}

/// Shaders and the buffer plan of one graph.
pub(crate) struct Compiled {
    pub shaders: Vec<GeneratedShader>,
    pub lanes: Vec<LanePlan>,
    pub tick_buffer: bool,
    /// Positions of the dual lanes.
    pub dual: Vec<usize>,
}

/// Returns whether an instruction of `block`, or of a region body in it, meets `test`.
pub(crate) fn any_instruction(block: &[Instruction], test: &impl Fn(&Operation) -> bool) -> bool {
    block.iter().any(|instruction| {
        test(&instruction.operation)
            || match &instruction.operation {
                Operation::Repeat(repeat) => any_instruction(&repeat.body, test),
                Operation::GridFold(fold) => any_instruction(&fold.body, test),
                Operation::RadiusFold(fold) => any_instruction(&fold.body, test),
                _ => false,
            }
    })
}

/// One shader the compiler writes, before it writes it.
struct ShaderPlan {
    role: ShaderRole,
    label: String,
    /// Document positions of the passes it runs.
    positions: Vec<usize>,
    emit_role: Role,
    sides: SideTable,
    pass_id: GridPassId,
    cost: u64,
}

/// Writes and composes every shader of `document`, with `outer_iteration` as the iteration word around every pass's
/// top for a test.
///
/// # Errors
///
/// Returns a diagnostic for each node no shader can run, and each problem of composition.
#[expect(
    clippy::too_many_lines,
    reason = "the buffer plan, the side tables and the shader list read in order"
)]
pub(crate) fn compile_shaders(
    document: &GraphDocument,
    report: &CheckReport,
    evaluator: &HostEvaluator,
    outer_iteration: Option<u32>,
) -> Result<Compiled, Vec<CodeDiagnostic>> {
    let mut problems = Vec::new();
    let lanes: Vec<LanePlan> = document
        .lanes
        .iter()
        .map(|lane| {
            if lane.domain != Domain::Cells {
                problems.push(CodeDiagnostic::error(format!(
                    "the lane '{}' lives on {}, and a grid's lanes live on cells",
                    lane.id,
                    lane.domain.as_str()
                )));
            }
            LanePlan {
                id: lane.id.clone(),
                label: format!("lane_{}_cells", lane.id),
                wgsl: WgslType::of(&lane.value_type),
                merged: None,
            }
        })
        .collect();
    let runs = |stage: PassStage| {
        document
            .passes
            .iter()
            .enumerate()
            .filter(move |(_, pass)| pass.stage == stage && pass.enabled != PassEnabled::Literal(false))
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
                problems.push(CodeDiagnostic::error(format!(
                    "the action '{}' reruns the tick pass '{run}', and the GPU runs an action's reruns on the host, \
                     which runs init passes alone",
                    action.id
                ))
                .with_pass(run.clone()));
            }
        }
    }
    let ticks: Vec<usize> = runs(PassStage::Tick).collect();
    let gpu_passes: Vec<usize> = [PassStage::Tick, PassStage::View, PassStage::Stats, PassStage::Action]
        .into_iter()
        .flat_map(runs)
        .collect();
    let tick_buffer = gpu_passes.iter().any(|&position| {
        program(position).is_some_and(|program| {
            let on_tick = document.passes[position].stage == PassStage::Tick;
            [&program.prelude, &program.element].into_iter().any(|block| {
                any_instruction(block, &|operation| {
                    matches!(operation, Operation::Tick) || (on_tick && matches!(operation, Operation::Draw { .. }))
                })
            })
        })
    });
    // A dual lane's writer is the one tick pass that writes it. A lane whose writer never runs is plain.
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
    let step_count = if ticks.is_empty() && tick_buffer {
        1
    } else {
        ticks.len()
    };
    let buffer_count = lanes.len() + usize::from(tick_buffer);
    let step_sides = |step: usize| {
        let mut rows = vec![SideRow::new(BufferSide::Current, BufferSide::Current); buffer_count];
        for &(lane, writer) in &dual {
            rows[lane] = match step.cmp(&writer) {
                std::cmp::Ordering::Less => SideRow::new(BufferSide::Current, BufferSide::Current),
                std::cmp::Ordering::Equal => SideRow::new(BufferSide::Current, BufferSide::Next),
                std::cmp::Ordering::Greater => SideRow::new(BufferSide::Next, BufferSide::Next),
            };
        }
        if tick_buffer && step + 1 == step_count {
            rows[lanes.len()] = SideRow::new(BufferSide::Current, BufferSide::Next);
        }
        SideTable::new(rows)
    };
    let input = EmitInput {
        document,
        report,
        evaluator,
        lanes: &lanes,
        tick_buffer,
        outer_iteration,
        agents: None,
    };
    let cost_of = |positions: &[usize]| {
        positions
            .iter()
            .filter_map(|&position| report.passes.get(position))
            .map(|pass| pass.cost)
            .fold(0u64, u64::saturating_add)
    };
    let mut plans: Vec<ShaderPlan> = Vec::new();
    if ticks.is_empty() && tick_buffer {
        plans.push(ShaderPlan {
            role: ShaderRole::Step,
            label: "advance_tick".to_owned(),
            positions: Vec::new(),
            emit_role: Role::Step { writes_tick: true },
            sides: step_sides(0),
            pass_id: GridPassId::Step(0),
            cost: 1,
        });
    }
    for (step, &position) in ticks.iter().enumerate() {
        plans.push(ShaderPlan {
            role: ShaderRole::Step,
            label: document.passes[position].id.clone(),
            positions: vec![position],
            emit_role: Role::Step {
                writes_tick: tick_buffer && step + 1 == step_count,
            },
            sides: step_sides(step),
            pass_id: GridPassId::Step(step),
            cost: cost_of(&[position]),
        });
    }
    let views: Vec<usize> = runs(PassStage::View).collect();
    let views_cost = cost_of(&views).max(1);
    plans.push(ShaderPlan {
        role: ShaderRole::Display,
        label: "display".to_owned(),
        positions: views,
        emit_role: Role::Display,
        sides: SideTable::in_place(buffer_count),
        pass_id: GridPassId::Display,
        cost: views_cost,
    });
    let stats: Vec<usize> = runs(PassStage::Stats).collect();
    let stats_cost = cost_of(&stats).max(1);
    plans.push(ShaderPlan {
        role: ShaderRole::Reduce,
        label: "reduce".to_owned(),
        positions: stats,
        emit_role: Role::Reduce,
        sides: SideTable::in_place(buffer_count),
        pass_id: GridPassId::Reduce,
        cost: stats_cost,
    });
    for (action_index, action) in document.actions.iter().enumerate() {
        let passes = runs(PassStage::Action)
            .filter(|&position| document.passes[position].action.as_deref() == Some(action.id.as_str()));
        for (pass_index, position) in passes.enumerate() {
            plans.push(ShaderPlan {
                role: ShaderRole::Action(action.id.clone()),
                label: document.passes[position].id.clone(),
                positions: vec![position],
                emit_role: Role::Action,
                sides: SideTable::in_place(buffer_count),
                pass_id: GridPassId::Action {
                    action: action_index,
                    pass: pass_index,
                },
                cost: cost_of(&[position]),
            });
        }
    }
    let mut shaders = Vec::with_capacity(plans.len());
    for ShaderPlan {
        role,
        label,
        positions,
        emit_role,
        sides,
        pass_id,
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
            counters,
            ..
        } = emitted;
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
                .map(|&position| document.passes[position].id.clone())
                .collect(),
            label,
            text,
            composed,
            ranges,
            bindings,
            uniform,
            sides,
            cost,
            counters,
            pass_id,
            agent: None,
        });
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Compiled {
        shaders,
        lanes,
        tick_buffer,
        dual: dual.into_iter().map(|(lane, _)| lane).collect(),
    })
}

/// Returns `problem` placed on the table of its node in `file`, or on its pass's table, when it names them and has no
/// place of its own.
pub(crate) fn place(file: &GraphFile, problem: CodeDiagnostic) -> CodeDiagnostic {
    if problem.span.is_some() {
        return problem;
    }
    let document = file.document();
    let places = file.places();
    let Some(position) = problem
        .pass
        .as_ref()
        .and_then(|pass| document.passes.iter().position(|known| known.id == *pass))
    else {
        return problem;
    };
    let Some(body) = places.passes.get(position) else {
        return problem;
    };
    let node = problem.node.as_ref().and_then(|node| {
        let index = document.passes[position]
            .body
            .nodes
            .iter()
            .position(|known| known.id.to_string() == *node)?;
        body.nodes.get(index)
    });
    let span = node.map_or_else(|| body.table.clone(), |node| node.table.clone());
    file.place(problem, span)
}

/// Returns the problem of the shader `label`, whose text of `bytes` bytes is past [`MAX_FILE_BYTES`], placed on the
/// pass whose nodes wrote the most of it in `ranges`.
pub(crate) fn oversized(label: &str, bytes: usize, ranges: &[NodeRange]) -> CodeDiagnostic {
    let mut written: Vec<(&str, usize)> = Vec::new();
    for range in ranges {
        let length = range.range.len();
        match written.iter_mut().find(|(pass, _)| *pass == range.pass) {
            Some((_, total)) => *total += length,
            None => written.push((&range.pass, length)),
        }
    }
    let problem = CodeDiagnostic::error(format!(
        "expands to {bytes} bytes of WGSL in the shader '{label}', past the {MAX_FILE_BYTES} one shader may hold"
    ))
    .with_note(
        "each node writes lines of its own, and a grid count writes one test per neighbour. A pass with fewer nodes \
         writes a shorter shader",
    );
    match written.iter().max_by_key(|(_, total)| *total) {
        Some((pass, _)) => problem.with_pass(*pass),
        None => problem,
    }
}

/// Note on a problem of a generated shader's composition.
const COMPILER_FAULT_NOTE: &str = "in WGSL the compiler generated, which is a fault of the compiler";

/// Returns `text` composed with the shared modules and `henad_graph::draw`, or its problems, each against the node
/// that wrote the line it is placed on.
pub(crate) fn compose(label: &str, text: &str, ranges: &[NodeRange]) -> Result<String, Vec<CodeDiagnostic>> {
    compose_wgsl(
        WgslFile { path: label, text },
        &[WgslFile {
            path: "henad_graph draw module",
            text: DRAW_WGSL,
        }],
    )
    .map(henad_compute::gpu::compose::ComposedWgsl::into_text)
    .map_err(|problems| {
        problems
            .into_iter()
            .map(|mut problem| {
                let place = problem.span.as_ref().map(|span| span.start);
                problem.file = None;
                problem.span = None;
                problem.line = None;
                problem.column = None;
                let node = place.and_then(|offset| {
                    ranges
                        .iter()
                        .filter(|range| range.range.contains(&offset))
                        .min_by_key(|range| range.range.len())
                });
                if let Some(range) = node {
                    problem = problem.with_pass(range.pass.clone()).with_node(range.node.to_string());
                }
                problem.with_note(format!("{COMPILER_FAULT_NOTE}, in the shader '{label}'"))
            })
            .collect()
    })
}

/// Returns the source of each `[[stat]]`: the stats pass that holds its node, the slot of its port, and the pass's
/// first counter in the reduce shader.
fn stat_sources(
    document: &GraphDocument,
    evaluator: &HostEvaluator,
    reduce: Option<&GeneratedShader>,
) -> Vec<StatSource> {
    let mut first_counters = Vec::new();
    let mut counter = 0;
    for id in reduce.map(GeneratedShader::passes).unwrap_or_default() {
        if let Some(position) = document.passes.iter().position(|pass| pass.id == *id) {
            first_counters.push((position, counter));
            if let Some(Ok(program)) = evaluator.program_at(position) {
                counter += program.tallies;
            }
        }
    }
    document
        .stats
        .iter()
        .map(|stat| {
            let found = first_counters.iter().find_map(|&(position, first)| {
                let Some(Ok(program)) = evaluator.program_at(position) else {
                    return None;
                };
                let slot = if let Some(port) = &stat.from.port {
                    program.outputs.get(&(stat.from.node, port.clone())).copied()
                } else {
                    let mut ports = program.outputs.iter().filter(|((node, _), _)| *node == stat.from.node);
                    let only = ports.next().map(|(_, slot)| *slot);
                    if ports.next().is_some() { None } else { only }
                }?;
                Some(StatSource {
                    position: Some(position),
                    slot,
                    first_counter: first,
                })
            });
            found.unwrap_or(StatSource {
                position: None,
                slot: 0,
                first_counter: 0,
            })
        })
        .collect()
}

/// Returns the palette the display shows when one palette colours every cell: that of the only `view.color` node, or
/// of the first category lane when there is no view pass. `None` otherwise.
fn display_palette(document: &GraphDocument) -> Option<Vec<[u8; 4]>> {
    let views: Vec<_> = document
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::View && pass.enabled != PassEnabled::Literal(false))
        .collect();
    let colors_of = |id: &str| {
        document
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
            })
    };
    if views.is_empty() {
        let category = document.lanes.iter().find_map(|lane| match &lane.value_type {
            crate::types::ValueType::Category(category) => Some(category.clone()),
            _ => None,
        })?;
        return colors_of(&category);
    }
    let mut palettes = views
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| node.op == "view.color")
        .filter_map(|node| node.config.get("palette").and_then(crate::value::ConfigValue::as_str));
    match (palettes.next(), palettes.next()) {
        (Some(id), None) => colors_of(id),
        _ => None,
    }
}
