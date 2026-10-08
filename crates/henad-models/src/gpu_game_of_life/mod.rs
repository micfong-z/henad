//! Game of Life as a [`GpuGridModel`], with the rules of [`crate::game_of_life`] and the grid in a storage buffer.
//!
//! Cells are bit-packed, 32 to a `u32`. Cell `x` of row `y` is bit `x % 32` of word `y * words_per_row + x / 32`, and
//! each row is padded with zero bits to [`words_per_row`] whole words.
//! The step dispatches one invocation per word. Per cell, 32 invocations would write the same word and race.
//! Display dispatches one invocation per texel and reduce one per cell. Each invocation extracts the bit of its cell.
//!
//! [`seed_random`] repeats the draws of the CPU model's `init`. For the same seed both backends start from the same
//! grid, and the CPU model is an exact oracle on every tick after.

use henad_compute::cpu::grid_engine::grid_init_rng;
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::gpu_grid_model::{GpuGridAction, GpuGridModel};
use henad_core::authoring::primitives::rng::xorshift64;
use henad_core::helpers::{extract_f32, extract_u32, f32_param, u32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use crate::game_of_life::PALETTE;
use crate::shader_bindings::gpu_game_of_life::randomise::Params as ActionParams;

// The list repeats the ids and order the CPU engine composes for `GameOfLifeModel`, so one parameter vector drives
// either backend.
henad_core::params! {
    const PARAM_WIDTH = u32_param("grid_width", "Grid Width", DEFAULT_DIM, 1, 16_384);
    const PARAM_HEIGHT = u32_param("grid_height", "Grid Height", DEFAULT_DIM, 1, 16_384);
    const PARAM_DENSITY = f32_param("density", "Initial Density", DEFAULT_DENSITY, 0.0, 1.0, Some(0.01));
}

const DEFAULT_DIM: u32 = 1024;
const DEFAULT_DENSITY: f32 = 0.3;

/// Returns the number of words in a padded row of `width` cells, 32 cells to a word.
pub fn words_per_row(width: u32) -> usize {
    (width as usize).div_ceil(32)
}

/// Returns a bit-packed grid of `width` by `height` cells, each alive with probability `density`.
///
/// The cells are drawn from `rng` in row-major order, with the draws and the threshold of `GameOfLifeModel::init`.
/// With `rng` from `grid_init_rng`, the grid matches the CPU model's bit for bit.
///
/// Padding bits, present when `width % 32 != 0`, start at zero, and the step keeps them at zero.
pub fn seed_random(width: u32, height: u32, density: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (density * u32::MAX as f32) as u32;
    let stride = words_per_row(width);
    let mut words = vec![0u32; stride * (height as usize)];
    for y in 0..height as usize {
        for x in 0..width as usize {
            rng = xorshift64(rng);
            if ((rng >> 32) as u32) < threshold {
                words[y * stride + (x / 32)] |= 1u32 << (x % 32);
            }
        }
    }
    words
}

/// Conway's Game of Life as a [`GpuGridModel`], over a bit-packed grid.
#[derive(Debug)]
pub struct GpuGameOfLife;

impl GpuGridModel for GpuGameOfLife {
    const NAME: &'static str = "Game of Life (GPU)";
    const ID: &'static str = "gpu_game_of_life";
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid, stepped entirely on the GPU";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Alive", PALETTE[1])];

    const BUFFERS: &'static [&'static str] = &["state"];

    const STEP_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_GAME_OF_LIFE_STEP;
    const DISPLAY_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_GAME_OF_LIFE_DISPLAY;
    const REDUCE_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_GAME_OF_LIFE_REDUCE;

    const STEP_SHADER: &'static str = crate::shader_bindings::gpu_game_of_life::step::SHADER_STRING;
    const DISPLAY_SHADER: &'static str = crate::shader_bindings::gpu_game_of_life::display::SHADER_STRING;
    const REDUCE_SHADER: &'static str = crate::shader_bindings::gpu_game_of_life::reduce::SHADER_STRING;

    const ACTIONS: &'static [GpuGridAction] = &[
        GpuGridAction {
            desc: ActionDescriptor::new("randomise", "Randomise"),
            shader: crate::shader_bindings::gpu_game_of_life::randomise::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_GAME_OF_LIFE_RANDOMISE,
        },
        GpuGridAction {
            desc: ActionDescriptor::new("clear", "Clear"),
            shader: crate::shader_bindings::gpu_game_of_life::clear::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_GAME_OF_LIFE_CLEAR,
        },
    ];

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        (
            extract_u32(params, PARAM_WIDTH, DEFAULT_DIM),
            extract_u32(params, PARAM_HEIGHT, DEFAULT_DIM),
        )
    }

    fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
        vec![words_per_row(width) * (height as usize)]
    }

    /// Returns one invocation per word of each row.
    fn step_dims(width: u32, height: u32) -> (u32, u32) {
        (words_per_row(width) as u32, height)
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        let density = extract_f32(params, PARAM_DENSITY, DEFAULT_DENSITY);
        vec![seed_random(width, height, density, grid_init_rng(seed))]
    }

    /// `step.wgsl` reads nothing but `dims: vec2<u32>`.
    fn step_params_bytes(width: u32, height: u32, _params: &[ParamValue]) -> Vec<u8> {
        bytemuck::cast_slice(&[width, height]).to_vec()
    }

    fn action_params_bytes(_action: usize, width: u32, height: u32, params: &[ParamValue], seed: u32) -> Vec<u8> {
        let density = extract_f32(params, PARAM_DENSITY, DEFAULT_DENSITY);
        bytemuck::bytes_of(&ActionParams {
            width,
            height,
            threshold: (density * u32::MAX as f32) as u32,
            seed,
        })
        .to_vec()
    }

    fn stats(counts: &[u32]) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(counts[0]))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::cpu::grid_engine::{GRID_PARAM_BASE, GridModelState};
    use henad_compute::gpu::GpuContext;
    use henad_compute::gpu::grid_engine::GpuGridState;
    use henad_compute::gpu::sim_thread::GpuSimState as _;
    use henad_compute::gpu::timing::TimestampQuery;
    use henad_core::authoring::model::grid_model::GridModel as _;
    use henad_core::grid::Grid2D;
    use henad_core::model::SimState as _;
    use henad_explore::testing::{TestDeviceRequest, headless_test_device};

    use crate::game_of_life::GameOfLifeModel;

    type State = GpuGridState<GpuGameOfLife>;

    pub(super) fn headless_context() -> Option<GpuContext> {
        headless_test_device(&TestDeviceRequest::baseline())
    }

    pub(super) fn params(width: u32, height: u32, density: f32) -> Vec<ParamValue> {
        vec![
            ParamValue::U32(width),
            ParamValue::U32(height),
            ParamValue::F32(density),
        ]
    }

    fn reported_alive(state: &State) -> u64 {
        match state.stats().first().map(|s| s.value.clone()) {
            Some(StatValue::Scalar(v)) => v as u64,
            other => panic!("expected a scalar Alive stat, got {other:?}"),
        }
    }

    /// Runs the display, reduce and readback passes as the sim thread's one-shot snapshot does.
    fn refresh_stats(ctx: &GpuContext, state: &mut State) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    /// The port repeats `GameOfLifeModel::init` rather than calling it, so every cell is compared.
    /// The CPU grid is packed into the shaders' layout. A width of 37 leaves padding bits, and
    /// the seed leaves them zero.
    #[test]
    fn the_seeded_grid_matches_the_cpu_init() {
        let (width, height) = (37u32, 23u32);
        let p = params(width, height, 0.3);
        let stride = words_per_row(width);
        for seed in [None, Some(7)] {
            let mut grid = Grid2D::new(width, height);
            GameOfLifeModel::init(&mut grid, &p[GRID_PARAM_BASE..], &mut grid_init_rng(seed));
            let mut cpu = vec![0u32; stride * height as usize];
            for (index, &cell) in grid.current().iter().enumerate() {
                let (x, y) = (index % width as usize, index / width as usize);
                cpu[y * stride + x / 32] |= u32::from(cell) << (x % 32);
            }
            assert_eq!(
                GpuGameOfLife::seed_buffers(width, height, &p, seed)[0],
                cpu,
                "the seeded words differ from the CPU grid for seed {seed:?}"
            );
        }
    }

    /// Checks the alive count that the GPU reduces against the CPU model's count, tick for tick.
    ///
    /// Equal params seed the same grid on both backends, and the CPU model is the oracle.
    #[test]
    fn gpu_alive_count_matches_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_alive_count_matches_cpu_model: no wgpu adapter available");
            return;
        };
        check_agreement_over_ticks(&ctx, 64, 64);
    }

    /// Checks the same oracle at a width that is neither a multiple of 32 nor a power of two.
    ///
    /// The width exercises both x-wrap boundaries of the packing.
    ///
    /// - The last word is ragged (50 % 32 = 18). It holds cells 32 to 49 in bits 0 to 17, and cell 49's right neighbour
    ///   wraps to cell 0 from a bit other than 31.
    /// - The width is not a power of two. The left neighbour of column 0 comes from a `% width`, which gives the right
    ///   answer for any width dividing 2^32 even when the arithmetic feeding it is wrong.
    #[test]
    fn gpu_alive_count_matches_cpu_model_at_ragged_width() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_alive_count_matches_cpu_model_at_ragged_width: no adapter");
            return;
        };
        check_agreement_over_ticks(&ctx, 50, 30);
    }

    /// Checks that a grid wider than `max_texture_dimension_2d` builds and steps.
    ///
    /// Only the width is over the limit, and this also pins the per-axis capping. Display runs at 4096 by 64, and step
    /// and reduce cover all 8200 columns.
    #[test]
    fn a_grid_past_the_texture_limit_still_builds_and_steps() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping a_grid_past_the_texture_limit_still_builds_and_steps: no adapter");
            return;
        };
        assert!(
            ctx.device.limits().max_texture_dimension_2d < 8200,
            "the test device is not a baseline one, so this proves nothing"
        );
        check_agreement_over_ticks(&ctx, 8200, 64);
    }

    fn check_agreement_over_ticks(ctx: &GpuContext, width: u32, height: u32) {
        let ctx = ctx.clone();
        let p = params(width, height, 0.3);

        let mut gpu = State::new(&ctx, &p);
        let mut cpu = GridModelState::<GameOfLifeModel>::from_params(&p);

        let cpu_alive = |cpu: &GridModelState<GameOfLifeModel>| -> u64 {
            match cpu.stats().first().map(|s| s.value.clone()) {
                Some(StatValue::Scalar(v)) => v as u64,
                other => panic!("expected a scalar Alive stat, got {other:?}"),
            }
        };

        for tick in 0..10 {
            refresh_stats(&ctx, &mut gpu);
            assert_eq!(
                reported_alive(&gpu),
                cpu_alive(&cpu),
                "GPU-reduced alive count must match the CPU model's at tick {tick} ({width}x{height})"
            );
            assert_eq!(gpu.tick(), cpu.tick(), "tick counters must stay in step");

            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            gpu.encode_steps(&mut encoder, 1, None);
            ctx.queue.submit(Some(encoder.finish()));
            cpu.step();
        }

        // The assertions above would also pass with both counts stuck at zero.
        refresh_stats(&ctx, &mut gpu);
        assert!(
            reported_alive(&gpu) > 0,
            "a {width}x{height} grid seeded at density 0.3 must have live cells after 10 ticks"
        );
    }

    /// Returns a baseline test device with `TIMESTAMP_QUERY`, which the app requests where the adapter supports it.
    ///
    /// The default test device requests no features.
    fn headless_timing_context() -> Option<GpuContext> {
        headless_test_device(
            &TestDeviceRequest::baseline().features(henad_compute::gpu::wgpu::Features::TIMESTAMP_QUERY),
        )
    }

    /// Checks that the GPU time per step is never zero and its readback never fails over many batches run back to back.
    ///
    /// Each batch is recorded, resolved and read as `GpuSimLoop::step_batch` does it. The test takes a reading after
    /// every batch, where the runner takes a reading once a second.
    #[test]
    fn gpu_timing_readback_is_stable_over_many_batches() {
        let Some(ctx) = headless_timing_context() else {
            log::warn!(
                "skipping gpu_timing_readback_is_stable_over_many_batches: \
                 no adapter with TIMESTAMP_QUERY available"
            );
            return;
        };

        let (width, height) = (256u32, 256u32);
        let mut state = State::new(&ctx, &params(width, height, 0.3));

        let tq = TimestampQuery::new(&ctx.device, &ctx.queue).expect("device has TIMESTAMP_QUERY");
        let batch_size = 64;
        let iterations = 200;
        let mut zero_count = 0usize;
        let mut none_count = 0usize;

        for _ in 0..iterations {
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            state.encode_steps(&mut encoder, batch_size, Some(tq.query_set()));
            let write_submission = ctx.queue.submit(Some(encoder.finish()));

            tq.resolve_after(&ctx.device, &ctx.queue, write_submission);

            match tq.read_gpu_us_per_step(&ctx.device, batch_size) {
                Some(us) if us <= 0.0 => zero_count += 1,
                Some(_) => {}
                None => none_count += 1,
            }
        }

        assert_eq!(
            none_count, 0,
            "readback failed (returned None) on {none_count}/{iterations} back-to-back batches"
        );
        assert_eq!(
            zero_count, 0,
            "readback returned 0 (end timestamp <= start timestamp) on \
             {zero_count}/{iterations} back-to-back batches"
        );
    }

    /// Checks the bit-sliced rule of `step.wgsl` against Conway's rule for all 256 neighbourhoods, n == 8 included.
    ///
    /// The shader counts neighbours with a carry-save adder, bit-sliced across sb0, sb1 and sb2, and the rule
    /// collapses to `alive = ~sb2 & sb1 & (sb0 | cells)`. The test pins two steps derived by hand: that identity
    /// for the 2-survives, 3-births rule, and the dropped weight-8 carry, which is sound only because n == 8 has
    /// sb1 == 0. `gpu_alive_count_matches_cpu_model` compares alive counts, and a rule error that kept the
    /// population would pass it.
    ///
    /// The test mirrors the shader's operations instead of invoking them. Every operation is bitwise and the 32 lanes
    /// are independent, so lane 0 suffices.
    #[test]
    fn swar_rule_matches_conway_for_every_neighbourhood() {
        fn full_add(a: u32, b: u32, c: u32) -> (u32, u32) {
            let t = a ^ b;
            (t ^ c, (a & b) | (c & t))
        }
        fn half_add(a: u32, b: u32) -> (u32, u32) {
            (a ^ b, a & b)
        }

        for mask in 0u32..256 {
            let nb: [u32; 8] = std::array::from_fn(|i| (mask >> i) & 1);
            for cell in 0u32..2 {
                let (a_sum, a_carry) = full_add(nb[0], nb[1], nb[2]);
                let (b_sum, b_carry) = full_add(nb[3], nb[4], nb[5]);
                let (c_sum, c_carry) = half_add(nb[6], nb[7]);

                let (sb0, d_carry) = full_add(a_sum, b_sum, c_sum);
                let (e_sum, e_carry) = full_add(a_carry, b_carry, c_carry);
                let (sb1, f_carry) = half_add(e_sum, d_carry);
                let sb2 = e_carry ^ f_carry;

                let alive = !sb2 & sb1 & (sb0 | cell) & 1;

                let n = mask.count_ones();
                let expected = u32::from(n == 3 || (n == 2 && cell == 1));
                assert_eq!(
                    alive, expected,
                    "SWAR rule disagrees with Conway at neighbours={n} (mask {mask:#010b}), cell={cell}"
                );
            }
        }
    }

    /// Checks that `population()` reports the total cell count, as `GridModelState` does.
    ///
    /// The alive count is a stat, and the two counts are easy to conflate.
    #[test]
    fn population_is_total_cells_not_alive_count() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping population_is_total_cells_not_alive_count: no adapter");
            return;
        };

        let (width, height) = (32u32, 16u32);
        let state = State::new(&ctx, &params(width, height, 0.3));
        assert_eq!(state.population(), u64::from(width) * u64::from(height));

        let cpu = GridModelState::<GameOfLifeModel>::from_params(&params(width, height, 0.3));
        assert_eq!(
            state.population(),
            cpu.population(),
            "GPU and CPU Game of Life must agree on what 'population' means"
        );
    }
}

/// Tests that drive a [`henad_compute::gpu::GpuSimThread`] or a registry entry as the app does.
///
/// The texture sampling in egui's paint callback needs a surface, and no test here covers it.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod runner_tests {
    use std::time::{Duration, Instant};

    use henad_compute::gpu::grid_engine::GpuGridState;
    use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
    use henad_compute::snapshot::{Snapshot, SnapshotView};

    use super::GpuGameOfLife;
    use super::tests::{headless_context, params};
    use henad_compute::entry::ModelState;
    use henad_core::view::StatValue;

    /// Polls until the thread publishes a snapshot satisfying `pred`, or the deadline passes.
    ///
    /// The GPU thread publishes on a wall-clock cadence, and a fixed sleep would be flakier.
    fn wait_for(thread: &mut GpuSimThread, timeout: Duration, pred: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(snap) = thread.take_snapshot()
                && pred(&snap)
            {
                return Some(snap);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        None
    }

    fn alive(snap: &Snapshot) -> u64 {
        match snap.stats.first().map(|s| s.value.clone()) {
            Some(StatValue::Scalar(v)) => v as u64,
            other => panic!("expected a scalar Alive stat, got {other:?}"),
        }
    }

    /// Checks that a GPU model's thread publishes snapshots carrying `SnapshotView::Gpu`, and steps when played.
    ///
    /// The viewport uses that variant to choose between uploading a `ColorImage` and issuing the paint callback.
    #[test]
    fn gpu_thread_publishes_gpu_snapshots_and_runs() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_publishes_gpu_snapshots_and_runs: no adapter");
            return;
        };

        let (width, height) = (128u32, 128u32);
        let state = GpuGridState::<GpuGameOfLife>::new(&ctx, &params(width, height, 0.3));
        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default(), None);

        // The thread publishes an initial snapshot before anything runs, and the viewport shows the
        // seeded grid before Play.
        let initial = wait_for(&mut thread, Duration::from_secs(5), |_| true)
            .expect("the GPU thread must publish an initial snapshot before Play");

        assert!(
            matches!(initial.view, SnapshotView::Gpu(_)),
            "a GPU model must publish SnapshotView::Gpu — the viewport branches on this variant"
        );
        assert_eq!(initial.tick, 0, "the initial snapshot is pre-step");
        assert_eq!(
            initial.population,
            u64::from(width) * u64::from(height),
            "population must report total cells"
        );
        let initial_alive = alive(&initial);
        assert!(
            initial_alive > 0 && initial_alive < initial.population,
            "a density-0.3 seed must be neither empty nor full, got {initial_alive} alive"
        );

        thread.play();
        let running = wait_for(&mut thread, Duration::from_secs(5), |s| s.tick > 0)
            .expect("playing must advance the tick counter and publish fresh snapshots");
        assert!(matches!(running.view, SnapshotView::Gpu(_)));
        assert!(alive(&running) > 0, "the sim must not have died out");

        thread.pause();
        drop(thread);
    }

    /// Checks that a `GpuSimThread` can be dropped mid-run and built again on the same context, three times.
    ///
    /// A model switch away and back does the same. Each drop shuts down and joins the OS thread and releases its
    /// buffers and pipelines. A thread that fails to join, or GPU state left dangling in the shared context, shows up
    /// here as a hang or a panic.
    #[test]
    fn gpu_thread_teardown_and_respawn_is_clean() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_teardown_and_respawn_is_clean: no adapter");
            return;
        };

        for round in 0..3 {
            let state = GpuGridState::<GpuGameOfLife>::new(&ctx, &params(64, 64, 0.3));
            let mut thread = GpuSimThread::new(ctx.clone(), Box::new(state), GpuBatchSettings::default(), None);
            thread.play();
            let snap = wait_for(&mut thread, Duration::from_secs(5), |s| s.tick > 0)
                .unwrap_or_else(|| panic!("round {round}: a respawned GPU thread must step"));
            assert!(matches!(snap.view, SnapshotView::Gpu(_)));
            // The thread is dropped mid-run, as a model switch drops it.
            drop(thread);
        }
    }

    /// Checks that a lookup in the example set with a context returns the GPU entry, and that its factory builds a
    /// `ModelState::Gpu` that steps. `HenadApp` routes that variant to the GPU thread.
    #[test]
    fn registry_with_gpu_context_offers_a_drivable_gpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping registry_with_gpu_context_offers_a_drivable_gpu_model: no adapter");
            return;
        };

        let models = crate::example_models();
        let entry = models
            .lookup("gpu_game_of_life", Some(&ctx))
            .expect("a GPU context must make the GPU model selectable");

        let built = entry
            .build(&params(32, 32, 0.3), None, Some(&ctx))
            .unwrap_or_else(|fault| panic!("the GPU entry's factory failed to build: {fault}"));
        let ModelState::Gpu(mut state) = built else {
            panic!("the GPU entry's factory must yield ModelState::Gpu, not ModelState::Cpu");
        };

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 4, None);
        ctx.queue.submit(Some(encoder.finish()));
        assert_eq!(state.tick(), 4, "the registry-built state must be steppable");
    }
}
