//! Calculate: a formula lowered to the text of one `math.expr` node.
//!
//! Each reference of the formula becomes an input `v1`, `v2` and so on of type `f32`, in the order the formula first
//! names it, and the node's output `out` holds the formula's value. A number is written with a point, so every step
//! works in `f32`. `^` is a call of `pow`, `smaller` of `min` and `larger` of `max`. The cells hold no arrows, so
//! `length` and `direction` do not lower on a grid.

use std::collections::BTreeMap;

use super::Site;
use super::card::CardLowering;
use super::text;
use super::value::Operand;
use crate::sheet::check::formula::{self, FormulaTree};
use crate::sheet::check::formula_chip;
use crate::sheet::document::SheetLine;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, PortRef};

/// Name of the output of a formula's expression node.
const OUTPUT: &str = "out";

impl CardLowering<'_, '_> {
    /// Returns the value of a Calculate line or phrase.
    pub(super) fn calculate(&mut self, site: &Site, line: &SheetLine) -> Result<Operand, String> {
        let formula = line
            .formula
            .as_deref()
            .ok_or_else(|| "a Calculate needs a formula".to_owned())?;
        let tree = formula::parse(formula)?;
        let formula_site = site.at("formula");
        let mut inputs: Vec<(String, InputValue)> = Vec::new();
        let mut names: BTreeMap<String, String> = BTreeMap::new();
        let expression = self.formula_text(&formula_site, &tree, &mut inputs, &mut names)?;
        let declared: Vec<ConfigValue> = inputs
            .iter()
            .map(|(name, _)| {
                ConfigValue::Table(BTreeMap::from([
                    ("name".to_owned(), text(name)),
                    ("type".to_owned(), text("f32")),
                ]))
            })
            .collect();
        let node = self.builder.emit(
            site,
            "math.expr",
            vec![
                ("text", text(&format!("{OUTPUT} = {expression}"))),
                ("inputs", ConfigValue::Array(declared)),
                ("outputs", ConfigValue::Array(vec![text(OUTPUT)])),
            ],
            inputs,
        );
        Ok(Operand::Link(
            PortRef {
                node,
                port: Some(OUTPUT.to_owned()),
            },
            ValueType::F32,
        ))
    }

    /// Returns the expression text of `tree`, adding an input to `inputs` for each reference that `names` does not
    /// hold yet.
    ///
    /// `names` maps each reference to the name of its input.
    fn formula_text(
        &mut self,
        site: &Site,
        tree: &FormulaTree<'_>,
        inputs: &mut Vec<(String, InputValue)>,
        names: &mut BTreeMap<String, String>,
    ) -> Result<String, String> {
        Ok(match tree {
            FormulaTree::Number(written) => {
                let number = written
                    .parse::<f64>()
                    .map_err(|error| format!("the number {written} cannot be read: {error}"))?;
                // The narrowing is checked below: a number past the range of `f32` is refused.
                let narrowed = number as f32;
                if !narrowed.is_finite() {
                    return Err(format!("the number {written} is too large for a number of the cells"));
                }
                // Debug writes a point or an exponent, so the expression reads the number as an `f32`.
                format!("{narrowed:?}")
            }
            FormulaTree::Reference(reference) => {
                if let Some(name) = names.get(*reference) {
                    return Ok(name.clone());
                }
                let chip = formula_chip(reference, self.builder.document);
                let operand = self.chip(site, &chip)?;
                let input = self.builder.input_as(site, &operand, &ValueType::F32, false)?;
                let name = format!("v{}", inputs.len() + 1);
                inputs.push((name.clone(), input));
                names.insert((*reference).to_owned(), name.clone());
                name
            }
            FormulaTree::Negate(value) => format!("-({})", self.formula_text(site, value, inputs, names)?),
            FormulaTree::Binary(symbol, left, right) => {
                let left = self.formula_text(site, left, inputs, names)?;
                let right = self.formula_text(site, right, inputs, names)?;
                if *symbol == '^' {
                    format!("pow({left}, {right})")
                } else {
                    format!("({left} {symbol} {right})")
                }
            }
            FormulaTree::Call(function, values) => {
                let call = match *function {
                    "smaller" => "min",
                    "larger" => "max",
                    other => return Err(format!("{other} reads an arrow, and the cells hold no arrows")),
                };
                let mut written = Vec::with_capacity(values.len());
                for value in values {
                    written.push(self.formula_text(site, value, inputs, names)?);
                }
                format!("{call}({})", written.join(", "))
            }
        })
    }
}
