// Add-back half of the scan. Lifts each block by the scanned total of every block before it.

struct ScanParams {
    n: u32,
    groups_x: u32,
    num_blocks: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> offsets: array<u32>;
@group(0) @binding(1) var<storage, read_write> data: array<u32>;
@group(0) @binding(2) var<uniform> params: ScanParams;

const WORKGROUP: u32 = 256u;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let block = wid.y * params.groups_x + wid.x;
    let i = block * WORKGROUP + lid.x;
    if (i >= params.n) {
        return;
    }
    data[i] = data[i] + offsets[block];
}
