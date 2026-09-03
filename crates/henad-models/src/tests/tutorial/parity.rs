//! Holds each tutorial's finished code to the model it teaches.
//!
//! Bit equality rather than a tolerance. Both sides run the same kernels through the same engine,
//! so anything less than identical means the page has drifted.

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_compute::cpu::field::scalar::{ScalarField, ScalarFieldSpec};
use henad_compute::cpu::grid_engine::GridModelState;
use henad_core::authoring::model::agent_model::{AgentLanes as _, AgentModel};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::model::SimState as _;
use henad_core::params::{ParamDescriptor, ParamValue};

const SEED: u64 = 0x5EED_0DE5_0DE5_5EED;

/// Ids, labels, defaults and the live/reload flag, in declaration order.
fn descriptor_shape(descs: &[ParamDescriptor]) -> Vec<(&str, &str, ParamValue, bool)> {
    descs
        .iter()
        .map(|d| (d.id, d.label, d.kind.default_value(), d.is_live()))
        .collect()
}

// --- Game of Life ---

/// Cells and stat values after `steps` ticks, as raw bits.
fn run_grid<M: GridModel>(params: &[ParamValue], steps: usize) -> (Vec<u8>, Vec<u64>) {
    let mut state = GridModelState::<M>::from_params_seeded(params, Some(SEED));
    for _ in 0..steps {
        state.step();
    }
    let cells = state.grid_view().expect("a grid model draws a grid").cells.to_vec();
    let stats = state.stats().iter().map(|e| e.value.scalar().to_bits()).collect();
    (cells, stats)
}

#[test]
fn the_game_of_life_tutorial_matches_the_shipped_model() {
    // Not square, so a transposed index shows up.
    let params = vec![ParamValue::U32(64), ParamValue::U32(48), ParamValue::F32(0.3)];

    let (taught_cells, taught_stats) = run_grid::<super::life::LifeModel>(&params, 200);
    let (shipped_cells, shipped_stats) = run_grid::<crate::game_of_life::GameOfLifeModel>(&params, 200);

    assert_eq!(
        taught_cells, shipped_cells,
        "docs/guide/first-model/game-of-life.md no longer produces the shipped model"
    );
    assert_eq!(taught_stats, shipped_stats, "the taught stats reduction has drifted");
    assert!(
        shipped_cells.contains(&1) && shipped_cells.contains(&0),
        "200 ticks left the grid uniform, so the comparison proves nothing"
    );
}

#[test]
fn the_game_of_life_tutorial_declares_the_same_parameters() {
    assert_eq!(
        descriptor_shape(&super::life::LifeModel::param_descriptors()),
        descriptor_shape(&crate::game_of_life::GameOfLifeModel::param_descriptors()),
    );
    assert_eq!(
        super::life::LifeModel::NEIGHBORHOOD,
        crate::game_of_life::GameOfLifeModel::NEIGHBORHOOD,
    );
}

// --- Ants ---

struct AntSnapshot {
    positions: Vec<u32>,
    colors: Vec<u8>,
    tally: u64,
    fields: Vec<u32>,
    display: Vec<u8>,
    stats: Vec<u64>,
}

/// Everything an ant model produces after `steps` ticks, floats as raw bits.
fn run_ants<A, S>(params: &[ParamValue], steps: usize) -> AntSnapshot
where
    A: AgentModel<Field = ScalarField<S>, Tally = u64>,
    S: ScalarFieldSpec,
{
    let mut state = AgentModelState::<A>::from_params_seeded(params, Some(SEED));
    for _ in 0..steps {
        state.step();
    }
    state.prepare_view();

    let (pos_x, pos_y) = state.lanes().positions();
    let mut fields = Vec::new();
    for f in 0..S::FIELDS {
        fields.extend(state.field().field(f).current().iter().map(|v| v.to_bits()));
    }

    AntSnapshot {
        positions: pos_x.iter().chain(pos_y).map(|v| v.to_bits()).collect(),
        colors: state.lanes().colors().expect("ants colour by has_food").to_vec(),
        tally: *state.tally(),
        fields,
        display: state.field().display_cells().to_vec(),
        stats: state.stats().iter().map(|e| e.value.scalar().to_bits()).collect(),
    }
}

#[test]
fn the_ants_tutorial_matches_the_shipped_model() {
    let params = vec![
        ParamValue::U32(500),
        ParamValue::F32(200.0),
        ParamValue::F32(200.0),
        ParamValue::F32(0.9),
        ParamValue::F32(1.0),
        ParamValue::F32(0.8),
        ParamValue::F32(0.1),
        ParamValue::F32(0.999),
    ];
    const STEPS: usize = 400;

    let taught = run_ants::<super::foraging::ForagingModel, super::foraging::field::PheromoneField>(&params, STEPS);
    let shipped = run_ants::<crate::ants::AntsModel, crate::ants::field::PheromoneField>(&params, STEPS);

    assert_eq!(
        taught.positions, shipped.positions,
        "docs/guide/first-model/ants.md no longer moves ants the way the shipped model does"
    );
    assert_eq!(taught.colors, shipped.colors, "the taught has_food lane has drifted");
    assert_eq!(taught.tally, shipped.tally, "the taught delivery tally has drifted");
    assert_eq!(taught.fields, shipped.fields, "the taught deposit or decay has drifted");
    assert_eq!(taught.display, shipped.display, "the taught quantisation has drifted");
    assert_eq!(taught.stats, shipped.stats, "the taught stats reduction has drifted");

    let nest = shipped.positions[0];
    assert!(
        shipped.positions.iter().any(|&p| p != nest),
        "{STEPS} ticks left every ant where it started, so the comparison proves nothing"
    );
}

#[test]
fn the_ants_tutorial_lays_out_the_same_world() {
    let (w, h) = (200u32, 200u32);
    let mut taught = vec![super::foraging::field::EMPTY; (w * h) as usize];
    let mut shipped = vec![crate::ants::field::EMPTY; (w * h) as usize];
    super::foraging::field::PheromoneField::build_sites(w, h, &mut taught);
    crate::ants::field::PheromoneField::build_sites(w, h, &mut shipped);

    assert_eq!(taught, shipped, "the taught site and obstacle layout has drifted");
    assert!(
        taught.contains(&super::foraging::field::OBSTACLE),
        "the obstacle blobs cover no cells, so the comparison proves little"
    );
}

#[test]
fn the_ants_tutorial_declares_the_same_parameters() {
    assert_eq!(
        descriptor_shape(&super::foraging::ForagingModel::param_descriptors()),
        descriptor_shape(&crate::ants::AntsModel::param_descriptors()),
    );
    assert_eq!(
        descriptor_shape(&super::foraging::field::PheromoneField::param_descriptors()),
        descriptor_shape(&crate::ants::field::PheromoneField::param_descriptors()),
    );
    assert_eq!(
        super::foraging::ForagingModel::CHUNK,
        crate::ants::AntsModel::CHUNK,
        "CHUNK sets the rng seeding granularity, so a mismatch changes results"
    );
}

// --- GPU Game of Life ---

/// Params in the order both GPU Life models declare them.
fn gpu_life_params(width: u32, height: u32) -> Vec<ParamValue> {
    vec![ParamValue::U32(width), ParamValue::U32(height), ParamValue::F32(0.3)]
}

#[test]
fn the_gpu_life_tutorial_seeds_the_same_grid() {
    use henad_core::authoring::model::gpu_grid_model::GpuGridModel as _;
    type Taught = super::gpu_life::GpuLifeModel;
    type Shipped = crate::gpu_game_of_life::GpuGameOfLife;

    // A ragged width, so the padding bits are compared too.
    let (w, h) = (50u32, 30u32);
    let params = gpu_life_params(w, h);

    assert_eq!(Taught::dims(&params), Shipped::dims(&params));
    assert_eq!(Taught::buffer_lens(w, h), Shipped::buffer_lens(w, h));
    assert_eq!(Taught::step_dims(w, h), Shipped::step_dims(w, h));
    assert_eq!(
        Taught::step_params_bytes(w, h, &params),
        Shipped::step_params_bytes(w, h, &params)
    );
    assert_eq!(
        Taught::seed_buffers(w, h, &params, Some(SEED)),
        Shipped::seed_buffers(w, h, &params, Some(SEED)),
        "docs/guide/first-model/gpu-game-of-life.md no longer seeds the shipped model's grid"
    );
    assert_eq!(
        descriptor_shape(&Taught::param_descriptors()),
        descriptor_shape(&Shipped::param_descriptors()),
    );
    assert_eq!(Taught::BUFFERS, Shipped::BUFFERS);
    assert_eq!(Taught::STATS.len(), Shipped::STATS.len());
}

#[test]
fn the_gpu_life_tutorial_matches_the_shipped_model() {
    use henad_compute::gpu::grid_engine::GpuGridState;
    use henad_compute::gpu::{GpuContext, GpuSimState};

    let Some(ctx) = crate::tests::support::headless_context("gpu_life_parity_device", wgpu::Features::empty()) else {
        log::warn!("skipping the_gpu_life_tutorial_matches_the_shipped_model: no adapter");
        return;
    };

    fn alive<S: GpuSimState>(ctx: &GpuContext, state: &mut S) -> u64 {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
        state.stats()[0].value.scalar() as u64
    }

    fn step<S: GpuSimState>(ctx: &GpuContext, state: &mut S) {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 1, None);
        ctx.queue.submit(Some(encoder.finish()));
    }

    let params = gpu_life_params(50, 30);
    let mut taught = GpuGridState::<super::gpu_life::GpuLifeModel>::new_seeded(&ctx, &params, Some(SEED));
    let mut shipped = GpuGridState::<crate::gpu_game_of_life::GpuGameOfLife>::new_seeded(&ctx, &params, Some(SEED));

    for tick in 0..20 {
        assert_eq!(
            alive(&ctx, &mut taught),
            alive(&ctx, &mut shipped),
            "docs/guide/first-model/gpu-game-of-life.md no longer produces the shipped model at tick {tick}"
        );
        step(&ctx, &mut taught);
        step(&ctx, &mut shipped);
    }
    assert!(
        alive(&ctx, &mut shipped) > 0,
        "20 ticks left the grid empty, so the comparison proves nothing"
    );
}

// --- GPU ants ---

fn gpu_foraging_params(num_agents: u32) -> Vec<ParamValue> {
    use henad_core::authoring::model::gpu_agent_model::GpuAgentModel as _;
    let mut values: Vec<ParamValue> = crate::gpu_ants::GpuAnts::param_descriptors()
        .iter()
        .map(|d| d.kind.default_value())
        .collect();
    values[henad_compute::cpu::agent_engine::NUM_AGENTS] = ParamValue::U32(num_agents);
    values
}

#[test]
fn the_gpu_foraging_tutorial_seeds_the_same_buffers() {
    use henad_compute::gpu::GpuAgentState;
    use henad_core::authoring::model::gpu_agent_model::{GpuAgentModel as _, PassCtx, PassId};
    type Taught = super::gpu_foraging::GpuForagingModel;
    type Shipped = crate::gpu_ants::GpuAnts;

    let params = gpu_foraging_params(2_000);
    let geom = GpuAgentState::<Shipped>::geometry_for(&params, &wgpu::Limits::default());

    assert_eq!(Taught::buffer_lens(&geom), Shipped::buffer_lens(&geom));
    assert_eq!(
        Taught::seed_buffers(&geom, &params, Some(SEED)),
        Shipped::seed_buffers(&geom, &params, Some(SEED)),
        "docs/guide/first-model/gpu-ants.md no longer seeds the shipped model's buffers"
    );

    let ctx = PassCtx {
        geom: &geom,
        invocations: geom.n_cells * 2,
        groups_x: 7,
    };
    for pass in [PassId::Step(0), PassId::Step(1), PassId::Display, PassId::Reduce] {
        assert_eq!(
            Taught::pass_params_bytes(pass, ctx, &params),
            Shipped::pass_params_bytes(pass, ctx, &params),
            "the taught uniform block for {pass:?} has drifted"
        );
    }

    assert_eq!(
        descriptor_shape(&Taught::param_descriptors()),
        descriptor_shape(&Shipped::param_descriptors()),
    );
    let flags =
        |specs: &[henad_core::authoring::model::gpu_agent_model::BufferSpec]| -> Vec<(&'static str, bool, bool)> {
            specs.iter().map(|b| (b.label, b.double_buffered, b.drawable)).collect()
        };
    assert_eq!(flags(Taught::BUFFERS), flags(Shipped::BUFFERS));
    assert_eq!(Taught::COUNTERS, Shipped::COUNTERS);
    assert_eq!(Taught::REDUCE.lanes, Shipped::REDUCE.lanes);
    assert_eq!(Taught::STATS.len(), Shipped::STATS.len());
}

#[test]
fn the_gpu_foraging_tutorial_matches_the_shipped_model() {
    use henad_compute::gpu::GpuAgentState;

    let Some(ctx) = crate::tests::support::headless_context("gpu_foraging_parity_device", wgpu::Features::empty())
    else {
        log::warn!("skipping the_gpu_foraging_tutorial_matches_the_shipped_model: no adapter");
        return;
    };

    let params = gpu_foraging_params(4_000);
    const STEPS: u32 = 300;

    let mut taught = GpuAgentState::<super::gpu_foraging::GpuForagingModel>::new_seeded(&ctx, &params, Some(SEED));
    let mut shipped = GpuAgentState::<crate::gpu_ants::GpuAnts>::new_seeded(&ctx, &params, Some(SEED));
    taught.run_batched(STEPS);
    shipped.run_batched(STEPS);

    // Buffer indices are declaration order, and both declare pos, state, colour, rng, field.
    for (index, what) in [
        (0, "positions"),
        (1, "packed state"),
        (2, "colours"),
        (4, "the pheromone field"),
    ] {
        assert_eq!(
            taught.read_buffer(index),
            shipped.read_buffer(index),
            "docs/guide/first-model/gpu-ants.md no longer produces the shipped model: {what} differ"
        );
    }

    let pos = shipped.read_buffer(0);
    assert!(
        pos.iter().any(|&p| p != pos[0]),
        "{STEPS} ticks left every ant where it started, so the comparison proves nothing"
    );
}
