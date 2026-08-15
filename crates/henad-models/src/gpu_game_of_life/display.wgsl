
#import shared::dims::Dims
@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: Dims;

// Palette matches `henad_models::game_of_life::PALETTE`: dead = 0x15/0x15/0x15, alive = 0x00/0xE6/0x76.
const DEAD_COLOR: vec4<f32> = vec4<f32>(21.0 / 255.0, 21.0 / 255.0, 21.0 / 255.0, 1.0);
const ALIVE_COLOR: vec4<f32> = vec4<f32>(0.0 / 255.0, 230.0 / 255.0, 118.0 / 255.0, 1.0);

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

    // Read the containing word and extract this cell's bit, unlike the per-word step pass.
    let words_per_row = (width + 31u) / 32u;
    let word = state[y * words_per_row + (x / 32u)];
    let cell = (word >> (x % 32u)) & 1u;
    let color = select(DEAD_COLOR, ALIVE_COLOR, cell == 1u);
    textureStore(output, vec2<i32>(global_id.xy), color);
}
