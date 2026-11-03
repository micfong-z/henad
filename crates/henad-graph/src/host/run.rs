//! Execution of a compiled pass over the elements of its domain, one element at a time in index order.

use henad_core::authoring::primitives::rng::{mix_seed, pcg_hash, random_float, reservoir_accept, xorshift64};
use henad_core::authoring::primitives::space::{Boundary, axis_delta, heading_octant, wrap_coord};
use henad_core::spatial_hash::SpatialHash;

use super::compile::{
    CompareOp, DrawForm, Instruction, LogicOp, Operation, Program, RadiusFold, Repeat, RepeatAccumulator, StencilFold,
};
use super::value::{BinaryOp, HostValue, binary, convert, order, shift, unary};
use super::{HostError, HostLanes, HostWorld, LaneColumn, PassOutcome, RunContext};
use crate::draw::{draw_bits, fold32, iteration_word, key_word, press_word};
use crate::ids::NodeId;
use crate::limits::{MAX_FOLD_CANDIDATES, MAX_WALK_CELLS};
use crate::node::Combine;

/// State of one run of a pass.
struct Machine<'a> {
    program: &'a Program,
    /// Lanes at the start of the pass, which every read sees.
    lanes: &'a HostLanes,
    /// New values of the lanes the pass writes, by lane position, committed at the end of the pass.
    staged: Vec<Option<LaneColumn>>,
    context: &'a RunContext,
    /// Key word of each draw, by draw number.
    key_words: Vec<u32>,
    press_seed: u32,
    tallies: Vec<u64>,
    colors: Vec<u32>,
    stream: &'a mut u64,
    /// Columns of the grid, for a stencil.
    width: u32,
    /// Rows of the grid, for a stencil.
    height: u32,
    bounded: bool,
    element: u32,
    /// Iteration word of the innermost region running, `None` outside every region.
    iteration: Option<u32>,
    /// World the pass runs over.
    world: HostWorld,
    /// Deposits staged by the pass, by position among the deposit columns.
    deposits: Vec<Option<LaneColumn>>,
    /// Counters after the pass's additions.
    counters: Vec<u32>,
    /// State of each float sum.
    sums: Vec<SumState>,
    /// Number of elements of the pass.
    elements: usize,
    /// Index of the positions at the start of the pass, built at the first radius fold, with its cell size.
    index: Option<(u32, SpatialHash)>,
}

/// State of one float sum while the elements fold.
#[derive(Debug, Clone, Default)]
struct SumState {
    /// Sum of the current chunk, per component.
    chunk: [f64; 2],
    /// Sum of the current chunk in `f32`, per component, for a sum whose chunks sum in `f32`.
    narrow: [f32; 2],
    /// Number of values in the current chunk.
    in_chunk: usize,
    /// Sum of the finished chunks, per component.
    total: [f64; 2],
    /// Whether a `vec2f` value was summed.
    vector: bool,
}

/// Number of elements a chunk of a float sum holds, as the stock statistics chunk them.
const SUM_CHUNK: usize = 8192;

/// Runs `program` over every element of its domain in `lanes`, and commits its writes.
///
/// # Errors
///
/// Returns [`HostError::Fault`] for a value the program cannot use, such as a lane of another size than the world's.
#[expect(
    clippy::too_many_lines,
    reason = "the staging, the elements and the commit read in order"
)]
pub(super) fn run(
    program: &Program,
    lanes: &mut HostLanes,
    params: &[HostValue],
    context: &RunContext,
    stream: &mut u64,
) -> Result<PassOutcome, HostError> {
    let elements = match program.domain {
        Some(domain) => lanes.world().elements(domain).ok_or_else(|| HostError::Fault {
            pass: program.pass.clone(),
            node: None,
            reason: format!("the world holds no {}", domain.as_str()),
        })?,
        None => 0,
    };
    let (width, height, boundary) = lanes.world().domain_grid();
    let bounded = boundary == Boundary::Bounded;
    let mut frame = vec![HostValue::Bool(false); program.slots];
    for (slot, value) in &program.constants {
        frame[*slot] = *value;
    }
    for (slot, param) in &program.params {
        frame[*slot] = *params.get(*param).ok_or_else(|| HostError::Fault {
            pass: program.pass.clone(),
            node: None,
            reason: format!("no value for parameter {param}"),
        })?;
    }
    let mut staged: Vec<Option<LaneColumn>> = vec![None; lanes.columns.len()];
    for &lane in &program.written {
        let fault = |reason: &str| HostError::Fault {
            pass: program.pass.clone(),
            node: None,
            reason: format!("{reason} {lane}"),
        };
        let column = lanes
            .columns
            .get(lane)
            .ok_or_else(|| fault("the lanes hold no lane at position"))?;
        // `staged` has one entry per column, so the position found above is in range.
        staged[lane] = Some(
            column
                .try_clone()
                .ok_or_else(|| fault("the host has no memory for a copy of the lane at position"))?,
        );
    }
    let mut deposits: Vec<Option<LaneColumn>> = vec![None; lanes.deposits.len()];
    for &layer in &program.deposited {
        if let Some(column) = lanes.deposits.get(layer) {
            deposits[layer] = Some(column.try_clone().ok_or_else(|| HostError::Fault {
                pass: program.pass.clone(),
                node: None,
                reason: "the host has no memory for the deposits".to_owned(),
            })?);
        }
    }
    let world = *lanes.world();
    let mut machine = Machine {
        program,
        lanes,
        staged,
        context,
        key_words: program
            .draw_keys
            .iter()
            .map(|&key| key_word(key, context.phase, context.seed))
            .collect(),
        press_seed: press_word(context.seed, context.press),
        tallies: vec![0; program.tallies],
        colors: Vec::new(),
        stream,
        width,
        height,
        bounded,
        element: 0,
        iteration: None,
        world,
        deposits,
        counters: lanes.counters.iter().map(|(_, value)| *value).collect(),
        sums: vec![SumState::default(); program.sums.len()],
        elements,
        index: None,
    };
    machine.execute(&program.prelude, &mut frame)?;
    for element in 0..elements {
        machine.element = u32::try_from(element).unwrap_or(u32::MAX);
        machine.execute(&program.element, &mut frame)?;
    }
    for (tally, slot) in machine.tallies.iter().zip(&program.tally_slots) {
        if let Some(target) = frame.get_mut(*slot) {
            *target = HostValue::U64(*tally);
        }
    }
    machine.finish_sums(&mut frame);
    machine.execute(&program.post, &mut frame)?;
    let colors = machine.colors;
    let staged = machine.staged;
    let deposits = machine.deposits;
    let counters = machine.counters;
    for (position, column) in staged.into_iter().enumerate() {
        if let Some(column) = column {
            lanes.replace(position, column);
        }
    }
    for (position, column) in deposits.into_iter().enumerate() {
        if let (Some(column), Some(target)) = (column, lanes.deposits.get_mut(position)) {
            *target = column;
        }
    }
    for ((_, value), counted) in lanes.counters.iter_mut().zip(counters) {
        *value = counted;
    }
    let mut outcome = PassOutcome {
        values: std::collections::BTreeMap::new(),
        colors: program.colors.then_some(colors),
        written: program.written.clone(),
    };
    for (node, port, slot) in &program.uniform_outputs {
        outcome
            .values
            .entry(*node)
            .or_default()
            .insert(port.clone(), frame[*slot]);
    }
    Ok(outcome)
}

/// Returns the frame of `program` after its prelude alone, run in `context` over `world` with no lane.
///
/// The prelude reads no lane, so the frame holds every value the same for every element.
///
/// # Errors
///
/// Returns [`HostError::Fault`] for a value the prelude cannot use.
pub(crate) fn prelude(
    program: &Program,
    params: &[HostValue],
    context: &RunContext,
    world: HostWorld,
) -> Result<Vec<HostValue>, HostError> {
    finish(program, params, context, world, None)
}

/// What the GPU reads back of a stats pass, in place of the elements.
#[derive(Debug, Clone, Default)]
pub(crate) struct Readback {
    /// Count of each tally, by tally number.
    pub tallies: Vec<u64>,
    /// Total of each float sum by sum number, and whether it sums `vec2f` values. A sum the readback leaves out is 0.
    pub sums: Vec<([f64; 2], bool)>,
    /// Value of each counter, in `[[counter]]` order.
    pub counters: Vec<u32>,
}

/// Returns the frame of `program` after its prelude and, when `readback` is `Some`, its post block, with each tally's
/// count, each sum's total and each counter in place of the elements.
///
/// The GPU counts and sums the reducers of a stats pass, and the host runs the values computed from them.
///
/// # Errors
///
/// Returns [`HostError::Fault`] for a value either block cannot use.
pub(crate) fn finish(
    program: &Program,
    params: &[HostValue],
    context: &RunContext,
    world: HostWorld,
    readback: Option<&Readback>,
) -> Result<Vec<HostValue>, HostError> {
    let lanes = HostLanes {
        world,
        columns: Vec::new(),
        lanes: 0,
        deposits: Vec::new(),
        counters: readback.map_or_else(Vec::new, |readback| {
            readback.counters.iter().map(|value| (String::new(), *value)).collect()
        }),
    };
    let mut frame = vec![HostValue::Bool(false); program.slots];
    for (slot, value) in &program.constants {
        frame[*slot] = *value;
    }
    for (slot, param) in &program.params {
        frame[*slot] = *params.get(*param).ok_or_else(|| HostError::Fault {
            pass: program.pass.clone(),
            node: None,
            reason: format!("no value for parameter {param}"),
        })?;
    }
    let mut stream = 0;
    let (width, height, boundary) = world.domain_grid();
    let mut machine = Machine {
        program,
        lanes: &lanes,
        staged: Vec::new(),
        context,
        key_words: program
            .draw_keys
            .iter()
            .map(|&key| key_word(key, context.phase, context.seed))
            .collect(),
        press_seed: press_word(context.seed, context.press),
        tallies: vec![0; program.tallies],
        colors: Vec::new(),
        stream: &mut stream,
        width,
        height,
        bounded: boundary == Boundary::Bounded,
        element: 0,
        iteration: None,
        world,
        deposits: Vec::new(),
        counters: Vec::new(),
        sums: vec![SumState::default(); program.sums.len()],
        elements: 0,
        index: None,
    };
    machine.execute(&program.prelude, &mut frame)?;
    if let Some(readback) = readback {
        for (slot, count) in program.tally_slots.iter().zip(&readback.tallies) {
            if let Some(target) = frame.get_mut(*slot) {
                *target = HostValue::U64(*count);
            }
        }
        let elements = program
            .domain
            .and_then(|domain| world.elements(domain))
            .unwrap_or(0)
            .max(1) as f64;
        for (number, spec) in program.sums.iter().enumerate() {
            let ([x, y], vector) = readback.sums.get(number).copied().unwrap_or(([0.0, 0.0], false));
            let scale = if spec.mean { 1.0 / elements } else { 1.0 };
            let value = if spec.vector.unwrap_or(vector) {
                HostValue::Vec2D([x * scale, y * scale])
            } else {
                HostValue::F64(x * scale)
            };
            if let Some(target) = frame.get_mut(spec.slot) {
                *target = value;
            }
        }
        machine.execute(&program.post, &mut frame)?;
    }
    Ok(frame)
}

/// Runs only the element block of `program` at `element`, with `iteration` as the iteration word of the region around
/// it, and returns the frame. Every lane must already hold the element.
///
/// # Errors
///
/// Returns the faults of [`run`].
#[cfg(test)]
pub(super) fn probe(
    program: &Program,
    lanes: &HostLanes,
    params: &[HostValue],
    context: &RunContext,
    element: u32,
    iteration: Option<u32>,
    stream: &mut u64,
) -> Result<Vec<HostValue>, HostError> {
    let mut frame = vec![HostValue::Bool(false); program.slots];
    for (slot, value) in &program.constants {
        frame[*slot] = *value;
    }
    for (slot, param) in &program.params {
        frame[*slot] = params.get(*param).copied().unwrap_or(HostValue::Bool(false));
    }
    let (width, height, boundary) = lanes.world().domain_grid();
    let bounded = boundary == Boundary::Bounded;
    let mut machine = Machine {
        program,
        lanes,
        staged: vec![None; lanes.columns.len()],
        context,
        key_words: program
            .draw_keys
            .iter()
            .map(|&key| key_word(key, context.phase, context.seed))
            .collect(),
        press_seed: press_word(context.seed, context.press),
        tallies: vec![0; program.tallies],
        colors: Vec::new(),
        stream,
        width,
        height,
        bounded,
        element,
        iteration,
        world: *lanes.world(),
        deposits: vec![None; lanes.deposits.len()],
        counters: lanes.counters.iter().map(|(_, value)| *value).collect(),
        sums: vec![SumState::default(); program.sums.len()],
        elements: 1,
        index: None,
    };
    machine.execute(&program.prelude, &mut frame)?;
    machine.execute(&program.element, &mut frame)?;
    Ok(frame)
}

impl Machine<'_> {
    /// Returns a fault of the node `node`.
    fn fault(&self, node: NodeId, reason: impl Into<String>) -> HostError {
        HostError::Fault {
            pass: self.program.pass.clone(),
            node: Some(node),
            reason: reason.into(),
        }
    }

    /// Runs `block` on `frame`.
    fn execute(&mut self, block: &[Instruction], frame: &mut [HostValue]) -> Result<(), HostError> {
        for instruction in block {
            self.step(instruction, frame)?;
        }
        Ok(())
    }

    /// Runs one instruction on `frame`.
    #[expect(clippy::too_many_lines, reason = "one arm per operation reads best as one table")]
    fn step(&mut self, instruction: &Instruction, frame: &mut [HostValue]) -> Result<(), HostError> {
        let node = instruction.node;
        let mismatch = |machine: &Self| machine.fault(node, "met a value of a type it does not take");
        let boolean = |machine: &Self, slot: usize| frame[slot].as_bool().ok_or_else(|| mismatch(machine));
        let word = |machine: &Self, slot: usize| frame[slot].as_u32().ok_or_else(|| mismatch(machine));
        let float = |machine: &Self, slot: usize| frame[slot].as_f32().ok_or_else(|| mismatch(machine));
        let vector = |machine: &Self, slot: usize| frame[slot].as_vec2f().ok_or_else(|| mismatch(machine));
        let value = match &instruction.operation {
            Operation::World => HostValue::Vec2F(self.world.extent()),
            Operation::Grid => HostValue::Vec2U([self.width, self.height]),
            Operation::CellXy => HostValue::Vec2U(self.cell_xy(self.element)),
            Operation::LaneGetAt { lane, at } => {
                let (HostValue::Cell(index) | HostValue::Agent(index)) = frame[*at] else {
                    return Err(mismatch(self));
                };
                let column = self
                    .lanes
                    .columns
                    .get(*lane)
                    .ok_or_else(|| self.fault(node, "reads a lane the world does not hold"))?;
                match column.get(index as usize) {
                    Some(found) => found,
                    None => HostValue::zero_of(column.value_type()).ok_or_else(|| mismatch(self))?,
                }
            }
            Operation::FieldGet {
                first,
                layers,
                layer,
                at,
                zero,
            } => {
                let column = self.layer_column(node, *first, *layers, frame[*layer])?;
                let cell = match at {
                    Some(slot) => match frame[*slot] {
                        HostValue::Cell(index) => index,
                        _ => return Err(mismatch(self)),
                    },
                    None => self.element,
                };
                self.lanes.columns[column].get(cell as usize).unwrap_or(*zero)
            }
            Operation::FieldSet {
                first,
                layers,
                layer,
                value,
            } => {
                let column = self.layer_column(node, *first, *layers, frame[*layer])?;
                let written = frame[*value];
                let fits = self
                    .staged
                    .get_mut(column)
                    .and_then(Option::as_mut)
                    .is_some_and(|staged| staged.set(self.element as usize, written));
                if !fits {
                    return Err(self.fault(node, "writes a value its field cannot hold"));
                }
                return Ok(());
            }
            Operation::Deposit {
                first,
                layers,
                layer,
                at,
                value,
            } => {
                let HostValue::Layer(number) = frame[*layer] else {
                    return Err(mismatch(self));
                };
                if number >= *layers {
                    return Err(self.fault(node, "deposits into a layer its field lacks"));
                }
                let HostValue::Cell(cell) = frame[*at] else {
                    return Err(mismatch(self));
                };
                let deposited = float(self, *value)?.max(0.0);
                if let Some(column) = self.deposits.get_mut(*first + number as usize).and_then(Option::as_mut)
                    && let Some(HostValue::F32(held)) = column.get(cell as usize)
                {
                    column.set(cell as usize, HostValue::F32(held.max(deposited)));
                }
                return Ok(());
            }
            Operation::CounterAdd { counter, amount } => {
                let amount = word(self, *amount)?;
                let total = self.counters.get_mut(*counter).ok_or_else(|| HostError::Fault {
                    pass: self.program.pass.clone(),
                    node: Some(node),
                    reason: "adds to a counter the world does not hold".to_owned(),
                })?;
                *total = total.wrapping_add(amount);
                return Ok(());
            }
            Operation::CounterTotal { counter } => HostValue::U64(u64::from(
                self.lanes
                    .counters
                    .get(*counter)
                    .map(|(_, value)| *value)
                    .ok_or_else(|| self.fault(node, "reads a counter the world does not hold"))?,
            )),
            Operation::Clamp { x, lo, hi } => {
                let raised = binary(BinaryOp::Max, frame[*x], frame[*lo]).ok_or_else(|| mismatch(self))?;
                binary(BinaryOp::Min, raised, frame[*hi]).ok_or_else(|| mismatch(self))?
            }
            Operation::Midpoint { a, b } => HostValue::F32(float(self, *a)?.midpoint(float(self, *b)?)),
            Operation::VecMake { x, y } => match (frame[*x], frame[*y]) {
                (HostValue::U32(x), HostValue::U32(y)) => HostValue::Vec2U([x, y]),
                (HostValue::I32(x), HostValue::I32(y)) => HostValue::Vec2I([x, y]),
                (HostValue::F32(x), HostValue::F32(y)) => HostValue::Vec2F([x, y]),
                (HostValue::F64(x), HostValue::F64(y)) => HostValue::Vec2D([x, y]),
                _ => return Err(mismatch(self)),
            },
            Operation::VecSplit { v, y_out } => {
                let (x, y) = match frame[*v] {
                    HostValue::Vec2U([x, y]) => (HostValue::U32(x), HostValue::U32(y)),
                    HostValue::Vec2I([x, y]) => (HostValue::I32(x), HostValue::I32(y)),
                    HostValue::Vec2F([x, y]) => (HostValue::F32(x), HostValue::F32(y)),
                    HostValue::Vec2D([x, y]) => (HostValue::F64(x), HostValue::F64(y)),
                    _ => return Err(mismatch(self)),
                };
                frame[*y_out] = y;
                x
            }
            Operation::VecScale { v, s } => {
                let [x, y] = vector(self, *v)?;
                let s = float(self, *s)?;
                HostValue::Vec2F([x * s, y * s])
            }
            Operation::VecDot { a, b } => {
                let ([ax, ay], [bx, by]) = (vector(self, *a)?, vector(self, *b)?);
                HostValue::F32(ax * bx + ay * by)
            }
            Operation::VecLength { v } => {
                let [x, y] = vector(self, *v)?;
                #[expect(
                    clippy::imprecise_flops,
                    reason = "the kind is defined as the square root written out"
                )]
                let length = (x * x + y * y).sqrt();
                HostValue::F32(length)
            }
            Operation::FromAngle { angle, length } => {
                let (angle, length) = (float(self, *angle)?, float(self, *length)?);
                HostValue::Vec2F([angle.cos() * length, angle.sin() * length])
            }
            Operation::Wrap { v, world } => match (frame[*v], frame[*world]) {
                (HostValue::F32(v), HostValue::F32(world)) => HostValue::F32(wrap_coord(v, world)),
                (HostValue::Vec2F([x, y]), HostValue::Vec2F([w, h])) => {
                    HostValue::Vec2F([wrap_coord(x, w), wrap_coord(y, h)])
                }
                _ => return Err(mismatch(self)),
            },
            Operation::Delta { a, b, world, squared } => {
                let ([ax, ay], [bx, by], [w, h]) = (vector(self, *a)?, vector(self, *b)?, vector(self, *world)?);
                let dx = axis_delta(ax, bx, w, Boundary::Torus);
                let dy = axis_delta(ay, by, h, Boundary::Torus);
                if *squared {
                    HostValue::F32(dx * dx + dy * dy)
                } else {
                    HostValue::Vec2F([dx, dy])
                }
            }
            Operation::CellIndex { xy, valid_out } => {
                let (x, y) = match frame[*xy] {
                    HostValue::Vec2I([x, y]) => (i64::from(x), i64::from(y)),
                    HostValue::Vec2U([x, y]) => (i64::from(x), i64::from(y)),
                    _ => return Err(mismatch(self)),
                };
                let cell = self.cell_at(x, y);
                frame[*valid_out] = HostValue::Bool(cell.is_some());
                HostValue::Cell(cell.unwrap_or(u32::MAX))
            }
            Operation::CellOf { p, xy_out, valid_out } => {
                let [px, py] = vector(self, *p)?;
                let (x, y) = (clamp_to_i32(px), clamp_to_i32(py));
                let cell = self.cell_at(i64::from(x), i64::from(y));
                frame[*xy_out] = HostValue::Vec2I([x, y]);
                frame[*valid_out] = HostValue::Bool(cell.is_some());
                HostValue::Cell(cell.unwrap_or(u32::MAX))
            }
            Operation::CellXyOf { cell } => {
                let HostValue::Cell(cell) = frame[*cell] else {
                    return Err(mismatch(self));
                };
                HostValue::Vec2U(self.cell_xy(cell))
            }
            Operation::Offset {
                xy,
                by,
                bounded,
                cell_out,
                valid_out,
            } => {
                let [x, y] = frame[*xy].as_vec2i().ok_or_else(|| mismatch(self))?;
                let [dx, dy] = frame[*by].as_vec2i().ok_or_else(|| mismatch(self))?;
                let (nx, ny, cell) = self.step_cell(i64::from(x), i64::from(y), i64::from(dx), i64::from(dy), *bounded);
                frame[*cell_out] = HostValue::Cell(cell.unwrap_or(u32::MAX));
                frame[*valid_out] = HostValue::Bool(cell.is_some());
                HostValue::Vec2I([nx, ny])
            }
            Operation::HeadingOctant { v } => {
                let [x, y] = vector(self, *v)?;
                HostValue::U32(u32::from(heading_octant(x, y)))
            }
            Operation::StepCode { d } => {
                let [x, y] = frame[*d].as_vec2i().ok_or_else(|| mismatch(self))?;
                HostValue::U32((x.wrapping_add(1).wrapping_mul(3).wrapping_add(y.wrapping_add(1))) as u32)
            }
            Operation::StepOffset { code } => {
                let code = word(self, *code)?;
                HostValue::Vec2I([((code / 3) as i32).wrapping_sub(1), ((code % 3) as i32).wrapping_sub(1)])
            }
            Operation::Choice3 { bits } => HostValue::I32((word(self, *bits)? % 3) as i32 - 1),
            Operation::Reservoir { bits, count } => {
                HostValue::Bool(reservoir_accept(word(self, *bits)?, word(self, *count)?))
            }
            Operation::StreamFloat { max, when } => {
                if boolean(self, *when)? {
                    *self.stream = xorshift64(*self.stream);
                    HostValue::F32(random_float((*self.stream >> 32) as u32, float(self, *max)?))
                } else {
                    HostValue::F32(0.0)
                }
            }
            Operation::GridFold(fold) => {
                self.grid_fold(node, fold, frame)?;
                return Ok(());
            }
            Operation::RadiusFold(fold) => {
                self.radius_fold(node, fold, frame)?;
                return Ok(());
            }
            Operation::Sum { value, sum } => {
                let value = frame[*value];
                let wide = self.program.sums.get(*sum).is_some_and(|spec| spec.wide_chunks);
                let state = self.sums.get_mut(*sum).ok_or_else(|| HostError::Fault {
                    pass: self.program.pass.clone(),
                    node: Some(node),
                    reason: "sums into a sum the pass lacks".to_owned(),
                })?;
                let parts = match value {
                    HostValue::F32(x) => [x, 0.0],
                    HostValue::Vec2F(pair) => {
                        state.vector = true;
                        pair
                    }
                    _ => return Err(mismatch(self)),
                };
                if wide {
                    state.chunk[0] += f64::from(parts[0]);
                    state.chunk[1] += f64::from(parts[1]);
                } else {
                    state.narrow[0] += parts[0];
                    state.narrow[1] += parts[1];
                }
                state.in_chunk += 1;
                if state.in_chunk == SUM_CHUNK {
                    close_chunk(state, wide);
                }
                return Ok(());
            }
            Operation::Index => HostValue::U32(self.element),
            Operation::Tick => HostValue::U32(self.context.tick),
            Operation::PressSeed => HostValue::U32(self.press_seed),
            Operation::LaneGet { lane } => self
                .lanes
                .columns
                .get(*lane)
                .and_then(|column| column.get(self.element as usize))
                .ok_or_else(|| self.fault(node, "reads past the end of its lane"))?,
            Operation::LaneSet { lane, value } => {
                let written = frame[*value];
                let fits = self
                    .staged
                    .get_mut(*lane)
                    .and_then(Option::as_mut)
                    .is_some_and(|column| column.set(self.element as usize, written));
                if !fits {
                    return Err(self.fault(node, "writes a value its lane cannot hold"));
                }
                return Ok(());
            }
            Operation::Binary { op, a, b } => binary(*op, frame[*a], frame[*b]).ok_or_else(|| mismatch(self))?,
            Operation::Shift { a, b, left } => {
                shift(frame[*a], word(self, *b)?, *left).ok_or_else(|| mismatch(self))?
            }
            Operation::Unary { op, a } => unary(*op, frame[*a]).ok_or_else(|| mismatch(self))?,
            Operation::Powi { base, exponent, max } => {
                let base = float(self, *base)?;
                let exponent = word(self, *exponent)?.min(*max);
                HostValue::F32(base.powi(i32::try_from(exponent).unwrap_or(i32::MAX)))
            }
            Operation::Pow { base, exponent } => HostValue::F32(float(self, *base)?.powf(float(self, *exponent)?)),
            Operation::Compare { op, a, b } => {
                let (a, b) = (frame[*a], frame[*b]);
                HostValue::Bool(match op {
                    CompareOp::Eq => a == b,
                    CompareOp::Ne => a != b,
                    CompareOp::Lt => order(a, b).is_some_and(std::cmp::Ordering::is_lt),
                    CompareOp::Le => order(a, b).is_some_and(std::cmp::Ordering::is_le),
                    CompareOp::Gt => order(a, b).is_some_and(std::cmp::Ordering::is_gt),
                    CompareOp::Ge => order(a, b).is_some_and(std::cmp::Ordering::is_ge),
                })
            }
            Operation::Logic { op, a, b } => {
                let a = boolean(self, *a)?;
                let b = match b {
                    Some(b) => boolean(self, *b)?,
                    None => false,
                };
                HostValue::Bool(match op {
                    LogicOp::And => a && b,
                    LogicOp::Or => a || b,
                    LogicOp::Xor => a != b,
                    LogicOp::Not => !a,
                })
            }
            Operation::Select {
                cond,
                if_true,
                if_false,
            } => {
                if boolean(self, *cond)? {
                    frame[*if_true]
                } else {
                    frame[*if_false]
                }
            }
            Operation::Pick { index, values } => {
                let index = word(self, *index)? as usize;
                *values
                    .get(index.min(values.len().saturating_sub(1)))
                    .ok_or_else(|| self.fault(node, "picks from an empty list"))?
            }
            Operation::Switch { select, cases, default } => {
                let variant = frame[*select].as_variant().ok_or_else(|| mismatch(self))? as usize;
                let chosen = cases.get(variant).copied().flatten().or(*default);
                frame[chosen.ok_or_else(|| self.fault(node, "has no case for the variant it selects"))?]
            }
            Operation::Transition { state, rules, fired } => {
                let mut current = frame[*state].as_variant().ok_or_else(|| mismatch(self))?;
                let mut phases: Vec<i64> = rules.iter().map(|rule| rule.phase).collect();
                phases.sort_unstable();
                phases.dedup();
                let mut fired_rules = vec![false; fired.len()];
                for phase in phases {
                    let start = current;
                    for rule in rules.iter().filter(|rule| rule.phase == phase) {
                        if rule.from != start || !boolean(self, rule.when)? {
                            continue;
                        }
                        current = match rule.to_input {
                            Some(slot) => frame[slot].as_variant().ok_or_else(|| mismatch(self))?,
                            None => rule.to.ok_or_else(|| self.fault(node, "has a rule with no target"))?,
                        };
                        if let Some(flag) = fired_rules.get_mut(rule.fired) {
                            *flag = true;
                        }
                        break;
                    }
                }
                for (slot, flag) in fired.iter().zip(fired_rules) {
                    if let Some(target) = frame.get_mut(*slot) {
                        *target = HostValue::Bool(flag);
                    }
                }
                HostValue::Category(current)
            }
            Operation::Convert { target, a } => convert(*target, frame[*a]).ok_or_else(|| mismatch(self))?,
            Operation::U32ToCat { a, last } => HostValue::Category(word(self, *a)?.min(*last)),
            Operation::CatToU32 { a } => HostValue::U32(frame[*a].as_variant().ok_or_else(|| mismatch(self))?),
            Operation::Draw { form, draw, input } => {
                let bits = draw_bits(
                    self.key_words[*draw],
                    self.element,
                    self.context.tick_word,
                    self.iteration,
                    0,
                );
                match form {
                    DrawForm::Bits => HostValue::U32(bits),
                    DrawForm::Choice3 => HostValue::I32((bits % 3) as i32 - 1),
                    DrawForm::Float => HostValue::F32(random_float(bits, float(self, *input)?)),
                    DrawForm::Below => HostValue::Bool(bits < word(self, *input)?),
                    DrawForm::Chance => HostValue::Bool(random_float(bits, 1.0) < float(self, *input)?),
                }
            }
            Operation::Threshold { p } => HostValue::U32((float(self, *p)? * u32::MAX as f32) as u32),
            Operation::Hash { x } => HostValue::U32(pcg_hash(word(self, *x)?)),
            Operation::ToFloat { bits, max } => HostValue::F32(random_float(word(self, *bits)?, float(self, *max)?)),
            Operation::Below { bits, threshold } => HostValue::Bool(word(self, *bits)? < word(self, *threshold)?),
            Operation::LaneSeed { salt } => {
                HostValue::U32(fold32(self.context.seed.map_or(*salt, |seed| mix_seed(seed ^ salt))))
            }
            Operation::StreamBits { when } => {
                if boolean(self, *when)? {
                    *self.stream = xorshift64(*self.stream);
                    HostValue::U32((*self.stream >> 32) as u32)
                } else {
                    HostValue::U32(0)
                }
            }
            Operation::Repeat(repeat) => {
                self.repeat(node, repeat, frame)?;
                return Ok(());
            }
            Operation::Put {
                running,
                combine,
                value,
                when,
            } => {
                if !boolean(self, *when)? {
                    return Ok(());
                }
                let current = frame[*running];
                let put = match value {
                    Some(slot) => frame[*slot],
                    None => HostValue::U32(1),
                };
                frame[*running] = match combine {
                    Combine::Sum | Combine::Count => binary(BinaryOp::Add, current, put),
                    Combine::Min => binary(BinaryOp::Min, current, put),
                    Combine::Max => binary(BinaryOp::Max, current, put),
                    Combine::Or => Some(HostValue::Bool(
                        current.as_bool().unwrap_or_default() || put.as_bool().unwrap_or_default(),
                    )),
                    Combine::And => Some(HostValue::Bool(
                        current.as_bool().unwrap_or_default() && put.as_bool().unwrap_or_default(),
                    )),
                    Combine::Carry => Some(put),
                }
                .ok_or_else(|| mismatch(self))?;
                return Ok(());
            }
            Operation::GridCount {
                lane,
                offsets,
                value,
                when,
            } => {
                if !boolean(self, *when)? {
                    HostValue::U32(0)
                } else {
                    let column = self
                        .lanes
                        .columns
                        .get(*lane)
                        .ok_or_else(|| self.fault(node, "counts a lane the world does not hold"))?;
                    HostValue::U32(self.count_neighbors(column, offsets, frame[*value]))
                }
            }
            Operation::LifeLike {
                cell,
                count,
                birth,
                survive,
                dead,
                alive,
            } => {
                let cell = frame[*cell].as_variant().ok_or_else(|| mismatch(self))?;
                let count = word(self, *count)?;
                let mask = if cell == *alive { *survive } else { *birth };
                let lives = count <= 9 && mask & (1 << count) != 0;
                HostValue::Category(if lives { *alive } else { *dead })
            }
            Operation::CountWhere { pred, tally } => {
                if boolean(self, *pred)? {
                    self.tallies[*tally] += 1;
                }
                return Ok(());
            }
            Operation::CountBy {
                value,
                first_tally,
                variants,
            } => {
                let variant = frame[*value].as_variant().ok_or_else(|| mismatch(self))?;
                if variant < *variants {
                    self.tallies[first_tally + variant as usize] += 1;
                }
                return Ok(());
            }
            Operation::Color { index, len } => {
                let (HostValue::U32(index) | HostValue::Category(index)) = frame[*index] else {
                    return Err(mismatch(self));
                };
                if *len == 0 {
                    return Err(self.fault(node, "colours from an empty palette"));
                }
                self.colors.push(index.min(len - 1));
                return Ok(());
            }
            Operation::Ramp { value, low, high, len } => {
                let (value, low, high) = (float(self, *value)?, float(self, *low)?, float(self, *high)?);
                let last = len.saturating_sub(1);
                let index = if high > low && !value.is_nan() {
                    let band = ((value - low) / (high - low) * *len as f32).floor().max(0.0);
                    (band as u32).min(last)
                } else {
                    0
                };
                HostValue::U32(index)
            }
        };
        if let Some(target) = frame.get_mut(instruction.out) {
            *target = value;
        }
        Ok(())
    }

    /// Returns the column position of the layer `layer` of a field whose first layer sits at `first`.
    fn layer_column(&self, node: NodeId, first: usize, layers: u32, layer: HostValue) -> Result<usize, HostError> {
        match layer {
            HostValue::Layer(number) if number < layers => Ok(first + number as usize),
            _ => Err(self.fault(node, "names a layer its field lacks")),
        }
    }

    /// Returns the column and row of the cell `cell` of the domain grid.
    fn cell_xy(&self, cell: u32) -> [u32; 2] {
        if self.width == 0 {
            return [0, 0];
        }
        [cell % self.width, cell / self.width]
    }

    /// Returns the cell at column `x` and row `y` of the domain grid, or `None` off it.
    fn cell_at(&self, x: i64, y: i64) -> Option<u32> {
        let (width, height) = (i64::from(self.width), i64::from(self.height));
        if x < 0 || y < 0 || x >= width || y >= height {
            return None;
        }
        u32::try_from(y * width + x).ok()
    }

    /// Returns the position `(x + dx, y + dy)`, wrapped on a torus, and its cell, `None` off the grid. `bounded` is
    /// `None` for the domain grid's own boundary.
    fn step_cell(&self, x: i64, y: i64, dx: i64, dy: i64, bounded: Option<bool>) -> (i32, i32, Option<u32>) {
        let (mut nx, mut ny) = (x + dx, y + dy);
        let bounded = bounded.unwrap_or(self.bounded);
        let (width, height) = (i64::from(self.width), i64::from(self.height));
        if !bounded && width > 0 && height > 0 {
            nx = nx.rem_euclid(width);
            ny = ny.rem_euclid(height);
        }
        let clamp = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        (clamp(nx), clamp(ny), self.cell_at(nx, ny))
    }

    /// Seeds the running value of each accumulator.
    fn seed(accumulators: &[RepeatAccumulator], frame: &mut [HostValue]) {
        for accumulator in accumulators {
            frame[accumulator.running] = match accumulator.init {
                Some(slot) => frame[slot],
                None => accumulator.identity,
            };
        }
    }

    /// Copies each accumulator's running value to its `prev.<acc>`.
    fn remember(accumulators: &[RepeatAccumulator], frame: &mut [HostValue]) {
        for accumulator in accumulators {
            frame[accumulator.prev] = frame[accumulator.running];
        }
    }

    /// Copies each accumulator's running value to its output.
    fn publish(accumulators: &[RepeatAccumulator], frame: &mut [HostValue]) {
        for accumulator in accumulators {
            frame[accumulator.out] = frame[accumulator.running];
        }
    }

    /// Runs a stencil fold at the current element.
    fn grid_fold(&mut self, node: NodeId, fold: &StencilFold, frame: &mut [HostValue]) -> Result<(), HostError> {
        Self::seed(&fold.accumulators, frame);
        let when = frame[fold.when]
            .as_bool()
            .ok_or_else(|| self.fault(node, "has a `when` that is no bool"))?;
        if when {
            let (x, y) = if let Some(slot) = fold.at {
                let [x, y] = frame[slot]
                    .as_vec2i()
                    .ok_or_else(|| self.fault(node, "folds around a value that is no vec2i"))?;
                (i64::from(x), i64::from(y))
            } else {
                let [x, y] = self.cell_xy(self.element);
                (i64::from(x), i64::from(y))
            };
            let outer = self.iteration;
            let radix = u32::try_from(fold.offsets.len()).unwrap_or(u32::MAX);
            for (k, &(dx, dy)) in fold.offsets.iter().enumerate() {
                let (nx, ny, cell) = self.step_cell(x, y, i64::from(dx), i64::from(dy), fold.bounded);
                let Some(cell) = cell else { continue };
                let k = u32::try_from(k).unwrap_or(u32::MAX);
                let [k_slot, offset, xy, cell_slot, diagonal] = fold.inner;
                frame[k_slot] = HostValue::U32(k);
                frame[offset] = HostValue::Vec2I([dx, dy]);
                frame[xy] = HostValue::Vec2I([nx, ny]);
                frame[cell_slot] = HostValue::Cell(cell);
                frame[diagonal] = HostValue::Bool(dx != 0 && dy != 0);
                Self::remember(&fold.accumulators, frame);
                self.iteration = Some(iteration_word(outer, radix, k));
                let result = self.execute(&fold.body, frame);
                self.iteration = outer;
                result?;
            }
        }
        Self::publish(&fold.accumulators, frame);
        Ok(())
    }

    /// Runs a radius fold at the current agent, over the positions at the start of the pass.
    fn radius_fold(&mut self, node: NodeId, fold: &RadiusFold, frame: &mut [HostValue]) -> Result<(), HostError> {
        Self::seed(&fold.accumulators, frame);
        let when = frame[fold.when]
            .as_bool()
            .ok_or_else(|| self.fault(node, "has a `when` that is no bool"))?;
        if !when {
            Self::publish(&fold.accumulators, frame);
            return Ok(());
        }
        let mismatch = || self.fault(node, "met a value of a type it does not take");
        let [cx, cy] = frame[fold.center].as_vec2f().ok_or_else(mismatch)?;
        let radius = frame[fold.radius].as_f32().ok_or_else(mismatch)?;
        let cell_size = frame[fold.cell_size].as_f32().ok_or_else(mismatch)?;
        let [world_w, world_h] = self.world.extent();
        let column = self
            .lanes
            .columns
            .get(fold.position_lane)
            .ok_or_else(|| self.fault(node, "reads a position lane the world does not hold"))?;
        let positions: Vec<[f32; 2]> = (0..column.len())
            .map(|index| column.get(index).and_then(HostValue::as_vec2f).unwrap_or([0.0, 0.0]))
            .collect();
        if self.index.as_ref().is_none_or(|(bits, _)| *bits != cell_size.to_bits()) {
            let xs: Vec<f32> = positions.iter().map(|position| position[0]).collect();
            let ys: Vec<f32> = positions.iter().map(|position| position[1]).collect();
            let mut index = SpatialHash::new(cell_size, world_w, world_h);
            index.build(&xs, &ys);
            self.index = Some((cell_size.to_bits(), index));
        }
        let visits = {
            let Some((_, index)) = &self.index else {
                return Err(self.fault(node, "has no index"));
            };
            walk(index, cx, cy, radius, &positions, world_w, world_h)
        };
        let visits = match visits {
            Ok(visits) => visits,
            Err(WalkEnd::Cells) => {
                return Err(self.fault(
                    node,
                    format!("Agents in radius would walk more than {MAX_WALK_CELLS} cells"),
                ));
            }
            Err(WalkEnd::Visits) => {
                return Err(self.fault(
                    node,
                    format!("Agents in radius visited more than {MAX_FOLD_CANDIDATES} agents"),
                ));
            }
            Err(WalkEnd::Broken) => return Err(self.fault(node, "walks an index that does not hold its positions")),
        };
        let mut k = 0u32;
        for (other, dx, dy, dist_sq) in visits {
            if fold.exclude_self && other == self.element {
                continue;
            }
            let [k_slot, other_slot, delta, dist] = fold.inner;
            frame[k_slot] = HostValue::U32(k);
            frame[other_slot] = HostValue::Agent(other);
            frame[delta] = HostValue::Vec2F([dx, dy]);
            frame[dist] = HostValue::F32(dist_sq);
            Self::remember(&fold.accumulators, frame);
            let outer = self.iteration;
            self.iteration = Some(other);
            let result = self.execute(&fold.body, frame);
            self.iteration = outer;
            result?;
            k += 1;
        }
        Self::publish(&fold.accumulators, frame);
        Ok(())
    }

    /// Writes each float sum's result into its slot.
    fn finish_sums(&mut self, frame: &mut [HostValue]) {
        let count = self.elements.max(1) as f64;
        for (spec, state) in self.program.sums.iter().zip(&mut self.sums) {
            if state.in_chunk > 0 {
                close_chunk(state, spec.wide_chunks);
            }
            let scale = if spec.mean { 1.0 / count } else { 1.0 };
            let [x, y] = state.total;
            let value = if spec.vector.unwrap_or(state.vector) {
                HostValue::Vec2D([x * scale, y * scale])
            } else {
                HostValue::F64(x * scale)
            };
            if let Some(target) = frame.get_mut(spec.slot) {
                *target = value;
            }
        }
    }

    /// Returns the number of stencil positions around the current element whose value in `column` equals `value`.
    fn count_neighbors(&self, column: &LaneColumn, offsets: &[(i64, i64)], value: HostValue) -> u32 {
        let (width, height) = (i64::from(self.width), i64::from(self.height));
        if width == 0 || height == 0 {
            return 0;
        }
        let x = i64::from(self.element) % width;
        let y = i64::from(self.element) / width;
        let mut count = 0;
        for (dx, dy) in offsets {
            let (mut nx, mut ny) = (x + dx, y + dy);
            if self.bounded {
                if nx < 0 || ny < 0 || nx >= width || ny >= height {
                    continue;
                }
            } else {
                nx = nx.rem_euclid(width);
                ny = ny.rem_euclid(height);
            }
            if column.get((ny * width + nx) as usize) == Some(value) {
                count += 1;
            }
        }
        count
    }

    /// Runs a Repeat at the current element.
    fn repeat(&mut self, node: NodeId, repeat: &Repeat, frame: &mut [HostValue]) -> Result<(), HostError> {
        let count = frame[repeat.count]
            .as_u32()
            .ok_or_else(|| self.fault(node, "counts with a value that is no u32"))?;
        let when = frame[repeat.when]
            .as_bool()
            .ok_or_else(|| self.fault(node, "has a `when` that is no bool"))?;
        let total = if when { count.min(repeat.max) } else { 0 };
        for accumulator in &repeat.accumulators {
            frame[accumulator.running] = match accumulator.init {
                Some(slot) => frame[slot],
                None => accumulator.identity,
            };
        }
        frame[repeat.total] = HostValue::U32(total);
        let outer = self.iteration;
        for k in 0..total {
            frame[repeat.k] = HostValue::U32(k);
            for accumulator in &repeat.accumulators {
                frame[accumulator.prev] = frame[accumulator.running];
            }
            self.iteration = Some(iteration_word(outer, repeat.max, k));
            let result = self.execute(&repeat.body, frame);
            if result.is_err() {
                self.iteration = outer;
                return result;
            }
        }
        self.iteration = outer;
        for accumulator in &repeat.accumulators {
            frame[accumulator.out] = frame[accumulator.running];
        }
        Ok(())
    }
}

/// Folds the current chunk of a float sum into its total and starts a new chunk from `+0.0`.
fn close_chunk(state: &mut SumState, wide: bool) {
    let chunk = if wide {
        state.chunk
    } else {
        [f64::from(state.narrow[0]), f64::from(state.narrow[1])]
    };
    state.total[0] += chunk[0];
    state.total[1] += chunk[1];
    state.chunk = [0.0, 0.0];
    state.narrow = [0.0, 0.0];
    state.in_chunk = 0;
}

/// Returns `x` truncated toward zero to an `i32`, clamped to the `f32` values an `i32` holds, as WGSL converts.
fn clamp_to_i32(x: f32) -> i32 {
    let top = f32::from_bits(0x4eff_ffff);
    (if x > top { top } else { x }) as i32
}

/// Reason a walk of [`walk`] stops before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkEnd {
    /// The walk would cover more than [`MAX_WALK_CELLS`] cells, and stops before the first.
    Cells,
    /// The walk met more than [`MAX_FOLD_CANDIDATES`] candidates.
    Visits,
    /// The index does not hold a cell or an agent the walk reaches.
    Broken,
}

/// Returns the agents within `radius` of `(x, y)` in the walk order of [`SpatialHash::for_each_within`], each with the
/// toroidal deltas to it and their squared length.
///
/// # Errors
///
/// Returns [`WalkEnd::Cells`] for a walk over more than [`MAX_WALK_CELLS`] cells, as `henad::hash::walk_cells` counts
/// them, and [`WalkEnd::Visits`] when the walk meets more than [`MAX_FOLD_CANDIDATES`] candidates.
fn walk(
    index: &SpatialHash,
    x: f32,
    y: f32,
    radius: f32,
    positions: &[[f32; 2]],
    world_w: f32,
    world_h: f32,
) -> Result<Vec<(u32, f32, f32, f32)>, WalkEnd> {
    let (grid_w, grid_h) = index.grid_dims();
    let (cell_w, cell_h) = index.cell_extents();
    let (cell_start, sorted) = index.buckets();
    let r2 = radius * radius;
    let cell_radius_x = (radius / cell_w).ceil() as i32;
    let cell_radius_y = (radius / cell_h).ceil() as i32;
    let cell_x = ((x * (1.0 / cell_w)).floor() as i32).rem_euclid(grid_w as i32);
    let cell_y = ((y * (1.0 / cell_h)).floor() as i32).rem_euclid(grid_h as i32);
    // `2 * reach + 1 > side`, written so that no reach overflows it, as `henad::hash::walk_bounds` writes it.
    let (y_lo, y_hi) = if cell_radius_y > (grid_h as i32 - 1) / 2 {
        (0, grid_h as i32 - 1)
    } else {
        (cell_y - cell_radius_y, cell_y + cell_radius_y)
    };
    let (x_lo, x_hi) = if cell_radius_x > (grid_w as i32 - 1) / 2 {
        (0, grid_w as i32 - 1)
    } else {
        (cell_x - cell_radius_x, cell_x + cell_radius_x)
    };
    // A negative radius covers no cell, as `henad::hash::walk_cells` counts it.
    let cells = (i64::from(x_hi) - i64::from(x_lo) + 1).max(0) * (i64::from(y_hi) - i64::from(y_lo) + 1).max(0);
    if cells > i64::from(MAX_WALK_CELLS) {
        return Err(WalkEnd::Cells);
    }
    let mut candidates = 0u32;
    let mut found = Vec::new();
    for grid_y in y_lo..=y_hi {
        let wrapped_y = grid_y.rem_euclid(grid_h as i32) as u32;
        for grid_x in x_lo..=x_hi {
            let wrapped_x = grid_x.rem_euclid(grid_w as i32) as u32;
            let cell = (wrapped_y * grid_w + wrapped_x) as usize;
            let bound = |cell: usize| cell_start.get(cell).map(|&start| start as usize).ok_or(WalkEnd::Broken);
            let (start, end) = (bound(cell)?, bound(cell + 1)?);
            for &agent in sorted.get(start..end).ok_or(WalkEnd::Broken)? {
                candidates += 1;
                if candidates > MAX_FOLD_CANDIDATES {
                    return Err(WalkEnd::Visits);
                }
                let [ox, oy] = *positions.get(agent as usize).ok_or(WalkEnd::Broken)?;
                let dx = axis_delta(x, ox, world_w, Boundary::Torus);
                let dy = axis_delta(y, oy, world_h, Boundary::Torus);
                let d2 = dx * dx + dy * dy;
                if d2 <= r2 {
                    found.push((agent, dx, dy, d2));
                }
            }
        }
    }
    Ok(found)
}
