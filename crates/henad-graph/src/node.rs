//! Nodes of a graph and the bodies that hold them, regions included.
//!
//! A *body* is the set of nodes of a pass or a function. A *region* is a node whose iteration runs other nodes of the
//! same body, such as `region.repeat`. Each of those nodes names its innermost region in [`Node::region`], and a nested
//! region names the region around it the same way. An accumulator is a value a region carries through its iterations.

use std::collections::BTreeMap;

use crate::ids::NodeId;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue};

/// Kind of the finite counting loop.
pub const OP_REGION_REPEAT: &str = "region.repeat";
/// Kind of the body node that feeds an accumulator of a region.
pub const OP_REGION_PUT: &str = "region.put";
/// Kind of the call of an embedded function, whose interface ports are all bare.
pub const OP_FUNCTION_CALL: &str = "fn.call";

/// One node of a pass or a function.
///
/// A node holds no label, position or colour. Those sit in the [`EditorRegion`](crate::editor::EditorRegion).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Node {
    /// Stable id, unique in the file across passes and functions.
    pub id: NodeId,
    /// Kind of the node, resolved at the file's library version.
    pub op: String,
    /// Constant attributes by key.
    pub config: BTreeMap<String, ConfigValue>,
    /// Input values by port name. A port made by configuration has a dotted name, such as `init.position`.
    pub inputs: BTreeMap<String, InputValue>,
    /// Innermost region the node belongs to, `None` for a node outside every region.
    pub region: Option<NodeId>,
}

impl Node {
    /// Returns a node of kind `op` with no configuration, no inputs and no region.
    pub fn new(id: NodeId, op: impl Into<String>) -> Self {
        Self {
            id,
            op: op.into(),
            config: BTreeMap::new(),
            inputs: BTreeMap::new(),
            region: None,
        }
    }

    /// Returns the accumulators the node's `config.acc` list declares, in list order.
    ///
    /// A node with no `config.acc` key declares none.
    ///
    /// # Errors
    ///
    /// Returns the message for a value that is not a list of `{ name, type, combine }` tables.
    pub fn accumulators(&self) -> Result<Vec<Accumulator>, String> {
        let Some(list) = self.config.get("acc") else {
            return Ok(Vec::new());
        };
        let items = list
            .as_array()
            .ok_or_else(|| format!("`config.acc` of node {} is no list of accumulators", self.id))?;
        items
            .iter()
            .map(|item| Accumulator::from_config(item).map_err(|reason| format!("node {}: {reason}", self.id)))
            .collect()
    }
}

/// A value a region carries through its iterations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Accumulator {
    /// Name of the accumulator, which names the region's output and its `init.<name>` input.
    pub name: String,
    /// Type of the value.
    pub value_type: ValueType,
    /// Rule that merges each `region.put` into the value.
    pub combine: Combine,
}

impl Accumulator {
    /// Returns the accumulator one element of a `config.acc` list declares.
    fn from_config(item: &ConfigValue) -> Result<Self, String> {
        let table = item
            .as_table()
            .ok_or_else(|| "an accumulator is a `{ name, type, combine }` table".to_owned())?;
        let text = |key: &str| {
            table
                .get(key)
                .and_then(ConfigValue::as_str)
                .ok_or_else(|| format!("an accumulator needs the string `{key}`"))
        };
        Ok(Self {
            name: text("name")?.to_owned(),
            value_type: text("type")?.parse()?,
            combine: Combine::from_name(text("combine")?)?,
        })
    }
}

/// Rule that merges a value into an accumulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Combine {
    /// Adds the value.
    Sum,
    /// Adds one for each put.
    Count,
    /// Keeps the smaller value.
    Min,
    /// Keeps the larger value.
    Max,
    /// Takes the logical or.
    Or,
    /// Takes the logical and.
    And,
    /// Replaces the value, making the accumulator a loop variable.
    Carry,
}

impl Combine {
    /// Returns the rule named `name` in a file.
    ///
    /// # Errors
    ///
    /// Returns the message for a name that is none of `sum`, `count`, `min`, `max`, `or`, `and` and `carry`.
    pub fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "sum" => Ok(Self::Sum),
            "count" => Ok(Self::Count),
            "min" => Ok(Self::Min),
            "max" => Ok(Self::Max),
            "or" => Ok(Self::Or),
            "and" => Ok(Self::And),
            "carry" => Ok(Self::Carry),
            _ => Err(format!("'{name}' is no combine rule")),
        }
    }

    /// Returns the name a file writes for the rule.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Count => "count",
            Self::Min => "min",
            Self::Max => "max",
            Self::Or => "or",
            Self::And => "and",
            Self::Carry => "carry",
        }
    }
}

/// Nodes of a pass or a function.
///
/// A duplicate id stays in the list, as two branches of one file can mint one id. Lookups return the first node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeBody {
    /// Nodes in natural id order after a read. The canonical writer sorts them again.
    pub nodes: Vec<Node>,
}

impl NodeBody {
    /// Returns the node with id `id`.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Returns the nodes whose innermost region is `region`.
    pub fn region_members(&self, region: NodeId) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |node| node.region == Some(region))
    }

    /// Returns the regions around node `id`, innermost first.
    ///
    /// The path ends at a region that names no region, at an id the body does not hold, and before a region it has
    /// already passed. A broken file therefore returns a finite path.
    pub fn region_path(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = self.node(id).and_then(|node| node.region);
        while let Some(region) = current {
            if region == id || path.contains(&region) {
                break;
            }
            path.push(region);
            current = self.node(region).and_then(|node| node.region);
        }
        path
    }

    /// Returns the ids that more than one node holds, each once, in natural order.
    pub fn duplicate_ids(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.nodes.iter().map(|node| node.id).collect();
        ids.sort_unstable();
        let mut duplicates: Vec<NodeId> = ids
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| pair[0])
            .collect();
        duplicates.dedup();
        duplicates
    }
}
