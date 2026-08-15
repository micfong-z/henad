//! Authoring API for agent models whose population lives entirely in GPU buffers.
//!
//! This is the GPU sibling of [`crate::authoring::agent_model::AgentModel`], and the agent-shaped
//! counterpart of [`crate::authoring::gpu_grid_model::GpuGridModel`]. A model declares its buffers,
//! its passes and its bindings as plain data, and the engine (`henad_compute::gpu::agent_engine`)
//! derives every wgpu object, the neighbour index, the ping-pong, the stat reduction and the whole
//! `SimState`/`GpuSimState` impl from them.
//!
//! Unlike a grid, an agent step is not one dispatch. Boids rebuilds a neighbour index and runs one
//! kernel; ants runs a kernel over agents and then one over cells. So a model declares a *list* of
//! passes rather than a fixed step/display/reduce triple.
//!
//! # Bindings
//!
//! A pass's [`Binding`] slice is positional: entry `i` is `@group(0) @binding(i)`. There is no
//! naming convention to keep in step, only an order.
//!
//! ```wgsl
//! // bindings: &[Binding::Read(0), Binding::Write(0), Binding::IndexSorted, Binding::Uniform]
//! @group(0) @binding(0) var<storage, read>       pos_in:  array<vec2<f32>>;
//! @group(0) @binding(1) var<storage, read_write> pos_out: array<vec2<f32>>;
//! @group(0) @binding(2) var<storage, read>       sorted:  array<u32>;
//! @group(0) @binding(3) var<uniform>             params:  Params;
//! ```
//!
//! [`Binding::Read`] and [`Binding::Write`] name a [`BufferSpec`] by index. They resolve to the
//! two sides of a `double_buffered` buffer and to the same buffer otherwise, so a model that reads
//! and writes in place declares `Write` alone.
//!
//! # Shader preludes
//!
//! Every pass shader is prepended with a shared prelude, so it may use `WORKGROUP` and
//! `linear_index(lid, wid, groups_x)` without declaring them. The prelude is a prefix, never
//! spliced in, so a compile error's line number is off by a constant.
//!
//! The reduce leaf is generated entirely, from [`GpuAgentModel::REDUCE_HEADER`] and
//! [`GpuAgentModel::REDUCE_VALUE`] — the workgroup tree around it is the same in every model.
//! Set `HENAD_DUMP_WGSL` to a directory to read back what was actually compiled.
//!
//! # Unchecked contracts
//!
//! Shaders are opaque strings to Rust, so most of this surfaces as a wgpu validation error at
//! model construction rather than at compile time:
//! - each pass's `@binding` indices must match the position of its [`Binding`] in the slice,
//!   and its declared WGSL types must match what the buffer actually holds,
//! - a pass shader must declare `@workgroup_size(256)` and fold with `linear_index`; a display
//!   shader must declare `@workgroup_size(N, N)` for its [`DisplaySpec::workgroup`],
//! - [`GpuAgentModel::buffer_lens`] and [`GpuAgentModel::seed_buffers`] must each return one entry
//!   per [`GpuAgentModel::BUFFERS`] entry, and a non-empty seed must be exactly `len * 4` bytes,
//! - [`GpuAgentModel::STATS`] length must equal the number of values
//!   [`GpuAgentModel::stats`] returns.

use crate::authoring::field::Extent;
use crate::params::{ParamDescriptor, ParamValue};
use crate::spatial_hash::HashGrid;
use crate::view::{StatDescriptor, StatValue};

/// One storage buffer of the model's state.
pub struct BufferSpec {
    pub label: &'static str,
    /// Doubled, for a buffer a pass reads the previous values of while writing this tick's.
    pub double_buffered: bool,
    /// Also a vertex stream, so the view can draw it without a copy.
    pub drawable: bool,
}

/// Where one bind group entry comes from. Position in a pass's slice is the binding index.
#[derive(Clone, Copy)]
pub enum Binding {
    Read(usize),
    /// The buffer's next side, or the buffer itself when it is not double buffered.
    Write(usize),
    IndexCellStart,
    IndexSorted,
    Counters,
    DisplayTexture,
    /// The reduction's leaf output. Only a reduce pass has one.
    ReducePartials,
    Uniform,
}

impl Binding {
    /// A uniform and a storage texture each count against their own limit, not this one.
    pub fn is_storage_buffer(self) -> bool {
        !matches!(self, Self::Uniform | Self::DisplayTexture)
    }
}

/// A pass's invocation domain.
#[derive(Clone, Copy)]
pub enum Domain {
    Agents,
    /// `n` invocations per cell, for a field with `n` layers.
    Cells(u32),
    /// The larger of the two, for a pass whose lanes span both.
    AgentsOrCells,
}

impl Domain {
    #[must_use]
    pub fn invocations(self, geom: &Geometry) -> u32 {
        let cells = geom.n_cells;
        match self {
            Self::Agents => geom.num_agents,
            Self::Cells(n) => cells * n,
            Self::AgentsOrCells => geom.num_agents.max(cells),
        }
    }
}

/// One compute pass of a step, run in declaration order.
pub struct PassSpec {
    pub label: &'static str,
    pub shader: &'static str,
    pub bindings: &'static [Binding],
    pub domain: Domain,
}

/// The pass that turns state into the display texture, for a model that draws a grid layer.
///
/// Dispatched over [`Geometry::display`], one invocation per texel, not per cell.
pub struct DisplaySpec {
    pub shader: &'static str,
    /// [`Binding::DisplayTexture`] is where the texture goes.
    pub bindings: &'static [Binding],
    /// Must match the `@workgroup_size(N, N)` the shader declares.
    pub workgroup: u32,
}

/// Which uniform block [`GpuAgentModel::pass_params_bytes`] is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassId {
    Step(usize),
    Display,
    Reduce,
}

/// The world these params describe, resolved once at construction.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub num_agents: u32,
    pub extent: Extent,
    pub width: u32,
    pub height: u32,
    pub n_cells: u32,
    /// Display texture size, capped under the cell grid on a large world. A display pass
    /// dispatches over this and reads the cell at `texel * grid / tex`.
    pub display: (u32, u32),
    /// Set when the model declares [`GpuAgentModel::INDEX`].
    pub index: Option<HashGrid>,
}

/// What a uniform block needs beyond the geometry. `groups_x` is the fold width the prelude's
/// `linear_index` expects, so a shader that folds has to carry it.
#[derive(Clone, Copy, Debug)]
pub struct PassCtx<'a> {
    pub geom: &'a Geometry,
    pub invocations: u32,
    pub groups_x: u32,
}

/// A population of agents stepped by compute shaders, with its state resident in GPU buffers.
///
/// See the module docs for the binding conventions and the contracts nothing checks.
pub trait GpuAgentModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;

    /// Stat series for the history chart. Declared once, so [`Self::stats`] returns bare values.
    const STATS: &'static [StatDescriptor];

    const BUFFERS: &'static [BufferSpec];
    /// Index into [`Self::BUFFERS`] of the `vec2<f32>` positions the view draws.
    const POS_BUFFER: usize;
    /// Index into [`Self::BUFFERS`] of the packed RGBA the view draws.
    const COLOR_BUFFER: usize;

    /// Rebuild a neighbour index from the positions before every step. Off for a model whose
    /// agents never read one another.
    const INDEX: bool = false;

    /// Persistent `u32` counters a kernel accumulates into. Never cleared, unlike the reduction.
    const COUNTERS: usize = 0;

    const STEP_PASSES: &'static [PassSpec];
    const DISPLAY: Option<DisplaySpec> = None;

    /// Values the leaf sums, one per lane.
    const REDUCE_LANES: usize;
    const REDUCE_DOMAIN: Domain = Domain::Agents;
    /// Must include [`Binding::ReducePartials`] and [`Binding::Uniform`].
    const REDUCE_BINDINGS: &'static [Binding];
    /// WGSL declarations the generated leaf needs: the bindings, and a `params` uniform carrying
    /// at least `lanes` and `groups_x`.
    const REDUCE_HEADER: &'static str;
    /// WGSL assigning `value`, with `lane` and the invocation index `i` in scope.
    const REDUCE_VALUE: &'static str;

    /// The full descriptor list. Unlike [`crate::authoring::agent_model::AgentModel`], nothing is
    /// prepended — a GPU model spells its list out, so it can mirror the exact parameter order of
    /// the CPU model it is compared against.
    fn param_descriptors() -> Vec<ParamDescriptor>;

    /// Population and world extent for these params. The engine clamps both to at least 1.
    fn dims(params: &[ParamValue]) -> (u32, Extent);

    /// Length in `u32`-sized elements of each buffer, in [`Self::BUFFERS`] order.
    fn buffer_lens(geom: &Geometry) -> Vec<usize>;

    /// Initial contents of each buffer, in [`Self::BUFFERS`] order. An empty vector leaves that
    /// buffer cleared, which is what a scratch buffer read before it is first written wants.
    ///
    /// Raw bytes rather than `u32`, because agent lanes are mixed and `henad-core` has no
    /// bytemuck. Only the current side is seeded; a double buffered one has its other side fully
    /// written by the first step.
    fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>>;

    /// Neighbour index cell size, read only when [`Self::INDEX`].
    fn index_cell_size(_params: &[ParamValue]) -> f32 {
        1.0
    }

    /// A pass's uniform block, as raw bytes. Models keep their own `#[repr(C)]` struct and hand
    /// over `bytemuck::bytes_of(&s).to_vec()`.
    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8>;

    /// Turn the reduction and the counters into values, in [`Self::STATS`] order.
    ///
    /// Both are all-zero until the first readback completes.
    fn stats(sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue>;
}
