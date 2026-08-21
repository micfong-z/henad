//! Generic engine turning any [`GpuGridModel`] into a runnable [`GpuSimState`].
//!
//! Compare with [`crate::cpu::grid_engine`].

use std::marker::PhantomData;
use std::sync::Arc;

use henad_core::authoring::model::binding::{BindingDecl, buffer_target};
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::model::{Model, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatEntry, stat_entries};

use crate::gpu::GpuContext;
use crate::gpu::capacity::{Demand, layout_entry, storage_bindings};
use crate::gpu::primitives::pipeline::{compute_pipeline, uniform_buffer};
use crate::gpu::primitives::readback::CounterReadback;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::view::display::{DisplayTarget, GpuDisplay, build_display_target};
use crate::snapshot::GpuSnapshot;

/// The uniform every display and reduce shader reads, mirroring `Dims` in `shared/dims.wgsl`.
///
/// Hand written rather than generated, since no shader in this crate uses the type and naga drops
/// what nothing references. `henad_models` sees both sides and asserts they agree.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Dims {
    pub grid: [u32; 2],
    pub tex: [u32; 2],
}

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

    /// Still a 2D grid, just getting its pixels from a texture instead of a cell buffer. The UI
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
    /// Display texture size, capped independently of the grid. See [`crate::display_scale`].
    tex: (u32, u32),
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

impl<M: GpuGridModel> GpuGridState<M> {
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// Resources that would be allocated for this model based on `params`.
    pub fn demand(params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        let (width, height) = M::dims(params);
        let (width, height) = (width.max(1), height.max(1));

        let mut demand = Demand::default();
        for (k, len) in M::buffer_lens(width, height).into_iter().enumerate() {
            demand.push_sides(&format!("{}_buffer{k}", M::ID), len, true);
        }
        demand.set_display(width, height, limits);
        for (label, storage) in Self::declared_passes() {
            demand.push_pass(label, storage);
        }
        demand
    }

    /// Storage buffers each generated pass binds. Read by both [`Self::demand`] and
    /// [`Self::max_storage_bindings`], so the device a host asks for and the shortfall the UI
    /// reports cannot disagree.
    fn declared_passes() -> Vec<(String, u32)> {
        vec![
            (format!("{}_step", M::ID), storage_bindings(M::STEP_BINDINGS)),
            (format!("{}_display", M::ID), storage_bindings(M::DISPLAY_BINDINGS)),
            (format!("{}_reduce", M::ID), storage_bindings(M::REDUCE_BINDINGS)),
        ]
    }

    /// Independent of params, so a host can ask before it has a device.
    pub fn max_storage_bindings() -> u32 {
        Self::declared_passes()
            .into_iter()
            .map(|(_, storage)| storage)
            .max()
            .unwrap_or(0)
    }

    /// Similar to [`Self::new`], with `seed` controlling the RNG used.
    ///
    /// If `None`, the model's fixed default seed is used.
    ///
    /// # Panics
    ///
    /// If the device cannot hold the model. The backstop, not the diagnostic, since a UI
    /// asks [`Self::demand`] first.
    #[expect(clippy::too_many_lines)]
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let (width, height) = M::dims(params);
        let (width, height) = (width.max(1), height.max(1));

        let shortfalls = Self::demand(params, &device.limits()).shortfalls(&device.limits());
        assert!(
            shortfalls.is_empty(),
            "{} does not fit this device at {width}x{height}: {}",
            M::ID,
            shortfalls.join("; ")
        );

        // --- Ping-ponged storage buffers, seeded from the model ---
        // Buffer lengths come from the model, not from the cell count: a bit-packed model holds
        // many cells per u32, so only it knows how long its buffers are.
        let buffer_lens = M::buffer_lens(width, height);
        assert_eq!(
            buffer_lens.len(),
            M::BUFFERS.len(),
            "{}: buffer_lens must return BUFFER_COUNT ({}) lengths, got {}",
            M::ID,
            M::BUFFERS.len(),
            buffer_lens.len()
        );
        let seeds = M::seed_buffers(width, height, params, seed);
        assert_eq!(
            seeds.len(),
            M::BUFFERS.len(),
            "{}: seed_buffers must return BUFFER_COUNT ({}) vectors, got {}",
            M::ID,
            M::BUFFERS.len(),
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
        // Display and reduce get their own small buffer rather than depending on the model's
        // layout starting with the dimensions.
        let step_params = M::step_params_bytes(width, height, params);
        let step_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{}_step_params_buffer", M::ID)),
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
            &format!("{}_dims_buffer", M::ID),
            bytemuck::bytes_of(&Dims {
                grid: [width, height],
                tex: [tex.0, tex.1],
            }),
        );

        let readback = CounterReadback::new(device, &format!("{}_counters", M::ID), M::STATS.len());

        // --- Pipelines ---
        // Every layout entry and every bind group entry comes from the name its shader gives the
        // binding, so a slot index cannot disagree with the shader that owns it.
        let resolve = |decl: &BindingDecl, a_is_current: bool| -> wgpu::BindingResource<'_> {
            if let Some((label, writes)) = buffer_target(decl) {
                let k = M::BUFFERS
                    .iter()
                    .position(|l| *l == label)
                    .unwrap_or_else(|| panic!("{}: no buffer labelled `{label}`, wanted by `{}`", M::ID, decl.name));
                let pair = &buffers[k];
                let (read, write) = if a_is_current {
                    (&pair.a, &pair.b)
                } else {
                    (&pair.b, &pair.a)
                };
                return if writes { write } else { read }.as_entire_binding();
            }
            match decl.name {
                "params" => step_params_buffer.as_entire_binding(),
                "dims" => dims_buffer.as_entire_binding(),
                "counters" => readback.binding(),
                "output" => wgpu::BindingResource::TextureView(&display_view),
                other => panic!("{}: `{other}` is reserved but the engine has no resource for it", M::ID),
            }
        };

        let build = |label: &str, shader: &str, decls: &[BindingDecl]| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = decls
                .iter()
                .enumerate()
                .map(|(i, decl)| layout_entry(i as u32, decl))
                .collect();
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(&format!("{}_{label}_layout", M::ID)),
                entries: &entries,
            });
            let make = |a_is_current: bool, side: &str| {
                let entries: Vec<wgpu::BindGroupEntry<'_>> = decls
                    .iter()
                    .enumerate()
                    .map(|(i, decl)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: resolve(decl, a_is_current),
                    })
                    .collect();
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("{}_{label}_bind_{side}", M::ID)),
                    layout: &layout,
                    entries: &entries,
                })
            };
            let binds = (make(true, "a"), make(false, "b"));
            let pipeline = compute_pipeline(device, &format!("{}_{label}", M::ID), shader, &layout);
            (pipeline, binds)
        };

        let (step_pipeline, (bind_a2b, bind_b2a)) = build("step", M::STEP_SHADER, M::STEP_BINDINGS);
        let (display_pipeline, (display_bind_a, display_bind_b)) =
            build("display", M::DISPLAY_SHADER, M::DISPLAY_BINDINGS);
        let (reduce_pipeline, (reduce_bind_a, reduce_bind_b)) = build("reduce", M::REDUCE_SHADER, M::REDUCE_BINDINGS);

        Self {
            width,
            height,
            tex,
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

    /// Workgroups for reduce, at one invocation per cell.
    fn cell_workgroups(&self) -> (u32, u32) {
        (
            self.width.div_ceil(M::WORKGROUP_SIZE),
            self.height.div_ceil(M::WORKGROUP_SIZE),
        )
    }

    /// Same as [`Self::cell_workgroups`] until the grid outgrows the texture cap.
    fn texel_workgroups(&self) -> (u32, u32) {
        (
            self.tex.0.div_ceil(M::WORKGROUP_SIZE),
            self.tex.1.div_ceil(M::WORKGROUP_SIZE),
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
        // Two ping-ponged sides per buffer, plus the capped RGBA display texture.
        let buffers: usize = M::buffer_lens(self.width, self.height)
            .iter()
            .map(|len| len * std::mem::size_of::<u32>() * 2)
            .sum();
        let display_texture = (self.tex.0 as usize) * (self.tex.1 as usize) * 4;
        buffers + display_texture
    }
}

impl<M: GpuGridModel> GpuSimState for GpuGridState<M> {
    /// Records `count` step dispatches into `encoder`, one compute pass per step.
    ///
    /// Each step is a read-after-write hazard on the ping-ponged state buffers, and wgpu only
    /// inserts barriers *between* passes, not between dispatches within one. So this opens one
    /// pass per step rather than looping dispatches inside one, which would read stale data.
    /// Batching happens at the *submission* level instead, one encoder for all `count` passes.
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
            // `Some`, so only the first and last passes of the batch get one.
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
        {
            let (wg_x, wg_y) = self.texel_workgroups();
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_grid_display_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, self.current_display_bind_group(), &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        let (wg_x, wg_y) = self.cell_workgroups();

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

    fn stats_readback_pending(&self) -> bool {
        self.readback.is_pending()
    }

    /// Grid only, a `GpuGridModel` has no agent layer.
    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: Some(Arc::clone(&self.display)),
            agents: None,
        }
    }
}
