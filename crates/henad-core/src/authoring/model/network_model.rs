//! Authoring API for network models, where neighbours are joined by edges.
//!
//! Each tick runs a sequential global pass that can change the graph, then a parallel node pass
//! that only reads it.

use crate::action::ActionDescriptor;
use crate::authoring::model::agent_model::AgentLanes;
use crate::authoring::model::field::Extent;
use crate::network::Network;
use crate::params::{ParamDescriptor, ParamValue};
use crate::view::{StatDescriptor, StatValue};

/// Spring layout constants, in units of the mean spacing between nodes.
#[derive(Clone, Copy, Debug)]
pub struct SpringParams {
    /// Pull along an edge, per unit of stretch.
    pub spring: f32,
    /// Rest length of an edge.
    pub length: f32,
    /// Push between nearby nodes.
    pub repulsion: f32,
    /// Distance beyond which nodes stop repelling.
    pub cutoff: f32,
}

impl SpringParams {
    /// Balanced so a joined pair settles a little above the mean spacing.
    ///
    /// Much more `repulsion` pushes every node to the walls.
    pub const DEFAULT: Self = Self {
        spring: 0.3,
        length: 1.0,
        repulsion: 0.05,
        cutoff: 2.5,
    };
}

/// Mutable access to the lanes, the graph and the model's own state.
///
/// Change the node count through [`Self::spawn`] and [`Self::retire`]. They keep the lanes and the
/// graph the same length.
pub struct Nodes<'a, N: NetworkModel + ?Sized> {
    pub lanes: &'a mut N::Lanes,
    pub graph: &'a mut Network,
    pub aux: &'a mut N::Aux,
}

impl<N: NetworkModel + ?Sized> Nodes<'_, N> {
    /// Spawns a new node and returns its index, growing every lane to fit.
    ///
    /// A reused slot keeps the lane values of the node retired from it.
    pub fn spawn(&mut self) -> u32 {
        let i = self.graph.spawn();
        self.lanes.grow(self.graph.slot_count());
        i
    }

    /// Retires node `i` with its edges, and sets its position to `NaN` so nothing draws it.
    pub fn retire(&mut self, i: u32) {
        self.graph.retire(i);
        let (pos_x, pos_y) = self.lanes.positions_mut();
        if let (Some(x), Some(y)) = (pos_x.get_mut(i as usize), pos_y.get_mut(i as usize)) {
            *x = f32::NAN;
            *y = f32::NAN;
        }
    }
}

/// Graph, hot params and extent, shared by every node kernel.
pub struct NodeCtx<'a, N: NetworkModel + ?Sized> {
    pub graph: &'a Network,
    pub params: &'a N::Params,
    pub extent: Extent,
}

/// A population of nodes joined by edges.
pub trait NetworkModel: Send + Sync + 'static {
    const NAME: &'static str;
    const ID: &'static str;
    const DESCRIPTION: &'static str;
    /// Node colours, indexed by the colour lane.
    const PALETTE: &'static [[u8; 4]];
    /// Edge colours, indexed by each edge's colour byte.
    const EDGE_PALETTE: &'static [[u8; 4]];
    /// Stat series for the history chart.
    const STATS: &'static [StatDescriptor];
    /// One-off actions, each shown as a button in the Parameters panel.
    const ACTIONS: &'static [ActionDescriptor] = &[];

    /// Nodes per chunk in the node pass. Each chunk gets its own RNG stream.
    const CHUNK: usize = 512;

    const DEFAULT_NODES: u32;
    const MAX_NODES: u32 = 10_000_000;
    /// World the layout spreads nodes over, for drawing only.
    const DEFAULT_EXTENT: Extent;
    const LAYOUT: SpringParams = SpringParams::DEFAULT;

    /// Node lanes, declared with `agent_lanes!`.
    type Lanes: AgentLanes;
    /// Pre-extracted hot parameters, rebuilt once per tick.
    type Params: Send + Sync;
    /// Model state outside the lanes and the graph, or `()` for none.
    type Aux: Default + Send + 'static;

    /// Model parameters, after the engine's `num_agents`, `world_width` and `world_height`.
    fn param_descriptors() -> Vec<ParamDescriptor>;
    /// Hot params for one tick, from this model's own slice of the params.
    fn from_params(params: &[ParamValue], extent: Extent) -> Self::Params;

    /// Whether edges are directed. Read every tick.
    fn directed(_params: &Self::Params) -> bool {
        false
    }

    /// Seeds the lanes and the edges. The graph starts with every node and no edges.
    fn init(nodes: &mut Nodes<'_, Self>, extent: Extent, params: &[ParamValue], rng: &mut u64);

    /// Sequential pass before the node pass. The graph can change here.
    fn run_global_pass(
        _nodes: &mut Nodes<'_, Self>,
        _params: &Self::Params,
        _extent: Extent,
        _rng: &mut u64,
        _tick: u64,
    ) {
    }

    /// Parallel pass over every slot, retired ones included. Usually one call to `lanes.run_pass`.
    fn run_node_pass(_lanes: &mut Self::Lanes, _ctx: &NodeCtx<'_, Self>, _seed: u64, _tick: u64) {}

    /// Runs [`Self::ACTIONS`] entry `action` between ticks, drawing from its own RNG stream.
    fn act(_action: usize, _nodes: &mut Nodes<'_, Self>, _extent: Extent, _params: &[ParamValue], _rng: &mut u64) {}

    /// Prepares state for drawing. Called before each snapshot, not every tick.
    fn prepare_view(_nodes: &mut Nodes<'_, Self>, _tick: u64) {}

    /// Current statistics, in [`Self::STATS`] order.
    ///
    /// A stat needing a walk of the graph is computed in [`Self::prepare_view`] and kept in `aux`.
    fn stats(lanes: &Self::Lanes, graph: &Network, aux: &Self::Aux) -> Vec<StatValue>;
}
