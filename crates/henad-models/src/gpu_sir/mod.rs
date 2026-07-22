//! GPU-accelerated SIR epidemic model
//!
//! This is similar to `gpu_game_of_life` but with 3 differences: three cell states instead of
//! two, a probabilistic transition rule, and therefore a per-cell RNG.
//!
//! The CPU model consumes a single RNG stream sequentially across a row, which has no GPU
//! equivalent. Instead each cell owns its own RNG state, stored in a ping-ponged `array<u32>`
//! buffer alongside the SIR state. Every step, a cell reads its own hash state, advances it
//! one round, and uses the result for its transition. This makes the GPU stream different from
//! the CPU stream. See `tests` for further details.
//!
//! That RNG buffer is why this model sets `BUFFER_COUNT = 2`: the engine ping-pongs the state and
//! RNG buffers together, in lockstep. Only the state buffer (index 0) is visible to the display
//! and reduce shaders.

use henad_compute::grid_engine::GRID_INIT_SEED;
use henad_core::gpu_grid_model::GpuGridModel;
use henad_core::helpers::{extract_f32, extract_u32, f32_param, stat, u32_param, xorshift64};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatEntry};

use crate::sir::PALETTE;

/// A domain-separated seed for the per-cell RNG buffer, so its stream doesn't start correlated
/// with the state-seeding stream (which reuses `GRID_INIT_SEED` directly).
const RNG_INIT_SEED: u64 = GRID_INIT_SEED ^ 0x5EED_5EED_5EED_5EED;

/// Param indices, matching `SirGridModel::from_params` so this model is a drop-in comparison
/// against the CPU one.
const PARAM_WIDTH: usize = 0;
const PARAM_HEIGHT: usize = 1;
const PARAM_INFECTION_RATE: usize = 2;
const PARAM_RECOVERY_RATE: usize = 3;
const PARAM_INITIAL_INFECTED_PCT: usize = 4;

const DEFAULT_DIM: u32 = 1024;
const DEFAULT_INFECTION_RATE: f32 = 0.3;
const DEFAULT_RECOVERY_RATE: f32 = 0.05;
const DEFAULT_INITIAL_INFECTED_PCT: f32 = 0.01;

const CELL_S: usize = 0;
const CELL_I: usize = 1;
const CELL_R: usize = 2;

/// Matches `Params` in `step.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct StepParams {
    width: u32,
    height: u32,
    infection_rate: f32,
    recovery_rate: f32,
}

/// CPU-seeded initial S/I state, identical to `SirGridModel::init`: same PRNG, same traversal
/// order, same threshold — so GPU and CPU start from a bit-identical grid.
pub fn seed_cells(width: u32, height: u32, initial_infected_pct: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (initial_infected_pct * u32::MAX as f32) as u32;
    let mut cells = vec![0u32; (width as usize) * (height as usize)];
    for cell in &mut cells {
        rng = xorshift64(rng);
        *cell = u32::from(((rng >> 32) as u32) < threshold);
    }
    cells
}

/// Matches `pcg_hash` in `step.wgsl` bit-for-bit (u32 arithmetic wraps identically on both sides).
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Initial per-cell RNG state, seeded independently of the S/I state via `RNG_INIT_SEED`.
fn seed_rng_states(width: u32, height: u32, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..(width as usize) * (height as usize))
        .map(|idx| pcg_hash(seed32 ^ idx as u32))
        .collect()
}

pub struct GpuSir;

impl GpuGridModel for GpuSir {
    const NAME: &'static str = "SIR Epidemic (GPU)";
    const ID: &'static str = "gpu_sir";
    const DESCRIPTION: &'static str =
        "Classic SIR compartmental model on a toroidal grid with Moore neighborhood, stepped entirely on the GPU";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    /// State buffer plus the per-cell RNG buffer. See the module docs.
    const BUFFER_COUNT: usize = 2;
    const STAT_COUNT: usize = 3;

    const STEP_SHADER: &'static str = include_str!("step.wgsl");
    const DISPLAY_SHADER: &'static str = include_str!("display.wgsl");
    const REDUCE_SHADER: &'static str = include_str!("reduce.wgsl");

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("grid_width", "Grid Width", DEFAULT_DIM, 1, 16_384),
            u32_param("grid_height", "Grid Height", DEFAULT_DIM, 1, 16_384),
            f32_param(
                "infection_rate",
                "Infection Rate",
                DEFAULT_INFECTION_RATE,
                0.0,
                1.0,
                Some(0.01),
            ),
            f32_param(
                "recovery_rate",
                "Recovery Rate",
                DEFAULT_RECOVERY_RATE,
                0.0,
                1.0,
                Some(0.01),
            ),
            f32_param(
                "initial_infected_pct",
                "Initial Infected %",
                DEFAULT_INITIAL_INFECTED_PCT,
                0.0,
                1.0,
                Some(0.001),
            ),
        ]
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        (
            extract_u32(params, PARAM_WIDTH, DEFAULT_DIM),
            extract_u32(params, PARAM_HEIGHT, DEFAULT_DIM),
        )
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue]) -> Vec<Vec<u32>> {
        let initial_infected_pct = extract_f32(params, PARAM_INITIAL_INFECTED_PCT, DEFAULT_INITIAL_INFECTED_PCT);
        vec![
            seed_cells(width, height, initial_infected_pct, GRID_INIT_SEED),
            seed_rng_states(width, height, RNG_INIT_SEED),
        ]
    }

    fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8> {
        bytemuck::bytes_of(&StepParams {
            width,
            height,
            infection_rate: extract_f32(params, PARAM_INFECTION_RATE, DEFAULT_INFECTION_RATE),
            recovery_rate: extract_f32(params, PARAM_RECOVERY_RATE, DEFAULT_RECOVERY_RATE),
        })
        .to_vec()
    }

    fn stat_descriptors() -> Vec<StatDescriptor> {
        vec![
            StatDescriptor {
                label: "Susceptible",
                color: PALETTE[0],
            },
            StatDescriptor {
                label: "Infected",
                color: PALETTE[1],
            },
            StatDescriptor {
                label: "Recovered",
                color: PALETTE[2],
            },
        ]
    }

    fn stats(counts: &[u32]) -> Vec<StatEntry> {
        vec![
            stat("Susceptible", f64::from(counts[CELL_S]), PALETTE[0]),
            stat("Infected", f64::from(counts[CELL_I]), PALETTE[1]),
            stat("Recovered", f64::from(counts[CELL_R]), PALETTE[2]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::gpu::GpuContext;
    use henad_compute::gpu::gpu_grid_engine::GpuGridState;
    use henad_compute::gpu::sim_thread::GpuSimState as _;
    use henad_compute::grid_engine::GridModelState;
    use henad_core::model::SimState as _;
    use henad_core::view::StatValue;

    use crate::sir::SirGridModel;

    type State = GpuGridState<GpuSir>;

    pub(super) fn headless_context() -> Option<GpuContext> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_sir_test_device"),
            ..Default::default()
        }))
        .ok()?;
        Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
    }

    pub(super) fn params(
        width: u32,
        height: u32,
        infection_rate: f32,
        recovery_rate: f32,
        initial_infected_pct: f32,
    ) -> Vec<ParamValue> {
        vec![
            ParamValue::U32(width),
            ParamValue::U32(height),
            ParamValue::F32(infection_rate),
            ParamValue::F32(recovery_rate),
            ParamValue::F32(initial_infected_pct),
        ]
    }

    fn sir_counts(state: &State) -> (u64, u64, u64) {
        let stats = state.stats();
        let scalar = |entry: &StatEntry| match &entry.value {
            StatValue::Scalar(v) => *v as u64,
            other => panic!("expected a scalar stat, got {other:?}"),
        };
        (scalar(&stats[0]), scalar(&stats[1]), scalar(&stats[2]))
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

    fn step_once(ctx: &GpuContext, state: &mut State) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 1, None);
        ctx.queue.submit(Some(encoder.finish()));
    }

    /// S+I+R must hold exactly at every tick regardless of how the per-cell RNG streams behave.
    #[test]
    fn population_conserved_over_many_ticks() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping population_conserved_over_many_ticks: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let mut state = State::new(&ctx, &params(width, height, 0.3, 0.05, 0.1));
        let total = u64::from(width) * u64::from(height);

        for tick in 0..50 {
            refresh_stats(&ctx, &mut state);
            let (s, i, r) = sir_counts(&state);
            assert_eq!(s + i + r, total, "S+I+R must equal total population at tick {tick}");
            step_once(&ctx, &mut state);
        }
    }

    /// With `infection_rate == 0`, S can never lose a member.
    #[test]
    fn zero_infection_rate_freezes_susceptible_count() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping zero_infection_rate_freezes_susceptible_count: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let mut state = State::new(&ctx, &params(width, height, 0.0, 0.05, 0.2));

        refresh_stats(&ctx, &mut state);
        let (initial_s, _, _) = sir_counts(&state);

        for tick in 0..20 {
            step_once(&ctx, &mut state);
            refresh_stats(&ctx, &mut state);
            let (s, _, _) = sir_counts(&state);
            assert_eq!(
                s, initial_s,
                "susceptible count must not change at tick {tick} when infection_rate is 0"
            );
        }
    }

    /// With `recovery_rate == 0`, I can never lose a member,
    /// so with any positive infection rate, I must be monotonically non-decreasing.
    #[test]
    fn zero_recovery_rate_keeps_infected_non_decreasing() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping zero_recovery_rate_keeps_infected_non_decreasing: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let mut state = State::new(&ctx, &params(width, height, 0.5, 0.0, 0.1));

        refresh_stats(&ctx, &mut state);
        let (_, mut prev_i, _) = sir_counts(&state);

        for tick in 0..20 {
            step_once(&ctx, &mut state);
            refresh_stats(&ctx, &mut state);
            let (_, i, r) = sir_counts(&state);
            assert!(
                i >= prev_i,
                "infected count must not decrease at tick {tick} when recovery_rate is 0"
            );
            assert_eq!(r, 0, "no cell can reach R when recovery_rate is 0 (tick {tick})");
            prev_i = i;
        }
    }

    /// Initial seeding uses the same PRNG, traversal order, and threshold as the CPU model, so the
    /// tick-0 compartment counts (before any RNG-dependent step) must match exactly.
    #[test]
    fn initial_seed_matches_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping initial_seed_matches_cpu_model: no adapter");
            return;
        };

        let (width, height) = (64u32, 64u32);
        let p = params(width, height, 0.3, 0.05, 0.2);

        let mut gpu = State::new(&ctx, &p);
        let cpu = GridModelState::<SirGridModel>::from_params(&p);

        refresh_stats(&ctx, &mut gpu);
        let (gpu_s, gpu_i, gpu_r) = sir_counts(&gpu);

        let cpu_scalar = |entry: &StatEntry| match &entry.value {
            StatValue::Scalar(v) => *v as u64,
            other => panic!("expected a scalar stat, got {other:?}"),
        };
        let cpu_stats = cpu.stats();
        assert_eq!(
            gpu_s,
            cpu_scalar(&cpu_stats[0]),
            "initial susceptible count must match the CPU model"
        );
        assert_eq!(
            gpu_i,
            cpu_scalar(&cpu_stats[1]),
            "initial infected count must match the CPU model"
        );
        assert_eq!(
            gpu_r,
            cpu_scalar(&cpu_stats[2]),
            "initial recovered count must be 0 on both backends"
        );
    }

    #[test]
    fn population_is_total_cells() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping population_is_total_cells: no adapter");
            return;
        };

        let (width, height) = (32u32, 16u32);
        let state = State::new(&ctx, &params(width, height, 0.3, 0.05, 0.1));
        assert_eq!(state.population(), u64::from(width) * u64::from(height));
    }
}

/// Tests that drive the real [`henad_compute::gpu::GpuSimThread`], similar to
/// `gpu_game_of_life`'s `runner_tests`.
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod runner_tests {
    use std::time::{Duration, Instant};

    use henad_compute::gpu::gpu_grid_engine::GpuGridState;
    use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
    use henad_compute::snapshot::{Snapshot, SnapshotView};
    use henad_core::view::StatValue;

    use super::GpuSir;
    use super::tests::{headless_context, params};
    use crate::registry::{ModelState, model_registry};

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

    fn sir_counts(snap: &Snapshot) -> (u64, u64, u64) {
        let scalar = |entry: &henad_core::view::StatEntry| match &entry.value {
            StatValue::Scalar(v) => *v as u64,
            other => panic!("expected a scalar stat, got {other:?}"),
        };
        (scalar(&snap.stats[0]), scalar(&snap.stats[1]), scalar(&snap.stats[2]))
    }

    #[test]
    fn gpu_thread_publishes_gpu_snapshots_and_conserves_population() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping gpu_thread_publishes_gpu_snapshots_and_conserves_population: no adapter");
            return;
        };

        let (width, height) = (128u32, 128u32);
        let state = GpuGridState::<GpuSir>::new(&ctx, &params(width, height, 0.3, 0.05, 0.1));
        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default());

        let initial = wait_for(&mut thread, Duration::from_secs(5), |_| true)
            .expect("the GPU thread must publish an initial snapshot before Play");
        assert!(matches!(initial.view, SnapshotView::Gpu(_)));
        let total = initial.population;
        let (s, i, r) = sir_counts(&initial);
        assert_eq!(s + i + r, total, "initial snapshot must already conserve population");

        thread.play();
        let running = wait_for(&mut thread, Duration::from_secs(5), |snap| snap.tick > 0)
            .expect("playing must advance the tick counter and publish fresh snapshots");
        assert!(matches!(running.view, SnapshotView::Gpu(_)));
        let (s, i, r) = sir_counts(&running);
        assert_eq!(s + i + r, total, "population must stay conserved while running");

        thread.pause();
        drop(thread);
    }

    #[test]
    fn registry_with_gpu_context_offers_a_drivable_gpu_sir_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping registry_with_gpu_context_offers_a_drivable_gpu_sir_model: no adapter");
            return;
        };

        let entries = model_registry(Some(ctx.clone()));
        let entry = entries
            .iter()
            .find(|e| e.id == "gpu_sir")
            .expect("a GPU context must make the GPU SIR model selectable");

        let ModelState::Gpu(mut state) = (entry.create)(&params(32, 32, 0.3, 0.05, 0.1)) else {
            panic!("the GPU SIR entry's factory must yield ModelState::Gpu, not ModelState::Cpu");
        };

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 4, None);
        ctx.queue.submit(Some(encoder.finish()));
        assert_eq!(state.tick(), 4, "the registry-built state must be steppable");
    }
}
