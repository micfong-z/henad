//! Values of a lowering: what a slot lowers to, and the conversions between the types the sheet's numbers take.
//!
//! A sheet has one type of number, and the graph has several. A count is `u32`, a Number property `f32`, a Whole number
//! property `i32`, a Number parameter `f32` and a Whole number parameter `u32`. A number the author typed takes the type
//! of the value it meets when that type holds it exactly, and otherwise both sides compare as `f32`.

use super::{Builder, Site, text};
use crate::ids::NodeId;
use crate::sheet::document::SheetLiteral;
use crate::types::ValueType;
use crate::value::{InputValue, Literal, MemberName, PortRef};

/// Result a value of a sheet lowers to.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Operand {
    /// An output of a node, of a known type.
    Link(PortRef, ValueType),
    /// A constant of a known type.
    Constant(Literal),
    /// A number the author typed, typed by the value it meets.
    Number(f64),
    /// A parameter of a known type.
    Param(String, ValueType),
    /// A state, whose property the value it meets fixes.
    State(String),
    /// A place: a cell of the ground under agents.
    Place(Box<PlaceValue>),
}

/// A cell of the ground under agents, as a line gives it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlaceValue {
    /// Column and row of the cell, a `vec2i`.
    pub xy: Operand,
    /// Amount the pick that found the cell ranked it by, `None` for a place no pick ranked.
    pub amount: Option<Operand>,
    /// Whether the cell is open, `None` when the cell's own test decides it.
    pub open: Option<Operand>,
}

impl Operand {
    /// Returns whether the operand is the same for every element: a typed number, a constant or a parameter.
    pub(super) fn is_uniform(&self) -> bool {
        matches!(self, Self::Number(_) | Self::Constant(_) | Self::Param(..))
    }

    /// Returns the type of the operand, `None` for a typed number or a state, whose type comes from what they meet.
    ///
    /// A place has the type of its column and row, `vec2i`.
    pub(super) fn value_type(&self) -> Option<ValueType> {
        match self {
            Self::Link(_, value_type) | Self::Param(_, value_type) => Some(value_type.clone()),
            Self::Constant(literal) => Some(literal.value_type()),
            Self::Place(_) => Some(ValueType::Vec2I),
            Self::Number(_) | Self::State(_) => None,
        }
    }
}

/// Returns the operand of the literal `literal`: a constant yes or no, or a typed number, `None` for a literal that
/// holds neither.
pub(super) fn literal_operand(literal: &SheetLiteral) -> Option<Operand> {
    match literal {
        SheetLiteral::Bool(value) => Some(Operand::Constant(Literal::Bool(*value))),
        literal => literal.number().map(Operand::Number),
    }
}

/// Returns whether `value_type` is a number type a sheet's numbers lower to.
fn is_number(value_type: &ValueType) -> bool {
    matches!(value_type, ValueType::U32 | ValueType::I32 | ValueType::F32)
}

/// Returns whether `number` is held exactly by `value_type`.
fn holds(value_type: &ValueType, number: f64) -> bool {
    match value_type {
        ValueType::U32 => number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&number),
        ValueType::I32 => number.fract() == 0.0 && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&number),
        ValueType::F32 => number.is_finite(),
        _ => false,
    }
}

/// Returns the type two operands that meet in a comparison or an arithmetic step take.
///
/// # Errors
///
/// Returns the message for two operands of types that cannot meet.
pub(super) fn common_type(first: &Operand, second: &Operand) -> Result<ValueType, String> {
    match (first.value_type(), second.value_type()) {
        (Some(ValueType::Category(property)), _) | (_, Some(ValueType::Category(property))) => {
            Ok(ValueType::Category(property))
        }
        (Some(one), Some(other)) if one == other => Ok(one),
        (Some(one), Some(other)) if is_number(&one) && is_number(&other) => Ok(ValueType::F32),
        (Some(known), None) | (None, Some(known)) => {
            let number = match (first, second) {
                (Operand::Number(number), _) | (_, Operand::Number(number)) => Some(*number),
                _ => None,
            };
            match number {
                Some(number) if is_number(&known) && !holds(&known, number) => Ok(ValueType::F32),
                _ => Ok(known),
            }
        }
        (None, None) => Ok(ValueType::F32),
        (Some(one), Some(other)) => Err(format!("a value of type {one} meets a value of type {other}")),
    }
}

/// Returns the type the values `operands` share, `None` when no operand has a type of its own.
///
/// Values of one type share it. Numbers of several types share `f32`, and so do numbers of one type beside a typed
/// number that type cannot hold exactly.
///
/// # Errors
///
/// Returns the message for values of kinds that cannot meet, such as a state beside a number.
pub(super) fn shared_type(operands: &[&Operand]) -> Result<Option<ValueType>, String> {
    let mut shared: Option<ValueType> = None;
    for operand in operands {
        let Some(found) = operand.value_type() else {
            continue;
        };
        shared = Some(match shared {
            None => found,
            Some(known) if known == found => known,
            Some(known) if is_number(&known) && is_number(&found) => ValueType::F32,
            Some(known) => return Err(format!("a value of type {known} meets a value of type {found}")),
        });
    }
    let Some(known) = shared else {
        return Ok(None);
    };
    let typed_numbers: Vec<f64> = operands
        .iter()
        .filter_map(|operand| match operand {
            Operand::Number(number) => Some(*number),
            _ => None,
        })
        .collect();
    if typed_numbers.is_empty() {
        return Ok(Some(known));
    }
    if !is_number(&known) {
        return Err(format!("a number meets a value of type {known}"));
    }
    if typed_numbers.iter().all(|number| holds(&known, *number)) {
        Ok(Some(known))
    } else {
        Ok(Some(ValueType::F32))
    }
}

/// Returns the only output of the node `node` as an input.
pub(super) fn output(node: NodeId) -> InputValue {
    InputValue::Link(PortRef { node, port: None })
}

/// Returns the output `port` of the node `node` as an input.
pub(super) fn port(node: NodeId, port: &str) -> InputValue {
    InputValue::Link(PortRef {
        node,
        port: Some(port.to_owned()),
    })
}

/// Returns the output `port` of the node `node` as an operand of type `value_type`.
pub(super) fn port_of(node: NodeId, port: &str, value_type: ValueType) -> Operand {
    Operand::Link(
        PortRef {
            node,
            port: Some(port.to_owned()),
        },
        value_type,
    )
}

/// Returns the literal of the state `state` of the States property `property`.
pub(super) fn state_literal(property: &str, state: &str) -> Literal {
    Literal::Category(MemberName {
        owner: property.to_owned(),
        member: state.to_owned(),
    })
}

/// Returns the literal of the layer `layer` of the field `field` as an input.
pub(super) fn layer_literal(field: &str, layer: &str) -> InputValue {
    InputValue::Literal(Literal::Layer(MemberName {
        owner: field.to_owned(),
        member: layer.to_owned(),
    }))
}

/// Returns the `f32` literal `value` as an input.
pub(super) fn number(value: f32) -> InputValue {
    InputValue::Literal(Literal::F32(value))
}

/// Returns `number` as an input of type `value_type`, dropping a fraction toward zero with `truncate` as
/// [`number_literal`] does.
///
/// # Errors
///
/// Returns the message for a number the type cannot hold.
fn number_input(number: f64, value_type: &ValueType, truncate: bool) -> Result<InputValue, String> {
    number_literal(number, value_type, truncate)
        .map(InputValue::Literal)
        .ok_or_else(|| format!("the number {number} cannot stand where a {value_type} is wanted"))
}

/// Returns the literal of `number` in `value_type`, `None` when the type cannot hold it.
///
/// With `truncate` a fraction is dropped toward zero, as a value written to a Whole number property is.
fn number_literal(number: f64, value_type: &ValueType, truncate: bool) -> Option<Literal> {
    let number = if truncate && *value_type != ValueType::F32 {
        number.trunc()
    } else {
        number
    };
    if !holds(value_type, number) {
        return None;
    }
    match value_type {
        // `holds` checked the range, so each cast is exact.
        ValueType::U32 => Some(Literal::U32(number as u32)),
        ValueType::I32 => Some(Literal::I32(number as i32)),
        ValueType::F32 => {
            let narrowed = number as f32;
            narrowed.is_finite().then_some(Literal::F32(narrowed))
        }
        _ => None,
    }
}

impl Builder<'_> {
    /// Returns `operand` as an input of type `target`, writing a conversion at `site` when its type differs.
    ///
    /// # Errors
    ///
    /// Returns the message for an operand that cannot take `target`.
    pub(super) fn input_as(
        &mut self,
        site: &Site,
        operand: &Operand,
        target: &ValueType,
    ) -> Result<InputValue, String> {
        self.convert(site, operand, target, false)
    }

    /// Returns `operand` as an input of type `target` as [`Self::input_as`] does, with a typed number's fraction
    /// dropped toward zero to fit an integer type, as a value written to a Whole number property is.
    ///
    /// # Errors
    ///
    /// Returns the message for an operand that cannot take `target`.
    pub(super) fn input_truncated(
        &mut self,
        site: &Site,
        operand: &Operand,
        target: &ValueType,
    ) -> Result<InputValue, String> {
        self.convert(site, operand, target, true)
    }

    /// Returns `operand` as an input of type `target`, writing a conversion at `site` when its type differs.
    ///
    /// With `truncate` a typed number with a fraction is dropped toward zero to fit an integer type.
    fn convert(
        &mut self,
        site: &Site,
        operand: &Operand,
        target: &ValueType,
        truncate: bool,
    ) -> Result<InputValue, String> {
        match operand {
            Operand::Place(place) => self.convert(site, &place.xy, target, truncate),
            Operand::Number(number) => number_input(*number, target, truncate),
            Operand::State(state) => match target {
                ValueType::Category(property) => Ok(InputValue::Literal(state_literal(property, state))),
                _ => Err(format!("the state '{state}' stands where a {target} is wanted")),
            },
            Operand::Constant(literal) if literal.value_type() == *target => Ok(InputValue::Literal(literal.clone())),
            Operand::Constant(literal) => {
                let number = match literal {
                    Literal::U32(value) => f64::from(*value),
                    Literal::I32(value) => f64::from(*value),
                    Literal::F32(value) => f64::from(*value),
                    _ => return Err(format!("a {} stands where a {target} is wanted", literal.value_type())),
                };
                number_input(number, target, truncate)
            }
            Operand::Link(link, value_type) => {
                self.conversion(site, InputValue::Link(link.clone()), value_type, target)
            }
            Operand::Param(id, value_type) => self.conversion(site, InputValue::Param(id.clone()), value_type, target),
        }
    }

    /// Returns `source`, of type `value_type`, as an input of type `target`, writing a conversion at `site` when the
    /// two types differ.
    ///
    /// # Errors
    ///
    /// Returns the message for two types with no conversion between them.
    fn conversion(
        &mut self,
        site: &Site,
        source: InputValue,
        value_type: &ValueType,
        target: &ValueType,
    ) -> Result<InputValue, String> {
        if value_type == target {
            return Ok(source);
        }
        let op = match (value_type, target) {
            (ValueType::U32 | ValueType::I32 | ValueType::Bool, ValueType::F32) => "conv.to_f32",
            (ValueType::U32 | ValueType::F32, ValueType::I32) => "conv.to_i32",
            (ValueType::I32 | ValueType::F32, ValueType::U32) => "conv.to_u32",
            (ValueType::Vec2I | ValueType::Vec2U, ValueType::Vec2F) => "conv.to_vec2f",
            (ValueType::Vec2F | ValueType::Vec2U, ValueType::Vec2I) => "conv.to_vec2i",
            _ => {
                return Err(format!(
                    "a value of type {value_type} stands where a {target} is wanted"
                ));
            }
        };
        let node = self.emit(site, op, Vec::new(), vec![("a".to_owned(), source)]);
        Ok(output(node))
    }

    /// Returns `operand` as a yes or no input.
    ///
    /// # Errors
    ///
    /// Returns the message for an operand that is no yes or no.
    pub(super) fn condition_input(&mut self, site: &Site, operand: &Operand) -> Result<InputValue, String> {
        self.input_as(site, operand, &ValueType::Bool)
    }

    /// Writes the node `op` over `inputs` at `site`, whose one output has type `value_type`, and returns that output.
    pub(super) fn node_value(
        &mut self,
        site: &Site,
        op: &str,
        inputs: Vec<(&str, InputValue)>,
        value_type: ValueType,
    ) -> Operand {
        let node = self.emit_over(site, op, inputs);
        Operand::Link(PortRef { node, port: None }, value_type)
    }

    /// Writes the node `op` over `inputs` at `site` and returns its one output as an input.
    pub(super) fn node_input(&mut self, site: &Site, op: &str, inputs: Vec<(&str, InputValue)>) -> InputValue {
        output(self.emit_over(site, op, inputs))
    }

    /// Writes a `logic.select` of `if_true` where `cond` holds and `if_false` elsewhere at `site`, and returns its
    /// output as an input.
    pub(super) fn select(
        &mut self,
        site: &Site,
        cond: InputValue,
        if_true: InputValue,
        if_false: InputValue,
    ) -> InputValue {
        self.node_input(
            site,
            "logic.select",
            vec![("cond", cond), ("if_true", if_true), ("if_false", if_false)],
        )
    }

    /// Writes a `logic.select` as [`Self::select`] does, and returns its output of type `value_type`.
    pub(super) fn select_value(
        &mut self,
        site: &Site,
        cond: InputValue,
        if_true: InputValue,
        if_false: InputValue,
        value_type: ValueType,
    ) -> Operand {
        self.node_value(
            site,
            "logic.select",
            vec![("cond", cond), ("if_true", if_true), ("if_false", if_false)],
            value_type,
        )
    }

    /// Returns `value` where `condition` holds and the zero of `value_type` elsewhere, written at `site`.
    ///
    /// # Errors
    ///
    /// Returns the message for a condition that is no yes or no, a value that cannot take `value_type`, or a type with
    /// no zero.
    pub(super) fn where_applies(
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
        let cond = self.condition_input(site, condition)?;
        let if_true = self.input_as(site, value, value_type)?;
        Ok(self.select_value(site, cond, if_true, InputValue::Literal(zero), value_type.clone()))
    }

    /// Returns the `f32` input `amount` where `condition` holds and 0 elsewhere, written at `site`.
    ///
    /// # Errors
    ///
    /// Returns the message for a condition that is no yes or no.
    pub(super) fn amount_where(
        &mut self,
        site: &Site,
        condition: &Operand,
        amount: InputValue,
    ) -> Result<InputValue, String> {
        if *condition == Operand::Constant(Literal::Bool(true)) {
            return Ok(amount);
        }
        let cond = self.condition_input(site, condition)?;
        Ok(self.select(site, cond, amount, number(0.0)))
    }

    /// Writes a `region.put` of `value`, or of no value for a count, into the accumulator `accumulator` where `when`
    /// holds, at `site`.
    pub(super) fn put(&mut self, site: &Site, accumulator: &str, value: Option<InputValue>, when: InputValue) {
        let mut inputs = Vec::with_capacity(2);
        if let Some(value) = value {
            inputs.push(("value".to_owned(), value));
        }
        inputs.push(("when".to_owned(), when));
        self.emit(site, "region.put", vec![("acc", text(accumulator))], inputs);
    }

    /// Writes a `cat.switch` over the States property `category` at `site`, taking the case of `cases` named by the
    /// state of `select`, and returns its temporary id.
    pub(super) fn switch(
        &mut self,
        site: &Site,
        category: &str,
        select: InputValue,
        cases: Vec<(&str, InputValue)>,
    ) -> NodeId {
        let mut inputs = vec![("select".to_owned(), select)];
        inputs.extend(cases.into_iter().map(|(state, value)| (format!("case.{state}"), value)));
        self.emit(site, "cat.switch", vec![("category", text(category))], inputs)
    }

    /// Writes the node `op` over `inputs` at `site`, with no configuration, and returns its temporary id.
    fn emit_over(&mut self, site: &Site, op: &str, inputs: Vec<(&str, InputValue)>) -> NodeId {
        let inputs = inputs
            .into_iter()
            .map(|(port, input)| (port.to_owned(), input))
            .collect();
        self.emit(site, op, Vec::new(), inputs)
    }

    /// Returns `first` and `second` joined by the logic node `op`, folding a constant side away.
    ///
    /// # Errors
    ///
    /// Returns the message for a side that is no yes or no.
    pub(super) fn join(&mut self, site: &Site, op: &str, first: &Operand, second: &Operand) -> Result<Operand, String> {
        let identity = op == "logic.and";
        for (one, other) in [(first, second), (second, first)] {
            if let Operand::Constant(Literal::Bool(value)) = one {
                // `true and x` and `false or x` are `x`, and `false and x` and `true or x` are constants.
                return if *value == identity {
                    Ok(other.clone())
                } else {
                    Ok(Operand::Constant(Literal::Bool(*value)))
                };
            }
        }
        let a = self.condition_input(site, first)?;
        let b = self.condition_input(site, second)?;
        Ok(self.node_value(site, op, vec![("a", a), ("b", b)], ValueType::Bool))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_number_takes_the_type_it_meets_when_it_fits() {
        let count = Operand::Link(
            PortRef {
                node: crate::ids::NodeId::new(1).unwrap_or_else(|| unreachable!()),
                port: None,
            },
            ValueType::U32,
        );
        assert_eq!(common_type(&count, &Operand::Number(3.0)), Ok(ValueType::U32));
        assert_eq!(common_type(&count, &Operand::Number(2.5)), Ok(ValueType::F32));
        assert_eq!(common_type(&count, &Operand::Number(-1.0)), Ok(ValueType::F32));
        let whole = Operand::Param("n".to_owned(), ValueType::I32);
        assert_eq!(common_type(&count, &whole), Ok(ValueType::F32));
        assert_eq!(
            common_type(&Operand::Number(1.0), &Operand::Number(2.0)),
            Ok(ValueType::F32)
        );
        assert_eq!(number_literal(2.7, &ValueType::I32, true), Some(Literal::I32(2)));
        assert_eq!(number_literal(2.7, &ValueType::I32, false), None);
    }
}
