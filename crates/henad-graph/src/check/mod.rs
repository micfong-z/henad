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

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};

use crate::document::GraphDocument;
use crate::file::{BodyPlaces, GraphFile, NodePlaces, Placer};
use crate::ids::NodeId;
use crate::limits::{MAX_NODES_IN_FILE, MAX_NODES_PER_BODY};
use crate::node::OP_FUNCTION_CALL;
use crate::normal::{NormalPass, first_free_id, normalise_body};
use crate::types::{Rate, ValueType};
use crate::value::InputValue;
use view::{BodyView, BodyViews, Owner};

/// Stat labels a model cannot take: the names of the columns that exported stats or sweep results write before the
/// stats, the run id of a sweep's series, the tick, and the time of a model with a time scale.
///
/// A label is compared with them exactly, so `Time` is a label of its own.
pub(crate) const RESERVED_STAT_LABELS: [&str; 3] = ["run_id", "tick", "time"];

/// Parameter ids a model cannot take: the names of the columns that `runs.csv` of a sweep writes beside the
/// parameters, the ids, seed and key of a run before them, and its outcome, timing and note after them.
///
/// An id is compared with them exactly.
pub(crate) const RUNS_CSV_COLUMNS: [&str; 14] = [
    "run_id",
    "config_id",
    "block",
    "rep",
    "seed",
    "run_key",
    "status",
    "stop_reason",
    "ticks",
    "population",
    "build_ms",
    "wall_ms",
    "steps_per_s",
    "note",
];

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
    /// One report per function whose body was checked, in file order, with the function's id. A function of a file
    /// past the node limits has none.
    pub functions: Vec<PassReport>,
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

/// Facts the checker inferred about one body, a pass or a function.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PassReport {
    /// Id of the pass or the function.
    pub id: String,
    /// Cost per element in units, or of one call for a function, saturated at [`u64::MAX`].
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
    let file_sized = check_file_size(&mut checker);
    let views = BodyViews::new(&mut checker);
    check_body_sizes(&mut checker, &views);
    let mut clean = BTreeMap::new();
    let mut function_ports = Vec::new();
    for function_view in &views.functions {
        if walks(file_sized, function_view)
            && let Owner::Function(function) = function_view.owner
        {
            let (is_clean, ports) = functions::check_function(&mut checker, function_view);
            clean.insert(function.id.clone(), is_clean);
            function_ports.push((function.id.clone(), ports));
        }
    }
    agents::check_finishes(&mut checker, &views.functions);
    let inlinable = functions::inlinable(&views, &clean);
    // The size once inlined is known before anything is inlined. A file past it is checked as written alone, so a
    // small file cannot make the checker build an expansion that large.
    let costs = cost::check(&mut checker, &views);
    let sized = file_sized && costs.inlinable;
    passes::check_keys(&mut checker, &views);
    let normal = normalise_passes(&mut checker, &views, &inlinable, sized);
    let checked: Vec<BodyView<'_>> = views
        .passes
        .into_iter()
        .zip(&normal)
        .map(|(written, normal)| match (written.pass(), normal) {
            (Some(pass), Some((normal, normal_places))) => BodyView::normalised(
                &mut checker,
                document,
                pass,
                &normal.body.nodes,
                normal_places,
                &normal.origins,
            ),
            _ => written,
        })
        .collect();
    let pass_ports = check_pass_bodies(&mut checker, &checked, &normal, sized);
    report.dual_lanes = passes::check_lanes(&mut checker, &checked);
    passes::check_stats(&mut checker, &checked, &pass_ports);
    agents::check_passes(&mut checker, &checked);
    report.functions = function_ports
        .into_iter()
        .map(|(id, ports)| PassReport {
            cost: costs.functions.get(&id).copied().unwrap_or_default(),
            id,
            outputs: ports.outputs,
            inner_outputs: ports.inner_outputs,
        })
        .collect();
    report.passes = document
        .passes
        .iter()
        .zip(pass_ports)
        .zip(costs.passes)
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

/// Returns whether a body is walked: the file is within the node limits and the body holds at most
/// [`MAX_NODES_PER_BODY`] nodes.
fn walks(sized: bool, view: &BodyView<'_>) -> bool {
    sized && view.nodes.len() <= MAX_NODES_PER_BODY
}

/// Reports a file past [`MAX_NODES_IN_FILE`], and returns whether the file is within it.
fn check_file_size(checker: &mut Checker<'_>) -> bool {
    let document = checker.file.document();
    let total = document
        .passes
        .iter()
        .map(|pass| pass.body.nodes.len())
        .chain(document.functions.iter().map(|function| function.body.nodes.len()))
        .fold(0usize, usize::saturating_add);
    if total > MAX_NODES_IN_FILE {
        checker.error(
            format!("the file holds {total} nodes, expected at most {MAX_NODES_IN_FILE}"),
            Some(checker.file.places().id.clone()),
        );
    }
    total <= MAX_NODES_IN_FILE
}

/// Reports each pass or function past [`MAX_NODES_PER_BODY`].
fn check_body_sizes(checker: &mut Checker<'_>, views: &BodyViews<'_>) {
    let places = checker.file.places();
    let bodies = views
        .passes
        .iter()
        .zip(&places.passes)
        .chain(views.functions.iter().zip(&places.functions));
    for (body, body_places) in bodies {
        if body.nodes.len() > MAX_NODES_PER_BODY {
            checker.error(
                format!(
                    "the table holds {} nodes, expected at most {MAX_NODES_PER_BODY} per pass or function",
                    body.nodes.len()
                ),
                Some(body_places.table.clone()),
            );
        }
    }
}

/// Returns each pass normalised with the places of its nodes, or `None` for a pass checked as written.
///
/// Each walked pass has its calls checked first. A pass is checked after normalisation when every call it makes
/// reaches an `inlinable` function and every expression expands. Otherwise the pass as written is checked, a call
/// standing for its function's ports.
fn normalise_passes(
    checker: &mut Checker<'_>,
    views: &BodyViews<'_>,
    inlinable: &BTreeSet<String>,
    sized: bool,
) -> Vec<Option<(NormalPass, BodyPlaces)>> {
    let document = checker.file.document();
    let places = checker.file.places();
    let mut next = first_free_id(document);
    views
        .passes
        .iter()
        .zip(&places.passes)
        .map(|(view, body_places)| {
            let pass = view.pass()?;
            if !walks(sized, view) {
                return None;
            }
            functions::check_pass(checker, view);
            let calls_clean = pass.body.nodes.iter().all(|node| {
                node.op != OP_FUNCTION_CALL || node.config_str("function").is_some_and(|id| inlinable.contains(id))
            });
            if !calls_clean {
                return None;
            }
            let normal = normalise_body(document, &pass.body, &mut next).ok()?;
            let normal_places = normal_places(view, body_places, &normal);
            Some((normal, normal_places))
        })
        .collect()
}

/// Checks the body of each walked pass of `checked` and the rules of every pass, and returns each pass's ports.
fn check_pass_bodies(
    checker: &mut Checker<'_>,
    checked: &[BodyView<'_>],
    normal: &[Option<(NormalPass, BodyPlaces)>],
    sized: bool,
) -> Vec<body::BodyPorts> {
    let document = checker.file.document();
    let mut pass_ports = Vec::with_capacity(checked.len());
    for (index, pass_view) in checked.iter().enumerate() {
        let ports = if walks(sized, pass_view) {
            let mut ports = body::check(checker, pass_view);
            schedule::check(checker, pass_view);
            agents::check_pass(checker, pass_view);
            if let Some(Some((normal, _))) = normal.get(index) {
                replaced_ports(&mut ports, normal, document);
            }
            ports
        } else {
            body::BodyPorts::default()
        };
        passes::check_pass(checker, index, pass_view);
        pass_ports.push(ports);
    }
    pass_ports
}

pub(crate) use config::literal_of;
pub(crate) use cost::is_pass_cost;

/// Returns the places of the nodes of a normalised pass: a node as written keeps its own, and a node normalisation
/// added takes the table of the node as written it comes from.
fn normal_places(view: &BodyView<'_>, places: &BodyPlaces, normal: &NormalPass) -> BodyPlaces {
    let place_of = |id: NodeId| places.nodes.get(*view.positions.get(&id)?);
    let nodes = normal
        .body
        .nodes
        .iter()
        .map(|node| {
            if let Some(origin) = normal.origins.get(&node.id) {
                let written = origin.written();
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
fn replaced_ports(ports: &mut body::BodyPorts, normal: &NormalPass, document: &GraphDocument) {
    for (node, outputs) in &normal.replaced {
        let mut record = BTreeMap::new();
        for (port, value) in outputs {
            let port_type = match value {
                InputValue::Literal(literal) => Some(PortType {
                    value_type: Some(literal.value_type()),
                    rate: Rate::Constant,
                }),
                InputValue::Param(id) => document.param(id).map(|param| PortType {
                    value_type: Some(config::param_type(param.kind())),
                    rate: Rate::Parameter,
                }),
                InputValue::Link(link) => ports
                    .outputs
                    .get(&link.node)
                    .and_then(|found| link.output_in(found).cloned()),
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
    placer: Placer<'a>,
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
