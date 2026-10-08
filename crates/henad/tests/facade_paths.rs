//! Items the tutorial crate never names, reached through the facade's paths alone.
//!
//! The file defines no model, needs no build script and never builds on a device. The parts that need no model run,
//! and the parts that need a model's entry compile in [`with_an_entry`], which no test calls. That module also holds
//! the two snippets `docs/guide/library.md` includes from outside `examples/complete.rs`. [`named_only`] names the
//! items whose paths are the whole check, and runs nothing.

// Proving a type that holds wgpu handles `Send` or `Sync` walks wgpu-core's registries, deeper than the default
// limit of 128.
#![recursion_limit = "256"]

use henad::SetupError;
use henad::action::ActionDescriptor;
use henad::authoring::{
    ComponentStats, Domain, Extent, GpuAgentAction, HashGrid, Network, PassSpec, SpatialHash, label_components,
};
use henad::explore::factor::{FactorSpec, LevelSpec};
use henad::explore::plan::Shard;
use henad::explore::spec::{ActionSpec, BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec};
use henad::explore::{Concurrency, Provenance, SweepOptions};
use henad::params::ValueError;

/// Returns an action whose pass dispatches once per agent and binds nothing.
fn scatter_action() -> GpuAgentAction {
    GpuAgentAction {
        desc: ActionDescriptor::new("scatter", "Scatter"),
        pass: PassSpec {
            label: "scatter",
            shader: "@compute @workgroup_size(64) fn main() {}",
            bindings: &[],
            domain: Domain::Agents,
        },
    }
}

/// Returns a two-factor sweep of `model` with an action whose tick is one of the factors.
fn spec(model: &str) -> SweepSpec {
    let mut spec = SweepSpec::new(model);
    spec.fixed.push(("grid_width".to_owned(), "64".to_owned()));
    spec.run = RunSettings {
        steps: 200,
        replicates: 3,
        ..RunSettings::default()
    };
    spec.measure = MeasureSettings {
        stats_every: 10,
        ..MeasureSettings::default()
    };
    spec.seeds = SeedSettings {
        root: 7,
        ..SeedSettings::default()
    };
    spec.actions.push(ActionSpec::new("seed_outbreak", 50));
    spec.blocks.push(BlockSpec {
        factors: vec![
            FactorSpec::param(
                "infection_rate",
                LevelSpec::Values(vec!["0.2".to_owned(), "0.4".to_owned()]),
            ),
            FactorSpec::action(
                "seed_outbreak",
                LevelSpec::Range {
                    min: 50.0,
                    max: 150.0,
                    step: Some(50.0),
                },
            ),
        ],
        ..BlockSpec::default()
    });
    spec
}

/// Returns the options of the second of three shards, on two lanes.
fn sharded_options() -> SweepOptions {
    let mut options = SweepOptions::new(Provenance::new(henad::build_info!(), Vec::new()));
    options.concurrency = Concurrency::Fixed(2.try_into().expect("two is not zero"));
    options.shard = Shard::new(1, 3).expect("shard 1 of 3 exists");
    options
}

/// Returns the reason a setup was refused, naming the parameter text that could not be read.
fn refusal(error: &SetupError) -> String {
    match error {
        SetupError::Param(ValueError::NotANumber { raw, .. }) => format!("'{raw}' is not a number"),
        SetupError::Param(ValueError::OutOfRange { value, min, max }) => format!("{value} is outside {min} to {max}"),
        other => other.to_string(),
    }
}

/// Counts the nodes of a three-node path and a lone node within reach of the origin, and labels its components.
fn neighbourhood() -> (usize, ComponentStats) {
    let grid = HashGrid::new(Extent { w: 16.0, h: 16.0 }, 4.0);
    let mut hash = SpatialHash::new(grid.cell_w, 16.0, 16.0);
    let (pos_x, pos_y) = (vec![1.0, 2.0, 3.0, 12.0], vec![1.0, 1.0, 1.0, 12.0]);
    hash.build(&pos_x, &pos_y);
    let mut near = Vec::new();
    hash.query_radius(0.0, 0.0, 4.0, &pos_x, &pos_y, &mut near);

    let mut graph = Network::new(4, false);
    graph.add_edge(0, 1, 0);
    graph.add_edge(1, 2, 0);
    let (mut label, mut scratch) = (Vec::new(), Vec::new());
    (near.len(), label_components(&graph, &mut label, &mut scratch))
}

#[test]
fn a_gpu_action_is_built_from_a_pass() {
    let action = scatter_action();
    assert_eq!(action.desc.id, "scatter", "the descriptor keeps its id");
    assert!(action.pass.bindings.is_empty(), "the pass binds nothing");
}

#[test]
fn a_sweep_spec_is_built_in_code() {
    let spec = spec("sir");
    assert_eq!(
        spec.blocks[0].factors.len(),
        2,
        "the block varies a parameter and an action's tick"
    );
    let options = sharded_options();
    assert_eq!(options.shard.index(), 1, "the options run the second shard");
    assert_eq!(options.concurrency.to_string(), "2", "the options run two lanes");
}

#[test]
fn a_parameter_refusal_names_the_text() {
    let error = SetupError::Param(ValueError::NotANumber {
        raw: "fast".to_owned(),
        source: "fast".parse::<f32>().expect_err("a word is no number"),
    });
    assert_eq!(refusal(&error), "'fast' is not a number", "the refusal names the text");
}

#[test]
fn the_spatial_index_and_the_components_are_reached() {
    let (near, components) = neighbourhood();
    assert_eq!(near, 3, "the three nodes of the path lie within reach of the origin");
    assert_eq!(components.count, 2, "the path and the lone node");
    assert_eq!(components.largest, 3, "the path holds three nodes");
}

/// Items named through their facade paths, with nothing to run. The check is that the module compiles.
#[expect(dead_code, reason = "the module names items, and no test calls them")]
mod named_only {
    use henad::gpu::DeviceError;
    use henad::gpu::wgpu::{Adapter, Limits};

    /// Types a public field, variant or return value of a listed item holds, and types a listed signature names.
    type PayloadTypes = (
        henad::gpu::Alloc,
        henad::gpu::PassBindings,
        henad::explore::BatchStanding,
        henad::explore::EvaluatedCandidate,
        henad::explore::EvaluationReading,
        henad::explore::ConfigFault,
        henad::explore::RefusedConfig,
        henad::explore::ProgressUpdate,
        henad::explore::SummaryError,
        henad::explore::SearchPlan,
        henad::explore::CsvError,
        henad::explore::design_csv::DesignTableError,
        henad::explore::design_csv::DesignTableValueError,
        henad::explore::design_rng::DesignRng,
        henad::action::RefusedActions<'static>,
    );

    /// Steps one command buffer of a GPU state may hold, for a host that encodes its own.
    const SUBMISSION_STEPS: u32 = henad::runner::MAX_STEPS_PER_SUBMISSION;

    /// Returns the limit a refusal of an adapter below the baseline names.
    fn short_limit(error: &DeviceError) -> Option<&'static str> {
        match error {
            DeviceError::BelowBaseline { limit, .. } => Some(*limit),
            _ => None,
        }
    }

    /// Returns the baseline limits raised to what the GPU models of `models` need on `adapter`, for a host that
    /// requests its own device.
    fn limits(adapter: &Adapter, models: &henad::ModelSet) -> Limits {
        henad::gpu::raise_limits(adapter, &Limits::default(), models.gpu_needs())
    }

    /// Calls `sample`, which a native build and a threaded web build can both hand to another thread.
    fn forward<F: FnMut() + henad::WasmNotSend + henad::WasmNotSync>(mut sample: F) {
        sample();
    }
}

/// Hosts that need a model's entry. They compile against the facade and never run.
#[expect(
    dead_code,
    reason = "this file defines no model, and nothing hands these functions an entry"
)]
mod with_an_entry {
    use std::sync::Arc;

    use henad::explore::outcome::RunStatus;
    use henad::explore::{ExploreError, NoProgress, RunRow, SweepOutput, SweepRecord, run_spec};
    use henad::runner::{FaultSink, SimThread, Snapshot, WakeFn};
    use henad::{Fault, ModelEntry, ModelSet, ModelState, Simulation};

    /// Runs the sweep of [`super::spec`] over `entry` into memory, as the second of three shards on two lanes.
    fn sweep(entry: &ModelEntry) -> Result<SweepRecord, ExploreError> {
        let spec = super::spec(entry.id());
        run_spec(
            entry,
            None,
            &spec,
            SweepOutput::Memory,
            &super::sharded_options(),
            &mut NoProgress,
        )
    }

    /// Builds `entry` at `params` on a paced runner, with a waker when `wake` is set, and returns its first snapshot.
    fn paced(entry: &ModelEntry, params: &[henad::params::ParamValue], wake: bool) -> Option<Snapshot> {
        let faults = FaultSink::new();
        let wake: Option<WakeFn> = wake.then(|| Arc::new(|| {}) as WakeFn);
        let ModelState::Cpu(state) = entry.build(params, None, None).ok()? else {
            return None;
        };
        let mut runner = SimThread::new(state, 60.0, wake, faults);
        runner.take_snapshot()
    }

    /// Returns whether a row's run finished without a fault.
    fn finished(row: &RunRow) -> bool {
        let status: RunStatus = row.outcome.status;
        matches!(status, RunStatus::Ok | RunStatus::NonFinite)
    }

    /// Builds GPU SIR from `models` on a headless device of its own, and returns its tick after ten steps.
    fn gpu_sir(models: &ModelSet) -> Result<u64, Box<dyn std::error::Error>> {
        // --8<-- [start:gpu_build]
        let gpu = henad::gpu::acquire_headless(models.gpu_needs())?;
        let mut simulation = models
            .get("gpu_sir")
            .ok_or("the example set lacks GPU SIR")?
            .setup()
            .build(Some(&gpu))?;
        // --8<-- [end:gpu_build]
        simulation.run_for(10)?;
        Ok(simulation.tick())
    }

    /// Steps `simulation` a thousand ticks inside one rayon scope, with room for work of the caller's own between
    /// two ticks.
    fn scoped_steps(simulation: &mut Simulation) -> Result<(), Fault> {
        // --8<-- [start:scoped_steps]
        rayon::scope(|_| -> Result<(), henad::Fault> {
            for _ in 0..1000 {
                simulation.step()?;
                // Per-tick work of your own.
            }
            Ok(())
        })?;
        // --8<-- [end:scoped_steps]
        Ok(())
    }
}
