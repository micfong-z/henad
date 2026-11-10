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
//! pass that writes a *dual* lane, one another element reads. Past the budget of
//! [`MAX_STORAGE_BINDINGS`](crate::limits::MAX_STORAGE_BINDINGS) storage buffers, lanes share a packed buffer
//! `lanes_<domain>_<k>` of `u32` words, read and written through `bitcast` at each lane's segment, as [`LaneBuffer`]
//! describes. Names the template owns start with `henad_`.
//!
//! *Uniform block.* A pass's prelude, the values the same for every element, runs on the host when the model is built,
//! and the shader reads the values in the uniform block `params`, with the grid's size, each draw's key word and the
//! press count of an action. A prelude value that reads the tick is computed in the shader, which reads the tick from a
//! one-word buffer that the last tick pass advances.
//!
//! *Regions.* A Repeat is a counted `for` loop that the compiler writes, with its accumulators as `var`s before it. A
//! grid count is unrolled over its stencil, one test per neighbour.
//!
//! *Probes.* [`GridProgram::compile_with_probes`] takes [`Probes`] beside the file, Boolean outputs of tick passes
//! whose fires a step counts on the engine's counted step. The counts sit in `counters` after the reduce tallies, in
//! [`GpuGridSpec::fire_counters`].
//!
//! *Fault names.* [`AgentProgram::compile_with_fault_names`] takes [`FaultNames`] beside the file, the words that name
//! each radius fold in the document the graph was lowered from. A fold's fault names those words.

pub(crate) mod agent_hooks;
pub(crate) mod agents;
pub(crate) mod emit;
mod fault_names;
mod hooks;
mod live;
pub(crate) mod pack;
pub(crate) mod probes;
pub(crate) mod uniform;

pub use agents::AgentProgram;
pub use fault_names::FaultNames;
pub use pack::LaneBuffer;
pub use probes::Probes;

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use henad_compute::entry::{EntryDeclarations, ModelEntry};
use henad_compute::gpu::compose::{MAX_FILE_BYTES, WgslFile, compose_wgsl};
use henad_compute::gpu::{
    AgentPassId, BufferSide, GpuGridSpec, GridActionSpec, GridBufferSpec, GridPassId, GridPassSpec, ShaderBinding,
    ShaderSpec, SideRow, SideTable,
};
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::gpu_agent_model::Domain as EngineDomain;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::u32_param;
use henad_core::metadata::{Structure, TimeScale};
use henad_core::params::ParamDescriptor;
use henad_core::provenance::ModelSource;
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

use crate::check::CheckReport;
use crate::document::{Action, Domain, GraphDocument, PassEnabled, PassStage, World};
use crate::draw::DRAW_WGSL;
use crate::file::GraphFile;
use crate::host::HostEvaluator;
use crate::host::program::{Instruction, Operation, Program, RepeatAccumulator};
#[cfg(test)]
use crate::host::{HostWorld, RunContext};
use crate::ids::NodeId;

use emit::{EmitInput, Emitted, LanePlan, Role, SumLane};
use hooks::{GraphHooks, PassUniform, StatSource, action_plans};
use pack::{LaneEntry, LaneLayout};
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
    /// Document position and sum number of each float lane of a reduce shader, a `vec2f` sum taking two. Empty for
    /// every other shader.
    sum_lanes: Vec<SumLane>,
    /// Pass of the spec the shader is.
    pass: EnginePass,
    /// Bytes of the shader's `var<workgroup>` declarations.
    workgroup_bytes: u64,
}

/// Pass of an engine's spec that a generated shader is, for its uniform block.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EnginePass {
    /// A pass of the grid engine.
    Grid(GridPassId),
    /// A pass of the agents engine, run over `domain`.
    Agents { pass: AgentPassId, domain: EngineDomain },
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
        innermost(&self.ranges, offset).map(|range| (range.pass.as_str(), range.node))
    }

    /// Cost of one invocation in work units, the checker's cost of the passes it runs.
    pub fn cost(&self) -> u64 {
        self.cost
    }

    /// Invocation domain of a shader of an agents world, `None` for a shader of a grid.
    pub(crate) fn domain(&self) -> Option<EngineDomain> {
        match self.pass {
            EnginePass::Agents { domain, .. } => Some(domain),
            EnginePass::Grid(_) => None,
        }
    }

    /// Uniform block of the shader.
    #[cfg(test)]
    pub(crate) fn uniform(&self) -> &UniformBlock {
        &self.uniform
    }

    /// Bindings of the shader in `@binding` order.
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

    /// Document position and sum number of each float lane of a reduce shader.
    #[cfg(test)]
    pub(crate) fn sum_lanes(&self) -> &[SumLane] {
        &self.sum_lanes
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
        pass.workgroup_bytes = self.workgroup_bytes;
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
    declarations: ProgramDeclarations,
    params: Vec<ParamDescriptor>,
    palette: Option<Vec<[u8; 4]>>,
    buffers: Vec<GridBufferSpec>,
    layout: LaneLayout,
    shaders: Vec<GeneratedShader>,
    hooks: GraphHooks,
    counters: usize,
    /// Words of `counters` that hold the counts of the probes.
    fire_counters: Range<usize>,
}

/// Declarations a compiled graph on a grid and one of agents read alike from the document.
struct ProgramDeclarations {
    /// Id of the model, the id of the graph's target.
    id: String,
    name: String,
    description: String,
    source: ModelSource,
    time: Option<TimeScale>,
    stats: Vec<StatDescriptor>,
    actions: Vec<ActionDescriptor>,
    /// Cost of the enabled init passes.
    init_cost: u64,
    /// Cost of the init passes each action reruns, in the order of the actions.
    rerun_costs: Vec<u64>,
}

impl ProgramDeclarations {
    /// Returns the declarations of `document`, compiled from `file`, with the pass costs of its check `report`.
    ///
    /// The target's name and description stand in for the graph's own when they are set.
    fn read(file: &GraphFile, document: &GraphDocument, report: &CheckReport) -> Self {
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
        let cost_at = |position: usize| report.passes.get(position).map_or(0, |pass| pass.cost);
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
                rerun_positions(document, action)
                    .map(cost_at)
                    .fold(0u64, u64::saturating_add)
            })
            .collect();
        Self {
            id: target.id.clone(),
            name: name.clone(),
            description: description.clone(),
            source: file.source(),
            time: document.time.clone(),
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
            init_cost,
            rerun_costs,
        }
    }

    /// Returns the declarations of the entry with the parameters `params`, of a spec of `topology` and `structure`.
    fn entry_declarations(
        &self,
        params: Vec<ParamDescriptor>,
        topology: TopologyHint,
        structure: Structure,
    ) -> EntryDeclarations {
        let mut declarations = EntryDeclarations::new(
            self.id.clone(),
            self.name.clone(),
            self.description.clone(),
            params,
            topology,
            structure,
        );
        declarations.stats = self.stats.clone();
        declarations.actions = self.actions.clone();
        declarations.time = self.time.clone();
        declarations
    }
}

/// Returns the positions in `document` of the init passes that `action` reruns, in the order of its runs.
fn rerun_positions<'a>(document: &'a GraphDocument, action: &'a Action) -> impl Iterator<Item = usize> + 'a {
    action.runs.iter().filter_map(|run| {
        document
            .passes
            .iter()
            .position(|pass| pass.id == *run && pass.stage == PassStage::Init)
    })
}

/// Prints the id and the shaders.
impl fmt::Debug for GridProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GridProgram")
            .field("id", &self.parts.declarations.id)
            .field("shaders", &self.parts.shaders)
            .finish_non_exhaustive()
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
    pub fn compile(file: &GraphFile) -> Result<Self, Vec<CodeDiagnostic>> {
        Self::compile_with_probes(file, &Probes::default())
    }

    /// Checks `file` and compiles every pass the GPU runs, each step counting the fires of its probes in `probes`.
    ///
    /// The probes change the shaders and the spec's [`GpuGridSpec::fire_counters`], and leave the file's text and its
    /// code identity alone.
    ///
    /// # Errors
    ///
    /// Returns the problems of [`Self::compile`], and a problem for each probe that names no `bool` output of a tick
    /// pass.
    pub fn compile_with_probes(file: &GraphFile, probes: &Probes) -> Result<Self, Vec<CodeDiagnostic>> {
        let report = file.check();
        if report.has_errors() {
            return Err(report.errors().cloned().collect());
        }
        let document = file.document();
        let evaluator = HostEvaluator::from_document(document).map_err(|error| error.diagnostics())?;
        let World::Grid(grid) = document.world else {
            return Err(vec![CodeDiagnostic::error(
                "the graph models agents, and `GridProgram` compiles graphs on a grid. Compile it with \
                 `AgentProgram`",
            )]);
        };
        let compiled = compile_shaders(document, &report, &evaluator, None, probes).map_err(|problems| {
            problems
                .into_iter()
                .map(|problem| place(file, problem))
                .collect::<Vec<_>>()
        })?;
        let evaluator = Arc::new(evaluator);
        let Compiled {
            shaders,
            layout,
            tick_buffer,
            fire_counters,
        } = compiled;

        let mut buffers: Vec<GridBufferSpec> = layout
            .groups
            .iter()
            .map(|group| GridBufferSpec::new(group.label.clone(), group.dual))
            .collect();
        if tick_buffer {
            buffers.push(GridBufferSpec::new("henad_tick", true));
        }

        let uniforms = shaders
            .iter()
            .filter_map(|shader| match shader.pass {
                EnginePass::Grid(id) => Some(PassUniform {
                    id,
                    block: shader.uniform.clone(),
                }),
                EnginePass::Agents { .. } => None,
            })
            .collect();
        let reduce = shaders.iter().find(|shader| shader.role == ShaderRole::Reduce);
        debug_assert_eq!(
            reduce.map_or(1, GeneratedShader::counters),
            fire_counters.start,
            "the probes' counts follow the reduce tallies"
        );
        let counters = fire_counters.end;
        let sum_lanes = reduce.map(|shader| shader.sum_lanes.clone()).unwrap_or_default();
        let stats = stat_sources(
            document,
            &evaluator,
            reduce.map(GeneratedShader::passes).unwrap_or_default(),
        );
        let hooks = GraphHooks {
            evaluator: Arc::clone(&evaluator),
            width: grid.width.clamp(1, MAX_GRID_SIDE),
            height: grid.height.clamp(1, MAX_GRID_SIDE),
            boundary: grid.boundary,
            lane_buffers: layout.groups.iter().map(|group| group.lanes.clone()).collect(),
            tick_buffer,
            uniforms,
            stats,
            sum_lanes,
            actions: action_plans(document),
        };

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
        params.extend(own_params(&document.params, &live::reload_only(&evaluator)));
        Ok(Self {
            parts: Arc::new(GridParts {
                declarations: ProgramDeclarations::read(file, document, &report),
                params,
                palette: display_palette(document),
                buffers,
                layout,
                shaders,
                hooks,
                counters,
                fire_counters,
            }),
        })
    }

    /// Id of the model, the id of the graph's target.
    pub fn id(&self) -> &str {
        &self.parts.declarations.id
    }

    /// Every shader, step passes first, then the display and reduce passes, then each action's passes.
    pub fn shaders(&self) -> &[GeneratedShader] {
        &self.parts.shaders
    }

    /// Returns the buffer and segment that hold the lane `id` on the device, `None` for a lane the graph lacks.
    pub fn lane_buffer(&self, id: &str) -> Option<LaneBuffer> {
        self.parts.layout.place_of(self.parts.hooks.evaluator.document(), id)
    }

    /// Returns the spec the GPU grid engine runs.
    pub fn spec(&self) -> GpuGridSpec {
        let parts = &*self.parts;
        let declarations = &parts.declarations;
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
            declarations.id.clone(),
            WORKGROUP_SIZE,
            parts.buffers.clone(),
            steps,
            display,
            reduce,
            declarations.stats.clone(),
            Box::new(parts.hooks.clone()),
        );
        spec.actions = declarations
            .actions
            .iter()
            .zip(&declarations.rerun_costs)
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
        spec.fire_counters = parts.fire_counters.clone();
        spec.sums = parts.hooks.sum_lanes.len();
        spec.init_cost = declarations.init_cost;
        spec
    }

    /// Returns the entry that builds the model on any device passed to its build. Its source records the graph's
    /// semantic text as its code.
    pub fn entry(&self) -> ModelEntry {
        let parts = &*self.parts;
        let spec = Arc::new(self.spec());
        let mut declarations =
            parts
                .declarations
                .entry_declarations(parts.params.clone(), TopologyHint::GRID, spec.structure());
        declarations.palette = parts.palette.clone().map(Cow::Owned);
        ModelEntry::from_gpu_grid_spec(declarations, parts.declarations.source.clone(), spec)
    }
}

/// Returns `params`, the graph's own parameters, each marked reload-only where `reload` holds at its position and live
/// otherwise.
pub(crate) fn own_params(params: &[ParamDescriptor], reload: &[bool]) -> Vec<ParamDescriptor> {
    params
        .iter()
        .enumerate()
        .map(|(position, descriptor)| {
            let descriptor = descriptor.clone();
            if reload.get(position).copied().unwrap_or(true) {
                descriptor.on_reload()
            } else {
                descriptor
            }
        })
        .collect()
}

/// Shaders and the buffer plan of one graph.
pub(crate) struct Compiled {
    pub shaders: Vec<GeneratedShader>,
    /// Lane buffers and the place of each lane in them.
    pub layout: LaneLayout,
    pub tick_buffer: bool,
    /// Words of `counters` that hold the probes' counts, after the reduce tallies.
    pub fire_counters: Range<usize>,
}

/// Returns whether an instruction of `block`, or of a region body in it, meets `test`.
pub(crate) fn any_instruction(block: &[Instruction], test: &impl Fn(&Operation) -> bool) -> bool {
    block.iter().any(|instruction| {
        test(&instruction.operation)
            || region(&instruction.operation).is_some_and(|(_, body)| any_instruction(body, test))
    })
}

/// Returns the accumulators and the body of a region's `operation`, `None` for an operation of no region.
fn region(operation: &Operation) -> Option<(&[RepeatAccumulator], &[Instruction])> {
    match operation {
        Operation::Repeat(repeat) => Some((&repeat.accumulators, &repeat.body)),
        Operation::GridFold(fold) => Some((&fold.accumulators, &fold.body)),
        Operation::RadiusFold(fold) => Some((&fold.accumulators, &fold.body)),
        _ => None,
    }
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
/// top for a test, each step counting the fires of its probes in `probes`.
///
/// # Errors
///
/// Returns a diagnostic for each node no shader can run, each probe that names no `bool` output of a tick pass, and
/// each problem of composition.
#[expect(
    clippy::too_many_lines,
    reason = "the buffer plan, the side tables and the shader list read in order"
)]
pub(crate) fn compile_shaders(
    document: &GraphDocument,
    report: &CheckReport,
    evaluator: &HostEvaluator,
    outer_iteration: Option<u32>,
    probes: &Probes,
) -> Result<Compiled, Vec<CodeDiagnostic>> {
    let mut problems = Vec::new();
    let probe_slots = probes::resolve(probes, document, evaluator, report).unwrap_or_else(|found| {
        problems.extend(found);
        Vec::new()
    });
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
    let program = |position: usize| compiled_at(evaluator, position);
    refuse_tick_reruns(document, &mut problems);
    let ticks: Vec<usize> = runs(PassStage::Tick).collect();
    let gpu_passes: Vec<usize> = [PassStage::Tick, PassStage::View, PassStage::Stats, PassStage::Action]
        .into_iter()
        .flat_map(runs)
        .collect();
    let tick_buffer = reads_tick(document, evaluator, &gpu_passes);
    let dual = dual_writers(document, report, evaluator, &ticks);
    let step_count = if ticks.is_empty() && tick_buffer {
        1
    } else {
        ticks.len()
    };
    // The probes' counts follow the reduce tallies, at least one word, in `counters`.
    let reduce_tallies = runs(PassStage::Stats)
        .filter_map(program)
        .map(|program| program.tallies)
        .sum::<usize>()
        .max(1);
    let reruns = rerun_writers(document, evaluator);
    let cells = u64::from(grid_cells(document));
    let entries: Vec<LaneEntry<'_>> = document
        .lanes
        .iter()
        .enumerate()
        .map(|(position, lane)| LaneEntry {
            label: lanes[position].label.clone(),
            value_type: &lane.value_type,
            domain: lane.domain,
            writer: dual
                .iter()
                .find(|(known, _)| *known == position)
                .map(|(_, writer)| *writer),
            held: true,
            packable: lanes[position].wgsl.is_some(),
            reruns: reruns[position].clone(),
            largest_count: cells,
        })
        .collect();
    let cost_of = |positions: &[usize]| summed_cost(report, positions);
    let views: Vec<usize> = runs(PassStage::View).collect();
    let stats: Vec<usize> = runs(PassStage::Stats).collect();
    let emit_all = |layout: &LaneLayout| -> Result<Vec<GeneratedShader>, Vec<CodeDiagnostic>> {
        let buffer_count = layout.groups.len() + usize::from(tick_buffer);
        let step_sides = |step: usize| {
            let mut rows = dual_rows(layout, &dual, step, buffer_count);
            if tick_buffer && step + 1 == step_count {
                rows[layout.groups.len()] = SideRow::new(BufferSide::Current, BufferSide::Next);
            }
            SideTable::new(rows)
        };
        let input = EmitInput {
            document,
            report,
            evaluator,
            lanes: &lanes,
            layout,
            tick_buffer,
            outer_iteration,
            agents: None,
            probes: &probe_slots,
            fire_base: reduce_tallies,
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
        plans.push(ShaderPlan {
            role: ShaderRole::Display,
            label: "display".to_owned(),
            positions: views.clone(),
            emit_role: Role::Display,
            sides: SideTable::in_place(buffer_count),
            pass_id: GridPassId::Display,
            cost: cost_of(&views).max(1),
        });
        plans.push(ShaderPlan {
            role: ShaderRole::Reduce,
            label: "reduce".to_owned(),
            positions: stats.clone(),
            emit_role: Role::Reduce,
            sides: SideTable::in_place(buffer_count),
            pass_id: GridPassId::Reduce,
            cost: cost_of(&stats).max(1),
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
        let mut problems = Vec::new();
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
            let (emitted, composed) = match write_shader(&input, emit_role, &positions, &sides, &label) {
                Ok(written) => written,
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
                sum_lanes,
                workgroup_bytes,
            } = emitted;
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
                sum_lanes,
                pass: EnginePass::Grid(pass_id),
                workgroup_bytes,
            });
        }
        if problems.is_empty() {
            Ok(shaders)
        } else {
            Err(problems)
        }
    };
    if !problems.is_empty() {
        if let Err(found) = emit_all(&LaneLayout::unpacked(&entries)) {
            problems.extend(found);
        }
        return Err(problems);
    }
    let (layout, shaders) = pack::choose(&entries, emit_all)?;
    Ok(Compiled {
        shaders,
        layout,
        tick_buffer,
        fire_counters: reduce_tallies..reduce_tallies + probes.len(),
    })
}

/// Returns, for each lane of `document` by position, the positions of the actions whose reruns write it, in order.
pub(crate) fn rerun_writers(document: &GraphDocument, evaluator: &HostEvaluator) -> Vec<Vec<usize>> {
    let mut writers = vec![Vec::new(); document.lanes.len()];
    for (action_index, action) in document.actions.iter().enumerate() {
        for position in rerun_positions(document, action) {
            let Some(program) = compiled_at(evaluator, position) else {
                continue;
            };
            for &lane in &program.written {
                if let Some(actions) = writers.get_mut(lane)
                    && actions.last() != Some(&action_index)
                {
                    actions.push(action_index);
                }
            }
        }
    }
    writers
}

/// Records a problem in `problems` for each run of an action of `document` that names a tick pass.
fn refuse_tick_reruns(document: &GraphDocument, problems: &mut Vec<CodeDiagnostic>) {
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
}

/// Returns whether a pass of `document` at `positions` reads the tick on the GPU, or draws in a tick pass.
fn reads_tick(document: &GraphDocument, evaluator: &HostEvaluator, positions: &[usize]) -> bool {
    positions.iter().any(|&position| {
        let Some(program) = compiled_at(evaluator, position) else {
            return false;
        };
        let on_tick = document.passes[position].stage == PassStage::Tick;
        [&program.prelude, &program.element].into_iter().any(|block| {
            any_instruction(block, &|operation| {
                matches!(operation, Operation::Tick) || (on_tick && matches!(operation, Operation::Draw { .. }))
            })
        })
    })
}

/// Returns each dual lane of `document` by position, with the position among `ticks` of the tick pass that writes it.
///
/// A dual lane's writer is the one tick pass that writes it. A lane whose writer never runs is plain.
fn dual_writers(
    document: &GraphDocument,
    report: &CheckReport,
    evaluator: &HostEvaluator,
    ticks: &[usize],
) -> Vec<(usize, usize)> {
    document
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| report.dual_lanes.contains(&lane.id))
        .filter_map(|(lane, _)| {
            ticks
                .iter()
                .position(|&position| {
                    compiled_at(evaluator, position).is_some_and(|program| program.written.contains(&lane))
                })
                .map(|step| (lane, step))
        })
        .collect()
}

/// Returns the side row of each of `buffer_count` buffers at the step `step`, the lane buffers of `layout` holding a
/// lane of `dual` read and written as the lane's writer step requires, and every other buffer in place.
fn dual_rows(layout: &LaneLayout, dual: &[(usize, usize)], step: usize, buffer_count: usize) -> Vec<SideRow> {
    let mut rows = vec![SideRow::new(BufferSide::Current, BufferSide::Current); buffer_count];
    for &(lane, writer) in dual {
        let Some(place) = layout.place(lane) else {
            continue;
        };
        rows[place.buffer()] = match step.cmp(&writer) {
            std::cmp::Ordering::Less => SideRow::new(BufferSide::Current, BufferSide::Current),
            std::cmp::Ordering::Equal => SideRow::new(BufferSide::Current, BufferSide::Next),
            std::cmp::Ordering::Greater => SideRow::new(BufferSide::Next, BufferSide::Next),
        };
    }
    rows
}

/// Returns the sum of the checker's costs in `report` of the passes at `positions`.
fn summed_cost(report: &CheckReport, positions: &[usize]) -> u64 {
    positions
        .iter()
        .filter_map(|&position| report.passes.get(position))
        .map(|pass| pass.cost)
        .fold(0u64, u64::saturating_add)
}

/// Writes the shader `label` of `role` over the passes at `positions`, bound through `sides`, and composes it.
///
/// # Errors
///
/// Returns the problems of the emission, of a text past [`MAX_FILE_BYTES`], or of the composition.
fn write_shader(
    input: &EmitInput<'_>,
    role: Role,
    positions: &[usize],
    sides: &SideTable,
    label: &str,
) -> Result<(Emitted, String), Vec<CodeDiagnostic>> {
    let emitted = emit::emit(input, role, positions, sides)?;
    if emitted.text.len() > MAX_FILE_BYTES {
        return Err(vec![oversized(label, emitted.text.len(), &emitted.ranges)]);
    }
    let composed = compose(label, &emitted.text, &emitted.ranges)?;
    Ok((emitted, composed))
}

/// Returns the program of the pass at `position` of the document `evaluator` runs, `None` for a pass the host cannot
/// compile.
fn compiled_at(evaluator: &HostEvaluator, position: usize) -> Option<&Program> {
    evaluator.program_at(position)?.as_ref().ok()
}

/// Returns the innermost range of `ranges` that holds the byte at `offset`.
fn innermost(ranges: &[NodeRange], offset: usize) -> Option<&NodeRange> {
    ranges
        .iter()
        .filter(|range| range.range.contains(&offset))
        .min_by_key(|range| range.range.len())
}

/// Returns the cells of the default grid of `document`, 1 for a document of agents.
fn grid_cells(document: &GraphDocument) -> u32 {
    match document.world {
        World::Grid(grid) => grid
            .width
            .clamp(1, MAX_GRID_SIDE)
            .saturating_mul(grid.height.clamp(1, MAX_GRID_SIDE)),
        World::Agents(_) => 1,
    }
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
                let node = place.and_then(|offset| innermost(ranges, offset));
                if let Some(range) = node {
                    problem = problem.with_pass(range.pass.clone()).with_node(range.node.to_string());
                }
                problem.with_note(format!("{COMPILER_FAULT_NOTE}, in the shader '{label}'"))
            })
            .collect()
    })
}

/// Returns the source of each `[[stat]]` of `document`: the first of the stats passes `reduce_passes`, in the order
/// of the reduce shader, that holds its node, the slot of its port, and the pass's first tally in the reduce shader.
/// `None` for a stat no pass of them holds.
fn stat_sources(
    document: &GraphDocument,
    evaluator: &HostEvaluator,
    reduce_passes: &[String],
) -> Vec<Option<StatSource>> {
    let mut first_tallies = Vec::new();
    let mut tally = 0;
    for id in reduce_passes {
        if let Some(position) = document.passes.iter().position(|pass| pass.id == *id) {
            first_tallies.push((position, tally));
            if let Some(program) = compiled_at(evaluator, position) {
                tally += program.tallies;
            }
        }
    }
    document
        .stats
        .iter()
        .map(|stat| {
            first_tallies.iter().find_map(|&(position, first_tally)| {
                let program = compiled_at(evaluator, position)?;
                let (_, slot) = program.port_slot(&stat.from)?;
                Some(StatSource {
                    position,
                    slot,
                    first_tally,
                })
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
