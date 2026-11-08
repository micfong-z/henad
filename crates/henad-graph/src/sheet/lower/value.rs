//! Values of a lowering: what a slot lowers to, and the conversions between the types the sheet's numbers take.
//!
//! A sheet has one type of number, and the graph has several. A count is `u32`, a Number property `f32`, a Whole number
//! property `i32`, a Number parameter `f32` and a Whole number parameter `u32`. A number the author typed takes the type
//! of the value it meets when that type holds it exactly, and otherwise both sides compare as `f32`.

use super::{Builder, Site};
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
    /// With `truncate` a typed number with a fraction is dropped toward zero to fit an integer type.
    ///
    /// # Errors
    ///
    /// Returns the message for an operand that cannot take `target`.
    pub(super) fn input_as(
        &mut self,
        site: &Site,
        operand: &Operand,
        target: &ValueType,
        truncate: bool,
    ) -> Result<InputValue, String> {
        match operand {
            Operand::Place(place) => self.input_as(site, &place.xy, target, truncate),
            Operand::Number(number) => number_literal(*number, target, truncate)
                .map(InputValue::Literal)
                .ok_or_else(|| format!("the number {number} cannot stand where a {target} is wanted")),
            Operand::State(state) => match target {
                ValueType::Category(property) => Ok(InputValue::Literal(Literal::Category(MemberName {
                    owner: property.clone(),
                    member: state.clone(),
                }))),
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
                number_literal(number, target, truncate)
                    .map(InputValue::Literal)
                    .ok_or_else(|| format!("the number {number} cannot stand where a {target} is wanted"))
            }
            Operand::Link(link, value_type) if value_type == target => Ok(InputValue::Link(link.clone())),
            Operand::Param(id, value_type) if value_type == target => Ok(InputValue::Param(id.clone())),
            Operand::Link(_, value_type) | Operand::Param(_, value_type) => {
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
                let source = match operand {
                    Operand::Link(link, _) => InputValue::Link(link.clone()),
                    Operand::Param(id, _) => InputValue::Param(id.clone()),
                    _ => unreachable!("the arm matched a link or a parameter"),
                };
                let node = self.emit(site, op, Vec::new(), vec![("a".to_owned(), source)]);
                Ok(InputValue::Link(PortRef { node, port: None }))
            }
        }
    }

    /// Returns `operand` as a yes or no input.
    ///
    /// # Errors
    ///
    /// Returns the message for an operand that is no yes or no.
    pub(super) fn condition_input(&mut self, site: &Site, operand: &Operand) -> Result<InputValue, String> {
        self.input_as(site, operand, &ValueType::Bool, false)
    }

    /// Writes the node `op` over `inputs` at `site`, whose one output has type `value_type`, and returns that output.
    pub(super) fn node_value(
        &mut self,
        site: &Site,
        op: &str,
        inputs: Vec<(&str, InputValue)>,
        value_type: ValueType,
    ) -> Operand {
        let node = self.emit(
            site,
            op,
            Vec::new(),
            inputs
                .into_iter()
                .map(|(port, input)| (port.to_owned(), input))
                .collect(),
        );
        Operand::Link(PortRef { node, port: None }, value_type)
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
