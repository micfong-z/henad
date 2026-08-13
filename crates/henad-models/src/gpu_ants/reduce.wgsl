// Leaf of the stat reduction, folding each workgroup into one group of two sums.
//
// The two lanes have different domains — one is per ant, the other per cell — so the dispatch
// covers whichever is longer and each lane bounds-checks its own.

struct Params {
    n: u32,
    lanes: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read> field: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

const WORKGROUP: u32 = 256u;
const HAS_FOOD_BIT: u32 = 0x100u;

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
        if (lane == 0u) {
            if (i < params.num_agents) {
                value = f32((state[i] & HAS_FOOD_BIT) != 0u);
            }
        } else {
            if (i < params.n_cells) {
                value = field[i] + field[params.n_cells + i];
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
