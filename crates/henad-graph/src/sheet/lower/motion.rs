//! Changes that steer, hold and move an agent: Steer, Keep between, Move by and a random direction.
//!
//! The Steers of a property add up onto its value at the start of the tick, each a term of the value times the
//! strength, or for "towards" the difference from my value times the strength. A Steer towards a part of a Look around
//! that found no one adds nothing. A Keep between holds the value inside its limits after the changes above it: a
//! number through a clamp, and an arrow through the clamp of its length, an arrow of length 0 turning into one of the
//! lower length along the x axis. The Move by lines add up, and the position moves by their sum and wraps at the
//! world's edges. Heading holds the last move that was not 0, and none before the first. "my new X" is the value before
//! its write. A random direction is an arrow of the given length at an angle drawn between 0 and 2 pi.

use super::Site;
use super::card::{CardLowering, Change, ChangeKind};
use super::ids::{GROUP_CHANGE, Owner, SheetKey};
use super::value::Operand;
use crate::sheet::document::{LineMode, PropertyType, SheetChip, SheetLine, SheetValue};
use crate::types::ValueType;
use crate::value::{InputValue, Literal, PortRef};

/// Smallest length an arrow is divided by when Keep between scales it.
const SMALLEST_LENGTH: f32 = 0.000_001;

impl<'a> CardLowering<'_, 'a> {
    /// Returns the condition of change line `id` at `site`: its own, `always` when it has none, joined with the When
    /// lines around it.
    fn change_condition(&mut self, site: &Site, id: &str, line: &'a SheetLine) -> Result<Operand, String> {
        let own = if line.condition.is_blank() {
            Operand::Constant(Literal::Bool(true))
        } else {
            self.slot(&site.at("condition"), &line.condition)?
        };
        self.inherit(site, id, own)
    }

    /// Returns `value` where `condition` holds and the zero of `value_type` elsewhere, written at `site`.
    fn where_applies(
        &mut self,
        site: &Site,
        condition: &Operand,
        value: &Operand,
        value_type: &ValueType,
    ) -> Result<Operand, String> {
        if matches!(condition, Operand::Constant(Literal::Bool(true))) {
            return Ok(value.clone());
        }
        let zero = Literal::zero_of(value_type).ok_or_else(|| format!("a {value_type} has no zero"))?;
        let cond = self.builder.condition_input(site, condition)?;
        let if_true = self.builder.input_as(site, value, value_type, false)?;
        Ok(self.builder.node_value(
            site,
            "logic.select",
            vec![
                ("cond", cond),
                ("if_true", if_true),
                ("if_false", InputValue::Literal(zero)),
            ],
            value_type.clone(),
        ))
    }

    /// Returns `value` times the number `strength`, a scaled arrow or a product of numbers, written at `site`.
    fn scaled(
        &mut self,
        site: &Site,
        value: &Operand,
        strength: &Operand,
        value_type: &ValueType,
    ) -> Result<Operand, String> {
        let value = self.builder.input_as(site, value, value_type, false)?;
        let strength = self.builder.input_as(site, strength, &ValueType::F32, false)?;
        Ok(if *value_type == ValueType::Vec2F {
            self.builder
                .node_value(site, "vec.scale", vec![("v", value), ("s", strength)], ValueType::Vec2F)
        } else {
            self.builder.node_value(
                site,
                "math.mul",
                vec![("a", value), ("b", strength)],
                value_type.clone(),
            )
        })
    }

    /// Lowers the Steer line `id`: its term, added to the value at the start of the tick, or into the Repeat that
    /// holds it.
    pub(super) fn steer(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let (index, property) = self.property(line.property.as_deref())?;
        let value_type = super::world::lane_type(property);
        let value = self.slot(&site.at("value"), &line.value)?;
        let strength = self.slot(&site.at("strength"), &line.strength)?;
        let towards = line.mode == Some(LineMode::Towards);
        let pushed = if towards {
            let current = self.builder.read(self.pass, &property.id)?;
            let a = self.builder.input_as(site, &value, &value_type, false)?;
            let b = self.builder.input_as(site, &current, &value_type, false)?;
            self.builder
                .node_value(site, "math.sub", vec![("a", a), ("b", b)], value_type.clone())
        } else {
            value
        };
        let mut term = self.scaled(site, &pushed, &strength, &value_type)?;
        if towards && let Some(count) = self.empty_count(line.value.value()) {
            // A Steer towards a value of no one adds nothing.
            let count = self.builder.input_as(site, &count, &ValueType::U32, false)?;
            let found = self.builder.node_value(
                site,
                "cmp.gt",
                vec![("a", count), ("b", InputValue::Literal(Literal::U32(0)))],
                ValueType::Bool,
            );
            term = self.where_applies(site, &found, &term, &value_type)?;
        }
        let condition = self.change_condition(site, id, line)?;
        if let Some(repeat) = self.repeat_of(id) {
            return self.put_in_repeat(site, repeat, index, &term, &condition);
        }
        let term = self.where_applies(&site.at("condition"), &condition, &term, &value_type)?;
        self.add_change(
            index,
            id,
            ChangeKind::Steer { term },
            Operand::Constant(Literal::Bool(true)),
        )
    }

    /// Returns the count of the Look around part that `value` reads, when that part is empty for no one.
    fn empty_count(&self, value: Option<&SheetValue>) -> Option<Operand> {
        let SheetValue::Chip(SheetChip::Result {
            line,
            part: Some(part),
            property: None,
        }) = value?
        else {
            return None;
        };
        self.empty.get(&(line.as_str(), part.as_str())).cloned()
    }

    /// Lowers the Keep between line `id`, a change that holds its property inside the line's limits.
    pub(super) fn keep_between(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let (index, _) = self.property(line.property.as_deref())?;
        let low = self.slot(&site.at("minimum"), &line.minimum)?;
        let high = self.slot(&site.at("maximum"), &line.maximum)?;
        let condition = self.change_condition(site, id, line)?;
        self.add_change(index, id, ChangeKind::Keep { low, high }, condition)
    }

    /// Lowers the Move by line `id`: its arrow, added to the position, or into the Repeat that holds it.
    pub(super) fn move_by(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let index = self
            .builder
            .properties()
            .iter()
            .position(|property| property.property_type == PropertyType::Position)
            .ok_or_else(|| "the agents have no Position to move".to_owned())?;
        let arrow = self.slot(&site.at("value"), &line.value)?;
        let condition = self.change_condition(site, id, line)?;
        if let Some(repeat) = self.repeat_of(id) {
            return self.put_in_repeat(site, repeat, index, &arrow, &condition);
        }
        let arrow = self.where_applies(&site.at("condition"), &condition, &arrow, &ValueType::Vec2F)?;
        self.add_change(
            index,
            id,
            ChangeKind::MoveBy { arrow },
            Operand::Constant(Literal::Bool(true)),
        )
    }

    /// Returns a random direction of the phrase or line `line`'s length, its angle drawn at `site`.
    pub(super) fn random_direction(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let length = self.slot(&site.at("length"), &line.length)?;
        let length = self.builder.input_as(site, &length, &ValueType::F32, false)?;
        let angle = self.draw(
            site,
            "rand.float",
            vec![("max", InputValue::Literal(Literal::F32(std::f32::consts::TAU)))],
        )?;
        Ok(self.builder.node_value(
            site,
            "vec.from_angle",
            vec![
                (
                    "angle",
                    InputValue::Link(PortRef {
                        node: angle,
                        port: None,
                    }),
                ),
                ("length", length),
            ],
            ValueType::Vec2F,
        ))
    }

    /// Returns `value` after the Steers and Keep betweens among `changes`, in row order.
    pub(super) fn steered(
        &mut self,
        value: Operand,
        changes: &[Change<'a>],
        value_type: &ValueType,
    ) -> Result<Operand, String> {
        let mut value = value;
        for change in changes {
            let site = self.change_site(change.line)?;
            match &change.kind {
                ChangeKind::Steer { term } => {
                    let a = self.builder.input_as(&site, &value, value_type, false)?;
                    let b = self.builder.input_as(&site, term, value_type, false)?;
                    value = self
                        .builder
                        .node_value(&site, "math.add", vec![("a", a), ("b", b)], value_type.clone());
                }
                ChangeKind::Keep { low, high } => {
                    let kept = self.kept(&site, &value, low, high, value_type)?;
                    value = if matches!(change.condition, Operand::Constant(Literal::Bool(true))) {
                        kept
                    } else {
                        let cond = self.builder.condition_input(&site, &change.condition)?;
                        let if_true = self.builder.input_as(&site, &kept, value_type, false)?;
                        let if_false = self.builder.input_as(&site, &value, value_type, false)?;
                        self.builder.node_value(
                            &site,
                            "logic.select",
                            vec![("cond", cond), ("if_true", if_true), ("if_false", if_false)],
                            value_type.clone(),
                        )
                    };
                }
                ChangeKind::Become { .. } | ChangeKind::Set { .. } | ChangeKind::MoveBy { .. } => {}
            }
        }
        Ok(value)
    }

    /// Returns `value` held between `low` and `high`, written at `site`: a number clamped, or an arrow whose length is
    /// clamped.
    fn kept(
        &mut self,
        site: &Site,
        value: &Operand,
        low: &Operand,
        high: &Operand,
        value_type: &ValueType,
    ) -> Result<Operand, String> {
        if *value_type != ValueType::Vec2F {
            let x = self.builder.input_as(site, value, value_type, false)?;
            let lo = self.builder.input_as(site, low, value_type, false)?;
            let hi = self.builder.input_as(site, high, value_type, false)?;
            return Ok(self.builder.node_value(
                site,
                "math.clamp",
                vec![("x", x), ("lo", lo), ("hi", hi)],
                value_type.clone(),
            ));
        }
        let arrow = self.builder.input_as(site, value, &ValueType::Vec2F, false)?;
        let lo = self.builder.input_as(site, low, &ValueType::F32, false)?;
        let hi = self.builder.input_as(site, high, &ValueType::F32, false)?;
        let length = self
            .builder
            .node_value(site, "vec.length", vec![("v", arrow.clone())], ValueType::F32);
        let length = self.builder.input_as(site, &length, &ValueType::F32, false)?;
        let clamped = self.builder.node_value(
            site,
            "math.clamp",
            vec![("x", length.clone()), ("lo", lo.clone()), ("hi", hi)],
            ValueType::F32,
        );
        let clamped = self.builder.input_as(site, &clamped, &ValueType::F32, false)?;
        let divisor = self.builder.node_value(
            site,
            "math.max",
            vec![
                ("a", length.clone()),
                ("b", InputValue::Literal(Literal::F32(SMALLEST_LENGTH))),
            ],
            ValueType::F32,
        );
        let divisor = self.builder.input_as(site, &divisor, &ValueType::F32, false)?;
        let ratio = self
            .builder
            .node_value(site, "math.div", vec![("a", clamped), ("b", divisor)], ValueType::F32);
        let ratio = self.builder.input_as(site, &ratio, &ValueType::F32, false)?;
        let scaled = self
            .builder
            .node_value(site, "vec.scale", vec![("v", arrow), ("s", ratio)], ValueType::Vec2F);
        let scaled = self.builder.input_as(site, &scaled, &ValueType::Vec2F, false)?;
        let moving = self.builder.node_value(
            site,
            "cmp.gt",
            vec![("a", length), ("b", InputValue::Literal(Literal::F32(0.0)))],
            ValueType::Bool,
        );
        let moving = self.builder.condition_input(site, &moving)?;
        let along_x = self.builder.node_value(
            site,
            "vec.make",
            vec![("x", lo), ("y", InputValue::Literal(Literal::F32(0.0)))],
            ValueType::Vec2F,
        );
        let along_x = self.builder.input_as(site, &along_x, &ValueType::Vec2F, false)?;
        Ok(self.builder.node_value(
            site,
            "logic.select",
            vec![("cond", moving), ("if_true", scaled), ("if_false", along_x)],
            ValueType::Vec2F,
        ))
    }

    /// Writes the position after the Move by lines among `changes`, and the Heading when it has a lane, and returns the
    /// new position. `site` is the site of the position's write and `current` its value at the start of the tick.
    pub(super) fn write_position(
        &mut self,
        site: &Site,
        current: &Operand,
        changes: &[Change<'a>],
    ) -> Result<Operand, String> {
        let mut total: Option<Operand> = None;
        for change in changes {
            let ChangeKind::MoveBy { arrow } = &change.kind else {
                continue;
            };
            total = Some(match total {
                None => arrow.clone(),
                Some(sum) => {
                    let line_site = self.change_site(change.line)?;
                    let a = self.builder.input_as(&line_site, &sum, &ValueType::Vec2F, false)?;
                    let b = self.builder.input_as(&line_site, arrow, &ValueType::Vec2F, false)?;
                    self.builder
                        .node_value(&line_site, "math.add", vec![("a", a), ("b", b)], ValueType::Vec2F)
                }
            });
        }
        let Some(total) = total else {
            return Ok(current.clone());
        };
        let world = self.builder.emit(site, "ctx.world", Vec::new(), Vec::new());
        let start = self.builder.input_as(site, current, &ValueType::Vec2F, false)?;
        let by = self.builder.input_as(site, &total, &ValueType::Vec2F, false)?;
        let moved = self.builder.node_value(
            site,
            "math.add",
            vec![("a", start), ("b", by.clone())],
            ValueType::Vec2F,
        );
        let moved = self.builder.input_as(site, &moved, &ValueType::Vec2F, false)?;
        let wrapped = self.builder.node_value(
            site,
            "space.wrap",
            vec![
                ("v", moved),
                (
                    "world",
                    InputValue::Link(PortRef {
                        node: world,
                        port: None,
                    }),
                ),
            ],
            ValueType::Vec2F,
        );
        let position = self
            .builder
            .properties()
            .iter()
            .find(|property| property.property_type == PropertyType::Position)
            .map(|property| property.id.as_str())
            .ok_or_else(|| "the agents have no Position to move".to_owned())?;
        let written = self.builder.input_as(site, &wrapped, &ValueType::Vec2F, false)?;
        self.builder.write(site, position, written)?;
        let heading = self.builder.properties().iter().enumerate().find(|(_, property)| {
            property.property_type == PropertyType::Heading && !self.builder.dropped.contains(&property.id)
        });
        if let Some((index, heading)) = heading {
            let heading_site = Site {
                owner: Owner::Sheet(SheetKey {
                    group: GROUP_CHANGE,
                    index: index as u32,
                }),
                path: format!("card.{}.{}", self.card.id, heading.id),
                ..site.clone()
            };
            let last = self.builder.read(self.pass, &heading.id)?;
            let last = self.builder.input_as(&heading_site, &last, &ValueType::Vec2F, false)?;
            let length = self
                .builder
                .node_value(&heading_site, "vec.length", vec![("v", by.clone())], ValueType::F32);
            let length = self.builder.input_as(&heading_site, &length, &ValueType::F32, false)?;
            let moving = self.builder.node_value(
                &heading_site,
                "cmp.gt",
                vec![("a", length), ("b", InputValue::Literal(Literal::F32(0.0)))],
                ValueType::Bool,
            );
            let moving = self.builder.condition_input(&heading_site, &moving)?;
            let new_heading = self.builder.node_value(
                &heading_site,
                "logic.select",
                vec![("cond", moving), ("if_true", by), ("if_false", last)],
                ValueType::Vec2F,
            );
            let new_heading = self
                .builder
                .input_as(&heading_site, &new_heading, &ValueType::Vec2F, false)?;
            self.builder.write(&heading_site, &heading.id, new_heading)?;
        }
        Ok(wrapped)
    }
}
