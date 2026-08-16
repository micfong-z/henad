// One level of a multi-level exclusive prefix sum. Each workgroup scans WORKGROUP elements and
// publishes its total to `block_sums`, which the level above scans in turn.

#import shared::prelude::WORKGROUP

struct ScanParams {
    n: u32,
    groups_x: u32,
    num_blocks: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var<storage, read_write> block_sums: array<u32>;
@group(0) @binding(3) var<uniform> params: ScanParams;

var<workgroup> partials: array<u32, 256>;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let block = wid.y * params.groups_x + wid.x;
    let t = lid.x;
    let i = block * WORKGROUP + t;

    // Reads as zero past the end rather than returning early, so every lane hits the barriers.
    var value: u32 = 0u;
    if (i < params.n) {
        value = input[i];
    }
    partials[t] = value;
    workgroupBarrier();

    // Hillis-Steele. Both barriers sit outside the branch, to keep control flow uniform.
    for (var offset: u32 = 1u; offset < WORKGROUP; offset = offset << 1u) {
        var addend: u32 = 0u;
        if (t >= offset) {
            addend = partials[t - offset];
        }
        workgroupBarrier();
        partials[t] = partials[t] + addend;
        workgroupBarrier();
    }

    let inclusive = partials[t];
    if (i < params.n) {
        output[i] = inclusive - value;
    }
    if (t == WORKGROUP - 1u && block < params.num_blocks) {
        block_sums[block] = inclusive;
    }
}
