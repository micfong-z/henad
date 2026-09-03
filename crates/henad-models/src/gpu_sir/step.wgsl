
#import shared::rng::{pcg_hash, random_float}
#import shared::space::{cell_index, offset_cell, TORUS}
struct Params {
    width: u32,
    height: u32,
    infection_rate: f32,
    recovery_rate: f32,
}

// --8<-- [start:bindings]
@group(0) @binding(0) var<storage, read> state_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(2) var<storage, read> rng_in: array<u32>;
@group(0) @binding(3) var<storage, read_write> rng_out: array<u32>;
@group(0) @binding(4) var<uniform> params: Params;
// --8<-- [end:bindings]

const S: u32 = 0u;
const I: u32 = 1u;
const R: u32 = 2u;

// Single-round integer hash (Jarzynski/O'Neill "pcg_hash"), not a full 64-bit PCG32 since WGSL has no
// u64. Storing each cell's hash state in its own ping-ponged buffer (rather than deriving it from
// a `tick` uniform) is what makes a batch of N steps safe to encode into one command buffer and
// submit once: any uniform written between passes in the same encoder would only become visible
// after the whole encoder submits, so every pass would see the same tick. Advancing the state
// in-buffer sidesteps that entirely.

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = params.width;
    let height = params.height;
    if (global_id.x >= width || global_id.y >= height) {
        return;
    }

    let x = global_id.x;
    let y = global_id.y;
    let idx = cell_index(x, y, width);

    let left = u32(offset_cell(x, y, -1, 0, width, height, TORUS).x);
    let right = u32(offset_cell(x, y, 1, 0, width, height, TORUS).x);
    let up = u32(offset_cell(x, y, 0, -1, width, height, TORUS).y);
    let down = u32(offset_cell(x, y, 0, 1, width, height, TORUS).y);

    var infected_neighbors: u32 = 0u;
    infected_neighbors += u32(state_in[cell_index(left, up, width)] == I);
    infected_neighbors += u32(state_in[cell_index(x, up, width)] == I);
    infected_neighbors += u32(state_in[cell_index(right, up, width)] == I);
    infected_neighbors += u32(state_in[cell_index(left, y, width)] == I);
    infected_neighbors += u32(state_in[cell_index(right, y, width)] == I);
    infected_neighbors += u32(state_in[cell_index(left, down, width)] == I);
    infected_neighbors += u32(state_in[cell_index(x, down, width)] == I);
    infected_neighbors += u32(state_in[cell_index(right, down, width)] == I);

    let rng_next = pcg_hash(rng_in[idx]);
    rng_out[idx] = rng_next;

    let cell = state_in[idx];
    var next_cell = cell;
    if (cell == S && infected_neighbors > 0u) {
        let prob_safe = pow(1.0 - params.infection_rate, f32(infected_neighbors));
        // `>=`, matching the CPU model. A zero draw would otherwise escape a rate of 1.
        if (random_float(rng_next, 1.0) >= prob_safe) {
            next_cell = I;
        }
    } else if (cell == I) {
        if (random_float(rng_next, 1.0) < params.recovery_rate) {
            next_cell = R;
        }
    }

    state_out[idx] = next_cell;
}
