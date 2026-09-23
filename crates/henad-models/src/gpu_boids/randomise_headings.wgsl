// Turns every boid a fresh way without touching its speed, one invocation per boid.

#import shared::prelude::linear_index
#import shared::rng::{pcg_hash, random_float}
#import shared::space::heading_octant

const TAU: f32 = 6.2831855;

struct Params {
    num_agents: u32,
    groups_x: u32,
    seed: u32,
    // Speed for a boid that is not moving, which has no heading to keep.
    stationary: f32,
    // Heading colours, in the uniform to keep a storage binding free. Indexed as
    // `palette[o >> 2u][o & 3u]`.
    palette: array<vec4<u32>, 2>,
}

@group(0) @binding(0) var<storage, read_write> vel_out: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> color_out: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x);
    if i >= params.num_agents {
        return;
    }

    let v = vel_out[i];
    var speed = length(v);
    if speed <= 0.0 {
        speed = params.stationary;
    }

    let angle = random_float(pcg_hash(params.seed ^ pcg_hash(i)), TAU);
    let turned = vec2<f32>(cos(angle) * speed, sin(angle) * speed);
    vel_out[i] = turned;

    let o = heading_octant(turned.x, turned.y);
    color_out[i] = params.palette[o >> 2u][o & 3u];
}
