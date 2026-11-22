//! GPU twin of [`henad_core::spatial_hash::SpatialHash`], rebuilt every tick with the same layout. A kernel walks
//! cell `c` as `sorted[cell_start[c]..cell_start[c + 1]]`.
//!
//! Note that the sort is not stable, unlike the CPU sort. Membership is the same, but a cell's slice comes out in
//! whatever order the atomics resolve in, so a kernel summing floats over a slice will not replay.

use henad_core::authoring::model::field::Extent;
pub use henad_core::spatial_hash::HashGrid;

use crate::gpu::primitives::dispatch::{WORKGROUP, linear_dispatch};
use crate::gpu::primitives::pipeline::{compute_pipeline, dispatch_pass, storage_buffer, uniform_buffer};
use crate::gpu::primitives::prefix_scan::PrefixScan;
use crate::gpu::primitives::readback::{CounterReadback, StatsPoll};
use crate::shader_bindings::primitives::hash_count::HashParams;

/// Most workgroups of the largest-bucket pass, one axis of a dispatch. Each invocation strides over the cells past it.
const LARGEST_GROUPS: u32 = 65_535;

/// Counting-sort spatial hash of a GPU population, with the tables, scan and pipelines its rebuild uses.
#[derive(Debug)]
pub struct GpuSpatialHash {
    grid: HashGrid,
    num_agents: u32,
    /// Workgroup rectangle over the agent domain, shared by the count and scatter passes.
    agent_groups: (u32, u32),

    /// Number of agents in each cell, with one extra trailing entry so the scan's last value is the population total
    /// and `cell_start[c + 1]` stays in bounds for the final cell.
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
    /// Builds the tables and pipelines of a hash over `extent`, with cells of `cell_size`, for `num_agents` agents.
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
        let count_layout = device.create_bind_group_layout(
            &crate::shader_bindings::primitives::hash_count::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let count_pipeline = compute_pipeline(
            device,
            &format!("{label}_hash_count"),
            crate::shader_bindings::primitives::hash_count::SHADER_STRING,
            &count_layout,
        );

        let scatter_layout = device.create_bind_group_layout(
            &crate::shader_bindings::primitives::hash_scatter::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
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
            crate::shader_bindings::primitives::hash_scatter::SHADER_STRING,
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
    /// `pos` is a single `array<vec2<f32>>` lane, since two lanes would cost another storage binding. Build one bind
    /// group per side up front, so stepping never rebuilds a bind group.
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

    /// Returns the cell-start table, bound as `array<u32>`. Cell `c` owns `cell_start[c]..cell_start[c + 1]` of
    /// [`Self::sorted_binding`].
    pub fn cell_start_binding(&self) -> wgpu::BindingResource<'_> {
        self.cell_start.as_entire_binding()
    }

    /// Returns the agent ids grouped by cell, bound as `array<u32>`.
    pub fn sorted_binding(&self) -> wgpu::BindingResource<'_> {
        self.sorted.as_entire_binding()
    }

    /// Records a full rebuild, one pass per stage. Each stage reads what the last one wrote, and wgpu synchronises
    /// only between passes.
    ///
    /// `begin_stamp` marks the start of a batch. It has to go here rather than on an empty pass. A stamp on an empty
    /// pass is not reliably written, and reads back as a zero `start`.
    pub fn encode_build(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        count_bind: &wgpu::BindGroup,
        begin_stamp: Option<(&wgpu::QuerySet, u32)>,
    ) {
        encoder.clear_buffer(&self.counts, 0, None);

        let stamp = begin_stamp.map(|(query_set, index)| wgpu::ComputePassTimestampWrites {
            query_set,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: None,
        });
        dispatch_pass(
            encoder,
            "henad_hash_count_pass",
            &self.count_pipeline,
            count_bind,
            self.agent_groups,
            stamp,
        );

        self.scan.encode(encoder);

        // The scatter bumps the cursor, so it works on a copy and `cell_start` survives.
        encoder.copy_buffer_to_buffer(&self.cell_start, 0, &self.cursor, 0, self.table_bytes());

        dispatch_pass(
            encoder,
            "henad_hash_scatter_pass",
            &self.scatter_pipeline,
            &self.scatter_bind,
            self.agent_groups,
            None,
        );
    }

    fn table_bytes(&self) -> u64 {
        u64::from(self.grid.num_cells() + 1) * std::mem::size_of::<u32>() as u64
    }

    /// Approximate device memory held by the three tables and the two agent arrays, in bytes.
    pub fn heap_bytes(&self) -> usize {
        let table = (self.grid.num_cells() as usize + 1) * std::mem::size_of::<u32>();
        let agent = self.num_agents as usize * std::mem::size_of::<u32>();
        // Three tables plus two agent arrays. The scan's intermediates are a rounding error.
        3 * table + 2 * agent
    }

    /// Returns the measurement of the largest bucket of this hash, with its pipeline built on `device`, each object
    /// labelled from `label`.
    pub fn largest_bucket(&self, device: &wgpu::Device, label: &str) -> LargestBucket {
        let readback = CounterReadback::new(device, &format!("{label}_hash_largest"), 1);
        let layout = device.create_bind_group_layout(
            &crate::shader_bindings::primitives::hash_largest::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{label}_hash_largest_bind")),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.counts.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: readback.binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.params.as_entire_binding(),
                },
            ],
        });
        let pipeline = compute_pipeline(
            device,
            &format!("{label}_hash_largest"),
            crate::shader_bindings::primitives::hash_largest::SHADER_STRING,
            &layout,
        );
        LargestBucket {
            pipeline,
            bind,
            groups: self.grid.num_cells().div_ceil(WORKGROUP).clamp(1, LARGEST_GROUPS),
            readback,
            consumed: 0,
        }
    }
}

/// Measurement of the largest bucket of a [`GpuSpatialHash`], the most agents one cell of its latest build holds: a
/// pass that writes it into one word, and the word's readback.
///
/// Each measurement is tagged with the tick of the step whose build it reads, and [`Self::poll`] returns one only once.
#[derive(Debug)]
pub struct LargestBucket {
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    /// Workgroups of the pass, one row.
    groups: u32,
    readback: CounterReadback,
    /// Tag of the latest measurement [`Self::poll`] returned, 0 before the first.
    consumed: u64,
}

impl LargestBucket {
    /// Records the measurement of the hash's latest build, tagged with `tick`, and the copy of its word for readback,
    /// and returns whether it recorded them.
    ///
    /// Nothing is recorded while the readback of an earlier measurement is in flight, since its copy would be
    /// skipped. Record it after the build, in the same encoder or a later one.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, tick: u64) -> bool {
        if self.readback.is_pending() {
            return false;
        }
        self.readback.encode_clear(encoder);
        dispatch_pass(
            encoder,
            "henad_hash_largest_pass",
            &self.pipeline,
            &self.bind,
            (self.groups, 1),
            None,
        );
        self.readback.encode_copy_tagged(encoder, tick);
        true
    }

    /// Starts the readback of the measurement [`Self::encode`] recorded. Call it once that encoder is submitted.
    pub fn begin_readback(&mut self) {
        self.readback.begin_map();
    }

    /// Returns whether a readback has begun and not finished.
    pub fn is_pending(&self) -> bool {
        self.readback.is_pending()
    }

    /// Polls the readback, waiting for the device when `block` holds and the target can wait, and returns the tick and
    /// the largest bucket of a measurement that landed since the last call.
    ///
    /// A measurement tagged with tick 0, before any build, is never returned.
    pub fn poll(&mut self, device: &wgpu::Device, block: bool) -> Option<(u64, u32)> {
        if self.readback.poll_or_block(device, block) != StatsPoll::Landed {
            return None;
        }
        let tick = self.readback.values_tag();
        if tick <= self.consumed {
            return None;
        }
        self.consumed = tick;
        self.readback.values().first().map(|&largest| (tick, largest))
    }
}

/// Raw buffers, for debugging and tests.
impl GpuSpatialHash {
    /// Buffer behind [`Self::cell_start_binding`].
    pub fn cell_start_buffer(&self) -> &wgpu::Buffer {
        &self.cell_start
    }

    /// Buffer behind [`Self::sorted_binding`].
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
    use crate::gpu::primitives::readback::read_words;
    use henad_core::authoring::model::field::Extent;
    use henad_core::authoring::primitives::rng::xorshift64;
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

    /// Returns `n` deterministic positions in a `w` by `h` world, so a failure is reproducible.
    fn positions(n: usize, w: f32, h: f32) -> (Vec<f32>, Vec<f32>) {
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        let mut unit = || {
            seed = xorshift64(seed);
            (seed >> 40) as f32 / 16_777_216.0
        };
        (0..n).map(|_| (unit() * w, unit() * h)).unzip()
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
            read_words(&ctx.device, &ctx.queue, hash.cell_start_buffer())[..cells].to_vec(),
            read_words(&ctx.device, &ctx.queue, hash.sorted_buffer())[..n as usize].to_vec(),
        )
    }

    /// Each cell holds the agents the CPU hash puts there, compared as sets since the atomic scatter does not fix the
    /// order within a cell.
    #[test]
    fn buckets_match_the_cpu_hash() {
        let Some(ctx) = headless_context("gpu_spatial_hash_test") else {
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

    /// Builds the hash for one set of positions and returns the largest bucket the device measures, the build tagged
    /// with tick 1.
    fn measured_largest(ctx: &crate::gpu::GpuContext, pos_x: &[f32], pos_y: &[f32], ext: Extent, cell: f32) -> u32 {
        let hash = GpuSpatialHash::new(&ctx.device, &ctx.queue, "test", ext, cell, pos_x.len() as u32);
        let interleaved: Vec<f32> = pos_x.iter().zip(pos_y).flat_map(|(&x, &y)| [x, y]).collect();
        let pos = storage_buffer(&ctx.device, "test_pos", interleaved.len());
        ctx.queue.write_buffer(&pos, 0, bytemuck::cast_slice(&interleaved));
        let bind = hash.bind_positions(&ctx.device, "test_count_bind", &pos);
        let mut largest = hash.largest_bucket(&ctx.device, "test");

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        hash.encode_build(&mut encoder, &bind, None);
        assert!(largest.encode(&mut encoder, 1), "nothing reads back yet");
        ctx.queue.submit(Some(encoder.finish()));
        largest.begin_readback();
        let (tick, value) = largest.poll(&ctx.device, true).expect("the measurement lands");
        assert_eq!(tick, 1);
        assert_eq!(largest.poll(&ctx.device, true), None, "a measurement is returned once");
        value
    }

    /// The largest bucket the device measures is the most agents any cell of the CPU hash holds, and every agent
    /// when all of them stand at one point.
    #[test]
    fn the_largest_bucket_matches_the_cpu_hash() {
        let Some(ctx) = headless_context("gpu_spatial_hash_largest_test") else {
            return;
        };
        let (w, h, cell, n) = (1_000.0f32, 730.0f32, 47.0f32, 5_000usize);
        let (pos_x, pos_y) = positions(n, w, h);
        let mut cpu = SpatialHash::new(cell, w, h);
        cpu.build(&pos_x, &pos_y);
        let (cpu_start, _) = cpu.buckets();
        let cpu_largest = cpu_start
            .windows(2)
            .map(|bounds| bounds[1] - bounds[0])
            .max()
            .expect("a cell");
        assert_eq!(measured_largest(&ctx, &pos_x, &pos_y, extent(w, h), cell), cpu_largest);

        let clustered = vec![160.5f32; n];
        assert_eq!(
            measured_largest(&ctx, &clustered, &clustered, extent(w, h), cell),
            n as u32,
            "every agent in one cell"
        );
    }

    /// Exercises the 2D dispatch fold and a multi-level scan.
    #[test]
    fn a_large_population_still_sorts_every_agent() {
        let Some(ctx) = headless_context("gpu_spatial_hash_large_test") else {
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
