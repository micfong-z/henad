use crate::params::ParamValue;
use crate::send_sync::WasmNotSend;
use crate::view::{EdgeView, GridView, PointView, StatEntry};

/// Object-safe, so the registry can type-erase every model behind one state.
pub trait SimState: WasmNotSend + 'static {
    fn step(&mut self);
    fn tick(&self) -> u64;
    /// Not exclusive with [`SimState::point_view`]. Return both to get agents drawn over a field.
    fn grid_view(&self) -> Option<GridView<'_>> {
        None
    }
    fn point_view(&self) -> Option<PointView<'_>> {
        None
    }
    /// Returns the edges to draw between the positions from [`SimState::point_view`].
    fn edge_view(&self) -> Option<EdgeView<'_>> {
        None
    }
    /// Called before a snapshot is built, so a model can turn its state into something drawable
    /// without paying for it every tick.
    fn prepare_view(&mut self) {}
    fn stats(&self) -> Vec<StatEntry>;
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool;
    /// Runs the model's action at `index`, between ticks. False when it declares no such one.
    fn act(&mut self, _index: usize) -> bool {
        false
    }
    /// Turns the layout on or off, with a time budget per publish in milliseconds.
    ///
    /// Returns false if the state has no layout.
    fn set_layout(&mut self, _on: bool, _budget_ms: f32) -> bool {
        false
    }
    /// Runs the layout for one time budget, if it is on.
    ///
    /// The runner decides which publishes call this.
    fn relax_layout(&mut self) {}
    fn population(&self) -> u64;
    /// Approximate, and only what this state owns.
    fn heap_bytes(&self) -> usize;
    /// Number of jobs one step splits into. `None` if a backend has no such split.
    fn parallel_jobs(&self) -> Option<usize> {
        None
    }
}
