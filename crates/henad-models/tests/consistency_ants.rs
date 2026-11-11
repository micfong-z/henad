//! Self consistency checks for ants, and the cross-engine gate.
//!
//! Randomness sits inside the advect pass, so two engines can agree on the rules and still disagree
//! on every trajectory. The gate below removes every draw instead of matching generators. See
//! `tests/fixtures/docs/ants_fixture.md`.

use std::collections::HashMap;

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_models::ants::AntsModel;
use henad_models::ants::field::{LOW_PHEROMONE, OBSTACLE, TO_FOOD, TO_HOME};

mod support;

#[path = "gates/ants.rs"]
mod gate;

use gate::{GATE_AGENTS, GATE_H, GATE_TICKS, GATE_W, GateResult, gate_field, gate_params};
use support::{fixture_dir, reference_fixtures};

type State = AgentModelState<AntsModel>;

const W: u32 = 100;
const H: u32 = 100;
const AGENTS: u32 = 400;

const CUTDOWN: f32 = 0.9;
const REWARD: f32 = 1.0;
const MOMENTUM: f32 = 0.8;
const RANDOM_ACTION: f32 = 0.1;

/// Evaporation factor, well below the 0.999 default so decay is visible within a short run.
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

// The cross-engine gate, whose ants, field and parameters `gates/ants.rs` holds.

/// Number of seeds, enough that a tie cannot hide behind luck.
///
/// A one-in-four tie survives six seeds with probability 0.75^6, about 18%, and 64 seeds with about 1e-8.
const GATE_SEEDS: u64 = 64;

fn gate_run(seed: Option<u64>, ticks: u32) -> GateResult {
    let p = gate_params();
    let mut state = State::from_agents_and_field(
        &p,
        seed,
        |lanes, _extent| {
            for (i, &(x, y, last_step, has_food, reward)) in GATE_AGENTS.iter().enumerate() {
                lanes.pos_x[i] = x;
                lanes.pos_y[i] = y;
                lanes.last_step[i] = last_step;
                lanes.has_food[i] = has_food;
                lanes.reward[i] = reward;
            }
        },
        |field, _extent| {
            assert!(field.seed_field(TO_FOOD, &gate_field(TO_FOOD)), "to-food layer");
            assert!(field.seed_field(TO_HOME, &gate_field(TO_HOME)), "to-home layer");
        },
    );
    for _ in 0..ticks {
        state.step();
    }
    let l = state.lanes();
    let agents = (0..GATE_AGENTS.len())
        .map(|i| (l.pos_x[i], l.pos_y[i], l.last_step[i], l.has_food[i], l.reward[i]))
        .collect();
    let (to_food, to_home) = fields(&state);
    (agents, to_food, to_home)
}

/// The whole gate rests on this property. Ties are the only place advection draws, so a field with
/// no ties makes the run reproducible in another engine, whatever generator that engine uses.
#[test]
fn the_seeded_field_has_no_ties_in_any_neighbourhood() {
    for layer in [TO_FOOD, TO_HOME] {
        let cells = gate_field(layer);
        for y in 0..GATE_H as i32 {
            for x in 0..GATE_W as i32 {
                let mut seen = Vec::with_capacity(9);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= GATE_W as i32 || ny >= GATE_H as i32 {
                            continue;
                        }
                        let v = cells[(ny * GATE_W as i32 + nx) as usize];
                        assert!(v > 0.0, "layer {layer} cell ({nx}, {ny}) is not positive");
                        assert!(
                            !seen.contains(&v),
                            "layer {layer} has two cells of {v} around ({x}, {y}), so a tie-break would draw"
                        );
                        seen.push(v);
                    }
                }
            }
        }
    }
}

/// Checks that every seed gives the same answer for the gate scenario. Another engine's generator has to give that
/// answer too.
#[test]
fn the_gate_scenario_does_not_depend_on_the_random_stream() {
    let reference = gate_run(Some(1), GATE_TICKS);
    for seed in 2..=GATE_SEEDS {
        let other = gate_run(Some(seed), GATE_TICKS);
        assert!(
            other == reference,
            "seed {seed} moved the gate scenario, so it draws somewhere and cannot gate another engine"
        );
    }
}

/// Checks that the tick after the gate draws. The gate stops before that tick to avoid the draw.
///
/// The test pins the reason instead of the number. If a change to the deposit rule moves the first
/// tie, this fails and the gate's tick count has to be chosen again instead of silently left too high.
#[test]
fn the_tick_after_the_gate_is_where_ties_begin() {
    let reference = gate_run(Some(1), GATE_TICKS + 1);
    let diverges = (2..=GATE_SEEDS).any(|seed| gate_run(Some(seed), GATE_TICKS + 1) != reference);
    assert!(
        diverges,
        "tick {} no longer draws, so the gate is shorter than it needs to be",
        GATE_TICKS + 1
    );
}

/// Both halves of the gate have to actually do something, or it would pass against a stub.
#[test]
fn the_gate_scenario_moves_ants_and_changes_the_field() {
    let (agents, to_food, to_home) = gate_run(Some(1), GATE_TICKS);
    let start: Vec<(f32, f32)> = GATE_AGENTS.iter().map(|&(x, y, ..)| (x, y)).collect();
    let moved = agents
        .iter()
        .zip(&start)
        .filter(|((x, y, ..), s)| (*x, *y) != **s)
        .count();
    assert!(moved >= GATE_AGENTS.len() - 2, "only {moved} of the gate's ants moved");

    // Above what pure evaporation would leave. Comparing against the seeded field instead passes
    // for a model that deposits nothing, since decay moves every cell.
    let decay = 0.999f32.powi(GATE_TICKS as i32);
    for (layer, ours) in [("to_food", &to_food), ("to_home", &to_home)] {
        let seeded = gate_field(if layer == "to_food" { TO_FOOD } else { TO_HOME });
        let raised = ours
            .iter()
            .zip(&seeded)
            .filter(|(got, want)| **got > **want * decay * (1.0 + 1e-6))
            .count();
        assert!(
            raised > 0,
            "no cell of {layer} rose above plain decay, so nothing was deposited"
        );
    }
}

/// Parses a fixture: `# key: value` header lines, an agent block, then one block per layer.
fn parse_gate_fixture(text: &str) -> (HashMap<String, String>, GateResult) {
    let mut header = HashMap::new();
    let mut agents = Vec::new();
    let mut to_food = Vec::new();
    let mut to_home = Vec::new();
    let mut section = "";

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ---") {
            section = match rest.trim().split(':').next().unwrap_or("").trim() {
                "agents" => "agents",
                "to_food" => "to_food",
                "to_home" => "to_home",
                other => panic!("unknown fixture section `{other}`"),
            };
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some((k, v)) = rest.split_once(':') {
                header.insert(k.trim().to_lowercase(), v.trim().to_owned());
            }
            continue;
        }
        let parse = |t: &str| t.parse::<f32>().unwrap_or_else(|_| panic!("`{t}` is not a number"));
        match section {
            "agents" => {
                let f: Vec<f32> = line.split_whitespace().map(parse).collect();
                assert_eq!(f.len(), 5, "an agent row is `x y last_step has_food reward`");
                agents.push((f[0], f[1], f[2] as u8, f[3] as u8, f[4]));
            }
            "to_food" => to_food.extend(line.split_whitespace().map(parse)),
            "to_home" => to_home.extend(line.split_whitespace().map(parse)),
            other => panic!("data before any section marker (section `{other}`)"),
        }
    }
    (header, (agents, to_food, to_home))
}

/// Relative tolerance of the gate, since Henad holds the field in `f32` and most engines hold it in `f64`.
const GATE_TOLERANCE: f32 = 1e-6;

fn close(got: f32, want: f32) -> bool {
    let scale = got.abs().max(want.abs()).max(1.0);
    (got - want).abs() <= GATE_TOLERANCE * scale
}

/// Checks Henad against every engine that has committed a fixture.
#[test]
fn matches_every_reference_fixture() {
    let dir = fixture_dir("ants");
    assert!(
        dir.is_dir(),
        "{} is missing, so no port is being checked at all",
        dir.display()
    );

    let mut checked = Vec::new();
    for (name, text) in reference_fixtures(&dir) {
        let (header, (agents, to_food, to_home)) = parse_gate_fixture(&text);
        let engine = header.get("engine").map_or("?", String::as_str);
        let steps: u32 = header
            .get("steps")
            .unwrap_or_else(|| panic!("{name} has no `steps` header"))
            .parse()
            .unwrap_or_else(|_| panic!("{name}: `steps` is not a number"));

        assert_eq!(
            steps, GATE_TICKS,
            "{name} was recorded at {steps} ticks where the scenario declares {GATE_TICKS}, the last \
             tick it stays draw free for. Regenerate it rather than comparing against a different run."
        );
        for (key, want) in [
            ("width", GATE_W),
            ("height", GATE_H),
            ("agents", GATE_AGENTS.len() as u32),
        ] {
            let got = header.get(key).and_then(|v| v.parse::<u32>().ok());
            assert_eq!(
                got,
                Some(want),
                "{name} declares {key} {got:?} where the scenario is {want}"
            );
        }

        // Seed 1 is the reference the stream-independence test is anchored on.
        let (ours_agents, ours_food, ours_home) = gate_run(Some(1), steps);
        assert_eq!(agents.len(), ours_agents.len(), "{name} has the wrong agent count");
        for (i, (got, want)) in ours_agents.iter().zip(&agents).enumerate() {
            assert_eq!(
                (got.0, got.1, got.2, got.3),
                (want.0, want.1, want.2, want.3),
                "ant {i}: Henad and {engine} disagree on cell, last step or load [{name}]"
            );
            assert!(
                close(got.4, want.4),
                "ant {i} reward: Henad {}, {engine} {} [{name}]",
                got.4,
                want.4
            );
        }
        for (layer, ours, theirs) in [("to_food", &ours_food, &to_food), ("to_home", &ours_home, &to_home)] {
            assert_eq!(ours.len(), theirs.len(), "{name}: {layer} has the wrong cell count");
            for (i, (got, want)) in ours.iter().zip(theirs).enumerate() {
                assert!(
                    close(*got, *want),
                    "{layer} cell {i}: Henad {got}, {engine} {want} after {steps} ticks [{name}]"
                );
            }
        }
        checked.push(format!(
            "{engine} / {}",
            header.get("scenario").map_or("?", String::as_str)
        ));
    }

    assert!(
        !checked.is_empty(),
        "{} exists but holds no fixtures, so nothing was checked",
        dir.display()
    );
}
