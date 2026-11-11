//! GPU ants, [`crate::ants`] with its population and pheromone field in GPU buffers.
//!
//! Tick 0 is bit identical since seeding uses [`AntsModel::init`] and
//! [`PheromoneField::build_sites`]. After that the RNG streams differ, as
//! [`crate::gpu_sir`] describes. Deposits still combine with `max`, which is order independent, so
//! unlike [`crate::gpu_boids`] a run does replay.

use henad_compute::cpu::agent_engine::{
    AGENT_INIT_SEED, NUM_AGENTS, WORLD_HEIGHT, WORLD_WIDTH, agent_init_rng, agent_model_param_descriptors, split_params,
};
use henad_compute::cpu::field::scalar::ScalarFieldSpec as _;
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{
    BufferSpec, DisplaySpec, Domain, Geometry, GpuAgentAction, GpuAgentModel, PassCtx, PassId, PassSpec, ReduceSpec,
};
use henad_core::helpers::{extract_f32, extract_u32};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use crate::ants::field::{CELL_PALETTE, EMPTY, LOW_PHEROMONE, PheromoneField, nest_position};
use crate::ants::{ANT_PALETTE, AntLanes, AntsModel};
use crate::port::{interleave, pack_palette, packed_color, pcg_states, rng_buffer_seed};
use crate::shader_bindings::gpu_ants::display::Params as DisplayParams;
use crate::shader_bindings::gpu_ants::merge::Params as MergeParams;
use crate::shader_bindings::gpu_ants::reduce::Params as ReduceParams;
use crate::shader_bindings::gpu_ants::reset_colony::Params as ActionParams;
use crate::shader_bindings::gpu_ants::state::{HAS_FOOD_BIT, HAS_REWARD_BIT};
use crate::shader_bindings::gpu_ants::step::Params as StepParams;

// The param list is [`agent_model_param_descriptors`] for [`AntsModel`] verbatim, so both backends
// take the same vector. Only the three engine parameters are read here, under the names
// that `cpu::agent_engine` assigns them. The other parameters come from `AntsModel::from_params` and
// `PheromoneField::from_params`.

henad_core::buffers! {
    const POS = "pos" drawable;
    const STATE = "state";
    const COLOR = "color" drawable;
    const RNG = "rng";
    const FIELD = "field";
    const ACCUM = "accum";
    const SITES = "sites";
}

/// Ant foraging as a [`GpuAgentModel`], seeded through [`AntsModel::init`].
#[derive(Debug)]
pub struct GpuAnts;

impl GpuAgentModel for GpuAnts {
    const NAME: &'static str = "Ant Foraging (GPU)";
    const ID: &'static str = "gpu_ants";
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, stepped entirely on the GPU";

    const STATS: &'static [StatDescriptor] = AntsModel::STATS;

    /// Nothing is double buffered. Ants never read one another, and deposits land in `accum`
    /// rather than in the field the step is reading.
    const BUFFERS: &'static [BufferSpec] = BUFFER_SPECS;
    const POS_BUFFER: usize = POS;
    const COLOR_BUFFER: usize = COLOR;

    /// Cumulative deliveries, so unlike a reduction target it is never cleared.
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

    const ACTIONS: &'static [GpuAgentAction] = &[GpuAgentAction {
        desc: ActionDescriptor::new("reset_colony", "Reset colony"),
        pass: PassSpec {
            label: "reset_colony",
            shader: crate::shader_bindings::gpu_ants::reset_colony::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_ANTS_RESET_COLONY,
            // The domain covers the ants and both field layers, so neither half is left short.
            domain: Domain::AgentsOrCells,
        },
    }];

    /// Two lanes, the ants carrying food and the total pheromone. Deliveries accumulate in a counter instead.
    const REDUCE: ReduceSpec = ReduceSpec {
        shader: crate::shader_bindings::gpu_ants::reduce::SHADER_STRING,
        bindings: crate::binding_decls::bindings::GPU_ANTS_REDUCE,
        lanes: 2,
        // The two lanes have different domains, so the tree covers the longer domain.
        domain: Domain::AgentsOrCells,
    };

    fn param_descriptors() -> Vec<ParamDescriptor> {
        agent_model_param_descriptors::<AntsModel>()
    }

    fn dims(params: &[ParamValue]) -> (u32, Extent) {
        (
            extract_u32(params, NUM_AGENTS, AntsModel::DEFAULT_AGENTS),
            Extent {
                w: extract_f32(params, WORLD_WIDTH, AntsModel::DEFAULT_EXTENT.w),
                h: extract_f32(params, WORLD_HEIGHT, AntsModel::DEFAULT_EXTENT.h),
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

        // Seeding through the model's own `init` keeps tick 0 bit identical with the CPU model.
        let mut lanes = AntLanes::alloc(n);
        let mut rng_state = agent_init_rng(seed);
        AntsModel::init(
            &mut lanes,
            geom.extent,
            split_params::<AntsModel>(params).0,
            &mut rng_state,
        );

        let packed: Vec<u32> = (0..n).map(|i| pack_state(&lanes, i)).collect();
        // The CPU lane holds palette indices. The GPU draws this buffer directly, and it holds colours.
        let colors: Vec<u32> = lanes.has_food.iter().map(|&f| packed_color(&ANT_PALETTE, f)).collect();

        // The sites come from the field spec, so the two backends cannot place the nest differently.
        let mut site_bytes = vec![EMPTY; n_cells];
        PheromoneField::build_sites(geom.width, geom.height, &mut site_bytes);
        let site_words: Vec<u32> = site_bytes.iter().map(|&s| u32::from(s)).collect();

        vec![
            bytemuck::cast_slice(&interleave(&lanes.pos_x, &lanes.pos_y)).to_vec(),
            bytemuck::cast_slice(&packed).to_vec(),
            bytemuck::cast_slice(&colors).to_vec(),
            bytemuck::cast_slice(&pcg_states(n, rng_buffer_seed(seed, AGENT_INIT_SEED))).to_vec(),
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
                let hot = AntsModel::from_params(split_params::<AntsModel>(params).0, Self::dims(params).1);
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
                    palette: ANT_PALETTE.map(u32::from_le_bytes),
                })
                .to_vec()
            }
            PassId::Step(_) => bytemuck::bytes_of(&MergeParams {
                n: ctx.invocations,
                groups_x: ctx.groups_x,
                evaporation: PheromoneField::from_params(split_params::<AntsModel>(params).1).evaporation,
                low: LOW_PHEROMONE,
            })
            .to_vec(),
            PassId::Display => bytemuck::bytes_of(&DisplayParams {
                width: geom.width,
                height: geom.height,
                n_cells: geom.n_cells,
                _pad: 0,
                tex: geom.display.into(),
                _pad2: [0; 2],
                palette: pack_palette(&CELL_PALETTE),
            })
            .to_vec(),
            PassId::Action(_) => bytemuck::bytes_of(&ActionParams {
                n: ctx.invocations,
                groups_x: ctx.groups_x,
                num_agents: geom.num_agents,
                n_cells: geom.n_cells,
                nest: nest_position(geom.width, geom.height).into(),
                color: u32::from_le_bytes(ANT_PALETTE[0]),
                _pad: 0,
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

/// Packs ant `i`'s last step, food flag and reward flag into one word, as `step.wgsl` reads them.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ants::field::{FOOD, HOME, OBSTACLE, TO_FOOD, TO_HOME};
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_compute::gpu::{GpuAgentState, GpuContext};
    use henad_core::model::SimState as _;
    use henad_explore::testing::{TestDeviceRequest, headless_test_device};

    type State = GpuAgentState<GpuAnts>;

    fn headless_context() -> Option<GpuContext> {
        headless_test_device(&TestDeviceRequest::baseline())
    }

    fn params(num_agents: u32, world: f32) -> Vec<ParamValue> {
        let mut values: Vec<ParamValue> = GpuAnts::param_descriptors()
            .iter()
            .map(|desc| desc.kind().default_value())
            .collect();
        values[NUM_AGENTS] = ParamValue::U32(num_agents);
        values[WORLD_WIDTH] = ParamValue::F32(world);
        values[WORLD_HEIGHT] = ParamValue::F32(world);
        values
    }

    /// Returns the current positions as the two scalar lanes the CPU model keeps.
    fn positions(state: &State) -> (Vec<f32>, Vec<f32>) {
        let floats: Vec<f32> = state.read_buffer(POS).iter().map(|&w| f32::from_bits(w)).collect();
        (
            floats.iter().step_by(2).copied().collect(),
            floats.iter().skip(1).step_by(2).copied().collect(),
        )
    }

    /// The CPU model only ever stores `0.0` or the reward param in its reward lane.
    ///
    /// The GPU port carries the lane as one bit of `state`, and stays inside eight storage buffers.
    #[test]
    fn the_cpu_reward_lane_only_ever_holds_two_values() {
        let values = params(500, 200.0);
        let reward = AntsModel::from_params(split_params::<AntsModel>(&values).0, GpuAnts::dims(&values).1).reward;
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

    /// Both backends seed through `AntsModel::init`, by default and from a seed, so any later divergence comes from
    /// the step.
    #[test]
    fn the_initial_colony_matches_the_cpu_model() {
        let Some(ctx) = headless_context() else {
            log::warn!("skipping the_initial_colony_matches_the_cpu_model: no adapter");
            return;
        };

        let values = params(2_000, 200.0);
        for seed in [None, Some(7)] {
            let gpu = State::new_seeded(&ctx, &values, seed);
            let cpu = AgentModelState::<AntsModel>::from_params_seeded(&values, seed);

            let (pos_x, pos_y) = positions(&gpu);
            let cpu_lanes = cpu.lanes();
            assert_eq!(pos_x, cpu_lanes.pos_x, "initial x positions differ for seed {seed:?}");
            assert_eq!(pos_y, cpu_lanes.pos_y, "initial y positions differ for seed {seed:?}");
        }
    }

    /// The reference's field is bounded, unlike the toroidal worlds of the other models.
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

    /// An obstacle check is easy to forget in the momentum and random action fallbacks.
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

    /// Checks the behaviour the model exists for. Nothing below happens if the deposit never lands, if the merge never
    /// runs, or if the trail is followed in the wrong direction.
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
        let scalar = |i: usize| match stats[i].value() {
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

        let StatValue::Scalar(total) = *state.stats()[2].value() else {
            panic!("total pheromone is a scalar");
        };
        assert!(
            (total - reference).abs() <= 1e-3 * reference.abs().max(1.0),
            "reduced total pheromone {total} disagrees with the field: {reference}"
        );
        assert!(reference > 0.0, "the field should hold pheromone after 200 ticks");
    }

    /// Unlike `gpu_boids`, nothing here depends on the order the GPU schedules work. Deposits
    /// combine with `max`, and no ant reads another's lanes. A run must therefore replay exactly.
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

        // The count is not a multiple of the workgroup width, so the ragged tail is covered.
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

    /// The field's parameters sit after the model's own parameters in the composed list, and the merge
    /// reads them from there.
    #[test]
    fn the_merge_pass_reads_the_evaporation_param() {
        let mut values = params(1_000, 200.0);
        let Some(index) = GpuAnts::param_descriptors()
            .iter()
            .position(|desc| desc.id() == "evaporation")
        else {
            panic!("gpu_ants declares no evaporation parameter");
        };
        values[index] = ParamValue::F32(0.95);

        let geom = State::geometry_for(&values, &wgpu::Limits::default());
        let ctx = PassCtx {
            geom: &geom,
            invocations: geom.n_cells * 2,
            groups_x: 1,
            seed: 0,
        };
        let bytes = GpuAnts::pass_params_bytes(PassId::Step(1), ctx, &values);
        let merge = bytemuck::pod_read_unaligned::<MergeParams>(&bytes);
        assert_eq!(
            merge.evaporation, 0.95,
            "the merge uniform ignores the evaporation parameter"
        );
    }

    /// The ants navigate between the site markers, and a layout mismatch would make the two backends
    /// different models.
    #[test]
    fn the_site_layout_matches_the_cpu_field() {
        let mut sites = vec![EMPTY; 200 * 200];
        PheromoneField::build_sites(200, 200, &mut sites);
        assert!(sites.contains(&HOME) && sites.contains(&FOOD) && sites.contains(&OBSTACLE));
        assert_eq!(TO_FOOD, 0, "the field buffer lays out to-food first");
        assert_eq!(TO_HOME, 1, "the field buffer lays out to-home second");
    }
}
