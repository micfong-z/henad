// Leaf of the stat reduction. One workgroup folds its slice down to one value per lane, and
// `GpuLaneReduce` owns every level above this.

#import shared::prelude::WORKGROUP
#import shared::reduce_tree::block_sum
#import gpu_ants::state::HAS_FOOD_BIT

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
        // One lane is per ant and the other per cell, so each bounds-checks its own domain.
        if (lane == 0u) {
            if (i < params.num_agents) {
                value = f32((state[i] & HAS_FOOD_BIT) != 0u);
            }
        } else {
            if (i < params.n_cells) {
                value = field[i] + field[params.n_cells + i];
            }
        }

        let total = block_sum(lid.x, value);
        if (lid.x == 0u) {
            partials[block * params.lanes + lane] = total;
        }
    }
}
