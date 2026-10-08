//! Snapshots a runner publishes for a host to draw: the tick, the stats and the model's view layers.
//!
//! A CPU model's layers are owned copies of its state. A GPU model's layers are handles to buffers and textures that
//! stay on the device.

use std::sync::Arc;

use henad_core::view::StatEntry;

use crate::gpu::view::agents::GpuAgents;
use crate::gpu::view::display::GpuDisplay;

/// Owned data snapshot produced by the sim thread for the UI to consume.
#[derive(Debug)]
pub struct Snapshot {
    /// Number of ticks stepped so far.
    pub tick: u64,
    /// Number of the runner's publishes so far, raised by one on each publish.
    pub serial: u64,
    /// Size of the population: its cells, its agents or its live nodes.
    pub population: u64,
    /// Approximate size in bytes of the state, on the heap for a CPU model and on the device for a GPU model.
    pub heap_bytes: usize,
    /// Measured rate in ticks per second, 0 while paused.
    pub actual_tps: f64,
    /// Smoothed engine time per tick in milliseconds.
    pub engine_ms: f64,
    /// Time spent preparing the view for this snapshot, including any layout, in milliseconds.
    pub view_ms: f64,
    /// Layers the host draws.
    pub view: SnapshotView,
    /// Current stat values, one per stat series.
    pub stats: Vec<StatEntry>,
}

/// View layers of a snapshot, copied from a CPU model or held on the GPU.
#[derive(Debug)]
pub enum SnapshotView {
    /// Layers copied from a CPU model's state.
    Cpu(CpuLayers),
    /// Handles to layers on the GPU. The model's state never leaves the GPU, so there are no cells to copy, only a
    /// texture to sample. See [`GpuSnapshot`].
    Gpu(GpuSnapshot),
}

/// A CPU model's owned layers, drawn field first, then the edges, then the agents over the top.
///
/// Each layer is optional, and a composite model publishes several.
#[derive(Debug, Default)]
pub struct CpuLayers {
    /// Grid, from [`SimState::grid_view`](henad_core::model::SimState::grid_view).
    pub grid: Option<GridSnapshot>,
    /// Agents or nodes, from [`SimState::point_view`](henad_core::model::SimState::point_view).
    pub points: Option<PointSnapshot>,
    /// Edges between the points, from [`SimState::edge_view`](henad_core::model::SimState::edge_view).
    pub edges: Option<Box<EdgeSnapshot>>,
}

impl CpuLayers {
    /// Returns whether the layers hold neither a grid nor points.
    pub fn is_empty(&self) -> bool {
        self.grid.is_none() && self.points.is_none()
    }
}

/// A GPU model's view, as handles to what is already on the GPU, with no owned pixel data.
///
/// The two layers mirror [`CpuLayers`] and composite the same way, field first and agents over
/// the top.
///
/// Each layer is held by `Arc`. An in-flight egui paint callback then keeps the pipeline, texture and lane buffers
/// alive even if the sim thread is torn down mid-frame.
#[derive(Debug, Default)]
pub struct GpuSnapshot {
    /// A texture the model's display pass has already written.
    pub display: Option<Arc<GpuDisplay>>,
    /// The model's own lane buffers, drawn in place.
    pub agents: Option<Arc<GpuAgents>>,
}

impl GpuSnapshot {
    /// Returns whether the snapshot holds neither layer.
    pub fn is_empty(&self) -> bool {
        self.display.is_none() && self.agents.is_none()
    }
}

/// Owned grid data, cloned from the sim state.
pub struct GridSnapshot {
    /// Width of the grid in cells.
    pub width: u32,
    /// Height of the grid in cells.
    pub height: u32,
    /// Palette index of each cell, row by row.
    pub cells: Vec<u8>,
    /// RGBA colour of each palette index.
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
    /// Source node of each edge, an index into the point layer.
    pub src: Vec<u32>,
    /// Destination node of each edge, an index into the point layer.
    pub dst: Vec<u32>,
    /// One palette index per edge, or empty if every edge has the same colour.
    pub color: Vec<u8>,
    /// RGBA colour of each palette index.
    pub palette: &'static [[u8; 4]],
    /// Whether the edges are directed.
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
    /// Position of each point along x.
    pub pos_x: Vec<f32>,
    /// Position of each point along y.
    pub pos_y: Vec<f32>,
    /// Width of the world the positions lie in.
    pub world_w: f32,
    /// Height of the world the positions lie in.
    pub world_h: f32,
    /// One palette index per agent, or empty if every agent has the same colour. An empty `Vec` represents no lane,
    /// so the buffer recycles like the rest.
    pub color: Vec<u8>,
    /// RGBA colour of each palette index.
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
