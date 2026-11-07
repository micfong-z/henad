//! Pick a cell: the cell among the 8 or the 4 around me with the most or least of a ground property, or one at random.
//!
//! A pick is three loop variables of a fold over the cells around, as [`super::fuse`] describes: the best value so far,
//! the cell that holds it and the number of cells that tie for it. A cell the pick considers, one that is open when the
//! line asks for open cells and passes its `only` test, replaces the best when it ranks higher or is the first, and a
//! cell that ties replaces it with a chance of one over the ties so far, through `rng.reservoir` over `rand.bits`, so
//! every tied cell is equally likely. A random pick counts the cells it considers and keeps each with a chance of one
//! over that count. A pick of no cell gives my own cell, its amount 0. The pick's place holds the cell and, for a pick
//! by rank, the amount: the best value.

use super::Site;
use super::card::CardLowering;
use super::fuse::{CellLook, inner};
use super::look::accumulator;
use super::text;
use super::value::{Operand, PlaceValue};
use crate::sheet::document::{PickRule, SheetLine};
use crate::types::ValueType;
use crate::value::{InputValue, Literal, PortRef};

impl<'a> CardLowering<'_, 'a> {
    /// Returns the place a Pick a cell line or phrase picks.
    pub(super) fn pick_cell(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let rule = line
            .pick
            .ok_or_else(|| "a pick needs a rule: most, least or random".to_owned())?;
        let ranked = rule != PickRule::Random;
        let look = self.enter_cells(site, line, if ranked { 3 } else { 2 })?;
        let body = self.pick_body(site, line, rule, &look);
        self.folds.pop();
        let names = body?;
        let cell = self.builder.my_cell(self.pass)?;
        let here = InputValue::Link(PortRef {
            node: cell,
            port: Some("xy".to_owned()),
        });
        let xy_name = format!("{}_xy", look.prefix);
        self.set_cell_init(&look, &xy_name, here);
        let xy = Operand::Link(
            PortRef {
                node: look.node,
                port: Some(xy_name.clone()),
            },
            ValueType::Vec2I,
        );
        self.add_cell_accumulator(&look, 1, accumulator(&xy_name, "vec2i", "carry"));
        let amount = if let Some((best, ties)) = names {
            self.set_cell_init(&look, &best, InputValue::Literal(Literal::F32(0.0)));
            self.set_cell_init(&look, &ties, InputValue::Literal(Literal::U32(0)));
            self.add_cell_accumulator(&look, 0, accumulator(&best, "f32", "carry"));
            self.add_cell_accumulator(&look, 2, accumulator(&ties, "u32", "carry"));
            let found = self.builder.node_value(
                site,
                "cmp.gt",
                vec![
                    ("a", inner(look.node, &ties)),
                    ("b", InputValue::Literal(Literal::U32(0))),
                ],
                ValueType::Bool,
            );
            let found = self.builder.condition_input(site, &found)?;
            Some(self.builder.node_value(
                site,
                "logic.select",
                vec![
                    ("cond", found),
                    ("if_true", inner(look.node, &best)),
                    ("if_false", InputValue::Literal(Literal::F32(0.0))),
                ],
                ValueType::F32,
            ))
        } else {
            let seen = format!("{}_seen", look.prefix);
            self.set_cell_init(&look, &seen, InputValue::Literal(Literal::U32(0)));
            self.add_cell_accumulator(&look, 2, accumulator(&seen, "u32", "carry"));
            None
        };
        Ok(Operand::Place(Box::new(PlaceValue { xy, amount, open: None })))
    }

    /// Writes the body of a pick in its fold, and returns the names of its best value and its ties for a pick by rank.
    fn pick_body(
        &mut self,
        site: &Site,
        line: &'a SheetLine,
        rule: PickRule,
        look: &CellLook,
    ) -> Result<Option<(String, String)>, String> {
        let inside = site.inside(look.node);
        let mut considered = look.within.clone();
        if line.open_only == Some(true) {
            let open = self.fold_open(look)?;
            considered = self.builder.join(&inside, "logic.and", &considered, &open)?;
        }
        let considered = self.with_only(&site.at("only").inside(look.node), considered, &line.only)?;
        let when = self.builder.condition_input(&inside, &considered)?;
        let xy_name = format!("{}_xy", look.prefix);
        let prev = |name: &str| inner(look.node, &format!("prev.{name}"));
        let here = inner(look.node, "xy");
        let put = |lowering: &mut Self, name: &str, value: InputValue| {
            lowering.builder.emit(
                &inside,
                "region.put",
                vec![("acc", text(name))],
                vec![("value".to_owned(), value), ("when".to_owned(), when.clone())],
            );
        };
        if rule == PickRule::Random {
            self.random_pick_body(&inside, look, &when)?;
            return Ok(None);
        }
        let ranked_by = line
            .ranked_by
            .as_deref()
            .ok_or_else(|| "the pick ranks by no property".to_owned())?;
        let best = format!("{}_best", look.prefix);
        let ties = format!("{}_ties", look.prefix);
        let value = self.their(&inside, ranked_by)?;
        let value = self.builder.input_as(&inside, &value, &ValueType::F32, false)?;
        let compare = if rule == PickRule::Most { "cmp.gt" } else { "cmp.lt" };
        let ranks_higher = self.builder.node_value(
            &inside,
            compare,
            vec![("a", value.clone()), ("b", prev(&best))],
            ValueType::Bool,
        );
        let first = self.builder.node_value(
            &inside,
            "cmp.eq",
            vec![("a", prev(&ties)), ("b", InputValue::Literal(Literal::U32(0)))],
            ValueType::Bool,
        );
        let better = self.builder.join(&inside, "logic.or", &first, &ranks_higher)?;
        let equal = self.builder.node_value(
            &inside,
            "cmp.eq",
            vec![("a", value.clone()), ("b", prev(&best))],
            ValueType::Bool,
        );
        let count = self.tie_count(&inside, prev(&ties), &equal, &better)?;
        let keep_tie = self.reservoir(&inside, count.clone())?;
        let keep_tie = Operand::Link(
            match keep_tie {
                InputValue::Link(link) => link,
                _ => return Err("the reservoir draw has no output".to_owned()),
            },
            ValueType::Bool,
        );
        let tie_kept = self.builder.join(&inside, "logic.and", &equal, &keep_tie)?;
        let accept = self.builder.join(&inside, "logic.or", &better, &tie_kept)?;
        let accept = self.builder.condition_input(&inside, &accept)?;
        let new_best = self.builder.node_value(
            &inside,
            "logic.select",
            vec![("cond", accept.clone()), ("if_true", value), ("if_false", prev(&best))],
            ValueType::F32,
        );
        let new_best = self.builder.input_as(&inside, &new_best, &ValueType::F32, false)?;
        let xy = self.builder.node_value(
            &inside,
            "logic.select",
            vec![("cond", accept), ("if_true", here), ("if_false", prev(&xy_name))],
            ValueType::Vec2I,
        );
        let xy = self.builder.input_as(&inside, &xy, &ValueType::Vec2I, false)?;
        put(self, &best, new_best);
        put(self, &xy_name, xy);
        put(self, &ties, count);
        Ok(Some((best, ties)))
    }

    /// Returns the number of cells that tie for the best so far after a cell, written at `inside`: 1 when the cell is
    /// `better`, one more than `ties` when it is `equal` to the best, and `ties` otherwise.
    fn tie_count(
        &mut self,
        inside: &Site,
        ties: InputValue,
        equal: &Operand,
        better: &Operand,
    ) -> Result<InputValue, String> {
        let one_more = self.builder.node_value(
            inside,
            "math.add",
            vec![("a", ties.clone()), ("b", InputValue::Literal(Literal::U32(1)))],
            ValueType::U32,
        );
        let one_more = self.builder.input_as(inside, &one_more, &ValueType::U32, false)?;
        let equal = self.builder.condition_input(inside, equal)?;
        let tied = self.builder.node_value(
            inside,
            "logic.select",
            vec![("cond", equal), ("if_true", one_more), ("if_false", ties)],
            ValueType::U32,
        );
        let tied = self.builder.input_as(inside, &tied, &ValueType::U32, false)?;
        let better = self.builder.condition_input(inside, better)?;
        let count = self.builder.node_value(
            inside,
            "logic.select",
            vec![
                ("cond", better),
                ("if_true", InputValue::Literal(Literal::U32(1))),
                ("if_false", tied),
            ],
            ValueType::U32,
        );
        self.builder.input_as(inside, &count, &ValueType::U32, false)
    }

    /// Writes the body of a random pick in the fold of `look` at `inside`, where `when` holds for a cell it considers:
    /// the count of cells considered and the cell kept.
    fn random_pick_body(&mut self, inside: &Site, look: &CellLook, when: &InputValue) -> Result<(), String> {
        let xy_name = format!("{}_xy", look.prefix);
        let prev = |name: &str| inner(look.node, &format!("prev.{name}"));
        let here = inner(look.node, "xy");
        let put = |lowering: &mut Self, name: &str, value: InputValue| {
            lowering.builder.emit(
                inside,
                "region.put",
                vec![("acc", text(name))],
                vec![("value".to_owned(), value), ("when".to_owned(), when.clone())],
            );
        };
        let seen = format!("{}_seen", look.prefix);
        let count = self.builder.node_value(
            inside,
            "math.add",
            vec![("a", prev(&seen)), ("b", InputValue::Literal(Literal::U32(1)))],
            ValueType::U32,
        );
        let count = self.builder.input_as(inside, &count, &ValueType::U32, false)?;
        let keep = self.reservoir(inside, count.clone())?;
        let xy = self.builder.node_value(
            inside,
            "logic.select",
            vec![("cond", keep), ("if_true", here), ("if_false", prev(&xy_name))],
            ValueType::Vec2I,
        );
        let xy = self.builder.input_as(inside, &xy, &ValueType::Vec2I, false)?;
        put(self, &xy_name, xy);
        put(self, &seen, count);
        Ok(())
    }

    /// Returns whether reservoir sampling keeps the `count`th candidate, from a draw of bits at `site`.
    fn reservoir(&mut self, site: &Site, count: InputValue) -> Result<InputValue, String> {
        let bits = self.draw(site, "rand.bits", Vec::new())?;
        let keep = self.builder.node_value(
            site,
            "rng.reservoir",
            vec![
                ("bits", InputValue::Link(PortRef { node: bits, port: None })),
                ("count", count),
            ],
            ValueType::Bool,
        );
        self.builder.condition_input(site, &keep)
    }
}
