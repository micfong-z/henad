//! GPU ants, [`crate::ants`] with its population and pheromone field in GPU buffers.
//!
//! Tick 0 is bit identical since seeding goes through [`AntsModel::init`] and
//! [`PheromoneField::build_sites`]. After that the RNG streams differ, for the reason
//! [`crate::gpu_sir`] gives. Deposits still combine with `max`, which is order independent, so
//! unlike [`crate::gpu_boids`] a run does replay.

use henad_compute::cpu::agent_engine::{AGENT_INIT_SEED, agent_model_param_descriptors};
use henad_compute::cpu::field::scalar::ScalarFieldSpec as _;
use henad_core::authoring::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::field::Extent;
use henad_core::authoring::gpu_agent_model::{
    Binding, BufferSpec, DisplaySpec, Domain, Geometry, GpuAgentModel, PassCtx, PassId, PassSpec,
};
use henad_core::helpers::{extract_f32, extract_u32, mix_seed};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use crate::ants::field::{CELL_PALETTE, EMPTY, LOW_PHEROMONE, PheromoneField};
use crate::ants::{ANT_PALETTE, AntLanes, AntsModel};
use crate::shader_bindings::gpu_ants::display::Params as DisplayParams;
use crate::shader_bindings::gpu_ants::merge::Params as MergeParams;
use crate::shader_bindings::gpu_ants::step::Params as StepParams;

/// The list is [`agent_model_param_descriptors`] for [`AntsModel`] verbatim, so both backends take
/// the same vector. Only these three are read here, the rest go through the two `from_params`.
const PARAM_NUM_AGENTS: usize = 0;
const PARAM_WORLD_WIDTH: usize = 1;
const PARAM_WORLD_HEIGHT: usize = 2;

/// Indices into [`GpuAnts::BUFFERS`].
const POS: usize = 0;
const STATE: usize = 1;
const COLOR: usize = 2;
const RNG: usize = 3;
const FIELD: usize = 4;
const ACCUM: usize = 5;
const SITES: usize = 6;

/// Domain separated from the ant seeding stream, so the two do not start correlated.
const RNG_INIT_SEED: u64 = AGENT_INIT_SEED ^ 0x5EED_5EED_5EED_5EED;

/// `state` packs what the CPU model keeps in three lanes. Mirrored in `step.wgsl`.
const HAS_FOOD_BIT: u32 = 0b01_00000000; // 0x100
const HAS_REWARD_BIT: u32 = 0b10_00000000; // 0x200

/// Matches `Params` in the generated reduce leaf.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ReduceParams {
    n: u32,
    lanes: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    _pad: [u32; 3],
}

pub struct GpuAnts;

impl GpuAgentModel for GpuAnts {
    const NAME: &'static str = "Ant Foraging (GPU)";
    const ID: &'static str = "gpu_ants";
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, stepped entirely on the GPU";

    const STATS: &'static [StatDescriptor] = AntsModel::STATS;

    /// Nothing is double buffered. Ants never read one another, and deposits land in `accum`
    /// rather than in the field the step is reading.
    const BUFFERS: &'static [BufferSpec] = &[
        BufferSpec {
            label: "pos",
            double_buffered: false,
            drawable: true,
        },
        BufferSpec {
            label: "state",
            double_buffered: false,
            drawable: false,
        },
        BufferSpec {
            label: "color",
            double_buffered: false,
            drawable: true,
        },
        BufferSpec {
            label: "rng",
            double_buffered: false,
            drawable: false,
        },
        BufferSpec {
            label: "field",
            double_buffered: false,
            drawable: false,
        },
        BufferSpec {
            label: "accum",
            double_buffered: false,
            drawable: false,
        },
        BufferSpec {
            label: "sites",
            double_buffered: false,
            drawable: false,
        },
    ];
    const POS_BUFFER: usize = POS;
    const COLOR_BUFFER: usize = COLOR;

    /// Cumulative deliveries, so unlike a reduction target it is never cleared.
    const COUNTERS: usize = 1;

    const STEP_PASSES: &'static [PassSpec] = &[
        PassSpec {
            label: "step",
            shader: crate::shader_bindings::gpu_ants::step::SHADER_STRING,
            bindings: &[
                Binding::Write(POS),
                Binding::Write(STATE),
                Binding::Write(COLOR),
                Binding::Write(RNG),
                Binding::Read(FIELD),
                Binding::Write(ACCUM),
                Binding::Read(SITES),
                Binding::Counters,
                Binding::Uniform,
            ],
            domain: Domain::Agents,
        },
        PassSpec {
            label: "merge",
            shader: crate::shader_bindings::gpu_ants::merge::SHADER_STRING,
            bindings: &[Binding::Write(FIELD), Binding::Write(ACCUM), Binding::Uniform],
            domain: Domain::Cells(2),
        },
    ];

    const DISPLAY: Option<DisplaySpec> = Some(DisplaySpec {
        shader: crate::shader_bindings::gpu_ants::display::SHADER_STRING,
        bindings: &[
            Binding::Read(FIELD),
            Binding::Read(SITES),
            Binding::DisplayTexture,
            Binding::Uniform,
        ],
        workgroup: 16,
    });

    /// Carrying food, total pheromone. Deliveries is an accumulating counter, not a reduction.
    const REDUCE_LANES: usize = 2;
    /// The two lanes have different domains, so the tree covers the longer one.
    const REDUCE_DOMAIN: Domain = Domain::AgentsOrCells;
    const REDUCE_BINDINGS: &'static [Binding] = &[
        Binding::Read(STATE),
        Binding::Read(FIELD),
        Binding::ReducePartials,
        Binding::Uniform,
    ];
    const REDUCE_HEADER: &'static str = r"
struct Params {
    n: u32,
    lanes: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read> field: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

const HAS_FOOD_BIT: u32 = 0x100u;
";
    /// One lane is per ant and the other per cell, so each bounds-checks its own domain.
    const REDUCE_VALUE: &'static str = r"
        if (lane == 0u) {
            if (i < params.num_agents) {
                value = f32((state[i] & HAS_FOOD_BIT) != 0u);
            }
        } else {
            if (i < params.n_cells) {
                value = field[i] + field[params.n_cells + i];
            }
        }
";

    fn param_descriptors() -> Vec<ParamDescriptor> {
        agent_model_param_descriptors::<AntsModel>()
    }

    fn dims(params: &[ParamValue]) -> (u32, Extent) {
        (
            extract_u32(params, PARAM_NUM_AGENTS, AntsModel::DEFAULT_AGENTS),
            Extent {
                w: extract_f32(params, PARAM_WORLD_WIDTH, AntsModel::DEFAULT_EXTENT.w),
                h: extract_f32(params, PARAM_WORLD_HEIGHT, AntsModel::DEFAULT_EXTENT.h),
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

        // Seeding through the model's own `init` is what keeps tick 0 bit identical. A port would
        // be free to drift.
        let mut lanes = AntLanes::alloc(n);
        let mut rng_state = seed.map_or(AGENT_INIT_SEED, mix_seed);
        AntsModel::init(&mut lanes, geom.extent, params, &mut rng_state);

        let positions: Vec<f32> = lanes
            .pos_x
            .iter()
            .zip(&lanes.pos_y)
            .flat_map(|(&x, &y)| [x, y])
            .collect();
        let packed: Vec<u32> = (0..n).map(|i| pack_state(&lanes, i)).collect();
        // The CPU lane holds palette indices, this one is drawn directly so it holds colours.
        let colors: Vec<u32> = lanes.has_food.iter().map(|&f| packed_ant_color(f)).collect();
        let rng_seed = seed.map_or(RNG_INIT_SEED, |s| mix_seed(s ^ RNG_INIT_SEED));

        // Through the field spec, so the two backends cannot place the nest differently.
        let mut site_bytes = vec![EMPTY; n_cells];
        PheromoneField::build_sites(geom.width, geom.height, &mut site_bytes);
        let site_words: Vec<u32> = site_bytes.iter().map(|&s| u32::from(s)).collect();

        vec![
            bytemuck::cast_slice(&positions).to_vec(),
            bytemuck::cast_slice(&packed).to_vec(),
            bytemuck::cast_slice(&colors).to_vec(),
            bytemuck::cast_slice(&seed_rng_states(n, rng_seed)).to_vec(),
            // The field starts empty, and `accum` is read before it is first written.
            Vec::new(),
            Vec::new(),
            bytemuck::cast_slice(&site_words).to_vec(),
        ]
    }

    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        let geom = ctx.geom;
        match pass {
            PassId::Step(0) => {
                let hot = AntsModel::from_params(params);
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
                lanes: Self::REDUCE_LANES as u32,
                groups_x: ctx.groups_x,
                num_agents: geom.num_agents,
                n_cells: geom.n_cells,
                _pad: [0; 3],
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

/// Matches `pcg_hash` in `step.wgsl` bit-for-bit (u32 arithmetic wraps identically on both sides).
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn seed_rng_states(n: usize, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..n).map(|i| pcg_hash(seed32 ^ i as u32)).collect()
}

/// Packed for the step uniform, from the one palette in `ants` so colours cannot drift.
fn packed_ant_palette() -> [u32; 2] {
    [u32::from_le_bytes(ANT_PALETTE[0]), u32::from_le_bytes(ANT_PALETTE[1])]
}

fn packed_ant_color(index: u8) -> u32 {
    let rgba = ANT_PALETTE.get(index as usize).copied().unwrap_or(ANT_PALETTE[0]);
    u32::from_le_bytes(rgba)
}

/// Same, for the display uniform. Indexed as `palette[i >> 2][i & 3]`.
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
    use crate::ants::field::{FOOD, HOME, OBSTACLE, TO_FOOD, TO_HOME};
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_compute::gpu::{GpuAgentState, GpuContext};
    use henad_core::model::SimState as _;

    type State = GpuAgentState<GpuAnts>;

    fn headless_context() -> Option<GpuContext> {
        crate::gpu_test_support::headless_context("gpu_ants_test_device", wgpu::Features::empty())
    }

    fn params(num_agents: u32, world: f32) -> Vec<ParamValue> {
        let mut values: Vec<ParamValue> = GpuAnts::param_descriptors()
            .iter()
            .map(|desc| desc.kind.default_value())
            .collect();
        values[PARAM_NUM_AGENTS] = ParamValue::U32(num_agents);
        values[PARAM_WORLD_WIDTH] = ParamValue::F32(world);
        values[PARAM_WORLD_HEIGHT] = ParamValue::F32(world);
        values
    }

    /// Current positions, as the two scalar lanes the CPU model keeps.
    fn positions(state: &State) -> (Vec<f32>, Vec<f32>) {
        let floats: Vec<f32> = state.read_buffer(POS).iter().map(|&w| f32::from_bits(w)).collect();
        (
            floats.iter().step_by(2).copied().collect(),
            floats.iter().skip(1).step_by(2).copied().collect(),
        )
    }

    /// The CPU model only ever stores `0.0` or the reward param in its reward lane, which is what
    /// lets the GPU port carry it as one bit of `state` and stay inside eight storage buffers.
    #[test]
    fn the_cpu_reward_lane_only_ever_holds_two_values() {
        let values = params(500, 200.0);
        let reward = AntsModel::from_params(&values).reward;
        let mut state = AgentModelState::<AntsModel>::from_params(&values);
        for tick in 0..200 {
            state.step();
            for (i, &r) in state.lanes().reward.iter().enumerate() {
                assert!(
                    r == 0.0 || r == reward,
                    "ant {i} holds reward {r} on tick {tick}, which is neither 0 nor {reward}"
                );
            }
        }
    }

    /// Both backends seed through `AntsModel::init`, so any later divergence is the step's.
    #[test]
    fn the_initial_colony_matches_the_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping the_initial_colony_matches_the_cpu_model: no adapter");
            return;
        };

        let values = params(2_000, 200.0);
        let gpu = State::new(&ctx, &values);
        let cpu = AgentModelState::<AntsModel>::from_params(&values);

        let (pos_x, pos_y) = positions(&gpu);
        let cpu_lanes = cpu.lanes();
        assert_eq!(pos_x, cpu_lanes.pos_x, "initial x positions must match the CPU model");
        assert_eq!(pos_y, cpu_lanes.pos_y, "initial y positions must match the CPU model");
    }

    /// The reference is bounded, not toroidal like the other models.
    #[test]
    fn ants_stay_inside_the_bounded_field() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping ants_stay_inside_the_bounded_field: no adapter");
            return;
        };

        let world = 200.0f32;
        let mut state = State::new(&ctx, &params(2_000, world));
        state.run_batched(200);

        let (pos_x, pos_y) = positions(&state);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "ant {i} left the field at ({x}, {y})"
            );
        }
    }

    /// The momentum and random action fallbacks are the easy ones to forget an obstacle check in.
    #[test]
    fn ants_never_enter_an_obstacle() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping ants_never_enter_an_obstacle: no adapter");
            return;
        };

        let mut sites = vec![EMPTY; 200 * 200];
        PheromoneField::build_sites(200, 200, &mut sites);

        let mut state = State::new(&ctx, &params(2_000, 200.0));
        state.run_batched(200);

        let (pos_x, pos_y) = positions(&state);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            let c = (y as usize) * 200 + (x as usize);
            assert_ne!(sites[c], OBSTACLE, "ant {i} is inside an obstacle");
        }
    }

    /// The whole point of the model. Nothing below happens if the deposit never lands, if the
    /// merge never runs, or if the trail is followed in the wrong direction.
    #[test]
    fn the_colony_lays_a_trail_and_delivers_food() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping the_colony_lays_a_trail_and_delivers_food: no adapter");
            return;
        };

        let mut state = State::new(&ctx, &params(2_000, 200.0));
        state.run_batched(1_500);
        state.refresh_stats();

        let stats = state.stats();
        let scalar = |i: usize| match &stats[i].value {
            StatValue::Scalar(v) => *v,
            other => panic!("ants report scalars, got {other:?}"),
        };
        assert!(
            scalar(2) > 0.0,
            "1500 ticks of depositing and the field holds no pheromone at all"
        );
        assert!(scalar(0) > 0.0, "no ant is carrying food after 1500 ticks");
        assert!(scalar(1) > 0.0, "no ant has ever delivered food home after 1500 ticks");
    }

    /// Cross-checks the reduction against the field it summed, which a stride mistake in the
    /// two-lane layout would not survive.
    #[test]
    fn total_pheromone_agrees_with_the_field() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping total_pheromone_agrees_with_the_field: no adapter");
            return;
        };

        let mut state = State::new(&ctx, &params(1_000, 100.0));
        state.run_batched(200);
        state.refresh_stats();

        let reference: f64 = state
            .read_buffer(FIELD)
            .iter()
            .map(|&w| f64::from(f32::from_bits(w)))
            .sum();

        let StatValue::Scalar(total) = state.stats()[2].value else {
            panic!("total pheromone is a scalar");
        };
        assert!(
            (total - reference).abs() <= 1e-3 * reference.abs().max(1.0),
            "reduced total pheromone {total} disagrees with the field: {reference}"
        );
        assert!(reference > 0.0, "the field should hold pheromone after 200 ticks");
    }

    /// Unlike `gpu_boids`, nothing here depends on the order the GPU schedules work: deposits
    /// combine with `max`, and no ant reads another's lanes. So a run must replay exactly.
    #[test]
    fn a_run_replays_bit_identically() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping a_run_replays_bit_identically: no adapter");
            return;
        };

        let run = || {
            let mut state = State::new(&ctx, &params(4_000, 200.0));
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

    /// Exercises the 2D dispatch fold in the step and merge kernels at once.
    #[test]
    fn a_population_past_one_workgroup_row_still_steps() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping a_population_past_one_workgroup_row_still_steps: no adapter");
            return;
        };

        // Not a multiple of the workgroup width, so the ragged tail is covered.
        let world = 1_000.0f32;
        let mut state = State::new(&ctx, &params(300_037, world));
        state.run_batched(5);

        let (pos_x, pos_y) = positions(&state);
        assert_eq!(pos_x.len(), 300_037);
        for (i, (&x, &y)) in pos_x.iter().zip(&pos_y).enumerate() {
            assert!(
                (0.0..world).contains(&x) && (0.0..world).contains(&y),
                "ant {i} left the field at ({x}, {y}); the dispatch fold probably missed it"
            );
        }
    }

    /// The site markers are what the ants navigate between, so a layout mismatch would make the
    /// two backends different models.
    #[test]
    fn the_site_layout_matches_the_cpu_field() {
        let mut sites = vec![EMPTY; 200 * 200];
        PheromoneField::build_sites(200, 200, &mut sites);
        assert!(sites.contains(&HOME) && sites.contains(&FOOD) && sites.contains(&OBSTACLE));
        assert_eq!(TO_FOOD, 0, "the field buffer lays out to-food first");
        assert_eq!(TO_HOME, 1, "the field buffer lays out to-home second");
    }
}
