//! Program of the host evaluator: a pass compiled to one instruction per node, with every value in a numbered slot of
//! one frame. The host evaluator runs a program, and the WGSL compiler writes a shader from one.
//!
//! A *slot* holds one value of the frame an element evaluates in. A *block* is a list of instructions. The top of a
//! pass splits into three blocks: the *prelude* of values that are the same for every element, run once, the
//! *element block*, run per element, and the *post block* of values computed from reducers, run once after the
//! elements. A region's body is a block of its own, run per iteration.

use std::collections::BTreeMap;

use henad_core::authoring::primitives::space::Boundary;

use super::value::{BinaryOp, Conversion, HostValue, UnaryOp};
use crate::document::{Domain, FieldCombine};
use crate::ids::NodeId;
use crate::node::Combine;
use crate::value::PortRef;

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
    /// Returns the port and slot of `output`, the node's only output when it names no port.
    pub(crate) fn port_slot(&self, output: &PortRef) -> Option<(String, usize)> {
        if let Some(port) = &output.port {
            return self
                .outputs
                .get(&(output.node, port.clone()))
                .map(|slot| (port.clone(), *slot));
        }
        let mut ports = self.outputs.iter().filter(|((node, _), _)| *node == output.node);
        let only = ports.next().map(|((_, port), slot)| (port.clone(), *slot));
        if ports.next().is_some() { None } else { only }
    }

    /// Returns the slot of output `port` of node `node`.
    #[cfg(test)]
    pub fn output_slot(&self, node: NodeId, port: &str) -> Option<usize> {
        let output = PortRef {
            node,
            port: Some(port.to_owned()),
        };
        self.port_slot(&output).map(|(_, slot)| slot)
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

impl CompareOp {
    /// Returns the comparison of the comparison kind `kind`, `None` for another kind.
    pub(crate) fn of_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            "cmp.eq" => Self::Eq,
            "cmp.ne" => Self::Ne,
            "cmp.lt" => Self::Lt,
            "cmp.le" => Self::Le,
            "cmp.gt" => Self::Gt,
            "cmp.ge" => Self::Ge,
            _ => return None,
        })
    }
}

/// Operation of a logic kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicOp {
    And,
    Or,
    Xor,
    Not,
}

impl LogicOp {
    /// Returns the operation of the logic kind `kind`, `None` for another kind.
    pub(crate) fn of_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            "logic.and" => Self::And,
            "logic.or" => Self::Or,
            "logic.xor" => Self::Xor,
            "logic.not" => Self::Not,
            _ => return None,
        })
    }
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

impl DrawForm {
    /// Returns the form of the counter-based draw kind `kind` and the input it reads besides its bits, `None` for
    /// another kind.
    pub(crate) fn of_kind(kind: &str) -> Option<(Self, Option<&'static str>)> {
        Some(match kind {
            "rand.float" => (Self::Float, Some("max")),
            "rand.below" => (Self::Below, Some("threshold")),
            "rand.chance" => (Self::Chance, Some("p")),
            "rand.bits" => (Self::Bits, None),
            "rand.choice3" => (Self::Choice3, None),
            _ => return None,
        })
    }
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
    /// Boundary of the domain grid the stencil steps on, `None` for the domain grid's own.
    pub boundary: Option<Boundary>,
    /// Slot of `at`, `None` for the element's own cell.
    pub at: Option<usize>,
    /// Slot of `when`.
    pub when: usize,
    /// Slot of the inner output `k`.
    pub k: usize,
    /// Slot of the inner output `offset`.
    pub offset: usize,
    /// Slot of the inner output `xy`.
    pub xy: usize,
    /// Slot of the inner output `cell`.
    pub cell: usize,
    /// Slot of the inner output `diagonal`.
    pub diagonal: usize,
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
    /// Slot of the inner output `k`.
    pub k: usize,
    /// Slot of the inner output `other`.
    pub other: usize,
    /// Slot of the inner output `delta`.
    pub delta: usize,
    /// Slot of the inner output `dist_sq`.
    pub dist_sq: usize,
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
        combine: FieldCombine,
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
        boundary: Option<Boundary>,
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
    pub(super) fn is_pure(&self) -> bool {
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
    pub(super) fn body_mut(&mut self) -> Option<&mut Vec<Instruction>> {
        match self {
            Self::Repeat(repeat) => Some(&mut repeat.body),
            Self::GridFold(fold) => Some(&mut fold.body),
            Self::RadiusFold(fold) => Some(&mut fold.body),
            _ => None,
        }
    }
}
