// Paints one texel per invocation, from the cell it samples.

#import henad::dims::{Dims, cell_at}

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: Dims;

// The two colours of `vote::PALETTE`.
const ZERO_COLOR: vec4<f32> = vec4<f32>(26.0 / 255.0, 26.0 / 255.0, 46.0 / 255.0, 1.0);
const ONE_COLOR: vec4<f32> = vec4<f32>(242.0 / 255.0, 166.0 / 255.0, 59.0 / 255.0, 1.0);

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if (global_id.x >= dims.tex.x || global_id.y >= dims.tex.y) {
        return;
    }
    let cell = cell_at(global_id.xy, dims);
    let value = state[cell.y * dims.grid.x + cell.x];
    textureStore(output, vec2<i32>(global_id.xy), select(ZERO_COLOR, ONE_COLOR, value == 1u));
}
