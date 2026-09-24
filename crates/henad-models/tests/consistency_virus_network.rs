//! Self-consistency checks for Virus on a Network.
//!
//! Every rate is checked against the edge list rather than the model's own rows,
//! so a row that drifted from the list would show up here too.

use std::collections::HashSet;

use henad_compute::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::StatValue;
use henad_models::virus_network::{INFECTED, RESISTANT, SUSCEPTIBLE, VirusNetwork};

type State = NetworkModelState<VirusNetwork>;

const SEED: u64 = 0x7185_0BEE_5EED_0001;

/// Buckets with fewer trials than this are skipped, to avoid false positives from small-number statistics.
const MIN_TRIALS: u64 = 1_000;

/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
const SIGMAS: f64 = 5.0;

/// Number of nodes, large enough that every bucket in these tests clears `MIN_TRIALS`.
const NODES: u32 = 200_000;

/// Initial outbreak size. A quarter of the nodes, so a susceptible node often has several infected neighbours.
const OUTBREAK: u32 = NODES / 4;

use ParamValue::{Bool, Choice, F32, U32};

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

fn build(overrides: &[(&str, ParamValue)]) -> State {
    State::from_params_seeded(&params(overrides), Some(SEED))
}

fn states(state: &State) -> Vec<u8> {
    state.lanes().state.clone()
}

fn band(expected: f64, trials: u64) -> f64 {
    SIGMAS * (expected * (1.0 - expected) / trials as f64).sqrt()
}

fn assert_rate(what: &str, hits: u64, trials: u64, expected: f64) {
    assert!(trials >= MIN_TRIALS, "{what}: only {trials} trials");
    let observed = hits as f64 / trials as f64;
    let allowed = band(expected, trials);
    assert!(
        (observed - expected).abs() <= allowed,
        "{what}: observed {observed:.6}, expected {expected:.6}, off by {:.6} (allowed {allowed:.6}) over {trials}",
        (observed - expected).abs()
    );
}

/// Returns, for each node, the number of infected nodes with an edge to it.
///
/// Counted from the edge list, which is read as directed only if `directed` is set.
fn infected_sources(state: &State, before: &[u8], directed: bool) -> Vec<usize> {
    let (src, dst, _) = state.graph().edges();
    let mut count = vec![0; before.len()];
    for (&a, &b) in src.iter().zip(dst) {
        if before[a as usize] == INFECTED {
            count[b as usize] += 1;
        }
        if !directed && before[b as usize] == INFECTED {
            count[a as usize] += 1;
        }
    }
    count
}

#[test]
fn encoding_matches_the_models_own_stats() {
    let state = build(&[("num_agents", U32(20_000)), ("initial_outbreak_size", U32(5_000))]);
    let cells = states(&state);
    let stats = state.stats();

    for (value, name) in [
        (SUSCEPTIBLE, "Susceptible"),
        (INFECTED, "Infected"),
        (RESISTANT, "Resistant"),
    ] {
        let counted = cells.iter().filter(|&&c| c == value).count() as f64;
        let entry = stats
            .iter()
            .find(|e| e.label == name)
            .unwrap_or_else(|| panic!("no `{name}` stat"));
        let StatValue::Scalar(reported) = entry.value else {
            panic!("`{name}` is not a scalar: {:?}", entry.value)
        };
        assert!(
            (counted - reported).abs() < 0.5,
            "state {value} counted {counted} but the model reports {reported} `{name}`"
        );
    }
}

/// Checks `P(S -> I | k infected sources) = 1 - (1 - c)^k` for every `k` with enough samples,
/// with the edges read both as directed and as undirected.
#[test]
fn infection_rate_matches_the_closed_form() {
    const SPREAD: f64 = 0.10;
    for directed in [false, true] {
        let mut state = build(&[
            ("num_agents", U32(NODES)),
            ("initial_outbreak_size", U32(OUTBREAK)),
            ("virus_spread_chance", F32(SPREAD as f32)),
            ("recovery_chance", F32(0.0)),
            ("directed", Bool(directed)),
        ]);
        let before = states(&state);
        let sources = infected_sources(&state, &before, directed);
        state.step();
        let after = states(&state);

        let mut trials = [0u64; 32];
        let mut infections = [0u64; 32];
        for i in 0..before.len() {
            if before[i] != SUSCEPTIBLE || sources[i] >= trials.len() {
                continue;
            }
            trials[sources[i]] += 1;
            if after[i] == INFECTED {
                infections[sources[i]] += 1;
            }
        }

        assert_eq!(
            infections[0], 0,
            "directed={directed}: a node with no infected source caught it"
        );
        let mut checked = 0;
        for k in 1..trials.len() {
            if trials[k] < MIN_TRIALS {
                continue;
            }
            let expected = 1.0 - (1.0 - SPREAD).powi(k as i32);
            assert_rate(
                &format!("directed={directed} k={k}"),
                infections[k],
                trials[k],
                expected,
            );
            checked += 1;
        }
        assert!(
            checked >= 3,
            "directed={directed}: only {checked} buckets had enough samples"
        );
    }
}

/// A checked node recovers with the recovery chance, and a recovered node becomes resistant with the resistance chance.
/// The others go back to susceptible.
#[test]
fn recovery_and_resistance_match_the_parameters() {
    const RECOVERY: f64 = 0.30;
    const RESISTANCE: f64 = 0.40;
    let mut state = build(&[
        ("num_agents", U32(NODES)),
        ("initial_outbreak_size", U32(OUTBREAK)),
        ("virus_spread_chance", F32(0.0)),
        ("recovery_chance", F32(RECOVERY as f32)),
        ("gain_resistance_chance", F32(RESISTANCE as f32)),
    ]);
    let before = states(&state);
    state.step();
    let after = states(&state);

    let (mut trials, mut to_s, mut to_r) = (0u64, 0u64, 0u64);
    for (&b, &a) in before.iter().zip(&after) {
        if b != INFECTED {
            continue;
        }
        trials += 1;
        match a {
            SUSCEPTIBLE => to_s += 1,
            RESISTANT => to_r += 1,
            _ => {}
        }
    }
    assert_rate("I -> S", to_s, trials, RECOVERY * (1.0 - RESISTANCE));
    assert_rate("I -> R", to_r, trials, RECOVERY * RESISTANCE);
}

/// A node infected this tick is checked for recovery in the same tick, as in NetLogo.
///
/// With a check every tick and certain resistance, `P(S -> R | k) = (1 - 0.9^k) 0.3`.
#[test]
fn a_node_infected_this_tick_can_recover_this_tick() {
    const SPREAD: f64 = 0.1;
    const RECOVERY: f64 = 0.3;
    let mut state = build(&[
        ("num_agents", U32(NODES)),
        ("initial_outbreak_size", U32(OUTBREAK)),
        ("virus_spread_chance", F32(SPREAD as f32)),
        ("virus_check_frequency", U32(1)),
        ("recovery_chance", F32(RECOVERY as f32)),
        ("gain_resistance_chance", F32(1.0)),
    ]);
    let before = states(&state);
    let sources = infected_sources(&state, &before, false);
    state.step();
    let after = states(&state);

    let mut trials = [0u64; 32];
    let mut recoveries = [0u64; 32];
    for i in 0..before.len() {
        if before[i] != SUSCEPTIBLE || sources[i] >= trials.len() {
            continue;
        }
        trials[sources[i]] += 1;
        if after[i] == RESISTANT {
            recoveries[sources[i]] += 1;
        }
    }
    let mut checked = 0;
    for k in 1..trials.len() {
        if trials[k] < MIN_TRIALS {
            continue;
        }
        let expected = (1.0 - (1.0 - SPREAD).powi(k as i32)) * RECOVERY;
        assert_rate(&format!("k={k}"), recoveries[k], trials[k], expected);
        checked += 1;
    }
    assert!(checked >= 3, "only {checked} buckets had enough samples");
}

/// With a check every `F` ticks, each node is checked once per `F` ticks, at its own phase.
/// Certain recovery and resistance make every check visible as a node turning resistant.
#[test]
fn each_node_is_checked_once_per_cycle() {
    const EVERY: u32 = 4;
    let mut state = build(&[
        ("num_agents", U32(NODES)),
        ("initial_outbreak_size", U32(OUTBREAK)),
        ("virus_spread_chance", F32(0.0)),
        ("virus_check_frequency", U32(EVERY)),
        ("recovery_chance", F32(1.0)),
        ("gain_resistance_chance", F32(1.0)),
    ]);

    for t in 1..=EVERY {
        state.step();
        let resistant = states(&state).iter().filter(|&&s| s == RESISTANT).count() as u64;
        if t < EVERY {
            assert_rate(
                &format!("checked by tick {t}"),
                resistant,
                u64::from(OUTBREAK),
                f64::from(t) / f64::from(EVERY),
            );
        } else {
            assert_eq!(
                resistant,
                u64::from(OUTBREAK),
                "a whole cycle left some infected node unchecked"
            );
        }
    }
}

/// Resistance is permanent, and a susceptible node only changes state if it has an infected source.
///
/// A node can go all the way to resistant in one tick. NetLogo checks for recovery after spreading,
/// so a node infected this tick can also recover this tick. The run includes a direction flip.
#[test]
fn only_the_declared_transitions_happen() {
    let mut state = build(&[
        ("num_agents", U32(20_000)),
        ("initial_outbreak_size", U32(2_000)),
        ("virus_spread_chance", F32(0.1)),
        ("recovery_chance", F32(0.1)),
        ("gain_resistance_chance", F32(0.3)),
        ("virus_check_frequency", U32(2)),
    ]);
    let mut before = states(&state);
    let mut directed = false;
    for tick in 0..100 {
        if tick == 50 {
            directed = true;
            state.set_param(index("directed"), &Bool(directed));
        }
        let sources = infected_sources(&state, &before, directed);
        state.step();
        let after = states(&state);
        for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
            assert!(
                b != RESISTANT || a == RESISTANT,
                "node {i} lost its resistance on tick {tick}"
            );
            assert!(
                b != SUSCEPTIBLE || a == SUSCEPTIBLE || sources[i] > 0,
                "node {i} went from susceptible to {a} with no infected source on tick {tick}"
            );
        }
        before = after;
    }
}

/// Three nodes with edges `0 -> 1` and `2 -> 0`, where node 0 is infected and spread is certain.
/// When directed, only node 1 catches the virus. When undirected, both nodes do.
#[test]
fn directed_infection_follows_the_edges() {
    fn run(directed: bool) -> Vec<u8> {
        let values = params(&[
            ("num_agents", U32(3)),
            ("average_node_degree", U32(1)),
            ("initial_outbreak_size", U32(1)),
            ("virus_spread_chance", F32(1.0)),
            ("recovery_chance", F32(0.0)),
            ("directed", Bool(directed)),
        ]);
        let mut state = State::from_graph(&values, Some(SEED), |nodes, _extent| {
            while nodes.graph.edge_count() > 0 {
                nodes.graph.remove_edge(0);
            }
            nodes.graph.add_edge(0, 1, 0);
            nodes.graph.add_edge(2, 0, 0);
            nodes.lanes.state.fill(SUSCEPTIBLE);
            nodes.lanes.state[0] = INFECTED;
        });
        state.step();
        states(&state)
    }
    assert_eq!(run(true), [INFECTED, INFECTED, SUSCEPTIBLE], "directed");
    assert_eq!(run(false), [INFECTED, INFECTED, INFECTED], "undirected");
}

/// Returns the unordered pairs in the edge list, after checking that none repeats and none is a loop.
fn simple_pairs(state: &State) -> HashSet<(u32, u32)> {
    let (src, dst, _) = state.graph().edges();
    let mut pairs = HashSet::new();
    for (&a, &b) in src.iter().zip(dst) {
        assert_ne!(a, b, "a self loop");
        assert!(pairs.insert((a.min(b), a.max(b))), "{a} and {b} are joined twice");
        assert!(
            state.graph().has_edge(a, b),
            "edge {a} to {b} is listed but not in the rows"
        );
    }
    pairs
}

#[test]
fn rewiring_keeps_the_edge_count_and_the_graph_simple() {
    for directed in [false, true] {
        let mut state = build(&[
            ("num_agents", U32(2_000)),
            ("directed", Bool(directed)),
            ("keep_rewiring", Bool(true)),
        ]);
        let edges = state.graph().edge_count();
        let before = simple_pairs(&state);

        for _ in 0..3_000 {
            assert!(state.act(0), "Rewire a link was refused");
        }
        for _ in 0..200 {
            state.step();
        }
        assert_eq!(
            state.graph().edge_count(),
            edges,
            "directed={directed}: rewiring changed the count"
        );

        // Each rewire moves a uniformly chosen edge, so an original edge survives with probability (1 - 1/m)^3200.
        let after = simple_pairs(&state);
        let kept = before.intersection(&after).count() as f64 / edges as f64;
        let expected = (1.0 - 1.0 / edges as f64).powi(3_200);
        assert!(
            (kept - expected).abs() < 0.05,
            "directed={directed}: {kept} of the edges stayed put, expected about {expected}"
        );
    }
}

fn mean_degree(state: &State) -> f64 {
    2.0 * state.graph().edge_count() as f64 / state.graph().node_count() as f64
}

#[test]
fn a_random_network_has_the_declared_edge_count() {
    for (nodes, degree) in [(10_001, 5), (10_000, 6), (7, 20)] {
        let state = build(&[("num_agents", U32(nodes)), ("average_node_degree", U32(degree))]);
        let complete = u64::from(nodes) * u64::from(nodes - 1) / 2;
        let wanted = (u64::from(degree) * u64::from(nodes)).div_ceil(2).min(complete);
        assert_eq!(
            state.graph().edge_count() as u64,
            wanted,
            "{nodes} nodes of degree {degree}"
        );
        simple_pairs(&state);
    }
}

/// The reach accounts for nodes near the edge of the world, which a plain `π r²` would leave short.
/// At 200 nodes that shortfall is about a tenth, well outside the band averaged over twenty networks.
#[test]
fn a_geometric_network_has_the_declared_mean_degree() {
    let geometric = |nodes: u32, seed: u64| {
        State::from_params_seeded(
            &params(&[("num_agents", U32(nodes)), ("network", Choice(1))]),
            Some(seed),
        )
    };

    let big = geometric(50_000, SEED);
    simple_pairs(&big);
    let got = mean_degree(&big);
    assert!((got - 6.0).abs() < 0.12, "mean degree {got} at 50000 nodes, wanting 6");

    let small: f64 = (0..20).map(|seed| mean_degree(&geometric(200, seed))).sum::<f64>() / 20.0;
    assert!((small - 6.0).abs() < 0.3, "mean degree {small} at 200 nodes, wanting 6");
}

/// A geometric network joins near neighbours, so its edges are far shorter than a random network's.
#[test]
fn a_geometric_network_joins_near_neighbours() {
    fn mean_length(network: usize) -> f64 {
        let state = build(&[("num_agents", U32(5_000)), ("network", Choice(network))]);
        let (src, dst, _) = state.graph().edges();
        let (x, y) = (&state.lanes().pos_x, &state.lanes().pos_y);
        let total: f64 = src
            .iter()
            .zip(dst)
            .map(|(&a, &b)| f64::from((x[a as usize] - x[b as usize]).hypot(y[a as usize] - y[b as usize])))
            .sum();
        total / src.len() as f64
    }
    let (random, geometric) = (mean_length(0), mean_length(1));
    assert!(
        geometric * 10.0 < random,
        "geometric edges average {geometric}, random ones {random}"
    );
}

/// Rewiring every tick for fifty times the edge count must leave memory bounded.
/// A row that leaked an entry per rewire would instead grow with the tick count.
///
/// The ceiling is eight times the packed size. Stale space stays under half of what the rows hold,
/// a row at most doubles past its packed size, and a `Vec` at most doubles past its length.
#[test]
fn rewiring_every_tick_leaves_memory_bounded() {
    let mut state = build(&[("num_agents", U32(400)), ("keep_rewiring", Bool(true))]);
    let start = state.graph().heap_bytes();
    let mut peak = start;
    for _ in 0..50 * state.graph().edge_count() {
        state.step();
        peak = peak.max(state.graph().heap_bytes());
    }
    assert!(
        peak < 8 * start,
        "the rows grew from {start} to {peak} bytes while rewiring"
    );
}
