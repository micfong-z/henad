// Quantises the field into the display texture, mirroring `PheromoneField::quantise`.

struct Params {
    width: u32,
    height: u32,
    n_cells: u32,
    _pad: u32,
    // `ants::field::CELL_PALETTE`, packed so the colours cannot drift from the CPU model's.
    palette: array<vec4<u32>, 4>,
}

@group(0) @binding(0) var<storage, read> field: array<f32>;
@group(0) @binding(1) var<storage, read> sites: array<u32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> params: Params;

const OBSTACLE: u32 = 1u;
const FOOD: u32 = 2u;
const HOME: u32 = 3u;

const LOW_PHEROMONE: f32 = 1e-14;
const DISPLAY_DECADES: f32 = 3.0;
const RAMP_STEPS: f32 = 6.0;
const INV_LOG2_10: f32 = 0.30103;

fn ramp_step(v: f32) -> u32 {
    if (v <= LOW_PHEROMONE) {
        return 0u;
    }
    let decades = log2(v) * INV_LOG2_10 / DISPLAY_DECADES + 1.0;
    if (decades <= 0.0) {
        return 0u;
    }
    return clamp(u32(clamp(decades * RAMP_STEPS, 0.0, RAMP_STEPS)), 1u, u32(RAMP_STEPS));
}

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if (global_id.x >= params.width || global_id.y >= params.height) {
        return;
    }

    let c = global_id.y * params.width + global_id.x;
    let site = sites[c];

    var index = 0u;
    if (site == OBSTACLE) {
        index = 13u;
    } else if (site == FOOD) {
        index = 14u;
    } else if (site == HOME) {
        index = 15u;
    } else {
        // Stronger route wins the cell, so overlapping trails stay legible.
        let to_food = field[c];
        let to_home = field[params.n_cells + c];
        var v = to_home;
        var base = 0u;
        if (to_food > to_home) {
            v = to_food;
            base = 6u;
        }
        let step = ramp_step(v);
        if (step != 0u) {
            index = base + step;
        }
    }

    let colour = unpack4x8unorm(params.palette[index >> 2u][index & 3u]);
    textureStore(output, vec2<i32>(global_id.xy), colour);
}
