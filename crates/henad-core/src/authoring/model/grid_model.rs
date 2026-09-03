use crate::grid::Grid2D;
use crate::params::{ParamDescriptor, ParamValue};
use crate::topology::NeighborhoodKind;
use crate::view::{StatDescriptor, StatValue};

/// A cellular automaton over `u8` cells.
///
/// The engine owns grid allocation, double buffering, chunking, tick counting, the views, and the
/// whole `SimState` impl.
pub trait GridModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;
    const PALETTE: &'static [[u8; 4]];
    const NEIGHBORHOOD: NeighborhoodKind;
    /// Stat series for the history chart. Declared once, so `stats` returns bare values.
    const STATS: &'static [StatDescriptor];

    /// Pre-extracted hot parameters, rebuilt once per tick. Keeps enum matching out of the inner
    /// loop.
    type Params: Send + Sync;

    /// Model parameters. The engine prepends grid width and height, but never shows them here.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Hot params for one tick. `params` is this model's own slice, so its indices are 0 based.
    fn from_params(params: &[ParamValue]) -> Self::Params;

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64);

    /// Must be pure beyond the rng. The engine runs rows in parallel.
    fn step_cell(cell: u8, neighbors: &[u8], params: &Self::Params, rng: &mut u64) -> u8;

    /// Current statistics, in [`Self::STATS`] order. Called on publish, not every tick.
    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue>;
}
