//! Generic engine turning any [`GpuAgentModel`] into a runnable [`GpuSimState`].
//!
//! Compare with [`crate::gpu::grid_engine`], and with [`crate::cpu::agent_engine`].

use std::marker::PhantomData;
use std::sync::Arc;

use henad_core::authoring::field::Extent;
use henad_core::authoring::gpu_agent_model::{Binding, Geometry, GpuAgentModel, PassCtx, PassId};
use henad_core::model::{Model, SimState};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatEntry, stat_entries};

use crate::display_scale::display_dims;
use crate::gpu::GpuContext;
use crate::gpu::capacity::Demand;
use crate::gpu::primitives::dispatch::linear_dispatch;
use crate::gpu::primitives::pipeline::{
    compute_pipeline, lane_buffer, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
};
use crate::gpu::primitives::readback::CounterReadback;
use crate::gpu::primitives::reduce::GpuLaneReduce;
use crate::gpu::primitives::spatial_hash::{GpuSpatialHash, HashGrid};
use crate::gpu::primitives::wgsl;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::view::agents::GpuAgents;
use crate::gpu::view::display::{DisplayTarget, GpuDisplay, build_display_target};
use crate::snapshot::GpuSnapshot;

/// Steps per submission. Enough passes in one command buffer trips the OS GPU watchdog, after
/// which every readback returns zeros with no error and no panic.
const STEPS_PER_SUBMISSION: u32 = 64;

/// A thing that exists once, or once per ping-ponged side.
struct Sides<T> {
    a: T,
    b: Option<T>,
}

impl<T> Sides<T> {
    fn pick(&self, a_is_current: bool) -> &T {
        if a_is_current {
            &self.a
        } else {
            self.b.as_ref().unwrap_or(&self.a)
        }
    }
}

struct BufferSides {
    a: wgpu::Buffer,
    b: Option<wgpu::Buffer>,
}

impl BufferSides {
    /// `(current, next)`. The same buffer twice when this one is written in place.
    fn sides(&self, a_is_current: bool) -> (&wgpu::Buffer, &wgpu::Buffer) {
        match &self.b {
            None => (&self.a, &self.a),
            Some(b) if a_is_current => (&self.a, b),
            Some(b) => (b, &self.a),
        }
    }
}

struct EncodedPass {
    label: String,
    pipeline: wgpu::ComputePipeline,
    binds: Sides<wgpu::BindGroup>,
    groups: (u32, u32),
}

impl EncodedPass {
    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        a_is_current: bool,
        timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(&self.label),
            timestamp_writes: timestamps,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.binds.pick(a_is_current), &[]);
        pass.dispatch_workgroups(self.groups.0, self.groups.1, 1);
    }
}

/// Bindings that count against `max_storage_buffers_per_shader_stage`.
fn storage_bindings(bindings: &[Binding]) -> u32 {
    bindings.iter().filter(|b| b.is_storage_buffer()).count() as u32
}

/// A `ComputePassTimestampWrites` needs at least one index set, so a pass in the middle of a batch
/// gets `None` rather than a struct with two `None`s.
fn stamps(
    query_set: Option<&wgpu::QuerySet>,
    opening: bool,
    closing: bool,
) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
    query_set
        .filter(|_| opening || closing)
        .map(|query_set| wgpu::ComputePassTimestampWrites {
            query_set,
            beginning_of_pass_write_index: opening.then_some(0),
            end_of_pass_write_index: closing.then_some(1),
        })
}

/// The `Model` half for a [`GpuAgentModel`]: metadata plus a state factory.
pub struct GpuAgentModelDescriptor<M: GpuAgentModel> {
    ctx: GpuContext,
    _marker: PhantomData<M>,
}

impl<M: GpuAgentModel> GpuAgentModelDescriptor<M> {
    pub fn new(ctx: GpuContext) -> Self {
        Self {
            ctx,
            _marker: PhantomData,
        }
    }
}

impl<M: GpuAgentModel> Model for GpuAgentModelDescriptor<M> {
    type State = GpuAgentState<M>;

    fn name(&self) -> &'static str {
        M::NAME
    }

    fn id(&self) -> &'static str {
        M::ID
    }

    fn description(&self) -> &'static str {
        M::DESCRIPTION
    }

    /// Everything is reload-only here, because `GpuAgentState::set_param` rejects the lot.
    fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        M::param_descriptors()
            .into_iter()
            .map(ParamDescriptor::on_reload)
            .collect()
    }

    fn stat_descriptors(&self) -> Vec<StatDescriptor> {
        M::STATS.to_vec()
    }

    /// A model that declares a display pass draws a grid layer under its agents.
    fn topology_hint(&self) -> TopologyHint {
        TopologyHint {
            grid: M::DISPLAY.is_some(),
            agents: true,
        }
    }

    fn create_state(&self, params: &[ParamValue]) -> Self::State {
        GpuAgentState::new(&self.ctx, params)
    }
}

/// GPU-resident state for a [`GpuAgentModel`]. Owned exclusively by the GPU sim thread once
/// spawned.
pub struct GpuAgentState<M: GpuAgentModel> {
    geom: Geometry,
    tick: u64,

    device: wgpu::Device,
    queue: wgpu::Queue,

    buffers: Vec<BufferSides>,
    /// `true` when the `a` side of every double buffered buffer holds the current state.
    current_is_a: bool,
    /// Set when some buffer asked to be double buffered. Nothing flips otherwise.
    ping_pong: bool,

    /// Spatial hash, if the model declares one.
    index: Option<GpuSpatialHash>,
    index_binds: Option<Sides<wgpu::BindGroup>>,

    steps: Vec<EncodedPass>,
    display: Option<(EncodedPass, Arc<GpuDisplay>)>,

    reduce: GpuLaneReduce,
    reduce_pass: EncodedPass,
    counters: Option<CounterReadback>,

    agents: Sides<Arc<GpuAgents>>,

    _marker: PhantomData<M>,
}

impl<M: GpuAgentModel> GpuAgentState<M> {
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// Geometry for `params`, without touching a device. `limits` only fixes the display cap.
    pub fn geometry_for(params: &[ParamValue], limits: &wgpu::Limits) -> Geometry {
        let (num_agents, extent) = M::dims(params);
        let num_agents = num_agents.max(1);
        let extent = Extent {
            w: extent.w.max(1.0),
            h: extent.h.max(1.0),
        };
        let (width, height) = extent.cells();
        Geometry {
            num_agents,
            extent,
            width,
            height,
            n_cells: width * height,
            display: display_dims(width, height, limits.max_texture_dimension_2d),
            index: M::INDEX.then(|| HashGrid::new(extent, M::index_cell_size(params))),
        }
    }

    /// Resources that would be allocated for this model based on `params`.
    pub fn demand(params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        let geom = Self::geometry_for(params, limits);
        let mut demand = Demand::default();
        for (spec, len) in M::BUFFERS.iter().zip(M::buffer_lens(&geom)) {
            demand.push_sides(&format!("{}_{}", M::ID, spec.label), len, spec.double_buffered);
        }
        if M::INDEX {
            demand.push_index(M::ID, geom.n_cells, geom.num_agents);
        }
        if M::DISPLAY.is_some() {
            demand.set_display(geom.width, geom.height, limits);
        }

        for (label, storage) in Self::declared_passes() {
            demand.push_pass(label, storage);
        }
        demand
    }

    /// Storage buffers each declared pass binds. Read by both [`Self::demand`] and
    /// [`Self::max_storage_bindings`], so the device a host asks for and the shortfall the UI
    /// reports cannot disagree.
    fn declared_passes() -> Vec<(String, u32)> {
        let mut passes: Vec<(String, u32)> = M::STEP_PASSES
            .iter()
            .map(|spec| (format!("{}_{}", M::ID, spec.label), storage_bindings(spec.bindings)))
            .collect();
        if let Some(spec) = &M::DISPLAY {
            passes.push((format!("{}_display", M::ID), storage_bindings(spec.bindings)));
        }
        passes.push((format!("{}_reduce_leaf", M::ID), storage_bindings(M::REDUCE_BINDINGS)));
        passes
    }

    /// Independent of params, so a host can ask before it has a device.
    pub fn max_storage_bindings() -> u32 {
        Self::declared_passes()
            .into_iter()
            .map(|(_, storage)| storage)
            .max()
            .unwrap_or(0)
    }

    /// `None` uses the model's own default seed.
    ///
    /// # Panics
    ///
    /// If the device cannot hold the model. The backstop, not the diagnostic, since a UI
    /// asks [`Self::demand`] first.
    #[expect(clippy::too_many_lines, reason = "one linear construction of every wgpu object")]
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;

        let limits = device.limits();
        let shortfalls = Self::demand(params, &limits).shortfalls(&limits);
        assert!(
            shortfalls.is_empty(),
            "{} does not fit this device at these params: {}",
            M::ID,
            shortfalls.join("; ")
        );

        let mut geom = Self::geometry_for(params, &limits);
        let (num_agents, extent) = (geom.num_agents, geom.extent);
        let (width, height) = (geom.width, geom.height);

        // The hash fits its own grid to the cell size, which may not be the field's.
        let index =
            M::INDEX.then(|| GpuSpatialHash::new(device, queue, M::ID, extent, M::index_cell_size(params), num_agents));
        geom.index = index.as_ref().map(GpuSpatialHash::grid);

        // --- Buffers ---
        let lens = M::buffer_lens(&geom);
        assert_eq!(
            lens.len(),
            M::BUFFERS.len(),
            "{}: buffer_lens must return one length per BUFFERS entry",
            M::ID
        );
        let seeds = M::seed_buffers(&geom, params, seed);
        assert_eq!(
            seeds.len(),
            M::BUFFERS.len(),
            "{}: seed_buffers must return one vector per BUFFERS entry",
            M::ID
        );

        let buffers: Vec<BufferSides> = M::BUFFERS
            .iter()
            .zip(&lens)
            .map(|(spec, &len)| {
                let make_buffer = |side: &str| {
                    let label = format!("{}_{}_{side}", M::ID, spec.label);
                    if spec.drawable {
                        lane_buffer(device, &label, len)
                    } else {
                        storage_buffer(device, &label, len)
                    }
                };
                BufferSides {
                    a: make_buffer("a"),
                    // Only the current side is seeded, so the other is written by the first step.
                    b: spec.double_buffered.then(|| make_buffer("b")),
                }
            })
            .collect();

        let mut clear = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some(&format!("{}_seed", M::ID)),
        });
        for (k, (buffer, bytes)) in buffers.iter().zip(&seeds).enumerate() {
            if bytes.is_empty() {
                clear.clear_buffer(&buffer.a, 0, None);
                continue;
            }
            assert_eq!(
                bytes.len(),
                lens[k] * std::mem::size_of::<u32>(),
                "{}: seed for buffer '{}' must be buffer_lens[{k}] words of bytes",
                M::ID,
                M::BUFFERS[k].label
            );
            queue.write_buffer(&buffer.a, 0, bytes);
        }
        queue.submit(Some(clear.finish()));

        let ping_pong = M::BUFFERS.iter().any(|spec| spec.double_buffered);
        let index_binds = index.as_ref().map(|hash| {
            let pos = &buffers[M::POS_BUFFER];
            Sides {
                a: hash.bind_positions(device, &format!("{}_hash_bind_a", M::ID), &pos.a),
                b: pos
                    .b
                    .as_ref()
                    .map(|b| hash.bind_positions(device, &format!("{}_hash_bind_b", M::ID), b)),
            }
        });

        let counters =
            (M::COUNTERS > 0).then(|| CounterReadback::new(device, &format!("{}_counters", M::ID), M::COUNTERS));

        let display_spec = M::DISPLAY;
        // The display pass binds the view, the snapshot carries the handle.
        let (display_view, display_handle) = match display_spec
            .as_ref()
            .map(|_| build_display_target(device, ctx.target_format, width, height))
        {
            Some(DisplayTarget { view, display, .. }) => (Some(view), Some(display)),
            None => (None, None),
        };

        let reduce_domain = M::REDUCE_DOMAIN.invocations(&geom);
        let reduce = GpuLaneReduce::new(device, queue, M::ID, M::REDUCE_LANES, reduce_domain);

        let build = PassBuilder {
            device,
            queue,
            geom: &geom,
            params,
            buffers: &buffers,
            ping_pong,
            index: index.as_ref(),
            counters: counters.as_ref(),
            display_view: display_view.as_ref(),
            reduce: &reduce,
        };

        let steps: Vec<EncodedPass> = M::STEP_PASSES
            .iter()
            .enumerate()
            .map(|(i, spec)| {
                let invocations = spec.domain.invocations(&geom);
                build.linear_pass::<M>(PassId::Step(i), spec.label, spec.shader, spec.bindings, invocations)
            })
            .collect();

        // One invocation per texel, as the grid engine's display does.
        let display = display_spec.as_ref().zip(display_handle).map(|(spec, handle)| {
            let (tex_w, tex_h) = geom.display;
            let groups = (tex_w.div_ceil(spec.workgroup), tex_h.div_ceil(spec.workgroup));
            let pass = build.pass::<M>(
                PassId::Display,
                "display",
                spec.shader,
                spec.bindings,
                groups,
                tex_w * tex_h,
            );
            (pass, handle)
        });

        let reduce_groups = reduce.agent_groups();
        let reduce_pass = build.pass::<M>(
            PassId::Reduce,
            "reduce_leaf",
            &wgsl::reduce_leaf(M::REDUCE_HEADER, M::REDUCE_VALUE),
            M::REDUCE_BINDINGS,
            reduce_groups,
            reduce_domain,
        );

        let make_agents = |a_is_current: bool| {
            let (pos, _) = buffers[M::POS_BUFFER].sides(a_is_current);
            let (color, _) = buffers[M::COLOR_BUFFER].sides(a_is_current);
            Arc::new(GpuAgents {
                pos: pos.clone(),
                color: color.clone(),
                count: num_agents,
                world_w: extent.w,
                world_h: extent.h,
            })
        };
        let agents = Sides {
            a: make_agents(true),
            b: ping_pong.then(|| make_agents(false)),
        };

        Self {
            geom,
            tick: 0,
            device: device.clone(),
            queue: queue.clone(),
            buffers,
            current_is_a: true,
            ping_pong,
            index,
            index_binds,
            steps,
            display,
            reduce,
            reduce_pass,
            counters,
            agents,
            _marker: PhantomData,
        }
    }

    /// Steps in submission-sized batches, as the real runner does.
    pub fn run_batched(&mut self, steps: u32) {
        let mut remaining = steps;
        while remaining > 0 {
            let batch = remaining.min(STEPS_PER_SUBMISSION);
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("henad_gpu_agent_batch"),
            });
            self.encode_steps(&mut encoder, batch, None);
            self.queue.submit(Some(encoder.finish()));
            remaining -= batch;
        }
    }

    /// Runs the snapshot passes and waits for the readback, so `stats()` reports the current tick.
    pub fn refresh_stats(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_agent_snapshot"),
        });
        self.encode_snapshot_passes(&mut encoder);
        let device = self.device.clone();
        self.queue.submit(Some(encoder.finish()));
        self.begin_stats_readback();
        self.poll_stats_readback(&device, true);
    }

    /// The current side of buffer `index`, as raw words. Blocks on the GPU.
    pub fn read_buffer(&self, index: usize) -> Vec<u32> {
        let (buffer, _) = self.buffers[index].sides(self.current_is_a);
        let size = buffer.size();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_gpu_agent_readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_agent_readback"),
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit(Some(encoder.finish()));

        let (tx, rx) = flume::bounded(1);
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("readback poll");
        rx.recv().expect("readback channel").expect("readback map");
        let data = staging.slice(..).get_mapped_range().expect("readback range");
        let out = bytemuck::cast_slice::<u8, u32>(&data).to_vec();
        drop(data);
        staging.unmap();
        out
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geom
    }
}

/// Carries what every pass needs to resolve its bindings into wgpu objects.
struct PassBuilder<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    geom: &'a Geometry,
    params: &'a [ParamValue],
    buffers: &'a [BufferSides],
    ping_pong: bool,
    index: Option<&'a GpuSpatialHash>,
    counters: Option<&'a CounterReadback>,
    display_view: Option<&'a wgpu::TextureView>,
    reduce: &'a GpuLaneReduce,
}

impl PassBuilder<'_> {
    /// A pass over a linear invocation domain, folded onto the 2D workgroup grid.
    fn linear_pass<M: GpuAgentModel>(
        &self,
        id: PassId,
        label: &str,
        shader: &str,
        bindings: &[Binding],
        invocations: u32,
    ) -> EncodedPass {
        self.pass::<M>(id, label, shader, bindings, linear_dispatch(invocations), invocations)
    }

    /// `groups.0` is the fold width the prelude's `linear_index` expects, so it goes in the
    /// uniform as `groups_x`.
    fn pass<M: GpuAgentModel>(
        &self,
        id: PassId,
        label: &str,
        shader: &str,
        bindings: &[Binding],
        groups: (u32, u32),
        invocations: u32,
    ) -> EncodedPass {
        let label = format!("{}_{label}", M::ID);

        let uniform = uniform_buffer(
            self.device,
            self.queue,
            &format!("{label}_params"),
            &M::pass_params_bytes(
                id,
                PassCtx {
                    geom: self.geom,
                    invocations,
                    groups_x: groups.0,
                },
                self.params,
            ),
        );

        let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
            .iter()
            .enumerate()
            .map(|(i, binding)| Self::layout_entry(i as u32, binding))
            .collect();
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_layout")),
            entries: &entries,
        });

        let make_bind = |a_is_current: bool, side: &str| {
            let entries: Vec<wgpu::BindGroupEntry<'_>> = bindings
                .iter()
                .enumerate()
                .map(|(i, binding)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: self.resource(binding, a_is_current, &uniform),
                })
                .collect();
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{label}_bind_{side}")),
                layout: &layout,
                entries: &entries,
            })
        };

        let binds = Sides {
            a: make_bind(true, "a"),
            b: self.ping_pong.then(|| make_bind(false, "b")),
        };

        let source = format!("{}\n{shader}", wgsl::PRELUDE);
        let pipeline = compute_pipeline(self.device, &label, &source, &layout);
        EncodedPass {
            label,
            pipeline,
            binds,
            groups,
        }
    }

    fn layout_entry(i: u32, binding: &Binding) -> wgpu::BindGroupLayoutEntry {
        match binding {
            Binding::Read(_) | Binding::IndexCellStart | Binding::IndexSorted => storage_entry(i, true),
            Binding::Write(_) | Binding::Counters | Binding::ReducePartials => storage_entry(i, false),
            Binding::Uniform => uniform_entry(i),
            Binding::DisplayTexture => wgpu::BindGroupLayoutEntry {
                binding: i,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        }
    }

    fn resource<'r>(
        &'r self,
        binding: &Binding,
        a_is_current: bool,
        uniform: &'r wgpu::Buffer,
    ) -> wgpu::BindingResource<'r> {
        match binding {
            Binding::Read(k) => self.buffers[*k].sides(a_is_current).0.as_entire_binding(),
            Binding::Write(k) => self.buffers[*k].sides(a_is_current).1.as_entire_binding(),
            Binding::IndexCellStart => self.index.expect("INDEX is declared").cell_start_binding(),
            Binding::IndexSorted => self.index.expect("INDEX is declared").sorted_binding(),
            Binding::Counters => self.counters.expect("COUNTERS is non-zero").binding(),
            Binding::DisplayTexture => {
                wgpu::BindingResource::TextureView(self.display_view.expect("DISPLAY is declared"))
            }
            Binding::ReducePartials => self.reduce.partials_binding(),
            Binding::Uniform => uniform.as_entire_binding(),
        }
    }
}

impl<M: GpuAgentModel> SimState for GpuAgentState<M> {
    /// Fallback for callers holding only a `SimState`. The sim thread batches instead.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_agent_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        let counters = self.counters.as_ref().map_or(&[][..], CounterReadback::values);
        stat_entries(M::STATS, M::stats(&self.reduce.sums(), counters, &self.geom))
    }

    /// Resizing or reseeding live is currently unsupported.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }

    fn population(&self) -> u64 {
        u64::from(self.geom.num_agents)
    }

    fn heap_bytes(&self) -> usize {
        let buffers: usize = self
            .buffers
            .iter()
            .map(|sides| (sides.a.size() + sides.b.as_ref().map_or(0, wgpu::Buffer::size)) as usize)
            .sum();
        let display = self.display.as_ref().map_or(0, |_| {
            (self.geom.display.0 as usize) * (self.geom.display.1 as usize) * 4
        });
        buffers
            + display
            + self.index.as_ref().map_or(0, GpuSpatialHash::heap_bytes)
            + self.reduce.heap_bytes()
            + M::COUNTERS * std::mem::size_of::<u32>()
    }
}

impl<M: GpuAgentModel> GpuSimState for GpuAgentState<M> {
    /// One compute pass per declared pass per step, all recorded into one encoder.
    ///
    /// A pass is the synchronization boundary wgpu inserts barriers at, so a step's passes
    /// cannot be collapsed into dispatches inside one pass. The later ones would read stale
    /// data.
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if count == 0 {
            return;
        }
        let last_pass = self.steps.len().saturating_sub(1);

        for i in 0..count {
            let is_first = i == 0;
            let is_last = i == count - 1;

            // A batch begins with the index rebuild, so the opening stamp goes on its counting
            // pass. A stamp on an empty pass is silently never written.
            if let (Some(hash), Some(binds)) = (&self.index, &self.index_binds) {
                hash.encode_build(
                    encoder,
                    binds.pick(self.current_is_a),
                    timestamps.filter(|_| is_first).map(|query_set| (query_set, 0)),
                );
            }

            for (j, pass) in self.steps.iter().enumerate() {
                let opening = is_first && j == 0 && self.index.is_none();
                let closing = is_last && j == last_pass;
                pass.encode(encoder, self.current_is_a, stamps(timestamps, opening, closing));
            }

            if self.ping_pong {
                self.current_is_a = !self.current_is_a;
            }
        }

        self.tick += u64::from(count);
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some((pass, _)) = &self.display {
            pass.encode(encoder, self.current_is_a, None);
        }
        self.reduce_pass.encode(encoder, self.current_is_a, None);
        self.reduce.encode(encoder);
        if let Some(counters) = &mut self.counters {
            counters.encode_copy(encoder);
        }
    }

    fn begin_stats_readback(&mut self) {
        self.reduce.begin_readback();
        if let Some(counters) = &mut self.counters {
            counters.begin_map();
        }
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        self.reduce.poll_readback(device, block);
        if let Some(counters) = &mut self.counters {
            if block {
                counters.poll_blocking(device);
            } else {
                counters.poll(device);
            }
        }
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: self.display.as_ref().map(|(_, display)| Arc::clone(display)),
            agents: Some(Arc::clone(self.agents.pick(self.current_is_a))),
        }
    }
}
