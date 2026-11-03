//! Checker of a graph file: the validation rules a grid can meet, each failure reported, all at once.
//!
//! The checker reads a [`GraphFile`] and returns a [`CheckReport`]. It does not stop at the first problem, and a node
//! that fails to type yields a hole, a port of unknown type that fits every socket, so one bad wire gives one
//! diagnostic. A *body* is the nodes of a pass or a function. An error blocks a build, and a warning does not.

mod body;
mod bounds;
mod config;
mod cost;
mod declarations;
mod passes;
mod view;

use std::collections::BTreeMap;
use std::ops::Range;

use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};

use crate::file::GraphFile;
use crate::ids::NodeId;
use crate::limits::{MAX_NODES_IN_FILE, MAX_NODES_PER_BODY};
use crate::types::{Rate, ValueType};

/// Result of checking a graph file.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CheckReport {
    /// Every problem found, errors and warnings, in file order.
    pub diagnostics: Vec<CodeDiagnostic>,
    /// Node ids that more than one node of the file holds. The file opens in a repair state that runs nothing.
    pub duplicate_ids: Vec<NodeId>,
    /// One report per pass, in stack order.
    pub passes: Vec<PassReport>,
    /// Ids of the lanes with two sides: read at another element in a tick pass that also writes them.
    pub dual_lanes: Vec<String>,
}

impl CheckReport {
    /// Returns whether the file has an error.
    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }

    /// Returns the errors, in file order.
    pub fn errors(&self) -> impl Iterator<Item = &CodeDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    /// Returns the warnings, in file order.
    pub fn warnings(&self) -> impl Iterator<Item = &CodeDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
    }
}

/// What the checker learned about one pass.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PassReport {
    /// Id of the pass.
    pub id: String,
    /// Cost per element in units, saturated at [`u64::MAX`].
    pub cost: u64,
    /// Inferred outputs of each node, by node id and port name. A region's inner outputs are in
    /// [`Self::inner_outputs`].
    pub outputs: BTreeMap<NodeId, BTreeMap<String, PortType>>,
    /// Inferred inner outputs of each region, by node id and port name.
    pub inner_outputs: BTreeMap<NodeId, BTreeMap<String, PortType>>,
}

/// Inferred type and rate of one port.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PortType {
    /// Type of the value, `None` for a hole.
    pub value_type: Option<ValueType>,
    /// Rate of the value.
    pub rate: Rate,
}

/// Returns the report of `file`.
pub fn check(file: &GraphFile) -> CheckReport {
    let mut checker = Checker {
        file,
        placer: file.placer(),
        diagnostics: Vec::new(),
    };
    let mut report = CheckReport::default();
    declarations::check(&mut checker);
    report.duplicate_ids = declarations::check_node_ids(&mut checker);

    let document = file.document();
    let total = document
        .passes
        .iter()
        .map(|pass| pass.body.nodes.len())
        .chain(document.functions.iter().map(|function| function.body.nodes.len()))
        .fold(0usize, usize::saturating_add);
    if total > MAX_NODES_IN_FILE {
        checker.error(
            format!("the file holds {total} nodes, and a file holds at most {MAX_NODES_IN_FILE}"),
            Some(file.places().id.clone()),
        );
    }
    let views = view::BodyViews::new(&mut checker);
    let places = file.places();
    let bodies = views
        .passes
        .iter()
        .zip(&places.passes)
        .chain(views.functions.iter().zip(&places.functions));
    for (body, body_places) in bodies {
        if body.nodes.len() > MAX_NODES_PER_BODY {
            checker.error(
                format!(
                    "the table holds {} nodes, and a pass or a function holds at most {MAX_NODES_PER_BODY}",
                    body.nodes.len()
                ),
                Some(body_places.table.clone()),
            );
        }
    }
    let mut pass_ports = Vec::with_capacity(document.passes.len());
    for (index, pass_view) in views.passes.iter().enumerate() {
        let ports = if total > MAX_NODES_IN_FILE || pass_view.nodes.len() > MAX_NODES_PER_BODY {
            body::BodyPorts::default()
        } else {
            body::check(&mut checker, pass_view)
        };
        passes::check_pass(&mut checker, index, pass_view);
        pass_ports.push(ports);
    }
    for function_view in &views.functions {
        if total <= MAX_NODES_IN_FILE && function_view.nodes.len() <= MAX_NODES_PER_BODY {
            body::check(&mut checker, function_view);
        }
    }
    report.dual_lanes = passes::check_lanes(&mut checker, &views);
    passes::check_keys(&mut checker, &views);
    passes::check_stats(&mut checker, &views, &pass_ports);
    let costs = cost::check(&mut checker, &views);
    report.passes = document
        .passes
        .iter()
        .zip(pass_ports)
        .zip(costs)
        .map(|((pass, ports), cost)| PassReport {
            id: pass.id.clone(),
            cost,
            outputs: ports.outputs,
            inner_outputs: ports.inner_outputs,
        })
        .collect();

    report.diagnostics = checker.diagnostics;
    report.diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.span.as_ref().map_or(0, |span| span.start),
            diagnostic.severity == DiagnosticSeverity::Warning,
        )
    });
    report
}

/// Returns the literal a one-key configuration table holds, for the reference evaluation of the tests.
#[cfg(test)]
pub(crate) fn literal_for_tests(value: &crate::value::ConfigValue) -> Option<crate::value::Literal> {
    config::literal_of(value)
}

/// State of one check.
struct Checker<'a> {
    file: &'a GraphFile,
    placer: crate::file::Placer<'a>,
    diagnostics: Vec<CodeDiagnostic>,
}

impl Checker<'_> {
    /// Adds an error at `place` of the file's places.
    fn error(&mut self, message: String, place: Option<Range<usize>>) {
        self.add(CodeDiagnostic::error(message), place);
    }

    /// Adds `diagnostic` placed at `place` of the file's places.
    fn add(&mut self, diagnostic: CodeDiagnostic, place: Option<Range<usize>>) {
        self.diagnostics.push(match place {
            Some(place) => self.placer.place(diagnostic, place),
            None => diagnostic,
        });
    }
}
