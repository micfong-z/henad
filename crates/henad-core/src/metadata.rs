//! Model metadata the engine derives from a model's declarations.

use crate::authoring::model::gpu_agent_model::{BufferSpec, PassSpec};
use crate::topology::NeighborhoodKind;

/// Model backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The model steps on the CPU.
    Cpu,
    /// The model steps on the GPU.
    Gpu,
}

impl Backend {
    /// Returns the label the UI shows, `CPU` or `GPU`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
        }
    }
}

/// One lane of a CPU agent model's struct-of-arrays storage.
///
/// `agent_lanes!` generates one `LaneSpec` per lane.
#[derive(Debug, Clone, Copy)]
pub struct LaneSpec {
    /// Name of the lane. For a double buffered lane, this is the name of the current side.
    pub name: &'static str,
    /// Element type as written in the declaration.
    pub ty: &'static str,
    /// Whether the lane is declared `dual`, with a second side the kernel writes.
    pub double_buffered: bool,
}

/// Storage layout of a model, by backend and topology.
#[derive(Debug, Clone)]
pub enum Structure {
    /// A CPU grid model.
    Grid {
        /// Neighbourhood a cell's rule reads.
        neighborhood: NeighborhoodKind,
    },
    /// A CPU agent model.
    Agents {
        /// Agents per chunk in a step pass.
        chunk: usize,
        /// Agent lanes, in declaration order.
        lanes: &'static [LaneSpec],
        /// [`crate::authoring::model::agent_model::NeighborIndex::KIND`].
        index: &'static str,
        /// [`crate::authoring::model::field::FieldLayer::KIND`].
        field: &'static str,
    },
    /// A CPU network model.
    Network {
        /// Number of nodes per chunk in the node pass.
        chunk: usize,
        /// Node lanes, in declaration order.
        lanes: &'static [LaneSpec],
        /// Edge colours, indexed by each edge's colour byte.
        edge_palette: &'static [[u8; 4]],
    },
    /// A GPU grid model.
    GpuGrid {
        /// Ping-ponged buffer labels.
        buffers: &'static [&'static str],
        /// Side of the square workgroup that every shader declares.
        workgroup: u32,
    },
    /// A GPU agent model.
    GpuAgents {
        /// Storage buffers, in declaration order.
        buffers: &'static [BufferSpec],
        /// Passes of one step, in the order they run.
        passes: &'static [PassSpec],
        /// Whether the engine rebuilds a neighbour index before every step.
        index: bool,
        /// Whether the model declares a display pass.
        display: bool,
        /// Number of persistent `u32` counters.
        counters: usize,
    },
}

/// Model metadata that the UI displays.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    /// Backend the model steps on.
    pub backend: Backend,
    /// Colours that the display layer or the agent population draws from. `None` for a GPU agent
    /// model, whose shaders write RGBA directly and declare no palette.
    pub palette: Option<&'static [[u8; 4]]>,
    /// Storage layout of the model.
    pub structure: Structure,
    /// Whether two builds on one seed step through identical states. `true` for every CPU model.
    pub replays_exactly: bool,
}
