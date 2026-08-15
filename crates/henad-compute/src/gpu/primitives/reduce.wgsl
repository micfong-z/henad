// One level of a multi-level float sum. Each workgroup folds WORKGROUP groups down to one.
//
// Group-major (`input[group * lanes + lane]`), so a model's leaf shader writes one contiguous
// group per workgroup and needs to know nothing about the tree above it.

#import shared::prelude::WORKGROUP

struct ReduceParams {
    n: u32,
    lanes: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: ReduceParams;

var<workgroup> scratch: array<f32, 256>;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let block = wid.y * params.groups_x + wid.x;
    let group = block * WORKGROUP + lid.x;

    // One lane at a time through the same scratch. The loop bound is a uniform, so the barriers
    // below stay in uniform control flow.
    for (var lane: u32 = 0u; lane < params.lanes; lane = lane + 1u) {
        var value: f32 = 0.0;
        if (group < params.n) {
            value = input[group * params.lanes + lane];
        }
        scratch[lid.x] = value;
        workgroupBarrier();

        // Fixed pairwise tree, so the rounding does not depend on scheduling.
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
            output[block * params.lanes + lane] = scratch[0];
        }
        workgroupBarrier();
    }
}
