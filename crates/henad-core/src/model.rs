use crate::params::{ParamDescriptor, ParamValue};
use crate::topology::TopologyHint;
use crate::view::{GridView, PointView, StatDescriptor, StatEntry};

/// Describes a simulation model and can create new simulation states.
pub trait Model: Send + Sync + 'static {
    type State: SimState;

    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn param_descriptors(&self) -> Vec<ParamDescriptor>;
    fn stat_descriptors(&self) -> Vec<StatDescriptor>;
    fn topology_hint(&self) -> TopologyHint;
    fn create_state(&self, params: &[ParamValue]) -> Self::State;
}

/// A running simulation state. Object-safe for type-erased model selection.
pub trait SimState: Send + 'static {
    fn step(&mut self);
    fn tick(&self) -> u64;
    /// Not exclusive with [`SimState::point_view`]. Return both to get agents drawn over a field.
    fn grid_view(&self) -> Option<GridView<'_>> {
        None
    }
    fn point_view(&self) -> Option<PointView<'_>> {
        None
    }
    /// Called before a snapshot is built, so a model can turn its state into something drawable
    /// without paying for it every tick.
    fn prepare_view(&mut self) {}
    fn stats(&self) -> Vec<StatEntry>;
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool;
    fn population(&self) -> u64;
    /// Approximate heap bytes owned by this state (grid buffers, etc.).
    fn heap_bytes(&self) -> usize;
}
