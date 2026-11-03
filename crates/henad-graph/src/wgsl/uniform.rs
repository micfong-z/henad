//! Uniform blocks of generated shaders: the fields a shader reads, their layout, and the bytes the host writes.
//!
//! A *field* is one member of a shader's `HenadParams` struct. Its value comes from the host: the grid's size, a value
//! of the pass's prelude that the host evaluates, a draw's key word, the press count, a table of powers, or whether a
//! pass bound to a parameter runs. Members follow WGSL's layout rules for the uniform address space: a scalar takes 4
//! bytes, a `vec2` takes 8 bytes at an 8-byte offset, and a table takes 16 bytes per four entries at a 16-byte offset.

use crate::draw::key_word;
use crate::host::compile::Program;
use crate::host::value::HostValue;
use crate::host::{HostEvaluator, HostWorld, RunContext, run};

/// Type of a value a generated shader computes or reads, as WGSL holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WgslType {
    Bool,
    U32,
    I32,
    F32,
    Vec2U,
    Vec2I,
    Vec2F,
}

impl WgslType {
    /// Returns the type that holds `value_type` in a shader, a category as its variant number, or `None` for a type
    /// only the host holds.
    pub(crate) fn of(value_type: &crate::types::ValueType) -> Option<Self> {
        use crate::types::ValueType;
        Some(match value_type {
            ValueType::Bool => Self::Bool,
            ValueType::U32 | ValueType::Category(_) => Self::U32,
            ValueType::I32 => Self::I32,
            ValueType::F32 => Self::F32,
            ValueType::Vec2U => Self::Vec2U,
            ValueType::Vec2I => Self::Vec2I,
            ValueType::Vec2F => Self::Vec2F,
            _ => return None,
        })
    }

    /// Returns the type of `value`, or `None` for a value only the host holds.
    pub(crate) fn of_value(value: HostValue) -> Option<Self> {
        Some(match value {
            HostValue::Bool(_) => Self::Bool,
            HostValue::U32(_) | HostValue::Category(_) => Self::U32,
            HostValue::I32(_) => Self::I32,
            HostValue::F32(_) => Self::F32,
            HostValue::Vec2U(_) => Self::Vec2U,
            HostValue::Vec2I(_) => Self::Vec2I,
            HostValue::Vec2F(_) => Self::Vec2F,
            _ => return None,
        })
    }

    /// Returns the WGSL name of the type.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U32 => "u32",
            Self::I32 => "i32",
            Self::F32 => "f32",
            Self::Vec2U => "vec2<u32>",
            Self::Vec2I => "vec2<i32>",
            Self::Vec2F => "vec2<f32>",
        }
    }

    /// Returns the WGSL name of the type a buffer or a uniform member stores it as: a `bool` as a `u32`.
    pub(crate) fn stored(self) -> &'static str {
        match self {
            Self::Bool => "u32",
            other => other.name(),
        }
    }

    /// Returns whether the type is a vector.
    pub(crate) fn is_vector(self) -> bool {
        matches!(self, Self::Vec2U | Self::Vec2I | Self::Vec2F)
    }

    /// Returns whether values of the type have an order that the comparison kinds read.
    pub(crate) fn is_ordered(self) -> bool {
        matches!(self, Self::U32 | Self::I32 | Self::F32)
    }
}

/// Where the value of a uniform field comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldSource {
    /// Columns of the grid.
    Width,
    /// Rows of the grid.
    Height,
    /// A slot of the prelude of the pass at a document position, which the host evaluates.
    Slot { position: usize, slot: usize },
    /// The key word of draw number `draw` of the pass at a document position.
    Key { position: usize, draw: usize },
    /// The number of the press, the tick word of a draw in an action.
    Press,
    /// Powers `base^0` to `base^max` of the prelude slot `base` of the pass at a document position.
    Table { position: usize, base: usize, max: u32 },
    /// Whether the pass at a document position runs, 1 or 0.
    Enabled { position: usize },
}

/// Shape of a uniform field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldShape {
    /// One value of this type.
    Value(WgslType),
    /// `len` `f32` entries packed four to a `vec4<f32>`.
    Table { len: u32 },
}

impl FieldShape {
    /// Returns the alignment and the size of the member in bytes.
    fn align_size(self) -> (usize, usize) {
        match self {
            Self::Value(value) if value.is_vector() => (8, 8),
            Self::Value(_) => (4, 4),
            Self::Table { len } => (16, 16 * (len as usize).div_ceil(4)),
        }
    }

    /// Returns the WGSL type of the member.
    pub(crate) fn wgsl(self) -> String {
        match self {
            Self::Value(value) => value.stored().to_owned(),
            Self::Table { len } => format!("array<vec4<f32>, {}>", len.div_ceil(4)),
        }
    }
}

/// One member of a shader's uniform block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UniformField {
    /// Name of the member.
    pub name: String,
    /// Shape of the member.
    pub shape: FieldShape,
    /// Where its value comes from.
    pub source: FieldSource,
    /// Byte offset of the member in the block.
    pub offset: usize,
}

/// Uniform block of one shader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UniformBlock {
    /// Members in offset order.
    pub fields: Vec<UniformField>,
    /// Size of the block in bytes, a multiple of 16.
    pub size: usize,
}

impl UniformBlock {
    /// Returns the block of `fields`, laid out in order with the tables last.
    pub(crate) fn new(mut fields: Vec<(String, FieldShape, FieldSource)>) -> Self {
        // Tables last, so no member follows an array. Vectors before them keep the scalars packed.
        fields.sort_by_key(|(_, shape, _)| match shape {
            FieldShape::Value(value) if value.is_vector() => 1,
            FieldShape::Value(_) => 0,
            FieldShape::Table { .. } => 2,
        });
        let mut offset: usize = 0;
        let mut laid = Vec::with_capacity(fields.len());
        for (name, shape, source) in fields {
            let (align, size) = shape.align_size();
            offset = offset.next_multiple_of(align);
            laid.push(UniformField {
                name,
                shape,
                source,
                offset,
            });
            offset += size;
        }
        Self {
            fields: laid,
            size: offset.next_multiple_of(16).max(16),
        }
    }

    /// Returns the WGSL declaration of the struct `HenadParams`.
    pub(crate) fn declaration(&self) -> String {
        let mut text = String::from("struct HenadParams {\n");
        for field in &self.fields {
            text.push_str(&format!("    {}: {},\n", field.name, field.shape.wgsl()));
        }
        text.push_str("}\n");
        text
    }

    /// Returns the bytes of the block for a grid of `width` by `height` with the document's parameters `params`, in
    /// `context`.
    ///
    /// # Errors
    ///
    /// Returns the fault of a prelude the host cannot evaluate.
    pub(crate) fn bytes(
        &self,
        evaluator: &HostEvaluator,
        world: HostWorld,
        params: &[HostValue],
        context: &RunContext,
    ) -> Result<Vec<u8>, String> {
        let (width, height) = match world {
            HostWorld::Grid { width, height, .. } => (width, height),
            HostWorld::Agents { count } => (count, 1),
        };
        let mut frames: Vec<(usize, Vec<HostValue>)> = Vec::new();
        let mut bytes = vec![0u8; self.size];
        for field in &self.fields {
            let words: Vec<u32> = match &field.source {
                FieldSource::Width => vec![width],
                FieldSource::Height => vec![height],
                FieldSource::Press => vec![context.press],
                FieldSource::Enabled { position } => vec![u32::from(evaluator.is_enabled(*position, params))],
                FieldSource::Key { position, draw } => {
                    let key = program_of(evaluator, *position)?
                        .draw_keys
                        .get(*draw)
                        .copied()
                        .unwrap_or_default();
                    vec![key_word(key, context.phase, context.seed)]
                }
                FieldSource::Slot { position, slot } => {
                    let frame = frame_of(&mut frames, evaluator, *position, world, params, context)?;
                    value_words(frame.get(*slot).copied().unwrap_or(HostValue::U32(0)))
                }
                FieldSource::Table { position, base, max } => {
                    let frame = frame_of(&mut frames, evaluator, *position, world, params, context)?;
                    let base = frame.get(*base).and_then(|value| value.as_f32()).unwrap_or(0.0);
                    (0..=*max)
                        .map(|exponent| base.powi(i32::try_from(exponent).unwrap_or(i32::MAX)).to_bits())
                        .collect()
                }
            };
            for (k, word) in words.iter().enumerate() {
                let start = field.offset + 4 * k;
                if let Some(target) = bytes.get_mut(start..start + 4) {
                    target.copy_from_slice(&word.to_le_bytes());
                }
            }
        }
        Ok(bytes)
    }
}

/// Returns the program of the pass at `position`.
fn program_of(evaluator: &HostEvaluator, position: usize) -> Result<&Program, String> {
    match evaluator.program_at(position) {
        Some(Ok(program)) => Ok(program),
        Some(Err(error)) => Err(error.to_string()),
        None => Err(format!("the file has no pass at position {position}")),
    }
}

/// Returns the prelude frame of the pass at `position`, evaluating it once per block.
fn frame_of<'a>(
    frames: &'a mut Vec<(usize, Vec<HostValue>)>,
    evaluator: &HostEvaluator,
    position: usize,
    world: HostWorld,
    params: &[HostValue],
    context: &RunContext,
) -> Result<&'a [HostValue], String> {
    let index = if let Some(index) = frames.iter().position(|(known, _)| *known == position) {
        index
    } else {
        {
            let program = program_of(evaluator, position)?;
            let frame = run::prelude(program, params, context, world).map_err(|error| error.to_string())?;
            frames.push((position, frame));
            frames.len() - 1
        }
    };
    Ok(&frames[index].1)
}

/// Returns the words a uniform member holds for `value`: a `bool` as 0 or 1, a vector as two words.
fn value_words(value: HostValue) -> Vec<u32> {
    match value {
        HostValue::Bool(flag) => vec![u32::from(flag)],
        HostValue::U32(word) | HostValue::Category(word) => vec![word],
        HostValue::I32(word) => vec![word as u32],
        HostValue::F32(word) => vec![word.to_bits()],
        HostValue::Vec2U([x, y]) => vec![x, y],
        HostValue::Vec2I([x, y]) => vec![x as u32, y as u32],
        HostValue::Vec2F([x, y]) => vec![x.to_bits(), y.to_bits()],
        HostValue::U64(_) | HostValue::F64(_) | HostValue::Vec2D(_) => vec![0],
    }
}
