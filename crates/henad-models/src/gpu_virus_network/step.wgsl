// One node per invocation, mirroring `virus_network/step.rs`.
//
// A susceptible node escapes each infected in-neighbour with its own independent chance, so one draw against the chance
// of escaping them all has the same distribution as the CPU's draw per neighbour.
//
// States are read from the packed copy, which a gather over millions of nodes can keep in cache.

#import shared::prelude::linear_index
#import shared::rng::next_float
#import gpu_virus_network::node_state::{SUSCEPTIBLE, INFECTED, RESISTANT, STATE_BITS, STATES_PER_WORD}

struct StepParams {
    num_nodes: u32,
    groups_x: u32,
    check_frequency: u32,
    // `1 - spread_chance`, the chance that one infected neighbour fails to infect.
    escape_chance: f32,
    recovery_chance: f32,
    resistance_chance: f32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read>       state_bits: array<u32>;
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
@group(0) @binding(2) var<storage, read_write> timer: array<u32>;
@group(0) @binding(3) var<storage, read_write> rng: array<u32>;
@group(0) @binding(4) var<storage, read>       row_start: array<u32>;
@group(0) @binding(5) var<storage, read>       entries: array<u32>;
@group(0) @binding(6) var<uniform>             params: StepParams;

// Returns node `i`'s state at the start of the tick.
fn state_at_start(i: u32) -> u32 {
    return (state_bits[i / STATES_PER_WORD] >> (STATE_BITS * (i % STATES_PER_WORD))) & 3u;
}

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x);
    if (i >= params.num_nodes) {
        return;
    }

    var check = timer[i] + 1u;
    if (check >= params.check_frequency) {
        check = 0u;
    }
    timer[i] = check;

    var r = rng[i];
    var next = state_at_start(i);
    if (next == SUSCEPTIBLE) {
        var survive = 1.0;
        let in_row_end = row_start[2u * i + 1u];
        for (var k = row_start[2u * i]; k < in_row_end; k++) {
            if (state_at_start(entries[k]) == INFECTED) {
                survive *= params.escape_chance;
            }
        }
        // `>=`, so a spread chance of 1 leaves nothing to escape with.
        if (next_float(&r, 1.0) >= survive) {
            next = INFECTED;
        }
    }
    // Checked after spreading, so a node infected this tick can already recover, as in NetLogo.
    if (next == INFECTED && check == 0u && next_float(&r, 1.0) < params.recovery_chance) {
        if (next_float(&r, 1.0) < params.resistance_chance) {
            next = RESISTANT;
        } else {
            next = SUSCEPTIBLE;
        }
    }
    rng[i] = r;
    state[i] = next;
}
