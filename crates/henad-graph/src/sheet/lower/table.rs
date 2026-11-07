//! The line table: the owner and slot path of every node a lowering writes.

use std::collections::BTreeMap;
use std::fmt;

use crate::ids::NodeId;

/// Owner of a node of a lowered sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeOwner {
    /// A line of a card, by id, whose node ids lie from `n(1000k + 1)` to `n(1000k + 999)` for line `l<k>`.
    Line(String),
    /// The sheet itself: a start, a measure, a colour rule, a button step, or a node a card's lines share, such as the
    /// read of one of my properties. These nodes take the ids `n1` to `n999`.
    Sheet,
}

/// Owner and slot path of one node of a lowered sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NodeOrigin {
    /// Line or part of the sheet the node was lowered from.
    pub owner: NodeOwner,
    /// Slot path inside the owner, such as `condition.right.2` for a line, or the part of the sheet for a node the
    /// sheet owns, such as `cells.status.starts_as` or `measure.alive`.
    pub path: String,
}

/// Table from each node id of a lowered sheet to the line or part of the sheet it was lowered from.
///
/// The table is explicit. A node id says which line owns it only as long as nothing renumbers it, and normalisation
/// gives the copies of an expression or a call ids past the file's last.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LineMap {
    /// Origin of each node, by id.
    origins: BTreeMap<NodeId, NodeOrigin>,
    /// Node whose output is the result of each line that works out a value, by line id.
    results: BTreeMap<String, NodeId>,
}

/// Prints the number of nodes in place of their origins.
impl fmt::Debug for LineMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LineMap")
            .field("len", &self.origins.len())
            .finish_non_exhaustive()
    }
}

impl LineMap {
    /// Records `origin` for node `id`.
    pub(super) fn insert(&mut self, id: NodeId, origin: NodeOrigin) {
        self.origins.insert(id, origin);
    }

    /// Records node `id` as the node whose output is the result of line `line_id`.
    pub(super) fn insert_result(&mut self, line_id: String, id: NodeId) {
        self.results.insert(line_id, id);
    }

    /// Returns the node whose output is the result of line `line_id`, `None` for a line that works out no value or
    /// whose value lowers to no node, such as a constant.
    ///
    /// The result of a Look around line is its fold, whose outputs are the line's parts.
    pub fn result_node(&self, line_id: &str) -> Option<NodeId> {
        self.results.get(line_id).copied()
    }

    /// Returns the origin of node `id`, `None` for a node the lowering did not write.
    pub fn origin(&self, id: NodeId) -> Option<&NodeOrigin> {
        self.origins.get(&id)
    }

    /// Returns the id of the line that owns node `id`, `None` for a node the sheet owns or the lowering did not write.
    pub fn line_of(&self, id: NodeId) -> Option<&str> {
        match &self.origin(id)?.owner {
            NodeOwner::Line(line) => Some(line),
            NodeOwner::Sheet => None,
        }
    }

    /// Returns the ids of the nodes that line `line_id` owns, in id order.
    pub fn nodes_of<'a>(&'a self, line_id: &'a str) -> impl Iterator<Item = NodeId> + 'a {
        self.origins
            .iter()
            .filter(move |(_, origin)| matches!(&origin.owner, NodeOwner::Line(line) if line == line_id))
            .map(|(id, _)| *id)
    }

    /// Returns every node with its origin, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &NodeOrigin)> {
        self.origins.iter().map(|(id, origin)| (*id, origin))
    }

    /// Number of nodes in the table.
    pub fn len(&self) -> usize {
        self.origins.len()
    }

    /// Returns whether the table holds no node.
    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }
}
