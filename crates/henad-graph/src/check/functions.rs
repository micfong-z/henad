//! Functions, calls and expressions: the rules of a function body, and the problems that keep a pass from being
//! normalised.

use std::collections::{BTreeMap, BTreeSet};

use super::Checker;
use super::view::{BodyView, BodyViews, Owner};
use crate::catalogue::Effect;
use crate::node::{OP_EXPRESSION, OP_FUNCTION_CALL, OP_FUNCTION_INPUT, OP_FUNCTION_OUTPUT};
use crate::value::ConfigValue;

/// Effects no function body holds, with the words a message names them by.
const SINKS: [(Effect, &str); 7] = [
    (Effect::WritesLane, "a lane write"),
    (Effect::WritesField, "a field write"),
    (Effect::Deposits, "a deposit"),
    (Effect::Counts, "a counter"),
    (Effect::View, "a colour"),
    (Effect::Reduce, "a reducer"),
    (Effect::DrawsStream, "a stream draw"),
];

/// Reports the problems of the pass body `view` that functions and expressions cause: an expression that does not
/// expand, and a function's input or output outside a function body.
pub(super) fn check_pass(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    check_expressions(checker, view);
    for (position, node) in view.nodes.iter().enumerate() {
        if node.op == OP_FUNCTION_INPUT || node.op == OP_FUNCTION_OUTPUT {
            view.error(
                checker,
                position,
                format!("{} belongs in a function body", view.name(position)),
                view.op_place(position),
            );
        }
    }
}

/// Checks the function body `view`, its types included, and returns whether it holds no error, with the inferred
/// outputs of its nodes.
pub(super) fn check_function(checker: &mut Checker<'_>, view: &BodyView<'_>) -> (bool, super::body::BodyPorts) {
    let before = errors(checker);
    let ports = super::body::check(checker, view);
    check_expressions(checker, view);
    check_body(checker, view);
    (errors(checker) == before, ports)
}

/// Returns the ids of the functions a pass can inline: those of `clean` holding no error whose calls all name
/// functions a pass can inline. A call cycle never settles, and its functions stay out.
pub(super) fn inlinable(views: &BodyViews<'_>, clean: &BTreeMap<String, bool>) -> BTreeSet<String> {
    let mut calls: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for view in &views.functions {
        let Owner::Function(function) = view.owner else {
            continue;
        };
        calls.insert(
            &function.id,
            view.nodes
                .iter()
                .filter(|node| node.op == OP_FUNCTION_CALL)
                .filter_map(|node| node.config.get("function").and_then(ConfigValue::as_str))
                .collect(),
        );
    }
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (id, is_clean) in clean {
            if !*is_clean || found.contains(id) {
                continue;
            }
            let ready = calls
                .get(id.as_str())
                .is_some_and(|callees| callees.iter().all(|callee| found.contains(*callee)));
            if ready {
                found.insert(id.clone());
                changed = true;
            }
        }
    }
    found
}

/// Returns the number of errors found so far.
fn errors(checker: &Checker<'_>) -> usize {
    checker
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == henad_core::diagnostic::DiagnosticSeverity::Error)
        .count()
}

/// Checks the rules only a function body holds: no sink, and one `fn.output` per output.
fn check_body(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let Owner::Function(function) = view.owner else { return };
    let mut outputs: BTreeMap<&str, usize> = BTreeMap::new();
    for (position, node) in view.nodes.iter().enumerate() {
        if let Some(def) = view.defs[position]
            && let Some((_, what)) = SINKS.iter().find(|(effect, _)| def.has_effect(*effect))
        {
            view.error(
                checker,
                position,
                format!(
                    "{} is {what}, and the function '{}' is pure, with no effect but its outputs",
                    view.name(position),
                    function.id
                ),
                view.op_place(position),
            );
        }
        if node.op == OP_FUNCTION_OUTPUT
            && let Some(port) = node.config.get("port").and_then(ConfigValue::as_str)
            && let Some(first) = outputs.insert(port, position)
        {
            {
                view.error(
                    checker,
                    position,
                    format!(
                        "{} sets the output '{port}', which {} sets already",
                        view.name(position),
                        view.nodes[first].id
                    ),
                    view.config_place(position, "port"),
                );
            }
        }
    }
    for port in &function.outputs {
        if !outputs.contains_key(port.port.as_str()) {
            checker.error(
                format!(
                    "the function '{}' never sets its output '{}', which needs one `fn.output`",
                    function.id, port.port
                ),
                view.places.table.clone().into(),
            );
        }
    }
}

/// Reports each expression node of the body whose text does not expand.
fn check_expressions(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    for (position, node) in view.nodes.iter().enumerate() {
        if node.op != OP_EXPRESSION || view.defs[position].is_none() {
            continue;
        }
        let mut next = 1;
        if let Err(reason) = crate::expr::expand(node, &mut next) {
            view.error(
                checker,
                position,
                format!("{}: the expression {reason}", view.name(position)),
                view.config_place(position, "text"),
            );
        }
    }
}
