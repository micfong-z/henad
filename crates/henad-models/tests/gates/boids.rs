//! The cross-engine gate of the boids: the two scenarios every engine starts from, the parameters it runs them at and
//! the reader of a reference fixture.
//!
//! `tests/consistency_boids.rs` holds the stock CPU boids to the gate, and `src/tests/graph_agent_parity.rs` the boids
//! graphs on the device. `tests/fixtures/docs/boids_fixture.md` holds how a fixture is produced.

use std::collections::HashMap;

use henad_core::params::ParamValue;

/// One boid as `(x, y, vx, vy)`.
pub type Boid = (f32, f32, f32, f32);

/// Boids of a scenario.
pub type Agents = &'static [Boid];

/// 8 boids with exact binary representations.
pub const BOIDS_8: Agents = &[
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
pub const SINE_42: Agents = &[
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

/// Scenarios of the gate, by the name a fixture's `scenario` header starts with.
pub const SCENARIOS: &[(&str, Agents)] = &[("boids-8", BOIDS_8), ("sine-42", SINE_42)];

/// Side of the gate's square world.
pub const WORLD: f32 = 100.0;
/// Largest speed of a boid in the gate.
pub const MAX_SPEED: f32 = 8.0;
/// Smallest speed of a boid in the gate.
pub const MIN_SPEED: f32 = 2.0;

/// Tolerance of the comparison, since Henad holds `f32` where the references hold `f64`.
pub const TOLERANCE: f32 = 1e-5;

/// Returns the gate's parameters for `agents` boids, by id.
pub fn gate_values(agents: usize) -> Vec<(&'static str, ParamValue)> {
    vec![
        ("num_agents", ParamValue::U32(agents as u32)),
        ("world_width", ParamValue::F32(WORLD)),
        ("world_height", ParamValue::F32(WORLD)),
        ("visual_range", ParamValue::F32(20.0)),
        ("protected_range", ParamValue::F32(5.0)),
        ("separation", ParamValue::F32(0.5)),
        ("alignment", ParamValue::F32(0.25)),
        ("cohesion", ParamValue::F32(0.125)),
        ("max_speed", ParamValue::F32(MAX_SPEED)),
        ("min_speed", ParamValue::F32(MIN_SPEED)),
    ]
}

/// Reference fixture of one engine: its header and its boids after the declared ticks.
pub struct Fixture {
    /// Header lines by key, the key lowercased.
    pub header: HashMap<String, String>,
    /// Boids in the order of the scenario.
    pub agents: Vec<Boid>,
}

/// Parses a fixture: `# key: value` header lines, then one `x y vx vy` row per agent.
pub fn parse_fixture(text: &str) -> Fixture {
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
