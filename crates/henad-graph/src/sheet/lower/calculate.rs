//! Calculate: a formula lowered to the text of one `math.expr` node.
//!
//! Each reference of the formula becomes an input `v1`, `v2` and so on, in the order the formula first names it, of
//! type `vec2f` for an arrow and `f32` for anything else, and the node's output `out` holds the formula's value. A
//! number is written with a point, so every step works in `f32`. `^` is a call of `pow`, `smaller` of `min`, `larger`
//! of `max` and `length` of `length`. `direction` is an arrow divided by its length, an arrow of length 0 staying 0.
//! The cells hold no arrows, so `length` and `direction` do not lower on a grid.
//!
//! A division by 0 gives 0. A divisor that is not a number other than 0 is bound to an output `d1`, `d2` and so on,
//! written before `out`, and the quotient is `if(d == 0.0, 0.0, left / if(d == 0.0, 1.0, d))`, per component for an
//! arrow divided by an arrow. The expression never divides by 0, whose result a shader leaves indeterminate.

use std::collections::BTreeMap;

use super::card::CardLowering;
use super::value::{Operand, port_of};
use super::{Builder, Site, text};
use crate::ids::NodeId;
use crate::sheet::check::formula::{self, FormulaTree};
use crate::sheet::check::formula_chip;
use crate::sheet::document::{SheetChip, SheetLine};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue};

/// Name of the output of a formula's expression node.
const OUTPUT: &str = "out";

/// Source a formula's references read: the lowering of a card's lines, or of a pass that reads my properties alone.
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
            _ => Err("a measure's formula can only read my properties and the parameters".to_owned()),
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
    let mut bindings = Vec::new();
    let mut text = FormulaText {
        inputs: &mut inputs,
        names: &mut names,
        bindings: &mut bindings,
    };
    let (expression, value_type) = formula_text(scope, &formula_site, &tree, &mut text)?;
    let node = expression_node(scope.builder(), site, &bindings, &expression, inputs);
    Ok(port_of(node, OUTPUT, value_type))
}

/// Writes the `math.expr` node `out = <expression>` over `inputs` at `site`, each input a name, its value and its
/// type, and returns its id.
///
/// `bindings` are assignments `name = expression` written before `out`, each name an output of the node too.
pub(super) fn expression_node(
    builder: &mut Builder<'_>,
    site: &Site,
    bindings: &[(String, String)],
    expression: &str,
    inputs: Vec<(String, InputValue, ValueType)>,
) -> NodeId {
    let declared: Vec<ConfigValue> = inputs
        .iter()
        .map(|(name, _, value_type)| {
            ConfigValue::Table(BTreeMap::from([
                ("name".to_owned(), text(name)),
                ("type".to_owned(), text(&value_type.to_string())),
            ]))
        })
        .collect();
    let mut assignments: Vec<String> = bindings
        .iter()
        .map(|(name, bound)| format!("{name} = {bound}"))
        .collect();
    assignments.push(format!("{OUTPUT} = {expression}"));
    let mut outputs: Vec<ConfigValue> = bindings.iter().map(|(name, _)| text(name)).collect();
    outputs.push(text(OUTPUT));
    builder.emit(
        site,
        "math.expr",
        vec![
            ("text", text(&assignments.join("; "))),
            ("inputs", ConfigValue::Array(declared)),
            ("outputs", ConfigValue::Array(outputs)),
        ],
        inputs.into_iter().map(|(name, input, _)| (name, input)).collect(),
    )
}

/// Inputs, references and bindings of a formula's expression, gathered while its text is written.
struct FormulaText<'t> {
    /// Each input: its name, its value and its type.
    inputs: &'t mut Vec<(String, InputValue, ValueType)>,
    /// Name and type of the input of each reference, by the reference.
    names: &'t mut BTreeMap<String, (String, ValueType)>,
    /// Assignments written before `out`, each its name and its expression.
    bindings: &'t mut Vec<(String, String)>,
}

impl FormulaText<'_> {
    /// Binds `expression` to a new name written before `out`, `prefix` and the number of bindings, and returns the
    /// name.
    fn bind(&mut self, prefix: char, expression: String) -> String {
        let name = format!("{prefix}{}", self.bindings.len() + 1);
        self.bindings.push((name.clone(), expression));
        name
    }
}

/// Returns whether `tree` is a number other than 0, or its negation.
fn is_nonzero_number(tree: &FormulaTree<'_>) -> bool {
    match tree {
        FormulaTree::Number(written) => written.parse::<f64>().is_ok_and(|number| number != 0.0),
        FormulaTree::Negate(value) => is_nonzero_number(value),
        _ => false,
    }
}

/// Returns the expression text of `left / right`, 0 where the divisor is 0, binding the divisor in `text`.
///
/// An arrow divided by an arrow is divided per component. A number divided by an arrow is left as it is, since the
/// expression refuses it.
fn quotient(
    text: &mut FormulaText<'_>,
    left: String,
    left_type: &ValueType,
    right: String,
    right_type: &ValueType,
) -> String {
    let guarded = |numerator: &str, divisor: &str| {
        format!("if({divisor} == 0.0, 0.0, {numerator} / if({divisor} == 0.0, 1.0, {divisor}))")
    };
    match (left_type, right_type) {
        (_, ValueType::F32) => {
            let divisor = text.bind('d', right);
            let zero = if *left_type == ValueType::Vec2F {
                "vec(0.0, 0.0)"
            } else {
                "0.0"
            };
            format!("if({divisor} == 0.0, {zero}, {left} / if({divisor} == 0.0, 1.0, {divisor}))")
        }
        (ValueType::Vec2F, ValueType::Vec2F) => {
            let numerator = text.bind('n', left);
            let divisor = text.bind('d', right);
            format!(
                "vec({}, {})",
                guarded(&format!("{numerator}.x"), &format!("{divisor}.x")),
                guarded(&format!("{numerator}.y"), &format!("{divisor}.y"))
            )
        }
        _ => format!("({left} / {right})"),
    }
}

/// Returns the expression text of `tree` and its type, adding an input to `text` for each reference it does not hold
/// yet, and a binding for each divisor that can be 0.
///
/// `names` maps each reference to the name and type of its input.
fn formula_text(
    scope: &mut dyn Scope<'_>,
    site: &Site,
    tree: &FormulaTree<'_>,
    text: &mut FormulaText<'_>,
) -> Result<(String, ValueType), String> {
    Ok(match tree {
        FormulaTree::Number(written) => {
            let number = written
                .parse::<f64>()
                .map_err(|error| format!("the number {written} cannot be read: {error}"))?;
            // The narrowing is checked below: a number past the range of `f32` is refused.
            let narrowed = number as f32;
            if !narrowed.is_finite() {
                return Err(format!("the number {written} is too large for the sheet"));
            }
            // Debug writes a point or an exponent, so the expression reads the number as an `f32`.
            (format!("{narrowed:?}"), ValueType::F32)
        }
        FormulaTree::Reference(reference) => {
            if let Some((name, value_type)) = text.names.get(*reference) {
                return Ok((name.clone(), value_type.clone()));
            }
            let chip = formula_chip(reference, scope.builder().document);
            let operand = scope.chip(site, &chip)?;
            let value_type = if operand.value_type() == Some(ValueType::Vec2F) {
                ValueType::Vec2F
            } else {
                ValueType::F32
            };
            let input = scope.builder().input_as(site, &operand, &value_type)?;
            let name = format!("v{}", text.inputs.len() + 1);
            text.inputs.push((name.clone(), input, value_type.clone()));
            text.names
                .insert((*reference).to_owned(), (name.clone(), value_type.clone()));
            (name, value_type)
        }
        FormulaTree::Negate(value) => {
            let (written, value_type) = formula_text(scope, site, value, text)?;
            (format!("-({written})"), value_type)
        }
        FormulaTree::Binary(symbol, left_tree, right_tree) => {
            let (left, left_type) = formula_text(scope, site, left_tree, text)?;
            let (right, right_type) = formula_text(scope, site, right_tree, text)?;
            let value_type = if left_type == ValueType::Vec2F || right_type == ValueType::Vec2F {
                ValueType::Vec2F
            } else {
                ValueType::F32
            };
            if *symbol == '^' {
                (format!("pow({left}, {right})"), value_type)
            } else if *symbol == '/' && !is_nonzero_number(right_tree) {
                (quotient(text, left, &left_type, right, &right_type), value_type)
            } else {
                (format!("({left} {symbol} {right})"), value_type)
            }
        }
        FormulaTree::Call(function, values) => {
            let mut written = Vec::with_capacity(values.len());
            let mut arrow = false;
            for value in values {
                let (value_text, value_type) = formula_text(scope, site, value, text)?;
                arrow |= value_type == ValueType::Vec2F;
                written.push(value_text);
            }
            let reads_arrow = |what: &str| {
                if arrow {
                    Ok(())
                } else {
                    Err(format!("{what} takes an arrow, got a number"))
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
                other => return Err(format!("unknown function '{other}' in the formula")),
            }
        }
    })
}
