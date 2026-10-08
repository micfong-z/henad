//! Authoring API for agent models whose population lives entirely in GPU buffers.
//!
//! [`GpuAgentModel`] is the GPU sibling of [`AgentModel`]. A model declares its buffers, its passes and
//! its bindings as plain data, and the engine in henad-compute derives every wgpu object, the neighbour
//! index, the ping-pong and the stat reduction from them. A step is a list of [`PassSpec`]s run in order.
//! The [GPU agent models](https://micfong-z.github.io/henad/authoring/gpu-agent-models/) page walks through
//! the buffers, the passes and the reduction.
//!
//! [`AgentModel`]: crate::authoring::model::agent_model::AgentModel

use crate::action::ActionDescriptor;
use crate::authoring::model::binding::BindingDecl;
use crate::authoring::model::field::Extent;
use crate::params::{ParamDescriptor, ParamValue};
use crate::spatial_hash::HashGrid;
use crate::view::{StatDescriptor, StatValue};

/// One storage buffer of the model's state.
#[derive(Debug)]
pub struct BufferSpec {
    /// Name that a shader's bindings refer to.
    ///
    /// It must not be reserved or end in `_in` or `_out`. The engine panics at construction on such a
    /// label.
    pub label: &'static str,
    /// Whether the buffer has a second side, for a pass that reads the previous values while it
    /// writes this tick's values.
    pub double_buffered: bool,
    /// Whether the buffer is also a vertex stream, for the view to draw without a copy.
    pub drawable: bool,
}

/// Declares a model's storage buffers and their indices in one place.
///
/// The index is the declaration's position, so it is derived rather than written down. Flags are
/// named rather than positional, as in `agent_lanes!`, and default off. Invoke it at module scope,
/// next to the impl that forwards `BUFFERS` to `BUFFER_SPECS`.
///
/// ```ignore
/// buffers! {
///     const POS = "pos" double_buffered drawable;
///     const VEL = "vel" double_buffered;
///     const SITES = "sites";
/// }
/// ```
#[macro_export]
macro_rules! buffers {
    ($($(#[$meta:meta])* $vis:vis const $name:ident = $label:literal $($flag:ident)*;)+) => {
        $crate::__indices!(0usize, $([$(#[$meta])* $vis $name],)+);

        /// This model's storage buffers, in index order.
        const BUFFER_SPECS: &[$crate::__macro_support::BufferSpec] = &[
            $($crate::__buffer_flags!(
                $crate::__macro_support::BufferSpec {
                    label: $label,
                    double_buffered: false,
                    drawable: false,
                }; $($flag)*
            )),+
        ];
    };
}

/// Folds each named flag onto a default [`BufferSpec`]. An unknown flag fails to match here.
#[doc(hidden)]
#[macro_export]
macro_rules! __buffer_flags {
    ($spec:expr;) => { $spec };
    ($spec:expr; double_buffered $($rest:ident)*) => {
        $crate::__buffer_flags!(
            $crate::__macro_support::BufferSpec { double_buffered: true, ..$spec }; $($rest)*
        )
    };
    ($spec:expr; drawable $($rest:ident)*) => {
        $crate::__buffer_flags!(
            $crate::__macro_support::BufferSpec { drawable: true, ..$spec }; $($rest)*
        )
    };
}

/// A pass's invocation domain.
#[derive(Debug, Clone, Copy)]
pub enum Domain {
    /// One invocation per agent.
    Agents,
    /// `n` invocations per cell, for a field with `n` layers.
    Cells(u32),
    /// One invocation per agent or per cell, whichever is more, for a pass whose lanes span agents and cells.
    AgentsOrCells,
}

impl Domain {
    /// Returns the number of invocations this domain covers in `geom`.
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
    /// Name of the pass, as the Model panel and GPU labels show it.
    pub label: &'static str,
    /// WGSL source of the pass.
    ///
    /// It must declare `@workgroup_size(256)` and fold its index with `henad::dispatch::linear_index`.
    /// The engine panics at construction on another size.
    pub shader: &'static str,
    /// The shader's `@group(0)` declarations, generated from it at build time.
    pub bindings: &'static [BindingDecl],
    /// Invocation domain of the pass.
    pub domain: Domain,
}

/// Prints the shader's length, not its source.
impl std::fmt::Debug for PassSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassSpec")
            .field("label", &self.label)
            .field("shader_len", &self.shader.len())
            .field("bindings", &self.bindings)
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

/// The pass that turns state into the display texture, for a model that draws a grid layer.
///
/// The engine dispatches it over [`Geometry::display`], one invocation per texel.
pub struct DisplaySpec {
    /// WGSL source of the pass.
    pub shader: &'static str,
    /// The shader's `@group(0)` declarations, generated from it at build time.
    pub bindings: &'static [BindingDecl],
    /// Side `N` of the `@workgroup_size(N, N)` the shader declares.
    ///
    /// The engine panics at construction on a shader that declares another size.
    pub workgroup: u32,
}

/// Prints the shader's length, not its source.
impl std::fmt::Debug for DisplaySpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplaySpec")
            .field("shader_len", &self.shader.len())
            .field("bindings", &self.bindings)
            .field("workgroup", &self.workgroup)
            .finish_non_exhaustive()
    }
}

/// The leaf of the stat reduction, which folds the population down to one value per lane.
///
/// The engine owns every level above it, so the leaf only has to write `partials`. Its shader
/// imports `henad::reduce_tree::block_sum` for the workgroup fold.
pub struct ReduceSpec {
    /// WGSL source of the leaf.
    ///
    /// It must declare `@workgroup_size(256)`, as a [`PassSpec::shader`] does. The engine panics at
    /// construction on another size.
    pub shader: &'static str,
    /// The shader's `@group(0)` declarations, generated from it at build time.
    pub bindings: &'static [BindingDecl],
    /// Number of values that the leaf sums, one per lane.
    pub lanes: usize,
    /// Invocation domain of the leaf.
    pub domain: Domain,
}

/// Prints the shader's length, not its source.
impl std::fmt::Debug for ReduceSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReduceSpec")
            .field("shader_len", &self.shader.len())
            .field("bindings", &self.bindings)
            .field("lanes", &self.lanes)
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

/// A one-off pass the user can trigger.
///
/// The engine dispatches it once over its own domain, and it writes the model's buffers in place,
/// since nothing ping-pongs afterwards. Its bindings therefore resolve read and write alike to the
/// side that holds the state now.
#[derive(Debug)]
pub struct GpuAgentAction {
    /// Id and button label of the action.
    pub desc: ActionDescriptor,
    /// Pass that the action runs.
    pub pass: PassSpec,
}

/// Pass for which [`GpuAgentModel::pass_params_bytes`] returns the uniform block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassId {
    /// Entry of [`GpuAgentModel::STEP_PASSES`] at this index.
    Step(usize),
    /// The [`GpuAgentModel::DISPLAY`] pass.
    Display,
    /// The [`GpuAgentModel::REDUCE`] leaf.
    Reduce,
    /// Entry of [`GpuAgentModel::ACTIONS`] at this index.
    Action(usize),
}

/// World that the parameters describe, resolved once at construction.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// Population, at least 1.
    pub num_agents: u32,
    /// World size, at least 1 on each axis.
    pub extent: Extent,
    /// Width of the cell grid, one cell per world unit.
    pub width: u32,
    /// Height of the cell grid, one cell per world unit.
    pub height: u32,
    /// Number of cells, `width * height`.
    pub n_cells: u32,
    /// Display texture size, capped under the cell grid on a large world. A display pass
    /// dispatches over this size and reads the cell at `texel * grid / tex`.
    pub display: (u32, u32),
    /// Cell geometry of the neighbour index, set when the model declares [`GpuAgentModel::INDEX`].
    pub index: Option<HashGrid>,
}

/// Values that a pass's uniform block can need, besides the parameters.
#[derive(Clone, Copy, Debug)]
pub struct PassCtx<'a> {
    /// World the parameters describe.
    pub geom: &'a Geometry,
    /// Number of invocations the pass dispatches.
    pub invocations: u32,
    /// Fold width that `henad::dispatch::linear_index` expects. A shader that folds carries it in its
    /// uniform block.
    pub groups_x: u32,
    /// Seed of an action pass, fresh on every press, and zero for every other pass.
    pub seed: u32,
}

/// A population of agents stepped by compute shaders, with its state resident in GPU buffers.
///
/// Each pass binds by name, as the [`binding`](crate::authoring::model::binding) module describes. A
/// binding resolves by its name alone, and its declared WGSL type must match what the buffer holds.
///
/// Rust sees the shaders as opaque strings, so the compiler checks none of the contracts the items
/// below state.
pub trait GpuAgentModel: Send + Sync + 'static {
    /// Name shown in the UI.
    const NAME: &'static str;
    /// Stable id that identifies the model in a model set, on the command line and in a spec file.
    const ID: &'static str;
    /// One-line description shown in the UI.
    const DESCRIPTION: &'static str;

    /// Stat series for the history chart. Declared once, so [`Self::stats`] returns bare values.
    const STATS: &'static [StatDescriptor];

    /// Storage buffers of the model's state, declared with [`buffers!`](crate::buffers).
    const BUFFERS: &'static [BufferSpec];
    /// Index into [`Self::BUFFERS`] of the `vec2<f32>` positions that the view draws.
    const POS_BUFFER: usize;
    /// Index into [`Self::BUFFERS`] of the packed RGBA that the view draws.
    const COLOR_BUFFER: usize;

    /// Whether the engine rebuilds a neighbour index from the positions before every step.
    ///
    /// Leave it off for a model whose agents never read one another.
    const INDEX: bool = false;

    /// Number of persistent `u32` counters that a kernel accumulates into.
    ///
    /// The engine never clears them, unlike the reduction's output.
    const COUNTERS: usize = 0;

    /// Passes of one step, run in declaration order.
    const STEP_PASSES: &'static [PassSpec];
    /// Display pass, for a model that draws a grid layer under its agents.
    const DISPLAY: Option<DisplaySpec> = None;

    /// One-off passes the user can trigger. Each gets a button in the Parameters panel.
    const ACTIONS: &'static [GpuAgentAction] = &[];

    /// Leaf of the stat reduction.
    const REDUCE: ReduceSpec;

    /// Whether two builds on one seed step through identical states.
    ///
    /// A model whose passes leave the order of their writes to the GPU declares `false`. The testing kit then skips
    /// the checks that compare two runs on one seed, and the app notes that a replayed run might differ from its
    /// recorded row.
    const REPLAYS_EXACTLY: bool = true;

    /// Returns the full descriptor list.
    ///
    /// Unlike [`crate::authoring::model::agent_model::AgentModel`], nothing is prepended. A GPU model
    /// spells its list out, so it can mirror the exact parameter order of the CPU model it is compared
    /// against.
    fn param_descriptors() -> Vec<ParamDescriptor>;

    /// Returns the population and the world extent for these params. The engine clamps the population and each
    /// axis of the extent to at least 1.
    fn dims(params: &[ParamValue]) -> (u32, Extent);

    /// Returns the length in `u32`-sized elements of each buffer, one per [`Self::BUFFERS`] entry.
    fn buffer_lens(geom: &Geometry) -> Vec<usize>;

    /// Returns the initial contents of each buffer as raw bytes, one vector per [`Self::BUFFERS`] entry.
    ///
    /// A non-empty vector must hold exactly `len * 4` bytes for the length that [`Self::buffer_lens`] returns.
    /// An empty vector leaves its buffer cleared, for a scratch buffer that is read before its first write.
    /// Only the current side is seeded, and the first step writes the other side of a double buffered
    /// buffer in full.
    fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>>;

    /// Returns the neighbour index's cell size for these params.
    ///
    /// The engine reads it once at construction, and only when [`Self::INDEX`] is set.
    fn index_cell_size(_params: &[ParamValue]) -> f32 {
        1.0
    }

    /// Returns the uniform block of `pass` as raw bytes.
    ///
    /// A model fills in the `Params` struct generated from that pass's shader and returns
    /// `bytemuck::bytes_of(&params).to_vec()`.
    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8>;

    /// Turns the reduction and the counters into values, in [`Self::STATS`] order.
    ///
    /// Both inputs are all-zero until the first readback completes. The engine pairs the values with
    /// [`Self::STATS`] by position and drops the values past the shorter list. The testing kit's
    /// `StatCount` check, given a device, catches a result with fewer values.
    fn stats(sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue>;
}

#[cfg(test)]
mod tests {
    /// The macro has to expand in function scope as well as module scope (C-ANYWHERE), and an
    /// entry has to take attributes (C-MACRO-ATTR).
    #[test]
    fn buffers_macro_expands_in_function_scope() {
        crate::buffers! {
            const POS = "pos" double_buffered drawable;
            /// An entry can carry a doc comment, and flags default off.
            const SITES = "sites";
        }
        assert_eq!((POS, SITES), (0, 1), "indices follow declaration order");
        assert_eq!(BUFFER_SPECS.len(), 2);
        assert!(BUFFER_SPECS[POS].double_buffered && BUFFER_SPECS[POS].drawable);
        assert!(!BUFFER_SPECS[SITES].double_buffered && !BUFFER_SPECS[SITES].drawable);
    }
}
