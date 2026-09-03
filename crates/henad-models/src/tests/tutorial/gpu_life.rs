//! GPU Game of Life as `docs/guide/first-model/gpu-game-of-life.md` builds it.
//!
//! The id is `gpu_life` rather than `gpu_game_of_life`, since the shipped model already holds
//! that one and the page tells a reader the same thing.
//!
//! The shaders are the shipped model's own. A shader carries no id, so what the page writes is
//! `gpu_game_of_life/*.wgsl` line for line, and this module binds those rather than carrying a
//! second copy. The page spells the generated paths `gpu_life`, after the directory a reader makes.

use henad_compute::cpu::grid_engine::GRID_INIT_SEED;
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::authoring::primitives::rng::{below, mix_seed, next_bits};
use henad_core::helpers::{extract_f32, extract_u32, f32_param, u32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use super::life::PALETTE;

henad_core::params! {
    const GRID_WIDTH = u32_param("grid_width", "Grid Width", 1024, 1, 16_384);
    const GRID_HEIGHT = u32_param("grid_height", "Grid Height", 1024, 1, 16_384);
    const DENSITY = f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01));
}

pub struct GpuLifeModel;

impl GpuGridModel for GpuLifeModel {
    const NAME: &'static str = "Game of Life (GPU)";
    const ID: &'static str = "gpu_life";
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid, stepped entirely on the GPU";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Alive", PALETTE[1])];

    const BUFFERS: &'static [&'static str] = &["state"];

    const STEP_SHADER: &'static str = crate::shader_bindings::gpu_game_of_life::step::SHADER_STRING;
    const DISPLAY_SHADER: &'static str = crate::shader_bindings::gpu_game_of_life::display::SHADER_STRING;
    const REDUCE_SHADER: &'static str = crate::shader_bindings::gpu_game_of_life::reduce::SHADER_STRING;

    const STEP_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_GAME_OF_LIFE_STEP;
    const DISPLAY_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_GAME_OF_LIFE_DISPLAY;
    const REDUCE_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_GAME_OF_LIFE_REDUCE;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        (
            extract_u32(params, GRID_WIDTH, 1024),
            extract_u32(params, GRID_HEIGHT, 1024),
        )
    }

    fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
        vec![words_per_row(width) * (height as usize)]
    }

    fn step_dims(width: u32, height: u32) -> (u32, u32) {
        (words_per_row(width) as u32, height)
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        let density = extract_f32(params, DENSITY, 0.3);
        let rng = seed.map_or(GRID_INIT_SEED, mix_seed);
        vec![seed_random(width, height, density, rng)]
    }

    fn step_params_bytes(width: u32, height: u32, _params: &[ParamValue]) -> Vec<u8> {
        bytemuck::cast_slice(&[width, height]).to_vec()
    }

    fn stats(counts: &[u32]) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(counts[0]))]
    }
}

/// Words per padded row. 32 cells to a `u32`, rounded up.
pub fn words_per_row(width: u32) -> usize {
    (width as usize).div_ceil(32)
}

fn seed_random(width: u32, height: u32, density: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (density * u32::MAX as f32) as u32;
    let stride = words_per_row(width);
    let mut words = vec![0u32; stride * (height as usize)];
    for y in 0..height as usize {
        for x in 0..width as usize {
            if below(next_bits(&mut rng), threshold) {
                words[y * stride + (x / 32)] |= 1u32 << (x % 32);
            }
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::cpu::grid_engine::GridModelState;
    use henad_compute::gpu::grid_engine::GpuGridState;
    use henad_compute::gpu::{GpuContext, GpuSimState as _};
    use henad_core::model::SimState as _;

    use crate::tests::tutorial::life::LifeModel;

    fn alive(stats: &[henad_core::view::StatEntry]) -> u64 {
        match stats.first().map(|s| s.value.clone()) {
            Some(StatValue::Scalar(v)) => v as u64,
            other => panic!("expected a scalar Alive stat, got {other:?}"),
        }
    }

    /// Runs display and reduce, then waits for the count to land, as a one-shot snapshot does.
    fn refresh_stats(ctx: &GpuContext, state: &mut GpuGridState<GpuLifeModel>) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    #[test]
    fn the_alive_count_matches_the_cpu_model() {
        let Some(ctx) = crate::tests::support::headless_context("gpu_life_test_device", wgpu::Features::empty()) else {
            log::warn!("skipping the_alive_count_matches_the_cpu_model: no wgpu adapter available");
            return;
        };

        // 50 is neither a multiple of 32 nor a power of two, so the ragged last word is covered.
        let params = vec![ParamValue::U32(50), ParamValue::U32(30), ParamValue::F32(0.3)];
        let mut gpu = GpuGridState::<GpuLifeModel>::new(&ctx, &params);
        let mut cpu = GridModelState::<LifeModel>::from_params(&params);

        for tick in 0..10 {
            refresh_stats(&ctx, &mut gpu);
            assert_eq!(
                alive(&gpu.stats()),
                alive(&cpu.stats()),
                "the GPU alive count must match the CPU model's at tick {tick}"
            );

            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            gpu.encode_steps(&mut encoder, 1, None);
            ctx.queue.submit(Some(encoder.finish()));
            cpu.step();
        }

        assert!(
            alive(&cpu.stats()) > 0,
            "the grid died out, so the comparison proves nothing"
        );
    }
}
