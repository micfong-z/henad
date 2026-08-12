//! Self consistency checks for ants.
//!
//! Ants Foraging does not have a cross-engine reference implementation, due to the complexity of the field update
//! and randomness.

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_models::ants::AntsModel;
use henad_models::ants::field::{LOW_PHEROMONE, OBSTACLE, TO_FOOD, TO_HOME};

type State = AgentModelState<AntsModel>;

const W: u32 = 100;
const H: u32 = 100;
const AGENTS: u32 = 400;

const CUTDOWN: f32 = 0.9;
const REWARD: f32 = 1.0;
const MOMENTUM: f32 = 0.8;
const RANDOM_ACTION: f32 = 0.1;

/// Well below the 0.999 default so decay is visible within a short run.
const EVAPORATION: f32 = 0.99;

const TICKS: usize = 40;

fn params_with(evaporation: f32) -> Vec<ParamValue> {
    vec![
        ParamValue::U32(AGENTS),
        ParamValue::F32(W as f32),
        ParamValue::F32(H as f32),
        ParamValue::F32(CUTDOWN),
        ParamValue::F32(REWARD),
        ParamValue::F32(MOMENTUM),
        ParamValue::F32(RANDOM_ACTION),
        ParamValue::F32(evaporation),
    ]
}

fn state() -> State {
    State::from_params_seeded(&params_with(EVAPORATION), Some(20_260_806))
}

fn fields(state: &State) -> (Vec<f32>, Vec<f32>) {
    (
        state.field().field(TO_FOOD).current().to_vec(),
        state.field().field(TO_HOME).current().to_vec(),
    )
}

fn occupied(state: &State) -> Vec<bool> {
    let lanes = state.lanes();
    let mut mask = vec![false; (W * H) as usize];
    for i in 0..lanes.pos_x.len() {
        mask[lanes.pos_y[i] as usize * W as usize + lanes.pos_x[i] as usize] = true;
    }
    mask
}

/// Deposits land on the depositing ant's own cell and nowhere else, so every unoccupied cell must
/// decay by exactly the evaporation factor.
///
/// This is an analytic check for ants.
#[test]
fn cells_without_an_ant_decay_by_exactly_the_evaporation_rate() {
    let mut state = state();
    // A few ticks first to ensure there is some trail to decay.
    for _ in 0..10 {
        state.step();
    }

    for tick in 0..TICKS {
        let before = fields(&state);
        let ants = occupied(&state);
        state.step();
        let after = fields(&state);

        let mut checked = 0usize;
        for (name, (b, a)) in [("to_food", (&before.0, &after.0)), ("to_home", (&before.1, &after.1))] {
            for i in 0..b.len() {
                if ants[i] {
                    continue;
                }
                let decayed = b[i] * EVAPORATION;
                let expected = if decayed < LOW_PHEROMONE { 0.0 } else { decayed };
                assert_eq!(
                    a[i], expected,
                    "{name} cell {i} held {} then {} at tick {tick}, expected {expected}",
                    b[i], a[i]
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "no unoccupied cells to check at tick {tick}");
    }
}

/// Ants move on the integer lattice, never leave the world, and never stand on an obstacle.
#[test]
fn ants_stay_on_the_lattice_inside_the_world_and_off_obstacles() {
    let mut state = state();

    for tick in 0..TICKS {
        state.step();
        let sites = state.field().sites().to_vec();
        let lanes = state.lanes();
        for i in 0..lanes.pos_x.len() {
            let (x, y) = (lanes.pos_x[i], lanes.pos_y[i]);
            assert!(
                (0.0..W as f32).contains(&x) && (0.0..H as f32).contains(&y),
                "ant {i} at ({x}, {y}) left the world at tick {tick}"
            );
            assert!(
                x.fract() == 0.0 && y.fract() == 0.0,
                "ant {i} at ({x}, {y}) came off the lattice at tick {tick}"
            );
            let cell = y as usize * W as usize + x as usize;
            assert_ne!(sites[cell], OBSTACLE, "ant {i} stood on an obstacle at tick {tick}");
            assert!(
                lanes.has_food[i] <= 1,
                "ant {i} has_food is {} at tick {tick}",
                lanes.has_food[i]
            );
        }
    }
}

/// Deliveries is a running total, so it can only ever increase.
#[test]
fn deliveries_never_decrease() {
    let mut state = state();
    let mut previous = *state.tally();

    for tick in 0..TICKS {
        state.step();
        let now = *state.tally();
        assert!(
            now >= previous,
            "deliveries fell from {previous} to {now} at tick {tick}"
        );
        previous = now;
    }
}
