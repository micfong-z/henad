struct Params {
    width: u32,
    height: u32,
    infection_rate: f32,
    recovery_rate: f32,
}

@group(0) @binding(0) var<storage, read> state_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(2) var<storage, read> rng_in: array<u32>;
@group(0) @binding(3) var<storage, read_write> rng_out: array<u32>;
@group(0) @binding(4) var<uniform> params: Params;

const S: u32 = 0u;
const I: u32 = 1u;
const R: u32 = 2u;

// Single-round integer hash (Jarzynski/O'Neill "pcg_hash"), not a full 64-bit PCG32 since WGSL has no
// u64. Storing each cell's hash state in its own ping-ponged buffer (rather than deriving it from
// a `tick` uniform) is what makes a batch of N steps safe to encode into one command buffer and
// submit once: any uniform written between passes in the same encoder would only become visible
// after the whole encoder submits, so every pass would see the same tick. Advancing the state
// in-buffer sidesteps that entirely.
fn pcg_hash(input: u32) -> u32 {
    var state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand01(h: u32) -> f32 {
    return f32(h) / 4294967295.0;
}

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
    let idx = y * width + x;

    let left = (x + width - 1u) % width;
    let right = (x + 1u) % width;
    let up = (y + height - 1u) % height;
    let down = (y + 1u) % height;

    var infected_neighbors: u32 = 0u;
    infected_neighbors += u32(state_in[up * width + left] == I);
    infected_neighbors += u32(state_in[up * width + x] == I);
    infected_neighbors += u32(state_in[up * width + right] == I);
    infected_neighbors += u32(state_in[y * width + left] == I);
    infected_neighbors += u32(state_in[y * width + right] == I);
    infected_neighbors += u32(state_in[down * width + left] == I);
    infected_neighbors += u32(state_in[down * width + x] == I);
    infected_neighbors += u32(state_in[down * width + right] == I);

    let rng_next = pcg_hash(rng_in[idx]);
    rng_out[idx] = rng_next;

    let cell = state_in[idx];
    var next_cell = cell;
    if (cell == S && infected_neighbors > 0u) {
        let prob_safe = pow(1.0 - params.infection_rate, f32(infected_neighbors));
        if (rand01(rng_next) > prob_safe) {
            next_cell = I;
        }
    } else if (cell == I) {
        if (rand01(rng_next) < params.recovery_rate) {
            next_cell = R;
        }
    }

    state_out[idx] = next_cell;
}
