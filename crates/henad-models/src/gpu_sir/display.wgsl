@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: vec2<u32>;

// Matches `henad_models::sir::PALETTE`.
const S_COLOUR: vec4<f32> = vec4<f32>(0.0 / 255.0, 122.0 / 255.0, 245.0 / 255.0, 1.0);
const I_COLOUR: vec4<f32> = vec4<f32>(228.0 / 255.0, 55.0 / 255.0, 72.0 / 255.0, 1.0);
const R_COLOUR: vec4<f32> = vec4<f32>(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0);

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = dims.x;
    let height = dims.y;
    if (global_id.x >= width || global_id.y >= height) {
        return;
    }

    let cell = state[global_id.y * width + global_id.x];
    var colour = S_COLOUR;
    if (cell == 1u) {
        colour = I_COLOUR;
    } else if (cell == 2u) {
        colour = R_COLOUR;
    }
    textureStore(output, vec2<i32>(global_id.xy), colour);
}
