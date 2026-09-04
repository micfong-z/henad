//! Self consistency checks for ants, and the cross-engine gate.
//!
//! Randomness sits inside the advect pass, so two engines can agree on the rules and still disagree
//! on every trajectory. The gate below removes every draw instead of matching generators. See
//! `tests/fixtures/docs/ants_fixture.md`.

use std::collections::HashMap;
use std::path::Path;

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

// --- The cross-engine gate ---------------------------------------------------------------------
//
// See `tests/fixtures/docs/ants_fixture.md`. Randomness in this model lives entirely inside
// `advect_agent`: a tie between two neighbours draws, and the momentum and random-action branches
// draw. The scenario below sets both probabilities to zero and seeds a field whose 3x3
// neighbourhoods hold no two equal values, so no draw can change the outcome and the run is the
// same under any generator. That is what makes another engine's answer comparable at all.

/// Small enough to write out in full, large enough for both obstacle blobs and both sites.
const GATE_W: u32 = 32;
const GATE_H: u32 = 32;
/// Deposits eventually recreate ties in the evolved field, and the run stops being
/// stream-independent at nine ticks. Five leaves headroom.
const GATE_TICKS: u32 = 5;

/// Twelve ants: the four corners, a top edge, both sites, two cells beside an obstacle, one ant
/// carrying momentum history, and two sharing a cell so the deposit combine has something to merge.
///
/// `(x, y, last_step, has_food, reward)`, with `last_step` encoded `(dx + 1) * 3 + (dy + 1)` and
/// 255 meaning no step yet.
const GATE_AGENTS: [(f32, f32, u8, u8, f32); 12] = [
    (0.0, 0.0, 255, 0, 1.0),
    (31.0, 0.0, 255, 1, 1.0),
    (0.0, 31.0, 255, 0, 0.5),
    (31.0, 31.0, 255, 1, 0.5),
    (16.0, 0.0, 255, 0, 1.0),
    (4.0, 4.0, 255, 0, 1.0),    // on the food source
    (28.0, 28.0, 255, 1, 1.0),  // on the nest
    (8.0, 12.0, 255, 0, 1.0),   // (9, 13) is an obstacle
    (22.0, 20.0, 255, 1, 1.0),  // (21, 19) is an obstacle
    (15.0, 15.0, 7, 0, 1.0),    // last step (1, 0)
    (15.0, 15.0, 255, 0, 0.25), // shares the cell above, lower reward
    (15.0, 16.0, 2, 1, 1.0),    // last step (-1, 1)
];

/// The seeded field, stated as a formula so every engine builds the same one without a data file.
///
/// Two neighbours collide only when `7dx + 13dy` (or `11dx + 5dy`) is a multiple of the modulus.
/// Across a 3x3 window those sums stay well inside one period, so the only collision is with the
/// cell itself. Values sit in `(0, 1]`, keeping the best neighbour off zero and the momentum
/// branch out of reach.
fn gate_field(layer: usize) -> Vec<f32> {
    let (a, b, m) = if layer == TO_FOOD { (7, 13, 97) } else { (11, 5, 89) };
    (0..GATE_H)
        .flat_map(|y| (0..GATE_W).map(move |x| ((a * x + b * y) % m + 1) as f32 / (m + 1) as f32))
        .collect()
}

fn gate_params() -> Vec<ParamValue> {
    vec![
        ParamValue::U32(GATE_AGENTS.len() as u32),
        ParamValue::F32(GATE_W as f32),
        ParamValue::F32(GATE_H as f32),
        ParamValue::F32(CUTDOWN),
        ParamValue::F32(REWARD),
        ParamValue::F32(0.0), // momentum, off so the branch cannot fire
        ParamValue::F32(0.0), // random action, off for the same reason
        ParamValue::F32(0.999),
    ]
}

/// Agent rows and both pheromone layers after `ticks`, which is exactly what a fixture holds.
type GateResult = (Vec<(f32, f32, u8, u8, f32)>, Vec<f32>, Vec<f32>);

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

/// The property the whole gate rests on. Ties are the only place advection draws, so a field with
/// no ties makes the run reproducible in an engine whose generator is nothing like Henad's.
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

/// What another engine has to be able to reproduce: the same answer from a different generator.
#[test]
fn the_gate_scenario_does_not_depend_on_the_random_stream() {
    let reference = gate_run(Some(1), GATE_TICKS);
    for seed in [2_u64, 3, 97, 20_260_904, u64::MAX] {
        let other = gate_run(Some(seed), GATE_TICKS);
        assert!(
            other == reference,
            "seed {seed} moved the gate scenario, so it draws somewhere and cannot gate another engine"
        );
    }
}

/// Both halves of the gate have to actually do something, or it would pass against a stub.
#[test]
fn the_gate_scenario_moves_ants_and_changes_the_field() {
    let (agents, to_food, to_home) = gate_run(None, GATE_TICKS);
    let start: Vec<(f32, f32)> = GATE_AGENTS.iter().map(|&(x, y, ..)| (x, y)).collect();
    let moved = agents
        .iter()
        .zip(&start)
        .filter(|((x, y, ..), s)| (*x, *y) != **s)
        .count();
    assert!(moved >= GATE_AGENTS.len() - 2, "only {moved} of the gate's ants moved");

    let (seed_food, seed_home) = (gate_field(TO_FOOD), gate_field(TO_HOME));
    assert_ne!(to_food, seed_food, "the to-food layer never changed");
    assert_ne!(to_home, seed_home, "the to-home layer never changed");
}

/// Parse a fixture: `# key: value` header lines, an agent block, then one block per layer.
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

/// Relative, because Henad holds the field in `f32` and most engines hold it in `f64`.
const GATE_TOLERANCE: f32 = 1e-6;

fn close(got: f32, want: f32) -> bool {
    let scale = got.abs().max(want.abs()).max(1.0);
    (got - want).abs() <= GATE_TOLERANCE * scale
}

/// Henad against every engine that has committed a fixture.
///
/// The directory arrives with the first port. Until then there is nothing to check, and
/// `the_gate_scenario_does_not_depend_on_the_random_stream` is what holds the scenario itself.
#[test]
fn matches_every_reference_fixture() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ants");
    if !dir.exists() {
        return;
    }

    let mut checked = Vec::new();
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let (header, (agents, to_food, to_home)) = parse_gate_fixture(&text);
        let engine = header.get("engine").map_or("?", String::as_str);
        let steps: u32 = header
            .get("steps")
            .unwrap_or_else(|| panic!("{name} has no `steps` header"))
            .parse()
            .unwrap_or_else(|_| panic!("{name}: `steps` is not a number"));

        let (ours_agents, ours_food, ours_home) = gate_run(None, steps);
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
