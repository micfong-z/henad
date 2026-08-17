pub mod field;
mod lanes;
mod step;

pub use crate::ants::lanes::{AntLanes, NO_STEP};

use henad_compute::cpu::field::scalar::{Deposits, ScalarField};
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_core::authoring::model::agent_model::{AgentModel, StepCtx};
use henad_core::authoring::model::field::Extent;
use henad_core::grid::Grid2D;
use henad_core::helpers::{extract_f32, f32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use crate::ants::field::{PheromoneField, TO_FOOD, TO_HOME, nest_cell};

/// Indexed by `has_food` itself, not by a copy of it.
pub const ANT_PALETTE: [[u8; 4]; 2] = [
    [0xE8, 0xE8, 0xF0, 0xFF], // searching
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying food
];

/// Stat series colours.
pub const STAT_PALETTE: [[u8; 4]; 3] = [
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying
    [0xF2, 0xE4, 0x5C, 0xFF], // deliveries
    [0x2E, 0x8B, 0xE8, 0xFF], // total pheromone
];

henad_core::params! {
    const UPDATE_CUTDOWN = f32_param("update_cutdown", "Trail Falloff", 0.9, 0.5, 1.0, Some(0.01));
    const REWARD = f32_param("reward", "Site Reward", 1.0, 0.1, 10.0, Some(0.1));
    const MOMENTUM = f32_param("momentum", "Momentum Probability", 0.8, 0.0, 1.0, Some(0.01));
    const RANDOM_ACTION = f32_param("random_action", "Random Action Probability", 0.1, 0.0, 1.0, Some(0.01));
}

/// Ant foraging, ported from krABMaga's `antsforaging`.
///
/// Three semantic divergences a comparison has to state. Deposits combine with `max` rather than
/// last-writer-wins. The pheromone field is all read old, all write new. The RNG is seeded per
/// chunk per tick rather than drawn per call.
///
/// The reference's biased neighbour tie-break is reproduced rather than corrected, see
/// [`step::advect_agent`].
pub struct AntsModel;

pub struct AntParams {
    pub w: i32,
    pub h: i32,
    pub cutdown: f32,
    /// Cutdown raised to the diagonal distance, since those neighbours are further away.
    pub diagonal: f32,
    pub reward: f32,
    pub momentum: f32,
    pub random_action: f32,
}

impl AgentModel for AntsModel {
    const NAME: &'static str = "Ant Foraging";
    const ID: &'static str = "ants";
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, around obstacles";
    const PALETTE: &'static [[u8; 4]] = &ANT_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Carrying Food", STAT_PALETTE[0]),
        StatDescriptor::new("Deliveries", STAT_PALETTE[1]),
        StatDescriptor::new("Total Pheromone", STAT_PALETTE[2]),
    ];
    const CHUNK: usize = 4096;
    const DEFAULT_AGENTS: u32 = 2_000;
    const MAX_AGENTS: u32 = 5_000_000;
    const DEFAULT_EXTENT: Extent = Extent { w: 200.0, h: 200.0 };

    type Lanes = AntLanes;
    type Field = ScalarField<PheromoneField>;
    type Index = henad_core::authoring::model::agent_model::NoIndex;
    type Params = AntParams;
    type Tally = u64;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue], extent: Extent) -> AntParams {
        let cutdown = extract_f32(params, UPDATE_CUTDOWN, 0.9);
        AntParams {
            w: extent.w as i32,
            h: extent.h as i32,
            cutdown,
            diagonal: cutdown.powf(std::f32::consts::SQRT_2),
            reward: extract_f32(params, REWARD, 1.0),
            momentum: extract_f32(params, MOMENTUM, 0.8),
            random_action: extract_f32(params, RANDOM_ACTION, 0.1),
        }
    }

    /// Ants start holding `reward` so they lay home pheromone immediately and the colony has a
    /// gradient to navigate back along.
    fn init(lanes: &mut AntLanes, extent: Extent, params: &[ParamValue], _rng: &mut u64) {
        let (width, height) = extent.cells();
        let nest = nest_cell(width, height) as u32;
        let (x, y) = ((nest % width) as f32, (nest / width) as f32);
        let reward = extract_f32(params, REWARD, 1.0);
        for i in 0..lanes.pos_x.len() {
            lanes.pos_x[i] = x;
            lanes.pos_y[i] = y;
            lanes.reward[i] = reward;
        }
    }

    fn run_deposit_pass(lanes: &AntLanes, deposits: &mut Deposits, ctx: &StepCtx<'_, Self>) {
        step::deposit(lanes, deposits, ctx);
    }

    fn run_step_pass(lanes: &mut AntLanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) -> u64 {
        step::advect(lanes, ctx, seed, tick)
    }

    fn stats(lanes: &AntLanes, field: &ScalarField<PheromoneField>, tally: &u64) -> Vec<StatValue> {
        let carrying = lanes.has_food.iter().filter(|&&f| f != 0).count();
        vec![
            StatValue::Scalar(carrying as f64),
            StatValue::Scalar(*tally as f64),
            StatValue::Scalar(total_pheromone(field.field(TO_FOOD), field.field(TO_HOME))),
        ]
    }
}

/// Summed chunk by chunk in index order, so rayon's scheduling cannot change the total.
fn total_pheromone(to_food: &Grid2D<f32>, to_home: &Grid2D<f32>) -> f64 {
    field_sum(to_food.current()) + field_sum(to_home.current())
}

fn field_sum(cells: &[f32]) -> f64 {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK,
        |r| cells[r].iter().map(|&v| f64::from(v)).sum::<f64>(),
        |a, b| a + b,
        0.0,
    )
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::ants::field::{CELL_PALETTE, OBSTACLE};
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_core::model::SimState as _;

    type State = AgentModelState<AntsModel>;

    fn default_state() -> State {
        State::from_params(&[ParamValue::U32(500), ParamValue::F32(200.0), ParamValue::F32(200.0)])
    }

    #[test]
    fn every_ant_starts_on_the_nest_holding_a_reward() {
        let state = default_state();
        let lanes = state.lanes();
        let nest = nest_cell(200, 200) as u32;
        let (nx, ny) = ((nest % 200) as f32, (nest / 200) as f32);
        for i in 0..lanes.pos_x.len() {
            assert_eq!((lanes.pos_x[i], lanes.pos_y[i]), (nx, ny), "ant {i} is not on the nest");
            assert_eq!(lanes.reward[i], 1.0, "ant {i} has no reward to spend");
        }
    }

    /// Every quantised value must land inside the palette, or the renderer silently draws entry 0.
    #[test]
    fn every_display_cell_indexes_the_palette() {
        let mut state = default_state();
        for _ in 0..50 {
            state.step();
        }
        state.prepare_view();
        for (c, &cell) in state.field().display_cells().iter().enumerate() {
            assert!(
                (cell as usize) < CELL_PALETTE.len(),
                "cell {c} quantized to {cell}, past the palette"
            );
        }
    }

    /// Forgetting the refresh leaves the grid layer frozen at construction, with sites still
    /// rendering and pheromone never appearing. It was wrong that way first.
    #[test]
    fn the_grid_layer_shows_pheromone_laid_since_construction() {
        let mut state = default_state();
        let trail_count = |s: &State| {
            s.field()
                .display_cells()
                .iter()
                .filter(|&&c| (1..=12).contains(&c))
                .count()
        };
        assert_eq!(trail_count(&state), 0, "no trail should exist before the first tick");

        for _ in 0..100 {
            state.step();
        }
        state.prepare_view();
        assert!(
            trail_count(&state) > 0,
            "ants have been depositing for 100 ticks but the grid layer shows no trail at all"
        );
    }

    /// Handed to the renderer directly, so it may only ever hold a valid palette index.
    #[test]
    fn has_food_stays_a_valid_palette_index() {
        let mut state = default_state();
        for _ in 0..100 {
            state.step();
        }
        assert!(
            state.lanes().has_food.iter().all(|&f| (f as usize) < ANT_PALETTE.len()),
            "has_food is doubling as the render lane, so it may only hold 0 or 1"
        );
    }

    #[test]
    fn ants_stay_inside_the_bounded_field() {
        let mut state = default_state();
        for tick in 0..200 {
            state.step();
            let lanes = state.lanes();
            for i in 0..lanes.pos_x.len() {
                let (x, y) = (lanes.pos_x[i], lanes.pos_y[i]);
                assert!(
                    (0.0..200.0).contains(&x) && (0.0..200.0).contains(&y),
                    "ant {i} left the field at ({x}, {y}) on tick {tick}; the reference is bounded, not toroidal"
                );
            }
        }
    }

    /// The momentum and random action fallbacks are the easy ones to forget an obstacle check in.
    #[test]
    fn ants_never_enter_an_obstacle() {
        let mut state = default_state();
        for tick in 0..200 {
            state.step();
            let sites = state.field().sites().to_vec();
            let lanes = state.lanes();
            for i in 0..lanes.pos_x.len() {
                let c = (lanes.pos_y[i] as u32 * 200 + lanes.pos_x[i] as u32) as usize;
                assert_ne!(sites[c], OBSTACLE, "ant {i} is inside an obstacle on tick {tick}");
            }
        }
    }

    /// Three things could leak scheduling into the result. The scatter arm comes from the worker
    /// count, the movement RNG is seeded per chunk, and deliveries are a parallel reduction.
    ///
    /// One worker also stands in for wasm, where the shadow arm reduces through a single grid.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        /// Ant cells, deliveries, and both pheromone fields as raw bits.
        fn run(threads: usize) -> (Vec<u32>, u64, Vec<u32>) {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let mut state = default_state();
                for _ in 0..200 {
                    state.step();
                }
                let lanes = state.lanes();
                let cells = lanes
                    .pos_x
                    .iter()
                    .zip(&lanes.pos_y)
                    .map(|(&x, &y)| y as u32 * 200 + x as u32)
                    .collect();
                let field = state
                    .field()
                    .field(TO_HOME)
                    .current()
                    .iter()
                    .chain(state.field().field(TO_FOOD).current())
                    .map(|v| v.to_bits())
                    .collect();
                (cells, *state.tally(), field)
            })
        }

        let (cells_1, deliveries_1, field_1) = run(1);
        let (cells_n, deliveries_n, field_n) = run(7);
        assert_eq!(cells_1, cells_n, "ant positions depend on the thread count");
        assert_eq!(deliveries_1, deliveries_n, "delivery count depends on the thread count");
        assert_eq!(
            field_1, field_n,
            "pheromone field is not bit-identical across thread counts"
        );
    }

    /// The engine owns the extent, so the agent layer and the field layer cannot disagree.
    #[test]
    fn both_layers_report_the_same_world() {
        let state = default_state();
        let points = state.point_view().expect("ants draw agents");
        let grid = state.grid_view().expect("ants draw a field");
        assert_eq!(
            (points.world_w, points.world_h),
            (grid.width as f32, grid.height as f32)
        );
    }
}
