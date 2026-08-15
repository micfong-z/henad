use henad_core::authoring::field::{Extent, FieldLayer as _};
use henad_core::authoring::grid_model::GridModel;
use henad_core::helpers::{extract_u32, u32_param};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamStore, ParamValue};
use henad_core::view::{GridView, StatEntry, stat_entries};

use crate::cpu::field::CaField;

pub use crate::cpu::field::GRID_INIT_SEED;

/// Engine wrapper that implements `SimState` for any `GridModel`.
pub struct GridModelState<M: GridModel> {
    field: CaField<M>,
    params: ParamStore,
    tick: u64,
}

impl<M: GridModel> GridModelState<M> {
    pub fn from_params(params: &[ParamValue]) -> Self {
        Self::from_params_seeded(params, None)
    }

    /// Build a state whose RNG starts from `seed`, or [`GRID_INIT_SEED`] when it is `None`.
    pub fn from_params_seeded(params: &[ParamValue], seed: Option<u64>) -> Self {
        let extent = Extent {
            w: extract_u32(params, 0, 1024) as f32,
            h: extract_u32(params, 1, 1024) as f32,
        };
        Self {
            field: CaField::with_seed(extent, params, seed),
            params: ParamStore::new(&grid_model_param_descriptors::<M>(), params),
            tick: 0,
        }
    }

    /// `None` unless `cells` is exactly the length `params` implies.
    pub fn from_cells(params: &[ParamValue], cells: &[u8]) -> Option<Self> {
        let extent = Extent {
            w: extract_u32(params, 0, 1024) as f32,
            h: extract_u32(params, 1, 1024) as f32,
        };
        Some(Self {
            field: CaField::from_cells(extent, cells)?,
            params: ParamStore::new(&grid_model_param_descriptors::<M>(), params),
            tick: 0,
        })
    }
}

/// Grid width and height, prepended to the model's own descriptors.
pub fn grid_model_param_descriptors<M: GridModel>() -> Vec<ParamDescriptor> {
    let mut descs = vec![
        u32_param("grid_width", "Grid Width", 1024, 1, 10_000).on_reload(),
        u32_param("grid_height", "Grid Height", 1024, 1, 10_000).on_reload(),
    ];
    descs.extend(M::param_descriptors());
    descs
}

impl<M: GridModel> SimState for GridModelState<M> {
    fn step(&mut self) {
        let hot = M::from_params(self.params.values());
        self.field.update(&(), &hot, self.tick);
        self.tick += 1;
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        self.field.grid_view()
    }

    fn stats(&self) -> Vec<StatEntry> {
        stat_entries(M::STATS, M::stats(self.field.grid()))
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.params.set(index, value)
    }

    fn population(&self) -> u64 {
        self.field.cell_count() as u64
    }

    fn heap_bytes(&self) -> usize {
        self.field.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::GridModelState;
    use henad_core::authoring::grid_model::GridModel;
    use henad_core::grid::Grid2D;
    use henad_core::helpers::xorshift64;
    use henad_core::model::SimState as _;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::topology::NeighborhoodKind;
    use henad_core::view::{StatDescriptor, StatValue};

    fn live_neighbors(neighbors: &[u8]) -> u8 {
        neighbors.iter().filter(|&&n| n != 0).count() as u8
    }

    /// A model that reports how many live neighbours each cell saw, so one step pins the exact set
    /// the engine gathered instead of whatever a real rule would collapse it to.
    macro_rules! counting_model {
        ($ty:ident, $id:literal, $kind:expr) => {
            struct $ty;

            impl GridModel for $ty {
                const NAME: &'static str = $id;
                const ID: &'static str = $id;
                const DESCRIPTION: &'static str = $id;
                const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]; 9];
                const NEIGHBORHOOD: NeighborhoodKind = $kind;
                const STATS: &'static [StatDescriptor] = &[];
                type Params = ();

                fn param_descriptors() -> Vec<ParamDescriptor> {
                    Vec::new()
                }

                fn from_params(_params: &[ParamValue]) -> Self::Params {}

                fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], rng: &mut u64) {
                    for cell in grid.current_mut() {
                        *rng = xorshift64(*rng);
                        *cell = (*rng & 1) as u8;
                    }
                }

                fn step_cell(_cell: u8, neighbors: &[u8], _params: &Self::Params, _rng: &mut u64) -> u8 {
                    live_neighbors(neighbors)
                }

                fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {
                    Vec::new()
                }
            }
        };
    }

    counting_model!(MooreCount, "moore_count", NeighborhoodKind::Moore);
    counting_model!(VnCount, "vn_count", NeighborhoodKind::VonNeumann);

    /// The plain modulo gather the row loops peel their edge columns to avoid.
    fn reference(cells: &[u8], w: usize, h: usize, moore: bool) -> Vec<u8> {
        let mut out = vec![0u8; cells.len()];
        for y in 0..h {
            let (ym, yp) = ((y + h - 1) % h, (y + 1) % h);
            for x in 0..w {
                let (xm, xp) = ((x + w - 1) % w, (x + 1) % w);
                let neighbors = if moore {
                    vec![
                        cells[ym * w + xm],
                        cells[ym * w + x],
                        cells[ym * w + xp],
                        cells[y * w + xm],
                        cells[y * w + xp],
                        cells[yp * w + xm],
                        cells[yp * w + x],
                        cells[yp * w + xp],
                    ]
                } else {
                    vec![
                        cells[ym * w + x],
                        cells[y * w + xm],
                        cells[y * w + xp],
                        cells[yp * w + x],
                    ]
                };
                out[y * w + x] = live_neighbors(&neighbors);
            }
        }
        out
    }

    /// Widths 1 and 2 are the interesting ones: both peeled columns land on the same cells.
    const SIZES: [(u32, u32); 7] = [(1, 1), (1, 6), (2, 2), (3, 4), (6, 1), (7, 9), (65, 3)];

    fn cells<M: GridModel>(state: &GridModelState<M>) -> Vec<u8> {
        state
            .grid_view()
            .expect("a grid model always has a grid view")
            .cells
            .to_vec()
    }

    #[test]
    fn moore_gather_wraps_like_the_reference() {
        for (w, h) in SIZES {
            let params = vec![ParamValue::U32(w), ParamValue::U32(h)];
            let mut state = GridModelState::<MooreCount>::from_params(&params);
            let before = cells(&state);
            state.step();
            assert_eq!(
                cells(&state),
                reference(&before, w as usize, h as usize, true),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn von_neumann_gather_wraps_like_the_reference() {
        for (w, h) in SIZES {
            let params = vec![ParamValue::U32(w), ParamValue::U32(h)];
            let mut state = GridModelState::<VnCount>::from_params(&params);
            let before = cells(&state);
            state.step();
            assert_eq!(
                cells(&state),
                reference(&before, w as usize, h as usize, false),
                "{w}x{h}"
            );
        }
    }

    /// Two seeds must give different runs, and `None` must reproduce the fixed default exactly.
    #[test]
    fn seeds_produce_independent_replicates() {
        let params = vec![ParamValue::U32(64), ParamValue::U32(64)];
        let run = |seed: Option<u64>| -> Vec<u8> {
            let mut state = GridModelState::<MooreCount>::from_params_seeded(&params, seed);
            state.step();
            cells(&state)
        };

        assert_eq!(run(None), run(None), "the default must be reproducible");
        assert_ne!(run(Some(1)), run(Some(2)), "different seeds must give different runs");
        assert_eq!(run(Some(7)), run(Some(7)), "a seed must still be reproducible");
        assert_ne!(run(None), run(Some(1)), "a user seed must not land on the default");

        // `xorshift64(0) == 0` is absorbing, so if the engine's RNG state is stuck it will produce a uniform grid.
        let zero = run(Some(0));
        assert!(
            zero.iter().any(|&c| c != zero[0]),
            "seed 0 produced a uniform grid, so its RNG state was stuck"
        );
    }

    /// The row seed comes from the row index, so a grid stepped in one thread and the same grid
    /// stepped across many must agree bit for bit.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        let params = vec![ParamValue::U32(64), ParamValue::U32(64)];
        let run = |threads: usize| -> Vec<u8> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let mut state = GridModelState::<MooreCount>::from_params(&params);
                for _ in 0..20 {
                    state.step();
                }
                cells(&state)
            })
        };
        assert_eq!(run(1), run(7), "grid contents depend on the thread count");
    }
}
