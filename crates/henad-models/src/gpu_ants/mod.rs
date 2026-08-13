//! GPU ants, [`crate::ants`] with its population and pheromone field in GPU buffers.
//!
//! Tick 0 is bit identical since seeding goes through [`AntsModel::init`] and
//! [`PheromoneField::build_sites`]. After that the RNG streams differ, for the reason
//! [`crate::gpu_sir`] gives. Deposits still combine with `max`, which is order independent, so
//! unlike [`crate::gpu_boids`] a run does replay.

use std::sync::Arc;

use henad_compute::cpu::agent_engine::{AGENT_INIT_SEED, agent_model_param_descriptors};
use henad_compute::cpu::field::scalar::ScalarFieldSpec as _;
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::primitives::dispatch::linear_dispatch;
use henad_compute::gpu::primitives::pipeline::{
    compute_pipeline, lane_buffer, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
};
use henad_compute::gpu::primitives::readback::CounterReadback;
use henad_compute::gpu::primitives::reduce::GpuLaneReduce;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::view::agents::GpuAgents;
use henad_compute::gpu::view::display::{DisplayTarget, GpuDisplay, build_display_target};
use henad_compute::snapshot::GpuSnapshot;
use henad_core::authoring::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::field::Extent;
use henad_core::helpers::{extract_f32, extract_u32, mix_seed};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatEntry, StatValue, stat_entries};

use crate::ants::field::{CELL_PALETTE, EMPTY, LOW_PHEROMONE, PheromoneField};
use crate::ants::{ANT_PALETTE, AntLanes, AntsModel};

/// The list is [`agent_model_param_descriptors`] for [`AntsModel`] verbatim, so both backends take
/// the same vector. Only these three are read here, the rest go through the two `from_params`.
const PARAM_NUM_AGENTS: usize = 0;
const PARAM_WORLD_WIDTH: usize = 1;
const PARAM_WORLD_HEIGHT: usize = 2;

/// Carrying food, total pheromone. Deliveries is an accumulating counter, not a reduction.
const REDUCE_LANES: usize = 2;

/// Domain separated from the ant seeding stream, so the two do not start correlated.
const RNG_INIT_SEED: u64 = AGENT_INIT_SEED ^ 0x5EED_5EED_5EED_5EED;

/// `state` packs what the CPU model keeps in three lanes. Mirrored in `step.wgsl`.
const HAS_FOOD_BIT: u32 = 0x100;
const HAS_REWARD_BIT: u32 = 0x200;

pub const NAME: &str = "Ant Foraging (GPU)";
pub const ID: &str = "gpu_ants";
pub const DESCRIPTION: &str =
    "Ants lay and follow pheromone trails between a nest and a food source, stepped entirely on the GPU";

/// Matches `Params` in `step.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct StepParams {
    num_agents: u32,
    groups_x: u32,
    grid_w: u32,
    grid_h: u32,

    n_cells: u32,
    cutdown: f32,
    diagonal: f32,
    reward: f32,

    momentum: f32,
    random_action: f32,
    palette: [u32; 2],
}

/// Matches `Params` in `merge.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MergeParams {
    n: u32,
    groups_x: u32,
    evaporation: f32,
    low: f32,
}

/// Matches `Params` in `display.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct DisplayParams {
    width: u32,
    height: u32,
    n_cells: u32,
    _pad: u32,
    palette: [[u32; 4]; 4],
}

/// Matches `Params` in `reduce.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ReduceParams {
    n: u32,
    lanes: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    _pad: [u32; 3],
}

pub struct GpuAntsState {
    num_agents: u32,
    tick: u64,

    device: wgpu::Device,
    queue: wgpu::Queue,

    /// Position, packed state, colour, RNG, both pheromone layers, this tick's deposits, sites.
    buffers: [wgpu::Buffer; 7],

    step_pipeline: wgpu::ComputePipeline,
    step_bind: wgpu::BindGroup,
    step_groups: (u32, u32),

    merge_pipeline: wgpu::ComputePipeline,
    merge_bind: wgpu::BindGroup,
    merge_groups: (u32, u32),

    display_pipeline: wgpu::ComputePipeline,
    display_bind: wgpu::BindGroup,
    display_groups: (u32, u32),
    display: Arc<GpuDisplay>,

    reduce: GpuLaneReduce,
    reduce_pipeline: wgpu::ComputePipeline,
    reduce_bind: wgpu::BindGroup,
    reduce_groups: (u32, u32),

    /// Cumulative, so unlike a reduction target it is never cleared.
    deliveries: CounterReadback,

    agents: Arc<GpuAgents>,
}

/// All reload only, since [`SimState::set_param`] rejects live edits.
#[must_use]
pub fn param_descriptors() -> Vec<ParamDescriptor> {
    agent_model_param_descriptors::<AntsModel>()
        .into_iter()
        .map(ParamDescriptor::on_reload)
        .collect()
}

#[must_use]
pub fn stat_descriptors() -> Vec<StatDescriptor> {
    AntsModel::STATS.to_vec()
}

impl GpuAntsState {
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// `None` uses the CPU model's default seed
    #[expect(clippy::too_many_lines, reason = "one linear construction of every wgpu object")]
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let num_agents = extract_u32(params, PARAM_NUM_AGENTS, AntsModel::DEFAULT_AGENTS).max(1);
        let extent = Extent {
            w: extract_f32(params, PARAM_WORLD_WIDTH, AntsModel::DEFAULT_EXTENT.w),
            h: extract_f32(params, PARAM_WORLD_HEIGHT, AntsModel::DEFAULT_EXTENT.h),
        };
        let (width, height) = extent.cells();
        let n = num_agents as usize;
        let n_cells = (width as usize) * (height as usize);

        // Seeding through the model's own `init` is what keeps tick 0 bit identical. A port would
        // be free to drift.
        let mut lanes = AntLanes::alloc(n);
        let mut rng_state = seed.map_or(AGENT_INIT_SEED, mix_seed);
        AntsModel::init(&mut lanes, extent, params, &mut rng_state);

        let pos = lane_buffer(device, "gpu_ants_pos", n * 2);
        let state = storage_buffer(device, "gpu_ants_state", n);
        let color = lane_buffer(device, "gpu_ants_color", n);
        let rng = storage_buffer(device, "gpu_ants_rng", n);
        let field = storage_buffer(device, "gpu_ants_field", n_cells * 2);
        let accum = storage_buffer(device, "gpu_ants_accum", n_cells * 2);
        let sites = storage_buffer(device, "gpu_ants_sites", n_cells);

        let positions: Vec<f32> = lanes
            .pos_x
            .iter()
            .zip(&lanes.pos_y)
            .flat_map(|(&x, &y)| [x, y])
            .collect();
        queue.write_buffer(&pos, 0, bytemuck::cast_slice(&positions));
        let packed: Vec<u32> = (0..n).map(|i| pack_state(&lanes, i)).collect();
        queue.write_buffer(&state, 0, bytemuck::cast_slice(&packed));
        // The CPU lane holds palette indices, this one is drawn directly so it holds colours.
        let colors: Vec<u32> = lanes.has_food.iter().map(|&f| packed_ant_color(f)).collect();
        queue.write_buffer(&color, 0, bytemuck::cast_slice(&colors));
        let rng_seed = seed.map_or(RNG_INIT_SEED, |s| mix_seed(s ^ RNG_INIT_SEED));
        queue.write_buffer(&rng, 0, bytemuck::cast_slice(&seed_rng_states(n, rng_seed)));

        // Through the field spec, so the two backends cannot place the nest differently.
        let mut site_bytes = vec![EMPTY; n_cells];
        PheromoneField::build_sites(width, height, &mut site_bytes);
        let site_words: Vec<u32> = site_bytes.iter().map(|&s| u32::from(s)).collect();
        queue.write_buffer(&sites, 0, bytemuck::cast_slice(&site_words));

        // `accum` is read before it is first written, and `merge` leaves it zeroed thereafter.
        let mut clear = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_ants_clear_accum"),
        });
        clear.clear_buffer(&accum, 0, None);
        queue.submit(Some(clear.finish()));

        // --- Step pipeline ---
        let deliveries = CounterReadback::new(device, "gpu_ants_deliveries", 1);
        let hot = AntsModel::from_params(params);
        let step_groups = linear_dispatch(num_agents);
        let step_params = uniform_buffer(
            device,
            queue,
            "gpu_ants_step_params",
            bytemuck::bytes_of(&StepParams {
                num_agents,
                groups_x: step_groups.0,
                grid_w: width,
                grid_h: height,
                n_cells: n_cells as u32,
                cutdown: hot.cutdown,
                diagonal: hot.diagonal,
                reward: hot.reward,
                momentum: hot.momentum,
                random_action: hot.random_action,
                palette: packed_ant_palette(),
            }),
        );
        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_ants_step_layout"),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, false),
                storage_entry(2, false),
                storage_entry(3, false),
                storage_entry(4, true),
                storage_entry(5, false),
                storage_entry(6, true),
                storage_entry(7, false),
                uniform_entry(8),
            ],
        });
        let step_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_ants_step_bind"),
            layout: &step_layout,
            entries: &[
                binding(0, pos.as_entire_binding()),
                binding(1, state.as_entire_binding()),
                binding(2, color.as_entire_binding()),
                binding(3, rng.as_entire_binding()),
                binding(4, field.as_entire_binding()),
                binding(5, accum.as_entire_binding()),
                binding(6, sites.as_entire_binding()),
                binding(7, deliveries.binding()),
                binding(8, step_params.as_entire_binding()),
            ],
        });
        let step_pipeline = compute_pipeline(device, "gpu_ants_step", include_str!("step.wgsl"), &step_layout);

        // --- Merge pipeline ---
        let merge_domain = (n_cells * 2) as u32;
        let merge_groups = linear_dispatch(merge_domain);
        let field_params = PheromoneField::from_params(params);
        let merge_params = uniform_buffer(
            device,
            queue,
            "gpu_ants_merge_params",
            bytemuck::bytes_of(&MergeParams {
                n: merge_domain,
                groups_x: merge_groups.0,
                evaporation: field_params.evaporation,
                low: LOW_PHEROMONE,
            }),
        );
        let merge_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_ants_merge_layout"),
            entries: &[storage_entry(0, false), storage_entry(1, false), uniform_entry(2)],
        });
        let merge_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_ants_merge_bind"),
            layout: &merge_layout,
            entries: &[
                binding(0, field.as_entire_binding()),
                binding(1, accum.as_entire_binding()),
                binding(2, merge_params.as_entire_binding()),
            ],
        });
        let merge_pipeline = compute_pipeline(device, "gpu_ants_merge", include_str!("merge.wgsl"), &merge_layout);

        // --- Display pipeline ---
        let DisplayTarget {
            view: display_view,
            display,
        } = build_display_target(device, ctx.target_format, width, height);
        let display_params = uniform_buffer(
            device,
            queue,
            "gpu_ants_display_params",
            bytemuck::bytes_of(&DisplayParams {
                width,
                height,
                n_cells: n_cells as u32,
                _pad: 0,
                palette: packed_cell_palette(),
            }),
        );
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_ants_display_layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                uniform_entry(3),
            ],
        });
        let display_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_ants_display_bind"),
            layout: &display_layout,
            entries: &[
                binding(0, field.as_entire_binding()),
                binding(1, sites.as_entire_binding()),
                binding(2, wgpu::BindingResource::TextureView(&display_view)),
                binding(3, display_params.as_entire_binding()),
            ],
        });
        let display_pipeline = compute_pipeline(
            device,
            "gpu_ants_display",
            include_str!("display.wgsl"),
            &display_layout,
        );

        // --- Stat reduction ---
        // The two lanes have different domains, so the tree covers the longer one.
        let reduce_domain = num_agents.max(n_cells as u32);
        let reduce = GpuLaneReduce::new(device, queue, ID, REDUCE_LANES, reduce_domain);
        let reduce_groups = reduce.agent_groups();
        let reduce_params = uniform_buffer(
            device,
            queue,
            "gpu_ants_reduce_params",
            bytemuck::bytes_of(&ReduceParams {
                n: reduce_domain,
                lanes: REDUCE_LANES as u32,
                groups_x: reduce_groups.0,
                num_agents,
                n_cells: n_cells as u32,
                _pad: [0; 3],
            }),
        );
        let reduce_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_ants_reduce_layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, true),
                storage_entry(2, false),
                uniform_entry(3),
            ],
        });
        let reduce_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_ants_reduce_bind"),
            layout: &reduce_layout,
            entries: &[
                binding(0, state.as_entire_binding()),
                binding(1, field.as_entire_binding()),
                binding(2, reduce.partials_binding()),
                binding(3, reduce_params.as_entire_binding()),
            ],
        });
        let reduce_pipeline = compute_pipeline(device, "gpu_ants_reduce", include_str!("reduce.wgsl"), &reduce_layout);

        let agents = Arc::new(GpuAgents {
            pos: pos.clone(),
            color: color.clone(),
            count: num_agents,
            world_w: extent.w,
            world_h: extent.h,
        });

        Self {
            num_agents,
            tick: 0,
            device: device.clone(),
            queue: queue.clone(),
            buffers: [pos, state, color, rng, field, accum, sites],
            step_pipeline,
            step_bind,
            step_groups,
            merge_pipeline,
            merge_bind,
            merge_groups,
            display_pipeline,
            display_bind,
            display_groups: (width.div_ceil(16), height.div_ceil(16)),
            display,
            reduce,
            reduce_pipeline,
            reduce_bind,
            reduce_groups,
            deliveries,
            agents,
        }
    }
}

fn binding(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

/// The three per-ant scalars the CPU keeps in separate lanes, as `step.wgsl` reads them.
fn pack_state(lanes: &AntLanes, i: usize) -> u32 {
    let mut packed = u32::from(lanes.last_step[i]);
    if lanes.has_food[i] != 0 {
        packed |= HAS_FOOD_BIT;
    }
    if lanes.reward[i] != 0.0 {
        packed |= HAS_REWARD_BIT;
    }
    packed
}

/// Matches `pcg_hash` in `step.wgsl` bit-for-bit (u32 arithmetic wraps identically on both sides).
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn seed_rng_states(n: usize, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..n).map(|i| pcg_hash(seed32 ^ i as u32)).collect()
}

/// Packed for the step uniform, from the one palette in `ants` so colours cannot drift.
fn packed_ant_palette() -> [u32; 2] {
    [u32::from_le_bytes(ANT_PALETTE[0]), u32::from_le_bytes(ANT_PALETTE[1])]
}

fn packed_ant_color(index: u8) -> u32 {
    let rgba = ANT_PALETTE.get(index as usize).copied().unwrap_or(ANT_PALETTE[0]);
    u32::from_le_bytes(rgba)
}

/// Same, for the display uniform. Indexed as `palette[i >> 2][i & 3]`.
fn packed_cell_palette() -> [[u32; 4]; 4] {
    let mut packed = [[0u32; 4]; 4];
    for (i, rgba) in CELL_PALETTE.iter().enumerate() {
        packed[i / 4][i % 4] = u32::from_le_bytes(*rgba);
    }
    packed
}

impl SimState for GpuAntsState {
    /// Fallback for callers holding only a `SimState`. The sim thread batches instead.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_ants_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        let sums = self.reduce.sums();
        stat_entries(
            AntsModel::STATS,
            vec![
                StatValue::Scalar(f64::from(sums[0])),
                StatValue::Scalar(f64::from(self.deliveries.values()[0])),
                StatValue::Scalar(f64::from(sums[1])),
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
        let buffers: usize = self.buffers.iter().map(|b| b.size() as usize).sum();
        buffers + self.reduce.heap_bytes()
    }
}

impl GpuSimState for GpuAntsState {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if count == 0 {
            return;
        }

        for i in 0..count {
            // Both passes do real work, so either can carry a stamp. A stamp on an empty pass is
            // silently never written.
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_ants_step_pass"),
                timestamp_writes: timestamps
                    .filter(|_| i == 0)
                    .map(|query_set| wgpu::ComputePassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: None,
                    }),
            });
            pass.set_pipeline(&self.step_pipeline);
            pass.set_bind_group(0, &self.step_bind, &[]);
            pass.dispatch_workgroups(self.step_groups.0, self.step_groups.1, 1);
            drop(pass);

            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_ants_merge_pass"),
                timestamp_writes: timestamps.filter(|_| i == count - 1).map(|query_set| {
                    wgpu::ComputePassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: None,
                        end_of_pass_write_index: Some(1),
                    }
                }),
            });
            pass.set_pipeline(&self.merge_pipeline);
            pass.set_bind_group(0, &self.merge_bind, &[]);
            pass.dispatch_workgroups(self.merge_groups.0, self.merge_groups.1, 1);
        }

        self.tick += u64::from(count);
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_ants_display_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, &self.display_bind, &[]);
            pass.dispatch_workgroups(self.display_groups.0, self.display_groups.1, 1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_ants_reduce_leaf_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.reduce_pipeline);
            pass.set_bind_group(0, &self.reduce_bind, &[]);
            pass.dispatch_workgroups(self.reduce_groups.0, self.reduce_groups.1, 1);
        }
        self.reduce.encode(encoder);
        self.deliveries.encode_copy(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.reduce.begin_readback();
        self.deliveries.begin_map();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        self.reduce.poll_readback(device, block);
        if block {
            self.deliveries.poll_blocking(device);
        } else {
            self.deliveries.poll(device);
        }
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: Some(Arc::clone(&self.display)),
            agents: Some(Arc::clone(&self.agents)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::field::{FOOD, HOME, OBSTACLE, TO_FOOD, TO_HOME};
    use henad_compute::cpu::agent_engine::AgentModelState;

    fn headless_context() -> Option<GpuContext> {
        crate::gpu_test_support::headless_context("gpu_ants_test_device", wgpu::Features::empty())
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

    /// Batched as the real runner does. Enough passes in one command buffer trips the OS GPU
    /// watchdog, after which every readback returns zeros with no error.
    const STEPS_PER_SUBMISSION: u32 = 64;

    fn step_n(ctx: &GpuContext, state: &mut GpuAntsState, count: u32) {
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

    fn refresh_stats(ctx: &GpuContext, state: &mut GpuAntsState) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    /// Reads `len` words out of a buffer.
    fn read_words(ctx: &GpuContext, buffer: &wgpu::Buffer, len: usize) -> Vec<u32> {
        let size = (len * std::mem::size_of::<u32>()) as u64;
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
        let out = bytemuck::cast_slice::<u8, u32>(&data)[..len].to_vec();
        drop(data);
        staging.unmap();
        out
    }

    const POS: usize = 0;
    const STATE: usize = 1;
    const FIELD: usize = 4;

    /// Current positions, as the two scalar lanes the CPU model keeps.
    fn positions(ctx: &GpuContext, state: &GpuAntsState) -> (Vec<f32>, Vec<f32>) {
        let n = state.num_agents as usize;
        let words = read_words(ctx, &state.buffers[POS], n * 2);
        let floats: Vec<f32> = words.iter().map(|&w| f32::from_bits(w)).collect();
        (
            floats.iter().step_by(2).copied().collect(),
            floats.iter().skip(1).step_by(2).copied().collect(),
        )
    }

    /// The CPU model only ever stores `0.0` or the reward param in its reward lane, which is what
    /// lets the GPU port carry it as one bit of `state` and stay inside eight storage buffers.
    #[test]
    fn the_cpu_reward_lane_only_ever_holds_two_values() {
        let values = params(500, 200.0);
        let reward = AntsModel::from_params(&values).reward;
        let mut state = AgentModelState::<AntsModel>::from_params(&values);
        for tick in 0..200 {
            state.step();
            for (i, &r) in state.lanes().reward.iter().enumerate() {
                assert!(
                    r == 0.0 || r == reward,
                    "ant {i} holds reward {r} on tick {tick}, which is neither 0 nor {reward}"
                );
            }
        }
    }

    /// Both backends seed through `AntsModel::init`, so any later divergence is the step's.
    #[test]
    fn the_initial_colony_matches_the_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping the_initial_colony_matches_the_cpu_model: no adapter");
            return;
        };

        let values = params(2_000, 200.0);
        let gpu = GpuAntsState::new(&ctx, &values);
        let cpu = AgentModelState::<AntsModel>::from_params(&values);

        let (pos_x, pos_y) = positions(&ctx, &gpu);
        let cpu_lanes = cpu.lanes();
        assert_eq!(pos_x, cpu_lanes.pos_x, "initial x positions must match the CPU model");
        assert_eq!(pos_y, cpu_lanes.pos_y, "initial y positions must match the CPU model");
    }

    /// The reference is bounded, not toroidal like the other models.
    #[test]
    fn ants_stay_inside_the_bounded_field() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping ants_stay_inside_the_bounded_field: no adapter");
            return;
        };

        let world = 200.0f32;
        let mut state = GpuAntsState::new(&ctx, &params(2_000, world));
        step_n(&ctx, &mut state, 200);

        let (pos_x, pos_y) = positions(&ctx, &state);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "ant {i} left the field at ({x}, {y})"
            );
        }
    }

    /// The momentum and random action fallbacks are the easy ones to forget an obstacle check in.
    #[test]
    fn ants_never_enter_an_obstacle() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping ants_never_enter_an_obstacle: no adapter");
            return;
        };

        let mut sites = vec![EMPTY; 200 * 200];
        PheromoneField::build_sites(200, 200, &mut sites);

        let mut state = GpuAntsState::new(&ctx, &params(2_000, 200.0));
        step_n(&ctx, &mut state, 200);

        let (pos_x, pos_y) = positions(&ctx, &state);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            let c = (y as usize) * 200 + (x as usize);
            assert_ne!(sites[c], OBSTACLE, "ant {i} is inside an obstacle");
        }
    }

    /// The whole point of the model. Nothing below happens if the deposit never lands, if the
    /// merge never runs, or if the trail is followed in the wrong direction.
    #[test]
    fn the_colony_lays_a_trail_and_delivers_food() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping the_colony_lays_a_trail_and_delivers_food: no adapter");
            return;
        };

        let mut state = GpuAntsState::new(&ctx, &params(2_000, 200.0));
        step_n(&ctx, &mut state, 1_500);
        refresh_stats(&ctx, &mut state);

        let stats = state.stats();
        let scalar = |i: usize| match &stats[i].value {
            StatValue::Scalar(v) => *v,
            other => panic!("ants report scalars, got {other:?}"),
        };
        assert!(
            scalar(2) > 0.0,
            "1500 ticks of depositing and the field holds no pheromone at all"
        );
        assert!(scalar(0) > 0.0, "no ant is carrying food after 1500 ticks");
        assert!(scalar(1) > 0.0, "no ant has ever delivered food home after 1500 ticks");
    }

    /// Cross-checks the reduction against the field it summed, which a stride mistake in the
    /// two-lane layout would not survive.
    #[test]
    fn total_pheromone_agrees_with_the_field() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping total_pheromone_agrees_with_the_field: no adapter");
            return;
        };

        let mut state = GpuAntsState::new(&ctx, &params(1_000, 100.0));
        step_n(&ctx, &mut state, 200);
        refresh_stats(&ctx, &mut state);

        let n_cells = 100 * 100;
        let words = read_words(&ctx, &state.buffers[FIELD], n_cells * 2);
        let reference: f64 = words.iter().map(|&w| f64::from(f32::from_bits(w))).sum();

        let StatValue::Scalar(total) = state.stats()[2].value else {
            panic!("total pheromone is a scalar");
        };
        assert!(
            (total - reference).abs() <= 1e-3 * reference.abs().max(1.0),
            "reduced total pheromone {total} disagrees with the field: {reference}"
        );
        assert!(reference > 0.0, "the field should hold pheromone after 200 ticks");
    }

    /// Unlike `gpu_boids`, nothing here depends on the order the GPU schedules work: deposits
    /// combine with `max`, and no ant reads another's lanes. So a run must replay exactly.
    #[test]
    fn a_run_replays_bit_identically() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping a_run_replays_bit_identically: no adapter");
            return;
        };

        let run = || {
            let mut state = GpuAntsState::new(&ctx, &params(4_000, 200.0));
            step_n(&ctx, &mut state, 300);
            let n = state.num_agents as usize;
            (
                read_words(&ctx, &state.buffers[POS], n * 2),
                read_words(&ctx, &state.buffers[STATE], n),
                read_words(&ctx, &state.buffers[FIELD], 200 * 200 * 2),
            )
        };

        let (pos_a, state_a, field_a) = run();
        let (pos_b, state_b, field_b) = run();
        assert_eq!(pos_a, pos_b, "ant positions are not reproducible");
        assert_eq!(state_a, state_b, "packed ant state is not reproducible");
        assert_eq!(field_a, field_b, "the pheromone field is not reproducible");
    }

    /// Exercises the 2D dispatch fold in the step and merge kernels at once.
    #[test]
    fn a_population_past_one_workgroup_row_still_steps() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping a_population_past_one_workgroup_row_still_steps: no adapter");
            return;
        };

        // Not a multiple of the workgroup width, so the ragged tail is covered.
        let world = 1_000.0f32;
        let mut state = GpuAntsState::new(&ctx, &params(300_037, world));
        step_n(&ctx, &mut state, 5);

        let (pos_x, pos_y) = positions(&ctx, &state);
        assert_eq!(pos_x.len(), 300_037);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "ant {i} left the field at ({x}, {y}); the dispatch fold probably missed it"
            );
        }
    }

    /// The site markers are what the ants navigate between, so a layout mismatch would make the
    /// two backends different models.
    #[test]
    fn the_site_layout_matches_the_cpu_field() {
        let mut sites = vec![EMPTY; 200 * 200];
        PheromoneField::build_sites(200, 200, &mut sites);
        assert!(sites.contains(&HOME) && sites.contains(&FOOD) && sites.contains(&OBSTACLE));
        assert_eq!(TO_FOOD, 0, "the field buffer lays out to-food first");
        assert_eq!(TO_HOME, 1, "the field buffer lays out to-home second");
    }
}
