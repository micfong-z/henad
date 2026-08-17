use henad_core::authoring::model::agent_model::{
    AgentLanes as _, AgentModel, ChunkTally as _, NeighborIndex as _, StepCtx,
};
use henad_core::authoring::model::field::{Extent, FieldLayer};
use henad_core::helpers::{extract_f32, extract_u32, f32_param, u32_param};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamStore, ParamValue};
use henad_core::view::{GridView, PointView, StatEntry, stat_entries};

/// Default RNG seed.
pub const AGENT_INIT_SEED: u64 = 0xA175_F01A_6ED5_0001;

/// Engine wrapper that implements `SimState` for any `AgentModel`.
pub struct AgentModelState<A: AgentModel> {
    lanes: A::Lanes,
    field: A::Field,
    index: A::Index,
    deposits: <A::Field as FieldLayer>::DepositLanes,
    params: ParamStore,
    extent: Extent,
    tally: A::Tally,
    seed: u64,
    tick: u64,
}

impl<A: AgentModel> AgentModelState<A> {
    pub fn from_params(params: &[ParamValue]) -> Self {
        Self::from_params_seeded(params, None)
    }

    /// Build a state whose RNG starts from `seed`, or [`AGENT_INIT_SEED`] when it is `None`.
    pub fn from_params_seeded(params: &[ParamValue], seed: Option<u64>) -> Self {
        let n = extract_u32(params, NUM_AGENTS, 10_000) as usize;
        let extent = Extent {
            w: extract_f32(params, WORLD_WIDTH, 1_000.0),
            h: extract_f32(params, WORLD_HEIGHT, 1_000.0),
        };

        let (own, field_params) = split_params::<A>(params);
        let mut lanes = A::Lanes::alloc(n);
        let mut seed = seed.map_or(AGENT_INIT_SEED, henad_core::authoring::primitives::rng::mix_seed);
        A::init(&mut lanes, extent, own, &mut seed);

        let field = A::Field::new(extent, field_params);
        let deposits = field.alloc_deposits(n);
        let hot = A::from_params(own, extent);
        let (pos_x, pos_y) = lanes.positions();
        let mut index = A::Index::new(extent, A::index_cell_size(&hot));
        index.rebuild(pos_x, pos_y, A::index_cell_size(&hot));

        Self {
            lanes,
            field,
            index,
            deposits,
            params: ParamStore::new(&agent_model_param_descriptors::<A>(), params),
            extent,
            tally: A::Tally::default(),
            seed,
            tick: 0,
        }
    }

    /// Build a state whose agents come from `seed_lanes`, for reproducing a particular run.
    pub fn from_agents(params: &[ParamValue], seed_lanes: impl FnOnce(&mut A::Lanes, Extent)) -> Self {
        let n = extract_u32(params, NUM_AGENTS, 10_000) as usize;
        let extent = Extent {
            w: extract_f32(params, WORLD_WIDTH, 1_000.0),
            h: extract_f32(params, WORLD_HEIGHT, 1_000.0),
        };

        let mut lanes = A::Lanes::alloc(n);
        let mut seed = AGENT_INIT_SEED;
        let (own, field_params) = split_params::<A>(params);

        // Init is still needed to ensure that seed is advanced to the right value for the first step.
        A::init(&mut lanes, extent, own, &mut seed);

        seed_lanes(&mut lanes, extent);

        let field = A::Field::new(extent, field_params);
        let deposits = field.alloc_deposits(n);
        let hot = A::from_params(own, extent);
        let (pos_x, pos_y) = lanes.positions();
        let mut index = A::Index::new(extent, A::index_cell_size(&hot));
        index.rebuild(pos_x, pos_y, A::index_cell_size(&hot));

        Self {
            lanes,
            field,
            index,
            deposits,
            params: ParamStore::new(&agent_model_param_descriptors::<A>(), params),
            extent,
            tally: A::Tally::default(),
            seed,
            tick: 0,
        }
    }

    pub fn lanes(&self) -> &A::Lanes {
        &self.lanes
    }

    pub fn field(&self) -> &A::Field {
        &self.field
    }

    pub fn tally(&self) -> &A::Tally {
        &self.tally
    }
}

/// The full descriptor list, with population and world extent prepended.
///
/// The extent is the engine's, not either layer's, so an agent layer and a field layer cannot
/// disagree about how big the world is.
/// Indices of the params the engine prepends before a model's own. A GPU port reads them too,
/// since it composes the same list.
pub const NUM_AGENTS: usize = 0;
pub const WORLD_WIDTH: usize = 1;
pub const WORLD_HEIGHT: usize = 2;

/// How many the engine prepends, and so where a model's own params start.
pub const AGENT_PARAM_BASE: usize = 3;

/// Splits a composed list into `(the model's own, its field layer's)`.
///
/// Computed from the descriptor lists rather than hard-coded, so a model or a field layer gaining a
/// parameter cannot shift the other's indices.
pub fn split_params<A: AgentModel>(params: &[ParamValue]) -> (&[ParamValue], &[ParamValue]) {
    let own = A::param_descriptors().len();
    let start = AGENT_PARAM_BASE.min(params.len());
    let mid = (start + own).min(params.len());
    (&params[start..mid], &params[mid..])
}

pub fn agent_model_param_descriptors<A: AgentModel>() -> Vec<ParamDescriptor> {
    let extent = A::DEFAULT_EXTENT;
    let mut descs = vec![
        u32_param("num_agents", "Number of Agents", A::DEFAULT_AGENTS, 1, A::MAX_AGENTS).on_reload(),
        f32_param("world_width", "World Width", extent.w, 1.0, 10_000.0, Some(50.0)).on_reload(),
        f32_param("world_height", "World Height", extent.h, 1.0, 10_000.0, Some(50.0)).on_reload(),
    ];
    descs.extend(A::param_descriptors());
    descs.extend(<A::Field as FieldLayer>::param_descriptors());
    descs
}

impl<A: AgentModel> SimState for AgentModelState<A> {
    fn step(&mut self) {
        let (own, field_slice) = split_params::<A>(self.params.values());
        let hot = A::from_params(own, self.extent);
        let field_params = <A::Field as FieldLayer>::from_params(field_slice);

        let (pos_x, pos_y) = self.lanes.positions();
        self.index.rebuild(pos_x, pos_y, A::index_cell_size(&hot));

        {
            let ctx = StepCtx::<A> {
                field: self.field.read(),
                index: &self.index,
                params: &hot,
                extent: self.extent,
            };
            A::run_deposit_pass(&self.lanes, &mut self.deposits, &ctx);
        }

        let tallied = {
            let ctx = StepCtx::<A> {
                field: self.field.read(),
                index: &self.index,
                params: &hot,
                extent: self.extent,
            };
            A::run_step_pass(&mut self.lanes, &ctx, self.seed, self.tick)
        };
        self.tally = std::mem::take(&mut self.tally).merge(tallied);

        self.field.update(&self.deposits, &field_params, self.tick);
        self.lanes.swap();
        self.seed = crate::cpu::primitives::chunked::advance_tick_seed(self.seed, self.tick);
        self.tick += 1;
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        self.field.grid_view()
    }

    fn point_view(&self) -> Option<PointView<'_>> {
        let (pos_x, pos_y) = self.lanes.positions();
        Some(PointView {
            pos_x,
            pos_y,
            world_w: self.extent.w,
            world_h: self.extent.h,
            color: self.lanes.colors(),
            palette: A::PALETTE,
        })
    }

    fn prepare_view(&mut self) {
        self.field.prepare_view();
    }

    fn stats(&self) -> Vec<StatEntry> {
        stat_entries(A::STATS, A::stats(&self.lanes, &self.field, &self.tally))
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.params.set(index, value)
    }

    fn population(&self) -> u64 {
        self.lanes.len() as u64
    }

    fn heap_bytes(&self) -> usize {
        self.lanes.heap_bytes() + self.field.heap_bytes() + self.index.heap_bytes()
    }
}
