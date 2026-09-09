// Empties the grid, one invocation per packed word.

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
    let stride = (params.width + 31u) / 32u;
    let word = global_id.x;
    let y = global_id.y;
    if word >= stride || y >= params.height {
        return;
    }
    state_out[y * stride + word] = 0u;
}
