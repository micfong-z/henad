//! Declaration tables: rules V2, V24, V25, V26 and V28 outside node bodies.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use henad_core::params::ParamKind;

use super::Checker;
use crate::document::{Domain, LaneRole, PassStage, Topology, World};
use crate::ids::{NodeId, is_id, is_model_id, is_port_segment};
use crate::limits::{
    MAX_ACTIONS, MAX_AGENTS, MAX_CATEGORIES, MAX_COUNTERS, MAX_DESCRIPTION_BYTES, MAX_FIELDS, MAX_FUNCTIONS,
    MAX_GRID_SIDE, MAX_ID_BYTES, MAX_LABEL_BYTES, MAX_LANES_PER_DOMAIN, MAX_LAYERS_PER_FIELD, MAX_PALETTE_COLORS,
    MAX_PALETTES, MAX_PARAMS, MAX_PASSES, MAX_STATS, MAX_VARIANTS, MAX_WORLD_SIDE,
};
use crate::types::ValueType;
use crate::value::Literal;

/// Parameter ids the prepended world parameters hold.
const RESERVED_PARAM_IDS: [&str; 5] = ["grid_width", "grid_height", "num_agents", "world_width", "world_height"];

/// Checks every declaration table of the file.
pub(super) fn check(checker: &mut Checker<'_>) {
    check_header(checker);
    check_world(checker);
    check_categories(checker);
    check_palettes(checker);
    check_lanes(checker);
    check_fields(checker);
    check_counters(checker);
    check_params(checker);
    check_stats(checker);
    check_actions(checker);
    check_functions(checker);
    check_pass_ids(checker);
}

/// Reports `id` when it breaks the id grammar or repeats an id of its kind.
fn check_id<'a>(
    checker: &mut Checker<'_>,
    kind: &str,
    id: &'a str,
    seen: &mut BTreeSet<&'a str>,
    place: Option<Range<usize>>,
) {
    if !is_id(id) {
        checker.error(
            format!("the {kind} id '{id}' is no id, which is a lowercase letter, then lowercase letters, digits and single `_`, up to {MAX_ID_BYTES} bytes"),
            place.clone(),
        );
    }
    if !seen.insert(id) {
        checker.error(format!("the file declares the {kind} '{id}' twice"), place);
    }
}

/// Reports a list past its limit.
fn check_count(checker: &mut Checker<'_>, what: &str, count: usize, max: usize, place: Option<Range<usize>>) {
    if count > max {
        checker.error(
            format!("the file holds {count} {what}, and a file holds at most {max}"),
            place,
        );
    }
}

/// Reports a label past its limit.
fn check_label(checker: &mut Checker<'_>, what: &str, label: &str, place: Option<Range<usize>>) {
    if label.len() > MAX_LABEL_BYTES {
        checker.error(
            format!(
                "{what} holds {} bytes, and a label holds at most {MAX_LABEL_BYTES}",
                label.len()
            ),
            place,
        );
    }
}

fn check_header(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = checker.file.places();
    if !is_model_id(&document.id) {
        checker.error(
            format!(
                "the model id '{}' is no id, which is a lowercase letter, then lowercase letters, digits and `_`",
                document.id
            ),
            Some(places.id.clone()),
        );
    }
    if !is_model_id(&document.target.id) {
        checker.error(
            format!(
                "the target id '{}' is no id, which is a lowercase letter, then lowercase letters, digits and `_`",
                document.target.id
            ),
            places.target.clone(),
        );
    }
    if document.library < 1 {
        checker.error("`library` counts from 1".to_owned(), Some(places.graph.clone()));
    }
    check_label(checker, "the model name", &document.name, Some(places.id.clone()));
    check_label(checker, "the target name", &document.target.name, places.target.clone());
    for description in [&document.description, &document.target.description] {
        if description.len() > MAX_DESCRIPTION_BYTES {
            checker.error(
                format!(
                    "a description holds {} bytes, and a description holds at most {MAX_DESCRIPTION_BYTES}",
                    description.len()
                ),
                Some(places.id.clone()),
            );
        }
    }
}

fn check_world(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let place = checker
        .file
        .places()
        .world
        .clone()
        .or_else(|| Some(checker.file.places().graph.clone()));
    match &document.world {
        World::Grid(grid) => {
            for (key, side) in [("width", grid.width), ("height", grid.height)] {
                if !(1..=MAX_GRID_SIDE).contains(&side) {
                    checker.error(
                        format!("the grid's `{key}` is {side}, outside 1 to {MAX_GRID_SIDE}"),
                        place.clone(),
                    );
                }
            }
        }
        World::Agents(agents) => {
            if agents.max_count > MAX_AGENTS {
                checker.error(
                    format!(
                        "`max_count` is {}, and a population holds at most {MAX_AGENTS}",
                        agents.max_count
                    ),
                    place.clone(),
                );
            }
            if agents.count > agents.max_count {
                checker.error(
                    format!("`count` is {}, past `max_count` of {}", agents.count, agents.max_count),
                    place.clone(),
                );
            }
            for (key, side) in [("width", agents.width), ("height", agents.height)] {
                if !(1.0..=MAX_WORLD_SIDE).contains(&side) {
                    checker.error(
                        format!("the world's `{key}` is {side}, outside 1 to {MAX_WORLD_SIDE}"),
                        place.clone(),
                    );
                }
            }
        }
    }
}

fn check_categories(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().categories;
    check_count(
        checker,
        "categories",
        document.categories.len(),
        MAX_CATEGORIES,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, category) in document.categories.iter().enumerate() {
        let place = places.get(index).cloned();
        check_id(checker, "category", &category.id, &mut seen, place.clone());
        if category.variants.is_empty() || category.variants.len() > MAX_VARIANTS {
            checker.error(
                format!(
                    "the category '{}' holds {} variants, and a category holds 1 to {MAX_VARIANTS}",
                    category.id,
                    category.variants.len()
                ),
                place.clone(),
            );
        }
        let mut names = BTreeSet::new();
        for variant in &category.variants {
            if !is_port_segment(&variant.name) {
                checker.error(
                    format!(
                        "the category '{}' names the variant '{}', outside the grammar of a port name",
                        category.id, variant.name
                    ),
                    place.clone(),
                );
            }
            if !names.insert(variant.name.as_str()) {
                checker.error(
                    format!(
                        "the category '{}' names the variant '{}' twice",
                        category.id, variant.name
                    ),
                    place.clone(),
                );
            }
            check_label(checker, "a variant label", &variant.label, place.clone());
        }
    }
}

fn check_palettes(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().palettes;
    check_count(
        checker,
        "palettes",
        document.palettes.len(),
        MAX_PALETTES,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, palette) in document.palettes.iter().enumerate() {
        let place = places.get(index).cloned();
        check_id(checker, "palette", &palette.id, &mut seen, place.clone());
        if document.category(&palette.id).is_some() {
            checker.error(
                format!(
                    "the palette '{}' takes the id of a category, and a palette's id is its own",
                    palette.id
                ),
                place.clone(),
            );
        }
        if palette.colors.is_empty() || palette.colors.len() > MAX_PALETTE_COLORS {
            checker.error(
                format!(
                    "the palette '{}' holds {} colours, and a palette holds 1 to {MAX_PALETTE_COLORS}",
                    palette.id,
                    palette.colors.len()
                ),
                place,
            );
        }
    }
}

/// Returns the clause saying why `initial` does not fit `value_type`, or `Ok` when it fits.
fn check_initial(checker: &Checker<'_>, value_type: &ValueType, initial: &Literal) -> Result<(), String> {
    let found = initial.value_type();
    if found != *value_type {
        return Err(format!("is {found}, and the type is {value_type}"));
    }
    if let Literal::Category(name) = initial {
        let known = checker
            .file
            .document()
            .category(&name.owner)
            .is_some_and(|category| category.variants.iter().any(|variant| variant.name == name.member));
        if !known {
            return Err(format!("names the variant '{name}', which the file does not declare"));
        }
    }
    Ok(())
}

/// Returns the clause saying what `value_type` names that the file does not declare, or `Ok` when it resolves.
fn check_declared(checker: &Checker<'_>, value_type: &ValueType) -> Result<(), String> {
    let document = checker.file.document();
    match value_type {
        ValueType::Category(id) if document.category(id).is_none() => Err(format!(
            "needs the category '{id}', and the file declares no such category"
        )),
        ValueType::Layer(id) if document.field(id).is_none() => {
            Err(format!("needs the field '{id}', and the file declares no such field"))
        }
        _ => Ok(()),
    }
}

fn check_lanes(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().lanes;
    let topology = document.world.topology();
    let mut seen = BTreeSet::new();
    let mut per_domain: BTreeMap<&str, usize> = BTreeMap::new();
    let mut positions = 0usize;
    for (index, lane) in document.lanes.iter().enumerate() {
        let place = places.get(index).cloned();
        check_id(checker, "lane", &lane.id, &mut seen, place.clone());
        let domain_fits = match topology {
            Topology::Grid => lane.domain == Domain::Cells,
            Topology::Agents => lane.domain == Domain::Agents,
        };
        if !domain_fits {
            checker.error(
                format!(
                    "the lane '{}' belongs to {}, which the {} topology has no lanes of",
                    lane.id,
                    lane.domain.as_str(),
                    topology.as_str()
                ),
                place.clone(),
            );
        }
        *per_domain.entry(lane.domain.as_str()).or_default() += 1;
        if lane.value_type.is_host_only() {
            checker.error(
                format!(
                    "the lane '{}' has the type {}, which has no form on the GPU, where lanes live",
                    lane.id, lane.value_type
                ),
                place.clone(),
            );
        }
        if let Err(reason) = check_declared(checker, &lane.value_type) {
            checker.error(format!("the lane '{}' {reason}", lane.id), place.clone());
        }
        if let Some(initial) = &lane.initial
            && let Err(reason) = check_initial(checker, &lane.value_type, initial)
        {
            checker.error(
                format!("the `initial` of the lane '{}' {reason}", lane.id),
                place.clone(),
            );
        }
        if lane.role == Some(LaneRole::Position) {
            positions += 1;
            if lane.value_type != ValueType::Vec2F || topology == Topology::Grid {
                checker.error(
                    format!("the position lane '{}' is a `vec2f` lane of agents", lane.id),
                    place.clone(),
                );
            }
        }
    }
    for (domain, count) in per_domain {
        if count > MAX_LANES_PER_DOMAIN {
            checker.error(
                format!("the file holds {count} lanes of {domain}, and a domain holds at most {MAX_LANES_PER_DOMAIN}"),
                places.first().cloned(),
            );
        }
    }
    let place = Some(checker.file.places().graph.clone());
    match topology {
        Topology::Grid if document.lanes.is_empty() => {
            checker.error("a grid model needs a lane of cells".to_owned(), place);
        }
        Topology::Agents if positions != 1 => {
            checker.error(
                format!(
                    "an agents model has exactly one lane with `role = \"position\"`, and this file has {positions}"
                ),
                place,
            );
        }
        _ => {}
    }
}

fn check_fields(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().fields;
    check_count(
        checker,
        "fields",
        document.fields.len(),
        MAX_FIELDS,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, field) in document.fields.iter().enumerate() {
        let place = places.get(index).cloned();
        check_id(checker, "field", &field.id, &mut seen, place.clone());
        if document.lane(&field.id).is_some() {
            checker.error(
                format!("the field '{}' takes the id of a lane", field.id),
                place.clone(),
            );
        }
        if document.world.topology() == Topology::Grid {
            checker.error(
                format!(
                    "the field '{}' needs the agents topology, and this file is a grid",
                    field.id
                ),
                place.clone(),
            );
        }
        if field.value_type != ValueType::F32 && !matches!(field.value_type, ValueType::Category(_)) {
            checker.error(
                format!(
                    "the field '{}' has the type {}, and a field holds `f32` or a category",
                    field.id, field.value_type
                ),
                place.clone(),
            );
        }
        if let Err(reason) = check_declared(checker, &field.value_type) {
            checker.error(format!("the field '{}' {reason}", field.id), place.clone());
        }
        if let Some(initial) = &field.initial
            && let Err(reason) = check_initial(checker, &field.value_type, initial)
        {
            checker.error(
                format!("the `initial` of the field '{}' {reason}", field.id),
                place.clone(),
            );
        }
        if field.layers.is_empty() || field.layers.len() > MAX_LAYERS_PER_FIELD {
            checker.error(
                format!(
                    "the field '{}' holds {} layers, and a field holds 1 to {MAX_LAYERS_PER_FIELD}",
                    field.id,
                    field.layers.len()
                ),
                place.clone(),
            );
        }
        if matches!(field.value_type, ValueType::Category(_)) && !field.is_static {
            checker.error(
                format!("the category field '{}' must be static", field.id),
                place.clone(),
            );
        }
        if !field.is_static && field.combine.is_none() {
            checker.error(
                format!("the field '{}' takes deposits, and needs a `combine`", field.id),
                place.clone(),
            );
        }
        if let Some(finish) = &field.finish
            && document.function(finish).is_none()
        {
            checker.error(
                format!(
                    "the field '{}' finishes with '{finish}', and the file declares no such function",
                    field.id
                ),
                place,
            );
        }
    }
}

fn check_counters(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().counters;
    check_count(
        checker,
        "counters",
        document.counters.len(),
        MAX_COUNTERS,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, counter) in document.counters.iter().enumerate() {
        let place = places.get(index).cloned();
        check_id(checker, "counter", &counter.id, &mut seen, place.clone());
        if document.world.topology() == Topology::Grid {
            checker.error(
                format!(
                    "the counter '{}' needs the agents topology, and this file is a grid",
                    counter.id
                ),
                place,
            );
        }
    }
}

fn check_params(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().params;
    check_count(
        checker,
        "parameters",
        document.params.len(),
        MAX_PARAMS,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, param) in document.params.iter().enumerate() {
        let place = places.get(index).cloned();
        let id = param.id();
        check_id(checker, "parameter", id, &mut seen, place.clone());
        if RESERVED_PARAM_IDS.contains(&id) || id.starts_with("henad_") {
            checker.error(format!("the parameter id '{id}' is reserved"), place.clone());
        }
        if document.lane(id).is_some() || document.field(id).is_some() {
            checker.error(
                format!("the parameter '{id}' takes the id of a lane or a field"),
                place.clone(),
            );
        }
        check_label(checker, "a parameter label", param.label(), place.clone());
        let in_bounds = match param.kind() {
            ParamKind::F32 { min, max, default, .. } => min <= default && default <= max,
            ParamKind::U32 { min, max, default } => min <= default && default <= max,
            _ => true,
        };
        if !in_bounds {
            checker.error(
                format!("the parameter '{id}' has a `default` outside its bounds"),
                place,
            );
        }
    }
    check_tick_length(checker);
}

/// Checks that the parameter setting `tick_unit` is an `f32` with a `min` above 0, hidden from the Parameters panel.
fn check_tick_length(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let Some(time) = &document.time else {
        return;
    };
    let id = &time.tick_length;
    let Some(index) = document.params.iter().position(|param| param.id() == id) else {
        checker.error(
            format!("the time scale names the parameter '{id}', and the file declares no parameter '{id}'"),
            None,
        );
        return;
    };
    let param = &document.params[index];
    let place = checker.file.places().params.get(index).cloned();
    match param.kind() {
        ParamKind::F32 { min, .. } if *min <= 0.0 => checker.error(
            format!("the parameter '{id}' sets `tick_unit` and needs a `min` above 0"),
            place.clone(),
        ),
        ParamKind::F32 { .. } => {}
        _ => checker.error(
            format!("the parameter '{id}' sets `tick_unit` and needs `kind = \"f32\"`"),
            place.clone(),
        ),
    }
    if !param.is_hidden() {
        checker.error(
            format!("the parameter '{id}' sets `tick_unit` and needs `hidden = true`"),
            place,
        );
    }
}

fn check_stats(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().stats;
    check_count(
        checker,
        "stats",
        document.stats.len(),
        MAX_STATS,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, stat) in document.stats.iter().enumerate() {
        let place = places.get(index).cloned();
        check_label(checker, "a stat label", &stat.label, place.clone());
        if !seen.insert(stat.label.as_str()) {
            checker.error(format!("the file declares the stat '{}' twice", stat.label), place);
        }
    }
}

fn check_actions(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().actions;
    check_count(
        checker,
        "actions",
        document.actions.len(),
        MAX_ACTIONS,
        places.first().cloned(),
    );
    let mut seen = BTreeSet::new();
    for (index, action) in document.actions.iter().enumerate() {
        let place = places.get(index).cloned();
        check_id(checker, "action", &action.id, &mut seen, place.clone());
        check_label(checker, "an action label", &action.label, place.clone());
        let has_pass = document
            .passes
            .iter()
            .any(|pass| pass.action.as_deref() == Some(action.id.as_str()));
        if !has_pass && action.runs.is_empty() {
            checker.error(
                format!("the action '{}' has no pass and reruns none", action.id),
                place.clone(),
            );
        }
        for id in &action.runs {
            match document.passes.iter().find(|pass| pass.id == *id) {
                None => checker.error(
                    format!(
                        "the action '{}' reruns the pass '{id}', and the file declares no such pass",
                        action.id
                    ),
                    place.clone(),
                ),
                Some(pass) if !matches!(pass.stage, PassStage::Init | PassStage::Tick) => checker.error(
                    format!(
                        "the action '{}' reruns the {} pass '{id}', and an action reruns an init or a tick pass",
                        action.id,
                        pass.stage.as_str()
                    ),
                    place.clone(),
                ),
                Some(_) => {}
            }
        }
    }
}

fn check_functions(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().functions;
    check_count(
        checker,
        "functions",
        document.functions.len(),
        MAX_FUNCTIONS,
        places.first().map(|places| places.table.clone()),
    );
    let mut seen = BTreeSet::new();
    for (index, function) in document.functions.iter().enumerate() {
        let place = places.get(index).map(|places| places.table.clone());
        check_id(checker, "function", &function.id, &mut seen, place);
    }
}

fn check_pass_ids(checker: &mut Checker<'_>) {
    let document = checker.file.document();
    let places = &checker.file.places().passes;
    check_count(
        checker,
        "passes",
        document.passes.len(),
        MAX_PASSES,
        places.first().map(|places| places.table.clone()),
    );
    let mut seen = BTreeSet::new();
    for (index, pass) in document.passes.iter().enumerate() {
        let place = places.get(index).map(|places| places.table.clone());
        check_id(checker, "pass", &pass.id, &mut seen, place);
    }
}

/// Reports each node id more than one node of the file holds, and returns those ids in natural order (V2).
pub(super) fn check_node_ids(checker: &mut Checker<'_>) -> Vec<NodeId> {
    let document = checker.file.document();
    let places = checker.file.places();
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    let bodies = document.passes.iter().map(|pass| &pass.body).zip(&places.passes).chain(
        document
            .functions
            .iter()
            .map(|function| &function.body)
            .zip(&places.functions),
    );
    for (body, body_places) in bodies {
        for (position, node) in body.nodes.iter().enumerate() {
            if !seen.insert(node.id) {
                duplicates.insert(node.id);
                checker.add(
                    henad_core::diagnostic::CodeDiagnostic::error(format!(
                        "two nodes hold the id {}, as a merge of two branches can leave them, and the file needs its duplicates renumbered",
                        node.id
                    ))
                    .with_node(node.id.to_string()),
                    body_places.nodes.get(position).map(|places| places.id.clone()),
                );
            }
        }
    }
    duplicates.into_iter().collect()
}
