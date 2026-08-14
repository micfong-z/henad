// Counting sort pass 1. Bins every agent and tallies how many landed in each cell.

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

// One `vec2` lane rather than two, to save a storage binding.
@group(0) @binding(0) var<storage, read> pos: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> agent_cell: array<u32>;
@group(0) @binding(3) var<uniform> params: HashParams;

const WORKGROUP: u32 = 256u;

// WGSL's `%` follows the sign of the dividend, so a position left of the origin would index
// backwards out of the grid.
fn wrap(v: i32, m: i32) -> i32 {
    return ((v % m) + m) % m;
}

fn cell_index(x: f32, y: f32) -> u32 {
    let cx = wrap(i32(floor(x * params.cell_w_inv)), i32(params.grid_w));
    let cy = wrap(i32(floor(y * params.cell_h_inv)), i32(params.grid_h));
    return u32(cy) * params.grid_w + u32(cx);
}

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

    let p = pos[i];
    let cell = cell_index(p.x, p.y);
    agent_cell[i] = cell;
    atomicAdd(&counts[cell], 1u);
}
