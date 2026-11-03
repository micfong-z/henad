//! Rules over a pass and over the passes together: V11, V17, V18, V23, V24, V29, V30 and V31.

use std::collections::{BTreeMap, BTreeSet};

use henad_core::params::ParamKind;

use super::Checker;
use super::body::BodyPorts;
use super::view::{BodyView, BodyViews};
use crate::catalogue::Effect;
use crate::document::{Domain, PassEnabled, PassStage, Topology};
use crate::types::{Rate, ValueType};
use crate::value::ConfigValue;

/// Returns the lanes the nodes of `view` with `effect` name in `config.lane`, in node order.
fn lanes_with_effect<'a>(view: &'a BodyView<'_>, effect: Effect) -> impl Iterator<Item = (usize, &'a str)> {
    view.nodes.iter().enumerate().filter_map(move |(position, node)| {
        view.defs[position]
            .filter(|def| def.has_effect(effect))
            .and_then(|_| node.config.get("lane"))
            .and_then(ConfigValue::as_str)
            .map(|lane| (position, lane))
    })
}

/// Checks the table of pass `index`, and what its nodes hold together.
pub(super) fn check_pass(checker: &mut Checker<'_>, index: usize, view: &BodyView<'_>) {
    let Some(pass) = view.pass() else { return };
    let document = checker.file.document();
    let place = checker
        .file
        .places()
        .passes
        .get(index)
        .map(|places| places.table.clone());
    let topology = document.world.topology();
    match pass.domain {
        // Library 1 has no domain-free reducer, so every pass runs over a domain, stats passes included.
        None => checker.error(
            format!("the {} pass '{}' needs a `domain`", pass.stage.as_str(), pass.id),
            place.clone(),
        ),
        Some(Domain::Agents) if topology == Topology::Grid => checker.error(
            format!("the pass '{}' runs over agents, and a grid holds cells", pass.id),
            place.clone(),
        ),
        _ => {}
    }
    match (&pass.action, pass.stage) {
        (None, PassStage::Action) => checker.error(
            format!("the action pass '{}' needs an `action`", pass.id),
            place.clone(),
        ),
        (Some(action), PassStage::Action) => {
            if !document.actions.iter().any(|known| known.id == *action) {
                checker.error(
                    format!(
                        "the pass '{}' belongs to the action '{action}', and the file declares no such action",
                        pass.id
                    ),
                    place.clone(),
                );
            }
        }
        (Some(_), _) => checker.error(
            format!(
                "the {} pass '{}' holds an `action`, which only an action pass holds",
                pass.stage.as_str(),
                pass.id
            ),
            place.clone(),
        ),
        (None, _) => {}
    }
    if let PassEnabled::Param(id) = &pass.enabled {
        let param = document.params.iter().find(|param| param.id() == id);
        if !param.is_some_and(|param| matches!(param.kind(), ParamKind::Bool { .. })) {
            checker.error(
                format!(
                    "the pass '{}' binds `enabled` to '{id}', which is no `bool` parameter",
                    pass.id
                ),
                place.clone(),
            );
        }
    }
    check_draw_order(checker, view, place.as_ref());
    check_lane_writes(checker, view);
    check_lane_domains(checker, view);
    let colors: Vec<usize> = (0..view.nodes.len())
        .filter(|&position| view.defs[position].is_some_and(|def| def.has_effect(Effect::View)))
        .collect();
    if let Some(&second) = colors.get(1) {
        view.error(
            checker,
            second,
            format!(
                "{} sets a colour the pass sets already, and a pass sets it once",
                view.name(second)
            ),
            view.table_place(second),
        );
    }
}

/// Reports a broken `draw_order`: each stream node of the pass is listed exactly once, and nothing else is (V17).
fn check_draw_order(checker: &mut Checker<'_>, view: &BodyView<'_>, place: Option<&std::ops::Range<usize>>) {
    let Some(pass) = view.pass() else { return };
    let mut listed = BTreeSet::new();
    for id in &pass.draw_order {
        if !listed.insert(*id) {
            checker.error(
                format!("the `draw_order` of the pass '{}' lists {id} twice", pass.id),
                place.cloned(),
            );
        }
        let is_stream = view
            .positions
            .get(id)
            .is_some_and(|&position| view.defs[position].is_some_and(|def| def.has_effect(Effect::DrawsStream)));
        if !is_stream {
            checker.error(
                format!(
                    "the `draw_order` of the pass '{}' lists {id}, which is no stream node of the pass",
                    pass.id
                ),
                place.cloned(),
            );
        }
    }
    for position in 0..view.nodes.len() {
        if view.defs[position].is_some_and(|def| def.has_effect(Effect::DrawsStream))
            && !listed.contains(&view.nodes[position].id)
        {
            view.error(
                checker,
                position,
                format!(
                    "{} draws from the stream, and the `draw_order` of its pass lists no such node",
                    view.name(position)
                ),
                view.table_place(position),
            );
        }
    }
}

/// Reports a lane written twice in the pass (V11).
fn check_lane_writes(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let mut written: BTreeMap<&str, usize> = BTreeMap::new();
    for (position, lane) in lanes_with_effect(view, Effect::WritesLane) {
        if let Some(&first) = written.get(lane) {
            view.error(
                checker,
                position,
                format!(
                    "{} writes the lane '{lane}', which {} writes already, and a pass writes a lane once per element",
                    view.name(position),
                    view.nodes[first].id
                ),
                view.config_place(position, "lane"),
            );
        } else {
            written.insert(lane, position);
        }
    }
}

/// Reports a lane read or written in a pass over another domain (V8).
fn check_lane_domains(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let Some(pass) = view.pass() else { return };
    let Some(domain) = pass.domain else { return };
    for (position, node) in view.nodes.iter().enumerate() {
        let Some(id) = node.config.get("lane").and_then(ConfigValue::as_str) else {
            continue;
        };
        if let Some(lane) = view.document.lanes.iter().find(|lane| lane.id == id)
            && lane.domain != domain
        {
            view.error(
                checker,
                position,
                format!(
                    "{} uses the lane '{id}' of {}, in a pass over {}",
                    view.name(position),
                    lane.domain.as_str(),
                    domain.as_str()
                ),
                view.config_place(position, "lane"),
            );
        }
    }
}

/// Checks the rules across the tick passes, and returns the dual lanes in declaration order (V11, V29, V30, V31).
pub(super) fn check_lanes(checker: &mut Checker<'_>, views: &BodyViews<'_>) -> Vec<String> {
    let document = checker.file.document();
    let places = checker.file.places();
    let mut dual = BTreeSet::new();
    for view in views
        .passes
        .iter()
        .filter(|view| view.pass().is_some_and(|pass| pass.stage == PassStage::Tick))
    {
        let written: BTreeSet<&str> = lanes_with_effect(view, Effect::WritesLane)
            .map(|(_, lane)| lane)
            .collect();
        for (_, lane) in lanes_with_effect(view, Effect::ReadsOtherLane) {
            if written.contains(lane) {
                dual.insert(lane.to_owned());
            }
        }
    }
    let mut writers: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, view) in views.passes.iter().enumerate() {
        let Some(pass) = view.pass() else { continue };
        let writes: BTreeSet<&str> = lanes_with_effect(view, Effect::WritesLane)
            .map(|(_, lane)| lane)
            .collect();
        let place = places.passes.get(index).map(|places| places.table.clone());
        if pass.stage == PassStage::Tick {
            for lane in &writes {
                writers.entry(*lane).or_default().push(index);
            }
        }
        if let PassEnabled::Param(id) = &pass.enabled
            && let Some(lane) = writes.iter().find(|lane| dual.contains(**lane))
        {
            checker.error(
                format!(
                    "the pass '{}' writes the two-sided lane '{lane}', so `enabled` cannot bind the parameter '{id}'",
                    pass.id
                ),
                place.clone(),
            );
        }
        let rerun = document.actions.iter().any(|action| action.runs.contains(&pass.id));
        if rerun
            && pass.stage == PassStage::Init
            && let Some((position, _)) = view
                .defs
                .iter()
                .enumerate()
                .find(|(_, def)| def.is_some_and(|def| def.has_effect(Effect::ReadsLane)))
        {
            view.error(
                checker,
                position,
                format!(
                    "{} reads a lane in the init pass '{}', which an action reruns on the host, where lanes do not live",
                    view.name(position),
                    pass.id
                ),
                view.table_place(position),
            );
        }
    }
    for (lane, passes) in &writers {
        if dual.contains(*lane) && passes.len() > 1 {
            for &index in passes {
                checker.error(
                    format!("the two-sided lane '{lane}' has more than one writer pass in a tick"),
                    places.passes.get(index).map(|places| places.table.clone()),
                );
            }
        }
    }
    check_tick_passes(checker, views);
    document
        .lanes
        .iter()
        .filter(|lane| dual.contains(&lane.id))
        .map(|lane| lane.id.clone())
        .collect()
}

/// Reports a grid without exactly one tick pass, and a tick pass of agents over another domain (V29).
fn check_tick_passes(checker: &mut Checker<'_>, views: &BodyViews<'_>) {
    let document = checker.file.document();
    let places = checker.file.places();
    let tick_passes: Vec<usize> = views
        .passes
        .iter()
        .enumerate()
        .filter(|(_, view)| view.pass().is_some_and(|pass| pass.stage == PassStage::Tick))
        .map(|(index, _)| index)
        .collect();
    match document.world.topology() {
        Topology::Grid if tick_passes.len() != 1 => checker.error(
            format!(
                "a grid model has exactly one tick pass, and this file has {}",
                tick_passes.len()
            ),
            tick_passes
                .get(1)
                .and_then(|&index| places.passes.get(index))
                .map(|places| places.table.clone())
                .or_else(|| Some(places.graph.clone())),
        ),
        Topology::Agents => {
            for index in tick_passes {
                if views.passes[index]
                    .pass()
                    .is_some_and(|pass| pass.domain != Some(Domain::Agents))
                {
                    checker.error(
                        "a tick pass of an agents model runs over agents".to_owned(),
                        places.passes.get(index).map(|places| places.table.clone()),
                    );
                }
            }
        }
        _ => {}
    }
}

/// Reports two draws or calls of the file with one key (V18).
pub(super) fn check_keys(checker: &mut Checker<'_>, views: &BodyViews<'_>) {
    let mut keys: BTreeMap<i64, String> = BTreeMap::new();
    for view in views.passes.iter().chain(&views.functions) {
        for (position, node) in view.nodes.iter().enumerate() {
            let draws = view.defs[position].is_some_and(|def| def.has_effect(Effect::DrawsCounter));
            if !draws && node.op != crate::node::OP_FUNCTION_CALL {
                continue;
            }
            let Some(key) = node.config.get("key").and_then(ConfigValue::as_integer) else {
                continue;
            };
            if let Some(first) = keys.get(&key) {
                view.error(
                    checker,
                    position,
                    format!(
                        "{} has the key {key}, which {first} has already, and two draws with one key correlate",
                        view.name(position)
                    ),
                    view.config_place(position, "key"),
                );
            } else {
                keys.insert(key, node.id.to_string());
            }
        }
    }
}

/// Checks each `[[stat]]` table against the stats passes (V23).
pub(super) fn check_stats(checker: &mut Checker<'_>, views: &BodyViews<'_>, ports: &[BodyPorts]) {
    let document = checker.file.document();
    for (index, stat) in document.stats.iter().enumerate() {
        let place = checker.file.places().stats.get(index).cloned();
        let found = views.passes.iter().zip(ports).find(|(view, _)| {
            view.pass().is_some_and(|pass| pass.stage == PassStage::Stats)
                && view.positions.contains_key(&stat.from.node)
        });
        let Some((_, ports)) = found else {
            checker.error(
                format!(
                    "the stat '{}' reads {}, and no stats pass holds that node",
                    stat.label, stat.from
                ),
                place,
            );
            continue;
        };
        // A body the checker did not walk has its own diagnostic.
        let Some(outputs) = ports.outputs.get(&stat.from.node) else {
            continue;
        };
        let port = match &stat.from.port {
            Some(name) => outputs.get(name),
            None if outputs.len() == 1 => outputs.values().next(),
            None => None,
        };
        let Some(port) = port else {
            checker.error(
                format!(
                    "the stat '{}' reads {}, which names no output of the node",
                    stat.label, stat.from
                ),
                place,
            );
            continue;
        };
        if port.rate > Rate::Tick {
            checker.error(
                format!(
                    "the stat '{}' reads {}, a value per element, and a stat reads a reducer's result",
                    stat.label, stat.from
                ),
                place.clone(),
            );
        }
        if let Some(value_type) = &port.value_type
            && !matches!(
                value_type,
                ValueType::U32 | ValueType::U64 | ValueType::F32 | ValueType::F64 | ValueType::Vec2D
            )
        {
            checker.error(
                format!(
                    "the stat '{}' reads {}, of type {value_type}, which no stat holds",
                    stat.label, stat.from
                ),
                place,
            );
        }
    }
}
