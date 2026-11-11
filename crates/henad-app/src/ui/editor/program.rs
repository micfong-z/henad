//! Compiled programs of graph models, and the count of documents loaded, which the graph editor and the sheet
//! editor share.

use std::sync::atomic::AtomicU64;

use henad_compute::entry::ModelEntry;
use henad_core::diagnostic::CodeDiagnostic;
use henad_graph::GraphFile;
use henad_graph::document::World;
use henad_graph::wgsl::{AgentProgram, GeneratedShader, GridProgram, Probes};

/// Number of graphs and sheets loaded so far. It keeps the egui state of each graph's canvases apart, and gives each
/// document a preview key of its own.
pub(crate) static LOADS: AtomicU64 = AtomicU64::new(0);

/// Compiled program of a graph model of either topology.
#[derive(Clone)]
pub enum GraphProgram {
    /// A model on a grid.
    Grid(GridProgram),
    /// A model of agents.
    Agents(AgentProgram),
}

/// Prints the model's id and the topology.
impl std::fmt::Debug for GraphProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (topology, id) = match self {
            Self::Grid(program) => ("grid", program.id()),
            Self::Agents(program) => ("agents", program.id()),
        };
        f.debug_struct("GraphProgram")
            .field("topology", &topology)
            .field("id", &id)
            .finish_non_exhaustive()
    }
}

impl GraphProgram {
    /// Compiles `file` through the compiler of its topology.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the file compiling.
    pub fn compile(file: &GraphFile) -> CompiledProgram {
        Self::compile_with_probes(file, &Probes::default())
    }

    /// Compiles `file` through the compiler of its topology, each step counting the fires of its probes in `probes`.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the file compiling, a probe that names no `bool` output of a tick pass included.
    pub fn compile_with_probes(file: &GraphFile, probes: &Probes) -> CompiledProgram {
        match file.document().world {
            World::Grid(_) => GridProgram::compile_with_probes(file, probes).map(Self::Grid),
            World::Agents(_) => AgentProgram::compile_with_probes(file, probes).map(Self::Agents),
            _ => Err(vec![CodeDiagnostic::error(
                "The Editor runs graphs on a grid or of agents.",
            )]),
        }
    }

    /// Generated shaders, in the order the engine runs them.
    pub fn shaders(&self) -> &[GeneratedShader] {
        match self {
            Self::Grid(program) => program.shaders(),
            Self::Agents(program) => program.shaders(),
        }
    }

    /// Returns the model entry that builds the program.
    pub fn entry(&self) -> ModelEntry {
        match self {
            Self::Grid(program) => program.entry(),
            Self::Agents(program) => program.entry(),
        }
    }
}

/// Program of one revision of the document: the compiled model, or the problems that stop it compiling.
pub type CompiledProgram = Result<GraphProgram, Vec<CodeDiagnostic>>;
