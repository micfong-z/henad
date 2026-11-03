//! Compilation of a pass into a program of the host evaluator: one instruction per node, in an order every input
//! precedes, with every value in a numbered slot of one frame.
//!
//! A *slot* holds one value of the frame an element evaluates in. A *block* is a list of instructions. The top of a
//! pass splits into three blocks: the *prelude* of values that are the same for every element, run once, the
//! *element block*, run per element, and the *post block* of values computed from reducers, run once after the
//! elements. A region's body is a block of its own, run per iteration. A pure value of a body that reads no iteration
//! and no element runs in the prelude instead.

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
    /// Each float sum, by sum number.
    pub sums: Vec<SumSpec>,
    /// Positions of the lanes and field layers the pass writes, in the order of [`super::HostLanes`].
    pub written: Vec<usize>,
    /// Positions among the deposit columns of the layers the pass deposits into.
    pub deposited: Vec<usize>,
    /// Whether the pass sets a colour per element.
    pub colors: bool,
    /// Outputs of the prelude and the post block, by node and port, with their slots.
    pub uniform_outputs: Vec<(NodeId, String, usize)>,
    /// Slot of every output by node and port.
    pub outputs: BTreeMap<(NodeId, String), usize>,
    /// Slot of every inner output of a region by node and port.
    pub inner_outputs: BTreeMap<(NodeId, String), usize>,
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
    /// The bits as they are.
    Bits,
    /// `bits % 3 - 1`.
    Choice3,
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

/// One float sum of `stat.sum` or `stat.mean`.
#[derive(Debug, Clone)]
pub(crate) struct SumSpec {
    /// Slot the result lands in after the elements.
    pub slot: usize,
    /// Whether each chunk sums in `f64`, where it otherwise sums in `f32`.
    pub wide_chunks: bool,
    /// Whether the result is the mean, the sum times `1 / max(n, 1)`.
    pub mean: bool,
    /// Whether the summed values are `vec2f`, `None` when only the values tell, which a sum over no element reads as
    /// a scalar.
    pub vector: Option<bool>,
}

/// A stencil fold and its body.
#[derive(Debug, Clone)]
pub(crate) struct StencilFold {
    /// Stencil in table order.
    pub offsets: Vec<(i32, i32)>,
    /// Whether a position past the edge of the domain grid is skipped, `None` for the domain grid's own boundary.
    pub bounded: Option<bool>,
    /// Slot of `at`, `None` for the element's own cell.
    pub at: Option<usize>,
    /// Slot of `when`.
    pub when: usize,
    /// Slots of the inner outputs `k`, `offset`, `xy`, `cell` and `diagonal`.
    pub inner: [usize; 5],
    /// Accumulators in `config.acc` order.
    pub accumulators: Vec<RepeatAccumulator>,
    /// Body, run per position.
    pub body: Vec<Instruction>,
}

/// A radius fold and its body.
#[derive(Debug, Clone)]
pub(crate) struct RadiusFold {
    /// Position of the position lane among the lanes.
    pub position_lane: usize,
    /// Slots of `center`, `radius`, `cell_size` and `when`.
    pub center: usize,
    pub radius: usize,
    pub cell_size: usize,
    pub when: usize,
    /// Whether the agent itself is left out.
    pub exclude_self: bool,
    /// Slots of the inner outputs `k`, `other`, `delta` and `dist_sq`.
    pub inner: [usize; 4],
    /// Accumulators in `config.acc` order.
    pub accumulators: Vec<RepeatAccumulator>,
    /// Body, run per agent visited.
    pub body: Vec<Instruction>,
}

/// What an instruction does. Every operand is a slot.
#[derive(Debug, Clone)]
pub(crate) enum Operation {
    World,
    Grid,
    CellXy,
    LaneGetAt {
        lane: usize,
        at: usize,
    },
    FieldGet {
        first: usize,
        layers: u32,
        layer: usize,
        at: Option<usize>,
        zero: HostValue,
    },
    FieldSet {
        first: usize,
        layers: u32,
        layer: usize,
        value: usize,
    },
    Deposit {
        first: usize,
        layers: u32,
        layer: usize,
        at: usize,
        value: usize,
    },
    CounterAdd {
        counter: usize,
        amount: usize,
    },
    CounterTotal {
        counter: usize,
    },
    Clamp {
        x: usize,
        lo: usize,
        hi: usize,
    },
    Midpoint {
        a: usize,
        b: usize,
    },
    VecMake {
        x: usize,
        y: usize,
    },
    VecSplit {
        v: usize,
        y_out: usize,
    },
    VecScale {
        v: usize,
        s: usize,
    },
    VecDot {
        a: usize,
        b: usize,
    },
    VecLength {
        v: usize,
    },
    FromAngle {
        angle: usize,
        length: usize,
    },
    Wrap {
        v: usize,
        world: usize,
    },
    Delta {
        a: usize,
        b: usize,
        world: usize,
        squared: bool,
    },
    CellIndex {
        xy: usize,
        valid_out: usize,
    },
    CellOf {
        p: usize,
        xy_out: usize,
        valid_out: usize,
    },
    CellXyOf {
        cell: usize,
    },
    Offset {
        xy: usize,
        by: usize,
        bounded: Option<bool>,
        cell_out: usize,
        valid_out: usize,
    },
    HeadingOctant {
        v: usize,
    },
    StepCode {
        d: usize,
    },
    StepOffset {
        code: usize,
    },
    Choice3 {
        bits: usize,
    },
    Reservoir {
        bits: usize,
        count: usize,
    },
    StreamFloat {
        max: usize,
        when: usize,
    },
    GridFold(Box<StencilFold>),
    RadiusFold(Box<RadiusFold>),
    Sum {
        value: usize,
        sum: usize,
    },
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

impl Operation {
    /// Returns the slots the operation reads, its region body included.
    #[expect(clippy::too_many_lines, reason = "one arm per operation reads best as one table")]
    pub(crate) fn operands(&self) -> Vec<usize> {
        match self {
            Self::Index
            | Self::Tick
            | Self::PressSeed
            | Self::LaneGet { .. }
            | Self::LaneSeed { .. }
            | Self::World
            | Self::Grid
            | Self::CellXy
            | Self::CounterTotal { .. } => Vec::new(),
            Self::FieldGet { layer, at, .. } => std::iter::once(*layer).chain(*at).collect(),
            Self::Deposit { layer, at, value, .. } => vec![*layer, *at, *value],
            Self::Clamp { x, lo, hi } => vec![*x, *lo, *hi],
            Self::Midpoint { a, b }
            | Self::VecDot { a, b }
            | Self::FieldSet { layer: a, value: b, .. }
            | Self::Reservoir { bits: a, count: b }
            | Self::Offset { xy: a, by: b, .. }
            | Self::Wrap { v: a, world: b }
            | Self::VecScale { v: a, s: b }
            | Self::FromAngle { angle: a, length: b }
            | Self::StreamFloat { max: a, when: b }
            | Self::VecMake { x: a, y: b }
            | Self::Binary { a, b, .. }
            | Self::Shift { a, b, .. }
            | Self::Compare { a, b, .. } => vec![*a, *b],
            Self::Delta { a, b, world, .. } => vec![*a, *b, *world],
            Self::LaneGetAt { at: a, .. }
            | Self::CounterAdd { amount: a, .. }
            | Self::VecSplit { v: a, .. }
            | Self::VecLength { v: a }
            | Self::HeadingOctant { v: a }
            | Self::CellIndex { xy: a, .. }
            | Self::CellOf { p: a, .. }
            | Self::CellXyOf { cell: a }
            | Self::StepCode { d: a }
            | Self::StepOffset { code: a }
            | Self::Choice3 { bits: a }
            | Self::Sum { value: a, .. } => vec![*a],
            Self::GridFold(fold) => {
                let mut found: Vec<usize> = fold.at.into_iter().chain([fold.when]).collect();
                found.extend(fold.accumulators.iter().filter_map(|accumulator| accumulator.init));
                found.extend(
                    fold.body
                        .iter()
                        .flat_map(|instruction| instruction.operation.operands()),
                );
                found
            }
            Self::RadiusFold(fold) => {
                let mut found = vec![fold.center, fold.radius, fold.cell_size, fold.when];
                found.extend(fold.accumulators.iter().filter_map(|accumulator| accumulator.init));
                found.extend(
                    fold.body
                        .iter()
                        .flat_map(|instruction| instruction.operation.operands()),
                );
                found
            }
            Self::LaneSet { value, .. } | Self::CountBy { value, .. } => vec![*value],
            Self::Unary { a, .. } | Self::Convert { a, .. } | Self::U32ToCat { a, .. } | Self::CatToU32 { a } => {
                vec![*a]
            }
            Self::Powi { base, exponent, .. } | Self::Pow { base, exponent } => vec![*base, *exponent],
            Self::Logic { a, b, .. } => std::iter::once(*a).chain(*b).collect(),
            Self::Select {
                cond,
                if_true,
                if_false,
            } => vec![*cond, *if_true, *if_false],
            Self::Pick { index, .. } | Self::Color { index, .. } => vec![*index],
            Self::Switch { select, cases, default } => std::iter::once(*select)
                .chain(cases.iter().flatten().copied())
                .chain(*default)
                .collect(),
            Self::Transition { state, rules, .. } => std::iter::once(*state)
                .chain(
                    rules
                        .iter()
                        .flat_map(|rule| std::iter::once(rule.when).chain(rule.to_input)),
                )
                .collect(),
            Self::Draw { input, .. } => vec![*input],
            Self::Threshold { p } => vec![*p],
            Self::Hash { x } => vec![*x],
            Self::ToFloat { bits, max } => vec![*bits, *max],
            Self::Below { bits, threshold } => vec![*bits, *threshold],
            Self::StreamBits { when } => vec![*when],
            Self::Repeat(repeat) => {
                let mut found = vec![repeat.count, repeat.when];
                found.extend(repeat.accumulators.iter().filter_map(|accumulator| accumulator.init));
                found.extend(
                    repeat
                        .body
                        .iter()
                        .flat_map(|instruction| instruction.operation.operands()),
                );
                found
            }
            Self::Put { value, when, .. } => std::iter::once(*when).chain(*value).collect(),
            Self::GridCount { value, when, .. } => vec![*value, *when],
            Self::LifeLike { cell, count, .. } => vec![*cell, *count],
            Self::CountWhere { pred, .. } => vec![*pred],
            Self::Ramp { value, low, high, .. } => vec![*value, *low, *high],
        }
    }

    /// Returns whether the operation computes a value from its operands and the run's context alone, the same for
    /// every element and every iteration when its operands are, and cannot fault in a checked file.
    ///
    /// A draw is not one, since it reads the iteration word of the region around it.
    fn is_pure(&self) -> bool {
        matches!(
            self,
            Self::PressSeed
                | Self::LaneSeed { .. }
                | Self::Binary { .. }
                | Self::Shift { .. }
                | Self::Unary { .. }
                | Self::Powi { .. }
                | Self::Pow { .. }
                | Self::Compare { .. }
                | Self::Logic { .. }
                | Self::Select { .. }
                | Self::Pick { .. }
                | Self::Switch { .. }
                | Self::Convert { .. }
                | Self::U32ToCat { .. }
                | Self::CatToU32 { .. }
                | Self::Threshold { .. }
                | Self::Hash { .. }
                | Self::ToFloat { .. }
                | Self::Below { .. }
                | Self::World
                | Self::Grid
                | Self::Clamp { .. }
                | Self::Midpoint { .. }
                | Self::VecMake { .. }
                | Self::VecScale { .. }
                | Self::VecDot { .. }
                | Self::VecLength { .. }
                | Self::FromAngle { .. }
                | Self::Wrap { .. }
                | Self::Delta { .. }
                | Self::HeadingOctant { .. }
                | Self::StepCode { .. }
                | Self::StepOffset { .. }
                | Self::Choice3 { .. }
                | Self::Reservoir { .. }
        )
    }

    /// Returns the body of a region's operation for writing.
    fn body_mut(&mut self) -> Option<&mut Vec<Instruction>> {
        match self {
            Self::Repeat(repeat) => Some(&mut repeat.body),
            Self::GridFold(fold) => Some(&mut fold.body),
            Self::RadiusFold(fold) => Some(&mut fold.body),
            _ => None,
        }
    }
}

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
        sums: Vec::new(),
        written: BTreeSet::new(),
        deposited: BTreeSet::new(),
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
        sums: Vec::new(),
        written: Vec::new(),
        deposited: Vec::new(),
        colors: false,
        uniform_outputs: Vec::new(),
        outputs: BTreeMap::new(),
        inner_outputs: BTreeMap::new(),
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
    program.sums = compiler.sums;
    program.written = compiler.written.into_iter().collect();
    program.deposited = compiler.deposited.into_iter().collect();
    program.colors = compiler.colors;
    program.outputs = compiler.outputs;
    program.inner_outputs = compiler.inner_outputs;
    hoist_uniform_body_values(&mut program);
    Ok(program)
}

/// Returns whether a stencil's boundary word skips positions past the edge, `None` for the domain grid's own.
fn boundary_of(word: &str) -> Option<bool> {
    match word {
        "torus" => Some(false),
        "bounded" => Some(true),
        _ => None,
    }
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
                }
            }
            "counter.add" => Operation::CounterAdd {
                counter: self.counter(position, &text("counter"))?,
                amount: self.required(position, "amount")?,
            },
            "stat.counter" => Operation::CounterTotal {
                counter: self.counter(position, &text("counter"))?,
            },
            "math.neg" | "math.floor" | "math.sqrt" | "math.recip" | "math.log2" | "math.log10" => Operation::Unary {
                op: match def.id {
                    "math.neg" => UnaryOp::Neg,
                    "math.floor" => UnaryOp::Floor,
                    "math.sqrt" => UnaryOp::Sqrt,
                    "math.recip" => UnaryOp::Recip,
                    "math.log2" => UnaryOp::Log2,
                    _ => UnaryOp::Log10,
                },
                a: self.required(position, "a")?,
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
            "conv.to_f64" | "conv.to_vec2f" | "conv.to_vec2i" | "conv.to_vec2u" => Operation::Convert {
                target: match def.id {
                    "conv.to_f64" => Conversion::ToF64,
                    "conv.to_vec2f" => Conversion::ToVec2F,
                    "conv.to_vec2i" => Conversion::ToVec2I,
                    _ => Conversion::ToVec2U,
                },
                a: self.required(position, "a")?,
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
                bounded: boundary_of(&text("boundary")),
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
            "rand.bits" | "rand.choice3" => {
                self.draw_keys.push(u32::try_from(integer("key")).unwrap_or(0));
                let always = self.constant(HostValue::Bool(true));
                Operation::Draw {
                    form: if def.id == "rand.bits" {
                        DrawForm::Bits
                    } else {
                        DrawForm::Choice3
                    },
                    draw: self.draw_keys.len() - 1,
                    input: always,
                }
            }
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
            bounded: boundary_of(boundary),
            at: self.input(position, "at")?,
            when: self.required(position, "when")?,
            inner: [
                self.inner_slot(id, "k")?,
                self.inner_slot(id, "offset")?,
                self.inner_slot(id, "xy")?,
                self.inner_slot(id, "cell")?,
                self.inner_slot(id, "diagonal")?,
            ],
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
            inner: [
                self.inner_slot(id, "k")?,
                self.inner_slot(id, "other")?,
                self.inner_slot(id, "delta")?,
                self.inner_slot(id, "dist_sq")?,
            ],
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
