//! Adjacency rows of a graph, rebuilt on the GPU from its edge list by a counting sort.
//!
//! Node `i` owns two rows in one table of `2n + 1` offsets: its in-row `entries[row_start[2i]..row_start[2i + 1]]` and
//! its out-row `entries[row_start[2i + 1]..row_start[2i + 2]]`. An undirected graph files every neighbour in the in-row
//! and leaves the out-row empty, as [`henad_core::network::Network`] keeps both directions in its in-rows.
//!
//! Not stable, unlike the CPU rows. Membership matches, but a row comes out in whatever order the atomics resolve in.

use crate::gpu::capacity::storage_in_layout;
use crate::gpu::primitives::dispatch::linear_dispatch;
use crate::gpu::primitives::pipeline::{bind_group, compute_pipeline, storage_buffer, uniform_buffer};
use crate::gpu::primitives::prefix_scan::PrefixScan;
use crate::shader_bindings::primitives::rows_count::RowsParams;
use crate::shader_bindings::shared::graph::Edge;

/// Number of `u32` words in one edge of the edge list.
pub const EDGE_WORDS: usize = 3;

const _: () = assert!(
    size_of::<Edge>() == EDGE_WORDS * size_of::<u32>(),
    "Edge must match the edge layer's instance stride"
);

pub struct GpuRows {
    num_nodes: u32,
    num_edges: u32,
    /// Workgroup rectangle over the edges, shared by the count and scatter passes.
    edge_groups: (u32, u32),

    counts: wgpu::Buffer,
    row_start: wgpu::Buffer,
    cursor: wgpu::Buffer,
    entries: wgpu::Buffer,

    scan: PrefixScan,
    count_pipeline: wgpu::ComputePipeline,
    count_bind: wgpu::BindGroup,
    scatter_pipeline: wgpu::ComputePipeline,
    scatter_bind: wgpu::BindGroup,
}

impl GpuRows {
    /// Creates the rows for the first `num_edges` edges of `edges`, an `array<Edge>` the caller owns.
    ///
    /// Nothing is built until [`Self::encode_build`] is recorded.
    ///
    /// # Panics
    ///
    /// If `edges` is shorter than `num_edges` edges, or than one edge, which a binding of `array<Edge>` needs at least.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        edges: &wgpu::Buffer,
        num_nodes: u32,
        num_edges: u32,
        directed: bool,
    ) -> Self {
        assert!(
            edges.size() >= (num_edges.max(1) as usize * EDGE_WORDS * size_of::<u32>()) as u64,
            "the edge list holds fewer than {num_edges} edges"
        );
        let table_len = 2 * num_nodes as usize + 1;
        let edge_groups = linear_dispatch(num_edges);

        let counts = storage_buffer(device, &format!("{label}_rows_counts"), table_len);
        let row_start = storage_buffer(device, &format!("{label}_rows_row_start"), table_len);
        let cursor = storage_buffer(device, &format!("{label}_rows_cursor"), table_len);
        let entries = storage_buffer(device, &format!("{label}_rows_entries"), 2 * num_edges as usize);

        let params = uniform_buffer(
            device,
            queue,
            &format!("{label}_rows_params"),
            bytemuck::bytes_of(&RowsParams {
                num_edges,
                directed: u32::from(directed),
                groups_x: edge_groups.0,
                _pad: 0,
            }),
        );

        let scan = PrefixScan::new(
            device,
            queue,
            &format!("{label}_rows"),
            &counts,
            &row_start,
            table_len as u32,
        );

        let count_layout = device.create_bind_group_layout(
            &crate::shader_bindings::primitives::rows_count::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let count_pipeline = compute_pipeline(
            device,
            &format!("{label}_rows_count"),
            crate::shader_bindings::primitives::rows_count::SHADER_STRING,
            &count_layout,
        );
        let count_bind = bind_group(
            device,
            &format!("{label}_rows_count_bind"),
            &count_layout,
            &[
                edges.as_entire_binding(),
                counts.as_entire_binding(),
                params.as_entire_binding(),
            ],
        );

        let scatter_layout = device.create_bind_group_layout(
            &crate::shader_bindings::primitives::rows_scatter::WgpuBindGroup0::LAYOUT_DESCRIPTOR,
        );
        let scatter_pipeline = compute_pipeline(
            device,
            &format!("{label}_rows_scatter"),
            crate::shader_bindings::primitives::rows_scatter::SHADER_STRING,
            &scatter_layout,
        );
        let scatter_bind = bind_group(
            device,
            &format!("{label}_rows_scatter_bind"),
            &scatter_layout,
            &[
                edges.as_entire_binding(),
                cursor.as_entire_binding(),
                entries.as_entire_binding(),
                params.as_entire_binding(),
            ],
        );

        Self {
            num_nodes,
            num_edges,
            edge_groups,
            counts,
            row_start,
            cursor,
            entries,
            scan,
            count_pipeline,
            count_bind,
            scatter_pipeline,
            scatter_bind,
        }
    }

    /// Records a full rebuild from the current edge list.
    ///
    /// Each stage reads what the last one wrote, so each gets its own pass. Note that no pass takes a timestamp, since
    /// a caller's step does not open with the rebuild.
    pub fn encode_build(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.counts, 0, None);

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_rows_count_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.count_pipeline);
            pass.set_bind_group(0, &self.count_bind, &[]);
            pass.dispatch_workgroups(self.edge_groups.0, self.edge_groups.1, 1);
        }

        self.scan.encode(encoder);

        // The scatter bumps the cursor, so it works on a copy and `row_start` survives.
        encoder.copy_buffer_to_buffer(&self.row_start, 0, &self.cursor, 0, self.table_bytes());

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("henad_rows_scatter_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.scatter_pipeline);
            pass.set_bind_group(0, &self.scatter_bind, &[]);
            pass.dispatch_workgroups(self.edge_groups.0, self.edge_groups.1, 1);
        }
    }

    /// Storage buffers each of the rebuild's passes binds, labelled as in [`crate::gpu::capacity::Demand`].
    pub fn storage_bindings() -> [(&'static str, u32); 2] {
        [
            (
                "rows_count",
                storage_in_layout(&crate::shader_bindings::primitives::rows_count::WgpuBindGroup0::LAYOUT_DESCRIPTOR),
            ),
            (
                "rows_scatter",
                storage_in_layout(&crate::shader_bindings::primitives::rows_scatter::WgpuBindGroup0::LAYOUT_DESCRIPTOR),
            ),
        ]
    }

    /// Bind as `array<u32>` of `2n + 1` offsets.
    pub fn row_start_binding(&self) -> wgpu::BindingResource<'_> {
        self.row_start.as_entire_binding()
    }

    /// Bind as `array<u32>` of neighbour ids, grouped by row.
    pub fn entries_binding(&self) -> wgpu::BindingResource<'_> {
        self.entries.as_entire_binding()
    }

    pub fn row_start_buffer(&self) -> &wgpu::Buffer {
        &self.row_start
    }

    pub fn entries_buffer(&self) -> &wgpu::Buffer {
        &self.entries
    }

    fn table_bytes(&self) -> u64 {
        (2 * u64::from(self.num_nodes) + 1) * size_of::<u32>() as u64
    }

    pub fn heap_bytes(&self) -> usize {
        // Three tables plus the entries. The scan's intermediates are a rounding error.
        3 * self.table_bytes() as usize + 2 * self.num_edges as usize * size_of::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::{EDGE_WORDS, GpuRows};
    use crate::gpu::GpuContext;
    use crate::gpu::headless_context;
    use crate::gpu::primitives::pipeline::storage_buffer;
    use crate::gpu::primitives::readback::read_words;
    use henad_core::authoring::primitives::rng::next_index;
    use henad_core::network::Network;

    /// Joins random pairs until the graph has `edges` edges, plus a hub linked to a tenth of the nodes.
    fn graph(nodes: u32, edges: usize, directed: bool) -> Network {
        let mut network = Network::new(nodes as usize, directed);
        let mut rng = 0x7A0B_5EED_0000_0001;
        for dest in (1..nodes).step_by(10) {
            network.add_edge(0, dest, 0);
        }
        while network.edge_count() < edges {
            let (a, b) = (next_index(&mut rng, nodes), next_index(&mut rng, nodes));
            if a != b {
                network.add_edge(a, b, 0);
            }
        }
        network
    }

    /// Builds the rows of `network` on the GPU and reads back `(row_start, entries)`.
    fn build(ctx: &GpuContext, network: &Network) -> (Vec<u32>, Vec<u32>) {
        let (src, dst, _) = network.edges();
        let words: Vec<u32> = src.iter().zip(dst).flat_map(|(&a, &b)| [a, b, 0]).collect();
        let edges = storage_buffer(&ctx.device, "test_edges", words.len().max(EDGE_WORDS));
        ctx.queue.write_buffer(&edges, 0, bytemuck::cast_slice(&words));

        let nodes = network.slot_count() as u32;
        let rows = GpuRows::new(
            &ctx.device,
            &ctx.queue,
            "test",
            &edges,
            nodes,
            src.len() as u32,
            network.directed(),
        );
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        rows.encode_build(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));

        let table = 2 * nodes as usize + 1;
        (
            read_words(&ctx.device, &ctx.queue, rows.row_start_buffer())[..table].to_vec(),
            read_words(&ctx.device, &ctx.queue, rows.entries_buffer())[..2 * src.len()].to_vec(),
        )
    }

    fn sorted(row: &[u32]) -> Vec<u32> {
        let mut row = row.to_vec();
        row.sort_unstable();
        row
    }

    /// Compares every row with the CPU graph's, as multisets, since the atomic scatter fixes no order within a row.
    fn assert_rows_match(network: &Network, row_start: &[u32], entries: &[u32]) {
        let nodes = network.slot_count();
        assert_eq!(
            row_start[2 * nodes] as usize,
            2 * network.edge_count(),
            "the last offset must count both ends of every edge"
        );
        for i in 0..nodes {
            let row = |k: usize| &entries[row_start[k] as usize..row_start[k + 1] as usize];
            let (in_row, out_row) = (row(2 * i), row(2 * i + 1));
            assert_eq!(
                sorted(in_row),
                sorted(network.in_neighbors(i as u32)),
                "in-row of node {i}"
            );
            let expected_out = if network.directed() {
                sorted(network.out_neighbors(i as u32))
            } else {
                Vec::new()
            };
            assert_eq!(sorted(out_row), expected_out, "out-row of node {i}");
        }
    }

    #[test]
    fn rows_match_the_cpu_graph() {
        let Some(ctx) = headless_context("gpu_rows_test", wgpu::Features::empty()) else {
            log::warn!("skipping rows_match_the_cpu_graph: no adapter");
            return;
        };
        for directed in [false, true] {
            let network = graph(2_000, 6_000, directed);
            let (row_start, entries) = build(&ctx, &network);
            assert_rows_match(&network, &row_start, &entries);
        }
    }

    /// A `2n + 1` table past 256² entries needs three scan levels.
    #[test]
    fn a_large_graph_still_files_every_entry() {
        let Some(ctx) = headless_context("gpu_rows_large_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_large_graph_still_files_every_entry: no adapter");
            return;
        };
        for directed in [false, true] {
            let network = graph(50_000, 150_000, directed);
            let (row_start, entries) = build(&ctx, &network);
            assert_rows_match(&network, &row_start, &entries);
        }
    }

    #[test]
    fn a_graph_without_edges_has_empty_rows() {
        let Some(ctx) = headless_context("gpu_rows_empty_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_graph_without_edges_has_empty_rows: no adapter");
            return;
        };
        let network = Network::new(5, false);
        let (row_start, _) = build(&ctx, &network);
        assert_eq!(row_start, vec![0; 11]);
    }
}
