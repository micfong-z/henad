/// Which display layers a model presents. A set, not a choice, so a composite model can say so.
///
/// Must match what the state's `grid_view` and `point_view` return. A registry test checks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopologyHint {
    pub grid: bool,
    pub agents: bool,
}

impl TopologyHint {
    pub const GRID: Self = Self {
        grid: true,
        agents: false,
    };
    pub const AGENTS: Self = Self {
        grid: false,
        agents: true,
    };
    pub const COMPOSITE: Self = Self {
        grid: true,
        agents: true,
    };
    pub const NONE: Self = Self {
        grid: false,
        agents: false,
    };
}

/// Kind of neighborhood for grid-based models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborhoodKind {
    /// 8-cell neighborhood
    Moore,
    /// 4-cell neighborhood
    VonNeumann,
}
