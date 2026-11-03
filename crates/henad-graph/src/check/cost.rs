//! Cost of a pass per element, rule V22, and the limits of the call graph.
//!
//! Costs are computed bottom-up with saturating arithmetic, over the call graph and then over each body's region tree,
//! before any function is inlined. A `Repeat` costs `1 + max * (body cost + 1)`. Nothing here recurses, so a deep file
//! cannot run out of stack.

use std::collections::BTreeMap;

use super::Checker;
use super::view::{BodyView, BodyViews};
use crate::catalogue::NodeCost;
use crate::limits::{MAX_CALL_DEPTH, MAX_NODES_INLINED, MAX_PASS_COST};
use crate::node::{OP_EXPRESSION, OP_FUNCTION_CALL};
use crate::value::ConfigValue;

/// Cost and inlined size of a function.
#[derive(Debug, Clone, Copy)]
struct FunctionSize {
    cost: u64,
    nodes: u64,
}

impl FunctionSize {
    /// Size of a function on a call cycle, which has its own diagnostic.
    const SATURATED: Self = Self {
        cost: u64::MAX,
        nodes: u64::MAX,
    };
}

/// Costs of a file's passes, and whether its inlined size is within [`MAX_NODES_INLINED`].
pub(super) struct Costs {
    /// Cost of each pass per element, in stack order.
    pub passes: Vec<u64>,
    /// Whether the file holds at most [`MAX_NODES_INLINED`] nodes once its calls and expressions are inlined.
    pub inlinable: bool,
}

/// Checks the costs of the file's passes, the shape of its call graph and its size once its calls and expressions are
/// inlined.
pub(super) fn check(checker: &mut Checker<'_>, views: &BodyViews<'_>) -> Costs {
    let sizes = function_sizes(checker, views);
    let mut costs = Vec::with_capacity(views.passes.len());
    let mut inlined = 0u64;
    for (index, view) in views.passes.iter().enumerate() {
        let body = body_size(view, &sizes);
        costs.push(body.cost);
        inlined = inlined.saturating_add(body.nodes);
        if body.cost > u64::from(MAX_PASS_COST) {
            report_pass_cost(checker, view, index, body.cost, &sizes);
        }
    }
    let inlinable = inlined <= MAX_NODES_INLINED as u64;
    if !inlinable {
        checker.error(
            format!(
                "the file holds {inlined} nodes after its function calls and expressions are inlined, and a file holds \
                 at most {MAX_NODES_INLINED}"
            ),
            Some(checker.file.places().id.clone()),
        );
    }
    Costs {
        passes: costs,
        inlinable,
    }
}

/// Reports a pass over the limit, and names its heaviest node.
fn report_pass_cost(
    checker: &mut Checker<'_>,
    view: &BodyView<'_>,
    index: usize,
    cost: u64,
    sizes: &BTreeMap<&str, FunctionSize>,
) {
    let Some(pass) = view.pass() else { return };
    let node_costs = node_costs(view, sizes);
    let heaviest = (0..view.nodes.len())
        .filter(|&position| view.parents[position].is_none())
        .max_by_key(|&position| node_costs[position]);
    let mut diagnostic = henad_core::diagnostic::CodeDiagnostic::error(format!(
        "the pass '{}' costs {cost} units per element, and a pass costs at most {MAX_PASS_COST}",
        pass.id
    ))
    .with_pass(pass.id.clone());
    if let Some(heaviest) = heaviest {
        diagnostic = diagnostic
            .with_node(view.nodes[heaviest].id.to_string())
            .with_note(format!("{} costs {}", view.name(heaviest), node_costs[heaviest]));
    }
    let place = checker
        .file
        .places()
        .passes
        .get(index)
        .map(|places| places.table.clone());
    checker.add(diagnostic, place);
}

/// Returns the size of every function, reporting a call cycle and a call chain past the depth limit.
fn function_sizes<'a>(checker: &mut Checker<'_>, views: &BodyViews<'a>) -> BTreeMap<&'a str, FunctionSize> {
    let functions = &checker.file.document().functions;
    let mut positions = BTreeMap::new();
    for (position, function) in functions.iter().enumerate() {
        positions.entry(function.id.as_str()).or_insert(position);
    }
    let count = functions.len().min(views.functions.len());
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut waiting = vec![0usize; count];
    for (position, view) in views.functions.iter().enumerate().take(count) {
        for node in view.nodes.iter().filter(|node| node.op == OP_FUNCTION_CALL) {
            if let Some(&callee) = node
                .config
                .get("function")
                .and_then(ConfigValue::as_str)
                .and_then(|id| positions.get(id))
            {
                callers[callee].push(position);
                waiting[position] += 1;
            }
        }
    }
    let mut sizes: BTreeMap<&'a str, FunctionSize> = BTreeMap::new();
    let mut depths = vec![1usize; count];
    let mut ready: Vec<usize> = (0..count).rev().filter(|&position| waiting[position] == 0).collect();
    let mut done = vec![false; count];
    while let Some(position) = ready.pop() {
        done[position] = true;
        let view = &views.functions[position];
        let id = view_function_id(view);
        sizes.insert(id, body_size(view, &sizes));
        if depths[position] == MAX_CALL_DEPTH + 1 {
            checker.error(
                format!("calls through the function '{id}' go deeper than {MAX_CALL_DEPTH} functions"),
                checker
                    .file
                    .places()
                    .functions
                    .get(position)
                    .map(|places| places.table.clone()),
            );
        }
        for &caller in &callers[position] {
            depths[caller] = depths[caller].max(depths[position].saturating_add(1));
            waiting[caller] -= 1;
            if waiting[caller] == 0 {
                ready.push(caller);
            }
        }
    }
    for (position, done) in done.iter().enumerate() {
        if !done {
            let id = view_function_id(&views.functions[position]);
            sizes.insert(id, FunctionSize::SATURATED);
            checker.error(
                format!("the function '{id}' calls itself, directly or through other functions"),
                checker
                    .file
                    .places()
                    .functions
                    .get(position)
                    .map(|places| places.table.clone()),
            );
        }
    }
    sizes
}

fn view_function_id<'a>(view: &BodyView<'a>) -> &'a str {
    match view.owner {
        super::view::Owner::Function(function) => &function.id,
        super::view::Owner::Pass(pass) => &pass.id,
    }
}

/// Returns the cost of each node of a body, a region's body included.
fn node_costs(view: &BodyView<'_>, sizes: &BTreeMap<&str, FunctionSize>) -> Vec<u64> {
    let count = view.nodes.len();
    let mut by_depth: Vec<usize> = (0..count).collect();
    by_depth.sort_unstable_by_key(|&position| std::cmp::Reverse(view.depths[position]));
    let mut body_costs = vec![0u64; count];
    let mut costs = vec![0u64; count];
    for position in by_depth {
        let cost = node_cost(view, position, body_costs[position], sizes);
        costs[position] = cost;
        if let Some(parent) = view.parents[position] {
            body_costs[parent] = body_costs[parent].saturating_add(cost);
        }
    }
    costs
}

/// Returns the cost of the node at `position`, whose region body costs `body`.
fn node_cost(view: &BodyView<'_>, position: usize, body: u64, sizes: &BTreeMap<&str, FunctionSize>) -> u64 {
    let node = &view.nodes[position];
    if node.op == OP_FUNCTION_CALL {
        let callee = node
            .config
            .get("function")
            .and_then(ConfigValue::as_str)
            .and_then(|id| sizes.get(id))
            .map_or(0, |size| size.cost);
        return callee.saturating_add(1);
    }
    let Some(def) = view.defs[position] else { return 1 };
    match def.cost {
        NodeCost::Units(units) => u64::from(units),
        NodeCost::PerRule(units) => {
            let rules = node
                .config
                .get("rules")
                .and_then(ConfigValue::as_array)
                .map_or(0, <[_]>::len);
            (rules as u64).saturating_mul(u64::from(units)).saturating_add(1)
        }
        NodeCost::Repeat => {
            // A broken `max` has its own diagnostic, and costs the largest loop a file can write.
            let max = node
                .config
                .get("max")
                .and_then(ConfigValue::as_integer)
                .and_then(|max| u64::try_from(max).ok())
                .unwrap_or(u64::from(crate::limits::MAX_REPEAT_ITERATIONS));
            max.saturating_mul(body.saturating_add(1)).saturating_add(1)
        }
        NodeCost::Stencil => {
            let positions = crate::catalogue::stencil_offsets(node).len() as u64;
            positions.saturating_mul(body.saturating_add(1)).saturating_add(1)
        }
        NodeCost::RadiusFold => body.saturating_add(8),
        NodeCost::Expression => crate::expr::expanded_nodes(node)
            .iter()
            .filter_map(|expanded| crate::catalogue::lookup(&expanded.op, crate::LIBRARY))
            .map(|def| match def.cost {
                NodeCost::Units(units) => u64::from(units),
                _ => 1,
            })
            .fold(0u64, u64::saturating_add),
    }
}

/// Returns the cost and the inlined size of a body.
fn body_size(view: &BodyView<'_>, sizes: &BTreeMap<&str, FunctionSize>) -> FunctionSize {
    let costs = node_costs(view, sizes);
    let mut size = FunctionSize { cost: 0, nodes: 0 };
    for (position, node) in view.nodes.iter().enumerate() {
        if view.parents[position].is_none() {
            size.cost = size.cost.saturating_add(costs[position]);
        }
        // An expression stands for the nodes it expands into, or for itself when it does not expand.
        let own = if node.op == OP_EXPRESSION {
            crate::expr::expanded_nodes(node).len().max(1) as u64
        } else {
            1
        };
        size.nodes = size.nodes.saturating_add(own);
        if node.op == OP_FUNCTION_CALL {
            let callee = node
                .config
                .get("function")
                .and_then(ConfigValue::as_str)
                .and_then(|id| sizes.get(id))
                .map_or(0, |callee| callee.nodes);
            size.nodes = size.nodes.saturating_add(callee);
        }
    }
    size
}
