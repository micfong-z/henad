//! Compilation of a pass into a program of the host evaluator: one instruction per node, in an order every input
//! precedes, with every value in a numbered slot of one frame.
//!
//! A *slot* holds one value of the frame an element evaluates in. A *block* is a list of instructions. The top of a
//! pass splits into three blocks: the *prelude* of values that are the same for every element, run once, the
//! *element block*, run per element, and the *post block* of values computed from reducers, run once after the
//! elements. A region's body is a block of its own, run per iteration.

use std::collections::{BTreeMap, BTreeSet};

use super::HostError;
use super::value::{BinaryOp, Conversion, HostValue, UnaryOp, extreme_of};
use crate::catalogue::{Effect, NodeDef, OutputRate, effective_config, lookup, transition_rule_ids};
use crate::check::literal_of;
use crate::document::{Domain, GraphDocument, Pass};
use crate::ids::NodeId;
use crate::node::{Combine, Node};
use crate::reader::read_u64;
use crate::types::Rate;
use crate::value::{ConfigValue, InputValue, PortRef};

/// A pass compiled for the host evaluator.
#[derive(Debug, Clone)]
pub(crate) struct Program {
    /// Id of the pass.
    pub pass: String,
    /// Elements the pass runs over.
    pub domain: Option<Domain>,
    /// Number of slots of the frame.
    pub slots: usize,
    /// Slots that hold a constant, with its value.
    pub constants: Vec<(usize, HostValue)>,
    /// Slots that hold a parameter, with the parameter's position in the document.
    pub params: Vec<(usize, usize)>,
    /// Node key of each counter-based draw, by draw number.
    pub draw_keys: Vec<u32>,
    /// Instructions run once before the elements.
    pub prelude: Vec<Instruction>,
    /// Instructions run per element.
    pub element: Vec<Instruction>,
    /// Instructions run once after the elements.
    pub post: Vec<Instruction>,
    /// Number of reducer tallies.
    pub tallies: usize,
    /// Slot each tally lands in after the elements, by tally number.
    pub tally_slots: Vec<usize>,
    /// Positions of the lanes the pass writes, in document order.
    pub written: Vec<usize>,
    /// Whether the pass sets a colour per element.
    pub colors: bool,
    /// Outputs of the prelude and the post block, by node and port, with their slots.
    pub uniform_outputs: Vec<(NodeId, String, usize)>,
    /// Slot of every output by node and port.
    pub outputs: BTreeMap<(NodeId, String), usize>,
}

impl Program {
    /// Returns the slot of output `port` of node `node`.
    #[cfg(test)]
    pub fn output_slot(&self, node: NodeId, port: &str) -> Option<usize> {
        self.outputs.get(&(node, port.to_owned())).copied()
    }
}

/// One node of a program.
#[derive(Debug, Clone)]
pub(crate) struct Instruction {
    /// Node the instruction comes from.
    pub node: NodeId,
    /// Slot of the node's first output. Unused for a node without one.
    pub out: usize,
    /// What the node does.
    pub operation: Operation,
}

/// Comparison of a comparison kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Operation of a logic kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicOp {
    And,
    Or,
    Xor,
    Not,
}

/// What a counter-based draw makes of its bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawForm {
    /// `random_float(bits, max)`.
    Float,
    /// `bits < threshold`.
    Below,
    /// `random_float(bits, 1.0) < p`.
    Chance,
}

/// One rule of a `cat.transition`.
#[derive(Debug, Clone)]
pub(crate) struct TransitionRule {
    /// Variant the rule leaves.
    pub from: u32,
    /// Variant the rule enters, unless `to_input` is set.
    pub to: Option<u32>,
    /// Slot of the `rule.<id>.to` input, when written.
    pub to_input: Option<usize>,
    /// Slot of the `rule.<id>.when` input.
    pub when: usize,
    /// Phase of the rule.
    pub phase: i64,
    /// Position of the rule's id among the node's `fired.<id>` outputs.
    pub fired: usize,
}

/// One accumulator of a Repeat.
#[derive(Debug, Clone)]
pub(crate) struct RepeatAccumulator {
    /// Slot of the `init.<acc>` input, when written.
    pub init: Option<usize>,
    /// Value before the first iteration when `init` is unwritten.
    pub identity: HostValue,
    /// Slot of the inner output `prev.<acc>`.
    pub prev: usize,
    /// Slot of the value the iterations change.
    pub running: usize,
    /// Slot of the output `<acc>`.
    pub out: usize,
}

/// A Repeat and its body.
#[derive(Debug, Clone)]
pub(crate) struct Repeat {
    /// Iteration bound `max`.
    pub max: u32,
    /// Slot of `count`.
    pub count: usize,
    /// Slot of `when`.
    pub when: usize,
    /// Slot of the inner output `k`.
    pub k: usize,
    /// Slot of the inner output `total`.
    pub total: usize,
    /// Accumulators in `config.acc` order.
    pub accumulators: Vec<RepeatAccumulator>,
    /// Body, run per iteration.
    pub body: Vec<Instruction>,
}

/// What an instruction does. Every operand is a slot.
#[derive(Debug, Clone)]
pub(crate) enum Operation {
    Index,
    Tick,
    PressSeed,
    LaneGet {
        lane: usize,
    },
    LaneSet {
        lane: usize,
        value: usize,
    },
    Binary {
        op: BinaryOp,
        a: usize,
        b: usize,
    },
    Shift {
        a: usize,
        b: usize,
        left: bool,
    },
    Unary {
        op: UnaryOp,
        a: usize,
    },
    Powi {
        base: usize,
        exponent: usize,
        max: u32,
    },
    Pow {
        base: usize,
        exponent: usize,
    },
    Compare {
        op: CompareOp,
        a: usize,
        b: usize,
    },
    Logic {
        op: LogicOp,
        a: usize,
        b: Option<usize>,
    },
    Select {
        cond: usize,
        if_true: usize,
        if_false: usize,
    },
    Pick {
        index: usize,
        values: Vec<HostValue>,
    },
    Switch {
        select: usize,
        cases: Vec<Option<usize>>,
        default: Option<usize>,
    },
    Transition {
        state: usize,
        rules: Vec<TransitionRule>,
        fired: Vec<usize>,
    },
    Convert {
        target: Conversion,
        a: usize,
    },
    U32ToCat {
        a: usize,
        last: u32,
    },
    CatToU32 {
        a: usize,
    },
    Draw {
        form: DrawForm,
        draw: usize,
        input: usize,
    },
    Threshold {
        p: usize,
    },
    Hash {
        x: usize,
    },
    ToFloat {
        bits: usize,
        max: usize,
    },
    Below {
        bits: usize,
        threshold: usize,
    },
    LaneSeed {
        salt: u64,
    },
    StreamBits {
        when: usize,
    },
    Repeat(Box<Repeat>),
    Put {
        running: usize,
        combine: Combine,
        value: Option<usize>,
        when: usize,
    },
    GridCount {
        lane: usize,
        offsets: Vec<(i64, i64)>,
        value: usize,
        when: usize,
    },
    LifeLike {
        cell: usize,
        count: usize,
        birth: u16,
        survive: u16,
        dead: u32,
        alive: u32,
    },
    CountWhere {
        pred: usize,
        tally: usize,
    },
    CountBy {
        value: usize,
        first_tally: usize,
        variants: u32,
    },
    Color {
        index: usize,
        len: u32,
    },
    Ramp {
        value: usize,
        low: usize,
        high: usize,
        len: u32,
    },
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
    written: BTreeSet<usize>,
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
    let parents = nodes.iter().map(|node| positions.get(&node.region?).copied()).collect();
    let mut compiler = Compiler {
        document,
        pass,
        positions,
        defs,
        parents,
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
        written: BTreeSet::new(),
        colors: false,
    };
    compiler.assign_output_slots();
    let order = compiler.order()?;
    let top = order.get(&None).cloned().unwrap_or_default();
    let timings = compiler.timings(&top)?;
    let mut program = Program {
        pass: pass.id.clone(),
        domain: pass.domain,
        slots: 0,
        constants: Vec::new(),
        params: Vec::new(),
        draw_keys: Vec::new(),
        prelude: Vec::new(),
        element: Vec::new(),
        post: Vec::new(),
        tallies: 0,
        tally_slots: Vec::new(),
        written: Vec::new(),
        colors: false,
        uniform_outputs: Vec::new(),
        outputs: BTreeMap::new(),
    };
    for (&position, timing) in top.iter().zip(&timings) {
        let instruction = compiler.instruction(position, &order)?;
        let node = &nodes[position];
        if *timing != Timing::Element || compiler.defs[position].has_effect(Effect::Reduce) {
            for (key, slot) in &compiler.outputs {
                if key.0 == node.id {
                    program.uniform_outputs.push((node.id, key.1.clone(), *slot));
                }
            }
        }
        match timing {
            Timing::Prelude => program.prelude.push(instruction),
            Timing::Element => program.element.push(instruction),
            Timing::Post => program.post.push(instruction),
        }
    }
    program.slots = compiler.slots;
    program.constants = compiler.constants;
    program.params = compiler.params.into_iter().map(|(param, slot)| (slot, param)).collect();
    program.draw_keys = compiler.draw_keys;
    program.tallies = compiler.tallies;
    program.tally_slots = compiler.tally_slots;
    program.written = compiler.written.into_iter().collect();
    program.colors = compiler.colors;
    program.outputs = compiler.outputs;
    Ok(program)
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

impl Compiler<'_> {
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

    /// Returns the regions around the node at `position`, innermost first.
    fn ancestors(&self, position: usize) -> Vec<usize> {
        let mut path = Vec::new();
        let mut current = self.parents[position];
        while let Some(region) = current {
            if region == position || path.contains(&region) {
                break;
            }
            path.push(region);
            current = self.parents[region];
        }
        path
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
        let ancestors: Vec<Vec<usize>> = (0..nodes.len()).map(|position| self.ancestors(position)).collect();
        for (position, node) in nodes.iter().enumerate() {
            for link in links(node) {
                let Some(&source) = self.positions.get(&link.node) else {
                    continue;
                };
                if let Some(edge) = self.scope_edge(&ancestors, source, position) {
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
    fn scope_edge(
        &self,
        ancestors: &[Vec<usize>],
        source: usize,
        target: usize,
    ) -> Option<(Option<usize>, usize, usize)> {
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
            let own = if per_element { Timing::Element } else { Timing::Prelude };
            let mut timing = own;
            let mut reads_element = own == Timing::Element;
            let mut reads_post = false;
            for link in self.body_links(position) {
                let Some(&source) = self.positions.get(&link.node) else {
                    continue;
                };
                if self.parents[source].is_some() || source == position {
                    continue;
                }
                let source_node = nodes[source].id;
                let source_reduces = self.defs[source].has_effect(Effect::Reduce);
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
                if inner != position && self.ancestors(inner).contains(&position) {
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
        if self.ancestors(reader).contains(&source) {
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
            None => match self.defs[position].input_default(name) {
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
                    .params
                    .iter()
                    .position(|param| param.id() == id)
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
            .categories
            .iter()
            .find(|known| known.id == category)
            .and_then(|known| known.variants.iter().position(|variant| variant.name == name))
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
            .categories
            .iter()
            .find(|known| known.id == category)
            .map_or(0, |known| u32::try_from(known.variants.len()).unwrap_or(u32::MAX))
    }

    /// Returns the position of the lane `id`.
    fn lane(&self, position: usize, id: &str) -> Result<usize, HostError> {
        self.document
            .lanes
            .iter()
            .position(|lane| lane.id == id)
            .ok_or_else(|| {
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
            "math.add" | "math.sub" | "math.mul" | "math.div" | "math.rem" | "math.min" | "math.max" | "bits.xor"
            | "bits.and" | "bits.or" => {
                let op = match def.id {
                    "math.add" => BinaryOp::Add,
                    "math.sub" => BinaryOp::Sub,
                    "math.mul" => BinaryOp::Mul,
                    "math.div" => BinaryOp::Div,
                    "math.rem" => BinaryOp::Rem,
                    "math.min" => BinaryOp::Min,
                    "math.max" => BinaryOp::Max,
                    "bits.xor" => BinaryOp::Xor,
                    "bits.and" => BinaryOp::And,
                    _ => BinaryOp::Or,
                };
                Operation::Binary {
                    op,
                    a: self.required(position, "a")?,
                    b: self.required(position, "b")?,
                }
            }
            "bits.shl" | "bits.shr" => Operation::Shift {
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
                left: def.id == "bits.shl",
            },
            "bits.not" | "math.abs" | "math.ceil" | "math.trunc" | "math.round" | "math.exp" | "math.log"
            | "math.sin" | "math.cos" => Operation::Unary {
                op: match def.id {
                    "bits.not" => UnaryOp::Not,
                    "math.abs" => UnaryOp::Abs,
                    "math.ceil" => UnaryOp::Ceil,
                    "math.trunc" => UnaryOp::Trunc,
                    "math.round" => UnaryOp::Round,
                    "math.exp" => UnaryOp::Exp,
                    "math.log" => UnaryOp::Log,
                    "math.sin" => UnaryOp::Sin,
                    _ => UnaryOp::Cos,
                },
                a: self.required(position, "a")?,
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
            "cmp.eq" | "cmp.ne" | "cmp.lt" | "cmp.le" | "cmp.gt" | "cmp.ge" => Operation::Compare {
                op: match def.id {
                    "cmp.eq" => CompareOp::Eq,
                    "cmp.ne" => CompareOp::Ne,
                    "cmp.lt" => CompareOp::Lt,
                    "cmp.le" => CompareOp::Le,
                    "cmp.gt" => CompareOp::Gt,
                    _ => CompareOp::Ge,
                },
                a: self.required(position, "a")?,
                b: self.required(position, "b")?,
            },
            "logic.and" | "logic.or" | "logic.xor" | "logic.not" => Operation::Logic {
                op: match def.id {
                    "logic.and" => LogicOp::And,
                    "logic.or" => LogicOp::Or,
                    "logic.xor" => LogicOp::Xor,
                    _ => LogicOp::Not,
                },
                a: self.required(position, "a")?,
                b: if def.id == "logic.not" {
                    None
                } else {
                    Some(self.required(position, "b")?)
                },
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
                    .categories
                    .iter()
                    .find(|known| known.id == category)
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
            "conv.to_f32" | "conv.to_u32" | "conv.to_i32" => Operation::Convert {
                target: match def.id {
                    "conv.to_f32" => Conversion::ToF32,
                    "conv.to_u32" => Conversion::ToU32,
                    _ => Conversion::ToI32,
                },
                a: self.required(position, "a")?,
            },
            "conv.u32_to_cat" => Operation::U32ToCat {
                a: self.required(position, "a")?,
                last: self.variant_count(&text("category")).saturating_sub(1),
            },
            "conv.cat_to_u32" => Operation::CatToU32 {
                a: self.required(position, "a")?,
            },
            "rand.float" | "rand.below" | "rand.chance" => {
                let (form, input) = match def.id {
                    "rand.float" => (DrawForm::Float, "max"),
                    "rand.below" => (DrawForm::Below, "threshold"),
                    _ => (DrawForm::Chance, "p"),
                };
                self.draw_keys.push(u32::try_from(integer("key")).unwrap_or(0));
                Operation::Draw {
                    form,
                    draw: self.draw_keys.len() - 1,
                    input: self.required(position, input)?,
                }
            }
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
                    .categories
                    .iter()
                    .find(|known| known.id == category)
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
            other => {
                return Err(unsupported(
                    self.pass,
                    Some(id),
                    format!("the kind `{other}` has no host form"),
                ));
            }
        };
        Ok(Instruction {
            node: id,
            out,
            operation,
        })
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
        let declared = node
            .accumulators()
            .map_err(|reason| unsupported(self.pass, Some(id), reason))?;
        let inner = |compiler: &Self, port: &str| {
            compiler
                .inner_outputs
                .get(&(id, port.to_owned()))
                .copied()
                .ok_or_else(|| unsupported(compiler.pass, Some(id), format!("has no inner output `{port}`")))
        };
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
                prev: inner(self, &format!("prev.{}", accumulator.name))?,
                running,
                out: self
                    .outputs
                    .get(&(id, accumulator.name.clone()))
                    .copied()
                    .ok_or_else(|| unsupported(self.pass, Some(id), format!("has no output `{}`", accumulator.name)))?,
            });
        }
        let repeat = Repeat {
            max,
            count: self.required(position, "count")?,
            when: self.required(position, "when")?,
            k: inner(self, "k")?,
            total: inner(self, "total")?,
            accumulators,
            body: self.block(position, order)?,
        };
        Ok(Operation::Repeat(Box::new(repeat)))
    }
}
