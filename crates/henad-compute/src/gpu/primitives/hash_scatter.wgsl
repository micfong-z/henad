// Counting sort pass 2. Each agent claims a slot by bumping its cell's write cursor.
//
// The cursor starts as a copy of the scanned offsets, so a cell's agents land contiguously. Which
// slot within the cell is whatever order the atomics resolve in.

#import shared::prelude::WORKGROUP

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

@group(0) @binding(0) var<storage, read> agent_cell: array<u32>;
@group(0) @binding(1) var<storage, read_write> cursor: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> sorted: array<u32>;
@group(0) @binding(3) var<uniform> params: HashParams;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = (wid.y * params.groups_x + wid.x) * WORKGROUP + lid.x;
    if (i >= params.num_agents) {
        return;
    }

    let slot = atomicAdd(&cursor[agent_cell[i]], 1u);
    sorted[slot] = i;
}
