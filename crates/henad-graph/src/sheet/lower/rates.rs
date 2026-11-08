//! Lines that draw at a rate: a rate alone, and a group of rates that compete.
//!
//! A rate `r` per unit of time holds within one tick with `1 - e^(-r dt)`, where `dt` is the length of one tick in the
//! rate's unit: the hidden parameter [`TICK_LENGTH`], counted in the tick's unit, and a literal factor when the rate's
//! unit differs (a rate per week in a model of days is multiplied by 1/7). A value that reads only parameters and
//! literals is the same for every element. The host computes it once per build, and the device reads it. Such a rate
//! with no count takes its chance from the series `h (1 - h (1/2 - h/6))` below a hazard of 2^-10, since `1 - e^(-h)`
//! in `f32` rounds to a multiple of 2^-24, and draws through `rng.threshold` and `rand.below` at 2^-32.
//!
//! A rate for each of a count `n` holds with `1 - e^(-r dt n)`. When `n` counts the cells around and `r` is the same
//! for every element, that is `1 - q^n` with `q = e^(-r dt)`, through a `math.powi` whose `max_exponent` is the number
//! of cells around. Otherwise it is a `math.exp` per element behind a `logic.select` that takes 1 for a count of 0 or
//! below, since `e^0` is not exactly 1 on every device.
//!
//! A *competing group* is the set of Become lines that [`CardRelations::competing_groups`] gives, less the lines that
//! do not run. Each line `i` of a group has a hazard `a_i`: its rate in the tick's unit, times its count when it has
//! one, and 0 where the rest of its condition fails. A cell leaves with chance `t = 1 - e^(-dt (a_1 + ... + a_k))`,
//! and line `j` holds when `u < t (a_1 + ... + a_j) / (a_1 + ... + a_k)`, for one `rand.float` draw `u` in `[0, 1)`
//! keyed by the group's highest line. The transition takes the first row that holds, so each line takes the share of
//! `t` its hazard gives it. A sum of 0 or below gives `t = 0` through a `logic.select`, the guard of a rate for each
//! of a count.
//!
//! [`CardRelations::competing_groups`]: crate::sheet::relations::CardRelations::competing_groups

use super::Site;
use super::card::CardLowering;
use super::value::Operand;
use crate::sheet::document::{LineKind, SheetLine, SheetValue};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// Hazard below which [`CardLowering::rate`] computes a chance by its series, where `1 - e^(-h)` in `f32` loses the
/// digits of `h`.
const SMALL_HAZARD: f32 = 1.0 / 1024.0;

/// Id of the hidden parameter that holds the length of one tick, in the tick's unit.
pub(super) const TICK_LENGTH: &str = "tick_length";

/// Smallest total hazard a group's shares divide by. Otherwise a group whose rates are all 0 would divide by 0.
const SMALLEST_TOTAL: f32 = 1e-30;

/// One line of a competing group, lowered up to its share.
#[derive(Debug, Clone)]
pub(super) struct GroupLine<'a> {
    /// Id of the line.
    line: &'a str,
    /// Site of the line's rate phrase.
    site: Site,
    /// Hazard of the line in the tick's unit, 0 where the rest of its condition fails.
    hazard: InputValue,
    /// Rest of the line's condition, the conditions of the When lines around it joined in.
    gate: Operand,
}

/// A competing group of a card: its lines in display order, and those lowered so far.
#[derive(Debug, Clone)]
pub(super) struct Group<'a> {
    /// Ids of the lines, in display order.
    pub(super) lines: Vec<&'a str>,
    /// Lines lowered so far, in display order.
    lowered: Vec<GroupLine<'a>>,
}

impl<'a> Group<'a> {
    /// Returns the group of the lines `lines`, given in display order, none of them lowered yet.
    pub(super) fn new(lines: Vec<&'a str>) -> Self {
        Self {
            lines,
            lowered: Vec::new(),
        }
    }
}

/// Returns the output of the temporary node `node` as an input.
fn link(node: crate::ids::NodeId) -> InputValue {
    InputValue::Link(PortRef { node, port: None })
}

/// Returns the `f32` literal `value`.
fn number(value: f32) -> InputValue {
    InputValue::Literal(Literal::F32(value))
}

impl<'a> CardLowering<'_, 'a> {
    /// Writes `op` over `inputs` at `site`, whose one output is an `f32`, and returns that output as an input.
    fn float(&mut self, site: &Site, op: &str, inputs: Vec<(&str, InputValue)>) -> Result<InputValue, String> {
        let value = self.builder.node_value(site, op, inputs, ValueType::F32);
        self.builder.input_as(site, &value, &ValueType::F32, false)
    }

    /// Returns `1 - e^(-hazard)` written at `site` with no cancellation: the series `h (1 - h (1/2 - h/6))` below
    /// [`SMALL_HAZARD`], and [`Self::chance_of`] from there up.
    fn exact_chance_of(&mut self, site: &Site, hazard: InputValue) -> Result<InputValue, String> {
        let direct = self.chance_of(site, hazard.clone())?;
        let sixth = self.float(site, "math.div", vec![("a", hazard.clone()), ("b", number(6.0))])?;
        let half = self.float(site, "math.sub", vec![("a", number(0.5)), ("b", sixth)])?;
        let second = self.float(site, "math.mul", vec![("a", hazard.clone()), ("b", half)])?;
        let first = self.float(site, "math.sub", vec![("a", number(1.0)), ("b", second)])?;
        let series = self.float(site, "math.mul", vec![("a", hazard.clone()), ("b", first)])?;
        let small = self.builder.node_value(
            site,
            "cmp.lt",
            vec![("a", hazard), ("b", number(SMALL_HAZARD))],
            ValueType::Bool,
        );
        let small = self.builder.condition_input(site, &small)?;
        self.float(
            site,
            "logic.select",
            vec![("cond", small), ("if_true", series), ("if_false", direct)],
        )
    }

    /// Returns `1 - e^(-hazard)` written at `site`.
    fn chance_of(&mut self, site: &Site, hazard: InputValue) -> Result<InputValue, String> {
        let negated = self.float(site, "math.neg", vec![("a", hazard)])?;
        let stays = self.float(site, "math.exp", vec![("a", negated)])?;
        self.float(site, "math.sub", vec![("a", number(1.0)), ("b", stays)])
    }

    /// Returns the rate of the rate phrase `line` in the tick's unit, read from `rate`, the operand of its rate slot.
    ///
    /// A rate in another unit than the tick's is multiplied by the ratio of the two units as a literal.
    fn rate_in_tick_unit(&mut self, site: &Site, line: &SheetLine, rate: &Operand) -> Result<InputValue, String> {
        let rate = self.builder.input_as(site, rate, &ValueType::F32, false)?;
        let unit = line
            .rate_unit
            .ok_or_else(|| "a rate needs its unit of time".to_owned())?;
        let time = self
            .builder
            .document
            .time
            .ok_or_else(|| "a rate needs the length of one tick".to_owned())?;
        let factor = time.tick_unit.days() / unit.days();
        if factor == 1.0 {
            return Ok(rate);
        }
        // The ratio of two units lies between 1/31557600 and 31557600. An `f32` holds it to its precision.
        self.float(site, "math.mul", vec![("a", rate), ("b", number(factor as f32))])
    }

    /// Returns the count of the rate phrase `line` as an `f32` input, `None` when it has no "for each".
    fn tries(&mut self, site: &Site, line: &'a SheetLine) -> Result<Option<InputValue>, String> {
        if line.tries.is_blank() {
            return Ok(None);
        }
        let tries = self.slot(&site.at("tries"), &line.tries)?;
        self.builder
            .input_as(&site.at("tries"), &tries, &ValueType::F32, false)
            .map(Some)
    }

    /// Returns the outcome of the rate phrase or rate line `line` lowered at `site`, drawn once.
    ///
    /// A rate the same for every element with no count is a chance the host computes once per build with no
    /// cancellation, compared as a `u32` threshold with all 32 bits of the draw. Every other rate is drawn by
    /// `rand.chance`, whose chance is resolved to 2^-24.
    pub(super) fn rate(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let rate = self.slot(&site.at("rate"), &line.rate)?;
        let uniform = matches!(rate, Operand::Number(_) | Operand::Constant(_) | Operand::Param(..));
        let scaled = self.rate_in_tick_unit(site, line, &rate)?;
        let hazard = self.float(
            site,
            "math.mul",
            vec![("a", scaled), ("b", InputValue::Param(TICK_LENGTH.to_owned()))],
        )?;
        if uniform && line.tries.is_blank() {
            let probability = self.exact_chance_of(site, hazard)?;
            let threshold = self
                .builder
                .node_value(site, "rng.threshold", vec![("p", probability)], ValueType::U32);
            let threshold = self.builder.input_as(site, &threshold, &ValueType::U32, false)?;
            let node = self.draw(site, "rand.below", vec![("threshold", threshold)])?;
            return Ok(Operand::Link(PortRef { node, port: None }, ValueType::Bool));
        }
        let probability = if line.tries.is_blank() {
            self.chance_of(site, hazard)?
        } else {
            match self.count_bound(line.tries.value()) {
                Some(bound) if uniform => {
                    let tries_site = site.at("tries");
                    let tries = self.slot(&tries_site, &line.tries)?;
                    let exponent = self.builder.input_as(&tries_site, &tries, &ValueType::U32, false)?;
                    let negated = self.float(site, "math.neg", vec![("a", hazard)])?;
                    let stays = self.float(site, "math.exp", vec![("a", negated)])?;
                    let node = self.builder.emit(
                        site,
                        "math.powi",
                        vec![("max_exponent", ConfigValue::Integer(i64::from(bound)))],
                        vec![("base".to_owned(), stays), ("exponent".to_owned(), exponent)],
                    );
                    self.float(site, "math.sub", vec![("a", number(1.0)), ("b", link(node))])?
                }
                _ => {
                    let tries = self
                        .tries(site, line)?
                        .ok_or_else(|| "a rate for each has no count".to_owned())?;
                    let total = self.float(site, "math.mul", vec![("a", hazard), ("b", tries.clone())])?;
                    let negated = self.float(site, "math.neg", vec![("a", total)])?;
                    let stays = self.float(site, "math.exp", vec![("a", negated)])?;
                    // No exposure at all keeps the cell, whatever the rate.
                    let none = self.builder.node_value(
                        site,
                        "cmp.le",
                        vec![("a", tries), ("b", number(0.0))],
                        ValueType::Bool,
                    );
                    let none = self.builder.condition_input(site, &none)?;
                    let guarded = self.float(
                        site,
                        "logic.select",
                        vec![("cond", none), ("if_true", number(1.0)), ("if_false", stays)],
                    )?;
                    self.float(site, "math.sub", vec![("a", number(1.0)), ("b", guarded)])?
                }
            }
        };
        let node = self.draw(site, "rand.chance", vec![("p", probability)])?;
        Ok(Operand::Link(PortRef { node, port: None }, ValueType::Bool))
    }

    /// Returns the index in [`CardLowering::groups`] of the competing group that holds line `id`.
    pub(super) fn group_of(&self, id: &str) -> Option<usize> {
        self.groups.iter().position(|group| group.lines.contains(&id))
    }

    /// Lowers the condition of line `id`, a Become of the competing group at `group`, up to its share, and returns a
    /// placeholder condition that [`Self::finish_group`] replaces once every line of the group is lowered.
    pub(super) fn group_line(
        &mut self,
        group: usize,
        site: &Site,
        id: &'a str,
        line: &'a SheetLine,
    ) -> Result<Operand, String> {
        let condition = line
            .condition
            .value()
            .ok_or_else(|| "a competing line has no condition".to_owned())?;
        let mut found = None;
        let own = self.gate(&site.at("condition"), condition, &mut found)?;
        let gate = self.inherit(site, id, own)?;
        let (rate_site, phrase) = found.ok_or_else(|| "a competing line holds no rate".to_owned())?;
        let rate = self.slot(&rate_site.at("rate"), &phrase.rate)?;
        let mut hazard = self.rate_in_tick_unit(&rate_site, phrase, &rate)?;
        if let Some(tries) = self.tries(&rate_site, phrase)? {
            hazard = self.float(&rate_site, "math.mul", vec![("a", hazard), ("b", tries)])?;
        }
        if gate != Operand::Constant(Literal::Bool(true)) {
            let cond = self.builder.condition_input(&rate_site, &gate)?;
            hazard = self.float(
                &rate_site,
                "logic.select",
                vec![("cond", cond), ("if_true", hazard), ("if_false", number(0.0))],
            )?;
        }
        self.groups[group].lowered.push(GroupLine {
            line: id,
            site: rate_site,
            hazard,
            gate,
        });
        Ok(Operand::Constant(Literal::Bool(false)))
    }

    /// Returns the condition `value` with its one rate phrase read as yes, recording the rate's site and phrase in
    /// `found`. The rate stands as the condition or as an item of an all of, nested all of included.
    fn gate(
        &mut self,
        site: &Site,
        value: &'a SheetValue,
        found: &mut Option<(Site, &'a SheetLine)>,
    ) -> Result<Operand, String> {
        match value {
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::Rate => {
                *found = Some((site.clone(), phrase));
                Ok(Operand::Constant(Literal::Bool(true)))
            }
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::AllOf => {
                let mut joined = Operand::Constant(Literal::Bool(true));
                for (index, item) in phrase.items.iter().enumerate() {
                    let item_site = site.at(&format!("items.{index}"));
                    let operand = self.gate(&item_site, item, found)?;
                    joined = self.builder.join(&item_site, "logic.and", &joined, &operand)?;
                }
                Ok(joined)
            }
            _ => self.value(site, value),
        }
    }

    /// Writes the draw and the shares of the competing group at `group` once its last line is lowered, and gives each
    /// of its changes its condition.
    pub(super) fn finish_group(&mut self, group: usize) -> Result<(), String> {
        let lines = self.groups[group].lowered.clone();
        if lines.len() < self.groups[group].lines.len() {
            return Ok(());
        }
        let Some(first) = lines.first() else {
            return Ok(());
        };
        let site = first.site.clone();
        let mut sums = vec![first.hazard.clone()];
        for line in &lines[1..] {
            let previous = sums
                .last()
                .cloned()
                .unwrap_or_else(|| unreachable!("the sums hold the first hazard"));
            sums.push(self.float(&site, "math.add", vec![("a", previous), ("b", line.hazard.clone())])?);
        }
        let total = sums
            .last()
            .cloned()
            .unwrap_or_else(|| unreachable!("the sums hold the first hazard"));
        let hazard = self.float(
            &site,
            "math.mul",
            vec![("a", total.clone()), ("b", InputValue::Param(TICK_LENGTH.to_owned()))],
        )?;
        let negated = self.float(&site, "math.neg", vec![("a", hazard)])?;
        let exp = self.float(&site, "math.exp", vec![("a", negated)])?;
        // A total hazard of 0 keeps the cell, as a lone rate at a count of 0 does.
        let none = self.builder.node_value(
            &site,
            "cmp.le",
            vec![("a", total.clone()), ("b", number(0.0))],
            ValueType::Bool,
        );
        let none = self.builder.condition_input(&site, &none)?;
        let stays = self.float(
            &site,
            "logic.select",
            vec![("cond", none), ("if_true", number(1.0)), ("if_false", exp)],
        )?;
        let leaves = self.float(&site, "math.sub", vec![("a", number(1.0)), ("b", stays)])?;
        let divisor = self.float(&site, "math.max", vec![("a", total), ("b", number(SMALLEST_TOTAL))])?;
        let drawn = self.draw(&site, "rand.float", vec![("max", number(1.0))])?;
        let last = lines.len() - 1;
        for (index, line) in lines.iter().enumerate() {
            let threshold = if index == last {
                leaves.clone()
            } else {
                let share = self.float(
                    &line.site,
                    "math.div",
                    vec![("a", sums[index].clone()), ("b", divisor.clone())],
                )?;
                self.float(&line.site, "math.mul", vec![("a", leaves.clone()), ("b", share)])?
            };
            let below = self.builder.node_value(
                &line.site,
                "cmp.lt",
                vec![("a", link(drawn)), ("b", threshold)],
                ValueType::Bool,
            );
            let condition = self.builder.join(&line.site, "logic.and", &line.gate, &below)?;
            let change = self
                .changes
                .values_mut()
                .flatten()
                .find(|change| change.line == line.line)
                .ok_or_else(|| format!("line {} of a competing group made no change", line.line))?;
            change.condition = condition;
        }
        Ok(())
    }
}
