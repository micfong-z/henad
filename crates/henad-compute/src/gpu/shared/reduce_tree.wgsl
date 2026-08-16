#define_import_path shared::reduce_tree
#import shared::prelude::WORKGROUP

var<workgroup> scratch: array<f32, WORKGROUP>;

// Sums `value` across the workgroup, in a fixed pairwise order so the result replays.
//
// Every invocation must reach this, since it barriers. Only invocation 0 gets the total. Leaves
// `scratch` free again, so a lane loop can call it once per lane.
fn block_sum(lid: u32, value: f32) -> f32 {
    scratch[lid] = value;
    workgroupBarrier();

    for (var stride: u32 = WORKGROUP / 2u; stride > 0u; stride = stride >> 1u) {
        var acc = scratch[lid];
        if (lid < stride) {
            acc = acc + scratch[lid + stride];
        }
        workgroupBarrier();
        scratch[lid] = acc;
        workgroupBarrier();
    }

    let total = scratch[0];
    workgroupBarrier();
    return total;
}
