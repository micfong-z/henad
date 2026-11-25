//! Layered left-to-right layout of a body, behind Arrange and the opening of a file without placement.
//!
//! The layout places canvas nodes: every node of the body, and the output half of each region. A canvas node's
//! *layer* is the length of the longest chain of wires that reaches it from a canvas node that reads nothing of the
//! body. A link to a region's inner output comes from its input half and a link to an accumulator from its output
//! half. Every node inside a region reads its input half and feeds its output half, which puts a region's body
//! between its halves, and a region's input half then moves right to the layer before its first reader. Layers become
//! columns from left to right, so every wire of an acyclic body runs rightwards.
//! Within a column a node sits at the mean row of the nodes it reads, ties broken by id. A column is as wide as its
//! widest node, and the nodes of a column stack with a gap between them.
//!
//! A *frame group* holds the members of one frame of the `[editor]` region that records the sheet lines its nodes came
//! from, as a converted copy's frames do. The layout keeps a group's nodes together: the group takes a band of rows of
//! its own across the columns its nodes and its title cover, below every node placed before it in those columns, with
//! room for the frame's margin and title. A node in no group is a group of its own, so a body with no such frame
//! stacks each column as before.

use std::collections::{BTreeMap, BTreeSet};

use henad_graph::catalogue::lookup;
use henad_graph::editor::EditorRegion;
use henad_graph::ids::NodeId;
use henad_graph::node::NodeBody;
use henad_graph::value::InputValue;

/// Size in canvas points assumed for a node whose size `arrange` is not given.
pub const NODE_SIZE: [f64; 2] = [220.0, 120.0];

/// Gap in canvas points between two columns.
pub const COLUMN_GAP: f64 = 60.0;

/// Gap in canvas points between two nodes of one column.
pub const ROW_GAP: f64 = 30.0;

/// Space in canvas points that a frame takes above its nodes, for its margin and its title strip, and below them.
const FRAME_SPACE: [f64; 2] = [36.0, 18.0];

/// Width in canvas points assumed for one character of a frame's title.
const TITLE_CHAR_WIDTH: f64 = 6.5;

/// The members of one titled frame, which the layout keeps together.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameGroup {
    /// Document nodes inside the frame.
    pub members: Vec<NodeId>,
    /// Width in canvas points assumed for the frame's title.
    pub title_width: f64,
}

/// Returns the frame groups of the canvas `canvas` in `editor`, such as `pass.step`, in file order: one per frame that
/// records the lines of a sheet its nodes came from, as the frames of a converted copy do. A frame drawn by hand makes
/// no group.
pub fn frame_groups(editor: &EditorRegion, canvas: &str) -> Vec<FrameGroup> {
    editor
        .frames()
        .into_iter()
        .filter(|frame| frame.scope == canvas && !frame.members.is_empty() && !frame.lines.is_empty())
        .map(|frame| FrameGroup {
            members: frame.members,
            title_width: frame.title.chars().count() as f64 * TITLE_CHAR_WIDTH,
        })
        .collect()
}

/// One node of a canvas: a document node, or the input half of a region, or a region's output half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanvasKey {
    /// Document node.
    pub node: NodeId,
    /// Whether the canvas node is the output half of the region `node`.
    pub output_half: bool,
}

impl CanvasKey {
    /// Returns the canvas node of a document node, or a region's input half.
    pub fn main(node: NodeId) -> Self {
        Self {
            node,
            output_half: false,
        }
    }

    /// Returns the output half of the region `node`.
    pub fn output(node: NodeId) -> Self {
        Self {
            node,
            output_half: true,
        }
    }
}

/// Returns the wires between the canvas nodes of `body`, as each canvas node's sources, with every canvas node in body
/// order.
fn canvas_wires(body: &NodeBody, library: u32) -> (Vec<CanvasKey>, BTreeMap<CanvasKey, BTreeSet<CanvasKey>>) {
    let mut keys: Vec<CanvasKey> = Vec::with_capacity(body.nodes.len());
    let mut regions = BTreeSet::new();
    for node in &body.nodes {
        if keys.contains(&CanvasKey::main(node.id)) {
            continue;
        }
        keys.push(CanvasKey::main(node.id));
        if lookup(&node.op, library).is_some_and(|def| def.is_region) {
            keys.push(CanvasKey::output(node.id));
            regions.insert(node.id);
        }
    }
    let known: BTreeSet<CanvasKey> = keys.iter().copied().collect();
    let mut sources: BTreeMap<CanvasKey, BTreeSet<CanvasKey>> = BTreeMap::new();
    for node in &body.nodes {
        for value in node.inputs.values() {
            let InputValue::Link(link) = value else { continue };
            if link.node == node.id {
                continue;
            }
            let inner = regions.contains(&link.node)
                && body.node(link.node).is_some_and(|region| {
                    let port = link.port.as_deref().unwrap_or_default();
                    lookup(&region.op, library).is_some_and(|def| {
                        def.resolved_inner_outputs(region)
                            .iter()
                            .any(|output| output.name == port)
                    })
                });
            let source = if regions.contains(&link.node) && !inner {
                CanvasKey::output(link.node)
            } else {
                CanvasKey::main(link.node)
            };
            if known.contains(&source) {
                sources.entry(CanvasKey::main(node.id)).or_default().insert(source);
            }
        }
    }
    for region in &regions {
        let contents = body.region_contents(*region);
        for member in &contents {
            sources
                .entry(CanvasKey::main(*member))
                .or_default()
                .insert(CanvasKey::main(*region));
        }
        let output = sources.entry(CanvasKey::output(*region)).or_default();
        output.insert(CanvasKey::main(*region));
        for member in contents {
            output.insert(CanvasKey::main(member));
            if regions.contains(&member) {
                output.insert(CanvasKey::output(member));
            }
        }
    }
    (keys, sources)
}

/// Returns a position for every canvas node of `body`, in the body's order with each output half after its region,
/// with the first column's top at `origin`.
///
/// `sizes` holds the drawn size of each canvas node it knows, and a node it lacks takes [`NODE_SIZE`]. A node on a
/// cycle, which a broken file can hold, goes to a column after every other node. The nodes of each of `groups` stay
/// together in a band of their own, as the module doc says.
pub fn arrange(
    body: &NodeBody,
    library: u32,
    origin: [f64; 2],
    sizes: &BTreeMap<CanvasKey, [f64; 2]>,
    groups: &[FrameGroup],
) -> Vec<(CanvasKey, [f64; 2])> {
    let size = |id: &CanvasKey| sizes.get(id).copied().unwrap_or(NODE_SIZE);
    let (ids, sources) = canvas_wires(body, library);
    let mut readers: BTreeMap<CanvasKey, BTreeSet<CanvasKey>> = BTreeMap::new();
    for (reader, from) in &sources {
        for source in from {
            readers.entry(*source).or_default().insert(*reader);
        }
    }

    // Kahn's algorithm over the wires, in id order, gives each node the longest chain that reaches it.
    let mut waiting: BTreeMap<CanvasKey, usize> = ids
        .iter()
        .map(|id| (*id, sources.get(id).map_or(0, BTreeSet::len)))
        .collect();
    let mut ready: BTreeSet<CanvasKey> = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut layers: BTreeMap<CanvasKey, usize> = BTreeMap::new();
    while let Some(id) = ready.pop_first() {
        let layer = sources
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|source| layers.get(source))
            .map(|layer| layer + 1)
            .max()
            .unwrap_or(0);
        layers.insert(id, layer);
        for reader in readers.get(&id).into_iter().flatten() {
            if let Some(count) = waiting.get_mut(reader) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.insert(*reader);
                }
            }
        }
    }
    let after_last = layers.values().max().map_or(0, |layer| layer + 1);
    for id in &ids {
        layers.entry(*id).or_insert(after_last);
    }
    // A region's input half moves right to the column before its first reader. Otherwise a region whose inputs come
    // from no node starts in the first column and its frame spans every column up to its body.
    let halves: Vec<CanvasKey> = ids
        .iter()
        .filter(|id| id.output_half)
        .map(|id| CanvasKey::main(id.node))
        .collect();
    for half in halves {
        let first_reader = readers
            .get(&half)
            .into_iter()
            .flatten()
            .filter_map(|reader| layers.get(reader))
            .min()
            .copied();
        if let (Some(first_reader), Some(layer)) = (first_reader, layers.get_mut(&half)) {
            *layer = (*layer).max(first_reader.saturating_sub(1));
        }
    }

    let mut columns: BTreeMap<usize, Vec<CanvasKey>> = BTreeMap::new();
    for id in &ids {
        columns.entry(layers[id]).or_default().push(*id);
    }
    let layout = order_columns(columns, &sources, origin[0], &size);
    let framed = group_of_nodes(body, library, groups);
    let places = place_groups(&ids, &layout, groups, &framed, origin[1], &size);
    ids.iter().map(|id| (*id, places[id])).collect()
}

/// Columns of a layout: the left edge and the width of each column, and the column and row of each canvas node.
struct ColumnLayout {
    /// Left edge of each column, in canvas points.
    lefts: Vec<f64>,
    /// Width of each column, in canvas points.
    widths: Vec<f64>,
    /// Column of each canvas node.
    column_of: BTreeMap<CanvasKey, usize>,
    /// Row of each canvas node in its column.
    rows: BTreeMap<CanvasKey, f64>,
}

/// Returns the layout of `columns`, the canvas nodes of each layer, with the nodes of a column ordered by the mean row
/// of their `sources` and the first column's left edge at `left`. `size` returns the size of a canvas node.
fn order_columns(
    mut columns: BTreeMap<usize, Vec<CanvasKey>>,
    sources: &BTreeMap<CanvasKey, BTreeSet<CanvasKey>>,
    left: f64,
    size: &impl Fn(&CanvasKey) -> [f64; 2],
) -> ColumnLayout {
    let mut left = left;
    let mut column_of: BTreeMap<CanvasKey, usize> = BTreeMap::new();
    let mut rows: BTreeMap<CanvasKey, f64> = BTreeMap::new();
    let mut lefts: Vec<f64> = Vec::with_capacity(columns.len());
    let mut widths: Vec<f64> = Vec::with_capacity(columns.len());
    for (index, column) in columns.values_mut().enumerate() {
        let barycenter = |id: &CanvasKey| {
            let placed: Vec<f64> = sources
                .get(id)
                .into_iter()
                .flatten()
                .filter_map(|source| rows.get(source).copied())
                .collect();
            if placed.is_empty() {
                f64::MAX
            } else {
                placed.iter().sum::<f64>() / placed.len() as f64
            }
        };
        let mut keyed: Vec<(f64, CanvasKey)> = column.iter().map(|id| (barycenter(id), *id)).collect();
        keyed.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
        *column = keyed.into_iter().map(|(_, id)| id).collect();
        let mut width: f64 = 0.0;
        for (row, id) in column.iter().enumerate() {
            rows.insert(*id, row as f64);
            column_of.insert(*id, index);
            width = width.max(size(id)[0]);
        }
        lefts.push(left);
        widths.push(width);
        left += width + COLUMN_GAP;
    }
    ColumnLayout {
        lefts,
        widths,
        column_of,
        rows,
    }
}

/// Returns the index in `groups` of the group of each node of `body` that one holds.
///
/// A node in several groups belongs to the first. A region whose own node or one of whose contents belongs to a group
/// joins that group with every node inside it. Otherwise the region's frame would reach across another group's band.
fn group_of_nodes(body: &NodeBody, library: u32, groups: &[FrameGroup]) -> BTreeMap<NodeId, usize> {
    let mut framed: BTreeMap<NodeId, usize> = groups
        .iter()
        .enumerate()
        .rev()
        .flat_map(|(index, group)| group.members.iter().map(move |member| (*member, index)))
        .collect();
    for node in &body.nodes {
        if !lookup(&node.op, library).is_some_and(|def| def.is_region) {
            continue;
        }
        let contents = body.region_contents(node.id);
        let group = std::iter::once(node.id)
            .chain(contents.iter().copied())
            .find_map(|member| framed.get(&member).copied());
        if let Some(group) = group {
            for member in std::iter::once(node.id).chain(contents) {
                framed.insert(member, group);
            }
        }
    }
    framed
}

/// Returns the place of each of `ids` in `layout`, the nodes of each of `groups` together in a band of their own, with
/// the bands' tops from `top` down. `framed` holds the group of each node in one, and `size` returns the size of a
/// canvas node.
fn place_groups(
    ids: &[CanvasKey],
    layout: &ColumnLayout,
    groups: &[FrameGroup],
    framed: &BTreeMap<NodeId, usize>,
    top: f64,
    size: &impl Fn(&CanvasKey) -> [f64; 2],
) -> BTreeMap<CanvasKey, [f64; 2]> {
    let ColumnLayout {
        lefts,
        widths,
        column_of,
        rows,
    } = layout;
    // Each canvas node joins the group of its document node, or a group of its own.
    let mut bands: BTreeMap<Option<usize>, Vec<CanvasKey>> = BTreeMap::new();
    let mut singles: Vec<CanvasKey> = Vec::new();
    for id in ids {
        match framed.get(&id.node) {
            Some(group) => bands.entry(Some(*group)).or_default().push(*id),
            None => singles.push(*id),
        }
    }
    let mut placed_bands: Vec<(Option<usize>, Vec<CanvasKey>)> = bands.into_iter().collect();
    placed_bands.extend(singles.into_iter().map(|id| (None, vec![id])));
    placed_bands.sort_by_key(|(_, members)| members.iter().map(|id| (column_of[id], rows[id] as usize, *id)).min());

    let mut skyline: Vec<f64> = vec![top; lefts.len()];
    let mut places: BTreeMap<CanvasKey, [f64; 2]> = BTreeMap::new();
    for (group, mut members) in placed_bands {
        members.sort_by_key(|id| (column_of[id], rows[id] as usize, *id));
        let first_column = members.iter().map(|id| column_of[id]).min().unwrap_or(0);
        let last_column = members.iter().map(|id| column_of[id]).max().unwrap_or(0);
        let [above, below] = if group.is_some() { FRAME_SPACE } else { [0.0, 0.0] };
        let title_right = group.map_or(0.0, |group| lefts[first_column] + groups[group].title_width);
        let right = (lefts[last_column] + widths[last_column]).max(title_right);
        let covered: Vec<usize> = (0..lefts.len())
            .filter(|&column| column >= first_column && lefts[column] < right)
            .collect();
        let band_top = covered.iter().map(|&column| skyline[column]).fold(top, f64::max) + above;
        let mut bottoms: BTreeMap<usize, f64> = BTreeMap::new();
        for id in &members {
            let column = column_of[id];
            let y = bottoms.get(&column).copied().unwrap_or(band_top);
            places.insert(*id, [lefts[column], y]);
            bottoms.insert(column, y + size(id)[1] + ROW_GAP);
        }
        let bottom = bottoms.values().copied().fold(band_top, f64::max) + below;
        for column in covered {
            skyline[column] = bottom;
        }
    }

    places
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use henad_graph::GraphFile;
    use henad_graph::catalogue::lookup;
    use henad_graph::value::InputValue;

    use henad_graph::edit::{EditCommand, GraphBuilder, Scope};
    use henad_graph::ids::NodeId;

    use super::{COLUMN_GAP, CanvasKey, NODE_SIZE, arrange};

    #[test]
    fn every_wire_of_a_starter_runs_rightwards_and_no_two_nodes_share_a_place() {
        let starters = henad_graph::starters::starters().iter();
        for starter in starters.chain(henad_graph::starters::agent_starters()) {
            let file = GraphFile::parse(starter.text).expect("the starter reads");
            for pass in &file.document().passes {
                let placed: BTreeMap<CanvasKey, [f64; 2]> = arrange(&pass.body, 1, [0.0, 0.0], &BTreeMap::new(), &[])
                    .into_iter()
                    .collect();
                let mains = placed.keys().filter(|key| !key.output_half).count();
                assert_eq!(mains, pass.body.nodes.len());
                for node in &pass.body.nodes {
                    for value in node.inputs.values() {
                        let InputValue::Link(link) = value else { continue };
                        // A link to an inner output leaves the input half, and any other link to a region its
                        // output half.
                        let source = pass.body.node(link.node).expect("the source");
                        let inner = lookup(&source.op, 1).is_some_and(|def| {
                            def.resolved_inner_outputs(source)
                                .iter()
                                .any(|output| Some(&output.name) == link.port.as_ref())
                        });
                        let output_half = CanvasKey::output(link.node);
                        let from = if !inner && placed.contains_key(&output_half) {
                            output_half
                        } else {
                            CanvasKey::main(link.node)
                        };
                        assert!(
                            placed[&from][0] + NODE_SIZE[0] + COLUMN_GAP <= placed[&CanvasKey::main(node.id)][0],
                            "{} reads {} from the left in {}",
                            node.id,
                            link.node,
                            starter.file_name
                        );
                    }
                }
                let mut places: Vec<[u64; 2]> = placed
                    .values()
                    .map(|pos| [pos[0].to_bits(), pos[1].to_bits()])
                    .collect();
                places.sort_unstable();
                places.dedup();
                assert_eq!(places.len(), placed.len(), "two nodes share a place in {}", pass.id);
            }
        }
    }

    #[test]
    fn a_region_body_sits_between_its_halves_and_its_readers_after_them() {
        let text = henad_graph::starters::starter("sir.toml").expect("a starter").text;
        let mut builder = GraphBuilder::from_file(&GraphFile::parse(text).expect("the starter reads"));
        let nodes = (12..=18).filter_map(NodeId::new).collect();
        let made = builder
            .apply(EditCommand::WrapInRepeat {
                scope: Scope::pass("step"),
                nodes,
                bounds: None,
            })
            .expect("the nodes are wrapped");
        let repeat = made[0];
        let body = &builder
            .document()
            .passes
            .iter()
            .find(|pass| pass.id == "step")
            .expect("the step pass")
            .body;
        let placed: BTreeMap<CanvasKey, [f64; 2]> = arrange(body, 1, [0.0, 0.0], &BTreeMap::new(), &[])
            .into_iter()
            .collect();
        assert_eq!(placed.len(), body.nodes.len() + 1, "the output half has a place");
        let input = placed[&CanvasKey::main(repeat)][0];
        let output = placed[&CanvasKey::output(repeat)][0];
        for member in body.region_contents(repeat) {
            let x = placed[&CanvasKey::main(member)][0];
            assert!(input < x && x < output, "{member} sits between the halves");
        }
        for reader in &body.nodes {
            let reads_output = reader.inputs.values().any(|value| {
                matches!(value, InputValue::Link(link) if link.node == repeat
                && !link.port.as_deref().is_some_and(|port| {
                    port == "k" || port == "total" || port.starts_with("prev.")
                }))
            });
            if reads_output && reader.region != Some(repeat) {
                assert!(
                    placed[&CanvasKey::main(reader.id)][0] > output,
                    "{} reads the output half",
                    reader.id
                );
            }
        }
    }

    #[test]
    fn the_input_half_of_a_region_sits_in_the_column_before_its_body() {
        let text = henad_graph::starters::starter("boids.toml").expect("a starter").text;
        let mut builder = GraphBuilder::from_file(&GraphFile::parse(text).expect("the starter reads"));
        let n37 = NodeId::new(37).expect("an id");
        let made = builder
            .apply(EditCommand::WrapInRepeat {
                scope: Scope::pass("flock"),
                nodes: vec![n37],
                bounds: None,
            })
            .expect("the node is wrapped");
        let body = &builder.document().passes[1].body;
        let placed: BTreeMap<CanvasKey, [f64; 2]> = arrange(body, 1, [0.0, 0.0], &BTreeMap::new(), &[])
            .into_iter()
            .collect();
        let input = placed[&CanvasKey::main(made[0])][0];
        let first = placed[&CanvasKey::main(n37)][0];
        assert!(input < first);
        assert!(
            !placed.values().any(|pos| input < pos[0] && pos[0] < first),
            "no column sits between the input half and the body, whose count reads no node"
        );
    }
}
