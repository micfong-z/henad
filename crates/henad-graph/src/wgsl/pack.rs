//! Placement of a graph's lanes in device buffers.
//!
//! Each lane of a compiled graph sits in one *lane buffer*. A lane buffer holds one *segment* per lane, every element
//! of the lane in element order, and the segments follow one another in the buffer. A lane buffer of one lane is the
//! lane's own buffer, and a lane buffer of several is *packed*: its words are `u32`, and a shader reads and writes each
//! lane's words at the lane's segment through `bitcast`.
//!
//! The compiler packs lanes only when a shader would bind more than [`MAX_STORAGE_BINDINGS`] storage buffers, a fixed
//! budget. A *plain* lane, one with one side, packs first. A *dual* lane, one with two sides that swap at the end of
//! the tick, packs only with dual lanes whose writer is the same step, so the lanes of one buffer share their sides at
//! every step. Lanes pack only with lanes of their domain that the same actions rerun, since a rerun uploads a buffer
//! whole. The drawable position lane never packs. A packed buffer holds at most [`MAX_PACKED_BUFFER_BYTES`] at the
//! largest count of its domain, and lanes past it start another buffer.

use std::ops::Range;

use henad_compute::gpu::ShaderBinding;
use henad_core::diagnostic::CodeDiagnostic;

use super::GeneratedShader;
use crate::document::{Domain, GraphDocument, Lane};
use crate::host::value::words_per_value;
use crate::limits::{MAX_PACKED_BUFFER_BYTES, MAX_STORAGE_BINDINGS};
use crate::types::ValueType;

/// Buffer and segment that hold one lane of a compiled graph on the device.
///
/// The segment starts `offset * count` words into the buffer and holds `words * count` words, where `count` is the
/// number of elements of the lane's domain. A buffer of `stride` words per element holds `stride * count` words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneBuffer {
    buffer: usize,
    offset: usize,
    words: usize,
    stride: usize,
}

impl LaneBuffer {
    /// Position of the buffer among the buffers of the program's spec, as `read_buffer` numbers them.
    pub fn buffer(&self) -> usize {
        self.buffer
    }

    /// Words of each element of the lane.
    pub fn words(&self) -> usize {
        self.words
    }

    /// Words per element of the lanes before this one in its buffer.
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    /// Returns whether the buffer holds other lanes beside this one.
    pub fn is_packed(&self) -> bool {
        self.stride != self.words
    }

    /// Returns the range of words the lane's segment takes in a buffer of `buffer_words` words.
    pub fn segment_range(&self, buffer_words: usize) -> Range<usize> {
        let count = buffer_words / self.stride.max(1);
        let start = self.offset * count;
        start..start + self.words * count
    }

    /// Returns the lane's words from `buffer`, the words of the whole buffer read back.
    ///
    /// Returns an empty slice when `buffer` is shorter than the segment.
    pub fn segment<'a>(&self, buffer: &'a [u32]) -> &'a [u32] {
        buffer.get(self.segment_range(buffer.len())).unwrap_or_default()
    }
}

/// One lane buffer of a compiled graph and the lanes it holds.
#[derive(Debug, Clone)]
pub(crate) struct LaneGroup {
    /// Label of the buffer.
    pub label: String,
    /// Positions of the lanes it holds, in segment order.
    pub lanes: Vec<usize>,
    /// Words of each lane per element, in the order of `lanes`.
    pub words: Vec<usize>,
    /// Domain of its lanes.
    pub domain: Domain,
    /// Whether the buffer has two sides. The sides swap at the end of the tick.
    pub dual: bool,
}

impl LaneGroup {
    /// Returns whether the buffer holds more than one lane.
    pub(crate) fn is_packed(&self) -> bool {
        self.lanes.len() > 1
    }
}

/// Placement of every lane of a compiled graph: its lane buffers, and the buffer and segment of each lane.
#[derive(Debug, Clone, Default)]
pub(crate) struct LaneLayout {
    /// Lane buffers, in buffer order. They come first among the spec's buffers.
    pub groups: Vec<LaneGroup>,
    /// Buffer and segment of each lane by its position, `None` for a lane no buffer holds.
    pub places: Vec<Option<LaneBuffer>>,
}

/// Lanes that may share one buffer: their domain, the step that writes them to their next side, and the actions whose
/// reruns write them.
type PackKey<'a> = (Domain, Option<usize>, &'a [usize]);

/// Lanes a packed layout may pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packing {
    /// Plain lanes only.
    Plain,
    /// Plain lanes, then dual lanes.
    PlainAndDual,
}

/// One lane as the layout places it.
#[derive(Debug, Clone)]
pub(crate) struct LaneEntry<'a> {
    /// Label of the lane's own buffer, `lane_<id>_<domain>`.
    pub label: String,
    /// Type of the lane's values.
    pub value_type: &'a ValueType,
    /// Domain of the lane.
    pub domain: Domain,
    /// Position among the steps of the step that writes the lane to its next side, `None` for a plain lane.
    pub writer: Option<usize>,
    /// Whether a buffer holds the lane. A merged lane of a field update has none.
    pub held: bool,
    /// Whether the lane may share a buffer: a lane a shader holds, other than the drawable position lane.
    pub packable: bool,
    /// Positions of the actions whose reruns write the lane, in order.
    pub reruns: Vec<usize>,
    /// Largest count of the lane's domain the file allows.
    pub largest_count: u64,
}

impl LaneEntry<'_> {
    /// Words of the lane per element.
    pub(crate) fn words(&self) -> usize {
        words_per_value(self.value_type).unwrap_or(1)
    }

    /// Bytes of the lane at the largest count of its domain.
    fn largest_bytes(&self) -> u64 {
        self.words() as u64 * 4 * self.largest_count
    }
}

impl LaneLayout {
    /// Returns the layout that gives every lane of `lanes` its own buffer.
    pub(crate) fn unpacked(lanes: &[LaneEntry<'_>]) -> Self {
        Self::of_groups(
            lanes,
            lanes.iter().enumerate().map(|(position, _)| vec![position]).collect(),
        )
    }

    /// Returns the layout that packs the lanes of `lanes` that `packing` allows, each with the lanes of its domain,
    /// its writer and its reruns.
    pub(crate) fn packed(lanes: &[LaneEntry<'_>], packing: Packing) -> Self {
        let mut sets: Vec<Vec<usize>> = Vec::new();
        // The open set of each key, by the position in `sets` of its last set.
        let mut open: Vec<(PackKey<'_>, usize)> = Vec::new();
        for (position, lane) in lanes.iter().enumerate() {
            let allowed = lane.packable && (lane.writer.is_none() || packing == Packing::PlainAndDual);
            if !allowed || lane.largest_bytes() > MAX_PACKED_BUFFER_BYTES {
                sets.push(vec![position]);
                continue;
            }
            let key = (lane.domain, lane.writer, lane.reruns.as_slice());
            let fits = |set: &[usize]| {
                let bytes: u64 = set.iter().map(|&known| lanes[known].largest_bytes()).sum();
                bytes + lane.largest_bytes() <= MAX_PACKED_BUFFER_BYTES
            };
            match open.iter_mut().find(|(known, _)| *known == key) {
                Some((_, index)) if fits(&sets[*index]) => sets[*index].push(position),
                Some((_, index)) => {
                    *index = sets.len();
                    sets.push(vec![position]);
                }
                None => {
                    open.push((key, sets.len()));
                    sets.push(vec![position]);
                }
            }
        }
        Self::of_groups(lanes, sets)
    }

    /// Returns the layout of `sets`, each the positions of the lanes one buffer holds, in the order of each set's first
    /// lane.
    fn of_groups(lanes: &[LaneEntry<'_>], mut sets: Vec<Vec<usize>>) -> Self {
        sets.retain(|set| set.first().is_some_and(|&first| lanes[first].held));
        sets.sort_by_key(|set| set.first().copied());
        let mut layout = Self {
            groups: Vec::new(),
            places: vec![None; lanes.len()],
        };
        // Domain of each packed buffer so far.
        let mut packed_buffers: Vec<Domain> = Vec::new();
        for set in sets {
            let buffer = layout.groups.len();
            let words: Vec<usize> = set.iter().map(|&position| lanes[position].words()).collect();
            let stride = words.iter().sum();
            let mut offset = 0;
            for (&position, &lane_words) in set.iter().zip(&words) {
                layout.places[position] = Some(LaneBuffer {
                    buffer,
                    offset,
                    words: lane_words,
                    stride,
                });
                offset += lane_words;
            }
            let first = &lanes[set[0]];
            let label = if set.len() > 1 {
                let number = packed_buffers.iter().filter(|domain| **domain == first.domain).count();
                packed_buffers.push(first.domain);
                format!("lanes_{}_{number}", first.domain.as_str())
            } else {
                first.label.clone()
            };
            layout.groups.push(LaneGroup {
                label,
                lanes: set,
                words,
                domain: first.domain,
                dual: first.writer.is_some(),
            });
        }
        layout
    }

    /// Returns the buffer and segment of the lane at `position`.
    pub(crate) fn place(&self, position: usize) -> Option<LaneBuffer> {
        self.places.get(position).copied().flatten()
    }

    /// Returns the buffer and segment of the lane `id` of `document`, the document the layout places.
    pub(crate) fn place_of(&self, document: &GraphDocument, id: &str) -> Option<LaneBuffer> {
        self.place(document.lanes.iter().position(|lane| lane.id == id)?)
    }

    /// Returns the lane buffer that holds the lane at `position`.
    pub(crate) fn group_of(&self, position: usize) -> Option<&LaneGroup> {
        self.groups.get(self.place(position)?.buffer)
    }

    /// Returns the label of each lane buffer in buffer order with the ids of the lanes it holds in segment order, where
    /// `lanes` holds the lanes of the graph by position.
    pub(crate) fn lane_ids(&self, lanes: &[Lane]) -> Vec<(String, Vec<String>)> {
        self.groups
            .iter()
            .map(|group| {
                let ids = group.lanes.iter().filter_map(|&position| lanes.get(position));
                (group.label.clone(), ids.map(|lane| lane.id.clone()).collect())
            })
            .collect()
    }
}

#[cfg(test)]
thread_local! {
    /// Whether [`choose`] keeps every lane in a buffer of its own on this thread, for a test that runs a packed model
    /// beside its unpacked twin.
    pub(crate) static UNPACKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Returns whether every shader of `bindings`, each the bindings of one shader, binds at most
/// [`MAX_STORAGE_BINDINGS`] storage buffers.
fn within_budget<'a>(bindings: impl IntoIterator<Item = &'a [ShaderBinding]>) -> bool {
    bindings
        .into_iter()
        .all(|shader| storage_bindings(shader) <= MAX_STORAGE_BINDINGS as usize)
}

/// Returns the number of storage buffers among `bindings`.
pub(crate) fn storage_bindings(bindings: &[ShaderBinding]) -> usize {
    bindings
        .iter()
        .filter(|binding| binding.kind.is_storage_buffer())
        .count()
}

/// Returns the layout of `lanes` that keeps every shader within [`MAX_STORAGE_BINDINGS`] storage buffers, with the
/// shaders `emit` writes for it.
///
/// `emit` writes every shader of the graph for one layout. The lanes stay unpacked while the shaders fit, then the
/// plain lanes pack, then the dual lanes too. A layout whose shaders still bind too many storage buffers is returned as
/// it is, and a device with too few bindings refuses the model when it builds.
///
/// # Errors
///
/// Returns the problems of the first emission that fails.
pub(crate) fn choose(
    lanes: &[LaneEntry<'_>],
    mut emit: impl FnMut(&LaneLayout) -> Result<Vec<GeneratedShader>, Vec<CodeDiagnostic>>,
) -> Result<(LaneLayout, Vec<GeneratedShader>), Vec<CodeDiagnostic>> {
    let mut layout = LaneLayout::unpacked(lanes);
    let mut shaders = emit(&layout)?;
    #[cfg(test)]
    if UNPACKED.with(std::cell::Cell::get) {
        return Ok((layout, shaders));
    }
    for packing in [Packing::Plain, Packing::PlainAndDual] {
        if within_budget(shaders.iter().map(GeneratedShader::bindings)) {
            break;
        }
        let next = LaneLayout::packed(lanes, packing);
        if next.groups.len() == layout.groups.len() {
            continue;
        }
        shaders = emit(&next)?;
        layout = next;
    }
    Ok((layout, shaders))
}

#[cfg(test)]
mod tests {
    use henad_compute::gpu::wgpu;

    use super::{LaneEntry, LaneLayout, Packing};
    use crate::document::Domain;
    use crate::limits::{MAX_PACKED_BUFFER_BYTES, MAX_STORAGE_BINDINGS};
    use crate::types::ValueType;

    /// Returns a packable plain lane `id` of one `f32` over agents, at most `largest_count` of them.
    fn lane(id: &str, largest_count: u64) -> LaneEntry<'static> {
        LaneEntry {
            label: format!("lane_{id}_agents"),
            value_type: &ValueType::F32,
            domain: Domain::Agents,
            writer: None,
            held: true,
            packable: true,
            reruns: Vec::new(),
            largest_count,
        }
    }

    /// Returns the lanes of each buffer of `layout`.
    fn sets(layout: &LaneLayout) -> Vec<Vec<usize>> {
        layout.groups.iter().map(|group| group.lanes.clone()).collect()
    }

    #[test]
    fn the_fixed_limits_are_the_webgpu_defaults() {
        let defaults = wgpu::Limits::default();
        assert_eq!(MAX_STORAGE_BINDINGS, defaults.max_storage_buffers_per_shader_stage);
        assert_eq!(MAX_PACKED_BUFFER_BYTES, defaults.max_storage_buffer_binding_size);
    }

    #[test]
    fn lanes_pack_only_with_lanes_of_their_domain_writer_and_reruns() {
        let mut lanes = vec![lane("a", 1000); 9];
        lanes[1].packable = false;
        lanes[2].domain = Domain::Cells;
        lanes[3].reruns = vec![0];
        lanes[4].writer = Some(0);
        lanes[5].writer = Some(1);
        lanes[6].writer = Some(0);
        lanes[7].value_type = &ValueType::Vec2F;
        lanes[8].domain = Domain::Cells;
        let plain = LaneLayout::packed(&lanes, Packing::Plain);
        assert_eq!(
            sets(&plain),
            [vec![0, 7], vec![1], vec![2, 8], vec![3], vec![4], vec![5], vec![6]]
        );
        let dual = LaneLayout::packed(&lanes, Packing::PlainAndDual);
        assert_eq!(
            sets(&dual),
            [vec![0, 7], vec![1], vec![2, 8], vec![3], vec![4, 6], vec![5]]
        );
        let labels: Vec<&str> = dual.groups.iter().map(|group| group.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "lanes_agents_0",
                "lane_a_agents",
                "lanes_cells_0",
                "lane_a_agents",
                "lanes_agents_1",
                "lane_a_agents"
            ]
        );
        assert!(dual.groups[4].dual && !dual.groups[0].dual);
        let vector = dual.place(7).expect("a place");
        assert_eq!((vector.buffer(), vector.offset(), vector.words()), (0, 1, 2));
        assert_eq!(
            vector.segment(&[10, 11, 12, 20, 21, 22, 23, 24, 25]),
            [20, 21, 22, 23, 24, 25]
        );
        assert!(vector.is_packed());
        assert!(!dual.place(1).expect("a place").is_packed());
    }

    #[test]
    fn a_packed_buffer_holds_at_most_the_baseline_binding_size_at_the_largest_count() {
        // Ten million agents of one word each take 40 MB a lane, and three lanes fit in 128 MiB.
        let lanes = vec![lane("a", 10_000_000); 5];
        let layout = LaneLayout::packed(&lanes, Packing::Plain);
        assert_eq!(sets(&layout), [vec![0, 1, 2], vec![3, 4]]);
        assert_eq!(layout.groups[1].label, "lanes_agents_1");
        // A lane past the size on its own keeps a buffer of its own.
        let wide = vec![lane("a", 40_000_000); 2];
        assert_eq!(sets(&LaneLayout::packed(&wide, Packing::Plain)), [vec![0], vec![1]]);
    }

    #[test]
    fn a_lane_no_buffer_holds_has_no_place() {
        let mut lanes = vec![lane("a", 1000); 3];
        lanes[2].held = false;
        lanes[2].packable = false;
        let layout = LaneLayout::packed(&lanes, Packing::Plain);
        assert_eq!(sets(&layout), [vec![0, 1]]);
        assert!(layout.place(2).is_none());
    }
}
