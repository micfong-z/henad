//! GPU twin of [`henad_core::spatial_hash::SpatialHash`], rebuilt every tick. Same layout, so a
//! kernel walks cell `c` as `sorted[cell_start[c]..cell_start[c + 1]]`.
//!
//! Not stable, unlike the CPU sort. Membership is the same, but a cell's slice comes out in
//! whatever order the atomics resolve in, so a kernel summing floats over one will not replay.

use henad_core::authoring::field::Extent;
pub use henad_core::spatial_hash::HashGrid;

use crate::gpu::primitives::dispatch::linear_dispatch;
use crate::gpu::primitives::pipeline::{
    compute_pipeline, storage_buffer, storage_entry, uniform_buffer, uniform_entry,
};
use crate::gpu::primitives::prefix_scan::PrefixScan;

/// Matches `HashParams` in `hash_count.wgsl` and `hash_scatter.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct HashParams {
    grid_w: u32,
    grid_h: u32,
    num_agents: u32,
    groups_x: u32,
    cell_w_inv: f32,
    cell_h_inv: f32,
    _pad0: u32,
    _pad1: u32,
}

pub struct GpuSpatialHash {
    grid: HashGrid,
    num_agents: u32,
    /// Workgroup rectangle over the agent domain, shared by the count and scatter passes.
    agent_groups: (u32, u32),

    /// One extra trailing entry, so the scan's last value is the population total and
    /// `cell_start[c + 1]` stays in bounds for the final cell.
    counts: wgpu::Buffer,
    cell_start: wgpu::Buffer,
    cursor: wgpu::Buffer,
    agent_cell: wgpu::Buffer,
    sorted: wgpu::Buffer,
    params: wgpu::Buffer,

    scan: PrefixScan,
    count_layout: wgpu::BindGroupLayout,
    count_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    scatter_bind: wgpu::BindGroup,
}

impl GpuSpatialHash {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        extent: Extent,
        cell_size: f32,
        num_agents: u32,
    ) -> Self {
        let grid = HashGrid::new(extent, cell_size);
        let table_len = grid.num_cells() as usize + 1;
        let agent_groups = linear_dispatch(num_agents);

        let counts = storage_buffer(device, &format!("{label}_hash_counts"), table_len);
        let cell_start = storage_buffer(device, &format!("{label}_hash_cell_start"), table_len);
        let cursor = storage_buffer(device, &format!("{label}_hash_cursor"), table_len);
        let agent_cell = storage_buffer(device, &format!("{label}_hash_agent_cell"), num_agents as usize);
        let sorted = storage_buffer(device, &format!("{label}_hash_sorted"), num_agents as usize);

        let params = uniform_buffer(
            device,
            queue,
            &format!("{label}_hash_params"),
            bytemuck::bytes_of(&HashParams {
                grid_w: grid.grid_w,
                grid_h: grid.grid_h,
                num_agents,
                groups_x: agent_groups.0,
                cell_w_inv: 1.0 / grid.cell_w,
                cell_h_inv: 1.0 / grid.cell_h,
                _pad0: 0,
                _pad1: 0,
            }),
        );

        let scan = PrefixScan::new(
            device,
            queue,
            &format!("{label}_hash"),
            &counts,
            &cell_start,
            table_len as u32,
        );

        // Positions are the model's own ping-ponged buffers, so `bind_positions` fills this in.
        let count_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_hash_count_layout")),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, false),
                uniform_entry(3),
            ],
        });
        let count_pipeline = compute_pipeline(
            device,
            &format!("{label}_hash_count"),
            include_str!("hash_count.wgsl"),
            &count_layout,
        );

        let scatter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_hash_scatter_layout")),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, false),
                uniform_entry(3),
            ],
        });
        let scatter_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{label}_hash_scatter_bind")),
            layout: &scatter_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: agent_cell.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cursor.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: sorted.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let scatter_pipeline = compute_pipeline(
            device,
            &format!("{label}_hash_scatter"),
            include_str!("hash_scatter.wgsl"),
            &scatter_layout,
        );

        Self {
            grid,
            num_agents,
            agent_groups,
            counts,
            cell_start,
            cursor,
            agent_cell,
            sorted,
            params,
            scan,
            count_layout,
            count_pipeline,
            scatter_pipeline,
            scatter_bind,
        }
    }

    /// Binds one side of a model's ping-ponged position lane to the counting pass.
    ///
    /// `pos` is `array<vec2<f32>>`, one lane not two, to save a storage binding. Build one per
    /// side up front so stepping never rebuilds a bind group.
    pub fn bind_positions(&self, device: &wgpu::Device, label: &str, pos: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.count_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pos.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.counts.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.agent_cell.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params.as_entire_binding(),
                },
            ],
        })
    }

    /// Cell geometry, for a model to mirror into its step uniform.
    pub fn grid(&self) -> HashGrid {
        self.grid
    }

    /// Bind as `array<u32>`. Cell `c` owns `cell_start[c]..cell_start[c + 1]` of
    /// [`Self::sorted_binding`].
    pub fn cell_start_binding(&self) -> wgpu::BindingResource<'_> {
        self.cell_start.as_entire_binding()
    }

    /// Bind as `array<u32>`. Agent ids grouped by cell.
    pub fn sorted_binding(&self) -> wgpu::BindingResource<'_> {
        self.sorted.as_entire_binding()
    }

    /// Records a full rebuild. Each stage reads what the last one wrote, so each gets its own
    /// pass, since wgpu only synchronises between passes.
    ///
    /// `begin_stamp` marks the start of a batch. It has to go here rather than on an empty pass,
    /// which does not reliably get written and reads back as a zero `start`.
    pub fn encode_build(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        count_bind: &wgpu::BindGroup,
        begin_stamp: Option<(&wgpu::QuerySet, u32)>,
    ) {
        encoder.clear_buffer(&self.counts, 0, None);

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_hash_count_pass"),
                timestamp_writes: begin_stamp.map(|(query_set, index)| wgpu::ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(index),
                    end_of_pass_write_index: None,
                }),
            });
            pass.set_pipeline(&self.count_pipeline);
            pass.set_bind_group(0, count_bind, &[]);
            pass.dispatch_workgroups(self.agent_groups.0, self.agent_groups.1, 1);
        }

        self.scan.encode(encoder);

        // The scatter bumps the cursor, so it works on a copy and `cell_start` survives.
        encoder.copy_buffer_to_buffer(&self.cell_start, 0, &self.cursor, 0, self.table_bytes());

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_hash_scatter_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.scatter_pipeline);
            pass.set_bind_group(0, &self.scatter_bind, &[]);
            pass.dispatch_workgroups(self.agent_groups.0, self.agent_groups.1, 1);
        }
    }

    fn table_bytes(&self) -> u64 {
        u64::from(self.grid.num_cells() + 1) * std::mem::size_of::<u32>() as u64
    }

    pub fn heap_bytes(&self) -> usize {
        let table = (self.grid.num_cells() as usize + 1) * std::mem::size_of::<u32>();
        let agent = self.num_agents as usize * std::mem::size_of::<u32>();
        // Three tables plus two agent arrays. The scan's intermediates are a rounding error.
        3 * table + 2 * agent
    }
}

/// Readable for debugging and tests.
impl GpuSpatialHash {
    pub fn cell_start_buffer(&self) -> &wgpu::Buffer {
        &self.cell_start
    }

    pub fn sorted_buffer(&self) -> &wgpu::Buffer {
        &self.sorted
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuSpatialHash, HashGrid};
    use crate::gpu::headless_context;
    use crate::gpu::primitives::dispatch::WORKGROUP;
    use crate::gpu::primitives::pipeline::storage_buffer;
    use henad_core::authoring::field::Extent;
    use henad_core::helpers::xorshift64;
    use henad_core::spatial_hash::SpatialHash;

    fn extent(w: f32, h: f32) -> Extent {
        Extent { w, h }
    }

    /// The two grids must agree cell for cell, or the queries walk different worlds.
    #[test]
    fn grid_geometry_matches_the_cpu_hash() {
        for (w, h, cell) in [
            (1_000.0, 1_000.0, 50.0),
            (1_000.0, 730.0, 47.0),
            (300.0, 300.0, 100.0),
            (100.0, 100.0, 200.0),
        ] {
            let gpu = HashGrid::new(extent(w, h), cell);
            let cpu = SpatialHash::new(cell, w, h);
            let (cpu_w, cpu_h) = cpu.grid_dims();
            assert_eq!(
                (gpu.grid_w, gpu.grid_h),
                (cpu_w, cpu_h),
                "grid dims for {w}x{h} @ {cell}"
            );
            let (cpu_cw, cpu_ch) = cpu.cell_extents();
            assert_eq!(
                (gpu.cell_w, gpu.cell_h),
                (cpu_cw, cpu_ch),
                "cell extents for {w}x{h} @ {cell}"
            );
        }
    }

    /// Deterministic, so a failure is reproducible.
    fn positions(n: usize, w: f32, h: f32) -> (Vec<f32>, Vec<f32>) {
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        let mut unit = || {
            seed = xorshift64(seed);
            (seed >> 40) as f32 / 16_777_216.0
        };
        (0..n).map(|_| (unit() * w, unit() * h)).unzip()
    }

    /// Reads a `u32` storage buffer back through a staging copy.
    fn read_u32(ctx: &crate::gpu::GpuContext, buffer: &wgpu::Buffer, len: usize) -> Vec<u32> {
        let size = (len * std::mem::size_of::<u32>()) as u64;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hash_test_staging"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        ctx.queue.submit(Some(encoder.finish()));

        let (tx, rx) = flume::bounded(1);
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll");
        rx.recv().expect("map channel").expect("map");

        let data = staging.slice(..).get_mapped_range().expect("map range");
        let out = bytemuck::cast_slice::<u8, u32>(&data)[..len].to_vec();
        drop(data);
        staging.unmap();
        out
    }

    /// Builds the hash for one set of positions and reads back `(cell_start, sorted)`.
    fn build(
        ctx: &crate::gpu::GpuContext,
        pos_x: &[f32],
        pos_y: &[f32],
        ext: Extent,
        cell: f32,
    ) -> (Vec<u32>, Vec<u32>) {
        let n = pos_x.len() as u32;
        let hash = GpuSpatialHash::new(&ctx.device, &ctx.queue, "test", ext, cell, n);

        // The lane is `array<vec2<f32>>`, so interleave the two test vectors on upload.
        let interleaved: Vec<f32> = pos_x.iter().zip(pos_y).flat_map(|(&x, &y)| [x, y]).collect();
        let pos = storage_buffer(&ctx.device, "test_pos", interleaved.len());
        ctx.queue.write_buffer(&pos, 0, bytemuck::cast_slice(&interleaved));

        let bind = hash.bind_positions(&ctx.device, "test_count_bind", &pos);

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        hash.encode_build(&mut encoder, &bind, None);
        ctx.queue.submit(Some(encoder.finish()));

        let cells = hash.grid().num_cells() as usize + 1;
        (
            read_u32(ctx, hash.cell_start_buffer(), cells),
            read_u32(ctx, hash.sorted_buffer(), n as usize),
        )
    }

    /// Compared as sets, since the atomic scatter does not fix the order within a cell.
    #[test]
    fn buckets_match_the_cpu_hash() {
        let Some(ctx) = headless_context("gpu_spatial_hash_test", wgpu::Features::empty()) else {
            log::warn!("skipping buckets_match_the_cpu_hash: no adapter");
            return;
        };

        let (w, h, cell, n) = (1_000.0f32, 730.0f32, 47.0f32, 5_000usize);
        let (pos_x, pos_y) = positions(n, w, h);
        let (cell_start, sorted) = build(&ctx, &pos_x, &pos_y, extent(w, h), cell);

        let mut cpu = SpatialHash::new(cell, w, h);
        cpu.build(&pos_x, &pos_y);
        let (cpu_start, cpu_sorted) = cpu.buckets();

        assert_eq!(
            cell_start, cpu_start,
            "cell offsets must match the CPU counting sort exactly"
        );
        assert_eq!(
            *cell_start.last().expect("offset table"),
            n as u32,
            "the trailing offset must be the population"
        );

        for c in 0..cell_start.len() - 1 {
            let range = cell_start[c] as usize..cell_start[c + 1] as usize;
            let mut gpu_cell = sorted[range.clone()].to_vec();
            let mut cpu_cell = cpu_sorted[range].to_vec();
            gpu_cell.sort_unstable();
            cpu_cell.sort_unstable();
            assert_eq!(gpu_cell, cpu_cell, "cell {c} holds a different set of agents");
        }
    }

    /// Exercises the 2D dispatch fold and a multi-level scan.
    #[test]
    fn a_large_population_still_sorts_every_agent() {
        let Some(ctx) = headless_context("gpu_spatial_hash_large_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_large_population_still_sorts_every_agent: no adapter");
            return;
        };

        // 4000 world units at cell size 5 gives 640_000 cells, so the scan needs three levels.
        let (w, h, cell, n) = (4_000.0f32, 4_000.0f32, 5.0f32, 200_000usize);
        let (pos_x, pos_y) = positions(n, w, h);
        let (cell_start, sorted) = build(&ctx, &pos_x, &pos_y, extent(w, h), cell);

        assert!(
            cell_start.len() > (WORKGROUP * WORKGROUP) as usize,
            "this test is meant to need more than two scan levels"
        );
        assert_eq!(
            *cell_start.last().expect("offset table"),
            n as u32,
            "the scan lost agents somewhere in the level chain"
        );

        let mut seen = sorted.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            n,
            "every agent must appear in the sorted array exactly once"
        );
    }
}
