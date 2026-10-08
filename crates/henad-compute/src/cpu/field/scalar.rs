//! Scalar `f32` fields written by agent deposits and decayed each tick.

use std::marker::PhantomData;

use henad_core::authoring::model::field::{Extent, FieldLayer};
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::GridView;

use crate::cpu::primitives::chunked::STATS_CHUNK;
use crate::cpu::primitives::scatter::{Combine, ScatterGrid};
use crate::for_each_chunk_mut;

/// Rules of a scalar field that only the model can supply.
pub trait ScalarFieldSpec: Send + Sync + 'static {
    /// Number of `f32` fields, all sharing the grid and the scatter scratch.
    const FIELDS: usize;
    /// Rule that combines deposits landing in one cell.
    const COMBINE: Combine;
    /// Colours for the quantised display layer.
    const PALETTE: &'static [[u8; 4]];

    /// Hot params, read once per tick and passed to [`Self::decay`].
    type Params: Send + Sync;

    /// Returns the field's own parameter descriptors, listed after the model's own descriptors.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Reads [`Self::Params`] from the field's slice of the parameter values.
    fn from_params(params: &[ParamValue]) -> Self::Params;

    /// Writes the static terrain into `sites`, one marker per cell, once at construction.
    fn build_sites(width: u32, height: u32, sites: &mut [u8]);

    /// Returns one cell's value a tick later. The scatter applies it after the merge.
    fn decay(v: f32, p: &Self::Params) -> f32;

    /// Writes one cell's palette index into `out`, from its terrain marker and every field's current value.
    fn quantize(site: u8, values: &[f32], out: &mut u8);
}

/// Deposit lanes, with one cell per agent and one value per agent and field.
pub struct Deposits {
    /// Cell each agent deposits into.
    pub cell: Vec<u32>,
    /// `values[f][i]` is agent `i`'s deposit into field `f`. An agent that writes one field leaves
    /// the others at the combine's identity, so every lane stays dense.
    pub values: Vec<Vec<f32>>,
}

/// Prints the agent and field counts, not the deposits.
impl std::fmt::Debug for Deposits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deposits")
            .field("len", &self.cell.len())
            .field("fields", &self.values.len())
            .finish_non_exhaustive()
    }
}

impl Deposits {
    /// Heap memory held by the lanes, in bytes.
    pub fn heap_bytes(&self) -> usize {
        self.cell.capacity() * size_of::<u32>()
            + self
                .values
                .iter()
                .map(|v| v.capacity() * size_of::<f32>())
                .sum::<usize>()
    }
}

/// `S::FIELDS` double buffered `f32` grids over one shared scatter scratch.
pub struct ScalarField<S: ScalarFieldSpec> {
    fields: Vec<Grid2D<f32>>,
    /// Scratch every field shares. The fields have one size and one combine, and scatter one after another.
    scatter: ScatterGrid,
    sites: Vec<u8>,
    /// Palette index of each cell, written by `prepare_view`. `GridView::cells` is `&[u8]` and a field is `f32`.
    display_cells: Vec<u8>,
    width: u32,
    height: u32,
    _marker: PhantomData<S>,
}

/// Prints the grid's size and the field count, not the values.
impl<S: ScalarFieldSpec> std::fmt::Debug for ScalarField<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScalarField")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("fields", &self.fields.len())
            .field("scatter", &self.scatter)
            .finish_non_exhaustive()
    }
}

impl<S: ScalarFieldSpec> ScalarField<S> {
    /// Grid of field `i`. Panics if `i` is out of range.
    pub fn field(&self, i: usize) -> &Grid2D<f32> {
        &self.fields[i]
    }

    /// Write access, for an action that rewrites a field.
    pub fn field_mut(&mut self, i: usize) -> &mut Grid2D<f32> {
        &mut self.fields[i]
    }

    /// Terrain marker of each cell, from [`ScalarFieldSpec::build_sites`].
    pub fn sites(&self) -> &[u8] {
        &self.sites
    }

    /// Width of the grid in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Palette index of each cell, as of the last `prepare_view`.
    pub fn display_cells(&self) -> &[u8] {
        &self.display_cells
    }

    /// Overwrites field `i`'s current values, for reproducing a particular run, and returns whether `i` is a valid
    /// field index and `values` matches its length.
    ///
    /// The counterpart of [`crate::cpu::grid_engine::GridModelState::from_cells`] for a field
    /// layer. Deposits and decay make an evolved field impractical to reach by stepping, and a
    /// hand-written one pins the advection rule across engines.
    pub fn seed_field(&mut self, i: usize, values: &[f32]) -> bool {
        let Some(grid) = self.fields.get_mut(i) else {
            return false;
        };
        let current = grid.current_mut();
        if current.len() != values.len() {
            return false;
        }
        current.copy_from_slice(values);
        true
    }

    /// Every field's current side, for an agent kernel to read.
    pub fn current(&self) -> ScalarRead<'_> {
        ScalarRead {
            fields: &self.fields,
            sites: &self.sites,
            width: self.width,
            height: self.height,
        }
    }
}

/// The field as an agent kernel sees it.
#[derive(Clone, Copy)]
pub struct ScalarRead<'a> {
    fields: &'a Vec<Grid2D<f32>>,
    /// Terrain marker of each cell.
    pub sites: &'a [u8],
    /// Width of the grid in cells.
    pub width: u32,
    /// Height of the grid in cells.
    pub height: u32,
}

/// Prints the grid's size and the field count, not the values.
impl std::fmt::Debug for ScalarRead<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScalarRead")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("fields", &self.fields.len())
            .finish_non_exhaustive()
    }
}

impl<'a> ScalarRead<'a> {
    /// Current values of field `i`. Panics if `i` is out of range.
    pub fn field(&self, i: usize) -> &'a [f32] {
        self.fields[i].current()
    }
}

impl<S: ScalarFieldSpec> FieldLayer for ScalarField<S> {
    const KIND: &'static str = "Scalar";

    type Params = S::Params;
    type Read<'a> = ScalarRead<'a>;
    type DepositLanes = Deposits;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        S::param_descriptors()
    }

    fn from_params(params: &[ParamValue]) -> S::Params {
        S::from_params(params)
    }

    fn new(extent: Extent, _params: &[ParamValue]) -> Self {
        let (width, height) = extent.cells();
        let n_cells = (width as usize) * (height as usize);
        let mut sites = vec![0u8; n_cells];
        S::build_sites(width, height, &mut sites);

        let mut field = Self {
            fields: (0..S::FIELDS).map(|_| Grid2D::new(width, height)).collect(),
            scatter: ScatterGrid::new(n_cells, S::COMBINE),
            sites,
            display_cells: vec![0; n_cells],
            width,
            height,
            _marker: PhantomData,
        };
        field.prepare_view();
        field
    }

    fn read(&self) -> ScalarRead<'_> {
        self.current()
    }

    fn alloc_deposits(&self, n: usize) -> Deposits {
        Deposits {
            cell: vec![0; n],
            values: (0..S::FIELDS).map(|_| vec![0.0; n]).collect(),
        }
    }

    fn update(&mut self, deposits: &Deposits, p: &S::Params, _tick: u64) {
        for (f, grid) in self.fields.iter_mut().enumerate() {
            {
                let (current, next) = grid.current_and_next_mut();
                // Decay runs in the merge pass, so nothing walks the grid a second time. Each cell decays after
                // its deposits are combined, and a fresh deposit is one step old when read.
                self.scatter
                    .scatter_then(&deposits.cell, &deposits.values[f], current, next, |v| S::decay(v, p));
            }
            grid.swap();
        }
    }

    fn prepare_view(&mut self) {
        let Self {
            fields,
            sites,
            display_cells,
            ..
        } = self;
        let current: Vec<&[f32]> = fields.iter().map(Grid2D::current).collect();

        for_each_chunk_mut!(display_cells.as_mut_slice(), STATS_CHUNK, |_c, base, cells| {
            let mut values = vec![0.0f32; current.len()];
            for (k, out) in cells.iter_mut().enumerate() {
                let c = base + k;
                for (v, field) in values.iter_mut().zip(current.iter()) {
                    *v = field[c];
                }
                S::quantize(sites[c], &values, out);
            }
        });
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        Some(GridView {
            width: self.width,
            height: self.height,
            cells: &self.display_cells,
            palette: S::PALETTE,
        })
    }

    fn cell_count(&self) -> usize {
        self.display_cells.len()
    }

    fn heap_bytes(&self) -> usize {
        self.fields.iter().map(Grid2D::heap_bytes).sum::<usize>()
            + self.scatter.heap_bytes()
            + self.sites.capacity()
            + self.display_cells.capacity()
    }
}

#[cfg(test)]
mod tests {
    use henad_core::authoring::model::field::{Extent, FieldLayer};
    use henad_core::params::{ParamDescriptor, ParamValue};

    use super::{ScalarField, ScalarFieldSpec};
    use crate::cpu::primitives::scatter::Combine;

    /// A field spec that implements no `Debug`.
    struct OpaqueSpec;

    impl ScalarFieldSpec for OpaqueSpec {
        const FIELDS: usize = 2;
        const COMBINE: Combine = Combine::Max;
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]];

        type Params = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            Vec::new()
        }

        fn from_params(_params: &[ParamValue]) {}

        fn build_sites(_width: u32, _height: u32, _sites: &mut [u8]) {}

        fn decay(v: f32, (): &()) -> f32 {
            v
        }

        fn quantize(_site: u8, _values: &[f32], out: &mut u8) {
            *out = 0;
        }
    }

    /// The field, its read view and its deposits implement `Debug` for a spec that does not, and print their sizes
    /// alone. A derive would bound the field's impl on the spec and print every value.
    #[test]
    fn the_field_prints_its_size_for_any_spec() {
        let field = <ScalarField<OpaqueSpec> as FieldLayer>::new(Extent { w: 64.0, h: 32.0 }, &[]);
        let shown = format!("{field:?}");
        assert!(
            shown.starts_with("ScalarField { width: 64, height: 32, fields: 2, scatter: ScatterGrid { n_cells: 2048,"),
            "{shown}"
        );
        assert_eq!(
            format!("{:?}", field.read()),
            "ScalarRead { width: 64, height: 32, fields: 2, .. }"
        );
        assert_eq!(
            format!("{:?}", field.alloc_deposits(100)),
            "Deposits { len: 100, fields: 2, .. }"
        );
    }
}
