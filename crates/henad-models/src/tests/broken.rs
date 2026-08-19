//! A model with a bug in it, for the tests that check a bug cannot end the process.

use henad_core::authoring::model::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatValue};

/// Divides by zero on the way in, standing in for whatever an author actually gets wrong.
pub struct DividesByZero;

impl GridModel for DividesByZero {
    const NAME: &'static str = "Divides By Zero";
    const ID: &'static str = "divides_by_zero";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[];
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        Vec::new()
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {
        let zero = std::hint::black_box(0);
        grid.current_mut()[0] = 1 / zero;
    }

    fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        cell
    }

    fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {
        Vec::new()
    }
}

/// Divides by zero mid-step instead, on whichever rayon worker owns the row.
pub struct DividesByZeroMidStep;

impl GridModel for DividesByZeroMidStep {
    const NAME: &'static str = "Divides By Zero Mid Step";
    const ID: &'static str = "divides_by_zero_mid_step";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[];
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        Vec::new()
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

    fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        let zero = std::hint::black_box(0);
        cell / zero
    }

    fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {
        Vec::new()
    }
}
