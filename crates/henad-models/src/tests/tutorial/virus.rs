//! Virus on a Network as `docs/guide/first-model/virus-network.md` builds it.
//!
//! The id is `virus` rather than `virus_network`, since the shipped model already holds that one
//! and the page tells a reader the same thing. The page leaves out the shipped model's geometric
//! generator, so this declares one parameter fewer.

use henad_compute::agent_lanes;
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_compute::for_each_chunk_mut;
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::{NetworkModel, NodeCtx, Nodes};
use henad_core::authoring::primitives::rng::{next_float, next_index};
use henad_core::helpers::{bool_param, extract_bool, extract_f32, extract_u32, f32_param, u32_param};
use henad_core::network::Network;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

agent_lanes! {
    pub struct VirusLanes {
        read VirusRead;
        chunk VirusChunk;
        dual state / next_state: u8,
        plain timer: u32 = 0,
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
    }
    color = state;
}

pub const SUSCEPTIBLE: u8 = 0;
pub const INFECTED: u8 = 1;
pub const RESISTANT: u8 = 2;

pub const PALETTE: [[u8; 4]; 3] = [
    [0x00, 0x7A, 0xF5, 0xFF], // Susceptible
    [0xE4, 0x37, 0x48, 0xFF], // Infected
    [0x80, 0x80, 0x80, 0xFF], // Resistant
];

pub const EDGE_OPEN: u8 = 0;
pub const EDGE_BLOCKED: u8 = 1;

pub const EDGE_PALETTE: [[u8; 4]; 2] = [
    [0xC8, 0xC8, 0xC8, 0xB0], // Open
    [0x50, 0x50, 0x50, 0x90], // Blocked
];

/// Fraction of the world that nodes are placed in. The rest is a margin along the borders.
const PLACED: f32 = 0.95;

/// Number of edges per chunk of the recolour pass.
const EDGE_CHUNK: usize = 8192;

/// Number of draws a rewire makes before giving up.
const REWIRE_TRIES: u32 = 64;

henad_core::params! {
    const AVERAGE_NODE_DEGREE = u32_param("average_node_degree", "Average Node Degree", 6, 1, 20).on_reload();
    const INITIAL_OUTBREAK_SIZE =
        u32_param("initial_outbreak_size", "Initial Outbreak Size", 3, 1, 10_000).on_reload();
    const VIRUS_SPREAD_CHANCE =
        f32_param("virus_spread_chance", "Virus Spread Chance", 0.025, 0.0, 1.0, Some(0.001)).percent();
    const VIRUS_CHECK_FREQUENCY = u32_param("virus_check_frequency", "Virus Check Frequency", 1, 1, 20);
    const RECOVERY_CHANCE = f32_param("recovery_chance", "Recovery Chance", 0.05, 0.0, 1.0, Some(0.001)).percent();
    const GAIN_RESISTANCE_CHANCE =
        f32_param("gain_resistance_chance", "Gain Resistance Chance", 0.05, 0.0, 1.0, Some(0.01)).percent();
    const DIRECTED = bool_param("directed", "Directed", false);
    const KEEP_REWIRING = bool_param("keep_rewiring", "Keep Rewiring", false);
}

henad_core::actions! {
    const REWIRE = ActionDescriptor::new("rewire", "Rewire a link");
}

pub struct VirusModel;

pub struct VirusParams {
    spread_chance: f32,
    check_frequency: u32,
    recovery_chance: f32,
    resistance_chance: f32,
    directed: bool,
    keep_rewiring: bool,
}

impl NetworkModel for VirusModel {
    const NAME: &'static str = "Virus on a Network";
    const ID: &'static str = "virus";
    const DESCRIPTION: &'static str = "A virus spreading over a network.";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const EDGE_PALETTE: &'static [[u8; 4]] = &EDGE_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Susceptible", PALETTE[0]),
        StatDescriptor::new("Infected", PALETTE[1]),
        StatDescriptor::new("Resistant", PALETTE[2]),
    ];
    const ACTIONS: &'static [ActionDescriptor] = ACTION_SPECS;
    const DEFAULT_NODES: u32 = 10_000;
    const DEFAULT_EXTENT: Extent = Extent { w: 1_000.0, h: 1_000.0 };

    type Lanes = VirusLanes;
    type Params = VirusParams;
    type Aux = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue], _extent: Extent) -> VirusParams {
        VirusParams {
            spread_chance: extract_f32(params, VIRUS_SPREAD_CHANCE, 0.025),
            check_frequency: extract_u32(params, VIRUS_CHECK_FREQUENCY, 1).max(1),
            recovery_chance: extract_f32(params, RECOVERY_CHANCE, 0.05),
            resistance_chance: extract_f32(params, GAIN_RESISTANCE_CHANCE, 0.05),
            directed: extract_bool(params, DIRECTED, false),
            keep_rewiring: extract_bool(params, KEEP_REWIRING, false),
        }
    }

    fn directed(params: &VirusParams) -> bool {
        params.directed
    }

    fn init(nodes: &mut Nodes<'_, Self>, extent: Extent, params: &[ParamValue], rng: &mut u64) {
        let lanes = &mut *nodes.lanes;
        let check_frequency = extract_u32(params, VIRUS_CHECK_FREQUENCY, 1).max(1);
        let area = Extent {
            w: extent.w * PLACED,
            h: extent.h * PLACED,
        };
        let (margin_x, margin_y) = (0.5 * (extent.w - area.w), 0.5 * (extent.h - area.h));
        for i in 0..lanes.pos_x.len() {
            lanes.pos_x[i] = margin_x + next_float(rng, area.w);
            lanes.pos_y[i] = margin_y + next_float(rng, area.h);
            lanes.timer[i] = next_index(rng, check_frequency);
        }

        let degree = extract_u32(params, AVERAGE_NODE_DEGREE, 6);
        random_graph(nodes.graph, degree, rng);

        infect_distinct(&mut lanes.state, extract_u32(params, INITIAL_OUTBREAK_SIZE, 3), rng);
    }

    fn run_global_pass(nodes: &mut Nodes<'_, Self>, params: &VirusParams, _extent: Extent, rng: &mut u64, _tick: u64) {
        if params.keep_rewiring {
            rewire(nodes.graph, &nodes.lanes.state, rng);
        }
    }

    fn run_node_pass(lanes: &mut VirusLanes, ctx: &NodeCtx<'_, Self>, seed: u64, tick: u64) {
        let (graph, params) = (ctx.graph, ctx.params);
        lanes.run_pass(Self::CHUNK, seed, tick, |i, k, read, out, rng| {
            step_node(i, k, read, out, graph, params, rng);
        });
    }

    #[expect(clippy::single_match, reason = "for future multi-action extendability")]
    fn act(action: usize, nodes: &mut Nodes<'_, Self>, _extent: Extent, _params: &[ParamValue], rng: &mut u64) {
        match action {
            REWIRE => rewire(nodes.graph, &nodes.lanes.state, rng),
            _ => {}
        }
    }

    fn prepare_view(nodes: &mut Nodes<'_, Self>, _tick: u64) {
        let state = &nodes.lanes.state;
        nodes
            .graph
            .update_colors(|src, dst, color| recolor(src, dst, color, state));
    }

    fn stats(lanes: &VirusLanes, _graph: &Network, (): &()) -> Vec<StatValue> {
        count_states(&lanes.state)
            .into_iter()
            .map(|count| StatValue::Scalar(count as f64))
            .collect()
    }
}

// --- Setup ---

/// Draws two distinct nodes uniformly at random.
fn random_pair(n: u32, rng: &mut u64) -> (u32, u32) {
    let a = next_index(rng, n);
    let b = next_index(rng, n - 1);
    (a, if b >= a { b + 1 } else { b })
}

/// Returns whether an edge joins `a` and `b` in either direction.
fn joined(graph: &Network, a: u32, b: u32) -> bool {
    graph.has_edge(a, b) || (graph.directed() && graph.has_edge(b, a))
}

/// Joins random pairs of nodes until the average degree reaches `degree`.
fn random_graph(graph: &mut Network, degree: u32, rng: &mut u64) {
    let n = graph.slot_count() as u64;
    if n < 2 {
        return;
    }
    let wanted = (u64::from(degree) * n).div_ceil(2).min(n * (n - 1) / 2) as usize;
    while graph.edge_count() < wanted {
        let (a, b) = random_pair(n as u32, rng);
        if !joined(graph, a, b) {
            graph.add_edge(a, b, EDGE_OPEN);
        }
    }
}

/// Infects `count` distinct nodes, like NetLogo's `n-of`.
fn infect_distinct(state: &mut [u8], count: u32, rng: &mut u64) {
    let n = state.len() as u32;
    for j in n - count.min(n)..n {
        let t = next_index(rng, j + 1);
        let pick = if state[t as usize] == INFECTED { j } else { t };
        state[pick as usize] = INFECTED;
    }
}

// --- Node pass ---

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
    if state == SUSCEPTIBLE {
        for &j in graph.in_neighbors(i as u32) {
            if read.state[j as usize] == INFECTED && next_float(rng, 1.0) < params.spread_chance {
                state = INFECTED;
                break;
            }
        }
    }
    if state == INFECTED && timer == 0 && next_float(rng, 1.0) < params.recovery_chance {
        state = if next_float(rng, 1.0) < params.resistance_chance {
            RESISTANT
        } else {
            SUSCEPTIBLE
        };
    }
    out.state[k] = state;
}

// --- Edges ---

/// Returns the colour of an edge between nodes in states `a` and `b`.
fn edge_color(a: u8, b: u8) -> u8 {
    if a == RESISTANT || b == RESISTANT {
        EDGE_BLOCKED
    } else {
        EDGE_OPEN
    }
}

/// Greys every edge that touches a resistant node, and returns whether any edge changed.
fn recolor(src: &[u32], dst: &[u32], color: &mut [u8], state: &[u8]) -> bool {
    let wanted = |e: usize| edge_color(state[src[e] as usize], state[dst[e] as usize]);
    let stale = {
        let color = &*color;
        reduce_chunks(
            color.len(),
            EDGE_CHUNK,
            |mut range| range.any(|e| color[e] != wanted(e)),
            |a, b| a || b,
            false,
        )
    };
    if stale {
        for_each_chunk_mut!(color, EDGE_CHUNK, |_c, base, slice| {
            for (k, c) in slice.iter_mut().enumerate() {
                *c = wanted(base + k);
            }
        });
    }
    stale
}

/// Moves one random edge to a random pair of nodes that are not already joined.
fn rewire(graph: &mut Network, state: &[u8], rng: &mut u64) {
    let (n, m) = (graph.slot_count() as u32, graph.edge_count() as u32);
    if n < 2 || m == 0 {
        return;
    }
    for _ in 0..REWIRE_TRIES {
        let (a, b) = random_pair(n, rng);
        if !joined(graph, a, b) {
            graph.remove_edge(next_index(rng, m));
            graph.add_edge(a, b, edge_color(state[a as usize], state[b as usize]));
            return;
        }
    }
}

// --- Statistics ---

/// Returns the numbers of susceptible, infected and resistant nodes.
fn count_states(state: &[u8]) -> [u64; 3] {
    reduce_chunks(
        state.len(),
        STATS_CHUNK,
        |range| {
            let mut counts = [0u64; 3];
            for &s in &state[range] {
                counts[usize::from(s)] += 1;
            }
            counts
        },
        |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
        [0; 3],
    )
}

#[test]
fn the_virus_walks_a_path_and_stops_at_a_resistant_node() {
    use henad_compute::cpu::network_engine::NetworkModelState;
    use henad_core::model::SimState as _;

    let params = vec![
        ParamValue::U32(5),      // num_agents
        ParamValue::F32(100.0),  // world_width
        ParamValue::F32(100.0),  // world_height
        ParamValue::U32(1),      // average_node_degree
        ParamValue::U32(1),      // initial_outbreak_size
        ParamValue::F32(1.0),    // virus_spread_chance
        ParamValue::U32(1),      // virus_check_frequency
        ParamValue::F32(0.0),    // recovery_chance
        ParamValue::F32(0.0),    // gain_resistance_chance
        ParamValue::Bool(false), // directed
        ParamValue::Bool(false), // keep_rewiring
    ];

    // A path from 0 to 4, with the outbreak at one end and a resistant node in the middle.
    let mut state = NetworkModelState::<VirusModel>::from_graph(&params, None, |nodes, _extent| {
        *nodes.graph = Network::new(5, false);
        for i in 0..4 {
            nodes.graph.add_edge(i, i + 1, EDGE_OPEN);
        }
        nodes
            .lanes
            .state
            .copy_from_slice(&[INFECTED, SUSCEPTIBLE, RESISTANT, SUSCEPTIBLE, SUSCEPTIBLE]);
    });

    state.step();
    assert_eq!(
        state.lanes().state,
        [INFECTED, INFECTED, RESISTANT, SUSCEPTIBLE, SUSCEPTIBLE],
        "the virus did not reach the next node"
    );

    for _ in 0..10 {
        state.step();
    }
    assert_eq!(
        state.lanes().state,
        [INFECTED, INFECTED, RESISTANT, SUSCEPTIBLE, SUSCEPTIBLE],
        "the virus crossed a resistant node"
    );

    state.prepare_view();
    let (_, _, color) = state.graph().edges();
    assert_eq!(
        color,
        [EDGE_OPEN, EDGE_BLOCKED, EDGE_BLOCKED, EDGE_OPEN],
        "the two edges touching node 2 are not grey"
    );
}
