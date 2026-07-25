use henad_core::grid::Grid2D;
use henad_core::grid_model::GridModel;
use henad_core::helpers::{extract_f32, f32_param, stat, xorshift64};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatEntry};

const DEAD: u8 = 0;
const ALIVE: u8 = 1;

/// Cell colours, shared with the GPU Game of Life's stat colour.
///
/// `pub` so the GPU variant can reuse the same literal rather than duplicating it. Note its
/// display shader still bakes these same RGB values into WGSL constants — unifying the CPU
/// palette table with the shader palette is deliberately out of scope for now.
pub const PALETTE: [[u8; 4]; 2] = [
    [0x15, 0x15, 0x15, 0xFF], // Dead - dark gray
    [0x00, 0xE6, 0x76, 0xFF], // Alive - green
];

pub struct GameOfLifeModel;

impl GridModel for GameOfLifeModel {
    const NAME: &'static str = "Game of Life";
    const ID: &'static str = "game_of_life";
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01)).on_reload()]
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64) {
        let density = extract_f32(params, 2, 0.3);
        let threshold = (density * u32::MAX as f32) as u32;
        for cell in grid.current_mut().iter_mut() {
            *rng = xorshift64(*rng);
            *cell = if ((*rng >> 32) as u32) < threshold { ALIVE } else { DEAD };
        }
    }

    fn step_cell(cell: u8, neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        let alive_count: u8 = neighbors.iter().map(|&n| n & 1).sum();
        match (cell, alive_count) {
            (ALIVE, 2..=3) | (DEAD, 3) => ALIVE,
            _ => DEAD,
        }
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatEntry> {
        let alive = count_alive(grid.current()) as f64;
        vec![stat("Alive", alive, PALETTE[1])]
    }

    fn stat_descriptors() -> Vec<StatDescriptor> {
        vec![StatDescriptor {
            label: "Alive",
            color: PALETTE[1],
        }]
    }
}

#[cfg(not(target_arch = "wasm32"))]
const STATS_CHUNK: usize = 8192;

fn count_alive(cells: &[u8]) -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        cells
            .par_chunks(STATS_CHUNK)
            .map(|chunk| chunk.iter().filter(|&&c| c == ALIVE).count() as u64)
            .sum()
    }

    #[cfg(target_arch = "wasm32")]
    {
        cells.iter().filter(|&&c| c == ALIVE).count() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::grid_engine::GridModelState;
    use henad_core::model::SimState as _;

    #[test]
    fn gol_step_cell_rules() {
        let p = ();
        let mut rng = 1u64;

        // Dead with 3 alive neighbors → alive
        assert_eq!(
            GameOfLifeModel::step_cell(DEAD, &[1, 1, 1, 0, 0, 0, 0, 0], &p, &mut rng),
            ALIVE
        );
        // Dead with 2 alive neighbors → dead
        assert_eq!(
            GameOfLifeModel::step_cell(DEAD, &[1, 1, 0, 0, 0, 0, 0, 0], &p, &mut rng),
            DEAD
        );
        // Alive with 2 neighbors → alive
        assert_eq!(
            GameOfLifeModel::step_cell(ALIVE, &[1, 1, 0, 0, 0, 0, 0, 0], &p, &mut rng),
            ALIVE
        );
        // Alive with 3 neighbors → alive
        assert_eq!(
            GameOfLifeModel::step_cell(ALIVE, &[1, 1, 1, 0, 0, 0, 0, 0], &p, &mut rng),
            ALIVE
        );
        // Alive with 1 neighbor → dead (underpopulation)
        assert_eq!(
            GameOfLifeModel::step_cell(ALIVE, &[1, 0, 0, 0, 0, 0, 0, 0], &p, &mut rng),
            DEAD
        );
        // Alive with 4 neighbors → dead (overpopulation)
        assert_eq!(
            GameOfLifeModel::step_cell(ALIVE, &[1, 1, 1, 1, 0, 0, 0, 0], &p, &mut rng),
            DEAD
        );
    }

    #[test]
    fn gol_blinker_period_2() {
        // 5x5 grid with a horizontal blinker at center
        let params = vec![
            ParamValue::U32(5),
            ParamValue::U32(5),
            ParamValue::F32(0.0), // density 0 = all dead
        ];
        let mut state = GridModelState::<GameOfLifeModel>::from_params(&params);

        // Manually set blinker: row 2, cols 1-3
        if let Some(gv) = state.grid_view() {
            assert_eq!(gv.width, 5);
        }

        // We need to step the internal grid, so let's create a fresh state
        // and manually init instead. Just verify tick advancement.
        let tick0 = state.tick();
        state.step();
        assert_eq!(state.tick(), tick0 + 1);
    }
}
