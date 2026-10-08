//! Authoring API for models whose state is a population of agents.

use crate::action::ActionDescriptor;
use crate::authoring::model::field::{Extent, FieldLayer};
use crate::metadata::LaneSpec;
use crate::params::{ParamDescriptor, ParamValue};
use crate::spatial_hash::SpatialHash;
use crate::view::{StatDescriptor, StatValue};

/// Struct-of-arrays agent storage.
///
/// The `agent_lanes!` macro writes the impl, and adds the chunked step driver `run_pass` to
/// the generated type as an inherent method.
pub trait AgentLanes: Send + Sync + 'static {
    /// Lanes as declared, for the Model panel.
    const LANES: &'static [LaneSpec];

    /// Allocates `n` agents, each lane at its initial value.
    fn alloc(n: usize) -> Self;
    /// Number of agent slots.
    fn len(&self) -> usize;
    /// Returns whether there are no agent slots.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Swaps the double buffered lanes. It does nothing when no lane is double buffered.
    fn swap(&mut self);
    /// Heap memory held by the lanes, in bytes.
    fn heap_bytes(&self) -> usize;

    /// Returns the position lanes, `pos_x` and `pos_y`.
    ///
    /// The engine builds the neighbour index and the point view from them.
    fn positions(&self) -> (&[f32], &[f32]);
    /// Returns the position lanes, mutably.
    fn positions_mut(&mut self) -> (&mut [f32], &mut [f32]);
    /// Extends every lane to length `n`, filling new slots as [`Self::alloc`] does.
    ///
    /// Note that this never shrinks the lanes.
    fn grow(&mut self, n: usize);

    /// Returns one palette index per agent, or `None` to colour the whole population `PALETTE[0]`.
    fn colors(&self) -> Option<&[u8]> {
        None
    }
}

/// Neighbour lookup rebuilt from agent positions each tick.
pub trait NeighborIndex: Send + Sync + 'static {
    /// Name of this index in the Model panel.
    const KIND: &'static str;

    /// Creates an empty index over `extent`, with cells `cell_size` wide.
    fn new(extent: Extent, cell_size: f32) -> Self;
    /// Rebuilds the index from agent positions, with cells `cell_size` wide.
    fn rebuild(&mut self, pos_x: &[f32], pos_y: &[f32], cell_size: f32);
    /// Heap memory held by the index, in bytes.
    fn heap_bytes(&self) -> usize;
}

/// An index that holds nothing, for a model whose agents never look at one another.
#[derive(Debug)]
pub struct NoIndex;

impl NeighborIndex for NoIndex {
    const KIND: &'static str = "None";

    fn new(_extent: Extent, _cell_size: f32) -> Self {
        Self
    }

    fn rebuild(&mut self, _pos_x: &[f32], _pos_y: &[f32], _cell_size: f32) {}

    fn heap_bytes(&self) -> usize {
        0
    }
}

impl NeighborIndex for SpatialHash {
    const KIND: &'static str = "Spatial hash";

    fn new(extent: Extent, cell_size: f32) -> Self {
        Self::new(cell_size, extent.w, extent.h)
    }

    fn rebuild(&mut self, pos_x: &[f32], pos_y: &[f32], cell_size: f32) {
        // Picks up a live edit to whatever parameter sets the cell size, then reindexes.
        self.rebuild_with_cell_size(cell_size, pos_x, pos_y);
        self.build(pos_x, pos_y);
    }

    fn heap_bytes(&self) -> usize {
        Self::heap_bytes(self)
    }
}

/// A per chunk reduction, merged in chunk order.
pub trait ChunkTally: Default + Send + Sized + 'static {
    /// Combines this tally with `other`, the tally that follows it in chunk order.
    fn merge(self, other: Self) -> Self;
}

impl ChunkTally for () {
    fn merge(self, (): Self) {}
}

/// Saturates at `u32::MAX`. A count over a whole run can pass it, and a wrapped total would look like a small count.
impl ChunkTally for u32 {
    fn merge(self, other: Self) -> Self {
        self.saturating_add(other)
    }
}

impl ChunkTally for u64 {
    fn merge(self, other: Self) -> Self {
        self + other
    }
}

/// Context an agent kernel reads besides its own lanes.
pub struct StepCtx<'a, A: AgentModel + ?Sized> {
    /// Field layer, as an agent kernel reads it.
    pub field: <A::Field as FieldLayer>::Read<'a>,
    /// Neighbour index, rebuilt from the positions before the step.
    pub index: &'a A::Index,
    /// Hot parameters of this tick.
    pub params: &'a A::Params,
    /// World size.
    pub extent: Extent,
}

impl<A: AgentModel + ?Sized> std::fmt::Debug for StepCtx<'_, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepCtx")
            .field("extent", &self.extent)
            .finish_non_exhaustive()
    }
}

/// A population of agents, optionally over a field.
///
/// The engine owns lane allocation, double buffering, chunking, seeding, parameter storage, the
/// views, and the whole `SimState` impl.
pub trait AgentModel: Send + Sync + 'static {
    /// Name shown in the UI.
    const NAME: &'static str;
    /// Stable id that identifies the model in a model set, on the command line and in a spec file.
    const ID: &'static str;
    /// One-line description shown in the UI.
    const DESCRIPTION: &'static str;
    /// Agent colours. The field layer carries its own colours.
    const PALETTE: &'static [[u8; 4]];
    /// Stat series for the history chart. Declared once, so `stats` returns bare values.
    const STATS: &'static [StatDescriptor];
    /// One-off steps the user can trigger. Each gets a button in the Parameters panel.
    const ACTIONS: &'static [ActionDescriptor] = &[];

    /// Agents per chunk in a step pass.
    ///
    /// The chunk index seeds the RNG, so the value is a fixed constant and never derived from the thread count.
    /// Keep it small enough that a typical population still splits across every core.
    const CHUNK: usize = 512;

    // Defaults for the three parameters the engine prepends.
    /// Default of `num_agents`.
    const DEFAULT_AGENTS: u32;
    /// Upper bound of `num_agents`.
    const MAX_AGENTS: u32 = 10_000_000;
    /// Defaults of `world_width` and `world_height`.
    const DEFAULT_EXTENT: Extent;

    /// Agent storage, declared with `agent_lanes!`.
    type Lanes: AgentLanes;
    /// Grid layer under the population, or [`NoField`](crate::authoring::model::field::NoField) for a model
    /// without a field.
    type Field: FieldLayer;
    /// Neighbour index, [`SpatialHash`] when agents read each other and [`NoIndex`] otherwise.
    type Index: NeighborIndex;
    /// Pre-extracted hot parameters, rebuilt once per tick.
    type Params: Send + Sync;
    /// Per chunk reduction, accumulated across ticks. `()` when there is nothing to count.
    ///
    /// The engine merges each tick's tally into one total for the whole run, and never resets it. A count of events
    /// per tick therefore grows with the run. A `u32` total stops at `u32::MAX`, and a `u64` holds any count a run
    /// can reach.
    type Tally: ChunkTally;

    /// Model parameters. `num_agents`, `world_width` and `world_height` are prepended by the
    /// engine at indices 0, 1 and 2.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Extracts the hot parameters for one tick. `params` is this model's own slice, so its
    /// indices are 0 based and cannot shift when the engine or a field layer changes.
    fn from_params(params: &[ParamValue], extent: Extent) -> Self::Params;

    /// Returns the neighbour index's cell size for these params.
    ///
    /// The engine reads it every tick, so a live edit takes effect.
    fn index_cell_size(_params: &Self::Params) -> f32 {
        1.0
    }

    /// Fills the lanes for a new run.
    ///
    /// `lanes` holds `num_agents` agents at their initial values, and `params` is this model's own slice.
    fn init(lanes: &mut Self::Lanes, extent: Extent, params: &[ParamValue], rng: &mut u64);

    /// Fills the field's deposit lanes before the step pass, without moving any agent.
    ///
    /// The default does nothing.
    fn run_deposit_pass(
        _lanes: &Self::Lanes,
        _deposits: &mut <Self::Field as FieldLayer>::DepositLanes,
        _ctx: &StepCtx<'_, Self>,
    ) {
    }

    /// Runs the step pass and returns its tally.
    ///
    /// The body is normally one call to the `run_pass` method that `agent_lanes!` generates, with a per agent kernel.
    fn run_step_pass(lanes: &mut Self::Lanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) -> Self::Tally;

    /// Runs [`Self::ACTIONS`] entry `action` over the current lanes and field.
    ///
    /// Receives the arguments of `init` plus the field. An action is a setup step that the user requests
    /// mid run. `rng` is a separate stream, so a press leaves the tick's draws where they were.
    fn act(
        _action: usize,
        _lanes: &mut Self::Lanes,
        _field: &mut Self::Field,
        _extent: Extent,
        _params: &[ParamValue],
        _rng: &mut u64,
    ) {
    }

    /// Current statistics, in [`Self::STATS`] order.
    fn stats(lanes: &Self::Lanes, field: &Self::Field, tally: &Self::Tally) -> Vec<StatValue>;
}

#[cfg(test)]
mod tests {
    use super::ChunkTally;

    /// A run-long `u32` count must saturate. Wrapped, it looks like a small number in a release build.
    #[test]
    fn a_u32_tally_saturates() {
        assert_eq!(ChunkTally::merge(u32::MAX - 1, 5), u32::MAX);
        assert_eq!(ChunkTally::merge(2_u32, 3), 5);
    }
}
