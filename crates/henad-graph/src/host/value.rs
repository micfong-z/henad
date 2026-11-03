//! Values the host evaluator computes, the lane columns it reads and writes, and the operations of the arithmetic,
//! bit and conversion kinds on them.

use std::cmp::Ordering;

use henad_core::params::ParamValue;

use crate::document::GraphDocument;
use crate::types::ValueType;
use crate::value::Literal;

/// One value of a graph on the host.
///
/// A category value is the number of its variant, counted from 0 in the order the category declares them. Two values
/// compare equal as the `cmp.eq` kind compares them: a float NaN equals nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum HostValue {
    /// A truth value.
    Bool(bool),
    /// A 32-bit unsigned integer.
    U32(u32),
    /// A 32-bit signed integer.
    I32(i32),
    /// A 32-bit float.
    F32(f32),
    /// Two `u32` components.
    Vec2U([u32; 2]),
    /// Two `i32` components.
    Vec2I([i32; 2]),
    /// Two `f32` components.
    Vec2F([f32; 2]),
    /// The variant with this number.
    Category(u32),
    /// A 64-bit unsigned integer.
    U64(u64),
    /// A 64-bit float.
    F64(f64),
    /// Two `f64` components.
    Vec2D([f64; 2]),
}

impl HostValue {
    /// Returns the zero of `value_type`: false, 0, the first variant of a category. `None` for a reference or a layer.
    pub fn zero_of(value_type: &ValueType) -> Option<Self> {
        Some(match value_type {
            ValueType::Bool => Self::Bool(false),
            ValueType::U32 => Self::U32(0),
            ValueType::I32 => Self::I32(0),
            ValueType::F32 => Self::F32(0.0),
            ValueType::Vec2U => Self::Vec2U([0, 0]),
            ValueType::Vec2I => Self::Vec2I([0, 0]),
            ValueType::Vec2F => Self::Vec2F([0.0, 0.0]),
            ValueType::Category(_) => Self::Category(0),
            ValueType::U64 => Self::U64(0),
            ValueType::F64 => Self::F64(0.0),
            ValueType::Vec2D => Self::Vec2D([0.0, 0.0]),
            _ => return None,
        })
    }

    /// Returns the value of `literal` in `document`, or `None` for a name the document does not declare.
    pub fn from_literal(literal: &Literal, document: &GraphDocument) -> Option<Self> {
        Some(match literal {
            Literal::Bool(flag) => Self::Bool(*flag),
            Literal::U32(number) => Self::U32(*number),
            Literal::I32(number) => Self::I32(*number),
            Literal::F32(number) => Self::F32(*number),
            Literal::U64(number) => Self::U64(*number),
            Literal::F64(number) => Self::F64(*number),
            Literal::Vec2U(pair) => Self::Vec2U(*pair),
            Literal::Vec2I(pair) => Self::Vec2I(*pair),
            Literal::Vec2F(pair) => Self::Vec2F(*pair),
            Literal::Category(name) => {
                let category = document.categories.iter().find(|category| category.id == name.owner)?;
                let position = category
                    .variants
                    .iter()
                    .position(|variant| variant.name == name.member)?;
                Self::Category(u32::try_from(position).ok()?)
            }
            Literal::ChoiceOption(name) => {
                let param = document.params.iter().find(|param| param.id() == name.owner)?;
                let henad_core::params::ParamKind::Choice { options, .. } = param.kind() else {
                    return None;
                };
                Self::U32(u32::try_from(options.position(&name.member)?).ok()?)
            }
            _ => return None,
        })
    }

    /// Returns the value of a parameter: an option's number for a choice.
    pub fn from_param(value: &ParamValue) -> Self {
        match value {
            ParamValue::F32(number) => Self::F32(*number),
            ParamValue::U32(number) => Self::U32(*number),
            ParamValue::Bool(flag) => Self::Bool(*flag),
            ParamValue::Choice(index) => Self::U32(u32::try_from(*index).unwrap_or(u32::MAX)),
        }
    }

    /// Returns the value as a truth value, or `None` for another type.
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(flag) => Some(flag),
            _ => None,
        }
    }

    /// Returns the value as a `u32`, or `None` for another type.
    pub fn as_u32(self) -> Option<u32> {
        match self {
            Self::U32(number) => Some(number),
            _ => None,
        }
    }

    /// Returns the value as an `f32`, or `None` for another type.
    pub fn as_f32(self) -> Option<f32> {
        match self {
            Self::F32(number) => Some(number),
            _ => None,
        }
    }

    /// Returns the variant number of a category value, or `None` for another type.
    pub fn as_variant(self) -> Option<u32> {
        match self {
            Self::Category(number) => Some(number),
            _ => None,
        }
    }
}

/// Returns the number of `u32` words a lane of `value_type` stores per element, or `None` for a type a lane cannot
/// hold on the host.
pub(crate) fn words_per_value(value_type: &ValueType) -> Option<usize> {
    Some(match value_type {
        ValueType::Bool | ValueType::U32 | ValueType::I32 | ValueType::F32 | ValueType::Category(_) => 1,
        ValueType::Vec2U | ValueType::Vec2I | ValueType::Vec2F | ValueType::U64 | ValueType::F64 => 2,
        ValueType::Vec2D => 4,
        _ => return None,
    })
}

/// One lane of every element of its domain, as the `u32` words a GPU buffer holds.
///
/// Each element takes the same number of words: one for a `bool` (0 or 1), a `u32`, an `i32`, an `f32` or a category
/// (its variant number), two for a `vec2`, a `u64` or an `f64` (low word first), and four for a `vec2d`. A narrower
/// [`storage`](crate::document::Lane::storage) is left to the upload.
#[derive(Clone)]
pub struct LaneColumn {
    id: String,
    value_type: ValueType,
    stride: usize,
    words: Vec<u32>,
}

/// Prints the lane id, its type and the number of elements in place of the words.
impl std::fmt::Debug for LaneColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaneColumn")
            .field("id", &self.id)
            .field("value_type", &self.value_type)
            .field("elements", &self.len())
            .finish_non_exhaustive()
    }
}

impl LaneColumn {
    /// Returns a column of `elements` values of `value_type`, each `value`, or `None` for a type a lane cannot hold
    /// on the host, a value of another type, or words the allocator cannot provide.
    pub(crate) fn filled(id: &str, value_type: &ValueType, elements: usize, value: HostValue) -> Option<Self> {
        let stride = words_per_value(value_type)?;
        let mut column = Self {
            id: id.to_owned(),
            value_type: value_type.clone(),
            stride,
            words: Vec::new(),
        };
        let mut encoded = [0u32; 4];
        column.encode(value, &mut encoded)?;
        column.words.try_reserve_exact(elements.checked_mul(stride)?).ok()?;
        for _ in 0..elements {
            column.words.extend_from_slice(&encoded[..stride]);
        }
        Some(column)
    }

    /// Returns a copy of the column, or `None` when the allocator cannot provide its words.
    pub(crate) fn try_clone(&self) -> Option<Self> {
        let mut words = Vec::new();
        words.try_reserve_exact(self.words.len()).ok()?;
        words.extend_from_slice(&self.words);
        Some(Self {
            id: self.id.clone(),
            value_type: self.value_type.clone(),
            stride: self.stride,
            words,
        })
    }

    /// Returns the column with `words` in place of its words, for a test that reads a lane back from a device.
    #[cfg(test)]
    pub(crate) fn with_words(&self, words: Vec<u32>) -> Self {
        Self {
            id: self.id.clone(),
            value_type: self.value_type.clone(),
            stride: self.stride,
            words,
        }
    }

    /// Id of the lane.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Type of the lane's values.
    pub fn value_type(&self) -> &ValueType {
        &self.value_type
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.words.len() / self.stride
    }

    /// Whether the column holds no element.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Words of every element, in element order.
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// Returns the value of element `index`, or `None` past the end.
    pub fn get(&self, index: usize) -> Option<HostValue> {
        let start = index.checked_mul(self.stride)?;
        let words = self.words.get(start..start.checked_add(self.stride)?)?;
        let wide = |low: u32, high: u32| u64::from(low) | (u64::from(high) << 32);
        Some(match self.value_type {
            ValueType::Bool => HostValue::Bool(words[0] != 0),
            ValueType::U32 => HostValue::U32(words[0]),
            ValueType::I32 => HostValue::I32(words[0] as i32),
            ValueType::F32 => HostValue::F32(f32::from_bits(words[0])),
            ValueType::Category(_) => HostValue::Category(words[0]),
            ValueType::Vec2U => HostValue::Vec2U([words[0], words[1]]),
            ValueType::Vec2I => HostValue::Vec2I([words[0] as i32, words[1] as i32]),
            ValueType::Vec2F => HostValue::Vec2F([f32::from_bits(words[0]), f32::from_bits(words[1])]),
            ValueType::U64 => HostValue::U64(wide(words[0], words[1])),
            ValueType::F64 => HostValue::F64(f64::from_bits(wide(words[0], words[1]))),
            ValueType::Vec2D => HostValue::Vec2D([
                f64::from_bits(wide(words[0], words[1])),
                f64::from_bits(wide(words[2], words[3])),
            ]),
            _ => return None,
        })
    }

    /// Sets element `index` to `value`, and returns whether it fit: `false` past the end or for a value of another
    /// type.
    pub fn set(&mut self, index: usize, value: HostValue) -> bool {
        let mut encoded = [0u32; 4];
        if self.encode(value, &mut encoded).is_none() {
            return false;
        }
        let stride = self.stride;
        let Some(start) = index.checked_mul(stride) else {
            return false;
        };
        match self.words.get_mut(start..start + stride) {
            Some(words) => {
                words.copy_from_slice(&encoded[..stride]);
                true
            }
            None => false,
        }
    }

    /// Writes the words of `value` into `encoded`, or returns `None` for a value of another type than the lane's.
    fn encode(&self, value: HostValue, encoded: &mut [u32; 4]) -> Option<()> {
        let halves = |word: u64| [word as u32, (word >> 32) as u32];
        match (&self.value_type, value) {
            (ValueType::Bool, HostValue::Bool(flag)) => encoded[0] = u32::from(flag),
            (ValueType::U32, HostValue::U32(number)) | (ValueType::Category(_), HostValue::Category(number)) => {
                encoded[0] = number;
            }
            (ValueType::I32, HostValue::I32(number)) => encoded[0] = number as u32,
            (ValueType::F32, HostValue::F32(number)) => encoded[0] = number.to_bits(),
            (ValueType::Vec2U, HostValue::Vec2U(pair)) => encoded[..2].copy_from_slice(&pair),
            (ValueType::Vec2I, HostValue::Vec2I([x, y])) => encoded[..2].copy_from_slice(&[x as u32, y as u32]),
            (ValueType::Vec2F, HostValue::Vec2F([x, y])) => encoded[..2].copy_from_slice(&[x.to_bits(), y.to_bits()]),
            (ValueType::U64, HostValue::U64(number)) => encoded[..2].copy_from_slice(&halves(number)),
            (ValueType::F64, HostValue::F64(number)) => encoded[..2].copy_from_slice(&halves(number.to_bits())),
            (ValueType::Vec2D, HostValue::Vec2D([x, y])) => {
                encoded[..2].copy_from_slice(&halves(x.to_bits()));
                encoded[2..].copy_from_slice(&halves(y.to_bits()));
            }
            _ => return None,
        }
        Some(())
    }
}

/// Operation of a numeric or bit kind on two values of one type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Min,
    Max,
    Xor,
    And,
    Or,
}

/// Applies `op` to `a` and `b`, per component on vectors, or returns `None` for two values it does not take.
pub(crate) fn binary(op: BinaryOp, a: HostValue, b: HostValue) -> Option<HostValue> {
    use HostValue::{F32, F64, I32, U32, U64, Vec2D, Vec2F, Vec2I, Vec2U};
    let unsigned = |a: u32, b: u32| -> Option<u32> {
        Some(match op {
            BinaryOp::Add => a.wrapping_add(b),
            BinaryOp::Sub => a.wrapping_sub(b),
            BinaryOp::Mul => a.wrapping_mul(b),
            BinaryOp::Div => a.checked_div(b).unwrap_or(a),
            BinaryOp::Rem => a.checked_rem(b).unwrap_or(0),
            BinaryOp::Min => a.min(b),
            BinaryOp::Max => a.max(b),
            BinaryOp::Xor => a ^ b,
            BinaryOp::And => a & b,
            BinaryOp::Or => a | b,
        })
    };
    let signed = |a: i32, b: i32| -> Option<i32> {
        Some(match op {
            BinaryOp::Add => a.wrapping_add(b),
            BinaryOp::Sub => a.wrapping_sub(b),
            BinaryOp::Mul => a.wrapping_mul(b),
            BinaryOp::Div if b == 0 => a,
            BinaryOp::Div => a.wrapping_div(b),
            BinaryOp::Rem if b == 0 => 0,
            BinaryOp::Rem => a.wrapping_rem(b),
            BinaryOp::Min => a.min(b),
            BinaryOp::Max => a.max(b),
            BinaryOp::Xor => a ^ b,
            BinaryOp::And => a & b,
            BinaryOp::Or => a | b,
        })
    };
    let wide = |a: u64, b: u64| -> Option<u64> {
        Some(match op {
            BinaryOp::Add => a.wrapping_add(b),
            BinaryOp::Sub => a.wrapping_sub(b),
            BinaryOp::Mul => a.wrapping_mul(b),
            BinaryOp::Div => a.checked_div(b).unwrap_or(a),
            BinaryOp::Rem => a.checked_rem(b).unwrap_or(0),
            BinaryOp::Min => a.min(b),
            BinaryOp::Max => a.max(b),
            BinaryOp::Xor => a ^ b,
            BinaryOp::And => a & b,
            BinaryOp::Or => a | b,
        })
    };
    let float = |a: f32, b: f32| -> Option<f32> {
        Some(match op {
            BinaryOp::Add => a + b,
            BinaryOp::Sub => a - b,
            BinaryOp::Mul => a * b,
            BinaryOp::Div => a / b,
            BinaryOp::Rem => a % b,
            BinaryOp::Min => a.min(b),
            BinaryOp::Max => a.max(b),
            BinaryOp::Xor | BinaryOp::And | BinaryOp::Or => return None,
        })
    };
    let double = |a: f64, b: f64| -> Option<f64> {
        Some(match op {
            BinaryOp::Add => a + b,
            BinaryOp::Sub => a - b,
            BinaryOp::Mul => a * b,
            BinaryOp::Div => a / b,
            BinaryOp::Rem => a % b,
            BinaryOp::Min => a.min(b),
            BinaryOp::Max => a.max(b),
            BinaryOp::Xor | BinaryOp::And | BinaryOp::Or => return None,
        })
    };
    Some(match (a, b) {
        (U32(a), U32(b)) => U32(unsigned(a, b)?),
        (I32(a), I32(b)) => I32(signed(a, b)?),
        (F32(a), F32(b)) => F32(float(a, b)?),
        (U64(a), U64(b)) => U64(wide(a, b)?),
        (F64(a), F64(b)) => F64(double(a, b)?),
        (Vec2U(a), Vec2U(b)) => Vec2U([unsigned(a[0], b[0])?, unsigned(a[1], b[1])?]),
        (Vec2I(a), Vec2I(b)) => Vec2I([signed(a[0], b[0])?, signed(a[1], b[1])?]),
        (Vec2F(a), Vec2F(b)) => Vec2F([float(a[0], b[0])?, float(a[1], b[1])?]),
        (Vec2D(a), Vec2D(b)) => Vec2D([double(a[0], b[0])?, double(a[1], b[1])?]),
        _ => return None,
    })
}

/// Returns `a` shifted by `amount` bits, left when `left` holds, with the amount taken modulo the bit width of `a`.
pub(crate) fn shift(a: HostValue, amount: u32, left: bool) -> Option<HostValue> {
    Some(match a {
        HostValue::U32(a) => HostValue::U32(if left {
            a.wrapping_shl(amount)
        } else {
            a.wrapping_shr(amount)
        }),
        HostValue::I32(a) => HostValue::I32(if left {
            a.wrapping_shl(amount)
        } else {
            a.wrapping_shr(amount)
        }),
        HostValue::U64(a) => HostValue::U64(if left {
            a.wrapping_shl(amount)
        } else {
            a.wrapping_shr(amount)
        }),
        _ => return None,
    })
}

/// Operation of a kind on one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Abs,
    Ceil,
    Trunc,
    Round,
    Exp,
    Log,
    Sin,
    Cos,
    Not,
}

/// Applies `op` to `a`, per component on vectors, or returns `None` for a value it does not take.
pub(crate) fn unary(op: UnaryOp, a: HostValue) -> Option<HostValue> {
    let float = |x: f32| -> f32 {
        match op {
            UnaryOp::Abs => x.abs(),
            UnaryOp::Ceil => x.ceil(),
            UnaryOp::Trunc => x.trunc(),
            UnaryOp::Round => x.round_ties_even(),
            UnaryOp::Exp => x.exp(),
            UnaryOp::Log => x.ln(),
            UnaryOp::Sin => x.sin(),
            UnaryOp::Cos => x.cos(),
            UnaryOp::Not => f32::NAN,
        }
    };
    let double = |x: f64| -> f64 {
        match op {
            UnaryOp::Abs => x.abs(),
            UnaryOp::Ceil => x.ceil(),
            UnaryOp::Trunc => x.trunc(),
            UnaryOp::Round => x.round_ties_even(),
            UnaryOp::Exp => x.exp(),
            UnaryOp::Log => x.ln(),
            UnaryOp::Sin => x.sin(),
            UnaryOp::Cos => x.cos(),
            UnaryOp::Not => f64::NAN,
        }
    };
    Some(match (op, a) {
        (UnaryOp::Not, HostValue::U32(x)) => HostValue::U32(!x),
        (UnaryOp::Not, HostValue::I32(x)) => HostValue::I32(!x),
        (UnaryOp::Not, HostValue::U64(x)) => HostValue::U64(!x),
        (UnaryOp::Not, _) => return None,
        (UnaryOp::Abs, HostValue::I32(x)) => HostValue::I32(x.wrapping_abs()),
        (UnaryOp::Abs, HostValue::Vec2I([x, y])) => HostValue::Vec2I([x.wrapping_abs(), y.wrapping_abs()]),
        (_, HostValue::F32(x)) => HostValue::F32(float(x)),
        (_, HostValue::Vec2F([x, y])) => HostValue::Vec2F([float(x), float(y)]),
        (_, HostValue::F64(x)) => HostValue::F64(double(x)),
        (_, HostValue::Vec2D([x, y])) => HostValue::Vec2D([double(x), double(y)]),
        _ => return None,
    })
}

/// Returns the ordering of two scalars of one type, `None` for a NaN or for two values without an order.
pub(crate) fn order(a: HostValue, b: HostValue) -> Option<Ordering> {
    match (a, b) {
        (HostValue::U32(a), HostValue::U32(b)) => a.partial_cmp(&b),
        (HostValue::I32(a), HostValue::I32(b)) => a.partial_cmp(&b),
        (HostValue::F32(a), HostValue::F32(b)) => a.partial_cmp(&b),
        (HostValue::U64(a), HostValue::U64(b)) => a.partial_cmp(&b),
        (HostValue::F64(a), HostValue::F64(b)) => a.partial_cmp(&b),
        _ => None,
    }
}

/// Target of a conversion kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Conversion {
    ToF32,
    ToU32,
    ToI32,
}

/// Largest `f32` below 2^32, the top of the range WGSL clamps an `f32` to before converting it to a `u32`.
const LARGEST_F32_BELOW_2_POW_32: f32 = f32::from_bits(0x4f7f_ffff);

/// Largest `f32` below 2^31, the top of the range WGSL clamps an `f32` to before converting it to an `i32`.
const LARGEST_F32_BELOW_2_POW_31: f32 = f32::from_bits(0x4eff_ffff);

/// Returns `x` clamped to `top` from above, and a NaN as it is. `f32::min` would turn a NaN into `top`.
fn clamp_above(x: f32, top: f32) -> f32 {
    if x > top { top } else { x }
}

/// Converts `a` as the conversion kinds define it, or returns `None` for a value the kind does not take.
///
/// A float converts as WGSL converts it: clamped to the values of its type that the target holds exactly, then
/// truncated toward zero. A NaN gives 0.
pub(crate) fn convert(target: Conversion, a: HostValue) -> Option<HostValue> {
    Some(match (target, a) {
        (Conversion::ToF32, HostValue::U32(x)) => HostValue::F32(x as f32),
        (Conversion::ToF32, HostValue::I32(x)) => HostValue::F32(x as f32),
        (Conversion::ToF32, HostValue::Bool(x)) => HostValue::F32(f32::from(u8::from(x))),
        (Conversion::ToF32, HostValue::U64(x)) => HostValue::F32(x as f32),
        (Conversion::ToF32, HostValue::F64(x)) => HostValue::F32(x as f32),
        (Conversion::ToU32, HostValue::F32(x)) => HostValue::U32(clamp_above(x, LARGEST_F32_BELOW_2_POW_32) as u32),
        (Conversion::ToU32, HostValue::I32(x)) => HostValue::U32(x as u32),
        (Conversion::ToU32, HostValue::Bool(x)) => HostValue::U32(u32::from(x)),
        (Conversion::ToU32, HostValue::U64(x)) => HostValue::U32(x as u32),
        (Conversion::ToU32, HostValue::F64(x)) => HostValue::U32(x as u32),
        (Conversion::ToI32, HostValue::F32(x)) => HostValue::I32(clamp_above(x, LARGEST_F32_BELOW_2_POW_31) as i32),
        (Conversion::ToI32, HostValue::U32(x)) => HostValue::I32(x as i32),
        (Conversion::ToI32, HostValue::Bool(x)) => HostValue::I32(i32::from(x)),
        _ => return None,
    })
}

/// Returns the identity of a `min` or `max` accumulator of `value_type`: the largest value for `min`, the smallest for
/// `max`. A float takes its largest finite value, which WGSL can write.
pub(crate) fn extreme_of(value_type: &ValueType, largest: bool) -> Option<HostValue> {
    Some(match (value_type, largest) {
        (ValueType::U32, true) => HostValue::U32(u32::MAX),
        (ValueType::U32, false) => HostValue::U32(0),
        (ValueType::I32, true) => HostValue::I32(i32::MAX),
        (ValueType::I32, false) => HostValue::I32(i32::MIN),
        (ValueType::F32, true) => HostValue::F32(f32::MAX),
        (ValueType::F32, false) => HostValue::F32(f32::MIN),
        (ValueType::Vec2U, true) => HostValue::Vec2U([u32::MAX; 2]),
        (ValueType::Vec2U, false) => HostValue::Vec2U([0; 2]),
        (ValueType::Vec2I, true) => HostValue::Vec2I([i32::MAX; 2]),
        (ValueType::Vec2I, false) => HostValue::Vec2I([i32::MIN; 2]),
        (ValueType::Vec2F, true) => HostValue::Vec2F([f32::MAX; 2]),
        (ValueType::Vec2F, false) => HostValue::Vec2F([f32::MIN; 2]),
        (ValueType::U64, true) => HostValue::U64(u64::MAX),
        (ValueType::U64, false) => HostValue::U64(0),
        (ValueType::F64, true) => HostValue::F64(f64::MAX),
        (ValueType::F64, false) => HostValue::F64(f64::MIN),
        _ => return None,
    })
}
