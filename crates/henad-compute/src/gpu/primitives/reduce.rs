//! Multi-level float sum over an agent population, for stats an `atomic<u32>` cannot hold.
//!
//! The model supplies the leaf, this owns the levels above it and the readback. Fixed pairwise
//! order throughout, so the sum is reproducible.

use crate::gpu::primitives::dispatch::{WORKGROUP, linear_dispatch};
use crate::gpu::primitives::pipeline::{
    compute_pipeline, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
};
use crate::gpu::primitives::readback::CounterReadback;
use crate::shader_bindings::primitives::reduce::ReduceParams;

struct Level {
    groups: (u32, u32),
    bind: wgpu::BindGroup,
}

pub struct GpuLaneReduce {
    lanes: usize,
    /// The leaf shader must dispatch exactly this, since the group index it writes is
    /// `wid.y * groups_x + wid.x`.
    agent_groups: (u32, u32),
    /// The leaf shader's output: one group of `lanes` floats per agent workgroup.
    partials: wgpu::Buffer,
    levels: Vec<Level>,
    pipeline: wgpu::ComputePipeline,
    readback: CounterReadback,
}

/// Groups left after each level. Always at least one, so the result reaches the readback buffer
/// even when the whole population fits one workgroup.
fn level_sizes(num_agents: u32) -> Vec<u32> {
    let mut sizes = Vec::new();
    let mut groups = num_agents.div_ceil(WORKGROUP).max(1);
    loop {
        sizes.push(groups);
        if groups == 1 {
            return sizes;
        }
        groups = groups.div_ceil(WORKGROUP);
    }
}

impl GpuLaneReduce {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, lanes: usize, num_agents: u32) -> Self {
        let sizes = level_sizes(num_agents);
        let agent_groups = linear_dispatch(num_agents);

        let partials = storage_buffer(device, &format!("{label}_reduce_partials"), sizes[0] as usize * lanes);
        // The last level writes straight into the readback's storage, so nothing extra is copied.
        let intermediates: Vec<wgpu::Buffer> = sizes[1..]
            .iter()
            .enumerate()
            .map(|(k, &n)| storage_buffer(device, &format!("{label}_reduce_level{}", k + 1), n as usize * lanes))
            .collect();
        let readback = CounterReadback::new(device, &format!("{label}_reduce"), lanes);

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_reduce_layout")),
            entries: &[storage_entry(0, true), storage_entry(1, false), uniform_entry(2)],
        });
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
                // output, which reads as a plausible but short sum rather than a crash.
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

    /// Bind as `array<f32>` in the leaf shader, written as `partials[group * lanes + lane]`.
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
        // No clear needed, the last level writes every entry unconditionally.
        self.readback.encode_copy(encoder);
    }

    pub fn begin_readback(&mut self) {
        self.readback.begin_map();
    }

    pub fn poll_readback(&mut self, device: &wgpu::Device, block: bool) {
        if block {
            self.readback.poll_blocking(device);
        } else {
            self.readback.poll(device);
        }
    }

    /// One per lane. All zero until the first readback completes.
    pub fn sums(&self) -> Vec<f32> {
        self.readback.values_f32().collect()
    }

    pub fn heap_bytes(&self) -> usize {
        // The leaf partials dominate, every level above divides by WORKGROUP.
        self.levels
            .first()
            .map_or(0, |_| self.lanes * std::mem::size_of::<f32>())
            + self.partials.size() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuLaneReduce, WORKGROUP, level_sizes};
    use crate::gpu::GpuContext;
    use crate::gpu::headless_context;
    use crate::gpu::primitives::pipeline::{
        compute_pipeline, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
    };

    /// Stands in for a model's leaf shader.
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

    /// Leaf plus tree over two lanes.
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
        assert_eq!(level_sizes(WORKGROUP), vec![1]);
        assert_eq!(level_sizes(WORKGROUP + 1), vec![2, 1]);
        assert_eq!(level_sizes(1_000_000), vec![3907, 16, 1]);
        assert!(level_sizes(100_000_000).len() <= 4);
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

    /// A stride mistake in the group-major layout would otherwise give plausible numbers.
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
