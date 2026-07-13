use std::marker::PhantomData;

use henad_core::grid::Grid2D;
use henad_core::grid_model::GridModel;
use henad_core::helpers::{extract_u32, u32_param, xorshift64};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{GridView, StatEntry};

/// Seed every `GridModel`'s `init` starts from.
///
/// Exported (rather than being a literal inside `from_params`) so a GPU re-implementation of a
/// CPU model can seed its grid bit-identically and therefore be checked against the CPU model as
/// an oracle. If this and the model's `init` agree, the two backends start from the same grid.
pub const GRID_INIT_SEED: u64 = 0xDEAD_BEEF_CAFE_1234;

/// Engine wrapper that implements `SimState` for any `GridModel`.
pub struct GridModelState<M: GridModel> {
    grid: Grid2D<u8>,
    tick: u64,
    params: Vec<ParamValue>,
    rng_state: u64,
    _marker: PhantomData<M>,
}

impl<M: GridModel> GridModelState<M> {
    pub fn from_params(params: &[ParamValue]) -> Self {
        let width = extract_u32(params, 0, 1024);
        let height = extract_u32(params, 1, 1024);
        let mut grid = Grid2D::new(width, height);
        let mut rng_state: u64 = GRID_INIT_SEED;
        M::init(&mut grid, params, &mut rng_state);
        Self {
            grid,
            tick: 0,
            params: params.to_vec(),
            rng_state,
            _marker: PhantomData,
        }
    }
}

/// Returns the full parameter descriptors list with grid width/height prepended.
pub fn grid_model_param_descriptors<M: GridModel>() -> Vec<ParamDescriptor> {
    let mut descs = vec![
        u32_param("grid_width", "Grid Width", 1024, 1, 10_000),
        u32_param("grid_height", "Grid Height", 1024, 1, 10_000),
    ];
    descs.extend(M::param_descriptors());
    descs
}

impl<M: GridModel> SimState for GridModelState<M> {
    fn step(&mut self) {
        let hot = M::from_params(&self.params);
        step_grid::<M>(&mut self.grid, &hot, &mut self.rng_state, self.tick);
        self.grid.swap();
        self.tick += 1;
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        Some(GridView {
            width: self.grid.width(),
            height: self.grid.height(),
            cells: self.grid.current(),
            palette: M::PALETTE,
        })
    }

    fn stats(&self) -> Vec<StatEntry> {
        M::stats(&self.grid)
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        if index >= 2 && index < self.params.len() {
            self.params[index] = value.clone();
            true
        } else {
            false
        }
    }

    fn population(&self) -> u64 {
        self.grid.len() as u64
    }

    fn heap_bytes(&self) -> usize {
        self.grid.heap_bytes()
    }
}

fn step_grid<M: GridModel>(grid: &mut Grid2D<u8>, hot: &M::Params, rng_state: &mut u64, tick: u64) {
    let w = grid.width();
    let h = grid.height();
    let ws = w as usize;
    let (current, next) = grid.current_and_next_mut();

    match M::NEIGHBORHOOD {
        NeighborhoodKind::Moore => {
            step_rows_moore::<M>(current, next, ws, w, h, hot, rng_state, tick);
        }
        NeighborhoodKind::VonNeumann => {
            step_rows_vn::<M>(current, next, ws, w, h, hot, rng_state, tick);
        }
    }
}

#[expect(clippy::too_many_arguments, reason = "grid step internals")]
fn step_rows_moore<M: GridModel>(
    current: &[u8],
    next: &mut [u8],
    ws: usize,
    w: u32,
    h: u32,
    hot: &M::Params,
    rng_state: &mut u64,
    tick: u64,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let global_seed = *rng_state;
        next.par_chunks_mut(ws)
            .enumerate()
            .for_each(|(y, next_row)| {
                let row_seed = global_seed ^ tick ^ (y as u64);
                let mut rng = xorshift64(row_seed.max(1));
                let ym = ((y as u32 + h - 1) % h) as usize;
                let yp = ((y as u32 + 1) % h) as usize;
                for x in 0..w {
                    let xs = x as usize;
                    let xm = ((x + w - 1) % w) as usize;
                    let xp = ((x + 1) % w) as usize;
                    let neighbors = [
                        current[ym * ws + xm],
                        current[ym * ws + xs],
                        current[ym * ws + xp],
                        current[y * ws + xm],
                        current[y * ws + xp],
                        current[yp * ws + xm],
                        current[yp * ws + xs],
                        current[yp * ws + xp],
                    ];
                    next_row[xs] = M::step_cell(current[y * ws + xs], &neighbors, hot, &mut rng);
                }
            });
        *rng_state = xorshift64(global_seed ^ tick);
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut rng = *rng_state;
        for (y, next_row) in next.chunks_mut(ws).enumerate() {
            let ym = ((y as u32 + h - 1) % h) as usize;
            let yp = ((y as u32 + 1) % h) as usize;
            for x in 0..w {
                let xs = x as usize;
                let xm = ((x + w - 1) % w) as usize;
                let xp = ((x + 1) % w) as usize;
                let neighbors = [
                    current[ym * ws + xm],
                    current[ym * ws + xs],
                    current[ym * ws + xp],
                    current[y * ws + xm],
                    current[y * ws + xp],
                    current[yp * ws + xm],
                    current[yp * ws + xs],
                    current[yp * ws + xp],
                ];
                next_row[xs] = M::step_cell(current[y * ws + xs], &neighbors, hot, &mut rng);
            }
        }
        *rng_state = rng;
    }
}

#[expect(clippy::too_many_arguments, reason = "grid step internals")]
fn step_rows_vn<M: GridModel>(
    current: &[u8],
    next: &mut [u8],
    ws: usize,
    w: u32,
    h: u32,
    hot: &M::Params,
    rng_state: &mut u64,
    tick: u64,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let global_seed = *rng_state;
        next.par_chunks_mut(ws)
            .enumerate()
            .for_each(|(y, next_row)| {
                let row_seed = global_seed ^ tick ^ (y as u64);
                let mut rng = xorshift64(row_seed.max(1));
                let ym = ((y as u32 + h - 1) % h) as usize;
                let yp = ((y as u32 + 1) % h) as usize;
                for x in 0..w {
                    let xs = x as usize;
                    let xm = ((x + w - 1) % w) as usize;
                    let xp = ((x + 1) % w) as usize;
                    let neighbors = [
                        current[ym * ws + xs],
                        current[y * ws + xm],
                        current[y * ws + xp],
                        current[yp * ws + xs],
                    ];
                    next_row[xs] = M::step_cell(current[y * ws + xs], &neighbors, hot, &mut rng);
                }
            });
        *rng_state = xorshift64(global_seed ^ tick);
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut rng = *rng_state;
        for (y, next_row) in next.chunks_mut(ws).enumerate() {
            let ym = ((y as u32 + h - 1) % h) as usize;
            let yp = ((y as u32 + 1) % h) as usize;
            for x in 0..w {
                let xs = x as usize;
                let xm = ((x + w - 1) % w) as usize;
                let xp = ((x + 1) % w) as usize;
                let neighbors = [
                    current[ym * ws + xs],
                    current[y * ws + xm],
                    current[y * ws + xp],
                    current[yp * ws + xs],
                ];
                next_row[xs] = M::step_cell(current[y * ws + xs], &neighbors, hot, &mut rng);
            }
        }
        *rng_state = rng;
    }
}
