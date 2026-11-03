//! Checker of a graph file: the validation rules a grid can meet, each failure reported, all at once.
//!
//! The checker reads a [`GraphFile`] and returns a [`CheckReport`]. It does not stop at the first problem, and a node
//! that fails to type yields a hole, a port of unknown type that fits every socket, so one bad wire gives one
//! diagnostic. A *body* is the nodes of a pass or a function. An error blocks a build, and a warning does not.

mod agents;
mod body;
mod bounds;
mod config;
mod cost;
mod declarations;
mod functions;
mod passes;
mod schedule;
mod view;

use std::collections::BTreeMap;
use std::ops::Range;

use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};

use crate::file::{BodyPlaces, GraphFile, NodePlaces};
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
#[expect(
    clippy::too_many_lines,
    reason = "the rules run in an order the later ones depend on"
)]
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
    let sized = total <= MAX_NODES_IN_FILE;
    let mut clean = BTreeMap::new();
    for function_view in &views.functions {
        if sized
            && function_view.nodes.len() <= MAX_NODES_PER_BODY
            && let view::Owner::Function(function) = function_view.owner
        {
            clean.insert(
                function.id.clone(),
                functions::check_function(&mut checker, function_view),
            );
        }
    }
    let inlinable = functions::inlinable(&views, &clean);
    passes::check_keys(&mut checker, &views);
    // Each pass is checked after normalisation when every call it makes reaches clean functions and every expression
    // expands. Otherwise the pass as written is checked, a call standing for its function's ports.
    let mut next = crate::normal::first_free_id(document);
    let normal: Vec<Option<(crate::normal::NormalPass, BodyPlaces)>> = views
        .passes
        .iter()
        .zip(&places.passes)
        .map(|(view, body_places)| {
            let pass = view.pass()?;
            if !sized || view.nodes.len() > MAX_NODES_PER_BODY {
                return None;
            }
            functions::check_pass(&mut checker, view);
            let calls_clean = pass.body.nodes.iter().all(|node| {
                node.op != crate::node::OP_FUNCTION_CALL
                    || node
                        .config
                        .get("function")
                        .and_then(crate::value::ConfigValue::as_str)
                        .is_some_and(|id| inlinable.contains(id))
            });
            if !calls_clean {
                return None;
            }
            let normal = crate::normal::normalise_body(document, &pass.body, &mut next).ok()?;
            let normal_places = normal_places(view, body_places, &normal);
            Some((normal, normal_places))
        })
        .collect();
    let checked: Vec<view::BodyView<'_>> = views
        .passes
        .iter()
        .zip(&normal)
        .map(|(written, normal)| match (written.pass(), normal) {
            (Some(pass), Some((normal, normal_places))) => view::BodyView::normalised(
                &mut checker,
                document,
                pass,
                &normal.body.nodes,
                normal_places,
                &normal.origins,
            ),
            _ => written.clone_view(),
        })
        .collect();
    let mut pass_ports = Vec::with_capacity(document.passes.len());
    for (index, pass_view) in checked.iter().enumerate() {
        let ports = if !sized || pass_view.nodes.len() > MAX_NODES_PER_BODY {
            body::BodyPorts::default()
        } else {
            let mut ports = body::check(&mut checker, pass_view);
            schedule::check(&mut checker, pass_view);
            agents::check_pass(&mut checker, pass_view);
            if let Some(Some((normal, _))) = normal.get(index) {
                replaced_ports(&mut ports, normal, document);
            }
            ports
        };
        passes::check_pass(&mut checker, index, pass_view);
        pass_ports.push(ports);
    }
    report.dual_lanes = passes::check_lanes(&mut checker, &checked);
    passes::check_stats(&mut checker, &checked, &pass_ports);
    agents::check_passes(&mut checker, &checked);
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

pub(crate) use config::literal_of;

/// Returns the places of the nodes of a normalised pass: a node as written keeps its own, and a node normalisation
/// added takes the table of the node as written it comes from.
fn normal_places(view: &view::BodyView<'_>, places: &BodyPlaces, normal: &crate::normal::NormalPass) -> BodyPlaces {
    let place_of = |id: crate::ids::NodeId| places.nodes.get(*view.positions.get(&id)?);
    let nodes = normal
        .body
        .nodes
        .iter()
        .map(|node| {
            if let Some(origin) = normal.origins.get(&node.id) {
                let written = origin.calls.first().copied().unwrap_or(origin.node);
                place_of(written).map_or_else(
                    || NodePlaces::at(places.table.clone()),
                    |found| NodePlaces::at(found.table.clone()),
                )
            } else {
                place_of(node.id)
                    .cloned()
                    .unwrap_or_else(|| NodePlaces::at(places.table.clone()))
            }
        })
        .collect();
    BodyPlaces {
        table: places.table.clone(),
        nodes,
    }
}

/// Adds to `ports` the outputs of each call and expression normalisation replaced, typed as the values they take.
fn replaced_ports(
    ports: &mut body::BodyPorts,
    normal: &crate::normal::NormalPass,
    document: &crate::document::GraphDocument,
) {
    for (node, outputs) in &normal.replaced {
        let mut record = BTreeMap::new();
        for (port, value) in outputs {
            let port_type = match value {
                crate::value::InputValue::Literal(literal) => Some(PortType {
                    value_type: Some(literal.value_type()),
                    rate: Rate::Constant,
                }),
                crate::value::InputValue::Param(id) => {
                    document
                        .params
                        .iter()
                        .find(|param| param.id() == id)
                        .map(|param| PortType {
                            value_type: Some(config::param_type(param.kind())),
                            rate: Rate::Parameter,
                        })
                }
                crate::value::InputValue::Link(link) => {
                    ports.outputs.get(&link.node).and_then(|found| match &link.port {
                        Some(name) => found.get(name).cloned(),
                        None if found.len() == 1 => found.values().next().cloned(),
                        None => None,
                    })
                }
            };
            if let Some(port_type) = port_type {
                record.insert(port.clone(), port_type);
            }
        }
        ports.outputs.insert(*node, record);
    }
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
