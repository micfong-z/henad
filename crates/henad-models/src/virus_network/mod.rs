//! NetLogo's Virus on a Network model, extended with rewiring and a directed variant.

mod lanes;
mod step;
mod wiring;

use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_compute::for_each_chunk_mut;
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::{NetworkModel, NodeCtx, Nodes};
use henad_core::authoring::primitives::rng::{next_float, next_index};
use henad_core::helpers::{
    bool_param, choice_param, extract_bool, extract_choice, extract_f32, extract_u32, f32_param, u32_param,
};
use henad_core::network::Network;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

pub use crate::virus_network::lanes::{VirusChunk, VirusLanes, VirusRead};

// Node states, which are also indices into `PALETTE`.
pub const SUSCEPTIBLE: u8 = 0;
pub const INFECTED: u8 = 1;
pub const RESISTANT: u8 = 2;

// Edge colours, which are also indices into `EDGE_PALETTE`.
pub const EDGE_OPEN: u8 = 0;
/// Colour of an edge with a resistant endpoint, which the virus can no longer cross.
pub const EDGE_BLOCKED: u8 = 1;

pub const PALETTE: [[u8; 4]; 3] = [
    [0x00, 0x7A, 0xF5, 0xFF], // Susceptible - blue
    [0xE4, 0x37, 0x48, 0xFF], // Infected - red
    [0x80, 0x80, 0x80, 0xFF], // Resistant - gray
];

pub const EDGE_PALETTE: [[u8; 4]; 2] = [
    [0xC8, 0xC8, 0xC8, 0xB0], // Open - light gray
    [0x50, 0x50, 0x50, 0x90], // Blocked - dark gray
];

/// Options of the `network` parameter.
const NETWORKS: &[&str] = &["Random", "Geometric"];
const RANDOM: usize = 0;
const GEOMETRIC: usize = 1;

/// Fraction of the world that nodes are placed in.
///
/// Like NetLogo, this keeps nodes away from the edges for visual reasons.
const PLACED: f32 = 0.95;

/// Number of edges per chunk of the recolour pass.
const EDGE_CHUNK: usize = 8192;

// --8<-- [start:params]
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
    const NETWORK = choice_param("network", "Network", NETWORKS, RANDOM).on_reload();
    const KEEP_REWIRING = bool_param("keep_rewiring", "Keep Rewiring", false);
}
// --8<-- [end:params]

henad_core::actions! {
    const REWIRE = ActionDescriptor::new("rewire", "Rewire a link");
}

pub struct VirusNetwork;

/// Hot parameters for one tick.
///
/// Chances are probabilities in `0..=1`. NetLogo's sliders give the same numbers as percentages.
pub struct VirusParams {
    pub(crate) spread_chance: f32,
    pub(crate) check_frequency: u32,
    pub(crate) recovery_chance: f32,
    pub(crate) resistance_chance: f32,
    directed: bool,
    keep_rewiring: bool,
}

/// Returns the colour of an edge between nodes in states `a` and `b`.
///
/// The edge turns grey once either end is resistant, as NetLogo's `become-resistant` does to `my-links`.
pub(crate) fn edge_color(a: u8, b: u8) -> u8 {
    if a == RESISTANT || b == RESISTANT {
        EDGE_BLOCKED
    } else {
        EDGE_OPEN
    }
}

impl NetworkModel for VirusNetwork {
    const NAME: &'static str = "Virus on a Network";
    const ID: &'static str = "virus_network";
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
        match extract_choice(params, NETWORK, RANDOM) {
            GEOMETRIC => wiring::geometric(nodes.graph, &lanes.pos_x, &lanes.pos_y, area, extent, degree, rng),
            _ => wiring::random(nodes.graph, degree, rng),
        }

        infect_distinct(&mut lanes.state, extract_u32(params, INITIAL_OUTBREAK_SIZE, 3), rng);
    }

    fn run_global_pass(nodes: &mut Nodes<'_, Self>, params: &VirusParams, _extent: Extent, rng: &mut u64, _tick: u64) {
        if params.keep_rewiring {
            wiring::rewire(nodes.graph, &nodes.lanes.state, rng);
        }
    }

    fn run_node_pass(lanes: &mut VirusLanes, ctx: &NodeCtx<'_, Self>, seed: u64, tick: u64) {
        step::run(lanes, ctx, seed, tick);
    }

    #[expect(clippy::single_match, reason = "for future multi-action extendability")]
    fn act(action: usize, nodes: &mut Nodes<'_, Self>, _extent: Extent, _params: &[ParamValue], rng: &mut u64) {
        match action {
            REWIRE => wiring::rewire(nodes.graph, &nodes.lanes.state, rng),
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

/// Infects `count` distinct nodes, like NetLogo's `n-of`.
///
/// Uses Floyd's sampling, which draws exactly `count` times no matter how close `count` is to the population.
fn infect_distinct(state: &mut [u8], count: u32, rng: &mut u64) {
    let n = state.len() as u32;
    for j in n - count.min(n)..n {
        let t = next_index(rng, j + 1);
        let pick = if state[t as usize] == INFECTED { j } else { t };
        state[pick as usize] = INFECTED;
    }
}

/// Greys every edge that touches a resistant node.
///
/// Returns whether any edge changed. Each edge is checked before it is written,
/// since most publishes find nothing to change, and a write would make the snapshot copy every edge again.
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

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
    use henad_core::model::SimState as _;

    type State = NetworkModelState<VirusNetwork>;

    /// Returns the default params with some overridden by id.
    ///
    /// # Panics
    /// Panics if an id is not declared by the model.
    fn params(overrides: &[(&str, ParamValue)]) -> Vec<ParamValue> {
        let descs = network_model_param_descriptors::<VirusNetwork>();
        for (id, _) in overrides {
            assert!(descs.iter().any(|d| d.id == *id), "no parameter '{id}'");
        }
        descs
            .iter()
            .map(|d| {
                overrides
                    .iter()
                    .find(|(id, _)| *id == d.id)
                    .map_or_else(|| d.kind.default_value(), |(_, v)| v.clone())
            })
            .collect()
    }

    fn index(id: &str) -> usize {
        network_model_param_descriptors::<VirusNetwork>()
            .iter()
            .position(|d| d.id == id)
            .unwrap_or_else(|| panic!("no parameter '{id}'"))
    }

    /// Everything a tick decides, except positions, which only the layout moves.
    #[derive(Debug, PartialEq)]
    struct Outcome {
        state: Vec<u8>,
        timer: Vec<u32>,
        src: Vec<u32>,
        dst: Vec<u32>,
    }

    fn outcome(state: &State) -> Outcome {
        let (src, dst, _) = state.graph().edges();
        Outcome {
            state: state.lanes().state.clone(),
            timer: state.lanes().timer.clone(),
            src: src.to_vec(),
            dst: dst.to_vec(),
        }
    }

    /// Returns params busy enough to cross chunk boundaries, with rewiring on.
    fn busy() -> Vec<ParamValue> {
        params(&[
            ("num_agents", ParamValue::U32(6_000)),
            ("initial_outbreak_size", ParamValue::U32(300)),
            ("virus_spread_chance", ParamValue::F32(0.08)),
            ("virus_check_frequency", ParamValue::U32(3)),
            ("keep_rewiring", ParamValue::Bool(true)),
        ])
    }

    fn run_busy(state: &mut State, ticks: u32, publish: bool) {
        for t in 0..ticks {
            if t == ticks / 2 {
                state.set_param(index("directed"), &ParamValue::Bool(true));
            }
            state.step();
            if publish {
                state.prepare_view();
            }
        }
    }

    /// Chunk seeds come from the chunk index,
    /// and the global pass and the recolour are either sequential or in chunk order,
    /// so the way rayon splits the work must not affect the result.
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        fn run(threads: usize) -> (Outcome, Vec<u8>) {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let mut state = State::from_params(&busy());
                run_busy(&mut state, 40, true);
                let colors = state.graph().edges().2.to_vec();
                (outcome(&state), colors)
            })
        }
        assert_eq!(run(1), run(7), "the run depends on the thread count");
    }

    /// Publishing only recolours edges, so how often it happens must not change the run.
    #[test]
    fn results_do_not_depend_on_the_publish_cadence() {
        let mut every = State::from_params(&busy());
        let mut never = State::from_params(&busy());
        run_busy(&mut every, 40, true);
        run_busy(&mut never, 40, false);
        assert_eq!(outcome(&every), outcome(&never), "publishing changed the run");

        never.prepare_view();
        assert_eq!(every.graph().edges().2, never.graph().edges().2, "the colours disagree");
    }

    #[test]
    fn the_outbreak_infects_exactly_the_declared_count() {
        for (nodes, outbreak) in [(50, 7), (50, 50), (50, 80), (1, 1)] {
            let state = State::from_params(&params(&[
                ("num_agents", ParamValue::U32(nodes)),
                ("initial_outbreak_size", ParamValue::U32(outbreak)),
            ]));
            let infected = state.lanes().state.iter().filter(|&&s| s == INFECTED).count();
            assert_eq!(infected, outbreak.min(nodes) as usize, "{outbreak} of {nodes}");
        }
    }

    #[test]
    fn a_recolour_greys_every_edge_touching_a_resistant_node() {
        let mut state = State::from_params(&params(&[("num_agents", ParamValue::U32(500))]));
        state.prepare_view();
        let quiet = state.graph().version();
        state.prepare_view();
        assert_eq!(
            state.graph().version(),
            quiet,
            "a recolour with nothing to do moved the version"
        );

        let mut resistant = State::from_graph(
            &params(&[("num_agents", ParamValue::U32(500))]),
            None,
            |nodes, _extent| {
                for i in (0..500).step_by(3) {
                    nodes.lanes.state[i] = RESISTANT;
                }
            },
        );
        let before = resistant.graph().version();
        resistant.prepare_view();
        assert!(
            resistant.graph().version() > before,
            "greying edges left the version behind"
        );

        let state = &resistant.lanes().state;
        let (src, dst, color) = resistant.graph().edges();
        for e in 0..src.len() {
            let touches = state[src[e] as usize] == RESISTANT || state[dst[e] as usize] == RESISTANT;
            let want = if touches { EDGE_BLOCKED } else { EDGE_OPEN };
            assert_eq!(color[e], want, "edge {e} from {} to {}", src[e], dst[e]);
        }
    }

    /// The edge list keeps its orientation either way, so a flip changes how edges are read but not which edges exist.
    #[test]
    fn flipping_the_direction_keeps_every_edge() {
        let mut state = State::from_params(&params(&[("num_agents", ParamValue::U32(800))]));
        let (src, dst, _) = state.graph().edges();
        let (src, dst) = (src.to_vec(), dst.to_vec());

        state.set_param(index("directed"), &ParamValue::Bool(true));
        state.step();
        assert!(state.graph().directed());
        assert_eq!((state.graph().edges().0, state.graph().edges().1), (&src[..], &dst[..]));

        state.set_param(index("directed"), &ParamValue::Bool(false));
        state.step();
        assert!(!state.graph().directed());
        assert_eq!((state.graph().edges().0, state.graph().edges().1), (&src[..], &dst[..]));
    }
}
