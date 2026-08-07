//! Generic engine turning any [`GpuGridModel`] into a runnable [`GpuSimState`].
//!
//! Compare with [`crate::grid_engine`].

use std::marker::PhantomData;
use std::sync::Arc;

use henad_core::gpu_grid_model::GpuGridModel;
use henad_core::model::{Model, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatEntry, stat_entries};

use crate::gpu::GpuContext;
use crate::gpu::display::{DisplayTarget, GpuDisplay, build_display_target};
use crate::gpu::readback::CounterReadback;
use crate::gpu::sim_thread::GpuSimState;

/// One ping-ponged pair of storage buffers.
struct BufferPair {
    a: wgpu::Buffer,
    b: wgpu::Buffer,
}

/// The `Model` half for a [`GpuGridModel`]: metadata plus a state factory.
///
/// Holds a cloned [`GpuContext`], which is how the registry hands a device down to a model without
/// any global state.
pub struct GpuGridModelDescriptor<M: GpuGridModel> {
    ctx: GpuContext,
    _marker: PhantomData<M>,
}

impl<M: GpuGridModel> GpuGridModelDescriptor<M> {
    pub fn new(ctx: GpuContext) -> Self {
        Self {
            ctx,
            _marker: PhantomData,
        }
    }
}

impl<M: GpuGridModel> Model for GpuGridModelDescriptor<M> {
    type State = GpuGridState<M>;

    fn name(&self) -> &'static str {
        M::NAME
    }

    fn id(&self) -> &'static str {
        M::ID
    }

    fn description(&self) -> &'static str {
        M::DESCRIPTION
    }

    /// Everything is reload-only here, because `GpuGridState::set_param` rejects the lot.
    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        M::param_descriptors()
            .into_iter()
            .map(ParamDescriptor::on_reload)
            .collect()
    }

    fn stat_descriptors(&self) -> Vec<StatDescriptor> {
        M::STATS.to_vec()
    }

    /// Still a 2D grid — it just gets its pixels from a texture instead of a cell buffer. The UI
    /// branches on the *snapshot* variant, not on this hint.
    fn topology_hint(&self) -> TopologyHint {
        TopologyHint::GRID
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        GpuGridState::new(&self.ctx, params)
    }
}

/// GPU-resident state for a [`GpuGridModel`]. Owned exclusively by the GPU sim thread once
/// spawned.
pub struct GpuGridState<M: GpuGridModel> {
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
    readback: CounterReadback,

    /// `true` when the `a` side of every buffer holds the current (latest) state.
    current_is_a: bool,

    _marker: PhantomData<M>,
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &'static str,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&format!("{label}_shader")),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{label}_pipeline_layout")),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(&format!("{label}_pipeline")),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

impl<M: GpuGridModel> GpuGridState<M> {
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// Similar to [`Self::new`], with `seed` controlling the RNG used.
    ///
    /// If `None`, the model's fixed default seed is used.
    #[expect(clippy::too_many_lines)]
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let (width, height) = M::dims(params);
        let (width, height) = (width.max(1), height.max(1));

        // --- Ping-ponged storage buffers, seeded from the model ---
        // Buffer lengths come from the model, not from the cell count: a bit-packed model holds
        // many cells per u32, so only it knows how long its buffers are.
        let buffer_lens = M::buffer_lens(width, height);
        assert_eq!(
            buffer_lens.len(),
            M::BUFFER_COUNT,
            "{}: buffer_lens must return BUFFER_COUNT ({}) lengths, got {}",
            M::ID,
            M::BUFFER_COUNT,
            buffer_lens.len()
        );
        let seeds = M::seed_buffers(width, height, params, seed);
        assert_eq!(
            seeds.len(),
            M::BUFFER_COUNT,
            "{}: seed_buffers must return BUFFER_COUNT ({}) vectors, got {}",
            M::ID,
            M::BUFFER_COUNT,
            seeds.len()
        );

        let buffers: Vec<BufferPair> = seeds
            .iter()
            .zip(&buffer_lens)
            .enumerate()
            .map(|(k, (seed, &len))| {
                assert_eq!(
                    seed.len(),
                    len,
                    "{}: seed buffer {k} must match buffer_lens[{k}] ({len}) elements, got {}",
                    M::ID,
                    seed.len()
                );
                let buffer_size = (len * std::mem::size_of::<u32>()) as u64;
                let make = |side: char| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("{}_buffer{k}_{side}", M::ID)),
                        size: buffer_size,
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                };
                let pair = BufferPair {
                    a: make('a'),
                    b: make('b'),
                };
                // Only the `a` side needs seeding since the `b` side is always written during stepping.
                queue.write_buffer(&pair.a, 0, bytemuck::cast_slice(seed));
                pair
            })
            .collect();

        // --- Uniforms ---
        // The step shader gets the model's full param block; display and reduce only ever read a
        // leading `vec2<u32> dims`, so they get their own small buffer rather than depending on
        // the model's layout starting with the dimensions.
        let step_params = M::step_params_bytes(width, height, params);
        let step_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{}_step_params_buffer", M::ID)),
            size: step_params.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&step_params_buffer, 0, &step_params);

        let dims_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{}_dims_buffer", M::ID)),
            size: (2 * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&dims_buffer, 0, bytemuck::cast_slice(&[width, height]));

        let DisplayTarget {
            view: display_view,
            display,
        } = build_display_target(device, ctx.target_format, width, height);

        let readback = CounterReadback::new(device, &format!("{}_counters", M::ID), M::STATS.len());

        // --- Step pipeline ---
        // Bindings 0..2K are interleaved (current, next) pairs; binding 2K is the step uniform.
        let mut step_entries = Vec::with_capacity(2 * M::BUFFER_COUNT + 1);
        for k in 0..M::BUFFER_COUNT {
            let base = 2 * k as u32;
            step_entries.push(storage_entry(base, true));
            step_entries.push(storage_entry(base + 1, false));
        }
        let step_uniform_binding = 2 * M::BUFFER_COUNT as u32;
        step_entries.push(uniform_entry(step_uniform_binding));

        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{}_step_bind_group_layout", M::ID)),
            entries: &step_entries,
        });
        let make_step_bind_group = |label: &str, a_is_current: bool| {
            let mut entries = Vec::with_capacity(2 * M::BUFFER_COUNT + 1);
            for (k, pair) in buffers.iter().enumerate() {
                let (current, next) = if a_is_current {
                    (&pair.a, &pair.b)
                } else {
                    (&pair.b, &pair.a)
                };
                let base = 2 * k as u32;
                entries.push(wgpu::BindGroupEntry {
                    binding: base,
                    resource: current.as_entire_binding(),
                });
                entries.push(wgpu::BindGroupEntry {
                    binding: base + 1,
                    resource: next.as_entire_binding(),
                });
            }
            entries.push(wgpu::BindGroupEntry {
                binding: step_uniform_binding,
                resource: step_params_buffer.as_entire_binding(),
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &step_layout,
                entries: &entries,
            })
        };
        let bind_a2b = make_step_bind_group(&format!("{}_bind_a2b", M::ID), true);
        let bind_b2a = make_step_bind_group(&format!("{}_bind_b2a", M::ID), false);
        let step_pipeline = compute_pipeline(device, &format!("{}_step", M::ID), M::STEP_SHADER, &step_layout);

        // --- Display pipeline (primary buffer -> RGBA texture) ---
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{}_display_bind_group_layout", M::ID)),
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
        // Display and reduce read the *primary* buffer only; auxiliary buffers are step-private.
        let primary = buffers.first().expect("BUFFER_COUNT must be at least 1");
        let display_bind_a = make_display_bind_group(&format!("{}_display_bind_a", M::ID), &primary.a);
        let display_bind_b = make_display_bind_group(&format!("{}_display_bind_b", M::ID), &primary.b);
        let display_pipeline = compute_pipeline(
            device,
            &format!("{}_display", M::ID),
            M::DISPLAY_SHADER,
            &display_layout,
        );

        // --- Reduce pipeline (primary buffer -> counters) ---
        let reduce_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{}_reduce_bind_group_layout", M::ID)),
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
                        resource: readback.binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: dims_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let reduce_bind_a = make_reduce_bind_group(&format!("{}_reduce_bind_a", M::ID), &primary.a);
        let reduce_bind_b = make_reduce_bind_group(&format!("{}_reduce_bind_b", M::ID), &primary.b);
        let reduce_pipeline = compute_pipeline(device, &format!("{}_reduce", M::ID), M::REDUCE_SHADER, &reduce_layout);

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
            readback,
            current_is_a: true,
            _marker: PhantomData,
        }
    }

    /// Workgroups covering the step pass's domain, which a packed model measures in words.
    fn step_workgroups(&self) -> (u32, u32) {
        let (x, y) = M::step_dims(self.width, self.height);
        (x.div_ceil(M::WORKGROUP_SIZE), y.div_ceil(M::WORKGROUP_SIZE))
    }

    /// Workgroups covering one invocation per cell, which is what display and reduce always want.
    fn cell_workgroups(&self) -> (u32, u32) {
        (
            self.width.div_ceil(M::WORKGROUP_SIZE),
            self.height.div_ceil(M::WORKGROUP_SIZE),
        )
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

impl<M: GpuGridModel> SimState for GpuGridState<M> {
    /// Single-step fallback for callers that only have a `SimState`. This is usually not called; the GPU sim thread calls `encode_steps` directly for batched stepping.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_grid_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        stat_entries(M::STATS, M::stats(self.readback.values()))
    }

    /// Resizing or reseeding live is currently unsupported.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }

    fn population(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn heap_bytes(&self) -> usize {
        // Two ping-ponged sides per buffer, plus the RGBA display texture. The display texture is
        // one texel per cell regardless of how densely the buffers pack them.
        let buffers: usize = M::buffer_lens(self.width, self.height)
            .iter()
            .map(|len| len * std::mem::size_of::<u32>() * 2)
            .sum();
        let display_texture = (self.width as usize) * (self.height as usize) * 4;
        buffers + display_texture
    }
}

impl<M: GpuGridModel> GpuSimState for GpuGridState<M> {
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
        let (wg_x, wg_y) = self.step_workgroups();
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
                label: Some("gpu_grid_step_pass"),
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
        let (wg_x, wg_y) = self.cell_workgroups();

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_grid_display_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, self.current_display_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        // Clear -> accumulate -> copy out. wgpu inserts the barriers between these because they
        // are separate passes/copies within the one encoder.
        self.readback.encode_clear(encoder);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_grid_reduce_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.reduce_pipeline);
            pass.set_bind_group(0, self.current_reduce_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }
        self.readback.encode_copy(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.readback.begin_map();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        if block {
            self.readback.poll_blocking(device);
        } else {
            self.readback.poll(device);
        }
    }

    fn display(&self) -> Arc<GpuDisplay> {
        Arc::clone(&self.display)
    }
}
