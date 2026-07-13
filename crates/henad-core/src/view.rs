/// A view of a 2D grid of cells for rendering.
///
/// Each cell is a `u8` index into the palette.
pub struct GridView<'a> {
    pub width: u32,
    pub height: u32,
    pub cells: &'a [u8],
    pub palette: &'static [[u8; 4]],
}

pub struct PointView<'a> {
    pub pos_x: &'a [f32],
    pub pos_y: &'a [f32],
    pub world_w: f32,
    pub world_h: f32,
    pub palette: &'static [u8; 4],
}

/// The value of a statistic entry.
#[derive(Debug, Clone)]
pub enum StatValue {
    Scalar(f64),
    Vector2D { x: f64, y: f64 },
    Histogram { edges: Vec<f64>, counts: Vec<u64> },
}

impl StatValue {
    /// Extract a single representative f64 for charting.
    /// Scalar → value, `Vector2D` → magnitude, Histogram → total count.
    pub fn scalar(&self) -> f64 {
        match self {
            Self::Scalar(v) => *v,
            Self::Vector2D { x, y } => x.hypot(*y),
            Self::Histogram { counts, .. } => counts.iter().sum::<u64>() as f64,
        }
    }
}

/// A single statistic entry for display.
#[derive(Debug, Clone)]
pub struct StatEntry {
    pub label: &'static str,
    pub value: StatValue,
    pub color: [u8; 4],
}

/// Metadata describing one stat series.
#[derive(Debug, Clone)]
pub struct StatDescriptor {
    pub label: &'static str,
    pub color: [u8; 4],
}

/// Ring-buffer history of stat values, polled every snapshot.
pub struct StatsHistory {
    /// One column per stat series, each holding `capacity` entries.
    columns: Vec<Vec<f64>>,
    /// Ticks corresponding to each entry in the columns. Same length as each column.
    ticks: Vec<u64>,
    descriptors: Vec<StatDescriptor>,
    /// Total number of entries written (may exceed capacity).
    write_count: usize,
    capacity: usize,
}

impl StatsHistory {
    /// Create a new history with the given stat descriptors and ring buffer capacity.
    pub fn new(descriptors: Vec<StatDescriptor>, capacity: usize) -> Self {
        let columns = vec![Vec::with_capacity(capacity); descriptors.len()];
        let ticks = Vec::with_capacity(capacity);
        Self {
            columns,
            ticks,
            descriptors,
            write_count: 0,
            capacity,
        }
    }

    /// Record one snapshot of stats. Called once per snapshot.
    pub fn push(&mut self, values: &[f64], tick: u64) {
        if self.write_count < self.capacity {
            // Still filling up — just append
            for (col, val) in self.columns.iter_mut().zip(values) {
                col.push(*val);
            }
            self.ticks.push(tick);
        } else {
            // Ring buffer full — overwrite oldest
            let idx = self.write_count % self.capacity;
            for (col, val) in self.columns.iter_mut().zip(values) {
                col[idx] = *val;
            }
            self.ticks[idx] = tick;
        }
        self.write_count += 1;
    }

    pub fn descriptors(&self) -> &[StatDescriptor] {
        &self.descriptors
    }

    /// Number of entries actually stored (up to capacity).
    pub fn len(&self) -> usize {
        self.write_count.min(self.capacity)
    }

    pub fn is_empty(&self) -> bool {
        self.write_count == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Total writes (including wrapped).
    pub fn write_count(&self) -> usize {
        self.write_count
    }

    /// Get the value at logical index `j` (0 = oldest visible entry) for column `col`.
    /// Returns `None` if out of bounds or no tick is available for that entry (shouldn't happen in practice).
    pub fn get(&self, col: usize, j: usize) -> Option<(f64, u64)> {
        let filled = self.len();
        if j >= filled {
            return None;
        }
        let column = self.columns.get(col)?;
        let start = self.write_count.saturating_sub(self.capacity);
        let buf_idx = (start + j) % self.capacity;
        let value = column.get(buf_idx).copied();
        let tick = self.ticks.get(buf_idx).copied()?;
        value.map(|v| (v, tick))
    }

    /// Get the tick value at logical index `j` (0 = oldest visible entry). Returns `None` if out of bounds.
    pub fn get_tick(&self, j: usize) -> Option<u64> {
        let filled = self.len();
        if j >= filled {
            return None;
        }
        let start = self.write_count.saturating_sub(self.capacity);
        let buf_idx = (start + j) % self.capacity;
        self.ticks.get(buf_idx).copied()
    }

    /// Heap bytes used by all column buffers.
    pub fn heap_bytes(&self) -> usize {
        self.columns.iter().map(|c| c.capacity() * 8).sum::<usize>() + self.ticks.capacity() * 8
    }

    /// Change the ring-buffer capacity, keeping the most recent entries.
    pub fn resize(&mut self, new_capacity: usize) {
        let filled = self.len();
        let keep = filled.min(new_capacity);
        let skip = filled - keep;

        let new_columns: Vec<Vec<f64>> = (0..self.columns.len())
            .map(|col| {
                (skip..filled)
                    .filter_map(|j| self.get(col, j).map(|(v, _)| v))
                    .collect()
            })
            .collect();

        let new_ticks: Vec<u64> = (skip..filled).filter_map(|j| self.get_tick(j)).collect();

        self.columns = new_columns;
        self.ticks = new_ticks;
        self.capacity = new_capacity;
        self.write_count = keep;
    }
}
