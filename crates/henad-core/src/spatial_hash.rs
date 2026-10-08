//! The counting-sort [`SpatialHash`] that agents use to find their neighbours, and the [`HashGrid`]
//! cell geometry both backends share.

use crate::authoring::model::field::Extent;
use crate::authoring::primitives::space::{Boundary, axis_delta};

/// Cell geometry on its own, for a caller that needs the grid without the buckets. A GPU model
/// mirrors it into its step uniform so its query walks the same grid as the CPU sort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HashGrid {
    /// Cells along x.
    pub grid_w: u32,
    /// Cells along y.
    pub grid_h: u32,
    /// World width one cell spans.
    pub cell_w: f32,
    /// World height one cell spans.
    pub cell_h: f32,
}

/// Maximum number of cells one index can hold, on either backend.
///
/// The grid is `(world / cell_size)^2`, and the world size and the cell size are both model parameters, so a small
/// cell on a large world requests a grid that no machine can hold. A coarser cell only makes a query scan candidates
/// that it then rejects, whereas an unbounded grid requests gigabytes.
pub const MAX_INDEX_CELLS: u64 = 1 << 22;

impl HashGrid {
    /// Returns the geometry that fits whole cells to the world.
    ///
    /// A query walks in cell index space, so cells all have to span the same distance or the wrap seam gets
    /// under-covered.
    ///
    /// A grid that would hold more than [`MAX_INDEX_CELLS`] cells is coarsened by one factor on both
    /// axes, and a `cell_size` that is not positive counts as 1. This is the one place the geometry is
    /// decided. [`SpatialHash`] and its GPU counterpart both build from here, so neither backend can walk a
    /// grid that the other backend did not sort.
    pub fn new(extent: Extent, cell_size: f32) -> Self {
        let cell_size = if cell_size > 0.0 { cell_size } else { 1.0 };
        let mut grid_w = (extent.w / cell_size).floor().max(1.0) as u32;
        let mut grid_h = (extent.h / cell_size).floor().max(1.0) as u32;

        if u64::from(grid_w) * u64::from(grid_h) > MAX_INDEX_CELLS {
            // Both axes by the same factor, so cells stay as square as the world lets them. Two
            // floors of the same divisor cannot leave the product above the cap.
            let scale = (f64::from(grid_w) * f64::from(grid_h) / MAX_INDEX_CELLS as f64).sqrt();
            grid_w = ((f64::from(grid_w) / scale).floor() as u32).max(1);
            grid_h = ((f64::from(grid_h) / scale).floor() as u32).max(1);
        }

        Self {
            grid_w,
            grid_h,
            cell_w: extent.w / grid_w as f32,
            cell_h: extent.h / grid_h as f32,
        }
    }

    /// Number of cells, `grid_w * grid_h`.
    pub fn num_cells(&self) -> u32 {
        self.grid_w * self.grid_h
    }
}

/// Flat counting-sort grid over agent positions, rebuilt every tick.
pub struct SpatialHash {
    /// Requested cell size, kept only to detect changes.
    cell_size: f32,
    // Actual cell extents, which tile the world exactly.
    cell_w: f32,
    cell_h: f32,
    cell_w_inv: f32,
    cell_h_inv: f32,
    grid_w: u32,
    grid_h: u32,
    world_w: f32,
    world_h: f32,
    /// Flat cell index of each agent.
    agent_cells: Vec<u32>,
    /// Agents sorted by cell index.
    sorted_agents: Vec<u32>,
    /// Start index of each cell in `sorted_agents`.
    cell_start: Vec<u32>,
}

/// Prints the cell geometry and the agent count, not the sorted agents.
impl std::fmt::Debug for SpatialHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpatialHash")
            .field("cell_size", &self.cell_size)
            .field("grid_w", &self.grid_w)
            .field("grid_h", &self.grid_h)
            .field("world_w", &self.world_w)
            .field("world_h", &self.world_h)
            .field("agent_count", &self.agent_cells.len())
            .finish_non_exhaustive()
    }
}

impl SpatialHash {
    /// Creates an empty hash over a `world_w` by `world_h` world, with cells about `cell_size` wide.
    pub fn new(cell_size: f32, world_w: f32, world_h: f32) -> Self {
        // `HashGrid` fits whole cells to the world and caps how many there are. Sharing it keeps this
        // sort and the GPU sort walking the same grid.
        let grid = HashGrid::new(Extent { w: world_w, h: world_h }, cell_size);
        let (grid_w, grid_h) = (grid.grid_w, grid.grid_h);
        let (cell_w, cell_h) = (grid.cell_w, grid.cell_h);
        let num_cells = grid_w * grid_h;

        Self {
            cell_size,
            cell_w,
            cell_h,
            cell_w_inv: 1.0 / cell_w,
            cell_h_inv: 1.0 / cell_h,
            grid_w,
            grid_h,
            world_w,
            world_h,
            agent_cells: Vec::new(),
            sorted_agents: Vec::new(),
            cell_start: vec![0; num_cells as usize + 1],
        }
    }

    /// Returns the flat index of the cell holding `(x, y)`.
    ///
    /// The position wraps, so a position outside the world still falls in a cell.
    #[inline]
    pub fn cell_index(&self, x: f32, y: f32) -> u32 {
        let cx = ((x * self.cell_w_inv).floor() as i32).rem_euclid(self.grid_w as i32) as u32;
        let cy = ((y * self.cell_h_inv).floor() as i32).rem_euclid(self.grid_h as i32) as u32;
        cy * self.grid_w + cx
    }

    /// Sorts the agents at `pos_x` and `pos_y` into their cells.
    pub fn build(&mut self, pos_x: &[f32], pos_y: &[f32]) {
        self.build_where(pos_x, pos_y, |_| true);
    }

    /// Like [`Self::build`], but skips agents that `include` rejects.
    ///
    /// Skipped agents are not returned by any query.
    pub fn build_where(&mut self, pos_x: &[f32], pos_y: &[f32], include: impl Fn(usize) -> bool) {
        /// Cell index marking a skipped agent.
        const NO_CELL: u32 = u32::MAX;

        let num_agents = pos_x.len() as u32;
        let num_cells = self.grid_w * self.grid_h;
        self.agent_cells.clear();
        self.sorted_agents.clear();
        self.cell_start.clear();
        self.agent_cells.reserve(num_agents as usize);
        self.sorted_agents.resize(num_agents as usize, 0);
        self.cell_start.resize((num_cells + 1) as usize, 0);

        // Assigns each agent to its cell and counts the agents per cell.
        for i in 0..num_agents {
            let cell = if include(i as usize) {
                self.cell_index(pos_x[i as usize], pos_y[i as usize])
            } else {
                NO_CELL
            };
            self.agent_cells.push(cell);
            if cell != NO_CELL {
                self.cell_start[cell as usize + 1] += 1;
            }
        }

        // A prefix sum gives the start index of each cell.
        for i in 1..=num_cells {
            self.cell_start[i as usize] += self.cell_start[i as usize - 1];
        }

        // Sorts the agents by cell index with a counting sort.
        let mut write_pos = self.cell_start.clone();
        for i in 0..num_agents {
            let cell = self.agent_cells[i as usize];
            if cell == NO_CELL {
                continue;
            }
            let pos = write_pos[cell as usize];
            self.sorted_agents[pos as usize] = i;
            write_pos[cell as usize] += 1;
        }
    }

    /// Fills `result` with every agent within `r` of `(x, y)`, clearing it first.
    ///
    /// Distances wrap at the world's edges, as for [`Self::for_each_within`].
    pub fn query_radius(&self, x: f32, y: f32, r: f32, pos_x: &[f32], pos_y: &[f32], result: &mut Vec<u32>) {
        result.clear();
        self.for_each_within(x, y, r, pos_x, pos_y, |agent_idx, _dx, _dy, _d2| {
            result.push(agent_idx);
        });
    }

    /// Visits every agent within `r` of `(x, y)`, passing the callback its index, the toroidal
    /// deltas to it and their squared length.
    ///
    /// The deltas come out of the range test either way. A kernel that needs them uses this method
    /// and computes each delta once, whereas a list of indices makes it recompute them all.
    pub fn for_each_within<F: FnMut(u32, f32, f32, f32)>(
        &self,
        x: f32,
        y: f32,
        r: f32,
        pos_x: &[f32],
        pos_y: &[f32],
        mut f: F,
    ) {
        let r2 = r * r;
        let cell_radius_x = (r / self.cell_w).ceil() as i32;
        let cell_radius_y = (r / self.cell_h).ceil() as i32;
        let cell_x = ((x * self.cell_w_inv).floor() as i32).rem_euclid(self.grid_w as i32);
        let cell_y = ((y * self.cell_h_inv).floor() as i32).rem_euclid(self.grid_h as i32);
        // In case the radius is larger than the world, avoid repeatedly wrapping the grid.
        let (y_lo, y_hi) = if 2 * cell_radius_y + 1 > self.grid_h as i32 {
            (0, self.grid_h as i32 - 1)
        } else {
            (cell_y - cell_radius_y, cell_y + cell_radius_y)
        };
        let (x_lo, x_hi) = if 2 * cell_radius_x + 1 > self.grid_w as i32 {
            (0, self.grid_w as i32 - 1)
        } else {
            (cell_x - cell_radius_x, cell_x + cell_radius_x)
        };

        for grid_y in y_lo..=y_hi {
            let wrapped_y = grid_y.rem_euclid(self.grid_h as i32) as u32;
            for grid_x in x_lo..=x_hi {
                let wrapped_x = grid_x.rem_euclid(self.grid_w as i32) as u32;
                let cell_index = wrapped_y * self.grid_w + wrapped_x;
                let start = self.cell_start[cell_index as usize] as usize;
                let end = self.cell_start[cell_index as usize + 1] as usize;
                for &agent_idx in &self.sorted_agents[start..end] {
                    let dx = axis_delta(x, pos_x[agent_idx as usize], self.world_w, Boundary::Torus);
                    let dy = axis_delta(y, pos_y[agent_idx as usize], self.world_h, Boundary::Torus);
                    let d2 = dx * dx + dy * dy;
                    if d2 <= r2 {
                        f(agent_idx, dx, dy, d2);
                    }
                }
            }
        }
    }

    /// Returns whether this hash was built with `cell_size`.
    pub fn cell_size_is(&self, cell_size: f32) -> bool {
        (self.cell_size - cell_size).abs() <= f32::EPSILON
    }

    /// Returns whether this hash was built for a `world_w` by `world_h` world.
    pub fn world_is(&self, world_w: f32, world_h: f32) -> bool {
        (self.world_w - world_w).abs() <= f32::EPSILON && (self.world_h - world_h).abs() <= f32::EPSILON
    }

    /// Cells along each axis, fitted to the world from the requested cell size.
    pub fn grid_dims(&self) -> (u32, u32) {
        (self.grid_w, self.grid_h)
    }

    /// World distance one cell spans on each axis.
    pub fn cell_extents(&self) -> (f32, f32) {
        (self.cell_w, self.cell_h)
    }

    /// Returns `(cell_start, sorted_agents)`, where cell `c` owns
    /// `sorted_agents[cell_start[c]..cell_start[c + 1]]`.
    pub fn buckets(&self) -> (&[u32], &[u32]) {
        (&self.cell_start, &self.sorted_agents)
    }

    /// Rebuilds the hash for `new_cell_size` and sorts the agents again, if the cell size changed.
    pub fn rebuild_with_cell_size(&mut self, new_cell_size: f32, pos_x: &[f32], pos_y: &[f32]) {
        if (new_cell_size - self.cell_size).abs() > f32::EPSILON {
            *self = Self::new(new_cell_size, self.world_w, self.world_h);
            self.build(pos_x, pos_y);
        }
    }

    /// Heap memory held by the hash, in bytes.
    pub fn heap_bytes(&self) -> usize {
        self.agent_cells.capacity() * 4 + self.sorted_agents.capacity() * 4 + self.cell_start.capacity() * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::primitives::rng::xorshift64;

    /// A cell size the UI admits would otherwise request a grid of hundreds of millions of cells.
    #[test]
    fn the_index_grid_is_capped() {
        let extent = Extent {
            w: 10_000.0,
            h: 10_000.0,
        };
        let grid = HashGrid::new(extent, 1.0);
        assert!(
            u64::from(grid.grid_w) * u64::from(grid.grid_h) <= MAX_INDEX_CELLS,
            "{}x{} is over the cap",
            grid.grid_w,
            grid.grid_h
        );
        // Cells still tile the world exactly, which is what the wrap in a query relies on.
        assert!((grid.cell_w * grid.grid_w as f32 - extent.w).abs() < 1e-3);
        assert!((grid.cell_h * grid.grid_h as f32 - extent.h).abs() < 1e-3);
    }

    /// Both backends walk one geometry, so a query cannot read cells the other sort never wrote.
    #[test]
    fn the_sort_and_the_shared_geometry_agree() {
        for (w, h, cell) in [
            (1000.0, 1000.0, 50.0),
            (1000.0, 1000.0, 47.0),
            (10_000.0, 10_000.0, 1.0),
        ] {
            let hash = SpatialHash::new(cell, w, h);
            let grid = HashGrid::new(Extent { w, h }, cell);
            assert_eq!(hash.grid_dims(), (grid.grid_w, grid.grid_h), "dims at cell {cell}");
            assert_eq!(
                hash.cell_extents(),
                (grid.cell_w, grid.cell_h),
                "extents at cell {cell}"
            );
        }
    }

    #[test]
    fn build_and_query_finds_all_close_agents() {
        // Three agents near (0, 0), and one far away.
        let pos_x = vec![0.0, 1.0, -2.0, 50.0];
        let pos_y = vec![0.0, 2.0, -1.0, 50.0];
        let mut sh = SpatialHash::new(10.0, 100.0, 100.0);
        sh.build(&pos_x, &pos_y);

        let mut result = Vec::new();
        sh.query_radius(0.0, 0.0, 5.0, &pos_x, &pos_y, &mut result);

        result.sort();
        assert_eq!(result, vec![0, 1, 2]);
    }

    #[test]
    fn toroidal_query_finds_wrapped_agent() {
        // The world is 100 by 100, and the wrap puts an agent at (99, 99) near (1, 1).
        let pos_x = vec![1.0, 99.0];
        let pos_y = vec![1.0, 99.0];
        let mut sh = SpatialHash::new(10.0, 100.0, 100.0);
        sh.build(&pos_x, &pos_y);

        let mut result = Vec::new();
        sh.query_radius(1.0, 1.0, 5.0, &pos_x, &pos_y, &mut result);

        result.sort();
        assert_eq!(result, vec![0, 1]);
    }

    /// Unsigned toroidal distance on one axis, unlike the signed `space::axis_delta`.
    fn axis_distance(a: f32, b: f32, world: f32) -> f32 {
        let d = (a - b).abs();
        d.min(world - d)
    }

    #[test]
    fn matches_brute_force_with_non_divisor_cell_size() {
        // 47 divides neither world axis, so the hash has to pick its own cell extents.
        let (world_w, world_h, r) = (1_000.0_f32, 730.0_f32, 47.0_f32);
        let mut seed = 0x1234_5678_9ABC_DEF0_u64;
        let mut unit = || {
            seed = xorshift64(seed);
            (seed >> 40) as f32 / 16_777_216.0
        };

        let mut pos_x = Vec::new();
        let mut pos_y = Vec::new();
        for _ in 0..500 {
            pos_x.push(unit() * world_w);
            pos_y.push(unit() * world_h);
        }

        let mut sh = SpatialHash::new(r, world_w, world_h);
        sh.build(&pos_x, &pos_y);

        let mut result = Vec::new();
        for i in 0..pos_x.len() {
            sh.query_radius(pos_x[i], pos_y[i], r, &pos_x, &pos_y, &mut result);
            result.sort();

            let mut expected: Vec<u32> = (0..pos_x.len() as u32)
                .filter(|&j| {
                    let dx = axis_distance(pos_x[j as usize], pos_x[i], world_w);
                    let dy = axis_distance(pos_y[j as usize], pos_y[i], world_h);
                    dx * dx + dy * dy <= r * r
                })
                .collect();
            expected.sort();

            assert_eq!(result, expected, "neighbors of agent {i} disagree with brute force");
        }
    }

    #[test]
    fn query_wider_than_grid_returns_each_agent_once() {
        // A radius of 100 in a world 300 wide leaves 3 cells per axis, so the walk spans the grid.
        let pos_x = vec![0.0, 60.0, 150.0];
        let pos_y = vec![0.0, 0.0, 0.0];
        let mut sh = SpatialHash::new(100.0, 300.0, 300.0);
        sh.build(&pos_x, &pos_y);

        let mut result = Vec::new();
        sh.query_radius(0.0, 0.0, 100.0, &pos_x, &pos_y, &mut result);

        result.sort();
        assert_eq!(result, vec![0, 1], "agent 2 is 150 away, and nothing may repeat");
    }

    #[test]
    fn single_cell_grid_returns_each_agent_once() {
        // A radius past half the world collapses the grid to one cell.
        let pos_x = vec![10.0, 20.0, 60.0];
        let pos_y = vec![10.0, 20.0, 60.0];
        let mut sh = SpatialHash::new(200.0, 100.0, 100.0);
        sh.build(&pos_x, &pos_y);

        let mut result = Vec::new();
        sh.query_radius(10.0, 10.0, 200.0, &pos_x, &pos_y, &mut result);

        result.sort();
        assert_eq!(result, vec![0, 1, 2], "one cell means one visit per agent");
    }
}
