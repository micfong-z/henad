// Largest bucket of the counting sort: the most agents one cell of the latest build holds.
//
// A workgroup takes the largest of its cells in workgroup memory, then adds one global atomic. The dispatch is at most
// 65535 workgroups wide, and each invocation strides over the cells past it.

#import henad::dispatch::WORKGROUP

struct HashParams {
    grid_w: u32,
    grid_h: u32,
    num_agents: u32,
    groups_x: u32,
    cell_w_inv: f32,
    cell_h_inv: f32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read> counts: array<u32>;
@group(0) @binding(1) var<storage, read_write> largest: array<atomic<u32>>;
@group(0) @binding(2) var<uniform> params: HashParams;

var<workgroup> group_largest: atomic<u32>;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    if (lid.x == 0u) {
        atomicStore(&group_largest, 0u);
    }
    // A barrier needs uniform control flow. The loop bounds the cells, and no invocation returns early.
    workgroupBarrier();
    let cells = params.grid_w * params.grid_h;
    var held = 0u;
    for (var cell = wid.x * WORKGROUP + lid.x; cell < cells; cell = cell + groups.x * WORKGROUP) {
        held = max(held, counts[cell]);
    }
    atomicMax(&group_largest, held);
    workgroupBarrier();
    if (lid.x == 0u) {
        atomicMax(&largest[0], atomicLoad(&group_largest));
    }
}
