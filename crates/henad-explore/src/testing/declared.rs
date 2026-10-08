//! Checks of an entry's declarations, which build nothing.

use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::simulation::RunSetup;
use henad_core::action::Schedule;
use henad_core::explore::spec::ACTION_COLUMN_PREFIX;
use henad_core::metadata::{Backend, Structure};
use henad_core::topology::TopologyHint;

use super::{declared_defaults, error_text};

/// Checks [`super::ModelCheck::ModelId`] by inserting `entry` into an empty set.
pub(super) fn model_id(entry: &ModelEntry) -> Result<(), String> {
    ModelSet::new(crate::ENGINE_BUILD)
        .insert(entry.clone())
        .map(|_| ())
        .map_err(|error| format!("The set refuses the entry: {error}."))
}

/// Checks [`super::ModelCheck::ParamIds`].
pub(super) fn param_ids(entry: &ModelEntry) -> Result<(), String> {
    let ids = entry.param_descriptors().iter().map(|descriptor| descriptor.id);
    let unnamed = unnameable(ids, |id| {
        id.starts_with(ACTION_COLUMN_PREFIX)
            .then_some("starts with 'action.', the prefix of an action's tick")
    });
    let message = [
        repeated_param_ids(entry),
        unnamed.map(|reasons| named_ids("parameter", &reasons)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    if message.is_empty() { Ok(()) } else { Err(message) }
}

/// Returns the sentences naming the parameter ids that `entry` declares more than once, `None` when no id repeats.
fn repeated_param_ids(entry: &ModelEntry) -> Option<String> {
    let repeated = repeated(entry.param_descriptors().iter().map(|descriptor| descriptor.id));
    if repeated.is_empty() {
        return None;
    }
    let engine_ids = prepended_ids(&entry.metadata().structure);
    let prepended: Vec<&str> = repeated.iter().copied().filter(|id| engine_ids.contains(id)).collect();
    let mut message = declared_twice("Parameter id", "Parameter ids", &repeated);
    match prepended.as_slice() {
        [] => {}
        [id] => message.push_str(&format!(
            " The engine prepends '{id}', and a model does not declare it itself."
        )),
        _ => message.push_str(&format!(
            " The engine prepends {}, and a model declares none of them itself.",
            quoted(&prepended)
        )),
    }
    Some(message)
}

/// Returns the parameter ids that the CPU engine for `structure` prepends to a model's own parameters. A GPU model
/// declares every parameter itself.
fn prepended_ids(structure: &Structure) -> &'static [&'static str] {
    match structure {
        Structure::Grid { .. } => &["grid_width", "grid_height"],
        Structure::Agents { .. } | Structure::Network { .. } => &["num_agents", "world_width", "world_height"],
        Structure::GpuGrid { .. } | Structure::GpuAgents { .. } => &[],
    }
}

/// Checks [`super::ModelCheck::StatLabels`].
pub(super) fn stat_labels(entry: &ModelEntry) -> Result<(), String> {
    let repeated = repeated(entry.stat_descriptors().iter().map(|descriptor| descriptor.label));
    if repeated.is_empty() {
        return Ok(());
    }
    Err(declared_twice("Stat label", "Stat labels", &repeated))
}

/// Checks [`super::ModelCheck::ActionIds`]. An id reaches the command line through `--act`, where duplicate ids are
/// ambiguous. It also identifies the action in `--vary action.NAME=LEVELS` and a design table's `action.NAME` column.
pub(super) fn action_ids(entry: &ModelEntry) -> Result<(), String> {
    let ids = || entry.action_descriptors().iter().map(|action| action.id);
    let repeated = repeated(ids());
    let repeated = (!repeated.is_empty()).then(|| declared_twice("Action id", "Action ids", &repeated));
    let unnamed = unnameable(ids(), |_| None).map(|reasons| named_ids("action", &reasons));
    let message = [repeated, unnamed].into_iter().flatten().collect::<Vec<_>>().join(" ");
    if message.is_empty() { Ok(()) } else { Err(message) }
}

/// Returns each id of `ids` that is invalid on the command line, in a spec file or in a design table, with the
/// reason, or `None` when every id is valid.
///
/// An id is rejected when it is empty or holds whitespace or `=`, or when `extra` returns a reason for it.
fn unnameable<'a>(
    ids: impl Iterator<Item = &'a str>,
    extra: impl Fn(&str) -> Option<&'static str>,
) -> Option<Vec<(&'a str, &'static str)>> {
    let refused: Vec<(&str, &str)> = ids
        .filter_map(|id| {
            let reason = if id.is_empty() {
                Some("is empty")
            } else if id.contains(char::is_whitespace) {
                Some("holds whitespace")
            } else if id.contains('=') {
                Some("holds '='")
            } else {
                extra(id)
            };
            reason.map(|reason| (id, reason))
        })
        .collect();
    (!refused.is_empty()).then_some(refused)
}

/// Returns the sentences that name each rejected `kind` id with its reason, from the `(id, reason)` pairs that
/// `unnameable` returns.
fn named_ids(kind: &str, refused: &[(&str, &str)]) -> String {
    let mut message: String = refused
        .iter()
        .map(|(id, reason)| format!("The {kind} id '{id}' {reason}. "))
        .collect();
    message.push_str(match refused {
        [_] => "The command line and a design table cannot name it.",
        _ => "The command line and a design table cannot name them.",
    });
    message
}

/// Checks [`super::ModelCheck::Palette`]. A view colours its cells or agents from the palette by index.
pub(super) fn palette(entry: &ModelEntry) -> Result<(), String> {
    match entry.metadata().palette {
        Some([]) => Err("The declared palette holds no colors.".to_owned()),
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

/// Checks [`super::ModelCheck::DefaultSetup`]. A default outside its own bounds, or of a different kind from its
/// descriptor, leaves the app's Build disabled on a fresh selection.
pub(super) fn default_setup(entry: &ModelEntry) -> Result<(), String> {
    let setup = entry.setup();
    let defaults = declared_defaults(entry);
    if setup.values() != defaults.as_slice() {
        return Err("The setup at the defaults holds other values than the declared defaults.".to_owned());
    }
    let checked = RunSetup::from_parts(entry, setup.values(), None, Schedule::default()).map_err(|error| {
        format!(
            "RunSetup::from_parts refuses the declared defaults: {}.",
            error_text(&error)
        )
    })?;
    if checked.values() != setup.values() {
        return Err("RunSetup::from_parts changed the declared defaults.".to_owned());
    }
    Ok(())
}

/// Checks [`super::ModelCheck::DefaultsFit`] against the WebGPU baseline, whatever limits the test device has.
/// Otherwise a model past the baseline passes every check and is rejected at its first Build in a browser.
pub(super) fn defaults_fit(entry: &ModelEntry) -> Result<(), String> {
    let shortfalls = entry.shortfalls(&declared_defaults(entry), &wgpu::Limits::default());
    if shortfalls.is_empty() {
        return Ok(());
    }
    Err(format!(
        "At the declared defaults, the model exceeds the WebGPU baseline: {}.",
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

/// Returns the sentence saying that `repeated` are declared more than once, each named by `singular` or `plural`.
fn declared_twice(singular: &str, plural: &str, repeated: &[&str]) -> String {
    match repeated {
        [item] => format!("{singular} '{item}' is declared more than once."),
        _ => format!("{plural} {} are declared more than once.", quoted(repeated)),
    }
}

/// Returns `items` quoted and joined by commas.
fn quoted(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| format!("'{item}'"))
        .collect::<Vec<_>>()
        .join(", ")
}
