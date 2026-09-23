// Infects that fraction of the cells still susceptible, one invocation per cell.
//
// Draws from the action's own seed rather than the per-cell `rng` buffer, so a press leaves the
// run's stream where it was.

#import shared::rng::{pcg_hash, below}
#import shared::space::cell_index

struct Params {
    width: u32,
    height: u32,
    threshold: u32,
    seed: u32,
}

@group(0) @binding(0) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;

const S: u32 = 0u;
const I: u32 = 1u;

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if global_id.x >= params.width || global_id.y >= params.height {
        return;
    }
    let idx = cell_index(global_id.x, global_id.y, params.width);
    if state_out[idx] != S {
        return;
    }
    if below(pcg_hash(params.seed ^ pcg_hash(idx)), params.threshold) {
        state_out[idx] = I;
    }
}
