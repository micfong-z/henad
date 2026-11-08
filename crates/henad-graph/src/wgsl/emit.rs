//! Emission of one generated shader: the engine-owned template of its role around the body of each pass it runs.
//!
//! The body of a pass is its host program, the instructions the host evaluator runs, written as WGSL. Every value is a
//! `let` named `n<id>_<port>`, with each `.` of a dotted port written `__`. The prelude values the host can evaluate at
//! build come in through the uniform block, and the prelude values that read the tick are computed in the shader.
//!
//! A shader of a grid runs one invocation per cell in workgroups of 16 by 16. A shader of an agents world, the display
//! pass aside, is a *linear pass*: one invocation per element of its domain in workgroups of 256, the index folded
//! with `henad::dispatch::linear_index`.

use std::collections::{BTreeMap, BTreeSet};

use henad_compute::gpu::{BufferSide, ShaderBinding, SideTable};
use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamKind;

use super::pack::{LaneGroup, LaneLayout};
use super::probes::ProbeSlot;
use super::uniform::{FieldShape, FieldSource, UniformBlock, WgslType};
use super::{NodeRange, WORKGROUP_SIZE};
use crate::catalogue::effective_config;
use crate::check::CheckReport;
use crate::document::{Domain, FieldCombine, GraphDocument, Pass, PassEnabled};
use crate::host::HostEvaluator;
use crate::host::compile::{
    CompareOp, DrawForm, Instruction, LogicOp, Operation, Program, RadiusFold, Repeat, RepeatAccumulator, StencilFold,
};
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
    /// The snapshot colour pass of an agents world, which runs the view passes over agents and writes each agent's
    /// colour.
    Color,
    /// The field update of the field at this position of the agents plan's fields, one invocation per cell of each
    /// layer, its slot `layer * cells + cell`.
    ///
    /// The body's element is the slot's cell, and a draw's element word is the slot.
    Merge(usize),
}

/// Side `N` of the `@workgroup_size(N)` of every linear pass.
pub(crate) const LINEAR_WORKGROUP: u32 = 256;

/// Word of an agents world's `counters` that a radius fold past its visit cap sets to its fault code.
pub(crate) const FAULT_WORD: usize = 0;

/// Word of an agents world's `counters` that holds the tick, which the last pass of a step advances.
pub(crate) const TICK_WORD: usize = 1;

/// Word of an agents world's `counters` that holds the document's first counter.
pub(crate) const FIRST_COUNTER: usize = 2;

/// Label of the buffer of packed RGBA words that the colour pass of an agents world writes and the point view draws.
pub(crate) const AGENT_COLORS: &str = "agent_colors";

/// How a lane is held on the device.
#[derive(Debug, Clone)]
pub(crate) struct LanePlan {
    /// Id of the lane.
    pub id: String,
    /// Label of its buffer, `lane_<id>_<domain>`.
    pub label: String,
    /// Type a shader holds the lane's values in, `None` for a lane only the host holds.
    pub wgsl: Option<WgslType>,
    /// Position among the agents plan's fields of the field whose merged value this lane stands for in a merge pass,
    /// `None` for a lane of the document.
    pub merged: Option<usize>,
}

/// How a field is held on the device: one buffer of its layers one after another, and for a field that takes
/// deposits one buffer of their merged deposits.
#[derive(Debug, Clone)]
pub(crate) struct FieldPlan {
    /// Id of the field.
    pub id: String,
    /// Number of layers.
    pub layers: u32,
    /// Type of a cell, `f32` or a category's variant number.
    pub wgsl: WgslType,
    /// Position of the field's first layer among the columns of the host lanes.
    pub first: usize,
    /// Whether the field takes deposits.
    pub dynamic: bool,
    /// Position of the field's first layer among the deposit columns of the host lanes.
    pub first_deposit: usize,
    /// Rule that merges the field's deposits, `None` for a static field.
    pub combine: Option<FieldCombine>,
}

impl FieldPlan {
    /// Label of the buffer of the field's layers.
    pub(crate) fn label(&self) -> String {
        format!("field_{}_cells", self.id)
    }

    /// Label of the buffer of the field's deposits.
    pub(crate) fn accum_label(&self) -> String {
        format!("accum_{}_cells", self.id)
    }
}

/// What every shader of an agents world shares beside the lanes.
#[derive(Debug, Clone)]
pub(crate) struct AgentPlan {
    /// Fields in `[[field]]` order.
    pub fields: Vec<FieldPlan>,
    /// Number of the document's counters, which take the words after the fault word of `counters`.
    pub counters: usize,
    /// Words of the `counters` buffer: the fault word, the document's counters, then the reduce leaf's tallies.
    pub counter_words: usize,
    /// Each radius fold that can fault, as its pass id and node. At position `p`, its fault code past its visit cap is
    /// `2 * p + 1`, and past its cell cap `2 * p + 2`.
    pub faults: Vec<(String, NodeId)>,
    /// Position of the position lane.
    pub position_lane: Option<usize>,
}

impl AgentPlan {
    /// Returns the field whose layers sit at the column `column` of the host lanes, with the layer's number.
    fn field_of(&self, column: usize) -> Option<(usize, u32)> {
        self.fields.iter().enumerate().find_map(|(index, field)| {
            let layer = column.checked_sub(field.first)?;
            (layer < field.layers as usize).then_some((index, layer as u32))
        })
    }

    /// Returns the field whose deposits sit at the deposit column `column`.
    fn field_of_deposit(&self, column: usize) -> Option<usize> {
        self.fields.iter().position(|field| {
            field.dynamic && column >= field.first_deposit && column < field.first_deposit + field.layers as usize
        })
    }
}

/// Declarations every shader of one graph shares.
pub(crate) struct EmitInput<'a> {
    pub document: &'a GraphDocument,
    pub report: &'a CheckReport,
    pub evaluator: &'a HostEvaluator,
    pub lanes: &'a [LanePlan],
    /// Lane buffers, and the buffer and segment of each lane.
    pub layout: &'a LaneLayout,
    /// Whether the buffers hold the one-word tick buffer, after the lanes.
    pub tick_buffer: bool,
    /// Iteration word of a region around the top of every pass, for a test that probes a draw inside one.
    pub outer_iteration: Option<u32>,
    /// Plan of an agents world, `None` for a grid.
    pub agents: Option<&'a AgentPlan>,
    /// Probes of the tick passes, which a step counts under its counting block.
    pub probes: &'a [ProbeSlot],
    /// Word of `counters` that holds the count of the first probe. The count of probe `i` is at `fire_base + i`.
    pub fire_base: usize,
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
    /// Number of `f32` lanes a reduce leaf of an agents world sums, each by the document position of its pass and the
    /// number of its sum, in lane order. A `vec2f` sum takes two lanes in a row.
    pub sum_lanes: Vec<(usize, usize)>,
    /// Bytes of the shader's `var<workgroup>` declarations.
    pub workgroup_bytes: u64,
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

    /// Returns the text with four more spaces before each non-empty line, and its ranges moved to match.
    ///
    /// A range that starts at the beginning of a line takes in the line's new indentation.
    fn indented(self) -> Self {
        let mut text = String::with_capacity(self.text.len());
        let mut starts = Vec::new();
        let mut offset = 0;
        for line in self.text.split_inclusive('\n') {
            if line != "\n" {
                starts.push(offset);
                text.push_str("    ");
            }
            text.push_str(line);
            offset += line.len();
        }
        let shift = |position: usize| position + 4 * starts.partition_point(|&start| start < position);
        let ranges = self
            .ranges
            .into_iter()
            .map(|mut range| {
                range.range = shift(range.range.start)..shift(range.range.end);
                range
            })
            .collect();
        Self { text, ranges }
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
pub(crate) fn defined(instruction: &Instruction) -> Vec<usize> {
    let mut slots = vec![instruction.out];
    match &instruction.operation {
        Operation::Transition { fired, .. } => slots.extend(fired.iter().copied()),
        Operation::VecSplit { y_out, .. } => slots.push(*y_out),
        Operation::CellIndex { valid_out, .. } => slots.push(*valid_out),
        Operation::CellOf { xy_out, valid_out, .. } => slots.extend([*xy_out, *valid_out]),
        Operation::Offset {
            cell_out, valid_out, ..
        } => slots.extend([*cell_out, *valid_out]),
        _ => {}
    }
    slots
}

/// Returns the packed RGBA of `color`, the word the point view reads.
pub(crate) fn packed_color(color: Color) -> u32 {
    u32::from_le_bytes(color)
}

/// Shared modules and helpers a shader uses.
#[derive(Default)]
struct Uses {
    pcg_hash: bool,
    random_float: bool,
    draw_core: bool,
    neighbors: bool,
    threshold: bool,
    /// `henad::space` functions besides the stencil's.
    space: BTreeSet<&'static str>,
    /// `henad::rng` functions besides `pcg_hash` and `random_float`.
    rng: BTreeSet<&'static str>,
    /// The helper `henad_step` of a stencil fold or an offset.
    step: bool,
    /// The neighbour index, its walk and the fault word.
    hash: bool,
    /// The `counters` buffer.
    counters: bool,
    /// The element's own cell, `henad_own_cell`.
    own_cell: bool,
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
    /// Fields some pass of the shader reads, by position among the agents plan's fields.
    fields_read: BTreeSet<usize>,
    /// Fields some pass of the shader writes.
    fields_written: BTreeSet<usize>,
    /// Fields some pass of the shader deposits into.
    deposited: BTreeSet<usize>,
    /// Lanes of a reduce leaf, by the document position of the pass and the number of the sum, with the first lane.
    sum_lanes: Vec<(usize, usize)>,
    /// Lanes some pass reads at another element, which a pass over agents bounds-checks.
    reads_other: BTreeSet<usize>,
    /// Palettes the colour pass reads as packed RGBA words, by constant name.
    packed: BTreeMap<String, Vec<Color>>,
    /// Slot in the workgroup array `henad_counter_tally` of each document counter some pass of the shader adds to, by
    /// the counter's position among the document's counters.
    counter_slots: BTreeMap<usize, usize>,
    /// Position among the probes of the probe each slot of the workgroup array `henad_fire_tally` tallies, in slot
    /// order.
    fire_slots: Vec<usize>,
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
    /// Stores of field writes, run at the end of the pass, each as its line.
    deferred: Vec<String>,
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
        reads_tick: matches!(role, Role::Step { writes_tick: true }) && input.agents.is_none(),
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
        fields_read: BTreeSet::new(),
        fields_written: BTreeSet::new(),
        deposited: BTreeSet::new(),
        sum_lanes: Vec::new(),
        reads_other: BTreeSet::new(),
        packed: BTreeMap::new(),
        counter_slots: BTreeMap::new(),
        fire_slots: Vec::new(),
    };
    if input.agents.is_some() {
        emitter.agent_fields(role);
    }
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
    if role == Role::Display && positions.is_empty() && input.agents.is_none() {
        body.push(&emitter.default_view());
    }
    if role == Role::Color && positions.is_empty() {
        body.push(&emitter.default_agent_color());
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

    /// Adds the uniform members every shader of an agents world reads in its template.
    fn agent_fields(&mut self, role: Role) {
        if role == Role::Display {
            self.field(
                "henad_texture",
                FieldShape::Value(WgslType::Vec2U),
                FieldSource::Texture,
            );
        } else {
            self.field("henad_groups_x", FieldShape::Value(WgslType::U32), FieldSource::GroupsX);
            self.field(
                "henad_agents",
                FieldShape::Value(WgslType::U32),
                FieldSource::Count(Domain::Agents),
            );
        }
        self.field(
            "henad_cells",
            FieldShape::Value(WgslType::U32),
            FieldSource::Count(Domain::Cells),
        );
    }

    /// Returns whether a pass of the shader writes a lane of `group`.
    fn group_written(&self, group: &LaneGroup) -> bool {
        group.lanes.iter().any(|lane| self.writes.contains(lane))
    }

    /// Returns one lane of each lane buffer some pass of the shader reads or writes, in buffer order, with the type of
    /// the buffer's elements.
    fn bound_lanes(&self) -> Vec<(usize, &'static str)> {
        let mut buffers: BTreeMap<usize, (usize, &'static str)> = BTreeMap::new();
        for &lane in self.reads.union(&self.writes) {
            let Some(plan) = self.input.lanes.get(lane) else {
                continue;
            };
            let (Some(wgsl), None, Some(group)) = (plan.wgsl, plan.merged, self.input.layout.group_of(lane)) else {
                continue;
            };
            let Some(place) = self.input.layout.place(lane) else {
                continue;
            };
            let element = if group.is_packed() { "u32" } else { wgsl.stored() };
            buffers.entry(place.buffer()).or_insert((lane, element));
        }
        buffers.into_values().collect()
    }

    /// Returns the binding name a lane's reads use, and the one its writes use.
    ///
    /// A lane shares the names of its lane buffer with every lane the buffer holds.
    fn lane_bindings(&self, lane: usize) -> (String, String) {
        let Some(place) = self.input.layout.place(lane) else {
            let base = self.input.lanes[lane].label.clone();
            return (base.clone(), base);
        };
        let group = &self.input.layout.groups[place.buffer()];
        let base = group.label.clone();
        let row = self.sides.rows().get(place.buffer()).copied();
        if self.group_written(group) && row.is_some_and(|row| row.write == BufferSide::Next) {
            (format!("{base}_in"), format!("{base}_out"))
        } else {
            (base.clone(), base)
        }
    }

    /// Returns the expression of the number of elements of `domain`.
    fn count_of(&mut self, domain: Domain) -> String {
        if self.input.agents.is_none() {
            return "(params.henad_width * params.henad_height)".to_owned();
        }
        match domain {
            Domain::Agents => {
                self.field(
                    "henad_agents",
                    FieldShape::Value(WgslType::U32),
                    FieldSource::Count(Domain::Agents),
                );
                "params.henad_agents".to_owned()
            }
            Domain::Cells => "params.henad_cells".to_owned(),
        }
    }

    /// Returns the expression of the first word of element `index` of `lane` in its packed buffer, `None` for a lane
    /// with a buffer of its own.
    fn packed_word(&mut self, lane: usize, index: &str) -> Option<String> {
        let group = self.input.layout.group_of(lane)?;
        if !group.is_packed() {
            return None;
        }
        let place = self.input.layout.place(lane)?;
        let domain = group.domain;
        let offset = place.offset();
        let element = match place.words() {
            1 => format!("({index})"),
            words => format!("{words}u * ({index})"),
        };
        Some(if offset == 0 {
            element
        } else {
            let count = self.count_of(domain);
            format!("{offset}u * {count} + {element}")
        })
    }

    /// Returns the expression that loads `lane` at the element `index`.
    fn load(&mut self, lane: usize, index: &str) -> String {
        if self.input.lanes.get(lane).is_some_and(|plan| plan.merged.is_some()) {
            return "henad_merged".to_owned();
        }
        self.reads.insert(lane);
        let (read, _) = self.lane_bindings(lane);
        let wgsl = self.input.lanes[lane].wgsl;
        if let Some(word) = self.packed_word(lane, index) {
            let scalar = |at: String| match wgsl {
                Some(WgslType::Bool) => format!("({read}[{at}] != 0u)"),
                Some(WgslType::I32 | WgslType::Vec2I) => format!("bitcast<i32>({read}[{at}])"),
                Some(WgslType::F32 | WgslType::Vec2F) => format!("bitcast<f32>({read}[{at}])"),
                _ => format!("{read}[{at}]"),
            };
            return match wgsl {
                Some(vector) if vector.is_vector() => format!(
                    "{}({}, {})",
                    vector.name(),
                    scalar(word.clone()),
                    scalar(format!("{word} + 1u"))
                ),
                _ => scalar(word),
            };
        }
        match wgsl {
            Some(WgslType::Bool) => format!("({read}[{index}] != 0u)"),
            _ => format!("{read}[{index}]"),
        }
    }

    /// Returns the expression that loads `lane` at the element `index` of another element, the type's zero past the
    /// end of the lane.
    fn load_other(&mut self, lane: usize, index: &str) -> String {
        self.reads_other.insert(lane);
        let load = self.load(lane, index);
        let (read, _) = self.lane_bindings(lane);
        let zero = self.input.lanes[lane]
            .wgsl
            .map_or_else(|| "0u".to_owned(), WgslType::zero);
        let bound = match self.input.layout.group_of(lane) {
            Some(group) if group.is_packed() => {
                let domain = group.domain;
                self.count_of(domain)
            }
            _ => format!("arrayLength(&{read})"),
        };
        format!("select({zero}, {load}, {index} < {bound})")
    }

    /// Returns the lines that store `value`, of the lane's type, into `lane` at the element `index`.
    fn store(&mut self, lane: usize, index: &str, value: &str, indent: &str) -> String {
        let (_, write) = self.lane_bindings(lane);
        let wgsl = self.input.lanes[lane].wgsl;
        let Some(word) = self.packed_word(lane, index) else {
            return format!("{indent}{write}[{index}] = {value};\n");
        };
        let bits = |value: String| match wgsl {
            Some(WgslType::I32 | WgslType::F32 | WgslType::Vec2I | WgslType::Vec2F) => format!("bitcast<u32>({value})"),
            _ => value,
        };
        match wgsl {
            Some(vector) if vector.is_vector() => format!(
                "{indent}{write}[{word}] = {};\n{indent}{write}[{word} + 1u] = {};\n",
                bits(format!("{value}.x")),
                bits(format!("{value}.y"))
            ),
            _ => format!("{indent}{write}[{word}] = {};\n", bits(value.to_owned())),
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
            deferred: Vec::new(),
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
        // A field layer sits after the lanes, and its write is a deferred store of its own.
        let written: Vec<usize> = program
            .written
            .iter()
            .copied()
            .filter(|&lane| lane < self.input.lanes.len())
            .collect();
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
        if matches!(self.role, Role::Step { .. }) {
            self.probe_tallies(&mut block, indent, &mut inner);
        }
        for &lane in &written {
            let Some(wgsl) = self.input.lanes.get(lane).and_then(|plan| plan.wgsl) else {
                continue;
            };
            let id = &self.input.lanes[lane].id;
            let value = if wgsl == WgslType::Bool {
                format!("u32(henad_write_{id})")
            } else {
                format!("henad_write_{id}")
            };
            if let Some(field) = self.input.lanes[lane].merged {
                let label = self
                    .input
                    .agents
                    .map(|plan| plan.fields[field].label())
                    .unwrap_or_default();
                inner.push(&format!("{indent}{label}[henad_slot] = {value};\n"));
                continue;
            }
            let line = self.store(lane, "henad_index", &value, indent);
            inner.push(&line);
        }
        for line in std::mem::take(&mut block.deferred) {
            inner.push(&format!("{indent}{line}\n"));
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
        // A pass of an agents world runs over its own domain, and a shader can run more invocations than it has.
        let guard = match (self.input.agents, self.role, pass.domain) {
            (Some(_), Role::Step { .. } | Role::Reduce | Role::Action | Role::Color, Some(Domain::Agents)) => {
                Some("henad_index < params.henad_agents")
            }
            (Some(_), Role::Step { .. } | Role::Reduce | Role::Action | Role::Color, Some(Domain::Cells)) => {
                Some("henad_index < params.henad_cells")
            }
            _ => None,
        };
        let condition = match (&enabled, guard) {
            (Some(name), Some(guard)) => Some(format!("params.{name} != 0u && {guard}")),
            (Some(name), None) => Some(format!("params.{name} != 0u")),
            (None, Some(guard)) => Some(guard.to_owned()),
            (None, None) => None,
        };
        match &condition {
            Some(condition) => text.push(&format!("    if ({condition}) {{\n")),
            None => text.push("    {\n"),
        }
        if self.input.agents.is_some() && pass.domain == Some(Domain::Cells) && self.role != Role::Display {
            text.push(&format!(
                "{indent}let henad_own_cell = vec2<u32>(henad_index % params.henad_width, henad_index / params.henad_width);\n"
            ));
            if self.uses.neighbors {
                text.push(&format!("{indent}henad_cell = henad_own_cell;\n"));
            }
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

    /// Writes the tally of each probe of the pass of `block`: under the counting block, 1 into the probe's slot of
    /// `henad_fire_tally` for an element at which the probe's output holds.
    fn probe_tallies(&mut self, block: &mut Block<'_>, indent: &str, text: &mut Text) {
        let probes: Vec<ProbeSlot> = self
            .input
            .probes
            .iter()
            .filter(|probe| probe.position == block.position)
            .copied()
            .collect();
        if probes.is_empty() {
            return;
        }
        self.field(
            "henad_count_fires",
            FieldShape::Value(WgslType::U32),
            FieldSource::CountFires,
        );
        for probe in probes {
            let fired = self.slot(block, probe.slot, probe.node);
            let local = self.fire_slots.len();
            self.fire_slots.push(probe.index);
            text.node_line(
                &block.pass.id,
                probe.node,
                &format!(
                    "{indent}if (params.henad_count_fires != 0u && {fired}) {{\n{indent}    \
                     atomicAdd(&henad_fire_tally[{local}u], 1u);\n{indent}}}\n"
                ),
            );
        }
    }

    /// Returns the lines of `main` that zero the workgroup array `henad_fire_tally` under the counting block, before a
    /// barrier, with `local` the invocation's index in its workgroup of `size`.
    fn fire_zero_lines(&self, local: &str, size: u32) -> String {
        let fired = self.fire_slots.len();
        if fired == 0 {
            return String::new();
        }
        // A barrier needs uniform control flow, and the flag is a uniform, the same for every invocation.
        format!(
            "    if (params.henad_count_fires != 0u) {{\n        for (var henad_f = {local}; henad_f < {fired}u; \
             henad_f = henad_f + {size}u) {{\n            atomicStore(&henad_fire_tally[henad_f], 0u);\n        \
             }}\n        workgroupBarrier();\n    }}\n"
        )
    }

    /// Returns the lines at the end of `main` that add each non-zero slot of `henad_fire_tally` to its probe's word of
    /// `counters` under the counting block, one global atomic per slot per workgroup, with `local` the invocation's
    /// index in its workgroup of `size`.
    fn fire_flush_lines(&self, local: &str, size: u32) -> String {
        if self.fire_slots.is_empty() {
            return String::new();
        }
        let mut lines = String::from("    if (params.henad_count_fires != 0u) {\n        workgroupBarrier();\n");
        for (slot, &index) in self.fire_slots.iter().enumerate() {
            lines.push_str(&format!(
                "        if ({local} == {invocation}u) {{\n            let henad_fires_{slot} = \
                 atomicLoad(&henad_fire_tally[{slot}u]);\n            \
                 if (henad_fires_{slot} != 0u) {{\n                \
                 atomicAdd(&counters[{word}u], henad_fires_{slot});\n            }}\n        }}\n",
                invocation = slot as u32 % size,
                word = self.input.fire_base + index
            ));
        }
        lines.push_str("    }\n");
        lines
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
            Operation::StreamBits { .. } | Operation::StreamFloat { .. } => {
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
                let reference =
                    matches!(Self::type_of(block, out), Some(WgslType::U32)) && matches!(target, Conversion::ToU32);
                let expression = match (target, source) {
                    // A cell or an agent is its index, a `u32` already.
                    (Conversion::ToU32, Some(WgslType::U32)) if reference => a,
                    (Conversion::ToF32, Some(WgslType::U32 | WgslType::I32 | WgslType::Bool)) => format!("f32({a})"),
                    (Conversion::ToVec2F, Some(WgslType::Vec2I | WgslType::Vec2U)) => format!("vec2<f32>({a})"),
                    (Conversion::ToVec2I, Some(WgslType::Vec2F)) => format!("vec2<i32>({a})"),
                    (Conversion::ToVec2I, Some(WgslType::Vec2U)) => format!("bitcast<vec2<i32>>({a})"),
                    (Conversion::ToVec2U, Some(WgslType::Vec2F)) => format!("vec2<u32>({a})"),
                    (Conversion::ToVec2U, Some(WgslType::Vec2I)) => format!("bitcast<vec2<u32>>({a})"),
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
                // Two layers of one cell draw apart in a merge, keyed on the slot.
                let element = if matches!(self.role, Role::Merge(_)) {
                    "henad_slot"
                } else {
                    "henad_index"
                };
                let bits = format!("draw_core(params.{key}, {element}, {tick}, {}, 0u)", block.iteration);
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
            Operation::Color { index, len } if self.role == Role::Color => {
                let index = self.slot(block, *index, node);
                match self.packed_palette_of(block, node) {
                    Some(constant) if *len > 0 => {
                        lines.push_str(&format!(
                            "{indent}henad_color = {constant}[min({index}, {}u)];\n",
                            len - 1
                        ));
                    }
                    _ => self.refuse(&pass_id, Some(node), "colours from an empty or unknown palette"),
                }
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
            Operation::World => {
                self.field("henad_world", FieldShape::Value(WgslType::Vec2F), FieldSource::World);
                let_out!("params.henad_world".to_owned());
            }
            Operation::Grid => let_out!("vec2<u32>(params.henad_width, params.henad_height)".to_owned()),
            Operation::CellXy => {
                self.uses.own_cell = true;
                let_out!("henad_own_cell".to_owned());
            }
            Operation::LaneGetAt { lane, at } => {
                let at = self.slot(block, *at, node);
                let load = self.load_other(*lane, &at);
                let_out!(load);
            }
            Operation::FieldGet {
                first,
                layers,
                layer,
                at,
                zero,
            } => {
                let Some((field, _)) = self.input.agents.and_then(|plan| plan.field_of(*first)) else {
                    self.refuse(&pass_id, Some(node), "reads a field the GPU does not hold");
                    return;
                };
                if self.role == Role::Merge(field) {
                    // Every slot of the field is written in place by its own invocation of the same pass.
                    self.refuse(
                        &pass_id,
                        Some(node),
                        "reads the field that its `finish` updates while the update writes that field in place. \
                         The merged value arrives as the input `value`",
                    );
                    return;
                }
                let layer = self.slot(block, *layer, node);
                let cell = match at {
                    Some(slot) => self.slot(block, *slot, node),
                    None => "henad_index".to_owned(),
                };
                let label = self.field_label(field);
                self.fields_read.insert(field);
                let zero = literal(*zero).unwrap_or_else(|| "0u".to_owned());
                let n = node.number();
                lines.push_str(&format!(
                    "{indent}let henad_cell_n{n} = {cell};\n{indent}let henad_layer_n{n} = {layer};\n"
                ));
                let_out!(format!(
                    "select({zero}, {label}[henad_layer_n{n} * params.henad_cells + henad_cell_n{n}], \
                     henad_cell_n{n} < params.henad_cells && henad_layer_n{n} < {layers}u)"
                ));
            }
            Operation::FieldSet {
                first,
                layers,
                layer,
                value,
            } => {
                let Some((field, _)) = self.input.agents.and_then(|plan| plan.field_of(*first)) else {
                    self.refuse(&pass_id, Some(node), "writes a field the GPU does not hold");
                    return;
                };
                let layer = self.slot(block, *layer, node);
                let value = self.slot(block, *value, node);
                let label = self.field_label(field);
                self.fields_written.insert(field);
                let n = node.number();
                lines.push_str(&format!("{indent}let henad_set_layer_n{n} = {layer};\n"));
                block.deferred.push(format!(
                    "if (henad_set_layer_n{n} < {layers}u) {{ {label}[henad_set_layer_n{n} * params.henad_cells + \
                     henad_index] = {value}; }}"
                ));
            }
            Operation::Deposit {
                first,
                layers,
                layer,
                at,
                value,
                combine,
            } => {
                let Some(field) = self.input.agents.and_then(|plan| plan.field_of_deposit(*first)) else {
                    self.refuse(&pass_id, Some(node), "deposits into a field the GPU does not hold");
                    return;
                };
                let layer = self.slot(block, *layer, node);
                let at = self.slot(block, *at, node);
                let value = self.slot(block, *value, node);
                let label = self.accum_label(field);
                self.deposited.insert(field);
                let n = node.number();
                // A deposit of zero, of a negative value or of a NaN adds 0 and issues no atomic. The bits of every
                // value above 0 order as the value. A sum adds the value as a count of its resolution, and the product
                // by the power of two 2^k is exact.
                let word = format!("{label}[henad_deposit_layer_n{n} * params.henad_cells + henad_deposit_cell_n{n}]");
                let atomic = match combine.deposit_scale() {
                    Some(scale) => {
                        let scale = literal(HostValue::F32(scale)).unwrap_or_default();
                        format!("atomicAdd(&{word}, u32(round(henad_deposit_n{n} * {scale})))")
                    }
                    None => format!("atomicMax(&{word}, bitcast<u32>(henad_deposit_n{n}))"),
                };
                lines.push_str(&format!(
                    "{indent}let henad_deposit_layer_n{n} = {layer};\n\
                     {indent}let henad_deposit_cell_n{n} = {at};\n\
                     {indent}let henad_deposit_n{n} = {value};\n\
                     {indent}if (henad_deposit_n{n} > 0.0f && henad_deposit_cell_n{n} < params.henad_cells && \
                     henad_deposit_layer_n{n} < {layers}u) {{\n\
                     {indent}    {atomic};\n\
                     {indent}}}\n"
                ));
            }
            Operation::CounterAdd { counter, amount } => {
                if !matches!(self.role, Role::Step { .. }) || self.input.agents.is_none() {
                    // The flush of the workgroup's tallies sits in the step template of an agents world alone.
                    self.refuse(
                        &pass_id,
                        Some(node),
                        "counts an event outside a tick pass of an agents world",
                    );
                    return;
                }
                let amount = self.slot(block, *amount, node);
                self.uses.counters = true;
                let next = self.counter_slots.len();
                let slot = *self.counter_slots.entry(*counter).or_insert(next);
                let n = node.number();
                lines.push_str(&format!(
                    "{indent}let henad_amount_n{n} = {amount};\n\
                     {indent}if (henad_amount_n{n} != 0u) {{\n\
                     {indent}    atomicAdd(&henad_counter_tally[{slot}u], henad_amount_n{n});\n\
                     {indent}}}\n"
                ));
            }
            Operation::CounterTotal { .. } | Operation::Midpoint { .. } => {
                // Both read only values at the tick rate or slower, which the host computes.
                self.refuse(&pass_id, Some(node), "is a value the host computes, met in a shader");
            }
            Operation::Clamp { x, lo, hi } => {
                let (x, lo, hi) = (
                    self.slot(block, *x, node),
                    self.slot(block, *lo, node),
                    self.slot(block, *hi, node),
                );
                let_out!(format!("min(max({x}, {lo}), {hi})"));
            }
            Operation::VecMake { x, y } => {
                let (x, y) = (self.slot(block, *x, node), self.slot(block, *y, node));
                match Self::type_of(block, out) {
                    Some(wgsl) if wgsl.is_vector() => let_out!(format!("{}({x}, {y})", wgsl.name())),
                    _ => self.refuse(&pass_id, Some(node), "makes a vector the GPU cannot hold"),
                }
            }
            Operation::VecSplit { v, y_out } => {
                let v = self.slot(block, *v, node);
                let y_name = self.target(block, *y_out, node);
                let_out!(format!("{v}.x"));
                lines.push_str(&format!("{indent}let {y_name} = {v}.y;\n"));
            }
            Operation::VecScale { v, s } => {
                let (v, s) = (self.slot(block, *v, node), self.slot(block, *s, node));
                let_out!(format!("{v} * {s}"));
            }
            Operation::VecDot { a, b } => {
                let (a, b) = (self.slot(block, *a, node), self.slot(block, *b, node));
                let_out!(format!("{a}.x * {b}.x + {a}.y * {b}.y"));
            }
            Operation::VecLength { v } => {
                let v = self.slot(block, *v, node);
                let_out!(format!("sqrt({v}.x * {v}.x + {v}.y * {v}.y)"));
            }
            Operation::FromAngle { angle, length } => {
                let (angle, length) = (self.slot(block, *angle, node), self.slot(block, *length, node));
                let_out!(format!("vec2<f32>(cos({angle}) * {length}, sin({angle}) * {length})"));
            }
            Operation::Wrap { v, world } => {
                let vector = Self::type_of(block, *v).is_some_and(WgslType::is_vector);
                let (v, world) = (self.slot(block, *v, node), self.slot(block, *world, node));
                self.uses.space.insert("wrap_coord");
                if vector {
                    let_out!(format!(
                        "vec2<f32>(wrap_coord({v}.x, {world}.x), wrap_coord({v}.y, {world}.y))"
                    ));
                } else {
                    let_out!(format!("wrap_coord({v}, {world})"));
                }
            }
            Operation::Delta { a, b, world, squared } => {
                let (a, b, world) = (
                    self.slot(block, *a, node),
                    self.slot(block, *b, node),
                    self.slot(block, *world, node),
                );
                self.uses.space.insert("axis_delta");
                self.uses.space.insert("TORUS");
                let delta = format!("henad_delta_n{}", node.number());
                lines.push_str(&format!(
                    "{indent}let {delta} = vec2<f32>(axis_delta({a}.x, {b}.x, {world}.x, TORUS), axis_delta({a}.y, \
                     {b}.y, {world}.y, TORUS));\n"
                ));
                if *squared {
                    let_out!(format!("{delta}.x * {delta}.x + {delta}.y * {delta}.y"));
                } else {
                    let_out!(delta);
                }
            }
            Operation::CellIndex { xy, valid_out } => {
                let signed = Self::type_of(block, *xy) == Some(WgslType::Vec2I);
                let xy = self.slot(block, *xy, node);
                let valid = self.target(block, *valid_out, node);
                lines.push_str(&format!("{indent}let {valid} = {};\n", inside(&xy, signed)));
                let_out!(format!(
                    "select(4294967295u, u32({xy}.y) * params.henad_width + u32({xy}.x), {valid})"
                ));
            }
            Operation::CellOf { p, xy_out, valid_out } => {
                let p = self.slot(block, *p, node);
                let xy = self.target(block, *xy_out, node);
                let valid = self.target(block, *valid_out, node);
                lines.push_str(&format!("{indent}let {xy} = vec2<i32>(i32({p}.x), i32({p}.y));\n"));
                lines.push_str(&format!("{indent}let {valid} = {};\n", inside(&xy, true)));
                let_out!(format!(
                    "select(4294967295u, u32({xy}.y) * params.henad_width + u32({xy}.x), {valid})"
                ));
            }
            Operation::CellXyOf { cell } => {
                let cell = self.slot(block, *cell, node);
                let_out!(format!(
                    "vec2<u32>({cell} % params.henad_width, {cell} / params.henad_width)"
                ));
            }
            Operation::Offset {
                xy,
                by,
                bounded,
                cell_out,
                valid_out,
            } => {
                let (xy, by) = (self.slot(block, *xy, node), self.slot(block, *by, node));
                let cell = self.target(block, *cell_out, node);
                let valid = self.target(block, *valid_out, node);
                let bounded = self.bounded(*bounded);
                self.uses.step = true;
                let step = format!("henad_step_n{}", node.number());
                lines.push_str(&format!("{indent}let {step} = henad_step({xy}, {by}, {bounded});\n"));
                lines.push_str(&format!("{indent}let {valid} = {step}.z != 0i;\n"));
                lines.push_str(&format!(
                    "{indent}let {cell} = select(4294967295u, u32({step}.y) * params.henad_width + u32({step}.x), \
                     {valid});\n"
                ));
                let_out!(format!("{step}.xy"));
            }
            Operation::HeadingOctant { v } => {
                let v = self.slot(block, *v, node);
                self.uses.space.insert("heading_octant");
                let_out!(format!("heading_octant({v}.x, {v}.y)"));
            }
            Operation::StepCode { d } => {
                let d = self.slot(block, *d, node);
                let_out!(format!("bitcast<u32>(({d}.x + 1i) * 3i + ({d}.y + 1i))"));
            }
            Operation::StepOffset { code } => {
                let code = self.slot(block, *code, node);
                let_out!(format!(
                    "vec2<i32>(bitcast<i32>({code} / 3u) - 1i, bitcast<i32>({code} % 3u) - 1i)"
                ));
            }
            Operation::Choice3 { bits } => {
                let bits = self.slot(block, *bits, node);
                let_out!(format!("i32({bits} % 3u) - 1i"));
            }
            Operation::Reservoir { bits, count } => {
                let (bits, count) = (self.slot(block, *bits, node), self.slot(block, *count, node));
                self.uses.random_float = true;
                let_out!(format!("random_float({bits}, 1.0f) < 1.0f / f32({count})"));
            }
            Operation::GridFold(fold) => {
                text.push(&lines);
                lines.clear();
                self.grid_fold(block, node, fold, indent, text);
            }
            Operation::RadiusFold(fold) => {
                text.push(&lines);
                lines.clear();
                self.radius_fold(block, node, fold, indent, text);
            }
            Operation::Sum { value, sum } => {
                if self.role != Role::Reduce {
                    self.refuse(
                        &pass_id,
                        Some(node),
                        "sums floats, which the GPU reduces in a stats pass alone",
                    );
                    return;
                }
                let vector = Self::type_of(block, *value).is_some_and(WgslType::is_vector);
                let value = self.slot(block, *value, node);
                let lane = self.sum_lanes.len();
                self.sum_lanes.push((block.position, *sum));
                if vector {
                    self.sum_lanes.push((block.position, *sum));
                    lines.push_str(&format!(
                        "{indent}henad_sum_{lane} = henad_sum_{lane} + {value}.x;\n{indent}henad_sum_{} = \
                         henad_sum_{} + {value}.y;\n",
                        lane + 1,
                        lane + 1
                    ));
                } else {
                    lines.push_str(&format!("{indent}henad_sum_{lane} = henad_sum_{lane} + {value};\n"));
                }
            }
        }
        if !lines.is_empty() {
            text.node_line(&pass_id, node, &lines);
        }
    }

    /// Writes a Repeat and its body as a counted loop.
    fn repeat(&mut self, block: &mut Block<'_>, node: NodeId, repeat: &Repeat, indent: &str, text: &mut Text) {
        let pass_id = block.pass.id.clone();
        let mut head = self.seed_accumulators(block, node, &repeat.accumulators, indent);
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
        head.push_str(&self.remember_accumulators(block, node, &repeat.accumulators, &inner));
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

    /// Returns the label of the buffer of the field at `field` of the agents plan.
    fn field_label(&self, field: usize) -> String {
        self.input
            .agents
            .and_then(|plan| plan.fields.get(field))
            .map(FieldPlan::label)
            .unwrap_or_default()
    }

    /// Returns the label of the buffer of the deposits of the field at `field` of the agents plan.
    fn accum_label(&self, field: usize) -> String {
        self.input
            .agents
            .and_then(|plan| plan.fields.get(field))
            .map(FieldPlan::accum_label)
            .unwrap_or_default()
    }

    /// Returns the WGSL `bool` of whether a stencil skips the positions past the edge, `bounded` or the domain grid's
    /// own boundary when it is `None`.
    fn bounded(&self, bounded: Option<bool>) -> &'static str {
        let own = matches!(
            self.input.document.world,
            crate::document::World::Grid(grid) if grid.boundary == Boundary::Bounded
        );
        if bounded.unwrap_or(own) { "true" } else { "false" }
    }

    /// Writes the head of a region's accumulators: one `var` each, seeded from its `init` or its identity.
    fn seed_accumulators(
        &mut self,
        block: &mut Block<'_>,
        node: NodeId,
        accumulators: &[RepeatAccumulator],
        indent: &str,
    ) -> String {
        let mut head = String::new();
        for accumulator in accumulators {
            let running = self.target(block, accumulator.running, node);
            let start = match accumulator.init {
                Some(slot) => self.slot(block, slot, node),
                None => literal(accumulator.identity).unwrap_or_else(|| {
                    self.refuse(&block.pass.id, Some(node), "holds an accumulator the GPU cannot hold");
                    "0u".to_owned()
                }),
            };
            let type_name = WgslType::of_value(accumulator.identity).map_or("u32", WgslType::name);
            head.push_str(&format!("{indent}var {running}: {type_name} = {start};\n"));
        }
        head
    }

    /// Returns the lines that copy each accumulator's running value to its `prev.<acc>`.
    fn remember_accumulators(
        &mut self,
        block: &Block<'_>,
        node: NodeId,
        accumulators: &[RepeatAccumulator],
        indent: &str,
    ) -> String {
        let mut lines = String::new();
        for accumulator in accumulators {
            let prev = self.target(block, accumulator.prev, node);
            let running = self.target(block, accumulator.running, node);
            lines.push_str(&format!("{indent}let {prev} = {running};\n"));
        }
        lines
    }

    /// Writes a stencil fold and its body as a loop over its offsets, each position past the edge skipped.
    fn grid_fold(&mut self, block: &mut Block<'_>, node: NodeId, fold: &StencilFold, indent: &str, text: &mut Text) {
        let pass_id = block.pass.id.clone();
        let n = node.number();
        let mut head = self.seed_accumulators(block, node, &fold.accumulators, indent);
        let when = self.slot(block, fold.when, node);
        let center = if let Some(slot) = fold.at {
            self.slot(block, slot, node)
        } else {
            self.uses.own_cell = true;
            "vec2<i32>(henad_own_cell)".to_owned()
        };
        let bounded = self.bounded(fold.bounded);
        self.uses.step = true;
        let radix = fold.offsets.len();
        let offsets: Vec<String> = fold
            .offsets
            .iter()
            .map(|(dx, dy)| format!("vec2<i32>({dx}i, {dy}i)"))
            .collect();
        let [k_slot, offset_slot, xy_slot, cell_slot, diagonal_slot] = fold.inner;
        let k = self.target(block, k_slot, node);
        let offset = self.target(block, offset_slot, node);
        let xy = self.target(block, xy_slot, node);
        let cell = self.target(block, cell_slot, node);
        let diagonal = self.target(block, diagonal_slot, node);
        head.push_str(&format!(
            "{indent}var henad_offsets_n{n} = array<vec2<i32>, {radix}>({});\n",
            offsets.join(", ")
        ));
        head.push_str(&format!("{indent}if ({when}) {{\n"));
        head.push_str(&format!("{indent}    let henad_center_n{n} = {center};\n"));
        head.push_str(&format!(
            "{indent}    for (var {k} = 0u; {k} < {radix}u; {k} = {k} + 1u) {{\n"
        ));
        let inner = format!("{indent}        ");
        head.push_str(&format!("{inner}let {offset} = henad_offsets_n{n}[{k}];\n"));
        head.push_str(&format!(
            "{inner}let henad_at_n{n} = henad_step(henad_center_n{n}, {offset}, {bounded});\n"
        ));
        head.push_str(&format!(
            "{inner}if (henad_at_n{n}.z == 0i) {{\n{inner}    continue;\n{inner}}}\n"
        ));
        head.push_str(&format!("{inner}let {xy} = henad_at_n{n}.xy;\n"));
        head.push_str(&format!(
            "{inner}let {cell} = u32({xy}.y) * params.henad_width + u32({xy}.x);\n"
        ));
        head.push_str(&format!(
            "{inner}let {diagonal} = {offset}.x != 0i && {offset}.y != 0i;\n"
        ));
        head.push_str(&self.remember_accumulators(block, node, &fold.accumulators, &inner));
        let iteration = format!("henad_iteration_n{n}");
        let word = if block.iteration == "0u" && self.input.outer_iteration.is_none() {
            k.clone()
        } else {
            format!("{} * {radix}u + {k}", block.iteration)
        };
        head.push_str(&format!("{inner}let {iteration} = {word};\n"));
        text.node_line(&pass_id, node, &head);
        let outer = std::mem::replace(&mut block.iteration, iteration);
        for instruction in &fold.body {
            self.instruction(block, instruction, &inner, text);
        }
        block.iteration = outer;
        text.node_line(&pass_id, node, &format!("{indent}    }}\n{indent}}}\n"));
    }

    /// Writes a radius fold and its body as the walk of `henad::hash` over the neighbour index, which stops and sets
    /// the fold's fault code past [`crate::limits::MAX_FOLD_CANDIDATES`] candidates, and does not start past
    /// [`crate::limits::MAX_WALK_CELLS`] cells, setting the code after it.
    #[expect(
        clippy::too_many_lines,
        reason = "the walk and the visit of one candidate read in order"
    )]
    fn radius_fold(&mut self, block: &mut Block<'_>, node: NodeId, fold: &RadiusFold, indent: &str, text: &mut Text) {
        let pass_id = block.pass.id.clone();
        let n = node.number();
        let code = self
            .input
            .agents
            .and_then(|plan| {
                plan.faults
                    .iter()
                    .position(|(pass, fault)| *pass == pass_id && *fault == node)
            })
            .map(|position| 2 * position + 1);
        let Some(code) = code else {
            self.refuse(
                &pass_id,
                Some(node),
                "walks a neighbour index outside a tick pass of an agents world",
            );
            return;
        };
        let mut head = self.seed_accumulators(block, node, &fold.accumulators, indent);
        let center = self.slot(block, fold.center, node);
        let radius = self.slot(block, fold.radius, node);
        let when = self.slot(block, fold.when, node);
        self.uses.hash = true;
        self.uses.counters = true;
        self.uses.space.insert("axis_delta");
        self.uses.space.insert("TORUS");
        self.field("henad_world", FieldShape::Value(WgslType::Vec2F), FieldSource::World);
        self.field(
            "henad_hash_grid",
            FieldShape::Value(WgslType::Vec2U),
            FieldSource::HashGrid,
        );
        self.field(
            "henad_hash_cell",
            FieldShape::Value(WgslType::Vec2F),
            FieldSource::HashCell,
        );
        self.field(
            "henad_hash_inv",
            FieldShape::Value(WgslType::Vec2F),
            FieldSource::HashInverse,
        );
        let [k_slot, other_slot, delta_slot, dist_slot] = fold.inner;
        let k = self.target(block, k_slot, node);
        let other = self.target(block, other_slot, node);
        let delta = self.target(block, delta_slot, node);
        let dist = self.target(block, dist_slot, node);
        let position = format!("henad_there_n{n}");
        let load = if let Some(lane) = self.input.agents.and_then(|plan| plan.position_lane) {
            self.load(lane, &format!("henad_other_n{n}"))
        } else {
            self.refuse(
                &pass_id,
                Some(node),
                "walks the agents of a world with no position lane",
            );
            "vec2<f32>(0.0f, 0.0f)".to_owned()
        };
        let skip_self = if fold.exclude_self {
            format!(" && henad_other_n{n} != henad_index")
        } else {
            String::new()
        };
        let i1 = format!("{indent}    ");
        let i2 = format!("{indent}        ");
        let i3 = format!("{indent}            ");
        let i4 = format!("{indent}                ");
        head.push_str(&format!("{indent}var henad_found_n{n} = 0u;\n"));
        head.push_str(&format!("{indent}if ({when}) {{\n"));
        head.push_str(&format!("{i1}let henad_center_n{n} = {center};\n"));
        head.push_str(&format!("{i1}let henad_reach_n{n} = {radius} * {radius};\n"));
        head.push_str(&format!(
            "{i1}let henad_bounds_n{n} = walk_bounds(henad_center_n{n}.x, henad_center_n{n}.y, {radius}, \
             params.henad_hash_grid, params.henad_hash_cell, params.henad_hash_inv);\n"
        ));
        head.push_str(&format!(
            "{i1}var henad_visits_n{n} = 0u;\n{i1}var henad_over_n{n} = false;\n"
        ));
        head.push_str(&format!(
            "{i1}if (walk_cells(henad_bounds_n{n}) > MAX_WALK_CELLS) {{\n{i1}    atomicMax(&counters[{FAULT_WORD}u], \
             {}u);\n{i1}    henad_over_n{n} = true;\n{i1}}}\n",
            code + 1
        ));
        head.push_str(&format!(
            "{i1}for (var henad_row_n{n} = henad_bounds_n{n}.z; henad_row_n{n} <= henad_bounds_n{n}.w && \
             !henad_over_n{n}; henad_row_n{n} = henad_row_n{n} + 1i) {{\n"
        ));
        head.push_str(&format!(
            "{i2}for (var henad_column_n{n} = henad_bounds_n{n}.x; henad_column_n{n} <= henad_bounds_n{n}.y && \
             !henad_over_n{n}; henad_column_n{n} = henad_column_n{n} + 1i) {{\n"
        ));
        head.push_str(&format!(
            "{i3}let henad_walk_cell_n{n} = walk_cell(henad_column_n{n}, henad_row_n{n}, params.henad_hash_grid);\n"
        ));
        head.push_str(&format!(
            "{i3}let henad_end_n{n} = cell_start[henad_walk_cell_n{n} + 1u];\n"
        ));
        head.push_str(&format!(
            "{i3}for (var henad_slot_n{n} = cell_start[henad_walk_cell_n{n}]; henad_slot_n{n} < henad_end_n{n}; \
             henad_slot_n{n} = henad_slot_n{n} + 1u) {{\n"
        ));
        head.push_str(&format!("{i4}henad_visits_n{n} = henad_visits_n{n} + 1u;\n"));
        head.push_str(&format!(
            "{i4}if (henad_visits_n{n} > MAX_VISITS) {{\n{i4}    atomicMax(&counters[{FAULT_WORD}u], {code}u);\n{i4}    \
             henad_over_n{n} = true;\n{i4}    break;\n{i4}}}\n"
        ));
        head.push_str(&format!("{i4}let henad_other_n{n} = sorted[henad_slot_n{n}];\n"));
        head.push_str(&format!("{i4}let {position} = {load};\n"));
        head.push_str(&format!(
            "{i4}let henad_delta_n{n} = vec2<f32>(axis_delta(henad_center_n{n}.x, {position}.x, params.henad_world.x, \
             TORUS), axis_delta(henad_center_n{n}.y, {position}.y, params.henad_world.y, TORUS));\n"
        ));
        head.push_str(&format!(
            "{i4}let henad_dist_n{n} = henad_delta_n{n}.x * henad_delta_n{n}.x + henad_delta_n{n}.y * \
             henad_delta_n{n}.y;\n"
        ));
        head.push_str(&format!("{i4}if (henad_dist_n{n} <= henad_reach_n{n}{skip_self}) {{\n"));
        let inner = format!("{indent}                    ");
        head.push_str(&format!("{inner}let {k} = henad_found_n{n};\n"));
        head.push_str(&format!("{inner}let {other} = henad_other_n{n};\n"));
        head.push_str(&format!("{inner}let {delta} = henad_delta_n{n};\n"));
        head.push_str(&format!("{inner}let {dist} = henad_dist_n{n};\n"));
        head.push_str(&self.remember_accumulators(block, node, &fold.accumulators, &inner));
        let iteration = format!("henad_iteration_n{n}");
        head.push_str(&format!("{inner}let {iteration} = henad_other_n{n};\n"));
        text.node_line(&pass_id, node, &head);
        let outer = std::mem::replace(&mut block.iteration, iteration);
        for instruction in &fold.body {
            self.instruction(block, instruction, &inner, text);
        }
        block.iteration = outer;
        text.node_line(
            &pass_id,
            node,
            &format!("{inner}henad_found_n{n} = henad_found_n{n} + 1u;\n{i4}}}\n{i3}}}\n{i2}}}\n{i1}}}\n{indent}}}\n"),
        );
    }

    /// Returns the constant of the packed RGBA words of the palette that the `view.color` node `node` names, declaring
    /// it once.
    fn packed_palette_of(&mut self, block: &Block<'_>, node: NodeId) -> Option<String> {
        let found = block.pass.body.nodes.iter().find(|candidate| candidate.id == node)?;
        let def = crate::catalogue::lookup(&found.op, self.input.document.library)?;
        let config = effective_config(def, found);
        let id = config.get("palette").and_then(ConfigValue::as_str)?;
        let constant = self.palette(id)?;
        let colors = self.palettes.remove(&constant)?;
        let packed = constant.replacen("henad_", "henad_packed_", 1);
        self.packed.insert(packed.clone(), colors);
        Some(packed)
    }

    /// Returns the colour pass of an agents world with no view pass over agents: the first category lane over agents
    /// coloured by its category, or white.
    fn default_agent_color(&mut self) -> String {
        let document = self.input.document;
        let lane = document
            .lanes
            .iter()
            .enumerate()
            .find_map(|(position, lane)| match &lane.value_type {
                crate::types::ValueType::Category(category) if lane.domain == Domain::Agents => {
                    Some((position, category.clone()))
                }
                _ => None,
            });
        let Some((position, category)) = lane else {
            return "    // No view pass over agents and no category lane: every agent is white.\n".to_owned();
        };
        let Some(constant) = self.palette(&category) else {
            return String::new();
        };
        let Some(colors) = self.palettes.remove(&constant) else {
            return String::new();
        };
        let last = colors.len() - 1;
        let packed = constant.replacen("henad_", "henad_packed_", 1);
        self.packed.insert(packed.clone(), colors);
        let load = self.load(position, "henad_index");
        format!(
            "    // No view pass over agents: the lane '{}' coloured by its category.\n    if (henad_index < \
             params.henad_agents) {{\n        henad_color = {packed}[min({load}, {last}u)];\n    }}\n",
            document.lanes[position].id
        )
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

    /// Writes the head of a shader into `text`: its comment, imports, uniform block, bindings, palettes and helpers.
    #[expect(
        clippy::too_many_lines,
        reason = "each import and helper reads best beside the others"
    )]
    fn header(&mut self, text: &mut Text, uniform: &UniformBlock, declarations: &str) {
        let agents = self.input.agents.is_some();
        text.push("// Generated by henad-graph. Every value is named after its node, n<id>_<port>.\n");
        let mut rng = Vec::new();
        if self.uses.pcg_hash {
            rng.push("pcg_hash");
        }
        if self.uses.random_float {
            rng.push("random_float");
        }
        rng.extend(self.uses.rng.iter().copied());
        if !rng.is_empty() {
            text.text
                .push_str(&format!("#import henad::rng::{{{}}}\n", rng.join(", ")));
        }
        if self.uses.draw_core {
            text.push("#import henad_graph::draw::draw_core\n");
        }
        let mut space: Vec<&str> = if self.uses.neighbors {
            vec!["offset_cell", "TORUS", "BOUNDED"]
        } else {
            Vec::new()
        };
        if self.uses.step {
            self.uses.space.insert("wrap_index");
        }
        for name in &self.uses.space {
            if !space.contains(name) {
                space.push(name);
            }
        }
        if !space.is_empty() {
            text.text
                .push_str(&format!("#import henad::space::{{{}}}\n", space.join(", ")));
        }
        if self.role == Role::Display {
            text.push("#import henad::dims::{Dims, cell_at}\n");
        }
        if agents && self.role != Role::Display {
            text.push("#import henad::dispatch::linear_index\n");
        }
        if self.role == Role::Reduce && !self.sum_lanes.is_empty() {
            text.push("#import henad::reduce_tree::block_sum\n");
        }
        if self.uses.hash {
            text.push("#import henad::hash::{walk_bounds, walk_cell, walk_cells, MAX_VISITS, MAX_WALK_CELLS}\n");
        }
        text.push("\n");
        text.push(&uniform.declaration());
        text.push("\n");
        text.push(declarations);
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
        for (constant, colors) in &self.packed {
            let words: Vec<String> = colors
                .iter()
                .map(|color| format!("{:#010x}u", packed_color(*color)))
                .collect();
            text.text.push_str(&format!(
                "const {constant} = array<u32, {}>({});\n\n",
                colors.len(),
                words.join(", ")
            ));
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
        if self.uses.step {
            text.push(
                "// The position `by` away from `xy`, wrapped unless `bounded`, and in `.z` whether it is on the domain\n\
                 // grid.\n\
                 fn henad_step(xy: vec2<i32>, by: vec2<i32>, bounded: bool) -> vec3<i32> {\n    \
                 var next = xy + by;\n    \
                 let side = vec2<i32>(i32(params.henad_width), i32(params.henad_height));\n    \
                 if (!bounded) {\n        \
                 next = vec2<i32>(wrap_index(next.x, side.x), wrap_index(next.y, side.y));\n    \
                 }\n    \
                 let on_grid = next.x >= 0i && next.y >= 0i && next.x < side.x && next.y < side.y;\n    \
                 return vec3<i32>(next, select(0i, 1i, on_grid));\n\
                 }\n\n",
            );
        }
    }

    /// Returns the shader of an agents world: its imports, uniform block, bindings and helpers, then `main` around
    /// `body`, a linear pass for every role but the display.
    #[expect(
        clippy::too_many_lines,
        reason = "the bindings and the template of every role read best in one place"
    )]
    fn assemble_agents(mut self, body: Text) -> Emitted {
        let Some(plan) = self.input.agents else {
            unreachable!("an agents shader has a plan");
        };
        let tallies = self.tally_offset;
        let tally_base = FIRST_COUNTER + plan.counters;
        let mut bindings: Vec<ShaderBinding> = Vec::new();
        let mut declarations = String::new();
        let storage = |read_only: bool| BindingKind::Storage { read_only };
        let bind = |bindings: &mut Vec<ShaderBinding>, declarations: &mut String, name: &str, kind, wgsl: &str| {
            declarations.push_str(&format!("@group(0) @binding({}) {wgsl};\n", bindings.len()));
            bindings.push(ShaderBinding::new(name.to_owned(), kind));
        };
        for (lane, element) in self.bound_lanes() {
            let (read, write) = self.lane_bindings(lane);
            let writes = self
                .input
                .layout
                .group_of(lane)
                .is_some_and(|group| self.group_written(group));
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
        let merged = match self.role {
            Role::Merge(field) => Some(field),
            _ => None,
        };
        for (index, field) in plan.fields.iter().enumerate() {
            let read = self.fields_read.contains(&index);
            let written = self.fields_written.contains(&index) || merged == Some(index);
            if read || written {
                let access = if written { "read_write" } else { "read" };
                let label = field.label();
                bind(
                    &mut bindings,
                    &mut declarations,
                    &label,
                    storage(!written),
                    &format!("var<storage, {access}> {label}: array<{}>", field.wgsl.stored()),
                );
            }
            if self.deposited.contains(&index) || merged == Some(index) {
                let label = field.accum_label();
                bind(
                    &mut bindings,
                    &mut declarations,
                    &label,
                    storage(false),
                    &format!("var<storage, read_write> {label}: array<atomic<u32>>"),
                );
            }
        }
        // The tick lives in the counters, read with an atomic load, and the step's last pass advances it.
        let writes_tick = matches!(self.role, Role::Step { writes_tick: true });
        if self.reads_tick || writes_tick || !self.fire_slots.is_empty() {
            self.uses.counters = true;
        }
        if self.uses.hash {
            bind(
                &mut bindings,
                &mut declarations,
                "cell_start",
                storage(true),
                "var<storage, read> cell_start: array<u32>",
            );
            bind(
                &mut bindings,
                &mut declarations,
                "sorted",
                storage(true),
                "var<storage, read> sorted: array<u32>",
            );
        }
        if self.uses.counters || (self.role == Role::Reduce && tallies > 0) {
            bind(
                &mut bindings,
                &mut declarations,
                "counters",
                storage(false),
                &format!(
                    "var<storage, read_write> counters: array<atomic<u32>, {}>",
                    plan.counter_words.max(1)
                ),
            );
        }
        if self.role == Role::Reduce && !self.sum_lanes.is_empty() {
            bind(
                &mut bindings,
                &mut declarations,
                "partials",
                storage(false),
                "var<storage, read_write> partials: array<f32>",
            );
        }
        if self.role == Role::Color {
            bind(
                &mut bindings,
                &mut declarations,
                AGENT_COLORS,
                storage(false),
                &format!("var<storage, read_write> {AGENT_COLORS}: array<u32>"),
            );
        }
        if self.role == Role::Display {
            bind(
                &mut bindings,
                &mut declarations,
                "output",
                BindingKind::StorageTexture,
                "var output: texture_storage_2d<rgba8unorm, write>",
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

        let mut text = Text::default();
        self.header(&mut text, &uniform, &declarations);
        if self.role == Role::Reduce && tallies > 0 {
            text.text.push_str(&format!(
                "var<workgroup> henad_partial: array<atomic<u32>, {tallies}>;\n\n"
            ));
        }
        let counted = self.counter_slots.len();
        if counted > 0 {
            text.text.push_str(&format!(
                "var<workgroup> henad_counter_tally: array<atomic<u32>, {counted}>;\n\n"
            ));
        }
        let fired = self.fire_slots.len();
        if fired > 0 {
            text.text.push_str(&format!(
                "var<workgroup> henad_fire_tally: array<atomic<u32>, {fired}>;\n\n"
            ));
        }
        let tick_line = if self.reads_tick {
            format!("    let henad_tick = atomicLoad(&counters[{TICK_WORD}u]);\n")
        } else {
            String::new()
        };
        if self.role == Role::Display {
            text.text.push_str(&format!(
                "@compute @workgroup_size({WORKGROUP_SIZE}, {WORKGROUP_SIZE})\n"
            ));
            text.push("fn main(@builtin(global_invocation_id) henad_id: vec3<u32>) {\n");
            text.push(
                "    if (henad_id.x >= params.henad_texture.x || henad_id.y >= params.henad_texture.y) {\n        \
                 return;\n    }\n",
            );
            text.push(
                "    let henad_xy = cell_at(henad_id.xy, Dims(vec2<u32>(params.henad_width, params.henad_height), \
                 params.henad_texture));\n",
            );
            if self.uses.neighbors {
                text.push("    henad_cell = henad_xy;\n");
            }
            if self.uses.own_cell {
                text.push("    let henad_own_cell = henad_xy;\n");
            }
            text.push(&tick_line);
            text.push("    let henad_index = henad_xy.y * params.henad_width + henad_xy.x;\n");
            text.push("    var henad_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);\n");
            text.append(body);
            text.push("    textureStore(output, vec2<i32>(henad_id.xy), henad_color);\n}\n");
        } else {
            text.text
                .push_str(&format!("@compute @workgroup_size({LINEAR_WORKGROUP})\n"));
            text.push(
                "fn main(\n    @builtin(local_invocation_id) henad_lid: vec3<u32>,\n    \
                 @builtin(workgroup_id) henad_wid: vec3<u32>,\n) {\n",
            );
            // A merge runs once per cell of each layer, its slot. Its body sees the slot's cell as its element.
            let invocation = if matches!(self.role, Role::Merge(_)) {
                "henad_slot"
            } else {
                "henad_index"
            };
            text.text.push_str(&format!(
                "    let {invocation} = linear_index(henad_lid, henad_wid, params.henad_groups_x);\n"
            ));
            text.push(&tick_line);
            if writes_tick {
                text.text.push_str(&format!(
                    "    if ({invocation} == 0u) {{\n        atomicAdd(&counters[{TICK_WORD}u], 1u);\n    }}\n"
                ));
            }
            if counted > 0 {
                // A barrier needs uniform control flow, so it sits at the top level of `main`, and no step returns
                // early.
                text.text.push_str(&format!(
                    "    if (henad_lid.x < {counted}u) {{\n        atomicStore(&henad_counter_tally[henad_lid.x], \
                     0u);\n    }}\n    workgroupBarrier();\n"
                ));
            }
            text.text
                .push_str(&self.fire_zero_lines("henad_lid.x", LINEAR_WORKGROUP));
            match self.role {
                Role::Reduce => {
                    if tallies > 0 {
                        text.text.push_str(&format!(
                            "    for (var henad_c = henad_lid.x; henad_c < {tallies}u; henad_c = henad_c + \
                             {LINEAR_WORKGROUP}u) {{\n        atomicStore(&henad_partial[henad_c], 0u);\n    }}\n    \
                             workgroupBarrier();\n"
                        ));
                    }
                    for lane in 0..self.sum_lanes.len() {
                        text.text.push_str(&format!("    var henad_sum_{lane} = 0.0f;\n"));
                    }
                    text.append(body);
                    if tallies > 0 {
                        text.text.push_str(&format!(
                            "    workgroupBarrier();\n    for (var henad_c = henad_lid.x; henad_c < {tallies}u; \
                             henad_c = henad_c + {LINEAR_WORKGROUP}u) {{\n        let henad_count = \
                             atomicLoad(&henad_partial[henad_c]);\n        if (henad_count > 0u) {{\n            \
                             atomicAdd(&counters[{tally_base}u + henad_c], henad_count);\n        }}\n    }}\n"
                        ));
                    }
                    let lanes = self.sum_lanes.len();
                    if lanes > 0 {
                        text.push("    let henad_block = henad_wid.y * params.henad_groups_x + henad_wid.x;\n");
                    }
                    for lane in 0..lanes {
                        text.text.push_str(&format!(
                            "    let henad_total_{lane} = block_sum(henad_lid.x, henad_sum_{lane});\n    if \
                             (henad_lid.x == 0u) {{\n        partials[henad_block * {lanes}u + {lane}u] = \
                             henad_total_{lane};\n    }}\n"
                        ));
                    }
                }
                Role::Color => {
                    text.push("    var henad_color = 4294967295u;\n");
                    text.append(body);
                    text.text.push_str(&format!(
                        "    if (henad_index < params.henad_agents) {{\n        {AGENT_COLORS}[henad_index] = \
                         henad_color;\n    }}\n"
                    ));
                }
                Role::Merge(field) => {
                    let field = &plan.fields[field];
                    let label = field.label();
                    let accum = field.accum_label();
                    text.text.push_str(&format!(
                        "    if (henad_slot < params.henad_cells * {}u) {{\n",
                        field.layers
                    ));
                    let mut inner = Text::default();
                    inner.push("    let henad_index = henad_slot % params.henad_cells;\n");
                    // A sum's word as a count of its resolution is exact in `f32` below 2^24, and so is its product
                    // by the power of two.
                    let (read, merged) = match field.combine.and_then(FieldCombine::resolution) {
                        Some(resolution) => (
                            format!("let henad_word = atomicLoad(&{accum}[henad_slot]);"),
                            format!(
                                "{label}[henad_slot] + f32(henad_word) * {}",
                                literal(HostValue::F32(resolution)).unwrap_or_default()
                            ),
                        ),
                        None => (
                            format!("let henad_deposit = bitcast<f32>(atomicLoad(&{accum}[henad_slot]));"),
                            format!("max({label}[henad_slot], henad_deposit)"),
                        ),
                    };
                    inner.text.push_str(&format!(
                        "    {read}\n    atomicStore(&{accum}[henad_slot], 0u);\n    let henad_merged = {merged};\n"
                    ));
                    if body.text.is_empty() {
                        inner
                            .text
                            .push_str(&format!("    {label}[henad_slot] = henad_merged;\n"));
                    } else {
                        inner.append(body);
                    }
                    text.append(inner.indented());
                    text.push("    }\n");
                }
                _ => text.append(body),
            }
            if counted > 0 {
                // Each counter takes one global atomic per workgroup, from the invocation of its slot.
                text.push("    workgroupBarrier();\n");
                for (&counter, &slot) in &self.counter_slots {
                    text.text.push_str(&format!(
                        "    if (henad_lid.x == {slot}u) {{\n        let henad_tally_{slot} = \
                         atomicLoad(&henad_counter_tally[{slot}u]);\n        if (henad_tally_{slot} != 0u) {{\n            \
                         atomicAdd(&counters[{word}u], henad_tally_{slot});\n        }}\n    }}\n",
                        word = FIRST_COUNTER + counter
                    ));
                }
            }
            text.text
                .push_str(&self.fire_flush_lines("henad_lid.x", LINEAR_WORKGROUP));
            text.push("}\n");
        }
        let workgroup_bytes = match self.role {
            Role::Reduce => {
                let sums = if self.sum_lanes.is_empty() {
                    0
                } else {
                    u64::from(LINEAR_WORKGROUP) * 4
                };
                tallies as u64 * 4 + sums
            }
            _ => (counted + fired) as u64 * 4,
        };
        Emitted {
            text: text.text,
            ranges: text.ranges,
            bindings,
            uniform,
            counters: tallies,
            sum_lanes: self.sum_lanes,
            workgroup_bytes,
        }
    }

    /// Returns the shader: its imports, uniform block, bindings and helpers, then `main` around `body`.
    #[expect(
        clippy::too_many_lines,
        reason = "the template of every role reads best in one place"
    )]
    fn assemble(mut self, body: Text) -> Emitted {
        if self.input.agents.is_some() {
            return self.assemble_agents(body);
        }
        let counters = self.tally_offset.max(1);
        let mut bindings: Vec<ShaderBinding> = Vec::new();
        let mut declarations = String::new();
        let storage = |read_only: bool| BindingKind::Storage { read_only };
        let bind = |bindings: &mut Vec<ShaderBinding>, declarations: &mut String, name: &str, kind, wgsl: &str| {
            declarations.push_str(&format!("@group(0) @binding({}) {wgsl};\n", bindings.len()));
            bindings.push(ShaderBinding::new(name.to_owned(), kind));
        };
        for (lane, element) in self.bound_lanes() {
            let (read, write) = self.lane_bindings(lane);
            let writes = self
                .input
                .layout
                .group_of(lane)
                .is_some_and(|group| self.group_written(group));
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
        let fired = self.fire_slots.len();
        if fired > 0 {
            // The reduce tallies come first, and the step adds to the probes' words after them.
            bind(
                &mut bindings,
                &mut declarations,
                "counters",
                storage(false),
                "var<storage, read_write> counters: array<atomic<u32>>",
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
            Role::Reduce => {
                bind(
                    &mut bindings,
                    &mut declarations,
                    "counters",
                    storage(false),
                    &format!("var<storage, read_write> counters: array<atomic<u32>, {counters}>"),
                );
                if !self.sum_lanes.is_empty() {
                    bind(
                        &mut bindings,
                        &mut declarations,
                        "partials",
                        storage(false),
                        "var<storage, read_write> partials: array<f32>",
                    );
                }
            }
            Role::Step { .. } | Role::Action | Role::Color | Role::Merge(_) => {}
        }

        let mut text = Text::default();
        self.header(&mut text, &uniform, &declarations);
        if self.role == Role::Reduce {
            text.text.push_str(&format!(
                "var<workgroup> henad_partial: array<atomic<u32>, {counters}>;\n\n"
            ));
        }
        if fired > 0 {
            text.text.push_str(&format!(
                "var<workgroup> henad_fire_tally: array<atomic<u32>, {fired}>;\n\n"
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
            Role::Step { .. } | Role::Action | Role::Color | Role::Merge(_) => {
                if fired > 0 {
                    text.push(
                        "fn main(\n    @builtin(global_invocation_id) henad_id: vec3<u32>,\n    \
                         @builtin(local_invocation_index) henad_local: u32,\n) {\n",
                    );
                } else {
                    text.push("fn main(@builtin(global_invocation_id) henad_id: vec3<u32>) {\n");
                }
                text.push(&tick_line);
                if writes_tick {
                    text.push(
                        "    if (henad_id.x == 0u && henad_id.y == 0u) {\n        henad_tick_out[0] = henad_tick + 1u;\n    }\n",
                    );
                }
                text.text
                    .push_str(&self.fire_zero_lines("henad_local", WORKGROUP_SIZE * WORKGROUP_SIZE));
                // The bounds check guards a block. An early return would leave the end of `main` in non-uniform
                // control flow.
                text.push("    if (henad_id.x < params.henad_width && henad_id.y < params.henad_height) {\n");
                let mut inner = Text::default();
                inner.push(cell_line);
                if self.uses.own_cell {
                    inner.push("    let henad_own_cell = henad_id.xy;\n");
                }
                inner.push("    let henad_index = henad_id.y * params.henad_width + henad_id.x;\n");
                inner.append(body);
                text.append(inner.indented());
                text.push("    }\n");
                text.text
                    .push_str(&self.fire_flush_lines("henad_local", WORKGROUP_SIZE * WORKGROUP_SIZE));
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
                if self.uses.own_cell {
                    text.push("    let henad_own_cell = henad_xy;\n");
                }
                text.push("    let henad_index = henad_xy.y * params.henad_width + henad_xy.x;\n");
                text.push("    var henad_color = vec4<f32>(0.0, 0.0, 0.0, 1.0);\n");
                text.append(body);
                text.push("    textureStore(output, vec2<i32>(henad_id.xy), henad_color);\n}\n");
            }
            Role::Reduce => {
                let lanes = self.sum_lanes.len();
                let workgroup_line = if lanes > 0 {
                    "    @builtin(workgroup_id) henad_wid: vec3<u32>,\n"
                } else {
                    ""
                };
                text.text.push_str(&format!(
                    "fn main(\n    @builtin(global_invocation_id) henad_id: vec3<u32>,\n    @builtin(local_invocation_index) henad_local: u32,\n{workgroup_line}) {{\n",
                ));
                text.text.push_str(&format!("    for (var henad_c = henad_local; henad_c < {counters}u; henad_c = henad_c + {}u) {{\n        atomicStore(&henad_partial[henad_c], 0u);\n    }}\n    workgroupBarrier();\n", WORKGROUP_SIZE * WORKGROUP_SIZE));
                for lane in 0..lanes {
                    text.text.push_str(&format!("    var henad_sum_{lane} = 0.0f;\n"));
                }
                text.push(&tick_line);
                text.push("    if (henad_id.x < params.henad_width && henad_id.y < params.henad_height) {\n");
                let mut inner = Text::default();
                inner.push(cell_line);
                if self.uses.own_cell {
                    inner.push("    let henad_own_cell = henad_id.xy;\n");
                }
                inner.push("    let henad_index = henad_id.y * params.henad_width + henad_id.x;\n");
                inner.append(body);
                text.append(inner.indented());
                text.push("    }\n    workgroupBarrier();\n");
                text.text.push_str(&format!("    for (var henad_c = henad_local; henad_c < {counters}u; henad_c = henad_c + {}u) {{\n        let henad_count = atomicLoad(&henad_partial[henad_c]);\n        if (henad_count > 0u) {{\n            atomicAdd(&counters[henad_c], henad_count);\n        }}\n    }}\n", WORKGROUP_SIZE * WORKGROUP_SIZE));
                if lanes > 0 {
                    // Every invocation reaches each `block_sum`, out of the cell's bounds check, since it barriers.
                    text.text.push_str(&format!(
                        "    let henad_block = henad_wid.y * ((params.henad_width + {side}u) / {WORKGROUP_SIZE}u) + \
                         henad_wid.x;\n",
                        side = WORKGROUP_SIZE - 1
                    ));
                }
                for lane in 0..lanes {
                    text.text.push_str(&format!(
                        "    let henad_total_{lane} = block_sum(henad_local, henad_sum_{lane});\n    if (henad_local == \
                         0u) {{\n        partials[henad_block * {lanes}u + {lane}u] = henad_total_{lane};\n    }}\n"
                    ));
                }
                text.push("}\n");
            }
        }
        Emitted {
            text: text.text,
            ranges: text.ranges,
            bindings,
            uniform,
            counters,
            workgroup_bytes: if self.role == Role::Reduce {
                let sums = if self.sum_lanes.is_empty() {
                    0
                } else {
                    u64::from(WORKGROUP_SIZE * WORKGROUP_SIZE) * 4
                };
                counters as u64 * 4 + sums
            } else {
                fired as u64 * 4
            },
            sum_lanes: self.sum_lanes,
        }
    }
}

/// Names the running value of every accumulator of every region in `block` after the accumulator's output, which
/// takes the running value once the loop ends.
fn name_running(block: &[Instruction], names: &mut [SlotName], types: &mut [Option<WgslType>]) {
    for instruction in block {
        let (accumulators, body) = match &instruction.operation {
            Operation::Repeat(repeat) => (&repeat.accumulators, &repeat.body),
            Operation::GridFold(fold) => (&fold.accumulators, &fold.body),
            Operation::RadiusFold(fold) => (&fold.accumulators, &fold.body),
            _ => continue,
        };
        for accumulator in accumulators {
            if let Some(name) = names.get(accumulator.out).cloned() {
                names[accumulator.running] = name;
            }
            types[accumulator.running] = WgslType::of_value(accumulator.identity);
        }
        name_running(body, names, types);
    }
}

/// Returns the WGSL `bool` of whether the cell `xy`, a `vec2<i32>` when `signed` and a `vec2<u32>` otherwise, lies on
/// the domain grid.
fn inside(xy: &str, signed: bool) -> String {
    if signed {
        format!("{xy}.x >= 0i && {xy}.y >= 0i && u32({xy}.x) < params.henad_width && u32({xy}.y) < params.henad_height")
    } else {
        format!("{xy}.x < params.henad_width && {xy}.y < params.henad_height")
    }
}

#[cfg(test)]
mod tests {
    use super::{Text, literal, value_name};
    use crate::host::value::HostValue;
    use crate::ids::NodeId;

    /// An indented text keeps its blank lines bare, and each range still covers its own line, indentation included.
    #[test]
    fn indented_text_moves_its_ranges_with_its_lines() {
        let node = NodeId::new(3).expect("an id");
        let mut text = Text::default();
        text.push("    {\n\n");
        text.node_line("step", node, "        let a = 1u;\n");
        text.push("    }\n");
        let text = text.indented();
        assert_eq!(text.text, "        {\n\n            let a = 1u;\n        }\n");
        assert_eq!(text.ranges.len(), 1);
        assert_eq!(&text.text[text.ranges[0].range.clone()], "            let a = 1u;\n");
    }

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
