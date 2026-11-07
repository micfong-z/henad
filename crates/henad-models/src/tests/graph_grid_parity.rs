//! The grid starters built from their model files, held to the stock models every tick: Game of Life to
//! `gpu_game_of_life` in cells, stats and every press of Randomise and Clear, and Vote to the template's Vote.

use henad_compute::cpu::grid_engine::GridModelState;
use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, StatValue};
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};

use super::graph_corpus::vote::Vote;
use super::lanes::GraphLanes;
use crate::gpu_game_of_life::words_per_row;

/// Returns the entry the model file of the starter `file_name` builds.
pub(super) fn starter_entry(file_name: &str) -> ModelEntry {
    let starter = henad_graph::starters::starter(file_name).unwrap_or_else(|| panic!("no starter {file_name}"));
    ModelFile::parse(starter.text)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{file_name}: {error}"))
}

/// Builds `entry` on `ctx` as a GPU state.
pub(super) fn build_gpu(
    entry: &ModelEntry,
    params: &[ParamValue],
    seed: Option<u64>,
    ctx: &GpuContext,
) -> Box<dyn GpuSimState> {
    match entry.build(params, seed, Some(ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) => panic!("{} built on the CPU", entry.id()),
        Err(fault) => panic!("{} did not build: {fault}", entry.id()),
    }
}

/// Returns the scalar stat labelled `label`.
pub(super) fn stat(stats: &[StatEntry], label: &str) -> f64 {
    match stats.iter().find(|entry| entry.label() == label).map(StatEntry::value) {
        Some(StatValue::Scalar(value)) => *value,
        other => panic!("no scalar stat `{label}`: {other:?}"),
    }
}

/// Runs the display and reduce passes of `state` and waits for its stats.
pub(super) fn snapshot(ctx: &GpuContext, state: &mut dyn GpuSimState) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
}

/// Steps `state` `count` times in one submission.
pub(super) fn step(ctx: &GpuContext, state: &mut dyn GpuSimState, count: u32) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_steps(&mut encoder, count, None);
    ctx.queue.submit(Some(encoder.finish()));
}

/// Presses action `index` of `state` in a submission of its own.
pub(super) fn press(ctx: &GpuContext, state: &mut dyn GpuSimState, index: usize) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    assert!(state.encode_action(&mut encoder, index), "action {index}");
    ctx.queue.submit(Some(encoder.finish()));
}

/// Returns the cells of the stock `gpu_game_of_life`, one word per cell in row-major order, unpacked from its 32 cells
/// to a word.
fn stock_life_cells(state: &dyn GpuSimState, width: u32, height: u32) -> Vec<u32> {
    let words = state.read_buffer(0).expect("the state buffer");
    let stride = words_per_row(width);
    (0..height as usize)
        .flat_map(|y| (0..width as usize).map(move |x| (y, x)))
        .map(|(y, x)| (words[y * stride + x / 32] >> (x % 32)) & 1)
        .collect()
}

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
    let cells = stock_life_cells(stock, width, height);
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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_game_of_life_graph_equals_gpu_game_of_life_every_tick: no adapter");
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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_game_of_life_graph_actions_equal_gpu_game_of_life_press_by_press: no adapter");
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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_vote_graph_equals_vote_every_tick: no adapter");
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
