//! The engine that runs a [`GpuGridModel`] as a [`GpuSimState`], the counterpart of [`crate::cpu::grid_engine`].

use std::marker::PhantomData;
use std::sync::Arc;

use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, stat_entries};

use crate::fault::FaultSink;
use crate::gpu::GpuContext;
use crate::gpu::built_pass::{BuiltPass, PassDescriptor, Sides, buffer_binding, stamps};
use crate::gpu::capacity::Demand;
use crate::gpu::grid_spec::{GpuGridSpec, GridPassCtx, GridPassId, GridPassSpec, GridPress};
use crate::gpu::press::{next_press_seed, upload_rerun};
use crate::gpu::primitives::pipeline::uniform_buffer;
use crate::gpu::primitives::readback::{CounterReadback, StatsPoll, read_words};
use crate::gpu::primitives::reduce::GpuLaneReduce;
use crate::gpu::shader_spec::ShaderBinding;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::view::display::{DisplayTarget, GpuDisplay, build_display_target};
use crate::snapshot::{FireCounts, GpuSnapshot};

/// The uniform every display and reduce shader reads, generated from `henad::dims` through `grid_dims.wgsl`.
pub(crate) type Dims = crate::shader_bindings::henad::dims::Dims;

/// A pass of the grid engine, keyed by the pass of the spec it was built from.
type GridPass = BuiltPass<GridPassId>;

/// GPU-resident state built from a [`GpuGridSpec`], with every buffer, pipeline and bind group the spec declares.
///
/// [`GpuGridState`] builds the spec from a model type and forwards every call here.
///
/// The workgroup counts of every pass and the size of the buffers are computed once at construction. A step calls no
/// hook of the spec. An action press calls one for its rerun and one per action pass for its uniform block, and
/// [`SimState::stats`] calls one for the values.
pub(crate) struct GpuGridEngine {
    spec: Arc<GpuGridSpec>,

    width: u32,
    height: u32,
    /// Display texture size, capped independently of the grid. See [`crate::display_scale`].
    tex: (u32, u32),
    tick: u64,

    /// Bytes of every side of every buffer.
    buffer_bytes: usize,
    /// Length in words of each buffer, in the spec's order.
    buffer_lens: Vec<usize>,

    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Sink a rerun's fault is reported to.
    faults: FaultSink,

    buffers: Vec<Sides<wgpu::Buffer>>,
    steps: Vec<GridPass>,
    display_pass: GridPass,
    display: Arc<GpuDisplay>,
    reduce: GridPass,
    readback: CounterReadback,
    /// Float sums of the reduce pass, `None` when the spec sums no lane.
    lane_reduce: Option<GpuLaneReduce>,

    /// Passes of each action, in the spec's order.
    actions: Vec<Vec<GridPass>>,
    /// Latest press, advanced on every press so that pressing twice draws twice.
    press: GridPress,
    /// Action stream that a rerun draws from, continued across presses.
    action_stream: u64,
    /// Seed of the run, which a rerun and an action's uniform block read.
    seed: Option<u64>,
    /// Parameter values, kept to rewrite the action uniforms on each press, and edited by a live
    /// [`SimState::set_param`].
    params: Vec<ParamValue>,
    /// Whether each parameter is live, in the order of [`Self::params`]. A parameter past its end is reload-only.
    live: Vec<bool>,
    /// Context of the build's uniform blocks for the step, display and reduce passes. A live edit writes the blocks
    /// again in this context.
    uniform_ctx: GridPassCtx,

    /// `true` when the `a` side of every buffer holds the current (latest) state.
    current_is_a: bool,
    /// Steps done when the latest reduce pass was recorded, which the stat values hook reads.
    stats_tick: u64,
    /// Tick the latest counted step reached, 0 before the first.
    counted_tick: u64,
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
    /// seed whose length differs from the length of its buffer.
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

        // Storage buffers, seeded from the model.
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

        let buffers: Vec<Sides<wgpu::Buffer>> = seeds
            .iter()
            .zip(&buffer_lens)
            .zip(&spec.buffers)
            .enumerate()
            .map(|(buffer_index, ((seed, &len), declared))| {
                assert_eq!(
                    seed.len(),
                    len,
                    "{id}: seed buffer {buffer_index} must match buffer_lens[{buffer_index}] ({len}) elements, got {}",
                    seed.len()
                );
                let buffer_size = (len * std::mem::size_of::<u32>()) as u64;
                let make = |side: char| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("{id}_buffer{buffer_index}_{side}")),
                        size: buffer_size,
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                };
                let buffer = Sides {
                    a: make('a'),
                    b: declared.double_buffered.then(|| make('b')),
                };
                // Only the `a` side needs seeding since a step pass writes the `b` side.
                queue.write_buffer(&buffer.a, 0, bytemuck::cast_slice(seed));
                buffer
            })
            .collect();

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

        let readback = CounterReadback::new(device, &format!("{id}_counters"), spec.counter_len());
        let cell_workgroups = workgroups((width, height), spec.workgroup_size);
        let lane_reduce = (spec.sums > 0)
            .then(|| GpuLaneReduce::over_blocks(device, queue, id, spec.sums, cell_workgroups.0 * cell_workgroups.1));

        // The seed is truncated from the stream the CPU engines use, as the WGSL generator is 32 bit. Each action
        // uniform is rewritten with a fresh seed on every press.
        let press = GridPress::new(henad_core::action::action_seed(seed) as u32, 0);
        let pass_ctx = GridPassCtx {
            width,
            height,
            seed,
            press,
            tick: 0,
            counting: false,
        };
        let counts_fires = !spec.fire_counters.is_empty();

        // Workgroups of every step and action pass, over the step domain the spec's hooks return, and of the display
        // pass, at one invocation per texel. Those equal the reduce pass's, at one per cell, until the grid outgrows
        // the texture cap.
        let step_workgroups = workgroups(spec.hooks.step_dims(width, height), spec.workgroup_size);
        let texel_workgroups = workgroups(tex, spec.workgroup_size);
        let build = |label: String, pass_label: String, pass: &GridPassSpec, uniform_of: GridPassId, groups| {
            let binds_params = pass.shader.bindings.iter().any(|decl| decl.name == "params");
            let block = |counting: bool| {
                spec.hooks
                    .pass_params_bytes(uniform_of, pass_ctx.with_counting(counting), params)
            };
            // A step's counting block differs from its block in the counting flag alone.
            let counts = counts_fires && binds_params && matches!(uniform_of, GridPassId::Step(_));
            let resolve = |decl: &ShaderBinding, a_is_current: bool| -> wgpu::BindingResource<'_> {
                let labels = spec.buffers.iter().map(|buffer| &*buffer.label);
                if let Some(resource) = buffer_binding(decl, id, labels, &buffers, &pass.sides, a_is_current) {
                    return resource;
                }
                match decl.name.as_ref() {
                    "dims" => dims_buffer.as_entire_binding(),
                    "counters" => readback.binding(),
                    "partials" => lane_reduce
                        .as_ref()
                        .unwrap_or_else(|| panic!("{id}: `partials` is bound, and the spec sums no lane"))
                        .partials_binding(),
                    "output" => wgpu::BindingResource::TextureView(&display_view),
                    other => panic!("{id}: `{other}` is reserved but the engine has no resource for it"),
                }
            };
            let descriptor = PassDescriptor {
                label,
                pass_label,
                id: uniform_of,
                shader: &pass.shader,
                groups,
                uniform: binds_params.then(|| block(false)),
                counting_uniform: counts.then(|| block(true)),
                both_sides: true,
            };
            BuiltPass::new(device, queue, descriptor, resolve)
        };

        let steps: Vec<GridPass> = spec
            .steps
            .iter()
            .enumerate()
            .map(|(i, pass)| {
                build(
                    format!("{id}_{}", pass.label),
                    "gpu_grid_step_pass".to_owned(),
                    pass,
                    GridPassId::Step(i),
                    step_workgroups,
                )
            })
            .collect();
        let display_pass = build(
            format!("{id}_{}", spec.display.label),
            "gpu_grid_display_pass".to_owned(),
            &spec.display,
            GridPassId::Display,
            texel_workgroups,
        );
        let reduce = build(
            format!("{id}_{}", spec.reduce.label),
            "gpu_grid_reduce_pass".to_owned(),
            &spec.reduce,
            GridPassId::Reduce,
            cell_workgroups,
        );
        let actions: Vec<Vec<GridPass>> = spec
            .actions
            .iter()
            .enumerate()
            .map(|(action, declared)| {
                declared
                    .passes
                    .iter()
                    .enumerate()
                    .map(|(pass, spec_pass)| {
                        let label = format!("{id}_action_{}", spec_pass.label);
                        build(
                            label.clone(),
                            label,
                            spec_pass,
                            GridPassId::Action { action, pass },
                            step_workgroups,
                        )
                    })
                    .collect()
            })
            .collect();

        let buffer_bytes = buffer_lens
            .iter()
            .zip(&spec.buffers)
            .map(|(len, buffer)| len * std::mem::size_of::<u32>() * if buffer.double_buffered { 2 } else { 1 })
            .sum();

        Self {
            width,
            height,
            tex,
            tick: 0,
            buffer_bytes,
            buffer_lens,
            device: device.clone(),
            queue: queue.clone(),
            faults: ctx.faults.clone(),
            buffers,
            steps,
            display_pass,
            display,
            reduce,
            readback,
            lane_reduce,
            actions,
            press,
            action_stream: henad_core::action::action_seed(seed),
            seed,
            params: params.to_vec(),
            live: Vec::new(),
            uniform_ctx: pass_ctx,
            current_is_a: true,
            stats_tick: 0,
            counted_tick: 0,
            spec,
        }
    }

    /// Returns the engine with each parameter live where `live` holds at its index.
    ///
    /// A live parameter must feed nothing but the uniform blocks, the reruns and the stat values. A parameter past
    /// the end of `live` stays reload-only, as every parameter of an engine built without this call does.
    pub(crate) fn with_live_params(mut self, live: Vec<bool>) -> Self {
        self.live = live;
        self
    }

    /// Returns the values a hook reads at `tick`, with the latest press and no counting.
    fn pass_ctx(&self, tick: u64) -> GridPassCtx {
        GridPassCtx {
            width: self.width,
            height: self.height,
            seed: self.seed,
            press: self.press,
            tick,
            counting: false,
        }
    }

    /// Writes the uniform blocks of the step, display and reduce passes again from [`Self::params`], in the context
    /// they were written in at build. An action's block is written at each press.
    fn rewrite_uniforms(&self) {
        let passes = self.steps.iter().chain([&self.display_pass, &self.reduce]);
        for pass in passes {
            if let Some(uniform) = &pass.uniform {
                let bytes = self
                    .spec
                    .hooks
                    .pass_params_bytes(pass.id, self.uniform_ctx, &self.params);
                self.queue.write_buffer(uniform, 0, &bytes);
            }
            if let Some(uniform) = &pass.counting_uniform {
                let ctx = self.uniform_ctx.with_counting(true);
                let bytes = self.spec.hooks.pass_params_bytes(pass.id, ctx, &self.params);
                self.queue.write_buffer(uniform, 0, &bytes);
            }
        }
    }

    /// Runs the rerun of action `index` on the host and uploads the buffers it writes to their current sides.
    ///
    /// Returns whether the rerun succeeded. A fault goes to the context's fault sink.
    fn rerun(&mut self, index: usize, ctx: GridPassCtx) -> bool {
        let outcome = self.spec.hooks.rerun(index, ctx, &self.params, &mut self.action_stream);
        upload_rerun(
            outcome,
            &self.spec.id,
            &self.faults,
            &self.queue,
            &self.buffers,
            &self.buffer_lens,
            self.current_is_a,
        )
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
        let ctx = self.pass_ctx(self.stats_tick);
        stat_entries(
            &self.spec.stats,
            self.spec.hooks.stats_with_sums(
                self.readback.values(),
                &self.lane_reduce.as_ref().map(GpuLaneReduce::sums).unwrap_or_default(),
                ctx,
                &self.params,
            ),
        )
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

    /// Sets a live parameter and writes the uniform blocks of the step, display and reduce passes again before the next
    /// step. Rejects a reload-only parameter and an index past the parameters.
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        if !self.live.get(index).copied().unwrap_or(false) {
            return false;
        }
        let Some(slot) = self.params.get_mut(index) else {
            return false;
        };
        *slot = value.clone();
        self.rewrite_uniforms();
        true
    }

    fn population(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn heap_bytes(&self) -> usize {
        // The ping-ponged buffers, plus the capped RGBA display texture and the partials of any float sums.
        let display_texture = (self.tex.0 as usize) * (self.tex.1 as usize) * 4;
        let sums = self.lane_reduce.as_ref().map_or(0, GpuLaneReduce::heap_bytes);
        self.buffer_bytes + display_texture + sums
    }
}

impl GpuSimState for GpuGridEngine {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if count == 0 {
            return;
        }
        let last_pass = self.steps.len().saturating_sub(1);
        let fires = self.spec.fire_counters.clone();
        for i in 0..count {
            // The last step of the call counts fires into a range cleared just before it.
            let counting = i == count - 1 && !fires.is_empty();
            if counting {
                self.readback.encode_clear_range(encoder, fires.clone());
            }
            for (pass_index, pass) in self.steps.iter().enumerate() {
                let opening = i == 0 && pass_index == 0;
                let closing = i == count - 1 && pass_index == last_pass;
                pass.encode(
                    encoder,
                    self.current_is_a,
                    stamps(timestamps, opening, closing),
                    counting,
                );
            }
            self.current_is_a = !self.current_is_a;
        }
        self.tick += u64::from(count);
        if !fires.is_empty() {
            self.counted_tick = self.tick;
        }
    }

    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        if index >= self.actions.len() {
            return false;
        }
        // A fresh seed per press, so pressing twice draws twice. The writes are queued before the
        // encoder is submitted, so the passes read the new values.
        self.press = GridPress::new(next_press_seed(self.press.seed), self.press.count.wrapping_add(1));
        let ctx = self.pass_ctx(self.tick);
        if !self.rerun(index, ctx) {
            return true;
        }
        for pass in &self.actions[index] {
            if let Some(uniform) = &pass.uniform {
                let bytes = self.spec.hooks.pass_params_bytes(pass.id, ctx, &self.params);
                self.queue.write_buffer(uniform, 0, &bytes);
            }
            pass.encode(encoder, self.current_is_a, None, false);
        }
        true
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.display_pass.encode(encoder, self.current_is_a, None, false);
        self.encode_stats_passes(encoder);
    }

    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        // Clear -> accumulate -> copy out. wgpu inserts the barriers between these because they
        // are separate passes/copies within the one encoder.
        self.stats_tick = self.tick;
        let fires = self.spec.fire_counters.clone();
        if fires.is_empty() {
            self.readback.encode_clear(encoder);
        } else {
            // The fire counts of the last counted step stay for the copy.
            self.readback.encode_clear_range(encoder, 0..fires.start);
            self.readback
                .encode_clear_range(encoder, fires.end..self.spec.counter_len());
        }
        self.reduce.encode(encoder, self.current_is_a, None, false);
        self.readback.encode_copy_tagged(encoder, self.counted_tick);
        if let Some(lane_reduce) = &mut self.lane_reduce {
            lane_reduce.encode(encoder);
        }
    }

    fn begin_stats_readback(&mut self) {
        self.readback.begin_map();
        if let Some(lane_reduce) = &mut self.lane_reduce {
            lane_reduce.begin_readback();
        }
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        let counters = self.readback.poll_or_block(device, block);
        let Some(lane_reduce) = &mut self.lane_reduce else {
            return counters;
        };
        counters.and(lane_reduce.poll_readback(device, block))
    }

    fn stats_readback_pending(&self) -> bool {
        self.readback.is_pending() || self.lane_reduce.as_ref().is_some_and(GpuLaneReduce::readback_pending)
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: Some(Arc::clone(&self.display)),
            agents: None,
        }
    }

    fn fire_counts(&self) -> FireCounts {
        self.readback.fire_counts(self.spec.fire_counters.clone())
    }

    fn read_buffer(&self, index: usize) -> Option<Vec<u32>> {
        let buffer = self.buffers.get(index)?;
        Some(read_words(&self.device, &self.queue, buffer.pick(self.current_is_a)))
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

    fn fire_counts(&self) -> FireCounts {
        self.engine.fire_counts()
    }

    fn read_buffer(&self, index: usize) -> Option<Vec<u32>> {
        self.engine.read_buffer(index)
    }
}
