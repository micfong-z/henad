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
use super::fuse::CellLook;
use super::look::accumulator;
use super::value::{Operand, PlaceValue, number, output, port, port_of};
use crate::sheet::document::{PickRule, SheetLine};
use crate::types::ValueType;
use crate::value::{InputValue, Literal};

/// Names of the accumulators of a pick, each starting with the prefix of its look.
#[derive(Debug)]
struct PickNames {
    /// Cell picked so far.
    xy: String,
    /// Best value so far, for a pick by rank.
    best: String,
    /// Number of cells that tie for the best value so far, for a pick by rank.
    ties: String,
    /// Number of cells considered so far, for a random pick.
    seen: String,
}

impl PickNames {
    /// Returns the names of the accumulators of a pick whose look has the prefix `prefix`.
    fn new(prefix: &str) -> Self {
        Self {
            xy: format!("{prefix}_xy"),
            best: format!("{prefix}_best"),
            ties: format!("{prefix}_ties"),
            seen: format!("{prefix}_seen"),
        }
    }
}

impl<'a> CardLowering<'_, 'a> {
    /// Returns the place a Pick a cell line or phrase picks.
    pub(super) fn pick_cell(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let rule = line
            .pick
            .ok_or_else(|| "a pick needs a rule: most, least or random".to_owned())?;
        let ranked = rule != PickRule::Random;
        let look = self.enter_cells(site, line, if ranked { 3 } else { 2 })?;
        let names = PickNames::new(&look.prefix);
        let body = self.pick_body(site, line, rule, &look, &names);
        self.folds.pop();
        body?;
        let cell = self.builder.my_cell(self.pass)?;
        let here = port(cell, "xy");
        self.set_cell_init(&look, &names.xy, here);
        let xy = port_of(look.node, &names.xy, ValueType::Vec2I);
        self.add_cell_accumulator(&look, 1, accumulator(&names.xy, "vec2i", "carry"));
        let amount = if ranked {
            self.set_cell_init(&look, &names.best, InputValue::Literal(Literal::F32(0.0)));
            self.set_cell_init(&look, &names.ties, InputValue::Literal(Literal::U32(0)));
            self.add_cell_accumulator(&look, 0, accumulator(&names.best, "f32", "carry"));
            self.add_cell_accumulator(&look, 2, accumulator(&names.ties, "u32", "carry"));
            let found = self.builder.node_input(
                site,
                "cmp.gt",
                vec![
                    ("a", port(look.node, &names.ties)),
                    ("b", InputValue::Literal(Literal::U32(0))),
                ],
            );
            Some(
                self.builder
                    .select_value(site, found, port(look.node, &names.best), number(0.0), ValueType::F32),
            )
        } else {
            self.set_cell_init(&look, &names.seen, InputValue::Literal(Literal::U32(0)));
            self.add_cell_accumulator(&look, 2, accumulator(&names.seen, "u32", "carry"));
            None
        };
        Ok(Operand::Place(Box::new(PlaceValue { xy, amount, open: None })))
    }

    /// Writes the body of a pick in its fold, into the accumulators `names`.
    fn pick_body(
        &mut self,
        site: &Site,
        line: &'a SheetLine,
        rule: PickRule,
        look: &CellLook,
        names: &PickNames,
    ) -> Result<(), String> {
        let inside = site.inside(look.node);
        let mut considered = look.within.clone();
        if line.open_only == Some(true) {
            let open = self.fold_open(look)?;
            considered = self.builder.join(&inside, "logic.and", &considered, &open)?;
        }
        let considered = self.with_only(&site.at("only").inside(look.node), considered, &line.only)?;
        let when = self.builder.condition_input(&inside, &considered)?;
        let prev = |name: &str| port(look.node, &format!("prev.{name}"));
        let here = port(look.node, "xy");
        let put = |lowering: &mut Self, name: &str, value: InputValue| {
            lowering.builder.put(&inside, name, Some(value), when.clone());
        };
        if rule == PickRule::Random {
            return self.random_pick_body(&inside, look, names, &when);
        }
        let ranked_by = line
            .ranked_by
            .as_deref()
            .ok_or_else(|| "the pick needs a property to rank by".to_owned())?;
        let PickNames { xy, best, ties, .. } = names;
        let value = self.their(&inside, ranked_by)?;
        let value = self.builder.input_as(&inside, &value, &ValueType::F32)?;
        let compare = if rule == PickRule::Most { "cmp.gt" } else { "cmp.lt" };
        let ranks_higher = self.builder.node_value(
            &inside,
            compare,
            vec![("a", value.clone()), ("b", prev(best))],
            ValueType::Bool,
        );
        let first = self.builder.node_value(
            &inside,
            "cmp.eq",
            vec![("a", prev(ties)), ("b", InputValue::Literal(Literal::U32(0)))],
            ValueType::Bool,
        );
        let better = self.builder.join(&inside, "logic.or", &first, &ranks_higher)?;
        let equal = self.builder.node_value(
            &inside,
            "cmp.eq",
            vec![("a", value.clone()), ("b", prev(best))],
            ValueType::Bool,
        );
        let count = self.tie_count(&inside, prev(ties), &equal, &better)?;
        let keep_tie = self.reservoir(&inside, count.clone())?;
        let tie_kept = self.builder.join(&inside, "logic.and", &equal, &keep_tie)?;
        let accept = self.builder.join(&inside, "logic.or", &better, &tie_kept)?;
        let accept = self.builder.condition_input(&inside, &accept)?;
        let new_best = self.builder.select(&inside, accept.clone(), value, prev(best));
        let new_xy = self.builder.select(&inside, accept, here, prev(xy));
        put(self, best, new_best);
        put(self, xy, new_xy);
        put(self, ties, count);
        Ok(())
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
        let one_more = self.builder.node_input(
            inside,
            "math.add",
            vec![("a", ties.clone()), ("b", InputValue::Literal(Literal::U32(1)))],
        );
        let equal = self.builder.condition_input(inside, equal)?;
        let tied = self.builder.select(inside, equal, one_more, ties);
        let better = self.builder.condition_input(inside, better)?;
        Ok(self
            .builder
            .select(inside, better, InputValue::Literal(Literal::U32(1)), tied))
    }

    /// Writes the body of a random pick in the fold of `look` at `inside`, where `when` holds for a cell it considers:
    /// the count of cells considered and the cell kept, into the accumulators `names`.
    fn random_pick_body(
        &mut self,
        inside: &Site,
        look: &CellLook,
        names: &PickNames,
        when: &InputValue,
    ) -> Result<(), String> {
        let prev = |name: &str| port(look.node, &format!("prev.{name}"));
        let here = port(look.node, "xy");
        let put = |lowering: &mut Self, name: &str, value: InputValue| {
            lowering.builder.put(inside, name, Some(value), when.clone());
        };
        let count = self.builder.node_input(
            inside,
            "math.add",
            vec![("a", prev(&names.seen)), ("b", InputValue::Literal(Literal::U32(1)))],
        );
        let keep = self.reservoir(inside, count.clone())?;
        let keep = self.builder.condition_input(inside, &keep)?;
        let xy = self.builder.select(inside, keep, here, prev(&names.xy));
        put(self, &names.xy, xy);
        put(self, &names.seen, count);
        Ok(())
    }

    /// Returns whether reservoir sampling keeps the `count`th candidate, from a draw of bits at `site`.
    fn reservoir(&mut self, site: &Site, count: InputValue) -> Result<Operand, String> {
        let bits = self.draw(site, "rand.bits", Vec::new())?;
        Ok(self.builder.node_value(
            site,
            "rng.reservoir",
            vec![("bits", output(bits)), ("count", count)],
            ValueType::Bool,
        ))
    }
}
