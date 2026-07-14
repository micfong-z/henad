//! GPU-accelerated SIR epidemic model
//!
//! This is similar to`gpu_game_of_life` but with 3 differences: three cell states instead of
//! two, a probabilistic transition rule, and therefore a per-cell RNG.
//!
//! The CPU model consumes a single RNG stream sequentially across a row, which has no GPU
//! equivalent. Instead each cell owns its own RNG state, stored in a ping-ponged `array<u32>`
//! buffer alongside the SIR state. Every step, a cell reads its own hash state, advances it
//! one round, and uses the result for its transition. This makes the GPU stream different from
//! CPU stream. See `tests` for further details.

use std::sync::Arc;

use henad_compute::gpu::GpuContext;
use henad_compute::gpu::display::{DisplayTarget, GpuDisplay, build_display_target};
use henad_compute::gpu::readback::CounterReadback;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::grid_engine::GRID_INIT_SEED;
use henad_core::helpers::{extract_f32, extract_u32, f32_param, stat, u32_param, xorshift64};
use henad_core::model::{Model, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatEntry};

use crate::sir::PALETTE;

const WORKGROUP_SIZE: u32 = 16;

/// A domain-separated seed for the per-cell RNG buffer, so its stream doesn't start correlated
/// with the state-seeding stream (which reuses `GRID_INIT_SEED` directly).
const RNG_INIT_SEED: u64 = GRID_INIT_SEED ^ 0x5EED_5EED_5EED_5EED;

/// Param indices, matching `SirGridModel::from_params` so this model is a drop-in comparison
/// against the CPU one.
const PARAM_WIDTH: usize = 0;
const PARAM_HEIGHT: usize = 1;
const PARAM_INFECTION_RATE: usize = 2;
const PARAM_RECOVERY_RATE: usize = 3;
const PARAM_INITIAL_INFECTED_PCT: usize = 4;

const DEFAULT_DIM: u32 = 1024;
const DEFAULT_INFECTION_RATE: f32 = 0.3;
const DEFAULT_RECOVERY_RATE: f32 = 0.05;
const DEFAULT_INITIAL_INFECTED_PCT: f32 = 0.01;

const CELL_S: u32 = 0;
const CELL_I: u32 = 1;
const CELL_R: u32 = 2;

/// Matches `Params` in `step.wgsl` / `dims` in `reduce.wgsl`+`display.wgsl` (those only use the
/// leading `vec2<u32>`, which is why this struct's first 8 bytes alone are also valid as a
/// `vec2<u32>` uniform).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct StepParams {
    width: u32,
    height: u32,
    infection_rate: f32,
    recovery_rate: f32,
}

fn workgroup_counts(width: u32, height: u32) -> (u32, u32) {
    (width.div_ceil(WORKGROUP_SIZE), height.div_ceil(WORKGROUP_SIZE))
}

/// CPU-seeded initial S/I state, identical to `SirGridModel::init`: same PRNG, same traversal
/// order, same threshold — so GPU and CPU start from a bit-identical grid.
pub fn seed_cells(width: u32, height: u32, initial_infected_pct: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (initial_infected_pct * u32::MAX as f32) as u32;
    let mut cells = vec![0u32; (width as usize) * (height as usize)];
    for cell in &mut cells {
        rng = xorshift64(rng);
        *cell = u32::from(((rng >> 32) as u32) < threshold);
    }
    cells
}

/// Matches `pcg_hash` in `step.wgsl` bit-for-bit (u32 arithmetic wraps identically on both sides).
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Initial per-cell RNG state, seeded independently of the S/I state via `RNG_INIT_SEED`.
fn seed_rng_states(width: u32, height: u32, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..(width as usize) * (height as usize))
        .map(|idx| pcg_hash(seed32 ^ idx as u32))
        .collect()
}

pub struct GpuSirModel {
    ctx: GpuContext,
}

impl GpuSirModel {
    pub fn new(ctx: GpuContext) -> Self {
        Self { ctx }
    }
}

impl Model for GpuSirModel {
    type State = GpuSirState;

    fn name(&self) -> &'static str {
        "SIR Epidemic (GPU)"
    }

    fn id(&self) -> &'static str {
        "gpu_sir"
    }

    fn description(&self) -> &'static str {
        "Classic SIR compartmental model on a toroidal grid with Moore neighborhood, stepped entirely on the GPU"
    }

    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        vec![
            u32_param("grid_width", "Grid Width", DEFAULT_DIM, 1, 16_384),
            u32_param("grid_height", "Grid Height", DEFAULT_DIM, 1, 16_384),
            f32_param(
                "infection_rate",
                "Infection Rate",
                DEFAULT_INFECTION_RATE,
                0.0,
                1.0,
                Some(0.01),
            ),
            f32_param(
                "recovery_rate",
                "Recovery Rate",
                DEFAULT_RECOVERY_RATE,
                0.0,
                1.0,
                Some(0.01),
            ),
            f32_param(
                "initial_infected_pct",
                "Initial Infected %",
                DEFAULT_INITIAL_INFECTED_PCT,
                0.0,
                1.0,
                Some(0.001),
            ),
        ]
    }

    fn stat_descriptors(&self) -> Vec<StatDescriptor> {
        vec![
            StatDescriptor {
                label: "Susceptible",
                color: PALETTE[0],
            },
            StatDescriptor {
                label: "Infected",
                color: PALETTE[1],
            },
            StatDescriptor {
                label: "Recovered",
                color: PALETTE[2],
            },
        ]
    }

    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::Grid2D
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        GpuSirState::new(&self.ctx, params)
    }
}

pub struct GpuSirState {
    width: u32,
    height: u32,
    tick: u64,

    device: wgpu::Device,
    queue: wgpu::Queue,

    step_pipeline: wgpu::ComputePipeline,
    bind_a2b: wgpu::BindGroup,
    bind_b2a: wgpu::BindGroup,

    display_pipeline: wgpu::ComputePipeline,
    display_bind_a: wgpu::BindGroup,
    display_bind_b: wgpu::BindGroup,
    display: Arc<GpuDisplay>,

    reduce_pipeline: wgpu::ComputePipeline,
    reduce_bind_a: wgpu::BindGroup,
    reduce_bind_b: wgpu::BindGroup,
    sir_readback: CounterReadback<3>,

    /// `true` when `state_a` holds the current (latest) state.
    current_is_a: bool,
}

impl GpuSirState {
    #[expect(
        clippy::too_many_lines,
        reason = "this will be simplified soon after some GPU model abstraction"
    )]
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let width = extract_u32(params, PARAM_WIDTH, DEFAULT_DIM).max(1);
        let height = extract_u32(params, PARAM_HEIGHT, DEFAULT_DIM).max(1);
        let infection_rate = extract_f32(params, PARAM_INFECTION_RATE, DEFAULT_INFECTION_RATE);
        let recovery_rate = extract_f32(params, PARAM_RECOVERY_RATE, DEFAULT_RECOVERY_RATE);
        let initial_infected_pct = extract_f32(params, PARAM_INITIAL_INFECTED_PCT, DEFAULT_INITIAL_INFECTED_PCT);

        let initial_cells = seed_cells(width, height, initial_infected_pct, GRID_INIT_SEED);
        let initial_rng = seed_rng_states(width, height, RNG_INIT_SEED);
        let cell_count = (width as usize) * (height as usize);
        let state_size = (cell_count * std::mem::size_of::<u32>()) as u64;

        let make_buffer = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let state_a = make_buffer("gpu_sir_state_a", state_size);
        let state_b = make_buffer("gpu_sir_state_b", state_size);
        queue.write_buffer(&state_a, 0, bytemuck::cast_slice(&initial_cells));

        let rng_a = make_buffer("gpu_sir_rng_a", state_size);
        let rng_b = make_buffer("gpu_sir_rng_b", state_size);
        queue.write_buffer(&rng_a, 0, bytemuck::cast_slice(&initial_rng));

        let step_params = StepParams {
            width,
            height,
            infection_rate,
            recovery_rate,
        };
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_sir_params_buffer"),
            size: std::mem::size_of::<StepParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&step_params));

        // Display/reduce shaders only read `vec2<u32> dims`, i.e. the leading 8 bytes of
        // `StepParams`, so using a separate, smaller uniform buffer for them here.
        let dims_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_sir_dims_buffer"),
            size: (2 * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&dims_buffer, 0, bytemuck::cast_slice(&[width, height]));

        let DisplayTarget {
            view: display_view,
            display,
        } = build_display_target(device, ctx.target_format, width, height);

        let sir_readback = CounterReadback::new(device, "gpu_sir_counts");

        // --- Step pipeline ---
        let storage_entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let step_shader = device.create_shader_module(wgpu::include_wgsl!("step.wgsl"));
        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_sir_step_bind_group_layout"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, true),
                storage_entry(3, false),
                uniform_entry(4),
            ],
        });
        let make_step_bind_group = |label: &str,
                                    state_in: &wgpu::Buffer,
                                    state_out: &wgpu::Buffer,
                                    rng_in: &wgpu::Buffer,
                                    rng_out: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &step_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: state_in.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: state_out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: rng_in.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: rng_out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let bind_a2b = make_step_bind_group("gpu_sir_bind_a2b", &state_a, &state_b, &rng_a, &rng_b);
        let bind_b2a = make_step_bind_group("gpu_sir_bind_b2a", &state_b, &state_a, &rng_b, &rng_a);
        let step_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu_sir_step_pipeline_layout"),
            bind_group_layouts: &[&step_layout],
            push_constant_ranges: &[],
        });
        let step_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu_sir_step_pipeline"),
            layout: Some(&step_pipeline_layout),
            module: &step_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // --- Display pipeline ---
        let display_shader = device.create_shader_module(wgpu::include_wgsl!("display.wgsl"));
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_sir_display_bind_group_layout"),
            entries: &[
                storage_entry(0, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                uniform_entry(2),
            ],
        });
        let make_display_bind_group = |label: &str, state: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &display_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: state.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&display_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: dims_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let display_bind_a = make_display_bind_group("gpu_sir_display_bind_a", &state_a);
        let display_bind_b = make_display_bind_group("gpu_sir_display_bind_b", &state_b);
        let display_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu_sir_display_pipeline_layout"),
            bind_group_layouts: &[&display_layout],
            push_constant_ranges: &[],
        });
        let display_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu_sir_display_pipeline"),
            layout: Some(&display_pipeline_layout),
            module: &display_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // --- Reduce pipeline ---
        let reduce_shader = device.create_shader_module(wgpu::include_wgsl!("reduce.wgsl"));
        let reduce_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_sir_reduce_bind_group_layout"),
            entries: &[storage_entry(0, true), storage_entry(1, false), uniform_entry(2)],
        });
        let make_reduce_bind_group = |label: &str, state: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &reduce_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: state.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: sir_readback.binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: dims_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let reduce_bind_a = make_reduce_bind_group("gpu_sir_reduce_bind_a", &state_a);
        let reduce_bind_b = make_reduce_bind_group("gpu_sir_reduce_bind_b", &state_b);
        let reduce_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu_sir_reduce_pipeline_layout"),
            bind_group_layouts: &[&reduce_layout],
            push_constant_ranges: &[],
        });
        let reduce_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu_sir_reduce_pipeline"),
            layout: Some(&reduce_pipeline_layout),
            module: &reduce_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        Self {
            width,
            height,
            tick: 0,
            device: device.clone(),
            queue: queue.clone(),
            step_pipeline,
            bind_a2b,
            bind_b2a,
            display_pipeline,
            display_bind_a,
            display_bind_b,
            display,
            reduce_pipeline,
            reduce_bind_a,
            reduce_bind_b,
            sir_readback,
            current_is_a: true,
        }
    }

    fn current_display_bind_group(&self) -> &wgpu::BindGroup {
        if self.current_is_a {
            &self.display_bind_a
        } else {
            &self.display_bind_b
        }
    }

    fn current_reduce_bind_group(&self) -> &wgpu::BindGroup {
        if self.current_is_a {
            &self.reduce_bind_a
        } else {
            &self.reduce_bind_b
        }
    }
}

impl SimState for GpuSirState {
    /// This should technically never be called.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_sir_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        let counts = self.sir_readback.values();
        vec![
            stat("Susceptible", f64::from(counts[CELL_S as usize]), PALETTE[0]),
            stat("Infected", f64::from(counts[CELL_I as usize]), PALETTE[1]),
            stat("Recovered", f64::from(counts[CELL_R as usize]), PALETTE[2]),
        ]
    }

    /// Resizing or reseeding live is currently unsupported.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }

    fn population(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn heap_bytes(&self) -> usize {
        let cells = (self.width as usize) * (self.height as usize);
        // Two ping-ponged state buffers, two ping-ponged RNG buffers, plus the display texture.
        let buffers = cells * std::mem::size_of::<u32>() * 4;
        let display_texture = cells * 4;
        buffers + display_texture
    }
}

impl GpuSimState for GpuSirState {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if count == 0 {
            return;
        }
        let (wg_x, wg_y) = workgroup_counts(self.width, self.height);
        for i in 0..count {
            let bind_group = if self.current_is_a {
                &self.bind_a2b
            } else {
                &self.bind_b2a
            };
            let is_first = i == 0;
            let is_last = i == count - 1;
            let timestamp_writes =
                timestamps
                    .filter(|_| is_first || is_last)
                    .map(|query_set| wgpu::ComputePassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: is_first.then_some(0),
                        end_of_pass_write_index: is_last.then_some(1),
                    });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_sir_step_pass"),
                timestamp_writes,
            });
            pass.set_pipeline(&self.step_pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
            drop(pass);
            self.current_is_a = !self.current_is_a;
        }
        self.tick += u64::from(count);
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let (wg_x, wg_y) = workgroup_counts(self.width, self.height);

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_sir_display_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, self.current_display_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        self.sir_readback.encode_clear(encoder);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_sir_reduce_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.reduce_pipeline);
            pass.set_bind_group(0, self.current_reduce_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }
        self.sir_readback.encode_copy(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.sir_readback.begin_map();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        if block {
            self.sir_readback.poll_blocking(device);
        } else {
            self.sir_readback.poll(device);
        }
    }

    fn display(&self) -> Arc<GpuDisplay> {
        Arc::clone(&self.display)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::grid_engine::GridModelState;
    use henad_core::view::StatValue;

    use crate::sir::SirGridModel;

    pub(super) fn headless_context() -> Option<GpuContext> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_sir_test_device"),
            ..Default::default()
        }))
        .ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
    }

    pub(super) fn params(
        width: u32,
        height: u32,
        infection_rate: f32,
        recovery_rate: f32,
        initial_infected_pct: f32,
    ) -> Vec<ParamValue> {
        vec![
            ParamValue::U32(width),
            ParamValue::U32(height),
            ParamValue::F32(infection_rate),
            ParamValue::F32(recovery_rate),
            ParamValue::F32(initial_infected_pct),
        ]
    }

    fn sir_counts(state: &GpuSirState) -> (u64, u64, u64) {
        let stats = state.stats();
        let scalar = |entry: &StatEntry| match &entry.value {
            StatValue::Scalar(v) => *v as u64,
            other => panic!("expected a scalar stat, got {other:?}"),
        };
        (scalar(&stats[0]), scalar(&stats[1]), scalar(&stats[2]))
    }

    /// Drives display + reduce + readback exactly as the sim thread's one-shot snapshot path does.
    fn refresh_stats(ctx: &GpuContext, state: &mut GpuSirState) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    fn step_once(ctx: &GpuContext, state: &mut GpuSirState) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 1, None);
        ctx.queue.submit(Some(encoder.finish()));
    }

    /// S+I+R must hold exactly at every tick regardless of how the per-cell RNG streams behave.
    #[test]
    fn population_conserved_over_many_ticks() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping population_conserved_over_many_ticks: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let mut state = GpuSirState::new(&ctx, &params(width, height, 0.3, 0.05, 0.1));
        let total = u64::from(width) * u64::from(height);

        for tick in 0..50 {
            refresh_stats(&ctx, &mut state);
            let (s, i, r) = sir_counts(&state);
            assert_eq!(s + i + r, total, "S+I+R must equal total population at tick {tick}");
            step_once(&ctx, &mut state);
        }
    }

    /// With `infection_rate == 0`, S can never lose a member.
    #[test]
    fn zero_infection_rate_freezes_susceptible_count() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping zero_infection_rate_freezes_susceptible_count: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let mut state = GpuSirState::new(&ctx, &params(width, height, 0.0, 0.05, 0.2));

        refresh_stats(&ctx, &mut state);
        let (initial_s, _, _) = sir_counts(&state);

        for tick in 0..20 {
            step_once(&ctx, &mut state);
            refresh_stats(&ctx, &mut state);
            let (s, _, _) = sir_counts(&state);
            assert_eq!(
                s, initial_s,
                "susceptible count must not change at tick {tick} when infection_rate is 0"
            );
        }
    }

    /// With `recovery_rate == 0`, I can never lose a member,
    /// so with any positive infection rate, I must be monotonically non-decreasing.
    #[test]
    fn zero_recovery_rate_keeps_infected_non_decreasing() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping zero_recovery_rate_keeps_infected_non_decreasing: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let mut state = GpuSirState::new(&ctx, &params(width, height, 0.5, 0.0, 0.1));

        refresh_stats(&ctx, &mut state);
        let (_, mut prev_i, _) = sir_counts(&state);

        for tick in 0..20 {
            step_once(&ctx, &mut state);
            refresh_stats(&ctx, &mut state);
            let (_, i, r) = sir_counts(&state);
            assert!(
                i >= prev_i,
                "infected count must not decrease at tick {tick} when recovery_rate is 0"
            );
            assert_eq!(r, 0, "no cell can reach R when recovery_rate is 0 (tick {tick})");
            prev_i = i;
        }
    }

    /// Initial seeding uses the same PRNG, traversal order, and threshold as the CPU model, so the
    /// tick-0 compartment counts (before any RNG-dependent step) must match exactly.
    #[test]
    fn initial_seed_matches_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping initial_seed_matches_cpu_model: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let p = params(width, height, 0.3, 0.05, 0.2);

        let mut gpu = GpuSirState::new(&ctx, &p);
        let cpu = GridModelState::<SirGridModel>::from_params(&p);

        refresh_stats(&ctx, &mut gpu);
        let (gpu_s, gpu_i, gpu_r) = sir_counts(&gpu);

        let cpu_scalar = |entry: &StatEntry| match &entry.value {
            StatValue::Scalar(v) => *v as u64,
            other => panic!("expected a scalar stat, got {other:?}"),
        };
        let cpu_stats = cpu.stats();
        assert_eq!(
            gpu_s,
            cpu_scalar(&cpu_stats[0]),
            "initial susceptible count must match the CPU model"
        );
        assert_eq!(
            gpu_i,
            cpu_scalar(&cpu_stats[1]),
            "initial infected count must match the CPU model"
        );
        assert_eq!(
            gpu_r,
            cpu_scalar(&cpu_stats[2]),
            "initial recovered count must be 0 on both backends"
        );
    }

    #[test]
    fn population_is_total_cells() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping population_is_total_cells: no adapter");
            return;
        };

        let (width, height) = (32u32, 16u32);
        let state = GpuSirState::new(&ctx, &params(width, height, 0.3, 0.05, 0.1));
        assert_eq!(state.population(), u64::from(width) * u64::from(height));
    }
}

/// Tests that drive the real [`henad_compute::gpu::GpuSimThread`], similar to
/// `gpu_game_of_life`'s `runner_tests`.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod runner_tests {
    use std::time::{Duration, Instant};

    use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
    use henad_compute::snapshot::{Snapshot, SnapshotView};
    use henad_core::view::StatValue;

    use super::GpuSirState;
    use super::tests::{headless_context, params};
    use crate::registry::{ModelState, model_registry};

    fn wait_for(thread: &mut GpuSimThread, timeout: Duration, pred: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(snap) = thread.take_snapshot() {
                if pred(&snap) {
                    return Some(snap);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        None
    }

    fn sir_counts(snap: &Snapshot) -> (u64, u64, u64) {
        let scalar = |entry: &henad_core::view::StatEntry| match &entry.value {
            StatValue::Scalar(v) => *v as u64,
            other => panic!("expected a scalar stat, got {other:?}"),
        };
        (scalar(&snap.stats[0]), scalar(&snap.stats[1]), scalar(&snap.stats[2]))
    }

    #[test]
    fn gpu_thread_publishes_gpu_snapshots_and_conserves_population() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_publishes_gpu_snapshots_and_conserves_population: no adapter");
            return;
        };

        let (width, height) = (128u32, 128u32);
        let state = GpuSirState::new(&ctx, &params(width, height, 0.3, 0.05, 0.1));
        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default());

        let initial = wait_for(&mut thread, Duration::from_secs(5), |_| true)
            .expect("the GPU thread must publish an initial snapshot before Play");
        assert!(matches!(initial.view, SnapshotView::Gpu(_)));
        let total = initial.population;
        let (s, i, r) = sir_counts(&initial);
        assert_eq!(s + i + r, total, "initial snapshot must already conserve population");

        thread.play();
        let running = wait_for(&mut thread, Duration::from_secs(5), |snap| snap.tick > 0)
            .expect("playing must advance the tick counter and publish fresh snapshots");
        assert!(matches!(running.view, SnapshotView::Gpu(_)));
        let (s, i, r) = sir_counts(&running);
        assert_eq!(s + i + r, total, "population must stay conserved while running");

        thread.pause();
        drop(thread);
    }

    #[test]
    fn registry_with_gpu_context_offers_a_drivable_gpu_sir_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping registry_with_gpu_context_offers_a_drivable_gpu_sir_model: no adapter");
            return;
        };

        let entries = model_registry(Some(ctx.clone()));
        let entry = entries
            .iter()
            .find(|e| e.id == "gpu_sir")
            .expect("a GPU context must make the GPU SIR model selectable");

        let ModelState::Gpu(mut state) = (entry.create)(&params(32, 32, 0.3, 0.05, 0.1)) else {
            panic!("the GPU SIR entry's factory must yield ModelState::Gpu, not ModelState::Cpu");
        };

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 4, None);
        ctx.queue.submit(Some(encoder.finish()));
        assert_eq!(state.tick(), 4, "the registry-built state must be steppable");
    }
}
