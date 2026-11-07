//! Probes of a compiled graph: the Boolean node outputs whose fires the GPU counts on the counted step.
//!
//! A *probe* names one Boolean output of a node in a tick pass. On the counted step, the last step of each batch the
//! engine records, the step's shader adds 1 for every element at which the output holds, and the engine reads the
//! counts back with the stats. Probes sit beside the document, outside its text and its code identity, so adding one
//! changes the shaders and no byte of the file.

use henad_core::diagnostic::CodeDiagnostic;

use crate::check::CheckReport;
use crate::document::{GraphDocument, PassEnabled, PassStage};
use crate::host::HostEvaluator;
use crate::host::compile::Program;
use crate::ids::NodeId;
use crate::types::ValueType;
use crate::value::PortRef;

/// Node outputs a compiled graph counts fires of, in the order of the counts.
///
/// Each names a Boolean output of a node in a tick pass and outside every region, such as `n16.fired.line_2`.
/// [`Self::default`] holds none, and a graph compiled with none counts nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probes {
    outputs: Vec<PortRef>,
}

impl Probes {
    /// Creates the probes of `outputs`, counted in that order.
    pub fn new(outputs: Vec<PortRef>) -> Self {
        Self { outputs }
    }

    /// Node outputs of the probes, in the order of the counts.
    pub fn outputs(&self) -> &[PortRef] {
        &self.outputs
    }

    /// Number of probes.
    pub fn len(&self) -> usize {
        self.outputs.len()
    }

    /// Returns whether there is no probe.
    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }
}

/// A probe resolved to the slot of its output in its pass's program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProbeSlot {
    /// Position of the probe among the [`Probes`], which is its count's position.
    pub index: usize,
    /// Document position of the tick pass that holds the node.
    pub position: usize,
    /// Slot of the output in the pass's program.
    pub slot: usize,
    /// Node of the output.
    pub node: NodeId,
}

/// Returns the slot of `output` in `program`, the node's only output when it names no port.
pub(crate) fn output_slot(program: &Program, output: &PortRef) -> Option<(String, usize)> {
    if let Some(port) = &output.port {
        return program
            .outputs
            .get(&(output.node, port.clone()))
            .map(|slot| (port.clone(), *slot));
    }
    let mut ports = program.outputs.iter().filter(|((node, _), _)| *node == output.node);
    let only = ports.next().map(|((_, port), slot)| (port.clone(), *slot));
    if ports.next().is_some() { None } else { only }
}

/// Returns each probe of `probes` resolved in `document`, whose host evaluator is `evaluator` and whose check is
/// `report`, leaving out a probe whose pass `enabled = false` turns off, since that pass never runs.
///
/// # Errors
///
/// Returns a problem for each probe that names no output of a tick pass, or one of an expression or a function call,
/// each that names a node inside a region, and each that names an output of another type than `bool`.
pub(crate) fn resolve(
    probes: &Probes,
    document: &GraphDocument,
    evaluator: &HostEvaluator,
    report: &CheckReport,
) -> Result<Vec<ProbeSlot>, Vec<CodeDiagnostic>> {
    let mut found = Vec::with_capacity(probes.len());
    let mut problems = Vec::new();
    for (index, output) in probes.outputs().iter().enumerate() {
        let located = document
            .passes
            .iter()
            .enumerate()
            .filter(|(_, pass)| pass.stage == PassStage::Tick)
            .find_map(|(position, pass)| {
                let Some(Ok(program)) = evaluator.program_at(position) else {
                    return None;
                };
                output_slot(program, output).map(|(port, slot)| (position, pass, port, slot))
            });
        let Some((position, pass, port, slot)) = located else {
            problems.push(CodeDiagnostic::error(format!(
                "the probe '{output}' names no output of a node in a tick pass, outside every expression and function \
                 call"
            )));
            continue;
        };
        let Some(node) = pass.body.nodes.iter().find(|node| node.id == output.node) else {
            problems.push(CodeDiagnostic::error(format!(
                "the probe '{output}' names no output of a node in a tick pass, outside every expression and function \
                 call"
            )));
            continue;
        };
        if node.region.is_some() {
            problems.push(
                CodeDiagnostic::error(format!(
                    "the probe '{output}' names a node inside a region, which runs once per iteration, and a probe \
                     counts once per element"
                ))
                .with_pass(pass.id.clone())
                .with_node(output.node.to_string()),
            );
            continue;
        }
        let value_type = report
            .passes
            .get(position)
            .and_then(|pass| pass.outputs.get(&output.node)?.get(&port))
            .and_then(|port_type| port_type.value_type.as_ref());
        if value_type != Some(&ValueType::Bool) {
            problems.push(
                CodeDiagnostic::error(format!(
                    "the probe '{output}' names an output that holds no bool, and a probe counts the elements where \
                     its output holds"
                ))
                .with_pass(pass.id.clone())
                .with_node(output.node.to_string()),
            );
            continue;
        }
        if pass.enabled != PassEnabled::Literal(false) {
            found.push(ProbeSlot {
                index,
                position,
                slot,
                node: output.node,
            });
        }
    }
    if problems.is_empty() { Ok(found) } else { Err(problems) }
}
