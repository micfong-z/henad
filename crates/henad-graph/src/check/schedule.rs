//! Rules over the order the top of a pass runs in: reducers end a map pass (`ir-spec.md` section 6.1), and the
//! `draw_order` of a pass agrees with its links (V17).
//!
//! The *top* of a pass is its nodes outside every region. A region stands for its body here, so a link from a body
//! node to a node at the top is a link of the region. A top node runs once before the elements when it reads only
//! values the same for every element, per element when it or a source runs per element, and once after the elements
//! when it reads a reducer's result. The host evaluator splits a pass the same way.

use std::collections::BTreeMap;

use super::Checker;
use super::view::BodyView;
use crate::catalogue::{Effect, NodeDef, OutputRate};
use crate::types::Rate;
use crate::value::InputValue;

/// When a node at the top of a pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Timing {
    /// Once, before the elements.
    Prelude,
    /// Per element.
    Element,
    /// Once, after the elements, from a reducer's result.
    Post,
}

/// A link that orders two nodes at the top of a pass.
struct TopLink<'a> {
    /// Position of the source, a node at the top.
    source: usize,
    /// Position of the node that holds the link, at the top or in the body of a region at the top.
    reader: usize,
    /// Input of the reader that holds the link.
    port: &'a str,
}

/// Checks the order the top of the pass of `view` runs in.
pub(super) fn check(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    if view.pass().is_none() {
        return;
    }
    let count = view.nodes.len();
    let tops: Vec<Option<usize>> = (0..count).map(|position| top_of(view, position)).collect();
    let mut incoming: Vec<Vec<TopLink<'_>>> = (0..count).map(|_| Vec::new()).collect();
    for (reader, node) in view.nodes.iter().enumerate() {
        let Some(top) = tops[reader] else { continue };
        for (port, value) in &node.inputs {
            let InputValue::Link(link) = value else { continue };
            let Some(&source) = view.positions.get(&link.node) else {
                continue;
            };
            if view.parents[source].is_none() && source != top {
                incoming[top].push(TopLink {
                    source,
                    reader,
                    port: port.as_str(),
                });
            }
        }
    }
    let order = top_order(view, &incoming);
    check_reducers(checker, view, &order, &incoming);
    check_draw_order(checker, view, &order, &incoming);
}

/// Returns the node at the top of the pass that holds the node at `position`: the node itself or its outermost region.
fn top_of(view: &BodyView<'_>, position: usize) -> Option<usize> {
    let mut current = position;
    // The region tree has no cycle once the view is built. The bound keeps a broken tree from looping.
    for _ in 0..=view.nodes.len() {
        match view.parents[current] {
            Some(parent) => current = parent,
            None => return Some(current),
        }
    }
    None
}

/// Returns the nodes at the top of the pass, each after its sources. A node on a cycle, which the body check reports,
/// is left out with every node after it.
fn top_order(view: &BodyView<'_>, incoming: &[Vec<TopLink<'_>>]) -> Vec<usize> {
    let count = view.nodes.len();
    let mut waiting: Vec<usize> = incoming.iter().map(Vec::len).collect();
    let mut readers: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (top, links) in incoming.iter().enumerate() {
        for link in links {
            readers[link.source].push(top);
        }
    }
    let mut ready: Vec<usize> = (0..count)
        .rev()
        .filter(|&position| view.parents[position].is_none() && waiting[position] == 0)
        .collect();
    let mut order = Vec::with_capacity(count);
    while let Some(position) = ready.pop() {
        order.push(position);
        for &reader in &readers[position] {
            waiting[reader] -= 1;
            if waiting[reader] == 0 {
                ready.push(reader);
            }
        }
    }
    order
}

/// Returns whether a node of `def` runs per element whatever it reads.
fn runs_per_element(def: &NodeDef) -> bool {
    def.is_region
        || !def.effects.is_empty()
        || def
            .outputs
            .iter()
            .any(|output| matches!(output.rate, OutputRate::Pinned(rate) if rate >= Rate::Element))
}

/// Reports a node that runs per element and reads a reducer's result of its own pass, directly or through values
/// computed from it.
fn check_reducers(checker: &mut Checker<'_>, view: &BodyView<'_>, order: &[usize], incoming: &[Vec<TopLink<'_>>]) {
    let mut timings = vec![Timing::Prelude; view.nodes.len()];
    let reduces = |position: usize| view.defs[position].is_some_and(|def| def.has_effect(Effect::Reduce));
    for &position in order {
        // A kind this build does not define has a diagnostic of its own.
        let Some(def) = view.defs[position] else { continue };
        let reduced = incoming[position]
            .iter()
            .find(|link| reduces(link.source) || timings[link.source] == Timing::Post);
        // A reducer runs per element, and its result is read after the elements.
        let per_element = runs_per_element(def)
            || incoming[position]
                .iter()
                .any(|link| !reduces(link.source) && timings[link.source] == Timing::Element);
        timings[position] = match (reduced, per_element) {
            (Some(link), true) => {
                view.error(
                    checker,
                    link.reader,
                    format!(
                        "{}: the input `{}` reads the result of a reducer of this pass, which is ready only \
                         after every element, and the node runs per element",
                        view.name(link.reader),
                        link.port
                    ),
                    view.input_place(link.reader, link.port),
                );
                Timing::Element
            }
            (Some(_), false) => Timing::Post,
            (None, true) => Timing::Element,
            (None, false) => Timing::Prelude,
        };
    }
}

/// Reports a stream draw that reads the draw of a node its pass's `draw_order` lists after it.
fn check_draw_order(checker: &mut Checker<'_>, view: &BodyView<'_>, order: &[usize], incoming: &[Vec<TopLink<'_>>]) {
    let Some(pass) = view.pass() else { return };
    if pass.draw_order.is_empty() {
        return;
    }
    let mut listed: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, id) in pass.draw_order.iter().enumerate() {
        if let Some(&position) = view.positions.get(id)
            && view.defs[position].is_some_and(|def| def.has_effect(Effect::DrawsStream))
        {
            listed.entry(position).or_insert(index);
        }
    }
    // The latest entry of `draw_order` among each node's sources, at any distance.
    let mut latest: Vec<Option<usize>> = vec![None; view.nodes.len()];
    for &position in order {
        let found = incoming[position]
            .iter()
            .filter_map(|link| latest[link.source].max(listed.get(&link.source).copied()))
            .max();
        latest[position] = found;
        if let (Some(&index), Some(later)) = (listed.get(&position), found)
            && later > index
        {
            view.error(
                checker,
                position,
                format!(
                    "{} reads the draw of {}, which the `draw_order` of the pass '{}' lists after it",
                    view.name(position),
                    pass.draw_order[later],
                    pass.id
                ),
                view.table_place(position),
            );
        }
    }
}
