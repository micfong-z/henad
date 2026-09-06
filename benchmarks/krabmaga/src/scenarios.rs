//! Starting states for the gate scenarios, as the declarations give them.
//!
//! Initial conditions are shared rather than reimplemented. Both engines have to begin from the
//! same numbers for the comparison to mean anything. It is the rules that are written
//! independently.
//!
//! Taken from `crates/henad-models/tests/fixtures/docs/`, and identical to the tables in
//! `consistency_boids.rs` and `consistency_ants.rs`.

/// `(x, y, vx, vy)`, world 100, one tick.
pub const BOIDS_8: [(f32, f32, f32, f32); 8] = [
    (10.0, 10.0, 1.0, 0.0),
    (13.0, 10.0, 1.0, 0.0),
    (30.0, 30.0, 0.0, 2.0),
    (42.0, 30.0, 2.0, 0.0),
    (75.0, 75.0, 0.5, 0.5),
    (2.0, 50.0, -3.0, 0.0),
    (98.0, 50.0, 3.0, 0.0),
    (60.0, 10.0, 6.0, 6.0),
];

pub const SINE_42: [(f32, f32, f32, f32); 42] = [
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

/// `visual_range`, `protected_range`, `separation`, `alignment`, `cohesion`, `max_speed`,
/// `min_speed`, shared by both boids scenarios.
pub const BOIDS_WORLD: f32 = 100.0;
pub const BOIDS_VISUAL_RANGE: f32 = 20.0;
pub const BOIDS_PROTECTED_RANGE: f32 = 5.0;
pub const BOIDS_SEPARATION: f32 = 0.5;
pub const BOIDS_ALIGNMENT: f32 = 0.25;
pub const BOIDS_COHESION: f32 = 0.125;
pub const BOIDS_MAX_SPEED: f32 = 8.0;
pub const BOIDS_MIN_SPEED: f32 = 2.0;

/// `(x, y, last_step, has_food, reward)` on a 32 by 32 lattice, five ticks.
pub const ANTS_AGENTS: [(i32, i32, u8, u8, f32); 12] = [
    (0, 0, 255, 0, 1.0),
    (31, 0, 255, 1, 1.0),
    (0, 31, 255, 0, 0.5),
    (31, 31, 255, 1, 0.5),
    (16, 0, 255, 0, 1.0),
    (4, 4, 255, 0, 1.0),
    (28, 28, 255, 1, 1.0),
    (8, 12, 255, 0, 1.0),
    (22, 20, 255, 1, 1.0),
    (15, 15, 7, 0, 1.0),
    (15, 15, 255, 0, 0.25),
    (15, 16, 2, 1, 1.0),
];

pub const ANTS_WORLD: i32 = 32;
pub const ANTS_CUTDOWN: f32 = 0.9;
pub const ANTS_REWARD: f32 = 1.0;
pub const ANTS_MOMENTUM: f64 = 0.0;
pub const ANTS_RANDOM_ACTION: f64 = 0.0;
pub const ANTS_EVAPORATION: f32 = 0.999;
pub const ANTS_STEPS: u32 = 4;

/// Distinct within every 3 by 3 neighbourhood, so no tie is reached and no draw is taken.
pub fn ants_field(x: i32, y: i32, to_food: bool) -> f32 {
    let (a, b, m) = if to_food { (7, 13, 97) } else { (11, 5, 89) };
    ((a * x + b * y).rem_euclid(m) + 1) as f32 / (m + 1) as f32
}

/// Cells alive at the start, as `(x, y)` offsets placed at the origin of a 64 by 64 torus.
pub const GLIDER: [(i32, i32); 5] = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
pub const R_PENTOMINO: [(i32, i32); 5] = [(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];
pub const LIFE_WORLD: i32 = 64;

/// SIR runs at the declaration's parameters rather than Henad's defaults. See `sir_fixture.md`.
pub const SIR_WORLD: i32 = 256;
pub const SIR_INFECTION_RATE: f64 = 0.08;
pub const SIR_RECOVERY_RATE: f64 = 0.3;
pub const SIR_INITIAL_INFECTED: f64 = 0.01;
pub const SIR_TICKS: u32 = 300;
