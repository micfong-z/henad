//! NetLogo's Team Assembly model, after Guimerà, Uzzi, Spiro and Amaral (2005).
//!
//! Each tick one team is assembled from newcomers, previous collaborators and other incumbents, and every pair of its
//! members is linked. A node that has not joined a team for more than `max_downtime` ticks retires.

mod assembly;
mod lanes;
mod live;
mod ring;

use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_compute::cpu::primitives::components::{ComponentStats, label_components};
use henad_compute::for_each_chunk_mut;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::{NetworkModel, Nodes, SpringParams};
use henad_core::helpers::{extract_f32, extract_u32, f32_param, u32_param};
use henad_core::network::Network;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use crate::team_assembly::live::LiveSet;
use crate::team_assembly::ring::RetirementRing;

pub use crate::team_assembly::lanes::{TeamChunk, TeamLanes, TeamRead};

// Node colours, which are also indices into `PALETTE`.
pub const IDLE: u8 = 0;
pub const INCUMBENT: u8 = 1;
pub const NEWCOMER: u8 = 2;

// Link colours, which are also indices into `EDGE_PALETTE`.
pub const NEWCOMER_NEWCOMER: u8 = 0;
pub const NEWCOMER_INCUMBENT: u8 = 1;
pub const INCUMBENT_INCUMBENT: u8 = 2;
/// Colour of a link between members who had collaborated before this team.
pub const REPEAT: u8 = 3;

pub const PALETTE: [[u8; 4]; 3] = [
    [0x8C, 0x8C, 0x8C, 0xFF], // Idle - gray
    [0xF5, 0xD0, 0x2E, 0xFF], // Incumbent team member - yellow
    [0x00, 0x7A, 0xF5, 0xFF], // Newcomer team member - blue
];

pub const EDGE_PALETTE: [[u8; 4]; 4] = [
    [0x3B, 0x82, 0xF6, 0xC0], // Newcomer-newcomer - blue
    [0x2D, 0xC7, 0x9A, 0xC0], // Newcomer-incumbent - turquoise
    [0xED, 0xD5, 0x31, 0xC0], // Incumbent-incumbent - yellow
    [0xE4, 0x37, 0x48, 0xC0], // Previous collaborators - red
];

/// Colours of the two component stats.
const COMPONENT_STAT_PALETTE: [[u8; 4]; 2] = [[0xE8, 0xE8, 0xE8, 0xFF], [0xA0, 0x7C, 0xF0, 0xFF]];

/// Number of nodes per chunk of the recolour pass.
const NODE_CHUNK: usize = 8192;

// --8<-- [start:params]
henad_core::params! {
    const TEAM_SIZE = u32_param("team_size", "Team Size", 4, 3, 8);
    /// Index of the number of ticks a node can go without joining a team before it retires.
    ///
    /// NetLogo's slider stops at 100. A larger value grows a larger steady-state population.
    const MAX_DOWNTIME = u32_param("max_downtime", "Max Downtime", 40, 7, 1_000_000);
    const P = f32_param("p", "Incumbent Chance", 0.4, 0.0, 1.0, Some(0.01)).percent();
    const Q = f32_param("q", "Collaborator Chance", 0.65, 0.0, 1.0, Some(0.01)).percent();
}
// --8<-- [end:params]

pub struct TeamAssembly;

/// Hot parameters for one tick.
///
/// Chances are probabilities in `0..=1`. NetLogo's sliders give the same numbers as percentages.
pub struct TeamParams {
    team_size: u32,
    max_downtime: u32,
    /// Chance that a member is an incumbent rather than a newcomer, NetLogo's `p`.
    incumbent_chance: f32,
    /// Chance that an incumbent member is a previous collaborator of the team, NetLogo's `q`.
    collaborator_chance: f32,
}

/// Model state kept outside the lanes and the graph.
#[derive(Default)]
pub struct TeamAux {
    ring: RetirementRing,
    /// Live nodes, listed from the graph on the first tick and kept by the model's own spawns and retirements.
    live: LiveSet,
    /// Whether `live` has been listed from the graph yet.
    live_listed: bool,
    /// Members of the team being assembled, in the order they were picked.
    team: Vec<u32>,
    /// Nodes outside the team linked to a member, the collaborator candidates for the next member.
    candidates: Vec<u32>,
    /// Number of members whose rows are already in `candidates`.
    scanned: usize,
    /// Nodes outside the team, listed when uniform draws keep landing in it.
    eligible: Vec<u32>,
    /// Nodes retiring this tick.
    retiring: Vec<u32>,
    /// Component stats, along with the graph version they were computed at.
    components: Option<(u64, ComponentStats)>,
    // Buffers for the component labelling.
    labels: Vec<u32>,
    label_scratch: Vec<u32>,
}

impl NetworkModel for TeamAssembly {
    const NAME: &'static str = "Team Assembly";
    const ID: &'static str = "team_assembly";
    const DESCRIPTION: &'static str = "Teams of newcomers and incumbents grow a collaboration network.";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const EDGE_PALETTE: &'static [[u8; 4]] = &EDGE_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Newcomer-Newcomer Links", EDGE_PALETTE[0]),
        StatDescriptor::new("Newcomer-Incumbent Links", EDGE_PALETTE[1]),
        StatDescriptor::new("Incumbent-Incumbent Links", EDGE_PALETTE[2]),
        StatDescriptor::new("Previous Collaborator Links", EDGE_PALETTE[3]),
        StatDescriptor::new("Giant Component Share", COMPONENT_STAT_PALETTE[0]),
        StatDescriptor::new("Mean Component Size", COMPONENT_STAT_PALETTE[1]),
    ];
    const DEFAULT_NODES: u32 = 4;
    const DEFAULT_EXTENT: Extent = Extent { w: 100.0, h: 100.0 };
    const LAYOUT: SpringParams = SpringParams {
        spring: 0.18,
        length: 0.0,
        repulsion: 0.05,
        cutoff: 2.5,
        saturation: 5.0,
    };

    type Lanes = TeamLanes;
    type Params = TeamParams;
    type Aux = TeamAux;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue], _extent: Extent) -> TeamParams {
        TeamParams {
            team_size: extract_u32(params, TEAM_SIZE, 4).max(1),
            max_downtime: extract_u32(params, MAX_DOWNTIME, 40),
            incumbent_chance: extract_f32(params, P, 0.4),
            collaborator_chance: extract_f32(params, Q, 0.65),
        }
    }

    fn init(nodes: &mut Nodes<'_, Self>, extent: Extent, params: &[ParamValue], _rng: &mut u64) {
        assembly::setup(nodes, extent, extract_u32(params, TEAM_SIZE, 4));
    }

    fn run_global_pass(nodes: &mut Nodes<'_, Self>, params: &TeamParams, extent: Extent, rng: &mut u64, tick: u64) {
        assembly::assemble(nodes, params, extent, rng, tick);
    }

    /// Colours the last team's members and labels the components, if the graph has changed since they were last
    /// labelled.
    fn prepare_view(nodes: &mut Nodes<'_, Self>, tick: u64) {
        let last = tick;
        let lanes = &mut *nodes.lanes;
        let (team_tick, spawn_tick) = (&lanes.team_tick, &lanes.spawn_tick);
        for_each_chunk_mut!(lanes.color, NODE_CHUNK, |_c, base, slice| {
            for (k, color) in slice.iter_mut().enumerate() {
                let i = base + k;
                *color = if team_tick[i] != last {
                    IDLE
                } else if spawn_tick[i] == last {
                    NEWCOMER
                } else {
                    INCUMBENT
                };
            }
        });

        let version = nodes.graph.version();
        let aux = &mut *nodes.aux;
        if aux.components.is_none_or(|(labeled, _)| labeled != version) {
            let components = label_components(nodes.graph, &mut aux.labels, &mut aux.label_scratch);
            aux.components = Some((version, components));
        }
    }

    /// Returns the link counts by colour, the share of nodes in the giant component and the mean component size.
    ///
    /// The component stats are the ones [`Self::prepare_view`] last computed.
    fn stats(_lanes: &TeamLanes, graph: &Network, aux: &TeamAux) -> Vec<StatValue> {
        let links = count_links(graph.edges().2);
        let components = aux
            .components
            .map_or_else(ComponentStats::default, |(_, components)| components);
        let nodes = graph.node_count() as f64;
        let giant_share = if nodes > 0.0 {
            components.largest as f64 / nodes
        } else {
            0.0
        };
        let mean_size = if components.count > 0 {
            nodes / components.count as f64
        } else {
            0.0
        };
        links
            .into_iter()
            .map(|count| count as f64)
            .chain([giant_share, mean_size])
            .map(StatValue::Scalar)
            .collect()
    }

    fn aux_heap_bytes(aux: &TeamAux) -> usize {
        let lists = aux.team.capacity()
            + aux.candidates.capacity()
            + aux.eligible.capacity()
            + aux.retiring.capacity()
            + aux.labels.capacity()
            + aux.label_scratch.capacity();
        aux.ring.heap_bytes() + aux.live.heap_bytes() + lists * size_of::<u32>()
    }
}

/// Returns the number of links of each colour.
fn count_links(color: &[u8]) -> [u64; 4] {
    reduce_chunks(
        color.len(),
        STATS_CHUNK,
        |range| {
            let mut counts = [0u64; 4];
            for &c in &color[range] {
                counts[usize::from(c).min(3)] += 1;
            }
            counts
        },
        |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]],
        [0; 4],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use henad_compute::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
    use henad_core::model::SimState as _;

    type State = NetworkModelState<TeamAssembly>;

    /// Returns the default params with some overridden by id.
    ///
    /// # Panics
    /// Panics if an id is not declared by the model.
    fn params(overrides: &[(&str, ParamValue)]) -> Vec<ParamValue> {
        let descs = network_model_param_descriptors::<TeamAssembly>();
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
        network_model_param_descriptors::<TeamAssembly>()
            .iter()
            .position(|d| d.id == id)
            .unwrap_or_else(|| panic!("no parameter '{id}'"))
    }

    /// Everything a run decides, with positions compared bit for bit.
    #[derive(Debug, PartialEq)]
    struct Outcome {
        occupied: Vec<bool>,
        spawn_tick: Vec<u64>,
        team_tick: Vec<u64>,
        pos: Vec<u32>,
        src: Vec<u32>,
        dst: Vec<u32>,
        link_color: Vec<u8>,
    }

    fn outcome(state: &State) -> Outcome {
        let lanes = state.lanes();
        let graph = state.graph();
        let (src, dst, link_color) = graph.edges();
        Outcome {
            occupied: (0..graph.slot_count() as u32).map(|i| graph.contains_node(i)).collect(),
            spawn_tick: lanes.spawn_tick.clone(),
            team_tick: lanes.team_tick.clone(),
            pos: lanes.pos_x.iter().chain(&lanes.pos_y).map(|v| v.to_bits()).collect(),
            src: src.to_vec(),
            dst: dst.to_vec(),
            link_color: link_color.to_vec(),
        }
    }

    /// Returns params busy enough to cross chunk boundaries, with nodes retiring and slots being reused.
    fn busy() -> Vec<ParamValue> {
        params(&[
            ("num_agents", ParamValue::U32(20_000)),
            ("team_size", ParamValue::U32(6)),
            ("max_downtime", ParamValue::U32(12)),
            ("p", ParamValue::F32(0.7)),
            ("q", ParamValue::F32(0.5)),
        ])
    }

    fn run_busy(state: &mut State, ticks: u32, publish: bool) {
        for t in 0..ticks {
            if t == ticks / 2 {
                state.set_param(index("max_downtime"), &ParamValue::U32(5));
            }
            state.step();
            if publish {
                state.prepare_view();
            }
        }
    }

    /// Every pass that runs in parallel either reads the graph or writes in chunk order,
    /// so the way rayon splits the work must not affect the result.
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        fn run(threads: usize) -> (Outcome, Vec<u8>, Vec<u64>) {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let mut state = State::from_params(&busy());
                run_busy(&mut state, 60, true);
                let stats = state
                    .stats()
                    .iter()
                    .map(|entry| entry.value.scalar().to_bits())
                    .collect();
                (outcome(&state), state.lanes().color.clone(), stats)
            })
        }
        assert_eq!(run(1), run(7), "the run depends on the thread count");
    }

    /// Publishing colours nodes and labels components, and nothing it writes feeds back into the run.
    #[test]
    fn results_do_not_depend_on_the_publish_cadence() {
        let mut every = State::from_params(&busy());
        let mut never = State::from_params(&busy());
        run_busy(&mut every, 60, true);
        run_busy(&mut never, 60, false);
        assert_eq!(outcome(&every), outcome(&never), "publishing changed the run");

        never.prepare_view();
        assert_eq!(every.lanes().color, never.lanes().color, "the node colors disagree");
    }

    /// NetLogo's setup builds one team whose members are all shown as newcomers, linked as incumbents.
    #[test]
    fn setup_builds_cliques_of_the_team_size() {
        let mut state = State::from_params(&params(&[
            ("num_agents", ParamValue::U32(10)),
            ("team_size", ParamValue::U32(4)),
        ]));
        let graph = state.graph();
        assert_eq!(graph.edge_count(), 6 + 6 + 1, "teams of 4, 4 and 2");
        assert!(graph.has_edge(0, 3) && graph.has_edge(4, 7) && graph.has_edge(8, 9));
        assert!(!graph.has_edge(3, 4), "two setup teams were joined");
        assert!(graph.edges().2.iter().all(|&c| c == INCUMBENT_INCUMBENT));

        state.prepare_view();
        assert!(state.lanes().color.iter().all(|&c| c == NEWCOMER));
    }

    /// Each incumbent pick is a previous collaborator with probability `q`,
    /// plus the chance that a draw from every node outside the team lands on one anyway.
    ///
    /// For each incumbent pick after the first, with `c` collaborators among the `e` nodes outside the team so far,
    /// a collaborator is picked with probability `q + (1 - q) c / e`.
    /// The hits are compared with the sum of those probabilities, within 5 standard deviations.
    /// Newcomers keep the population large and sparse, so those probabilities stay well short of 1.
    #[test]
    fn collaborators_are_picked_at_rate_q() {
        const Q: f64 = 0.65;
        let mut state = State::from_params_seeded(
            &params(&[
                ("num_agents", ParamValue::U32(400)),
                ("team_size", ParamValue::U32(5)),
                ("max_downtime", ParamValue::U32(30)),
                ("p", ParamValue::F32(0.8)),
                ("q", ParamValue::F32(Q as f32)),
            ]),
            Some(0x9A7E),
        );
        let (mut hits, mut expected, mut variance) = (0.0, 0.0, 0.0);
        for _ in 0..2_000 {
            // The rows as they stand while the team is picked. Links and retirements come after the last pick.
            let graph = state.graph();
            let live: BTreeSet<u32> = (0..graph.slot_count() as u32)
                .filter(|&i| graph.contains_node(i))
                .collect();
            let rows: Vec<Vec<u32>> = (0..graph.slot_count() as u32)
                .map(|i| graph.in_neighbors(i).to_vec())
                .collect();
            state.step();

            let team = state.aux().team.clone();
            let now = state.tick();
            for k in 1..team.len() {
                // A newcomer came from the `p` draw, and never reached the `q` draw.
                if state.lanes().spawn_tick[team[k] as usize] == now {
                    continue;
                }
                let so_far = &team[..k];
                let collaborators: BTreeSet<u32> = so_far
                    .iter()
                    .flat_map(|&i| rows.get(i as usize).map_or(&[][..], Vec::as_slice))
                    .copied()
                    .filter(|j| !so_far.contains(j))
                    .collect();
                if collaborators.is_empty() {
                    continue;
                }
                let outside = live.iter().filter(|j| !so_far.contains(j)).count();
                let p = Q + (1.0 - Q) * collaborators.len() as f64 / outside as f64;
                hits += f64::from(u8::from(collaborators.contains(&team[k])));
                expected += p;
                variance += p * (1.0 - p);
            }
        }
        assert!(
            variance > 400.0,
            "the picks were too certain to tell q apart, variance {variance:.1}"
        );
        let allowed = 5.0 * variance.sqrt();
        assert!(
            (hits - expected).abs() <= allowed,
            "{hits} collaborator picks, expected {expected:.1} within {allowed:.1}"
        );
    }
}
