//! Holds each tutorial's finished code to the model it teaches.
//!
//! Bit equality rather than a tolerance. Both sides run the same kernels through the same engine,
//! so anything less than identical means the page has drifted.

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_compute::cpu::field::scalar::{ScalarField, ScalarFieldSpec};
use henad_compute::cpu::grid_engine::GridModelState;
use henad_compute::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
use henad_core::authoring::model::agent_model::{AgentLanes as _, AgentModel};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::authoring::model::network_model::NetworkModel;
use henad_core::model::SimState as _;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::StatDescriptor;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};

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

// --- Virus on a Network ---

/// A change made to a running network state before one of its ticks.
enum Nudge {
    /// Sets the parameter with this id.
    Set(&'static str, ParamValue),
    /// Presses the action with this id.
    Press(&'static str),
}

struct NetworkSnapshot {
    states: Vec<u8>,
    timers: Vec<u32>,
    positions: Vec<u32>,
    src: Vec<u32>,
    dst: Vec<u32>,
    edge_colors: Vec<u8>,
    /// Hash of the edge list and its colours after every tick, taken before that tick's recolour.
    trail: u64,
    directed: bool,
    version: u64,
    stats: Vec<u64>,
}

/// Returns `N`'s full parameter list at its defaults, apart from `overrides`.
///
/// `overrides` names parameters by id. Each side builds its own list, since the taught model declares one
/// parameter fewer.
fn network_params<N: NetworkModel>(overrides: &[(&str, ParamValue)]) -> Vec<ParamValue> {
    let descs = network_model_param_descriptors::<N>();
    for (id, _) in overrides {
        assert!(descs.iter().any(|d| d.id == *id), "{} has no parameter '{id}'", N::ID);
    }
    descs
        .iter()
        .map(|d| {
            overrides
                .iter()
                .find(|(id, _)| *id == d.id)
                .map_or_else(|| d.kind.default_value(), |(_, v)| v.clone())
        })
        .collect()
}

/// Everything a network model produces after `steps` ticks, floats as raw bits.
///
/// `nudges` are applied before the tick they name. `timers` reads the one lane that no trait exposes.
fn run_network<N: NetworkModel>(
    overrides: &[(&str, ParamValue)],
    steps: u64,
    nudges: &[(u64, Nudge)],
    timers: fn(&N::Lanes) -> &[u32],
) -> NetworkSnapshot {
    let descs = network_model_param_descriptors::<N>();
    let mut state = NetworkModelState::<N>::from_params_seeded(&network_params::<N>(overrides), Some(SEED));
    let mut trail = DefaultHasher::new();
    for tick in 0..steps {
        for (_, nudge) in nudges.iter().filter(|(at, _)| *at == tick) {
            match nudge {
                Nudge::Set(id, value) => {
                    let index = descs
                        .iter()
                        .position(|d| d.id == *id)
                        .unwrap_or_else(|| panic!("{} has no parameter '{id}'", N::ID));
                    assert!(state.set_param(index, value), "'{id}' is not a live parameter");
                }
                Nudge::Press(id) => {
                    let index = N::ACTIONS
                        .iter()
                        .position(|a| a.id == *id)
                        .unwrap_or_else(|| panic!("{} has no action '{id}'", N::ID));
                    assert!(state.act(index), "the engine refused action '{id}'");
                }
            }
        }
        state.step();
        // Hashed before any recolour. A later recolour would paint over an edge that a rewire added in the wrong
        // colour.
        state.graph().edges().hash(&mut trail);
        // An uneven cadence, so some recolours land between edge moves and some wait through several.
        if tick % 7 == 0 {
            state.prepare_view();
        }
    }
    state.prepare_view();

    let (pos_x, pos_y) = state.lanes().positions();
    let (src, dst, edge_colors) = state.graph().edges();
    NetworkSnapshot {
        states: state.lanes().colors().expect("nodes colour by state").to_vec(),
        timers: timers(state.lanes()).to_vec(),
        positions: pos_x.iter().chain(pos_y).map(|v| v.to_bits()).collect(),
        src: src.to_vec(),
        dst: dst.to_vec(),
        edge_colors: edge_colors.to_vec(),
        trail: trail.finish(),
        directed: state.graph().directed(),
        version: state.graph().version(),
        stats: state.stats().iter().map(|e| e.value.scalar().to_bits()).collect(),
    }
}

/// Runs the taught and shipped models side by side and demands the same bits from both.
///
/// Returns the shipped model's snapshots at tick 0 and at the end, for the caller's own checks.
fn assert_virus_parity(
    overrides: &[(&str, ParamValue)],
    steps: u64,
    nudges: &[(u64, Nudge)],
) -> (NetworkSnapshot, NetworkSnapshot) {
    type Taught = super::virus::VirusModel;
    type Shipped = crate::virus_network::VirusNetwork;

    let taught = run_network::<Taught>(overrides, steps, nudges, |lanes| &lanes.timer);
    let shipped = run_network::<Shipped>(overrides, steps, nudges, |lanes| &lanes.timer);

    assert_eq!(
        taught.states, shipped.states,
        "docs/guide/first-model/virus-network.md no longer spreads the virus the way the shipped model does"
    );
    assert_eq!(taught.timers, shipped.timers, "the taught check timer has drifted");
    assert_eq!(
        taught.positions, shipped.positions,
        "the taught node placement has drifted"
    );
    assert_eq!(
        (&taught.src, &taught.dst),
        (&shipped.src, &shipped.dst),
        "the taught random graph or rewire has drifted"
    );
    assert_eq!(
        taught.edge_colors, shipped.edge_colors,
        "the taught edge recolour has drifted"
    );
    assert_eq!(
        taught.trail, shipped.trail,
        "the taught edges or their colours drifted on some tick before a recolour"
    );
    assert_eq!(
        taught.directed, shipped.directed,
        "the taught graph's direction has drifted"
    );
    assert_eq!(
        taught.version, shipped.version,
        "the taught model changes the graph a different number of times"
    );
    assert_eq!(taught.stats, shipped.stats, "the taught stats reduction has drifted");

    let start = run_network::<Shipped>(overrides, 0, &[], |lanes| &lanes.timer);
    (start, shipped)
}

/// Enough nodes to cross several chunks, and a virus lively enough to leave resistant nodes behind.
const BUSY_VIRUS: [(&str, ParamValue); 5] = [
    ("num_agents", ParamValue::U32(3_000)),
    ("initial_outbreak_size", ParamValue::U32(60)),
    ("virus_spread_chance", ParamValue::F32(0.08)),
    ("virus_check_frequency", ParamValue::U32(3)),
    ("gain_resistance_chance", ParamValue::F32(0.3)),
];

#[test]
fn the_virus_tutorial_matches_the_shipped_model_while_rewiring() {
    let mut overrides = BUSY_VIRUS.to_vec();
    overrides.push(("keep_rewiring", ParamValue::Bool(true)));
    let nudges = [
        (100, Nudge::Set("directed", ParamValue::Bool(true))),
        (200, Nudge::Set("directed", ParamValue::Bool(false))),
    ];

    let (start, end) = assert_virus_parity(&overrides, 300, &nudges);

    assert!(
        end.states.contains(&super::virus::RESISTANT) && end.edge_colors.contains(&super::virus::EDGE_BLOCKED),
        "no edge was greyed, so the recolour went untested"
    );
    assert_ne!(
        (start.src, start.dst),
        (end.src, end.dst),
        "no edge moved, so the rewire went untested"
    );
}

#[test]
fn the_virus_tutorial_matches_the_shipped_model_when_directed_and_pressed() {
    let mut overrides = BUSY_VIRUS.to_vec();
    overrides.push(("directed", ParamValue::Bool(true)));
    let nudges: Vec<(u64, Nudge)> = (0..300)
        .step_by(10)
        .map(|tick| (tick, Nudge::Press("rewire")))
        .collect();

    let (start, end) = assert_virus_parity(&overrides, 300, &nudges);

    assert!(end.directed, "the graph was never directed");
    // Only an infection takes a node out of the susceptible state.
    assert!(
        start
            .states
            .iter()
            .zip(&end.states)
            .any(|(&s, &e)| s == super::virus::SUSCEPTIBLE && e != super::virus::SUSCEPTIBLE),
        "the virus never spread, so the node pass went untested"
    );
    assert_ne!(
        (start.src, start.dst),
        (end.src, end.dst),
        "no press moved an edge, so the action went untested"
    );
}

#[test]
fn the_virus_tutorial_declares_the_same_parameters_and_actions() {
    type Taught = super::virus::VirusModel;
    type Shipped = crate::virus_network::VirusNetwork;

    // Kinds are compared through `Debug`. It carries the ranges and the slider step.
    let shape = |descs: Vec<ParamDescriptor>| -> Vec<_> {
        descs
            .into_iter()
            .filter(|d| d.id != "network")
            .map(|d| (d.id, d.label, format!("{:?}", d.kind), d.apply, d.format))
            .collect()
    };
    assert_eq!(
        shape(network_model_param_descriptors::<Taught>()),
        shape(network_model_param_descriptors::<Shipped>()),
        "the page declares its parameters differently, apart from the generator choice it leaves out"
    );

    let network = network_model_param_descriptors::<Shipped>()
        .into_iter()
        .find(|d| d.id == "network")
        .expect("the shipped model offers a generator choice");
    assert_eq!(
        network.kind.default_value(),
        ParamValue::Choice(0),
        "the page builds the random graph, so the shipped default has to be the random one"
    );

    assert_eq!(Taught::ACTIONS, Shipped::ACTIONS);
    assert_eq!(Taught::PALETTE, Shipped::PALETTE);
    assert_eq!(Taught::EDGE_PALETTE, Shipped::EDGE_PALETTE);
    let stats =
        |descs: &[StatDescriptor]| -> Vec<(&str, [u8; 4])> { descs.iter().map(|d| (d.label, d.color)).collect() };
    assert_eq!(stats(Taught::STATS), stats(Shipped::STATS));
    assert_eq!(
        Taught::CHUNK,
        Shipped::CHUNK,
        "CHUNK sets the rng seeding granularity, so a mismatch changes results"
    );
    assert_eq!(Taught::DEFAULT_NODES, Shipped::DEFAULT_NODES);
    assert_eq!(Taught::MAX_NODES, Shipped::MAX_NODES);
    assert_eq!(
        (Taught::DEFAULT_EXTENT.w, Taught::DEFAULT_EXTENT.h),
        (Shipped::DEFAULT_EXTENT.w, Shipped::DEFAULT_EXTENT.h)
    );
    // The page has the reader pick the model by name, next to the shipped one.
    assert_eq!(
        (Taught::NAME, Taught::DESCRIPTION),
        (Shipped::NAME, Shipped::DESCRIPTION)
    );
    // No run above lays out, so the springs are compared here. `SpringParams` has no `PartialEq`.
    assert_eq!(
        format!("{:?}", Taught::LAYOUT),
        format!("{:?}", Shipped::LAYOUT),
        "the taught layout springs have drifted"
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
        seed: 0,
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
