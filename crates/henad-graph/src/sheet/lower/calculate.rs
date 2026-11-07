//! Calculate: a formula lowered to the text of one `math.expr` node.
//!
//! Each reference of the formula becomes an input `v1`, `v2` and so on, in the order the formula first names it, of
//! type `vec2f` for an arrow and `f32` for anything else, and the node's output `out` holds the formula's value. A
//! number is written with a point, so every step works in `f32`. `^` is a call of `pow`, `smaller` of `min`, `larger`
//! of `max` and `length` of `length`. `direction` is an arrow divided by its length, an arrow of length 0 staying 0.
//! The cells hold no arrows, so `length` and `direction` do not lower on a grid.

use std::collections::BTreeMap;

use super::card::CardLowering;
use super::value::Operand;
use super::{Builder, Site, text};
use crate::sheet::check::formula::{self, FormulaTree};
use crate::sheet::check::formula_chip;
use crate::sheet::document::{SheetChip, SheetLine};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, PortRef};

/// Name of the output of a formula's expression node.
const OUTPUT: &str = "out";

/// What a formula's references read: the lowering of a card's lines, or of a pass that reads my properties alone.
pub(super) trait Scope<'a> {
    /// The lowering.
    fn builder(&mut self) -> &mut Builder<'a>;

    /// Returns the operand of `chip`, written at `site`.
    fn chip(&mut self, site: &Site, chip: &SheetChip) -> Result<Operand, String>;
}

/// A pass whose formulas read my properties and the parameters alone, such as the measures.
pub(super) struct PassScope<'b, 'a> {
    /// The lowering.
    pub(super) builder: &'b mut Builder<'a>,
    /// Index of the pass.
    pub(super) pass: usize,
}

impl<'a> Scope<'a> for PassScope<'_, 'a> {
    fn builder(&mut self) -> &mut Builder<'a> {
        self.builder
    }

    fn chip(&mut self, _site: &Site, chip: &SheetChip) -> Result<Operand, String> {
        match chip {
            SheetChip::My(property) => self.builder.read(self.pass, property),
            SheetChip::Param(id) => self.builder.param(id),
            _ => Err("a measure's formula reads my properties and the parameters alone".to_owned()),
        }
    }
}

impl<'a> Scope<'a> for CardLowering<'_, 'a> {
    fn builder(&mut self) -> &mut Builder<'a> {
        self.builder
    }

    fn chip(&mut self, site: &Site, chip: &SheetChip) -> Result<Operand, String> {
        CardLowering::chip(self, site, chip)
    }
}

impl CardLowering<'_, '_> {
    /// Returns the value of a Calculate line or phrase.
    pub(super) fn calculate(&mut self, site: &Site, line: &SheetLine) -> Result<Operand, String> {
        calculate(self, site, line)
    }
}

/// Returns the value of the Calculate line or phrase `line`, its references read through `scope`.
pub(super) fn calculate(scope: &mut dyn Scope<'_>, site: &Site, line: &SheetLine) -> Result<Operand, String> {
    let formula = line
        .formula
        .as_deref()
        .ok_or_else(|| "a Calculate needs a formula".to_owned())?;
    let tree = formula::parse(formula)?;
    let formula_site = site.at("formula");
    let mut inputs: Vec<(String, InputValue, ValueType)> = Vec::new();
    let mut names: BTreeMap<String, (String, ValueType)> = BTreeMap::new();
    let (expression, value_type) = formula_text(scope, &formula_site, &tree, &mut inputs, &mut names)?;
    let declared: Vec<ConfigValue> = inputs
        .iter()
        .map(|(name, _, value_type)| {
            ConfigValue::Table(BTreeMap::from([
                ("name".to_owned(), text(name)),
                ("type".to_owned(), text(&value_type.to_string())),
            ]))
        })
        .collect();
    let node = scope.builder().emit(
        site,
        "math.expr",
        vec![
            ("text", text(&format!("{OUTPUT} = {expression}"))),
            ("inputs", ConfigValue::Array(declared)),
            ("outputs", ConfigValue::Array(vec![text(OUTPUT)])),
        ],
        inputs.into_iter().map(|(name, input, _)| (name, input)).collect(),
    );
    Ok(Operand::Link(
        PortRef {
            node,
            port: Some(OUTPUT.to_owned()),
        },
        value_type,
    ))
}

/// Returns the expression text of `tree` and its type, adding an input to `inputs` for each reference that `names`
/// does not hold yet.
///
/// `names` maps each reference to the name and type of its input.
fn formula_text(
    scope: &mut dyn Scope<'_>,
    site: &Site,
    tree: &FormulaTree<'_>,
    inputs: &mut Vec<(String, InputValue, ValueType)>,
    names: &mut BTreeMap<String, (String, ValueType)>,
) -> Result<(String, ValueType), String> {
    Ok(match tree {
        FormulaTree::Number(written) => {
            let number = written
                .parse::<f64>()
                .map_err(|error| format!("the number {written} cannot be read: {error}"))?;
            // The narrowing is checked below: a number past the range of `f32` is refused.
            let narrowed = number as f32;
            if !narrowed.is_finite() {
                return Err(format!("the number {written} is too large for a number of the sheet"));
            }
            // Debug writes a point or an exponent, so the expression reads the number as an `f32`.
            (format!("{narrowed:?}"), ValueType::F32)
        }
        FormulaTree::Reference(reference) => {
            if let Some((name, value_type)) = names.get(*reference) {
                return Ok((name.clone(), value_type.clone()));
            }
            let chip = formula_chip(reference, scope.builder().document);
            let operand = scope.chip(site, &chip)?;
            let value_type = if operand.value_type() == Some(ValueType::Vec2F) {
                ValueType::Vec2F
            } else {
                ValueType::F32
            };
            let input = scope.builder().input_as(site, &operand, &value_type, false)?;
            let name = format!("v{}", inputs.len() + 1);
            inputs.push((name.clone(), input, value_type.clone()));
            names.insert((*reference).to_owned(), (name.clone(), value_type.clone()));
            (name, value_type)
        }
        FormulaTree::Negate(value) => {
            let (text, value_type) = formula_text(scope, site, value, inputs, names)?;
            (format!("-({text})"), value_type)
        }
        FormulaTree::Binary(symbol, left, right) => {
            let (left, left_type) = formula_text(scope, site, left, inputs, names)?;
            let (right, right_type) = formula_text(scope, site, right, inputs, names)?;
            let value_type = if left_type == ValueType::Vec2F || right_type == ValueType::Vec2F {
                ValueType::Vec2F
            } else {
                ValueType::F32
            };
            if *symbol == '^' {
                (format!("pow({left}, {right})"), value_type)
            } else {
                (format!("({left} {symbol} {right})"), value_type)
            }
        }
        FormulaTree::Call(function, values) => {
            let mut written = Vec::with_capacity(values.len());
            let mut arrow = false;
            for value in values {
                let (text, value_type) = formula_text(scope, site, value, inputs, names)?;
                arrow |= value_type == ValueType::Vec2F;
                written.push(text);
            }
            let reads_arrow = |what: &str| {
                if arrow {
                    Ok(())
                } else {
                    Err(format!("{what} reads an arrow, and its value is a number"))
                }
            };
            match *function {
                "smaller" => (format!("min({})", written.join(", ")), ValueType::F32),
                "larger" => (format!("max({})", written.join(", ")), ValueType::F32),
                "length" => {
                    reads_arrow("length")?;
                    (format!("length({})", written.join(", ")), ValueType::F32)
                }
                "direction" => {
                    reads_arrow("direction")?;
                    let value = written.join(", ");
                    (format!("({value} / max(length({value}), 0.000001))"), ValueType::Vec2F)
                }
                other => return Err(format!("the formula calls '{other}', which no formula knows")),
            }
        }
    })
}
