use std::marker::PhantomData;

use henad_core::grid::Grid2D;
use henad_core::grid_model::GridModel;
#[cfg(not(target_arch = "wasm32"))]
use henad_core::helpers::xorshift64;
use henad_core::helpers::{extract_u32, u32_param};
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
    /// Cached from the descriptors so `set_param` can reject reload-only indices without
    /// rebuilding the list every time a slider moves.
    live_params: Vec<bool>,
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
            live_params: grid_model_param_descriptors::<M>()
                .iter()
                .map(ParamDescriptor::is_live)
                .collect(),
            rng_state,
            _marker: PhantomData,
        }
    }
}

/// Returns the full parameter descriptors list with grid width/height prepended.
pub fn grid_model_param_descriptors<M: GridModel>() -> Vec<ParamDescriptor> {
    let mut descs = vec![
        u32_param("grid_width", "Grid Width", 1024, 1, 10_000).on_reload(),
        u32_param("grid_height", "Grid Height", 1024, 1, 10_000).on_reload(),
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
        if self.live_params.get(index) == Some(&true) && index < self.params.len() {
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
    let h = grid.height();
    let ws = grid.width() as usize;
    let (current, next) = grid.current_and_next_mut();

    match M::NEIGHBORHOOD {
        NeighborhoodKind::Moore => {
            step_rows_moore::<M>(current, next, ws, h, hot, rng_state, tick);
        }
        NeighborhoodKind::VonNeumann => {
            step_rows_vn::<M>(current, next, ws, h, hot, rng_state, tick);
        }
    }
}

/// The rows above, at, and below `y`, wrapped vertically. Sliced to exactly one row wide so a
/// neighbour access is a single index rather than a `row * stride + x` multiply-add.
#[inline]
fn neighbor_rows(current: &[u8], ws: usize, y: usize, h: u32) -> [&[u8]; 3] {
    let hs = h as usize;
    let ym = if y == 0 { hs - 1 } else { y - 1 };
    let yp = if y + 1 == hs { 0 } else { y + 1 };
    [
        &current[ym * ws..ym * ws + ws],
        &current[y * ws..y * ws + ws],
        &current[yp * ws..yp * ws + ws],
    ]
}

/// Derived per row rather than shared, so results don't depend on how rayon chunks the rows.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn row_rng(global_seed: u64, tick: u64, y: usize) -> u64 {
    xorshift64((global_seed ^ tick ^ (y as u64)).max(1))
}

#[inline(always)]
fn moore_cell<M: GridModel>(rows: [&[u8]; 3], xm: usize, x: usize, xp: usize, hot: &M::Params, rng: &mut u64) -> u8 {
    let [up, mid, down] = rows;
    let neighbors = [up[xm], up[x], up[xp], mid[xm], mid[xp], down[xm], down[x], down[xp]];
    M::step_cell(mid[x], &neighbors, hot, rng)
}

#[inline(always)]
fn vn_cell<M: GridModel>(rows: [&[u8]; 3], xm: usize, x: usize, xp: usize, hot: &M::Params, rng: &mut u64) -> u8 {
    let [up, mid, down] = rows;
    let neighbors = [up[x], mid[xm], mid[xp], down[x]];
    M::step_cell(mid[x], &neighbors, hot, rng)
}

/// Only the first and last column wrap in x, so both are peeled off and the interior runs without
/// the per-cell modulo. `last.min(1)` covers a one-column grid, where both wraps land on x 0.
#[inline(always)]
fn step_row_moore<M: GridModel>(rows: [&[u8]; 3], next_row: &mut [u8], hot: &M::Params, rng: &mut u64) {
    let Some(last) = next_row.len().checked_sub(1) else {
        return;
    };
    next_row[0] = moore_cell::<M>(rows, last, 0, last.min(1), hot, rng);
    if let Some(interior) = next_row.get_mut(1..last) {
        for (i, out) in interior.iter_mut().enumerate() {
            let x = i + 1;
            *out = moore_cell::<M>(rows, x - 1, x, x + 1, hot, rng);
        }
    }
    if last > 0 {
        next_row[last] = moore_cell::<M>(rows, last - 1, last, 0, hot, rng);
    }
}

/// Von Neumann counterpart of [`step_row_moore`].
#[inline(always)]
fn step_row_vn<M: GridModel>(rows: [&[u8]; 3], next_row: &mut [u8], hot: &M::Params, rng: &mut u64) {
    let Some(last) = next_row.len().checked_sub(1) else {
        return;
    };
    next_row[0] = vn_cell::<M>(rows, last, 0, last.min(1), hot, rng);
    if let Some(interior) = next_row.get_mut(1..last) {
        for (i, out) in interior.iter_mut().enumerate() {
            let x = i + 1;
            *out = vn_cell::<M>(rows, x - 1, x, x + 1, hot, rng);
        }
    }
    if last > 0 {
        next_row[last] = vn_cell::<M>(rows, last - 1, last, 0, hot, rng);
    }
}

#[cfg_attr(
    target_arch = "wasm32",
    expect(
        unused_variables,
        reason = "wasm threads one rng across rows, so it needs no per-tick seed"
    )
)]
fn step_rows_moore<M: GridModel>(
    current: &[u8],
    next: &mut [u8],
    ws: usize,
    h: u32,
    hot: &M::Params,
    rng_state: &mut u64,
    tick: u64,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let global_seed = *rng_state;
        next.par_chunks_mut(ws).enumerate().for_each(|(y, next_row)| {
            let mut rng = row_rng(global_seed, tick, y);
            step_row_moore::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
        });
        *rng_state = xorshift64(global_seed ^ tick);
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut rng = *rng_state;
        for (y, next_row) in next.chunks_mut(ws).enumerate() {
            step_row_moore::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
        }
        *rng_state = rng;
    }
}

#[cfg_attr(
    target_arch = "wasm32",
    expect(
        unused_variables,
        reason = "wasm threads one rng across rows, so it needs no per-tick seed"
    )
)]
fn step_rows_vn<M: GridModel>(
    current: &[u8],
    next: &mut [u8],
    ws: usize,
    h: u32,
    hot: &M::Params,
    rng_state: &mut u64,
    tick: u64,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        let global_seed = *rng_state;
        next.par_chunks_mut(ws).enumerate().for_each(|(y, next_row)| {
            let mut rng = row_rng(global_seed, tick, y);
            step_row_vn::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
        });
        *rng_state = xorshift64(global_seed ^ tick);
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut rng = *rng_state;
        for (y, next_row) in next.chunks_mut(ws).enumerate() {
            step_row_vn::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
        }
        *rng_state = rng;
    }
}

#[cfg(test)]
mod tests {
    use super::GridModelState;
    use henad_core::grid::Grid2D;
    use henad_core::grid_model::GridModel;
    use henad_core::helpers::xorshift64;
    use henad_core::model::SimState as _;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::topology::NeighborhoodKind;
    use henad_core::view::{StatDescriptor, StatEntry};

    fn live_neighbors(neighbors: &[u8]) -> u8 {
        neighbors.iter().filter(|&&n| n != 0).count() as u8
    }

    /// A model that reports how many live neighbours each cell saw, so one step pins the exact set
    /// the engine gathered instead of whatever a real rule would collapse it to.
    macro_rules! counting_model {
        ($ty:ident, $id:literal, $kind:expr) => {
            struct $ty;

            impl GridModel for $ty {
                const NAME: &'static str = $id;
                const ID: &'static str = $id;
                const DESCRIPTION: &'static str = $id;
                const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]; 9];
                const NEIGHBORHOOD: NeighborhoodKind = $kind;
                type Params = ();

                fn param_descriptors() -> Vec<ParamDescriptor> {
                    Vec::new()
                }

                fn from_params(_params: &[ParamValue]) -> Self::Params {}

                fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], rng: &mut u64) {
                    for cell in grid.current_mut() {
                        *rng = xorshift64(*rng);
                        *cell = (*rng & 1) as u8;
                    }
                }

                fn step_cell(_cell: u8, neighbors: &[u8], _params: &Self::Params, _rng: &mut u64) -> u8 {
                    live_neighbors(neighbors)
                }

                fn stats(_grid: &Grid2D<u8>) -> Vec<StatEntry> {
                    Vec::new()
                }

                fn stat_descriptors() -> Vec<StatDescriptor> {
                    Vec::new()
                }
            }
        };
    }

    counting_model!(MooreCount, "moore_count", NeighborhoodKind::Moore);
    counting_model!(VnCount, "vn_count", NeighborhoodKind::VonNeumann);

    /// The plain modulo gather the row loops peel their edge columns to avoid.
    fn reference(cells: &[u8], w: usize, h: usize, moore: bool) -> Vec<u8> {
        let mut out = vec![0u8; cells.len()];
        for y in 0..h {
            let (ym, yp) = ((y + h - 1) % h, (y + 1) % h);
            for x in 0..w {
                let (xm, xp) = ((x + w - 1) % w, (x + 1) % w);
                let neighbors = if moore {
                    vec![
                        cells[ym * w + xm],
                        cells[ym * w + x],
                        cells[ym * w + xp],
                        cells[y * w + xm],
                        cells[y * w + xp],
                        cells[yp * w + xm],
                        cells[yp * w + x],
                        cells[yp * w + xp],
                    ]
                } else {
                    vec![
                        cells[ym * w + x],
                        cells[y * w + xm],
                        cells[y * w + xp],
                        cells[yp * w + x],
                    ]
                };
                out[y * w + x] = live_neighbors(&neighbors);
            }
        }
        out
    }

    /// Widths 1 and 2 are the interesting ones: both peeled columns land on the same cells.
    const SIZES: [(u32, u32); 7] = [(1, 1), (1, 6), (2, 2), (3, 4), (6, 1), (7, 9), (65, 3)];

    fn cells<M: GridModel>(state: &GridModelState<M>) -> Vec<u8> {
        state
            .grid_view()
            .expect("a grid model always has a grid view")
            .cells
            .to_vec()
    }

    #[test]
    fn moore_gather_wraps_like_the_reference() {
        for (w, h) in SIZES {
            let params = vec![ParamValue::U32(w), ParamValue::U32(h)];
            let mut state = GridModelState::<MooreCount>::from_params(&params);
            let before = cells(&state);
            state.step();
            assert_eq!(
                cells(&state),
                reference(&before, w as usize, h as usize, true),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn von_neumann_gather_wraps_like_the_reference() {
        for (w, h) in SIZES {
            let params = vec![ParamValue::U32(w), ParamValue::U32(h)];
            let mut state = GridModelState::<VnCount>::from_params(&params);
            let before = cells(&state);
            state.step();
            assert_eq!(
                cells(&state),
                reference(&before, w as usize, h as usize, false),
                "{w}x{h}"
            );
        }
    }
}
