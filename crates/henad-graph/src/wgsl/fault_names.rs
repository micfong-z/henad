//! Names of the nodes whose faults a run reports, in the words of the document a graph was written from.

use std::collections::BTreeMap;
use std::fmt;

use crate::ids::NodeId;

/// Table from the id of a node that can fault to the words that name it in the document the graph was lowered from,
/// such as "line 3, Flockmates" for a radius fold a sheet lowered.
///
/// [`AgentProgram::compile_with_fault_names`](super::AgentProgram::compile_with_fault_names) takes it beside the graph.
/// A fault of a named node names the words in place of the pass, the node and the line of the graph's text. A node the
/// table does not hold keeps the graph's own message.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct FaultNames {
    /// Words of each node, by id.
    names: BTreeMap<NodeId, String>,
}

/// Prints the number of names in place of the names.
impl fmt::Debug for FaultNames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaultNames")
            .field("len", &self.names.len())
            .finish_non_exhaustive()
    }
}

impl FaultNames {
    /// Records `name` as the words of node `node`, in place of any words it had.
    pub fn insert(&mut self, node: NodeId, name: impl Into<String>) {
        self.names.insert(node, name.into());
    }

    /// Returns the words of node `node`, `None` for a node the table does not hold.
    pub fn get(&self, node: NodeId) -> Option<&str> {
        self.names.get(&node).map(String::as_str)
    }

    /// Returns every node with its words, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &str)> {
        self.names.iter().map(|(node, name)| (*node, name.as_str()))
    }

    /// Number of names in the table.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns whether the table holds no name.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}
