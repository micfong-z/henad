/// A 2D grid for rendering, each cell a `u8` index into the palette.
pub struct GridView<'a> {
    pub width: u32,
    pub height: u32,
    pub cells: &'a [u8],
    pub palette: &'static [[u8; 4]],
}

/// An agent population for rendering.
///
/// Both layers are stretched to the same rect, so a composite model wants
/// `world_w = width as f32`. Nothing checks this across the crate boundary.
pub struct PointView<'a> {
    pub pos_x: &'a [f32],
    pub pos_y: &'a [f32],
    pub world_w: f32,
    pub world_h: f32,
    /// One palette index per agent. `None` colours the whole population `palette[0]`.
    pub color: Option<&'a [u8]>,
    pub palette: &'static [[u8; 4]],
}

#[derive(Debug, Clone)]
pub enum StatValue {
    Scalar(f64),
    Vector2D { x: f64, y: f64 },
    Histogram { edges: Vec<f64>, counts: Vec<u64> },
}

impl StatValue {
    /// A single representative value, for charting.
    pub fn scalar(&self) -> f64 {
        match self {
            Self::Scalar(v) => *v,
            Self::Vector2D { x, y } => x.hypot(*y),
            Self::Histogram { counts, .. } => counts.iter().sum::<u64>() as f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatEntry {
    pub label: &'static str,
    pub value: StatValue,
    pub color: [u8; 4],
}

#[derive(Debug, Clone)]
pub struct StatDescriptor {
    pub label: &'static str,
    pub color: [u8; 4],
}

impl StatDescriptor {
    pub const fn new(label: &'static str, color: [u8; 4]) -> Self {
        Self { label, color }
    }
}

/// Pairs a model's declared series with the values it just produced.
///
/// A model declares labels and colours once as a const and returns bare values, so the two cannot
/// drift. A short `values` leaves the trailing series out rather than mislabelling anything.
pub fn stat_entries(descriptors: &'static [StatDescriptor], values: Vec<StatValue>) -> Vec<StatEntry> {
    descriptors
        .iter()
        .zip(values)
        .map(|(d, value)| StatEntry {
            label: d.label,
            value,
            color: d.color,
        })
        .collect()
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

    pub fn push(&mut self, values: &[f64], tick: u64) {
        if self.write_count < self.capacity {
            for (col, val) in self.columns.iter_mut().zip(values) {
                col.push(*val);
            }
            self.ticks.push(tick);
        } else {
            // Full, so overwrite the oldest.
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

    /// Value and tick at logical index `j`, where 0 is the oldest visible entry.
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

    /// Tick at logical index `j`, where 0 is the oldest visible entry.
    pub fn tick(&self, j: usize) -> Option<u64> {
        let filled = self.len();
        if j >= filled {
            return None;
        }
        let start = self.write_count.saturating_sub(self.capacity);
        let buf_idx = (start + j) % self.capacity;
        self.ticks.get(buf_idx).copied()
    }

    pub fn heap_bytes(&self) -> usize {
        self.columns.iter().map(|c| c.capacity() * 8).sum::<usize>() + self.ticks.capacity() * 8
    }

    /// Keeps the most recent entries.
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

        let new_ticks: Vec<u64> = (skip..filled).filter_map(|j| self.tick(j)).collect();

        self.columns = new_columns;
        self.ticks = new_ticks;
        self.capacity = new_capacity;
        self.write_count = keep;
    }
}
