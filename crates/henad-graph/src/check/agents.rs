//! Rules of fields, counters, other elements and folds: V12, V14, V25, the place of `lane.get_at`, the nesting of
//! regions, and V18 after normalisation.

use std::collections::BTreeMap;

use super::Checker;
use super::view::BodyView;
use crate::catalogue::Effect;
use crate::document::{Domain, LaneRole, PassStage};
use crate::ids::NodeId;
use crate::value::{ConfigValue, InputValue, Literal};

/// Kind of the radius fold.
const RADIUS_FOLD: &str = "agent.radius_fold";
/// Kind of the stencil fold.
const GRID_FOLD: &str = "grid.fold";
/// Kind of the finite loop.
const REPEAT: &str = "region.repeat";
/// Deepest structure the source of a `cell_size` is compared to.
const MAX_SOURCE_DEPTH: usize = 32;

/// Checks the rules of one pass that read its fields, other elements and folds.
pub(super) fn check_pass(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let Some(pass) = view.pass() else { return };
    let document = view.document;
    let mut set_layers: BTreeMap<(&str, Option<String>), usize> = BTreeMap::new();
    for (position, node) in view.nodes.iter().enumerate() {
        let Some(def) = view.defs[position] else { continue };
        let field = node
            .config
            .get("field")
            .and_then(ConfigValue::as_str)
            .and_then(|id| document.fields.iter().find(|field| field.id == id));
        if def.has_effect(Effect::Deposits)
            && let Some(field) = field
            && field.is_static
        {
            view.error(
                checker,
                position,
                format!(
                    "{} deposits into the static field '{}', which takes no deposits",
                    view.name(position),
                    field.id
                ),
                view.config_place(position, "field"),
            );
        }
        if def.has_effect(Effect::WritesField)
            && let Some(field) = field
        {
            if field.is_static && pass.stage != PassStage::Init {
                view.error(
                    checker,
                    position,
                    format!(
                        "{} writes the static field '{}', which only an init pass writes",
                        view.name(position),
                        field.id
                    ),
                    view.op_place(position),
                );
            }
            // A layer the file names as a literal is one layer, and any other value can be every layer.
            let layer = match node.inputs.get("layer") {
                None => field.layers.first().cloned(),
                Some(InputValue::Literal(Literal::Layer(name))) => Some(name.member.clone()),
                Some(_) => None,
            };
            let clash = set_layers
                .iter()
                .find(|((known, known_layer), _)| {
                    *known == field.id.as_str() && (layer.is_none() || known_layer.is_none() || *known_layer == layer)
                })
                .map(|(_, &first)| first);
            if let Some(first) = clash {
                view.error(
                    checker,
                    position,
                    format!(
                        "{} writes a layer of the field '{}' that {} may write already, and a pass writes a layer once per cell",
                        view.name(position),
                        field.id,
                        view.nodes[first].id
                    ),
                    view.input_place(position, "layer"),
                );
            } else {
                set_layers.insert((field.id.as_str(), layer), position);
            }
        }
        if node.op == "lane.get_at" {
            check_get_at(checker, view, position);
        }
        if def.is_region {
            check_nesting(checker, view, position);
        }
    }
}

/// Reports a `lane.get_at` whose `at` is no inner output of a region around it: another element is reached only
/// through a region.
fn check_get_at(checker: &mut Checker<'_>, view: &BodyView<'_>, position: usize) {
    let Some(InputValue::Link(link)) = view.nodes[position].inputs.get("at") else {
        return;
    };
    let Some(&source) = view.positions.get(&link.node) else {
        return;
    };
    let inner = link
        .port
        .as_deref()
        .is_some_and(|port| port == "other" || port == "cell");
    if !(inner && view.is_inside(position, source)) {
        view.error(
            checker,
            position,
            format!(
                "{} reads another element at {link}, and reaches one only through `cell` or `other` of a fold around it",
                view.name(position)
            ),
            view.input_place(position, "at"),
        );
    }
}

/// Reports a region nested where library 1 allows no nesting: a Repeat holds a Repeat or a stencil fold, a stencil
/// fold holds a Repeat, and a radius fold holds no region and sits in none.
fn check_nesting(checker: &mut Checker<'_>, view: &BodyView<'_>, position: usize) {
    let Some(parent) = view.parents[position] else { return };
    let (inner, outer) = (view.nodes[position].op.as_str(), view.nodes[parent].op.as_str());
    let allowed = matches!((outer, inner), (REPEAT, REPEAT | GRID_FOLD) | (GRID_FOLD, REPEAT));
    // A region of an unknown kind or a broken region tree has its own diagnostic.
    if allowed || view.defs[parent].is_none_or(|def| !def.is_region) {
        return;
    }
    let reason = if inner == RADIUS_FOLD || outer == RADIUS_FOLD {
        "a radius fold visits as many agents as it meets, and holds no region and sits in none"
    } else {
        "a Repeat holds a Repeat or a neighborhood fold, and a neighborhood fold holds a Repeat"
    };
    view.error(
        checker,
        position,
        format!(
            "{} sits in the region {}, and {reason}",
            view.name(position),
            view.nodes[parent].id
        ),
        view.region_place(position),
    );
}

/// Reports a `field.get` of a field in its own `finish` function, or in a function that function calls.
///
/// The update of a field runs the `finish` once per cell of each layer and writes the result in place, so a read of
/// the same field in that pass would race with the invocations that write it.
pub(super) fn check_finishes(checker: &mut Checker<'_>, functions: &[BodyView<'_>]) {
    let document = checker.file.document();
    for field in document.fields.iter().filter(|field| !field.is_static) {
        let Some(finish) = field.finish.as_deref() else {
            continue;
        };
        // The functions the update reaches, each visited once, so a cycle of calls ends.
        let mut reached: Vec<&str> = vec![finish];
        let mut next = 0;
        while let Some(&id) = reached.get(next) {
            next += 1;
            let Some(view) = functions
                .iter()
                .find(|view| matches!(view.owner, super::view::Owner::Function(function) if function.id == id))
            else {
                continue;
            };
            for (position, node) in view.nodes.iter().enumerate() {
                let config = |key: &str| node.config.get(key).and_then(ConfigValue::as_str);
                if node.op == crate::node::OP_FUNCTION_CALL
                    && let Some(callee) = config("function")
                    && !reached.contains(&callee)
                {
                    reached.push(callee);
                }
                if node.op == "field.get" && config("field") == Some(field.id.as_str()) {
                    view.error(
                        checker,
                        position,
                        format!(
                            "{} reads the field '{}' in the update that runs its `finish` '{finish}', which writes \
                             that field in place. The merged value arrives as the input `value`",
                            view.name(position),
                            field.id
                        ),
                        view.config_place(position, "field"),
                    );
                }
            }
        }
    }
}

/// Checks the rules across the passes of an agents model: V12, V14, and the uniqueness of the keys normalisation
/// composes (V18).
pub(super) fn check_passes(checker: &mut Checker<'_>, views: &[BodyView<'_>]) {
    let document = checker.file.document();
    let position_lane = document
        .lanes
        .iter()
        .find(|lane| lane.role == Some(LaneRole::Position))
        .map(|lane| lane.id.as_str());
    // V12: a radius fold reads the index built from the positions at the start of the tick.
    let mut written_in: Option<String> = None;
    let mut sources: Vec<(usize, usize, String)> = Vec::new();
    for (index, view) in views.iter().enumerate() {
        let Some(pass) = view.pass() else { continue };
        if pass.stage != PassStage::Tick {
            continue;
        }
        for (position, node) in view.nodes.iter().enumerate() {
            if node.op != RADIUS_FOLD {
                continue;
            }
            if let Some(writer) = &written_in {
                view.error(
                    checker,
                    position,
                    format!(
                        "{} reads positions after the pass '{writer}' writes them, and the index holds the positions at the start of the tick",
                        view.name(position)
                    ),
                    view.op_place(position),
                );
            }
            if let Some(source) = node.inputs.get("cell_size") {
                sources.push((index, position, source_key(view, source, 0)));
            }
        }
        let writes_position = view.nodes.iter().any(|node| {
            node.op == "lane.set" && node.config.get("lane").and_then(ConfigValue::as_str) == position_lane
        });
        if writes_position && written_in.is_none() {
            written_in = Some(pass.id.clone());
        }
    }
    // V14: the engine sizes one index for every radius fold.
    if let Some((_, _, first)) = sources.first().cloned() {
        for (index, position, key) in sources.iter().skip(1) {
            if *key != first {
                let view = &views[*index];
                view.error(
                    checker,
                    *position,
                    format!(
                        "{} takes its `cell_size` from another source than the first radius fold, and every radius fold shares one index",
                        view.name(*position)
                    ),
                    view.input_place(*position, "cell_size"),
                );
            }
        }
    }
    check_composed_keys(checker, views);
}

/// Returns a text that two inputs share when they give one value: a literal, a parameter, or a node of the same
/// kind, configuration and inputs, compared to a fixed depth.
fn source_key(view: &BodyView<'_>, value: &InputValue, depth: usize) -> String {
    match value {
        InputValue::Literal(literal) => format!("{literal:?}"),
        InputValue::Param(id) => format!("param {id}"),
        InputValue::Link(link) => {
            let Some(&position) = view.positions.get(&link.node) else {
                return format!("missing {link}");
            };
            if depth >= MAX_SOURCE_DEPTH {
                return format!("node {link}");
            }
            let node = &view.nodes[position];
            let inputs: Vec<String> = node
                .inputs
                .iter()
                .map(|(port, input)| format!("{port}={}", source_key(view, input, depth + 1)))
                .collect();
            format!("{}{:?}({}).{:?}", node.op, node.config, inputs.join(","), link.port)
        }
    }
}

/// Reports two draws that normalisation gave one key, where one of them sits inside a call. The written keys are
/// checked apart.
fn check_composed_keys(checker: &mut Checker<'_>, views: &[BodyView<'_>]) {
    let mut keys: BTreeMap<i64, (NodeId, bool)> = BTreeMap::new();
    for view in views {
        for (position, node) in view.nodes.iter().enumerate() {
            if view.defs[position].is_none_or(|def| !def.has_effect(Effect::DrawsCounter)) {
                continue;
            }
            let Some(key) = node.config.get("key").and_then(ConfigValue::as_integer) else {
                continue;
            };
            let copied = view.origin(position).is_some_and(|origin| !origin.calls.is_empty());
            match keys.get(&key) {
                Some((first, first_copied)) if copied || *first_copied => view.error(
                    checker,
                    position,
                    format!(
                        "{} draws with the key {key} inside a call, which another draw ({first}) has too, and two draws with one key correlate. Remint the key of a call",
                        view.name(position)
                    ),
                    view.table_place(position),
                ),
                Some(_) => {}
                None => {
                    keys.insert(key, (node.id, copied));
                }
            }
        }
    }
}

/// Returns whether `at` is required on the node in `view`: the own-cell default holds over cells, and a pass over
/// agents has no own cell.
pub(super) fn own_cell_missing(view: &BodyView<'_>) -> bool {
    view.pass().is_some_and(|pass| pass.domain == Some(Domain::Agents))
}
