//! Authoring API for grid models whose state lives entirely in GPU buffers.
//!
//! [`GpuGridModel`] is the GPU sibling of [`GridModel`]. A model declares const metadata, three WGSL
//! passes and a few pure functions, and the engine in henad-compute derives every buffer, pipeline and
//! bind group from them. The [GPU grid models](https://micfong-z.github.io/henad/authoring/gpu-grid-models/)
//! page walks through the passes, the bindings and the sampled display texture.
//!
//! [`GridModel`]: crate::authoring::model::grid_model::GridModel

use crate::action::ActionDescriptor;
use crate::authoring::model::binding::BindingDecl;
use crate::params::{ParamDescriptor, ParamValue};
use crate::view::{StatDescriptor, StatValue};

/// A one-off compute pass the user can trigger.
///
/// The engine dispatches it over [`GpuGridModel::step_dims`] like a step, and it writes the current
/// side in place, since nothing ping-pongs afterwards. Its bindings therefore resolve read and write
/// alike to the side that holds the state now.
pub struct GpuGridAction {
    /// Id and button label of the action.
    pub desc: ActionDescriptor,
    /// WGSL source of the pass.
    pub shader: &'static str,
    /// The shader's `@group(0)` declarations, generated from it at build time.
    pub bindings: &'static [BindingDecl],
}

/// Prints the shader's length, not its source.
impl std::fmt::Debug for GpuGridAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuGridAction")
            .field("desc", &self.desc)
            .field("shader_len", &self.shader.len())
            .field("bindings", &self.bindings)
            .finish_non_exhaustive()
    }
}

/// A grid model stepped by a compute shader, with its state resident in GPU storage buffers.
///
/// All [`Self::BUFFERS`] ping-pong together. A step reads every buffer's current side and writes every
/// buffer's next side. Each pass binds a buffer by its label, with an optional `_in` or `_out` suffix,
/// and the access mode picks the side. A `read` binding gets the current side, and a `read_write` binding
/// gets the next side. The [`binding`](crate::authoring::model::binding) module lists the reserved names.
///
/// Rust sees the shaders as opaque strings, so the compiler checks none of the contracts the items
/// below state.
pub trait GpuGridModel: Send + Sync + 'static {
    /// Name shown in the UI.
    const NAME: &'static str;
    /// Stable id that identifies the model in a model set, on the command line and in a spec file.
    const ID: &'static str;
    /// One-line description shown in the UI.
    const DESCRIPTION: &'static str;

    /// Palette used by the stats UI. The display shader writes RGBA directly, so it has its own
    /// copy of the colours, and the model must keep both copies in agreement.
    const PALETTE: &'static [[u8; 4]];

    /// Side `N` of the `@workgroup_size(N, N)` every shader declares, actions included.
    ///
    /// The engine panics at construction on a shader that declares another literal size.
    const WORKGROUP_SIZE: u32 = 16;

    /// Stat series for the history chart.
    ///
    /// Its length must equal the number of `atomic<u32>` in the reduce shader's `counters` binding. A
    /// shorter array validates, and the stats past its end read zero.
    const STATS: &'static [StatDescriptor];

    /// One-off passes the user can trigger. Each gets a button in the Parameters panel.
    const ACTIONS: &'static [GpuGridAction] = &[];

    /// Labels of the buffers ping-ponged per step. A shader's binding names refer to these labels.
    ///
    /// A plain model has one label, and a model that also carries per-cell RNG state has two. A label
    /// must not be reserved or end in `_in` or `_out`. The engine panics at construction on such a label.
    const BUFFERS: &'static [&'static str];

    // Each pass's `@group(0)` declarations, generated from its shader.
    /// Declarations of the step shader.
    const STEP_BINDINGS: &'static [BindingDecl];
    /// Declarations of the display shader.
    const DISPLAY_BINDINGS: &'static [BindingDecl];
    /// Declarations of the reduce shader.
    const REDUCE_BINDINGS: &'static [BindingDecl];

    // WGSL source for the compute shaders.
    /// Step shader, dispatched over [`Self::step_dims`].
    const STEP_SHADER: &'static str;
    /// Display shader, dispatched once per display texel.
    ///
    /// The display texture is capped well under the largest grids, so a texel reads the cell at
    /// `texel * grid / tex`. The `Dims` uniform of `henad::dims` carries both sizes.
    const DISPLAY_SHADER: &'static str;
    /// Reduce shader, dispatched once per cell.
    ///
    /// A bit-packed model's reduce shader reads a word and extracts its own bit.
    const REDUCE_SHADER: &'static str;

    /// Whether two builds on one seed step through identical states.
    ///
    /// A model whose passes leave the order of their writes to the GPU declares `false`. The testing kit then skips
    /// the checks that compare two runs on one seed, and the app notes that a replayed run might differ from its
    /// recorded row.
    const REPLAYS_EXACTLY: bool = true;

    /// Returns the full descriptor list.
    ///
    /// Unlike [`crate::authoring::model::grid_model::GridModel`], width and height are *not* prepended. A
    /// GPU model spells its list out, so it can mirror the exact parameter order of the CPU model it is
    /// compared against.
    fn param_descriptors() -> Vec<ParamDescriptor>;

    /// Returns the grid dimensions for these params. The engine clamps each dimension to at least 1.
    fn dims(params: &[ParamValue]) -> (u32, u32);

    /// Returns the length in `u32` elements of each ping-ponged buffer, in [`Self::BUFFERS`] order.
    ///
    /// The default is one element per cell. A bit-packed model overrides this to return its word
    /// count, and must override [`Self::step_dims`] to match, so that one invocation owns one word.
    /// The engine asserts at construction that the result holds one length per buffer.
    fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
        vec![(width as usize) * (height as usize); Self::BUFFERS.len()]
    }

    /// Returns the dispatch domain of the step pass, in invocations.
    ///
    /// The default is one invocation per cell. Reduce always dispatches `(width, height)`, and display
    /// dispatches one invocation per texel.
    fn step_dims(width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    /// Returns the initial contents of each ping-ponged buffer, uploaded once at construction.
    ///
    /// The result holds one vector per [`Self::BUFFERS`] label, in that order, each with the length that
    /// [`Self::buffer_lens`] returns. The engine asserts the count and the lengths at construction. Index 0 is the
    /// primary state buffer that the shipped display and reduce shaders read.
    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>>;

    /// Returns the step shader's uniform block as raw bytes.
    ///
    /// A model fills in the `Params` struct generated from its step shader and returns
    /// `bytemuck::bytes_of(&params).to_vec()`. A model whose step needs nothing but the dimensions can
    /// return the dimensions themselves.
    fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8>;

    /// Returns an action's uniform block as raw bytes.
    ///
    /// `seed` is fresh on every press, so a shader that draws gets a new stream each time. The default
    /// returns the step's block, enough for an action that needs nothing but the dimensions.
    fn action_params_bytes(_action: usize, width: u32, height: u32, params: &[ParamValue], _seed: u32) -> Vec<u8> {
        Self::step_params_bytes(width, height, params)
    }

    /// Turns the counters read back from the reduce shader into values, in [`Self::STATS`] order.
    ///
    /// `counts` has `STATS.len()` entries, and is all-zero until the first readback completes. The
    /// engine pairs the values with [`Self::STATS`] by position and drops the values past the shorter
    /// list. The testing kit's `StatCount` check, given a device, catches a result with fewer values.
    fn stats(counts: &[u32]) -> Vec<StatValue>;
}
