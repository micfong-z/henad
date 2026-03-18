use crate::params::{ParamDescriptor, ParamValue};
use crate::topology::TopologyHint;
use crate::view::{GridView, PointView, StatEntry, StatsHistory};

/// Describes a simulation model and can create new simulation states.
pub trait Model: Send + Sync + 'static {
    type State: SimState;

    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn param_descriptors(&self) -> Vec<ParamDescriptor>;
    fn topology_hint(&self) -> TopologyHint;
    fn create_state(&self, params: &[ParamValue]) -> Self::State;
}

/// A running simulation state. Object-safe for type-erased model selection.
pub trait SimState: Send + 'static {
    fn step(&mut self);
    fn tick(&self) -> u64;
    fn grid_view(&self) -> Option<GridView<'_>> {
        None
    }
    fn point_view(&self) -> Option<PointView<'_>> {
        None
    }
    fn stats(&self) -> Vec<StatEntry>;
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool;
    fn get_param(&self, index: usize) -> ParamValue;
    fn population(&self) -> u64;
    fn stats_history(&self) -> &StatsHistory;
    /// Change the stats ring-buffer capacity, keeping the most recent entries.
    fn resize_history(&mut self, capacity: usize);
    /// Approximate heap bytes owned by this state (grid buffers, history, etc.).
    fn heap_bytes(&self) -> usize;
}
