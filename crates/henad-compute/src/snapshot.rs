use std::sync::Arc;

use henad_core::view::StatEntry;

use crate::gpu::view::agents::GpuAgents;
use crate::gpu::view::display::GpuDisplay;

/// Owned data snapshot produced by the sim thread for the UI to consume.
#[derive(Debug)]
pub struct Snapshot {
    pub tick: u64,
    pub serial: u64,
    pub population: u64,
    pub heap_bytes: usize,
    pub actual_tps: f64,
    /// Smoothed engine time per tick in milliseconds.
    pub engine_ms: f64,
    /// Time spent preparing the view for this snapshot, including any layout, in milliseconds.
    pub view_ms: f64,
    pub view: SnapshotView,
    /// Current stat values (one per stat series).
    pub stats: Vec<StatEntry>,
}

#[derive(Debug)]
pub enum SnapshotView {
    Cpu(CpuLayers),
    /// The model's state never left the GPU, so there are no cells to copy, only a texture to
    /// sample. See [`GpuSnapshot`].
    Gpu(GpuSnapshot),
}

/// A CPU model's owned layers, drawn field first and agents over the top. Both optional, so a
/// composite model can publish both.
#[derive(Debug, Default)]
pub struct CpuLayers {
    pub grid: Option<GridSnapshot>,
    pub points: Option<PointSnapshot>,
    pub edges: Option<Box<EdgeSnapshot>>,
}

impl CpuLayers {
    pub fn is_empty(&self) -> bool {
        self.grid.is_none() && self.points.is_none()
    }
}

/// A GPU model's view. No owned pixel data, just handles to what is already on the GPU.
///
/// The two layers mirror [`CpuLayers`] and composite the same way, field first and agents over
/// the top.
///
/// Held by `Arc` so an in-flight egui paint callback keeps the pipeline, texture and lane buffers
/// alive even if the sim thread is torn down mid-frame.
#[derive(Debug, Default)]
pub struct GpuSnapshot {
    /// A texture the model's display pass has already written.
    pub display: Option<Arc<GpuDisplay>>,
    /// The model's own lane buffers, drawn in place.
    pub agents: Option<Arc<GpuAgents>>,
}

impl GpuSnapshot {
    pub fn is_empty(&self) -> bool {
        self.display.is_none() && self.agents.is_none()
    }
}

/// Owned grid data, cloned from the sim state.
pub struct GridSnapshot {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<u8>,
    pub palette: &'static [[u8; 4]],
}

/// Prints the grid's size, not its cells.
impl std::fmt::Debug for GridSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridSnapshot")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("palette_len", &self.palette.len())
            .finish_non_exhaustive()
    }
}

/// Owned edge list, cloned from the sim state.
#[derive(Default)]
pub struct EdgeSnapshot {
    /// Graph version at the time the list was copied.
    pub version: u64,
    pub src: Vec<u32>,
    pub dst: Vec<u32>,
    /// One palette index per edge, or empty if every edge has the same colour.
    pub color: Vec<u8>,
    pub palette: &'static [[u8; 4]],
    pub directed: bool,
}

/// Prints the edge count and the version, not the edges.
impl std::fmt::Debug for EdgeSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeSnapshot")
            .field("version", &self.version)
            .field("len", &self.src.len())
            .field("colored", &!self.color.is_empty())
            .field("palette_len", &self.palette.len())
            .field("directed", &self.directed)
            .finish_non_exhaustive()
    }
}

/// Owned point cloud data, cloned from the sim state.
pub struct PointSnapshot {
    pub pos_x: Vec<f32>,
    pub pos_y: Vec<f32>,
    pub world_w: f32,
    pub world_h: f32,
    /// One palette index per agent. Empty means uniform, so `refill` can recycle it like the rest.
    pub color: Vec<u8>,
    pub palette: &'static [[u8; 4]],
}

/// Prints the point count and the world, not the positions.
impl std::fmt::Debug for PointSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointSnapshot")
            .field("len", &self.pos_x.len())
            .field("world_w", &self.world_w)
            .field("world_h", &self.world_h)
            .field("colored", &!self.color.is_empty())
            .field("palette_len", &self.palette.len())
            .finish_non_exhaustive()
    }
}
