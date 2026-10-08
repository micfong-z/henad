//! Multi-level float sum over an agent population, for stats an `atomic<u32>` cannot hold.
//!
//! The model supplies the leaf shader, and [`GpuLaneReduce`] owns the levels above it and the readback. Every level
//! pairs in a fixed order, so the sum is reproducible.

use crate::gpu::primitives::dispatch::{WORKGROUP, linear_dispatch};
use crate::gpu::primitives::pipeline::{compute_pipeline, storage_buffer, uniform_buffer};
use crate::gpu::primitives::readback::{CounterReadback, StatsPoll};
use crate::shader_bindings::primitives::reduce::ReduceParams;

#[derive(Debug)]
struct Level {
    groups: (u32, u32),
    bind: wgpu::BindGroup,
}

/// Float sums of `lanes` lanes over a population, folded level by level from the partials a model's leaf shader
/// writes.
#[derive(Debug)]
pub struct GpuLaneReduce {
    lanes: usize,
    /// Workgroup rectangle of the leaf shader over the agents. The leaf shader must dispatch exactly this, since the
    /// group index it writes is `wid.y * groups_x + wid.x`.
    agent_groups: (u32, u32),
    /// The leaf shader's output: one group of `lanes` floats per agent workgroup.
    partials: wgpu::Buffer,
    levels: Vec<Level>,
    pipeline: wgpu::ComputePipeline,
    readback: CounterReadback,
}

/// Number of blocks the leaf dispatches over `num_agents` agents, one partial each.
///
/// Past `MAX_GROUPS_PER_DIM` groups the folded rectangle overshoots the population, and every
/// block it dispatches writes a partial. A surplus block writes zero.
fn leaf_blocks(num_agents: u32) -> u32 {
    let (groups_x, groups_y) = linear_dispatch(num_agents);
    groups_x * groups_y
}

/// Returns the number of groups left after each level, starting from the blocks the leaf dispatches. Always at least
/// one, so the result reaches the readback buffer even when the whole population fits one
/// workgroup.
fn level_sizes(leaf_blocks: u32) -> Vec<u32> {
    let mut sizes = Vec::new();
    let mut groups = leaf_blocks.max(1);
    loop {
        sizes.push(groups);
        if groups == 1 {
            return sizes;
        }
        groups = groups.div_ceil(WORKGROUP);
    }
}

impl GpuLaneReduce {
    /// Builds the levels and the readback for `lanes` lanes over `num_agents` agents.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, lanes: usize, num_agents: u32) -> Self {
        let agent_groups = linear_dispatch(num_agents);
        let sizes = level_sizes(leaf_blocks(num_agents));

        let partials = storage_buffer(device, &format!("{label}_reduce_partials"), sizes[0] as usize * lanes);
        // The last level writes straight into the readback's storage, so nothing extra is copied.
        let intermediates: Vec<wgpu::Buffer> = sizes[1..]
            .iter()
            .enumerate()
            .map(|(k, &n)| storage_buffer(device, &format!("{label}_reduce_level{}", k + 1), n as usize * lanes))
            .collect();
        let readback = CounterReadback::new(device, &format!("{label}_reduce"), lanes);

        let layout = device
            .create_bind_group_layout(&crate::shader_bindings::primitives::reduce::WgpuBindGroup0::LAYOUT_DESCRIPTOR);
        let pipeline = compute_pipeline(
            device,
            &format!("{label}_reduce"),
            crate::shader_bindings::primitives::reduce::SHADER_STRING,
            &layout,
        );

        let input_at = |i: usize| if i == 0 { &partials } else { &intermediates[i - 1] };
        let levels = sizes
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                // One workgroup folds WORKGROUP groups into one, so the domain is the group
                // count. Dispatching `n` instead lets surplus workgroups clamp-write over the
                // output, and the result is a plausible but short sum with no error.
                let groups = linear_dispatch(n);
                let params = uniform_buffer(
                    device,
                    queue,
                    &format!("{label}_reduce_params{i}"),
                    bytemuck::bytes_of(&ReduceParams {
                        n,
                        lanes: lanes as u32,
                        groups_x: groups.0,
                        _pad: 0,
                    }),
                );
                let output = if i + 1 == sizes.len() {
                    readback.binding()
                } else {
                    intermediates[i].as_entire_binding()
                };
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("{label}_reduce_bind{i}")),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: input_at(i).as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: output,
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: params.as_entire_binding(),
                        },
                    ],
                });
                Level { groups, bind }
            })
            .collect();

        Self {
            lanes,
            agent_groups,
            partials,
            levels,
            pipeline,
            readback,
        }
    }

    /// Returns the partials buffer, which the leaf shader binds as `array<f32>` and writes as
    /// `partials[group * lanes + lane]`.
    pub fn partials_binding(&self) -> wgpu::BindingResource<'_> {
        self.partials.as_entire_binding()
    }

    /// Dispatch dimensions of the leaf shader. `groups_x` also goes in its uniform.
    pub fn agent_groups(&self) -> (u32, u32) {
        self.agent_groups
    }

    /// Records the tree and the staging copy. Record the leaf pass before calling.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        for level in &self.levels {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_reduce_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &level.bind, &[]);
            pass.dispatch_workgroups(level.groups.0, level.groups.1, 1);
        }
        // The last level writes every entry unconditionally, and no clear is needed.
        self.readback.encode_copy(encoder);
    }

    /// Starts the async map of the sums. Call it right after submitting the encoder [`Self::encode`] recorded into.
    pub fn begin_readback(&mut self) {
        self.readback.begin_map();
    }

    /// Returns whether a readback started by [`Self::begin_readback`] has not completed yet.
    pub fn readback_pending(&self) -> bool {
        self.readback.is_pending()
    }

    /// Completes an in-flight readback, waiting when `block` is set, and returns its state after the poll.
    pub fn poll_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        if block {
            self.readback.poll_blocking(device)
        } else {
            self.readback.poll(device)
        }
    }

    /// Sum of each lane, all zero until the first readback completes.
    pub fn sums(&self) -> Vec<f32> {
        self.readback.values_f32().collect()
    }

    /// Approximate device memory held by the leaf partials and the final sums, in bytes.
    pub fn heap_bytes(&self) -> usize {
        // The leaf partials dominate. Every level above divides by WORKGROUP.
        self.levels
            .first()
            .map_or(0, |_| self.lanes * std::mem::size_of::<f32>())
            + self.partials.size() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuLaneReduce, WORKGROUP, leaf_blocks, level_sizes, linear_dispatch};
    use crate::gpu::GpuContext;
    use crate::gpu::headless_context;
    use crate::gpu::primitives::pipeline::{
        compute_pipeline, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
    };

    /// Test leaf shader, standing in for the leaf shader of a model.
    const LEAF: &str = r"
struct LeafParams { n: u32, lanes: u32, groups_x: u32, _pad: u32 }

@group(0) @binding(0) var<storage, read> lane_a: array<f32>;
@group(0) @binding(1) var<storage, read> lane_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;
@group(0) @binding(3) var<uniform> params: LeafParams;

const WORKGROUP: u32 = 256u;
var<workgroup> scratch: array<f32, 256>;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let block = wid.y * params.groups_x + wid.x;
    let i = block * WORKGROUP + lid.x;

    for (var lane: u32 = 0u; lane < 2u; lane = lane + 1u) {
        var value: f32 = 0.0;
        if (i < params.n) {
            if (lane == 0u) { value = lane_a[i]; } else { value = lane_b[i]; }
        }
        scratch[lid.x] = value;
        workgroupBarrier();
        for (var stride: u32 = WORKGROUP / 2u; stride > 0u; stride = stride >> 1u) {
            var acc = scratch[lid.x];
            if (lid.x < stride) { acc = acc + scratch[lid.x + stride]; }
            workgroupBarrier();
            scratch[lid.x] = acc;
            workgroupBarrier();
        }
        if (lid.x == 0u) { partials[block * params.lanes + lane] = scratch[0]; }
        workgroupBarrier();
    }
}
";

    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct LeafParams {
        n: u32,
        lanes: u32,
        groups_x: u32,
        _pad: u32,
    }

    /// Returns the sums of lanes `a` and `b`, computed by the leaf shader and the tree on the GPU.
    fn sum_lanes(ctx: &GpuContext, a: &[f32], b: &[f32]) -> Vec<f32> {
        let n = a.len() as u32;
        let mut reduce = GpuLaneReduce::new(&ctx.device, &ctx.queue, "test", 2, n);

        let upload = |label: &str, values: &[f32]| {
            let buffer = storage_buffer(&ctx.device, label, values.len());
            ctx.queue.write_buffer(&buffer, 0, bytemuck::cast_slice(values));
            buffer
        };
        let lane_a = upload("test_lane_a", a);
        let lane_b = upload("test_lane_b", b);

        let layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                storage_entry(0, true),
                storage_entry(1, true),
                storage_entry(2, false),
                uniform_entry(3),
            ],
        });
        let pipeline = compute_pipeline(&ctx.device, "test_leaf", LEAF, &layout);
        let groups = reduce.agent_groups();
        let params = uniform_buffer(
            &ctx.device,
            &ctx.queue,
            "test_leaf_params",
            bytemuck::bytes_of(&LeafParams {
                n,
                lanes: 2,
                groups_x: groups.0,
                _pad: 0,
            }),
        );
        let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: lane_a.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: lane_b.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: reduce.partials_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups.0, groups.1, 1);
        }
        reduce.encode(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        reduce.begin_readback();
        reduce.poll_readback(&ctx.device, true);

        reduce.sums()
    }

    /// The chain always ends at one group.
    #[test]
    fn levels_bottom_out_at_a_single_group() {
        assert_eq!(level_sizes(0), vec![1]);
        assert_eq!(level_sizes(1), vec![1]);
        assert_eq!(level_sizes(2), vec![2, 1]);
        assert_eq!(level_sizes(WORKGROUP), vec![WORKGROUP, 1]);
        assert_eq!(level_sizes(WORKGROUP + 1), vec![WORKGROUP + 1, 2, 1]);
        assert_eq!(level_sizes(3907), vec![3907, 16, 1]);
        assert!(level_sizes(390_625).len() <= 4);
    }

    /// The leaf sizes its partials to every block of a folded dispatch, past 65,535 groups as below.
    #[test]
    fn partials_cover_every_dispatched_block() {
        for n in [1, 256 * 65_535, 256 * 65_535 + 1, 4100 * 4100, 100_000_000] {
            let (x, y) = linear_dispatch(n);
            assert_eq!(leaf_blocks(n), x * y, "{n} invocations");
            assert_eq!(level_sizes(leaf_blocks(n))[0], x * y, "{n} invocations");
            assert!(
                u64::from(x * y) * u64::from(WORKGROUP) >= u64::from(n),
                "{n} invocations"
            );
        }
        // One group past a full row folds into two rows of 65,535 groups.
        assert_eq!(leaf_blocks(256 * 65_535 + 1), 2 * 65_535);
    }

    /// Partials sized from the population would hold 65,536 groups where the folded dispatch writes 131,070, and
    /// drop the blocks past the buffer from the sum.
    #[test]
    fn a_built_reduce_holds_a_partial_for_every_dispatched_block() {
        let Some(ctx) = headless_context("gpu_reduce_partials_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_built_reduce_holds_a_partial_for_every_dispatched_block: no adapter");
            return;
        };
        let lanes = 2;
        let reduce = GpuLaneReduce::new(&ctx.device, &ctx.queue, "test", lanes, 256 * 65_535 + 1);
        let (groups_x, groups_y) = reduce.agent_groups();
        assert_eq!((groups_x, groups_y), (65_535, 2));
        let partial_bytes = u64::from(groups_x * groups_y) * lanes as u64 * std::mem::size_of::<f32>() as u64;
        assert_eq!(reduce.partials.size(), partial_bytes);
    }

    /// Sizes straddle the workgroup width, including a ragged tail and a multi-level chain.
    #[test]
    fn sums_match_a_cpu_reference() {
        let Some(ctx) = headless_context("gpu_reduce_test", wgpu::Features::empty()) else {
            log::warn!("skipping sums_match_a_cpu_reference: no adapter");
            return;
        };

        for n in [1usize, 255, 256, 257, 1_000, 70_000] {
            let a: Vec<f32> = (0..n).map(|i| (i % 17) as f32 * 0.25).collect();
            let b: Vec<f32> = (0..n).map(|i| 1.0 - (i % 5) as f32 * 0.1).collect();
            let expect_a: f64 = a.iter().map(|&v| f64::from(v)).sum();
            let expect_b: f64 = b.iter().map(|&v| f64::from(v)).sum();

            let got = sum_lanes(&ctx, &a, &b);
            let close = |got: f32, want: f64| (f64::from(got) - want).abs() <= 1e-4 * want.abs().max(1.0);
            assert!(close(got[0], expect_a), "n={n} lane a: {} vs {expect_a}", got[0]);
            assert!(close(got[1], expect_b), "n={n} lane b: {} vs {expect_b}", got[1]);
        }
    }

    /// Each lane sums on its own. A stride mistake in the group-major layout would otherwise give plausible numbers.
    #[test]
    fn lanes_stay_separate() {
        let Some(ctx) = headless_context("gpu_reduce_lane_test", wgpu::Features::empty()) else {
            log::warn!("skipping lanes_stay_separate: no adapter");
            return;
        };

        let n = 5_000;
        let a = vec![1.0f32; n];
        let b = vec![0.0f32; n];
        let got = sum_lanes(&ctx, &a, &b);
        assert_eq!(got[0], n as f32, "lane a must total the population");
        assert_eq!(got[1], 0.0, "lane b must stay empty");
    }
}
