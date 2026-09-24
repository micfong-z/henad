//! GPU Virus on a Network, [`crate::virus_network`] with its nodes and edges in GPU buffers.
//!
//! Tick 0 is bit identical, since seeding goes through the CPU model's `init`. After that each node draws from its own
//! `pcg_hash` stream, so a run replays but does not follow the CPU's.

use std::sync::Arc;

use henad_compute::cpu::network_engine::{
    NETWORK_INIT_SEED, NETWORK_PARAM_BASE, NUM_NODES, NetworkModelState, WORLD_HEIGHT, WORLD_WIDTH,
    network_model_param_descriptors,
};
use henad_compute::gpu::capacity::Demand;
use henad_compute::gpu::primitives::dispatch::linear_dispatch;
use henad_compute::gpu::primitives::pipeline::{
    bind_group, compute_pipeline, lane_buffer, storage_buffer, uniform_buffer,
};
use henad_compute::gpu::primitives::readback::CounterReadback;
use henad_compute::gpu::primitives::rows::EDGE_WORDS;
use henad_compute::gpu::{GpuAgents, GpuContext, GpuEdges, GpuRows, GpuSimState};
use henad_compute::snapshot::GpuSnapshot;
use henad_core::action::action_seed;
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::{NetworkModel as _, Nodes};
use henad_core::authoring::primitives::rng::mix_seed;
use henad_core::helpers::{extract_f32, extract_u32};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatEntry, StatValue, stat_entries};

use crate::shader_bindings::gpu_virus_network::node_state::STATES_PER_WORD;
use crate::shader_bindings::gpu_virus_network::pack::PackParams;
use crate::shader_bindings::gpu_virus_network::recolor_edges::EdgeColorParams;
use crate::shader_bindings::gpu_virus_network::recolor_nodes::NodeColorParams;
use crate::shader_bindings::gpu_virus_network::rewire::RewireParams;
use crate::shader_bindings::gpu_virus_network::step::StepParams;
use crate::virus_network::{EDGE_PALETTE, PALETTE, REWIRE_TRIES, VirusNetwork};

pub const ID: &str = "gpu_virus_network";
pub const NAME: &str = "Virus on a Network (GPU)";
pub const DESCRIPTION: &str = "A virus spreading over a network, stepped entirely on the GPU.";

// Salts that keep the GPU's streams apart from the seed the CPU `init` draws from.
const RNG_SALT: u64 = 0x5EED_5EED_5EED_5EED;
const REWIRE_SALT: u64 = 0x2E_1CE5_5EED_0001;

// Indices into the `streams` buffer, one word each.
const TICK_STREAM: usize = 0;
const ACTION_STREAM: usize = 1;

// Listed in the Model panel. The first two passes run only with Keep Rewiring.
pub const BUFFERS: &[&str] = &[
    "state",
    "state_bits",
    "timer",
    "rng",
    "pos",
    "color",
    "edges",
    "row_start",
    "entries",
    "streams",
];
pub const PASSES: &[&str] = &["rewire", "rows rebuild", "step", "pack"];

/// Returns the full param list, which is the CPU model's, every entry reload-only.
pub fn param_descriptors() -> Vec<ParamDescriptor> {
    network_model_param_descriptors::<VirusNetwork>()
        .into_iter()
        .map(ParamDescriptor::on_reload)
        .collect()
}

/// A compute pass with one bind group, or one per RNG stream.
struct Pass {
    label: &'static str,
    pipeline: wgpu::ComputePipeline,
    binds: Vec<wgpu::BindGroup>,
    groups: (u32, u32),
}

impl Pass {
    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind: usize,
        stamps: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(self.label),
            timestamp_writes: stamps,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.binds[bind], &[]);
        pass.dispatch_workgroups(self.groups.0, self.groups.1, 1);
    }
}

pub struct GpuVirusNetwork {
    device: wgpu::Device,
    queue: wgpu::Queue,
    num_nodes: u32,
    tick: u64,

    state: wgpu::Buffer,
    /// Every node's state packed into 2 bits, as of the start of the tick.
    state_bits: wgpu::Buffer,
    timer: wgpu::Buffer,
    rng: wgpu::Buffer,
    /// The RNG words of the tick's rewire and of the Rewire a link action.
    streams: wgpu::Buffer,
    rows: GpuRows,
    counters: CounterReadback,
    keep_rewiring: bool,

    rewire: Pass,
    step: Pass,
    pack: Pass,
    recolor_nodes: Pass,
    recolor_edges: Pass,

    agents: Arc<GpuAgents>,
    edges: Arc<GpuEdges>,
}

impl GpuVirusNetwork {
    pub fn new(ctx: &GpuContext, params: &[ParamValue], seed: Option<u64>) -> Self {
        Self::from_cpu(ctx, params, seed, &NetworkModelState::from_params_seeded(params, seed))
    }

    /// Creates a state, then passes it to `seed_graph` to set up a specific graph, as
    /// [`NetworkModelState::from_graph`] does.
    pub fn from_graph(
        ctx: &GpuContext,
        params: &[ParamValue],
        seed: Option<u64>,
        seed_graph: impl FnOnce(&mut Nodes<'_, VirusNetwork>, Extent),
    ) -> Self {
        Self::from_cpu(
            ctx,
            params,
            seed,
            &NetworkModelState::from_graph(params, seed, seed_graph),
        )
    }

    /// Resources the model would allocate for `params`, with the edge count a Random network would have.
    ///
    /// Note that a Geometric network only aims at that count, and construction checks the real one.
    pub fn demand(params: &[ParamValue]) -> Demand {
        let nodes = extract_u32(params, NUM_NODES, VirusNetwork::DEFAULT_NODES);
        let degree_index = VirusNetwork::param_descriptors()
            .iter()
            .position(|desc| desc.id == "average_node_degree")
            .map_or(usize::MAX, |i| NETWORK_PARAM_BASE + i);
        let degree = u64::from(extract_u32(params, degree_index, 6));
        let n = u64::from(nodes);
        let edges = (degree * n).div_ceil(2).min(n * n.saturating_sub(1) / 2);
        demand_for(nodes, edges as u32)
    }

    /// Storage buffers the widest pass binds.
    pub fn max_storage_bindings() -> u32 {
        passes().into_iter().map(|(_, storage)| storage).max().unwrap_or(0)
    }

    #[expect(clippy::too_many_lines, reason = "one linear construction of every wgpu object")]
    fn from_cpu(
        ctx: &GpuContext,
        params: &[ParamValue],
        seed: Option<u64>,
        cpu: &NetworkModelState<VirusNetwork>,
    ) -> Self {
        let (device, queue) = (&ctx.device, &ctx.queue);
        let lanes = cpu.lanes();
        let graph = cpu.graph();
        let num_nodes = lanes.state.len() as u32;
        let num_edges = graph.edge_count() as u32;

        let limits = device.limits();
        let shortfalls = demand_for(num_nodes, num_edges).shortfalls(&limits);
        assert!(
            shortfalls.is_empty(),
            "{ID} does not fit this device at these params: {}",
            shortfalls.join("; ")
        );

        let extent = Extent {
            w: extract_f32(params, WORLD_WIDTH, VirusNetwork::DEFAULT_EXTENT.w),
            h: extract_f32(params, WORLD_HEIGHT, VirusNetwork::DEFAULT_EXTENT.h),
        };
        let hot = VirusNetwork::from_params(&params[NETWORK_PARAM_BASE.min(params.len())..], extent);
        let n = num_nodes as usize;

        let states: Vec<u32> = lanes.state.iter().map(|&s| u32::from(s)).collect();
        let state = storage_buffer(device, &format!("{ID}_state"), n);
        queue.write_buffer(&state, 0, bytemuck::cast_slice(&states));
        let num_words = num_nodes.div_ceil(STATES_PER_WORD);
        let state_bits = storage_buffer(device, &format!("{ID}_state_bits"), num_words as usize);
        let timer = storage_buffer(device, &format!("{ID}_timer"), n);
        queue.write_buffer(&timer, 0, bytemuck::cast_slice(&lanes.timer));

        // Hashed before each draw, so neighbouring nodes need no more than distinct words.
        let rng_seed = seed.map_or(NETWORK_INIT_SEED ^ RNG_SALT, |s| mix_seed(s ^ RNG_SALT));
        let rng_base = (rng_seed ^ (rng_seed >> 32)) as u32;
        let words: Vec<u32> = (0..num_nodes).map(|i| rng_base ^ i).collect();
        let rng = storage_buffer(device, &format!("{ID}_rng"), n);
        queue.write_buffer(&rng, 0, bytemuck::cast_slice(&words));

        let positions: Vec<f32> = lanes
            .pos_x
            .iter()
            .zip(&lanes.pos_y)
            .flat_map(|(&x, &y)| [x, y])
            .collect();
        let pos = lane_buffer(device, &format!("{ID}_pos"), 2 * n);
        queue.write_buffer(&pos, 0, bytemuck::cast_slice(&positions));
        let node_palette = packed(PALETTE);
        let colors: Vec<u32> = states.iter().map(|&s| node_palette[s as usize]).collect();
        let color = lane_buffer(device, &format!("{ID}_color"), n);
        queue.write_buffer(&color, 0, bytemuck::cast_slice(&colors));

        let edge_palette = packed(EDGE_PALETTE);
        let (src, dst, edge_colors) = graph.edges();
        let edge_words: Vec<u32> = (0..src.len())
            .flat_map(|e| [src[e], dst[e], edge_palette[usize::from(edge_colors[e])]])
            .collect();
        let edge_list = lane_buffer(device, &format!("{ID}_edges"), EDGE_WORDS * num_edges.max(1) as usize);
        queue.write_buffer(&edge_list, 0, bytemuck::cast_slice(&edge_words));

        let rows = GpuRows::new(device, queue, ID, &edge_list, num_nodes, num_edges, graph.directed());
        let counters = CounterReadback::new(device, &format!("{ID}_counters"), 3);

        let fold = |word: u64| (word ^ (word >> 32)) as u32;
        let tick_word = seed.map_or(NETWORK_INIT_SEED ^ REWIRE_SALT, |s| mix_seed(s ^ REWIRE_SALT));
        let streams = storage_buffer(device, &format!("{ID}_streams"), 2);
        queue.write_buffer(
            &streams,
            0,
            bytemuck::cast_slice(&[fold(tick_word), fold(action_seed(seed))]),
        );
        let rewire = rewire_pass(
            device,
            queue,
            &edge_list,
            &rows,
            &streams,
            num_nodes,
            num_edges,
            graph.directed(),
        );

        let node_groups = linear_dispatch(num_nodes);
        let edge_groups = linear_dispatch(num_edges);

        let step_uniform = uniform_buffer(
            device,
            queue,
            &format!("{ID}_step_params"),
            bytemuck::bytes_of(&StepParams {
                num_nodes,
                groups_x: node_groups.0,
                check_frequency: hot.check_frequency,
                escape_chance: 1.0 - hot.spread_chance,
                recovery_chance: hot.recovery_chance,
                resistance_chance: hot.resistance_chance,
                _pad0: 0,
                _pad1: 0,
            }),
        );
        let step_layout = device.create_bind_group_layout(
            &crate::shader_bindings::gpu_virus_network::step::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let step = Pass {
            label: "gpu_virus_network_step",
            pipeline: compute_pipeline(
                device,
                &format!("{ID}_step"),
                crate::shader_bindings::gpu_virus_network::step::SHADER_STRING,
                &step_layout,
            ),
            binds: vec![bind_group(
                device,
                &format!("{ID}_step_bind"),
                &step_layout,
                &[
                    state_bits.as_entire_binding(),
                    state.as_entire_binding(),
                    timer.as_entire_binding(),
                    rng.as_entire_binding(),
                    rows.row_start_binding(),
                    rows.entries_binding(),
                    step_uniform.as_entire_binding(),
                ],
            )],
            groups: node_groups,
        };

        let word_groups = linear_dispatch(num_words);
        let pack_uniform = uniform_buffer(
            device,
            queue,
            &format!("{ID}_pack_params"),
            bytemuck::bytes_of(&PackParams {
                num_nodes,
                num_words,
                groups_x: word_groups.0,
                _pad: 0,
            }),
        );
        let pack_layout = device.create_bind_group_layout(
            &crate::shader_bindings::gpu_virus_network::pack::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let pack = Pass {
            label: "gpu_virus_network_pack",
            pipeline: compute_pipeline(
                device,
                &format!("{ID}_pack"),
                crate::shader_bindings::gpu_virus_network::pack::SHADER_STRING,
                &pack_layout,
            ),
            binds: vec![bind_group(
                device,
                &format!("{ID}_pack_bind"),
                &pack_layout,
                &[
                    state.as_entire_binding(),
                    state_bits.as_entire_binding(),
                    pack_uniform.as_entire_binding(),
                ],
            )],
            groups: word_groups,
        };

        let node_color_uniform = uniform_buffer(
            device,
            queue,
            &format!("{ID}_recolor_nodes_params"),
            bytemuck::bytes_of(&NodeColorParams {
                num_nodes,
                groups_x: node_groups.0,
                _pad0: 0,
                _pad1: 0,
                palette: [node_palette[0], node_palette[1], node_palette[2], 0],
            }),
        );
        let node_color_layout = device.create_bind_group_layout(
            &crate::shader_bindings::gpu_virus_network::recolor_nodes::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let recolor_nodes = Pass {
            label: "gpu_virus_network_recolor_nodes",
            pipeline: compute_pipeline(
                device,
                &format!("{ID}_recolor_nodes"),
                crate::shader_bindings::gpu_virus_network::recolor_nodes::SHADER_STRING,
                &node_color_layout,
            ),
            binds: vec![bind_group(
                device,
                &format!("{ID}_recolor_nodes_bind"),
                &node_color_layout,
                &[
                    state.as_entire_binding(),
                    color.as_entire_binding(),
                    counters.binding(),
                    node_color_uniform.as_entire_binding(),
                ],
            )],
            groups: node_groups,
        };

        let edge_color_uniform = uniform_buffer(
            device,
            queue,
            &format!("{ID}_recolor_edges_params"),
            bytemuck::bytes_of(&EdgeColorParams {
                num_edges,
                groups_x: edge_groups.0,
                open: edge_palette[usize::from(crate::virus_network::EDGE_OPEN)],
                blocked: edge_palette[usize::from(crate::virus_network::EDGE_BLOCKED)],
            }),
        );
        let edge_color_layout = device.create_bind_group_layout(
            &crate::shader_bindings::gpu_virus_network::recolor_edges::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let recolor_edges = Pass {
            label: "gpu_virus_network_recolor_edges",
            pipeline: compute_pipeline(
                device,
                &format!("{ID}_recolor_edges"),
                crate::shader_bindings::gpu_virus_network::recolor_edges::SHADER_STRING,
                &edge_color_layout,
            ),
            binds: vec![bind_group(
                device,
                &format!("{ID}_recolor_edges_bind"),
                &edge_color_layout,
                &[
                    state_bits.as_entire_binding(),
                    edge_list.as_entire_binding(),
                    edge_color_uniform.as_entire_binding(),
                ],
            )],
            groups: edge_groups,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_virus_network_seed"),
        });
        rows.encode_build(&mut encoder);
        pack.encode(&mut encoder, 0, None);
        queue.submit(Some(encoder.finish()));

        let agents = Arc::new(GpuAgents {
            pos,
            color,
            count: num_nodes,
            world_w: extent.w,
            world_h: extent.h,
        });
        let edges = Arc::new(GpuEdges {
            edges: edge_list,
            count: num_edges,
            directed: graph.directed(),
        });

        Self {
            device: device.clone(),
            queue: queue.clone(),
            num_nodes,
            tick: 0,
            state,
            state_bits,
            timer,
            rng,
            streams,
            rows,
            counters,
            keep_rewiring: hot.keep_rewiring,
            rewire,
            step,
            pack,
            recolor_nodes,
            recolor_edges,
            agents,
            edges,
        }
    }
}

impl SimState for GpuVirusNetwork {
    /// Fallback for callers holding only a `SimState`. The sim thread batches instead.
    fn step(&mut self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_virus_network_single_step"),
        });
        self.encode_steps(&mut encoder, 1, None);
        self.queue.submit(Some(encoder.finish()));
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn stats(&self) -> Vec<StatEntry> {
        let counts = self.counters.values();
        stat_entries(
            VirusNetwork::STATS,
            counts
                .iter()
                .map(|&count| StatValue::Scalar(f64::from(count)))
                .collect(),
        )
    }

    fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
        false
    }

    /// Encodes the action into a submission of its own.
    fn act(&mut self, index: usize) -> bool {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_virus_network_action"),
        });
        if !self.encode_action(&mut encoder, index) {
            return false;
        }
        self.queue.submit(Some(encoder.finish()));
        true
    }

    fn population(&self) -> u64 {
        u64::from(self.num_nodes)
    }

    fn heap_bytes(&self) -> usize {
        let buffers = [
            &self.state,
            &self.state_bits,
            &self.timer,
            &self.rng,
            &self.streams,
            &self.agents.pos,
            &self.agents.color,
            &self.edges.edges,
        ];
        buffers.iter().map(|buffer| buffer.size() as usize).sum::<usize>()
            + self.rows.heap_bytes()
            + 3 * size_of::<u32>()
    }
}

impl GpuSimState for GpuVirusNetwork {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        for i in 0..count {
            let (first, last) = (i == 0, i == count - 1);
            if self.keep_rewiring {
                self.rewire
                    .encode(encoder, TICK_STREAM, stamps(timestamps, first, false));
                self.rows.encode_build(encoder);
            }
            self.step
                .encode(encoder, 0, stamps(timestamps, first && !self.keep_rewiring, false));
            self.pack.encode(encoder, 0, stamps(timestamps, false, last));
        }
        self.tick += u64::from(count);
    }

    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        if VirusNetwork::ACTIONS.get(index).map(|action| action.id) != Some("rewire") {
            return false;
        }
        self.rewire.encode(encoder, ACTION_STREAM, None);
        self.rows.encode_build(encoder);
        true
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.counters.encode_clear(encoder);
        self.recolor_nodes.encode(encoder, 0, None);
        self.recolor_edges.encode(encoder, 0, None);
        self.counters.encode_copy(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.counters.begin_map();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) {
        if block {
            self.counters.poll_blocking(device);
        } else {
            self.counters.poll(device);
        }
    }

    fn stats_readback_pending(&self) -> bool {
        self.counters.is_pending()
    }

    fn view(&self) -> GpuSnapshot {
        GpuSnapshot {
            display: None,
            agents: Some(Arc::clone(&self.agents)),
            edges: Some(Arc::clone(&self.edges)),
        }
    }
}

/// Returns the timestamp writes for a pass that opens or closes a batch, or `None` for one that does neither.
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

/// Builds the single-invocation rewire, with one bind group per RNG stream.
#[expect(clippy::too_many_arguments, reason = "the rewire binds most of the graph")]
fn rewire_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    edges: &wgpu::Buffer,
    rows: &GpuRows,
    streams: &wgpu::Buffer,
    num_nodes: u32,
    num_edges: u32,
    directed: bool,
) -> Pass {
    let layout = device.create_bind_group_layout(
        &crate::shader_bindings::gpu_virus_network::rewire::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
    );
    let binds = [TICK_STREAM, ACTION_STREAM].map(|stream| {
        let uniform = uniform_buffer(
            device,
            queue,
            &format!("{ID}_rewire_params{stream}"),
            bytemuck::bytes_of(&RewireParams {
                num_nodes,
                num_edges,
                directed: u32::from(directed),
                tries: REWIRE_TRIES,
                stream: stream as u32,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            }),
        );
        bind_group(
            device,
            &format!("{ID}_rewire_bind{stream}"),
            &layout,
            &[
                edges.as_entire_binding(),
                rows.row_start_binding(),
                rows.entries_binding(),
                streams.as_entire_binding(),
                uniform.as_entire_binding(),
            ],
        )
    });
    Pass {
        label: "gpu_virus_network_rewire",
        pipeline: compute_pipeline(
            device,
            &format!("{ID}_rewire"),
            crate::shader_bindings::gpu_virus_network::rewire::SHADER_STRING,
            &layout,
        ),
        binds: binds.into(),
        groups: (1, 1),
    }
}

/// Every pass the model encodes, with the storage buffers each binds.
fn passes() -> Vec<(&'static str, u32)> {
    let storage = |decls: &[BindingDecl]| decls.iter().filter(|decl| decl.kind.is_storage_buffer()).count() as u32;
    let mut passes = vec![
        (
            "rewire",
            storage(crate::binding_decls::bindings::GPU_VIRUS_NETWORK_REWIRE),
        ),
        ("step", storage(crate::binding_decls::bindings::GPU_VIRUS_NETWORK_STEP)),
        ("pack", storage(crate::binding_decls::bindings::GPU_VIRUS_NETWORK_PACK)),
        (
            "recolor_nodes",
            storage(crate::binding_decls::bindings::GPU_VIRUS_NETWORK_RECOLOR_NODES),
        ),
        (
            "recolor_edges",
            storage(crate::binding_decls::bindings::GPU_VIRUS_NETWORK_RECOLOR_EDGES),
        ),
    ];
    passes.extend(GpuRows::storage_bindings());
    passes
}

fn demand_for(num_nodes: u32, num_edges: u32) -> Demand {
    let n = num_nodes as usize;
    let mut demand = Demand::default();
    demand.push(format!("{ID}_state"), n);
    demand.push(format!("{ID}_state_bits"), num_nodes.div_ceil(STATES_PER_WORD) as usize);
    for (label, words) in [("timer", n), ("rng", n), ("pos", 2 * n), ("color", n)] {
        demand.push(format!("{ID}_{label}"), words);
    }
    demand.push(format!("{ID}_edges"), EDGE_WORDS * num_edges.max(1) as usize);
    demand.push_rows(ID, num_nodes, num_edges);
    demand.push(format!("{ID}_streams"), 2);
    demand.push(format!("{ID}_counters"), 3);
    for (label, storage) in passes() {
        demand.push_pass(format!("{ID}_{label}"), storage);
    }
    demand
}

/// Returns `palette` as packed RGBA, the form a shader writes into a drawable lane.
fn packed<const N: usize>(palette: [[u8; 4]; N]) -> [u32; N] {
    palette.map(u32::from_le_bytes)
}

#[cfg(test)]
impl GpuVirusNetwork {
    /// Returns every node's state at the current tick. Blocks on the GPU.
    pub(crate) fn read_states(&self) -> Vec<u32> {
        henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.state)
            [..self.num_nodes as usize]
            .to_vec()
    }

    /// Returns every node's state as unpacked from `state_bits`.
    pub(crate) fn read_state_bits(&self) -> Vec<u32> {
        let words = henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.state_bits);
        (0..self.num_nodes)
            .map(|i| {
                (words[(i / STATES_PER_WORD) as usize]
                    >> (crate::shader_bindings::gpu_virus_network::node_state::STATE_BITS * (i % STATES_PER_WORD)))
                    & 3
            })
            .collect()
    }

    pub(crate) fn read_timers(&self) -> Vec<u32> {
        henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.timer)
            [..self.num_nodes as usize]
            .to_vec()
    }

    /// Returns every node's packed RGBA colour, as the last snapshot passes painted it.
    pub(crate) fn read_node_colors(&self) -> Vec<u32> {
        henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.agents.color)
            [..self.num_nodes as usize]
            .to_vec()
    }

    /// Returns the positions as `(x, y)` pairs.
    pub(crate) fn read_positions(&self) -> Vec<f32> {
        let words = henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.agents.pos);
        words[..2 * self.num_nodes as usize]
            .iter()
            .map(|&w| f32::from_bits(w))
            .collect()
    }

    /// Returns the edge list as `(src, dst, color)` words.
    pub(crate) fn read_edges(&self) -> Vec<[u32; 3]> {
        let words = henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.edges.edges);
        words[..EDGE_WORDS * self.edges.count as usize]
            .chunks_exact(EDGE_WORDS)
            .map(|edge| [edge[0], edge[1], edge[2]])
            .collect()
    }

    /// Returns the RNG words of the tick's rewire and of the Rewire a link action.
    pub(crate) fn read_streams(&self) -> [u32; 2] {
        let words = henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, &self.streams);
        [words[TICK_STREAM], words[ACTION_STREAM]]
    }

    /// Returns `(row_start, entries)`, laid out as [`GpuRows`] describes.
    pub(crate) fn read_rows(&self) -> (Vec<u32>, Vec<u32>) {
        let read = |buffer| henad_compute::gpu::primitives::readback::read_words(&self.device, &self.queue, buffer);
        let table = 2 * self.num_nodes as usize + 1;
        (
            read(self.rows.row_start_buffer())[..table].to_vec(),
            read(self.rows.entries_buffer())[..2 * self.edges.count as usize].to_vec(),
        )
    }
}
