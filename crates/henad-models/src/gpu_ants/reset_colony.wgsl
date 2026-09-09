// Wipes both trails and puts the ants back on the nest, holding a reward.
//
// One invocation covers a cell of each field layer and, where the index reaches, an ant. The
// domain is the longer of the two, so neither half is left short.

#import shared::prelude::linear_index
#import gpu_ants::state::{HAS_REWARD_BIT}

struct Params {
    n: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    nest: vec2<f32>,
    // Searching, which every ant is again after this.
    color: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read_write> pos: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
@group(0) @binding(2) var<storage, read_write> color: array<u32>;
@group(0) @binding(3) var<storage, read_write> field: array<f32>;
@group(0) @binding(4) var<storage, read_write> accum: array<u32>;
@group(0) @binding(5) var<uniform> params: Params;

// Matches `ants::lanes::NO_STEP`. No step taken yet, so momentum has nothing to continue.
const NO_STEP: u32 = 255u;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x);
    if i >= params.n {
        return;
    }

    // Both layers per invocation, since the domain counts cells once.
    if i < params.n_cells {
        field[i] = 0.0;
        field[i + params.n_cells] = 0.0;
        accum[i] = 0u;
        accum[i + params.n_cells] = 0u;
    }

    if i < params.num_agents {
        pos[i] = params.nest;
        state[i] = NO_STEP | HAS_REWARD_BIT;
        color[i] = params.color;
    }
}
