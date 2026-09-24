//! A GPU network's edge list, handed to the UI to draw in place.

/// The edge list of a network whose graph lives on the GPU.
///
/// `Arc`d in a snapshot so an in-flight paint callback keeps the buffer alive if the sim thread is torn down mid-frame.
pub struct GpuEdges {
    /// `array<Edge>` of `(src, dst, color)` words, bound as an instance buffer.
    pub edges: wgpu::Buffer,
    pub count: u32,
    pub directed: bool,
}
