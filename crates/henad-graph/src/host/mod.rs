//! Host evaluator: runs a checked graph's passes on the CPU, one element at a time in index order.
//!
//! A GPU target runs its init passes here when the model is built, and the reruns of an action's init passes when it
//! is pressed, then uploads the lanes they write, as the stock GPU ports seed on the host. Every other pass runs on the
//! device. The evaluator can still run any pass, which is how the known-answer cases hold the host forms.
//!
//! A pass reads the lanes as they were at its start, and its writes commit at its end. Values the same for every
//! element run once before the elements, reducers fold in element order, and values computed from a reducer run once
//! after them. A *stream* is the serial `xorshift64` state a `stream.*` draw advances: the init stream continues across
//! the init passes, and the action stream across presses.

mod compile;
mod run;
mod value;

use std::collections::BTreeMap;
use std::fmt;

use henad_compute::cpu::agent_engine::agent_init_rng;
use henad_compute::cpu::grid_engine::grid_init_rng;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamValue;

use crate::document::{Domain, GraphDocument, PassEnabled, PassStage, World};
use crate::draw::{PHASE_INIT, PHASE_TICK, action_phase};
use crate::file::GraphFile;
use crate::ids::NodeId;
use crate::value::PortRef;

pub use value::{HostValue, LaneColumn};

use compile::Program;

/// Elements of a world: the cells of a grid or the agents of a population.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostWorld {
    /// A grid of cells.
    Grid {
        /// Columns.
        width: u32,
        /// Rows.
        height: u32,
        /// What a stencil sees past the edge.
        boundary: Boundary,
    },
    /// A population of agents.
    Agents {
        /// Number of agents.
        count: u32,
    },
}

impl HostWorld {
    /// Returns the world of `document` at the defaults of its world table.
    pub fn of(document: &GraphDocument) -> Self {
        match document.world {
            World::Grid(grid) => Self::Grid {
                width: grid.width,
                height: grid.height,
                boundary: grid.boundary,
            },
            World::Agents(agents) => Self::Agents { count: agents.count },
        }
    }

    /// Returns the number of elements of `domain`, or `None` for a domain the world does not hold.
    pub fn elements(&self, domain: Domain) -> Option<usize> {
        match (self, domain) {
            (Self::Grid { width, height, .. }, Domain::Cells) => (*width as usize).checked_mul(*height as usize),
            (Self::Agents { count }, Domain::Agents) => Some(*count as usize),
            _ => None,
        }
    }
}

/// Bytes the lanes of one world may take on the host, 4 GiB.
///
/// The lanes upload to the device, where one storage buffer binding addresses at most 4 GiB. A pass that writes lanes
/// holds a copy of each until its end, so a run can take twice this.
pub const MAX_HOST_LANE_BYTES: u64 = 1 << 32;

/// Every lane of a world, in the document's `[[lane]]` order.
#[derive(Debug, Clone)]
pub struct HostLanes {
    world: HostWorld,
    columns: Vec<LaneColumn>,
}

impl HostLanes {
    /// Returns the lanes of `document` over `world`, each element at the lane's initial value.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Unsupported`] for a lane whose domain the world does not hold, or whose type has no host
    /// value, for lanes past [`MAX_HOST_LANE_BYTES`] together, which is checked before anything is allocated, and for
    /// a lane the allocator cannot provide.
    pub fn new(document: &GraphDocument, world: HostWorld) -> Result<Self, HostError> {
        let refused = |reason: String| HostError::Unsupported {
            pass: String::new(),
            node: None,
            reason,
        };
        let mut bytes: u64 = 0;
        for lane in &document.lanes {
            let lane_bytes = world
                .elements(lane.domain)
                .zip(value::words_per_value(&lane.value_type))
                .and_then(|(elements, stride)| u64::try_from(elements).ok()?.checked_mul(stride as u64 * 4));
            bytes = bytes.saturating_add(lane_bytes.unwrap_or(0));
        }
        if bytes > MAX_HOST_LANE_BYTES {
            return Err(refused(format!(
                "the lanes take {bytes} bytes over this world, and the host holds at most {MAX_HOST_LANE_BYTES}"
            )));
        }
        let mut columns = Vec::with_capacity(document.lanes.len());
        for lane in &document.lanes {
            let elements = world
                .elements(lane.domain)
                .ok_or_else(|| refused(format!("the lane '{}' lives on a domain the world lacks", lane.id)))?;
            let initial = lane
                .initial
                .as_ref()
                .and_then(|literal| HostValue::from_literal(literal, document))
                .or_else(|| HostValue::zero_of(&lane.value_type))
                .ok_or_else(|| refused(format!("the lane '{}' has a type with no host value", lane.id)))?;
            let column = LaneColumn::filled(&lane.id, &lane.value_type, elements, initial)
                .ok_or_else(|| refused(format!("the lane '{}' does not fit the host", lane.id)))?;
            columns.push(column);
        }
        Ok(Self { world, columns })
    }

    /// World the lanes cover.
    pub fn world(&self) -> &HostWorld {
        &self.world
    }

    /// Every column, in the document's `[[lane]]` order.
    pub fn columns(&self) -> &[LaneColumn] {
        &self.columns
    }

    /// Returns the column of the lane `id`.
    pub fn column(&self, id: &str) -> Option<&LaneColumn> {
        self.columns.iter().find(|column| column.id() == id)
    }

    /// Returns the column of the lane `id` for writing.
    pub fn column_mut(&mut self, id: &str) -> Option<&mut LaneColumn> {
        self.columns.iter_mut().find(|column| column.id() == id)
    }

    /// Replaces the column at `position` with `column`.
    fn replace(&mut self, position: usize, column: LaneColumn) {
        if let Some(target) = self.columns.get_mut(position) {
            *target = column;
        }
    }
}

/// Words of a run of a pass that its nodes read: the seed, the phase, the tick and the press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunContext {
    /// Run seed.
    pub seed: Option<u64>,
    /// Phase word, see [`crate::draw`].
    pub phase: u32,
    /// Value of `ctx.tick`: the steps done.
    pub tick: u32,
    /// Word `t` a counter-based draw reads: the tick in a tick pass, the press count in an action, 0 in an init pass.
    pub tick_word: u32,
    /// Number of the press in an action, from 1.
    pub press: u32,
}

impl RunContext {
    /// Context of an init pass.
    pub fn init(seed: Option<u64>) -> Self {
        Self {
            seed,
            phase: PHASE_INIT,
            tick: 0,
            tick_word: 0,
            press: 0,
        }
    }

    /// Context of a tick, view or stats pass after `tick` steps.
    pub fn tick(seed: Option<u64>, tick: u32) -> Self {
        Self {
            seed,
            phase: PHASE_TICK,
            tick,
            tick_word: tick,
            press: 0,
        }
    }

    /// Context of press `press` of the action at position `action`, after `tick` steps.
    pub fn action(seed: Option<u64>, action: usize, press: u32, tick: u32) -> Self {
        Self {
            seed,
            phase: action_phase(action),
            tick,
            tick_word: press,
            press,
        }
    }

    /// Returns the context with the phase word `phase`.
    pub fn with_phase(mut self, phase: u32) -> Self {
        self.phase = phase;
        self
    }
}

/// What one run of a pass leaves besides its lane writes.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub struct PassOutcome {
    /// Values the same for every element, by node and port: parameters, tick values and the reducers' results.
    pub values: BTreeMap<NodeId, BTreeMap<String, HostValue>>,
    /// Palette index of each element, for a pass that sets colours.
    pub colors: Option<Vec<u32>>,
    /// Positions of the lanes the pass writes, in the document's `[[lane]]` order.
    pub written: Vec<usize>,
}

/// Prints the values and the written lanes, and the number of colours in place of the colours.
impl fmt::Debug for PassOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassOutcome")
            .field("values", &self.values)
            .field("colors", &self.colors.as_ref().map(Vec::len))
            .field("written", &self.written)
            .finish_non_exhaustive()
    }
}

impl PassOutcome {
    /// Returns the value at `port`, a reference such as `n2.S`, or `n3` for a node's only output.
    pub fn value(&self, port: &PortRef) -> Option<HostValue> {
        let outputs = self.values.get(&port.node)?;
        match &port.port {
            Some(name) => outputs.get(name).copied(),
            None if outputs.len() == 1 => outputs.values().next().copied(),
            None => None,
        }
    }
}

/// Reason the host evaluator cannot run a graph or a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostError {
    /// The checker found errors in the file.
    Checked(Vec<CodeDiagnostic>),
    /// A pass holds what the host cannot run, such as a kind with no host form.
    Unsupported {
        /// Id of the pass, empty for a declaration.
        pass: String,
        /// Node at fault.
        node: Option<NodeId>,
        /// What the host cannot run.
        reason: String,
    },
    /// The file declares no pass with this id.
    NoPass(String),
    /// The file declares no action with this id.
    NoAction(String),
    /// A run met a value it cannot use.
    Fault {
        /// Id of the pass.
        pass: String,
        /// Node at fault.
        node: Option<NodeId>,
        /// What went wrong.
        reason: String,
    },
}

impl HostError {
    /// Returns the error as diagnostics, with the pass and node ids where known.
    pub fn diagnostics(&self) -> Vec<CodeDiagnostic> {
        let located = |pass: &str, node: &Option<NodeId>| {
            let mut diagnostic = CodeDiagnostic::error(self.to_string());
            if !pass.is_empty() {
                diagnostic = diagnostic.with_pass(pass);
            }
            if let Some(node) = node {
                diagnostic = diagnostic.with_node(node.to_string());
            }
            diagnostic
        };
        match self {
            Self::Checked(diagnostics) => diagnostics.clone(),
            Self::Unsupported { pass, node, .. } | Self::Fault { pass, node, .. } => vec![located(pass, node)],
            Self::NoPass(_) | Self::NoAction(_) => vec![CodeDiagnostic::error(self.to_string())],
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let place = |f: &mut fmt::Formatter<'_>, pass: &str, node: &Option<NodeId>| {
            if let Some(node) = node {
                write!(f, "node {node} ")?;
            }
            if pass.is_empty() {
                Ok(())
            } else {
                write!(f, "in pass '{pass}' ")
            }
        };
        match self {
            Self::Checked(diagnostics) => write!(f, "the file has {} errors", diagnostics.len()),
            Self::Unsupported { pass, node, reason } => {
                write!(f, "the host cannot run ")?;
                place(f, pass, node)?;
                write!(f, "here: {reason}")
            }
            Self::NoPass(id) => write!(f, "the file declares no pass '{id}'"),
            Self::NoAction(id) => write!(f, "the file declares no action '{id}'"),
            Self::Fault { pass, node, reason } => {
                place(f, pass, node)?;
                write!(f, "faulted: {reason}")
            }
        }
    }
}

impl std::error::Error for HostError {}

/// Host evaluator of one graph: every pass compiled once, run as many times as needed.
#[derive(Clone)]
pub struct HostEvaluator {
    document: GraphDocument,
    programs: Vec<Result<Program, HostError>>,
}

/// Prints the graph's id and the number of passes.
impl fmt::Debug for HostEvaluator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostEvaluator")
            .field("id", &self.document.id)
            .field("passes", &self.programs.len())
            .finish_non_exhaustive()
    }
}

impl HostEvaluator {
    /// Returns the evaluator of `file`, after checking it.
    ///
    /// A pass the host cannot run, such as a tick pass with a kind that has only a WGSL form, compiles to an error
    /// that a run of it returns.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Checked`] with the errors when the checker finds any, and [`HostError::Unsupported`] when
    /// an init pass or a pass an action reruns holds what the host cannot run.
    pub fn new(file: &GraphFile) -> Result<Self, HostError> {
        let report = file.check();
        if report.has_errors() {
            return Err(HostError::Checked(report.errors().cloned().collect()));
        }
        Self::from_document(file.document().clone())
    }

    /// Returns the evaluator of `document`, which the caller has checked.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Unsupported`] when an init pass or a pass an action reruns holds what the host cannot run.
    pub(crate) fn from_document(document: GraphDocument) -> Result<Self, HostError> {
        let programs: Vec<Result<Program, HostError>> = document
            .passes
            .iter()
            .map(|pass| compile::compile(&document, pass))
            .collect();
        for (pass, program) in document.passes.iter().zip(&programs) {
            let rerun = document.actions.iter().any(|action| action.runs.contains(&pass.id));
            if let Err(error) = program
                && (pass.stage == PassStage::Init || rerun)
            {
                return Err(error.clone());
            }
        }
        Ok(Self { document, programs })
    }

    /// Document the evaluator runs.
    pub fn document(&self) -> &GraphDocument {
        &self.document
    }

    /// Returns the values of the document's parameters: `params` in schema order, and the default of each parameter
    /// that `params` lacks or gives a value of another kind.
    fn param_values(&self, params: &[ParamValue]) -> Vec<HostValue> {
        self.document
            .params
            .iter()
            .enumerate()
            .map(|(position, descriptor)| {
                let default = descriptor.kind().default_value();
                let value = params
                    .get(position)
                    .filter(|value| std::mem::discriminant(*value) == std::mem::discriminant(&default))
                    .unwrap_or(&default);
                HostValue::from_param(value)
            })
            .collect()
    }

    /// Returns whether the pass at `position` runs with `params`.
    fn is_enabled(&self, position: usize, params: &[HostValue]) -> bool {
        match &self.document.passes[position].enabled {
            PassEnabled::Literal(enabled) => *enabled,
            PassEnabled::Param(id) => self
                .document
                .params
                .iter()
                .position(|param| param.id() == id)
                .and_then(|param| params.get(param))
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
        }
    }

    /// Returns [`HostError::Fault`] for the pass `pass` when `lanes` do not hold the document's lanes: one column per
    /// `[[lane]]`, in order, of the lane's id and type, with one value per element of the lane's domain.
    fn check_lanes(&self, pass: &str, lanes: &HostLanes) -> Result<(), HostError> {
        let fault = |reason: String| HostError::Fault {
            pass: pass.to_owned(),
            node: None,
            reason,
        };
        let declared = &self.document.lanes;
        if lanes.columns.len() != declared.len() {
            return Err(fault(format!(
                "the lanes hold {} columns, and the file declares {} lanes",
                lanes.columns.len(),
                declared.len()
            )));
        }
        for (column, lane) in lanes.columns.iter().zip(declared) {
            if column.id() != lane.id || *column.value_type() != lane.value_type {
                return Err(fault(format!(
                    "the lanes hold the column '{}' of {} where the file declares the lane '{}' of {}",
                    column.id(),
                    column.value_type(),
                    lane.id,
                    lane.value_type
                )));
            }
            if lanes.world.elements(lane.domain) != Some(column.len()) {
                return Err(fault(format!(
                    "the column '{}' holds {} values, and its world holds another number of {}",
                    column.id(),
                    column.len(),
                    lane.domain.as_str()
                )));
            }
        }
        Ok(())
    }

    /// Returns the position and program of the pass `id`.
    fn program(&self, id: &str) -> Result<(usize, &Program), HostError> {
        let position = self
            .document
            .passes
            .iter()
            .position(|pass| pass.id == id)
            .ok_or_else(|| HostError::NoPass(id.to_owned()))?;
        match &self.programs[position] {
            Ok(program) => Ok((position, program)),
            Err(error) => Err(error.clone()),
        }
    }

    /// Returns the lanes of `world` after the init passes, run in stack order on the init stream, each pass that its
    /// `enabled` turns off skipped.
    ///
    /// `params` holds the values of the document's parameters in schema order, without the engine's own. The init
    /// stream starts as the stock engine of the topology starts it: `grid_init_rng(seed)` on a grid and
    /// `agent_init_rng(seed)` on agents.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`HostLanes::new`], and [`HostError::Fault`] for a run that faults.
    pub fn init(&self, world: HostWorld, params: &[ParamValue], seed: Option<u64>) -> Result<HostLanes, HostError> {
        let mut lanes = HostLanes::new(&self.document, world)?;
        let values = self.param_values(params);
        let mut stream = match world {
            HostWorld::Agents { .. } => agent_init_rng(seed),
            HostWorld::Grid { .. } => grid_init_rng(seed),
        };
        let context = RunContext::init(seed);
        for (position, pass) in self.document.passes.iter().enumerate() {
            if pass.stage != PassStage::Init || !self.is_enabled(position, &values) {
                continue;
            }
            let program = self.programs[position].as_ref().map_err(Clone::clone)?;
            run::run(program, &mut lanes, &values, &context, &mut stream)?;
        }
        Ok(lanes)
    }

    /// Reruns the init passes that the action `action` names in `runs`, in list order, for press `press`, and returns
    /// the positions of the lanes they write, which a GPU target uploads.
    ///
    /// A rerun ignores the pass's `enabled`, draws on `stream`, the action stream that starts at
    /// [`action_seed(seed)`](henad_core::action::action_seed) and continues across presses, and keys its counter-based
    /// draws on the action's phase and the press count. A rerun reads no lane. Tick passes of `runs` run on the device,
    /// and this skips them.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::NoAction`] for an unknown id, and [`HostError::Fault`] for `lanes` that do not hold the
    /// document's lanes or a run that faults.
    pub fn rerun(
        &self,
        action: &str,
        press: u32,
        lanes: &mut HostLanes,
        params: &[ParamValue],
        seed: Option<u64>,
        stream: &mut u64,
    ) -> Result<Vec<usize>, HostError> {
        let action_position = self
            .document
            .actions
            .iter()
            .position(|known| known.id == action)
            .ok_or_else(|| HostError::NoAction(action.to_owned()))?;
        let values = self.param_values(params);
        let context = RunContext::action(seed, action_position, press, 0);
        if let Some(first) = self.document.actions[action_position].runs.first() {
            self.check_lanes(first, lanes)?;
        }
        let mut written = Vec::new();
        for pass in &self.document.actions[action_position].runs {
            let (position, program) = self.program(pass)?;
            if self.document.passes[position].stage != PassStage::Init {
                continue;
            }
            written.extend(run::run(program, lanes, &values, &context, stream)?.written);
        }
        written.sort_unstable();
        written.dedup();
        Ok(written)
    }

    /// Runs the pass `pass` once over `lanes` in `context`, commits its writes and returns what else it leaves.
    ///
    /// `stream` is the stream its `stream.*` draws advance. The pass runs whatever its stage and its `enabled`, which
    /// is how a known-answer case runs a tick, view or stats pass on the host.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::NoPass`] for an unknown id, [`HostError::Unsupported`] for a pass with a kind the host
    /// cannot run, and [`HostError::Fault`] for `lanes` that do not hold the document's lanes, such as lanes of
    /// another file, or for a run that faults.
    pub fn run_pass(
        &self,
        pass: &str,
        lanes: &mut HostLanes,
        params: &[ParamValue],
        context: &RunContext,
        stream: &mut u64,
    ) -> Result<PassOutcome, HostError> {
        let (_, program) = self.program(pass)?;
        self.check_lanes(pass, lanes)?;
        let values = self.param_values(params);
        run::run(program, lanes, &values, context, stream)
    }

    /// Returns every value the element block of the pass `pass` computes at `element`, by node and port, with
    /// `iteration` as the iteration word of a region around the pass's top. Writes go nowhere.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Self::run_pass`].
    #[cfg(test)]
    pub(crate) fn probe(
        &self,
        pass: &str,
        lanes: &HostLanes,
        context: &RunContext,
        element: u32,
        iteration: Option<u32>,
        stream: &mut u64,
    ) -> Result<BTreeMap<(NodeId, String), HostValue>, HostError> {
        let (position, program) = self.program(pass)?;
        let values = self.param_values(&[]);
        let frame = run::probe(program, lanes, &values, context, element, iteration, stream)?;
        let pass = &self.document.passes[position];
        let mut found = BTreeMap::new();
        for node in &pass.body.nodes {
            let Some(def) = crate::catalogue::lookup(&node.op, self.document.library) else {
                continue;
            };
            for output in def.resolved_outputs(node, &self.document) {
                if let Some(slot) = program.output_slot(node.id, &output.name) {
                    found.insert((node.id, output.name), frame[slot]);
                }
            }
        }
        Ok(found)
    }
}
