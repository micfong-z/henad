use henad_core::view::StatEntry;

/// Owned data snapshot produced by the sim thread for the UI to consume.
pub struct Snapshot {
    pub tick: u64,
    pub population: u64,
    pub heap_bytes: usize,
    pub actual_tps: f64,
    /// Mutually exclusive: either grid data or point data.
    pub view: SnapshotView,
    /// Current stat values (one per stat series).
    pub stats: Vec<StatEntry>,
}

pub enum SnapshotView {
    Grid(GridSnapshot),
    Points(PointSnapshot),
    None,
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
