//! Authoring API for cellular automata over `u8` cells.

use crate::action::ActionDescriptor;
use crate::grid::Grid2D;
use crate::params::{ParamDescriptor, ParamValue};
use crate::topology::NeighborhoodKind;
use crate::view::{StatDescriptor, StatValue};

/// A cellular automaton over `u8` cells.
///
/// The engine owns grid allocation, double buffering, chunking, tick counting, the views, and the
/// whole `SimState` impl.
pub trait GridModel: Send + Sync + 'static {
    /// Name shown in the UI.
    const NAME: &'static str;
    /// Stable id that identifies the model in a model set, on the command line and in a spec file.
    const ID: &'static str;
    /// One-line description shown in the UI.
    const DESCRIPTION: &'static str;
    /// Cell colours, indexed by cell value.
    const PALETTE: &'static [[u8; 4]];
    /// Neighbourhood whose cells [`Self::step_cell`] receives.
    const NEIGHBORHOOD: NeighborhoodKind;
    /// Stat series for the history chart. Declared once, so `stats` returns bare values.
    const STATS: &'static [StatDescriptor];
    /// One-off steps the user can trigger. Each gets a button in the Parameters panel.
    const ACTIONS: &'static [ActionDescriptor] = &[];

    /// Hot parameters, extracted once per tick to keep enum matching out of the inner loop.
    type Params: Send + Sync;

    /// Model parameters. The engine prepends grid width and height, but never shows them here.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Extracts the hot parameters for one tick. `params` is this model's own slice, so its indices are 0 based.
    fn from_params(params: &[ParamValue]) -> Self::Params;

    /// Fills the grid for a new run.
    ///
    /// `params` is this model's own slice, as for [`Self::from_params`].
    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64);

    /// Returns the next value of `cell`, given its neighbours.
    ///
    /// `neighbors` follows the order of [`MOORE_ROW_MAJOR`] or [`VON_NEUMANN`], as [`Self::NEIGHBORHOOD`] picks.
    /// The function must be pure apart from `rng`, since the engine steps rows in parallel.
    ///
    /// [`MOORE_ROW_MAJOR`]: crate::authoring::primitives::space::MOORE_ROW_MAJOR
    /// [`VON_NEUMANN`]: crate::authoring::primitives::space::VON_NEUMANN
    fn step_cell(cell: u8, neighbors: &[u8], params: &Self::Params, rng: &mut u64) -> u8;

    /// Runs [`Self::ACTIONS`] entry `action` over the current cells.
    ///
    /// Receives the arguments of `init`. An action is a setup step that the user requests mid run. `rng`
    /// is a separate stream, so a press leaves the tick's draws where they were.
    fn act(_action: usize, _grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

    /// Current statistics, in [`Self::STATS`] order.
    ///
    /// A step never calls it. The engine calls it when it publishes a snapshot or takes a sample.
    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue>;
}
