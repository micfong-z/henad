//! A [`GridModel`] gather rule as a [`FieldLayer`].

use std::marker::PhantomData;

use henad_core::authoring::model::field::{Extent, FieldLayer};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::GridView;

use crate::cpu::primitives::chunked::{advance_tick_seed, chunk_seed};
use crate::for_each_chunk_mut;

/// State a grid model's random number generator (RNG) starts from when a run has no seed.
pub const GRID_INIT_SEED: u64 = 0xDEAD_BEEF_CAFE_1234;

/// Returns the state a grid model's RNG starts from: `seed` mixed, or [`GRID_INIT_SEED`] when it is `None`.
///
/// A GPU port that reproduces its CPU model's tick 0 starts from the same state.
pub fn grid_init_rng(seed: Option<u64>) -> u64 {
    seed.map_or(GRID_INIT_SEED, henad_core::authoring::primitives::rng::mix_seed)
}

/// Double-buffered `u8` cells stepped by `M`'s neighbourhood rule.
pub struct CaField<M: GridModel> {
    grid: Grid2D<u8>,
    /// Base seed, advanced once per tick and fanned out per row by `chunk_seed`.
    seed: u64,
    _marker: PhantomData<M>,
}

/// Prints the model's name and the grid's size, not its cells.
impl<M: GridModel> std::fmt::Debug for CaField<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaField")
            .field("model", &M::NAME)
            .field("grid", &self.grid)
            .finish_non_exhaustive()
    }
}

impl<M: GridModel> CaField<M> {
    /// Read access for tests and for `M::stats`.
    pub fn grid(&self) -> &Grid2D<u8> {
        &self.grid
    }

    /// Write access, for an action that rewrites the cells.
    pub fn grid_mut(&mut self) -> &mut Grid2D<u8> {
        &mut self.grid
    }

    /// Number of rayon jobs one tick splits into.
    pub fn parallel_jobs(&self) -> usize {
        let width = self.grid.width() as usize;
        let height = self.grid.height() as usize;
        height.div_ceil(rows_per_leaf(width, height))
    }

    /// Builds a field seeded from `seed`, or from [`GRID_INIT_SEED`] when it is `None`.
    pub fn with_seed(extent: Extent, params: &[ParamValue], seed: Option<u64>) -> Self {
        let (width, height) = extent.cells();
        let mut grid = Grid2D::new(width, height);
        let mut seed = grid_init_rng(seed);
        M::init(&mut grid, params, &mut seed);
        Self {
            grid,
            seed,
            _marker: PhantomData,
        }
    }

    /// Builds a field holding `cells`, seeded from [`GRID_INIT_SEED`].
    ///
    /// Returns `None` unless `cells` is exactly the length `extent` implies.
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
    const KIND: &'static str = "Cellular automaton";

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

/// Floor on the number of cells one rayon leaf takes. Below this the split tree and the wake-up cost more
/// than the rows do.
///
/// A row's cost varies with what its cells hold, and a larger floor stops rayon balancing that by stealing.
const MIN_LEAF_CELLS: usize = 8_192;

/// Returns the floor on the number of rows one rayon leaf takes, at least one and at most the grid's height.
///
/// A grid with rows to spare still splits down to it, and rayon balances the rest by stealing. A small grid is a
/// single job. The floor does not depend on the worker count.
fn rows_per_leaf(width: usize, height: usize) -> usize {
    MIN_LEAF_CELLS.div_ceil(width.max(1)).min(height).max(1)
}

fn step_grid<M: GridModel>(grid: &mut Grid2D<u8>, hot: &M::Params, seed: u64, tick: u64) {
    let h = grid.height();
    let ws = grid.width() as usize;
    let leaf = rows_per_leaf(ws, h as usize);
    let (current, next) = grid.current_and_next_mut();

    match M::NEIGHBORHOOD {
        NeighborhoodKind::Moore => {
            for_each_chunk_mut!(next, ws, min_leaf leaf, |y, _base, next_row| {
                let mut rng = chunk_seed(seed, tick, y);
                step_row_moore::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
            });
        }
        NeighborhoodKind::VonNeumann => {
            for_each_chunk_mut!(next, ws, min_leaf leaf, |y, _base, next_row| {
                let mut rng = chunk_seed(seed, tick, y);
                step_row_vn::<M>(neighbor_rows(current, ws, y, h), next_row, hot, &mut rng);
            });
        }
    }
}

/// Returns the rows above, at and below `y`, wrapped vertically.
///
/// Each slice is exactly one row wide. A neighbour access is then a single index instead of a `row * stride + x`
/// multiply-add.
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

/// Steps one row of a Moore grid into `next_row`.
///
/// Only the first and last column wrap in x. Both columns are peeled off, and the interior runs an `enumerate()`
/// loop without a per-cell modulo. Keep that shape on this hot path. `last.min(1)` covers a one-column grid, where
/// both wraps land on x 0.
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

#[cfg(test)]
mod tests {
    use henad_core::authoring::primitives::space::{MOORE_ROW_MAJOR, VON_NEUMANN};
    use henad_core::view::{StatDescriptor, StatValue};

    use super::*;

    /// Each cell holds its own offset from the centre, so a neighbour slice spells out the order.
    fn encode(dx: i32, dy: i32) -> u8 {
        ((dx + 1) * 3 + (dy + 1)) as u8
    }

    const CENTER: u8 = 4;

    macro_rules! order_probe {
        ($name:ident, $kind:expr, $table:expr) => {
            struct $name;

            impl GridModel for $name {
                const NAME: &'static str = "order probe";
                const ID: &'static str = "order_probe";
                const DESCRIPTION: &'static str = "";
                const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]];
                const NEIGHBORHOOD: NeighborhoodKind = $kind;
                const STATS: &'static [StatDescriptor] = &[];
                type Params = ();

                fn param_descriptors() -> Vec<ParamDescriptor> {
                    Vec::new()
                }

                fn from_params(_params: &[ParamValue]) {}

                fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

                fn step_cell(cell: u8, neighbors: &[u8], (): &(), _rng: &mut u64) -> u8 {
                    if cell == CENTER {
                        let want: Vec<u8> = $table.iter().map(|&(dx, dy)| encode(dx, dy)).collect();
                        assert_eq!(
                            neighbors,
                            want.as_slice(),
                            "step_cell's neighbour order must match the published table"
                        );
                    }
                    cell
                }

                fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {
                    Vec::new()
                }
            }
        };
    }

    order_probe!(MooreProbe, NeighborhoodKind::Moore, MOORE_ROW_MAJOR);
    order_probe!(VonNeumannProbe, NeighborhoodKind::VonNeumann, VON_NEUMANN);

    fn probe_grid() -> Grid2D<u8> {
        let mut grid = Grid2D::new(3, 3);
        let cells = grid.current_mut();
        for dy in -1..=1 {
            for dx in -1..=1 {
                cells[((dy + 1) * 3 + (dx + 1)) as usize] = encode(dx, dy);
            }
        }
        grid
    }

    /// A model indexes its `neighbors` slice by position, so the engine's gather order is API.
    /// The assertion itself lives inside the probe's `step_cell`.
    #[test]
    fn the_gather_order_matches_the_published_moore_table() {
        step_grid::<MooreProbe>(&mut probe_grid(), &(), 1, 0);
    }

    #[test]
    fn the_gather_order_matches_the_published_von_neumann_table() {
        step_grid::<VonNeumannProbe>(&mut probe_grid(), &(), 1, 0);
    }

    /// A grid under the leaf floor runs on one worker however wide the pool is, and the benchmark
    /// CSV carries this number so such a run is not mistaken for a full-width one.
    #[test]
    fn a_grid_under_the_leaf_floor_is_one_job() {
        let small = CaField::<MooreProbe>::with_seed(Extent { w: 64.0, h: 64.0 }, &[], None);
        assert_eq!(small.parallel_jobs(), 1);

        // 8192 cells is two rows at this width.
        let wide = CaField::<MooreProbe>::with_seed(Extent { w: 4096.0, h: 100.0 }, &[], None);
        assert_eq!(wide.parallel_jobs(), 50);
    }

    /// The field implements `Debug` for a model that does not, and prints its size alone. A derive would bound its
    /// impl on the model and print every cell.
    #[test]
    fn the_field_prints_its_size_for_any_model() {
        let field = CaField::<MooreProbe>::with_seed(Extent { w: 64.0, h: 32.0 }, &[], None);
        assert_eq!(
            format!("{field:?}"),
            "CaField { model: \"order probe\", grid: Grid2D { width: 64, height: 32, .. }, .. }"
        );
    }
}
