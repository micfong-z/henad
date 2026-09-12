use crate::params::{ParamDescriptor, ParamValue};
use crate::send_sync::{WasmNotSend, WasmNotSync};
use crate::topology::TopologyHint;
use crate::view::{EdgeView, GridView, PointView, StatDescriptor, StatEntry};

pub trait Model: WasmNotSend + WasmNotSync + 'static {
    type State: SimState;

    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn param_descriptors(&self) -> Vec<ParamDescriptor>;
    fn stat_descriptors(&self) -> Vec<StatDescriptor>;
    fn topology_hint(&self) -> TopologyHint;
    fn create_state(&self, params: &[ParamValue]) -> Self::State;
}

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
    /// Edges drawn between [`SimState::point_view`]'s positions.
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
    /// Returns false when the state has no layout.
    fn set_layout(&mut self, _on: bool, _budget_ms: f32) -> bool {
        false
    }
    fn population(&self) -> u64;
    /// Approximate, and only what this state owns.
    fn heap_bytes(&self) -> usize;
    /// Number of jobs one step splits into. `None` if a backend has no such split.
    fn parallel_jobs(&self) -> Option<usize> {
        None
    }
}
