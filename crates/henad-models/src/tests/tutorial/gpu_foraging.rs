//! GPU ant foraging as `docs/guide/first-model/gpu-ants.md` builds it.
//!
//! The id is `gpu_foraging` rather than `gpu_ants`, since the shipped model already holds that one
//! and the page tells a reader the same thing.
//!
//! The shaders are the shipped model's own. A shader carries no id, so what the page writes is
//! `gpu_ants/*.wgsl` line for line, and this module binds those rather than carrying a second
//! copy. The page spells the generated paths `gpu_foraging`, after the directory a reader makes.

use henad_compute::cpu::agent_engine::{
    AGENT_INIT_SEED, NUM_AGENTS, WORLD_HEIGHT, WORLD_WIDTH, agent_model_param_descriptors, split_params,
};
use henad_compute::cpu::field::scalar::ScalarFieldSpec as _;
use henad_core::authoring::model::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{
    BufferSpec, DisplaySpec, Domain, Geometry, GpuAgentModel, PassCtx, PassId, PassSpec, ReduceSpec,
};
use henad_core::authoring::primitives::rng::mix_seed;
use henad_core::helpers::{extract_f32, extract_u32};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use super::foraging::field::{CELL_PALETTE, EMPTY, LOW_PHEROMONE, PheromoneField};
use super::foraging::{ANT_PALETTE, AntLanes, ForagingModel};
use crate::shader_bindings::gpu_ants::display::Params as DisplayParams;
use crate::shader_bindings::gpu_ants::merge::Params as MergeParams;
use crate::shader_bindings::gpu_ants::reduce::Params as ReduceParams;
use crate::shader_bindings::gpu_ants::step::Params as StepParams;

henad_core::buffers! {
    const POS = "pos" drawable;
    const STATE = "state";
    const COLOR = "color" drawable;
    const RNG = "rng";
    const FIELD = "field";
    const ACCUM = "accum";
    const SITES = "sites";
}

/// `state` packs what the CPU model keeps in three lanes. Mirrored in `state.wgsl`.
const HAS_FOOD_BIT: u32 = 0b01_00000000; // 0x100
const HAS_REWARD_BIT: u32 = 0b10_00000000; // 0x200

/// Domain separated from the ant seeding stream, so the two do not start correlated.
const RNG_INIT_SEED: u64 = AGENT_INIT_SEED ^ 0x5EED_5EED_5EED_5EED;

pub struct GpuForagingModel;

impl GpuAgentModel for GpuForagingModel {
    const NAME: &'static str = "Ant Foraging (GPU)";
    const ID: &'static str = "gpu_foraging";
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, stepped entirely on the GPU";
    const STATS: &'static [StatDescriptor] = ForagingModel::STATS;

    const BUFFERS: &'static [BufferSpec] = SPECS;
    const POS_BUFFER: usize = POS;
    const COLOR_BUFFER: usize = COLOR;

    const COUNTERS: usize = 1;

    const STEP_PASSES: &'static [PassSpec] = &[
        PassSpec {
            label: "step",
            shader: crate::shader_bindings::gpu_ants::step::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_ANTS_STEP,
            domain: Domain::Agents,
        },
        PassSpec {
            label: "merge",
            shader: crate::shader_bindings::gpu_ants::merge::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_ANTS_MERGE,
            domain: Domain::Cells(2),
        },
    ];

    const DISPLAY: Option<DisplaySpec> = Some(DisplaySpec {
        shader: crate::shader_bindings::gpu_ants::display::SHADER_STRING,
        bindings: crate::binding_decls::bindings::GPU_ANTS_DISPLAY,
        workgroup: 16,
    });

    const REDUCE: ReduceSpec = ReduceSpec {
        shader: crate::shader_bindings::gpu_ants::reduce::SHADER_STRING,
        bindings: crate::binding_decls::bindings::GPU_ANTS_REDUCE,
        lanes: 2,
        domain: Domain::AgentsOrCells,
    };

    fn param_descriptors() -> Vec<ParamDescriptor> {
        agent_model_param_descriptors::<ForagingModel>()
    }

    fn dims(params: &[ParamValue]) -> (u32, Extent) {
        (
            extract_u32(params, NUM_AGENTS, ForagingModel::DEFAULT_AGENTS),
            Extent {
                w: extract_f32(params, WORLD_WIDTH, ForagingModel::DEFAULT_EXTENT.w),
                h: extract_f32(params, WORLD_HEIGHT, ForagingModel::DEFAULT_EXTENT.h),
            },
        )
    }

    fn buffer_lens(geom: &Geometry) -> Vec<usize> {
        let n = geom.num_agents as usize;
        let cells = geom.n_cells as usize;
        vec![n * 2, n, n, n, cells * 2, cells * 2, cells]
    }

    fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        let n = geom.num_agents as usize;
        let n_cells = geom.n_cells as usize;

        let mut lanes = AntLanes::alloc(n);
        let mut rng_state = seed.map_or(AGENT_INIT_SEED, mix_seed);
        ForagingModel::init(
            &mut lanes,
            geom.extent,
            split_params::<ForagingModel>(params).0,
            &mut rng_state,
        );

        let positions: Vec<f32> = lanes
            .pos_x
            .iter()
            .zip(&lanes.pos_y)
            .flat_map(|(&x, &y)| [x, y])
            .collect();
        let packed: Vec<u32> = (0..n).map(|i| pack_state(&lanes, i)).collect();
        let colors: Vec<u32> = lanes.has_food.iter().map(|&f| packed_ant_color(f)).collect();
        let rng_seed = seed.map_or(RNG_INIT_SEED, |s| mix_seed(s ^ RNG_INIT_SEED));

        let mut site_bytes = vec![EMPTY; n_cells];
        PheromoneField::build_sites(geom.width, geom.height, &mut site_bytes);
        let site_words: Vec<u32> = site_bytes.iter().map(|&s| u32::from(s)).collect();

        vec![
            bytemuck::cast_slice(&positions).to_vec(),
            bytemuck::cast_slice(&packed).to_vec(),
            bytemuck::cast_slice(&colors).to_vec(),
            bytemuck::cast_slice(&seed_rng_states(n, rng_seed)).to_vec(),
            Vec::new(),
            Vec::new(),
            bytemuck::cast_slice(&site_words).to_vec(),
        ]
    }

    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        let geom = ctx.geom;
        match pass {
            PassId::Step(0) => {
                let hot = ForagingModel::from_params(split_params::<ForagingModel>(params).0, Self::dims(params).1);
                bytemuck::bytes_of(&StepParams {
                    num_agents: geom.num_agents,
                    groups_x: ctx.groups_x,
                    grid_w: geom.width,
                    grid_h: geom.height,
                    n_cells: geom.n_cells,
                    cutdown: hot.cutdown,
                    diagonal: hot.diagonal,
                    reward: hot.reward,
                    momentum: hot.momentum,
                    random_action: hot.random_action,
                    palette: packed_ant_palette(),
                })
                .to_vec()
            }
            PassId::Step(_) => bytemuck::bytes_of(&MergeParams {
                n: ctx.invocations,
                groups_x: ctx.groups_x,
                evaporation: PheromoneField::from_params(params).evaporation,
                low: LOW_PHEROMONE,
            })
            .to_vec(),
            PassId::Display => bytemuck::bytes_of(&DisplayParams {
                width: geom.width,
                height: geom.height,
                n_cells: geom.n_cells,
                _pad: 0,
                tex: [geom.display.0, geom.display.1],
                _pad2: [0; 2],
                palette: packed_cell_palette(),
            })
            .to_vec(),
            PassId::Reduce => bytemuck::bytes_of(&ReduceParams {
                n: ctx.invocations,
                lanes: Self::REDUCE.lanes as u32,
                groups_x: ctx.groups_x,
                num_agents: geom.num_agents,
                n_cells: geom.n_cells,
                ..bytemuck::Zeroable::zeroed()
            })
            .to_vec(),
        }
    }

    fn stats(sums: &[f32], counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        vec![
            StatValue::Scalar(f64::from(sums[0])),
            StatValue::Scalar(f64::from(counters[0])),
            StatValue::Scalar(f64::from(sums[1])),
        ]
    }
}

/// The three per-ant scalars the CPU keeps in separate lanes, as `step.wgsl` reads them.
fn pack_state(lanes: &AntLanes, i: usize) -> u32 {
    let mut packed = u32::from(lanes.last_step[i]);
    if lanes.has_food[i] != 0 {
        packed |= HAS_FOOD_BIT;
    }
    if lanes.reward[i] != 0.0 {
        packed |= HAS_REWARD_BIT;
    }
    packed
}

/// Matches `pcg_hash` in `shared::rng` bit for bit, since `u32` arithmetic wraps the same on both sides.
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn seed_rng_states(n: usize, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..n).map(|i| pcg_hash(seed32 ^ i as u32)).collect()
}

fn packed_ant_palette() -> [u32; 2] {
    [u32::from_le_bytes(ANT_PALETTE[0]), u32::from_le_bytes(ANT_PALETTE[1])]
}

fn packed_ant_color(index: u8) -> u32 {
    let rgba = ANT_PALETTE.get(index as usize).copied().unwrap_or(ANT_PALETTE[0]);
    u32::from_le_bytes(rgba)
}

/// Indexed as `palette[i >> 2][i & 3]` in `display.wgsl`.
fn packed_cell_palette() -> [[u32; 4]; 4] {
    let mut packed = [[0u32; 4]; 4];
    for (i, rgba) in CELL_PALETTE.iter().enumerate() {
        packed[i / 4][i % 4] = u32::from_le_bytes(*rgba);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::gpu::GpuAgentState;

    #[test]
    fn a_run_replays_bit_identically() {
        let Some(ctx) = crate::tests::support::headless_context("gpu_foraging_test_device", wgpu::Features::empty())
        else {
            log::warn!("skipping a_run_replays_bit_identically: no wgpu adapter available");
            return;
        };

        let mut params: Vec<ParamValue> = GpuForagingModel::param_descriptors()
            .iter()
            .map(|d| d.kind.default_value())
            .collect();
        params[NUM_AGENTS] = ParamValue::U32(4_000);

        let run = || {
            let mut state = GpuAgentState::<GpuForagingModel>::new(&ctx, &params);
            state.run_batched(300);
            (
                state.read_buffer(POS),
                state.read_buffer(STATE),
                state.read_buffer(FIELD),
            )
        };

        let (pos_a, state_a, field_a) = run();
        let (pos_b, state_b, field_b) = run();
        assert_eq!(pos_a, pos_b, "ant positions are not reproducible");
        assert_eq!(state_a, state_b, "packed ant state is not reproducible");
        assert_eq!(field_a, field_b, "the pheromone field is not reproducible");
    }
}
