//! The engine that runs a [`GpuAgentModel`] as a [`GpuSimState`], the counterpart of [`crate::cpu::agent_engine`].

use std::marker::PhantomData;
use std::sync::Arc;

use henad_core::authoring::model::gpu_agent_model::{Geometry, GpuAgentModel, PassCtx, PassId};
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, stat_entries};

use crate::gpu::agent_spec::{BufferSide, GpuAgentSpec, SideTable};
use crate::gpu::capacity::{Demand, layout_entry};
use crate::gpu::primitives::dispatch::linear_dispatch;
use crate::gpu::primitives::pipeline::{compute_pipeline, lane_buffer, storage_buffer, uniform_buffer};
use crate::gpu::primitives::readback::{CounterReadback, StatsPoll};
use crate::gpu::primitives::reduce::GpuLaneReduce;
use crate::gpu::primitives::spatial_hash::GpuSpatialHash;
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::view::agents::GpuAgents;
use crate::gpu::view::display::{DisplayTarget, GpuDisplay, build_display_target};
use crate::gpu::{GpuContext, MAX_STEPS_PER_SUBMISSION};
use crate::snapshot::GpuSnapshot;

/// A resource that exists once, or once per ping-ponged side.
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
    /// Returns `(current, next)`, the same buffer twice when this one is written in place.
    fn sides(&self, a_is_current: bool) -> (&wgpu::Buffer, &wgpu::Buffer) {
        match &self.b {
            None => (&self.a, &self.a),
            Some(b) if a_is_current => (&self.a, b),
            Some(b) => (b, &self.a),
        }
    }

    /// Returns the buffer on `side`, the same buffer on either side when this one is written in place.
    fn side(&self, a_is_current: bool, side: BufferSide) -> &wgpu::Buffer {
        let (current, next) = self.sides(a_is_current);
        match side {
            BufferSide::Current => current,
            BufferSide::Next => next,
        }
    }
}

struct EncodedPass {
    label: String,
    pipeline: wgpu::ComputePipeline,
    binds: Sides<wgpu::BindGroup>,
    groups: (u32, u32),
    /// Uniform of the pass, kept so an action can reseed it before a press. Unused for a pass that is not an action.
    uniform: wgpu::Buffer,
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

/// Returns the timestamp writes of a pass, `None` for a pass that neither opens nor closes a batch.
///
/// A `ComputePassTimestampWrites` needs at least one index set.
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

/// GPU-resident state built from a [`GpuAgentSpec`], with every buffer, pass and bind group the spec declares.
///
/// [`GpuAgentState`] builds the spec from a model type and forwards every call here.
///
/// A step calls no hook of the spec, an action press calls one for its uniform block, and [`SimState::stats`] calls one
/// for the values.
pub(crate) struct GpuAgentEngine {
    spec: Arc<GpuAgentSpec>,
    geom: Geometry,
    tick: u64,

    device: wgpu::Device,
    queue: wgpu::Queue,

    buffers: Vec<BufferSides>,
    /// `true` when the `a` side of every double buffered buffer holds the current state.
    current_is_a: bool,
    /// Set when some buffer requests double buffering. Nothing flips otherwise.
    ping_pong: bool,

    /// Spatial hash, if the model declares one.
    index: Option<GpuSpatialHash>,
    index_binds: Option<Sides<wgpu::BindGroup>>,

    steps: Vec<EncodedPass>,
    display: Option<(EncodedPass, Arc<GpuDisplay>)>,

    reduce: GpuLaneReduce,
    reduce_pass: EncodedPass,
    counters: Option<CounterReadback>,

    actions: Vec<EncodedPass>,
    /// Invocations of each action, in the order of [`Self::actions`].
    action_invocations: Vec<u32>,
    /// Seed of the next action press, advanced per press so that pressing twice draws twice.
    action_seed: u32,
    /// Parameter values, kept to rewrite the action uniforms on each press.
    params: Vec<ParamValue>,

    agents: Sides<Arc<GpuAgents>>,
}

impl std::fmt::Debug for GpuAgentEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuAgentEngine")
            .field("model", &self.spec.id)
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}

impl GpuAgentEngine {
    /// Builds the state for `spec` on `ctx`, seeded with `seed`, or with the model's own default seed when it is `None`.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot hold the model, or if [`GpuAgentSpec::assert_contracts`] panics. Also panics if the
    /// hooks return lengths or seeds for another number of buffers than [`GpuAgentSpec::buffers`] holds, or a seed
    /// whose length differs from the length of its buffer.
    #[expect(clippy::too_many_lines, reason = "one linear construction of every wgpu object")]
    #[cfg_attr(
        all(target_arch = "wasm32", target_feature = "atomics"),
        expect(
            clippy::arc_with_non_send_sync,
            reason = "the agent layers hold wgpu buffers, which atomics leave unsendable"
        )
    )]
    pub(crate) fn new(ctx: &GpuContext, spec: Arc<GpuAgentSpec>, params: &[ParamValue], seed: Option<u64>) -> Self {
        let device = &ctx.device;
        let queue = &ctx.queue;
        let id = &*spec.id;

        let limits = device.limits();
        let shortfalls = spec.demand(params, &limits).shortfalls(&limits);
        assert!(
            shortfalls.is_empty(),
            "{id} does not fit this device at these params: {}",
            shortfalls.join("; ")
        );
        spec.assert_contracts();

        let mut geom = spec.geometry(params, &limits);
        let (num_agents, extent) = (geom.num_agents, geom.extent);
        let (width, height) = (geom.width, geom.height);

        // The hash fits its own grid to the index cell size. That size can differ from the field's cell size.
        let index = spec.index.then(|| {
            GpuSpatialHash::new(
                device,
                queue,
                id,
                extent,
                spec.hooks.index_cell_size(params),
                num_agents,
            )
        });
        geom.index = index.as_ref().map(GpuSpatialHash::grid);

        // Buffers.
        let lens = spec.hooks.buffer_lens(&geom);
        assert_eq!(
            lens.len(),
            spec.buffers.len(),
            "{id}: buffer_lens must return one length per BUFFERS entry"
        );
        let seeds = spec.hooks.seed_buffers(&geom, params, seed);
        assert_eq!(
            seeds.len(),
            spec.buffers.len(),
            "{id}: seed_buffers must return one vector per BUFFERS entry"
        );

        let buffers: Vec<BufferSides> = spec
            .buffers
            .iter()
            .zip(&lens)
            .map(|(buffer, &len)| {
                let make_buffer = |side: &str| {
                    let label = format!("{id}_{}_{side}", buffer.label);
                    if buffer.drawable {
                        lane_buffer(device, &label, len)
                    } else {
                        storage_buffer(device, &label, len)
                    }
                };
                BufferSides {
                    a: make_buffer("a"),
                    // Only the current side is seeded, so the other side is written by the first step.
                    b: buffer.double_buffered.then(|| make_buffer("b")),
                }
            })
            .collect();

        let mut clear = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some(&format!("{id}_seed")),
        });
        for (k, (buffer, bytes)) in buffers.iter().zip(&seeds).enumerate() {
            if bytes.is_empty() {
                clear.clear_buffer(&buffer.a, 0, None);
                continue;
            }
            assert_eq!(
                bytes.len(),
                lens[k] * std::mem::size_of::<u32>(),
                "{id}: seed for buffer '{}' must be buffer_lens[{k}] words of bytes",
                spec.buffers[k].label
            );
            queue.write_buffer(&buffer.a, 0, bytes);
        }
        queue.submit(Some(clear.finish()));

        let ping_pong = spec.buffers.iter().any(|buffer| buffer.double_buffered);
        let index_binds = index.as_ref().map(|hash| {
            let pos = &buffers[spec.pos_buffer];
            Sides {
                a: hash.bind_positions(device, &format!("{id}_hash_bind_a"), &pos.a),
                b: pos
                    .b
                    .as_ref()
                    .map(|b| hash.bind_positions(device, &format!("{id}_hash_bind_b"), b)),
            }
        });

        let counters =
            (spec.counters > 0).then(|| CounterReadback::new(device, &format!("{id}_counters"), spec.counters));

        // The display pass binds the view, and the snapshot carries the handle.
        let (display_view, display_handle) = match spec
            .display
            .as_ref()
            .map(|_| build_display_target(device, ctx.target_format, width, height))
        {
            Some(DisplayTarget { view, display, .. }) => (Some(view), Some(display)),
            None => (None, None),
        };

        let reduce_domain = spec.reduce.domain.invocations(&geom);
        let reduce = GpuLaneReduce::new(device, queue, id, spec.reduce.lanes, reduce_domain);

        let build = PassBuilder {
            device,
            queue,
            spec: &spec,
            geom: &geom,
            params,
            buffers: &buffers,
            ping_pong,
            index: index.as_ref(),
            counters: counters.as_ref(),
            display_view: display_view.as_ref(),
            reduce: &reduce,
        };

        let steps: Vec<EncodedPass> = spec
            .steps
            .iter()
            .enumerate()
            .map(|(i, pass)| {
                let invocations = pass.domain.invocations(&geom);
                build.pass(
                    PassId::Step(i),
                    &pass.label,
                    &pass.shader,
                    &pass.sides,
                    linear_dispatch(invocations),
                    invocations,
                    0,
                )
            })
            .collect();

        // One invocation per texel, as the grid engine's display pass does.
        let display = spec.display.as_ref().zip(display_handle).map(|(pass, handle)| {
            let (tex_w, tex_h) = geom.display;
            let groups = (tex_w.div_ceil(pass.workgroup), tex_h.div_ceil(pass.workgroup));
            let encoded = build.pass(
                PassId::Display,
                "display",
                &pass.shader,
                &pass.sides,
                groups,
                tex_w * tex_h,
                0,
            );
            (encoded, handle)
        });

        let reduce_pass = build.pass(
            PassId::Reduce,
            "reduce_leaf",
            &spec.reduce.shader,
            &spec.reduce.sides,
            reduce.agent_groups(),
            reduce_domain,
            0,
        );

        // Truncated from the same stream the CPU engines use, since the WGSL generator is 32 bit.
        let action_seed = henad_core::action::action_seed(seed) as u32;
        let action_invocations: Vec<u32> = spec
            .actions
            .iter()
            .map(|action| action.domain.invocations(&geom))
            .collect();
        let actions: Vec<EncodedPass> = spec
            .actions
            .iter()
            .zip(&action_invocations)
            .enumerate()
            .map(|(i, (action, &invocations))| {
                build.pass(
                    PassId::Action(i),
                    &format!("action_{}", action.id),
                    &action.shader,
                    &action.sides,
                    linear_dispatch(invocations),
                    invocations,
                    action_seed,
                )
            })
            .collect();

        let make_agents = |a_is_current: bool| {
            let (pos, _) = buffers[spec.pos_buffer].sides(a_is_current);
            let (color, _) = buffers[spec.color_buffer].sides(a_is_current);
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
            spec,
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
            actions,
            action_invocations,
            action_seed,
            params: params.to_vec(),
            agents,
        }
    }

    /// Submits `steps` steps, at most [`MAX_STEPS_PER_SUBMISSION`] per command buffer, as the runner does. Nothing
    /// waits on the GPU.
    pub(crate) fn run_batched(&mut self, steps: u32) {
        let mut remaining = steps;
        while remaining > 0 {
            let batch = remaining.min(MAX_STEPS_PER_SUBMISSION);
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("henad_gpu_agent_batch"),
            });
            self.encode_steps(&mut encoder, batch, None);
            self.queue.submit(Some(encoder.finish()));
            remaining -= batch;
        }
    }

    /// Runs the snapshot passes and waits for the readback, so `stats()` reports the current tick.
    pub(crate) fn refresh_stats(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_agent_snapshot"),
        });
        self.encode_snapshot_passes(&mut encoder);
        let device = self.device.clone();
        self.queue.submit(Some(encoder.finish()));
        self.begin_stats_readback();
        self.poll_stats_readback(&device, true);
    }

    /// Returns the current side of buffer `index` as raw words, blocking on the GPU.
    ///
    /// # Panics
    ///
    /// Panics if the readback fails.
    pub(crate) fn read_buffer(&self, index: usize) -> Vec<u32> {
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
}

/// Carries what every pass needs to resolve its bindings into wgpu objects.
struct PassBuilder<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    spec: &'a GpuAgentSpec,
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
    /// Builds a pass over `groups` workgroups, binding each buffer through `sides`.
    ///
    /// `groups.0` is the fold width that `henad::dispatch::linear_index` expects, and goes in the uniform as
    /// `groups_x`. `seed` goes in the uniform as well, and is zero for a pass that is not an action.
    #[expect(
        clippy::too_many_arguments,
        reason = "every argument is a pass fact, and each call site names them"
    )]
    fn pass(
        &self,
        id: PassId,
        label: &str,
        shader: &ShaderSpec,
        sides: &SideTable,
        groups: (u32, u32),
        invocations: u32,
        seed: u32,
    ) -> EncodedPass {
        let label = format!("{}_{label}", self.spec.id);

        let uniform = uniform_buffer(
            self.device,
            self.queue,
            &format!("{label}_params"),
            &self.spec.hooks.pass_params_bytes(
                id,
                PassCtx {
                    geom: self.geom,
                    invocations,
                    groups_x: groups.0,
                    seed,
                },
                self.params,
            ),
        );

        let entries: Vec<wgpu::BindGroupLayoutEntry> = shader
            .bindings
            .iter()
            .enumerate()
            .map(|(i, binding)| layout_entry(i as u32, binding))
            .collect();
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_layout")),
            entries: &entries,
        });

        let make_bind = |a_is_current: bool, side: &str| {
            let entries: Vec<wgpu::BindGroupEntry<'_>> = shader
                .bindings
                .iter()
                .enumerate()
                .map(|(i, binding)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: self.resource(binding, a_is_current, sides, &uniform),
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

        let pipeline = compute_pipeline(self.device, &label, &shader.source, &layout);
        EncodedPass {
            label,
            pipeline,
            binds,
            groups,
            uniform,
        }
    }

    /// Returns the resource that a shader binding's name refers to. The name alone links the binding to its resource,
    /// and `sides` picks the side of a buffer.
    ///
    /// Panics rather than returning an error, since a name that resolves to nothing is a shader
    /// and model that disagree, and no run can be correct after that.
    fn resource<'r>(
        &'r self,
        binding: &ShaderBinding,
        a_is_current: bool,
        sides: &SideTable,
        uniform: &'r wgpu::Buffer,
    ) -> wgpu::BindingResource<'r> {
        let id = &self.spec.id;
        if let Some((label, writes)) = binding.buffer_target() {
            let k = self
                .spec
                .buffers
                .iter()
                .position(|buffer| buffer.label == label)
                .unwrap_or_else(|| panic!("{id}: no buffer labelled `{label}`, wanted by `{}`", binding.name));
            let row = sides.rows()[k];
            let side = if writes { row.write } else { row.read };
            return self.buffers[k].side(a_is_current, side).as_entire_binding();
        }
        match binding.name.as_ref() {
            "params" => uniform.as_entire_binding(),
            "cell_start" => self.index.expect("INDEX is declared").cell_start_binding(),
            "sorted" => self.index.expect("INDEX is declared").sorted_binding(),
            "counters" => self.counters.expect("COUNTERS is non-zero").binding(),
            "partials" => self.reduce.partials_binding(),
            "output" => wgpu::BindingResource::TextureView(self.display_view.expect("DISPLAY is declared")),
            other => panic!("{id}: `{other}` is reserved but the engine has no resource for it"),
        }
    }
}

impl SimState for GpuAgentEngine {
    /// Steps once in its own submission, for a caller that holds only a `SimState`. The GPU runner batches
    /// instead.
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
        stat_entries(
            &self.spec.stats,
            self.spec.hooks.stats(&self.reduce.sums(), counters, &self.geom),
        )
    }

    /// Encodes the action into its own submission.
    ///
    /// The GPU runner calls [`GpuSimState::encode_action`] instead, and submits the action before
    /// its snapshot.
    fn act(&mut self, index: usize) -> bool {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_agent_action"),
        });
        if !GpuSimState::encode_action(self, &mut encoder, index) {
            return false;
        }
        self.queue.submit(Some(encoder.finish()));
        true
    }

    /// Rejects every live edit. Its entry declares every parameter reload-only.
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
            + self.spec.counters * std::mem::size_of::<u32>()
    }
}

impl GpuSimState for GpuAgentEngine {
    /// Records one compute pass per declared pass per step, all into one encoder.
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
            &self.spec.hooks.pass_params_bytes(
                PassId::Action(index),
                PassCtx {
                    geom: &self.geom,
                    invocations: self.action_invocations[index],
                    groups_x: action.groups.0,
                    seed: self.action_seed,
                },
                &self.params,
            ),
        );
        action.encode(encoder, self.current_is_a, None);
        true
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some((pass, _)) = &self.display {
            pass.encode(encoder, self.current_is_a, None);
        }
        self.encode_stats_passes(encoder);
    }

    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
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

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        let sums_poll = self.reduce.poll_readback(device, block);
        let counters_poll = match &mut self.counters {
            Some(readback) if block => readback.poll_blocking(device),
            Some(readback) => readback.poll(device),
            None => StatsPoll::Landed,
        };
        match (sums_poll, counters_poll) {
            (StatsPoll::Pending, _) | (_, StatsPoll::Pending) => StatsPoll::Pending,
            (StatsPoll::Failed, _) | (_, StatsPoll::Failed) => StatsPoll::Failed,
            (StatsPoll::Landed, StatsPoll::Landed) => StatsPoll::Landed,
        }
    }

    fn stats_readback_pending(&self) -> bool {
        self.reduce.readback_pending() || self.counters.as_ref().is_some_and(CounterReadback::is_pending)
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: self.display.as_ref().map(|(_, display)| Arc::clone(display)),
            agents: Some(Arc::clone(self.agents.pick(self.current_is_a))),
        }
    }

    fn read_buffer(&self, index: usize) -> Option<Vec<u32>> {
        (index < self.buffers.len()).then(|| Self::read_buffer(self, index))
    }
}

/// GPU-resident state for a [`GpuAgentModel`], with every buffer, pass and bind group its model declares.
pub struct GpuAgentState<M: GpuAgentModel> {
    engine: GpuAgentEngine,
    _marker: PhantomData<M>,
}

impl<M: GpuAgentModel> std::fmt::Debug for GpuAgentState<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuAgentState")
            .field("model", &M::ID)
            .field("tick", &self.engine.tick)
            .finish_non_exhaustive()
    }
}

impl<M: GpuAgentModel> GpuAgentState<M> {
    /// Builds the state on `ctx`, seeded with the model's default seed.
    ///
    /// # Panics
    ///
    /// Panics as [`Self::new_seeded`] does.
    pub fn new(ctx: &GpuContext, params: &[ParamValue]) -> Self {
        Self::new_seeded(ctx, params, None)
    }

    /// Returns the geometry for `params`, without touching a device. `limits` sets only the display cap.
    pub fn geometry_for(params: &[ParamValue], limits: &wgpu::Limits) -> Geometry {
        GpuAgentSpec::from_model::<M>().geometry(params, limits)
    }

    /// Resources that would be allocated for this model based on `params`.
    pub fn demand(params: &[ParamValue], limits: &wgpu::Limits) -> Demand {
        GpuAgentSpec::from_model::<M>().demand(params, limits)
    }

    /// Returns the most storage buffers any declared pass binds. It does not depend on params, so a host can query it
    /// before it has a device.
    pub fn max_storage_bindings() -> u32 {
        GpuAgentSpec::from_model::<M>().max_storage_bindings()
    }

    /// Builds the state on `ctx`, seeded with `seed`, or with the model's own default seed when it is `None`.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot hold the model. A host calls [`Self::demand`] first, and this assert is the
    /// backstop. Also panics if a shader declares a workgroup size other than the one its pass dispatches, or a
    /// buffer label is reserved or ends in `_in` or `_out`.
    ///
    /// Panics if a pass binds one side of a buffer both read-only and writable. An action binds in place, so an action
    /// with both `x_in` and `x_out` panics here even if it is never pressed. Panics if [`GpuAgentModel::POS_BUFFER`] or
    /// [`GpuAgentModel::COLOR_BUFFER`] lies past the buffers or is not drawable, even for a model that only runs
    /// headless. Panics if no step pass writes a double buffered buffer through a writable binding.
    pub fn new_seeded(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        Self {
            engine: GpuAgentEngine::new(ctx, Arc::new(GpuAgentSpec::from_model::<M>()), params, seed),
            _marker: PhantomData,
        }
    }

    /// Submits `steps` steps, at most [`MAX_STEPS_PER_SUBMISSION`] per command buffer, as the runner does. Nothing
    /// waits on the GPU.
    pub fn run_batched(&mut self, steps: u32) {
        self.engine.run_batched(steps);
    }

    /// Runs the snapshot passes and waits for the readback, so `stats()` reports the current tick.
    pub fn refresh_stats(&mut self) {
        self.engine.refresh_stats();
    }

    /// Returns the current side of buffer `index` as raw words, blocking on the GPU.
    ///
    /// # Panics
    ///
    /// Panics if the readback fails.
    pub fn read_buffer(&self, index: usize) -> Vec<u32> {
        self.engine.read_buffer(index)
    }

    /// Geometry the state was built at.
    pub fn geometry(&self) -> &Geometry {
        &self.engine.geom
    }
}

// Forwards every call to the engine.
impl<M: GpuAgentModel> SimState for GpuAgentState<M> {
    /// Steps once in its own submission, for a caller that holds only a `SimState`. The GPU runner batches
    /// instead.
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
impl<M: GpuAgentModel> GpuSimState for GpuAgentState<M> {
    /// Records one compute pass per declared pass per step, all into one encoder.
    ///
    /// A pass is the synchronization boundary wgpu inserts barriers at, so a step's passes
    /// cannot be collapsed into dispatches inside one pass. The later ones would read stale
    /// data.
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

    fn view(&self) -> GpuSnapshot {
        self.engine.view()
    }

    fn read_buffer(&self, index: usize) -> Option<Vec<u32>> {
        GpuSimState::read_buffer(&self.engine, index)
    }
}
