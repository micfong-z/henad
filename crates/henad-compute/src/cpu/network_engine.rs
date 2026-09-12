//! Generic engine turning any [`NetworkModel`] into a runnable [`SimState`].
//!
//! Compare with [`crate::cpu::agent_engine`].

use henad_core::action::action_seed;
use henad_core::authoring::model::agent_model::AgentLanes as _;
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::{NetworkModel, NodeCtx, Nodes};
use henad_core::authoring::primitives::rng::mix_seed;
use henad_core::helpers::{extract_f32, extract_u32, f32_param, u32_param};
use henad_core::model::SimState;
use henad_core::network::Network;
use henad_core::params::{ParamDescriptor, ParamStore, ParamValue};
use henad_core::view::{EdgeView, PointView, StatEntry, stat_entries};
use web_time::Instant;

use crate::cpu::layout::{LayoutScratch, spring_step};
use crate::cpu::primitives::chunked::advance_tick_seed;

/// Default RNG seed.
pub const NETWORK_INIT_SEED: u64 = 0x3141_5926_5EED_0001;

/// Salts separating the global and layout RNG streams.
const GLOBAL_SALT: u64 = 0x53_5897_5EED_0001;
const LAYOUT_SALT: u64 = 0x93_2384_5EED_0001;

/// Indices of the params the engine prepends to a model's own.
///
/// The node count keeps the id `num_agents` for the benchmark scripts.
pub const NUM_NODES: usize = 0;
pub const WORLD_WIDTH: usize = 1;
pub const WORLD_HEIGHT: usize = 2;

/// Number of params the engine prepends.
pub const NETWORK_PARAM_BASE: usize = 3;

/// Returns the model's own slice of the full param list.
fn own_params(params: &[ParamValue]) -> &[ParamValue] {
    &params[NETWORK_PARAM_BASE.min(params.len())..]
}

pub fn network_model_param_descriptors<N: NetworkModel>() -> Vec<ParamDescriptor> {
    let extent = N::DEFAULT_EXTENT;
    let mut descs = vec![
        u32_param("num_agents", "Number of Nodes", N::DEFAULT_NODES, 1, N::MAX_NODES).on_reload(),
        f32_param("world_width", "World Width", extent.w, 1.0, 10_000.0, Some(1.0)).on_reload(),
        f32_param("world_height", "World Height", extent.h, 1.0, 10_000.0, Some(1.0)).on_reload(),
    ];
    descs.extend(N::param_descriptors());
    descs
}

/// Layout pacing and scratch buffers.
struct LayoutState {
    on: bool,
    /// Milliseconds one publish may spend relaxing the layout.
    budget_ms: f32,
    scratch: LayoutScratch,
}

impl LayoutState {
    const DEFAULT_BUDGET_MS: f32 = 4.0;
}

/// Engine wrapper that implements `SimState` for any `NetworkModel`.
pub struct NetworkModelState<N: NetworkModel> {
    lanes: N::Lanes,
    graph: Network,
    aux: N::Aux,
    params: ParamStore,
    extent: Extent,
    /// Node pass seed, split per chunk.
    seed: u64,
    /// Global pass RNG stream.
    global_seed: u64,
    action_seed: u64,
    tick: u64,
    layout: LayoutState,
}

impl<N: NetworkModel> NetworkModelState<N> {
    pub fn from_params(params: &[ParamValue]) -> Self {
        Self::from_params_seeded(params, None)
    }

    /// Creates a state seeded with `seed`, or [`NETWORK_INIT_SEED`] if `None`.
    pub fn from_params_seeded(params: &[ParamValue], seed: Option<u64>) -> Self {
        Self::build(params, seed, |_nodes, _extent| {})
    }

    /// Creates a state, then passes it to `seed_graph` to set up a particular graph.
    pub fn from_graph(
        params: &[ParamValue],
        seed: Option<u64>,
        seed_graph: impl FnOnce(&mut Nodes<'_, N>, Extent),
    ) -> Self {
        Self::build(params, seed, seed_graph)
    }

    fn build(params: &[ParamValue], seed: Option<u64>, seed_graph: impl FnOnce(&mut Nodes<'_, N>, Extent)) -> Self {
        let n = extract_u32(params, NUM_NODES, N::DEFAULT_NODES) as usize;
        let extent = Extent {
            w: extract_f32(params, WORLD_WIDTH, N::DEFAULT_EXTENT.w),
            h: extract_f32(params, WORLD_HEIGHT, N::DEFAULT_EXTENT.h),
        };

        let own = own_params(params);
        let hot = N::from_params(own, extent);
        let mut lanes = N::Lanes::alloc(n);
        let mut aux = N::Aux::default();
        let mut graph = Network::new(n, N::directed(&hot));
        let mut rng = seed.map_or(NETWORK_INIT_SEED, mix_seed);

        {
            let mut nodes = Nodes::<N> {
                lanes: &mut lanes,
                graph: &mut graph,
                aux: &mut aux,
            };
            N::init(&mut nodes, extent, own, &mut rng);
            seed_graph(&mut nodes, extent);
        }
        // Reclaims the space relocations left during `init`.
        graph.rebuild();

        Self {
            lanes,
            graph,
            aux,
            params: ParamStore::new(&network_model_param_descriptors::<N>(), params),
            extent,
            seed: rng,
            global_seed: mix_seed(rng ^ GLOBAL_SALT),
            action_seed: action_seed(seed),
            tick: 0,
            layout: LayoutState {
                on: false,
                budget_ms: LayoutState::DEFAULT_BUDGET_MS,
                scratch: LayoutScratch::new(mix_seed(rng ^ LAYOUT_SALT)),
            },
        }
    }

    pub fn lanes(&self) -> &N::Lanes {
        &self.lanes
    }

    pub fn graph(&self) -> &Network {
        &self.graph
    }

    pub fn aux(&self) -> &N::Aux {
        &self.aux
    }
}

impl<N: NetworkModel> SimState for NetworkModelState<N> {
    fn step(&mut self) {
        let hot = N::from_params(own_params(self.params.values()), self.extent);
        self.graph.set_directed(N::directed(&hot));

        {
            let Self {
                lanes,
                graph,
                aux,
                global_seed,
                ..
            } = self;
            let mut nodes = Nodes::<N> { lanes, graph, aux };
            N::run_global_pass(&mut nodes, &hot, self.extent, global_seed, self.tick);
        }

        {
            let ctx = NodeCtx::<N> {
                graph: &self.graph,
                params: &hot,
                extent: self.extent,
            };
            N::run_node_pass(&mut self.lanes, &ctx, self.seed, self.tick);
        }

        self.lanes.swap();
        if self.graph.should_repack() {
            self.graph.rebuild();
        }
        self.seed = advance_tick_seed(self.seed, self.tick);
        self.tick += 1;
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn point_view(&self) -> Option<PointView<'_>> {
        let (pos_x, pos_y) = self.lanes.positions();
        Some(PointView {
            pos_x,
            pos_y,
            world_w: self.extent.w,
            world_h: self.extent.h,
            color: self.lanes.colors(),
            palette: N::PALETTE,
        })
    }

    fn edge_view(&self) -> Option<EdgeView<'_>> {
        let (src, dst, color) = self.graph.edges();
        Some(EdgeView {
            src,
            dst,
            color: Some(color),
            palette: N::EDGE_PALETTE,
            directed: self.graph.directed(),
            version: self.graph.version(),
        })
    }

    /// Runs the model's `prepare_view`, then relaxes the layout within the budget, at least once.
    fn prepare_view(&mut self) {
        {
            let Self { lanes, graph, aux, .. } = self;
            let mut nodes = Nodes::<N> { lanes, graph, aux };
            N::prepare_view(&mut nodes, self.tick);
        }

        if !self.layout.on {
            return;
        }
        let started = Instant::now();
        let budget = f64::from(self.layout.budget_ms);
        loop {
            let Self {
                lanes, graph, layout, ..
            } = self;
            let (pos_x, pos_y) = lanes.positions_mut();
            spring_step(pos_x, pos_y, graph, self.extent, N::LAYOUT, &mut layout.scratch);
            if started.elapsed().as_secs_f64() * 1000.0 >= budget {
                return;
            }
        }
    }

    fn stats(&self) -> Vec<StatEntry> {
        stat_entries(N::STATS, N::stats(&self.lanes, &self.graph, &self.aux))
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.params.set(index, value)
    }

    fn act(&mut self, index: usize) -> bool {
        if index >= N::ACTIONS.len() {
            return false;
        }
        let Self {
            lanes,
            graph,
            aux,
            params,
            extent,
            action_seed,
            ..
        } = self;
        let own = own_params(params.values());
        let mut nodes = Nodes::<N> { lanes, graph, aux };
        N::act(index, &mut nodes, *extent, own, action_seed);
        true
    }

    fn set_layout(&mut self, on: bool, budget_ms: f32) -> bool {
        self.layout.on = on;
        self.layout.budget_ms = budget_ms.max(0.0);
        true
    }

    /// Number of nodes, excluding retired slots.
    fn population(&self) -> u64 {
        self.graph.node_count() as u64
    }

    fn heap_bytes(&self) -> usize {
        self.lanes.heap_bytes() + self.graph.heap_bytes() + self.layout.scratch.heap_bytes()
    }

    /// Number of node pass chunks, counted over every slot.
    fn parallel_jobs(&self) -> Option<usize> {
        Some(self.graph.slot_count().div_ceil(N::CHUNK.max(1)))
    }
}

#[cfg(test)]
mod tests {
    #![expect(dead_code, reason = "the generated chunk view carries every lane, used or not")]

    use super::{NUM_NODES, NetworkModelState, network_model_param_descriptors};
    use henad_core::action::ActionDescriptor;
    use henad_core::authoring::model::field::Extent;
    use henad_core::authoring::model::network_model::{NetworkModel, NodeCtx, Nodes};
    use henad_core::model::SimState as _;
    use henad_core::network::Network;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::view::{StatDescriptor, StatValue};

    const WORLD: Extent = Extent { w: 100.0, h: 100.0 };

    crate::agent_lanes! {
        struct RingLanes {
            read RingRead;
            chunk RingChunk;
            dual state / next_state: u8,
            plain pos_x: f32 = 0.0,
            plain pos_y: f32 = 0.0,
        }
        color = state;
    }

    /// Ring where a lit node lights its neighbours each tick.
    struct Ring;

    impl NetworkModel for Ring {
        const NAME: &'static str = "Ring";
        const ID: &'static str = "ring";
        const DESCRIPTION: &'static str = "A ring for testing the engine";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255], [255, 0, 0, 255]];
        const EDGE_PALETTE: &'static [[u8; 4]] = &[[128, 128, 128, 255]];
        const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Lit", [255, 0, 0, 255])];
        const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor::new("light", "Light every node")];
        const DEFAULT_NODES: u32 = 8;
        const DEFAULT_EXTENT: Extent = WORLD;

        type Lanes = RingLanes;
        type Params = ();
        type Aux = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            Vec::new()
        }

        fn from_params(_params: &[ParamValue], _extent: Extent) {}

        fn init(nodes: &mut Nodes<'_, Self>, extent: Extent, _params: &[ParamValue], _rng: &mut u64) {
            let n = nodes.graph.slot_count() as u32;
            for i in 0..n {
                let angle = std::f32::consts::TAU * i as f32 / n as f32;
                nodes.lanes.pos_x[i as usize] = extent.w * 0.5 * (1.0 + 0.8 * angle.cos());
                nodes.lanes.pos_y[i as usize] = extent.h * 0.5 * (1.0 + 0.8 * angle.sin());
            }
            for i in 0..n.saturating_sub(1) {
                nodes.graph.add_edge(i, i + 1, 0);
            }
            if n > 2 {
                nodes.graph.add_edge(n - 1, 0, 0);
            }
            nodes.lanes.state[0] = 1;
        }

        fn run_node_pass(lanes: &mut Self::Lanes, ctx: &NodeCtx<'_, Self>, seed: u64, tick: u64) {
            let graph = ctx.graph;
            lanes.run_pass(
                Self::CHUNK,
                seed,
                tick,
                |i, k, read, chunk: &mut RingChunk<'_>, _rng| {
                    let mut lit = read.state[i];
                    for &j in graph.in_neighbors(i as u32) {
                        lit |= read.state[j as usize];
                    }
                    chunk.state[k] = lit;
                },
            );
        }

        fn act(_action: usize, nodes: &mut Nodes<'_, Self>, _extent: Extent, _params: &[ParamValue], _rng: &mut u64) {
            nodes.lanes.state.fill(1);
        }

        fn stats(lanes: &Self::Lanes, _graph: &Network, (): &()) -> Vec<StatValue> {
            vec![StatValue::Scalar(lanes.state.iter().filter(|&&s| s == 1).count() as f64)]
        }
    }

    /// Spawns one node and retires another every tick.
    struct Churn;

    impl NetworkModel for Churn {
        const NAME: &'static str = "Churn";
        const ID: &'static str = "churn";
        const DESCRIPTION: &'static str = "Spawns and retires, for testing the population";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]];
        const EDGE_PALETTE: &'static [[u8; 4]] = &[[128, 128, 128, 255]];
        const STATS: &'static [StatDescriptor] = &[];
        const DEFAULT_NODES: u32 = 4;
        const DEFAULT_EXTENT: Extent = WORLD;

        type Lanes = RingLanes;
        type Params = ();
        type Aux = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            Vec::new()
        }

        fn from_params(_params: &[ParamValue], _extent: Extent) {}

        fn init(nodes: &mut Nodes<'_, Self>, _extent: Extent, _params: &[ParamValue], _rng: &mut u64) {
            nodes.graph.add_edge(0, 1, 0);
        }

        fn run_global_pass(nodes: &mut Nodes<'_, Self>, (): &(), _extent: Extent, _rng: &mut u64, tick: u64) {
            let fresh = nodes.spawn();
            nodes.graph.add_edge(fresh, 0, 0);
            if tick > 0 {
                nodes.retire(1);
            }
        }

        fn stats(_lanes: &Self::Lanes, _graph: &Network, (): &()) -> Vec<StatValue> {
            Vec::new()
        }
    }

    fn ring(nodes: u32) -> NetworkModelState<Ring> {
        let mut params: Vec<ParamValue> = network_model_param_descriptors::<Ring>()
            .iter()
            .map(|d| d.kind.default_value())
            .collect();
        params[NUM_NODES] = ParamValue::U32(nodes);
        NetworkModelState::<Ring>::from_params(&params)
    }

    fn lit(state: &NetworkModelState<Ring>) -> f64 {
        match &state.stats()[0].value {
            StatValue::Scalar(v) => *v,
            other => panic!("Lit is not a scalar: {other:?}"),
        }
    }

    #[test]
    fn the_engine_prepends_the_size_parameters() {
        let descs = network_model_param_descriptors::<Ring>();
        assert_eq!(descs[0].id, "num_agents", "the scaling scripts look this up by id");
        assert_eq!(descs[0].label, "Number of Nodes");
        assert_eq!(descs[1].id, "world_width");
        assert_eq!(descs[2].id, "world_height");
        assert!(
            descs.iter().all(|d| !d.is_live()),
            "size is fixed once a state is built"
        );
    }

    #[test]
    fn the_node_pass_reads_its_neighbours() {
        let mut state = ring(9);
        assert_eq!(lit(&state), 1.0, "one node starts lit");
        // Two more per tick, one each way.
        state.step();
        assert_eq!(lit(&state), 3.0);
        state.step();
        assert_eq!(lit(&state), 5.0);
        for _ in 0..8 {
            state.step();
        }
        assert_eq!(lit(&state), 9.0, "the whole ring is lit");
    }

    #[test]
    fn both_views_describe_the_same_nodes() {
        let state = ring(6);
        let points = state.point_view().expect("a network model draws its nodes");
        let edges = state.edge_view().expect("and its edges");

        assert_eq!(points.pos_x.len(), 6);
        assert_eq!(edges.src.len(), 6, "a ring of six has six edges");
        assert!(!edges.directed);
        assert!(edges.version > 0, "the edges were built, so the version moved");
        for (&a, &b) in edges.src.iter().zip(edges.dst) {
            assert!(
                (a as usize) < points.pos_x.len() && (b as usize) < points.pos_x.len(),
                "an endpoint has no position"
            );
        }
    }

    #[test]
    fn an_action_reaches_the_lanes() {
        let mut state = ring(7);
        assert!(state.act(0));
        assert_eq!(lit(&state), 7.0, "the action lit every node");
        assert!(!state.act(1), "there is no second action");
    }

    #[test]
    fn the_layout_is_accepted_and_moves_nodes() {
        let mut state = ring(12);
        assert!(state.set_layout(true, 4.0), "a network state paces its own layout");

        let before: Vec<u32> = state
            .point_view()
            .expect("points")
            .pos_x
            .iter()
            .map(|v| v.to_bits())
            .collect();
        state.prepare_view();
        let after: Vec<u32> = state
            .point_view()
            .expect("points")
            .pos_x
            .iter()
            .map(|v| v.to_bits())
            .collect();
        assert_ne!(before, after, "the layout ran but nothing moved");

        state.set_layout(false, 4.0);
        let held: Vec<u32> = state
            .point_view()
            .expect("points")
            .pos_x
            .iter()
            .map(|v| v.to_bits())
            .collect();
        state.prepare_view();
        let still: Vec<u32> = state
            .point_view()
            .expect("points")
            .pos_x
            .iter()
            .map(|v| v.to_bits())
            .collect();
        assert_eq!(held, still, "the layout kept running once switched off");
    }

    #[test]
    fn population_counts_the_living_and_jobs_count_the_slots() {
        let params: Vec<ParamValue> = network_model_param_descriptors::<Churn>()
            .iter()
            .map(|d| d.kind.default_value())
            .collect();
        let mut state = NetworkModelState::<Churn>::from_params(&params);
        assert_eq!(state.population(), 4);

        state.step();
        assert_eq!(state.population(), 5, "the first tick only spawns");
        assert_eq!(state.graph().slot_count(), 5);

        // Spawn runs before retire, so a slot retired this tick is reused next tick.
        state.step();
        assert_eq!(state.population(), 5, "one in and one out");
        assert_eq!(state.graph().slot_count(), 6);

        for _ in 0..6 {
            state.step();
        }
        assert_eq!(state.population(), 5, "the population holds");
        assert_eq!(state.graph().slot_count(), 6, "every later spawn reused a retired slot");
        assert_eq!(
            state.parallel_jobs(),
            Some(state.graph().slot_count().div_ceil(Churn::CHUNK)),
            "jobs cover every slot, not every live node"
        );
    }

    #[test]
    fn a_retired_node_keeps_its_lane_row_and_loses_its_position() {
        let params: Vec<ParamValue> = network_model_param_descriptors::<Churn>()
            .iter()
            .map(|d| d.kind.default_value())
            .collect();
        let mut state = NetworkModelState::<Churn>::from_params(&params);
        state.step();
        state.step();

        let points = state.point_view().expect("points");
        assert_eq!(points.pos_x.len(), state.graph().slot_count(), "every slot keeps a row");
        let dead: Vec<usize> = (0..state.graph().slot_count())
            .filter(|&i| !state.graph().contains_node(i as u32))
            .collect();
        assert!(!dead.is_empty(), "the churn retired nobody");
        for i in dead {
            assert!(points.pos_x[i].is_nan(), "a retired node still has a position to draw");
        }
    }

    #[test]
    fn the_graph_holds_its_shape_across_a_repack() {
        let mut state = ring(40);
        for _ in 0..30 {
            state.step();
        }
        let (src, dst, _) = state.graph().edges();
        assert_eq!(src.len(), 40);
        for (&a, &b) in src.iter().zip(dst) {
            assert!(state.graph().has_edge(a, b), "an edge in the list is not in the rows");
        }
    }
}
