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
    /// Mutually exclusive: either grid data or point data.
    pub view: SnapshotView,
    /// Current stat values (one per stat series).
    pub stats: Vec<StatEntry>,
}

pub enum SnapshotView {
    Grid(GridSnapshot),
    Points(PointSnapshot),
    /// The model's state never left the GPU — there are no cells to copy, only a texture to
    /// sample. See [`GpuSnapshot`].
    Gpu(GpuSnapshot),
    None,
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
    pub palette: &'static [u8; 4],
}
