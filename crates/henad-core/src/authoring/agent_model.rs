//! Authoring API for models whose state is a population of agents.

use crate::authoring::field::{Extent, FieldLayer};
use crate::params::{ParamDescriptor, ParamValue};
use crate::spatial_hash::SpatialHash;
use crate::view::{StatDescriptor, StatValue};

/// Struct-of-arrays agent storage.
///
/// Written by the `agent_lanes!` macro rather than by hand. The chunked step driver is an inherent
/// method on the generated type, not a trait method, so its closure can name concrete borrow types.
pub trait AgentLanes: Send + Sync + 'static {
    fn alloc(n: usize) -> Self;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Swaps the double buffered lanes. No-op when none are.
    fn swap(&mut self);
    fn heap_bytes(&self) -> usize;

    /// Every model has positions, so the engine builds the neighbour index and the point view.
    fn positions(&self) -> (&[f32], &[f32]);

    /// One palette index per agent. `None` colours the whole population `PALETTE[0]`.
    fn colors(&self) -> Option<&[u8]> {
        None
    }
}

/// Neighbour lookup rebuilt from agent positions each tick.
pub trait NeighborIndex: Send + Sync + 'static {
    fn new(extent: Extent, cell_size: f32) -> Self;
    fn rebuild(&mut self, pos_x: &[f32], pos_y: &[f32], cell_size: f32);
    fn heap_bytes(&self) -> usize;
}

/// For models whose agents never look at one another.
pub struct NoIndex;

impl NeighborIndex for NoIndex {
    fn new(_extent: Extent, _cell_size: f32) -> Self {
        Self
    }

    fn rebuild(&mut self, _pos_x: &[f32], _pos_y: &[f32], _cell_size: f32) {}

    fn heap_bytes(&self) -> usize {
        0
    }
}

impl NeighborIndex for SpatialHash {
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
    fn merge(self, other: Self) -> Self;
}

impl ChunkTally for () {
    fn merge(self, (): Self) {}
}

impl ChunkTally for u32 {
    fn merge(self, other: Self) -> Self {
        self + other
    }
}

impl ChunkTally for u64 {
    fn merge(self, other: Self) -> Self {
        self + other
    }
}

/// Context an agent kernel reads besides its own lanes.
pub struct StepCtx<'a, A: AgentModel + ?Sized> {
    pub field: <A::Field as FieldLayer>::Read<'a>,
    pub index: &'a A::Index,
    pub params: &'a A::Params,
    pub extent: Extent,
}

/// A population of agents, optionally over a field.
///
/// The engine owns lane allocation, double buffering, chunking, seeding, parameter storage, the
/// views, and the whole `SimState` impl.
pub trait AgentModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;
    /// Agent colours. The field layer carries its own.
    const PALETTE: &'static [[u8; 4]];
    /// Stat series for the history chart. Declared once, so `stats` returns bare values.
    const STATS: &'static [StatDescriptor];

    /// Agents per chunk in a step pass. Fixed rather than derived from the thread count, since
    /// chunk index seeds the RNG and results must not depend on the machine. Small enough that a
    /// typical population still splits across every core.
    const CHUNK: usize = 512;

    /// Defaults for the three parameters the engine prepends.
    const DEFAULT_AGENTS: u32;
    const MAX_AGENTS: u32 = 10_000_000;
    const DEFAULT_EXTENT: Extent;

    type Lanes: AgentLanes;
    type Field: FieldLayer;
    type Index: NeighborIndex;
    /// Pre-extracted hot parameters, rebuilt once per tick.
    type Params: Send + Sync;
    /// Per chunk reduction, accumulated across ticks. `()` when there is nothing to count.
    type Tally: ChunkTally;

    /// Model parameters. `num_agents`, `world_width` and `world_height` are prepended by the
    /// engine at indices 0, 1 and 2.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Hot params for one tick, extracted once. `params` is this model's own slice, so its
    /// indices are 0 based and cannot shift when the engine or a field layer changes.
    fn from_params(params: &[ParamValue], extent: Extent) -> Self::Params;

    /// Neighbour index cell size for these params, read every tick so a live edit lands.
    fn index_cell_size(_params: &Self::Params) -> f32 {
        1.0
    }

    fn init(lanes: &mut Self::Lanes, extent: Extent, params: &[ParamValue], rng: &mut u64);

    /// Optional first pass, for filling a field's deposit lanes without moving.
    fn run_deposit_pass(
        _lanes: &Self::Lanes,
        _deposits: &mut <Self::Field as FieldLayer>::DepositLanes,
        _ctx: &StepCtx<'_, Self>,
    ) {
    }

    /// The step pass. Generated by `agent_lanes!` from a per agent kernel.
    fn run_step_pass(lanes: &mut Self::Lanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) -> Self::Tally;

    /// Current statistics, in [`Self::STATS`] order.
    fn stats(lanes: &Self::Lanes, field: &Self::Field, tally: &Self::Tally) -> Vec<StatValue>;
}
