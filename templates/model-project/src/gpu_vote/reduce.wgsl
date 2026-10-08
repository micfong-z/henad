// Counts the cells set to 1. Each workgroup sums into a local counter, then adds it to the global counter once.

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

    // The bounds check is an `if` instead of an early `return`, since every invocation has to reach both barriers.
    if (global_id.x < dims.grid.x && global_id.y < dims.grid.y) {
        atomicAdd(&partial, state[global_id.y * dims.grid.x + global_id.x]);
    }
    workgroupBarrier();

    if (local_index == 0u) {
        atomicAdd(&counters, atomicLoad(&partial));
    }
}
