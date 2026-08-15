//! Authoring API for grid models whose state lives entirely in GPU buffers.
//!
//! This is the GPU sibling of [`crate::authoring::grid_model::GridModel`], and it plays the same role: a
//! model declares const metadata plus a few pure functions, and the engine
//! (`henad_compute::gpu::grid_engine`) derives every buffer, layout, pipeline, bind group, and
//! the whole `SimState`/`GpuSimState` impl from them.
//!
//! # Binding conventions
//!
//! The engine generates bind group layouts from [`GpuGridModel::BUFFER_COUNT`] alone, so a
//! model's shaders must declare exactly these bindings, all in `@group(0)`:
//!
//! **`STEP_SHADER`**, for `K = BUFFER_COUNT` ping-ponged buffers. Bindings `0..2K` are
//! interleaved read/write pairs, and binding `2K` is the step uniform.
//!
//! ```wgsl
//! @group(0) @binding(0) var<storage, read>       buf0_in;   // buffer 0, current
//! @group(0) @binding(1) var<storage, read_write> buf0_out;  // buffer 0, next
//! @group(0) @binding(2) var<storage, read>       buf1_in;   // buffer 1, current  (K >= 2)
//! @group(0) @binding(3) var<storage, read_write> buf1_out;  // buffer 1, next     (K >= 2)
//! @group(0) @binding(4) var<uniform>             params;    // at binding 2K
//! ```
//!
//! All `K` buffers are ping-ponged together, in lockstep. A step reads every buffer's current
//! side and writes every buffer's next side.
//!
//! **`DISPLAY_SHADER`** and **`REDUCE_SHADER`** see only buffer 0, the *primary* state buffer.
//! Auxiliary buffers, a per-cell RNG say, are step-private.
//!
//! # Buffer length and dispatch domain
//!
//! The engine takes no view on how a model maps cells onto `u32`s. A model declares how long each
//! buffer is ([`GpuGridModel::buffer_lens`]) and how many invocations its step needs
//! ([`GpuGridModel::step_dims`]). Both default to one `u32` and one invocation per cell, which is
//! what an unpacked model wants. A bit-packed model overrides them to work in words instead, say
//! 32 cells per `u32` with rows padded to whole words, so that a single invocation owns a whole
//! word and no two invocations ever write the same one.
//!
//! Reduce always dispatches one invocation per *cell*, so a packed model's reduce shader reads a
//! word and extracts its own bit.
//!
//! # Display is a sampled view, not a mirror
//!
//! The display texture is capped well under the grid (`henad_compute::display_scale`). Display
//! therefore dispatches one invocation per *texel*, reading the cell at `texel * grid / tex`. The
//! two pairs of dimensions are equal until the grid outgrows the cap.
//!
//! ```wgsl
//! struct Dims {
//!     grid: vec2<u32>,
//!     tex: vec2<u32>,
//! }
//!
//! // display.wgsl
//! @group(0) @binding(0) var<storage, read> state: array<u32>;
//! @group(0) @binding(1) var out_tex: texture_storage_2d<rgba8unorm, write>;
//! @group(0) @binding(2) var<uniform> dims: Dims;
//!
//! // reduce.wgsl
//! @group(0) @binding(0) var<storage, read> state: array<u32>;
//! @group(0) @binding(1) var<storage, read_write> totals: array<atomic<u32>, STAT_COUNT>;
//! @group(0) @binding(2) var<uniform> dims: Dims;
//! ```
//!
//! # Unchecked contracts
//!
//! Nothing here can be verified at compile time. The shaders are opaque strings to Rust, so a
//! mismatch surfaces as a wgpu validation error at model construction. A valid model has all of
//! the following true.
//! - [`Self::WORKGROUP_SIZE`] must equal the `@workgroup_size(N, N)` all three shaders declare,
//! - [`Self::STATS`] length must equal the reduce shader's `atomic<u32>` array length and the
//!   number of entries [`Self::stats`] returns,
//! - [`Self::buffer_lens`] must return exactly [`Self::BUFFER_COUNT`] lengths, and
//!   [`Self::seed_buffers`] exactly that many vectors, of exactly those lengths.

use crate::params::{ParamDescriptor, ParamValue};
use crate::view::{StatDescriptor, StatValue};

/// A grid model stepped by a compute shader, with its state resident in GPU storage buffers.
///
/// See the module docs for the binding conventions the shaders must follow.
pub trait GpuGridModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;

    /// Palette used by the stats UI. The display shader writes RGBA directly, so it has its own
    /// copy of the colours, and keeping the two in agreement is on the model.
    const PALETTE: &'static [[u8; 4]];

    /// Must match the `@workgroup_size(N, N)` declared by all three shaders.
    const WORKGROUP_SIZE: u32 = 16;

    /// Buffers ping-ponged per step. `1` for a plain state buffer, `2` for a model that also
    /// carries per-cell RNG state.
    const BUFFER_COUNT: usize = 1;

    /// Stat series for the history chart. Its length is how many `u32` counters the reduce shader
    /// accumulates.
    const STATS: &'static [StatDescriptor];

    /// WGSL source for the compute shaders.
    const STEP_SHADER: &'static str;
    const DISPLAY_SHADER: &'static str;
    const REDUCE_SHADER: &'static str;

    /// The full descriptor list. Unlike [`crate::authoring::grid_model::GridModel`], width and
    /// height are *not* prepended. A GPU model spells its list out, so it can mirror the exact
    /// parameter order of the CPU model it is compared against.
    fn param_descriptors() -> Vec<ParamDescriptor>;

    /// Grid dimensions for these params. The engine clamps both to at least 1.
    fn dims(params: &[ParamValue]) -> (u32, u32);

    /// Length in `u32` elements of each ping-ponged buffer, in binding order.
    ///
    /// Defaults to one element per cell. A bit-packed model overrides this to return its word
    /// count, and must override [`Self::step_dims`] to match, so that one invocation owns one word.
    fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
        vec![(width as usize) * (height as usize); Self::BUFFER_COUNT]
    }

    /// Dispatch domain of the step pass, in invocations.
    ///
    /// Defaults to one invocation per cell. Reduce always dispatches `(width, height)` and display
    /// one per texel, so neither is affected by this.
    fn step_dims(width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    /// Initial contents of each ping-ponged buffer, CPU-seeded and uploaded once at construction.
    ///
    /// Returns [`Self::BUFFER_COUNT`] vectors, whose lengths match [`Self::buffer_lens`], in
    /// binding order. Index 0 is the primary state buffer that display and reduce read.
    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>>;

    /// The step shader's uniform block, as raw bytes.
    ///
    /// Bytes rather than a `bytemuck::Pod` bound because `henad-core` has no bytemuck. Models keep
    /// their own `#[repr(C)]` struct and hand over `bytemuck::bytes_of(&s).to_vec()`. A model
    /// whose step needs nothing but the dimensions can return the dims themselves.
    fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8>;

    /// Turn the counters read back from the reduce shader into values, in [`Self::STATS`] order.
    ///
    /// `counts` has `STATS.len()` entries, and is all-zero until the first readback completes.
    fn stats(counts: &[u32]) -> Vec<StatValue>;
}
