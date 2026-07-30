use std::mem::swap;

use henad_core::helpers::{extract_f32, extract_u32, xorshift64};
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::spatial_hash::SpatialHash;
use henad_core::view::{PointView, StatEntry, StatValue};

pub const PALETTE: [[u8; 4]; 3] = [
    [0xE4, 0x37, 0x48, 0xFF], // Max speed - red
    [0xFF, 0xC1, 0x07, 0xFF], // Avg speed - yellow
    [0x00, 0x7A, 0xF5, 0xFF], // Min speed - blue
];

/// Agent colours by heading octant. Not by speed, which collapses to one colour once the flock
/// settles at `min_speed`. Cyclic, so a turning flock shifts hue instead of jumping.
pub const HEADING_PALETTE: [[u8; 4]; 8] = [
    [0xE4, 0x37, 0x48, 0xFF], // [0, 45)    E -> SE
    [0xF0, 0x7A, 0x28, 0xFF], // [45, 90)   SE -> S
    [0xFF, 0xC1, 0x07, 0xFF], // [90, 135)  S -> SW
    [0x8B, 0xC3, 0x4A, 0xFF], // [135, 180) SW -> W
    [0x1E, 0xA8, 0x7C, 0xFF], // [180, 225) W -> NW
    [0x00, 0x9E, 0xC8, 0xFF], // [225, 270) NW -> N
    [0x00, 0x7A, 0xF5, 0xFF], // [270, 315) N -> NE
    [0x8E, 0x54, 0xD8, 0xFF], // [315, 360) NE -> E
];

pub struct BoidsState {
    pub(crate) pos_x: Vec<f32>,
    pub(crate) pos_y: Vec<f32>,
    pub(crate) vel_x: Vec<f32>,
    pub(crate) vel_y: Vec<f32>,
    pub(crate) next_pos_x: Vec<f32>,
    pub(crate) next_pos_y: Vec<f32>,
    pub(crate) next_vel_x: Vec<f32>,
    pub(crate) next_vel_y: Vec<f32>,
    /// Heading octant per boid, indexing [`HEADING_PALETTE`]. Written in the same pass as `next_*`
    /// so it costs no extra pass. Not double-buffered, each boid only writes its own slot.
    pub(crate) color: Vec<u8>,
    pub(crate) hash: SpatialHash,
    pub(crate) world_w: f32,
    pub(crate) world_h: f32,
    pub(crate) num_boids: u32,
    pub(crate) visual_range: f32,
    pub(crate) protected_range: f32,
    pub(crate) separation_factor: f32,
    pub(crate) alignment_factor: f32,
    pub(crate) cohesion_factor: f32,
    pub(crate) max_speed: f32,
    pub(crate) min_speed: f32,
    pub(crate) tick: u64,
    pub(crate) rng_state: u64,
}

impl BoidsState {
    pub fn from_params(params: &[ParamValue]) -> Self {
        let num_boids = extract_u32(params, 0, 50_000);
        let world_w = extract_f32(params, 1, 1_000.0);
        let world_h = extract_f32(params, 2, 1_000.0);
        let visual_range = extract_f32(params, 3, 50.0);
        let protected_range = extract_f32(params, 4, 8.0);
        let separation_factor = extract_f32(params, 5, 0.05);
        let alignment_factor = extract_f32(params, 6, 0.05);
        let cohesion_factor = extract_f32(params, 7, 0.0005);
        let max_speed = extract_f32(params, 8, 15.0);
        let min_speed = extract_f32(params, 9, 3.0);

        let n = num_boids as usize;
        let hash = SpatialHash::new(visual_range, world_w, world_h);

        let mut state = Self {
            pos_x: vec![0.0; n],
            pos_y: vec![0.0; n],
            vel_x: vec![0.0; n],
            vel_y: vec![0.0; n],
            next_pos_x: vec![0.0; n],
            next_pos_y: vec![0.0; n],
            next_vel_x: vec![0.0; n],
            next_vel_y: vec![0.0; n],
            color: vec![0; n],
            hash,
            world_w,
            world_h,
            num_boids,
            visual_range,
            protected_range,
            separation_factor,
            alignment_factor,
            cohesion_factor,
            max_speed,
            min_speed,
            tick: 0,
            rng_state: 0xDEAD_BEEF_CAFE_1234,
        };

        state.initialize();
        state
    }

    fn initialize(&mut self) {
        let speed = 0.5 * (self.min_speed + self.max_speed);
        let inv_u32_range = 1.0f32 / (u32::MAX as f32 + 1.0);
        let mut rng = self.rng_state;

        for i in 0..self.num_boids as usize {
            rng = xorshift64(rng);
            let rx = ((rng >> 32) as u32) as f32 * inv_u32_range;

            rng = xorshift64(rng);
            let ry = ((rng >> 32) as u32) as f32 * inv_u32_range;

            self.pos_x[i] = rx * self.world_w;
            self.pos_y[i] = ry * self.world_h;

            rng = xorshift64(rng);
            let ra = ((rng >> 32) as u32) as f32 * inv_u32_range;
            let angle = ra * std::f32::consts::TAU;
            self.vel_x[i] = angle.cos() * speed;
            self.vel_y[i] = angle.sin() * speed;
            // The initial snapshot is published before any tick, so seed the lane here too.
            self.color[i] = super::step::heading_octant(self.vel_x[i], self.vel_y[i]);
        }
        self.rng_state = rng;
    }

    pub fn swap_buffers(&mut self) {
        swap(&mut self.pos_x, &mut self.next_pos_x);
        swap(&mut self.pos_y, &mut self.next_pos_y);
        swap(&mut self.vel_x, &mut self.next_vel_x);
        swap(&mut self.vel_y, &mut self.next_vel_y);
    }
}

/// Boids per rayon chunk in the stats reduction. See `crate::sir::STATS_CHUNK`.
const STATS_CHUNK: usize = 8192;

#[derive(Clone, Copy)]
struct VelSums {
    speed: f64,
    vx: f64,
    vy: f64,
}

impl VelSums {
    const ZERO: Self = Self {
        speed: 0.0,
        vx: 0.0,
        vy: 0.0,
    };

    fn add(self, other: Self) -> Self {
        Self {
            speed: self.speed + other.speed,
            vx: self.vx + other.vx,
            vy: self.vy + other.vy,
        }
    }
}

fn chunk_sums(vel_x: &[f32], vel_y: &[f32]) -> VelSums {
    let mut speed_sum = 0.0;
    let mut vx_sum = 0.0;
    let mut vy_sum = 0.0;
    for (&vx, &vy) in vel_x.iter().zip(vel_y.iter()) {
        #[expect(
            clippy::imprecise_flops,
            reason = "this won't overflow and we want to avoid extra casts"
        )]
        let speed = (vx * vx + vy * vy).sqrt();
        speed_sum += speed;
        vx_sum += f64::from(vx);
        vy_sum += f64::from(vy);
    }
    VelSums {
        speed: speed_sum as f64,
        vx: vx_sum,
        vy: vy_sum,
    }
}

/// Totals over every boid, summed chunk-by-chunk in index order so the result does not depend on
/// how rayon schedules the work, and is identical on the wasm path.
fn velocity_sums(vel_x: &[f32], vel_y: &[f32]) -> VelSums {
    #[cfg(not(target_arch = "wasm32"))]
    let partials: Vec<VelSums> = {
        use rayon::prelude::*;
        vel_x
            .par_chunks(STATS_CHUNK)
            .zip(vel_y.par_chunks(STATS_CHUNK))
            .map(|(xs, ys)| chunk_sums(xs, ys))
            .collect()
    };

    #[cfg(target_arch = "wasm32")]
    let partials: Vec<VelSums> = vel_x
        .chunks(STATS_CHUNK)
        .zip(vel_y.chunks(STATS_CHUNK))
        .map(|(xs, ys)| chunk_sums(xs, ys))
        .collect();

    partials.into_iter().fold(VelSums::ZERO, VelSums::add)
}

impl SimState for BoidsState {
    fn step(&mut self) {
        super::step::step(self);
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn point_view(&self) -> Option<PointView<'_>> {
        Some(PointView {
            pos_x: &self.pos_x,
            pos_y: &self.pos_y,
            world_w: self.world_w,
            world_h: self.world_h,
            color: Some(&self.color),
            palette: &HEADING_PALETTE,
        })
    }

    fn stats(&self) -> Vec<StatEntry> {
        let sums = velocity_sums(&self.vel_x, &self.vel_y);
        let inv = 1.0 / f64::from(self.num_boids.max(1));
        vec![
            StatEntry {
                label: "Average Speed",
                value: StatValue::Scalar(sums.speed * inv),
                color: PALETTE[1],
            },
            StatEntry {
                label: "Average Velocity",
                value: StatValue::Vector2D {
                    x: sums.vx * inv,
                    y: sums.vy * inv,
                },
                color: PALETTE[2],
            },
        ]
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        match (index, value) {
            (3, ParamValue::F32(v)) => {
                self.visual_range = *v;
                self.hash
                    .rebuild_with_cell_size(self.visual_range, &self.pos_x, &self.pos_y);
                true
            }
            (4, ParamValue::F32(v)) => {
                self.protected_range = *v;
                true
            }
            (5, ParamValue::F32(v)) => {
                self.separation_factor = *v;
                true
            }
            (6, ParamValue::F32(v)) => {
                self.alignment_factor = *v;
                true
            }
            (7, ParamValue::F32(v)) => {
                self.cohesion_factor = *v;
                true
            }
            (8, ParamValue::F32(v)) => {
                self.max_speed = *v;
                true
            }
            (9, ParamValue::F32(v)) => {
                self.min_speed = *v;
                true
            }
            _ => false,
        }
    }

    fn population(&self) -> u64 {
        self.num_boids as u64
    }

    fn heap_bytes(&self) -> usize {
        8 * 4 * self.pos_x.capacity() + self.color.capacity() + self.hash.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::{BoidsState, STATS_CHUNK};
    use henad_core::model::SimState as _;
    use henad_core::params::ParamValue;
    use henad_core::view::StatValue;

    /// The plain sequential `hypot` form the chunked reduction replaced.
    fn reference(vel_x: &[f32], vel_y: &[f32]) -> (f64, f64, f64) {
        let (mut speed, mut vx, mut vy) = (0.0, 0.0, 0.0);
        for (&x, &y) in vel_x.iter().zip(vel_y.iter()) {
            speed += f64::from(x.hypot(y));
            vx += f64::from(x);
            vy += f64::from(y);
        }
        (speed, vx, vy)
    }

    /// Deliberately not a multiple of `STATS_CHUNK`, so the ragged final chunk is covered.
    fn state_spanning_several_chunks() -> BoidsState {
        let n = STATS_CHUNK as u32 * 2 + 37;
        let params = vec![ParamValue::U32(n), ParamValue::F32(4_000.0), ParamValue::F32(4_000.0)];
        let mut state = BoidsState::from_params(&params);
        for _ in 0..5 {
            state.step();
        }
        state
    }

    #[test]
    fn stats_match_the_sequential_reference() {
        let state = state_spanning_several_chunks();
        let (speed, vx, vy) = reference(&state.vel_x, &state.vel_y);
        let inv = 1.0 / f64::from(state.num_boids);
        let stats = state.stats();

        let StatValue::Scalar(avg_speed) = stats[0].value else {
            panic!("average speed is a scalar");
        };
        let StatValue::Vector2D { x, y } = stats[1].value else {
            panic!("average velocity is a vector");
        };

        // f32 speed accumulation within a chunk is the loosest term here.
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-4 * b.abs().max(1.0);
        assert!(close(avg_speed, speed * inv), "speed {avg_speed} vs {}", speed * inv);
        assert!(close(x, vx * inv), "vx {x} vs {}", vx * inv);
        assert!(close(y, vy * inv), "vy {y} vs {}", vy * inv);
    }

    /// Boids move, so a stale cached average would show up as stats that never change.
    #[test]
    fn stats_track_the_current_velocities() {
        let mut state = state_spanning_several_chunks();
        let before = state.stats();
        for _ in 0..20 {
            state.step();
        }
        let after = state.stats();

        let (StatValue::Vector2D { x: bx, .. }, StatValue::Vector2D { x: ax, .. }) =
            (&before[1].value, &after[1].value)
        else {
            panic!("average velocity is a vector");
        };
        assert!(
            (ax - bx).abs() > f64::EPSILON,
            "average velocity never moved: {bx} then {ax}"
        );
    }
}
