/// Hint about the spatial topology of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyHint {
    Grid2D,
    PointCloud,
    Network,
    NonSpatial,
}

/// Kind of neighborhood for grid-based models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborhoodKind {
    /// 8-cell neighborhood
    Moore,
    /// 4-cell neighborhood
    VonNeumann,
}
