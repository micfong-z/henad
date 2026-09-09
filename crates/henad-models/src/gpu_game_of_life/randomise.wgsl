// Refills the grid at the density slider, one invocation per packed word.
//
// Draws per cell as `GameOfLifeModel::init` does, but from `pcg_hash` over the cell index rather
// than a walked xorshift64 stream. An action is no part of the tick-0 oracle, so the two backends
// need not land on the same grid here.

#import shared::rng::{pcg_hash, below}

struct Params {
    width: u32,
    height: u32,
    threshold: u32,
    seed: u32,
}

@group(0) @binding(0) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = params.width;
    let stride = (width + 31u) / 32u;
    let word = global_id.x;
    let y = global_id.y;
    if word >= stride || y >= params.height {
        return;
    }

    // Trailing bits of a ragged last word hold no cell and must stay zero, so the loop stops at
    // the real ones rather than filling all 32.
    let first = word * 32u;
    let cells_here = min(width - first, 32u);
    var bits = 0u;
    for (var j = 0u; j < cells_here; j = j + 1u) {
        // Its own word per cell. Two cells off one draw would correlate.
        let draw = pcg_hash(params.seed ^ pcg_hash(y * width + first + j));
        if below(draw, params.threshold) {
            bits = bits | (1u << j);
        }
    }
    state_out[y * stride + word] = bits;
}
