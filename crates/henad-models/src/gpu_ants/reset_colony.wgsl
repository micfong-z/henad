// Wipes both trails and puts the ants back on the nest, holding a reward.
//
// One invocation covers a cell of each field layer and, where the index is below the ant count,
// an ant. The domain is the larger of the cell count and the ant count, so neither half is left
// short.

#import henad::dispatch::linear_index
#import gpu_ants::state::{HAS_REWARD_BIT}

struct Params {
    n: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    nest: vec2<f32>,
    // Colour of a searching ant, as every ant is after the reset.
    color: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read_write> pos: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
@group(0) @binding(2) var<storage, read_write> color: array<u32>;
@group(0) @binding(3) var<storage, read_write> field: array<f32>;
@group(0) @binding(4) var<storage, read_write> accum: array<u32>;
@group(0) @binding(5) var<uniform> params: Params;

// Matches `ants::lanes::NO_STEP`, the `last_step` of an ant that has not stepped yet.
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

    // Each invocation clears both layers, since the domain counts cells once.
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
