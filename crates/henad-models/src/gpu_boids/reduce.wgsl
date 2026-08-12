// Leaf of the stat reduction. Folds each workgroup's boids into one group of three sums, which
// `henad_compute::gpu::reduce` then sums.
//
// The `sqrt` is per agent, as in `boids::velocity_sums`. Mean speed rebuilt from mean velocity is
// a different quantity for a turning flock.

struct ReduceParams {
    n: u32,
    lanes: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> vel: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<uniform> params: ReduceParams;

const WORKGROUP: u32 = 256u;

var<workgroup> scratch: array<f32, 256>;

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
        scratch[lid.x] = value;
        workgroupBarrier();

        for (var stride: u32 = WORKGROUP / 2u; stride > 0u; stride = stride >> 1u) {
            var acc = scratch[lid.x];
            if (lid.x < stride) {
                acc = acc + scratch[lid.x + stride];
            }
            workgroupBarrier();
            scratch[lid.x] = acc;
            workgroupBarrier();
        }

        if (lid.x == 0u) {
            partials[block * params.lanes + lane] = scratch[0];
        }
        workgroupBarrier();
    }
}
