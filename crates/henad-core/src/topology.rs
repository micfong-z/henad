/// Display layers a model presents. A set, not a choice, so a composite model can say so.
///
/// Must match what the state's `grid_view`, `point_view` and `edge_view` return. A registry test
/// checks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopologyHint {
    pub grid: bool,
    pub agents: bool,
    pub edges: bool,
}

impl TopologyHint {
    pub const GRID: Self = Self {
        grid: true,
        agents: false,
        edges: false,
    };
    pub const AGENTS: Self = Self {
        grid: false,
        agents: true,
        edges: false,
    };
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
    pub const NONE: Self = Self {
        grid: false,
        agents: false,
        edges: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborhoodKind {
    /// 8-cell neighborhood
    Moore,
    /// 4-cell neighborhood
    VonNeumann,
}
