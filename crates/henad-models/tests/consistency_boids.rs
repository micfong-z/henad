//! Cross-engine and self consistency check for boids.
//!
//! See `tests/fixtures/docs/boids_fixture.md` for how a fixture is produced.

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_models::boids::BoidsModel;

mod support;

#[path = "gates/boids.rs"]
mod gate;

use gate::{Agents, Boid, MAX_SPEED, MIN_SPEED, SCENARIOS, TOLERANCE, WORLD, parse_fixture};
use support::{fixture_dir, reference_fixtures};

/// Resolves a fixture's `scenario` header to the agents it started from.
fn scenario(name: &str) -> Agents {
    let key = name.split_whitespace().next().unwrap_or_default();
    SCENARIOS
        .iter()
        .find(|(id, _)| *id == key)
        .map(|(_, agents)| *agents)
        .unwrap_or_else(|| panic!("fixture declares unknown scenario `{key}`"))
}

/// One tick, as `boids_fixture.md` declares for both scenarios.
///
/// Henad holds `f32` where the other engines hold `f64`, and the model is chaotic enough that a second
/// tick would put the two flocks past any useful tolerance.
const DECLARED_STEPS: u32 = 1;

/// Returns the parameters of the stock boids in the gate, in the order of its schema.
fn params(agents: usize) -> Vec<ParamValue> {
    let values = gate::gate_values(agents);
    let models = henad_models::example_models();
    let entry = models.get("boids").expect("the stock boids");
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            values
                .iter()
                .find(|(id, _)| *id == descriptor.id())
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("the gate sets no {}", descriptor.id()))
        })
        .collect()
}

fn run(agents: &[Boid], steps: u32) -> Vec<Boid> {
    let p = params(agents.len());
    let mut state = AgentModelState::<BoidsModel>::from_agents(&p, |lanes, _extent| {
        for (i, &(x, y, vx, vy)) in agents.iter().enumerate() {
            lanes.pos_x[i] = x;
            lanes.pos_y[i] = y;
            lanes.vel_x[i] = vx;
            lanes.vel_y[i] = vy;
        }
    });
    for _ in 0..steps {
        state.step();
    }
    let l = state.lanes();
    (0..agents.len())
        .map(|i| (l.pos_x[i], l.pos_y[i], l.vel_x[i], l.vel_y[i]))
        .collect()
}

/// Checks Henad's agents against every reference, within tolerance.
#[test]
fn matches_every_reference_fixture() {
    let dir = fixture_dir("boids");
    let mut seen: Vec<String> = Vec::new();

    for (name, text) in reference_fixtures(&dir) {
        let fixture = parse_fixture(&text);
        let engine = fixture.header.get("engine").map_or("?", String::as_str);
        let steps: u32 = fixture
            .header
            .get("steps")
            .unwrap_or_else(|| panic!("{name} has no `steps` header"))
            .parse()
            .unwrap_or_else(|_| panic!("{name}: `steps` is not a number"));

        let which = fixture
            .header
            .get("scenario")
            .unwrap_or_else(|| panic!("{name} has no `scenario` header"));
        let agents = scenario(which);
        assert_eq!(
            steps, DECLARED_STEPS,
            "{name} was recorded at {steps} ticks where `{which}` declares {DECLARED_STEPS}"
        );
        if let Some(world) = fixture.header.get("world") {
            assert_eq!(
                world.parse::<f32>().ok(),
                Some(WORLD),
                "{name} declares world {world} where the scenario is {WORLD}"
            );
        }

        assert_eq!(
            fixture.agents.len(),
            agents.len(),
            "{name} has {} agents, scenario `{which}` has {}",
            fixture.agents.len(),
            agents.len()
        );

        let ours = run(agents, steps);
        for (i, (got, want)) in ours.iter().zip(&fixture.agents).enumerate() {
            for (label, g, w) in [
                ("pos_x", got.0, want.0),
                ("pos_y", got.1, want.1),
                ("vel_x", got.2, want.2),
                ("vel_y", got.3, want.3),
            ] {
                assert!(
                    (g - w).abs() <= TOLERANCE,
                    "agent {i} {label}: Henad {g}, {engine} {w} (diff {}) after {steps} step(s) [{name}, {which}]",
                    (g - w).abs()
                );
            }
        }
        seen.push(which.clone());
    }

    assert!(!seen.is_empty(), "no fixtures found in {}", dir.display());
    for (id, _) in SCENARIOS {
        assert!(
            seen.iter().any(|s| s.split_whitespace().next() == Some(*id)),
            "scenario `{id}` has no fixture in {}, so it is never actually run",
            dir.display()
        );
    }
}

/// Speed is clamped into `[min_speed, max_speed]` on every tick, and positions stay inside the world.
///
/// This is a self-consistency check.
#[test]
fn speed_and_position_stay_in_bounds() {
    let mut p = params(500);
    p[0] = ParamValue::U32(500);
    let mut state = AgentModelState::<BoidsModel>::from_params(&p);

    for tick in 0..50 {
        state.step();
        let l = state.lanes();
        for i in 0..500 {
            let speed = l.vel_x[i].hypot(l.vel_y[i]);
            assert!(
                (MIN_SPEED - TOLERANCE..=MAX_SPEED + TOLERANCE).contains(&speed),
                "agent {i} speed {speed} outside [{MIN_SPEED}, {MAX_SPEED}] at tick {tick}"
            );
            assert!(
                (0.0..WORLD).contains(&l.pos_x[i]) && (0.0..WORLD).contains(&l.pos_y[i]),
                "agent {i} at ({}, {}) left the world at tick {tick}",
                l.pos_x[i],
                l.pos_y[i]
            );
        }
    }
}
