// The voting rule, one invocation per cell. A cell takes the majority of itself and its eight neighbours.

#import henad::space::{cell_index, offset_cell, TORUS}

struct Params {
    width: u32,
    height: u32,
}

@group(0) @binding(0) var<storage, read> state_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = params.width;
    let height = params.height;
    if (global_id.x >= width || global_id.y >= height) {
        return;
    }

    // The 3x3 block, the cell itself included.
    var votes = 0u;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let neighbor = offset_cell(global_id.x, global_id.y, dx, dy, width, height, TORUS);
            votes += state_in[cell_index(u32(neighbor.x), u32(neighbor.y), width)];
        }
    }
    state_out[cell_index(global_id.x, global_id.y, width)] = select(0u, 1u, votes >= 5u);
}
