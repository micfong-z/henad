//! The count bound warning of a Repeat, rule V21.
//!
//! The inferred bound of a `count` follows parameter bounds, `math.min`, `math.max`, `math.clamp`, `math.add`, literals
//! and an outer `k` or `total`. A value it does not follow has no bound and no warning.

use std::collections::BTreeMap;

use henad_core::params::ParamKind;

use super::Checker;
use super::view::BodyView;
use crate::catalogue::NodeCost;
use crate::node::OP_REGION_REPEAT;
use crate::value::{InputValue, Literal};

/// Depth of links the bound follows. A deeper chain has no bound.
const MAX_BOUND_DEPTH: usize = 64;

/// Warns of each Repeat whose `count` can pass its `max`.
pub(super) fn warn_counts(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let mut memo = BTreeMap::new();
    for (position, node) in view.nodes.iter().enumerate() {
        if view.defs[position].is_none_or(|def| def.cost != NodeCost::Repeat) {
            continue;
        }
        let Some(max) = node.config_integer("max") else {
            continue;
        };
        let Some(count) = node.inputs.get("count") else {
            continue;
        };
        if let Some(bound) = value_bound(view, count, &mut memo, 0)
            && u64::try_from(max).is_ok_and(|max| bound > max)
        {
            view.warning(
                checker,
                position,
                format!(
                    "{}: `count` can reach {bound}, and `max` is {max}, so the loop stops at {max}",
                    view.name(position)
                ),
                view.input_place(position, "count"),
            );
        }
    }
}

/// Returns the largest value `value` can hold, or `None` when the walk cannot say.
fn value_bound(
    view: &BodyView<'_>,
    value: &InputValue,
    memo: &mut BTreeMap<(usize, Option<String>), Option<u64>>,
    depth: usize,
) -> Option<u64> {
    if depth > MAX_BOUND_DEPTH {
        return None;
    }
    match value {
        InputValue::Literal(Literal::U32(number)) => Some(u64::from(*number)),
        InputValue::Param(id) => {
            let param = view.document.param(id)?;
            match param.kind() {
                ParamKind::U32 { max, .. } => Some(u64::from(*max)),
                ParamKind::Choice { options, .. } => Some(options.len().saturating_sub(1) as u64),
                _ => None,
            }
        }
        InputValue::Link(link) => {
            let source = *view.positions.get(&link.node)?;
            let key = (source, link.port.clone());
            if let Some(bound) = memo.get(&key) {
                return *bound;
            }
            // A cycle reads the entry as no bound, and has its own diagnostic.
            memo.insert(key.clone(), None);
            let bound = node_bound(view, source, link.port.as_deref(), memo, depth + 1);
            memo.insert(key, bound);
            bound
        }
        _ => None,
    }
}

/// Returns the largest value the output `port` of the node at `position` can hold.
fn node_bound(
    view: &BodyView<'_>,
    position: usize,
    port: Option<&str>,
    memo: &mut BTreeMap<(usize, Option<String>), Option<u64>>,
    depth: usize,
) -> Option<u64> {
    let node = &view.nodes[position];
    let mut input = |name: &str| value_bound(view, node.inputs.get(name)?, memo, depth);
    match (node.op.as_str(), port) {
        ("math.add", _) => Some(input("a")?.saturating_add(input("b")?)),
        ("math.max", _) => Some(input("a")?.max(input("b")?)),
        ("math.min", _) => match (input("a"), input("b")) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(known), None) | (None, Some(known)) => Some(known),
            (None, None) => None,
        },
        ("math.clamp", _) => input("hi"),
        (OP_REGION_REPEAT, Some("k")) => repeat_max(node).map(|max| max.saturating_sub(1)),
        (OP_REGION_REPEAT, Some("total")) => repeat_max(node),
        _ => None,
    }
}

/// Returns the bound `config.max` of a Repeat.
fn repeat_max(node: &crate::node::Node) -> Option<u64> {
    u64::try_from(node.config_integer("max")?).ok()
}
