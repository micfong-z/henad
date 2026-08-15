
#import shared::dims::Dims
@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: Dims;

// Matches `henad_models::sir::PALETTE`.
const S_COLOR: vec4<f32> = vec4<f32>(0.0 / 255.0, 122.0 / 255.0, 245.0 / 255.0, 1.0);
const I_COLOR: vec4<f32> = vec4<f32>(228.0 / 255.0, 55.0 / 255.0, 72.0 / 255.0, 1.0);
const R_COLOR: vec4<f32> = vec4<f32>(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0);

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if (global_id.x >= dims.tex.x || global_id.y >= dims.tex.y) {
        return;
    }

    // One invocation per texel, which is one cell until the grid outgrows the texture cap.
    let width = dims.grid.x;
    let x = global_id.x * width / dims.tex.x;
    let y = global_id.y * dims.grid.y / dims.tex.y;

    let cell = state[y * width + x];
    var color = S_COLOR;
    if (cell == 1u) {
        color = I_COLOR;
    } else if (cell == 2u) {
        color = R_COLOR;
    }
    textureStore(output, vec2<i32>(global_id.xy), color);
}
