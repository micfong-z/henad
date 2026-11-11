//! The grid starters built from their model files, held to the stock models every tick: Game of Life to
//! `gpu_game_of_life` in cells, stats and every press of Randomise and Clear, and Vote to the template's Vote.

use henad_compute::cpu::grid_engine::GridModelState;
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;

use super::graph_corpus::vote::Vote;
use super::lanes::GraphLanes;
use super::support::{
    baseline_device, buffer_words, build_gpu, press, snapshot, starter_entry, stat, step, unpacked_life_cells,
};

/// Holds the graph and the stock Game of Life equal in cells and in the Alive count.
fn assert_life_equal(
    ctx: &GpuContext,
    lanes: &GraphLanes,
    graph: &mut dyn GpuSimState,
    stock: &mut dyn GpuSimState,
    size: (u32, u32),
    at: &str,
) {
    let (width, height) = size;
    let cells = unpacked_life_cells(&buffer_words(stock, 0), width, height);
    assert!(lanes.read(graph, "state") == cells, "the cells differ {at}");
    snapshot(ctx, graph);
    snapshot(ctx, stock);
    let alive = stat(&stock.stats(), "Alive");
    assert_eq!(stat(&graph.stats(), "Alive"), alive, "Alive {at}");
    assert_eq!(
        alive,
        cells.iter().map(|&cell| f64::from(cell)).sum::<f64>(),
        "the stock count {at}"
    );
}

/// The Game of Life starter equals the stock `gpu_game_of_life` every tick, in cells and the Alive count, at an odd
/// size that leaves a partly filled word in each packed row and at a size of whole words, and over a batch of 64
/// steps in one submission.
#[test]
fn the_game_of_life_graph_equals_gpu_game_of_life_every_tick() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let graph_entry = starter_entry("game_of_life.toml");
    let lanes = GraphLanes::of_starter("game_of_life.toml");
    let models = crate::example_models();
    let stock_entry = models.get("gpu_game_of_life").expect("the stock model");
    for ((width, height), density, seed) in [((37, 23), 0.3, Some(3)), ((128, 96), 0.42, None)] {
        let params = [
            ParamValue::U32(width),
            ParamValue::U32(height),
            ParamValue::F32(density),
        ];
        let mut graph = build_gpu(&graph_entry, &params, seed, &ctx);
        let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
        for tick in 0..=100u64 {
            assert_eq!(graph.tick(), tick);
            assert_life_equal(
                &ctx,
                &lanes,
                &mut *graph,
                &mut *stock,
                (width, height),
                &format!("at tick {tick} of {width}x{height}"),
            );
            step(&ctx, &mut *graph, 1);
            step(&ctx, &mut *stock, 1);
        }
        step(&ctx, &mut *graph, 64);
        step(&ctx, &mut *stock, 64);
        assert_life_equal(
            &ctx,
            &lanes,
            &mut *graph,
            &mut *stock,
            (width, height),
            "after a batch of 64 steps",
        );
    }
}

/// Randomise and Clear of the Game of Life starter equal the stock `gpu_game_of_life`'s press by press: each press,
/// at seed 0 and with no seed, between steps, leaves equal cells and counts, and two presses of Randomise draw apart.
#[test]
fn the_game_of_life_graph_actions_equal_gpu_game_of_life_press_by_press() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let graph_entry = starter_entry("game_of_life.toml");
    let lanes = GraphLanes::of_starter("game_of_life.toml");
    let models = crate::example_models();
    let stock_entry = models.get("gpu_game_of_life").expect("the stock model");
    let (width, height) = (45u32, 30u32);
    for seed in [Some(0), None, Some(u64::MAX)] {
        let params = [ParamValue::U32(width), ParamValue::U32(height), ParamValue::F32(0.25)];
        let mut graph = build_gpu(&graph_entry, &params, seed, &ctx);
        let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
        let mut randomised = Vec::new();
        // Randomise, Randomise, step, Clear, Randomise, step: presses of both actions share one count.
        for (k, action) in [0usize, 0, 1, 0, 0].into_iter().enumerate() {
            press(&ctx, &mut *graph, action);
            press(&ctx, &mut *stock, action);
            let at = format!("after press {} (action {action}) at seed {seed:?}", k + 1);
            assert_life_equal(&ctx, &lanes, &mut *graph, &mut *stock, (width, height), &at);
            if action == 0 {
                randomised.push(lanes.read(&*graph, "state"));
            }
            step(&ctx, &mut *graph, 3);
            step(&ctx, &mut *stock, 3);
            assert_life_equal(
                &ctx,
                &lanes,
                &mut *graph,
                &mut *stock,
                (width, height),
                &format!("3 steps {at}"),
            );
        }
        assert!(
            randomised.windows(2).all(|pair| pair[0] != pair[1]),
            "two presses drew alike"
        );
    }
}

/// The Vote starter equals the template's Vote every tick, in cells and the Ones count, which reaches a fixed point
/// once the majorities settle.
#[test]
fn the_vote_graph_equals_vote_every_tick() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let graph_entry = starter_entry("vote.toml");
    let lanes = GraphLanes::of_starter("vote.toml");
    for ((width, height), density, seed) in [((41, 29), 0.5, Some(5)), ((96, 64), 0.37, None)] {
        let params = [
            ParamValue::U32(width),
            ParamValue::U32(height),
            ParamValue::F32(density),
        ];
        let mut graph = build_gpu(&graph_entry, &params, seed, &ctx);
        let mut cpu = GridModelState::<Vote>::from_params_seeded(&params, seed);
        for tick in 0..=60u64 {
            let at = format!("at tick {tick} of {width}x{height}");
            let cells: Vec<u32> = cpu
                .grid_view()
                .map(|view| view.cells.iter().map(|&cell| u32::from(cell)).collect())
                .unwrap_or_default();
            assert!(lanes.read(&*graph, "vote") == cells, "the cells differ {at}");
            snapshot(&ctx, &mut *graph);
            assert_eq!(stat(&graph.stats(), "Ones"), stat(&cpu.stats(), "Ones"), "Ones {at}");
            step(&ctx, &mut *graph, 1);
            cpu.step();
        }
    }
}
