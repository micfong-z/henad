//! The engine that runs a [`GpuGridModel`] as a [`GpuSimState`], the counterpart of [`crate::cpu::grid_engine`].

use std::marker::PhantomData;
use std::sync::Arc;

use henad_core::authoring::model::binding::{BindingDecl, buffer_target};
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, stat_entries};

use crate::gpu::GpuContext;
use crate::gpu::capacity::{Demand, layout_entry};
use crate::gpu::grid_spec::{GpuGridSpec, GridShaderSpec};
use crate::gpu::primitives::pipeline::{compute_pipeline, uniform_buffer};
use crate::gpu::primitives::readback::{CounterReadback, StatsPoll};
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::view::display::{DisplayTarget, GpuDisplay, build_display_target};
use crate::snapshot::GpuSnapshot;

/// The uniform every display and reduce shader reads, generated from `henad::dims` through `grid_dims.wgsl`.
pub(crate) type Dims = crate::shader_bindings::henad::dims::Dims;

/// One ping-ponged pair of storage buffers.
struct BufferPair {
    a: wgpu::Buffer,
    b: wgpu::Buffer,
}

/// A built action pass, with the uniform kept so a press can reseed it.
struct ActionPass {
    label: String,
    pipeline: wgpu::ComputePipeline,
    bind_a: wgpu::BindGroup,
    bind_b: wgpu::BindGroup,
    uniform: wgpu::Buffer,
}

impl ActionPass {
    fn bind(&self, a_is_current: bool) -> &wgpu::BindGroup {
        if a_is_current { &self.bind_a } else { &self.bind_b }
    }
}

/// GPU-resident state built from a [`GpuGridSpec`], with every buffer, pipeline and bind group the spec declares.
///
/// [`GpuGridState`] builds the spec from a model type and forwards every call here.
///
/// The workgroup counts of every pass and the size of the ping-ponged buffers are computed once at construction. A
/// step calls no hook of the spec, an action press calls one for its uniform block, and [`SimState::stats`] calls one
/// for the values.
pub(crate) struct GpuGridEngine {
    spec: Arc<GpuGridSpec>,

    width: u32,
    height: u32,
    /// Display texture size, capped independently of the grid. See [`crate::display_scale`].
    tex: (u32, u32),
    tick: u64,

    /// Workgroups of the step pass and of every action, over the step domain the spec's hooks return.
    step_workgroups: (u32, u32),
    /// Workgroups of the reduce pass, at one invocation per cell.
    cell_workgroups: (u32, u32),
    /// Workgroups of the display pass, at one invocation per texel. Equal to [`Self::cell_workgroups`] until the grid
    /// outgrows the texture cap.
    texel_workgroups: (u32, u32),
    /// Bytes of both sides of every ping-ponged buffer.
    buffer_bytes: usize,

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

    actions: Vec<ActionPass>,
    /// Seed of the next action press, advanced per press so that pressing twice draws twice.
    action_seed: u32,
    /// Parameter values, kept to rewrite the action uniforms on each press. Never edited, since a GPU grid model
    /// declares every parameter reload-only.
    params: Vec<ParamValue>,

    /// `true` when the `a` side of every buffer holds the current (latest) state.
    current_is_a: bool,
}

impl std::fmt::Debug for GpuGridEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuGridEngine")
            .field("model", &self.spec.id)
            .field("tick", &self.tick)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Returns the workgroups of side `workgroup_size` that cover a `width` by `height` domain.
fn workgroups((width, height): (u32, u32), workgroup_size: u32) -> (u32, u32) {
    (width.div_ceil(workgroup_size), height.div_ceil(workgroup_size))
}

impl GpuGridEngine {
    /// Builds the state for `spec` on `ctx`, seeded with `seed`, or with the model's fixed default seed when it is
    /// `None`.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot hold the model, or if [`GpuGridSpec::assert_contracts`] panics. Also panics if
    /// the hooks return lengths or seeds for another number of buffers than [`GpuGridSpec::buffers`] holds, or a
    /// seed whose length differs from its buffer's.
    #[expect(clippy::too_many_lines)]
    pub(crate) fn new(ctx: &GpuContext, spec: Arc<GpuGridSpec>, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;
        let id = &*spec.id;

        let (width, height) = spec.dims(params);

        let shortfalls = spec.demand(params, &device.limits()).shortfalls(&device.limits());
        assert!(
            shortfalls.is_empty(),
            "{id} does not fit this device at {width}x{height}: {}",
            shortfalls.join("; ")
        );
        spec.assert_contracts();

        // Ping-ponged storage buffers, seeded from the model.
        // Buffer lengths come from the model. A bit-packed model holds many cells per u32, and only the
        // model knows its buffer lengths.
        let buffer_lens = spec.hooks.buffer_lens(width, height);
        assert_eq!(
            buffer_lens.len(),
            spec.buffers.len(),
            "{id}: buffer_lens must return BUFFER_COUNT ({}) lengths, got {}",
            spec.buffers.len(),
            buffer_lens.len()
        );
        let seeds = spec.hooks.seed_buffers(width, height, params, seed);
        assert_eq!(
            seeds.len(),
            spec.buffers.len(),
            "{id}: seed_buffers must return BUFFER_COUNT ({}) vectors, got {}",
            spec.buffers.len(),
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
                    "{id}: seed buffer {k} must match buffer_lens[{k}] ({len}) elements, got {}",
                    seed.len()
                );
                let buffer_size = (len * std::mem::size_of::<u32>()) as u64;
                let make = |side: char| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("{id}_buffer{k}_{side}")),
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

        // Uniforms.
        // Display and reduce get their own small buffer rather than depending on the model's
        // layout starting with the dimensions.
        let step_params = spec.hooks.step_params_bytes(width, height, params);
        let step_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{id}_step_params_buffer")),
            size: step_params.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&step_params_buffer, 0, &step_params);

        let DisplayTarget {
            view: display_view,
            dims: tex,
            display,
        } = build_display_target(device, ctx.target_format, width, height);

        let dims_buffer = uniform_buffer(
            device,
            queue,
            &format!("{id}_dims_buffer"),
            bytemuck::bytes_of(&Dims {
                grid: [width, height],
                tex: tex.into(),
            }),
        );

        let readback = CounterReadback::new(device, &format!("{id}_counters"), spec.stats.len());

        // Pipelines.
        // Every layout entry and every bind group entry comes from the name its shader gives the
        // binding, so a slot index cannot disagree with the shader that owns it.
        // The action uniforms are built before the pipelines, so the resolver below can return an action uniform by
        // index. Each uniform is rewritten with a fresh seed on every press. The seed is truncated from the
        // stream the CPU engines use, as the WGSL generator is 32 bit.
        let action_seed = henad_core::action::action_seed(seed) as u32;
        let action_uniforms: Vec<wgpu::Buffer> = spec
            .actions
            .iter()
            .enumerate()
            .map(|(i, action)| {
                uniform_buffer(
                    device,
                    queue,
                    &format!("{id}_action_{}_params", action.id),
                    &spec.hooks.action_params_bytes(i, width, height, params, action_seed),
                )
            })
            .collect();

        // An action writes the side that already holds the state. Nothing swaps after an action, so
        // writing the far side would throw the work away.
        let resolve = |decl: &BindingDecl, a_is_current: bool, action: Option<usize>| -> wgpu::BindingResource<'_> {
            if let Some((label, writes)) = buffer_target(decl) {
                let k = spec
                    .buffers
                    .iter()
                    .position(|l| *l == label)
                    .unwrap_or_else(|| panic!("{id}: no buffer labelled `{label}`, wanted by `{}`", decl.name));
                let pair = &buffers[k];
                let (read, write) = if a_is_current {
                    (&pair.a, &pair.b)
                } else {
                    (&pair.b, &pair.a)
                };
                return if writes && action.is_none() { write } else { read }.as_entire_binding();
            }
            match decl.name {
                "params" => match action {
                    Some(i) => action_uniforms[i].as_entire_binding(),
                    None => step_params_buffer.as_entire_binding(),
                },
                "dims" => dims_buffer.as_entire_binding(),
                "counters" => readback.binding(),
                "output" => wgpu::BindingResource::TextureView(&display_view),
                other => panic!("{id}: `{other}` is reserved but the engine has no resource for it"),
            }
        };

        let build = |label: &str, shader: &GridShaderSpec, action: Option<usize>| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = shader
                .bindings
                .iter()
                .enumerate()
                .map(|(i, decl)| layout_entry(i as u32, decl))
                .collect();
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(&format!("{id}_{label}_layout")),
                entries: &entries,
            });
            let make = |a_is_current: bool, side: &str| {
                let entries: Vec<wgpu::BindGroupEntry<'_>> = shader
                    .bindings
                    .iter()
                    .enumerate()
                    .map(|(i, decl)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: resolve(decl, a_is_current, action),
                    })
                    .collect();
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("{id}_{label}_bind_{side}")),
                    layout: &layout,
                    entries: &entries,
                })
            };
            let binds = (make(true, "a"), make(false, "b"));
            let pipeline = compute_pipeline(device, &format!("{id}_{label}"), &shader.source, &layout);
            (pipeline, binds)
        };

        let (step_pipeline, (bind_a2b, bind_b2a)) = build("step", &spec.step, None);
        let (display_pipeline, (display_bind_a, display_bind_b)) = build("display", &spec.display, None);
        let (reduce_pipeline, (reduce_bind_a, reduce_bind_b)) = build("reduce", &spec.reduce, None);

        let actions: Vec<ActionPass> = spec
            .actions
            .iter()
            .enumerate()
            .map(|(i, action)| {
                let label = format!("action_{}", action.id);
                let (pipeline, (bind_a, bind_b)) = build(&label, &action.shader, Some(i));
                ActionPass {
                    label: format!("{id}_{label}"),
                    pipeline,
                    bind_a,
                    bind_b,
                    uniform: action_uniforms[i].clone(),
                }
            })
            .collect();

        let step_workgroups = workgroups(spec.hooks.step_dims(width, height), spec.workgroup_size);
        let cell_workgroups = workgroups((width, height), spec.workgroup_size);
        let texel_workgroups = workgroups(tex, spec.workgroup_size);
        // Two ping-ponged sides per buffer.
        let buffer_bytes = buffer_lens.iter().map(|len| len * std::mem::size_of::<u32>() * 2).sum();

        Self {
            spec,
            width,
            height,
            tex,
            tick: 0,
            step_workgroups,
            cell_workgroups,
            texel_workgroups,
            buffer_bytes,
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
            actions,
            action_seed,
            params: params.to_vec(),
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

impl SimState for GpuGridEngine {
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
        stat_entries(&self.spec.stats, self.spec.hooks.stats(self.readback.values()))
    }

    fn act(&mut self, index: usize) -> bool {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_grid_action"),
        });
        if !GpuSimState::encode_action(self, &mut encoder, index) {
            return false;
        }
        self.queue.submit(Some(encoder.finish()));
        true
    }

    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }

    fn population(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn heap_bytes(&self) -> usize {
        // The ping-ponged buffers, plus the capped RGBA display texture.
        let display_texture = (self.tex.0 as usize) * (self.tex.1 as usize) * 4;
        self.buffer_bytes + display_texture
    }
}

impl GpuSimState for GpuGridEngine {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if count == 0 {
            return;
        }
        let (wg_x, wg_y) = self.step_workgroups;
        for i in 0..count {
            let bind_group = if self.current_is_a {
                &self.bind_a2b
            } else {
                &self.bind_b2a
            };
            let is_first = i == 0;
            let is_last = i == count - 1;
            // A `ComputePassTimestampWrites` requires at least one of the two indices to be
            // `Some`, so only the first and last passes of the batch get timestamp writes.
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

    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        let Some(action) = self.actions.get(index) else {
            return false;
        };
        // A fresh seed per press, so pressing twice draws twice. The write is queued before the
        // encoder is submitted, so the pass reads the new value.
        self.action_seed = self.action_seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        self.queue.write_buffer(
            &action.uniform,
            0,
            &self
                .spec
                .hooks
                .action_params_bytes(index, self.width, self.height, &self.params, self.action_seed),
        );

        let (groups_x, groups_y) = self.step_workgroups;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(&action.label),
            timestamp_writes: None,
        });
        pass.set_pipeline(&action.pipeline);
        pass.set_bind_group(0, action.bind(self.current_is_a), &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
        true
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        {
            let (wg_x, wg_y) = self.texel_workgroups;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_grid_display_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, self.current_display_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }
        self.encode_stats_passes(encoder);
    }

    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let (wg_x, wg_y) = self.cell_workgroups;

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

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        if block {
            self.readback.poll_blocking(device)
        } else {
            self.readback.poll(device)
        }
    }

    fn stats_readback_pending(&self) -> bool {
        self.readback.is_pending()
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: Some(Arc::clone(&self.display)),
            agents: None,
        }
    }
}

/// GPU-resident state for a [`GpuGridModel`], with every buffer, pipeline and bind group its model declares.
pub struct GpuGridState<M: GpuGridModel> {
    engine: GpuGridEngine,
    _marker: PhantomData<M>,
}

impl<M: GpuGridModel> std::fmt::Debug for GpuGridState<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuGridState")
            .field("model", &M::ID)
            .field("tick", &self.engine.tick)
            .field("width", &self.engine.width)
            .field("height", &self.engine.height)
            .finish_non_exhaustive()
    }
}

impl<M: GpuGridModel> GpuGridState<M> {
    /// Builds the state on `ctx`, seeded with the model's default seed.
    ///
    /// # Panics
    ///
    /// Panics as [`Self::new_seeded`] does.
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// Resources that would be allocated for this model based on `params`.
    pub fn demand(params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        GpuGridSpec::from_model::<M>().demand(params, limits)
    }

    /// Returns the most storage buffers any declared pass binds. It does not depend on params, so a host can query it
    /// before it has a device.
    pub fn max_storage_bindings() -> u32 {
        GpuGridSpec::from_model::<M>().max_storage_bindings()
    }

    /// Builds the state on `ctx`, seeded with `seed`, or with the model's fixed default seed when it is `None`.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot hold the model. A host calls [`Self::demand`] first, and this assert is the
    /// backstop. Also panics if a shader declares a workgroup size other than
    /// [`GpuGridModel::WORKGROUP_SIZE`], or a buffer label is reserved or ends in `_in` or `_out`.
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        Self {
            engine: GpuGridEngine::new(ctx, Arc::new(GpuGridSpec::from_model::<M>()), params, seed),
            _marker: PhantomData,
        }
    }
}

// Forwards every call to the engine.
impl<M: GpuGridModel> SimState for GpuGridState<M> {
    /// Steps once in its own submission, for a caller that holds only a `SimState`. The GPU runner calls
    /// `encode_steps` directly and batches the steps.
    fn step(&mut self) {
        self.engine.step();
    }

    fn tick(&self) -> u64 {
        self.engine.tick()
    }

    fn stats(&self) -> Vec<StatEntry> {
        self.engine.stats()
    }

    /// Encodes the action into its own submission.
    ///
    /// The GPU runner calls [`GpuSimState::encode_action`] instead, and submits the action before
    /// its snapshot.
    fn act(&mut self, index: usize) -> bool {
        self.engine.act(index)
    }

    /// Rejects every live edit. Its entry declares every parameter reload-only.
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.engine.set_param(index, value)
    }

    fn population(&self) -> u64 {
        self.engine.population()
    }

    fn heap_bytes(&self) -> usize {
        self.engine.heap_bytes()
    }
}

// Forwards every call to the engine.
impl<M: GpuGridModel> GpuSimState for GpuGridState<M> {
    /// Records `count` step dispatches into `encoder`, one compute pass per step.
    ///
    /// Each step is a read-after-write hazard on the ping-ponged state buffers, and wgpu inserts
    /// barriers only between passes. Dispatches looped inside one pass would read stale data.
    /// Batching happens at the submission level instead, with one encoder for all `count` passes.
    ///
    /// If `timestamps` is `Some`, the first pass's beginning and the last pass's end are stamped
    /// into query indices 0 and 1 so the caller can measure GPU time for the whole batch.
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        self.engine.encode_steps(encoder, count, timestamps);
    }

    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        self.engine.encode_action(encoder, index)
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.engine.encode_snapshot_passes(encoder);
    }

    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.engine.encode_stats_passes(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.engine.begin_stats_readback();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        self.engine.poll_stats_readback(device, block)
    }

    fn stats_readback_pending(&self) -> bool {
        self.engine.stats_readback_pending()
    }

    /// Returns the display layer alone. A `GpuGridModel` has no agent layer.
    fn view(&self) -> GpuSnapshot {
        self.engine.view()
    }
}
