@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: vec2<u32>;

// Palette matches `henad_models::game_of_life::PALETTE`: dead = 0x15/0x15/0x15, alive = 0x00/0xE6/0x76.
const DEAD_COLOUR: vec4<f32> = vec4<f32>(21.0 / 255.0, 21.0 / 255.0, 21.0 / 255.0, 1.0);
const ALIVE_COLOUR: vec4<f32> = vec4<f32>(0.0 / 255.0, 230.0 / 255.0, 118.0 / 255.0, 1.0);

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = dims.x;
    let height = dims.y;
    if (global_id.x >= width || global_id.y >= height) {
        return;
    }

    let cell = state[global_id.y * width + global_id.x];
    let colour = select(DEAD_COLOUR, ALIVE_COLOUR, cell == 1u);
    textureStore(output, vec2<i32>(global_id.xy), colour);
}
