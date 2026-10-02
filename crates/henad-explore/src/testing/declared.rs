//! Checks of an entry's declarations, which build nothing.

use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::simulation::RunSetup;
use henad_core::action::Schedule;
use henad_core::metadata::{Backend, Structure};
use henad_core::topology::TopologyHint;

use super::declared_defaults;

/// Parameter ids an engine prepends to a model's own.
const PREPENDED_IDS: [&str; 5] = ["grid_width", "grid_height", "num_agents", "world_width", "world_height"];

/// Checks [`super::ModelCheck::ModelId`] by inserting `entry` into an empty set.
pub(super) fn model_id(entry: &ModelEntry) -> Result<(), String> {
    ModelSet::new(crate::ENGINE_BUILD)
        .insert(entry.clone())
        .map(|_| ())
        .map_err(|error| format!("The set refuses the entry: {error}."))
}

/// Checks [`super::ModelCheck::ParamIds`].
pub(super) fn param_ids(entry: &ModelEntry) -> Result<(), String> {
    let repeated = repeated(entry.param_descriptors().iter().map(|descriptor| descriptor.id));
    if repeated.is_empty() {
        return Ok(());
    }
    let prepended: Vec<&str> = repeated
        .iter()
        .copied()
        .filter(|id| PREPENDED_IDS.contains(id))
        .collect();
    let mut message = format!("Parameter ids {} are declared more than once.", quoted(&repeated));
    if !prepended.is_empty() {
        message.push_str(&format!(
            " The engine prepends {}, and a model declares none of them itself.",
            quoted(&prepended)
        ));
    }
    Err(message)
}

/// Checks [`super::ModelCheck::StatLabels`].
pub(super) fn stat_labels(entry: &ModelEntry) -> Result<(), String> {
    let repeated = repeated(entry.stat_descriptors().iter().map(|descriptor| descriptor.label));
    if repeated.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Stat labels {} are declared more than once.",
        quoted(&repeated)
    ))
}

/// Checks [`super::ModelCheck::ActionIds`]. An id reaches the command line through `--act`, where two the same are
/// ambiguous.
pub(super) fn action_ids(entry: &ModelEntry) -> Result<(), String> {
    let repeated = repeated(entry.action_descriptors().iter().map(|action| action.id));
    if repeated.is_empty() {
        return Ok(());
    }
    Err(format!("Action ids {} are declared more than once.", quoted(&repeated)))
}

/// Checks [`super::ModelCheck::Palette`]. A view colours its cells or agents from the palette by index.
pub(super) fn palette(entry: &ModelEntry) -> Result<(), String> {
    match entry.metadata().palette {
        Some([]) => Err("The declared palette holds no colours.".to_owned()),
        _ => Ok(()),
    }
}

/// Checks [`super::ModelCheck::Metadata`]. Only the Model panel reads the metadata, and nothing else notices a
/// mis-registered entry.
pub(super) fn metadata(entry: &ModelEntry) -> Result<(), String> {
    let metadata = entry.metadata();
    let backend = metadata.backend;
    let gpu = backend == Backend::Gpu;
    let mut problems = Vec::new();

    match entry.demand(&declared_defaults(entry), &wgpu::Limits::default()) {
        Some(_) if !gpu => problems.push(format!("declares {backend:?}, but carries a device capacity")),
        Some(demand) if demand.bytes() == 0 => problems.push("declares a device demand of no bytes".to_owned()),
        None if gpu => problems.push(format!("declares {backend:?}, but carries no device capacity")),
        _ => {}
    }
    if gpu != entry.gpu_needs().is_some() {
        problems.push(format!(
            "declares {backend:?}, but {} device needs",
            if gpu { "declares no" } else { "declares" }
        ));
    }
    if !gpu && !metadata.replays_exactly {
        problems.push("declares that a CPU model does not replay exactly".to_owned());
    }
    let hint = entry.topology_hint();
    let agrees = match &metadata.structure {
        Structure::Grid { .. } | Structure::GpuGrid { .. } => hint == TopologyHint::GRID,
        Structure::Agents { .. } | Structure::GpuAgents { .. } => hint.agents,
        Structure::Network { .. } => hint.agents && hint.edges,
    };
    if !agrees {
        problems.push(format!(
            "declares a structure that the topology hint {hint:?} disagrees with"
        ));
    }
    if gpu
        != matches!(
            metadata.structure,
            Structure::GpuGrid { .. } | Structure::GpuAgents { .. }
        )
    {
        problems.push(format!("declares {backend:?}, but a structure for the other backend"));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("The entry {}.", problems.join(", and it ")))
    }
}

/// Checks [`super::ModelCheck::DefaultSetup`]. A default outside its own bounds, or of another kind than its
/// descriptor, leaves the app's Build disabled on a fresh selection.
pub(super) fn default_setup(entry: &ModelEntry) -> Result<(), String> {
    let setup = entry.setup();
    let defaults = declared_defaults(entry);
    if setup.values() != defaults.as_slice() {
        return Err("The setup at the defaults holds other values than the declared defaults.".to_owned());
    }
    let checked = RunSetup::from_parts(entry, setup.values(), None, Schedule::default())
        .map_err(|error| format!("RunSetup::from_parts refuses the declared defaults: {error}."))?;
    if checked.values() != setup.values() {
        return Err("RunSetup::from_parts changed the declared defaults.".to_owned());
    }
    Ok(())
}

/// Checks [`super::ModelCheck::DefaultsFit`] against the WebGPU baseline, never against the test device's limits.
/// Otherwise a model past the baseline passes every check and is refused at its first Build in a browser.
pub(super) fn defaults_fit(entry: &ModelEntry) -> Result<(), String> {
    let shortfalls = entry.shortfalls(&declared_defaults(entry), &wgpu::Limits::default());
    if shortfalls.is_empty() {
        return Ok(());
    }
    Err(format!(
        "The declared defaults exceed the WebGPU baseline: {}.",
        shortfalls.join("; ")
    ))
}

/// Returns every item of `items` that occurs more than once, each once, in order of its first repeat.
fn repeated<'a>(items: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = Vec::new();
    let mut repeated = Vec::new();
    for item in items {
        if seen.contains(&item) {
            if !repeated.contains(&item) {
                repeated.push(item);
            }
        } else {
            seen.push(item);
        }
    }
    repeated
}

/// Returns `items` quoted and joined by commas.
fn quoted(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| format!("'{item}'"))
        .collect::<Vec<_>>()
        .join(", ")
}
