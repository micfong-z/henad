/// Spatial hash for efficient neighbor queries in 2D space.
pub struct SpatialHash {
    cell_size: f32,
    cell_size_inv: f32,
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
        let grid_w = (world_w / cell_size).ceil().max(1.0) as u32;
        let grid_h = (world_h / cell_size).ceil().max(1.0) as u32;
        let num_cells = grid_w * grid_h;

        Self {
            cell_size,
            cell_size_inv: 1.0 / cell_size,
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
        let cx = ((x * self.cell_size_inv).floor() as i32).rem_euclid(self.grid_w as i32) as u32;
        let cy = ((y * self.cell_size_inv).floor() as i32).rem_euclid(self.grid_h as i32) as u32;
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

    pub fn query_radius(
        &self,
        x: f32,
        y: f32,
        r: f32,
        pos_x: &[f32],
        pos_y: &[f32],
        result: &mut Vec<u32>,
    ) {
        result.clear();
        let r2 = r * r;
        let cell_radius = (r * self.cell_size_inv).ceil() as i32;
        let cell_x = ((x * self.cell_size_inv).floor() as i32).rem_euclid(self.grid_w as i32);
        let cell_y = ((y * self.cell_size_inv).floor() as i32).rem_euclid(self.grid_h as i32);
        let half_w = self.world_w * 0.5;
        let half_h = self.world_h * 0.5;

        for grid_y in (cell_y - cell_radius)..=(cell_y + cell_radius) {
            let wrapped_y = grid_y.rem_euclid(self.grid_h as i32) as u32;
            for grid_x in (cell_x - cell_radius)..=(cell_x + cell_radius) {
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

    pub fn rebuild_with_cell_size(&mut self, new_cell_size: f32, pos_x: &[f32], pos_y: &[f32]) {
        if (new_cell_size - self.cell_size).abs() > f32::EPSILON {
            *self = Self::new(new_cell_size, self.world_w, self.world_h);
            self.build(pos_x, pos_y);
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.agent_cells.capacity() * 4
            + self.sorted_agents.capacity() * 4
            + self.cell_start.capacity() * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
