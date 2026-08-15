//! Multi-level exclusive prefix sum, standing in for the counting sort's serial running total.
//!
//! Each workgroup scans [`WORKGROUP`] elements, the level above scans their totals, and the
//! results are added back down the chain. Levels are built once at construction.

use crate::gpu::primitives::dispatch::{WORKGROUP, linear_dispatch};
use crate::gpu::primitives::pipeline::{
    compute_pipeline, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
};

/// Matches `ScanParams` in `scan.wgsl` and `scan_add.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ScanParams {
    n: u32,
    groups_x: u32,
    num_blocks: u32,
    _pad: u32,
}

/// Its bind groups keep the buffers it touches alive.
struct Level {
    groups: (u32, u32),
    scan_bind: wgpu::BindGroup,
    /// `None` at the top, whose single block already holds the whole scan.
    add_bind: Option<wgpu::BindGroup>,
}

pub struct PrefixScan {
    levels: Vec<Level>,
    scan_pipeline: wgpu::ComputePipeline,
    add_pipeline: wgpu::ComputePipeline,
}

/// The caller's `n`, then one entry per workgroup, until one workgroup covers the level.
fn level_sizes(n: u32) -> Vec<u32> {
    let mut sizes = Vec::new();
    let mut level = n.max(1);
    loop {
        sizes.push(level);
        let blocks = level.div_ceil(WORKGROUP);
        if blocks <= 1 {
            return sizes;
        }
        level = blocks;
    }
}

/// Level independent, so built once.
fn build_pipelines(
    device: &wgpu::Device,
    label: &str,
) -> (
    wgpu::BindGroupLayout,
    wgpu::BindGroupLayout,
    wgpu::ComputePipeline,
    wgpu::ComputePipeline,
) {
    let scan_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{label}_scan_layout")),
        entries: &[
            storage_entry(0, true),
            storage_entry(1, false),
            storage_entry(2, false),
            uniform_entry(3),
        ],
    });
    let add_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{label}_scan_add_layout")),
        entries: &[storage_entry(0, true), storage_entry(1, false), uniform_entry(2)],
    });
    let scan_pipeline = compute_pipeline(
        device,
        &format!("{label}_scan"),
        crate::shader_bindings::primitives::scan::SHADER_STRING,
        &scan_layout,
    );
    let add_pipeline = compute_pipeline(
        device,
        &format!("{label}_scan_add"),
        crate::shader_bindings::primitives::scan_add::SHADER_STRING,
        &add_layout,
    );
    (scan_layout, add_layout, scan_pipeline, add_pipeline)
}

impl PrefixScan {
    /// Scans `n` elements of `input` into `output`, both caller owned. Intermediates are ours.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        input: &wgpu::Buffer,
        output: &wgpu::Buffer,
        n: u32,
    ) -> Self {
        let (scan_layout, add_layout, scan_pipeline, add_pipeline) = build_pipelines(device, label);

        let sizes = level_sizes(n);

        // Level 0 writes the caller's output, the rest need their own. Built before the bind
        // groups, since a level's add pass reads the next level's output.
        let upper_outputs: Vec<wgpu::Buffer> = sizes[1..]
            .iter()
            .enumerate()
            .map(|(k, &m)| storage_buffer(device, &format!("{label}_scan_out{}", k + 1), m as usize))
            .collect();
        let sums: Vec<wgpu::Buffer> = sizes
            .iter()
            .enumerate()
            .map(|(i, &m)| storage_buffer(device, &format!("{label}_scan_sums{i}"), m.div_ceil(WORKGROUP) as usize))
            .collect();

        let output_at = |i: usize| if i == 0 { output } else { &upper_outputs[i - 1] };
        let input_at = |i: usize| if i == 0 { input } else { &sums[i - 1] };

        let levels = sizes
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                let groups = linear_dispatch(m);
                let params = uniform_buffer(
                    device,
                    queue,
                    &format!("{label}_scan_params{i}"),
                    bytemuck::bytes_of(&ScanParams {
                        n: m,
                        groups_x: groups.0,
                        num_blocks: m.div_ceil(WORKGROUP),
                        _pad: 0,
                    }),
                );

                let scan_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("{label}_scan_bind{i}")),
                    layout: &scan_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: input_at(i).as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: output_at(i).as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: sums[i].as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: params.as_entire_binding(),
                        },
                    ],
                });

                let add_bind = (i + 1 < sizes.len()).then(|| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("{label}_scan_add_bind{i}")),
                        layout: &add_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: output_at(i + 1).as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: output_at(i).as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: params.as_entire_binding(),
                            },
                        ],
                    })
                });

                Level {
                    groups,
                    scan_bind,
                    add_bind,
                }
            })
            .collect();

        Self {
            levels,
            scan_pipeline,
            add_pipeline,
        }
    }

    /// Records the whole scan. Each pass depends on the last, and wgpu only synchronises between
    /// passes, not within one.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for level in &self.levels {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_scan_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.scan_pipeline);
            pass.set_bind_group(0, &level.scan_bind, &[]);
            pass.dispatch_workgroups(level.groups.0, level.groups.1, 1);
        }

        // Top down, since a level can only be lifted once the one above it is correct.
        for level in self.levels.iter().rev() {
            let Some(add_bind) = &level.add_bind else {
                continue;
            };
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_scan_add_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.add_pipeline);
            pass.set_bind_group(0, add_bind, &[]);
            pass.dispatch_workgroups(level.groups.0, level.groups.1, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WORKGROUP, level_sizes};

    /// Never an empty chain.
    #[test]
    fn levels_bottom_out_at_a_single_workgroup() {
        assert_eq!(level_sizes(0), vec![1]);
        assert_eq!(level_sizes(1), vec![1]);
        assert_eq!(level_sizes(WORKGROUP), vec![WORKGROUP]);
        assert_eq!(level_sizes(WORKGROUP + 1), vec![WORKGROUP + 1, 2]);
        assert_eq!(level_sizes(1_000), vec![1_000, 4]);

        for n in [100_000u32, 10_000_000, 100_000_000] {
            let sizes = level_sizes(n);
            assert_eq!(sizes[0], n);
            assert_eq!(
                *sizes.last().expect("at least one level"),
                sizes.last().copied().unwrap_or(1).min(WORKGROUP),
                "the top level must fit one workgroup"
            );
            assert!(sizes.len() <= 4, "{n} elements should not need {} levels", sizes.len());
        }
    }
}
