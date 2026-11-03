//! Value types and rates of a graph.
//!
//! A *value type* is what a socket carries, written in a file as a type string such as `vec2f` or `cat:health`. A
//! *rate* is how often a value changes.

use std::fmt;
use std::str::FromStr;

use crate::ids::is_id;

/// Type of a value in a graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueType {
    /// A truth value, stored on the GPU as a `u32` word.
    Bool,
    /// A 32-bit unsigned integer.
    U32,
    /// A 32-bit signed integer.
    I32,
    /// A 32-bit float.
    F32,
    /// Two `u32` components.
    Vec2U,
    /// Two `i32` components.
    Vec2I,
    /// Two `f32` components.
    Vec2F,
    /// A variant of the category with this id.
    Category(String),
    /// A layer index of the field with this id.
    Layer(String),
    /// A reference to a grid cell, which allows no arithmetic.
    Cell,
    /// A reference to an agent, which allows no arithmetic.
    Agent,
    /// A 64-bit unsigned integer, on the host only.
    U64,
    /// A 64-bit float, on the host only.
    F64,
    /// Two `f64` components, on the host only.
    Vec2D,
}

impl ValueType {
    /// Returns whether the type exists on the host alone, with no WGSL form.
    pub fn is_host_only(&self) -> bool {
        matches!(self, Self::U64 | Self::F64 | Self::Vec2D)
    }
}

impl fmt::Display for ValueType {
    /// Writes the type string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => f.write_str("bool"),
            Self::U32 => f.write_str("u32"),
            Self::I32 => f.write_str("i32"),
            Self::F32 => f.write_str("f32"),
            Self::Vec2U => f.write_str("vec2u"),
            Self::Vec2I => f.write_str("vec2i"),
            Self::Vec2F => f.write_str("vec2f"),
            Self::Category(id) => write!(f, "cat:{id}"),
            Self::Layer(id) => write!(f, "layer:{id}"),
            Self::Cell => f.write_str("cell"),
            Self::Agent => f.write_str("agent"),
            Self::U64 => f.write_str("u64"),
            Self::F64 => f.write_str("f64"),
            Self::Vec2D => f.write_str("vec2d"),
        }
    }
}

impl FromStr for ValueType {
    type Err = String;

    /// Reads a type string.
    ///
    /// # Errors
    ///
    /// Returns the message for text that names no type, or a `cat:` or `layer:` id outside the id grammar.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let declared = |id: &str, kind: &str| {
            if is_id(id) {
                Ok(id.to_owned())
            } else {
                Err(format!("'{id}' is no {kind} id"))
            }
        };
        if let Some(id) = text.strip_prefix("cat:") {
            return declared(id, "category").map(Self::Category);
        }
        if let Some(id) = text.strip_prefix("layer:") {
            return declared(id, "field").map(Self::Layer);
        }
        match text {
            "bool" => Ok(Self::Bool),
            "u32" => Ok(Self::U32),
            "i32" => Ok(Self::I32),
            "f32" => Ok(Self::F32),
            "vec2u" => Ok(Self::Vec2U),
            "vec2i" => Ok(Self::Vec2I),
            "vec2f" => Ok(Self::Vec2F),
            "cell" => Ok(Self::Cell),
            "agent" => Ok(Self::Agent),
            "u64" => Ok(Self::U64),
            "f64" => Ok(Self::F64),
            "vec2d" => Ok(Self::Vec2D),
            _ => Err(format!("'{text}' is no value type")),
        }
    }
}

/// How often a value changes, ordered from the slowest to the fastest.
///
/// A node's output takes the [join](Self::join) of its inputs' rates unless its definition pins it. A uniform value
/// feeds an element socket by broadcast, and an element value reaches a uniform socket only through a reducer or an
/// accumulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rate {
    /// A constant of the file.
    Constant,
    /// A parameter, fixed when the model is built.
    Parameter,
    /// A value that changes once per tick.
    Tick,
    /// A value per element of the pass domain.
    Element,
    /// A value per element and region iteration, inside a region body.
    Pair,
}

impl Rate {
    /// Returns the faster of the two rates.
    pub fn join(self, other: Self) -> Self {
        self.max(other)
    }

    /// Returns whether a value at this rate is the same for every element.
    pub fn is_uniform(self) -> bool {
        self <= Self::Tick
    }
}
