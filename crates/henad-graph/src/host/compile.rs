//! Compilation of a pass into a [`Program`] of the host evaluator: one instruction per node, in an order every input
//! precedes, with every value in a numbered slot of one frame.
//!
//! A pure value of a region body that reads no iteration and no element runs in the prelude instead.

use std::collections::{BTreeMap, BTreeSet};

use henad_core::authoring::primitives::space::Boundary;

use super::HostError;
use super::program::{
    CompareOp, DrawForm, Instruction, LogicOp, Operation, Program, RadiusFold, Repeat, RepeatAccumulator, StencilFold,
    SumSpec, TransitionRule,
};
use super::value::{BinaryOp, Conversion, HostValue, UnaryOp, extreme_of};
use crate::catalogue::{Effect, NodeDef, OutputRate, effective_config, lookup, transition_rule_ids};
use crate::check::literal_of;
use crate::document::{FieldCombine, GraphDocument, Pass};
use crate::ids::NodeId;
use crate::node::{Combine, Node};
use crate::reader::read_u64;
use crate::types::Rate;
use crate::value::{ConfigValue, InputValue, PortRef};

/// Moves every instruction of a region body in `program`'s element block that is pure and reads only values the same
/// for every element and iteration to the end of the prelude, nested bodies included, in body order.
///
/// Such a value is the same in every iteration, so the prelude computes it once. The WGSL compiler then reads it from
/// the uniform block, as it reads every other prelude value.
fn hoist_uniform_body_values(program: &mut Program) {
    let mut uniform: BTreeSet<usize> = program
        .constants
        .iter()
        .map(|(slot, _)| *slot)
        .chain(program.params.iter().map(|(slot, _)| *slot))
        .collect();
    for instruction in &program.prelude {
        uniform.insert(instruction.out);
        if let Operation::Transition { fired, .. } = &instruction.operation {
            uniform.extend(fired.iter().copied());
        }
    }
    let mut hoisted = Vec::new();
    for instruction in &mut program.element {
        if let Some(body) = instruction.operation.body_mut() {
            hoist_from_body(body, &mut uniform, &mut hoisted);
        }
    }
    program.prelude.extend(hoisted);
}

/// Moves the instructions of `repeat`'s body, and of the bodies nested in it, that are pure and read only `uniform`
/// slots into `hoisted`, and adds the slots they fill to `uniform`.
fn hoist_from_body(body: &mut Vec<Instruction>, uniform: &mut BTreeSet<usize>, hoisted: &mut Vec<Instruction>) {
    let taken = std::mem::take(body);
    for mut instruction in taken {
        if let Some(inner) = instruction.operation.body_mut() {
            hoist_from_body(inner, uniform, hoisted);
        }
        let operation = &instruction.operation;
        if operation.is_pure() && operation.operands().iter().all(|slot| uniform.contains(slot)) {
            uniform.insert(instruction.out);
            hoisted.push(instruction);
        } else {
            body.push(instruction);
        }
    }
}

/// When an instruction at the top of a pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Timing {
    /// Once, before the elements.
    Prelude,
    /// Per element.
    Element,
    /// Once, after the elements.
    Post,
}

/// State of the compilation of one pass.
struct Compiler<'a> {
    document: &'a GraphDocument,
    pass: &'a Pass,
    /// Position of each node in the body, the first node of a duplicate id.
    positions: BTreeMap<NodeId, usize>,
    /// Definition of each node.
    defs: Vec<&'static NodeDef>,
    /// Enclosing region of each node, by position, `None` at the top of the pass.
    parents: Vec<Option<usize>>,
    /// Regions around each node, by position, innermost first.
    ancestors: Vec<Vec<usize>>,
    /// Slot of each output by node and port.
    outputs: BTreeMap<(NodeId, String), usize>,
    /// Slot of each inner output of a region by node and port.
    inner_outputs: BTreeMap<(NodeId, String), usize>,
    /// Slot of the only output of each node that has exactly one.
    only_outputs: BTreeMap<NodeId, usize>,
    /// Slot of the first output of each node that has one.
    first_outputs: BTreeMap<NodeId, usize>,
    /// Running slot of each accumulator by region and name.
    running: BTreeMap<(NodeId, String), (usize, Combine)>,
    slots: usize,
    constants: Vec<(usize, HostValue)>,
    params: BTreeMap<usize, usize>,
    draw_keys: Vec<u32>,
    tallies: usize,
    tally_slots: Vec<usize>,
    sums: Vec<SumSpec>,
    written: BTreeSet<usize>,
    deposited: BTreeSet<usize>,
    colors: bool,
}

/// Returns the program of `pass` in `document`.
///
/// # Errors
///
/// Returns [`HostError::Unsupported`] for a node of a kind with no host form, and for a body a checked file never
/// holds, such as a cycle.
pub(crate) fn compile(document: &GraphDocument, pass: &Pass) -> Result<Program, HostError> {
    let nodes = &pass.body.nodes;
    let mut positions = BTreeMap::new();
    let mut defs = Vec::with_capacity(nodes.len());
    for (position, node) in nodes.iter().enumerate() {
        positions.entry(node.id).or_insert(position);
        let def = lookup(&node.op, document.library)
            .filter(|def| def.backends.host)
            .ok_or_else(|| unsupported(pass, Some(node.id), format!("the kind `{}` has no host form", node.op)))?;
        defs.push(def);
    }
    let parents: Vec<Option<usize>> = nodes.iter().map(|node| positions.get(&node.region?).copied()).collect();
    let ancestors = (0..nodes.len()).map(|position| ancestors(&parents, position)).collect();
    let mut compiler = Compiler {
        document,
        pass,
        positions,
        defs,
        parents,
        ancestors,
        outputs: BTreeMap::new(),
        inner_outputs: BTreeMap::new(),
        only_outputs: BTreeMap::new(),
        first_outputs: BTreeMap::new(),
        running: BTreeMap::new(),
        slots: 0,
        constants: Vec::new(),
        params: BTreeMap::new(),
        draw_keys: Vec::new(),
        tallies: 0,
        tally_slots: Vec::new(),
        sums: Vec::new(),
        written: BTreeSet::new(),
        deposited: BTreeSet::new(),
        colors: false,
    };
    compiler.assign_output_slots();
    let order = compiler.order()?;
    let top = order.get(&None).cloned().unwrap_or_default();
    let timings = compiler.timings(&top)?;
    let (mut prelude, mut element, mut post) = (Vec::new(), Vec::new(), Vec::new());
    let mut uniform_outputs = Vec::new();
    for (&position, timing) in top.iter().zip(&timings) {
        let instruction = compiler.instruction(position, &order)?;
        let id = nodes[position].id;
        if *timing != Timing::Element || compiler.defs[position].has_effect(Effect::Reduce) {
            let node_outputs = compiler
                .outputs
                .range((id, String::new())..)
                .take_while(|((node, _), _)| *node == id);
            for ((_, port), slot) in node_outputs {
                uniform_outputs.push((id, port.clone(), *slot));
            }
        }
        match timing {
            Timing::Prelude => prelude.push(instruction),
            Timing::Element => element.push(instruction),
            Timing::Post => post.push(instruction),
        }
    }
    let mut program = Program {
        pass: pass.id.clone(),
        domain: pass.domain,
        slots: compiler.slots,
        constants: compiler.constants,
        params: compiler.params.into_iter().map(|(param, slot)| (slot, param)).collect(),
        draw_keys: compiler.draw_keys,
        prelude,
        element,
        post,
        tallies: compiler.tallies,
        tally_slots: compiler.tally_slots,
        sums: compiler.sums,
        written: compiler.written.into_iter().collect(),
        deposited: compiler.deposited.into_iter().collect(),
        colors: compiler.colors,
        uniform_outputs,
        outputs: compiler.outputs,
        inner_outputs: compiler.inner_outputs,
    };
    hoist_uniform_body_values(&mut program);
    Ok(program)
}

/// Returns the boundary a stencil's boundary word names, `None` for the domain grid's own.
fn boundary_of(word: &str) -> Option<Boundary> {
    match word {
        "torus" => Some(Boundary::Torus),
        "bounded" => Some(Boundary::Bounded),
        _ => None,
    }
}

/// Returns the regions around the node at `position`, innermost first, where `parents` holds the enclosing region of
/// each node.
fn ancestors(parents: &[Option<usize>], position: usize) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = parents[position];
    while let Some(region) = current {
        if region == position || path.contains(&region) {
            break;
        }
        path.push(region);
        current = parents[region];
    }
    path
}

/// Returns the error for a pass the host cannot run.
fn unsupported(pass: &Pass, node: Option<NodeId>, reason: String) -> HostError {
    HostError::Unsupported {
        pass: pass.id.clone(),
        node,
        reason,
    }
}

/// Returns the links among `node`'s inputs.
fn links(node: &Node) -> impl Iterator<Item = &PortRef> {
    node.inputs.values().filter_map(|value| match value {
        InputValue::Link(port) => Some(port),
        _ => None,
    })
}

impl<'a> Compiler<'a> {
    /// Returns a fresh slot.
    fn slot(&mut self) -> usize {
        self.slots += 1;
        self.slots - 1
    }

    /// Returns a slot holding the constant `value`.
    fn constant(&mut self, value: HostValue) -> usize {
        let slot = self.slot();
        self.constants.push((slot, value));
        slot
    }

    /// Gives every output and inner output of every node a slot.
    fn assign_output_slots(&mut self) {
        let nodes = &self.pass.body.nodes;
        for (position, node) in nodes.iter().enumerate() {
            if self.positions.get(&node.id) != Some(&position) {
                continue;
            }
            let def = self.defs[position];
            let outputs = def.resolved_outputs(node, self.document);
            for output in &outputs {
                let slot = self.slot();
                self.outputs.insert((node.id, output.name.clone()), slot);
                self.first_outputs.entry(node.id).or_insert(slot);
                if outputs.len() == 1 {
                    self.only_outputs.insert(node.id, slot);
                }
            }
            for output in def.resolved_inner_outputs(node) {
                let slot = self.slot();
                self.inner_outputs.insert((node.id, output.name), slot);
            }
        }
    }

    /// Returns the order of the members of every scope: the top of the pass under `None` and each region's body under
    /// its position. Every member follows the members whose values it or its body reads, ties broken by node id, and
    /// the pass's stream draws follow `draw_order`.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Unsupported`] for a cycle.
    fn order(&self) -> Result<BTreeMap<Option<usize>, Vec<usize>>, HostError> {
        let nodes = &self.pass.body.nodes;
        let mut members: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            if self.positions.get(&node.id) == Some(&position) {
                members.entry(self.parents[position]).or_default().push(position);
            }
        }
        let mut edges: BTreeMap<Option<usize>, BTreeSet<(usize, usize)>> = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            for link in links(node) {
                let Some(&source) = self.positions.get(&link.node) else {
                    continue;
                };
                if let Some(edge) = self.scope_edge(source, position) {
                    edges.entry(edge.0).or_default().insert((edge.1, edge.2));
                }
            }
        }
        let draws: Vec<usize> = self
            .pass
            .draw_order
            .iter()
            .filter_map(|id| self.positions.get(id).copied())
            .collect();
        for pair in draws.windows(2) {
            if self.parents[pair[0]].is_none() && self.parents[pair[1]].is_none() {
                edges.entry(None).or_default().insert((pair[0], pair[1]));
            }
        }
        let mut order = BTreeMap::new();
        for (scope, scope_members) in members {
            let scope_edges = edges.remove(&scope).unwrap_or_default();
            order.insert(scope, self.sort_scope(&scope_members, &scope_edges)?);
        }
        Ok(order)
    }

    /// Returns the scope and the two members of it that a link from `source` to `target` orders, as
    /// `(scope, before, after)`, or `None` when the link orders no two members of one scope: a read of an enclosing
    /// region's inner output, or a read of a node in an outer scope.
    fn scope_edge(&self, source: usize, target: usize) -> Option<(Option<usize>, usize, usize)> {
        let ancestors = &self.ancestors;
        if ancestors[target].contains(&source) {
            return None;
        }
        let scopes = ancestors[target].iter().map(|&region| Some(region)).chain([None]);
        for scope in scopes {
            let inside = scope.is_none_or(|region| ancestors[source].contains(&region));
            if !inside {
                continue;
            }
            let member_of = |position: usize| {
                std::iter::once(position)
                    .chain(ancestors[position].iter().copied())
                    .find(|&candidate| self.parents[candidate] == scope)
            };
            let (before, after) = (member_of(source)?, member_of(target)?);
            return (before != after).then_some((scope, before, after));
        }
        None
    }

    /// Returns `members` in an order that every edge of `edges` respects, ties broken by node id.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Unsupported`] for a cycle.
    fn sort_scope(&self, members: &[usize], edges: &BTreeSet<(usize, usize)>) -> Result<Vec<usize>, HostError> {
        let nodes = &self.pass.body.nodes;
        let mut waiting: BTreeMap<usize, usize> = members.iter().map(|&member| (member, 0)).collect();
        let mut after: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(before, later) in edges {
            *waiting.entry(later).or_default() += 1;
            after.entry(before).or_default().push(later);
        }
        let mut ready: BTreeSet<(NodeId, usize)> = waiting
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(&member, _)| (nodes[member].id, member))
            .collect();
        let mut order = Vec::with_capacity(members.len());
        while let Some((_, member)) = ready.pop_first() {
            order.push(member);
            for &later in after.get(&member).map_or(&[][..], Vec::as_slice) {
                if let Some(count) = waiting.get_mut(&later) {
                    *count -= 1;
                    if *count == 0 {
                        ready.insert((nodes[later].id, later));
                    }
                }
            }
        }
        if order.len() == members.len() {
            Ok(order)
        } else {
            Err(unsupported(
                self.pass,
                None,
                "the pass holds a cycle, or a stream draw out of its `draw_order`".to_owned(),
            ))
        }
    }

    /// Returns when each member of the top of the pass runs.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Unsupported`] for a value per element that reads a reducer of its own pass.
    fn timings(&self, top: &[usize]) -> Result<Vec<Timing>, HostError> {
        let nodes = &self.pass.body.nodes;
        let mut timing_of: BTreeMap<NodeId, Timing> = BTreeMap::new();
        let mut timings = Vec::with_capacity(top.len());
        for &position in top {
            let node = &nodes[position];
            let def = self.defs[position];
            let per_element = def.is_region
                || !def.effects.is_empty()
                || def
                    .outputs
                    .iter()
                    .any(|output| matches!(output.rate, OutputRate::Pinned(rate) if rate >= Rate::Element));
            // A counter total is known only from the readback after the elements, so it runs in the post block.
            let own = if per_element {
                Timing::Element
            } else if def.is_ready_after_elements() {
                Timing::Post
            } else {
                Timing::Prelude
            };
            let mut timing = own;
            let mut reads_element = own == Timing::Element;
            let mut reads_post = own == Timing::Post;
            for link in self.body_links(position) {
                let Some(&source) = self.positions.get(&link.node) else {
                    continue;
                };
                if self.parents[source].is_some() || source == position {
                    continue;
                }
                let source_node = nodes[source].id;
                let source_reduces = self.defs[source].is_ready_after_elements();
                match timing_of.get(&source_node) {
                    _ if source_reduces => reads_post = true,
                    Some(Timing::Post) => reads_post = true,
                    Some(Timing::Element) => reads_element = true,
                    _ => {}
                }
            }
            if reads_post && reads_element {
                return Err(unsupported(
                    self.pass,
                    Some(node.id),
                    "reads a reduced value and a value per element at once".to_owned(),
                ));
            }
            if reads_post {
                timing = Timing::Post;
            } else if reads_element {
                timing = Timing::Element;
            }
            timing_of.insert(node.id, timing);
            timings.push(timing);
        }
        Ok(timings)
    }

    /// Returns the links of the node at `position` and, for a region, of every node of its body.
    fn body_links(&self, position: usize) -> Vec<&PortRef> {
        let nodes = &self.pass.body.nodes;
        let mut found: Vec<&PortRef> = links(&nodes[position]).collect();
        if self.defs[position].is_region {
            for (inner, node) in nodes.iter().enumerate() {
                if inner != position && self.ancestors[inner].contains(&position) {
                    found.extend(links(node));
                }
            }
        }
        found
    }

    /// Returns the slot a link from the node at `reader` reads.
    fn link_slot(&self, reader: usize, link: &PortRef) -> Result<usize, HostError> {
        let node = &self.pass.body.nodes[reader];
        let broken = || {
            unsupported(
                self.pass,
                Some(node.id),
                format!("reads '{link}', which names no output"),
            )
        };
        let source = *self.positions.get(&link.node).ok_or_else(broken)?;
        if self.ancestors[reader].contains(&source) {
            let port = link.port.as_ref().ok_or_else(broken)?;
            return self
                .inner_outputs
                .get(&(link.node, port.clone()))
                .copied()
                .ok_or_else(broken);
        }
        match &link.port {
            Some(port) => self.outputs.get(&(link.node, port.clone())).copied(),
            None => self.only_outputs.get(&link.node).copied(),
        }
        .ok_or_else(broken)
    }

    /// Returns the slot of input `name` of the node at `position`, or `None` for an unwritten input without a literal
    /// default.
    fn input(&mut self, position: usize, name: &str) -> Result<Option<usize>, HostError> {
        let node = &self.pass.body.nodes[position];
        let value = match node.inputs.get(name) {
            Some(value) => value.clone(),
            None => match self.defs[position].input_default(node, self.document, name) {
                Some(value) => value,
                None => return Ok(None),
            },
        };
        Ok(Some(match value {
            InputValue::Link(link) => self.link_slot(position, &link)?,
            InputValue::Literal(literal) => {
                let value = HostValue::from_literal(&literal, self.document).ok_or_else(|| {
                    unsupported(
                        self.pass,
                        Some(node.id),
                        format!("gives `{name}` a literal with no host value"),
                    )
                })?;
                self.constant(value)
            }
            InputValue::Param(id) => {
                let param = self
                    .document
                    .param_position(&id)
                    .ok_or_else(|| unsupported(self.pass, Some(node.id), format!("binds the parameter '{id}'")))?;
                if let Some(&slot) = self.params.get(&param) {
                    slot
                } else {
                    let slot = self.slot();
                    self.params.insert(param, slot);
                    slot
                }
            }
        }))
    }

    /// Returns the slot of the required input `name` of the node at `position`.
    fn required(&mut self, position: usize, name: &str) -> Result<usize, HostError> {
        let id = self.pass.body.nodes[position].id;
        self.input(position, name)?
            .ok_or_else(|| unsupported(self.pass, Some(id), format!("leaves `{name}` unwritten")))
    }

    /// Returns the instructions of the members of the scope of `region`, in order.
    fn block(
        &mut self,
        region: usize,
        order: &BTreeMap<Option<usize>, Vec<usize>>,
    ) -> Result<Vec<Instruction>, HostError> {
        let members = order.get(&Some(region)).cloned().unwrap_or_default();
        members
            .into_iter()
            .map(|position| self.instruction(position, order))
            .collect()
    }

    /// Returns the position of the variant `name` of `category`.
    fn variant(&self, position: usize, category: &str, name: &str) -> Result<u32, HostError> {
        self.document
            .category(category)
            .and_then(|known| known.variant_position(name))
            .and_then(|index| u32::try_from(index).ok())
            .ok_or_else(|| {
                unsupported(
                    self.pass,
                    Some(self.pass.body.nodes[position].id),
                    format!("names the variant '{name}' of '{category}'"),
                )
            })
    }

    /// Returns the number of variants of `category`.
    fn variant_count(&self, category: &str) -> u32 {
        self.document
            .category(category)
            .map_or(0, |known| u32::try_from(known.variants.len()).unwrap_or(u32::MAX))
    }

    /// Returns the position of the lane `id`.
    fn lane(&self, position: usize, id: &str) -> Result<usize, HostError> {
        self.document.lane_position(id).ok_or_else(|| {
            unsupported(
                self.pass,
                Some(self.pass.body.nodes[position].id),
                format!("names the lane '{id}'"),
            )
        })
    }

    /// Returns the number of colours of the palette `id`, a `[[palette]]` table or a category.
    fn palette_len(&self, id: &str) -> u32 {
        self.document
            .palettes
            .iter()
            .find(|palette| palette.id == id)
            .map_or_else(
                || self.variant_count(id),
                |palette| u32::try_from(palette.colors.len()).unwrap_or(u32::MAX),
            )
    }

    /// Returns the instruction of the node at `position`.
    #[expect(clippy::too_many_lines, reason = "one arm per kind reads best as one table")]
    fn instruction(
        &mut self,
        position: usize,
        order: &BTreeMap<Option<usize>, Vec<usize>>,
    ) -> Result<Instruction, HostError> {
        let node = &self.pass.body.nodes[position];
        let def = self.defs[position];
        let config = effective_config(def, node);
        let id = node.id;
        let text = |key: &str| {
            config
                .get(key)
                .and_then(ConfigValue::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let integer = |key: &str| config.get(key).and_then(ConfigValue::as_integer).unwrap_or_default();
        let out = self.first_outputs.get(&id).copied().unwrap_or(usize::MAX);
        let operation = match def.id {
            "ctx.world" => Operation::World,
            "ctx.grid" => Operation::Grid,
            "ctx.cell_xy" => Operation::CellXy,
            "lane.get_at" => Operation::LaneGetAt {
                lane: self.lane(position, &text("lane"))?,
                at: self.required(position, "at")?,
            },
            "field.get" => {
                let (first, layers, field) = self.field(position, &text("field"))?;
                let zero = HostValue::zero_of(&field.value_type)
                    .ok_or_else(|| unsupported(self.pass, Some(id), "reads a field with no host value".to_owned()))?;
                Operation::FieldGet {
                    first,
                    layers,
                    layer: self.layer_input(position)?,
                    at: self.input(position, "at")?,
                    zero,
                }
            }
            "field.set" => {
                let (first, layers, _) = self.field(position, &text("field"))?;
                for layer in 0..layers as usize {
                    self.written.insert(first + layer);
                }
                Operation::FieldSet {
                    first,
                    layers,
                    layer: self.layer_input(position)?,
                    value: self.required(position, "value")?,
                }
            }
            "field.deposit" => {
                let field_id = text("field");
                let (_, layers, _) = self.field(position, &field_id)?;
                let first = self.first_deposit(position, &field_id)?;
                for layer in 0..layers as usize {
                    self.deposited.insert(first + layer);
                }
                Operation::Deposit {
                    first,
                    layers,
                    layer: self.layer_input(position)?,
                    at: self.required(position, "at")?,
                    value: self.required(position, "value")?,
                    combine: self
                        .document
                        .field(&field_id)
                        .and_then(|field| field.combine)
                        .unwrap_or(FieldCombine::Max),
                }
            }
            "counter.add" => Operation::CounterAdd {
                counter: self.counter(position, &text("counter"))?,
                amount: self.required(position, "amount")?,
            },
            "stat.counter" => Operation::CounterTotal {
                counter: self.counter(position, &text("counter"))?,
            },
            "math.clamp" => Operation::Clamp {
                x: self.required(position, "x")?,
                lo: self.required(position, "lo")?,
                hi: self.required(position, "hi")?,
            },
            "math.midpoint" => Operation::Midpoint {
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
            },
            "vec.make" => Operation::VecMake {
                x: self.required(position, "x")?,
                y: self.required(position, "y")?,
            },
            "vec.split" => Operation::VecSplit {
                v: self.required(position, "v")?,
                y_out: self.output_slot(id, "y")?,
            },
            "vec.scale" => Operation::VecScale {
                v: self.required(position, "v")?,
                s: self.required(position, "s")?,
            },
            "vec.dot" => Operation::VecDot {
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
            },
            "vec.length" => Operation::VecLength {
                v: self.required(position, "v")?,
            },
            "vec.from_angle" => Operation::FromAngle {
                angle: self.required(position, "angle")?,
                length: self.required(position, "length")?,
            },
            "space.wrap" => Operation::Wrap {
                v: self.required(position, "v")?,
                world: self.required(position, "world")?,
            },
            "space.delta" | "space.dist_sq" => Operation::Delta {
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
                world: self.required(position, "world")?,
                squared: def.id == "space.dist_sq",
            },
            "space.cell_index" => Operation::CellIndex {
                xy: self.required(position, "xy")?,
                valid_out: self.output_slot(id, "valid")?,
            },
            "space.cell_of" => Operation::CellOf {
                p: self.required(position, "p")?,
                xy_out: self.output_slot(id, "xy")?,
                valid_out: self.output_slot(id, "valid")?,
            },
            "space.cell_xy" => Operation::CellXyOf {
                cell: self.required(position, "cell")?,
            },
            "space.offset" => Operation::Offset {
                xy: self.required(position, "xy")?,
                by: self.required(position, "by")?,
                boundary: boundary_of(&text("boundary")),
                cell_out: self.output_slot(id, "cell")?,
                valid_out: self.output_slot(id, "valid")?,
            },
            "space.heading_octant" => Operation::HeadingOctant {
                v: self.required(position, "v")?,
            },
            "space.step_code" => Operation::StepCode {
                d: self.required(position, "d")?,
            },
            "space.step_offset" => Operation::StepOffset {
                code: self.required(position, "code")?,
            },
            "rng.choice3" => Operation::Choice3 {
                bits: self.required(position, "bits")?,
            },
            "rng.reservoir" => Operation::Reservoir {
                bits: self.required(position, "bits")?,
                count: self.required(position, "count")?,
            },
            "stream.float" => Operation::StreamFloat {
                max: self.required(position, "max")?,
                when: self.required(position, "when")?,
            },
            "grid.fold" => self.grid_fold(position, order)?,
            "agent.radius_fold" => self.radius_fold(position, order)?,
            "stat.sum" | "stat.mean" => {
                let vector = self.vector_input(position);
                self.sums.push(SumSpec {
                    slot: out,
                    wide_chunks: text("chunk_acc") != "f32",
                    mean: def.id == "stat.mean",
                    vector,
                });
                Operation::Sum {
                    value: self.required(position, "value")?,
                    sum: self.sums.len() - 1,
                }
            }
            "ctx.index" => Operation::Index,
            "ctx.tick" => Operation::Tick,
            "ctx.press_seed" => Operation::PressSeed,
            "lane.get" => Operation::LaneGet {
                lane: self.lane(position, &text("lane"))?,
            },
            "lane.set" => {
                let lane = self.lane(position, &text("lane"))?;
                self.written.insert(lane);
                Operation::LaneSet {
                    lane,
                    value: self.required(position, "value")?,
                }
            }
            "bits.shl" | "bits.shr" => Operation::Shift {
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
                left: def.id == "bits.shl",
            },
            "math.powi" => Operation::Powi {
                base: self.required(position, "base")?,
                exponent: self.required(position, "exponent")?,
                max: u32::try_from(integer("max_exponent")).unwrap_or(0),
            },
            "math.pow" => Operation::Pow {
                base: self.required(position, "base")?,
                exponent: self.required(position, "exponent")?,
            },
            "logic.select" => Operation::Select {
                cond: self.required(position, "cond")?,
                if_true: self.required(position, "if_true")?,
                if_false: self.required(position, "if_false")?,
            },
            "logic.pick" => {
                let values = config
                    .get("values")
                    .and_then(ConfigValue::as_array)
                    .unwrap_or_default()
                    .iter()
                    .map(|value| {
                        literal_of(value)
                            .and_then(|literal| HostValue::from_literal(&literal, self.document))
                            .ok_or_else(|| {
                                unsupported(self.pass, Some(id), "holds a value with no host form".to_owned())
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Operation::Pick {
                    index: self.required(position, "index")?,
                    values,
                }
            }
            "cat.switch" => {
                let category = text("category");
                let names: Vec<String> = self
                    .document
                    .category(&category)
                    .map(|known| known.variants.iter().map(|variant| variant.name.clone()).collect())
                    .unwrap_or_default();
                let mut cases = Vec::with_capacity(names.len());
                for name in names {
                    cases.push(self.input(position, &format!("case.{name}"))?);
                }
                Operation::Switch {
                    select: self.required(position, "select")?,
                    cases,
                    default: self.input(position, "default")?,
                }
            }
            "cat.transition" => self.transition(position, &config)?,
            "conv.u32_to_cat" => Operation::U32ToCat {
                a: self.required(position, "a")?,
                last: self.variant_count(&text("category")).saturating_sub(1),
            },
            "conv.cat_to_u32" => Operation::CatToU32 {
                a: self.required(position, "a")?,
            },
            "rng.threshold" => Operation::Threshold {
                p: self.required(position, "p")?,
            },
            "rng.hash" => Operation::Hash {
                x: self.required(position, "x")?,
            },
            "rng.to_float" => Operation::ToFloat {
                bits: self.required(position, "bits")?,
                max: self.required(position, "max")?,
            },
            "rng.below" => Operation::Below {
                bits: self.required(position, "bits")?,
                threshold: self.required(position, "threshold")?,
            },
            "rng.lane_seed" => Operation::LaneSeed {
                salt: read_u64(&text("salt"))
                    .or_else(|| u64::try_from(integer("salt")).ok())
                    .unwrap_or(0),
            },
            "stream.bits" => Operation::StreamBits {
                when: self.required(position, "when")?,
            },
            "region.repeat" => self.repeat(position, order)?,
            "region.put" => {
                let region = self.parents[position]
                    .map(|region| self.pass.body.nodes[region].id)
                    .ok_or_else(|| unsupported(self.pass, Some(id), "sits in no region".to_owned()))?;
                let name = text("acc");
                let (running, combine) = *self
                    .running
                    .get(&(region, name.clone()))
                    .ok_or_else(|| unsupported(self.pass, Some(id), format!("feeds no accumulator '{name}'")))?;
                Operation::Put {
                    running,
                    combine,
                    value: self.input(position, "value")?,
                    when: self.required(position, "when")?,
                }
            }
            "grid.count" => {
                let von_neumann = text("neighborhood") == "von_neumann";
                let center = config
                    .get("include_center")
                    .and_then(ConfigValue::as_bool)
                    .unwrap_or_default();
                let mut offsets = Vec::with_capacity(9);
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let is_center = dx == 0 && dy == 0;
                        let diagonal = dx != 0 && dy != 0;
                        if (is_center && !center) || (diagonal && von_neumann) {
                            continue;
                        }
                        offsets.push((dx, dy));
                    }
                }
                Operation::GridCount {
                    lane: self.lane(position, &text("lane"))?,
                    offsets,
                    value: self.required(position, "value")?,
                    when: self.required(position, "when")?,
                }
            }
            "grid.life_like" => {
                let category = text("category");
                let mask = |key: &str| {
                    config
                        .get(key)
                        .and_then(ConfigValue::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(ConfigValue::as_integer)
                        .filter(|count| (0..=9).contains(count))
                        .fold(0u16, |mask, count| mask | (1 << count))
                };
                Operation::LifeLike {
                    cell: self.required(position, "cell")?,
                    count: self.required(position, "count")?,
                    birth: mask("birth"),
                    survive: mask("survive"),
                    dead: self.variant(position, &category, &text("dead"))?,
                    alive: self.variant(position, &category, &text("alive"))?,
                }
            }
            "stat.count_where" => {
                let tally = self.tally(out);
                Operation::CountWhere {
                    pred: self.required(position, "pred")?,
                    tally,
                }
            }
            "stat.count_by" => {
                let category = text("category");
                let names: Vec<String> = self
                    .document
                    .category(&category)
                    .map(|known| known.variants.iter().map(|variant| variant.name.clone()).collect())
                    .unwrap_or_default();
                let first_tally = self.tallies;
                for name in &names {
                    let slot = self.outputs.get(&(id, name.clone())).copied().unwrap_or(usize::MAX);
                    self.tally(slot);
                }
                Operation::CountBy {
                    value: self.required(position, "value")?,
                    first_tally,
                    variants: u32::try_from(names.len()).unwrap_or(u32::MAX),
                }
            }
            "view.color" => {
                self.colors = true;
                Operation::Color {
                    index: self.required(position, "index")?,
                    len: self.palette_len(&text("palette")),
                }
            }
            "view.ramp" => Operation::Ramp {
                value: self.required(position, "value")?,
                low: self.required(position, "low")?,
                high: self.required(position, "high")?,
                len: self.palette_len(&text("palette")),
            },
            kind => {
                if let Some((form, input)) = DrawForm::of_kind(kind) {
                    self.draw_keys.push(u32::try_from(integer("key")).unwrap_or(0));
                    let input = match input {
                        Some(name) => self.required(position, name)?,
                        None => self.constant(HostValue::Bool(true)),
                    };
                    Operation::Draw {
                        form,
                        draw: self.draw_keys.len() - 1,
                        input,
                    }
                } else {
                    self.operator(position, kind)?.ok_or_else(|| {
                        unsupported(self.pass, Some(id), format!("the kind `{kind}` has no host form"))
                    })?
                }
            }
        };
        Ok(Instruction {
            node: id,
            out,
            operation,
        })
    }

    /// Returns the operation of the node at `position` of the unary, binary, comparison, logic or conversion kind
    /// `kind`, `None` for another kind.
    fn operator(&mut self, position: usize, kind: &str) -> Result<Option<Operation>, HostError> {
        Ok(Some(if let Some(op) = UnaryOp::of_kind(kind) {
            Operation::Unary {
                op,
                a: self.required(position, "a")?,
            }
        } else if let Some(op) = BinaryOp::of_kind(kind) {
            Operation::Binary {
                op,
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
            }
        } else if let Some(op) = CompareOp::of_kind(kind) {
            Operation::Compare {
                op,
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
            }
        } else if let Some(op) = LogicOp::of_kind(kind) {
            Operation::Logic {
                op,
                a: self.required(position, "a")?,
                b: if op == LogicOp::Not {
                    None
                } else {
                    Some(self.required(position, "b")?)
                },
            }
        } else if let Some(target) = Conversion::of_kind(kind) {
            Operation::Convert {
                target,
                a: self.required(position, "a")?,
            }
        } else {
            return Ok(None);
        }))
    }

    /// Returns the slot of output `port` of node `id`.
    fn output_slot(&self, id: NodeId, port: &str) -> Result<usize, HostError> {
        self.outputs
            .get(&(id, port.to_owned()))
            .copied()
            .ok_or_else(|| unsupported(self.pass, Some(id), format!("has no output `{port}`")))
    }

    /// Returns the position among the columns of the field `id`'s first layer, its number of layers and the field.
    fn field(&self, position: usize, id: &str) -> Result<(usize, u32, &'a crate::document::Field), HostError> {
        let mut first = self.document.lanes.len();
        for field in &self.document.fields {
            if field.id == id {
                return Ok((first, u32::try_from(field.layers.len()).unwrap_or(u32::MAX), field));
            }
            first += field.layers.len();
        }
        Err(unsupported(
            self.pass,
            Some(self.pass.body.nodes[position].id),
            format!("names the field '{id}'"),
        ))
    }

    /// Returns the position among the deposit columns of the first layer of the field `id`, which takes deposits.
    fn first_deposit(&self, position: usize, id: &str) -> Result<usize, HostError> {
        let mut first = 0;
        for field in self.document.fields.iter().filter(|field| !field.is_static) {
            if field.id == id {
                return Ok(first);
            }
            first += field.layers.len();
        }
        Err(unsupported(
            self.pass,
            Some(self.pass.body.nodes[position].id),
            format!("deposits into '{id}', which takes no deposits"),
        ))
    }

    /// Returns the slot of the `layer` input of the field kind at `position`, its first layer when unwritten.
    fn layer_input(&mut self, position: usize) -> Result<usize, HostError> {
        match self.input(position, "layer")? {
            Some(slot) => Ok(slot),
            None => Ok(self.constant(HostValue::Layer(0))),
        }
    }

    /// Returns the position of the counter `id`.
    fn counter(&self, position: usize, id: &str) -> Result<usize, HostError> {
        self.document
            .counters
            .iter()
            .position(|counter| counter.id == id)
            .ok_or_else(|| {
                unsupported(
                    self.pass,
                    Some(self.pass.body.nodes[position].id),
                    format!("names the counter '{id}'"),
                )
            })
    }

    /// Returns whether the `value` input of the reducer at `position` reads a `vec2f`, where a lane read tells it.
    fn vector_input(&self, position: usize) -> Option<bool> {
        let node = &self.pass.body.nodes[position];
        let Some(InputValue::Link(link)) = node.inputs.get("value") else {
            return None;
        };
        let source_node = &self.pass.body.nodes[*self.positions.get(&link.node)?];
        if source_node.op != "lane.get" {
            return None;
        }
        let id = source_node.config.get("lane").and_then(ConfigValue::as_str)?;
        let lane = self.document.lanes.iter().find(|lane| lane.id == id)?;
        Some(lane.value_type == crate::types::ValueType::Vec2F)
    }

    /// Returns the accumulators of the region at `position`, each running value given a slot.
    fn accumulators(&mut self, position: usize) -> Result<Vec<RepeatAccumulator>, HostError> {
        let node = &self.pass.body.nodes[position];
        let id = node.id;
        let declared = node
            .accumulators()
            .map_err(|reason| unsupported(self.pass, Some(id), reason))?;
        let mut accumulators = Vec::with_capacity(declared.len());
        for accumulator in &declared {
            let identity = match accumulator.combine {
                Combine::Min => extreme_of(&accumulator.value_type, true),
                Combine::Max => extreme_of(&accumulator.value_type, false),
                Combine::And => Some(HostValue::Bool(true)),
                _ => HostValue::zero_of(&accumulator.value_type),
            }
            .ok_or_else(|| {
                unsupported(
                    self.pass,
                    Some(id),
                    format!(
                        "holds the accumulator '{}' of a type with no host value",
                        accumulator.name
                    ),
                )
            })?;
            let running = self.slot();
            self.running
                .insert((id, accumulator.name.clone()), (running, accumulator.combine));
            accumulators.push(RepeatAccumulator {
                init: self.input(position, &format!("init.{}", accumulator.name))?,
                identity,
                prev: self.inner_slot(id, &format!("prev.{}", accumulator.name))?,
                running,
                out: self.output_slot(id, &accumulator.name)?,
            });
        }
        Ok(accumulators)
    }

    /// Returns the slot of the inner output `port` of the region `id`.
    fn inner_slot(&self, id: NodeId, port: &str) -> Result<usize, HostError> {
        self.inner_outputs
            .get(&(id, port.to_owned()))
            .copied()
            .ok_or_else(|| unsupported(self.pass, Some(id), format!("has no inner output `{port}`")))
    }

    /// Returns the operation of the stencil fold at `position`, its body compiled.
    fn grid_fold(
        &mut self,
        position: usize,
        order: &BTreeMap<Option<usize>, Vec<usize>>,
    ) -> Result<Operation, HostError> {
        let node = &self.pass.body.nodes[position];
        let id = node.id;
        let config = effective_config(self.defs[position], node);
        let offsets = crate::catalogue::stencil_offsets(&self.pass.body.nodes[position]);
        let boundary = config.get("boundary").and_then(ConfigValue::as_str).unwrap_or("domain");
        let accumulators = self.accumulators(position)?;
        let fold = StencilFold {
            offsets,
            boundary: boundary_of(boundary),
            at: self.input(position, "at")?,
            when: self.required(position, "when")?,
            k: self.inner_slot(id, "k")?,
            offset: self.inner_slot(id, "offset")?,
            xy: self.inner_slot(id, "xy")?,
            cell: self.inner_slot(id, "cell")?,
            diagonal: self.inner_slot(id, "diagonal")?,
            accumulators,
            body: self.block(position, order)?,
        };
        Ok(Operation::GridFold(Box::new(fold)))
    }

    /// Returns the operation of the radius fold at `position`, its body compiled.
    fn radius_fold(
        &mut self,
        position: usize,
        order: &BTreeMap<Option<usize>, Vec<usize>>,
    ) -> Result<Operation, HostError> {
        let node = &self.pass.body.nodes[position];
        let id = node.id;
        let config = effective_config(self.defs[position], node);
        let position_lane = self
            .document
            .lanes
            .iter()
            .position(|lane| lane.role == Some(crate::document::LaneRole::Position))
            .ok_or_else(|| unsupported(self.pass, Some(id), "needs a position lane".to_owned()))?;
        let accumulators = self.accumulators(position)?;
        let fold = RadiusFold {
            position_lane,
            center: self.required(position, "center")?,
            radius: self.required(position, "radius")?,
            cell_size: self.required(position, "cell_size")?,
            when: self.required(position, "when")?,
            exclude_self: config
                .get("exclude_self")
                .and_then(ConfigValue::as_bool)
                .unwrap_or(true),
            k: self.inner_slot(id, "k")?,
            other: self.inner_slot(id, "other")?,
            delta: self.inner_slot(id, "delta")?,
            dist_sq: self.inner_slot(id, "dist_sq")?,
            accumulators,
            body: self.block(position, order)?,
        };
        Ok(Operation::RadiusFold(Box::new(fold)))
    }

    /// Returns a fresh tally whose count lands in `slot`.
    fn tally(&mut self, slot: usize) -> usize {
        self.tally_slots.push(slot);
        self.tallies += 1;
        self.tallies - 1
    }

    /// Returns the operation of the `cat.transition` at `position`.
    fn transition(&mut self, position: usize, config: &BTreeMap<String, ConfigValue>) -> Result<Operation, HostError> {
        let node = &self.pass.body.nodes[position];
        let id = node.id;
        let category = config
            .get("category")
            .and_then(ConfigValue::as_str)
            .unwrap_or_default()
            .to_owned();
        let rule_ids = transition_rule_ids(node);
        let mut fired = Vec::with_capacity(rule_ids.len());
        for rule in &rule_ids {
            let slot = self
                .outputs
                .get(&(id, format!("fired.{rule}")))
                .copied()
                .unwrap_or(usize::MAX);
            fired.push(slot);
        }
        let mut rules = Vec::new();
        for rule in config.get("rules").and_then(ConfigValue::as_array).unwrap_or_default() {
            let Some(table) = rule.as_table() else { continue };
            let field = |key: &str| table.get(key).and_then(ConfigValue::as_str).map(str::to_owned);
            let rule_id = field("id").unwrap_or_default();
            let from = self.variant(position, &category, &field("from").unwrap_or_default())?;
            let to = match field("to") {
                Some(name) => Some(self.variant(position, &category, &name)?),
                None => None,
            };
            rules.push(TransitionRule {
                from,
                to,
                to_input: self.input(position, &format!("rule.{rule_id}.to"))?,
                when: self.required(position, &format!("rule.{rule_id}.when"))?,
                phase: table.get("phase").and_then(ConfigValue::as_integer).unwrap_or_default(),
                fired: rule_ids
                    .iter()
                    .position(|known| *known == rule_id)
                    .unwrap_or(usize::MAX),
            });
        }
        Ok(Operation::Transition {
            state: self.required(position, "state")?,
            rules,
            fired,
        })
    }

    /// Returns the operation of the Repeat at `position`, its body compiled.
    fn repeat(&mut self, position: usize, order: &BTreeMap<Option<usize>, Vec<usize>>) -> Result<Operation, HostError> {
        let node = &self.pass.body.nodes[position];
        let id = node.id;
        let config = effective_config(self.defs[position], node);
        let max = config
            .get("max")
            .and_then(ConfigValue::as_integer)
            .and_then(|max| u32::try_from(max).ok())
            .unwrap_or(0);
        let accumulators = self.accumulators(position)?;
        let repeat = Repeat {
            max,
            count: self.required(position, "count")?,
            when: self.required(position, "when")?,
            k: self.inner_slot(id, "k")?,
            total: self.inner_slot(id, "total")?,
            accumulators,
            body: self.block(position, order)?,
        };
        Ok(Operation::Repeat(Box::new(repeat)))
    }
}
