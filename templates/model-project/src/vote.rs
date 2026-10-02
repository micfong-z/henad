//! Vichniac's voting rule. A cell takes the majority of itself and its eight neighbours.

use henad::authoring::prelude::*;

pub(crate) const PALETTE: [[u8; 4]; 2] = [[0x1A, 0x1A, 0x2E, 0xFF], [0xF2, 0xA6, 0x3B, 0xFF]];

henad::params! {
    const DENSITY = f32_param("density", "Initial density", 0.5, 0.0, 1.0, Some(0.01)).on_reload();
}

pub(crate) struct Vote;

impl GridModel for Vote {
    const NAME: &'static str = "Vote";
    const ID: &'static str = "vote";
    const DESCRIPTION: &'static str = "Each cell takes the majority of its neighbourhood";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Ones", PALETTE[1])];

    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64) {
        let threshold = (extract_f32(params, DENSITY, 0.5) * u32::MAX as f32) as u32;
        for cell in grid.current_mut().iter_mut() {
            *cell = u8::from(below(next_bits(rng), threshold));
        }
    }

    fn step_cell(cell: u8, neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        u8::from(cell + neighbors.iter().sum::<u8>() >= 5)
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        let cells = grid.current();
        let ones = reduce_chunks(
            cells.len(),
            STATS_CHUNK,
            |range| cells[range].iter().map(|&cell| u64::from(cell)).sum::<u64>(),
            |a, b| a + b,
            0,
        );
        vec![StatValue::Scalar(ones as f64)]
    }
}
