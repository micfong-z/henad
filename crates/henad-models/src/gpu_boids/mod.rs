//! GPU boids, [`crate::boids`] with its population in GPU buffers.
//!
//! Tick 0 is bit identical since seeding goes through [`BoidsModel::init`]. After that, the
//! neighbour index does not fix the order within a cell, so trajectories are likely different.

use henad_compute::cpu::agent_engine::{AGENT_INIT_SEED, agent_model_param_descriptors};
use henad_core::authoring::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::field::Extent;
use henad_core::authoring::gpu_agent_model::{
    Binding, BufferSpec, Domain, Geometry, GpuAgentModel, PassCtx, PassId, PassSpec,
};
use henad_core::helpers::{extract_f32, extract_u32, mix_seed};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use crate::boids::{BoidLanes, BoidsModel, HEADING_PALETTE};
use crate::shader_bindings::gpu_boids::step::Params as StepParams;

/// The list is [`agent_model_param_descriptors`] for [`BoidsModel`] verbatim, so both backends
/// take the same vector. Only these three are read here, the rest go through
/// [`BoidsModel::from_params`].
const PARAM_NUM_AGENTS: usize = 0;
const PARAM_WORLD_WIDTH: usize = 1;
const PARAM_WORLD_HEIGHT: usize = 2;

/// Indices into [`GpuBoids::BUFFERS`].
const POS: usize = 0;
const VEL: usize = 1;
const COLOR: usize = 2;

/// Matches `Params` in the generated reduce leaf.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ReduceParams {
    n: u32,
    lanes: u32,
    groups_x: u32,
    _pad: u32,
}

pub struct GpuBoids;

impl GpuAgentModel for GpuBoids {
    const NAME: &'static str = "Boids Flocking (GPU)";
    const ID: &'static str = "gpu_boids";
    const DESCRIPTION: &'static str =
        "A simulation of flocking behavior in a group of boids, stepped entirely on the GPU";

    const STATS: &'static [StatDescriptor] = BoidsModel::STATS;

    /// All double buffered, since a boid reads its neighbours' current values while writing its
    /// own next ones.
    const BUFFERS: &'static [BufferSpec] = &[
        BufferSpec {
            label: "pos",
            double_buffered: true,
            drawable: true,
        },
        BufferSpec {
            label: "vel",
            double_buffered: true,
            drawable: false,
        },
        BufferSpec {
            label: "color",
            double_buffered: true,
            drawable: true,
        },
    ];
    const POS_BUFFER: usize = POS;
    const COLOR_BUFFER: usize = COLOR;

    const INDEX: bool = true;

    const STEP_PASSES: &'static [PassSpec] = &[PassSpec {
        label: "step",
        shader: crate::shader_bindings::gpu_boids::step::SHADER_STRING,
        bindings: &[
            Binding::Read(POS),
            Binding::Read(VEL),
            Binding::Write(POS),
            Binding::Write(VEL),
            Binding::Write(COLOR),
            Binding::IndexCellStart,
            Binding::IndexSorted,
            Binding::Uniform,
        ],
        domain: Domain::Agents,
    }];

    /// Speed, x velocity, y velocity.
    const REDUCE_LANES: usize = 3;
    const REDUCE_BINDINGS: &'static [Binding] = &[Binding::Read(VEL), Binding::ReducePartials, Binding::Uniform];
    const REDUCE_HEADER: &'static str = r"
struct Params {
    n: u32,
    lanes: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> vel: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;
";
    /// The `sqrt` is per agent, as in `boids::velocity_sums`. Mean speed rebuilt from mean
    /// velocity is a different quantity for a turning flock.
    const REDUCE_VALUE: &'static str = r"
        if (i < params.n) {
            let v = vel[i];
            if (lane == 0u) {
                value = length(v);
            } else if (lane == 1u) {
                value = v.x;
            } else {
                value = v.y;
            }
        }
";

    fn param_descriptors() -> Vec<ParamDescriptor> {
        agent_model_param_descriptors::<BoidsModel>()
    }

    fn dims(params: &[ParamValue]) -> (u32, Extent) {
        (
            extract_u32(params, PARAM_NUM_AGENTS, BoidsModel::DEFAULT_AGENTS),
            Extent {
                w: extract_f32(params, PARAM_WORLD_WIDTH, BoidsModel::DEFAULT_EXTENT.w),
                h: extract_f32(params, PARAM_WORLD_HEIGHT, BoidsModel::DEFAULT_EXTENT.h),
            },
        )
    }

    fn buffer_lens(geom: &Geometry) -> Vec<usize> {
        let n = geom.num_agents as usize;
        vec![n * 2, n * 2, n]
    }

    fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        let n = geom.num_agents as usize;

        // Seeding through the model's own `init` is what keeps tick 0 bit identical. A port
        // would be free to drift.
        let mut lanes = BoidLanes::alloc(n);
        let mut rng = seed.map_or(AGENT_INIT_SEED, mix_seed);
        BoidsModel::init(&mut lanes, geom.extent, params, &mut rng);

        // The CPU lane holds palette indices, this one is drawn directly so it holds colours.
        let colors: Vec<u32> = lanes.color.iter().map(|&c| packed_palette_color(c)).collect();
        vec![
            bytemuck::cast_slice(&interleave(&lanes.pos_x, &lanes.pos_y)).to_vec(),
            bytemuck::cast_slice(&interleave(&lanes.vel_x, &lanes.vel_y)).to_vec(),
            bytemuck::cast_slice(&colors).to_vec(),
        ]
    }

    fn index_cell_size(params: &[ParamValue]) -> f32 {
        BoidsModel::index_cell_size(&BoidsModel::from_params(params))
    }

    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        if pass == PassId::Reduce {
            return bytemuck::bytes_of(&ReduceParams {
                n: ctx.invocations,
                lanes: Self::REDUCE_LANES as u32,
                groups_x: ctx.groups_x,
                _pad: 0,
            })
            .to_vec();
        }

        let hot = BoidsModel::from_params(params);
        let grid = ctx.geom.index.expect("INDEX is declared");
        bytemuck::bytes_of(&StepParams {
            num_agents: ctx.geom.num_agents,
            groups_x: ctx.groups_x,
            grid_w: grid.grid_w,
            grid_h: grid.grid_h,
            cell_w: grid.cell_w,
            cell_h: grid.cell_h,
            cell_w_inv: 1.0 / grid.cell_w,
            cell_h_inv: 1.0 / grid.cell_h,
            world_w: hot.world_w,
            world_h: hot.world_h,
            half_w: hot.half_w,
            half_h: hot.half_h,
            visual_range: hot.visual_range,
            visual_sq: hot.visual_sq,
            protected_sq: hot.protected_sq,
            separation: hot.separation_factor,
            alignment: hot.alignment_factor,
            cohesion: hot.cohesion_factor,
            max_speed: hot.max_speed,
            min_speed: hot.min_speed,
            palette: packed_heading_palette(),
        })
        .to_vec()
    }

    fn stats(sums: &[f32], _counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
        let inv = 1.0 / f64::from(geom.num_agents.max(1));
        vec![
            StatValue::Scalar(f64::from(sums[0]) * inv),
            StatValue::Vector2D {
                x: f64::from(sums[1]) * inv,
                y: f64::from(sums[2]) * inv,
            },
        ]
    }
}

/// Into the `vec2` layout the shaders read.
fn interleave(xs: &[f32], ys: &[f32]) -> Vec<f32> {
    xs.iter().zip(ys).flat_map(|(&x, &y)| [x, y]).collect()
}

/// Packed for the step uniform, from the one palette in `boids` so colours cannot drift.
fn packed_heading_palette() -> [[u32; 4]; 2] {
    let mut packed = [[0u32; 4]; 2];
    for (i, rgba) in HEADING_PALETTE.iter().enumerate() {
        packed[i / 4][i % 4] = u32::from_le_bytes(*rgba);
    }
    packed
}

/// Same packing the CPU agent upload uses.
fn packed_palette_color(index: u8) -> u32 {
    let rgba = HEADING_PALETTE
        .get(index as usize)
        .copied()
        .unwrap_or(HEADING_PALETTE[0]);
    u32::from_le_bytes(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_compute::gpu::{GpuAgentState, GpuContext};
    use henad_core::model::SimState as _;

    type State = GpuAgentState<GpuBoids>;

    fn headless_context() -> Option<GpuContext> {
        crate::gpu_test_support::headless_context("gpu_boids_test_device", wgpu::Features::empty())
    }

    fn params(num_agents: u32, world: f32) -> Vec<ParamValue> {
        let mut values: Vec<ParamValue> = GpuBoids::param_descriptors()
            .iter()
            .map(|desc| desc.kind.default_value())
            .collect();
        values[PARAM_NUM_AGENTS] = ParamValue::U32(num_agents);
        values[PARAM_WORLD_WIDTH] = ParamValue::F32(world);
        values[PARAM_WORLD_HEIGHT] = ParamValue::F32(world);
        values
    }

    /// Back into the two scalar lanes the CPU model keeps.
    fn split(interleaved: &[u32]) -> (Vec<f32>, Vec<f32>) {
        let floats: Vec<f32> = interleaved.iter().map(|&w| f32::from_bits(w)).collect();
        (
            floats.iter().step_by(2).copied().collect(),
            floats.iter().skip(1).step_by(2).copied().collect(),
        )
    }

    /// Current-side positions and velocities, as `(pos_x, pos_y, vel_x, vel_y)`.
    fn lanes(state: &State) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let (pos_x, pos_y) = split(&state.read_buffer(POS));
        let (vel_x, vel_y) = split(&state.read_buffer(VEL));
        (pos_x, pos_y, vel_x, vel_y)
    }

    /// A boid drifting out of the world would break both the renderer and the index.
    #[test]
    fn boids_stay_inside_the_toroidal_world() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping boids_stay_inside_the_toroidal_world: no adapter");
            return;
        };

        let world = 800.0f32;
        let mut state = State::new(&ctx, &params(4_000, world));
        state.run_batched(60);

        let (pos_x, pos_y, _, _) = lanes(&state);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "boid {i} left the world at ({x}, {y})"
            );
        }
    }

    /// The speed clamp divides by a value that can be zero.
    #[test]
    fn speeds_stay_within_the_configured_band() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping speeds_stay_within_the_configured_band: no adapter");
            return;
        };

        let values = params(4_000, 800.0);
        // Through the model's own extraction, so the test cannot disagree with the kernel.
        let hot = BoidsModel::from_params(&values);
        let (max_speed, min_speed) = (hot.max_speed, hot.min_speed);

        let mut state = State::new(&ctx, &values);
        state.run_batched(60);

        let (_, _, vel_x, vel_y) = lanes(&state);
        for (i, (&vx, &vy)) in vel_x.iter().zip(&vel_y).enumerate() {
            let speed = vx.hypot(vy);
            // The clamp is applied in `f32`, so allow a last-bit tolerance on both ends.
            assert!(
                speed <= max_speed * 1.001 && speed >= min_speed * 0.999,
                "boid {i} has speed {speed}, outside [{min_speed}, {max_speed}]"
            );
        }
    }

    /// Both backends seed through `BoidsModel::init`, so any later divergence is the step's.
    #[test]
    fn initial_flock_matches_the_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping initial_flock_matches_the_cpu_model: no adapter");
            return;
        };

        let values = params(2_000, 800.0);
        let gpu = State::new(&ctx, &values);
        let cpu = AgentModelState::<BoidsModel>::from_params(&values);

        let (pos_x, pos_y, vel_x, vel_y) = lanes(&gpu);
        let cpu_lanes = cpu.lanes();
        assert_eq!(pos_x, cpu_lanes.pos_x, "initial x positions must match the CPU model");
        assert_eq!(pos_y, cpu_lanes.pos_y, "initial y positions must match the CPU model");
        assert_eq!(vel_x, cpu_lanes.vel_x, "initial x velocities must match the CPU model");
        assert_eq!(vel_y, cpu_lanes.vel_y, "initial y velocities must match the CPU model");
    }

    /// Catches a reduction that lost or double counted a workgroup.
    #[test]
    fn average_speed_is_consistent_with_the_population() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping average_speed_is_consistent_with_the_population: no adapter");
            return;
        };

        let values = params(10_000, 1_000.0);
        // Through the model's own extraction, so the test cannot disagree with the kernel.
        let hot = BoidsModel::from_params(&values);
        let (max_speed, min_speed) = (hot.max_speed, hot.min_speed);

        let mut state = State::new(&ctx, &values);
        state.run_batched(30);
        state.refresh_stats();

        let stats = state.stats();
        let StatValue::Scalar(avg_speed) = stats[0].value else {
            panic!("average speed is a scalar");
        };
        assert!(
            avg_speed >= f64::from(min_speed) * 0.999 && avg_speed <= f64::from(max_speed) * 1.001,
            "average speed {avg_speed} is outside [{min_speed}, {max_speed}]"
        );

        // Cross-check against the lanes, which a stride bug would not survive.
        let (_, _, vel_x, vel_y) = lanes(&state);
        let reference: f64 = vel_x
            .iter()
            .zip(&vel_y)
            .map(|(&x, &y)| f64::from(x.hypot(y)))
            .sum::<f64>()
            / vel_x.len() as f64;
        assert!(
            (avg_speed - reference).abs() <= 1e-3 * reference.abs().max(1.0),
            "reduced average speed {avg_speed} disagrees with the lanes: {reference}"
        );
    }

    /// Mean velocity near mean speed, which cannot happen if the index comes back empty.
    ///
    /// 800 ticks rather than 200, where alignment is still climbing and lands anywhere in
    /// 0.08..0.36 run to run.
    #[test]
    fn the_flock_aligns_over_time() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping the_flock_aligns_over_time: no adapter");
            return;
        };

        let mut state = State::new(&ctx, &params(20_000, 800.0));
        state.run_batched(800);
        state.refresh_stats();

        let stats = state.stats();
        let (StatValue::Scalar(avg_speed), StatValue::Vector2D { x, y }) = (&stats[0].value, &stats[1].value) else {
            panic!("boids report a scalar speed and a vector velocity");
        };
        let alignment = x.hypot(*y) / avg_speed.max(f64::EPSILON);
        assert!(
            alignment > 0.8,
            "mean velocity is {alignment} of mean speed; the flock is not aligning, so neighbours are probably not being found"
        );
    }

    /// Exercises the 2D dispatch fold in every kernel at once.
    #[test]
    fn a_population_past_one_workgroup_row_still_steps() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping a_population_past_one_workgroup_row_still_steps: no adapter");
            return;
        };

        // Not a multiple of the workgroup width, so the ragged tail is covered.
        let world = 4_000.0f32;
        let mut state = State::new(&ctx, &params(300_037, world));
        state.run_batched(5);

        let (pos_x, pos_y, _, _) = lanes(&state);
        assert_eq!(pos_x.len(), 300_037);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "boid {i} left the world at ({x}, {y}); the dispatch fold probably missed it"
            );
        }
    }
}
