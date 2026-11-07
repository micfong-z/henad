//! Live and reload-only parameters of a compiled graph.
//!
//! A *live* parameter reaches a running model: the engine writes the uniform blocks of the passes again with its new
//! value, and the next step reads it. A *reload-only* parameter is read while the model is built, so an edit of it
//! needs a new build. A graph's own parameter is reload-only when the host reads it at build: an init pass reads it or
//! runs by it, it sets the reach or the cell size of a radius fold (the neighbour index and the work budget both follow
//! them), or it is the tick length of the graph's time scale. Every other one is live. The sizes and counts the entry
//! puts in front of the graph's own parameters are reload-only.

use std::collections::BTreeSet;

use crate::document::{PassEnabled, PassStage};
use crate::host::HostEvaluator;
use crate::host::compile::{Instruction, Operation, Program};

use super::emit::defined;

/// Returns whether each parameter of the document `evaluator` runs is reload-only, in the document's order.
pub(crate) fn reload_only(evaluator: &HostEvaluator) -> Vec<bool> {
    let document = evaluator.document();
    let position_of = |id: &str| document.params.iter().position(|param| param.id() == id);
    let mut read: BTreeSet<usize> = BTreeSet::new();
    if let Some(time) = &document.time {
        read.extend(position_of(&time.tick_length));
    }
    for (position, pass) in document.passes.iter().enumerate() {
        if pass.stage == PassStage::Init
            && let PassEnabled::Param(id) = &pass.enabled
        {
            read.extend(position_of(id));
        }
        let Some(Ok(program)) = evaluator.program_at(position) else {
            continue;
        };
        if pass.stage == PassStage::Init {
            read.extend(program.params.iter().map(|&(_, param)| param));
        } else {
            read.extend(reach_params(program));
        }
    }
    (0..document.params.len()).map(|param| read.contains(&param)).collect()
}

/// Returns the positions of the parameters that the reach or the cell size of a radius fold of `program` reads.
fn reach_params(program: &Program) -> BTreeSet<usize> {
    let instructions = flattened(&program.prelude)
        .into_iter()
        .chain(flattened(&program.element))
        .collect::<Vec<_>>();
    let mut needed: BTreeSet<usize> = instructions
        .iter()
        .filter_map(|instruction| match &instruction.operation {
            Operation::RadiusFold(fold) => Some([fold.radius, fold.cell_size]),
            _ => None,
        })
        .flatten()
        .collect();
    if needed.is_empty() {
        return BTreeSet::new();
    }
    // Every slot a needed slot is computed from is needed too, until nothing new is found.
    loop {
        let before = needed.len();
        for instruction in &instructions {
            if matches!(instruction.operation, Operation::RadiusFold(_)) {
                continue;
            }
            if defined(instruction).iter().any(|slot| needed.contains(slot)) {
                needed.extend(instruction.operation.operands());
            }
        }
        if needed.len() == before {
            break;
        }
    }
    program
        .params
        .iter()
        .filter(|(slot, _)| needed.contains(slot))
        .map(|&(_, param)| param)
        .collect()
}

/// Returns the instructions of `block` with the bodies of its regions, nested ones included.
fn flattened(block: &[Instruction]) -> Vec<&Instruction> {
    let mut found = Vec::new();
    for instruction in block {
        found.push(instruction);
        match &instruction.operation {
            Operation::Repeat(repeat) => found.extend(flattened(&repeat.body)),
            Operation::GridFold(fold) => found.extend(flattened(&fold.body)),
            Operation::RadiusFold(fold) => found.extend(flattened(&fold.body)),
            _ => {}
        }
    }
    found
}
