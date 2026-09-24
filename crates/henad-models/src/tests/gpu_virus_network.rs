//! GPU Virus on a Network against the CPU model it ports, and against the rates the CPU consistency suite checks.
//!
//! The GPU draws from its own streams after tick 0, so every check past the first tick is statistical or an invariant.

use std::collections::HashSet;

use henad_compute::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
use henad_compute::gpu::{GpuContext, GpuSimState as _, MAX_STEPS_PER_SUBMISSION};
use henad_core::authoring::model::network_model::Nodes;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue::{self, Bool, Choice, F32, U32};
use henad_core::view::StatValue;

use crate::gpu_virus_network::GpuVirusNetwork;
use crate::virus_network::{EDGE_PALETTE, INFECTED, PALETTE, RESISTANT, SUSCEPTIBLE, VirusNetwork, edge_color};

type CpuState = NetworkModelState<VirusNetwork>;

const SEED: u64 = 0x6B0_5EED_0000_0001;

/// Buckets with fewer trials than this are skipped, to avoid false positives from small-number statistics.
const MIN_TRIALS: u64 = 1_000;

/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
const SIGMAS: f64 = 5.0;

/// Number of nodes in the rate tests, as in the CPU suite.
const NODES: u32 = 200_000;
const OUTBREAK: u32 = NODES / 4;

const GEOMETRIC: usize = 1;

fn context() -> Option<GpuContext> {
    crate::tests::support::headless_context("gpu_virus_network_test_device", wgpu::Features::empty())
}

/// Returns the default params with some overridden by id. A later override of the same id wins.
///
/// # Panics
///
/// If an id is not declared by the model.
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
                .rfind(|(id, _)| *id == d.id)
                .map_or_else(|| d.kind.default_value(), |(_, value)| value.clone())
        })
        .collect()
}

fn build(ctx: &GpuContext, overrides: &[(&str, ParamValue)]) -> GpuVirusNetwork {
    GpuVirusNetwork::new(ctx, &params(overrides), Some(SEED))
}

/// Params busy enough that every transition happens within a few ticks.
fn busy(directed: bool) -> Vec<(&'static str, ParamValue)> {
    vec![
        ("num_agents", U32(6_000)),
        ("initial_outbreak_size", U32(300)),
        ("virus_spread_chance", F32(0.08)),
        ("virus_check_frequency", U32(3)),
        ("directed", Bool(directed)),
    ]
}

/// [`busy`], rewiring an edge every tick if `rewiring` is set.
fn busy_rewiring(directed: bool, rewiring: bool) -> Vec<(&'static str, ParamValue)> {
    [busy(directed), vec![("keep_rewiring", Bool(rewiring))]].concat()
}

/// Steps `ticks` ticks, one submission per entry of `batches`.
fn run_batches(ctx: &GpuContext, state: &mut GpuVirusNetwork, batches: &[u32]) {
    for &count in batches {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, count, None);
        ctx.queue.submit(Some(encoder.finish()));
    }
}

/// Steps `ticks` ticks in full submissions.
fn run(ctx: &GpuContext, state: &mut GpuVirusNetwork, ticks: u32) {
    let mut batches = vec![MAX_STEPS_PER_SUBMISSION; (ticks / MAX_STEPS_PER_SUBMISSION) as usize];
    if !ticks.is_multiple_of(MAX_STEPS_PER_SUBMISSION) {
        batches.push(ticks % MAX_STEPS_PER_SUBMISSION);
    }
    run_batches(ctx, state, &batches);
}

/// Runs the snapshot passes and returns the susceptible, infected and resistant counts they read back.
fn counts(ctx: &GpuContext, state: &mut GpuVirusNetwork) -> [u64; 3] {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
    let stats = state.stats();
    [0, 1, 2].map(|k| match &stats[k].value {
        StatValue::Scalar(count) => *count as u64,
        other => panic!("expected a scalar stat, got {other:?}"),
    })
}

fn states(state: &GpuVirusNetwork) -> Vec<u8> {
    state.read_states().into_iter().map(|s| s as u8).collect()
}

/// Everything a tick decides, apart from edge colours, which only a publish paints.
fn outcome(state: &GpuVirusNetwork) -> (Vec<u32>, Vec<u32>, Vec<[u32; 2]>) {
    let edges = state
        .read_edges()
        .into_iter()
        .map(|[source, dest, _]| [source, dest])
        .collect();
    (state.read_states(), state.read_timers(), edges)
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

/// Returns, for each node, the number of infected nodes with an edge to it, read as directed only if `directed` is set.
fn infected_sources(edges: &[[u32; 3]], before: &[u8], directed: bool) -> Vec<usize> {
    let mut count = vec![0; before.len()];
    for &[source, dest, _] in edges {
        if before[source as usize] == INFECTED {
            count[dest as usize] += 1;
        }
        if !directed && before[dest as usize] == INFECTED {
            count[source as usize] += 1;
        }
    }
    count
}

/// Returns `(trials, hits)` per number of infected sources, over the susceptible nodes `hit` picks out.
fn by_sources(before: &[u8], sources: &[usize], hit: impl Fn(usize) -> bool) -> ([u64; 32], [u64; 32]) {
    let (mut trials, mut hits) = ([0u64; 32], [0u64; 32]);
    for i in 0..before.len() {
        if before[i] != SUSCEPTIBLE || sources[i] >= trials.len() {
            continue;
        }
        trials[sources[i]] += 1;
        if hit(i) {
            hits[sources[i]] += 1;
        }
    }
    (trials, hits)
}

fn sorted(row: &[u32]) -> Vec<u32> {
    let mut row = row.to_vec();
    row.sort_unstable();
    row
}

#[test]
fn tick_zero_matches_the_cpu_model() {
    let Some(ctx) = context() else {
        return;
    };
    let node_palette = PALETTE.map(u32::from_le_bytes);
    let edge_palette = EDGE_PALETTE.map(u32::from_le_bytes);
    for directed in [false, true] {
        for network in [0, GEOMETRIC] {
            let values = params(&[
                ("num_agents", U32(5_000)),
                ("directed", Bool(directed)),
                ("network", Choice(network)),
            ]);
            let what = format!("directed={directed} network={network}");
            let cpu = CpuState::from_params_seeded(&values, Some(SEED));
            let mut gpu = GpuVirusNetwork::new(&ctx, &values, Some(SEED));
            let lanes = cpu.lanes();

            assert_eq!(states(&gpu), lanes.state, "{what}: states");
            assert_eq!(gpu.read_timers(), lanes.timer, "{what}: timers");
            let positions: Vec<u32> = lanes
                .pos_x
                .iter()
                .zip(&lanes.pos_y)
                .flat_map(|(x, y)| [x.to_bits(), y.to_bits()])
                .collect();
            let gpu_positions: Vec<u32> = gpu.read_positions().iter().map(|p| p.to_bits()).collect();
            assert_eq!(gpu_positions, positions, "{what}: positions");

            let (src, dst, colors) = cpu.graph().edges();
            let edges: Vec<[u32; 3]> = (0..src.len())
                .map(|e| [src[e], dst[e], edge_palette[usize::from(colors[e])]])
                .collect();
            assert!(!edges.is_empty(), "{what}: the graph has no edges to compare");
            assert_eq!(gpu.read_edges(), edges, "{what}: edge list");

            let (row_start, entries) = gpu.read_rows();
            let graph = cpu.graph();
            for i in 0..graph.slot_count() {
                let row = |k: usize| &entries[row_start[k] as usize..row_start[k + 1] as usize];
                assert_eq!(
                    sorted(row(2 * i)),
                    sorted(graph.in_neighbors(i as u32)),
                    "{what}: in-row of node {i}"
                );
                let out_row = if directed {
                    sorted(graph.out_neighbors(i as u32))
                } else {
                    Vec::new()
                };
                assert_eq!(sorted(row(2 * i + 1)), out_row, "{what}: out-row of node {i}");
            }

            let [s, i, r] = counts(&ctx, &mut gpu);
            let count = |state: u8| lanes.state.iter().filter(|&&s| s == state).count() as u64;
            assert_eq!(
                [s, i, r],
                [SUSCEPTIBLE, INFECTED, RESISTANT].map(count),
                "{what}: counts"
            );
            let colors: Vec<u32> = lanes.state.iter().map(|&s| node_palette[usize::from(s)]).collect();
            assert_eq!(gpu.read_node_colors(), colors, "{what}: node colours");
        }
    }
}

#[test]
fn a_run_replays_bit_for_bit() {
    let Some(ctx) = context() else {
        return;
    };
    for (directed, rewiring) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut first = build(&ctx, &busy_rewiring(directed, rewiring));
        let mut second = build(&ctx, &busy_rewiring(directed, rewiring));
        run(&ctx, &mut first, 100);
        run(&ctx, &mut second, 100);
        assert_eq!(
            outcome(&first),
            outcome(&second),
            "directed={directed} rewiring={rewiring}"
        );
    }
}

/// The GPU counterpart of the CPU's thread-count test. A submission boundary must not change the run.
#[test]
fn results_do_not_depend_on_how_steps_are_batched() {
    let Some(ctx) = context() else {
        return;
    };
    for (directed, rewiring) in [(false, false), (true, false), (false, true), (true, true)] {
        let what = format!("directed={directed} rewiring={rewiring}");
        let outcomes: Vec<_> = [vec![64, 36], vec![1, 63, 36], [vec![7; 14], vec![2]].concat()]
            .iter()
            .map(|batches| {
                let mut state = build(&ctx, &busy_rewiring(directed, rewiring));
                run_batches(&ctx, &mut state, batches);
                assert_eq!(state.tick(), 100);
                outcome(&state)
            })
            .collect();
        assert_eq!(outcomes[0], outcomes[1], "{what}: 64+36 against 1+63+36");
        assert_eq!(outcomes[0], outcomes[2], "{what}: 64+36 against sevens");
    }
}

/// Publishing only recolours and counts, so how often it happens must not change the run.
#[test]
fn results_do_not_depend_on_the_publish_cadence() {
    let Some(ctx) = context() else {
        return;
    };
    for (directed, rewiring) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut every = build(&ctx, &busy_rewiring(directed, rewiring));
        let mut never = build(&ctx, &busy_rewiring(directed, rewiring));
        for _ in 0..60 {
            run(&ctx, &mut every, 1);
            counts(&ctx, &mut every);
        }
        run(&ctx, &mut never, 60);
        assert_eq!(
            outcome(&every),
            outcome(&never),
            "directed={directed} rewiring={rewiring}"
        );
    }
}

/// The node pass reads every state from the packed copy, which must hold what the states say after each tick.
#[test]
fn the_packed_states_follow_the_states() {
    let Some(ctx) = context() else {
        return;
    };
    // An odd count, so the last word is only partly used.
    let mut state = build(&ctx, &[busy(false), vec![("num_agents", U32(6_007))]].concat());
    for tick in 0..40 {
        assert_eq!(state.read_state_bits(), state.read_states(), "tick {tick}");
        run(&ctx, &mut state, 1);
    }
}

/// Every node is counted in exactly one state, and the counts match the states they were read from.
#[test]
fn encoding_matches_the_models_own_stats() {
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(&ctx, &busy(false));
    for tick in 0..60 {
        let counted = counts(&ctx, &mut state);
        let cells = states(&state);
        let tally =
            [SUSCEPTIBLE, INFECTED, RESISTANT].map(|value| cells.iter().filter(|&&c| c == value).count() as u64);
        assert_eq!(counted, tally, "tick {tick}");
        assert_eq!(counted.iter().sum::<u64>(), cells.len() as u64, "tick {tick}");
        run(&ctx, &mut state, 1);
    }
}

/// After a publish, each node shows its state's colour and each edge touching a resistant node is greyed.
#[test]
fn a_publish_paints_nodes_and_edges_from_the_states() {
    let Some(ctx) = context() else {
        return;
    };
    let node_palette = PALETTE.map(u32::from_le_bytes);
    let edge_palette = EDGE_PALETTE.map(u32::from_le_bytes);
    for directed in [false, true] {
        let mut state = build(
            &ctx,
            &[
                busy_rewiring(directed, true),
                vec![("gain_resistance_chance", F32(0.5))],
            ]
            .concat(),
        );
        run(&ctx, &mut state, 80);
        counts(&ctx, &mut state);

        let cells = states(&state);
        assert!(
            cells.contains(&RESISTANT),
            "directed={directed}: no node turned resistant, so no edge could be greyed"
        );
        let expected: Vec<u32> = cells.iter().map(|&s| node_palette[usize::from(s)]).collect();
        assert_eq!(state.read_node_colors(), expected, "directed={directed}: node colours");
        for (e, [source, dest, color]) in state.read_edges().into_iter().enumerate() {
            let wanted = edge_color(cells[source as usize], cells[dest as usize]);
            assert_eq!(
                color,
                edge_palette[usize::from(wanted)],
                "directed={directed}: edge {e} from {source} to {dest}"
            );
        }
    }
}

#[test]
fn a_zero_spread_chance_infects_nobody() {
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(&ctx, &[busy(false), vec![("virus_spread_chance", F32(0.0))]].concat());
    let mut before = states(&state);
    for tick in 0..30 {
        run(&ctx, &mut state, 1);
        let after = states(&state);
        for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
            assert!(
                b != SUSCEPTIBLE || a == SUSCEPTIBLE,
                "node {i} caught it with no spread on tick {tick}"
            );
        }
        before = after;
    }
}

#[test]
fn a_zero_recovery_chance_keeps_every_infection() {
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(&ctx, &[busy(false), vec![("recovery_chance", F32(0.0))]].concat());
    let mut before = states(&state);
    for tick in 0..30 {
        run(&ctx, &mut state, 1);
        let after = states(&state);
        for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
            assert!(b != INFECTED || a == INFECTED, "node {i} recovered on tick {tick}");
            assert_ne!(a, RESISTANT, "node {i} became resistant on tick {tick}");
        }
        before = after;
    }
}

/// Checks `P(S -> I | k infected sources) = 1 - (1 - c)^k`, with the edges read both as directed and as undirected.
#[test]
fn infection_rate_matches_the_closed_form() {
    const SPREAD: f64 = 0.10;
    let Some(ctx) = context() else {
        return;
    };
    for directed in [false, true] {
        let mut state = build(
            &ctx,
            &[
                ("num_agents", U32(NODES)),
                ("initial_outbreak_size", U32(OUTBREAK)),
                ("virus_spread_chance", F32(SPREAD as f32)),
                ("recovery_chance", F32(0.0)),
                ("directed", Bool(directed)),
            ],
        );
        let before = states(&state);
        let sources = infected_sources(&state.read_edges(), &before, directed);
        run(&ctx, &mut state, 1);
        let after = states(&state);

        let (trials, infections) = by_sources(&before, &sources, |i| after[i] == INFECTED);
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

/// With certain spread and no recovery, a susceptible node is infected exactly when the edge list gives it an infected
/// source. A row missing or gaining an entry shows up here as a single node.
#[test]
fn infection_follows_the_edge_list_exactly() {
    let Some(ctx) = context() else {
        return;
    };
    for directed in [false, true] {
        let mut state = build(
            &ctx,
            &[
                ("num_agents", U32(NODES)),
                ("initial_outbreak_size", U32(NODES / 50)),
                ("virus_spread_chance", F32(1.0)),
                ("recovery_chance", F32(0.0)),
                ("directed", Bool(directed)),
            ],
        );
        let before = states(&state);
        let sources = infected_sources(&state.read_edges(), &before, directed);
        run(&ctx, &mut state, 1);
        let after = states(&state);
        for i in 0..before.len() {
            if before[i] == SUSCEPTIBLE {
                assert_eq!(
                    after[i] == INFECTED,
                    sources[i] > 0,
                    "directed={directed}: node {i} with {} infected sources",
                    sources[i]
                );
            }
        }
    }
}

/// A checked node recovers with the recovery chance, and a recovered node becomes resistant with the resistance chance.
#[test]
fn recovery_and_resistance_match_the_parameters() {
    const RECOVERY: f64 = 0.30;
    const RESISTANCE: f64 = 0.40;
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(
        &ctx,
        &[
            ("num_agents", U32(NODES)),
            ("initial_outbreak_size", U32(OUTBREAK)),
            ("virus_spread_chance", F32(0.0)),
            ("recovery_chance", F32(RECOVERY as f32)),
            ("gain_resistance_chance", F32(RESISTANCE as f32)),
        ],
    );
    let before = states(&state);
    run(&ctx, &mut state, 1);
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
/// With a check every tick and certain resistance, `P(S -> R | k) = (1 - 0.9^k) 0.3`. A single draw shared between
/// infection and recovery would make that zero.
#[test]
fn a_node_infected_this_tick_can_recover_this_tick() {
    const SPREAD: f64 = 0.1;
    const RECOVERY: f64 = 0.3;
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(
        &ctx,
        &[
            ("num_agents", U32(NODES)),
            ("initial_outbreak_size", U32(OUTBREAK)),
            ("virus_spread_chance", F32(SPREAD as f32)),
            ("virus_check_frequency", U32(1)),
            ("recovery_chance", F32(RECOVERY as f32)),
            ("gain_resistance_chance", F32(1.0)),
        ],
    );
    let before = states(&state);
    let sources = infected_sources(&state.read_edges(), &before, false);
    run(&ctx, &mut state, 1);
    let after = states(&state);

    let (trials, recoveries) = by_sources(&before, &sources, |i| after[i] == RESISTANT);
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
#[test]
fn each_node_is_checked_once_per_cycle() {
    const EVERY: u32 = 4;
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(
        &ctx,
        &[
            ("num_agents", U32(NODES)),
            ("initial_outbreak_size", U32(OUTBREAK)),
            ("virus_spread_chance", F32(0.0)),
            ("virus_check_frequency", U32(EVERY)),
            ("recovery_chance", F32(1.0)),
            ("gain_resistance_chance", F32(1.0)),
        ],
    );
    for t in 1..=EVERY {
        run(&ctx, &mut state, 1);
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
/// The CPU suite flips the direction mid-run. Here `directed` is reload-only, so each direction gets a run of its own.
#[test]
fn only_the_declared_transitions_happen() {
    let Some(ctx) = context() else {
        return;
    };
    for directed in [false, true] {
        let mut state = build(
            &ctx,
            &[
                ("num_agents", U32(20_000)),
                ("initial_outbreak_size", U32(2_000)),
                ("virus_spread_chance", F32(0.1)),
                ("recovery_chance", F32(0.1)),
                ("gain_resistance_chance", F32(0.3)),
                ("virus_check_frequency", U32(2)),
                ("directed", Bool(directed)),
            ],
        );
        let edges = state.read_edges();
        let mut before = states(&state);
        for tick in 0..100 {
            let sources = infected_sources(&edges, &before, directed);
            run(&ctx, &mut state, 1);
            let after = states(&state);
            for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
                assert!(
                    b != RESISTANT || a == RESISTANT,
                    "directed={directed}: node {i} lost its resistance on tick {tick}"
                );
                assert!(
                    b != SUSCEPTIBLE || a == SUSCEPTIBLE || sources[i] > 0,
                    "directed={directed}: node {i} went from susceptible to {a} with no infected source on tick {tick}"
                );
            }
            before = after;
        }
    }
}

/// Three nodes with edges `0 -> 1` and `2 -> 0`, where node 0 is infected and spread is certain.
/// When directed, only node 1 catches the virus. When undirected, both nodes do.
#[test]
fn directed_infection_follows_the_edges() {
    let Some(ctx) = context() else {
        return;
    };
    let run_graph = |directed: bool| {
        let values = params(&[
            ("num_agents", U32(3)),
            ("average_node_degree", U32(1)),
            ("initial_outbreak_size", U32(1)),
            ("virus_spread_chance", F32(1.0)),
            ("recovery_chance", F32(0.0)),
            ("directed", Bool(directed)),
        ]);
        let mut state = GpuVirusNetwork::from_graph(&ctx, &values, Some(SEED), |nodes, _extent| {
            clear_edges(nodes);
            nodes.graph.add_edge(0, 1, 0);
            nodes.graph.add_edge(2, 0, 0);
            nodes.lanes.state.fill(SUSCEPTIBLE);
            nodes.lanes.state[0] = INFECTED;
        });
        run(&ctx, &mut state, 1);
        states(&state)
    };
    assert_eq!(run_graph(true), [INFECTED, INFECTED, SUSCEPTIBLE], "directed");
    assert_eq!(run_graph(false), [INFECTED, INFECTED, INFECTED], "undirected");
}

fn clear_edges(nodes: &mut Nodes<'_, VirusNetwork>) {
    while nodes.graph.edge_count() > 0 {
        nodes.graph.remove_edge(0);
    }
}

/// A single node, and two nodes with no edge between them, build, step and rewire with empty rows.
#[test]
fn a_graph_without_edges_steps_and_rewires() {
    let Some(ctx) = context() else {
        return;
    };
    let mut single = build(
        &ctx,
        &[
            ("num_agents", U32(1)),
            ("initial_outbreak_size", U32(1)),
            ("keep_rewiring", Bool(true)),
        ],
    );
    run(&ctx, &mut single, 10);
    assert!(single.act(0), "Rewire a link was refused");
    assert_eq!(counts(&ctx, &mut single).iter().sum::<u64>(), 1);

    let values = params(&[
        ("num_agents", U32(2)),
        ("network", Choice(GEOMETRIC)),
        ("keep_rewiring", Bool(true)),
    ]);
    let mut pair = GpuVirusNetwork::from_graph(&ctx, &values, Some(SEED), |nodes, _extent| clear_edges(nodes));
    assert!(pair.read_edges().is_empty());
    run(&ctx, &mut pair, 10);
    assert!(pair.act(0), "Rewire a link was refused");
    assert!(pair.read_edges().is_empty(), "a rewire made an edge from nothing");
    assert_eq!(counts(&ctx, &mut pair).iter().sum::<u64>(), 2);
}

/// Returns the unordered pairs in the edge list, after checking that none repeats and none is a loop.
fn simple_pairs(edges: &[[u32; 3]]) -> HashSet<(u32, u32)> {
    let mut pairs = HashSet::new();
    for &[a, b, _] in edges {
        assert_ne!(a, b, "a self loop");
        assert!(pairs.insert((a.min(b), a.max(b))), "{a} and {b} are joined twice");
    }
    pairs
}

/// Compares every row with the rows the edge list implies, as multisets.
fn assert_rows_follow_the_edge_list(state: &GpuVirusNetwork, directed: bool, what: &str) {
    let (row_start, entries) = state.read_rows();
    let nodes = (row_start.len() - 1) / 2;
    let mut expected = vec![Vec::new(); 2 * nodes];
    for [source, dest, _] in state.read_edges() {
        expected[2 * dest as usize].push(source);
        expected[2 * source as usize + usize::from(directed)].push(dest);
    }
    for (k, wanted) in expected.iter().enumerate() {
        let row = &entries[row_start[k] as usize..row_start[k + 1] as usize];
        assert_eq!(sorted(row), sorted(wanted), "{what}: row {k}");
    }
}

#[test]
fn rewiring_keeps_the_edge_count_and_the_graph_simple() {
    let Some(ctx) = context() else {
        return;
    };
    for directed in [false, true] {
        let mut state = build(
            &ctx,
            &[
                ("num_agents", U32(2_000)),
                ("directed", Bool(directed)),
                ("keep_rewiring", Bool(true)),
            ],
        );
        let before = simple_pairs(&state.read_edges());
        let edges = before.len();

        for _ in 0..3_000 {
            assert!(state.act(0), "Rewire a link was refused");
        }
        run(&ctx, &mut state, 200);
        let listed = state.read_edges();
        assert_eq!(listed.len(), edges, "directed={directed}: rewiring changed the count");
        assert_rows_follow_the_edge_list(&state, directed, &format!("directed={directed}"));

        // Each rewire moves a uniformly chosen edge, so an original edge survives with probability (1 - 1/m)^3200.
        let after = simple_pairs(&listed);
        let kept = before.intersection(&after).count() as f64 / edges as f64;
        let expected = (1.0 - 1.0 / edges as f64).powi(3_200);
        assert!(
            (kept - expected).abs() < 0.05,
            "directed={directed}: {kept} of the edges stayed put, expected about {expected}"
        );
    }
}

/// One press removes one pair and adds one the graph did not have.
#[test]
fn a_press_moves_exactly_one_edge() {
    let Some(ctx) = context() else {
        return;
    };
    for directed in [false, true] {
        let mut state = build(&ctx, &[("num_agents", U32(2_000)), ("directed", Bool(directed))]);
        for press in 0..20 {
            let what = format!("directed={directed} press {press}");
            let listed_before = state.read_edges();
            assert!(state.act(0), "Rewire a link was refused");
            let listed_after = state.read_edges();
            let (before, after) = (simple_pairs(&listed_before), simple_pairs(&listed_after));
            assert_eq!(
                before.difference(&after).count(),
                1,
                "{what}: removed other than one pair"
            );
            assert_eq!(
                after.difference(&before).count(),
                1,
                "{what}: added other than one pair"
            );
            assert_rows_follow_the_edge_list(&state, directed, &what);

            // The CPU's order, `remove_edge` then `add_edge`: the last edge fills the removed slot and the new edge
            // goes last.
            let pair = |[a, b, _]: [u32; 3]| (a.min(b), a.max(b));
            let last = listed_before.len() - 1;
            let removed = (0..listed_before.len())
                .find(|&e| !after.contains(&pair(listed_before[e])))
                .expect("one pair was removed");
            assert!(
                !before.contains(&pair(listed_after[last])),
                "{what}: the new edge is not last"
            );
            for e in 0..last {
                let wanted = if e == removed {
                    listed_before[last]
                } else {
                    listed_before[e]
                };
                assert_eq!(pair(listed_after[e]), pair(wanted), "{what}: edge {e} moved");
            }
        }
    }
}

/// The action's RNG word lives on the GPU, so two presses in one encoder draw twice, as two submissions do.
#[test]
fn two_presses_in_one_encoder_match_two_submissions() {
    let Some(ctx) = context() else {
        return;
    };
    let mut apart = build(&ctx, &busy(false));
    let mut together = build(&ctx, &busy(false));
    assert!(apart.act(0) && apart.act(0));

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    assert!(together.encode_action(&mut encoder, 0) && together.encode_action(&mut encoder, 0));
    ctx.queue.submit(Some(encoder.finish()));

    assert_eq!(outcome(&apart), outcome(&together));
    assert_ne!(
        outcome(&apart).2,
        outcome(&build(&ctx, &busy(false))).2,
        "the presses moved nothing"
    );
    assert!(!apart.act(1), "an action past the one declared was accepted");
}

/// Rewiring adds a rewire and a rows rebuild to every step, and a full submission of them must still run.
#[test]
fn a_full_submission_of_rewiring_steps_executes() {
    let Some(ctx) = context() else {
        return;
    };
    // A `2n + 1` row table past 256² entries needs three scan levels, the longest rebuild.
    let nodes = 50_000;
    let mut state = build(
        &ctx,
        &[
            ("num_agents", U32(nodes)),
            ("initial_outbreak_size", U32(nodes / 10)),
            ("keep_rewiring", Bool(true)),
        ],
    );
    let before = simple_pairs(&state.read_edges());
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_steps(&mut encoder, MAX_STEPS_PER_SUBMISSION, None);
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);

    let total: f64 = state.stats().iter().map(|stat| stat.value.scalar()).sum();
    assert_eq!(
        total,
        f64::from(nodes),
        "the submission was dropped, or a node went uncounted"
    );
    let moved = before.difference(&simple_pairs(&state.read_edges())).count();
    assert!(
        moved > (MAX_STEPS_PER_SUBMISSION / 2) as usize,
        "only {moved} edges moved over {MAX_STEPS_PER_SUBMISSION} rewiring steps"
    );
    assert_rows_follow_the_edge_list(&state, false, "after a full submission");
}

/// Each rewire draws from a word of its own, and advances only that word.
#[test]
fn each_rewire_advances_only_its_own_stream() {
    let Some(ctx) = context() else {
        return;
    };
    let mut state = build(&ctx, &busy_rewiring(false, true));
    let [tick, action] = state.read_streams();
    assert_ne!(tick, action, "the two streams start on the same word");

    assert!(state.act(0));
    let [tick_after_press, action_after_press] = state.read_streams();
    assert_eq!(tick_after_press, tick, "a press drew from the tick's stream");
    assert_ne!(action_after_press, action, "a press left its own stream where it was");

    run(&ctx, &mut state, 1);
    let [tick_after_step, action_after_step] = state.read_streams();
    assert_ne!(tick_after_step, tick, "a rewiring tick left its stream where it was");
    assert_eq!(
        action_after_step, action_after_press,
        "a tick drew from the action's stream"
    );
}
