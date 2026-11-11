//! Tests of the snapshots the CPU runner builds from a state's views.

use crate::cpu::sim_thread::build_snapshot;
use crate::snapshot::SnapshotView;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::{EdgeView, GridView, PointView, StatEntry};

const PALETTE: &[[u8; 4]] = &[[1, 2, 3, 4], [5, 6, 7, 8]];

/// A model with nodes and edges.
struct Graph {
    pos: Vec<f32>,
    src: Vec<u32>,
    dst: Vec<u32>,
    color: Vec<u8>,
    version: u64,
}

impl Graph {
    fn new(edges: usize, version: u64) -> Self {
        Self {
            pos: vec![0.0; 8],
            src: (0..edges as u32).collect(),
            dst: (0..edges as u32).map(|i| i + 1).collect(),
            color: vec![0; edges],
            version,
        }
    }
}

impl SimState for Graph {
    fn step(&mut self) {}
    fn tick(&self) -> u64 {
        0
    }
    fn point_view(&self) -> Option<PointView<'_>> {
        Some(PointView {
            pos_x: &self.pos,
            pos_y: &self.pos,
            world_w: 1.0,
            world_h: 1.0,
            color: None,
            palette: PALETTE,
        })
    }
    fn edge_view(&self) -> Option<EdgeView<'_>> {
        Some(EdgeView {
            src: &self.src,
            dst: &self.dst,
            color: Some(&self.color),
            palette: PALETTE,
            directed: false,
            version: self.version,
        })
    }
    fn stats(&self) -> Vec<StatEntry> {
        Vec::new()
    }
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }
    fn population(&self) -> u64 {
        self.pos.len() as u64
    }
    fn heap_bytes(&self) -> usize {
        0
    }
}

/// A model with a field and agents. `build_snapshot` has to publish both.
///
/// The model owns its palette, and its views borrow the palette for `'a`.
struct Composite {
    cells: Vec<u8>,
    pos_x: Vec<f32>,
    pos_y: Vec<f32>,
    color: Vec<u8>,
    with_color: bool,
    palette: Vec<[u8; 4]>,
}

impl Composite {
    fn new(agents: usize, with_color: bool) -> Self {
        Self {
            cells: vec![1; 12],
            pos_x: (0..agents).map(|i| i as f32).collect(),
            pos_y: (0..agents).map(|i| i as f32 * 2.0).collect(),
            color: (0..agents).map(|i| (i % 2) as u8).collect(),
            with_color,
            palette: PALETTE.to_vec(),
        }
    }
}

impl SimState for Composite {
    fn step(&mut self) {}
    fn tick(&self) -> u64 {
        0
    }
    fn grid_view(&self) -> Option<GridView<'_>> {
        Some(GridView {
            width: 4,
            height: 3,
            cells: &self.cells,
            palette: &self.palette,
        })
    }
    fn point_view(&self) -> Option<PointView<'_>> {
        Some(PointView {
            pos_x: &self.pos_x,
            pos_y: &self.pos_y,
            world_w: 4.0,
            world_h: 3.0,
            color: self.with_color.then_some(&self.color),
            palette: &self.palette,
        })
    }
    fn stats(&self) -> Vec<StatEntry> {
        Vec::new()
    }
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }
    fn population(&self) -> u64 {
        self.pos_x.len() as u64
    }
    fn heap_bytes(&self) -> usize {
        0
    }
}

fn layers(view: &SnapshotView) -> &crate::snapshot::CpuLayers {
    match view {
        SnapshotView::Cpu(l) => l,
        SnapshotView::Gpu(_) => panic!("expected a CPU snapshot"),
    }
}

/// Publishing reaches `point_view` when there is a grid too. Otherwise a composite model silently
/// drops every agent.
#[test]
fn a_composite_model_publishes_both_layers() {
    let mut state = Composite::new(3, true);
    let snap = build_snapshot(None, &mut state, 0.0, 0.0, 0, false);
    let layers = layers(&snap.view);

    let grid = layers.grid.as_ref().expect("field layer was dropped");
    assert_eq!((grid.width, grid.height), (4, 3));
    assert_eq!(grid.cells.len(), 12);

    let points = layers.points.as_ref().expect("agent layer was dropped");
    assert_eq!(points.pos_x, vec![0.0, 1.0, 2.0]);
    assert_eq!(points.pos_y, vec![0.0, 2.0, 4.0]);
    assert_eq!(points.color, vec![0, 1, 0]);
}

/// An absent lane must arrive empty. The renderer treats an empty lane as uniform.
#[test]
fn a_model_without_a_color_lane_publishes_an_empty_one() {
    let mut state = Composite::new(2, false);
    let snap = build_snapshot(None, &mut state, 0.0, 0.0, 0, false);
    let points = layers(&snap.view).points.as_ref().expect("agent layer was dropped");
    assert!(points.color.is_empty());
    assert_eq!(points.pos_x.len(), 2);
}

/// The colour lane has to recycle alongside the position lanes.
#[test]
fn recycling_reuses_the_color_lane_across_a_length_change() {
    let mut big = Composite::new(64, true);
    let first = build_snapshot(None, &mut big, 0.0, 0.0, 0, false);
    let capacity = layers(&first.view)
        .points
        .as_ref()
        .map(|p| p.color.capacity())
        .unwrap_or_default();
    assert!(capacity >= 64);

    let mut small = Composite::new(5, true);
    let second = build_snapshot(Some(first), &mut small, 0.0, 0.0, 0, false);
    let points = layers(&second.view).points.as_ref().expect("agent layer was dropped");
    assert_eq!(points.color, vec![0, 1, 0, 1, 0]);
    assert_eq!(points.color.capacity(), capacity, "the color lane reallocated");
    assert_eq!(points.pos_x.len(), 5);
}

/// Each publish copies the palettes, and a recycled snapshot takes the copies into the room the last one left.
#[test]
fn a_snapshot_palette_is_refilled_without_reallocating() {
    let mut state = Composite::new(3, true);
    let first = build_snapshot(None, &mut state, 0.0, 0.0, 1, false);
    let grid = layers(&first.view).grid.as_ref().expect("field layer");
    let points = layers(&first.view).points.as_ref().expect("agent layer");
    let (grid_palette, point_palette) = (grid.palette.as_ptr(), points.palette.as_ptr());
    assert_eq!(grid.palette, PALETTE);

    state.palette[1] = [9, 9, 9, 255];
    let second = build_snapshot(Some(first), &mut state, 0.0, 0.0, 2, false);
    let grid = layers(&second.view).grid.as_ref().expect("field layer");
    let points = layers(&second.view).points.as_ref().expect("agent layer");
    assert_eq!(grid.palette, state.palette, "the edit did not reach the grid layer");
    assert_eq!(points.palette, state.palette, "the edit did not reach the agent layer");
    assert_eq!(grid.palette.as_ptr(), grid_palette, "the grid palette reallocated");
    assert_eq!(points.palette.as_ptr(), point_palette, "the agent palette reallocated");

    let mut graph = Graph::new(4, 1);
    let first = build_snapshot(None, &mut graph, 0.0, 0.0, 1, false);
    let edge_palette = layers(&first.view).edges.as_ref().expect("edges").palette.as_ptr();
    let second = build_snapshot(Some(first), &mut graph, 0.0, 0.0, 2, false);
    let edges = layers(&second.view).edges.as_ref().expect("edges");
    assert_eq!(edges.palette, PALETTE);
    assert_eq!(edges.palette.as_ptr(), edge_palette, "the edge palette reallocated");
}

#[test]
fn an_unchanged_edge_list_is_handed_back_untouched() {
    let mut model = Graph::new(500, 7);
    let first = build_snapshot(None, &mut model, 0.0, 0.0, 1, false);
    let ptr = layers(&first.view).edges.as_ref().expect("edges").src.as_ptr();

    // The version is unchanged, so nothing is copied.
    model.src[0] = 999;
    let second = build_snapshot(Some(first), &mut model, 0.0, 0.0, 2, false);
    let edges = layers(&second.view).edges.as_ref().expect("edges");
    assert_eq!(edges.src.as_ptr(), ptr, "the edge list reallocated");
    assert_eq!(edges.src[0], 0, "an unchanged version was copied anyway");
}

#[test]
fn a_changed_edge_list_is_refilled_into_the_same_room() {
    let mut model = Graph::new(500, 7);
    let first = build_snapshot(None, &mut model, 0.0, 0.0, 1, false);
    let capacity = layers(&first.view).edges.as_ref().expect("edges").src.capacity();

    model.version = 8;
    model.src[0] = 999;
    let second = build_snapshot(Some(first), &mut model, 0.0, 0.0, 2, false);
    let edges = layers(&second.view).edges.as_ref().expect("edges");
    assert_eq!(edges.src[0], 999, "the change did not reach the snapshot");
    assert_eq!(edges.version, 8);
    assert_eq!(edges.src.capacity(), capacity, "the edge list reallocated");
}

#[test]
fn an_edge_list_that_changed_length_is_refilled() {
    let mut model = Graph::new(500, 7);
    let first = build_snapshot(None, &mut model, 0.0, 0.0, 1, false);

    let mut shorter = Graph::new(3, 7);
    let second = build_snapshot(Some(first), &mut shorter, 0.0, 0.0, 2, false);
    let edges = layers(&second.view).edges.as_ref().expect("edges");
    assert_eq!(edges.src.len(), 3, "a shorter list was passed through whole");
}

#[test]
fn a_model_without_edges_publishes_none() {
    let mut graph = Graph::new(4, 1);
    let first = build_snapshot(None, &mut graph, 0.0, 0.0, 1, false);
    assert!(layers(&first.view).edges.is_some());

    let mut plain = Composite::new(4, true);
    let second = build_snapshot(Some(first), &mut plain, 0.0, 0.0, 2, false);
    assert!(
        layers(&second.view).edges.is_none(),
        "the edge layer outlived its model"
    );
}

#[test]
fn the_serial_is_whatever_the_publish_was_given() {
    let mut model = Graph::new(2, 1);
    assert_eq!(build_snapshot(None, &mut model, 0.0, 0.0, 41, false).serial, 41);
    assert_eq!(build_snapshot(None, &mut model, 0.0, 0.0, 42, false).serial, 42);
}
