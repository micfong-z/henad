// Packs every node's state into `state_bits`, for the next node pass to read.

#import shared::prelude::linear_index
#import gpu_virus_network::node_state::{STATE_BITS, STATES_PER_WORD}

struct PackParams {
    num_nodes: u32,
    num_words: u32,
    groups_x: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read>       state: array<u32>;
@group(0) @binding(1) var<storage, read_write> state_bits: array<u32>;
@group(0) @binding(2) var<uniform>             params: PackParams;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let w = linear_index(lid, wid, params.groups_x);
    if (w >= params.num_words) {
        return;
    }

    let first = w * STATES_PER_WORD;
    let last = min(first + STATES_PER_WORD, params.num_nodes);
    var word = 0u;
    for (var i = first; i < last; i++) {
        word |= state[i] << (STATE_BITS * (i - first));
    }
    state_bits[w] = word;
}
