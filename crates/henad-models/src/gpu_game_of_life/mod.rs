//! GPU-accelerated Game of Life: the same rules as [`crate::game_of_life`], but with all state
//! resident in GPU storage buffers.
//!
//! Everything structural — buffers, ping-ponging, pipelines, bind groups, batched step encoding,
//! the `SimState`/`GpuSimState` impls — comes from `henad_compute::gpu::gpu_grid_engine` via the
//! [`GpuGridModel`] trait. What is left here is what actually makes this Game of Life: the
//! shaders, the seeding, and the metadata.
//!
//! # State layout
//!
//! One `array<u32>` storage buffer (one cell per element, no bit packing), ping-ponged: each step
//! reads one side and writes the other. The grid never leaves the GPU. What the CPU sees is only:
//! an RGBA display texture (written by `display.wgsl` at the display cadence, sampled by the
//! viewport) and a single `u32` alive-count (produced by `reduce.wgsl`, read back asynchronously).
//!
//! # Correctness oracle
//!
//! Seeding uses the same `xorshift64` PRNG, the same `GRID_INIT_SEED`, the same traversal order,
//! and the same density threshold as the CPU `GameOfLifeModel`. Given identical params the two
//! backends therefore start from a **bit-identical** grid and must agree forever after — which is
//! what `tests::gpu_alive_count_matches_cpu_model` checks.

use henad_compute::grid_engine::GRID_INIT_SEED;
use henad_core::gpu_grid_model::GpuGridModel;
use henad_core::helpers::{extract_f32, extract_u32, f32_param, stat, u32_param, xorshift64};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatEntry};

use crate::game_of_life::PALETTE;

/// Param indices, matching `grid_model_param_descriptors` + `GameOfLifeModel::param_descriptors`
/// so this model is a drop-in comparison against the CPU one.
const PARAM_WIDTH: usize = 0;
const PARAM_HEIGHT: usize = 1;
const PARAM_DENSITY: usize = 2;

const DEFAULT_DIM: u32 = 1024;
const DEFAULT_DENSITY: f32 = 0.3;

/// CPU-seeded random fill at the given density.
///
/// Deliberately identical to `GameOfLifeModel::init`: same PRNG, same traversal order, same
/// threshold. See the module docs — this is what makes the CPU model a usable oracle.
pub fn seed_random(width: u32, height: u32, density: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (density * u32::MAX as f32) as u32;
    let mut cells = vec![0u32; (width as usize) * (height as usize)];
    for cell in &mut cells {
        rng = xorshift64(rng);
        *cell = u32::from(((rng >> 32) as u32) < threshold);
    }
    cells
}

pub struct GpuGameOfLife;

impl GpuGridModel for GpuGameOfLife {
    const NAME: &'static str = "Game of Life (GPU)";
    const ID: &'static str = "gpu_game_of_life";
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid, stepped entirely on the GPU";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const STAT_COUNT: usize = 1;

    const STEP_SHADER: &'static str = include_str!("step.wgsl");
    const DISPLAY_SHADER: &'static str = include_str!("display.wgsl");
    const REDUCE_SHADER: &'static str = include_str!("reduce.wgsl");

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("grid_width", "Grid Width", DEFAULT_DIM, 1, 16_384),
            u32_param("grid_height", "Grid Height", DEFAULT_DIM, 1, 16_384),
            f32_param("density", "Initial Density", DEFAULT_DENSITY, 0.0, 1.0, Some(0.01)),
        ]
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        (
            extract_u32(params, PARAM_WIDTH, DEFAULT_DIM),
            extract_u32(params, PARAM_HEIGHT, DEFAULT_DIM),
        )
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue]) -> Vec<Vec<u32>> {
        let density = extract_f32(params, PARAM_DENSITY, DEFAULT_DENSITY);
        vec![seed_random(width, height, density, GRID_INIT_SEED)]
    }

    /// `step.wgsl` reads nothing but `dims: vec2<u32>`.
    fn step_params_bytes(width: u32, height: u32, _params: &[ParamValue]) -> Vec<u8> {
        bytemuck::cast_slice(&[width, height]).to_vec()
    }

    fn stat_descriptors() -> Vec<StatDescriptor> {
        vec![StatDescriptor {
            label: "Alive",
            color: PALETTE[1],
        }]
    }

    fn stats(counts: &[u32]) -> Vec<StatEntry> {
        vec![stat("Alive", f64::from(counts[0]), PALETTE[1])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::gpu::GpuContext;
    use henad_compute::gpu::gpu_grid_engine::GpuGridState;
    use henad_compute::gpu::sim_thread::GpuSimState as _;
    use henad_compute::gpu::timing::TimestampQuery;
    use henad_compute::grid_engine::GridModelState;
    use henad_core::model::SimState as _;
    use henad_core::view::StatValue;

    use crate::game_of_life::GameOfLifeModel;

    type State = GpuGridState<GpuGameOfLife>;

    pub(super) fn headless_context() -> Option<GpuContext> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_gol_test_device"),
            ..Default::default()
        }))
        .ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
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

    /// Drives display + reduce + readback exactly as the sim thread's one-shot snapshot path does.
    fn refresh_stats(ctx: &GpuContext, state: &mut State) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    /// End-to-end agreement with the CPU model, which is the real correctness oracle: identical
    /// params seed a bit-identical grid, so the alive count the GPU reduces on-device must equal
    /// the alive count the CPU model counts in Rust, tick for tick.
    #[test]
    fn gpu_alive_count_matches_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_alive_count_matches_cpu_model: no wgpu adapter available");
            return;
        };

        let (width, height) = (64u32, 64u32);
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
                "GPU-reduced alive count must match the CPU model's at tick {tick}"
            );
            assert_eq!(gpu.tick(), cpu.tick(), "tick counters must stay in step");

            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            gpu.encode_steps(&mut encoder, 1, None);
            ctx.queue.submit(Some(encoder.finish()));
            cpu.step();
        }

        // Sanity: the fixtures above would also pass if both counts were stuck at zero.
        refresh_stats(&ctx, &mut gpu);
        assert!(
            reported_alive(&gpu) > 0,
            "a 64x64 grid seeded at density 0.3 must have live cells after 10 ticks"
        );
    }

    /// Like `headless_context`, but requests `TIMESTAMP_QUERY` explicitly (mirroring what the app
    /// does when the adapter supports it), since the default test device requests no features.
    fn headless_timing_context() -> Option<GpuContext> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_gol_timing_test_device"),
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
    }

    /// Regression test for "GPU time/step flickers to 0/None during a sustained run": runs many
    /// batches back to back exactly like `GpuSimLoop::step_batch` records/resolves/reads a
    /// timestamped batch, but takes a reading on *every* iteration instead of once/second, to
    /// shake out an intermittent zero or failed readback far more aggressively than the real
    /// once-per-second cadence would in a short-lived interactive session.
    ///
    /// Lives here rather than next to `TimestampQuery` in `henad-compute` because it needs a
    /// concrete GPU model to stamp real batches with, and `henad-compute` has none by design.
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

    /// `population()` reports total cells (like `GridModelState`), *not* the alive count — the
    /// alive count is a stat. Pinned because it is an easy thing to conflate.
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

/// Tests that drive the real [`henad_compute::gpu::GpuSimThread`] rather than poking the state
/// directly — i.e. the integration surface the GUI actually uses: registry -> `ModelState::Gpu` ->
/// spawn thread -> play/pause/step -> read the published `Snapshot`.
///
/// These exist because the GUI itself cannot be driven headlessly. Everything the manual "select
/// the GPU model, press play, check the stat, switch away and back" check would exercise is
/// covered here except the final texture *sampling* (the egui paint callback), which needs a
/// surface to draw into.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod runner_tests {
    use std::time::{Duration, Instant};

    use henad_compute::gpu::gpu_grid_engine::GpuGridState;
    use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
    use henad_compute::snapshot::{Snapshot, SnapshotView};
    use henad_core::view::StatValue;

    use super::GpuGameOfLife;
    use super::tests::{headless_context, params};
    use crate::registry::{ModelState, model_registry};

    /// Spins until the thread publishes a snapshot satisfying `pred`, or the deadline passes.
    /// The GPU thread publishes on a ~16ms wall-clock cadence, so polling is the honest way to
    /// wait for one — a fixed sleep would be flakier.
    fn wait_for(thread: &mut GpuSimThread, timeout: Duration, pred: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(snap) = thread.take_snapshot() {
                if pred(&snap) {
                    return Some(snap);
                }
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

    /// The end-to-end path the viewport depends on: a GPU-backed model must publish snapshots
    /// carrying `SnapshotView::Gpu` (never a `Grid`), because the viewport branches on exactly
    /// that variant to choose between uploading a `ColorImage` and issuing the paint callback.
    #[test]
    fn gpu_thread_publishes_gpu_snapshots_and_runs() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_publishes_gpu_snapshots_and_runs: no adapter");
            return;
        };

        let (width, height) = (128u32, 128u32);
        let state = GpuGridState::<GpuGameOfLife>::new(&ctx, &params(width, height, 0.3));
        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default());

        // The thread publishes an initial snapshot before anything runs, so the viewport shows the
        // seeded grid the moment the model is loaded rather than staying blank until Play.
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

    /// The manual check that cannot be clicked headlessly: select GPU, play, switch to another
    /// model, switch back. Each switch drops the `GpuSimThread` (shutting down and joining its OS
    /// thread, releasing its buffers/pipelines) and builds a fresh one from the *same* injected
    /// context. A thread that fails to join, or GPU state left dangling in the shared context,
    /// shows up here as a hang or a panic.
    #[test]
    fn gpu_thread_teardown_and_respawn_is_clean() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_teardown_and_respawn_is_clean: no adapter");
            return;
        };

        for round in 0..3 {
            let state = GpuGridState::<GpuGameOfLife>::new(&ctx, &params(64, 64, 0.3));
            let mut thread = GpuSimThread::new(ctx.clone(), Box::new(state), GpuBatchSettings::default());
            thread.play();
            let snap = wait_for(&mut thread, Duration::from_secs(5), |s| s.tick > 0)
                .unwrap_or_else(|| panic!("round {round}: a respawned GPU thread must step"));
            assert!(matches!(snap.view, SnapshotView::Gpu(_)));
            // Dropped mid-run, exactly as a model switch does — not from a paused state.
            drop(thread);
        }
    }

    /// With a context, the GPU entry is offered by the registry, its factory yields a
    /// `ModelState::Gpu` (so `HenadApp` routes it to the GPU thread rather than the CPU one), and
    /// the state it builds is drivable. The mirror of
    /// `registry::tests::registry_without_gpu_context_offers_no_gpu_models`.
    #[test]
    fn registry_with_gpu_context_offers_a_drivable_gpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping registry_with_gpu_context_offers_a_drivable_gpu_model: no adapter");
            return;
        };

        let entries = model_registry(Some(ctx.clone()));
        let entry = entries
            .iter()
            .find(|e| e.id == "gpu_game_of_life")
            .expect("a GPU context must make the GPU model selectable");

        // Note there is no context argument here: the registry closure captured its own clone.
        let ModelState::Gpu(mut state) = (entry.create)(&params(32, 32, 0.3)) else {
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
