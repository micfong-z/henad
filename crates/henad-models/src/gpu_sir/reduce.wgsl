// Counts S/I/R cells on the GPU.

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read_write> totals: array<atomic<u32>, 3>;
@group(0) @binding(2) var<uniform> dims: vec2<u32>;

var<workgroup> partial: array<atomic<u32>, 3>;

@compute
@workgroup_size(16, 16)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    if (local_index == 0u) {
        atomicStore(&partial[0], 0u);
        atomicStore(&partial[1], 0u);
        atomicStore(&partial[2], 0u);
    }
    workgroupBarrier();

    let width = dims.x;
    let height = dims.y;
    if (global_id.x < width && global_id.y < height) {
        let cell = state[global_id.y * width + global_id.x];
        atomicAdd(&partial[cell], 1u);
    }
    workgroupBarrier();

    if (local_index == 0u) {
        atomicAdd(&totals[0], atomicLoad(&partial[0]));
        atomicAdd(&totals[1], atomicLoad(&partial[1]));
        atomicAdd(&totals[2], atomicLoad(&partial[2]));
    }
}
