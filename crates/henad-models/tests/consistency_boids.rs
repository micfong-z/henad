//! Cross-engine and self consistency check for boids.
//!
//! See `tests/fixtures/docs/boids_fixture.md` for how a fixture is produced.

use std::collections::HashMap;
use std::path::Path;

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_models::boids::BoidsModel;

/// `(x, y, vx, vy)`
type Agents = &'static [(f32, f32, f32, f32)];

/// 8 boids with exact binary representations.
const BOIDS_8: Agents = &[
    (10.0, 10.0, 1.0, 0.0),
    (13.0, 10.0, 1.0, 0.0),
    (30.0, 30.0, 0.0, 2.0),
    (42.0, 30.0, 2.0, 0.0),
    (75.0, 75.0, 0.5, 0.5),
    (2.0, 50.0, -3.0, 0.0),
    (98.0, 50.0, 3.0, 0.0),
    (60.0, 10.0, 6.0, 6.0),
];

/// 42 boids sampled along two sine periods, quantised to exact binary representations (1/8).
#[rustfmt::skip]
const SINE_42: Agents = &[
    (0.0, 61.125, 2.0, 0.0),
    (2.25, 73.5, 2.375, 2.875),
    (4.625, 83.75, -1.25, 5.375),
    (6.875, 91.0, -6.5, 3.125),
    (9.125, 94.625, -8.125, -3.875),
    (11.375, 94.25, -2.375, -10.5),
    (13.75, 90.0, 1.25, -1.625),
    (16.0, 82.25, 3.75, 0.0),
    (18.25, 71.5, 3.375, 4.25),
    (20.625, 58.875, -1.625, 7.125),
    (22.875, 45.5, -8.125, 3.875),
    (25.125, 32.5, -9.625, -4.625),
    (27.375, 21.0, -0.5, -2.0),
    (29.75, 12.125, 2.375, -2.875),
    (32.0, 6.625, 5.5, 0.0),
    (34.25, 5.0, 4.5, 5.625),
    (36.625, 7.375, -2.0, 8.75),
    (38.875, 13.5, -9.625, 4.625),
    (41.125, 22.875, -1.75, -0.875),
    (43.375, 34.625, -0.875, -3.625),
    (45.75, 47.75, 3.375, -4.25),
    (48.0, 61.125, 7.25, 0.0),
    (50.25, 73.5, 5.625, 7.0),
    (52.625, 83.75, -2.375, 10.5),
    (54.875, 91.0, -1.75, 0.875),
    (57.125, 94.625, -3.375, -1.625),
    (59.375, 94.25, -1.25, -5.375),
    (61.75, 90.0, 4.5, -5.625),
    (64.0, 82.25, 9.0, 0.0),
    (66.25, 71.5, 6.75, 8.375),
    (68.625, 58.875, -0.5, 2.0),
    (70.875, 45.5, -3.375, 1.625),
    (73.125, 32.5, -5.0, -2.375),
    (75.375, 21.0, -1.625, -7.125),
    (77.75, 12.125, 5.625, -7.0),
    (80.0, 6.625, 10.75, 0.0),
    (82.25, 5.0, 1.25, 1.625),
    (84.625, 7.375, -0.875, 3.625),
    (86.875, 13.5, -5.0, 2.375),
    (89.125, 22.875, -6.5, -3.125),
    (91.375, 34.625, -2.0, -8.75),
    (93.75, 47.75, 6.75, -8.375),
];

const SCENARIOS: &[(&str, Agents)] = &[("boids-8", BOIDS_8), ("sine-42", SINE_42)];

/// Resolves a fixture's `scenario` header to the agents it started from.
fn scenario(name: &str) -> Agents {
    let key = name.split_whitespace().next().unwrap_or_default();
    SCENARIOS
        .iter()
        .find(|(id, _)| *id == key)
        .map(|(_, agents)| *agents)
        .unwrap_or_else(|| panic!("fixture declares unknown scenario `{key}`"))
}

/// Where the reference fixtures live.
///
/// `HENAD_FIXTURE_DIR` points the gate at a directory holding one engine's candidates, so a
/// failure names that engine and no tracked fixture is written or removed to find out.
fn fixture_dir(model: &str) -> std::path::PathBuf {
    match std::env::var_os("HENAD_FIXTURE_DIR") {
        Some(root) => std::path::PathBuf::from(root).join(model),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(model),
    }
}

/// One tick, as `boids_fixture.md` declares for both scenarios.
///
/// Henad holds `f32` where the others hold `f64`, and the model is chaotic enough that a second
/// tick would put the two flocks past any useful tolerance.
const DECLARED_STEPS: u32 = 1;

const WORLD: f32 = 100.0;
const MAX_SPEED: f32 = 8.0;
const MIN_SPEED: f32 = 2.0;

fn params(agents: usize) -> Vec<ParamValue> {
    vec![
        ParamValue::U32(agents as u32),
        ParamValue::F32(WORLD),     // world_width
        ParamValue::F32(WORLD),     // world_height
        ParamValue::F32(20.0),      // visual_range
        ParamValue::F32(5.0),       // protected_range
        ParamValue::F32(0.5),       // separation
        ParamValue::F32(0.25),      // alignment
        ParamValue::F32(0.125),     // cohesion
        ParamValue::F32(MAX_SPEED), // max_speed
        ParamValue::F32(MIN_SPEED), // min_speed
    ]
}

fn run(agents: &[(f32, f32, f32, f32)], steps: u32) -> Vec<(f32, f32, f32, f32)> {
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

/// `f32` and `f64` are not bitwise identical, so we need a tolerance for the comparison.
const TOLERANCE: f32 = 1e-5;

struct Fixture {
    header: HashMap<String, String>,
    agents: Vec<(f32, f32, f32, f32)>,
}

/// Parses a fixture: `# key: value` header lines, then one `x y vx vy` row per agent.
fn parse_fixture(text: &str) -> Fixture {
    let mut header = HashMap::new();
    let mut agents = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some((k, v)) = rest.split_once(':') {
                header.insert(k.trim().to_lowercase(), v.trim().to_owned());
            }
            continue;
        }
        let vals: Vec<f32> = line
            .split_whitespace()
            .map(|t| t.parse().unwrap_or_else(|_| panic!("`{t}` is not a number")))
            .collect();
        assert_eq!(vals.len(), 4, "expected `x y vx vy`, got {} values", vals.len());
        agents.push((vals[0], vals[1], vals[2], vals[3]));
    }

    Fixture { header, agents }
}

/// Henad's agents against every reference within tolerance.
#[test]
fn matches_every_reference_fixture() {
    let dir = fixture_dir("boids");
    let mut seen: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }

        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
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
