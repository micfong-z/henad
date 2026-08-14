use crate::authoring::field::Extent;

/// Cell geometry on its own, for a caller that needs the grid without the buckets. A GPU model
/// mirrors it into its step uniform so its query walks the same grid as the CPU sort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HashGrid {
    pub grid_w: u32,
    pub grid_h: u32,
    pub cell_w: f32,
    pub cell_h: f32,
}

impl HashGrid {
    /// Fits whole cells to the world, same as [`SpatialHash::new`]. A query walks in cell index
    /// space, so cells all have to span the same distance or the wrap seam gets under-covered.
    #[must_use]
    pub fn new(extent: Extent, cell_size: f32) -> Self {
        let cell_size = if cell_size > 0.0 { cell_size } else { 1.0 };
        let grid_w = (extent.w / cell_size).floor().max(1.0) as u32;
        let grid_h = (extent.h / cell_size).floor().max(1.0) as u32;
        Self {
            grid_w,
            grid_h,
            cell_w: extent.w / grid_w as f32,
            cell_h: extent.h / grid_h as f32,
        }
    }

    #[must_use]
    pub fn num_cells(&self) -> u32 {
        self.grid_w * self.grid_h
    }
}

/// Spatial hash for efficient neighbor queries in 2D space.
pub struct SpatialHash {
    /// Requested cell size, only kept to detect changes
    cell_size: f32,
    /// Actual cell extents, which tile the world exactly
    cell_w: f32,
    cell_h: f32,
    cell_w_inv: f32,
    cell_h_inv: f32,
    grid_w: u32,
    grid_h: u32,
    world_w: f32,
    world_h: f32,
    /// Cell flat-index for each agent
    agent_cells: Vec<u32>,
    /// Agents sorted by cell index
    sorted_agents: Vec<u32>,
    /// Start index of each cell in `sorted_agents`
    cell_start: Vec<u32>,
}

impl SpatialHash {
    pub fn new(cell_size: f32, world_w: f32, world_h: f32) -> Self {
        // `query_radius` walks a neighborhood in cell index space, so every cell has to span the
        // same world distance, otherwise the wrap seam gets under-covered. Fit the cells to the
        // world instead of using `cell_size` directly, rounding the count down so they only grow.
        let grid_w = (world_w / cell_size).floor().max(1.0) as u32;
        let grid_h = (world_h / cell_size).floor().max(1.0) as u32;
        let cell_w = world_w / grid_w as f32;
        let cell_h = world_h / grid_h as f32;
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

    /// Returns the flat cell index for a given position (x, y) safely.
    #[inline]
    pub fn cell_index(&self, x: f32, y: f32) -> u32 {
        let cx = ((x * self.cell_w_inv).floor() as i32).rem_euclid(self.grid_w as i32) as u32;
        let cy = ((y * self.cell_h_inv).floor() as i32).rem_euclid(self.grid_h as i32) as u32;
        cy * self.grid_w + cx
    }

    pub fn build(&mut self, pos_x: &[f32], pos_y: &[f32]) {
        let num_agents = pos_x.len() as u32;
        let num_cells = self.grid_w * self.grid_h;
        self.agent_cells.clear();
        self.sorted_agents.clear();
        self.cell_start.clear();
        self.agent_cells.reserve(num_agents as usize);
        self.sorted_agents.resize(num_agents as usize, 0);
        self.cell_start.resize((num_cells + 1) as usize, 0);

        // Assign agents to cells and count agents per cell
        for i in 0..num_agents {
            let cell = self.cell_index(pos_x[i as usize], pos_y[i as usize]);
            self.agent_cells.push(cell);
            self.cell_start[cell as usize + 1] += 1;
        }

        // Prefix sum to get start index of each cell
        for i in 1..=num_cells {
            self.cell_start[i as usize] += self.cell_start[i as usize - 1];
        }

        // Sort agents by cell index using counting sort
        let mut write_pos = self.cell_start.clone();
        for i in 0..num_agents {
            let cell = self.agent_cells[i as usize];
            let pos = write_pos[cell as usize];
            self.sorted_agents[pos as usize] = i;
            write_pos[cell as usize] += 1;
        }
    }

    pub fn query_radius(&self, x: f32, y: f32, r: f32, pos_x: &[f32], pos_y: &[f32], result: &mut Vec<u32>) {
        result.clear();
        let r2 = r * r;
        let cell_radius_x = (r / self.cell_w).ceil() as i32;
        let cell_radius_y = (r / self.cell_h).ceil() as i32;
        let cell_x = ((x * self.cell_w_inv).floor() as i32).rem_euclid(self.grid_w as i32);
        let cell_y = ((y * self.cell_h_inv).floor() as i32).rem_euclid(self.grid_h as i32);
        let half_w = self.world_w * 0.5;
        let half_h = self.world_h * 0.5;

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
                let start = self.cell_start[cell_index as usize];
                let end = self.cell_start[cell_index as usize + 1];
                for i in start..end {
                    let agent_idx = self.sorted_agents[i as usize];
                    let raw_dx = pos_x[agent_idx as usize] - x;
                    let raw_dy = pos_y[agent_idx as usize] - y;
                    let dx = (raw_dx + half_w).rem_euclid(self.world_w) - half_w;
                    let dy = (raw_dy + half_h).rem_euclid(self.world_h) - half_h;
                    if dx * dx + dy * dy <= r2 {
                        result.push(agent_idx);
                    }
                }
            }
        }
    }

    /// Cells along each axis. Fitted to the world, not derived from `cell_size` directly.
    #[must_use]
    pub fn grid_dims(&self) -> (u32, u32) {
        (self.grid_w, self.grid_h)
    }

    /// World distance one cell spans on each axis.
    #[must_use]
    pub fn cell_extents(&self) -> (f32, f32) {
        (self.cell_w, self.cell_h)
    }

    /// `(cell_start, sorted_agents)`.
    ///
    /// Note that cell `c` owns `sorted_agents[cell_start[c]..cell_start[c + 1]]`.
    #[must_use]
    pub fn buckets(&self) -> (&[u32], &[u32]) {
        (&self.cell_start, &self.sorted_agents)
    }

    pub fn rebuild_with_cell_size(&mut self, new_cell_size: f32, pos_x: &[f32], pos_y: &[f32]) {
        if (new_cell_size - self.cell_size).abs() > f32::EPSILON {
            *self = Self::new(new_cell_size, self.world_w, self.world_h);
            self.build(pos_x, pos_y);
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.agent_cells.capacity() * 4 + self.sorted_agents.capacity() * 4 + self.cell_start.capacity() * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::xorshift64;

    #[test]
    fn build_and_query_finds_all_close_agents() {
        // 3 agents near (0,0), 1 agent far away
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
        // World is 100x100, agent at (99,99) should be near (1,1) due to wrapping
        let pos_x = vec![1.0, 99.0];
        let pos_y = vec![1.0, 99.0];
        let mut sh = SpatialHash::new(10.0, 100.0, 100.0);
        sh.build(&pos_x, &pos_y);

        let mut result = Vec::new();
        sh.query_radius(1.0, 1.0, 5.0, &pos_x, &pos_y, &mut result);

        result.sort();
        assert_eq!(result, vec![0, 1]);
    }

    /// Toroidal distance on one axis
    fn axis_delta(a: f32, b: f32, world: f32) -> f32 {
        let d = (a - b).abs();
        d.min(world - d)
    }

    #[test]
    fn matches_brute_force_with_non_divisor_cell_size() {
        // 47 divides neither world axis, so the hash has to pick its own cell extents
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
                    let dx = axis_delta(pos_x[j as usize], pos_x[i], world_w);
                    let dy = axis_delta(pos_y[j as usize], pos_y[i], world_h);
                    dx * dx + dy * dy <= r * r
                })
                .collect();
            expected.sort();

            assert_eq!(result, expected, "neighbors of agent {i} disagree with brute force");
        }
    }

    #[test]
    fn query_wider_than_grid_returns_each_agent_once() {
        // Radius 100 into a 300 wide world leaves 3 cells per axis, so the walk spans the grid
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
        // Radius past half the world collapses the grid to one cell
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
