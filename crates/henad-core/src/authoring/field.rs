//! The grid slot. Whatever owns cells, updates them once per tick, and draws them.

use crate::params::{ParamDescriptor, ParamValue};
use crate::view::GridView;

/// The world rectangle every display layer stretches to.
///
/// One extent for the whole model, so an agent layer and a field layer cannot disagree about how
/// big the world is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub w: f32,
    pub h: f32,
}

impl Extent {
    /// Cell dimensions of a field tiling this extent at one cell per unit.
    pub fn cells(self) -> (u32, u32) {
        (self.w.max(1.0) as u32, self.h.max(1.0) as u32)
    }
}

/// A layer of cells stepped once per tick.
///
/// Implemented by `CaField` (a [`crate::authoring::grid_model::GridModel`] gather rule) and by scatter-plus-decay
/// fields, so an agent model can sit over either.
pub trait FieldLayer: Send + 'static {
    /// Must agree with what `grid_view` returns.
    const HAS_GRID: bool = true;

    /// Hot parameters, rebuilt once per tick.
    type Params: Send + Sync;
    /// The field as an agent kernel sees it.
    type Read<'a>
    where
        Self: 'a;
    /// Per agent deposit lanes the agent passes fill, `()` for a field that takes none.
    type DepositLanes: Send + 'static;

    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// `params` is this layer's own slice, so its indices are 0 based and do not move when the
    /// model above it gains a parameter.
    fn from_params(params: &[ParamValue]) -> Self::Params;
    fn new(extent: Extent, params: &[ParamValue]) -> Self;

    fn read(&self) -> Self::Read<'_>;
    /// Lanes sized for `n` agents, reused every tick rather than reallocated.
    fn alloc_deposits(&self, n: usize) -> Self::DepositLanes;
    fn update(&mut self, deposits: &Self::DepositLanes, p: &Self::Params, tick: u64);

    /// Turns cells into palette indices. Called before a snapshot rather than every tick.
    fn prepare_view(&mut self) {}

    fn grid_view(&self) -> Option<GridView<'_>>;
    fn cell_count(&self) -> usize;
    fn heap_bytes(&self) -> usize;
}

/// The empty grid slot, for a model that is agents only.
pub struct NoField;

impl FieldLayer for NoField {
    const HAS_GRID: bool = false;

    type Params = ();
    type Read<'a> = ();
    type DepositLanes = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        Vec::new()
    }

    fn from_params(_params: &[ParamValue]) {}

    fn new(_extent: Extent, _params: &[ParamValue]) -> Self {
        Self
    }

    fn read(&self) {}

    fn alloc_deposits(&self, _n: usize) {}

    fn update(&mut self, _deposits: &(), _p: &(), _tick: u64) {}

    fn grid_view(&self) -> Option<GridView<'_>> {
        None
    }

    fn cell_count(&self) -> usize {
        0
    }

    fn heap_bytes(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_rounds_down_to_whole_cells() {
        assert_eq!(Extent { w: 200.0, h: 200.0 }.cells(), (200, 200));
        assert_eq!(Extent { w: 10.9, h: 4.2 }.cells(), (10, 4));
    }

    /// A zero extent would give a field with no cells and divide-by-zero indexing.
    #[test]
    fn extent_never_collapses_below_one_cell() {
        assert_eq!(Extent { w: 0.0, h: 0.0 }.cells(), (1, 1));
        assert_eq!(Extent { w: -5.0, h: 0.4 }.cells(), (1, 1));
    }
}
