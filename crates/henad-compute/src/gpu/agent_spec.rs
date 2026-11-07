//! Declarations of a GPU agent model held as values. The engine in [`super::agent_engine`] builds its buffers, pipelines
//! and bind groups from them.
//!
//! A [`GpuAgentSpec`] holds a model's buffers and passes, borrowed from its consts or owned, and its functions behind
//! [`AgentHooks`]. [`GpuAgentSpec::from_model`] reads a spec from a model type, and [`GpuAgentSpec::new`] builds one from
//! values for a model composed at run time.
//!
//! A pass binds through a [`SideTable`], which says for each buffer the side its reads and its writes reach. The
//! double buffered buffers swap once, at the end of the step, whatever the tables say.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Range;

use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{
    BufferSpec, Domain, Geometry, GpuAgentModel, PassCtx, PassId, PassSpec,
};
use henad_core::metadata::{GpuBufferInfo, GpuPassInfo, Structure};
use henad_core::params::ParamValue;
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatValue};

use crate::display_scale::display_dims;
use crate::gpu::capacity::{Demand, PassKind, storage_bindings};
use crate::gpu::contracts::{assert_buffer_labels, assert_workgroup_size};
use crate::gpu::primitives::dispatch::WORKGROUP;
use crate::gpu::primitives::spatial_hash::HashGrid;
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sides::{BufferSide, SideTable};

/// One storage buffer of a model's state, the form of [`BufferSpec`] that can own its label.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentBufferSpec {
    /// Name that a shader's bindings refer to. It must not be reserved or end in `_in` or `_out`.
    pub label: Cow<'static, str>,
    /// Whether the buffer has a second side. A step pass must write the next side, since the sides swap every step.
    pub double_buffered: bool,
    /// Whether the buffer is also a vertex stream, for the view to draw without a copy.
    pub drawable: bool,
}

impl AgentBufferSpec {
    /// Creates the declaration of buffer `label`.
    pub fn new(label: impl Into<Cow<'static, str>>, double_buffered: bool, drawable: bool) -> Self {
        Self {
            label: label.into(),
            double_buffered,
            drawable,
        }
    }
}

impl From<&BufferSpec> for AgentBufferSpec {
    fn from(spec: &BufferSpec) -> Self {
        Self::new(spec.label, spec.double_buffered, spec.drawable)
    }
}

/// One compute pass of a step, the form of [`PassSpec`] that can own its strings, with its [`SideTable`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentPassSpec {
    /// Name of the pass, part of its GPU labels.
    pub label: Cow<'static, str>,
    /// Shader of the pass, which declares `@workgroup_size(256)` and folds its index with
    /// `henad::dispatch::linear_index`.
    pub shader: ShaderSpec,
    /// Invocation domain of the pass.
    pub domain: Domain,
    /// Sides the pass binds.
    pub sides: SideTable,
    /// Cost of one invocation in work units, as a graph model's checker counts it. Zero when the cost is unknown, as
    /// for a hand-written shader, and [`Self::new`] leaves it at zero.
    pub cost: u64,
    /// Size in bytes of the uniform block the shader binds. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub uniform_bytes: u64,
    /// Bytes of the shader's `var<workgroup>` declarations. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub workgroup_bytes: u64,
}

impl AgentPassSpec {
    /// Creates pass `label`.
    pub fn new(label: impl Into<Cow<'static, str>>, shader: ShaderSpec, domain: Domain, sides: SideTable) -> Self {
        Self {
            label: label.into(),
            shader,
            domain,
            sides,
            cost: 0,
            uniform_bytes: 0,
            workgroup_bytes: 0,
        }
    }

    /// Returns the pass `spec` declares, in today's single orientation over `buffers` buffers.
    fn from_model(spec: &'static PassSpec, buffers: usize) -> Self {
        Self::new(
            spec.label,
            ShaderSpec::borrowed(spec.shader, spec.bindings),
            spec.domain,
            SideTable::swapping(buffers),
        )
    }
}

/// The pass that turns state into the display texture, dispatched once per texel of [`Geometry::display`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentDisplaySpec {
    /// Shader of the pass, which declares `@workgroup_size(N, N)` for [`Self::workgroup`].
    pub shader: ShaderSpec,
    /// Side `N` of the workgroup.
    pub workgroup: u32,
    /// Sides the pass binds.
    pub sides: SideTable,
    /// Cost of one invocation in work units, as a graph model's checker counts it. Zero when the cost is unknown, as
    /// for a hand-written shader, and [`Self::new`] leaves it at zero.
    pub cost: u64,
    /// Size in bytes of the uniform block the shader binds. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub uniform_bytes: u64,
    /// Bytes of the shader's `var<workgroup>` declarations. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub workgroup_bytes: u64,
}

impl AgentDisplaySpec {
    /// Creates a display pass.
    pub fn new(shader: ShaderSpec, workgroup: u32, sides: SideTable) -> Self {
        Self {
            shader,
            workgroup,
            sides,
            cost: 0,
            uniform_bytes: 0,
            workgroup_bytes: 0,
        }
    }
}

/// The leaf of the stat reduction, which folds the population down to one value per lane into `partials`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentReduceSpec {
    /// Shader of the leaf, which declares `@workgroup_size(256)`.
    pub shader: ShaderSpec,
    /// Number of values that the leaf sums, one per lane.
    pub lanes: usize,
    /// Invocation domain of the leaf.
    pub domain: Domain,
    /// Sides the leaf binds.
    pub sides: SideTable,
    /// Cost of one invocation in work units, as a graph model's checker counts it. Zero when the cost is unknown, as
    /// for a hand-written shader, and [`Self::new`] leaves it at zero.
    pub cost: u64,
    /// Size in bytes of the uniform block the shader binds. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub uniform_bytes: u64,
    /// Bytes of the shader's `var<workgroup>` declarations. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub workgroup_bytes: u64,
}

impl AgentReduceSpec {
    /// Creates a reduce leaf.
    pub fn new(shader: ShaderSpec, lanes: usize, domain: Domain, sides: SideTable) -> Self {
        Self {
            shader,
            lanes,
            domain,
            sides,
            cost: 0,
            uniform_bytes: 0,
            workgroup_bytes: 0,
        }
    }
}

/// A one-off pass the user can trigger, dispatched once over its own domain.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentActionSpec {
    /// Id of the action, part of the labels of its pass and its uniform.
    pub id: Cow<'static, str>,
    /// Shader of the pass, which declares `@workgroup_size(256)`.
    pub shader: ShaderSpec,
    /// Invocation domain of the pass.
    pub domain: Domain,
    /// Sides the pass binds, usually [`SideTable::in_place`], since nothing swaps after an action.
    pub sides: SideTable,
    /// Cost of one invocation in work units, as a graph model's checker counts it. Zero when the cost is unknown, as
    /// for a hand-written shader, and [`Self::new`] leaves it at zero.
    pub cost: u64,
    /// Size in bytes of the uniform block the shader binds. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub uniform_bytes: u64,
    /// Bytes of the shader's `var<workgroup>` declarations. Zero when unknown, as for a hand-written shader, and
    /// [`Self::new`] leaves it at zero.
    pub workgroup_bytes: u64,
    /// Cost per element of the init passes a press reruns on the host before the pass, in work units. Zero when
    /// the press reruns nothing, and [`Self::new`] leaves it at zero.
    pub rerun_cost: u64,
}

impl AgentActionSpec {
    /// Creates action `id`.
    pub fn new(id: impl Into<Cow<'static, str>>, shader: ShaderSpec, domain: Domain, sides: SideTable) -> Self {
        Self {
            id: id.into(),
            shader,
            domain,
            sides,
            cost: 0,
            uniform_bytes: 0,
            workgroup_bytes: 0,
            rerun_cost: 0,
        }
    }
}

/// Pass whose uniform block [`AgentHooks::run_params_bytes`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentPassId {
    /// Entry of [`GpuAgentSpec::steps`] at this index.
    Step(usize),
    /// The [`GpuAgentSpec::color`] pass.
    Color,
    /// The [`GpuAgentSpec::display`] pass.
    Display,
    /// The [`GpuAgentSpec::reduce`] leaf.
    Reduce,
    /// Entry of [`GpuAgentSpec::actions`] at this index.
    Action(usize),
}

impl AgentPassId {
    /// Returns the [`PassId`] a [`GpuAgentModel`] knows this pass by, `None` for the colour pass, which no such model
    /// declares.
    pub fn model_pass(self) -> Option<PassId> {
        match self {
            Self::Step(index) => Some(PassId::Step(index)),
            Self::Color => None,
            Self::Display => Some(PassId::Display),
            Self::Reduce => Some(PassId::Reduce),
            Self::Action(index) => Some(PassId::Action(index)),
        }
    }
}

/// Values of a run that a uniform block, a rerun or the stat values can read besides the parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentRun {
    /// Seed of the run, `None` for the model's fixed default seed.
    pub seed: Option<u64>,
    /// Number of presses of any action so far, the press being run included. Zero before the first press.
    pub presses: u32,
    /// Steps done when the hook runs: zero at construction and at a live edit of a parameter, the engine's tick at a
    /// press, and for the stat values the steps done when the reduce leaf was recorded.
    pub tick: u64,
    /// Whether the block is the counting block of a step pass. The last step of each batch binds it, every other
    /// block holds false, and [`Self::new`] leaves it false.
    pub counting: bool,
}

impl AgentRun {
    /// Creates the values of a run on `seed` after `presses` presses and `tick` steps.
    pub fn new(seed: Option<u64>, presses: u32, tick: u64) -> Self {
        Self {
            seed,
            presses,
            tick,
            counting: false,
        }
    }

    /// Returns the values with [`Self::counting`] set to `counting`.
    pub fn with_counting(self, counting: bool) -> Self {
        Self { counting, ..self }
    }
}

/// Buffers that a rerun on the host wrote, each as its buffer's position in [`GpuAgentSpec::buffers`] and its new
/// words.
pub type AgentUploads = Vec<(usize, Vec<u32>)>;

/// Functions of a GPU agent model that the engine calls, each the counterpart of the [`GpuAgentModel`] function of the
/// same name.
///
/// The engine calls no hook per step or per element. It reads the sizes and seeds once at construction, a uniform block
/// once per build or action press, and the stat values once per read.
///
/// The methods with a default body serve a model composed at run time. [`Self::run_params_bytes`] and
/// [`Self::run_stats`] forward to [`Self::pass_params_bytes`] and [`Self::stats`] by default, and the engine calls
/// them in their place.
pub trait AgentHooks: Send + Sync {
    /// Returns the population and the world extent for `params`, before the engine clamps each to at least 1.
    fn dims(&self, params: &[ParamValue]) -> (u32, Extent);

    /// Returns the length in `u32` words of each buffer, in [`GpuAgentSpec::buffers`] order.
    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize>;

    /// Returns the initial contents of each buffer as raw bytes, in [`GpuAgentSpec::buffers`] order.
    ///
    /// A non-empty vector holds exactly `len * 4` bytes for the length [`Self::buffer_lens`] returns, and an empty one
    /// leaves its buffer cleared.
    fn seed_buffers(&self, geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>>;

    /// Returns the neighbour index's cell size for `params`, read when [`GpuAgentSpec::index`] is set.
    fn index_cell_size(&self, params: &[ParamValue]) -> f32;

    /// Returns the uniform block of `pass` as raw bytes.
    ///
    /// [`PassId::Step`] and [`PassId::Action`] index [`GpuAgentSpec::steps`] and [`GpuAgentSpec::actions`].
    fn pass_params_bytes(&self, pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8>;

    /// Turns the reduction and the counters into values, in [`GpuAgentSpec::stats`] order.
    fn stats(&self, sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue>;

    /// Returns the uniform block of `pass` in the run `run` as raw bytes.
    ///
    /// The default forwards to [`Self::pass_params_bytes`], and returns 16 zero bytes for the colour pass.
    ///
    /// # Errors
    ///
    /// Returns the reason the block has no value, such as a fault of the host evaluation it needs. A build then fails
    /// with the reason, and a press is refused with it.
    fn run_params_bytes(
        &self,
        pass: AgentPassId,
        ctx: PassCtx<'_>,
        _run: AgentRun,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        Ok(match pass.model_pass() {
            Some(pass) => self.pass_params_bytes(pass, ctx, params),
            None => vec![0; 16],
        })
    }

    /// Turns the reduction and the counters into values in the run `run` with parameters `params`, in
    /// [`GpuAgentSpec::stats`] order.
    ///
    /// The default forwards to [`Self::stats`].
    fn run_stats(
        &self,
        sums: &[f32],
        counters: &[u32],
        geom: &Geometry,
        _run: AgentRun,
        _params: &[ParamValue],
    ) -> Vec<StatValue> {
        self.stats(sums, counters, geom)
    }

    /// Runs on the host what a press of action `action` reruns before its pass, advancing `stream`, and returns the
    /// buffers to upload to their current sides.
    ///
    /// The default reruns nothing.
    ///
    /// # Errors
    ///
    /// Returns the reason the press is refused. The engine then runs none of the action's passes.
    fn rerun(
        &self,
        _action: usize,
        _geom: &Geometry,
        _run: AgentRun,
        _params: &[ParamValue],
        _stream: &mut u64,
    ) -> Result<AgentUploads, String> {
        Ok(Vec::new())
    }

    /// Returns the fault the counters read back report, `None` when they report none.
    ///
    /// The engine reads it after each stats readback and ends the run with the fault. The default reports none.
    fn fault(&self, _counters: &[u32]) -> Option<String> {
        None
    }

    /// Returns the work units one invocation of the step pass at `step` of [`GpuAgentSpec::steps`] adds to the cost
    /// the pass declares, for `params` over `geom`.
    ///
    /// A model composed at run time adds here the work that depends on the parameters, such as the cells and agents a
    /// radius query walks, so [`GpuAgentSpec::demand`] counts it. The default adds none.
    fn step_cost(&self, _step: usize, _geom: &Geometry, _params: &[ParamValue]) -> u64 {
        0
    }

    /// Returns whether a shader of the model can record a fault in the counters, which [`Self::fault`] reads.
    ///
    /// When it can, the engine also reads the counters back once the steps a caller waits for have finished, so a run
    /// that reads no stats still ends with the fault. The default returns false.
    fn reports_faults(&self) -> bool {
        false
    }
}

/// Hooks of model `M`, each calling the associated function of the same name.
struct AgentModelHooks<M>(PhantomData<M>);

impl<M: GpuAgentModel> AgentHooks for AgentModelHooks<M> {
    fn dims(&self, params: &[ParamValue]) -> (u32, Extent) {
        M::dims(params)
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        M::buffer_lens(geom)
    }

    fn seed_buffers(&self, geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        M::seed_buffers(geom, params, seed)
    }

    fn index_cell_size(&self, params: &[ParamValue]) -> f32 {
        M::index_cell_size(params)
    }

    fn pass_params_bytes(&self, pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        M::pass_params_bytes(pass, ctx, params)
    }

    fn stats(&self, sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
        M::stats(sums, counters, geom)
    }
}

/// One pass as [`GpuAgentSpec::demand`] counts it.
struct DeclaredPass {
    label: String,
    kind: PassKind,
    storage: u32,
    /// Invocation domain, `None` for the display pass, which runs once per texel.
    domain: Option<Domain>,
    cost: u64,
    uniform_bytes: u64,
    workgroup_bytes: u64,
}

/// Facts of one pass that its demand reads.
struct PassFacts<'a> {
    shader: &'a ShaderSpec,
    domain: Option<Domain>,
    cost: u64,
    uniform_bytes: u64,
    workgroup_bytes: u64,
}

impl<'a> From<&'a AgentPassSpec> for PassFacts<'a> {
    fn from(pass: &'a AgentPassSpec) -> Self {
        Self {
            shader: &pass.shader,
            domain: Some(pass.domain),
            cost: pass.cost,
            uniform_bytes: pass.uniform_bytes,
            workgroup_bytes: pass.workgroup_bytes,
        }
    }
}

/// Declarations of a GPU agent model, held as values.
///
/// Each label the engine gives a wgpu object starts with [`Self::id`].
#[non_exhaustive]
pub struct GpuAgentSpec {
    /// Id of the model.
    pub id: Cow<'static, str>,
    /// Storage buffers of the model's state.
    pub buffers: Vec<AgentBufferSpec>,
    /// Index into [`Self::buffers`] of the `vec2<f32>` positions that the view draws. The buffer must be drawable.
    pub pos_buffer: usize,
    /// Index into [`Self::buffers`] of the packed RGBA that the view draws. The buffer must be drawable.
    pub color_buffer: usize,
    /// Whether the engine rebuilds a neighbour index from the positions before every step. [`Self::new`] leaves this
    /// off.
    pub index: bool,
    /// Number of persistent `u32` counters that a kernel accumulates into. [`Self::new`] leaves this at zero.
    pub counters: usize,
    /// Passes of one step, run in order. [`Self::new`] leaves this empty.
    pub steps: Vec<AgentPassSpec>,
    /// Display pass, for a model that draws a grid layer under its agents. [`Self::new`] leaves this `None`.
    pub display: Option<AgentDisplaySpec>,
    /// Leaf of the stat reduction.
    pub reduce: AgentReduceSpec,
    /// One-off passes the user can trigger, in the order of their buttons. [`Self::new`] leaves this empty.
    pub actions: Vec<AgentActionSpec>,
    /// Pass that writes the colour buffer, run before the display pass of every snapshot. A model whose steps write
    /// the colours has none, and [`Self::new`] leaves this `None`.
    pub color: Option<AgentPassSpec>,
    /// Range of [`Self::counters`] that the engine clears before every reduce leaf, for counts that the leaf takes
    /// afresh. [`Self::new`] leaves this empty.
    pub cleared_counters: Range<usize>,
    /// Range of [`Self::counters`] that the step passes count fires into, empty for a model with no probes, as
    /// [`Self::new`] leaves it.
    ///
    /// The engine clears the range before the last step of each batch of steps and binds that step's counting blocks,
    /// written with [`AgentRun::counting`] set. The counts read back with the other counters.
    pub fire_counters: Range<usize>,
    /// Cost per element of the init passes the host runs at build, in work units. Zero for a model seeded otherwise,
    /// and [`Self::new`] leaves it at zero.
    pub init_cost: u64,
    /// Stat series, in the order of the values [`AgentHooks::stats`] returns.
    pub stats: Cow<'static, [StatDescriptor]>,
    /// Functions the engine calls for the sizes, the seeds, the uniform blocks and the stat values.
    pub hooks: Box<dyn AgentHooks>,
}

/// Prints the declarations, and leaves out the hooks.
impl fmt::Debug for GpuAgentSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuAgentSpec")
            .field("id", &self.id)
            .field("buffers", &self.buffers)
            .field("pos_buffer", &self.pos_buffer)
            .field("color_buffer", &self.color_buffer)
            .field("index", &self.index)
            .field("counters", &self.counters)
            .field("steps", &self.steps)
            .field("display", &self.display)
            .field("reduce", &self.reduce)
            .field("actions", &self.actions)
            .field("color", &self.color)
            .field("cleared_counters", &self.cleared_counters)
            .field("fire_counters", &self.fire_counters)
            .field("init_cost", &self.init_cost)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl GpuAgentSpec {
    /// Creates the spec of model `id` with no index, counters, step passes, display or actions.
    ///
    /// Each of those is a public field, for a model that declares otherwise.
    pub fn new(
        id: impl Into<Cow<'static, str>>,
        buffers: Vec<AgentBufferSpec>,
        pos_buffer: usize,
        color_buffer: usize,
        reduce: AgentReduceSpec,
        stats: impl Into<Cow<'static, [StatDescriptor]>>,
        hooks: Box<dyn AgentHooks>,
    ) -> Self {
        Self {
            id: id.into(),
            buffers,
            pos_buffer,
            color_buffer,
            index: false,
            counters: 0,
            steps: Vec::new(),
            display: None,
            reduce,
            actions: Vec::new(),
            color: None,
            cleared_counters: 0..0,
            fire_counters: 0..0,
            init_cost: 0,
            stats: stats.into(),
            hooks,
        }
    }

    /// Returns the declarations of model `M`, borrowing its strings and its stats.
    ///
    /// Every pass but an action reads each current side and writes each next side, and every action binds in place.
    /// One swap per step then gives each pass the orientation the model's shaders assume.
    pub(crate) fn from_model<M: GpuAgentModel>() -> Self {
        let count = M::BUFFERS.len();
        let reduce = AgentReduceSpec::new(
            ShaderSpec::borrowed(M::REDUCE.shader, M::REDUCE.bindings),
            M::REDUCE.lanes,
            M::REDUCE.domain,
            SideTable::swapping(count),
        );
        let mut spec = Self::new(
            M::ID,
            M::BUFFERS.iter().map(AgentBufferSpec::from).collect(),
            M::POS_BUFFER,
            M::COLOR_BUFFER,
            reduce,
            M::STATS,
            Box::new(AgentModelHooks::<M>(PhantomData)),
        );
        spec.index = M::INDEX;
        spec.counters = M::COUNTERS;
        spec.steps = M::STEP_PASSES
            .iter()
            .map(|pass| AgentPassSpec::from_model(pass, count))
            .collect();
        spec.display = M::DISPLAY.as_ref().map(|display| {
            AgentDisplaySpec::new(
                ShaderSpec::borrowed(display.shader, display.bindings),
                display.workgroup,
                SideTable::swapping(count),
            )
        });
        spec.actions = M::ACTIONS
            .iter()
            .map(|action| {
                AgentActionSpec::new(
                    action.desc.id(),
                    ShaderSpec::borrowed(action.pass.shader, action.pass.bindings),
                    action.pass.domain,
                    SideTable::in_place(count),
                )
            })
            .collect();
        spec
    }

    /// Returns the [`Structure`] an entry of this model declares.
    pub fn structure(&self) -> Structure {
        Structure::GpuAgents {
            buffers: Cow::Owned(
                self.buffers
                    .iter()
                    .map(|buffer| GpuBufferInfo::new(buffer.label.clone(), buffer.double_buffered, buffer.drawable))
                    .collect(),
            ),
            passes: Cow::Owned(
                self.steps
                    .iter()
                    .map(|pass| GpuPassInfo::new(pass.label.clone(), pass.domain))
                    .collect(),
            ),
            index: self.index,
            display: self.display.is_some(),
            counters: self.counters,
        }
    }

    /// Returns the display layers an entry of this model presents.
    ///
    /// A model that declares a display pass draws a grid layer under its agents.
    pub fn topology_hint(&self) -> TopologyHint {
        TopologyHint {
            grid: self.display.is_some(),
            agents: true,
            edges: false,
        }
    }

    /// Returns the geometry for `params`, without touching a device. `limits` sets only the display cap.
    pub fn geometry(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Geometry {
        let (num_agents, extent) = self.hooks.dims(params);
        let num_agents = num_agents.max(1);
        let extent = Extent {
            w: extent.w.max(1.0),
            h: extent.h.max(1.0),
        };
        let (width, height) = extent.cells();
        Geometry {
            num_agents,
            extent,
            width,
            height,
            n_cells: width * height,
            display: display_dims(width, height, limits.max_texture_dimension_2d),
            index: self
                .index
                .then(|| HashGrid::new(extent, self.hooks.index_cell_size(params))),
        }
    }

    /// Resources that would be allocated for this model based on `params`.
    pub fn demand(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        let geom = self.geometry(params, limits);
        let mut demand = Demand::default();
        for (buffer, len) in self.buffers.iter().zip(self.hooks.buffer_lens(&geom)) {
            demand.push_sides(&format!("{}_{}", self.id, buffer.label), len, buffer.double_buffered);
        }
        if let Some(grid) = geom.index {
            demand.push_index(&self.id, grid.num_cells(), geom.num_agents);
        }
        if self.display.is_some() {
            demand.set_display(geom.width, geom.height, limits);
        }
        // A hand-written shader has no cost, and its work counts as zero. The steps come first.
        for (position, pass) in self.declared_passes().into_iter().enumerate() {
            let invocations = match pass.kind {
                PassKind::Display if pass.domain.is_none() => u64::from(geom.display.0) * u64::from(geom.display.1),
                _ => pass.domain.map_or(0, |domain| u64::from(domain.invocations(&geom))),
            };
            let cost = if position < self.steps.len() {
                pass.cost.saturating_add(self.hooks.step_cost(position, &geom, params))
            } else {
                pass.cost
            };
            let recorded = demand.push_pass(pass.label, pass.kind, pass.storage, cost.saturating_mul(invocations));
            recorded.uniform_bytes = pass.uniform_bytes;
            recorded.workgroup_bytes = pass.workgroup_bytes;
        }
        // A pass on the host runs over agents or cells, and the larger of the two bounds either.
        let elements = u64::from(geom.num_agents.max(geom.n_cells));
        if self.init_cost > 0 {
            demand.push_host(format!("{}_init", self.id), self.init_cost.saturating_mul(elements));
        }
        for action in self.actions.iter().filter(|action| action.rerun_cost > 0) {
            demand.push_host(
                format!("{}_action_{}_rerun", self.id, action.id),
                action.rerun_cost.saturating_mul(elements),
            );
        }
        demand
    }

    /// Every pass with its kind, the storage buffers it binds, its domain, its cost per invocation and its memory
    /// sizes. Read by both [`Self::demand`] and [`Self::max_storage_bindings`], so the device that a host requests and
    /// the shortfall that the UI reports cannot disagree.
    fn declared_passes(&self) -> Vec<DeclaredPass> {
        let declared = |label: String, kind: PassKind, pass: PassFacts<'_>| DeclaredPass {
            label,
            kind,
            storage: storage_bindings(&pass.shader.bindings),
            domain: pass.domain,
            cost: pass.cost,
            uniform_bytes: pass.uniform_bytes,
            workgroup_bytes: pass.workgroup_bytes,
        };
        let mut passes: Vec<DeclaredPass> = self
            .steps
            .iter()
            .map(|pass| declared(format!("{}_{}", self.id, pass.label), PassKind::Step, pass.into()))
            .collect();
        if let Some(color) = &self.color {
            passes.push(declared(format!("{}_color", self.id), PassKind::Display, color.into()));
        }
        if let Some(display) = &self.display {
            passes.push(declared(
                format!("{}_display", self.id),
                PassKind::Display,
                PassFacts {
                    shader: &display.shader,
                    domain: None,
                    cost: display.cost,
                    uniform_bytes: display.uniform_bytes,
                    workgroup_bytes: display.workgroup_bytes,
                },
            ));
        }
        passes.push(declared(
            format!("{}_reduce_leaf", self.id),
            PassKind::Reduce,
            PassFacts {
                shader: &self.reduce.shader,
                domain: Some(self.reduce.domain),
                cost: self.reduce.cost,
                uniform_bytes: self.reduce.uniform_bytes,
                workgroup_bytes: self.reduce.workgroup_bytes,
            },
        ));
        for (index, action) in self.actions.iter().enumerate() {
            passes.push(declared(
                format!("{}_action_{}", self.id, action.id),
                PassKind::Action(index),
                PassFacts {
                    shader: &action.shader,
                    domain: Some(action.domain),
                    cost: action.cost,
                    uniform_bytes: action.uniform_bytes,
                    workgroup_bytes: action.workgroup_bytes,
                },
            ));
        }
        passes
    }

    /// Returns the most storage buffers any declared pass binds. It does not depend on params, so a host can query it
    /// before it has a device.
    pub fn max_storage_bindings(&self) -> u32 {
        self.declared_passes()
            .into_iter()
            .map(|pass| pass.storage)
            .max()
            .unwrap_or(0)
    }

    /// Returns each pass the engine builds, as its name in a message, its shader, its side table and the workgroup
    /// size it dispatches.
    fn shaders(&self) -> impl Iterator<Item = (&str, &ShaderSpec, &SideTable, [u32; 3])> {
        // Every pass but the display folds a linear domain onto workgroups of `WORKGROUP`.
        let linear = [WORKGROUP, 1, 1];
        self.steps
            .iter()
            .map(move |pass| (pass.label.as_ref(), &pass.shader, &pass.sides, linear))
            .chain(
                self.color
                    .iter()
                    .map(move |pass| ("colour", &pass.shader, &pass.sides, linear)),
            )
            .chain([("reduce leaf", &self.reduce.shader, &self.reduce.sides, linear)])
            .chain(
                self.actions
                    .iter()
                    .map(move |action| (action.id.as_ref(), &action.shader, &action.sides, linear)),
            )
            .chain(self.display.iter().map(|display| {
                (
                    "display",
                    &display.shader,
                    &display.sides,
                    [display.workgroup, display.workgroup, 1],
                )
            }))
    }

    /// Asserts that the buffers, the shaders and the side tables agree with what the engine binds and dispatches.
    ///
    /// # Panics
    ///
    /// Panics if a buffer label is reserved or ends in `_in` or `_out`, if a shader declares a workgroup size other
    /// than the one its pass dispatches, if the position or colour index lies past the buffers, or if a side table
    /// holds another number of rows than there are buffers. Also panics if a pass binds one side of a buffer both
    /// read-only and writable, which wgpu refuses at the dispatch.
    ///
    /// Panics if the position or colour buffer is not drawable, since the view binds both as vertex streams and wgpu
    /// refuses any other buffer at the first draw. Panics if no step pass has a writable binding of a double buffered
    /// buffer that reaches [`BufferSide::Next`]. The sides swap every step, and the state would otherwise alternate
    /// between the seed and the cleared side. Panics if [`Self::fire_counters`] ends past [`Self::counters`].
    pub(crate) fn assert_contracts(&self) {
        let id = &*self.id;
        assert_buffer_labels(id, self.buffers.iter().map(|buffer| buffer.label.as_ref()));
        assert!(
            self.fire_counters.end <= self.counters,
            "{id}: the fire counters {:?} end past the {} counters",
            self.fire_counters,
            self.counters
        );
        for (role, index) in [("position", self.pos_buffer), ("colour", self.color_buffer)] {
            assert!(
                index < self.buffers.len(),
                "{id}: the {role} buffer index {index} lies past the {} buffers",
                self.buffers.len()
            );
            assert!(
                self.buffers[index].drawable,
                "{id}: the {role} buffer `{}` is not drawable, and the view draws it",
                self.buffers[index].label
            );
        }

        for (pass, shader, sides, dispatched) in self.shaders() {
            assert_workgroup_size(id, pass, &shader.source, dispatched);
            assert!(
                sides.rows().len() == self.buffers.len(),
                "{id}: the side table of {pass} holds {} rows for {} buffers",
                sides.rows().len(),
                self.buffers.len()
            );
            self.assert_no_conflict(pass, shader, sides);
        }
        self.assert_next_sides_written();
    }

    /// Asserts that a step pass writes the next side of each double buffered buffer.
    ///
    /// Reads each table by buffer position, so it runs after the check of the tables' lengths.
    fn assert_next_sides_written(&self) {
        for (k, buffer) in self.buffers.iter().enumerate() {
            let written = self.steps.iter().any(|pass| {
                pass.sides.rows()[k].write == BufferSide::Next
                    && pass
                        .shader
                        .bindings
                        .iter()
                        .filter_map(ShaderBinding::buffer_target)
                        .any(|(label, writes)| writes && label == buffer.label)
            });
            assert!(
                written || !buffer.double_buffered,
                "{}: no step pass writes the next side of double buffered buffer `{}`",
                self.id,
                buffer.label
            );
        }
    }

    /// Asserts that no side of a buffer is bound in `shader` both read-only and writable.
    fn assert_no_conflict(&self, pass: &str, shader: &ShaderSpec, sides: &SideTable) {
        let reached = |writes: bool| {
            let rows = sides.rows();
            shader
                .bindings
                .iter()
                .filter_map(|binding| binding.buffer_target())
                .filter(move |&(_, target_writes)| target_writes == writes)
                .filter_map(move |(label, _)| {
                    let k = self.buffers.iter().position(|buffer| buffer.label == label)?;
                    let side = if writes { rows[k].write } else { rows[k].read };
                    Some((k, self.buffers[k].double_buffered.then_some(side)))
                })
        };
        for read in reached(false) {
            assert!(
                !reached(true).any(|write| write == read),
                "{}: the {pass} pass binds one side of buffer `{}` both read-only and writable",
                self.id,
                self.buffers[read.0].label
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use henad_core::action::ActionDescriptor;
    use henad_core::authoring::model::binding::{BindingDecl, BindingKind};
    use henad_core::authoring::model::field::Extent;
    use henad_core::authoring::model::gpu_agent_model::{
        BufferSpec, DisplaySpec, Domain, Geometry, GpuAgentAction, GpuAgentModel, PassCtx, PassId, PassSpec, ReduceSpec,
    };
    use henad_core::metadata::Structure;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::view::{StatDescriptor, StatValue};

    use super::{AgentPassSpec, GpuAgentSpec, ShaderBinding, ShaderSpec};
    use crate::gpu::sides::{BufferSide, SideRow, SideTable};

    const fn storage(name: &'static str, read_only: bool) -> BindingDecl {
        BindingDecl {
            name,
            kind: BindingKind::Storage { read_only },
        }
    }

    const fn uniform(name: &'static str) -> BindingDecl {
        BindingDecl {
            name,
            kind: BindingKind::Uniform,
        }
    }

    /// A model that is never built, with a distinct value in every declaration and every function.
    struct Probe;

    impl GpuAgentModel for Probe {
        const NAME: &'static str = "Probe";
        const ID: &'static str = "probe";
        const DESCRIPTION: &'static str = "Declarations read by the spec test";
        const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Low", [1, 2, 3, 255])];
        const BUFFERS: &'static [BufferSpec] = &[
            BufferSpec {
                label: "pos",
                double_buffered: true,
                drawable: true,
            },
            BufferSpec {
                label: "color",
                double_buffered: false,
                drawable: true,
            },
        ];
        const POS_BUFFER: usize = 0;
        const COLOR_BUFFER: usize = 1;
        const INDEX: bool = true;
        const COUNTERS: usize = 2;
        const STEP_PASSES: &'static [PassSpec] = &[PassSpec {
            label: "walk",
            shader: "walk shader",
            bindings: &[
                storage("pos_in", true),
                storage("pos_out", false),
                storage("color", false),
                uniform("params"),
            ],
            domain: Domain::Agents,
        }];
        const DISPLAY: Option<DisplaySpec> = Some(DisplaySpec {
            shader: "display shader",
            bindings: &[storage("pos", true), uniform("params")],
            workgroup: 16,
        });
        const ACTIONS: &'static [GpuAgentAction] = &[GpuAgentAction {
            desc: ActionDescriptor::new("spark", "Spark"),
            pass: PassSpec {
                label: "spark",
                shader: "action shader",
                bindings: &[storage("pos", false), uniform("params")],
                domain: Domain::AgentsOrCells,
            },
        }];
        const REDUCE: ReduceSpec = ReduceSpec {
            shader: "reduce shader",
            bindings: &[storage("pos", true), storage("partials", false), uniform("params")],
            lanes: 3,
            domain: Domain::Agents,
        };

        fn param_descriptors() -> Vec<ParamDescriptor> {
            Vec::new()
        }

        fn dims(params: &[ParamValue]) -> (u32, Extent) {
            let count = match params {
                [ParamValue::U32(count), ..] => *count,
                _ => 100,
            };
            (count, Extent { w: 20.0, h: 0.0 })
        }

        fn buffer_lens(geom: &Geometry) -> Vec<usize> {
            vec![geom.num_agents as usize * 2, geom.num_agents as usize]
        }

        fn seed_buffers(geom: &Geometry, _params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
            vec![vec![geom.width as u8], vec![seed.map_or(0, |seed| seed as u8)]]
        }

        fn index_cell_size(_params: &[ParamValue]) -> f32 {
            5.0
        }

        fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, _params: &[ParamValue]) -> Vec<u8> {
            match pass {
                PassId::Action(_) => vec![ctx.seed as u8],
                _ => vec![ctx.invocations as u8],
            }
        }

        fn stats(sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
            vec![StatValue::Scalar(
                f64::from(sums[0]) + f64::from(counters[0]) + f64::from(geom.num_agents),
            )]
        }
    }

    /// Returns whether `shader` borrows `source` and the name of each of `bindings`, in the kinds they declare.
    fn borrows(shader: &ShaderSpec, source: &str, bindings: &[BindingDecl]) -> bool {
        matches!(shader.source, Cow::Borrowed(text) if text == source)
            && shader.bindings.len() == bindings.len()
            && shader.bindings.iter().zip(bindings).all(|(binding, decl)| {
                matches!(binding.name, Cow::Borrowed(name) if name == decl.name) && binding.kind == decl.kind
            })
    }

    /// The spec of a model borrows every string it holds, and each hook calls the function of the same name.
    #[test]
    fn a_model_spec_borrows_its_declarations_and_calls_its_functions() {
        let spec = GpuAgentSpec::from_model::<Probe>();
        assert!(matches!(spec.id, Cow::Borrowed("probe")), "id");
        assert!(matches!(spec.buffers[0].label, Cow::Borrowed("pos")), "buffer label");
        assert!(spec.buffers[0].double_buffered && spec.buffers[0].drawable);
        assert!(!spec.buffers[1].double_buffered && spec.buffers[1].drawable);
        assert_eq!((spec.pos_buffer, spec.color_buffer), (0, 1));
        assert!(spec.index);
        assert_eq!(spec.counters, 2);

        let walk = &spec.steps[0];
        assert!(matches!(walk.label, Cow::Borrowed("walk")), "step label");
        assert!(
            borrows(&walk.shader, "walk shader", Probe::STEP_PASSES[0].bindings),
            "step"
        );
        assert!(matches!(walk.domain, Domain::Agents));
        let display = spec.display.as_ref().expect("the probe declares a display");
        assert_eq!(display.workgroup, 16);
        assert!(borrows(
            &display.shader,
            "display shader",
            &[storage("pos", true), uniform("params")]
        ));
        assert!(borrows(&spec.reduce.shader, "reduce shader", Probe::REDUCE.bindings));
        assert_eq!(spec.reduce.lanes, 3);
        assert!(matches!(spec.actions[0].id, Cow::Borrowed("spark")), "action id");
        assert!(borrows(
            &spec.actions[0].shader,
            "action shader",
            Probe::ACTIONS[0].pass.bindings
        ));
        assert!(
            matches!(spec.stats, Cow::Borrowed(stats) if stats == Probe::STATS),
            "stats"
        );

        let geom = spec.geometry(&[ParamValue::U32(10)], &wgpu::Limits::default());
        assert_eq!(
            (geom.num_agents, geom.width, geom.height),
            (10, 20, 1),
            "the extent is clamped"
        );
        assert_eq!(spec.hooks.buffer_lens(&geom), vec![20, 10]);
        assert_eq!(spec.hooks.seed_buffers(&geom, &[], Some(7)), vec![vec![20], vec![7]]);
        assert!((spec.hooks.index_cell_size(&[]) - 5.0).abs() < f32::EPSILON);
        let ctx = PassCtx {
            geom: &geom,
            invocations: 33,
            groups_x: 1,
            seed: 44,
        };
        assert_eq!(spec.hooks.pass_params_bytes(PassId::Step(0), ctx, &[]), vec![33]);
        assert_eq!(spec.hooks.pass_params_bytes(PassId::Action(0), ctx, &[]), vec![44]);
        let values = spec.hooks.stats(&[1.0], &[2], &geom);
        assert!(matches!(values[0], StatValue::Scalar(value) if (value - 13.0).abs() < f64::EPSILON));
    }

    /// A step, display and reduce pass reads the current side and writes the next, and an action stays in place.
    #[test]
    fn a_model_spec_binds_in_the_single_orientation() {
        let spec = GpuAgentSpec::from_model::<Probe>();
        let swapping = SideRow::new(BufferSide::Current, BufferSide::Next);
        let in_place = SideRow::new(BufferSide::Current, BufferSide::Current);
        assert_eq!(spec.steps[0].sides.rows(), [swapping, swapping]);
        let display = spec.display.as_ref().expect("the probe declares a display");
        assert_eq!(display.sides.rows(), [swapping, swapping]);
        assert_eq!(spec.reduce.sides.rows(), [swapping, swapping]);
        assert_eq!(spec.actions[0].sides.rows(), [in_place, in_place]);
    }

    /// The structure describes the buffers and step passes, and the demand counts each pass's storage bindings.
    #[test]
    fn a_spec_describes_its_structure_and_demand() {
        let spec = GpuAgentSpec::from_model::<Probe>();
        let Structure::GpuAgents {
            buffers,
            passes,
            index,
            display,
            counters,
        } = spec.structure()
        else {
            panic!("an agent spec describes a GPU agent model");
        };
        let labels: Vec<&str> = buffers.iter().map(|buffer| buffer.label.as_ref()).collect();
        assert_eq!(labels, ["pos", "color"]);
        assert!(buffers[0].double_buffered && !buffers[1].double_buffered);
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].label, "walk");
        assert!(index && display);
        assert_eq!(counters, 2);

        let demand = spec.demand(&[ParamValue::U32(10)], &wgpu::Limits::default());
        let allocs: Vec<(&str, u64)> = demand
            .buffers
            .iter()
            .map(|alloc| (alloc.label.as_str(), alloc.bytes))
            .take(3)
            .collect();
        assert_eq!(
            allocs,
            [("probe_pos_a", 80), ("probe_pos_b", 80), ("probe_color_a", 40)]
        );
        assert_eq!(demand.texture, Some((20, 1)));
        let pass_list: Vec<(&str, u32)> = demand
            .passes
            .iter()
            .map(|pass| (pass.label.as_str(), pass.storage))
            .collect();
        assert_eq!(
            pass_list,
            [
                ("probe_walk", 3),
                ("probe_display", 1),
                ("probe_reduce_leaf", 2),
                ("probe_action_spark", 1),
            ]
        );
        assert_eq!(spec.max_storage_bindings(), 3);
    }

    /// Returns a spec of [`Probe`] whose shaders declare the workgroup sizes the engine dispatches.
    fn valid_spec() -> GpuAgentSpec {
        let mut spec = GpuAgentSpec::from_model::<Probe>();
        let linear = || Cow::Owned("@compute @workgroup_size(256) fn main() {}".to_owned());
        spec.steps[0].shader.source = linear();
        spec.reduce.shader.source = linear();
        spec.actions[0].shader.source = linear();
        spec
    }

    #[test]
    fn a_valid_spec_meets_its_contracts() {
        valid_spec().assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the spark shader declares @workgroup_size(16, 16, 1)")]
    fn an_action_shader_of_another_workgroup_size_is_refused() {
        let mut spec = valid_spec();
        spec.actions[0].shader.source = Cow::Owned("@compute @workgroup_size(16, 16) fn main() {}".to_owned());
        spec.assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the side table of walk holds 1 rows for 2 buffers")]
    fn a_side_table_of_another_length_is_refused() {
        let mut spec = valid_spec();
        spec.steps[0].sides = SideTable::swapping(1);
        spec.assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the walk pass binds one side of buffer `pos` both read-only and writable")]
    fn a_table_that_reads_and_writes_one_side_is_refused() {
        let mut spec = valid_spec();
        spec.steps[0].sides = SideTable::in_place(2);
        spec.assert_contracts();
    }

    /// An action reads `pos_in` and `pos_out` as one side, since nothing swaps after a press. The build refuses this
    /// even if the action is never pressed.
    #[test]
    #[should_panic(expected = "probe: the spark pass binds one side of buffer `pos` both read-only and writable")]
    fn an_action_that_reads_and_writes_one_side_is_refused() {
        let mut spec = valid_spec();
        spec.actions[0].shader.bindings = [storage("pos_in", true), storage("pos_out", false)]
            .iter()
            .map(ShaderBinding::from)
            .collect();
        spec.assert_contracts();
    }

    /// The action of the probe writes `pos` in place, which leaves the next side unwritten.
    #[test]
    #[should_panic(expected = "probe: no step pass writes the next side of double buffered buffer `pos`")]
    fn a_double_buffered_buffer_without_a_step_pass_is_refused() {
        let mut spec = valid_spec();
        spec.steps.clear();
        spec.assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: no step pass writes the next side of double buffered buffer `pos`")]
    fn a_double_buffered_buffer_a_step_pass_only_reads_is_refused() {
        let mut spec = valid_spec();
        spec.steps[0].shader.bindings = [storage("pos_in", true), storage("color", false)]
            .iter()
            .map(ShaderBinding::from)
            .collect();
        spec.assert_contracts();
    }

    /// A write to the current side leaves the next side unwritten, and the swap shows it.
    #[test]
    #[should_panic(expected = "probe: no step pass writes the next side of double buffered buffer `pos`")]
    fn a_double_buffered_buffer_a_step_pass_writes_in_place_is_refused() {
        let mut spec = valid_spec();
        spec.steps[0].sides = SideTable::new(vec![SideRow::new(BufferSide::Next, BufferSide::Current); 2]);
        spec.assert_contracts();
    }

    /// One writer of the next side is enough, whatever an earlier pass writes.
    #[test]
    fn a_double_buffered_buffer_with_one_writer_of_its_next_side_meets_its_contracts() {
        let mut spec = valid_spec();
        let in_place = AgentPassSpec::new(
            "first",
            spec.steps[0].shader.clone(),
            Domain::Agents,
            SideTable::new(vec![SideRow::new(BufferSide::Next, BufferSide::Current); 2]),
        );
        spec.steps.insert(0, in_place);
        spec.assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the position buffer `pos` is not drawable")]
    fn a_position_buffer_that_is_not_drawable_is_refused() {
        let mut spec = valid_spec();
        spec.buffers[0].drawable = false;
        spec.assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the colour buffer `color` is not drawable")]
    fn a_colour_buffer_that_is_not_drawable_is_refused() {
        let mut spec = valid_spec();
        spec.buffers[1].drawable = false;
        spec.assert_contracts();
    }

    #[test]
    #[should_panic(expected = "probe: the position buffer index 5 lies past the 2 buffers")]
    fn a_position_index_past_the_buffers_is_refused() {
        let mut spec = valid_spec();
        spec.pos_buffer = 5;
        spec.assert_contracts();
    }
}
