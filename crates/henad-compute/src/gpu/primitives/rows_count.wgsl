// Counting sort pass 1 for adjacency rows. Tallies the entries each row receives.

#import shared::prelude::linear_index
#import shared::graph::Edge

struct RowsParams {
    num_edges: u32,
    // 1 on a directed graph, where the source end of an edge files into the source's out-row.
    directed: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> edges: array<Edge>;
@group(0) @binding(1) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(2) var<uniform> params: RowsParams;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let e = linear_index(lid, wid, params.groups_x);
    if (e >= params.num_edges) {
        return;
    }

    let edge = edges[e];
    atomicAdd(&counts[2u * edge.dst], 1u);
    atomicAdd(&counts[2u * edge.src + params.directed], 1u);
}
