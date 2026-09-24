// Paints each node from the palette and counts the nodes in each state.

#import shared::prelude::linear_index

struct NodeColorParams {
    num_nodes: u32,
    groups_x: u32,
    _pad0: u32,
    _pad1: u32,
    // Packed RGBA, indexed by state.
    palette: vec4<u32>,
}

@group(0) @binding(0) var<storage, read>       state: array<u32>;
@group(0) @binding(1) var<storage, read_write> color: array<u32>;
@group(0) @binding(2) var<storage, read_write> counters: array<atomic<u32>, 3>;
@group(0) @binding(3) var<uniform>             params: NodeColorParams;

var<workgroup> partial: array<atomic<u32>, 3>;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    if (lid.x == 0u) {
        atomicStore(&partial[0], 0u);
        atomicStore(&partial[1], 0u);
        atomicStore(&partial[2], 0u);
    }
    workgroupBarrier();

    // No early return, since every invocation has to reach both barriers.
    let i = linear_index(lid, wid, params.groups_x);
    if (i < params.num_nodes) {
        let s = state[i];
        color[i] = params.palette[s];
        atomicAdd(&partial[s], 1u);
    }
    workgroupBarrier();

    if (lid.x == 0u) {
        atomicAdd(&counters[0], atomicLoad(&partial[0]));
        atomicAdd(&counters[1], atomicLoad(&partial[1]));
        atomicAdd(&counters[2], atomicLoad(&partial[2]));
    }
}
