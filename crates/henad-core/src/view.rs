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
///
/// The charts read one `f64` per series per frame, so that is what a sample costs. A series a
/// scalar cannot round-trip keeps its full value alongside, which is what lets an export carry the
/// same columns the headless runner writes.
pub struct StatsHistory {
    /// One column per stat series, each holding `capacity` entries.
    columns: Vec<Vec<f64>>,
    /// Full values, `None` for a scalar series whose `f64` column already holds everything.
    full: Vec<Option<Vec<StatValue>>>,
    /// Ticks corresponding to each entry in the columns. Same length as each column.
    ticks: Vec<u64>,
    descriptors: Vec<StatDescriptor>,
    /// Total number of entries written (may exceed capacity).
    write_count: usize,
    /// `None` retains every sample, so a whole run can be exported.
    capacity: Option<usize>,
}

impl StatsHistory {
    pub fn new(descriptors: Vec<StatDescriptor>, capacity: Option<usize>) -> Self {
        let reserve = capacity.unwrap_or(0);
        let columns = vec![Vec::with_capacity(reserve); descriptors.len()];
        let full = vec![None; descriptors.len()];
        let ticks = Vec::with_capacity(reserve);
        Self {
            columns,
            full,
            ticks,
            descriptors,
            write_count: 0,
            capacity,
        }
    }

    /// Record one sample.
    ///
    /// The first sample fixes which series keep a full value, the same way it fixes a CSV column
    /// layout. A series that changes kind later is a model bug, and
    /// [`crate::export::StatsWriter`] already reports it as one.
    pub fn push_entries(&mut self, stats: &[StatEntry], tick: u64) {
        if self.write_count == 0 {
            for (slot, entry) in self.full.iter_mut().zip(stats) {
                if !matches!(entry.value, StatValue::Scalar(_)) {
                    *slot = Some(Vec::new());
                }
            }
        }

        match self.capacity {
            Some(capacity) if self.write_count >= capacity => {
                // Full, so overwrite the oldest.
                let idx = self.write_count % capacity;
                for (col, entry) in self.columns.iter_mut().zip(stats) {
                    col[idx] = entry.value.scalar();
                }
                for (slot, entry) in self.full.iter_mut().zip(stats) {
                    if let Some(values) = slot {
                        values[idx] = entry.value.clone();
                    }
                }
                self.ticks[idx] = tick;
            }
            _ => {
                for (col, entry) in self.columns.iter_mut().zip(stats) {
                    col.push(entry.value.scalar());
                }
                for (slot, entry) in self.full.iter_mut().zip(stats) {
                    if let Some(values) = slot {
                        values.push(entry.value.clone());
                    }
                }
                self.ticks.push(tick);
            }
        }
        self.write_count += 1;
    }

    pub fn descriptors(&self) -> &[StatDescriptor] {
        &self.descriptors
    }

    /// Number of entries actually stored (up to capacity).
    pub fn len(&self) -> usize {
        self.capacity.map_or(self.write_count, |cap| self.write_count.min(cap))
    }

    pub fn is_empty(&self) -> bool {
        self.write_count == 0
    }

    /// `None` while the history is retaining everything.
    pub fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    /// Total writes (including wrapped).
    pub fn write_count(&self) -> usize {
        self.write_count
    }

    /// Buffer slot holding logical index `j`, where 0 is the oldest visible entry.
    fn buf_index(&self, j: usize) -> Option<usize> {
        if j >= self.len() {
            return None;
        }
        match self.capacity {
            Some(capacity) => Some((self.write_count.saturating_sub(capacity) + j) % capacity),
            None => Some(j),
        }
    }

    /// Value and tick at logical index `j`, where 0 is the oldest visible entry.
    pub fn get(&self, col: usize, j: usize) -> Option<(f64, u64)> {
        let idx = self.buf_index(j)?;
        let value = self.columns.get(col)?.get(idx).copied()?;
        let tick = self.ticks.get(idx).copied()?;
        Some((value, tick))
    }

    /// Tick at logical index `j`, where 0 is the oldest visible entry.
    pub fn tick(&self, j: usize) -> Option<u64> {
        self.ticks.get(self.buf_index(j)?).copied()
    }

    /// The sample at logical index `j`, in the shape [`crate::export::StatsWriter`] takes.
    pub fn entries(&self, j: usize) -> Option<Vec<StatEntry>> {
        let idx = self.buf_index(j)?;
        let entries = self
            .descriptors
            .iter()
            .enumerate()
            .map(|(col, desc)| {
                let value = match self.full.get(col).and_then(Option::as_ref) {
                    Some(values) => values[idx].clone(),
                    None => StatValue::Scalar(self.columns[col][idx]),
                };
                StatEntry {
                    label: desc.label,
                    value,
                    color: desc.color,
                }
            })
            .collect();
        Some(entries)
    }

    pub fn heap_bytes(&self) -> usize {
        let scalars = self.columns.iter().map(|c| c.capacity() * 8).sum::<usize>();
        let full = self
            .full
            .iter()
            .flatten()
            .map(|values| {
                values.capacity() * size_of::<StatValue>() + values.iter().map(stat_value_heap).sum::<usize>()
            })
            .sum::<usize>();
        scalars + full + self.ticks.capacity() * 8
    }

    /// Keeps the most recent entries.
    pub fn resize(&mut self, new_capacity: Option<usize>) {
        let filled = self.len();
        let keep = new_capacity.map_or(filled, |cap| filled.min(cap));
        let skip = filled - keep;

        let slots: Vec<usize> = (skip..filled).filter_map(|j| self.buf_index(j)).collect();

        let new_columns: Vec<Vec<f64>> = self
            .columns
            .iter()
            .map(|column| slots.iter().map(|&i| column[i]).collect())
            .collect();
        let new_full: Vec<Option<Vec<StatValue>>> = self
            .full
            .iter()
            .map(|slot| {
                slot.as_ref()
                    .map(|values| slots.iter().map(|&i| values[i].clone()).collect())
            })
            .collect();
        let new_ticks: Vec<u64> = slots.iter().map(|&i| self.ticks[i]).collect();

        self.columns = new_columns;
        self.full = new_full;
        self.ticks = new_ticks;
        self.capacity = new_capacity;
        self.write_count = keep;
    }
}

/// Heap a stat value owns beyond its own bytes. Only a histogram has any.
fn stat_value_heap(value: &StatValue) -> usize {
    match value {
        StatValue::Scalar(_) | StatValue::Vector2D { .. } => 0,
        StatValue::Histogram { edges, counts } => edges.capacity() * 8 + counts.capacity() * 8,
    }
}

#[cfg(test)]
mod tests {
    use super::{StatDescriptor, StatEntry, StatValue, StatsHistory};

    const C: [u8; 4] = [1, 2, 3, 255];

    fn history(capacity: Option<usize>, labels: &[&'static str]) -> StatsHistory {
        let descriptors = labels.iter().map(|l| StatDescriptor::new(l, C)).collect();
        StatsHistory::new(descriptors, capacity)
    }

    fn scalars(values: &[f64]) -> Vec<StatEntry> {
        values
            .iter()
            .map(|v| StatEntry {
                label: "s",
                value: StatValue::Scalar(*v),
                color: C,
            })
            .collect()
    }

    fn vec2(x: f64, y: f64) -> Vec<StatEntry> {
        vec![StatEntry {
            label: "v",
            value: StatValue::Vector2D { x, y },
            color: C,
        }]
    }

    #[test]
    fn a_bounded_history_keeps_the_newest_entries() {
        let mut h = history(Some(3), &["a"]);
        for tick in 0u32..5 {
            h.push_entries(&scalars(&[f64::from(tick)]), u64::from(tick));
        }
        assert_eq!(h.len(), 3);
        assert_eq!(h.get(0, 0), Some((2.0, 2)));
        assert_eq!(h.get(0, 2), Some((4.0, 4)));
        assert_eq!(h.get(0, 3), None);
    }

    #[test]
    fn an_unlimited_history_drops_nothing() {
        let mut h = history(None, &["a"]);
        for tick in 0u32..1000 {
            h.push_entries(&scalars(&[f64::from(tick)]), u64::from(tick));
        }
        assert_eq!(h.len(), 1000);
        assert_eq!(h.capacity(), None);
        assert_eq!(h.get(0, 0), Some((0.0, 0)));
        assert_eq!(h.get(0, 999), Some((999.0, 999)));
    }

    /// The regression this exists for: a scalar cannot round-trip a vector, so an export drawn
    /// from history would lose x and y.
    #[test]
    fn a_vector_series_keeps_its_components() {
        let mut h = history(Some(4), &["v"]);
        h.push_entries(&vec2(3.0, 4.0), 0);
        h.push_entries(&vec2(6.0, 8.0), 1);

        // The chart still sees one number, the magnitude.
        assert_eq!(h.get(0, 0), Some((5.0, 0)));

        let sample = h.entries(1).expect("index 1 exists");
        assert!(
            matches!(sample[0].value, StatValue::Vector2D { x, y } if x == 6.0 && y == 8.0),
            "got {:?}",
            sample[0].value
        );
        assert_eq!(sample[0].label, "v");
    }

    #[test]
    fn a_scalar_series_rebuilds_from_its_own_column() {
        let mut h = history(Some(4), &["a", "b"]);
        h.push_entries(&scalars(&[1.0, 2.0]), 7);
        let sample = h.entries(0).expect("index 0 exists");
        assert_eq!(sample.len(), 2);
        assert!(matches!(sample[0].value, StatValue::Scalar(v) if v == 1.0));
        assert!(matches!(sample[1].value, StatValue::Scalar(v) if v == 2.0));
        assert!(h.entries(1).is_none());
    }

    /// Wrapping must not scramble the full values against their scalars.
    #[test]
    fn full_values_wrap_with_their_column() {
        let mut h = history(Some(2), &["v"]);
        for i in 0u32..5 {
            h.push_entries(&vec2(f64::from(i), 0.0), u64::from(i));
        }
        for (j, expected) in [3.0, 4.0].into_iter().enumerate() {
            let sample = h.entries(j).expect("both slots are filled");
            assert!(
                matches!(sample[0].value, StatValue::Vector2D { x, .. } if x == expected),
                "slot {j} got {:?}",
                sample[0].value
            );
            assert_eq!(h.get(0, j).map(|(v, _)| v), Some(expected));
        }
    }

    #[test]
    fn shrinking_keeps_the_newest_and_growing_keeps_everything() {
        let mut h = history(Some(10), &["v"]);
        for i in 0u32..6 {
            h.push_entries(&vec2(f64::from(i), 0.0), u64::from(i));
        }

        h.resize(Some(3));
        assert_eq!(h.len(), 3);
        assert_eq!(h.tick(0), Some(3));
        assert!(matches!(h.entries(0).expect("kept")[0].value, StatValue::Vector2D { x, .. } if x == 3.0));

        h.resize(None);
        assert_eq!(h.len(), 3, "growing adds no samples back");
        h.push_entries(&vec2(9.0, 0.0), 9);
        assert_eq!(h.len(), 4);
        assert_eq!(h.tick(3), Some(9));
    }

    #[test]
    fn a_scalar_series_costs_less_than_a_vector_one() {
        let mut scalar = history(Some(64), &["a"]);
        let mut vector = history(Some(64), &["v"]);
        for i in 0u32..64 {
            scalar.push_entries(&scalars(&[f64::from(i)]), u64::from(i));
            vector.push_entries(&vec2(f64::from(i), 0.0), u64::from(i));
        }
        assert!(
            vector.heap_bytes() > scalar.heap_bytes(),
            "{} vs {}",
            vector.heap_bytes(),
            scalar.heap_bytes()
        );
    }
}
