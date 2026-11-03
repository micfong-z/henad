//! Emission of one generated shader: the engine-owned template of its role around the body of each pass it runs.
//!
//! The body of a pass is its host program, the instructions the host evaluator runs, written as WGSL. Every value is a
//! `let` named `n<id>_<port>`, with each `.` of a dotted port written `__`. The prelude values the host can evaluate at
//! build come in through the uniform block, and the prelude values that read the tick are computed in the shader.

use std::collections::{BTreeMap, BTreeSet};

use henad_compute::gpu::{BufferSide, ShaderBinding, SideTable};
use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamKind;

use super::uniform::{FieldShape, FieldSource, UniformBlock, WgslType};
use super::{NodeRange, WORKGROUP_SIZE};
use crate::catalogue::effective_config;
use crate::check::CheckReport;
use crate::document::{GraphDocument, Pass, PassEnabled};
use crate::host::HostEvaluator;
use crate::host::compile::{CompareOp, DrawForm, Instruction, LogicOp, Operation, Program, Repeat};
use crate::host::value::{BinaryOp, Conversion, HostValue, UnaryOp};
use crate::ids::NodeId;
use crate::node::Combine;
use crate::value::{Color, ConfigValue};

/// Role of a generated shader in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// A step pass. `writes_tick` holds for the last step pass, which advances the tick.
    Step { writes_tick: bool },
    /// The display pass, one invocation per texel.
    Display,
    /// The reduce pass, one invocation per cell.
    Reduce,
    /// A pass of the action at this position of the document's actions.
    Action,
}

/// How a lane is held on the device.
#[derive(Debug, Clone)]
pub(crate) struct LanePlan {
    /// Id of the lane.
    pub id: String,
    /// Type a shader holds the lane's values in, `None` for a lane only the host holds.
    pub wgsl: Option<WgslType>,
}

/// Declarations every shader of one graph shares.
pub(crate) struct EmitInput<'a> {
    pub document: &'a GraphDocument,
    pub report: &'a CheckReport,
    pub evaluator: &'a HostEvaluator,
    pub lanes: &'a [LanePlan],
    /// Whether the buffers hold the one-word tick buffer, after the lanes.
    pub tick_buffer: bool,
    /// Iteration word of a region around the top of every pass, for a test that probes a draw inside one.
    pub outer_iteration: Option<u32>,
}

/// One shader written out.
#[derive(Debug, Clone)]
pub(crate) struct Emitted {
    /// Text of the shader before composition.
    pub text: String,
    /// Byte ranges of the text that each node wrote.
    pub ranges: Vec<NodeRange>,
    /// Bindings in `@binding` order.
    pub bindings: Vec<ShaderBinding>,
    /// Uniform block bound as `params`.
    pub uniform: UniformBlock,
    /// Number of counters of a reduce shader, at least 1.
    pub counters: usize,
}

/// Text with the byte ranges each node wrote in it.
#[derive(Default)]
struct Text {
    text: String,
    ranges: Vec<NodeRange>,
}

impl Text {
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// Appends `other`, moving its ranges by the length of the text so far.
    fn append(&mut self, other: Self) {
        let shift = self.text.len();
        self.text.push_str(&other.text);
        self.ranges.extend(other.ranges.into_iter().map(|mut range| {
            range.range = range.range.start + shift..range.range.end + shift;
            range
        }));
    }

    /// Appends `line` as a statement of `node` in `pass`.
    fn node_line(&mut self, pass: &str, node: NodeId, line: &str) {
        let start = self.text.len();
        self.text.push_str(line);
        self.ranges.push(NodeRange {
            range: start..self.text.len(),
            pass: pass.to_owned(),
            node,
        });
    }
}

/// Name a slot has in a shader.
#[derive(Debug, Clone)]
enum SlotName {
    /// A `let` or `var` of this name.
    Value(String),
    /// A constant written in place.
    Literal(String),
    /// A constant with no WGSL form.
    HostOnly,
    /// A slot no value fills.
    Unnamed,
}

/// Returns the WGSL name of output `port` of node `node`: `n<id>_<port>`, each `.` written `__`.
///
/// A port segment starts with a letter and holds no `__`, so a run of two underscores marks the dot and a run of
/// three a segment that ends in `_` before it. Two values never share a name.
pub(crate) fn value_name(node: NodeId, port: &str) -> String {
    format!("n{}_{}", node.number(), port.replace('.', "__"))
}

/// Returns the WGSL literal of `value`, or `None` for a value only the host holds.
pub(crate) fn literal(value: HostValue) -> Option<String> {
    let float = |number: f32| {
        if number == 0.0 && number.is_sign_negative() {
            "-0.0f".to_owned()
        } else if number == 0.0 || number.is_normal() {
            format!("{number:?}f")
        } else {
            format!("bitcast<f32>({:#010x}u)", number.to_bits())
        }
    };
    let signed = |number: i32| {
        if number < 0 {
            format!("i32({number})")
        } else {
            format!("{number}i")
        }
    };
    Some(match value {
        HostValue::Bool(flag) => flag.to_string(),
        HostValue::U32(number)
        | HostValue::Category(number)
        | HostValue::Layer(number)
        | HostValue::Cell(number)
        | HostValue::Agent(number) => format!("{number}u"),
        HostValue::I32(number) => signed(number),
        HostValue::F32(number) => float(number),
        HostValue::Vec2U([x, y]) => format!("vec2<u32>({x}u, {y}u)"),
        HostValue::Vec2I([x, y]) => format!("vec2<i32>({}, {})", signed(x), signed(y)),
        HostValue::Vec2F([x, y]) => format!("vec2<f32>({}, {})", float(x), float(y)),
        HostValue::U64(_) | HostValue::F64(_) | HostValue::Vec2D(_) => return None,
    })
}

/// Returns the slots `instruction` fills.
fn defined(instruction: &Instruction) -> Vec<usize> {
    let mut slots = vec![instruction.out];
    if let Operation::Transition { fired, .. } = &instruction.operation {
        slots.extend(fired.iter().copied());
    }
    slots
}

/// Shared modules and helpers a shader uses.
#[derive(Default)]
struct Uses {
    pcg_hash: bool,
    random_float: bool,
    draw_core: bool,
    neighbors: bool,
    threshold: bool,
}

/// State of the emission of one shader.
struct Emitter<'a> {
    input: &'a EmitInput<'a>,
    role: Role,
    sides: &'a SideTable,
    /// Whether the shader runs more than one pass, whose uniform members then take a prefix.
    shared: bool,
    /// Lanes some pass of the shader writes.
    writes: BTreeSet<usize>,
    reads: BTreeSet<usize>,
    reads_tick: bool,
    uses: Uses,
    /// Palettes the shader colours from, by constant name.
    palettes: BTreeMap<String, Vec<Color>>,
    fields: Vec<(String, FieldShape, FieldSource)>,
    problems: Vec<CodeDiagnostic>,
    /// Counters before the current pass's tallies.
    tally_offset: usize,
}

/// State of the emission of one pass's body.
struct Block<'a> {
    /// Position of the pass among the shader's passes.
    k: usize,
    position: usize,
    pass: &'a Pass,
    names: Vec<SlotName>,
    types: Vec<Option<WgslType>>,
    /// Slots whose values the host evaluates, by the name of their uniform member and its source.
    host: BTreeMap<usize, (FieldShape, FieldSource)>,
    /// Host slots the body reads.
    used_host: BTreeSet<usize>,
    /// Node each slot belongs to.
    owners: BTreeMap<usize, NodeId>,
    /// Iteration word of the innermost region being written.
    iteration: String,
}

/// Returns the shader of `role` that runs the passes at `positions` of the document, each bound through `sides`.
///
/// # Errors
///
/// Returns a diagnostic for each node the shader cannot run, with its pass and node.
pub(crate) fn emit(
    input: &EmitInput<'_>,
    role: Role,
    positions: &[usize],
    sides: &SideTable,
) -> Result<Emitted, Vec<CodeDiagnostic>> {
    let mut emitter = Emitter {
        input,
        role,
        sides,
        shared: positions.len() > 1,
        writes: BTreeSet::new(),
        reads: BTreeSet::new(),
        reads_tick: matches!(role, Role::Step { writes_tick: true }),
        uses: Uses::default(),
        palettes: BTreeMap::new(),
        fields: vec![
            (
                "henad_width".to_owned(),
                FieldShape::Value(WgslType::U32),
                FieldSource::Width,
            ),
            (
                "henad_height".to_owned(),
                FieldShape::Value(WgslType::U32),
                FieldSource::Height,
            ),
        ],
        problems: Vec::new(),
        tally_offset: 0,
    };
    let mut programs = Vec::with_capacity(positions.len());
    for &position in positions {
        let pass = &input.document.passes[position];
        match input.evaluator.program_at(position) {
            Some(Ok(program)) => {
                emitter.writes.extend(program.written.iter().copied());
                programs.push((position, pass, program));
            }
            Some(Err(error)) => emitter.problems.extend(error.diagnostics()),
            None => {}
        }
    }
    let mut body = Text::default();
    for (k, (position, pass, program)) in programs.into_iter().enumerate() {
        let block = emitter.block(k, position, pass, program);
        body.append(block);
    }
    if role == Role::Display && positions.is_empty() {
        body.push(&emitter.default_view());
    }
    if !emitter.problems.is_empty() {
        return Err(emitter.problems);
    }
    Ok(emitter.assemble(body))
}

impl Emitter<'_> {
    /// Records a problem of `node` in `pass`.
    fn refuse(&mut self, pass: &str, node: Option<NodeId>, message: impl Into<String>) {
        let mut problem = CodeDiagnostic::error(message.into()).with_pass(pass);
        if let Some(node) = node {
            problem = problem.with_node(node.to_string());
        }
        self.problems.push(problem);
    }

    /// Returns the name of uniform member `base` of the pass at position `k` among the shader's passes.
    fn field_name(&self, k: usize, base: &str) -> String {
        if self.shared {
            format!("henad_p{k}_{base}")
        } else {
            base.to_owned()
        }
    }

    /// Adds the uniform member `name` unless the block has it.
    fn field(&mut self, name: &str, shape: FieldShape, source: FieldSource) {
        if !self.fields.iter().any(|(known, ..)| known == name) {
            self.fields.push((name.to_owned(), shape, source));
        }
    }

    /// Returns the binding name a lane's reads use, and the one its writes use.
    fn lane_bindings(&self, lane: usize) -> (String, String) {
        let id = &self.input.lanes[lane].id;
        let base = format!("lane_{id}_cells");
        let row = self.sides.rows().get(lane).copied();
        let writes = self.writes.contains(&lane);
        if writes && row.is_some_and(|row| row.write == BufferSide::Next) {
            (format!("{base}_in"), format!("{base}_out"))
        } else {
            (base.clone(), base)
        }
    }

    /// Returns the expression that loads `lane` at the element `index`.
    fn load(&mut self, lane: usize, index: &str) -> String {
        self.reads.insert(lane);
        let (read, _) = self.lane_bindings(lane);
        match self.input.lanes[lane].wgsl {
            Some(WgslType::Bool) => format!("({read}[{index}] != 0u)"),
            _ => format!("{read}[{index}]"),
        }
    }

    /// Returns the body of one pass, in a block of its own.
    #[expect(
        clippy::too_many_lines,
        reason = "the naming, the uniform members and the body of one pass read in order"
    )]
    fn block(&mut self, k: usize, position: usize, pass: &Pass, program: &Program) -> Text {
        let report = self.input.report.passes.get(position);
        let mut names = vec![SlotName::Unnamed; program.slots];
        let mut types: Vec<Option<WgslType>> = vec![None; program.slots];
        let mut owners = BTreeMap::new();
        for ((node, port), slot) in program.outputs.iter().chain(&program.inner_outputs) {
            names[*slot] = SlotName::Value(value_name(*node, port));
            owners.insert(*slot, *node);
            let port_type = report.and_then(|report| {
                let inner = || report.inner_outputs.get(node)?.get(port);
                report
                    .outputs
                    .get(node)
                    .and_then(|ports| ports.get(port))
                    .or_else(inner)
            });
            types[*slot] = port_type
                .and_then(|port_type| port_type.value_type.as_ref())
                .and_then(WgslType::of);
        }
        for (slot, value) in &program.constants {
            names[*slot] = literal(*value).map_or(SlotName::HostOnly, SlotName::Literal);
            types[*slot] = WgslType::of_value(*value);
        }
        let mut host = BTreeMap::new();
        for (slot, param) in &program.params {
            let descriptor = &self.input.document.params[*param];
            let wgsl = match descriptor.kind() {
                ParamKind::F32 { .. } => WgslType::F32,
                ParamKind::Bool { .. } => WgslType::Bool,
                _ => WgslType::U32,
            };
            names[*slot] = SlotName::Value(format!("henad_param_{}", descriptor.id()));
            types[*slot] = Some(wgsl);
            host.insert(
                *slot,
                (FieldShape::Value(wgsl), FieldSource::Slot { position, slot: *slot }),
            );
        }
        name_running(&program.element, &mut names, &mut types);
        // A prelude value that reads the tick changes within a submission, and the shader computes it.
        let mut on_device: BTreeSet<usize> = BTreeSet::new();
        let mut device_prelude = Vec::new();
        for instruction in &program.prelude {
            let reads_device = matches!(instruction.operation, Operation::Tick)
                || instruction
                    .operation
                    .operands()
                    .iter()
                    .any(|slot| on_device.contains(slot));
            if reads_device {
                on_device.extend(defined(instruction));
                device_prelude.push(instruction);
            } else {
                for slot in defined(instruction) {
                    if let Some(wgsl) = types.get(slot).copied().flatten() {
                        host.insert(slot, (FieldShape::Value(wgsl), FieldSource::Slot { position, slot }));
                    }
                }
            }
        }
        let mut block = Block {
            k,
            position,
            pass,
            names,
            types,
            host,
            used_host: BTreeSet::new(),
            owners,
            iteration: self
                .input
                .outer_iteration
                .map_or_else(|| "0u".to_owned(), |word| format!("{word}u")),
        };
        if !program.post.is_empty() && self.role != Role::Reduce {
            self.refuse(
                &pass.id,
                program.post.first().map(|instruction| instruction.node),
                "computes a value from a reducer outside a stats pass, which the GPU cannot run",
            );
        }
        let indent = "        ";
        let mut inner = Text::default();
        for instruction in &device_prelude {
            self.instruction(&mut block, instruction, indent, &mut inner);
        }
        let written: Vec<usize> = program.written.clone();
        for &lane in &written {
            let Some(wgsl) = self.input.lanes.get(lane).and_then(|plan| plan.wgsl) else {
                self.refuse(
                    &pass.id,
                    None,
                    format!(
                        "writes the lane '{}', whose type the GPU cannot hold",
                        self.input.lanes[lane].id
                    ),
                );
                continue;
            };
            let load = self.load(lane, "henad_index");
            let id = &self.input.lanes[lane].id;
            inner.push(&format!("{indent}var henad_write_{id}: {} = {load};\n", wgsl.name()));
        }
        for instruction in &program.element {
            self.instruction(&mut block, instruction, indent, &mut inner);
        }
        for &lane in &written {
            let Some(wgsl) = self.input.lanes.get(lane).and_then(|plan| plan.wgsl) else {
                continue;
            };
            let (_, write) = self.lane_bindings(lane);
            let id = &self.input.lanes[lane].id;
            let value = if wgsl == WgslType::Bool {
                format!("u32(henad_write_{id})")
            } else {
                format!("henad_write_{id}")
            };
            inner.push(&format!("{indent}{write}[henad_index] = {value};\n"));
        }
        self.tally_offset += program.tallies;
        let mut text = Text::default();
        let enabled = match &pass.enabled {
            PassEnabled::Param(_) => {
                let name = self.field_name(k, "henad_enabled");
                self.field(
                    &name,
                    FieldShape::Value(WgslType::U32),
                    FieldSource::Enabled { position },
                );
                Some(name)
            }
            PassEnabled::Literal(_) => None,
        };
        text.text.push_str(&format!("    // Pass '{}'.\n", pass.id));
        match &enabled {
            Some(name) => text.push(&format!("    if (params.{name} != 0u) {{\n")),
            None => text.push("    {\n"),
        }
        for slot in block.used_host.clone() {
            let (shape, source) = block.host[&slot].clone();
            let SlotName::Value(value) = &block.names[slot] else {
                continue;
            };
            let member = self.field_name(k, value);
            self.field(&member, shape, source);
            let read = if shape == FieldShape::Value(WgslType::Bool) {
                format!("params.{member} != 0u")
            } else {
                format!("params.{member}")
            };
            let line = format!("{indent}let {value} = {read};\n");
            match block.owners.get(&slot) {
                Some(node) => text.node_line(&pass.id, *node, &line),
                None => text.push(&line),
            }
        }
        text.append(inner);
        text.push("    }\n");
        text
    }

    /// Returns the expression of `slot`, and records a host slot the body reads.
    fn slot(&mut self, block: &mut Block<'_>, slot: usize, node: NodeId) -> String {
        if block.host.contains_key(&slot) {
            block.used_host.insert(slot);
        }
        match block.names.get(slot) {
            Some(SlotName::Value(name)) => name.clone(),
            Some(SlotName::Literal(text)) => text.clone(),
            Some(SlotName::HostOnly) => {
                self.refuse(
                    &block.pass.id,
                    Some(node),
                    "reads a constant only the host holds, such as a u64 or an f64, in a pass the GPU runs",
                );
                "0u".to_owned()
            }
            _ => {
                self.refuse(&block.pass.id, Some(node), "reads a value no node computes");
                "0u".to_owned()
            }
        }
    }

    /// Returns the type of `slot`.
    fn type_of(block: &Block<'_>, slot: usize) -> Option<WgslType> {
        block.types.get(slot).copied().flatten()
    }

    /// Returns the name of `slot`, which an instruction fills.
    fn target(&mut self, block: &Block<'_>, slot: usize, node: NodeId) -> String {
        if let Some(SlotName::Value(name)) = block.names.get(slot) {
            name.clone()
        } else {
            self.refuse(&block.pass.id, Some(node), "has an output the shader cannot name");
            format!("henad_unnamed_{slot}")
        }
    }

    /// Writes `instruction` at `indent` into `text`.
    #[expect(clippy::too_many_lines, reason = "one arm per operation reads best as one table")]
    fn instruction(&mut self, block: &mut Block<'_>, instruction: &Instruction, indent: &str, text: &mut Text) {
        let node = instruction.node;
        let pass_id = block.pass.id.clone();
        let mut lines = String::new();
        let out = instruction.out;
        macro_rules! let_out {
            ($expr:expr) => {{
                let expr = $expr;
                let name = self.target(block, out, node);
                lines.push_str(&format!("{indent}let {name} = {expr};\n"));
            }};
        }
        match &instruction.operation {
            Operation::Index => let_out!("henad_index".to_owned()),
            Operation::Tick => {
                if !self.input.tick_buffer {
                    self.refuse(
                        &pass_id,
                        Some(node),
                        "reads the tick, and the compiler planned no tick buffer",
                    );
                }
                self.reads_tick = true;
                let_out!("henad_tick".to_owned());
            }
            Operation::PressSeed | Operation::LaneSeed { .. } => {
                // Both are prelude values the host evaluates, and a shader never computes them.
                self.refuse(&pass_id, Some(node), "is a value the host computes, met in a shader");
            }
            Operation::StreamBits { .. } => {
                self.refuse(
                    &pass_id,
                    Some(node),
                    "draws from the serial stream, which only an init pass on the host can",
                );
            }
            Operation::LaneGet { lane } => {
                let load = self.load(*lane, "henad_index");
                let_out!(load);
            }
            Operation::LaneSet { lane, value } => {
                let value = self.slot(block, *value, node);
                let id = self.input.lanes[*lane].id.clone();
                lines.push_str(&format!("{indent}henad_write_{id} = {value};\n"));
            }
            Operation::Binary { op, a, b } => {
                let (a, b) = (self.slot(block, *a, node), self.slot(block, *b, node));
                let_out!(match op {
                    BinaryOp::Add => format!("{a} + {b}"),
                    BinaryOp::Sub => format!("{a} - {b}"),
                    BinaryOp::Mul => format!("{a} * {b}"),
                    BinaryOp::Div => format!("{a} / {b}"),
                    BinaryOp::Rem => format!("{a} % {b}"),
                    BinaryOp::Min => format!("min({a}, {b})"),
                    BinaryOp::Max => format!("max({a}, {b})"),
                    BinaryOp::Xor => format!("{a} ^ {b}"),
                    BinaryOp::And => format!("{a} & {b}"),
                    BinaryOp::Or => format!("{a} | {b}"),
                });
            }
            Operation::Shift { a, b, left } => {
                let (a, b) = (self.slot(block, *a, node), self.slot(block, *b, node));
                let shift = if *left { "<<" } else { ">>" };
                let_out!(format!("{a} {shift} ({b} & 31u)"));
            }
            Operation::Unary { op, a } => {
                let a = self.slot(block, *a, node);
                let_out!(match op {
                    UnaryOp::Abs => format!("abs({a})"),
                    UnaryOp::Ceil => format!("ceil({a})"),
                    UnaryOp::Trunc => format!("trunc({a})"),
                    UnaryOp::Round => format!("round({a})"),
                    UnaryOp::Exp => format!("exp({a})"),
                    UnaryOp::Log => format!("log({a})"),
                    UnaryOp::Sin => format!("sin({a})"),
                    UnaryOp::Cos => format!("cos({a})"),
                    UnaryOp::Not => format!("~{a}"),
                    UnaryOp::Neg => format!("-({a})"),
                    UnaryOp::Floor => format!("floor({a})"),
                    UnaryOp::Sqrt => format!("sqrt({a})"),
                    UnaryOp::Recip => format!("1.0f / ({a})"),
                    UnaryOp::Log2 => format!("log2({a})"),
                    UnaryOp::Log10 => format!("log2({a}) * 0.30102999566398120f"),
                });
            }
            Operation::Powi { base, exponent, max } => {
                if !block.host.contains_key(base) && !matches!(block.names.get(*base), Some(SlotName::Literal(_))) {
                    self.refuse(
                        &pass_id,
                        Some(node),
                        "raises a base the host cannot compute at build, and the shader reads a table of its powers",
                    );
                }
                let exponent = self.slot(block, *exponent, node);
                let member = self.field_name(block.k, &format!("henad_powers_n{}", node.number()));
                self.field(
                    &member,
                    FieldShape::Table {
                        len: max.saturating_add(1),
                    },
                    FieldSource::Table {
                        position: block.position,
                        base: *base,
                        max: *max,
                    },
                );
                let entry = format!("henad_power_n{}", node.number());
                lines.push_str(&format!("{indent}let {entry} = min({exponent}, {max}u);\n"));
                let_out!(format!("params.{member}[{entry} >> 2u][{entry} & 3u]"));
            }
            Operation::Pow { base, exponent } => {
                let (base, exponent) = (self.slot(block, *base, node), self.slot(block, *exponent, node));
                let_out!(format!("pow({base}, {exponent})"));
            }
            Operation::Compare { op, a, b } => {
                let wgsl = Self::type_of(block, *a);
                let (a, b) = (self.slot(block, *a, node), self.slot(block, *b, node));
                let vector = wgsl.is_some_and(WgslType::is_vector);
                let ordered = wgsl.is_some_and(WgslType::is_ordered);
                let_out!(match op {
                    CompareOp::Eq if vector => format!("all({a} == {b})"),
                    CompareOp::Ne if vector => format!("any({a} != {b})"),
                    CompareOp::Eq => format!("{a} == {b}"),
                    CompareOp::Ne => format!("{a} != {b}"),
                    // The host orders no two values of a type without an order, and every comparison of them is false.
                    _ if !ordered => "false".to_owned(),
                    CompareOp::Lt => format!("{a} < {b}"),
                    CompareOp::Le => format!("{a} <= {b}"),
                    CompareOp::Gt => format!("{a} > {b}"),
                    CompareOp::Ge => format!("{a} >= {b}"),
                });
            }
            Operation::Logic { op, a, b } => {
                let a = self.slot(block, *a, node);
                let b = b.map(|b| self.slot(block, b, node)).unwrap_or_default();
                let_out!(match op {
                    LogicOp::And => format!("{a} && {b}"),
                    LogicOp::Or => format!("{a} || {b}"),
                    LogicOp::Xor => format!("{a} != {b}"),
                    LogicOp::Not => format!("!{a}"),
                });
            }
            Operation::Select {
                cond,
                if_true,
                if_false,
            } => {
                let cond = self.slot(block, *cond, node);
                let if_true = self.slot(block, *if_true, node);
                let if_false = self.slot(block, *if_false, node);
                let_out!(format!("select({if_false}, {if_true}, {cond})"));
            }
            Operation::Pick { index, values } => {
                let index = self.slot(block, *index, node);
                let entries: Option<Vec<String>> = values.iter().map(|value| literal(*value)).collect();
                let wgsl = values.first().copied().and_then(WgslType::of_value);
                match (entries, wgsl) {
                    (Some(entries), Some(wgsl)) if !entries.is_empty() => {
                        let table = format!("henad_values_n{}", node.number());
                        lines.push_str(&format!(
                            "{indent}var {table} = array<{}, {}>({});\n",
                            wgsl.name(),
                            entries.len(),
                            entries.join(", ")
                        ));
                        let_out!(format!("{table}[min({index}, {}u)]", entries.len() - 1));
                    }
                    _ => self.refuse(&pass_id, Some(node), "picks from values the GPU cannot hold"),
                }
            }
            Operation::Switch { select, cases, default } => {
                let select = self.slot(block, *select, node);
                let wgsl = Self::type_of(block, out);
                let fallback = default.or_else(|| cases.iter().flatten().next().copied());
                let fallback = match fallback {
                    Some(slot) => self.slot(block, slot, node),
                    None => "0u".to_owned(),
                };
                let name = self.target(block, out, node);
                let type_name = wgsl.map_or("u32", WgslType::name);
                lines.push_str(&format!("{indent}var {name}: {type_name} = {fallback};\n"));
                lines.push_str(&format!("{indent}switch ({select}) {{\n"));
                for (variant, case) in cases.iter().enumerate() {
                    if let Some(case) = case {
                        let value = self.slot(block, *case, node);
                        lines.push_str(&format!("{indent}    case {variant}u: {{ {name} = {value}; }}\n"));
                    }
                }
                lines.push_str(&format!("{indent}    default: {{}}\n"));
                lines.push_str(&format!("{indent}}}\n"));
            }
            Operation::Transition { state, rules, fired } => {
                let state = self.slot(block, *state, node);
                let name = self.target(block, out, node);
                lines.push_str(&format!("{indent}var {name}: u32 = {state};\n"));
                let fired_names: Vec<String> = fired.iter().map(|slot| self.target(block, *slot, node)).collect();
                for fired_name in &fired_names {
                    lines.push_str(&format!("{indent}var {fired_name} = false;\n"));
                }
                let mut phases: Vec<i64> = rules.iter().map(|rule| rule.phase).collect();
                phases.sort_unstable();
                phases.dedup();
                for phase in phases {
                    let start = format!("henad_start_n{}", node.number());
                    lines.push_str(&format!("{indent}{{\n"));
                    lines.push_str(&format!("{indent}    let {start} = {name};\n"));
                    let mut first = true;
                    for rule in rules.iter().filter(|rule| rule.phase == phase) {
                        let when = self.slot(block, rule.when, node);
                        let to = match (rule.to_input, rule.to) {
                            (Some(slot), _) => self.slot(block, slot, node),
                            (None, Some(variant)) => format!("{variant}u"),
                            (None, None) => {
                                self.refuse(&pass_id, Some(node), "has a rule with no target");
                                "0u".to_owned()
                            }
                        };
                        let keyword = if first { "if" } else { "} else if" };
                        first = false;
                        lines.push_str(&format!(
                            "{indent}    {keyword} ({start} == {}u && {when}) {{\n",
                            rule.from
                        ));
                        lines.push_str(&format!("{indent}        {name} = {to};\n"));
                        if let Some(fired_name) = fired_names.get(rule.fired) {
                            lines.push_str(&format!("{indent}        {fired_name} = true;\n"));
                        }
                    }
                    if !first {
                        lines.push_str(&format!("{indent}    }}\n"));
                    }
                    lines.push_str(&format!("{indent}}}\n"));
                }
            }
            Operation::Convert { target, a } => {
                let source = Self::type_of(block, *a);
                let a = self.slot(block, *a, node);
                let expression = match (target, source) {
                    (Conversion::ToF32, Some(WgslType::U32 | WgslType::I32 | WgslType::Bool)) => format!("f32({a})"),
                    (Conversion::ToU32, Some(WgslType::F32 | WgslType::Bool)) => format!("u32({a})"),
                    (Conversion::ToU32, Some(WgslType::I32)) => format!("bitcast<u32>({a})"),
                    (Conversion::ToI32, Some(WgslType::F32 | WgslType::Bool)) => format!("i32({a})"),
                    (Conversion::ToI32, Some(WgslType::U32)) => format!("bitcast<i32>({a})"),
                    _ => {
                        self.refuse(&pass_id, Some(node), "converts a value the GPU cannot hold");
                        "0u".to_owned()
                    }
                };
                let_out!(expression);
            }
            Operation::U32ToCat { a, last } => {
                let a = self.slot(block, *a, node);
                let_out!(format!("min({a}, {last}u)"));
            }
            Operation::CatToU32 { a } => {
                let a = self.slot(block, *a, node);
                let_out!(a);
            }
            Operation::Draw { form, draw, input } => {
                let input = self.slot(block, *input, node);
                let key = self.field_name(block.k, &format!("henad_key_n{}", node.number()));
                self.field(
                    &key,
                    FieldShape::Value(WgslType::U32),
                    FieldSource::Key {
                        position: block.position,
                        draw: *draw,
                    },
                );
                let tick = if self.role == Role::Action {
                    let press = self.field_name(block.k, "henad_press");
                    self.field(&press, FieldShape::Value(WgslType::U32), FieldSource::Press);
                    format!("params.{press}")
                } else {
                    self.reads_tick = true;
                    "henad_tick".to_owned()
                };
                self.uses.draw_core = true;
                let bits = format!("draw_core(params.{key}, henad_index, {tick}, {}, 0u)", block.iteration);
                let expression = match form {
                    DrawForm::Float => {
                        self.uses.random_float = true;
                        format!("random_float({bits}, {input})")
                    }
                    DrawForm::Below => format!("{bits} < {input}"),
                    DrawForm::Bits => bits,
                    DrawForm::Choice3 => format!("i32(({bits}) % 3u) - 1i"),
                    DrawForm::Chance => {
                        self.uses.random_float = true;
                        format!("random_float({bits}, 1.0f) < {input}")
                    }
                };
                let_out!(expression);
            }
            Operation::Threshold { p } => {
                let p = self.slot(block, *p, node);
                self.uses.threshold = true;
                let_out!(format!("henad_threshold({p})"));
            }
            Operation::Hash { x } => {
                let x = self.slot(block, *x, node);
                self.uses.pcg_hash = true;
                let_out!(format!("pcg_hash({x})"));
            }
            Operation::ToFloat { bits, max } => {
                let (bits, max) = (self.slot(block, *bits, node), self.slot(block, *max, node));
                self.uses.random_float = true;
                let_out!(format!("random_float({bits}, {max})"));
            }
            Operation::Below { bits, threshold } => {
                let (bits, threshold) = (self.slot(block, *bits, node), self.slot(block, *threshold, node));
                let_out!(format!("{bits} < {threshold}"));
            }
            Operation::Repeat(repeat) => {
                text.push(&lines);
                lines.clear();
                self.repeat(block, node, repeat, indent, text);
            }
            Operation::Put {
                running,
                combine,
                value,
                when,
            } => {
                let when = self.slot(block, *when, node);
                let value = match value {
                    Some(slot) => self.slot(block, *slot, node),
                    None => "1u".to_owned(),
                };
                let running = self.target(block, *running, node);
                let combined = match combine {
                    Combine::Sum | Combine::Count => format!("{running} + {value}"),
                    Combine::Min => format!("min({running}, {value})"),
                    Combine::Max => format!("max({running}, {value})"),
                    Combine::Or => format!("{running} || {value}"),
                    Combine::And => format!("{running} && {value}"),
                    Combine::Carry => value,
                };
                lines.push_str(&format!("{indent}if ({when}) {{ {running} = {combined}; }}\n"));
            }
            Operation::GridCount {
                lane,
                offsets,
                value,
                when,
            } => {
                let compared = self.input.lanes.get(*lane).and_then(|plan| plan.wgsl);
                let value = self.slot(block, *value, node);
                let when = self.slot(block, *when, node);
                let name = self.target(block, out, node);
                self.uses.neighbors = true;
                lines.push_str(&format!("{indent}var {name} = 0u;\n"));
                lines.push_str(&format!("{indent}if ({when}) {{\n"));
                for (k, (dx, dy)) in offsets.iter().enumerate() {
                    let at = format!("henad_at_{k}");
                    let load = self.load(*lane, &format!("u32({at}.y) * params.henad_width + u32({at}.x)"));
                    let equal = if compared.is_some_and(WgslType::is_vector) {
                        format!("all({load} == {value})")
                    } else {
                        format!("{load} == {value}")
                    };
                    lines.push_str(&format!("{indent}    let {at} = henad_at({dx}, {dy});\n"));
                    lines.push_str(&format!(
                        "{indent}    if ({at}.z != 0 && {equal}) {{ {name} = {name} + 1u; }}\n"
                    ));
                }
                lines.push_str(&format!("{indent}}}\n"));
            }
            Operation::LifeLike {
                cell,
                count,
                birth,
                survive,
                dead,
                alive,
            } => {
                let (cell, count) = (self.slot(block, *cell, node), self.slot(block, *count, node));
                let mask = format!("henad_mask_n{}", node.number());
                lines.push_str(&format!(
                    "{indent}let {mask} = select({birth}u, {survive}u, {cell} == {alive}u);\n"
                ));
                let_out!(format!(
                    "select({dead}u, {alive}u, {count} <= 9u && ((({mask} >> min({count}, 9u)) & 1u) != 0u))"
                ));
            }
            Operation::CountWhere { pred, tally } => {
                if self.role != Role::Reduce {
                    self.refuse(&pass_id, Some(node), "counts outside a stats pass");
                }
                let pred = self.slot(block, *pred, node);
                let counter = self.tally_offset + tally;
                lines.push_str(&format!(
                    "{indent}if ({pred}) {{ atomicAdd(&henad_partial[{counter}], 1u); }}\n"
                ));
            }
            Operation::CountBy {
                value,
                first_tally,
                variants,
            } => {
                if self.role != Role::Reduce {
                    self.refuse(&pass_id, Some(node), "counts outside a stats pass");
                }
                let value = self.slot(block, *value, node);
                let first = self.tally_offset + first_tally;
                lines.push_str(&format!(
                    "{indent}if ({value} < {variants}u) {{ atomicAdd(&henad_partial[{first}u + {value}], 1u); }}\n"
                ));
            }
            Operation::Color { index, len } => {
                if self.role != Role::Display {
                    self.refuse(&pass_id, Some(node), "colours outside a view pass");
                }
                let index = self.slot(block, *index, node);
                let palette = self.palette_of(block, node);
                match palette {
                    Some(constant) if *len > 0 => {
                        let colors = format!("henad_colors_n{}", node.number());
                        lines.push_str(&format!("{indent}var {colors} = {constant};\n"));
                        lines.push_str(&format!(
                            "{indent}henad_color = {colors}[min({index}, {}u)];\n",
                            len - 1
                        ));
                    }
                    _ => self.refuse(&pass_id, Some(node), "colours from an empty or unknown palette"),
                }
            }
            Operation::Ramp { value, low, high, len } => {
                let value = self.slot(block, *value, node);
                let low = self.slot(block, *low, node);
                let high = self.slot(block, *high, node);
                let last = len.saturating_sub(1);
                let_out!(format!(
                    "select(0u, min(u32(max(floor(({value} - {low}) / ({high} - {low}) * {}f), 0.0f)), {last}u), {high} > {low})",
                    *len as f32
                ));
            }
            _ => self.refuse(&pass_id, Some(node), "has no WGSL form in this build"),
        }
        if !lines.is_empty() {
            text.node_line(&pass_id, node, &lines);
        }
    }

    /// Writes a Repeat and its body as a counted loop.
    fn repeat(&mut self, block: &mut Block<'_>, node: NodeId, repeat: &Repeat, indent: &str, text: &mut Text) {
        let pass_id = block.pass.id.clone();
        let mut head = String::new();
        for accumulator in &repeat.accumulators {
            let running = self.target(block, accumulator.running, node);
            let start = match accumulator.init {
                Some(slot) => self.slot(block, slot, node),
                None => literal(accumulator.identity).unwrap_or_else(|| {
                    self.refuse(&pass_id, Some(node), "holds an accumulator the GPU cannot hold");
                    "0u".to_owned()
                }),
            };
            let type_name = WgslType::of_value(accumulator.identity).map_or("u32", WgslType::name);
            head.push_str(&format!("{indent}var {running}: {type_name} = {start};\n"));
        }
        let count = self.slot(block, repeat.count, node);
        let when = self.slot(block, repeat.when, node);
        let total = self.target(block, repeat.total, node);
        let k = self.target(block, repeat.k, node);
        let max = repeat.max;
        head.push_str(&format!(
            "{indent}let {total} = select(0u, min({count}, {max}u), {when});\n"
        ));
        head.push_str(&format!(
            "{indent}for (var {k} = 0u; {k} < {total}; {k} = {k} + 1u) {{\n"
        ));
        let inner = format!("{indent}    ");
        for accumulator in &repeat.accumulators {
            let prev = self.target(block, accumulator.prev, node);
            let running = self.target(block, accumulator.running, node);
            head.push_str(&format!("{inner}let {prev} = {running};\n"));
        }
        let iteration = format!("henad_iteration_n{}", node.number());
        let word = if block.iteration == "0u" && self.input.outer_iteration.is_none() {
            k.clone()
        } else {
            format!("{} * {max}u + {k}", block.iteration)
        };
        head.push_str(&format!("{inner}let {iteration} = {word};\n"));
        text.node_line(&pass_id, node, &head);
        let outer = std::mem::replace(&mut block.iteration, iteration);
        for instruction in &repeat.body {
            self.instruction(block, instruction, &inner, text);
        }
        block.iteration = outer;
        text.node_line(&pass_id, node, &format!("{indent}}}\n"));
    }

    /// Returns the constant of the palette that the `view.color` node `node` names, declaring it once.
    fn palette_of(&mut self, block: &Block<'_>, node: NodeId) -> Option<String> {
        let found = block.pass.body.nodes.iter().find(|candidate| candidate.id == node)?;
        let def = crate::catalogue::lookup(&found.op, self.input.document.library)?;
        let config = effective_config(def, found);
        let id = config.get("palette").and_then(ConfigValue::as_str)?;
        self.palette(id)
    }

    /// Returns the constant of the palette `id`, a `[[palette]]` table or a category, declaring it once.
    fn palette(&mut self, id: &str) -> Option<String> {
        let document = self.input.document;
        let (constant, colors) = if let Some(palette) = document.palettes.iter().find(|palette| palette.id == id) {
            (format!("henad_palette_{id}"), palette.colors.clone())
        } else {
            let category = document.categories.iter().find(|category| category.id == id)?;
            (
                format!("henad_category_{id}"),
                category.variants.iter().map(|variant| variant.color).collect(),
            )
        };
        if colors.is_empty() {
            return None;
        }
        self.palettes.insert(constant.clone(), colors);
        Some(constant)
    }

    /// Returns the view of a document with no view pass: the first category lane coloured by its category, or black.
    fn default_view(&mut self) -> String {
        let document = self.input.document;
        let lane = document
            .lanes
            .iter()
            .enumerate()
            .find_map(|(position, lane)| match &lane.value_type {
                crate::types::ValueType::Category(category) => Some((position, category.clone())),
                _ => None,
            });
        let Some((position, category)) = lane else {
            return "    // No view pass and no category lane: every cell is black.\n".to_owned();
        };
        let Some(constant) = self.palette(&category) else {
            return String::new();
        };
        let last = self.palettes.get(&constant).map_or(0, |colors| colors.len() - 1);
        let load = self.load(position, "henad_index");
        format!(
            "    // No view pass: the lane '{}' coloured by its category.\n    var henad_colors = {constant};\n    henad_color = henad_colors[min({load}, {last}u)];\n",
            document.lanes[position].id
        )
    }

    /// Returns the shader: its imports, uniform block, bindings and helpers, then `main` around `body`.
    #[expect(
        clippy::too_many_lines,
        reason = "the template of every role reads best in one place"
    )]
    fn assemble(mut self, body: Text) -> Emitted {
        let counters = self.tally_offset.max(1);
        let mut bindings: Vec<ShaderBinding> = Vec::new();
        let mut declarations = String::new();
        let storage = |read_only: bool| BindingKind::Storage { read_only };
        let bind = |bindings: &mut Vec<ShaderBinding>, declarations: &mut String, name: &str, kind, wgsl: &str| {
            declarations.push_str(&format!("@group(0) @binding({}) {wgsl};\n", bindings.len()));
            bindings.push(ShaderBinding::new(name.to_owned(), kind));
        };
        let lanes: BTreeSet<usize> = self.reads.union(&self.writes).copied().collect();
        for lane in lanes {
            let Some(wgsl) = self.input.lanes[lane].wgsl else {
                continue;
            };
            let element = wgsl.stored();
            let (read, write) = self.lane_bindings(lane);
            let writes = self.writes.contains(&lane);
            if read == write {
                let access = if writes { "read_write" } else { "read" };
                bind(
                    &mut bindings,
                    &mut declarations,
                    &read,
                    storage(!writes),
                    &format!("var<storage, {access}> {read}: array<{element}>"),
                );
            } else {
                bind(
                    &mut bindings,
                    &mut declarations,
                    &read,
                    storage(true),
                    &format!("var<storage, read> {read}: array<{element}>"),
                );
                bind(
                    &mut bindings,
                    &mut declarations,
                    &write,
                    storage(false),
                    &format!("var<storage, read_write> {write}: array<{element}>"),
                );
            }
        }
        let writes_tick = matches!(self.role, Role::Step { writes_tick: true });
        let tick_read = if writes_tick { "henad_tick_in" } else { "henad_tick" };
        if self.reads_tick || writes_tick {
            bind(
                &mut bindings,
                &mut declarations,
                tick_read,
                storage(true),
                &format!("var<storage, read> {tick_read}: array<u32>"),
            );
        }
        if writes_tick {
            bind(
                &mut bindings,
                &mut declarations,
                "henad_tick_out",
                storage(false),
                "var<storage, read_write> henad_tick_out: array<u32>",
            );
        }
        let uniform = UniformBlock::new(std::mem::take(&mut self.fields));
        bind(
            &mut bindings,
            &mut declarations,
            "params",
            BindingKind::Uniform,
            "var<uniform> params: HenadParams",
        );
        match self.role {
            Role::Display => {
                bind(
                    &mut bindings,
                    &mut declarations,
                    "output",
                    BindingKind::StorageTexture,
                    "var output: texture_storage_2d<rgba8unorm, write>",
                );
                bind(
                    &mut bindings,
                    &mut declarations,
                    "dims",
                    BindingKind::Uniform,
                    "var<uniform> dims: Dims",
                );
            }
            Role::Reduce => bind(
                &mut bindings,
                &mut declarations,
                "counters",
                storage(false),
                &format!("var<storage, read_write> counters: array<atomic<u32>, {counters}>"),
            ),
            Role::Step { .. } | Role::Action => {}
        }

        let mut text = Text::default();
        text.push("// Generated by henad-graph. Every value is named after its node, n<id>_<port>.\n");
        let mut rng = Vec::new();
        if self.uses.pcg_hash {
            rng.push("pcg_hash");
        }
        if self.uses.random_float {
            rng.push("random_float");
        }
        if !rng.is_empty() {
            text.text
                .push_str(&format!("#import henad::rng::{{{}}}\n", rng.join(", ")));
        }
        if self.uses.draw_core {
            text.push("#import henad_graph::draw::draw_core\n");
        }
        if self.uses.neighbors {
            text.push("#import henad::space::{offset_cell, TORUS, BOUNDED}\n");
        }
        if self.role == Role::Display {
            text.push("#import henad::dims::{Dims, cell_at}\n");
        }
        text.push("\n");
        text.push(&uniform.declaration());
        text.push("\n");
        text.push(&declarations);
        text.push("\n");
        for (constant, colors) in &self.palettes {
            text.text
                .push_str(&format!("const {constant} = array<vec4<f32>, {}>(\n", colors.len()));
            for [red, green, blue, alpha] in colors {
                text.text.push_str(&format!(
                    "    vec4<f32>({red}.0 / 255.0, {green}.0 / 255.0, {blue}.0 / 255.0, {alpha}.0 / 255.0),\n"
                ));
            }
            text.push(");\n\n");
        }
        if self.uses.threshold {
            text.push(
                "// The word below which a draw is a success at probability p, as the host computes it: p times 2^32,\n\
                 // saturating at the top.\n\
                 fn henad_threshold(p: f32) -> u32 {\n    \
                 let scaled = p * 4294967296.0f;\n    \
                 return select(u32(scaled), 4294967295u, scaled >= 4294967296.0f);\n\
                 }\n\n",
            );
        }
        if self.uses.neighbors {
            let boundary = match self.input.document.world {
                crate::document::World::Grid(grid) if grid.boundary == Boundary::Bounded => "BOUNDED",
                _ => "TORUS",
            };
            text.text.push_str(&format!("var<private> henad_cell: vec2<u32>;\n\n\
                 // The cell dx east and dy south of this cell, and whether it is on the grid.\n\
                 fn henad_at(dx: i32, dy: i32) -> vec3<i32> {{\n    \
                 return offset_cell(henad_cell.x, henad_cell.y, dx, dy, params.henad_width, params.henad_height, {boundary});\n\
                 }}\n\n"));
        }
        if self.role == Role::Reduce {
            text.text.push_str(&format!(
                "var<workgroup> henad_partial: array<atomic<u32>, {counters}>;\n\n"
            ));
        }
        text.text.push_str(&format!(
            "@compute @workgroup_size({WORKGROUP_SIZE}, {WORKGROUP_SIZE})\n"
        ));
        let cell_line = if self.uses.neighbors {
            "    henad_cell = henad_id.xy;\n"
        } else {
            ""
        };
        let tick_line = if self.reads_tick || writes_tick {
            format!("    let henad_tick = {tick_read}[0];\n")
        } else {
            String::new()
        };
        match self.role {
            Role::Step { .. } | Role::Action => {
                text.push("fn main(@builtin(global_invocation_id) henad_id: vec3<u32>) {\n");
                text.push(&tick_line);
                if writes_tick {
                    text.push(
                        "    if (henad_id.x == 0u && henad_id.y == 0u) {\n        henad_tick_out[0] = henad_tick + 1u;\n    }\n",
                    );
                }
                text.push(
                    "    if (henad_id.x >= params.henad_width || henad_id.y >= params.henad_height) {\n        return;\n    }\n",
                );
                text.push(cell_line);
                text.push("    let henad_index = henad_id.y * params.henad_width + henad_id.x;\n");
                text.append(body);
                text.push("}\n");
            }
            Role::Display => {
                text.push("fn main(@builtin(global_invocation_id) henad_id: vec3<u32>) {\n");
                text.push("    if (henad_id.x >= dims.tex.x || henad_id.y >= dims.tex.y) {\n        return;\n    }\n");
                text.push("    let henad_xy = cell_at(henad_id.xy, dims);\n");
                if self.uses.neighbors {
                    text.push("    henad_cell = henad_xy;\n");
                }
                text.push(&tick_line);
                text.push("    let henad_index = henad_xy.y * params.henad_width + henad_xy.x;\n");
                text.push("    var henad_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);\n");
                text.append(body);
                text.push("    textureStore(output, vec2<i32>(henad_id.xy), henad_color);\n}\n");
            }
            Role::Reduce => {
                text.push(
                    "fn main(\n    @builtin(global_invocation_id) henad_id: vec3<u32>,\n    @builtin(local_invocation_index) henad_local: u32,\n) {\n",
                );
                text.text.push_str(&format!("    for (var henad_c = henad_local; henad_c < {counters}u; henad_c = henad_c + {}u) {{\n        atomicStore(&henad_partial[henad_c], 0u);\n    }}\n    workgroupBarrier();\n", WORKGROUP_SIZE * WORKGROUP_SIZE));
                text.push(&tick_line);
                text.push("    if (henad_id.x < params.henad_width && henad_id.y < params.henad_height) {\n");
                if self.uses.neighbors {
                    text.push("    henad_cell = henad_id.xy;\n");
                }
                text.push("    let henad_index = henad_id.y * params.henad_width + henad_id.x;\n");
                text.append(body);
                text.push("    }\n    workgroupBarrier();\n");
                text.text.push_str(&format!("    for (var henad_c = henad_local; henad_c < {counters}u; henad_c = henad_c + {}u) {{\n        let henad_count = atomicLoad(&henad_partial[henad_c]);\n        if (henad_count > 0u) {{\n            atomicAdd(&counters[henad_c], henad_count);\n        }}\n    }}\n}}\n", WORKGROUP_SIZE * WORKGROUP_SIZE));
            }
        }
        Emitted {
            text: text.text,
            ranges: text.ranges,
            bindings,
            uniform,
            counters,
        }
    }
}

/// Names the running value of every accumulator of every Repeat in `block` after the accumulator's output, which takes
/// the running value once the loop ends.
fn name_running(block: &[Instruction], names: &mut [SlotName], types: &mut [Option<WgslType>]) {
    for instruction in block {
        if let Operation::Repeat(repeat) = &instruction.operation {
            for accumulator in &repeat.accumulators {
                if let Some(name) = names.get(accumulator.out).cloned() {
                    names[accumulator.running] = name;
                }
                types[accumulator.running] = WgslType::of_value(accumulator.identity);
            }
            name_running(&repeat.body, names, types);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{literal, value_name};
    use crate::host::value::HostValue;
    use crate::ids::NodeId;

    /// A dotted port, an accumulator whose name holds an underscore, and a segment ending in one each take a name of
    /// their own.
    #[test]
    fn value_names_stay_apart() {
        let node = NodeId::new(12).expect("an id");
        let names = [
            value_name(node, "prev.close"),
            value_name(node, "prev_close"),
            value_name(node, "prev_.close"),
        ];
        assert_eq!(names[0], "n12_prev__close");
        assert_eq!(names[1], "n12_prev_close");
        assert_eq!(names[2], "n12_prev___close");
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn literals_are_exact() {
        assert_eq!(literal(HostValue::U32(7)).as_deref(), Some("7u"));
        assert_eq!(literal(HostValue::I32(i32::MIN)).as_deref(), Some("i32(-2147483648)"));
        assert_eq!(literal(HostValue::F32(0.3)).as_deref(), Some("0.3f"));
        assert_eq!(literal(HostValue::F32(-0.0)).as_deref(), Some("-0.0f"));
        assert_eq!(
            literal(HostValue::F32(f32::from_bits(1))).as_deref(),
            Some("bitcast<f32>(0x00000001u)")
        );
        assert_eq!(literal(HostValue::U64(1)), None);
    }
}
