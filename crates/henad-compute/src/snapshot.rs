use std::sync::Arc;

use henad_core::view::StatEntry;

use crate::gpu::display::GpuDisplay;

/// Owned data snapshot produced by the sim thread for the UI to consume.
pub struct Snapshot {
    pub tick: u64,
    pub population: u64,
    pub heap_bytes: usize,
    pub actual_tps: f64,
    /// Smoothed engine time per tick in milliseconds.
    pub engine_ms: f64,
    pub view: SnapshotView,
    /// Current stat values (one per stat series).
    pub stats: Vec<StatEntry>,
}

pub enum SnapshotView {
    Cpu(CpuLayers),
    /// The model's state never left the GPU — there are no cells to copy, only a texture to
    /// sample. See [`GpuSnapshot`].
    Gpu(GpuSnapshot),
}

/// A CPU model's owned layers, drawn field first and agents over the top. Both optional, so a
/// composite model can publish both.
#[derive(Default)]
pub struct CpuLayers {
    pub grid: Option<GridSnapshot>,
    pub points: Option<PointSnapshot>,
}

impl CpuLayers {
    pub fn is_empty(&self) -> bool {
        self.grid.is_none() && self.points.is_none()
    }
}

/// A GPU model's view: no owned pixel data at all, just a handle to the already-rendered display
/// texture and the pipeline that samples it.
///
/// This is the reason the GPU display path goes through `Snapshot` rather than through
/// `SimState::grid_view()` / `View`: those live in `henad-core`, which must never see a `wgpu`
/// type. `Snapshot` lives here in `henad-compute`, which does depend on wgpu, so it is the
/// natural seam. CPU models keep producing the owned [`GridSnapshot`]/[`PointSnapshot`] exactly
/// as before; a GPU model skips `grid_view()` entirely.
///
/// Held by `Arc` so an in-flight egui paint callback keeps the pipeline and texture alive even if
/// the sim thread is torn down mid-frame (e.g. the user switches models).
pub struct GpuSnapshot {
    pub display: Arc<GpuDisplay>,
}

/// Owned grid data — cells are cloned from the sim state.
pub struct GridSnapshot {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<u8>,
    pub palette: &'static [[u8; 4]],
}

/// Owned point cloud data — positions are cloned from the sim state.
pub struct PointSnapshot {
    pub pos_x: Vec<f32>,
    pub pos_y: Vec<f32>,
    pub world_w: f32,
    pub world_h: f32,
    /// One palette index per agent. Empty means uniform, so `refill` can recycle it like the rest.
    pub color: Vec<u8>,
    pub palette: &'static [[u8; 4]],
}
