//! The boids graphs held to the stock `gpu_boids` on one device.
//!
//! The stock port's neighbour index orders the agents of a cell as the GPU's atomics resolve, so no two runs of it
//! replay, and neither do the graphs that fold over neighbours. `gpu_boids.toml` and the starter `boids.toml` are held
//! to the stock model at tick 0 in every lane, then step for step in their statistics within a tolerance, and in the
//! engine-owned fields of a sweep, the fields `interleaved_gpu_boids_runs_commit_in_plan_order` compares. Past that
//! they meet the stock port's own checks: every boid stays in the world, every speed in its band, and the reduced
//! average speed agrees with the lanes.
//!
//! The graphs' radius fold faults past 65536 visits, where the stock port walks on.
//! [`the_boids_graphs_walk_past_the_cap_only_where_the_budget_refuses_them`] documents the corner of the stock bounds
//! where a walk passes the cap, and the part of it the submission budget refuses before a build.

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_compute::entry::ModelEntry;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::{GpuAgentState, GpuContext, SUBMISSION_WORK_BUDGET};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::GpuAgentModel as _;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::spatial_hash::HashGrid;
use henad_core::view::StatValue;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};
use henad_graph::GraphFile;

use super::graph_ants_parity::{graph_buffer, params_for, planted_entry};
use super::graph_grid_parity::{build_gpu, snapshot, starter_entry, step};
use super::graph_sir_parity::fixture_entry;
use crate::boids::BoidsModel;
use crate::gpu_boids::GpuBoids;

/// Largest distance between a statistic of a graph and the stock port's over [`STEPS`] steps, relative to the larger of
/// the stock value and 1.
const STATS_TOLERANCE: f64 = 0.002;
/// Steps a comparison of statistics runs.
const STEPS: u32 = 8;

/// Returns the parameters of a boids model of `num_agents` agents in a square world of side `world`, the others at
/// their defaults.
fn boids_params(num_agents: u32, world: f32) -> Vec<ParamValue> {
    GpuBoids::param_descriptors()
        .iter()
        .map(|descriptor| match descriptor.id() {
            "num_agents" => ParamValue::U32(num_agents),
            "world_width" | "world_height" => ParamValue::F32(world),
            _ => descriptor.kind().default_value(),
        })
        .collect()
}

/// Returns the text of the fixture graph `file_name`.
fn graph_text(file_name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/graphs")
        .join(file_name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Returns the text of the starter `file_name`.
fn starter_text(file_name: &str) -> String {
    henad_graph::starters::starter(file_name)
        .unwrap_or_else(|| panic!("no starter {file_name}"))
        .text
        .to_owned()
}

/// Returns the entries of the two boids graphs, each with its name.
fn boids_graphs() -> [(&'static str, ModelEntry); 2] {
    [
        ("gpu_boids.toml", fixture_entry("gpu_boids.toml")),
        ("boids.toml", starter_entry("boids.toml")),
    ]
}

/// Returns the stat values of `state` as plain numbers, a vector as its two components.
fn stat_values(ctx: &GpuContext, state: &mut dyn GpuSimState) -> Vec<f64> {
    snapshot(ctx, state);
    state
        .stats()
        .iter()
        .flat_map(|stat| match stat.value() {
            StatValue::Scalar(value) => vec![*value],
            StatValue::Vector2D { x, y } => vec![*x, *y],
            StatValue::Histogram { .. } => panic!("a boids model reports no histogram"),
        })
        .collect()
}

/// Splits an interleaved `vec2<f32>` buffer into its two components.
fn split(words: &[u32]) -> (Vec<f32>, Vec<f32>) {
    let floats: Vec<f32> = words.iter().map(|&word| f32::from_bits(word)).collect();
    (
        floats.iter().step_by(2).copied().collect(),
        floats.iter().skip(1).step_by(2).copied().collect(),
    )
}

/// Each boids graph builds, seeds its position and velocity buffers as the stock port does, and steps.
#[test]
fn the_boids_graphs_seed_like_gpu_boids_and_step() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_boids_graphs_seed_like_gpu_boids_and_step: no adapter");
        return;
    };
    let models = crate::example_models();
    let stock_entry = models.get("gpu_boids").expect("the stock model");
    for (name, entry) in boids_graphs() {
        for seed in [None, Some(7)] {
            let params = boids_params(2_000, 400.0);
            let mut graph = build_gpu(&entry, &params, seed, &ctx);
            let stock = build_gpu(stock_entry, &params, seed, &ctx);
            for buffer in 0..2 {
                assert!(
                    graph.read_buffer(buffer) == stock.read_buffer(buffer),
                    "{name}: buffer {buffer} differs from gpu_boids at tick 0, seed {seed:?}"
                );
            }
            step(&ctx, &mut *graph, 3);
            let values = stat_values(&ctx, &mut *graph);
            assert!(values.iter().all(|value| value.is_finite()), "{name}: {values:?}");
            assert_eq!(graph.tick(), 3);
        }
    }
}

/// Each boids graph tracks the stock `gpu_boids` step for step: every statistic stays within [`STATS_TOLERANCE`] of the
/// stock port's over [`STEPS`] steps, at two seeds.
#[test]
fn the_boids_graphs_track_gpu_boids_in_their_statistics() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_boids_graphs_track_gpu_boids_in_their_statistics: no adapter");
        return;
    };
    let models = crate::example_models();
    let stock_entry = models.get("gpu_boids").expect("the stock model");
    for (name, entry) in boids_graphs() {
        for seed in [Some(7), Some(11)] {
            let params = boids_params(2_000, 200.0);
            let mut graph = build_gpu(&entry, &params, seed, &ctx);
            let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
            for tick in 0..=STEPS {
                if tick > 0 {
                    step(&ctx, &mut *graph, 1);
                    step(&ctx, &mut *stock, 1);
                }
                let (found, expected) = (stat_values(&ctx, &mut *graph), stat_values(&ctx, &mut *stock));
                assert_eq!(found.len(), expected.len(), "{name}");
                for (k, (found, expected)) in found.iter().zip(&expected).enumerate() {
                    let scale = expected.abs().max(1.0);
                    assert!(
                        (found - expected).abs() <= STATS_TOLERANCE * scale,
                        "{name}: stat value {k} is {found} at tick {tick}, seed {seed:?}, and gpu_boids reads {expected}"
                    );
                }
            }
        }
    }
}

/// Every boid of each graph stays in the toroidal world, every speed in the band of the parameters, and the reduced
/// average speed agrees with the velocity buffer, over a population past one workgroup row, as the stock port's tests
/// hold it.
#[test]
fn the_boids_graphs_keep_the_stock_ports_invariants() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_boids_graphs_keep_the_stock_ports_invariants: no adapter");
        return;
    };
    for (name, entry) in boids_graphs() {
        for (num_agents, world, steps) in [(4_000, 800.0f32, 60), (300_037, 4_000.0, 5)] {
            let params = boids_params(num_agents, world);
            let mut graph = build_gpu(&entry, &params, None, &ctx);
            step(&ctx, &mut *graph, steps);
            let values = stat_values(&ctx, &mut *graph);
            let (pos_x, pos_y) = split(&graph.read_buffer(0).expect("the position lane"));
            let (vel_x, vel_y) = split(&graph.read_buffer(1).expect("the velocity lane"));
            assert_eq!(pos_x.len(), num_agents as usize, "{name}");
            for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
                assert!(
                    (0.0..world).contains(&x) && (0.0..world).contains(&y),
                    "{name}: boid {i} left the world at ({x}, {y})"
                );
            }
            // `min_speed` and `max_speed` at their defaults.
            let (min_speed, max_speed) = (3.0f32, 15.0f32);
            for (i, (&vx, &vy)) in vel_x.iter().zip(&vel_y).enumerate() {
                let speed = vx.hypot(vy);
                assert!(
                    speed <= max_speed * 1.001 && speed >= min_speed * 0.999,
                    "{name}: boid {i} has speed {speed}, outside [{min_speed}, {max_speed}]"
                );
            }
            let reference = vel_x
                .iter()
                .zip(&vel_y)
                .map(|(&x, &y)| f64::from(x.hypot(y)))
                .sum::<f64>()
                / vel_x.len() as f64;
            assert!(
                (values[0] - reference).abs() <= 1e-3 * reference,
                "{name}: the reduced average speed {} disagrees with the lanes: {reference}",
                values[0]
            );
        }
    }
}

/// Returns the fields of a sweep's tables that the engine fixes whatever values the model computes: every field of
/// `runs.csv` up to the population, but the run key, and the run and tick of every row of `series.csv`.
///
/// The run key hashes the model's schema, which reads the model id, so a graph's `node_<model>` id keys apart from the
/// stock model.
fn engine_owned(files: &henad_explore::output::memory::SweepFiles) -> (Vec<Vec<String>>, Vec<(String, String)>) {
    let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).expect("a table is UTF-8");
    let rows = |bytes: &[u8]| -> Vec<Vec<String>> {
        text(bytes)
            .lines()
            .map(|line| line.split(',').map(str::to_owned).collect())
            .collect()
    };
    let runs = rows(&files.runs);
    let population = runs[0]
        .iter()
        .position(|name| name == "population")
        .expect("runs.csv has a population column");
    let key = runs[0]
        .iter()
        .position(|name| name == "run_key")
        .expect("runs.csv has a run key column");
    let runs = runs
        .iter()
        .map(|record| {
            record[..=population]
                .iter()
                .enumerate()
                .filter(|(column, _)| *column != key)
                .map(|(_, field)| field.clone())
                .collect()
        })
        .collect();
    let series = rows(&files.series)
        .iter()
        .map(|record| (record[0].clone(), record[1].clone()))
        .collect();
    (runs, series)
}

/// Runs the sweep of `interleaved_gpu_boids_runs_commit_in_plan_order` over `entry` in memory, on `tracks` GPU tracks.
fn boids_sweep(entry: &ModelEntry, ctx: &GpuContext, tracks: usize) -> henad_explore::output::memory::SweepFiles {
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_explore::exec::Concurrency;
    use henad_explore::handle::SweepOutput;
    use henad_explore::sweep::{Provenance, SweepOptions, run_spec};

    let mut spec = SweepSpec::new(entry.id());
    spec.fixed = [("num_agents", "2000"), ("world_width", "400"), ("world_height", "400")]
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect();
    spec.run.steps = 30;
    spec.run.replicates = 2;
    spec.measure.stats_every = 3;
    spec.measure.series_every = 3;
    spec.seeds.root = 9;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param(
            "separation",
            LevelSpec::Values(vec!["0.05".to_owned(), "0.5".to_owned()]),
        )],
        design_seed: None,
    }];
    henad_compute::fault::install_panic_hook();
    let mut options = SweepOptions::new(Provenance::new(
        henad_core::build_info!(),
        vec!["henad-models-tests".to_owned()],
    ));
    options.concurrency = Concurrency::Fixed(tracks.try_into().expect("a track count above 0"));
    let record = run_spec(
        entry,
        Some(ctx),
        &spec,
        SweepOutput::Memory,
        &options,
        &mut henad_explore::progress::NoProgress,
    )
    .unwrap_or_else(|error| panic!("{}: the sweep failed: {error}", entry.id()));
    assert_eq!(
        (record.report.counts.rows, record.report.counts.ok),
        (4, 4),
        "{}",
        entry.id()
    );
    record.files.expect("a sweep in memory returns its files")
}

/// `gpu_boids.toml` equals the stock `gpu_boids` in the engine-owned fields of a sweep, on one GPU track and on four:
/// run order, ids, seeds, parameters, statuses, ticks and sampled ticks.
#[test]
fn the_boids_graph_equals_gpu_boids_in_the_engine_owned_fields_of_a_sweep() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_boids_graph_equals_gpu_boids_in_the_engine_owned_fields_of_a_sweep: no adapter");
        return;
    };
    let models = crate::example_models();
    let stock_entry = models.get("gpu_boids").expect("the stock model");
    let stock = engine_owned(&boids_sweep(stock_entry, &ctx, 1));
    assert_eq!(
        stock.1.len(),
        1 + 4 * 11,
        "ticks 0 to 30 of every run, every third tick, and the header"
    );
    let graph_entry = fixture_entry("gpu_boids.toml");
    for tracks in [1, 4] {
        assert_eq!(
            engine_owned(&boids_sweep(&graph_entry, &ctx, tracks)),
            stock,
            "on {tracks} tracks"
        );
    }
}

/// Returns the most agents one walk of a radius fold of `radius` over an index of cells `cell_size` wide visits, over
/// every agent at the interleaved positions `pos` in a world of `extent`.
///
/// The walk is `henad::hash`'s: the cells within `ceil(radius / cell)` of the agent's cell on each axis, wrapped, or
/// the whole axis when that reach covers it. Each cell counts its agents before the distance test.
fn largest_walk(pos: &[u32], extent: Extent, radius: f32, cell_size: f32) -> u32 {
    let grid = HashGrid::new(extent, cell_size);
    let (width, height) = (grid.grid_w as i64, grid.grid_h as i64);
    let (inverse_w, inverse_h) = (1.0 / grid.cell_w, 1.0 / grid.cell_h);
    let mut counts = vec![0u32; (width * height) as usize];
    let cell_of = |x: f32, y: f32| {
        let column = i64::from((x * inverse_w).floor() as i32).rem_euclid(width);
        let row = i64::from((y * inverse_h).floor() as i32).rem_euclid(height);
        (column, row)
    };
    for xy in pos.chunks_exact(2) {
        let (column, row) = cell_of(f32::from_bits(xy[0]), f32::from_bits(xy[1]));
        counts[(row * width + column) as usize] += 1;
    }
    let reach = |side: i64, cell: f32| {
        let reach = i64::from((radius / cell).ceil() as i32);
        if 2 * reach + 1 > side { None } else { Some(reach) }
    };
    let (reach_x, reach_y) = (reach(width, grid.cell_w), reach(height, grid.cell_h));
    let span = |centre: i64, reach: Option<i64>, side: i64| match reach {
        Some(reach) => (centre - reach)..=(centre + reach),
        None => 0..=side - 1,
    };
    let mut largest = 0;
    for row in 0..height {
        for column in 0..width {
            if counts[(row * width + column) as usize] == 0 {
                continue;
            }
            let mut visits = 0;
            for walked_row in span(row, reach_y, height) {
                for walked_column in span(column, reach_x, width) {
                    let cell = walked_row.rem_euclid(height) * width + walked_column.rem_euclid(width);
                    visits += counts[cell as usize];
                }
            }
            largest = largest.max(visits);
        }
    }
    largest
}

/// Most agents a radius fold visits before it faults, `MAX_FOLD_CANDIDATES` of henad-graph.
const VISIT_CAP: u32 = 65_536;

/// The corner of the stock boids' bounds where the boids graphs' radius fold would fault, and the part of it the
/// submission budget refuses before a build. The bounds are kept: up to a million agents, `visual_range` up to 200, and
/// a world of 1 to 10000 a side.
///
/// The walk is a model on the CPU, not a device run. It counts, from the stock seed at tick 0, which the graphs share,
/// the agents in the 7 by 7 cells of a third of the visual range around each boid, which a uniform population puts at
/// about `num_agents * 49 * (visual_range / 3)^2 / (width * height)`. A million boids at a visual range of 200 walk past
/// the cap in a square world below about 1820 a side, and at the default range of 50 below about 460 a side. At the
/// default population of 50000 no world reaches the cap. Note that a flock gathers boids as it steps, so a run's walks
/// grow past these tick-0 values.
///
/// The submission budget charges each step the cells and the uniform-spread visits of every walk, at most the cap per
/// agent, and refuses a graph before it builds. It refuses both graphs in every row whose walk passes the cap, so none
/// of those configurations runs, and none of them faults. A million boids build only in a large world at the default
/// range, such as 10000 a side. In the default world at the default range, the budget admits about 195000 boids of
/// `gpu_boids.toml` and about 201000 of `boids.toml`. A run can still fault where the budget admits it, when a flock
/// gathers more boids in one walk than the uniform spread the charge assumes.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the walks and the charges are information for the record"
)]
fn the_boids_graphs_walk_past_the_cap_only_where_the_budget_refuses_them() {
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: u64::MAX,
        max_buffer_size: u64::MAX,
        max_storage_buffers_per_shader_stage: 16,
        ..wgpu::Limits::default()
    };
    let graphs = boids_graphs();
    // Each row reads: agents, visual range, world side, whether the longest walk passes the cap, and whether the
    // budget refuses `gpu_boids.toml` and `boids.toml`.
    for (num_agents, visual_range, side, past_cap, refused) in [
        (1_000_000, 200.0f32, 1_000.0f32, true, [true, true]),
        (1_000_000, 200.0, 1_500.0, true, [true, true]),
        (1_000_000, 200.0, 2_000.0, false, [true, true]),
        (1_000_000, 200.0, 10_000.0, false, [true, true]),
        (1_000_000, 50.0, 400.0, true, [true, true]),
        (1_000_000, 50.0, 500.0, false, [true, true]),
        (1_000_000, 50.0, 1_000.0, false, [true, true]),
        (1_000_000, 50.0, 10_000.0, false, [false, false]),
        (190_000, 50.0, 1_000.0, false, [false, false]),
        (200_000, 50.0, 1_000.0, false, [true, false]),
        (210_000, 50.0, 1_000.0, false, [true, true]),
        (100_000, 200.0, 1_000.0, false, [true, true]),
        (100_000, 200.0, 10_000.0, false, [false, false]),
        (50_000, 200.0, 1.0, false, [true, true]),
        (50_000, 200.0, 1_000.0, false, [true, false]),
        (50_000, 50.0, 1_000.0, false, [false, false]),
    ] {
        let params: Vec<ParamValue> = GpuBoids::param_descriptors()
            .iter()
            .map(|descriptor| match descriptor.id() {
                "num_agents" => ParamValue::U32(num_agents),
                "world_width" | "world_height" => ParamValue::F32(side),
                "visual_range" => ParamValue::F32(visual_range),
                _ => descriptor.kind().default_value(),
            })
            .collect();
        let geometry = GpuAgentState::<GpuBoids>::geometry_for(&params, &wgpu::Limits::default());
        let buffers = GpuBoids::seed_buffers(&geometry, &params, None);
        let pos: Vec<u32> = buffers[0]
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        let extent = Extent { w: side, h: side };
        let walk = largest_walk(&pos, extent, visual_range, visual_range / 3.0);
        let row = format!("{num_agents} boids, visual range {visual_range}, world {side}");
        for ((name, entry), refused) in graphs.iter().zip(refused) {
            let demand = entry.demand(&params, &limits).expect("a GPU entry has a demand");
            let submission = demand.submission_work();
            eprintln!("{row}, {name}: the longest walk visits {walk} agents, and a submission is charged {submission}");
            assert_eq!(
                submission > SUBMISSION_WORK_BUDGET,
                refused,
                "{row}, {name}: a submission is charged {submission}"
            );
            assert_eq!(
                entry.shortfalls(&params, &limits).is_empty(),
                !refused,
                "{row}, {name}: the shortfalls and the charge disagree"
            );
        }
        assert_eq!(
            walk > VISIT_CAP,
            past_cap,
            "{row}: the longest walk visits {walk} agents"
        );
    }
}

// The cross-engine gate of `tests/consistency_boids.rs`, copied since an integration test exports nothing. Its fixture
// document is `tests/fixtures/docs/boids_fixture.md`.

/// One boid as `(x, y, vx, vy)`.
type Boid = (f32, f32, f32, f32);

/// Boids of a scenario.
type Agents = &'static [Boid];

/// 8 boids with exact binary representations.
const BOIDS_8: Agents = &[
    (10.0, 10.0, 1.0, 0.0),
    (13.0, 10.0, 1.0, 0.0),
    (30.0, 30.0, 0.0, 2.0),
    (42.0, 30.0, 2.0, 0.0),
    (75.0, 75.0, 0.5, 0.5),
    (2.0, 50.0, -3.0, 0.0),
    (98.0, 50.0, 3.0, 0.0),
    (60.0, 10.0, 6.0, 6.0),
];

/// 42 boids sampled along two sine periods, quantised to exact binary representations (1/8).
#[rustfmt::skip]
const SINE_42: Agents = &[
    (0.0, 61.125, 2.0, 0.0),
    (2.25, 73.5, 2.375, 2.875),
    (4.625, 83.75, -1.25, 5.375),
    (6.875, 91.0, -6.5, 3.125),
    (9.125, 94.625, -8.125, -3.875),
    (11.375, 94.25, -2.375, -10.5),
    (13.75, 90.0, 1.25, -1.625),
    (16.0, 82.25, 3.75, 0.0),
    (18.25, 71.5, 3.375, 4.25),
    (20.625, 58.875, -1.625, 7.125),
    (22.875, 45.5, -8.125, 3.875),
    (25.125, 32.5, -9.625, -4.625),
    (27.375, 21.0, -0.5, -2.0),
    (29.75, 12.125, 2.375, -2.875),
    (32.0, 6.625, 5.5, 0.0),
    (34.25, 5.0, 4.5, 5.625),
    (36.625, 7.375, -2.0, 8.75),
    (38.875, 13.5, -9.625, 4.625),
    (41.125, 22.875, -1.75, -0.875),
    (43.375, 34.625, -0.875, -3.625),
    (45.75, 47.75, 3.375, -4.25),
    (48.0, 61.125, 7.25, 0.0),
    (50.25, 73.5, 5.625, 7.0),
    (52.625, 83.75, -2.375, 10.5),
    (54.875, 91.0, -1.75, 0.875),
    (57.125, 94.625, -3.375, -1.625),
    (59.375, 94.25, -1.25, -5.375),
    (61.75, 90.0, 4.5, -5.625),
    (64.0, 82.25, 9.0, 0.0),
    (66.25, 71.5, 6.75, 8.375),
    (68.625, 58.875, -0.5, 2.0),
    (70.875, 45.5, -3.375, 1.625),
    (73.125, 32.5, -5.0, -2.375),
    (75.375, 21.0, -1.625, -7.125),
    (77.75, 12.125, 5.625, -7.0),
    (80.0, 6.625, 10.75, 0.0),
    (82.25, 5.0, 1.25, 1.625),
    (84.625, 7.375, -0.875, 3.625),
    (86.875, 13.5, -5.0, 2.375),
    (89.125, 22.875, -6.5, -3.125),
    (91.375, 34.625, -2.0, -8.75),
    (93.75, 47.75, 6.75, -8.375),
];

/// Scenarios of the gate, by the name a fixture's `scenario` header starts with.
const SCENARIOS: &[(&str, Agents)] = &[("boids-8", BOIDS_8), ("sine-42", SINE_42)];

/// Side of the gate's world.
const GATE_WORLD: f32 = 100.0;
/// Speed bounds of the gate.
const GATE_SPEEDS: (f32, f32) = (2.0, 8.0);
/// Tolerance of the gate, as `consistency_boids.rs` sets it for an `f64` reference.
const GATE_TOLERANCE: f32 = 1e-5;

/// Returns the gate's parameters for `agents` boids, by id.
fn gate_values(agents: usize) -> Vec<(&'static str, ParamValue)> {
    vec![
        ("num_agents", ParamValue::U32(agents as u32)),
        ("world_width", ParamValue::F32(GATE_WORLD)),
        ("world_height", ParamValue::F32(GATE_WORLD)),
        ("visual_range", ParamValue::F32(20.0)),
        ("protected_range", ParamValue::F32(5.0)),
        ("separation", ParamValue::F32(0.5)),
        ("alignment", ParamValue::F32(0.25)),
        ("cohesion", ParamValue::F32(0.125)),
        ("max_speed", ParamValue::F32(GATE_SPEEDS.1)),
        ("min_speed", ParamValue::F32(GATE_SPEEDS.0)),
    ]
}

/// Runs the stock CPU boids one tick from `agents`, as `consistency_boids.rs` does.
fn cpu_boids(agents: Agents) -> Vec<Boid> {
    let models = crate::example_models();
    let entry = models.get("boids").expect("the stock CPU boids");
    let params = params_for(entry, &gate_values(agents.len()));
    let mut state = AgentModelState::<BoidsModel>::from_agents(&params, |lanes, _extent| {
        for (i, &(x, y, vx, vy)) in agents.iter().enumerate() {
            lanes.pos_x[i] = x;
            lanes.pos_y[i] = y;
            lanes.vel_x[i] = vx;
            lanes.vel_y[i] = vy;
        }
    });
    state.step();
    let lanes = state.lanes();
    (0..agents.len())
        .map(|i| (lanes.pos_x[i], lanes.pos_y[i], lanes.vel_x[i], lanes.vel_y[i]))
        .collect()
}

/// Runs the boids graph `file` one tick on the device from `agents`, planted in its position and velocity lanes.
fn graph_boids(ctx: &GpuContext, file: &GraphFile, agents: Agents) -> Vec<Boid> {
    let interleaved =
        |pair: fn(&Boid) -> [f32; 2]| -> Vec<u32> { agents.iter().flat_map(pair).map(f32::to_bits).collect() };
    let entry = planted_entry(
        file,
        vec![
            ("lane_pos_agents", interleaved(|agent| [agent.0, agent.1])),
            ("lane_vel_agents", interleaved(|agent| [agent.2, agent.3])),
        ],
    );
    let params = params_for(&entry, &gate_values(agents.len()));
    let mut state = build_gpu(&entry, &params, None, ctx);
    step(ctx, &mut *state, 1);
    let (pos_x, pos_y) = split(&graph_buffer(&entry, &*state, "lane_pos_agents"));
    let (vel_x, vel_y) = split(&graph_buffer(&entry, &*state, "lane_vel_agents"));
    (0..agents.len())
        .map(|i| (pos_x[i], pos_y[i], vel_x[i], vel_y[i]))
        .collect()
}

/// Returns each reference fixture of the boids gate as its file name, its scenario's name and its boids after the
/// declared tick.
fn boids_fixtures() -> Vec<(String, String, Vec<Boid>)> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/boids");
    let mut fixtures = Vec::new();
    for path in std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("a directory entry").path())
    {
        if path.extension().is_none_or(|extension| extension != "txt") {
            continue;
        }
        let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut scenario = None;
        let mut agents = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if let Some(rest) = line.strip_prefix('#') {
                if let Some(("scenario", value)) = rest.split_once(':').map(|(key, value)| (key.trim(), value)) {
                    scenario = value.split_whitespace().next().map(str::to_owned);
                }
                continue;
            }
            let values: Vec<f32> = line
                .split_whitespace()
                .map(|token| token.parse().unwrap_or_else(|_| panic!("{name}: `{token}`")))
                .collect();
            agents.push((values[0], values[1], values[2], values[3]));
        }
        fixtures.push((
            name.clone(),
            scenario.unwrap_or_else(|| panic!("{name} names no scenario")),
            agents,
        ));
    }
    fixtures
}

/// Returns whether two boids are within [`GATE_TOLERANCE`] in every component.
fn boids_close(found: &Boid, expected: &Boid) -> bool {
    [
        (found.0, expected.0),
        (found.1, expected.1),
        (found.2, expected.2),
        (found.3, expected.3),
    ]
    .iter()
    .all(|(a, b)| (a - b).abs() <= GATE_TOLERANCE)
}

/// Each boids graph on the device meets the cross-engine gate of `consistency_boids.rs`: one tick from each planted
/// scenario lands within the gate's tolerance of the stock CPU boids and of every engine's reference fixture.
#[test]
fn the_boids_graphs_meet_the_cross_engine_gate() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_boids_graphs_meet_the_cross_engine_gate: no adapter");
        return;
    };
    let fixtures = boids_fixtures();
    for (name, scenario) in SCENARIOS {
        assert!(
            fixtures.iter().any(|(_, known, _)| known == name),
            "the scenario {name} has no fixture"
        );
        let cpu = cpu_boids(scenario);
        for (graph, text) in [
            ("gpu_boids.toml", graph_text("gpu_boids.toml")),
            ("boids.toml", starter_text("boids.toml")),
        ] {
            let file = GraphFile::parse(&text).expect("the graph reads");
            let found = graph_boids(&ctx, &file, scenario);
            for (i, (found, expected)) in found.iter().zip(&cpu).enumerate() {
                assert!(
                    boids_close(found, expected),
                    "{graph}, {name}: boid {i} is {found:?} on the device and {expected:?} on the CPU"
                );
            }
            for (fixture, _, expected) in fixtures.iter().filter(|(_, known, _)| known == name) {
                assert_eq!(expected.len(), found.len(), "{fixture}");
                for (i, (found, expected)) in found.iter().zip(expected).enumerate() {
                    assert!(
                        boids_close(found, expected),
                        "{graph}, {fixture}: boid {i} is {found:?} on the device and {expected:?} in the fixture"
                    );
                }
            }
        }
    }
}

/// Every speed of each boids graph stays in the gate's band and every boid in the world over 50 ticks of 500 boids,
/// the self-consistency check of `consistency_boids.rs`.
#[test]
fn the_boids_graphs_keep_speed_and_position_in_bounds() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_boids_graphs_keep_speed_and_position_in_bounds: no adapter");
        return;
    };
    for (name, entry) in boids_graphs() {
        let params = params_for(&entry, &gate_values(500));
        let mut state = build_gpu(&entry, &params, None, &ctx);
        for tick in 0..50 {
            step(&ctx, &mut *state, 1);
            let (pos_x, pos_y) = split(&state.read_buffer(0).expect("the position lane"));
            let (vel_x, vel_y) = split(&state.read_buffer(1).expect("the velocity lane"));
            for i in 0..500 {
                let speed = vel_x[i].hypot(vel_y[i]);
                assert!(
                    (GATE_SPEEDS.0 - GATE_TOLERANCE..=GATE_SPEEDS.1 + GATE_TOLERANCE).contains(&speed),
                    "{name}: boid {i} has speed {speed} at tick {tick}"
                );
                assert!(
                    (0.0..GATE_WORLD).contains(&pos_x[i]) && (0.0..GATE_WORLD).contains(&pos_y[i]),
                    "{name}: boid {i} left the world at ({}, {}) at tick {tick}",
                    pos_x[i],
                    pos_y[i]
                );
            }
        }
    }
}
