//! A GPU model's agent lanes, handed to the UI to draw in place.
//!
//! Unlike a grid there is nothing to rasterise first, so lanes need `VERTEX` usage and the colour
//! lane holds packed RGBA, there being no upload step to widen an index.

/// One side of a model's ping-ponged agent lanes.
///
/// `Arc`d in a snapshot so an in-flight paint callback keeps the buffers alive if the sim thread
/// is torn down mid-frame.
pub struct GpuAgents {
    /// `array<vec2<f32>>`, one instance stream carrying both position attributes.
    pub pos: wgpu::Buffer,
    /// `array<u32>` of packed RGBA, bound as `Unorm8x4`.
    pub color: wgpu::Buffer,
    pub count: u32,
    /// For the vertex shader's world to clip transform.
    pub world_w: f32,
    pub world_h: f32,
}
