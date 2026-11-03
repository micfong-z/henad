//! Semantic document of a graph file: every table before `[editor]`.
//!
//! Declaration order is meaningful. Parameter and statistic order are schema order, variant order is a category's
//! numbering, and pass order within a stage is execution order. Inferred types and rates are never part of the
//! document, only the declared ones.

use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::ParamDescriptor;

use crate::ids::NodeId;
use crate::node::NodeBody;
use crate::types::ValueType;
use crate::value::{Color, Literal, PortRef};

/// Every semantic table of a graph file.
///
/// The type implements no `PartialEq`. Two documents are equal when their [canonical text](crate::GraphFile::to_toml)
/// is equal.
#[derive(Clone)]
#[non_exhaustive]
pub struct GraphDocument {
    /// Model id of the header, the default of the target's id.
    pub id: String,
    /// Display name of the header.
    pub name: String,
    /// Description of the header.
    pub description: String,
    /// Node library version the file is written at.
    pub library: u32,
    /// The file's one compile target.
    pub target: Target,
    /// World table, which also selects the topology.
    pub world: World,
    /// `[[category]]` tables, in file order.
    pub categories: Vec<Category>,
    /// `[[palette]]` tables, in file order.
    pub palettes: Vec<Palette>,
    /// `[[lane]]` tables, in file order.
    pub lanes: Vec<Lane>,
    /// `[[field]]` tables, in file order.
    pub fields: Vec<Field>,
    /// `[[counter]]` tables, in file order.
    pub counters: Vec<Counter>,
    /// `[[param]]` tables, in schema order.
    pub params: Vec<ParamDescriptor>,
    /// `[[stat]]` tables, in schema order.
    pub stats: Vec<Stat>,
    /// `[[action]]` tables, in file order.
    pub actions: Vec<Action>,
    /// `[[function]]` tables, in file order.
    pub functions: Vec<Function>,
    /// `[[pass]]` tables in stack order: by stage, then in file order within a stage.
    pub passes: Vec<Pass>,
}

/// Prints the model id, the library and the number of each declaration and of the passes' nodes.
impl std::fmt::Debug for GraphDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nodes: usize = self.passes.iter().map(|pass| pass.body.nodes.len()).sum();
        f.debug_struct("GraphDocument")
            .field("id", &self.id)
            .field("library", &self.library)
            .field("world", &self.world)
            .field("lanes", &self.lanes.len())
            .field("params", &self.params.len())
            .field("stats", &self.stats.len())
            .field("actions", &self.actions.len())
            .field("functions", &self.functions.len())
            .field("passes", &self.passes.len())
            .field("nodes", &nodes)
            .finish_non_exhaustive()
    }
}

impl GraphDocument {
    /// Returns an empty document with the model id `id` over `world`, at the newest node library this build reads.
    ///
    /// The name and the target take the id, and the document holds no declaration and no pass.
    pub fn new(id: &str, world: World) -> Self {
        Self {
            id: id.to_owned(),
            name: id.to_owned(),
            description: String::new(),
            library: crate::file::LIBRARY,
            target: Target {
                id: id.to_owned(),
                name: id.to_owned(),
                description: String::new(),
            },
            world,
            categories: Vec::new(),
            palettes: Vec::new(),
            lanes: Vec::new(),
            fields: Vec::new(),
            counters: Vec::new(),
            params: Vec::new(),
            stats: Vec::new(),
            actions: Vec::new(),
            functions: Vec::new(),
            passes: Vec::new(),
        }
    }
}

/// The `[[target]]` table, always on the GPU backend.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Target {
    /// Entry id.
    pub id: String,
    /// Entry name.
    pub name: String,
    /// Entry description.
    pub description: String,
}

/// Engine family of a graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Topology {
    /// A grid of cells.
    Grid,
    /// A population of agents in a continuous world.
    Agents,
}

impl Topology {
    /// Returns the name a file writes for the topology.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Agents => "agents",
        }
    }
}

/// World table of a graph.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum World {
    /// The `[grid]` table.
    Grid(GridWorld),
    /// The `[agents]` table.
    Agents(AgentsWorld),
}

impl World {
    /// Returns the topology the world belongs to.
    pub fn topology(&self) -> Topology {
        match self {
            Self::Grid(_) => Topology::Grid,
            Self::Agents(_) => Topology::Agents,
        }
    }
}

/// The `[grid]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GridWorld {
    /// Default of the prepended `grid_width` parameter, in cells.
    pub width: u32,
    /// Default of the prepended `grid_height` parameter, in cells.
    pub height: u32,
    /// What a stencil sees past the edge.
    pub boundary: Boundary,
}

impl Default for GridWorld {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            boundary: Boundary::Torus,
        }
    }
}

/// The `[agents]` table.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct AgentsWorld {
    /// Default of the prepended `num_agents` parameter.
    pub count: u32,
    /// Upper bound of the prepended `num_agents` parameter.
    pub max_count: u32,
    /// Default of the prepended `world_width` parameter, in world units.
    pub width: f32,
    /// Default of the prepended `world_height` parameter, in world units.
    pub height: f32,
}

impl Default for AgentsWorld {
    fn default() -> Self {
        Self {
            count: 1000,
            max_count: 10_000_000,
            width: 1000.0,
            height: 1000.0,
        }
    }
}

/// A `[[category]]` table: a named set of variants, numbered in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Category {
    /// Category id.
    pub id: String,
    /// Variants in numbering order.
    pub variants: Vec<Variant>,
}

impl Category {
    /// Returns the category `id` of `variants`, numbered in order.
    pub fn new(id: &str, variants: Vec<Variant>) -> Self {
        Self {
            id: id.to_owned(),
            variants,
        }
    }
}

/// One variant of a [`Category`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Variant {
    /// Name of the variant, which also names `case.<name>` ports.
    pub name: String,
    /// Display label.
    pub label: String,
    /// Colour of the variant in the category's palette.
    pub color: Color,
}

impl Variant {
    /// Returns the variant `name` in `color`, labelled with its name.
    pub fn new(name: &str, color: Color) -> Self {
        Self {
            name: name.to_owned(),
            label: name.to_owned(),
            color,
        }
    }
}

/// A `[[palette]]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Palette {
    /// Palette id, distinct from every category id.
    pub id: String,
    /// Colours in index order.
    pub colors: Vec<Color>,
}

impl Palette {
    /// Returns the palette `id` of `colors`, in index order.
    pub fn new(id: &str, colors: Vec<Color>) -> Self {
        Self {
            id: id.to_owned(),
            colors,
        }
    }
}

/// Elements a lane belongs to or a pass runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Domain {
    /// The cells of a grid.
    Cells,
    /// The agents of a population.
    Agents,
}

impl Domain {
    /// Returns the name a file writes for the domain.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cells => "cells",
            Self::Agents => "agents",
        }
    }
}

/// Storage width of a `u32` or category lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LaneStorage {
    /// One byte per element.
    U8,
    /// Two bytes per element.
    U16,
    /// Four bytes per element.
    U32,
}

impl LaneStorage {
    /// Returns the name a file writes for the storage.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
        }
    }
}

/// Special meaning of a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LaneRole {
    /// The `vec2f` lane the spatial index and the point view read.
    Position,
}

impl LaneRole {
    /// Returns the name a file writes for the role.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Position => "position",
        }
    }
}

/// A `[[lane]]` table: one value per element of a domain.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Lane {
    /// Lane id.
    pub id: String,
    /// Domain that owns the lane.
    pub domain: Domain,
    /// Compute type of the lane.
    pub value_type: ValueType,
    /// Storage width, `Some` on a `u32` or category lane alone.
    pub storage: Option<LaneStorage>,
    /// Value at allocation. `None` when the type has no literal zero or names a category the file does not declare.
    pub initial: Option<Literal>,
    /// Special meaning of the lane.
    pub role: Option<LaneRole>,
}

impl Lane {
    /// Returns the lane `id` of `value_type` over `domain`, with no storage, no role, and the type's zero as its
    /// initial value.
    pub fn new(id: &str, domain: Domain, value_type: ValueType) -> Self {
        Self {
            id: id.to_owned(),
            domain,
            initial: Literal::zero_of(&value_type),
            value_type,
            storage: None,
            role: None,
        }
    }
}

/// Rule that merges deposits into a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldCombine {
    /// Keeps the largest deposit.
    Max,
}

impl FieldCombine {
    /// Returns the name a file writes for the rule.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Max => "max",
        }
    }
}

/// A `[[field]]` table: layers of values that tile the world, on the agents topology.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Field {
    /// Field id.
    pub id: String,
    /// Value type of every layer.
    pub value_type: ValueType,
    /// Layer names.
    pub layers: Vec<String>,
    /// Whether the field is written only in init and action passes and never deposited into.
    pub is_static: bool,
    /// Rule that merges deposits. Absent on a static field.
    pub combine: Option<FieldCombine>,
    /// Id of the function run per cell after the merge.
    pub finish: Option<String>,
    /// Value of every cell at allocation. `None` when the type has no literal zero or names a category the file does
    /// not declare.
    pub initial: Option<Literal>,
}

/// A `[[counter]]` table: a run-long tally.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Counter {
    /// Counter id.
    pub id: String,
}

/// A `[[stat]]` table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Stat {
    /// Column key, part of the schema hash.
    pub label: String,
    /// Tick-rate output of a stats pass node that gives the value.
    pub from: PortRef,
    /// Series colour.
    pub color: Color,
}

impl Stat {
    /// Returns the statistic `label` of the output `from`, drawn in `color`.
    pub fn new(label: &str, from: PortRef, color: Color) -> Self {
        Self {
            label: label.to_owned(),
            from,
            color,
        }
    }
}

/// An `[[action]]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Action {
    /// Action id, part of the schema hash.
    pub id: String,
    /// Button label.
    pub label: String,
    /// Ids of the init or tick passes rerun first, in list order.
    pub runs: Vec<String>,
}

impl Action {
    /// Returns the action `id` with the button label `label`, which reruns no pass.
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            runs: Vec::new(),
        }
    }
}

/// One input or output of a [`Function`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FunctionPort {
    /// Port name.
    pub port: String,
    /// Type of the port.
    pub value_type: ValueType,
}

/// A `[[function]]` table: a pure subgraph embedded in the file.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Function {
    /// Function id.
    pub id: String,
    /// Inputs in order.
    pub inputs: Vec<FunctionPort>,
    /// Outputs in order.
    pub outputs: Vec<FunctionPort>,
    /// Body, which uses `fn.input` and `fn.output` nodes.
    pub body: NodeBody,
}

/// Stage of a pass, ordered as the pass stack runs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PassStage {
    /// Runs once when the model is built, on the host.
    Init,
    /// Runs every tick.
    Tick,
    /// Runs to colour the view.
    View,
    /// Runs when statistics are sampled.
    Stats,
    /// Runs when its action is pressed.
    Action,
}

impl PassStage {
    /// Returns the name a file writes for the stage.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Tick => "tick",
            Self::View => "view",
            Self::Stats => "stats",
            Self::Action => "action",
        }
    }
}

/// A pass's `enabled` key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PassEnabled {
    /// A constant.
    Literal(bool),
    /// A binding to the `bool` parameter with this id.
    Param(String),
}

/// A `[[pass]]` table.
///
/// Every pass is a map pass: one graph per element of its domain.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Pass {
    /// Pass id.
    pub id: String,
    /// Stage the pass belongs to.
    pub stage: PassStage,
    /// Action the pass belongs to, in the action stage.
    pub action: Option<String>,
    /// Elements the pass runs over, `None` for a stats pass of domain-free reducers.
    pub domain: Option<Domain>,
    /// Whether the pass runs.
    pub enabled: PassEnabled,
    /// Every `stream.*` node of the pass once, in the order each element draws them.
    pub draw_order: Vec<NodeId>,
    /// Nodes of the pass.
    pub body: NodeBody,
}

impl Pass {
    /// Returns the empty pass `id` of `stage` over `domain`, always enabled, in no action.
    pub fn new(id: &str, stage: PassStage, domain: Option<Domain>) -> Self {
        Self {
            id: id.to_owned(),
            stage,
            action: None,
            domain,
            enabled: PassEnabled::Literal(true),
            draw_order: Vec::new(),
            body: NodeBody::default(),
        }
    }
}
