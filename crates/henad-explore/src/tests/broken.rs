//! Models with bugs in them, for the tests that check a failed run is recorded and the rest of a sweep carries on, and
//! that the testing kit reports each bug under its check. A few sound models at the edge of a contract sit beside
//! them, for the kit to pass.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::agent_model::{AgentModel, NoIndex, StepCtx};
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::field::{Extent, NoField};
use henad_core::authoring::model::gpu_agent_model::{
    BufferSpec, DisplaySpec, Geometry, GpuAgentAction, GpuAgentModel, PassCtx, PassId, PassSpec, ReduceSpec,
};
use henad_core::authoring::model::gpu_grid_model::{GpuGridAction, GpuGridModel};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::authoring::model::network_model::{NetworkModel, Nodes};
use henad_core::authoring::primitives::rng::{next_float, xorshift64};
use henad_core::grid::Grid2D;
use henad_core::helpers::u32_param;
use henad_core::model::SimState;
use henad_core::network::Network;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{EdgeView, GridView, PointView, StatDescriptor, StatEntry, StatValue};
use std::sync::Arc;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::{GpuContext, GpuSimState, MAX_STEPS_PER_SUBMISSION, StatsPoll};
use henad_compute::snapshot::GpuSnapshot;
use henad_models::gpu_boids::GpuBoids;
use henad_models::gpu_sir::GpuSir;

/// Divides by `init_divisor` while it builds and by `divisor` in every step, so a sweep reaching 0 panics there.
pub struct DividesByParam;

impl GridModel for DividesByParam {
    const NAME: &'static str = "Divides By Param";
    const ID: &'static str = "divides_by_param";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Cells", [0xFF, 0xFF, 0xFF, 0xFF])];
    type Params = u8;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("divisor", "Divisor", 1, 0, 4),
            u32_param("init_divisor", "Init divisor", 1, 0, 4),
        ]
    }

    fn from_params(params: &[ParamValue]) -> u8 {
        param_as_u8(params, 0)
    }

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], _rng: &mut u64) {
        grid.current_mut()[0] = 1 / param_as_u8(params, 1);
    }

    fn step_cell(cell: u8, _neighbors: &[u8], divisor: &u8, _rng: &mut u64) -> u8 {
        cell / *divisor
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(
            grid.current().iter().map(|&cell| f64::from(cell)).sum(),
        )]
    }
}

/// Counts every cell down by one per step from `countdown`, and reports the inverse of the first cell's count.
///
/// The inverse is not finite from the tick the count reaches 0.
pub struct InverseOfCountdown;

impl GridModel for InverseOfCountdown {
    const NAME: &'static str = "Inverse Of Countdown";
    const ID: &'static str = "inverse_of_countdown";
    const DESCRIPTION: &'static str = "A model whose stat stops being finite, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]];
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Inverse", [0xFF, 0xFF, 0xFF, 0xFF])];
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![u32_param("countdown", "Countdown", 8, 0, 255)]
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], _rng: &mut u64) {
        grid.current_mut().fill(param_as_u8(params, 0));
    }

    fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
        cell.saturating_sub(1)
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(1.0 / f64::from(grid.current()[0]))]
    }
}

/// Returns parameter `index` of the model's own parameters as a `u8`, saturating at its largest value.
fn param_as_u8(params: &[ParamValue], index: usize) -> u8 {
    match params.get(index) {
        Some(&ParamValue::U32(value)) => u8::try_from(value).unwrap_or(u8::MAX),
        _ => 1,
    }
}

/// One bug a [`BuggyState`] adds to the state it wraps.
#[derive(Clone, Copy)]
pub enum Bug {
    /// Rejects every action.
    RefusesActions,
    /// Returns every stat but the last.
    DropsLastStat,
    /// Reports every edit as applied live, the reload parameters included.
    AcceptsEveryEdit,
    /// Returns no grid view.
    HidesGrid,
    /// Reports a step split into no jobs.
    ReportsNoJobs,
}

/// CPU state that steps as the state it wraps, with one [`Bug`].
pub struct BuggyState {
    state: Box<dyn SimState>,
    bug: Bug,
}

impl BuggyState {
    /// Returns `entry` with every CPU state it builds wrapped, so each has `bug`.
    pub fn wrap(entry: ModelEntry, bug: Bug) -> ModelEntry {
        entry.wrap_factory(|create| {
            Arc::new(
                move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| match create(
                    params, seed, gpu,
                )? {
                    ModelState::Cpu(state) => Ok(ModelState::Cpu(Box::new(Self { state, bug }))),
                    ModelState::Gpu(state) => Ok(ModelState::Gpu(state)),
                },
            )
        })
    }
}

impl SimState for BuggyState {
    fn step(&mut self) {
        self.state.step();
    }

    fn tick(&self) -> u64 {
        self.state.tick()
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        match self.bug {
            Bug::HidesGrid => None,
            _ => self.state.grid_view(),
        }
    }

    fn point_view(&self) -> Option<PointView<'_>> {
        self.state.point_view()
    }

    fn edge_view(&self) -> Option<EdgeView<'_>> {
        self.state.edge_view()
    }

    fn prepare_view(&mut self) {
        self.state.prepare_view();
    }

    fn stats(&self) -> Vec<StatEntry> {
        let mut stats = self.state.stats();
        if matches!(self.bug, Bug::DropsLastStat) {
            stats.pop();
        }
        stats
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        let accepted = self.state.set_param(index, value);
        accepted || matches!(self.bug, Bug::AcceptsEveryEdit)
    }

    fn act(&mut self, index: usize) -> bool {
        match self.bug {
            Bug::RefusesActions => false,
            _ => self.state.act(index),
        }
    }

    fn population(&self) -> u64 {
        self.state.population()
    }

    fn heap_bytes(&self) -> usize {
        self.state.heap_bytes()
    }

    fn parallel_jobs(&self) -> Option<usize> {
        match self.bug {
            Bug::ReportsNoJobs => Some(0),
            _ => self.state.parallel_jobs(),
        }
    }
}

henad_compute::agent_lanes! {
    /// Position, written in place.
    pub struct CounterLanes {
        read CounterRead;
        chunk CounterChunk;
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
    }
}

/// Moves each agent by the next value of a counter that every chunk shares, so its position depends on the order in
/// which the workers reach the agents.
///
/// One worker takes the chunks in order, and several interleave them. The positions then depend on the thread count.
pub struct SharedAccumulator;

/// World side of [`SharedAccumulator`], and the modulus of its positions.
const COUNTER_SIDE: u64 = 128;

impl AgentModel for SharedAccumulator {
    const NAME: &'static str = "Shared Accumulator";
    const ID: &'static str = "shared_accumulator";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0xFF, 0xFF, 0xFF, 0xFF]];
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Weighted position", [0xFF, 0xFF, 0xFF, 0xFF])];
    const DEFAULT_AGENTS: u32 = 256;
    const DEFAULT_EXTENT: Extent = Extent {
        w: COUNTER_SIDE as f32,
        h: COUNTER_SIDE as f32,
    };

    type Lanes = CounterLanes;
    type Field = NoField;
    type Index = NoIndex;
    /// Counter of the agents moved this tick.
    type Params = Mutex<u64>;
    type Tally = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        Vec::new()
    }

    fn from_params(_params: &[ParamValue], _extent: Extent) -> Mutex<u64> {
        Mutex::new(0)
    }

    fn init(_lanes: &mut CounterLanes, _extent: Extent, _params: &[ParamValue], _rng: &mut u64) {}

    fn run_step_pass(lanes: &mut CounterLanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) {
        let counter = ctx.params;
        lanes.run_pass(Self::CHUNK, seed, tick, |_, k, _read, chunk, _rng| {
            let mut next = counter.lock().unwrap_or_else(PoisonError::into_inner);
            // Both terms stay below the side, and every value is exact in an `f32`.
            let moved = (chunk.pos_x[k] as u64 + *next % COUNTER_SIDE) % COUNTER_SIDE;
            chunk.pos_x[k] = moved as f32;
            *next += 1;
        });
    }

    fn stats(lanes: &CounterLanes, _field: &NoField, (): &()) -> Vec<StatValue> {
        let weighted = lanes
            .pos_x
            .iter()
            .enumerate()
            .map(|(index, &x)| index as f64 * f64::from(x))
            .sum();
        vec![StatValue::Scalar(weighted)]
    }
}

/// Writes a grid model with one cell value drawn per cell at build, a step that changes nothing, actions that change
/// nothing, and one stat per entry of `stats`, each the count of live cells. `init` runs after the draw.
macro_rules! grid_model {
    (
        $(#[$meta:meta])*
        $name:ident, id: $id:expr, palette: $palette:expr, stats: $stats:expr, actions: $actions:expr,
        init: |$grid:ident, $rng:ident| $init:block
    ) => {
        $(#[$meta])*
        pub struct $name;

        impl GridModel for $name {
            const NAME: &'static str = stringify!($name);
            const ID: &'static str = $id;
            const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
            const PALETTE: &'static [[u8; 4]] = $palette;
            const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
            const STATS: &'static [StatDescriptor] = $stats;
            const ACTIONS: &'static [ActionDescriptor] = $actions;
            type Params = ();

            fn param_descriptors() -> Vec<ParamDescriptor> {
                Vec::new()
            }

            fn from_params(_params: &[ParamValue]) {}

            fn init($grid: &mut Grid2D<u8>, _params: &[ParamValue], $rng: &mut u64) {
                for cell in $grid.current_mut() {
                    *$rng = xorshift64(*$rng);
                    *cell = u8::from(*$rng & 1 == 1);
                }
                $init
            }

            fn step_cell(cell: u8, _neighbors: &[u8], (): &(), _rng: &mut u64) -> u8 {
                cell
            }

            fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
                let live = grid.current().iter().filter(|&&cell| cell == 1).count();
                vec![StatValue::Scalar(live as f64); $stats.len()]
            }
        }
    };
}

/// Colours of the two cell values of a [`grid_model!`].
const TWO_COLORS: &[[u8; 4]] = &[[0, 0, 0, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]];

/// One stat of a [`grid_model!`].
const LIVE: &[StatDescriptor] = &[StatDescriptor::new("Live", [0xFF, 0xFF, 0xFF, 0xFF])];

grid_model! {
    /// Has an id with a space and a capital letter in it.
    BadId, id: "Bad Id", palette: TWO_COLORS, stats: LIVE, actions: &[], init: |_grid, _rng| {}
}

grid_model! {
    /// Declares its one stat label twice.
    RepeatsStatLabel,
    id: "repeats_stat_label",
    palette: TWO_COLORS,
    stats: &[
        StatDescriptor::new("Live", [0xFF, 0xFF, 0xFF, 0xFF]),
        StatDescriptor::new("Live", [0xFF, 0, 0, 0xFF]),
    ],
    actions: &[],
    init: |_grid, _rng| {}
}

grid_model! {
    /// Declares two actions under one id.
    RepeatsActionId,
    id: "repeats_action_id",
    palette: TWO_COLORS,
    stats: LIVE,
    actions: &[
        ActionDescriptor::new("clear", "Clear"),
        ActionDescriptor::new("clear", "Clear again"),
    ],
    init: |_grid, _rng| {}
}

grid_model! {
    /// Declares a palette with no colours.
    EmptyPalette, id: "empty_palette", palette: &[], stats: LIVE, actions: &[], init: |_grid, _rng| {}
}

/// Builds of [`CountsBuilds`] so far.
static BUILDS: AtomicU64 = AtomicU64::new(0);

grid_model! {
    /// Writes the number of builds before it into its first cell, so no two builds agree.
    CountsBuilds, id: "counts_builds", palette: TWO_COLORS, stats: LIVE, actions: &[], init: |grid, _rng| {
        grid.current_mut()[0] = u8::from(BUILDS.fetch_add(1, Ordering::Relaxed) % 2 == 1);
    }
}

grid_model! {
    /// Writes the pool's width into its first row as that many live cells, so a build depends on the thread count.
    ReadsPoolWidth, id: "reads_pool_width", palette: TWO_COLORS, stats: LIVE, actions: &[], init: |grid, _rng| {
        let width = grid.width() as usize;
        let live = rayon::current_num_threads().min(width);
        let row = &mut grid.current_mut()[..width];
        row.fill(0);
        row[..live].fill(1);
    }
}

grid_model! {
    /// Writes the pool's width into its first row as the position of the row's one live cell, so a build's state
    /// depends on the thread count and its stats do not.
    PlacesCellByPoolWidth,
    id: "places_cell_by_pool_width",
    palette: TWO_COLORS,
    stats: LIVE,
    actions: &[],
    init: |grid, _rng| {
        let width = grid.width() as usize;
        let row = &mut grid.current_mut()[..width];
        row.fill(0);
        row[rayon::current_num_threads() % width] = 1;
    }
}

grid_model! {
    /// Keeps one live cell, placed by the seed, so two seeds differ in the state alone. The model is sound.
    PlacesCellBySeed, id: "places_cell_by_seed", palette: TWO_COLORS, stats: LIVE, actions: &[], init: |grid, rng| {
        let cells = grid.current_mut();
        cells.fill(0);
        *rng = xorshift64(*rng);
        let index = (*rng % cells.len() as u64) as usize;
        cells[index] = 1;
    }
}

/// Declares parameter and action ids that the command line cannot accept, and an action id holding `@`. The command
/// line accepts `@` in an action id.
pub struct UnnameableIds;

impl GridModel for UnnameableIds {
    const NAME: &'static str = "Unnameable Ids";
    const ID: &'static str = "unnameable_ids";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = TWO_COLORS;
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = LIVE;
    const ACTIONS: &'static [ActionDescriptor] = &[
        ActionDescriptor::new("spawn@centre", "Spawn at centre"),
        ActionDescriptor::new("clear=all", "Clear all"),
    ];
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("rate=high", "Rate", 1, 0, 4),
            u32_param("spread rate", "Spread rate", 1, 0, 4),
            u32_param("action.delay", "Delay", 1, 0, 4),
            u32_param("count", "Count", 1, 0, 4),
        ]
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], rng: &mut u64) {
        for cell in grid.current_mut() {
            *rng = xorshift64(*rng);
            *cell = u8::from(*rng & 1 == 1);
        }
    }

    fn step_cell(cell: u8, _neighbors: &[u8], (): &(), _rng: &mut u64) -> u8 {
        cell
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(
            grid.current().iter().filter(|&&cell| cell == 1).count() as f64,
        )]
    }
}

/// Declares a parameter default outside its own bounds. Nothing checks it before the kit.
pub struct DefaultOutOfBounds;

impl GridModel for DefaultOutOfBounds {
    const NAME: &'static str = "Default Out Of Bounds";
    const ID: &'static str = "default_out_of_bounds";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = TWO_COLORS;
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
    const STATS: &'static [StatDescriptor] = LIVE;
    type Params = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![u32_param("initial_infected", "Initial infected", 500, 1, 100)]
    }

    fn from_params(_params: &[ParamValue]) {}

    fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], rng: &mut u64) {
        for cell in grid.current_mut() {
            *rng = xorshift64(*rng);
            *cell = u8::from(*rng & 1 == 1);
        }
    }

    fn step_cell(cell: u8, _neighbors: &[u8], (): &(), _rng: &mut u64) -> u8 {
        cell
    }

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        let live = grid.current().iter().filter(|&&cell| cell == 1).count();
        vec![StatValue::Scalar(live as f64)]
    }
}

/// Declares its own `num_agents` parameter beside the engine's `num_agents`, and its own `grid_width` parameter twice.
pub struct DeclaresNumAgents;

impl AgentModel for DeclaresNumAgents {
    const NAME: &'static str = "Declares Num Agents";
    const ID: &'static str = "declares_num_agents";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0xFF, 0xFF, 0xFF, 0xFF]];
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Mean x", [0xFF, 0xFF, 0xFF, 0xFF])];
    const DEFAULT_AGENTS: u32 = 256;
    const DEFAULT_EXTENT: Extent = Extent { w: 128.0, h: 128.0 };

    type Lanes = CounterLanes;
    type Field = NoField;
    type Index = NoIndex;
    type Params = ();
    type Tally = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        vec![
            u32_param("num_agents", "Number of Agents", 8, 1, 64),
            u32_param("grid_width", "Grid Width", 8, 1, 64),
            u32_param("grid_width", "Grid Width again", 8, 1, 64),
        ]
    }

    fn from_params(_params: &[ParamValue], _extent: Extent) {}

    fn init(lanes: &mut CounterLanes, extent: Extent, _params: &[ParamValue], rng: &mut u64) {
        for (x, y) in lanes.pos_x.iter_mut().zip(&mut lanes.pos_y) {
            *x = next_float(rng, extent.w);
            *y = next_float(rng, extent.h);
        }
    }

    fn run_step_pass(_lanes: &mut CounterLanes, _ctx: &StepCtx<'_, Self>, _seed: u64, _tick: u64) {}

    fn stats(lanes: &CounterLanes, _field: &NoField, (): &()) -> Vec<StatValue> {
        let sum: f64 = lanes.pos_x.iter().map(|&x| f64::from(x)).sum();
        vec![StatValue::Scalar(sum / lanes.pos_x.len().max(1) as f64)]
    }
}

henad_compute::agent_lanes! {
    /// Position, and the number of view preparations each node has seen.
    pub struct ViewCountLanes {
        read ViewCountRead;
        chunk ViewCountChunk;
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
        plain views: u32 = 0,
    }
}

/// Counts every view preparation in a lane of each node, so its state depends on how often it is sampled.
pub struct CountsViews;

impl NetworkModel for CountsViews {
    const NAME: &'static str = "Counts Views";
    const ID: &'static str = "counts_views";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = &[[0xFF, 0xFF, 0xFF, 0xFF]];
    const EDGE_PALETTE: &'static [[u8; 4]] = &[[0x80, 0x80, 0x80, 0xFF]];
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Views", [0xFF, 0xFF, 0xFF, 0xFF])];
    const DEFAULT_NODES: u32 = 16;
    const DEFAULT_EXTENT: Extent = Extent { w: 128.0, h: 128.0 };

    type Lanes = ViewCountLanes;
    type Params = ();
    type Aux = ();

    fn param_descriptors() -> Vec<ParamDescriptor> {
        Vec::new()
    }

    fn from_params(_params: &[ParamValue], _extent: Extent) {}

    fn init(nodes: &mut Nodes<'_, Self>, extent: Extent, _params: &[ParamValue], rng: &mut u64) {
        let lanes = &mut *nodes.lanes;
        for (x, y) in lanes.pos_x.iter_mut().zip(&mut lanes.pos_y) {
            *x = next_float(rng, extent.w);
            *y = next_float(rng, extent.h);
        }
    }

    fn prepare_view(nodes: &mut Nodes<'_, Self>, _tick: u64) {
        for views in &mut nodes.lanes.views {
            *views += 1;
        }
    }

    fn stats(lanes: &ViewCountLanes, _graph: &Network, (): &()) -> Vec<StatValue> {
        vec![StatValue::Scalar(
            lanes.views.iter().map(|&views| f64::from(views)).sum(),
        )]
    }
}

/// One bug a [`BuggyGpuState`] adds to the state it wraps.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GpuBug {
    /// Once any state of the entry has encoded [`MAX_STEPS_PER_SUBMISSION`] steps in one submission, every state of
    /// the entry reads every stat back as zero, as every buffer of a device that the watchdog stopped does.
    ZeroesFullSubmissions,
    /// Records no stats passes, so a sampled slice begins no readback.
    SkipsStatsPasses,
}

/// GPU state that steps as the state it wraps, with one [`GpuBug`].
pub struct BuggyGpuState {
    state: Box<dyn GpuSimState>,
    bug: GpuBug,
    /// Whether a state of the entry has encoded a full submission, shared by every state the entry builds, as a single
    /// device is shared.
    stopped: Arc<AtomicBool>,
}

impl BuggyGpuState {
    /// Returns `entry` with every GPU state it builds wrapped, so each has `bug`, all of them sharing one stopped
    /// flag.
    pub fn wrap(entry: ModelEntry, bug: GpuBug) -> ModelEntry {
        let stopped = Arc::new(AtomicBool::new(false));
        entry.wrap_factory(|create| {
            Arc::new(
                move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| match create(
                    params, seed, gpu,
                )? {
                    ModelState::Gpu(state) => Ok(ModelState::Gpu(Box::new(Self {
                        state,
                        bug,
                        stopped: Arc::clone(&stopped),
                    }))),
                    ModelState::Cpu(state) => Ok(ModelState::Cpu(state)),
                },
            )
        })
    }
}

impl SimState for BuggyGpuState {
    fn step(&mut self) {
        self.state.step();
    }

    fn tick(&self) -> u64 {
        self.state.tick()
    }

    fn prepare_view(&mut self) {
        self.state.prepare_view();
    }

    fn stats(&self) -> Vec<StatEntry> {
        let mut stats = self.state.stats();
        if self.stopped.load(Ordering::Relaxed) {
            for stat in &mut stats {
                stat.value = match &stat.value {
                    StatValue::Scalar(_) => StatValue::Scalar(0.0),
                    StatValue::Vector2D { .. } => StatValue::Vector2D { x: 0.0, y: 0.0 },
                    StatValue::Histogram { edges, counts } => StatValue::Histogram {
                        edges: edges.clone(),
                        counts: vec![0; counts.len()],
                    },
                };
            }
        }
        stats
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.state.set_param(index, value)
    }

    fn act(&mut self, index: usize) -> bool {
        self.state.act(index)
    }

    fn population(&self) -> u64 {
        self.state.population()
    }

    fn heap_bytes(&self) -> usize {
        self.state.heap_bytes()
    }

    fn parallel_jobs(&self) -> Option<usize> {
        self.state.parallel_jobs()
    }
}

impl GpuSimState for BuggyGpuState {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        if self.bug == GpuBug::ZeroesFullSubmissions && count >= MAX_STEPS_PER_SUBMISSION {
            self.stopped.store(true, Ordering::Relaxed);
        }
        self.state.encode_steps(encoder, count, timestamps);
    }

    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        self.state.encode_action(encoder, index)
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.state.encode_snapshot_passes(encoder);
    }

    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.bug != GpuBug::SkipsStatsPasses {
            self.state.encode_stats_passes(encoder);
        }
    }

    fn begin_stats_readback(&mut self) {
        self.state.begin_stats_readback();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        self.state.poll_stats_readback(device, block)
    }

    fn stats_readback_pending(&self) -> bool {
        self.state.stats_readback_pending()
    }

    fn view(&self) -> GpuSnapshot {
        self.state.view()
    }
}

/// Side of [`OversizedGpuSir`]'s default grid. A buffer of that many cells squared is 256 MiB, past the WebGPU
/// baseline's 128 MiB storage binding.
const OVERSIZED_SIDE: u32 = 8192;

/// [`GpuSir`] with a default grid past the WebGPU baseline.
pub struct OversizedGpuSir;

impl GpuGridModel for OversizedGpuSir {
    const NAME: &'static str = "Oversized GPU SIR";
    const ID: &'static str = "oversized_gpu_sir";
    const DESCRIPTION: &'static str = "A deliberately broken model, registered only by tests";
    const PALETTE: &'static [[u8; 4]] = GpuSir::PALETTE;
    const WORKGROUP_SIZE: u32 = GpuSir::WORKGROUP_SIZE;
    const STATS: &'static [StatDescriptor] = GpuSir::STATS;
    const ACTIONS: &'static [GpuGridAction] = GpuSir::ACTIONS;
    const BUFFERS: &'static [&'static str] = GpuSir::BUFFERS;
    const STEP_BINDINGS: &'static [BindingDecl] = GpuSir::STEP_BINDINGS;
    const DISPLAY_BINDINGS: &'static [BindingDecl] = GpuSir::DISPLAY_BINDINGS;
    const REDUCE_BINDINGS: &'static [BindingDecl] = GpuSir::REDUCE_BINDINGS;
    const STEP_SHADER: &'static str = GpuSir::STEP_SHADER;
    const DISPLAY_SHADER: &'static str = GpuSir::DISPLAY_SHADER;
    const REDUCE_SHADER: &'static str = GpuSir::REDUCE_SHADER;
    const REPLAYS_EXACTLY: bool = GpuSir::REPLAYS_EXACTLY;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        let mut descriptors = GpuSir::param_descriptors();
        for descriptor in &mut descriptors {
            if let ParamKind::U32 { default, .. } = &mut descriptor.kind
                && matches!(descriptor.id, "grid_width" | "grid_height")
            {
                *default = OVERSIZED_SIDE;
            }
        }
        descriptors
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        GpuSir::dims(params)
    }

    fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
        GpuSir::buffer_lens(width, height)
    }

    fn step_dims(width: u32, height: u32) -> (u32, u32) {
        GpuSir::step_dims(width, height)
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        GpuSir::seed_buffers(width, height, params, seed)
    }

    fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8> {
        GpuSir::step_params_bytes(width, height, params)
    }

    fn action_params_bytes(action: usize, width: u32, height: u32, params: &[ParamValue], seed: u32) -> Vec<u8> {
        GpuSir::action_params_bytes(action, width, height, params, seed)
    }

    fn stats(counts: &[u32]) -> Vec<StatValue> {
        GpuSir::stats(counts)
    }
}

/// [`GpuBoids`] declaring no stats, like a model that only draws. The model is sound.
pub struct StatlessGpuBoids;

impl GpuAgentModel for StatlessGpuBoids {
    const NAME: &'static str = "Statless GPU Boids";
    const ID: &'static str = "statless_gpu_boids";
    const DESCRIPTION: &'static str = "A model with no stats, registered only by tests";
    const STATS: &'static [StatDescriptor] = &[];
    const BUFFERS: &'static [BufferSpec] = GpuBoids::BUFFERS;
    const POS_BUFFER: usize = GpuBoids::POS_BUFFER;
    const COLOR_BUFFER: usize = GpuBoids::COLOR_BUFFER;
    const INDEX: bool = GpuBoids::INDEX;
    const COUNTERS: usize = GpuBoids::COUNTERS;
    const STEP_PASSES: &'static [PassSpec] = GpuBoids::STEP_PASSES;
    const DISPLAY: Option<DisplaySpec> = GpuBoids::DISPLAY;
    const ACTIONS: &'static [GpuAgentAction] = GpuBoids::ACTIONS;
    const REDUCE: ReduceSpec = GpuBoids::REDUCE;
    const REPLAYS_EXACTLY: bool = GpuBoids::REPLAYS_EXACTLY;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        GpuBoids::param_descriptors()
    }

    fn dims(params: &[ParamValue]) -> (u32, Extent) {
        GpuBoids::dims(params)
    }

    fn buffer_lens(geom: &Geometry) -> Vec<usize> {
        GpuBoids::buffer_lens(geom)
    }

    fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        GpuBoids::seed_buffers(geom, params, seed)
    }

    fn index_cell_size(params: &[ParamValue]) -> f32 {
        GpuBoids::index_cell_size(params)
    }

    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        GpuBoids::pass_params_bytes(pass, ctx, params)
    }

    fn stats(_sums: &[f32], _counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        Vec::new()
    }
}
