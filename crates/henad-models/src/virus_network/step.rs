use henad_core::authoring::model::network_model::{NetworkModel as _, NodeCtx};
use henad_core::authoring::primitives::rng::next_float;
use henad_core::network::Network;

use crate::virus_network::lanes::{VirusChunk, VirusLanes, VirusRead};
use crate::virus_network::{INFECTED, RESISTANT, SUSCEPTIBLE, VirusNetwork, VirusParams};

pub(crate) fn run(lanes: &mut VirusLanes, ctx: &NodeCtx<'_, VirusNetwork>, seed: u64, tick: u64) {
    let (graph, params) = (ctx.graph, ctx.params);
    lanes.run_pass(VirusNetwork::CHUNK, seed, tick, |i, k, read, out, rng| {
        step_node(i, k, read, out, graph, params, rng);
    });
}

// --8<-- [start:step_node]
#[inline]
fn step_node(
    i: usize,
    k: usize,
    read: VirusRead<'_>,
    out: &mut VirusChunk<'_>,
    graph: &Network,
    params: &VirusParams,
    rng: &mut u64,
) {
    let timer = out.timer[k] + 1;
    let timer = if timer >= params.check_frequency { 0 } else { timer };
    out.timer[k] = timer;

    let mut state = read.state[i];
    // Infection is pulled by the susceptible node rather than pushed by infected ones.
    // It reads the state at the start of the tick, so each infected neighbour is one independent chance either way.
    if state == SUSCEPTIBLE {
        for &j in graph.in_neighbors(i as u32) {
            if read.state[j as usize] == INFECTED && next_float(rng, 1.0) < params.spread_chance {
                state = INFECTED;
                break;
            }
        }
    }
    // Checked after spreading, so a node infected this tick can already recover, as in NetLogo.
    if state == INFECTED && timer == 0 && next_float(rng, 1.0) < params.recovery_chance {
        state = if next_float(rng, 1.0) < params.resistance_chance {
            RESISTANT
        } else {
            SUSCEPTIBLE
        };
    }
    out.state[k] = state;
}
// --8<-- [end:step_node]
