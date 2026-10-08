//! A GPU model's agent lanes, passed to the UI to draw in place.
//!
//! Unlike a grid, agents have nothing to rasterise first. Their lanes need `VERTEX` usage, and the colour lane holds
//! packed RGBA, since no upload step widens a palette index.

/// One side of a model's ping-ponged agent lanes.
///
/// A snapshot holds it in an `Arc`, so an in-flight paint callback keeps the buffers alive if the sim thread
/// is torn down mid-frame.
#[derive(Debug)]
pub struct GpuAgents {
    /// Positions as `array<vec2<f32>>`, one instance stream carrying both position attributes.
    pub pos: wgpu::Buffer,
    /// Colours as `array<u32>` of packed RGBA, bound as `Unorm8x4`.
    pub color: wgpu::Buffer,
    /// Number of agents.
    pub count: u32,
    /// Width of the world, for the vertex shader's world-to-clip transform.
    pub world_w: f32,
    /// Height of the world, for the vertex shader's world-to-clip transform.
    pub world_h: f32,
}
