//! Places on the ground, and the lines of a card of agents that read and mark it.
//!
//! A *place* is a cell a line names: here, ahead, a picked cell, or the choice between them. A cell is *open* when it
//! lies on the grid and no blocked state of the ground covers it, and the cell ahead is open only when the agent has a
//! heading. A place's property is the field read at its cell. Move to takes the agent to the centre of the place of
//! the first line whose condition holds, when that place is open, and its Heading becomes the step to that cell, the
//! shorter way round a world that wraps. An agent whose place is not open stays where it is, even where a lower Move
//! to holds.
//! Leave deposits its amount at my cell, 0 where its condition fails, and Leaves on one field that cannot both apply
//! merge into one deposit whose layer and amount are chosen by their conditions. Record adds 1 to the event's counter
//! where its condition holds.

use super::card::{CardLowering, Change, ChangeKind};
use super::ids::{GROUP_CHANGE, LEAVE_INDEX_BASE};
use super::value::{Operand, PlaceValue, layer_literal, output, port, port_of, state_literal};
use super::{Builder, Site, text};
use crate::ids::NodeId;
use crate::sheet::document::{CellWhich, Edges, PropertyType, SheetLine, SheetProperty};
use crate::sheet::sentence;
use crate::types::ValueType;
use crate::value::{InputValue, Literal, PortRef};

/// A Leave line that runs: the field layer it marks, its amount and its condition.
#[derive(Debug, Clone)]
pub(super) struct LeaveMark<'a> {
    /// Id of the line.
    line: &'a str,
    /// Field the line marks.
    field: String,
    /// Ground property the line marks, the layer of the field.
    property: &'a str,
    /// Amount the line leaves.
    amount: Operand,
    /// Condition under which the line leaves its amount, the conditions of the When lines around it joined in.
    condition: Operand,
}

impl<'a> CardLowering<'_, 'a> {
    /// Returns the ground property `property` at the cell `at`, read at `site`.
    pub(super) fn ground_at(&mut self, site: &Site, property: &str, at: InputValue) -> Result<Operand, String> {
        let found = self.builder.ground_property(property)?;
        let value_type = super::world::lane_type(found);
        let (config, mut inputs) = self.builder.ground_access(property)?;
        inputs.push(("at".to_owned(), at));
        let node = self.builder.emit(site, "field.get", config, inputs);
        Ok(Operand::Link(PortRef { node, port: None }, value_type))
    }

    /// Returns whether no blocked state of the ground covers the cell `at`, written at `site`.
    pub(super) fn open_at(&mut self, site: &Site, at: &InputValue) -> Result<Operand, String> {
        let document = self.builder.document;
        let blocked = document
            .ground
            .as_ref()
            .map_or(&[][..], |ground| ground.blocked.as_slice());
        let mut open = Operand::Constant(Literal::Bool(true));
        for entry in blocked {
            let value = self.ground_at(site, &entry.property, at.clone())?;
            let category = ValueType::Category(entry.property.clone());
            let value = self.builder.input_as(site, &value, &category)?;
            let other = self.builder.node_value(
                site,
                "cmp.ne",
                vec![
                    ("a", value),
                    ("b", InputValue::Literal(state_literal(&entry.property, &entry.state))),
                ],
                ValueType::Bool,
            );
            open = self.builder.join(site, "logic.and", &open, &other)?;
        }
        Ok(open)
    }

    /// Returns my cell as a place.
    pub(super) fn my_place(&mut self) -> Result<Operand, String> {
        let cell = self.builder.my_cell(self.pass)?;
        Ok(Operand::Place(Box::new(PlaceValue {
            xy: port_of(cell, "xy", ValueType::Vec2I),
            amount: None,
            open: None,
        })))
    }

    /// Returns the cell of the place `place` and whether it lies on the grid, through a `space.cell_index` at `site`.
    fn place_cell(&mut self, site: &Site, place: &PlaceValue) -> Result<NodeId, String> {
        let xy = self.builder.input_as(site, &place.xy, &ValueType::Vec2I)?;
        Ok(self
            .builder
            .emit(site, "space.cell_index", Vec::new(), vec![("xy".to_owned(), xy)]))
    }

    /// Returns whether the place `place` is open, written at `site`.
    pub(super) fn place_open(&mut self, site: &Site, place: &PlaceValue) -> Result<Operand, String> {
        if let Some(open) = &place.open {
            return Ok(open.clone());
        }
        let cell = self.place_cell(site, place)?;
        let open = self.open_at(site, &port(cell, "cell"))?;
        let valid = port_of(cell, "valid", ValueType::Bool);
        self.builder.join(site, "logic.and", &valid, &open)
    }

    /// Returns the ground property `property` at the place `place`, written at `site`.
    pub(super) fn place_property(
        &mut self,
        site: &Site,
        place: &PlaceValue,
        property: &str,
    ) -> Result<Operand, String> {
        let cell = self.place_cell(site, place)?;
        self.ground_at(site, property, port(cell, "cell"))
    }

    /// Returns the part `part` of the place `place` that line `line` gives, written at `site`: its amount or whether
    /// it is open.
    pub(super) fn place_part(
        &mut self,
        site: &Site,
        line: &str,
        place: &PlaceValue,
        part: &str,
    ) -> Result<Operand, String> {
        match part {
            "amount" => place
                .amount
                .clone()
                .ok_or_else(|| format!("line {line} gives a place that no pick ranked, so it has no amount")),
            "open" => self.place_open(site, place),
            other => Err(format!("line {line} has no part '{other}' to read")),
        }
    }

    /// Returns the place a Look at a cell line or phrase names: my cell, the cell ahead of me, or the place in its
    /// slot.
    pub(super) fn look_at_cell(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        match line.which {
            Some(CellWhich::Here) => self.my_place(),
            Some(CellWhich::Ahead) => self.ahead(site),
            Some(CellWhich::At) => match self.slot(&site.at("place"), &line.place)? {
                place @ Operand::Place(_) => Ok(place),
                _ => Err("the cell to look at is no place".to_owned()),
            },
            None => Err("the line names no cell to look at".to_owned()),
        }
    }

    /// Returns the cell ahead of me, one step of my Heading from my cell, open only when I have a heading.
    fn ahead(&mut self, site: &Site) -> Result<Operand, String> {
        let heading = self
            .builder
            .properties()
            .iter()
            .find(|property| property.property_type == PropertyType::Heading)
            .map(|property| property.id.clone());
        let heading = match heading {
            Some(heading) if self.builder.cell_mover => heading,
            _ => return Err(sentence::AHEAD_NEEDS_CELLS.to_owned()),
        };
        let step = self.builder.read(self.pass, &heading)?;
        let step = self.builder.input_as(site, &step, &ValueType::Vec2I)?;
        let cell = self.builder.my_cell(self.pass)?;
        let here = port(cell, "xy");
        let mut xy = self.builder.node_value(
            site,
            "math.add",
            vec![("a", here), ("b", step.clone())],
            ValueType::Vec2I,
        );
        if self.builder.document.world.edges == Edges::Wrap {
            let world = self.builder.emit(site, "ctx.world", Vec::new(), Vec::new());
            let point = self.builder.input_as(site, &xy, &ValueType::Vec2F)?;
            let wrapped = self
                .builder
                .node_input(site, "space.wrap", vec![("v", point), ("world", output(world))]);
            xy = self
                .builder
                .node_value(site, "conv.to_vec2i", vec![("a", wrapped)], ValueType::Vec2I);
        }
        let mut place = PlaceValue {
            xy,
            amount: None,
            open: None,
        };
        let open = self.place_open(site, &place)?;
        let moving = self.builder.node_value(
            site,
            "cmp.ne",
            vec![("a", step), ("b", InputValue::Literal(Literal::Vec2I([0, 0])))],
            ValueType::Bool,
        );
        place.open = Some(self.builder.join(site, "logic.and", &open, &moving)?);
        Ok(Operand::Place(Box::new(place)))
    }

    /// Returns the place a Choose gives from the places `places`, each with the site of its row and the condition that
    /// picks it, as `logic.select` chains from the last place up over `otherwise`.
    ///
    /// The chosen place keeps an amount when every place has one.
    pub(super) fn choose_place(
        &mut self,
        places: Vec<(Site, PlaceValue, InputValue)>,
        otherwise: PlaceValue,
    ) -> Result<Operand, String> {
        let mut chosen = otherwise;
        for (row_site, place, condition) in places.into_iter().rev() {
            let pick = |builder: &mut Builder<'_>, a: &Operand, b: &Operand, value_type: ValueType| {
                let if_true = builder.input_as(&row_site, a, &value_type)?;
                let if_false = builder.input_as(&row_site, b, &value_type)?;
                Ok::<_, String>(builder.select_value(&row_site, condition.clone(), if_true, if_false, value_type))
            };
            let xy = pick(self.builder, &place.xy, &chosen.xy, ValueType::Vec2I)?;
            let amount = match (&place.amount, &chosen.amount) {
                (Some(a), Some(b)) => Some(pick(self.builder, a, b, ValueType::F32)?),
                _ => None,
            };
            chosen = PlaceValue { xy, amount, open: None };
        }
        Ok(Operand::Place(Box::new(chosen)))
    }

    /// Records the Move to line `id`, a change of my Position to the place in its slot.
    pub(super) fn move_to(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let (index, _) = self
            .builder
            .position()
            .ok_or_else(|| "the agents have no Position to move".to_owned())?;
        let Operand::Place(place) = self.slot(&site.at("value"), &line.value)? else {
            return Err("Move to takes a place".to_owned());
        };
        let condition = self.change_condition(site, id, line)?;
        self.add_change(index, id, ChangeKind::MoveTo { place: *place }, condition)
    }

    /// Writes my Position `position` after the Move to lines among `changes`, and my Heading when it has a lane, and
    /// returns the new position. `site` is the site of the position's write and `current` its value at the start of the
    /// tick.
    ///
    /// The first Move to whose condition holds decides, and it applies only where its place is open, so an agent never
    /// enters a blocked cell or leaves the grid.
    pub(super) fn write_moves(
        &mut self,
        site: &Site,
        position: &SheetProperty,
        current: &Operand,
        changes: &[Change<'a>],
    ) -> Result<Operand, String> {
        let cell = self.builder.my_cell(self.pass)?;
        let here = port_of(cell, "xy", ValueType::Vec2I);
        let moves: Vec<(&Change<'a>, &PlaceValue)> = changes
            .iter()
            .filter_map(|change| match &change.kind {
                ChangeKind::MoveTo { place } => Some((change, place)),
                _ => None,
            })
            .collect();
        // A row decides where its condition holds and no higher row's does.
        let mut decides = Vec::with_capacity(moves.len());
        let mut above = Operand::Constant(Literal::Bool(false));
        for (index, (change, _)) in moves.iter().enumerate() {
            let line_site = self.change_site(change.line)?;
            let below_none = match &above {
                Operand::Constant(Literal::Bool(value)) => Operand::Constant(Literal::Bool(!value)),
                above => {
                    let input = self.builder.condition_input(&line_site, above)?;
                    self.builder
                        .node_value(&line_site, "logic.not", vec![("a", input)], ValueType::Bool)
                }
            };
            decides.push(
                self.builder
                    .join(&line_site, "logic.and", &change.condition, &below_none)?,
            );
            if index + 1 < moves.len() {
                above = self.builder.join(&line_site, "logic.or", &above, &change.condition)?;
            }
        }
        let mut xy = here.clone();
        let mut moved = Operand::Constant(Literal::Bool(false));
        for ((change, place), decides) in moves.into_iter().zip(decides).rev() {
            let line_site = self.change_site(change.line)?;
            let open = self.place_open(&line_site, place)?;
            let applies = self.builder.join(&line_site, "logic.and", &decides, &open)?;
            let condition = self.builder.condition_input(&line_site, &applies)?;
            let if_true = self.builder.input_as(&line_site, &place.xy, &ValueType::Vec2I)?;
            let if_false = self.builder.input_as(&line_site, &xy, &ValueType::Vec2I)?;
            xy = self
                .builder
                .select_value(&line_site, condition, if_true, if_false, ValueType::Vec2I);
            moved = self.builder.join(&line_site, "logic.or", &applies, &moved)?;
        }
        if moved == Operand::Constant(Literal::Bool(false)) {
            return Ok(current.clone());
        }
        let corner = self.builder.input_as(site, &xy, &ValueType::Vec2F)?;
        let center = self.builder.node_value(
            site,
            "math.add",
            vec![
                ("a", corner.clone()),
                ("b", InputValue::Literal(Literal::Vec2F([0.5, 0.5]))),
            ],
            ValueType::Vec2F,
        );
        let moved_to = if moved == Operand::Constant(Literal::Bool(true)) {
            center
        } else {
            let condition = self.builder.condition_input(site, &moved)?;
            let if_true = self.builder.input_as(site, &center, &ValueType::Vec2F)?;
            let if_false = self.builder.input_as(site, current, &ValueType::Vec2F)?;
            self.builder
                .select_value(site, condition, if_true, if_false, ValueType::Vec2F)
        };
        let written = self.builder.input_as(site, &moved_to, &ValueType::Vec2F)?;
        self.builder.write(site, &position.id, written)?;
        self.write_step_heading(&here, corner)?;
        Ok(moved_to)
    }

    /// Writes my Heading, when it has a lane, as the step from my cell `here` to the cell `corner`, the shorter way
    /// round a world that wraps, keeping the last step when the two are one cell.
    fn write_step_heading(&mut self, here: &Operand, corner: InputValue) -> Result<(), String> {
        if let Some((index, heading)) = self.builder.heading_lane() {
            let heading_site = self.change_write_site(index, &heading.id);
            let world = self.builder.emit(&heading_site, "ctx.world", Vec::new(), Vec::new());
            let from = self.builder.input_as(&heading_site, here, &ValueType::Vec2F)?;
            let delta = self.builder.node_value(
                &heading_site,
                "space.delta",
                vec![("a", from), ("b", corner), ("world", output(world))],
                ValueType::Vec2F,
            );
            let step = self.builder.input_as(&heading_site, &delta, &ValueType::Vec2I)?;
            let last = self.builder.read(self.pass, &heading.id)?;
            let last = self.builder.input_as(&heading_site, &last, &ValueType::Vec2I)?;
            let stepped = self.builder.node_input(
                &heading_site,
                "cmp.ne",
                vec![("a", step.clone()), ("b", InputValue::Literal(Literal::Vec2I([0, 0])))],
            );
            let new_heading = self.builder.select(&heading_site, stepped, step, last);
            self.builder.write(&heading_site, &heading.id, new_heading)?;
        }
        Ok(())
    }

    /// Records the Leave line `id`, a mark of its amount on a property of the ground at my cell.
    pub(super) fn leave(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let property = line
            .ground_property
            .as_deref()
            .ok_or_else(|| "the line names no property of the ground".to_owned())?;
        let field = self.builder.ground_field(property)?.to_owned();
        let amount = self.slot(&site.at("amount"), &line.amount)?;
        let condition = self.change_condition(site, id, line)?;
        self.fires_when(id, &condition);
        self.leaves.push(LeaveMark {
            line: id,
            field,
            property,
            amount,
            condition,
        });
        Ok(())
    }

    /// Writes the Record line `id`: 1 added to its event's counter where its condition holds.
    pub(super) fn record(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let event = line
            .event
            .as_deref()
            .ok_or_else(|| "the line names no event".to_owned())?;
        let condition = self.change_condition(site, id, line)?;
        self.fires_when(id, &condition);
        let amount = match condition {
            Operand::Constant(Literal::Bool(true)) => InputValue::Literal(Literal::U32(1)),
            condition => {
                let condition = self.builder.condition_input(site, &condition)?;
                self.builder.node_input(site, "conv.to_u32", vec![("a", condition)])
            }
        };
        self.builder.emit(
            site,
            "counter.add",
            vec![("counter", text(event))],
            vec![("amount".to_owned(), amount)],
        );
        Ok(())
    }

    /// Writes the deposits of the card's Leave lines: one per line, or one per field for Leaves on that field that
    /// cannot both apply.
    pub(super) fn write_leaves(&mut self) -> Result<(), String> {
        let leaves = std::mem::take(&mut self.leaves);
        if leaves.is_empty() {
            return Ok(());
        }
        let cell = self.builder.my_cell(self.pass)?;
        let at = port(cell, "cell");
        let mut fields: Vec<&str> = Vec::new();
        for leave in &leaves {
            if !fields.contains(&leave.field.as_str()) {
                fields.push(&leave.field);
            }
        }
        for (field_index, field) in fields.into_iter().enumerate() {
            let marks: Vec<&LeaveMark<'a>> = leaves.iter().filter(|leave| leave.field == field).collect();
            let exclusive = marks.len() > 1
                && marks.iter().enumerate().all(|(upper, first)| {
                    marks
                        .iter()
                        .skip(upper + 1)
                        .all(|second| !self.relations.can_both_apply(first.line, second.line))
                });
            if exclusive {
                let site = Site::sheet(
                    self.pass,
                    GROUP_CHANGE,
                    LEAVE_INDEX_BASE + field_index as u32,
                    format!("card.{}.{field}", self.card.id),
                );
                let mut chosen: Option<(InputValue, InputValue)> = None;
                for mark in marks.iter().rev() {
                    let line_site = self.change_site(mark.line)?;
                    let layer = layer_literal(field, mark.property);
                    let amount = self.builder.input_as(&line_site, &mark.amount, &ValueType::F32)?;
                    chosen = Some(match chosen {
                        None => (layer, self.builder.amount_where(&line_site, &mark.condition, amount)?),
                        Some((other_layer, other_amount)) => {
                            let condition = self.builder.condition_input(&line_site, &mark.condition)?;
                            let layer = self.builder.select(&line_site, condition.clone(), layer, other_layer);
                            let amount = self.builder.select(&line_site, condition, amount, other_amount);
                            (layer, amount)
                        }
                    });
                }
                if let Some((layer, amount)) = chosen {
                    self.deposit(&site, field, layer, at.clone(), amount);
                }
            } else {
                for mark in marks {
                    let line_site = self.change_site(mark.line)?;
                    let amount = self.builder.input_as(&line_site, &mark.amount, &ValueType::F32)?;
                    let amount = self.builder.amount_where(&line_site, &mark.condition, amount)?;
                    self.deposit(
                        &line_site,
                        field,
                        layer_literal(field, mark.property),
                        at.clone(),
                        amount,
                    );
                }
            }
        }
        Ok(())
    }

    /// Writes a `field.deposit` of `amount` into layer `layer` of field `field` at the cell `at`, at `site`.
    fn deposit(&mut self, site: &Site, field: &str, layer: InputValue, at: InputValue, amount: InputValue) {
        self.builder.emit(
            site,
            "field.deposit",
            vec![("field", text(field))],
            vec![
                ("layer".to_owned(), layer),
                ("at".to_owned(), at),
                ("value".to_owned(), amount),
            ],
        );
    }
}
