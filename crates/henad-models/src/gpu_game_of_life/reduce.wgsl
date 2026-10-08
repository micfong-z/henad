// Counts alive cells entirely on the GPU, so `SimState::stats()` never has to read the grid back
// to the CPU. The pass runs only when the stats are sampled.
//
// The reduction has two levels. Every invocation adds its cell into a workgroup-local atomic, then
// one invocation per workgroup adds that total into the global counter, once per 256 cells.

#import henad::dims::Dims

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read_write> counters: atomic<u32>;
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

    // The bounds check wraps the work in an `if`. Every invocation in the workgroup has to reach
    // both barriers, and an early `return` in a partial grid tile would skip them.
    let width = dims.grid.x;
    let height = dims.grid.y;
    if (global_id.x < width && global_id.y < height) {
        // Each invocation reads the word holding its cell and extracts the cell's bit. A per-word
        // countOneBits would have to dispatch over words and mask off the padding bits of each
        // row's last word, and this pass runs only when the stats are sampled.
        let words_per_row = (width + 31u) / 32u;
        let word = state[global_id.y * words_per_row + (global_id.x / 32u)];
        if (((word >> (global_id.x % 32u)) & 1u) == 1u) {
            atomicAdd(&partial, 1u);
        }
    }
    workgroupBarrier();

    if (local_index == 0u) {
        atomicAdd(&counters, atomicLoad(&partial));
    }
}
