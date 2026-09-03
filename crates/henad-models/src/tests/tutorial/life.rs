//! Game of Life as `docs/guide/first-model/game-of-life.md` builds it.
//!
//! The id is `life` rather than `game_of_life`, since the shipped model already holds that one
//! and the page tells a reader the same thing.

use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::authoring::primitives::rng::{below, next_bits};
use henad_core::grid::Grid2D;
use henad_core::helpers::{extract_f32, f32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatValue};

const DEAD: u8 = 0;
const ALIVE: u8 = 1;

pub const PALETTE: [[u8; 4]; 2] = [
    [0x15, 0x15, 0x15, 0xFF], // Dead
    [0x00, 0xE6, 0x76, 0xFF], // Alive
];

henad_core::params! {
    const DENSITY = f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01)).on_reload();
}

pub struct LifeModel;

impl GridModel for LifeModel {
    const NAME: &'static str = "Game of Life";
    const ID: &'static str = "life";
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Alive", PALETTE[1])];

    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64) {
        let density = extract_f32(params, DENSITY, 0.3);
        let threshold = (density * u32::MAX as f32) as u32;
        for cell in grid.current_mut().iter_mut() {
            *cell = if below(next_bits(rng), threshold) { ALIVE } else { DEAD };
        }
    }

    fn step_cell(cell: u8, neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        let alive_count: u8 = neighbors.iter().sum();
        match (cell, alive_count) {
            (ALIVE, 2..=3) | (DEAD, 3) => ALIVE,
            _ => DEAD,
        }
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(count_alive(grid.current()) as f64)]
    }
}

fn count_alive(cells: &[u8]) -> u64 {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK,
        |r| cells[r].iter().filter(|&&c| c == ALIVE).count() as u64,
        |a, b| a + b,
        0,
    )
}

#[test]
fn a_blinker_rotates_and_comes_back() {
    use henad_compute::cpu::grid_engine::GridModelState;
    use henad_core::model::SimState as _;

    // A 5x5 grid, so the pattern stays clear of the wrap.
    let params = vec![ParamValue::U32(5), ParamValue::U32(5), ParamValue::F32(0.0)];
    let horizontal = {
        let mut cells = vec![DEAD; 25];
        cells[11] = ALIVE;
        cells[12] = ALIVE;
        cells[13] = ALIVE;
        cells
    };
    let vertical = {
        let mut cells = vec![DEAD; 25];
        cells[7] = ALIVE;
        cells[12] = ALIVE;
        cells[17] = ALIVE;
        cells
    };

    let mut state = GridModelState::<LifeModel>::from_cells(&params, &horizontal)
        .expect("the cell buffer matches the declared grid size");

    state.step();
    assert_eq!(
        state.grid_view().expect("grid view").cells,
        &vertical[..],
        "the blinker did not rotate on the first tick"
    );

    state.step();
    assert_eq!(
        state.grid_view().expect("grid view").cells,
        &horizontal[..],
        "the blinker did not come back on the second"
    );
}
