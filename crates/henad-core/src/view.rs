//! Views a state passes to the renderer, and the stat values and history the UI charts.

/// A 2D grid for rendering, each cell a `u8` index into the palette.
pub struct GridView<'a> {
    /// Width in cells.
    pub width: u32,
    /// Height in cells.
    pub height: u32,
    /// Palette index of each cell, row-major.
    pub cells: &'a [u8],
    /// RGBA colours that the cells index.
    pub palette: &'static [[u8; 4]],
}

/// Prints the grid's size, not its cells.
impl std::fmt::Debug for GridView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridView")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("palette_len", &self.palette.len())
            .finish_non_exhaustive()
    }
}

/// An agent population for rendering.
///
/// Both layers are stretched to the same rect, so a composite model wants
/// `world_w = width as f32`. Nothing checks this across the crate boundary.
pub struct PointView<'a> {
    /// Position of each agent along x.
    pub pos_x: &'a [f32],
    /// Position of each agent along y.
    pub pos_y: &'a [f32],
    /// Width of the world the positions lie in.
    pub world_w: f32,
    /// Height of the world the positions lie in.
    pub world_h: f32,
    /// One palette index per agent. `None` colours the whole population `palette[0]`.
    pub color: Option<&'a [u8]>,
    /// RGBA colours that the agents index.
    pub palette: &'static [[u8; 4]],
}

/// Prints the point count and the world, not the positions.
impl std::fmt::Debug for PointView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointView")
            .field("len", &self.pos_x.len())
            .field("world_w", &self.world_w)
            .field("world_h", &self.world_h)
            .field("colored", &self.color.is_some())
            .field("palette_len", &self.palette.len())
            .finish_non_exhaustive()
    }
}

/// Edges for rendering. Endpoints are indices into the point view's positions.
pub struct EdgeView<'a> {
    /// Source node of each edge.
    pub src: &'a [u32],
    /// Destination node of each edge.
    pub dst: &'a [u32],
    /// One palette index per edge. `None` colours every edge `palette[0]`.
    pub color: Option<&'a [u8]>,
    /// RGBA colours that the edges index.
    pub palette: &'static [[u8; 4]],
    /// Whether the edges are directed.
    pub directed: bool,
    /// Version of the graph. It moves whenever the edges, their colours or their direction change.
    pub version: u64,
}

/// Prints the edge count and the version, not the edges.
impl std::fmt::Debug for EdgeView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeView")
            .field("len", &self.src.len())
            .field("colored", &self.color.is_some())
            .field("palette_len", &self.palette.len())
            .field("directed", &self.directed)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// Value of one stat series at one sample.
#[derive(Debug, Clone)]
pub enum StatValue {
    /// A single number.
    Scalar(f64),
    /// A 2D vector.
    Vector2D {
        /// Component along the x axis.
        x: f64,
        /// Component along the y axis.
        y: f64,
    },
    /// A histogram.
    Histogram {
        /// Bucket boundaries, one more than there are buckets.
        edges: Vec<f64>,
        /// Number of values in each bucket. `counts[i]` counts the values in `[edges[i], edges[i + 1])`.
        counts: Vec<u64>,
    },
}

impl StatValue {
    /// Returns one representative number, for charting: the value itself, a vector's magnitude or a
    /// histogram's total count.
    pub fn scalar(&self) -> f64 {
        match self {
            Self::Scalar(v) => *v,
            Self::Vector2D { x, y } => x.hypot(*y),
            Self::Histogram { counts, .. } => counts.iter().sum::<u64>() as f64,
        }
    }
}

/// One sample of a stat series, with the series' label and colour.
#[derive(Debug, Clone)]
pub struct StatEntry {
    /// Label of the series.
    pub label: &'static str,
    /// Value of this sample.
    pub value: StatValue,
    /// RGBA colour of the series.
    pub color: [u8; 4],
}

/// A stat series a model declares.
#[derive(Debug, Clone)]
pub struct StatDescriptor {
    /// Label that identifies the series in the UI, in result columns and in a stop condition.
    pub label: &'static str,
    /// RGBA colour of the series in the chart.
    pub color: [u8; 4],
}

impl StatDescriptor {
    /// Creates a descriptor from a label and a colour.
    pub const fn new(label: &'static str, color: [u8; 4]) -> Self {
        Self { label, color }
    }
}

/// Pairs a model's declared series with the values it just produced.
///
/// A model declares labels and colours once as a const and returns bare values, so the labels and the values
/// cannot drift apart. A short `values` leaves the trailing series out rather than mislabelling anything.
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
/// The charts read one `f64` per series per frame, so that is what a sample costs. A series that
/// a scalar cannot round-trip keeps its full value alongside. An export then carries the same columns
/// that the headless runner writes.
pub struct StatsHistory {
    /// One column per stat series, each holding `capacity` entries.
    columns: Vec<Vec<f64>>,
    /// Full values, `None` for a scalar series whose `f64` column already holds everything.
    full: Vec<Option<Vec<StatValue>>>,
    /// Ticks corresponding to each entry in the columns. Same length as each column.
    ticks: Vec<u64>,
    descriptors: Vec<StatDescriptor>,
    /// Total number of entries written, past the capacity once the history wraps.
    write_count: usize,
    /// Maximum number of samples kept, `None` to keep every sample so a whole run can be exported.
    capacity: Option<usize>,
}

/// Prints the series labels and the sample counts, not the samples.
impl std::fmt::Debug for StatsHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let labels: Vec<&str> = self.descriptors.iter().map(|descriptor| descriptor.label).collect();
        f.debug_struct("StatsHistory")
            .field("labels", &labels)
            .field("len", &self.len())
            .field("write_count", &self.write_count)
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl StatsHistory {
    /// Creates an empty history of `descriptors`, keeping at most `capacity` samples, or every sample
    /// for `None`.
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

    /// Records one sample, replacing the newest entry if it has the same tick.
    ///
    /// A publish repeats a tick when nothing stepped, as after an action or while a paused layout
    /// relaxes. The replacement keeps one entry per tick, holding the latest stats for it.
    ///
    /// The first sample fixes which series keep a full value, the same way it fixes a CSV column
    /// layout. A series that changes kind later is a model bug, and
    /// [`crate::export::StatsWriter`] already reports it as a bug.
    pub fn push_entries(&mut self, stats: &[StatEntry], tick: u64) {
        let newest = self.len().checked_sub(1).and_then(|j| self.buf_index(j));
        if let Some(idx) = newest
            && self.ticks[idx] == tick
        {
            self.write_at(idx, stats, tick);
            return;
        }

        if self.write_count == 0 {
            for (slot, entry) in self.full.iter_mut().zip(stats) {
                if !matches!(entry.value, StatValue::Scalar(_)) {
                    *slot = Some(Vec::new());
                }
            }
        }

        match self.capacity {
            Some(capacity) if self.write_count >= capacity => {
                // The buffer is full, so the oldest entry is overwritten.
                self.write_at(self.write_count % capacity, stats, tick);
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

    /// Writes a sample over buffer slot `idx`.
    fn write_at(&mut self, idx: usize, stats: &[StatEntry], tick: u64) {
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

    /// Series the history records.
    pub fn descriptors(&self) -> &[StatDescriptor] {
        &self.descriptors
    }

    /// Number of entries stored, at most the capacity.
    pub fn len(&self) -> usize {
        self.capacity.map_or(self.write_count, |cap| self.write_count.min(cap))
    }

    /// Returns whether no sample has been recorded.
    pub fn is_empty(&self) -> bool {
        self.write_count == 0
    }

    /// Maximum number of samples the history keeps, or `None` while it keeps every sample.
    pub fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    /// Total number of writes, including those that wrapped.
    pub fn write_count(&self) -> usize {
        self.write_count
    }

    /// Returns the buffer slot holding logical index `j`, where 0 is the oldest visible entry.
    fn buf_index(&self, j: usize) -> Option<usize> {
        if j >= self.len() {
            return None;
        }
        match self.capacity {
            Some(capacity) => Some((self.write_count.saturating_sub(capacity) + j) % capacity),
            None => Some(j),
        }
    }

    /// Returns the value of series `col` and its tick at logical index `j`, where 0 is the oldest visible entry.
    pub fn get(&self, col: usize, j: usize) -> Option<(f64, u64)> {
        let idx = self.buf_index(j)?;
        let value = self.columns.get(col)?.get(idx).copied()?;
        let tick = self.ticks.get(idx).copied()?;
        Some((value, tick))
    }

    /// Returns the tick at logical index `j`, where 0 is the oldest visible entry.
    pub fn tick(&self, j: usize) -> Option<u64> {
        self.ticks.get(self.buf_index(j)?).copied()
    }

    /// Returns the sample at logical index `j`, in the shape that [`crate::export::StatsWriter`] accepts.
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

    /// Heap memory held by the history, in bytes.
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

    /// Changes the capacity to `new_capacity`, keeping the most recent entries that fit.
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

/// Heap memory that a stat value owns beyond its own bytes. Only a histogram owns heap memory.
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

    /// An action publishes at the tick it was pressed on. Its stats replace that tick's entry
    /// rather than adding a second entry, whether or not a bounded history has wrapped.
    #[test]
    fn a_sample_at_the_newest_tick_replaces_it() {
        for (capacity, ticks) in [(None, 3u32), (Some(2), 2), (Some(2), 5)] {
            let mut h = history(capacity, &["v"]);
            for tick in 0..ticks {
                h.push_entries(&vec2(f64::from(tick), 0.0), u64::from(tick));
            }
            let newest = u64::from(ticks - 1);
            let (len, writes) = (h.len(), h.write_count());
            h.push_entries(&vec2(3.0, 4.0), newest);

            assert_eq!((h.len(), h.write_count()), (len, writes), "a second entry was added");
            assert_eq!(h.get(0, len - 1), Some((5.0, newest)));
            let sample = h.entries(len - 1).expect("the newest entry exists");
            assert!(matches!(sample[0].value, StatValue::Vector2D { x, y } if x == 3.0 && y == 4.0));
            assert_eq!(h.tick(len - 2), Some(newest - 1), "an older entry changed");
        }
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

    /// A scalar cannot round-trip a vector, and an export drawn from history must still carry x and y.
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
