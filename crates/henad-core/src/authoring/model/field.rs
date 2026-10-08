//! The grid slot an agent model sits over, and the [`Extent`] of the world.
//!
//! A field layer owns cells, updates them once per tick and draws them.

use crate::params::{ParamDescriptor, ParamValue};
use crate::view::GridView;

/// The world rectangle every display layer stretches to.
///
/// The whole model shares one extent. An agent layer and a field layer then cannot disagree about
/// how big the world is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    /// Width in world units.
    pub w: f32,
    /// Height in world units.
    pub h: f32,
}

impl Extent {
    /// Returns the cell dimensions of a field that tiles this extent at one cell per unit.
    pub fn cells(self) -> (u32, u32) {
        (self.w.max(1.0) as u32, self.h.max(1.0) as u32)
    }
}

/// A layer of cells stepped once per tick.
///
/// henad-compute implements it for `CaField`, a [`crate::authoring::model::grid_model::GridModel`] as a layer, and
/// for `ScalarField`, scatter-plus-decay layers. An agent model sits over either layer, or over [`NoField`].
pub trait FieldLayer: Send + 'static {
    /// Whether the layer draws a grid. It must agree with [`Self::grid_view`].
    const HAS_GRID: bool = true;

    /// Name of this layer in the Model panel.
    const KIND: &'static str;

    /// Hot parameters, rebuilt once per tick.
    type Params: Send + Sync;
    /// The field as an agent kernel sees it.
    type Read<'a>
    where
        Self: 'a;
    /// Per agent deposit lanes, filled by the agent passes, or `()` for a field without deposit lanes.
    type DepositLanes: Send + 'static;

    /// Returns this layer's own parameters, listed after the model's parameters.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Extracts the hot parameters for one tick.
    ///
    /// `params` is this layer's own slice, so its indices are 0 based and do not move when the
    /// model above it gains a parameter.
    fn from_params(params: &[ParamValue]) -> Self::Params;
    /// Creates the layer over `extent`, from this layer's own slice of the parameters.
    fn new(extent: Extent, params: &[ParamValue]) -> Self;

    /// Returns the field as an agent kernel reads it.
    fn read(&self) -> Self::Read<'_>;
    /// Allocates deposit lanes for `n` agents. The engine reuses them every tick.
    fn alloc_deposits(&self, n: usize) -> Self::DepositLanes;
    /// Advances the layer by one tick, with this tick's deposits.
    fn update(&mut self, deposits: &Self::DepositLanes, p: &Self::Params, tick: u64);

    /// Turns cells into palette indices. The engine calls it before each snapshot.
    fn prepare_view(&mut self) {}

    /// Returns the cells to draw, or `None` for a layer without a grid.
    fn grid_view(&self) -> Option<GridView<'_>>;
    /// Number of cells.
    fn cell_count(&self) -> usize;
    /// Heap memory held by the layer, in bytes.
    fn heap_bytes(&self) -> usize;
}

/// The empty grid slot, for a model that is agents only.
#[derive(Debug)]
pub struct NoField;

impl FieldLayer for NoField {
    const HAS_GRID: bool = false;
    const KIND: &'static str = "None";

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
