// Counts alive cells entirely on the GPU, so `SimState::stats()` never has to read the grid back
// to the CPU. Dispatched at the display cadence (~16ms), not every step.
//
// Two-level reduction: every invocation adds its cell into a workgroup-local atomic, then one
// invocation per workgroup does a single atomicAdd into the global total. That is 1 global atomic
// per 256 cells instead of 1 per cell, which is the difference between a negligible pass and a
// contended one at the grid sizes this engine targets.

struct Dims {
    grid: vec2<u32>,
    tex: vec2<u32>,
}

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read_write> total: atomic<u32>;
@group(0) @binding(2) var<uniform> dims: Dims;

var<workgroup> partial: atomic<u32>;

@compute
@workgroup_size(16, 16)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    if (local_index == 0u) {
        atomicStore(&partial, 0u);
    }
    workgroupBarrier();

    // Guarded with an `if` rather than an early `return`: the barriers below must be reached by
    // every invocation in the workgroup, and a partial grid tile would otherwise diverge.
    let width = dims.grid.x;
    let height = dims.grid.y;
    if (global_id.x < width && global_id.y < height) {
        // One invocation per cell, reading the containing word and extracting this cell's bit.
        // Deliberately not a per-word countOneBits, which would need this pass to dispatch over
        // words, and the padding bits in a row's last word would then have to be masked off.
        // This runs at the display cadence, so the simpler form is worth more than the speed.
        let words_per_row = (width + 31u) / 32u;
        let word = state[global_id.y * words_per_row + (global_id.x / 32u)];
        if (((word >> (global_id.x % 32u)) & 1u) == 1u) {
            atomicAdd(&partial, 1u);
        }
    }
    workgroupBarrier();

    if (local_index == 0u) {
        atomicAdd(&total, atomicLoad(&partial));
    }
}
