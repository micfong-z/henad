//! Authoring API for grid models whose state lives entirely in GPU buffers.
//!
//! This is the GPU sibling of [`crate::grid_model::GridModel`], and it plays the same role: a
//! model declares const metadata plus a few pure functions, and the engine
//! (`henad_compute::gpu::gpu_grid_engine`) derives every buffer, layout, pipeline, bind group, and
//! the whole `SimState`/`GpuSimState` impl from them.
//!
//! # Binding conventions
//!
//! The engine generates bind group layouts from [`GpuGridModel::BUFFER_COUNT`] alone, so a
//! model's shaders must declare exactly these bindings, all in `@group(0)`:
//!
//! **`STEP_SHADER`** — for `K = BUFFER_COUNT` ping-ponged buffers, bindings `0..2K` are
//! interleaved read/write pairs, and binding `2K` is the step uniform:
//!
//! ```wgsl
//! @group(0) @binding(0) var<storage, read>       buf0_in;   // buffer 0, current
//! @group(0) @binding(1) var<storage, read_write> buf0_out;  // buffer 0, next
//! @group(0) @binding(2) var<storage, read>       buf1_in;   // buffer 1, current  (K >= 2)
//! @group(0) @binding(3) var<storage, read_write> buf1_out;  // buffer 1, next     (K >= 2)
//! @group(0) @binding(4) var<uniform>             params;    // at binding 2K
//! ```
//!
//! All `K` buffers are ping-ponged together, in lockstep: a step reads every buffer's current
//! side and writes every buffer's next side.
//!
//! **`DISPLAY_SHADER`** and **`REDUCE_SHADER`** see only buffer 0 — the *primary* state buffer.
//! Auxiliary buffers (a per-cell RNG, say) are step-private:
//!
//! ```wgsl
//! // display.wgsl
//! @group(0) @binding(0) var<storage, read> state: array<u32>;
//! @group(0) @binding(1) var out_tex: texture_storage_2d<rgba8unorm, write>;
//! @group(0) @binding(2) var<uniform> dims: vec2<u32>;
//!
//! // reduce.wgsl
//! @group(0) @binding(0) var<storage, read> state: array<u32>;
//! @group(0) @binding(1) var<storage, read_write> totals: array<atomic<u32>, STAT_COUNT>;
//! @group(0) @binding(2) var<uniform> dims: vec2<u32>;
//! ```
//!
//! # Unchecked contracts
//!
//! Nothing here can be verified at compile time — the shaders are opaque strings to Rust, so a
//! mismatch surfaces as a wgpu validation error at model construction. The following must be true for
//! a model to be valid:
//! - [`Self::WORKGROUP_SIZE`] must equal the `@workgroup_size(N, N)` all three shaders declare,
//! - [`Self::STAT_COUNT`] must equal the reduce shader's `atomic<u32>` array length and the number
//!   of entries [`Self::stats`] and [`Self::stat_descriptors`] return,
//! - [`Self::seed_buffers`] must return exactly [`Self::BUFFER_COUNT`] vectors of `width * height` elements each.

use crate::params::{ParamDescriptor, ParamValue};
use crate::view::{StatDescriptor, StatEntry};

/// A grid model stepped by a compute shader, with its state resident in GPU storage buffers.
///
/// See the module docs for the binding conventions the shaders must follow.
pub trait GpuGridModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;

    /// Palette used by the stats UI. The display shader has its own copy of the colours, since it
    /// writes RGBA directly; keeping the two in agreement is on the model.
    const PALETTE: &'static [[u8; 4]];

    /// Must match the `@workgroup_size(N, N)` declared by all three shaders.
    const WORKGROUP_SIZE: u32 = 16;

    /// How many `array<u32>` buffers are ping-ponged per step. `1` for a plain state buffer; `2`
    /// for a model that also carries per-cell RNG state.
    const BUFFER_COUNT: usize = 1;

    /// How many `u32` counters the reduce shader accumulates.
    const STAT_COUNT: usize;

    /// WGSL source for the compute shaders.
    const STEP_SHADER: &'static str;
    const DISPLAY_SHADER: &'static str;
    const REDUCE_SHADER: &'static str;

    /// Declare parameters for the UI. Unlike [`crate::grid_model::GridModel`], width and height
    /// are *not* auto-prepended — a GPU model spells out its full list, so that it can mirror the
    /// exact parameter order of the CPU model it is compared against.
    fn param_descriptors() -> Vec<ParamDescriptor>;

    /// Grid dimensions for these params. The engine clamps both to at least 1.
    fn dims(params: &[ParamValue]) -> (u32, u32);

    /// Initial contents of each ping-ponged buffer, CPU-seeded and uploaded once at construction.
    ///
    /// Returns [`Self::BUFFER_COUNT`] vectors, each of `width * height` elements, in binding
    /// order — index 0 is the primary state buffer that display and reduce read.
    fn seed_buffers(width: u32, height: u32, params: &[ParamValue]) -> Vec<Vec<u32>>;

    /// The step shader's uniform block, as raw bytes.
    ///
    /// Bytes rather than a `bytemuck::Pod` bound because `henad-core` has no bytemuck; models keep
    /// their own `#[repr(C)]` struct and hand over `bytemuck::bytes_of(&s).to_vec()`. A model
    /// whose step needs nothing but the dimensions can return the dims themselves.
    fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8>;

    /// Declare stat series for the history chart. Must have [`Self::STAT_COUNT`] entries.
    fn stat_descriptors() -> Vec<StatDescriptor>;

    /// Turn the counters read back from the reduce shader into displayable stats.
    ///
    /// `counts` has [`Self::STAT_COUNT`] entries, and is all-zero until the first readback
    /// completes.
    fn stats(counts: &[u32]) -> Vec<StatEntry>;
}
