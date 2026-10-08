//! Checks of a state built at the check values against what its entry declares.

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::GpuContext;
use henad_core::metadata::Backend;
use henad_core::model::SimState;
use henad_core::params::ParamValue;

use super::{SEED, fault_text};

/// Builds `entry` at `values`, on `gpu` for a GPU model.
fn build(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> Result<ModelState, String> {
    entry
        .build(values, Some(SEED), gpu)
        .map_err(|fault| format!("The model did not build. {}", fault_text(&fault)))
}

/// Returns the runner interface of either variant of `ModelState`.
fn sim_state(state: &mut ModelState) -> &mut dyn SimState {
    match state {
        ModelState::Cpu(state) => state.as_mut(),
        ModelState::Gpu(state) => state.as_mut(),
    }
}

/// Checks [`super::ModelCheck::ApplyModes`]. The Parameters panel labels a parameter from its descriptor and the
/// state decides what it accepts. When the descriptor and the state disagree, the panel misreports an edit.
pub(super) fn apply_modes(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> Result<(), String> {
    let mut built = build(entry, values, gpu)?;
    let state = sim_state(&mut built);
    let mut disagreeing = Vec::new();
    for (index, descriptor) in entry.param_descriptors().iter().enumerate() {
        let accepted = state.set_param(index, &values[index]);
        if accepted != descriptor.is_live() {
            disagreeing.push(format!(
                "'{}' is declared {:?}, but the state {} a live edit",
                descriptor.id,
                descriptor.apply,
                if accepted { "accepts" } else { "refuses" }
            ));
        }
    }
    if disagreeing.is_empty() {
        Ok(())
    } else {
        Err(format!("Parameter {}.", disagreeing.join(", and ")))
    }
}

/// Checks [`super::ModelCheck::Views`]. Nothing else reads the topology hint, and it drifts from the views unless
/// checked.
pub(super) fn views(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> Result<(), String> {
    let backend = entry.metadata().backend;
    let hint = entry.topology_hint();
    let built = build(entry, values, gpu)?;
    let (grid, agents, edges) = match (&built, backend) {
        (ModelState::Cpu(state), Backend::Cpu) => (
            Some(state.grid_view().is_some()),
            Some(state.point_view().is_some()),
            Some(state.edge_view().is_some()),
        ),
        // A GPU state publishes its views through its snapshot.
        (ModelState::Gpu(state), Backend::Gpu) => {
            let view = state.view();
            (Some(view.display.is_some()), Some(view.agents.is_some()), None)
        }
        _ => {
            return Err(format!(
                "The entry declares {backend:?}, but its factory returns the other arm."
            ));
        }
    };
    let mut disagreeing = Vec::new();
    for (layer, declared, found) in [
        ("grid", hint.grid, grid),
        ("agents", hint.agents, agents),
        ("edges", hint.edges, edges),
    ] {
        if found.is_some_and(|found| found != declared) {
            disagreeing.push(format!("{layer}={declared}"));
        }
    }
    if disagreeing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "The topology hint declares {}, but the state's views disagree.",
            disagreeing.join(" and ")
        ))
    }
}

/// Checks [`super::ModelCheck::ParallelJobs`]. The benchmark CSV carries the job count beside the thread count, and a
/// blank cell there has to mean a GPU model.
pub(super) fn parallel_jobs(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> Result<(), String> {
    let mut built = build(entry, values, gpu)?;
    let cpu = matches!(built, ModelState::Cpu(_));
    match sim_state(&mut built).parallel_jobs() {
        Some(0) => Err("A step splits into no jobs.".to_owned()),
        Some(jobs) if !cpu => Err(format!("A GPU state reports {jobs} parallel jobs.")),
        None if cpu => Err("A CPU state reports no parallel jobs.".to_owned()),
        _ => Ok(()),
    }
}

/// Checks [`super::ModelCheck::Actions`]. The Parameters panel draws a button per declared action and the state
/// decides what it runs. When the declaration and the state disagree, a button does nothing.
pub(super) fn actions(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> Result<(), String> {
    let mut built = build(entry, values, gpu)?;
    let state = sim_state(&mut built);
    let declared = entry.action_descriptors();
    let refused: Vec<String> = declared
        .iter()
        .enumerate()
        .filter(|&(index, _)| !state.act(index))
        .map(|(index, action)| format!("'{}' at index {index}", action.id))
        .collect();
    if !refused.is_empty() {
        return Err(format!("The state refuses declared action {}.", refused.join(" and ")));
    }
    if state.act(declared.len()) {
        return Err(format!(
            "The state accepts an action at index {}, past the {} it declares.",
            declared.len(),
            declared.len()
        ));
    }
    Ok(())
}

/// Checks [`super::ModelCheck::StatCount`]. Labels and colours are declared once and paired with values by position,
/// and a model that returns too few values loses its trailing series without an error.
pub(super) fn stat_count(entry: &ModelEntry, values: &[ParamValue], gpu: Option<&GpuContext>) -> Result<(), String> {
    let mut built = build(entry, values, gpu)?;
    let found = sim_state(&mut built).stats().len();
    let declared = entry.stat_descriptors().len();
    if found == declared {
        Ok(())
    } else {
        Err(format!(
            "The entry declares {declared} stats, and the state returns {found} values."
        ))
    }
}
