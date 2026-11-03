//! Nodes of a graph and the bodies that hold them, regions included.
//!
//! A *body* is the set of nodes of a pass or a function. A *region* is a node whose iteration runs other nodes of the
//! same body, such as `region.repeat`. Each of those nodes names its innermost region in [`Node::region`], and a nested
//! region names the region around it the same way. An accumulator is a value a region carries through its iterations.

use std::collections::BTreeMap;

use crate::catalogue::TypeClass;
use crate::ids::NodeId;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue};

/// Kind of the finite counting loop.
pub const OP_REGION_REPEAT: &str = "region.repeat";
/// Kind of the body node that feeds an accumulator of a region.
pub const OP_REGION_PUT: &str = "region.put";
/// Kind of the call of an embedded function, whose interface ports are all bare.
pub const OP_FUNCTION_CALL: &str = "fn.call";
/// Kind of a function's input inside its body.
pub const OP_FUNCTION_INPUT: &str = "fn.input";
/// Kind of a function's output inside its body.
pub const OP_FUNCTION_OUTPUT: &str = "fn.output";
/// Kind of the expression node, whose input ports are all bare.
pub const OP_EXPRESSION: &str = "math.expr";

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
    /// Returns the accumulator `name` of type `value_type`, merged by `combine`.
    pub fn new(name: impl Into<String>, value_type: ValueType, combine: Combine) -> Self {
        Self {
            name: name.into(),
            value_type,
            combine,
        }
    }

    /// Returns the element of a `config.acc` list that declares the accumulator.
    pub fn to_config(&self) -> ConfigValue {
        ConfigValue::Table(BTreeMap::from([
            ("name".to_owned(), ConfigValue::String(self.name.clone())),
            ("type".to_owned(), ConfigValue::String(self.value_type.to_string())),
            (
                "combine".to_owned(),
                ConfigValue::String(self.combine.as_str().to_owned()),
            ),
        ]))
    }

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
    /// Every rule, in the order the catalogue lists them.
    pub const ALL: [Self; 7] = [
        Self::Sum,
        Self::Count,
        Self::Min,
        Self::Max,
        Self::Or,
        Self::And,
        Self::Carry,
    ];

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

    /// Returns whether the rule applies to values of `value_type`: `sum`, `min` and `max` to numbers and vectors,
    /// `count` to `u32`, `or` and `and` to `bool`, and `carry` to every type.
    pub fn applies_to(self, value_type: &ValueType) -> bool {
        const NUMERIC: [TypeClass; 8] = [
            TypeClass::U32,
            TypeClass::I32,
            TypeClass::F32,
            TypeClass::Vec2U,
            TypeClass::Vec2I,
            TypeClass::Vec2F,
            TypeClass::U64,
            TypeClass::F64,
        ];
        match self {
            Self::Sum | Self::Min | Self::Max => NUMERIC.iter().any(|class| class.holds(value_type)),
            Self::Count => *value_type == ValueType::U32,
            Self::Or | Self::And => *value_type == ValueType::Bool,
            Self::Carry => true,
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

    /// Returns every node whose region path holds `region`, members of nested regions included, in body order.
    ///
    /// A cycle of regions in a broken file ends the walk at a node it has already reached.
    pub fn region_contents(&self, region: NodeId) -> Vec<NodeId> {
        let mut reached = std::collections::BTreeSet::from([region]);
        let mut stack = vec![region];
        while let Some(current) = stack.pop() {
            for member in self.region_members(current) {
                if reached.insert(member.id) {
                    stack.push(member.id);
                }
            }
        }
        reached.remove(&region);
        let mut contents: Vec<NodeId> = Vec::with_capacity(reached.len());
        for node in &self.nodes {
            if reached.remove(&node.id) {
                contents.push(node.id);
            }
        }
        contents
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
