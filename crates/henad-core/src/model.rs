//! The [`SimState`] interface the runner drives.

use crate::params::ParamValue;
use crate::send_sync::WasmNotSend;
use crate::view::{EdgeView, GridView, PointView, StatEntry};

/// A running simulation, as the runner drives it.
///
/// The trait is object-safe, and a model entry type-erases every model behind it. A model implements
/// one of the authoring traits, and the engine implements this trait.
pub trait SimState: WasmNotSend + 'static {
    /// Advances the state by one tick.
    fn step(&mut self);
    /// Number of ticks stepped so far.
    fn tick(&self) -> u64;
    /// Returns the grid layer to draw, if the state has one.
    ///
    /// A state can return both a grid and agents from [`SimState::point_view`], and the agents are then drawn
    /// over the field.
    fn grid_view(&self) -> Option<GridView<'_>> {
        None
    }
    /// Returns the agents to draw, if the state has any.
    fn point_view(&self) -> Option<PointView<'_>> {
        None
    }
    /// Returns the edges to draw between the positions from [`SimState::point_view`].
    fn edge_view(&self) -> Option<EdgeView<'_>> {
        None
    }
    /// Prepares the views before a snapshot is built.
    ///
    /// A model turns its state into something drawable here, without paying for it every tick.
    fn prepare_view(&mut self) {}
    /// Returns the current statistics, one entry per declared series.
    fn stats(&self) -> Vec<StatEntry>;
    /// Sets parameter `index` to `value`, and returns whether the state accepted the edit.
    ///
    /// A state rejects an edit to a parameter declared [`ParamApply::OnReload`].
    ///
    /// [`ParamApply::OnReload`]: crate::params::ParamApply::OnReload
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool;
    /// Runs the model's action at `index`, between ticks, and returns whether it ran.
    ///
    /// It returns false when the model declares no such action.
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
    /// Size of the population: its cells, its agents or its live nodes.
    fn population(&self) -> u64;
    /// Approximate size in bytes of the memory this state owns, on the host for a CPU state and on the device
    /// for a GPU state.
    fn heap_bytes(&self) -> usize;
    /// Number of jobs that one step splits into. `None` if a backend has no such split.
    fn parallel_jobs(&self) -> Option<usize> {
        None
    }
}
