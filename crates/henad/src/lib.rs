#![cfg_attr(all(feature = "app", feature = "example-models"), doc = include_str!("../README.md"))]
#![cfg_attr(
    not(all(feature = "app", feature = "example-models")),
    doc = "Henad, a parallel agent-based modelling engine."
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(ambiguous_glob_reexports)]

pub use henad_compute::entry::{ModelEntry, ModelLookupError, ModelSet, ModelSetError, ModelState};
pub use henad_compute::fault::{Fault, FaultKind, install_panic_hook};
pub use henad_compute::simulation::{ExportError, RunSetup, SetupError, Simulation, SimulationViews, StatSample};
pub use henad_core::metadata::{Backend, LaneSpec, ModelMetadata, Structure};
pub use henad_core::provenance::{BuildInfo, ModelSource};
pub use henad_core::topology::TopologyHint;
pub use henad_explore::ENGINE_BUILD;

// A glob, since henad-core's `params` names a module as well as the macro. The facade's own `params` module
// shadows the first.
pub use root_macros::*;

mod root_macros {
    pub use henad_compute::{agent_lanes, for_each_chunk_mut, include_shaders};
    pub use henad_core::{actions, buffers, build_info, params};
}

/// Parameter descriptors and values, and the text form `--set` reads.
pub mod params {
    pub use henad_core::explore::value::{ValueError, format_value};
    pub use henad_core::params::{ParamApply, ParamDescriptor, ParamFormat, ParamKind, ParamValue};
}

/// Statistic descriptors and values, and the CSV writer of a stat series.
pub mod stats {
    pub use henad_core::export::{StatColumns, StatsWriteError, StatsWriter};
    pub use henad_core::view::{StatDescriptor, StatEntry, StatValue};
}

/// Views a CPU model's state hands a host to draw.
pub mod views {
    pub use henad_core::view::{EdgeView, GridView, PointView};
}

/// Action descriptors and the schedule of actions a run fires.
pub mod action {
    pub use henad_core::action::{ActionDescriptor, Fire, Schedule, ScheduleError, Scheduled};
}

/// The GPU device a model builds on, its sizing, and the headless device a program acquires.
pub mod gpu {
    pub use henad_compute::gpu::{Demand, GpuContext, GpuNeeds, wgpu};
    pub use henad_compute::runtime_info::{HostInfo, RuntimeInfo};
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
    pub use henad_explore::device::{DeviceError, acquire_headless};
}

/// Paced runners that step a model off the host's thread, and the snapshots they publish.
pub mod runner {
    pub use henad_compute::cpu::sim_thread::{SimCommand, SimThread, WakeFn};
    pub use henad_compute::fault::FaultSink;
    pub use henad_compute::gpu::sim_thread::GpuBatchSettings;
    pub use henad_compute::gpu::{GpuAgents, GpuDisplay, GpuSimState, GpuSimThread, GpuStats, StatsPoll};
    pub use henad_compute::snapshot::{
        CpuLayers, EdgeSnapshot, GpuSnapshot, GridSnapshot, PointSnapshot, Snapshot, SnapshotView,
    };
    pub use henad_core::model::SimState;
}

/// Engine states a test or a host builds directly, without an entry.
pub mod engine {
    pub use henad_compute::cpu::network_engine::NetworkModelState;
    pub use henad_compute::cpu::{AgentModelState, GridModelState};
    pub use henad_compute::gpu::{GpuAgentState, GpuGridState};
}

/// Sweeps, searches, result folders and replay.
///
/// The planning modules keep their names, and a type inside one of them has its path there, as in
/// [`spec::BlockSpec`](explore::spec::BlockSpec).
pub mod explore {
    pub use henad_core::explore::replay::Replay;
    pub use henad_core::explore::{design, factor, measure, outcome, plan, reducer, search, seed, spec, stop};
    pub use henad_explore::exec::{ActiveRun, Concurrency, ExecutionError, ExecutionLayout, SweepControl};
    pub use henad_explore::handle::{
        SweepEvent, SweepOutput, SweepPhase, SweepProgress, SweepRun, SweepRunOptions, SweepStartError,
    };
    pub use henad_explore::merge::{MergeError, MergeReport, merge};
    pub use henad_explore::output::OutputError;
    pub use henad_explore::output::manifest::{
        BuildRole, Manifest, ManifestAxisRanges, ManifestBlock, ManifestColumns, ManifestDesignTable, ManifestError,
        ManifestExecution, ManifestLimits, ManifestMode, ManifestModel, ManifestPlan, ManifestRuntime, ManifestSearch,
        ManifestSeeds, ManifestSession, ManifestShard, ManifestSpecSource, ManifestStatus, ManifestTimestamps,
        RecordedBuild, ResultCounts,
    };
    pub use henad_explore::output::memory::SweepFiles;
    pub use henad_explore::output::read::ReadError;
    pub use henad_explore::output::resume::ResumeError;
    pub use henad_explore::output::search_tables::SearchHistory;
    pub use henad_explore::probe::{CapacityError, ProbeError, ProbeReport};
    pub use henad_explore::progress::{NoProgress, Progress, ProgressEvent};
    pub use henad_explore::result_set::{ResultReplayError, ResultSet, ResultSetError, RunRow};
    pub use henad_explore::schema::schema_json;
    pub use henad_explore::search_run::{SearchOutline, SearchPlanError, SearchUpdate};
    pub use henad_explore::spec_file::{DesignTableFile, ExecutionTable, LoadedSpec, SpecFileError};
    pub use henad_explore::sweep::{
        ExploreError, Provenance, SpecSource, SweepEnd, SweepOptions, SweepOutline, SweepRecord, SweepReport,
        SweepWarning,
    };
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
    pub use henad_explore::{
        result_set::{DirectorySeries, read_directory_series},
        sweep::{plan_spec, run_spec},
    };
}

/// Benchmarks that time a model's steps, as `henad-cli` does.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
pub mod benchmark {
    pub use henad_explore::benchmark::{
        BenchmarkEvent, BenchmarkReport, BenchmarkSettings, RepetitionReport, run_benchmark,
    };
}

/// Everything a model is written against: the model traits, their types, the helpers and the primitives.
pub mod authoring {
    pub use henad_core::authoring::model::agent_model::*;
    pub use henad_core::authoring::model::binding::*;
    pub use henad_core::authoring::model::field::*;
    pub use henad_core::authoring::model::gpu_agent_model::*;
    pub use henad_core::authoring::model::gpu_grid_model::*;
    pub use henad_core::authoring::model::grid_model::*;
    pub use henad_core::authoring::model::network_model::*;
    pub use henad_core::grid::Grid2D;
    pub use henad_core::helpers::*;
    pub use henad_core::network::Network;
    pub use henad_core::spatial_hash::{HashGrid, SpatialHash};
    pub use henad_core::topology::NeighborhoodKind;

    pub use henad_compute::cpu::agent_engine::{
        AGENT_INIT_SEED, AGENT_PARAM_BASE, NUM_AGENTS, WORLD_HEIGHT, WORLD_WIDTH, agent_init_rng,
        agent_model_param_descriptors, split_params,
    };
    pub use henad_compute::cpu::field::ca::CaField;
    pub use henad_compute::cpu::field::scalar::{Deposits, ScalarField, ScalarFieldSpec, ScalarRead};
    pub use henad_compute::cpu::grid_engine::{GRID_INIT_SEED, GRID_PARAM_BASE, grid_init_rng};
    pub use henad_compute::cpu::network_engine::{
        NETWORK_INIT_SEED, NETWORK_PARAM_BASE, NUM_NODES, network_model_param_descriptors,
    };
    pub use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
    pub use henad_compute::cpu::primitives::components::{ComponentStats, label_components};
    pub use henad_compute::cpu::primitives::scatter::Combine;
    pub use henad_compute::entry::{
        register_agent_model, register_gpu_agent_model, register_gpu_grid_model, register_grid_model,
        register_network_model,
    };

    pub use bytemuck;

    /// Primitives a kernel calls, each module named as its WGSL twin is imported.
    pub mod primitives {
        pub use henad_core::authoring::primitives::{rng, space, wgsl};
    }

    /// Names a model's source file imports whole.
    pub mod prelude {
        pub use henad_core::action::ActionDescriptor;
        pub use henad_core::authoring::model::agent_model::{AgentModel, NoIndex, StepCtx};
        pub use henad_core::authoring::model::binding::BindingDecl;
        pub use henad_core::authoring::model::field::{Extent, FieldLayer, NoField};
        pub use henad_core::authoring::model::gpu_agent_model::GpuAgentModel;
        pub use henad_core::authoring::model::gpu_grid_model::GpuGridModel;
        pub use henad_core::authoring::model::grid_model::GridModel;
        pub use henad_core::authoring::model::network_model::{NetworkModel, NodeCtx, Nodes};
        pub use henad_core::authoring::primitives::rng::{
            below, choice3, mix_seed, next_bits, next_float, next_index, random_float, reservoir_accept, xorshift64,
        };
        pub use henad_core::authoring::primitives::space::{
            Boundary, MOORE_COLUMN_MAJOR, MOORE_ROW_MAJOR, VON_NEUMANN, cell_index, dist_sq, offset_cell,
        };
        pub use henad_core::grid::Grid2D;
        pub use henad_core::helpers::*;
        pub use henad_core::network::Network;
        pub use henad_core::params::{ParamDescriptor, ParamValue};
        pub use henad_core::spatial_hash::SpatialHash;
        pub use henad_core::topology::NeighborhoodKind;
        pub use henad_core::view::{StatDescriptor, StatValue};

        pub use henad_compute::cpu::agent_engine::{AGENT_PARAM_BASE, agent_init_rng};
        pub use henad_compute::cpu::field::ca::CaField;
        pub use henad_compute::cpu::field::scalar::{Deposits, ScalarField, ScalarFieldSpec, ScalarRead};
        pub use henad_compute::cpu::grid_engine::{GRID_PARAM_BASE, grid_init_rng};
        pub use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
        pub use henad_compute::cpu::primitives::scatter::Combine;

        pub use bytemuck;
    }
}

/// The ten example models and the set that registers them.
#[cfg(feature = "example-models")]
#[cfg_attr(docsrs, doc(cfg(feature = "example-models")))]
pub mod models {
    pub use henad_models::ants::AntsModel;
    pub use henad_models::boids::BoidsModel;
    pub use henad_models::game_of_life::GameOfLifeModel;
    pub use henad_models::gpu_ants::GpuAnts;
    pub use henad_models::gpu_boids::GpuBoids;
    pub use henad_models::gpu_game_of_life::GpuGameOfLife;
    pub use henad_models::gpu_sir::GpuSir;
    pub use henad_models::sir::SirGridModel;
    pub use henad_models::team_assembly::TeamAssembly;
    pub use henad_models::virus_network::VirusNetwork;
    pub use henad_models::{
        ants, boids, example_models, game_of_life, gpu_ants, gpu_boids, gpu_game_of_life, gpu_sir, sir, team_assembly,
        virus_network,
    };
}

/// The app, opened over a host's own model set.
#[cfg(feature = "app")]
#[cfg_attr(docsrs, doc(cfg(feature = "app")))]
pub mod app {
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
    pub use henad_app::{AppError, results_folder, run_native};
    pub use henad_app::{AppOpening, AppOptions, OpenAt};
    #[cfg(target_arch = "wasm32")]
    #[cfg_attr(docsrs, doc(cfg(target_arch = "wasm32")))]
    pub use henad_app::{WebStartError, init_web_logger, start_web};
}

/// The command line, run over a host's own model set.
#[cfg(all(feature = "cli", not(target_arch = "wasm32")))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "cli", not(target_arch = "wasm32")))))]
pub mod cli {
    pub use henad_cli::{CliOptions, SOME_RUNS_NOT_OK, run};
}

#[cfg(feature = "testing")]
#[cfg_attr(docsrs, doc(cfg(feature = "testing")))]
pub use henad_explore::testing;

/// Names a program that builds, runs and sweeps models imports whole.
pub mod prelude {
    pub use henad_compute::entry::ModelSet;
    pub use henad_compute::simulation::{RunSetup, Simulation, StatSample};
    pub use henad_core::explore::replay::Replay;
    pub use henad_core::explore::spec::SweepSpec;
    pub use henad_core::params::ParamValue;
    pub use henad_core::view::StatValue;
    pub use henad_explore::handle::SweepOutput;
    pub use henad_explore::progress::NoProgress;
    pub use henad_explore::result_set::ResultSet;
    pub use henad_explore::spec_file::LoadedSpec;
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
    pub use henad_explore::sweep::run_spec;
    pub use henad_explore::sweep::{Provenance, SweepOptions};

    #[cfg(feature = "app")]
    #[cfg_attr(docsrs, doc(cfg(feature = "app")))]
    pub use henad_app::{AppOpening, AppOptions, OpenAt};
}

#[cfg(test)]
mod tests {
    /// Returns the lines of the README's first `rust,no_run` fence, each with its line break.
    fn readme_program(readme: &str) -> String {
        let mut lines = readme.split_inclusive('\n');
        assert!(
            lines.by_ref().any(|line| line.trim_end() == "```rust,no_run"),
            "the README holds a `rust,no_run` fence"
        );
        lines.take_while(|line| line.trim_end() != "```").collect()
    }

    /// Returns the example without the snippet markers that the guide includes its regions by.
    fn example_program(example: &str) -> String {
        example
            .split_inclusive('\n')
            .filter(|line| !line.trim_start().starts_with("// --8<--"))
            .collect()
    }

    #[test]
    fn the_readme_program_matches_the_example() {
        let readme = readme_program(include_str!("../README.md"));
        assert!(
            readme == example_program(include_str!("../examples/complete.rs")),
            "README.md's program differs from examples/complete.rs, and the two are held equal byte for byte, \
             apart from the example's snippet markers"
        );
    }
}
