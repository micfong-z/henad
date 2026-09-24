// Counting sort pass 2 for adjacency rows. Each entry claims a slot by bumping its row's write cursor.
//
// The cursor starts as a copy of the scanned offsets, so a row's entries land contiguously. The slot within the row is
// whatever order the atomics resolve in.

#import shared::prelude::linear_index
#import shared::graph::Edge

struct RowsParams {
    num_edges: u32,
    directed: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> edges: array<Edge>;
@group(0) @binding(1) var<storage, read_write> cursor: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> entries: array<u32>;
@group(0) @binding(3) var<uniform> params: RowsParams;

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
    entries[atomicAdd(&cursor[2u * edge.dst], 1u)] = edge.src;
    entries[atomicAdd(&cursor[2u * edge.src + params.directed], 1u)] = edge.dst;
}
