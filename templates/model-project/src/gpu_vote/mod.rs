//! The voting rule stepped on the GPU, one `u32` per cell.

use henad::authoring::prelude::*;

use crate::vote::{PALETTE, Vote};

henad::params! {
    const GRID_WIDTH = u32_param("grid_width", "Grid width", 1024, 1, 16_384);
    const GRID_HEIGHT = u32_param("grid_height", "Grid height", 1024, 1, 16_384);
    const DENSITY = f32_param("density", "Initial density", 0.5, 0.0, 1.0, Some(0.01)).on_reload();
}

pub(crate) struct GpuVote;

impl GpuGridModel for GpuVote {
    const NAME: &'static str = "Vote (GPU)";
    const ID: &'static str = "gpu_vote";
    const DESCRIPTION: &'static str = "Each cell takes the majority of its neighbourhood, stepped on the GPU";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Ones", PALETTE[1])];

    const BUFFERS: &'static [&'static str] = &["state"];

    const STEP_SHADER: &'static str = crate::shader_bindings::gpu_vote::step::SHADER_STRING;
    const DISPLAY_SHADER: &'static str = crate::shader_bindings::gpu_vote::display::SHADER_STRING;
    const REDUCE_SHADER: &'static str = crate::shader_bindings::gpu_vote::reduce::SHADER_STRING;

    const STEP_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_VOTE_STEP;
    const DISPLAY_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_VOTE_DISPLAY;
    const REDUCE_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_VOTE_REDUCE;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        (
            extract_u32(params, GRID_WIDTH, 1024),
            extract_u32(params, GRID_HEIGHT, 1024),
        )
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        // Runs the CPU twin's `init` from the engine's starting state, and tick 0 matches the CPU model bit for bit.
        // The port declares `grid_width` and `grid_height` first, where the CPU engine prepends them.
        let mut grid = Grid2D::new(width, height);
        <Vote as GridModel>::init(
            &mut grid,
            &params[GRID_PARAM_BASE.min(params.len())..],
            &mut grid_init_rng(seed),
        );
        vec![grid.current().iter().map(|&cell| u32::from(cell)).collect()]
    }

    fn step_params_bytes(width: u32, height: u32, _params: &[ParamValue]) -> Vec<u8> {
        let uniform = crate::shader_bindings::gpu_vote::step::Params { width, height };
        bytemuck::bytes_of(&uniform).to_vec()
    }

    fn stats(counts: &[u32]) -> Vec<StatValue> {
        vec![StatValue::Scalar(f64::from(counts[0]))]
    }
}
