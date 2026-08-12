//! A [`GridModel`] gather rule as a [`FieldLayer`].

use std::marker::PhantomData;

use henad_core::authoring::field::{Extent, FieldLayer};
use henad_core::authoring::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::GridView;

use crate::cpu::primitives::chunked::{advance_tick_seed, chunk_seed};
use crate::for_each_chunk_mut;

/// Seed every `GridModel`'s `init` starts from.
///
/// Exported so a GPU re-implementation of a CPU model can seed its grid bit-identically and
/// therefore be checked against the CPU model as an oracle.
pub const GRID_INIT_SEED: u64 = 0xDEAD_BEEF_CAFE_1234;

/// Double-buffered `u8` cells stepped by `M`'s neighbourhood rule.
pub struct CaField<M: GridModel> {
    grid: Grid2D<u8>,
    /// Advanced once per tick, then fanned out per row by `chunk_seed`.
    seed: u64,
    _marker: PhantomData<M>,
}

impl<M: GridModel> CaField<M> {
    /// Read access for tests and for `M::stats`.
    pub fn grid(&self) -> &Grid2D<u8> {
        &self.grid
    }

    /// Build a field seeded from `seed`, or from [`GRID_INIT_SEED`] when it is `None`.
    pub fn with_seed(extent: Extent, params: &[ParamValue], seed: Option<u64>) -> Self {
        let (width, height) = extent.cells();
        let mut grid = Grid2D::new(width, height);
        let mut seed = seed.map_or(GRID_INIT_SEED, henad_core::helpers::mix_seed);
        M::init(&mut grid, params, &mut seed);
        Self {
            grid,
            seed,
            _marker: PhantomData,
        }
    }

    /// Build a field with specified `cells`.
    ///
    /// Returns `Some` if the `cells` slice is the right length for the grid dimensions in `extent`, or `None` if not.
    pub fn from_cells(extent: Extent, cells: &[u8]) -> Option<Self> {
        let (width, height) = extent.cells();
        if cells.len() != width as usize * height as usize {
            return None;
        }
        let mut grid = Grid2D::new(width, height);
        grid.current_mut().copy_from_slice(cells);
        Some(Self {
            grid,
            seed: GRID_INIT_SEED,
            _marker: PhantomData,
        })
    }
}

impl<M: GridModel> FieldLayer for CaField<M> {
    type Params = M::Params;
    type Read<'a> = &'a [u8];
    type DepositLanes = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        M::param_descriptors()
    }

    fn from_params(params: &[ParamValue]) -> M::Params {
        M::from_params(params)
    }

    fn new(extent: Extent, params: &[ParamValue]) -> Self {
        Self::with_seed(extent, params, None)
    }

    fn read(&self) -> &[u8] {
        self.grid.current()
    }

    fn alloc_deposits(&self, _n: usize) {}

    fn update(&mut self, (): &(), p: &M::Params, tick: u64) {
        step_grid::<M>(&mut self.grid, p, self.seed, tick);
        self.seed = advance_tick_seed(self.seed, tick);
        self.grid.swap();
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        Some(GridView {
            width: self.grid.width(),
            height: self.grid.height(),
            cells: self.grid.current(),
            palette: M::PALETTE,
        })
    }

    fn cell_count(&self) -> usize {
        self.grid.len()
    }

    fn heap_bytes(&self) -> usize {
        self.grid.heap_bytes()
    }
}

fn step_grid<M: GridModel>(grid: &mut Grid2D<u8>, hot: &M::Params, seed: u64, tick: u64) {
    let h = grid.height();
    let ws = grid.width() as usize;
    let (current, next) = grid.current_and_next_mut();

    match M::NEIGHBORHOOD {
        NeighborhoodKind::Moore => {
            for_each_chunk_mut!(next, ws, |y, _base, next_row| {
                let mut rng = chunk_seed(seed, tick, y);
                step_row_moore::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
            });
        }
        NeighborhoodKind::VonNeumann => {
            for_each_chunk_mut!(next, ws, |y, _base, next_row| {
                let mut rng = chunk_seed(seed, tick, y);
                step_row_vn::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
            });
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
