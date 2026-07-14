//! GPU-accelerated Game of Life: the same rules as [`crate::game_of_life`], but with all state
//! resident in GPU storage buffers.
//!
//! # Why this implements `Model` + `SimState` by hand
//!
//! `GridModel` exists for models expressible as "a pure `step_cell` function, run over a `Grid2D`
//! by rayon". This model is not that: there is no per-cell Rust function, no `Grid2D`, and no
//! rayon — the step *is* a WGSL shader, and the engine's job is to batch dispatches rather than
//! to parallelize a loop. So, exactly like `boids` (which doesn't fit `GridModel` either), it
//! implements the full `Model`/`SimState` pair directly and leans on the batching GPU runner in
//! `henad_compute::gpu`.
//!
//! A shared `GpuGridModel` trait is deliberately *not* extracted here: with one GPU model there
//! is nothing to generalize from. That happens once a second one (GPU SIR) exists.
//!
//! # State layout
//!
//! Two `array<u32>` storage buffers (one cell per element, no bit packing) are ping-ponged: each
//! step reads one and writes the other. The grid never leaves the GPU. What the CPU sees is only:
//! an RGBA display texture (written by `display.wgsl` at the display cadence, sampled by the
//! viewport) and a single `u32` alive-count (produced by `reduce.wgsl`, read back asynchronously).
//!
//! # Correctness oracle
//!
//! Seeding uses the same `xorshift64` PRNG, the same `GRID_INIT_SEED`, the same traversal order,
//! and the same density threshold as the CPU `GameOfLifeModel`. Given identical params the two
//! backends therefore start from a **bit-identical** grid and must agree forever after — which is
//! what `tests::gpu_alive_count_matches_cpu_model` checks.

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

use crate::game_of_life::PALETTE;

const WORKGROUP_SIZE: u32 = 16;

/// Param indices, matching `grid_model_param_descriptors` + `GameOfLifeModel::param_descriptors`
/// so this model is a drop-in comparison against the CPU one.
const PARAM_WIDTH: usize = 0;
const PARAM_HEIGHT: usize = 1;
const PARAM_DENSITY: usize = 2;

const DEFAULT_DIM: u32 = 1024;
const DEFAULT_DENSITY: f32 = 0.3;

/// Grid dimensions, laid out to match `dims: vec2<u32>` in the shaders.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GridDims {
    width: u32,
    height: u32,
}

fn workgroup_counts(width: u32, height: u32) -> (u32, u32) {
    (width.div_ceil(WORKGROUP_SIZE), height.div_ceil(WORKGROUP_SIZE))
}

/// CPU-seeded random fill at the given density.
///
/// Deliberately identical to `GameOfLifeModel::init`: same PRNG, same traversal order, same
/// threshold. See the module docs — this is what makes the CPU model a usable oracle.
pub fn seed_random(width: u32, height: u32, density: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (density * u32::MAX as f32) as u32;
    let mut cells = vec![0u32; (width as usize) * (height as usize)];
    for cell in &mut cells {
        rng = xorshift64(rng);
        *cell = u32::from(((rng >> 32) as u32) < threshold);
    }
    cells
}

/// The model descriptor. Holds a cloned [`GpuContext`], which is how the registry hands the
/// device down to a model without any global state.
pub struct GpuGameOfLifeModel {
    ctx: GpuContext,
}

impl GpuGameOfLifeModel {
    pub fn new(ctx: GpuContext) -> Self {
        Self { ctx }
    }
}

impl Model for GpuGameOfLifeModel {
    type State = GpuGameOfLifeState;

    fn name(&self) -> &'static str {
        "Game of Life (GPU)"
    }

    fn id(&self) -> &'static str {
        "gpu_game_of_life"
    }

    fn description(&self) -> &'static str {
        "Conway's Game of Life on a toroidal grid, stepped entirely on the GPU"
    }

    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        vec![
            u32_param("grid_width", "Grid Width", DEFAULT_DIM, 1, 16_384),
            u32_param("grid_height", "Grid Height", DEFAULT_DIM, 1, 16_384),
            f32_param("density", "Initial Density", DEFAULT_DENSITY, 0.0, 1.0, Some(0.01)),
        ]
    }

    fn stat_descriptors(&self) -> Vec<StatDescriptor> {
        vec![StatDescriptor {
            label: "Alive",
            color: PALETTE[1],
        }]
    }

    /// Still a 2D grid — it just gets its pixels from a texture instead of a cell buffer. The UI
    /// branches on the *snapshot* variant, not on this hint.
    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::Grid2D
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        GpuGameOfLifeState::new(&self.ctx, params)
    }
}

/// GPU-resident Game of Life state. Owned exclusively by the GPU sim thread once spawned.
pub struct GpuGameOfLifeState {
    width: u32,
    height: u32,
    tick: u64,

    device: wgpu::Device,
    queue: wgpu::Queue,

    // The bind groups below hold their own strong references to these buffers, so production code
    // never needs the handles again — every param is construction-time, so there is no reseed and
    // no resize. Tests do need them, to stamp in a known pattern and to read the grid back.
    #[cfg(test)]
    buffer_a: wgpu::Buffer,
    #[cfg(test)]
    buffer_b: wgpu::Buffer,

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
    alive_readback: CounterReadback<1>,

    /// `true` when `buffer_a` holds the current (latest) state.
    current_is_a: bool,
}

impl GpuGameOfLifeState {
    #[expect(
        clippy::too_many_lines,
        reason = "one-shot resource setup: a linear sequence of wgpu object creation calls that would only be split up by moving the same sequence into more functions"
    )]
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let width = extract_u32(params, PARAM_WIDTH, DEFAULT_DIM).max(1);
        let height = extract_u32(params, PARAM_HEIGHT, DEFAULT_DIM).max(1);
        let density = extract_f32(params, PARAM_DENSITY, DEFAULT_DENSITY);

        let initial_cells = seed_random(width, height, density, GRID_INIT_SEED);
        let cell_count = (width as usize) * (height as usize);
        let buffer_size = (cell_count * std::mem::size_of::<u32>()) as u64;

        let make_state_buffer = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: buffer_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let buffer_a = make_state_buffer("gpu_gol_buffer_a");
        let buffer_b = make_state_buffer("gpu_gol_buffer_b");
        queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(&initial_cells));

        let dims_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_dims_buffer"),
            size: std::mem::size_of::<GridDims>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&dims_buffer, 0, bytemuck::bytes_of(&GridDims { width, height }));

        let DisplayTarget {
            view: display_view,
            display,
        } = build_display_target(device, ctx.target_format, width, height);

        let alive_readback = CounterReadback::new(device, "gpu_gol_alive");

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
            label: Some("gpu_gol_step_bind_group_layout"),
            entries: &[storage_entry(0, true), storage_entry(1, false), uniform_entry(2)],
        });
        let make_step_bind_group = |label: &str, current: &wgpu::Buffer, next: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &step_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: current.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: next.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: dims_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let bind_a2b = make_step_bind_group("gpu_gol_bind_a2b", &buffer_a, &buffer_b);
        let bind_b2a = make_step_bind_group("gpu_gol_bind_b2a", &buffer_b, &buffer_a);
        let step_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu_gol_step_pipeline_layout"),
            bind_group_layouts: &[&step_layout],
            push_constant_ranges: &[],
        });
        let step_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu_gol_step_pipeline"),
            layout: Some(&step_pipeline_layout),
            module: &step_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // --- Display pipeline (state -> RGBA texture) ---
        let display_shader = device.create_shader_module(wgpu::include_wgsl!("display.wgsl"));
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_gol_display_bind_group_layout"),
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
        let display_bind_a = make_display_bind_group("gpu_gol_display_bind_a", &buffer_a);
        let display_bind_b = make_display_bind_group("gpu_gol_display_bind_b", &buffer_b);
        let display_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu_gol_display_pipeline_layout"),
            bind_group_layouts: &[&display_layout],
            push_constant_ranges: &[],
        });
        let display_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu_gol_display_pipeline"),
            layout: Some(&display_pipeline_layout),
            module: &display_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        // --- Reduce pipeline (state -> alive count) ---
        let reduce_shader = device.create_shader_module(wgpu::include_wgsl!("reduce.wgsl"));
        let reduce_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_gol_reduce_bind_group_layout"),
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
                        resource: alive_readback.binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: dims_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let reduce_bind_a = make_reduce_bind_group("gpu_gol_reduce_bind_a", &buffer_a);
        let reduce_bind_b = make_reduce_bind_group("gpu_gol_reduce_bind_b", &buffer_b);
        let reduce_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu_gol_reduce_pipeline_layout"),
            bind_group_layouts: &[&reduce_layout],
            push_constant_ranges: &[],
        });
        let reduce_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu_gol_reduce_pipeline"),
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
            #[cfg(test)]
            buffer_a,
            #[cfg(test)]
            buffer_b,
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
            alive_readback,
            current_is_a: true,
        }
    }

    /// The buffer holding the latest state. Test-only on purpose: production code never reads the
    /// grid back to the CPU.
    #[cfg(test)]
    fn current_buffer(&self) -> &wgpu::Buffer {
        if self.current_is_a {
            &self.buffer_a
        } else {
            &self.buffer_b
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

impl SimState for GpuGameOfLifeState {
    /// Single-step fallback for callers that only have a `SimState`. The GPU sim thread does
    /// **not** go through this — it batches many steps into one submission via
    /// [`GpuSimState::encode_steps`], which is the entire point of the GPU backend. Stepping one
    /// tick per submission like this is correct but slow, so it exists mainly to honour the trait.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_gol_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        vec![stat("Alive", f64::from(self.alive_readback.values()[0]), PALETTE[1])]
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
        let state_buffers = cells * std::mem::size_of::<u32>() * 2;
        let display_texture = cells * 4;
        state_buffers + display_texture
    }
}

impl GpuSimState for GpuGameOfLifeState {
    /// Records `count` step dispatches into `encoder`, one compute pass per step.
    ///
    /// Each step is a read-after-write hazard on the ping-ponged state buffers, and wgpu only
    /// inserts synchronization barriers *between* passes, not between dispatches within a single
    /// pass — so this deliberately opens one pass per step rather than looping dispatches inside
    /// one pass (which would read stale data). Batching still happens at the *submission* level:
    /// the caller records all `count` passes into one encoder and submits once, which is what
    /// keeps submission overhead low.
    ///
    /// If `timestamps` is `Some`, the first pass's beginning and the last pass's end are stamped
    /// into query indices 0 and 1 so the caller can measure GPU time for the whole batch.
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
            // A `ComputePassTimestampWrites` requires at least one of the two indices to be
            // `Some`, so only the first and last passes of the batch get one — everything in
            // between gets `None`.
            let timestamp_writes =
                timestamps
                    .filter(|_| is_first || is_last)
                    .map(|query_set| wgpu::ComputePassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: is_first.then_some(0),
                        end_of_pass_write_index: is_last.then_some(1),
                    });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_gol_step_pass"),
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
                label: Some("gpu_gol_display_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, self.current_display_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        // Clear -> accumulate -> copy out. wgpu inserts the barriers between these because they
        // are separate passes/copies within the one encoder.
        self.alive_readback.encode_clear(encoder);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_gol_reduce_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.reduce_pipeline);
            pass.set_bind_group(0, self.current_reduce_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }
        self.alive_readback.encode_copy(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.alive_readback.begin_map();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        if block {
            self.alive_readback.poll_blocking(device);
        } else {
            self.alive_readback.poll(device);
        }
    }

    fn display(&self) -> Arc<GpuDisplay> {
        Arc::clone(&self.display)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::gpu::timing::TimestampQuery;
    use henad_compute::grid_engine::GridModelState;
    use henad_core::view::StatValue;

    use crate::game_of_life::GameOfLifeModel;

    pub(super) fn headless_context() -> Option<GpuContext> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_gol_test_device"),
            ..Default::default()
        }))
        .ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
    }

    fn read_buffer(ctx: &GpuContext, buffer: &wgpu::Buffer, len: usize) -> Vec<u32> {
        let size = (len * std::mem::size_of::<u32>()) as u64;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_test_readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        ctx.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = flume::bounded(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            drop(tx.send(result));
        });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll failed");
        rx.recv()
            .expect("map_async channel closed")
            .expect("buffer mapping failed");

        let data = slice.get_mapped_range();
        let cells: Vec<u32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        staging.unmap();
        cells
    }

    pub(super) fn params(width: u32, height: u32, density: f32) -> Vec<ParamValue> {
        vec![
            ParamValue::U32(width),
            ParamValue::U32(height),
            ParamValue::F32(density),
        ]
    }

    fn reported_alive(state: &GpuGameOfLifeState) -> u64 {
        match state.stats().first().map(|s| s.value.clone()) {
            Some(StatValue::Scalar(v)) => v as u64,
            other => panic!("expected a scalar Alive stat, got {other:?}"),
        }
    }

    /// Drives display + reduce + readback exactly as the sim thread's one-shot snapshot path does.
    fn refresh_stats(ctx: &GpuContext, state: &mut GpuGameOfLifeState) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    fn cpu_reference_step(cells: &[u32], width: u32, height: u32) -> Vec<u32> {
        use henad_core::grid_model::GridModel as _;
        let (w, h) = (width as usize, height as usize);
        let mut next = vec![0u32; w * h];
        let mut rng = 0u64;
        for y in 0..h {
            for x in 0..w {
                let xm1 = (x + w - 1) % w;
                let xp1 = (x + 1) % w;
                let ym1 = (y + h - 1) % h;
                let yp1 = (y + 1) % h;
                let neighbor_coords = [
                    (xm1, ym1),
                    (x, ym1),
                    (xp1, ym1),
                    (xm1, y),
                    (xp1, y),
                    (xm1, yp1),
                    (x, yp1),
                    (xp1, yp1),
                ];
                let neighbors: Vec<u8> = neighbor_coords
                    .iter()
                    .map(|&(nx, ny)| cells[ny * w + nx] as u8)
                    .collect();
                let cell = cells[y * w + x] as u8;
                next[y * w + x] = u32::from(GameOfLifeModel::step_cell(cell, &neighbors, &(), &mut rng) == 1);
            }
        }
        next
    }

    /// Exercises the exact code path the sim thread uses: several steps recorded into one
    /// encoder and submitted once, not one submit per step.
    #[test]
    fn gpu_matches_cpu_reference() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_matches_cpu_reference: no wgpu adapter available");
            return;
        };

        let (width, height) = (64, 64);
        let mut state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.3));
        let initial = seed_random(width, height, 0.3, GRID_INIT_SEED);

        let ticks = 5;
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, ticks, None);
        ctx.queue.submit(Some(encoder.finish()));

        let mut cpu_state = initial;
        for _ in 0..ticks {
            cpu_state = cpu_reference_step(&cpu_state, width, height);
        }

        let gpu_state = read_buffer(&ctx, state.current_buffer(), (width as usize) * (height as usize));

        assert_eq!(
            gpu_state, cpu_state,
            "GPU state after a batch of {ticks} steps in one submission must match {ticks} CPU steps"
        );
        assert_eq!(
            state.tick(),
            u64::from(ticks),
            "a batch of {ticks} steps must advance the tick counter by {ticks}"
        );
    }

    #[test]
    fn blinker_returns_after_two_ticks() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping blinker_returns_after_two_ticks: no wgpu adapter available");
            return;
        };

        // Density 0 => an all-dead grid we can stamp a blinker onto.
        let (width, height) = (10u32, 10u32);
        let mut state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.0));

        let mut initial = vec![0u32; (width * height) as usize];
        // Horizontal blinker in the middle, away from the toroidal edges.
        initial[5 * width as usize + 3] = 1;
        initial[5 * width as usize + 4] = 1;
        initial[5 * width as usize + 5] = 1;
        ctx.queue
            .write_buffer(&state.buffer_a, 0, bytemuck::cast_slice(&initial));

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 2, None);
        ctx.queue.submit(Some(encoder.finish()));

        let gpu_state = read_buffer(&ctx, state.current_buffer(), (width as usize) * (height as usize));

        assert_eq!(
            gpu_state, initial,
            "blinker must return to its original state after 2 ticks"
        );
    }

    /// The GPU reduction is the *only* source of the alive count, so this pins it against a known
    /// pattern: a blinker is 3 alive cells at every tick of its period-2 cycle.
    #[test]
    fn gpu_reduction_counts_known_pattern() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_reduction_counts_known_pattern: no wgpu adapter available");
            return;
        };

        let (width, height) = (10u32, 10u32);
        let mut state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.0));

        let mut initial = vec![0u32; (width * height) as usize];
        initial[5 * width as usize + 3] = 1;
        initial[5 * width as usize + 4] = 1;
        initial[5 * width as usize + 5] = 1;
        ctx.queue
            .write_buffer(&state.buffer_a, 0, bytemuck::cast_slice(&initial));

        for tick in 0..4 {
            refresh_stats(&ctx, &mut state);
            assert_eq!(
                reported_alive(&state),
                3,
                "a blinker has exactly 3 alive cells at every tick (checked at tick {tick})"
            );

            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            state.encode_steps(&mut encoder, 1, None);
            ctx.queue.submit(Some(encoder.finish()));
        }
    }

    /// End-to-end agreement with the CPU model, which is the real correctness oracle: identical
    /// params seed a bit-identical grid, so the alive count the GPU reduces on-device must equal
    /// the alive count the CPU model counts in Rust, tick for tick.
    #[test]
    fn gpu_alive_count_matches_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_alive_count_matches_cpu_model: no wgpu adapter available");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let p = params(width, height, 0.3);

        let mut gpu = GpuGameOfLifeState::new(&ctx, &p);
        let mut cpu = GridModelState::<GameOfLifeModel>::from_params(&p);

        let cpu_alive = |cpu: &GridModelState<GameOfLifeModel>| -> u64 {
            match cpu.stats().first().map(|s| s.value.clone()) {
                Some(StatValue::Scalar(v)) => v as u64,
                other => panic!("expected a scalar Alive stat, got {other:?}"),
            }
        };

        for tick in 0..10 {
            refresh_stats(&ctx, &mut gpu);
            assert_eq!(
                reported_alive(&gpu),
                cpu_alive(&cpu),
                "GPU-reduced alive count must match the CPU model's at tick {tick}"
            );
            assert_eq!(gpu.tick(), cpu.tick(), "tick counters must stay in step");

            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            gpu.encode_steps(&mut encoder, 1, None);
            ctx.queue.submit(Some(encoder.finish()));
            cpu.step();
        }

        // Sanity: the fixtures above would also pass if both counts were stuck at zero.
        refresh_stats(&ctx, &mut gpu);
        assert!(
            reported_alive(&gpu) > 0,
            "a 64x64 grid seeded at density 0.3 must have live cells after 10 ticks"
        );
    }

    /// Like `headless_context`, but requests `TIMESTAMP_QUERY` explicitly (mirroring what the app
    /// does when the adapter supports it), since the default test device requests no features.
    fn headless_timing_context() -> Option<GpuContext> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_gol_timing_test_device"),
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
    }

    /// Regression test for "GPU time/step flickers to 0/None during a sustained run": runs many
    /// batches back to back exactly like `GpuSimLoop::step_batch` records/resolves/reads a
    /// timestamped batch, but takes a reading on *every* iteration instead of once/second, to
    /// shake out an intermittent zero or failed readback far more aggressively than the real
    /// once-per-second cadence would in a short-lived interactive session.
    ///
    /// Lives here rather than next to `TimestampQuery` in `henad-compute` because it needs a
    /// concrete GPU model to stamp real batches with, and `henad-compute` has none by design.
    #[test]
    fn gpu_timing_readback_is_stable_over_many_batches() {
        let Some(ctx) = headless_timing_context() else {
            log::warn!(
                "skipping gpu_timing_readback_is_stable_over_many_batches: \
                 no adapter with TIMESTAMP_QUERY available"
            );
            return;
        };

        let (width, height) = (256u32, 256u32);
        let mut state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.3));

        let tq = TimestampQuery::new(&ctx.device, &ctx.queue).expect("device has TIMESTAMP_QUERY");
        let batch_size = 64;
        let iterations = 200;
        let mut zero_count = 0usize;
        let mut none_count = 0usize;

        for _ in 0..iterations {
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            state.encode_steps(&mut encoder, batch_size, Some(tq.query_set()));
            let write_submission = ctx.queue.submit(Some(encoder.finish()));

            tq.resolve_after(&ctx.device, &ctx.queue, write_submission);

            match tq.read_gpu_us_per_step(&ctx.device, batch_size) {
                Some(us) if us <= 0.0 => zero_count += 1,
                Some(_) => {}
                None => none_count += 1,
            }
        }

        assert_eq!(
            none_count, 0,
            "readback failed (returned None) on {none_count}/{iterations} back-to-back batches"
        );
        assert_eq!(
            zero_count, 0,
            "readback returned 0 (end timestamp <= start timestamp) on \
             {zero_count}/{iterations} back-to-back batches"
        );
    }

    /// `population()` reports total cells (like `GridModelState`), *not* the alive count — the
    /// alive count is a stat. Pinned because it is an easy thing to conflate.
    #[test]
    fn population_is_total_cells_not_alive_count() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping population_is_total_cells_not_alive_count: no adapter");
            return;
        };

        let (width, height) = (32u32, 16u32);
        let state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.3));
        assert_eq!(state.population(), u64::from(width) * u64::from(height));

        let cpu = GridModelState::<GameOfLifeModel>::from_params(&params(width, height, 0.3));
        assert_eq!(
            state.population(),
            cpu.population(),
            "GPU and CPU Game of Life must agree on what 'population' means"
        );
    }
}

/// Tests that drive the real [`henad_compute::gpu::GpuSimThread`] rather than poking the state
/// directly — i.e. the integration surface the GUI actually uses: registry -> `ModelState::Gpu` ->
/// spawn thread -> play/pause/step -> read the published `Snapshot`.
///
/// These exist because the GUI itself cannot be driven headlessly. Everything the manual "select
/// the GPU model, press play, check the stat, switch away and back" check would exercise is
/// covered here except the final texture *sampling* (the egui paint callback), which needs a
/// surface to draw into.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod runner_tests {
    use std::time::{Duration, Instant};

    use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
    use henad_compute::snapshot::{Snapshot, SnapshotView};
    use henad_core::view::StatValue;

    use super::GpuGameOfLifeState;
    use super::tests::{headless_context, params};
    use crate::registry::{ModelState, model_registry};

    /// Spins until the thread publishes a snapshot satisfying `pred`, or the deadline passes.
    /// The GPU thread publishes on a ~16ms wall-clock cadence, so polling is the honest way to
    /// wait for one — a fixed sleep would be flakier.
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

    fn alive(snap: &Snapshot) -> u64 {
        match snap.stats.first().map(|s| s.value.clone()) {
            Some(StatValue::Scalar(v)) => v as u64,
            other => panic!("expected a scalar Alive stat, got {other:?}"),
        }
    }

    /// The end-to-end path the viewport depends on: a GPU-backed model must publish snapshots
    /// carrying `SnapshotView::Gpu` (never a `Grid`), because the viewport branches on exactly
    /// that variant to choose between uploading a `ColorImage` and issuing the paint callback.
    #[test]
    fn gpu_thread_publishes_gpu_snapshots_and_runs() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_publishes_gpu_snapshots_and_runs: no adapter");
            return;
        };

        let (width, height) = (128u32, 128u32);
        let state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.3));
        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default());

        // The thread publishes an initial snapshot before anything runs, so the viewport shows the
        // seeded grid the moment the model is loaded rather than staying blank until Play.
        let initial = wait_for(&mut thread, Duration::from_secs(5), |_| true)
            .expect("the GPU thread must publish an initial snapshot before Play");

        assert!(
            matches!(initial.view, SnapshotView::Gpu(_)),
            "a GPU model must publish SnapshotView::Gpu — the viewport branches on this variant"
        );
        assert_eq!(initial.tick, 0, "the initial snapshot is pre-step");
        assert_eq!(
            initial.population,
            u64::from(width) * u64::from(height),
            "population must report total cells"
        );
        let initial_alive = alive(&initial);
        assert!(
            initial_alive > 0 && initial_alive < initial.population,
            "a density-0.3 seed must be neither empty nor full, got {initial_alive} alive"
        );

        thread.play();
        let running = wait_for(&mut thread, Duration::from_secs(5), |s| s.tick > 0)
            .expect("playing must advance the tick counter and publish fresh snapshots");
        assert!(matches!(running.view, SnapshotView::Gpu(_)));
        assert!(alive(&running) > 0, "the sim must not have died out");

        thread.pause();
        drop(thread);
    }

    /// The manual check that cannot be clicked headlessly: select GPU, play, switch to another
    /// model, switch back. Each switch drops the `GpuSimThread` (shutting down and joining its OS
    /// thread, releasing its buffers/pipelines) and builds a fresh one from the *same* injected
    /// context. A thread that fails to join, or GPU state left dangling in the shared context,
    /// shows up here as a hang or a panic.
    #[test]
    fn gpu_thread_teardown_and_respawn_is_clean() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_teardown_and_respawn_is_clean: no adapter");
            return;
        };

        for round in 0..3 {
            let state = GpuGameOfLifeState::new(&ctx, &params(64, 64, 0.3));
            let mut thread = GpuSimThread::new(ctx.clone(), Box::new(state), GpuBatchSettings::default());
            thread.play();
            let snap = wait_for(&mut thread, Duration::from_secs(5), |s| s.tick > 0)
                .unwrap_or_else(|| panic!("round {round}: a respawned GPU thread must step"));
            assert!(matches!(snap.view, SnapshotView::Gpu(_)));
            // Dropped mid-run, exactly as a model switch does — not from a paused state.
            drop(thread);
        }
    }

    /// The population/stat oracle check, driven through the *published snapshot* rather than by
    /// calling `stats()` on the state directly: a blinker is 3 alive cells on every tick of its
    /// period-2 cycle, so the number the stats panel would show must be 3 at every step.
    ///
    /// Exercises `step_once` -> `encode_snapshot_passes` -> blocking readback -> publish, which is
    /// the path the Step button takes.
    #[test]
    fn stepping_a_blinker_reports_three_alive_every_tick() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping stepping_a_blinker_reports_three_alive_every_tick: no adapter");
            return;
        };

        let (width, height) = (16u32, 16u32);
        // Density 0 => an all-dead grid; stamp the blinker in before the state is handed to the
        // thread (after that, the thread owns it exclusively).
        let state = GpuGameOfLifeState::new(&ctx, &params(width, height, 0.0));
        let mut cells = vec![0u32; (width * height) as usize];
        for x in 6..9 {
            cells[8 * width as usize + x] = 1;
        }
        ctx.queue.write_buffer(&state.buffer_a, 0, bytemuck::cast_slice(&cells));

        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default());

        let initial = wait_for(&mut thread, Duration::from_secs(5), |_| true).expect("initial snapshot");
        assert_eq!(alive(&initial), 3, "a blinker starts with 3 alive cells");

        for step in 1..=4u64 {
            thread.step_once();
            let snap = wait_for(&mut thread, Duration::from_secs(5), |s| s.tick == step)
                .unwrap_or_else(|| panic!("step {step}: no snapshot published at tick {step}"));
            assert_eq!(
                alive(&snap),
                3,
                "a blinker has exactly 3 alive cells at every tick (tick {step})"
            );
        }
    }

    /// With a context, the GPU entry is offered by the registry, its factory yields a
    /// `ModelState::Gpu` (so `HenadApp` routes it to the GPU thread rather than the CPU one), and
    /// the state it builds is drivable. The mirror of
    /// `registry::tests::registry_without_gpu_context_offers_no_gpu_models`.
    #[test]
    fn registry_with_gpu_context_offers_a_drivable_gpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping registry_with_gpu_context_offers_a_drivable_gpu_model: no adapter");
            return;
        };

        let entries = model_registry(Some(ctx.clone()));
        let entry = entries
            .iter()
            .find(|e| e.id == "gpu_game_of_life")
            .expect("a GPU context must make the GPU model selectable");

        // Note there is no context argument here: the registry closure captured its own clone.
        let ModelState::Gpu(mut state) = (entry.create)(&params(32, 32, 0.3)) else {
            panic!("the GPU entry's factory must yield ModelState::Gpu, not ModelState::Cpu");
        };

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 4, None);
        ctx.queue.submit(Some(encoder.finish()));
        assert_eq!(state.tick(), 4, "the registry-built state must be steppable");
    }
}
