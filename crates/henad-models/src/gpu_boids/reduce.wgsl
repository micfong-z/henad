// Leaf of the stat reduction. One workgroup folds its slice of the population down to one value
// per lane, and `GpuLaneReduce` owns every level above this.

#import shared::prelude::{WORKGROUP, linear_index}
#import shared::reduce_tree::block_sum

struct Params {
    n: u32,
    lanes: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> vel: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let block = wid.y * params.groups_x + wid.x;
    let i = block * WORKGROUP + lid.x;

    for (var lane: u32 = 0u; lane < params.lanes; lane = lane + 1u) {
        var value: f32 = 0.0;
        // The `sqrt` is per agent, as in `boids::velocity_sums`. Mean speed rebuilt from mean
        // velocity is a different quantity for a turning flock.
        if (i < params.n) {
            let v = vel[i];
            if (lane == 0u) {
                value = length(v);
            } else if (lane == 1u) {
                value = v.x;
            } else {
                value = v.y;
            }
        }

        let total = block_sum(lid.x, value);
        if (lid.x == 0u) {
            partials[block * params.lanes + lane] = total;
        }
    }
}
