// Greys every edge that touches a resistant node, as `virus_network::edge_color` does.

#import shared::prelude::linear_index
#import shared::graph::Edge
#import gpu_virus_network::node_state::RESISTANT

struct EdgeColorParams {
    num_edges: u32,
    groups_x: u32,
    // Packed RGBA of an open edge and of a blocked one.
    open: u32,
    blocked: u32,
}

@group(0) @binding(0) var<storage, read>       state: array<u32>;
@group(0) @binding(1) var<storage, read_write> edges: array<Edge>;
@group(0) @binding(2) var<uniform>             params: EdgeColorParams;

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
    let blocked = state[edge.src] == RESISTANT || state[edge.dst] == RESISTANT;
    edges[e].color = select(params.open, params.blocked, blocked);
}
