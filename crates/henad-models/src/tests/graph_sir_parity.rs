//! The SIR graphs held to the stock `gpu_sir` on one device.
//!
//! `gpu_sir_exact.toml` advances a per-cell `rng` lane as the stock step does, so it equals `gpu_sir` in cells, `rng`
//! words and counts every tick and press by press. The starter `sir.toml` draws counter-based numbers instead. It
//! equals `gpu_sir` at tick 0, and every tick under the rate gates, the rates of 0 and 1 that leave no decision to a
//! draw. Past those it is held to the bands of `tests/consistency_sir.rs`.

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::params::ParamValue;
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};

use super::graph_grid_parity::{build_gpu, press, snapshot, starter_entry, stat, step};
use super::lanes::GraphLanes;

/// Labels of the SIR stats, in the order both models declare them.
const LABELS: [&str; 3] = ["Susceptible", "Infected", "Recovered"];

/// Returns the text of the fixture `tests/fixtures/graphs/<file_name>`.
pub(super) fn fixture_text(file_name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/graphs")
        .join(file_name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Returns the entry the fixture `tests/fixtures/graphs/<file_name>` builds.
pub(super) fn fixture_entry(file_name: &str) -> ModelEntry {
    ModelFile::parse(&fixture_text(file_name))
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{file_name}: {error}"))
}

/// Returns the parameters of a SIR model at `width` by `height` cells.
fn sir_params(width: u32, height: u32, infection_rate: f32, recovery_rate: f32, initial: f32) -> [ParamValue; 5] {
    [
        ParamValue::U32(width),
        ParamValue::U32(height),
        ParamValue::F32(infection_rate),
        ParamValue::F32(recovery_rate),
        ParamValue::F32(initial),
    ]
}

/// Returns the words of buffer `index` of `state`.
fn words(state: &dyn GpuSimState, index: usize) -> Vec<u32> {
    state.read_buffer(index).expect("a buffer of the state")
}

/// Holds `graph` and `stock` equal in cells and in each SIR count, and in their `rng` buffers when `rng` holds.
fn assert_sir_equal(
    ctx: &GpuContext,
    lanes: &GraphLanes,
    graph: &mut dyn GpuSimState,
    stock: &mut dyn GpuSimState,
    rng: bool,
    at: &str,
) {
    let cells = words(stock, 0);
    assert!(lanes.read(graph, "health") == cells, "the cells differ {at}");
    if rng {
        assert!(lanes.read(graph, "rng") == words(stock, 1), "the rng words differ {at}");
    }
    snapshot(ctx, graph);
    snapshot(ctx, stock);
    let (graph_stats, stock_stats) = (graph.stats(), stock.stats());
    for (state, label) in LABELS.iter().enumerate() {
        let expected = stat(&stock_stats, label);
        assert_eq!(stat(&graph_stats, label), expected, "{label} {at}");
        let counted = cells.iter().filter(|&&cell| cell as usize == state).count();
        assert_eq!(expected, counted as f64, "the stock {label} count {at}");
    }
}

/// `gpu_sir_exact.toml` equals the stock `gpu_sir` every tick in cells, `rng` words and counts: at an odd size and a
/// size of whole workgroups, at the defaults and at a high initial share, over single steps, a batch of 64 steps in
/// one submission, and presses of Seed outbreak between steps.
#[test]
fn the_exact_sir_graph_equals_gpu_sir_every_tick() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_exact_sir_graph_equals_gpu_sir_every_tick: no adapter");
        return;
    };
    let graph_entry = fixture_entry("gpu_sir_exact.toml");
    let lanes = GraphLanes::of_text(&fixture_text("gpu_sir_exact.toml"));
    let models = crate::example_models();
    let stock_entry = models.get("gpu_sir").expect("the stock model");
    let runs = [
        ((37, 23), (0.3, 0.05, 0.2), Some(3)),
        ((128, 96), (0.3, 0.05, 0.01), None),
        ((64, 48), (0.8, 0.2, 0.05), Some(u64::MAX)),
    ];
    for ((width, height), (infection_rate, recovery_rate, initial), seed) in runs {
        let params = sir_params(width, height, infection_rate, recovery_rate, initial);
        let mut graph = build_gpu(&graph_entry, &params, seed, &ctx);
        let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
        let size = format!("{width}x{height} at seed {seed:?}");
        for tick in 0..=100u64 {
            assert_eq!(graph.tick(), tick);
            let at = format!("at tick {tick} of {size}");
            assert_sir_equal(&ctx, &lanes, &mut *graph, &mut *stock, true, &at);
            step(&ctx, &mut *graph, 1);
            step(&ctx, &mut *stock, 1);
        }
        step(&ctx, &mut *graph, 64);
        step(&ctx, &mut *stock, 64);
        let at = format!("after a batch of 64 steps of {size}");
        assert_sir_equal(&ctx, &lanes, &mut *graph, &mut *stock, true, &at);
        for k in 1..=3 {
            press(&ctx, &mut *graph, 0);
            press(&ctx, &mut *stock, 0);
            let at = format!("after press {k} of {size}");
            assert_sir_equal(&ctx, &lanes, &mut *graph, &mut *stock, true, &at);
            step(&ctx, &mut *graph, 5);
            step(&ctx, &mut *stock, 5);
            assert_sir_equal(&ctx, &lanes, &mut *graph, &mut *stock, true, &format!("5 steps {at}"));
        }
    }
}

/// The starter `sir.toml` equals the stock `gpu_sir` at tick 0 at any rates, and every tick and press under each rate
/// gate: an infection rate of 1, which infects every susceptible cell with an infected neighbour, and of 0, which
/// infects none, each with a recovery rate of 0 or 1.
#[test]
fn the_sir_graph_equals_gpu_sir_at_tick_0_and_under_the_rate_gates() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_sir_graph_equals_gpu_sir_at_tick_0_and_under_the_rate_gates: no adapter");
        return;
    };
    let graph_entry = starter_entry("sir.toml");
    let lanes = GraphLanes::of_starter("sir.toml");
    let models = crate::example_models();
    let stock_entry = models.get("gpu_sir").expect("the stock model");
    let (width, height) = (45u32, 30u32);
    for seed in [Some(0), None, Some(9)] {
        let params = sir_params(width, height, 0.3, 0.05, 0.2);
        let mut graph = build_gpu(&graph_entry, &params, seed, &ctx);
        let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
        assert_sir_equal(
            &ctx,
            &lanes,
            &mut *graph,
            &mut *stock,
            false,
            &format!("at tick 0 at seed {seed:?}"),
        );
    }
    for (infection_rate, recovery_rate) in [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)] {
        for seed in [Some(4), None] {
            let params = sir_params(width, height, infection_rate, recovery_rate, 0.03);
            let mut graph = build_gpu(&graph_entry, &params, seed, &ctx);
            let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
            let gate = format!("infection {infection_rate}, recovery {recovery_rate}, seed {seed:?}");
            for tick in 0..=40u64 {
                let at = format!("at tick {tick} under {gate}");
                assert_sir_equal(&ctx, &lanes, &mut *graph, &mut *stock, false, &at);
                if tick % 10 == 5 {
                    press(&ctx, &mut *graph, 0);
                    press(&ctx, &mut *stock, 0);
                    assert_sir_equal(
                        &ctx,
                        &lanes,
                        &mut *graph,
                        &mut *stock,
                        false,
                        &format!("after a press {at}"),
                    );
                }
                step(&ctx, &mut *graph, 1);
                step(&ctx, &mut *stock, 1);
            }
        }
    }
}

// The bands of `tests/consistency_sir.rs`, at its size, rates and seed.
const SIDE: usize = 1024;
const BETA: f64 = 0.3;
const GAMMA: f64 = 0.05;
const INITIAL_INFECTED: f64 = 0.5;
const BAND_SEED: u64 = 0x5152_0BEE_5EED_0001;
/// Buckets thinner than this are skipped, as in the consistency test.
const MIN_TRIALS: u64 = 1_000;
/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
const SIGMAS: f64 = 5.0;

// Cell states, as the stock model and the starter's category order them.
const S: u32 = 0;
const I: u32 = 1;
const R: u32 = 2;

/// Returns the number of infected neighbours of `(x, y)` in a Moore neighbourhood, wrapping toroidally.
fn infected_neighbours(grid: &[u32], x: usize, y: usize) -> usize {
    let mut count = 0;
    for dy in [SIDE - 1, 0, 1] {
        for dx in [SIDE - 1, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }
            count += usize::from(grid[((y + dy) % SIDE) * SIDE + (x + dx) % SIDE] == I);
        }
    }
    count
}

/// Returns the half-width of the acceptance band for a binomial proportion `expected` over `trials` trials.
fn band(expected: f64, trials: u64) -> f64 {
    SIGMAS * (expected * (1.0 - expected) / trials as f64).sqrt()
}

/// The starter `sir.toml` on the device meets the bands of `tests/consistency_sir.rs` over the step from tick 0, at
/// the consistency test's seed and one more: `P(S -> I | k infected neighbours) = 1 - (1 - beta)^k` for every bucket
/// of at least 1000 cells, no infection without an infected neighbour, and `P(I -> R) = gamma`. Over five ticks the
/// cells move only from S to I to R, and the population is conserved.
#[test]
fn the_sir_graph_meets_the_sir_bands() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_sir_graph_meets_the_sir_bands: no adapter");
        return;
    };
    let entry = starter_entry("sir.toml");
    let lanes = GraphLanes::of_starter("sir.toml");
    let side = SIDE as u32;
    let params = sir_params(side, side, BETA as f32, GAMMA as f32, INITIAL_INFECTED as f32);
    for seed in [BAND_SEED, BAND_SEED ^ 1] {
        let mut state = build_gpu(&entry, &params, Some(seed), &ctx);
        let mut before = lanes.read(&*state, "health");
        for tick in 0..5 {
            step(&ctx, &mut *state, 1);
            let after = lanes.read(&*state, "health");
            for (index, (&was, &now)) in before.iter().zip(&after).enumerate() {
                assert!(
                    matches!((was, now), (S, S | I) | (I, I | R) | (R, R)),
                    "cell {index} went from {was} to {now} on tick {tick} at seed {seed:#x}"
                );
            }
            snapshot(&ctx, &mut *state);
            let total: f64 = LABELS.iter().map(|label| stat(&state.stats(), label)).sum();
            assert_eq!(total, (SIDE * SIDE) as f64, "the population after tick {tick}");
            if tick == 0 {
                check_rates(&before, &after, seed);
            }
            before = after;
        }
    }
}

/// Holds the step from `before` to `after` to the infection and recovery bands.
fn check_rates(before: &[u32], after: &[u32], seed: u64) {
    let mut trials = [0u64; 9];
    let mut infections = [0u64; 9];
    let (mut infected, mut recoveries) = (0u64, 0u64);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let index = y * SIDE + x;
            match before[index] {
                S => {
                    let k = infected_neighbours(before, x, y);
                    trials[k] += 1;
                    infections[k] += u64::from(after[index] == I);
                }
                I => {
                    infected += 1;
                    recoveries += u64::from(after[index] == R);
                }
                _ => {}
            }
        }
    }
    let mut checked = 0;
    for (k, (&count, &hits)) in trials.iter().zip(&infections).enumerate() {
        if count < MIN_TRIALS {
            continue;
        }
        let expected = 1.0 - (1.0 - BETA).powi(k as i32);
        let observed = hits as f64 / count as f64;
        let allowed = band(expected, count);
        assert!(
            (observed - expected).abs() <= allowed,
            "seed {seed:#x}, k={k}: observed {observed:.6}, expected {expected:.6}, allowed {allowed:.6} over {count} cells"
        );
        checked += 1;
    }
    assert_eq!(
        infections[0], 0,
        "seed {seed:#x}: a cell with no infected neighbour got infected"
    );
    assert!(
        checked >= 8,
        "seed {seed:#x}: only {checked} buckets had enough samples to check"
    );
    assert!(
        infected >= MIN_TRIALS,
        "seed {seed:#x}: only {infected} infected cells to sample"
    );
    let observed = recoveries as f64 / infected as f64;
    let allowed = band(GAMMA, infected);
    assert!(
        (observed - GAMMA).abs() <= allowed,
        "seed {seed:#x}: recovery rate {observed:.6}, expected {GAMMA:.6}, allowed {allowed:.6} over {infected} cells"
    );
}
