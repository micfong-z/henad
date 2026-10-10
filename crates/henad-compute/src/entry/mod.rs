//! Model entries, the type-erased form in which a host offers a model, and the sets that hold them.
//!
//! A [`ModelEntry`] holds a model's declarations and the factory that builds it on any device. The `register_*`
//! functions make one from an authoring trait. They are generic, so the engine that runs a model is monomorphised in
//! the crate that calls them. A [`ModelSet`] holds the entries a host offers, each id once.
#![cfg_attr(
    all(target_arch = "wasm32", target_feature = "atomics"),
    expect(
        clippy::arc_with_non_send_sync,
        reason = "a factory is not `Send` on wasm with atomics, see `send_sync`"
    )
)]

mod set;

#[cfg(feature = "runtime-wgsl")]
pub(crate) use set::check_id_grammar;
pub use set::{ModelLookupError, ModelSet, ModelSetError, ModelSetIter};

use std::any::type_name;
use std::borrow::Cow;
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
use henad_core::provenance::{ModelCode, ModelSource};
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
use crate::gpu::grid_engine::GpuGridEngine;
use crate::gpu::grid_spec::GpuGridSpec;
use crate::gpu::sim_thread::GpuSimState;
use crate::gpu::{GpuContext, GpuNeeds};
use crate::simulation::RunSetup;

/// A freshly built simulation state, tagged with the runner that can drive it.
///
/// The two arms are not interchangeable. A CPU state steps one tick per call on a
/// [`SimThread`](crate::cpu::sim_thread::SimThread), and a GPU state encodes many steps into one submission on a
/// [`GpuSimThread`](crate::gpu::GpuSimThread). A caller picks the runner from the arm without downcasting.
pub enum ModelState {
    /// A CPU state, for a [`SimThread`](crate::cpu::sim_thread::SimThread).
    Cpu(Box<dyn SimState>),
    /// A GPU state, for a [`GpuSimThread`](crate::gpu::GpuSimThread).
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
/// The `Option<u64>` is the seed. `None` falls back to the model's default seed.
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

/// Declarations of one model, as values: everything an entry holds apart from its source and how it builds.
///
/// The backend comes from the constructor that takes the declarations, [`ModelEntry::new_cpu`] or
/// [`ModelEntry::new_gpu`].
#[derive(Debug)]
pub(crate) struct EntryDeclarations {
    /// Id in the model id grammar, checked when the entry joins a set.
    pub(crate) id: String,
    /// Name shown in the UI.
    pub(crate) name: String,
    /// One-line description shown in the UI.
    pub(crate) description: String,
    /// Every parameter, the engine's own included, in the order a build reads its values.
    pub(crate) params: Vec<ParamDescriptor>,
    /// Stats, in the order the state returns them.
    pub(crate) stats: Vec<StatDescriptor>,
    /// One-off steps, in the order the Parameters panel draws their buttons.
    pub(crate) actions: Vec<ActionDescriptor>,
    /// Display layers the model presents.
    pub(crate) topology_hint: TopologyHint,
    /// Storage layout of the model.
    pub(crate) structure: Structure,
    /// Colours of the display layer or the agent population, `None` for a model whose shaders write RGBA.
    pub(crate) palette: Option<Cow<'static, [[u8; 4]]>>,
    /// Whether two builds on one seed step through identical states.
    pub(crate) replays_exactly: bool,
}

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
    /// Closure that computes the device demand, `None` for a CPU model. A CPU model allocates on the host and has no
    /// device limit to miss.
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
    /// Stable id that identifies the model in a model set, on the command line and in a spec file.
    pub fn id(&self) -> &str {
        &self.parts.id
    }

    /// Name shown in the UI.
    pub fn name(&self) -> &str {
        &self.parts.name
    }

    /// One-line description shown in the UI.
    pub fn description(&self) -> &str {
        &self.parts.description
    }

    /// Every parameter, in the order a build reads its values.
    pub fn param_descriptors(&self) -> &[ParamDescriptor] {
        &self.parts.param_descriptors
    }

    /// Stats the model reports, in the order its state returns them.
    pub fn stat_descriptors(&self) -> &[StatDescriptor] {
        &self.parts.stat_descriptors
    }

    /// One-off steps the model offers, in the order the Parameters panel draws their buttons.
    pub fn action_descriptors(&self) -> &[ActionDescriptor] {
        &self.parts.action_descriptors
    }

    /// Display layers the model presents.
    pub fn topology_hint(&self) -> TopologyHint {
        self.parts.topology_hint
    }

    /// Declared facts about the model, derived from its declarations.
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

    /// Origin of the model's code: its type path always, and the registering crate's build once a set records it,
    /// or the code of a model composed at run time.
    pub fn source(&self) -> &ModelSource {
        &self.parts.source
    }

    /// Returns the position of parameter `id` in [`Self::param_descriptors`].
    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.parts.param_descriptors.iter().position(|desc| desc.id() == id)
    }

    /// Returns the position of action `id` in [`Self::action_descriptors`].
    pub fn action_index(&self, id: &str) -> Option<usize> {
        self.parts
            .action_descriptors
            .iter()
            .position(|action| action.id() == id)
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
    /// Returns a [`Fault`] when the build panics, the device rejects it, or a GPU model receives no device.
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

    /// Returns the reasons a device with `limits` cannot build the model at `params`, empty when it can.
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

    /// Returns the entry with its source marked as code that a dev host loaded from a plugin. A runtime source stays
    /// runtime.
    #[doc(hidden)]
    pub fn __into_plugin(self) -> Self {
        let source = self.parts.source.clone().__into_plugin();
        self.with_source(source)
    }

    /// Returns the entry with the source of a model composed at run time from `code`, labelled with the entry's id.
    ///
    /// The entry builds its state as before, and its source records no build. Tests give a compiled model runtime code
    /// this way.
    #[doc(hidden)]
    pub fn __with_code(self, code: ModelCode) -> Self {
        let source = ModelSource::runtime(self.id(), code);
        self.with_source(source)
    }

    /// Returns the entry with its source replaced by `source`.
    fn with_source(mut self, source: ModelSource) -> Self {
        Arc::make_mut(&mut self.parts).source = source;
        self
    }
}

impl ModelEntry {
    /// Returns the entry of a CPU model with `declarations` and `source`, whose `build` returns a state for
    /// positional parameter values and a seed.
    ///
    /// The entry holds no device needs and no capacity check. Each build runs inside [`catching`], and a panic
    /// returns a [`Fault`].
    pub(crate) fn new_cpu<F>(declarations: EntryDeclarations, source: ModelSource, build: F) -> Self
    where
        F: Fn(&[ParamValue], Option<u64>) -> Box<dyn SimState> + WasmNotSend + WasmNotSync + 'static,
    {
        let factory = move |params: &[ParamValue], seed: Option<u64>, _gpu: Option<&GpuContext>| {
            catching(BUILDING, || ModelState::Cpu(build(params, seed)))
        };
        Self::from_declarations(declarations, Backend::Cpu, source, Arc::new(factory), None, None)
    }

    /// Returns the entry of a GPU model with `declarations` and `source`, whose `build` returns a state on a device,
    /// and whose `demand` computes its device resources before anything is allocated.
    ///
    /// `needs` are the device needs above the WebGPU baseline that a host requests before any model builds. A build
    /// with no device returns a refused [`Fault`]. Each build runs inside [`catching_on`], and a panic or a device
    /// error returns a [`Fault`].
    pub(crate) fn new_gpu<F, D>(
        declarations: EntryDeclarations,
        source: ModelSource,
        needs: GpuNeeds,
        build: F,
        demand: D,
    ) -> Self
    where
        F: Fn(&GpuContext, &[ParamValue], Option<u64>) -> Box<dyn GpuSimState> + WasmNotSend + WasmNotSync + 'static,
        D: Capacity + 'static,
    {
        let id = declarations.id.clone();
        let factory = move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| {
            let ctx = gpu.ok_or_else(|| no_device(&id))?;
            catching_on(ctx, BUILDING, || ModelState::Gpu(build(ctx, params, seed)))
        };
        Self::from_declarations(
            declarations,
            Backend::Gpu,
            source,
            Arc::new(factory),
            Some(needs),
            Some(Arc::new(demand)),
        )
    }

    /// Returns the entry of a GPU grid model with `declarations` and `source`, whose builds and demand share `spec`.
    pub(crate) fn from_grid_spec(declarations: EntryDeclarations, source: ModelSource, spec: Arc<GpuGridSpec>) -> Self {
        let needs = GpuNeeds::with_storage_buffers(spec.max_storage_bindings());
        let build_spec = Arc::clone(&spec);
        Self::new_gpu(
            declarations,
            source,
            needs,
            move |ctx, params, seed| -> Box<dyn GpuSimState> {
                Box::new(GpuGridEngine::new(ctx, Arc::clone(&build_spec), params, seed))
            },
            move |params, limits| spec.demand(params, limits),
        )
    }

    /// Returns the entry that holds `declarations`, with the metadata of a model on `backend`.
    fn from_declarations(
        declarations: EntryDeclarations,
        backend: Backend,
        source: ModelSource,
        factory: Arc<dyn Factory>,
        gpu_needs: Option<GpuNeeds>,
        capacity: Option<Arc<dyn Capacity>>,
    ) -> Self {
        let EntryDeclarations {
            id,
            name,
            description,
            params,
            stats,
            actions,
            topology_hint,
            structure,
            palette,
            replays_exactly,
        } = declarations;
        let metadata = ModelMetadata::new(backend, structure).replays_exactly(replays_exactly);
        let metadata = match palette {
            Some(palette) => metadata.palette(palette),
            None => metadata,
        };
        Self {
            parts: Arc::new(EntryParts {
                id,
                name,
                description,
                param_descriptors: params,
                stat_descriptors: stats,
                action_descriptors: actions,
                topology_hint,
                metadata,
                gpu_needs,
                source,
                factory,
                capacity,
            }),
        }
    }
}

/// Returns the fault a GPU model raises when it receives no device.
fn no_device(id: &str) -> Fault {
    Fault::refused(
        BUILDING,
        format!("model '{id}' runs on the GPU and needs a device to build on"),
    )
}

/// Returns an entry for the CPU grid model `M`.
pub fn register_grid_model<M: GridModel>() -> ModelEntry {
    let declarations = EntryDeclarations {
        id: M::ID.to_owned(),
        name: M::NAME.to_owned(),
        description: M::DESCRIPTION.to_owned(),
        params: grid_model_param_descriptors::<M>(),
        stats: M::STATS.to_vec(),
        actions: M::ACTIONS.to_vec(),
        topology_hint: TopologyHint::GRID,
        structure: Structure::Grid {
            neighborhood: M::NEIGHBORHOOD,
        },
        palette: Some(Cow::Borrowed(M::PALETTE)),
        replays_exactly: true,
    };
    let source = ModelSource::__from_type_path(type_name::<M>());
    ModelEntry::new_cpu(declarations, source, |params, seed| {
        Box::new(GridModelState::<M>::from_params_seeded(params, seed))
    })
}

/// Returns an entry for the CPU agent model `A`.
pub fn register_agent_model<A: AgentModel>() -> ModelEntry {
    let declarations = EntryDeclarations {
        id: A::ID.to_owned(),
        name: A::NAME.to_owned(),
        description: A::DESCRIPTION.to_owned(),
        params: agent_model_param_descriptors::<A>(),
        stats: A::STATS.to_vec(),
        actions: A::ACTIONS.to_vec(),
        topology_hint: TopologyHint {
            grid: <A::Field as FieldLayer>::HAS_GRID,
            agents: true,
            edges: false,
        },
        structure: Structure::Agents {
            chunk: A::CHUNK,
            lanes: <A::Lanes as AgentLanes>::LANES,
            index: <A::Index as NeighborIndex>::KIND,
            field: <A::Field as FieldLayer>::KIND,
        },
        palette: Some(Cow::Borrowed(A::PALETTE)),
        replays_exactly: true,
    };
    let source = ModelSource::__from_type_path(type_name::<A>());
    ModelEntry::new_cpu(declarations, source, |params, seed| {
        Box::new(AgentModelState::<A>::from_params_seeded(params, seed))
    })
}

/// Returns an entry for the CPU network model `N`.
pub fn register_network_model<N: NetworkModel>() -> ModelEntry {
    let declarations = EntryDeclarations {
        id: N::ID.to_owned(),
        name: N::NAME.to_owned(),
        description: N::DESCRIPTION.to_owned(),
        params: network_model_param_descriptors::<N>(),
        stats: N::STATS.to_vec(),
        actions: N::ACTIONS.to_vec(),
        topology_hint: TopologyHint::NETWORK,
        structure: Structure::Network {
            chunk: N::CHUNK,
            lanes: <N::Lanes as AgentLanes>::LANES,
            edge_palette: N::EDGE_PALETTE,
        },
        palette: Some(Cow::Borrowed(N::PALETTE)),
        replays_exactly: true,
    };
    let source = ModelSource::__from_type_path(type_name::<N>());
    ModelEntry::new_cpu(declarations, source, |params, seed| {
        Box::new(NetworkModelState::<N>::from_params_seeded(params, seed))
    })
}

/// Returns an entry for the GPU grid model `M`.
///
/// Every parameter applies on reload, since the GPU state accepts no live edit.
pub fn register_gpu_grid_model<M: GpuGridModel>() -> ModelEntry {
    let declarations = EntryDeclarations {
        id: M::ID.to_owned(),
        name: M::NAME.to_owned(),
        description: M::DESCRIPTION.to_owned(),
        params: M::param_descriptors()
            .into_iter()
            .map(ParamDescriptor::on_reload)
            .collect(),
        stats: M::STATS.to_vec(),
        actions: M::ACTIONS.iter().map(|action| action.desc.clone()).collect(),
        // The grid reaches the view as a texture instead of a cell buffer.
        topology_hint: TopologyHint::GRID,
        structure: Structure::GpuGrid {
            buffers: M::BUFFERS,
            workgroup: M::WORKGROUP_SIZE,
        },
        palette: Some(Cow::Borrowed(M::PALETTE)),
        replays_exactly: M::REPLAYS_EXACTLY,
    };
    let source = ModelSource::__from_type_path(type_name::<M>());
    ModelEntry::from_grid_spec(declarations, source, Arc::new(GpuGridSpec::from_model::<M>()))
}

/// Returns an entry for the GPU agent model `M`.
///
/// Every parameter applies on reload, since the GPU state accepts no live edit.
pub fn register_gpu_agent_model<M: GpuAgentModel>() -> ModelEntry {
    let declarations = EntryDeclarations {
        id: M::ID.to_owned(),
        name: M::NAME.to_owned(),
        description: M::DESCRIPTION.to_owned(),
        params: M::param_descriptors()
            .into_iter()
            .map(ParamDescriptor::on_reload)
            .collect(),
        stats: M::STATS.to_vec(),
        actions: M::ACTIONS.iter().map(|action| action.desc.clone()).collect(),
        // A model that declares a display pass draws a grid layer under its agents.
        topology_hint: TopologyHint {
            grid: M::DISPLAY.is_some(),
            agents: true,
            edges: false,
        },
        structure: Structure::GpuAgents {
            buffers: M::BUFFERS,
            passes: M::STEP_PASSES,
            index: M::INDEX,
            display: M::DISPLAY.is_some(),
            counters: M::COUNTERS,
        },
        // No palette. A GPU agent model's shaders write RGBA themselves.
        palette: None,
        replays_exactly: M::REPLAYS_EXACTLY,
    };
    let source = ModelSource::__from_type_path(type_name::<M>());
    let needs = GpuNeeds::with_storage_buffers(GpuAgentState::<M>::max_storage_bindings());
    ModelEntry::new_gpu(
        declarations,
        source,
        needs,
        |ctx, params, seed| Box::new(GpuAgentState::<M>::new_seeded(ctx, params, seed)),
        GpuAgentState::<M>::demand,
    )
}

#[cfg(test)]
mod tests {
    use std::any::type_name;
    use std::borrow::Cow;

    use henad_core::action::ActionDescriptor;
    use henad_core::authoring::model::grid_model::GridModel;
    use henad_core::authoring::primitives::rng::{below, next_bits};
    use henad_core::explore::fingerprint::schema_hash;
    use henad_core::grid::Grid2D;
    use henad_core::helpers::{extract_f32, f32_param};
    use henad_core::metadata::Structure;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::provenance::ModelSource;
    use henad_core::topology::{NeighborhoodKind, TopologyHint};
    use henad_core::view::{StatDescriptor, StatValue};

    use super::{EntryDeclarations, ModelEntry, ModelState, register_grid_model};
    use crate::cpu::grid_engine::{GridModelState, grid_model_param_descriptors};

    /// Grid model whose cells switch on at random, with a parameter, a stat, an action and a palette to compare.
    struct Flicker;

    impl GridModel for Flicker {
        const NAME: &'static str = "Flicker";
        const ID: &'static str = "flicker";
        const DESCRIPTION: &'static str = "Cells that switch on at random.";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255], [255, 255, 255, 255]];
        const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::VonNeumann;
        const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("On", [255, 255, 255, 255])];
        const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor::new("clear", "Clear")];
        type Params = f32;

        fn param_descriptors() -> Vec<ParamDescriptor> {
            vec![f32_param("chance", "Chance", 0.2, 0.0, 1.0, Some(0.01))]
        }

        fn from_params(params: &[ParamValue]) -> f32 {
            extract_f32(params, 0, 0.2)
        }

        fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

        fn step_cell(cell: u8, _neighbors: &[u8], chance: &f32, rng: &mut u64) -> u8 {
            let threshold = (*chance * u32::MAX as f32) as u32;
            if below(next_bits(rng), threshold) { 1 } else { cell }
        }

        fn act(_action: usize, grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {
            grid.current_mut().fill(0);
        }

        fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
            let on = grid.current().iter().filter(|&&cell| cell == 1).count();
            vec![StatValue::Scalar(on as f64)]
        }
    }

    /// Returns the stat values of `entry` built at its defaults on 32 by 16 cells and seed 7, after each of 4 steps.
    fn stepped_stats(entry: &ModelEntry) -> Vec<f64> {
        let mut params = entry.setup().values().to_vec();
        params[0] = ParamValue::U32(32);
        params[1] = ParamValue::U32(16);
        let Ok(ModelState::Cpu(mut state)) = entry.build(&params, Some(7), None) else {
            panic!("a CPU model builds without a device");
        };
        (0..4)
            .flat_map(|_| {
                state.step();
                state.stats().into_iter().map(|stat| stat.value().scalar())
            })
            .collect()
    }

    #[test]
    fn an_entry_from_values_reads_like_the_registered_one() {
        let declarations = EntryDeclarations {
            id: "flicker".to_owned(),
            name: "Flicker".to_owned(),
            description: "Cells that switch on at random.".to_owned(),
            params: grid_model_param_descriptors::<Flicker>(),
            stats: vec![StatDescriptor::owned("On".to_owned(), [255, 255, 255, 255])],
            actions: vec![ActionDescriptor::owned("clear".to_owned(), "Clear".to_owned())],
            topology_hint: TopologyHint::GRID,
            structure: Structure::Grid {
                neighborhood: NeighborhoodKind::VonNeumann,
            },
            palette: Some(Cow::Owned(vec![[0, 0, 0, 255], [255, 255, 255, 255]])),
            replays_exactly: true,
        };
        let source = ModelSource::__from_type_path(type_name::<Flicker>());
        let from_values = ModelEntry::new_cpu(declarations, source, |params, seed| {
            Box::new(GridModelState::<Flicker>::from_params_seeded(params, seed))
        });
        let registered = register_grid_model::<Flicker>();

        assert_eq!(
            format!("{from_values:?}"),
            format!("{registered:?}"),
            "the two entries hold the same declarations, metadata, needs and source"
        );
        assert_eq!(schema_hash(&from_values.schema()), schema_hash(&registered.schema()));
        assert_eq!(from_values.gpu_needs(), None);
        assert!(from_values.demand(&[], &wgpu::Limits::default()).is_none());
        let stats = stepped_stats(&from_values);
        assert!(stats.iter().any(|&on| on > 0.0), "{stats:?}");
        assert_eq!(stats, stepped_stats(&registered));
    }
}
