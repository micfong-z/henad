//! Self-consistency checks for Team Assembly.
//!
//! Every check reads the lanes and the graph from outside the model, and compares them with a scan or a closed form
//! instead of the model's own bookkeeping.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use henad_compute::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
use henad_core::model::SimState as _;
use henad_core::network::Network;
use henad_core::params::ParamValue;
use henad_models::team_assembly::{
    IDLE, INCUMBENT, INCUMBENT_INCUMBENT, NEWCOMER, NEWCOMER_INCUMBENT, NEWCOMER_NEWCOMER, REPEAT, TeamAssembly,
};

type State = NetworkModelState<TeamAssembly>;

const SEED: u64 = 0x7EA3_A55E_5EED_0001;

/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
const SIGMAS: f64 = 5.0;

use ParamValue::{F32, U32};

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

fn build(overrides: &[(&str, ParamValue)]) -> State {
    State::from_params_seeded(&params(overrides), Some(SEED))
}

/// Returns the tick of the step just taken, as the lanes store it.
fn now(state: &State) -> u64 {
    state.tick()
}

/// Returns whether each slot holds a live node.
fn occupied(graph: &Network) -> Vec<bool> {
    (0..graph.slot_count() as u32).map(|i| graph.contains_node(i)).collect()
}

/// Returns the members of the team assembled in the step just taken.
fn team(state: &State) -> Vec<u32> {
    let graph = state.graph();
    let t = now(state);
    (0..graph.slot_count() as u32)
        .filter(|&i| graph.contains_node(i) && state.lanes().team_tick[i as usize] == t)
        .collect()
}

/// Returns the color of every link, keyed by its ends in ascending order.
fn links(graph: &Network) -> BTreeMap<(u32, u32), u8> {
    let (src, dst, color) = graph.edges();
    let mut links = BTreeMap::new();
    for e in 0..src.len() {
        let pair = (src[e].min(dst[e]), src[e].max(dst[e]));
        assert!(links.insert(pair, color[e]).is_none(), "{pair:?} is linked twice");
    }
    links
}

/// Returns the tick a node's downtime is counted from, given the tick of its last team.
///
/// Setup nodes have `team_tick` 0, but NetLogo first counts their downtime on the first tick.
fn retirement_key(team_tick: u64) -> u64 {
    team_tick.max(1)
}

fn stat_values(state: &mut State) -> Vec<f64> {
    state.prepare_view();
    state.stats().iter().map(|entry| entry.value.scalar()).collect()
}

/// Every team has exactly `team_size` distinct members, each linked to every other.
#[test]
fn every_team_is_a_clique_of_the_team_size() {
    let mut state = build(&[
        ("num_agents", U32(40)),
        ("team_size", U32(5)),
        ("max_downtime", U32(20)),
        ("p", F32(0.6)),
        ("q", F32(0.6)),
    ]);
    for _ in 0..400 {
        state.step();
        let members = team(&state);
        assert_eq!(
            members.len(),
            5,
            "tick {}: a team of {} distinct members",
            state.tick(),
            members.len()
        );
        for (k, &a) in members.iter().enumerate() {
            for &b in &members[k + 1..] {
                assert!(
                    state.graph().has_edge(a, b),
                    "tick {}: {a} and {b} are not linked",
                    state.tick()
                );
            }
        }
    }
}

/// A link between members turns red if the pair had collaborated before, and a new link is colored by the
/// incumbency of its ends. Every other link keeps its color.
#[test]
fn links_are_colored_by_history_and_incumbency() {
    let mut state = build(&[
        ("num_agents", U32(60)),
        ("team_size", U32(4)),
        ("max_downtime", U32(15)),
        ("p", F32(0.7)),
        ("q", F32(0.8)),
    ]);
    let (mut repeats, mut mixed) = (0, 0);
    for _ in 0..600 {
        let before = links(state.graph());
        state.step();
        let after = links(state.graph());
        let t = now(&state);
        let members = team(&state);
        let spawn_tick = &state.lanes().spawn_tick;
        for (&(a, b), &color) in &after {
            let in_team = members.contains(&a) && members.contains(&b);
            let expected = if !in_team {
                *before.get(&(a, b)).expect("a link outside the team appeared")
            } else if before.contains_key(&(a, b)) {
                repeats += 1;
                REPEAT
            } else {
                let incumbents = [a, b].iter().filter(|&&i| spawn_tick[i as usize] != t).count();
                mixed += usize::from(incumbents == 1);
                [NEWCOMER_NEWCOMER, NEWCOMER_INCUMBENT, INCUMBENT_INCUMBENT][incumbents]
            };
            assert_eq!(color, expected, "tick {t}: link {a}-{b}");
        }
    }
    assert!(repeats > 0 && mixed > 0, "the run never exercised every color");
}

/// A node that last joined a team at tick `k` is still live at tick `k + max_downtime`, and gone at the tick after.
///
/// This checks every slot after every step, retired and reused slots included.
#[test]
fn a_node_retires_after_exactly_max_downtime_plus_one_idle_ticks() {
    const MAX_DOWNTIME: u32 = 9;
    for p in [0.0, 0.4] {
        let mut state = build(&[
            ("num_agents", U32(12)),
            ("team_size", U32(4)),
            ("max_downtime", U32(MAX_DOWNTIME)),
            ("p", F32(p)),
        ]);
        let mut retired = 0;
        for _ in 0..300 {
            state.step();
            let t = now(&state);
            let live = occupied(state.graph());
            for (i, &is_live) in live.iter().enumerate() {
                let idle = t - retirement_key(state.lanes().team_tick[i]);
                assert_eq!(
                    is_live,
                    idle <= u64::from(MAX_DOWNTIME),
                    "p={p} tick {t}: slot {i} idle for {idle} ticks"
                );
                retired += usize::from(!is_live);
            }
        }
        assert!(retired > 0, "p={p}: nothing retired, so the check proved little");
    }
}

/// The ring retires exactly the nodes an O(V) scan of `team_tick` would, while `max_downtime` goes up and down.
#[test]
fn the_ring_retires_what_the_scan_would() {
    let mut state = build(&[
        ("num_agents", U32(200)),
        ("team_size", U32(6)),
        ("max_downtime", U32(30)),
        ("p", F32(0.5)),
        ("q", F32(0.5)),
    ]);
    let schedule = [30, 12, 7, 40, 9, 9, 25, 8];
    for tick in 0..1_600u32 {
        let max_downtime = schedule[(tick / 200) as usize];
        state.set_param(index("max_downtime"), &U32(max_downtime));
        let live_before = occupied(state.graph());
        state.step();

        let t = now(&state);
        let lanes = state.lanes();
        let live_after = occupied(state.graph());
        for (i, &is_live) in live_after.iter().enumerate() {
            let existed = live_before.get(i).copied().unwrap_or(false) || lanes.spawn_tick[i] == t;
            let idle = t - retirement_key(lanes.team_tick[i]);
            let expected = existed && idle <= u64::from(max_downtime);
            assert_eq!(
                is_live, expected,
                "tick {t}, max_downtime {max_downtime}: slot {i} idle for {idle} ticks"
            );
        }
    }
}

/// A retired node takes its links with it, and the node count matches the occupied slots.
#[test]
fn retired_nodes_take_their_links_with_them() {
    let mut state = build(&[
        ("num_agents", U32(100)),
        ("team_size", U32(5)),
        ("max_downtime", U32(10)),
    ]);
    for _ in 0..500 {
        state.step();
        let graph = state.graph();
        let (src, dst, _) = graph.edges();
        for (&a, &b) in src.iter().zip(dst) {
            assert!(
                graph.contains_node(a) && graph.contains_node(b),
                "link {a}-{b} touches a retired node"
            );
        }
        let live = occupied(graph).iter().filter(|&&l| l).count();
        assert_eq!(graph.node_count(), live);
    }
}

/// Each member is a newcomer with probability `1 - p`.
#[test]
fn newcomer_rate_matches_p() {
    const P: f32 = 0.3;
    let mut state = build(&[
        ("num_agents", U32(100)),
        ("team_size", U32(5)),
        ("max_downtime", U32(40)),
        ("p", F32(P)),
    ]);
    let (mut newcomers, mut members) = (0u64, 0u64);
    for _ in 0..6_000 {
        state.step();
        let t = now(&state);
        for i in team(&state) {
            members += 1;
            newcomers += u64::from(state.lanes().spawn_tick[i as usize] == t);
        }
    }
    let expected = 1.0 - f64::from(P);
    let observed = newcomers as f64 / members as f64;
    let allowed = SIGMAS * (expected * (1.0 - expected) / members as f64).sqrt();
    assert!(
        (observed - expected).abs() <= allowed,
        "newcomer share {observed:.5}, expected {expected:.5} within {allowed:.5} over {members} members"
    );
}

/// The four link stats count the links by color, so together they count every link.
#[test]
fn link_counts_sum_to_the_link_count() {
    let mut state = build(&[("num_agents", U32(50)), ("max_downtime", U32(25))]);
    for _ in 0..300 {
        state.step();
        let stats = stat_values(&mut state);
        let (_, _, color) = state.graph().edges();
        for (c, &count) in stats[..4].iter().enumerate() {
            let listed = color.iter().filter(|&&k| usize::from(k) == c).count();
            assert_eq!(count, listed as f64, "tick {}: color {c}", state.tick());
        }
        assert_eq!(stats[..4].iter().sum::<f64>(), state.graph().edge_count() as f64);
    }
}

/// The giant component share and the mean component size agree with a depth-first search,
/// whose component sizes add up to the population.
#[test]
fn component_stats_match_a_depth_first_search() {
    let mut state = build(&[
        ("num_agents", U32(400)),
        ("team_size", U32(4)),
        ("max_downtime", U32(30)),
        ("p", F32(0.35)),
        ("q", F32(0.4)),
    ]);
    for round in 0..40 {
        for _ in 0..25 {
            state.step();
        }
        let stats = stat_values(&mut state);
        let graph = state.graph();

        let mut seen = vec![false; graph.slot_count()];
        let mut sizes = Vec::new();
        for start in 0..graph.slot_count() as u32 {
            if seen[start as usize] || !graph.contains_node(start) {
                continue;
            }
            seen[start as usize] = true;
            let mut stack = vec![start];
            let mut size = 0usize;
            while let Some(i) = stack.pop() {
                size += 1;
                for &j in graph.in_neighbors(i) {
                    if !seen[j as usize] {
                        seen[j as usize] = true;
                        stack.push(j);
                    }
                }
            }
            sizes.push(size);
        }

        let population = graph.node_count();
        assert_eq!(
            sizes.iter().sum::<usize>(),
            population,
            "round {round}: sizes do not add up"
        );
        let largest = sizes.iter().copied().max().unwrap_or(0);
        assert_eq!(
            stats[4],
            largest as f64 / population as f64,
            "round {round}: giant share"
        );
        assert_eq!(
            stats[5],
            population as f64 / sizes.len() as f64,
            "round {round}: mean size"
        );
    }
}

/// With no incumbents ever picked, each tick adds `team_size` newcomers and retires the cohort from
/// `max_downtime + 1` ticks earlier, so the population settles at exactly `team_size * (max_downtime + 1)`.
#[test]
fn population_settles_at_team_size_times_max_downtime_plus_one_without_incumbents() {
    let mut state = build(&[
        ("num_agents", U32(8)),
        ("team_size", U32(4)),
        ("max_downtime", U32(20)),
        ("p", F32(0.0)),
    ]);
    for _ in 0..200 {
        state.step();
        if state.tick() >= 22 {
            assert_eq!(state.population(), 4 * 21, "tick {}", state.tick());
        }
    }
}

/// NetLogo stops with an error when a team needs an incumbent and none is left outside it. A newcomer joins instead.
#[test]
fn a_team_larger_than_the_population_is_filled_with_newcomers() {
    let mut state = build(&[
        ("num_agents", U32(3)),
        ("team_size", U32(3)),
        ("max_downtime", U32(10)),
        ("p", F32(1.0)),
    ]);
    state.set_param(index("team_size"), &U32(8));
    state.step();
    let members = team(&state);
    assert_eq!(members.len(), 8);
    let newcomers = members
        .iter()
        .filter(|&&i| state.lanes().spawn_tick[i as usize] == 1)
        .count();
    assert_eq!(newcomers, 5, "the three incumbents join, and newcomers fill the rest");
}

/// Returns the number of clusters in the team, linked before the step, that still have a collaborator outside the team.
fn open_clusters(before: &BTreeMap<(u32, u32), u8>, members: &[u32]) -> usize {
    let mut neighbors: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(a, b) in before.keys() {
        neighbors.entry(a).or_default().push(b);
        neighbors.entry(b).or_default().push(a);
    }
    let mut cluster: BTreeMap<u32, usize> = BTreeMap::new();
    let mut open = 0;
    for &start in members {
        if cluster.contains_key(&start) {
            continue;
        }
        let id = cluster.len();
        cluster.insert(start, id);
        let (mut stack, mut outside) = (vec![start], false);
        while let Some(i) = stack.pop() {
            for &j in neighbors.get(&i).map_or(&[][..], Vec::as_slice) {
                if !members.contains(&j) {
                    outside = true;
                } else if let Entry::Vacant(slot) = cluster.entry(j) {
                    slot.insert(id);
                    stack.push(j);
                }
            }
        }
        open += usize::from(outside);
    }
    open
}

/// With `q` certain, a member is a previous collaborator of the team whenever the team has one outside it.
/// The team then only starts a new cluster once every earlier cluster has no collaborator left outside,
/// so at most one cluster still has one. With `q` at zero, members are drawn at random and this breaks.
#[test]
fn with_q_certain_a_team_grows_through_previous_collaborators() {
    for q in [1.0, 0.0] {
        let mut state = build(&[
            ("num_agents", U32(300)),
            ("team_size", U32(6)),
            ("max_downtime", U32(60)),
            ("p", F32(1.0)),
            ("q", F32(q)),
        ]);
        let mut broken = 0;
        for _ in 0..400 {
            let before = links(state.graph());
            state.step();
            let open = open_clusters(&before, &team(&state));
            if q == 1.0 {
                assert!(
                    open <= 1,
                    "tick {}: {open} clusters still had outside collaborators",
                    state.tick()
                );
            }
            broken += usize::from(open > 1);
        }
        if q == 0.0 {
            assert!(
                broken > 0,
                "random members never broke the property, so it cannot tell q apart"
            );
        }
    }
}

/// After each step, the members of that step's team show as newcomers or incumbents and every other node as idle.
#[test]
fn the_last_team_is_colored_by_incumbency() {
    let mut state = build(&[
        ("num_agents", U32(60)),
        ("team_size", U32(5)),
        ("max_downtime", U32(15)),
        ("p", F32(0.5)),
    ]);
    let mut seen = [false; 3];
    for _ in 0..300 {
        state.step();
        state.prepare_view();
        let t = now(&state);
        let lanes = state.lanes();
        for i in 0..state.graph().slot_count() {
            if !state.graph().contains_node(i as u32) {
                continue;
            }
            let expected = if lanes.team_tick[i] != t {
                IDLE
            } else if lanes.spawn_tick[i] == t {
                NEWCOMER
            } else {
                INCUMBENT
            };
            assert_eq!(lanes.color[i], expected, "tick {t}: node {i}");
            seen[usize::from(expected)] = true;
        }
    }
    assert_eq!(seen, [true; 3], "the run never showed every color");
}

/// A live node sits inside the world at a finite position, and a retired slot sits at `NaN`,
/// however often slots are retired and handed to newcomers.
#[test]
fn live_nodes_have_finite_positions_in_the_world() {
    let mut state = build(&[
        ("num_agents", U32(200)),
        ("team_size", U32(6)),
        ("max_downtime", U32(8)),
        ("p", F32(0.3)),
    ]);
    let (w, h) = (100.0, 100.0);
    for _ in 0..500 {
        state.step();
        let lanes = state.lanes();
        for i in 0..state.graph().slot_count() {
            let (x, y) = (lanes.pos_x[i], lanes.pos_y[i]);
            if state.graph().contains_node(i as u32) {
                assert!(
                    (0.0..=w).contains(&x) && (0.0..=h).contains(&y),
                    "tick {}: live node {i} at ({x}, {y})",
                    state.tick()
                );
            } else {
                assert!(
                    x.is_nan() && y.is_nan(),
                    "tick {}: retired slot {i} at ({x}, {y})",
                    state.tick()
                );
            }
        }
    }
}

/// The colors and the link counts hold past the first chunk of the passes that compute them,
/// on a population big enough that newcomers land in a later chunk.
#[test]
fn colors_and_link_counts_hold_across_chunks() {
    let mut state = build(&[("num_agents", U32(20_000)), ("team_size", U32(6)), ("p", F32(0.3))]);
    for _ in 0..3 {
        state.step();
        let stats = stat_values(&mut state);
        let t = now(&state);
        let lanes = state.lanes();
        let graph = state.graph();
        for i in 0..graph.slot_count() {
            if !graph.contains_node(i as u32) {
                continue;
            }
            let expected = if lanes.team_tick[i] != t {
                IDLE
            } else if lanes.spawn_tick[i] == t {
                NEWCOMER
            } else {
                INCUMBENT
            };
            assert_eq!(lanes.color[i], expected, "tick {t}: node {i}");
        }
        let (_, _, color) = graph.edges();
        assert!(color.len() > 8_192, "the links fit in one chunk, so this proved little");
        for (c, &count) in stats[..4].iter().enumerate() {
            let listed = color.iter().filter(|&&k| usize::from(k) == c).count();
            assert_eq!(count, listed as f64, "tick {t}: color {c}");
        }
    }
}
