//! Model entries, the type-erased form in which a host offers a model, and the sets that hold them.
//!
//! A [`ModelEntry`] holds a model's declarations and the factory that builds it on any device. The `register_*`
//! functions make one from an authoring trait. They are generic, so the engine a model steps on is monomorphised in
//! the crate that calls them. A [`ModelSet`] holds the entries a host offers, each id once.
#![cfg_attr(
    all(target_arch = "wasm32", target_feature = "atomics"),
    expect(
        clippy::arc_with_non_send_sync,
        reason = "a factory is not `Send` on wasm with atomics, see `send_sync`"
    )
)]

mod set;

pub use set::{ModelLookupError, ModelSet, ModelSetError, ModelSetIter};

use std::any::type_name;
use std::fmt;
use std::sync::Arc;

use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::agent_model::{AgentLanes, AgentModel, NeighborIndex};
use henad_core::authoring::model::field::FieldLayer;
use henad_core::authoring::model::gpu_agent_model::GpuAgentModel;
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::authoring::model::network_model::NetworkModel;
use henad_core::explore::plan::ModelSchema;
use henad_core::metadata::{Backend, ModelMetadata, Structure};
use henad_core::model::SimState;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::provenance::ModelSource;
use henad_core::send_sync::{WasmNotSend, WasmNotSync};
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

use crate::cpu::agent_engine::{AgentModelState, agent_model_param_descriptors};
use crate::cpu::grid_engine::{GridModelState, grid_model_param_descriptors};
use crate::cpu::network_engine::{NetworkModelState, network_model_param_descriptors};
use crate::fault::{BUILDING, Fault, catching};
use crate::gpu::agent_engine::GpuAgentState;
use crate::gpu::capacity::Demand;
use crate::gpu::fault::catching_on;
use crate::gpu::grid_engine::GpuGridState;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::{GpuContext, GpuNeeds};
use crate::simulation::RunSetup;

/// A freshly built simulation state, tagged with the runner that can drive it.
///
/// The two arms are not interchangeable. A CPU state steps one tick per call on a
/// [`SimThread`](crate::cpu::sim_thread::SimThread), and a GPU state encodes many steps into one submission on a
/// [`GpuSimThread`](crate::gpu::GpuSimThread). A caller picks the runner from the arm without downcasting.
pub enum ModelState {
    Cpu(Box<dyn SimState>),
    Gpu(Box<dyn GpuSimState>),
}

/// Prints the backend alone, since a state holds the whole simulation.
impl fmt::Debug for ModelState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu(_) => f.debug_tuple("Cpu").finish_non_exhaustive(),
            Self::Gpu(_) => f.debug_tuple("Gpu").finish_non_exhaustive(),
        }
    }
}

/// Closure behind a model's factory.
///
/// A CPU model ignores the device. Public for [`ModelEntry::wrap_factory`] alone.
/// The `Option<u64>` is the seed. `None` falls back to the model's default.
#[doc(hidden)]
pub trait Factory:
    Fn(&[ParamValue], Option<u64>, Option<&GpuContext>) -> Result<ModelState, Fault> + WasmNotSend + WasmNotSync
{
}

impl<F> Factory for F where
    F: Fn(&[ParamValue], Option<u64>, Option<&GpuContext>) -> Result<ModelState, Fault> + WasmNotSend + WasmNotSync
{
}

/// Closure that computes a GPU model's device demand at some params, for a device with some limits.
pub(crate) trait Capacity: Fn(&[ParamValue], &wgpu::Limits) -> Demand + WasmNotSend + WasmNotSync {}

impl<F: Fn(&[ParamValue], &wgpu::Limits) -> Demand + WasmNotSend + WasmNotSync> Capacity for F {}

/// One model a host can offer: its declarations, and the factory that builds it on any device.
///
/// Cheap to clone. Clones share the declarations and the factory.
#[derive(Clone)]
pub struct ModelEntry {
    parts: Arc<EntryParts>,
}

#[derive(Clone)]
struct EntryParts {
    id: String,
    name: String,
    description: String,
    param_descriptors: Vec<ParamDescriptor>,
    stat_descriptors: Vec<StatDescriptor>,
    action_descriptors: Vec<ActionDescriptor>,
    topology_hint: TopologyHint,
    metadata: ModelMetadata,
    gpu_needs: Option<GpuNeeds>,
    source: ModelSource,
    factory: Arc<dyn Factory>,
    /// `None` for a CPU model, which allocates on the host and has no device limit to miss.
    capacity: Option<Arc<dyn Capacity>>,
}

/// Prints the declarations, and leaves out the factory and the capacity closures.
impl fmt::Debug for ModelEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = &self.parts;
        f.debug_struct("ModelEntry")
            .field("id", &parts.id)
            .field("name", &parts.name)
            .field("description", &parts.description)
            .field("param_descriptors", &parts.param_descriptors)
            .field("stat_descriptors", &parts.stat_descriptors)
            .field("action_descriptors", &parts.action_descriptors)
            .field("topology_hint", &parts.topology_hint)
            .field("metadata", &parts.metadata)
            .field("gpu_needs", &parts.gpu_needs)
            .field("source", &parts.source)
            .finish_non_exhaustive()
    }
}

impl ModelEntry {
    pub fn id(&self) -> &str {
        &self.parts.id
    }

    pub fn name(&self) -> &str {
        &self.parts.name
    }

    pub fn description(&self) -> &str {
        &self.parts.description
    }

    pub fn param_descriptors(&self) -> &[ParamDescriptor] {
        &self.parts.param_descriptors
    }

    pub fn stat_descriptors(&self) -> &[StatDescriptor] {
        &self.parts.stat_descriptors
    }

    /// One-off steps the model offers, in the order the Parameters panel draws their buttons.
    pub fn action_descriptors(&self) -> &[ActionDescriptor] {
        &self.parts.action_descriptors
    }

    pub fn topology_hint(&self) -> TopologyHint {
        self.parts.topology_hint
    }

    /// Declared facts about the model, derived from its trait consts.
    pub fn metadata(&self) -> &ModelMetadata {
        &self.parts.metadata
    }

    /// Declarations a plan checks a spec against, readable with no device.
    pub fn schema(&self) -> ModelSchema<'_> {
        ModelSchema {
            id: &self.parts.id,
            params: &self.parts.param_descriptors,
            stats: &self.parts.stat_descriptors,
            actions: &self.parts.action_descriptors,
        }
    }

    /// Device needs above the WebGPU baseline. `None` for a CPU model.
    pub fn gpu_needs(&self) -> Option<GpuNeeds> {
        self.parts.gpu_needs
    }

    /// Origin of the model's code: its type path always, and the registering crate's build once a set records it.
    pub fn source(&self) -> &ModelSource {
        &self.parts.source
    }

    /// Returns the position of parameter `id` in [`Self::param_descriptors`].
    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.parts.param_descriptors.iter().position(|desc| desc.id == id)
    }

    /// Returns the position of action `id` in [`Self::action_descriptors`].
    pub fn action_index(&self, id: &str) -> Option<usize> {
        self.parts.action_descriptors.iter().position(|action| action.id == id)
    }

    /// Returns a setup at the declared defaults, the default seed and no actions.
    pub fn setup(&self) -> RunSetup {
        RunSetup::new(self.clone())
    }

    /// Builds the model from positional values, unchecked. [`RunSetup`] is the checked path.
    ///
    /// A CPU model ignores `gpu`, and a GPU model builds on it.
    ///
    /// # Errors
    ///
    /// Returns a [`Fault`] when the build panics, the device refuses it, or a GPU model is handed no device.
    pub fn build(
        &self,
        params: &[ParamValue],
        seed: Option<u64>,
        gpu: Option<&GpuContext>,
    ) -> Result<ModelState, Fault> {
        (self.parts.factory)(params, seed, gpu)
    }

    /// Returns the device resources the model would allocate at `params` on a device with `limits`.
    ///
    /// Returns `None` for a CPU model. It allocates on the host and knows its footprint only once built.
    pub fn demand(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Option<Demand> {
        self.parts.capacity.as_ref().map(|capacity| capacity(params, limits))
    }

    /// Returns the reasons a device with `limits` cannot build the model at `params`, none when nothing stops it.
    pub fn shortfalls(&self, params: &[ParamValue], limits: &wgpu::Limits) -> Vec<String> {
        self.demand(params, limits)
            .map_or_else(Vec::new, |demand| demand.shortfalls(limits))
    }

    /// Returns the entry with its factory wrapped, as henad-explore's test harnesses do to inject a fault.
    #[doc(hidden)]
    pub fn wrap_factory(self, wrap: impl FnOnce(Arc<dyn Factory>) -> Arc<dyn Factory>) -> Self {
        let mut parts = Arc::unwrap_or_clone(self.parts);
        parts.factory = wrap(parts.factory);
        Self { parts: Arc::new(parts) }
    }

    /// Returns the entry with `source` in place of its own.
    fn with_source(mut self, source: ModelSource) -> Self {
        Arc::make_mut(&mut self.parts).source = source;
        self
    }
}

/// Returns the fault a GPU model raises when it is handed no device.
fn no_device(id: &str) -> Fault {
    Fault::refused(
        BUILDING,
        format!("model '{id}' runs on the GPU and needs a device to build on"),
    )
}

/// Returns an entry for the CPU grid model `M`.
pub fn register_grid_model<M: GridModel>() -> ModelEntry {
    ModelEntry {
        parts: Arc::new(EntryParts {
            id: M::ID.to_owned(),
            name: M::NAME.to_owned(),
            description: M::DESCRIPTION.to_owned(),
            param_descriptors: grid_model_param_descriptors::<M>(),
            stat_descriptors: M::STATS.to_vec(),
            action_descriptors: M::ACTIONS.to_vec(),
            topology_hint: TopologyHint::GRID,
            metadata: ModelMetadata {
                backend: Backend::Cpu,
                palette: Some(M::PALETTE),
                structure: Structure::Grid {
                    neighborhood: M::NEIGHBORHOOD,
                },
                replays_exactly: true,
            },
            gpu_needs: None,
            source: ModelSource::__from_type_path(type_name::<M>()),
            factory: Arc::new(|params: &[ParamValue], seed: Option<u64>, _gpu: Option<&GpuContext>| {
                catching(BUILDING, || {
                    ModelState::Cpu(Box::new(GridModelState::<M>::from_params_seeded(params, seed)))
                })
            }),
            capacity: None,
        }),
    }
}

/// Returns an entry for the CPU agent model `A`.
pub fn register_agent_model<A: AgentModel>() -> ModelEntry {
    ModelEntry {
        parts: Arc::new(EntryParts {
            id: A::ID.to_owned(),
            name: A::NAME.to_owned(),
            description: A::DESCRIPTION.to_owned(),
            param_descriptors: agent_model_param_descriptors::<A>(),
            stat_descriptors: A::STATS.to_vec(),
            action_descriptors: A::ACTIONS.to_vec(),
            topology_hint: TopologyHint {
                grid: <A::Field as FieldLayer>::HAS_GRID,
                agents: true,
                edges: false,
            },
            metadata: ModelMetadata {
                backend: Backend::Cpu,
                palette: Some(A::PALETTE),
                structure: Structure::Agents {
                    chunk: A::CHUNK,
                    lanes: <A::Lanes as AgentLanes>::LANES,
                    index: <A::Index as NeighborIndex>::KIND,
                    field: <A::Field as FieldLayer>::KIND,
                },
                replays_exactly: true,
            },
            gpu_needs: None,
            source: ModelSource::__from_type_path(type_name::<A>()),
            factory: Arc::new(|params: &[ParamValue], seed: Option<u64>, _gpu: Option<&GpuContext>| {
                catching(BUILDING, || {
                    ModelState::Cpu(Box::new(AgentModelState::<A>::from_params_seeded(params, seed)))
                })
            }),
            capacity: None,
        }),
    }
}

/// Returns an entry for the CPU network model `N`.
pub fn register_network_model<N: NetworkModel>() -> ModelEntry {
    ModelEntry {
        parts: Arc::new(EntryParts {
            id: N::ID.to_owned(),
            name: N::NAME.to_owned(),
            description: N::DESCRIPTION.to_owned(),
            param_descriptors: network_model_param_descriptors::<N>(),
            stat_descriptors: N::STATS.to_vec(),
            action_descriptors: N::ACTIONS.to_vec(),
            topology_hint: TopologyHint::NETWORK,
            metadata: ModelMetadata {
                backend: Backend::Cpu,
                palette: Some(N::PALETTE),
                structure: Structure::Network {
                    chunk: N::CHUNK,
                    lanes: <N::Lanes as AgentLanes>::LANES,
                    edge_palette: N::EDGE_PALETTE,
                },
                replays_exactly: true,
            },
            gpu_needs: None,
            source: ModelSource::__from_type_path(type_name::<N>()),
            factory: Arc::new(|params: &[ParamValue], seed: Option<u64>, _gpu: Option<&GpuContext>| {
                catching(BUILDING, || {
                    ModelState::Cpu(Box::new(NetworkModelState::<N>::from_params_seeded(params, seed)))
                })
            }),
            capacity: None,
        }),
    }
}

/// Returns an entry for the GPU grid model `M`.
///
/// Every parameter applies on reload, since the GPU state accepts no live edit.
pub fn register_gpu_grid_model<M: GpuGridModel>() -> ModelEntry {
    ModelEntry {
        parts: Arc::new(EntryParts {
            id: M::ID.to_owned(),
            name: M::NAME.to_owned(),
            description: M::DESCRIPTION.to_owned(),
            param_descriptors: M::param_descriptors()
                .into_iter()
                .map(ParamDescriptor::on_reload)
                .collect(),
            stat_descriptors: M::STATS.to_vec(),
            action_descriptors: M::ACTIONS.iter().map(|action| action.desc).collect(),
            // The grid reaches the view as a texture in place of a cell buffer.
            topology_hint: TopologyHint::GRID,
            metadata: ModelMetadata {
                backend: Backend::Gpu,
                palette: Some(M::PALETTE),
                structure: Structure::GpuGrid {
                    buffers: M::BUFFERS,
                    workgroup: M::WORKGROUP_SIZE,
                },
                replays_exactly: M::REPLAYS_EXACTLY,
            },
            gpu_needs: Some(GpuNeeds::with_storage_buffers(GpuGridState::<M>::max_storage_bindings())),
            source: ModelSource::__from_type_path(type_name::<M>()),
            factory: Arc::new(|params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| {
                let ctx = gpu.ok_or_else(|| no_device(M::ID))?;
                catching_on(ctx, BUILDING, || {
                    ModelState::Gpu(Box::new(GpuGridState::<M>::new_seeded(ctx, params, seed)))
                })
            }),
            capacity: Some(Arc::new(GpuGridState::<M>::demand)),
        }),
    }
}

/// Returns an entry for the GPU agent model `M`.
///
/// Every parameter applies on reload, since the GPU state accepts no live edit.
pub fn register_gpu_agent_model<M: GpuAgentModel>() -> ModelEntry {
    ModelEntry {
        parts: Arc::new(EntryParts {
            id: M::ID.to_owned(),
            name: M::NAME.to_owned(),
            description: M::DESCRIPTION.to_owned(),
            param_descriptors: M::param_descriptors()
                .into_iter()
                .map(ParamDescriptor::on_reload)
                .collect(),
            stat_descriptors: M::STATS.to_vec(),
            action_descriptors: M::ACTIONS.iter().map(|action| action.desc).collect(),
            // A model that declares a display pass draws a grid layer under its agents.
            topology_hint: TopologyHint {
                grid: M::DISPLAY.is_some(),
                agents: true,
                edges: false,
            },
            metadata: ModelMetadata {
                backend: Backend::Gpu,
                // A GPU agent model's shaders write RGBA themselves.
                palette: None,
                structure: Structure::GpuAgents {
                    buffers: M::BUFFERS,
                    passes: M::STEP_PASSES,
                    index: M::INDEX,
                    display: M::DISPLAY.is_some(),
                    counters: M::COUNTERS,
                },
                replays_exactly: M::REPLAYS_EXACTLY,
            },
            gpu_needs: Some(GpuNeeds::with_storage_buffers(
                GpuAgentState::<M>::max_storage_bindings(),
            )),
            source: ModelSource::__from_type_path(type_name::<M>()),
            factory: Arc::new(|params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| {
                let ctx = gpu.ok_or_else(|| no_device(M::ID))?;
                catching_on(ctx, BUILDING, || {
                    ModelState::Gpu(Box::new(GpuAgentState::<M>::new_seeded(ctx, params, seed)))
                })
            }),
            capacity: Some(Arc::new(GpuAgentState::<M>::demand)),
        }),
    }
}
