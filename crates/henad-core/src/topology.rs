//! Display layers a model presents, and the neighbourhood a grid rule reads.

/// Display layers a model presents.
///
/// The layers form a set, and a composite model presents more than one layer. They must match what the
/// state's `grid_view`, `point_view` and `edge_view` return, and the testing kit checks that they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopologyHint {
    /// Whether the model draws a grid layer.
    pub grid: bool,
    /// Whether the model draws agents or nodes.
    pub agents: bool,
    /// Whether the model draws edges between its nodes.
    pub edges: bool,
}

impl TopologyHint {
    /// A grid alone.
    pub const GRID: Self = Self {
        grid: true,
        agents: false,
        edges: false,
    };
    /// Agents alone.
    pub const AGENTS: Self = Self {
        grid: false,
        agents: true,
        edges: false,
    };
    /// Agents over a grid.
    pub const COMPOSITE: Self = Self {
        grid: true,
        agents: true,
        edges: false,
    };
    /// Nodes drawn as agents, with edges between them.
    pub const NETWORK: Self = Self {
        grid: false,
        agents: true,
        edges: true,
    };
    /// No layer.
    pub const NONE: Self = Self {
        grid: false,
        agents: false,
        edges: false,
    };
}

/// Neighbourhood a grid rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborhoodKind {
    /// The 8 surrounding cells.
    Moore,
    /// The 4 orthogonal neighbours.
    VonNeumann,
}
