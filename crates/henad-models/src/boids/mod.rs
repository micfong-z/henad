mod lanes;
mod step;

use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_core::authoring::model::agent_model::{AgentModel, StepCtx};
use henad_core::authoring::model::field::{Extent, NoField};
use henad_core::authoring::primitives::rng::xorshift64;
use henad_core::helpers::{extract_f32, f32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::spatial_hash::SpatialHash;
use henad_core::view::{StatDescriptor, StatValue};

pub use crate::boids::lanes::{BoidChunk, BoidLanes, BoidRead};

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

henad_core::params! {
    const VISUAL_RANGE = f32_param("visual_range", "Visual Range", 50.0, 1.0, 200.0, Some(1.0));
    const PROTECTED_RANGE = f32_param("protected_range", "Protected Range", 8.0, 0.5, 50.0, Some(0.5));
    const SEPARATION = f32_param("separation", "Separation", 0.05, 0.0, 2.0, Some(0.01));
    const ALIGNMENT = f32_param("alignment", "Alignment", 0.05, 0.0, 2.0, Some(0.01));
    const COHESION = f32_param("cohesion", "Cohesion", 0.0005, 0.0, 0.01, Some(0.0001));
    const MAX_SPEED = f32_param("max_speed", "Max Speed", 15.0, 1.0, 50.0, Some(0.5));
    const MIN_SPEED = f32_param("min_speed", "Min Speed", 3.0, 0.5, 20.0, Some(0.5));
}

pub struct BoidsModel;

/// Squared ranges and half extents precomputed, so the inner loop does no setup per neighbour.
pub struct BoidParams {
    pub visual_range: f32,
    pub visual_sq: f32,
    pub protected_sq: f32,
    pub separation_factor: f32,
    pub alignment_factor: f32,
    pub cohesion_factor: f32,
    pub max_speed: f32,
    pub min_speed: f32,
    pub half_w: f32,
    pub half_h: f32,
    pub world_w: f32,
    pub world_h: f32,
}

impl AgentModel for BoidsModel {
    const NAME: &'static str = "Boids Flocking";
    const ID: &'static str = "boids";
    const DESCRIPTION: &'static str = "A simulation of flocking behavior in a group of boids.";
    const PALETTE: &'static [[u8; 4]] = &HEADING_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Average Speed", PALETTE[1]),
        StatDescriptor::new("Average Velocity", PALETTE[2]),
    ];
    const DEFAULT_AGENTS: u32 = 50_000;
    const MAX_AGENTS: u32 = 1_000_000;
    const DEFAULT_EXTENT: Extent = Extent { w: 1_000.0, h: 1_000.0 };

    type Lanes = BoidLanes;
    type Field = NoField;
    type Index = SpatialHash;
    type Params = BoidParams;
    type Tally = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue], extent: Extent) -> BoidParams {
        let visual_range = extract_f32(params, VISUAL_RANGE, 50.0);
        let protected_range = extract_f32(params, PROTECTED_RANGE, 8.0);
        let (world_w, world_h) = (extent.w, extent.h);
        BoidParams {
            visual_range,
            visual_sq: visual_range * visual_range,
            protected_sq: protected_range * protected_range,
            separation_factor: extract_f32(params, SEPARATION, 0.05),
            alignment_factor: extract_f32(params, ALIGNMENT, 0.05),
            cohesion_factor: extract_f32(params, COHESION, 0.0005),
            max_speed: extract_f32(params, MAX_SPEED, 15.0),
            min_speed: extract_f32(params, MIN_SPEED, 3.0),
            half_w: 0.5 * world_w,
            half_h: 0.5 * world_h,
            world_w,
            world_h,
        }
    }

    fn index_cell_size(params: &BoidParams) -> f32 {
        params.visual_range
    }

    fn init(lanes: &mut BoidLanes, extent: Extent, params: &[ParamValue], rng: &mut u64) {
        let max_speed = extract_f32(params, MAX_SPEED, 15.0);
        let min_speed = extract_f32(params, MIN_SPEED, 3.0);
        let speed = 0.5 * (min_speed + max_speed);
        let inv_u32_range = 1.0f32 / (u32::MAX as f32 + 1.0);
        let mut unit = || {
            *rng = xorshift64(*rng);
            ((*rng >> 32) as u32) as f32 * inv_u32_range
        };

        for i in 0..lanes.pos_x.len() {
            lanes.pos_x[i] = unit() * extent.w;
            lanes.pos_y[i] = unit() * extent.h;
            let angle = unit() * std::f32::consts::TAU;
            lanes.vel_x[i] = angle.cos() * speed;
            lanes.vel_y[i] = angle.sin() * speed;
            // The initial snapshot is published before any tick, so seed the lane here too.
            lanes.color[i] = step::heading_octant(lanes.vel_x[i], lanes.vel_y[i]);
        }
    }

    fn run_step_pass(lanes: &mut BoidLanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) {
        step::run(lanes, ctx, seed, tick);
    }

    fn stats(lanes: &BoidLanes, _field: &NoField, (): &()) -> Vec<StatValue> {
        let sums = velocity_sums(&lanes.vel_x, &lanes.vel_y);
        let inv = 1.0 / (lanes.vel_x.len().max(1) as f64);
        vec![
            StatValue::Scalar(sums.speed * inv),
            StatValue::Vector2D {
                x: sums.vx * inv,
                y: sums.vy * inv,
            },
        ]
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_core::model::SimState as _;
    use henad_core::view::StatValue;

    type State = AgentModelState<BoidsModel>;

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
    fn state_spanning_several_chunks() -> State {
        let n = STATS_CHUNK as u32 * 2 + 37;
        let params = vec![ParamValue::U32(n), ParamValue::F32(4_000.0), ParamValue::F32(4_000.0)];
        let mut state = State::from_params(&params);
        for _ in 0..5 {
            state.step();
        }
        state
    }

    #[test]
    fn stats_match_the_sequential_reference() {
        let state = state_spanning_several_chunks();
        let lanes = state.lanes();
        let (speed, vx, vy) = reference(&lanes.vel_x, &lanes.vel_y);
        let inv = 1.0 / lanes.vel_x.len() as f64;
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

    /// The chunk seed comes from the chunk index, so the flock must not depend on how rayon
    /// splits the work.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        fn run(threads: usize) -> Vec<u32> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let params = vec![ParamValue::U32(20_000), ParamValue::F32(800.0), ParamValue::F32(800.0)];
                let mut state = State::from_params(&params);
                for _ in 0..30 {
                    state.step();
                }
                let lanes = state.lanes();
                lanes
                    .pos_x
                    .iter()
                    .zip(&lanes.pos_y)
                    .flat_map(|(x, y)| [x.to_bits(), y.to_bits()])
                    .collect()
            })
        }
        assert_eq!(run(1), run(7), "boid positions depend on the thread count");
    }

    /// The engine owns the world extent, so the point view must report it rather than a lane range.
    #[test]
    fn point_view_reports_the_engine_extent() {
        let params = vec![ParamValue::U32(64), ParamValue::F32(512.0), ParamValue::F32(256.0)];
        let state = State::from_params(&params);
        let view = state.point_view().expect("boids draw an agent layer");
        assert_eq!((view.world_w, view.world_h), (512.0, 256.0));
        assert!(state.grid_view().is_none(), "boids have no field layer");
    }

    #[test]
    fn from_agents_initializes_velocities_from_model_params() {
        let params = vec![
            ParamValue::U32(1),
            ParamValue::F32(100.0),
            ParamValue::F32(100.0),
            ParamValue::F32(20.0),
            ParamValue::F32(5.0),
            ParamValue::F32(0.5),
            ParamValue::F32(0.25),
            ParamValue::F32(0.125),
            ParamValue::F32(8.0),
            ParamValue::F32(2.0),
        ];
        let state = State::from_agents(&params, |lanes, _extent| {
            lanes.pos_x[0] = 50.0;
            lanes.pos_y[0] = 50.0;
        });
        let lanes = state.lanes();

        // Midpoint of the configured band, not an exact compare: the speed goes through
        // `sin`/`cos`, whose last bit is a libm detail and so varies by platform.
        let speed = lanes.vel_x[0].hypot(lanes.vel_y[0]);
        assert!(
            (speed - 5.0).abs() < 1e-4,
            "initial speed should be 0.5 * (min_speed + max_speed), got {speed}"
        );
    }
}

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

/// Totals over every boid, summed chunk by chunk in index order so the result does not depend on
/// how rayon schedules the work.
fn velocity_sums(vel_x: &[f32], vel_y: &[f32]) -> VelSums {
    reduce_chunks(
        vel_x.len(),
        STATS_CHUNK,
        |r| {
            let mut speed_sum = 0.0f32;
            let mut vx_sum = 0.0;
            let mut vy_sum = 0.0;
            for (&vx, &vy) in vel_x[r.clone()].iter().zip(vel_y[r].iter()) {
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
                speed: f64::from(speed_sum),
                vx: vx_sum,
                vy: vy_sum,
            }
        },
        VelSums::add,
        VelSums::ZERO,
    )
}
