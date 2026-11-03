//! Layered left-to-right layout of a body, behind Arrange and the opening of a file without placement.
//!
//! A node's *layer* is the length of the longest chain of wires that reaches it from a node that reads no other node
//! of the body. Layers become columns from left to right, so every wire of an acyclic body runs rightwards. Within a
//! column a node sits at the mean row of the nodes it reads, ties broken by id. A column is as wide as its widest
//! node, and the nodes of a column stack with a gap between them.

use std::collections::{BTreeMap, BTreeSet};

use henad_graph::ids::NodeId;
use henad_graph::node::NodeBody;
use henad_graph::value::InputValue;

/// Size in canvas points assumed for a node whose size `arrange` is not given.
pub const NODE_SIZE: [f64; 2] = [220.0, 120.0];

/// Gap in canvas points between two columns.
pub const COLUMN_GAP: f64 = 60.0;

/// Gap in canvas points between two nodes of one column.
pub const ROW_GAP: f64 = 30.0;

/// Returns a position for every node of `body`, in the body's order, with the first column's top at `origin`.
///
/// `sizes` holds the drawn size of each node it knows, and a node it lacks takes [`NODE_SIZE`]. A node on a cycle,
/// which a broken file can hold, goes to a column after every other node.
pub fn arrange(body: &NodeBody, origin: [f64; 2], sizes: &BTreeMap<NodeId, [f64; 2]>) -> Vec<(NodeId, [f64; 2])> {
    let size = |id: &NodeId| sizes.get(id).copied().unwrap_or(NODE_SIZE);
    let mut ids: Vec<NodeId> = Vec::with_capacity(body.nodes.len());
    for node in &body.nodes {
        if !ids.contains(&node.id) {
            ids.push(node.id);
        }
    }
    let known: BTreeSet<NodeId> = ids.iter().copied().collect();
    let mut readers: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    let mut sources: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for node in &body.nodes {
        for value in node.inputs.values() {
            if let InputValue::Link(link) = value
                && known.contains(&link.node)
                && link.node != node.id
            {
                readers.entry(link.node).or_default().insert(node.id);
                sources.entry(node.id).or_default().insert(link.node);
            }
        }
    }

    // Kahn's algorithm over the wires, in id order, gives each node the longest chain that reaches it.
    let mut waiting: BTreeMap<NodeId, usize> = ids
        .iter()
        .map(|id| (*id, sources.get(id).map_or(0, BTreeSet::len)))
        .collect();
    let mut ready: BTreeSet<NodeId> = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut layers: BTreeMap<NodeId, usize> = BTreeMap::new();
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

    let mut columns: BTreeMap<usize, Vec<NodeId>> = BTreeMap::new();
    for id in &ids {
        columns.entry(layers[id]).or_default().push(*id);
    }
    let mut rows: BTreeMap<NodeId, f64> = BTreeMap::new();
    let mut places: BTreeMap<NodeId, [f64; 2]> = BTreeMap::new();
    let mut left = origin[0];
    for column in columns.values_mut() {
        let barycenter = |id: &NodeId| {
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
        let mut keyed: Vec<(f64, NodeId)> = column.iter().map(|id| (barycenter(id), *id)).collect();
        keyed.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
        let mut top = origin[1];
        let mut width: f64 = 0.0;
        for (row, (_, id)) in keyed.iter().enumerate() {
            rows.insert(*id, row as f64);
            places.insert(*id, [left, top]);
            let [node_width, node_height] = size(id);
            top += node_height + ROW_GAP;
            width = width.max(node_width);
        }
        left += width + COLUMN_GAP;
    }

    ids.iter().map(|id| (*id, places[id])).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use henad_graph::GraphFile;
    use henad_graph::value::InputValue;

    use super::{COLUMN_GAP, NODE_SIZE, arrange};

    #[test]
    fn every_wire_of_a_starter_runs_rightwards_and_no_two_nodes_share_a_place() {
        for starter in henad_graph::starters::starters() {
            let file = GraphFile::parse(starter.text).expect("the starter reads");
            for pass in &file.document().passes {
                let placed: BTreeMap<_, _> = arrange(&pass.body, [0.0, 0.0], &BTreeMap::new()).into_iter().collect();
                assert_eq!(placed.len(), pass.body.nodes.len());
                for node in &pass.body.nodes {
                    for value in node.inputs.values() {
                        if let InputValue::Link(link) = value {
                            assert!(
                                placed[&link.node][0] + NODE_SIZE[0] + COLUMN_GAP <= placed[&node.id][0],
                                "{} reads {} from the left in {}",
                                node.id,
                                link.node,
                                starter.file_name
                            );
                        }
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
}
