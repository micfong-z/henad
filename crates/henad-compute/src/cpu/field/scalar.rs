//! Scalar `f32` fields written by agent deposits and decayed each tick.

use std::marker::PhantomData;

use henad_core::authoring::field::{Extent, FieldLayer};
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::GridView;

use crate::cpu::primitives::chunked::STATS_CHUNK;
use crate::cpu::primitives::scatter::{Combine, ScatterGrid};
use crate::for_each_chunk_mut;

/// The rules a scalar field needs that the mechanics cannot supply.
pub trait ScalarFieldSpec: Send + Sync + 'static {
    /// How many fields share the grid and the scatter scratch.
    const FIELDS: usize;
    /// How deposits landing in the same cell combine.
    const COMBINE: Combine;
    /// Colours for the quantised display layer.
    const PALETTE: &'static [[u8; 4]];

    type Params: Send + Sync;

    fn param_descriptors() -> Vec<ParamDescriptor>;
    fn from_params(params: &[ParamValue]) -> Self::Params;

    /// Static terrain, written once at construction.
    fn build_sites(width: u32, height: u32, sites: &mut [u8]);

    fn decay(v: f32, p: &Self::Params) -> f32;

    /// One cell's palette index, from the terrain and every field's current value.
    fn quantise(site: u8, values: &[f32], out: &mut u8);
}

/// Per agent deposit lanes: one cell each, and one value per field.
pub struct Deposits {
    pub cell: Vec<u32>,
    /// `values[f][i]` is agent `i`'s deposit into field `f`. An agent that writes one field leaves
    /// the others at the combine's identity, so every lane stays dense.
    pub values: Vec<Vec<f32>>,
}

impl Deposits {
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
    /// Shared by every field. Same dimensions, same combine, and the calls are sequential.
    scatter: ScatterGrid,
    sites: Vec<u8>,
    /// `GridView::cells` is `&[u8]` but a field is `f32`, so the layer owns the quantisation.
    display_cells: Vec<u8>,
    width: u32,
    height: u32,
    _marker: PhantomData<S>,
}

impl<S: ScalarFieldSpec> ScalarField<S> {
    pub fn field(&self, i: usize) -> &Grid2D<f32> {
        &self.fields[i]
    }

    pub fn sites(&self) -> &[u8] {
        &self.sites
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn display_cells(&self) -> &[u8] {
        &self.display_cells
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

/// What an agent kernel sees of the field.
#[derive(Clone, Copy)]
pub struct ScalarRead<'a> {
    fields: &'a Vec<Grid2D<f32>>,
    pub sites: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl<'a> ScalarRead<'a> {
    pub fn field(&self, i: usize) -> &'a [f32] {
        self.fields[i].current()
    }
}

impl<S: ScalarFieldSpec> FieldLayer for ScalarField<S> {
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
                self.scatter.scatter(&deposits.cell, &deposits.values[f], current, next);
            }
            // Decay after the merge, so a fresh deposit is already one step old when read.
            for_each_chunk_mut!(grid.next_mut(), STATS_CHUNK, |_c, _base, cells| {
                for v in cells.iter_mut() {
                    *v = S::decay(*v, p);
                }
            });
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
                S::quantise(sites[c], &values, out);
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
