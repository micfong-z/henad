//! GPU boids, [`crate::boids`] with its population in GPU buffers.
//!
//! Tick 0 is bit identical since seeding goes through [`BoidsModel::init`]. After that, the
//! neighbour index does not fix the order within a cell, so trajectories are likely different.

use std::sync::Arc;

use henad_compute::cpu::agent_engine::{AGENT_INIT_SEED, agent_model_param_descriptors};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::primitives::dispatch::linear_dispatch;
use henad_compute::gpu::primitives::pipeline::{
    compute_pipeline, lane_buffer, storage_entry, uniform_buffer, uniform_entry,
};
use henad_compute::gpu::primitives::reduce::GpuLaneReduce;
use henad_compute::gpu::primitives::spatial_hash::GpuSpatialHash;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::view::agents::GpuAgents;
use henad_compute::snapshot::GpuSnapshot;
use henad_core::authoring::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::field::Extent;
use henad_core::helpers::{extract_f32, extract_u32, mix_seed};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatEntry, StatValue, stat_entries};

use crate::boids::{BoidLanes, BoidsModel, HEADING_PALETTE};

/// The list is [`agent_model_param_descriptors`] for [`BoidsModel`] verbatim, so both backends
/// take the same vector. Only these three are read here, the rest go through
/// [`BoidsModel::from_params`].
const PARAM_NUM_AGENTS: usize = 0;
const PARAM_WORLD_WIDTH: usize = 1;
const PARAM_WORLD_HEIGHT: usize = 2;

/// Speed, x velocity, y velocity.
const REDUCE_LANES: usize = 3;

pub const NAME: &str = "Boids Flocking (GPU)";
pub const ID: &str = "gpu_boids";
pub const DESCRIPTION: &str = "A simulation of flocking behavior in a group of boids, stepped entirely on the GPU";

/// Matches `Params` in `step.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct StepParams {
    num_agents: u32,
    groups_x: u32,
    grid_w: u32,
    grid_h: u32,

    cell_w: f32,
    cell_h: f32,
    cell_w_inv: f32,
    cell_h_inv: f32,

    world_w: f32,
    world_h: f32,
    half_w: f32,
    half_h: f32,

    visual_range: f32,
    visual_sq: f32,
    protected_sq: f32,
    separation: f32,

    alignment: f32,
    cohesion: f32,
    max_speed: f32,
    min_speed: f32,

    /// Heading colours, in the uniform to keep a storage binding free.
    palette: [[u32; 4]; 2],
}

/// Matches `ReduceParams` in `reduce.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ReduceParams {
    n: u32,
    lanes: u32,
    groups_x: u32,
    _pad: u32,
}

/// One ping-ponged lane.
struct LanePair {
    a: wgpu::Buffer,
    b: wgpu::Buffer,
}

impl LanePair {
    /// `words` is elements per agent, e.g. 2 for a `vec2` lane and 1 for a scalar.
    fn new(device: &wgpu::Device, name: &str, agents: usize, words: usize) -> Self {
        Self {
            a: lane_buffer(device, &format!("gpu_boids_{name}_a"), agents * words),
            b: lane_buffer(device, &format!("gpu_boids_{name}_b"), agents * words),
        }
    }

    fn sides(&self, a_is_current: bool) -> (&wgpu::Buffer, &wgpu::Buffer) {
        if a_is_current {
            (&self.a, &self.b)
        } else {
            (&self.b, &self.a)
        }
    }
}

pub struct GpuBoidsState {
    num_agents: u32,
    tick: u64,

    device: wgpu::Device,
    queue: wgpu::Queue,

    /// Position, velocity, colour
    lanes: [LanePair; 3],

    hash: GpuSpatialHash,
    hash_bind_a: wgpu::BindGroup,
    hash_bind_b: wgpu::BindGroup,

    step_pipeline: wgpu::ComputePipeline,
    step_bind_a2b: wgpu::BindGroup,
    step_bind_b2a: wgpu::BindGroup,
    step_groups: (u32, u32),

    reduce: GpuLaneReduce,
    reduce_pipeline: wgpu::ComputePipeline,
    reduce_bind_a: wgpu::BindGroup,
    reduce_bind_b: wgpu::BindGroup,
    reduce_groups: (u32, u32),

    agents_a: Arc<GpuAgents>,
    agents_b: Arc<GpuAgents>,

    /// `true` when the `a` side of every lane holds the current state.
    current_is_a: bool,
}

/// All reload only, since [`SimState::set_param`] rejects live edits.
#[must_use]
pub fn param_descriptors() -> Vec<ParamDescriptor> {
    agent_model_param_descriptors::<BoidsModel>()
        .into_iter()
        .map(ParamDescriptor::on_reload)
        .collect()
}

#[must_use]
pub fn stat_descriptors() -> Vec<StatDescriptor> {
    BoidsModel::STATS.to_vec()
}

impl GpuBoidsState {
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// `None` uses the CPU model's default seed
    #[expect(clippy::too_many_lines, reason = "one linear construction of every wgpu object")]
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let num_agents = extract_u32(params, PARAM_NUM_AGENTS, BoidsModel::DEFAULT_AGENTS).max(1);
        let extent = Extent {
            w: extract_f32(params, PARAM_WORLD_WIDTH, BoidsModel::DEFAULT_EXTENT.w),
            h: extract_f32(params, PARAM_WORLD_HEIGHT, BoidsModel::DEFAULT_EXTENT.h),
        };
        let n = num_agents as usize;

        // Seeding through the model's own `init` is what keeps tick 0 bit identical. A port
        // would be free to drift.
        let mut lanes = BoidLanes::alloc(n);
        let mut rng = seed.map_or(AGENT_INIT_SEED, mix_seed);
        BoidsModel::init(&mut lanes, extent, params, &mut rng);

        let pos = LanePair::new(device, "pos", n, 2);
        let vel = LanePair::new(device, "vel", n, 2);
        let color = LanePair::new(device, "color", n, 1);

        // Only `a` is seeded, `b` is fully written by the first step.
        queue.write_buffer(&pos.a, 0, bytemuck::cast_slice(&interleave(&lanes.pos_x, &lanes.pos_y)));
        queue.write_buffer(&vel.a, 0, bytemuck::cast_slice(&interleave(&lanes.vel_x, &lanes.vel_y)));
        // The CPU lane holds palette indices, this one is drawn directly so it holds colours.
        let seeded_colors: Vec<u32> = lanes.color.iter().map(|&c| packed_palette_color(c)).collect();
        queue.write_buffer(&color.a, 0, bytemuck::cast_slice(&seeded_colors));

        // --- Neighbour index ---
        let hot = BoidsModel::from_params(params);
        let cell_size = BoidsModel::index_cell_size(&hot);
        let hash = GpuSpatialHash::new(device, queue, ID, extent, cell_size, num_agents);
        let hash_bind_a = hash.bind_positions(device, "gpu_boids_hash_bind_a", &pos.a);
        let hash_bind_b = hash.bind_positions(device, "gpu_boids_hash_bind_b", &pos.b);

        // --- Step pipeline ---
        let grid = hash.grid();
        let step_groups = linear_dispatch(num_agents);
        let step_params = uniform_buffer(
            device,
            queue,
            "gpu_boids_step_params",
            bytemuck::bytes_of(&StepParams {
                num_agents,
                groups_x: step_groups.0,
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
            }),
        );

        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_boids_step_layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, true),
                storage_entry(2, false),
                storage_entry(3, false),
                storage_entry(4, false),
                storage_entry(5, true),
                storage_entry(6, true),
                uniform_entry(7),
            ],
        });
        let make_step_bind = |label: &str, a_is_current: bool| {
            let (pos_in, pos_out) = pos.sides(a_is_current);
            let (vel_in, vel_out) = vel.sides(a_is_current);
            let (_, color_out) = color.sides(a_is_current);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &step_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: pos_in.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: vel_in.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pos_out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: vel_out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: color_out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: hash.cell_start_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: hash.sorted_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: step_params.as_entire_binding(),
                    },
                ],
            })
        };
        let step_bind_a2b = make_step_bind("gpu_boids_step_bind_a2b", true);
        let step_bind_b2a = make_step_bind("gpu_boids_step_bind_b2a", false);
        let step_pipeline = compute_pipeline(device, "gpu_boids_step", include_str!("step.wgsl"), &step_layout);

        // --- Stat reduction ---
        let reduce = GpuLaneReduce::new(device, queue, ID, REDUCE_LANES, num_agents);
        let reduce_groups = reduce.agent_groups();
        let reduce_params = uniform_buffer(
            device,
            queue,
            "gpu_boids_reduce_params",
            bytemuck::bytes_of(&ReduceParams {
                n: num_agents,
                lanes: REDUCE_LANES as u32,
                groups_x: reduce_groups.0,
                _pad: 0,
            }),
        );
        let reduce_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_boids_reduce_layout"),
            entries: &[storage_entry(0, true), storage_entry(1, false), uniform_entry(2)],
        });
        let make_reduce_bind = |label: &str, a_is_current: bool| {
            let (v, _) = vel.sides(a_is_current);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &reduce_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: v.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: reduce.partials_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: reduce_params.as_entire_binding(),
                    },
                ],
            })
        };
        let reduce_bind_a = make_reduce_bind("gpu_boids_reduce_bind_a", true);
        let reduce_bind_b = make_reduce_bind("gpu_boids_reduce_bind_b", false);
        let reduce_pipeline = compute_pipeline(device, "gpu_boids_reduce", include_str!("reduce.wgsl"), &reduce_layout);

        // What the UI draws, one handle per side.
        let make_agents = |a_is_current: bool| {
            let (p, _) = pos.sides(a_is_current);
            let (c, _) = color.sides(a_is_current);
            Arc::new(GpuAgents {
                pos: p.clone(),
                color: c.clone(),
                count: num_agents,
                world_w: extent.w,
                world_h: extent.h,
            })
        };
        let agents_a = make_agents(true);
        let agents_b = make_agents(false);

        Self {
            num_agents,
            tick: 0,
            device: device.clone(),
            queue: queue.clone(),
            lanes: [pos, vel, color],
            hash,
            hash_bind_a,
            hash_bind_b,
            step_pipeline,
            step_bind_a2b,
            step_bind_b2a,
            step_groups,
            reduce,
            reduce_pipeline,
            reduce_bind_a,
            reduce_bind_b,
            reduce_groups,
            agents_a,
            agents_b,
            current_is_a: true,
        }
    }

    fn current_hash_bind(&self) -> &wgpu::BindGroup {
        if self.current_is_a {
            &self.hash_bind_a
        } else {
            &self.hash_bind_b
        }
    }

    fn current_step_bind(&self) -> &wgpu::BindGroup {
        if self.current_is_a {
            &self.step_bind_a2b
        } else {
            &self.step_bind_b2a
        }
    }

    fn current_reduce_bind(&self) -> &wgpu::BindGroup {
        if self.current_is_a {
            &self.reduce_bind_a
        } else {
            &self.reduce_bind_b
        }
    }

    #[cfg(test)]
    fn current_velocity_lane(&self) -> &wgpu::Buffer {
        let (vel, _) = self.lanes[1].sides(self.current_is_a);
        vel
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

impl SimState for GpuBoidsState {
    /// Fallback for callers holding only a `SimState`. The sim thread batches instead.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_boids_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        let sums = self.reduce.sums();
        let inv = 1.0 / f64::from(self.num_agents.max(1));
        stat_entries(
            BoidsModel::STATS,
            vec![
                StatValue::Scalar(f64::from(sums[0]) * inv),
                StatValue::Vector2D {
                    x: f64::from(sums[1]) * inv,
                    y: f64::from(sums[2]) * inv,
                },
            ],
        )
    }

    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }

    fn population(&self) -> u64 {
        u64::from(self.num_agents)
    }

    fn heap_bytes(&self) -> usize {
        let lanes: usize = self
            .lanes
            .iter()
            .map(|pair| (pair.a.size() + pair.b.size()) as usize)
            .sum();
        lanes + self.hash.heap_bytes() + self.reduce.heap_bytes()
    }
}

impl GpuSimState for GpuBoidsState {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if count == 0 {
            return;
        }

        for i in 0..count {
            let is_first = i == 0;
            let is_last = i == count - 1;

            // A batch begins with the index rebuild, so the opening stamp goes on the hash's
            // first pass. Otherwise the first rebuild falls outside the measurement.
            self.hash.encode_build(
                encoder,
                self.current_hash_bind(),
                timestamps.filter(|_| is_first).map(|query_set| (query_set, 0)),
            );

            let timestamp_writes = timestamps
                .filter(|_| is_last)
                .map(|query_set| wgpu::ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(1),
                });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_boids_step_pass"),
                timestamp_writes,
            });
            pass.set_pipeline(&self.step_pipeline);
            pass.set_bind_group(0, self.current_step_bind(), &[]);
            pass.dispatch_workgroups(self.step_groups.0, self.step_groups.1, 1);
            drop(pass);

            self.current_is_a = !self.current_is_a;
        }

        self.tick += u64::from(count);
    }

    /// No display pass, the lane buffers are the view.
    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_boids_reduce_leaf_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.reduce_pipeline);
            pass.set_bind_group(0, self.current_reduce_bind(), &[]);
            pass.dispatch_workgroups(self.reduce_groups.0, self.reduce_groups.1, 1);
        }
        self.reduce.encode(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.reduce.begin_readback();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        self.reduce.poll_readback(device, block);
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: None,
            agents: Some(Arc::clone(if self.current_is_a {
                &self.agents_a
            } else {
                &self.agents_b
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_compute::gpu::GpuContext;

    fn headless_context() -> Option<GpuContext> {
        crate::gpu_test_support::headless_context("gpu_boids_test_device", wgpu::Features::empty())
    }

    fn params(num_agents: u32, world: f32) -> Vec<ParamValue> {
        let mut values: Vec<ParamValue> = param_descriptors()
            .iter()
            .map(|desc| desc.kind.default_value())
            .collect();
        values[PARAM_NUM_AGENTS] = ParamValue::U32(num_agents);
        values[PARAM_WORLD_WIDTH] = ParamValue::F32(world);
        values[PARAM_WORLD_HEIGHT] = ParamValue::F32(world);
        values
    }

    /// Batched as the real runner does. An agent step records five passes, so a few hundred in
    /// one command buffer trips the OS GPU watchdog and every later readback returns zeros.
    const STEPS_PER_SUBMISSION: u32 = 64;

    fn step_n(ctx: &GpuContext, state: &mut GpuBoidsState, count: u32) {
        let mut remaining = count;
        while remaining > 0 {
            let batch = remaining.min(STEPS_PER_SUBMISSION);
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            state.encode_steps(&mut encoder, batch, None);
            ctx.queue.submit(Some(encoder.finish()));
            remaining -= batch;
        }
    }

    fn refresh_stats(ctx: &GpuContext, state: &mut GpuBoidsState) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    /// Reads a lane back into a `Vec<f32>`.
    fn read_lane(ctx: &GpuContext, buffer: &wgpu::Buffer, len: usize) -> Vec<f32> {
        let size = (len * std::mem::size_of::<f32>()) as u64;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        ctx.queue.submit(Some(encoder.finish()));

        let (tx, rx) = flume::bounded(1);
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        rx.recv().expect("channel").expect("map");
        let data = staging.slice(..).get_mapped_range();
        let out = bytemuck::cast_slice::<u8, f32>(&data)[..len].to_vec();
        drop(data);
        staging.unmap();
        out
    }

    /// Back into the two scalar lanes the CPU model keeps.
    fn split(interleaved: &[f32]) -> (Vec<f32>, Vec<f32>) {
        (
            interleaved.iter().step_by(2).copied().collect(),
            interleaved.iter().skip(1).step_by(2).copied().collect(),
        )
    }

    /// Current-side positions and velocities, as `(pos_x, pos_y, vel_x, vel_y)`.
    fn lanes(ctx: &GpuContext, state: &GpuBoidsState) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let agents = if state.current_is_a {
            &state.agents_a
        } else {
            &state.agents_b
        };
        let n = state.num_agents as usize;
        let (pos_x, pos_y) = split(&read_lane(ctx, &agents.pos, n * 2));
        let (vel_x, vel_y) = split(&read_lane(ctx, state.current_velocity_lane(), n * 2));
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
        let mut state = GpuBoidsState::new(&ctx, &params(4_000, world));
        step_n(&ctx, &mut state, 60);

        let (pos_x, pos_y, _, _) = lanes(&ctx, &state);
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

        let mut state = GpuBoidsState::new(&ctx, &values);
        step_n(&ctx, &mut state, 60);

        let (_, _, vel_x, vel_y) = lanes(&ctx, &state);
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
        let gpu = GpuBoidsState::new(&ctx, &values);
        let cpu = AgentModelState::<BoidsModel>::from_params(&values);

        let (pos_x, pos_y, vel_x, vel_y) = lanes(&ctx, &gpu);
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

        let mut state = GpuBoidsState::new(&ctx, &values);
        step_n(&ctx, &mut state, 30);
        refresh_stats(&ctx, &mut state);

        let stats = state.stats();
        let StatValue::Scalar(avg_speed) = stats[0].value else {
            panic!("average speed is a scalar");
        };
        assert!(
            avg_speed >= f64::from(min_speed) * 0.999 && avg_speed <= f64::from(max_speed) * 1.001,
            "average speed {avg_speed} is outside [{min_speed}, {max_speed}]"
        );

        // Cross-check against the lanes, which a stride bug would not survive.
        let (_, _, vel_x, vel_y) = lanes(&ctx, &state);
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

        let mut state = GpuBoidsState::new(&ctx, &params(20_000, 800.0));
        step_n(&ctx, &mut state, 800);
        refresh_stats(&ctx, &mut state);

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
        let mut state = GpuBoidsState::new(&ctx, &params(300_037, world));
        step_n(&ctx, &mut state, 5);

        let (pos_x, pos_y, _, _) = lanes(&ctx, &state);
        assert_eq!(pos_x.len(), 300_037);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "boid {i} left the world at ({x}, {y}); the dispatch fold probably missed it"
            );
        }
    }
}
