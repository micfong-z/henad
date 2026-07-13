use crate::grid::Grid2D;
use crate::params::{ParamDescriptor, ParamValue};
use crate::topology::NeighborhoodKind;
use crate::view::{StatDescriptor, StatEntry};

/// Simple API for grid-based cellular automata.
///
/// The engine handles: `Grid2D` allocation, double-buffering, parallel step execution,
/// tick counting, `grid_view` construction, and snapshot production.
///
/// Model authors implement this trait with const metadata + pure functions.
/// The engine wraps it in `GridModelState<M>` (in `henad-compute`) which implements `SimState`.
pub trait GridModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;
    const PALETTE: &'static [[u8; 4]];
    const NEIGHBORHOOD: NeighborhoodKind;

    /// Pre-extracted hot parameters. Constructed once per tick via `from_params`,
    /// then passed by reference to every `step_cell` call. This guarantees zero
    /// per-cell overhead — no enum matching inside the inner loop.
    type Params: Send + Sync;

    /// Declare model-specific parameters for the UI.
    /// Grid width and height are auto-prepended by the engine at indices 0 and 1.
    fn param_descriptors() -> Vec<ParamDescriptor>;

    /// Extract hot parameters from the full `ParamValue` slice once per tick.
    fn from_params(params: &[ParamValue]) -> Self::Params;

    /// Initialize the grid cells. Called once when the model is created.
    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64);

    /// Compute the next state of a single cell given its current state
    /// and its neighbors' current states.
    ///
    /// This function is called once per cell per tick. It must be pure
    /// (no side effects beyond the rng). The engine calls it in parallel
    /// across rows on native, sequentially on WASM.
    fn step_cell(cell: u8, neighbors: &[u8], params: &Self::Params, rng: &mut u64) -> u8;

    /// Compute current statistics from the grid state.
    ///
    /// Called on demand when a snapshot is published. Implementations should parallelize this if the computation is expensive.
    fn stats(grid: &Grid2D<u8>) -> Vec<StatEntry>;

    /// Declare stat series for the history chart.
    fn stat_descriptors() -> Vec<StatDescriptor>;
}
