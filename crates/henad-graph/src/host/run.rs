//! Execution of a compiled pass over the elements of its domain, one element at a time in index order.

use henad_core::authoring::primitives::rng::{mix_seed, pcg_hash, random_float, xorshift64};

use super::compile::{CompareOp, DrawForm, Instruction, LogicOp, Operation, Program, Repeat};
use super::value::{BinaryOp, HostValue, binary, convert, order, shift, unary};
use super::{HostError, HostLanes, HostWorld, LaneColumn, PassOutcome, RunContext};
use crate::draw::{draw_bits, fold32, iteration_word, key_word, press_word};
use crate::ids::NodeId;
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
}

/// Runs `program` over every element of its domain in `lanes`, and commits its writes.
///
/// # Errors
///
/// Returns [`HostError::Fault`] for a value the program cannot use, such as a lane of another size than the world's.
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
    let (width, height, bounded) = match lanes.world() {
        HostWorld::Grid {
            width,
            height,
            boundary,
        } => (
            *width,
            *height,
            *boundary == henad_core::authoring::primitives::space::Boundary::Bounded,
        ),
        HostWorld::Agents { .. } => (0, 0, false),
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
    let mut staged: Vec<Option<LaneColumn>> = vec![None; lanes.columns().len()];
    for &lane in &program.written {
        let fault = |reason: &str| HostError::Fault {
            pass: program.pass.clone(),
            node: None,
            reason: format!("{reason} {lane}"),
        };
        let column = lanes
            .columns()
            .get(lane)
            .ok_or_else(|| fault("the lanes hold no lane at position"))?;
        // `staged` has one entry per column, so the position found above is in range.
        staged[lane] = Some(
            column
                .try_clone()
                .ok_or_else(|| fault("the host has no memory for a copy of the lane at position"))?,
        );
    }
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
    machine.execute(&program.post, &mut frame)?;
    let colors = machine.colors;
    let staged = machine.staged;
    for (position, column) in staged.into_iter().enumerate() {
        if let Some(column) = column {
            lanes.replace(position, column);
        }
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

/// Returns the frame of `program` after its prelude and, when `tallies` is `Some`, its post block, with each tally's
/// count in place of the elements.
///
/// The GPU counts the tallies of a stats pass, and the host runs the values computed from them.
///
/// # Errors
///
/// Returns [`HostError::Fault`] for a value either block cannot use.
pub(crate) fn finish(
    program: &Program,
    params: &[HostValue],
    context: &RunContext,
    world: HostWorld,
    tallies: Option<&[u64]>,
) -> Result<Vec<HostValue>, HostError> {
    let lanes = HostLanes {
        world,
        columns: Vec::new(),
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
        width: 0,
        height: 0,
        bounded: false,
        element: 0,
        iteration: None,
    };
    machine.execute(&program.prelude, &mut frame)?;
    if let Some(tallies) = tallies {
        for (slot, count) in program.tally_slots.iter().zip(tallies) {
            if let Some(target) = frame.get_mut(*slot) {
                *target = HostValue::U64(*count);
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
    let (width, height, bounded) = match lanes.world() {
        HostWorld::Grid {
            width,
            height,
            boundary,
        } => (
            *width,
            *height,
            *boundary == henad_core::authoring::primitives::space::Boundary::Bounded,
        ),
        HostWorld::Agents { .. } => (0, 0, false),
    };
    let mut machine = Machine {
        program,
        lanes,
        staged: vec![None; lanes.columns().len()],
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
        let value = match &instruction.operation {
            Operation::Index => HostValue::U32(self.element),
            Operation::Tick => HostValue::U32(self.context.tick),
            Operation::PressSeed => HostValue::U32(self.press_seed),
            Operation::LaneGet { lane } => self
                .lanes
                .columns()
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
                        .columns()
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
