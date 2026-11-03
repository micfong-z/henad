//! Values of a graph file: port references, literals, input values and configuration values.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::ids::{NodeId, is_port_segment};
use crate::types::ValueType;

/// An RGBA colour, read from 3 or 4 bytes and always written as 4.
pub type Color = [u8; 4];

/// A reference to a node's output, written `"n12"` or `"n12.port"`.
///
/// `"n12"` names the node's only output. Inside a region body `"n7.delta"` names the region's inner output `delta`, and
/// outside it `"n7.sum"` names the accumulator output `sum`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortRef {
    /// Node that holds the port.
    pub node: NodeId,
    /// Port name, dotted when configuration made it, or `None` for the node's only output.
    pub port: Option<String>,
}

impl fmt::Display for PortRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.port {
            Some(port) => write!(f, "{}.{port}", self.node),
            None => write!(f, "{}", self.node),
        }
    }
}

impl FromStr for PortRef {
    type Err = String;

    /// Reads a reference.
    ///
    /// # Errors
    ///
    /// Returns the message for text with another form of node id, or a port segment outside the grammar
    /// `[A-Za-z][A-Za-z0-9_]{0,63}`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (node, port) = match text.split_once('.') {
            Some((node, port)) => (node, Some(port)),
            None => (text, None),
        };
        let node = node.parse()?;
        if let Some(port) = port
            && !port.split('.').all(is_port_segment)
        {
            return Err(format!(
                "'{text}' names no port, whose segments are letters, digits and `_`"
            ));
        }
        Ok(Self {
            node,
            port: port.map(str::to_owned),
        })
    }
}

/// A name of the form `<owner>.<member>`, such as a category's variant `health.I`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberName {
    /// Id of the declaration that owns the member.
    pub owner: String,
    /// Name of the member.
    pub member: String,
}

impl fmt::Display for MemberName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.owner, self.member)
    }
}

/// A constant value of a graph, written as a one-key table tagged by its type.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Literal {
    /// `{ bool = true }`.
    Bool(bool),
    /// `{ u32 = 3 }`.
    U32(u32),
    /// `{ i32 = -1 }`.
    I32(i32),
    /// `{ f32 = 1.0 }`, always finite.
    F32(f32),
    /// `{ u64 = "0xdeadbeef" }`, a string since TOML integers are signed.
    U64(u64),
    /// `{ f64 = 0.407 }`, always finite.
    F64(f64),
    /// `{ vec2u = [1, 2] }`.
    Vec2U([u32; 2]),
    /// `{ vec2i = [0, -1] }`.
    Vec2I([i32; 2]),
    /// `{ vec2f = [0.0, 0.5] }`, always finite.
    Vec2F([f32; 2]),
    /// `{ cat = "health.I" }`, a variant of a category.
    Category(MemberName),
    /// `{ layer = "pheromone.to_food" }`, a layer of a field.
    Layer(MemberName),
    /// `{ option = "network.Geometric" }`, the `u32` index of an option of a choice parameter.
    ChoiceOption(MemberName),
}

impl Literal {
    /// Returns the type of the literal.
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::U32(_) | Self::ChoiceOption(_) => ValueType::U32,
            Self::I32(_) => ValueType::I32,
            Self::F32(_) => ValueType::F32,
            Self::U64(_) => ValueType::U64,
            Self::F64(_) => ValueType::F64,
            Self::Vec2U(_) => ValueType::Vec2U,
            Self::Vec2I(_) => ValueType::Vec2I,
            Self::Vec2F(_) => ValueType::Vec2F,
            Self::Category(name) => ValueType::Category(name.owner.clone()),
            Self::Layer(name) => ValueType::Layer(name.owner.clone()),
        }
    }

    /// Returns the zero of `value_type`, or `None` for a type whose zero has no literal form: a category, a layer, a
    /// reference and `vec2d`.
    pub fn zero_of(value_type: &ValueType) -> Option<Self> {
        match value_type {
            ValueType::Bool => Some(Self::Bool(false)),
            ValueType::U32 => Some(Self::U32(0)),
            ValueType::I32 => Some(Self::I32(0)),
            ValueType::F32 => Some(Self::F32(0.0)),
            ValueType::U64 => Some(Self::U64(0)),
            ValueType::F64 => Some(Self::F64(0.0)),
            ValueType::Vec2U => Some(Self::Vec2U([0, 0])),
            ValueType::Vec2I => Some(Self::Vec2I([0, 0])),
            ValueType::Vec2F => Some(Self::Vec2F([0.0, 0.0])),
            ValueType::Category(_) | ValueType::Layer(_) | ValueType::Cell | ValueType::Agent | ValueType::Vec2D => {
                None
            }
        }
    }
}

/// Value of one input of a node.
///
/// The three forms are never ambiguous, since a link is a string and the other two are one-key tables.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum InputValue {
    /// A link to another node's output.
    Link(PortRef),
    /// A constant.
    Literal(Literal),
    /// A binding to the parameter with this id, written `{ param = "id" }`.
    Param(String),
}

/// Value of one configuration key of a node.
///
/// Configuration selects code and is never connectable. At the top level of a node it is a scalar or an array, and a
/// table appears only as an element of an array.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConfigValue {
    /// A truth value.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A finite float.
    Float(f64),
    /// A string.
    String(String),
    /// An array, written on one line when it holds no table.
    Array(Vec<Self>),
    /// A one-line inline table, as an element of an array.
    Table(BTreeMap<String, Self>),
}

impl ConfigValue {
    /// Returns the value as a string, or `None` for another type.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    /// Returns the value as an integer, or `None` for another type.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(number) => Some(*number),
            _ => None,
        }
    }

    /// Returns the value as a truth value, or `None` for another type.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as an array, or `None` for another type.
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Returns the value as a table, or `None` for another type.
    pub fn as_table(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }
}
