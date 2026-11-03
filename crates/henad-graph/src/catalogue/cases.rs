//! Known-answer cases of a node kind.
//!
//! A *node case* gives one node's configuration, inputs and context, and its expected outputs. A *pass case* holds a
//! small graph with one pass, the lanes before it runs, and what the pass leaves, for a kind that needs other elements
//! or a body. Both backends run the same cases.

use crate::types::ValueType;

/// One known answer of a kind.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct KnownAnswer {
    /// Short name of the case, unique within the kind.
    pub name: &'static str,
    /// Backends the answer holds on.
    pub scope: CaseScope,
    /// The case itself.
    pub form: CaseForm,
}

/// Backends a known answer holds on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaseScope {
    /// The host and WGSL.
    Both,
    /// The host alone. WGSL leaves the answer indeterminate, since a shader may assume no NaN and no infinity.
    Host,
}

/// Shape of a known-answer case.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum CaseForm {
    /// One node.
    Node(NodeCase),
    /// One pass over a small grid.
    Pass(PassCase),
}

/// One node's inputs and context, and its expected outputs.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct NodeCase {
    /// Lines of the node's table that set its configuration, such as `config.max_exponent = 8`.
    pub config: &'static str,
    /// Input values by port name.
    pub inputs: &'static [(&'static str, CaseValue)],
    /// Values the node's context supplies.
    pub context: CaseContext,
    /// Expected output values by port name.
    pub outputs: &'static [(&'static str, CaseValue)],
    /// Expected state of the serial stream after the node runs, for a kind that draws from it.
    pub stream_after: Option<u64>,
}

/// Values a node's context supplies in a [`NodeCase`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct CaseContext {
    /// Run seed.
    pub seed: Option<u64>,
    /// Phase word of the stage, which the key word of a draw folds in.
    pub phase: u32,
    /// Element index.
    pub element: u32,
    /// Tick count.
    pub tick: u32,
    /// Region iteration word, `None` outside a region.
    pub iteration: Option<u32>,
    /// Number of the press in an action, from 1.
    pub press: u32,
    /// State of the serial stream before the node runs.
    pub stream: u64,
}

impl CaseContext {
    /// Context of a case that reads none of it.
    pub const NONE: Self = Self {
        seed: None,
        phase: 0,
        element: 0,
        tick: 0,
        iteration: None,
        press: 1,
        stream: 0,
    };
}

/// One pass over a small grid and what it leaves.
///
/// The case file is a grid header of the given size, then [`Self::tables`]. The case runs the pass with the id `case`.
/// The tables can hold other passes for the file to check, and the case does not run them.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct PassCase {
    /// Columns of the grid.
    pub width: u32,
    /// Rows of the grid.
    pub height: u32,
    /// Whether the grid's boundary is bounded, where a stencil sees nothing past the edge, in place of a torus.
    pub bounded: bool,
    /// Tables after the world table: declarations and passes.
    pub tables: &'static str,
    /// Lane values before the pass, in row-major order, by lane id. A lane left out holds its initial value.
    pub before: &'static [(&'static str, &'static [CaseValue])],
    /// Lane values after the pass, in row-major order, by lane id.
    pub after: &'static [(&'static str, &'static [CaseValue])],
    /// Expected tick-rate outputs by port reference such as `n2.S`, for a stats pass.
    pub stats: &'static [(&'static str, CaseValue)],
    /// Expected palette index per element, for a view pass.
    pub colors: &'static [u32],
}

impl PassCase {
    /// A pass case with nothing set, for a `static` table to fill.
    pub(crate) const EMPTY: Self = Self {
        width: 1,
        height: 1,
        bounded: false,
        tables: "",
        before: &[],
        after: &[],
        stats: &[],
        colors: &[],
    };
}

/// A value of a known answer.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CaseValue {
    /// A `bool`.
    Bool(bool),
    /// A `u32`.
    U32(u32),
    /// An `i32`.
    I32(i32),
    /// An `f32`, compared bit for bit.
    F32(f32),
    /// A `u64`.
    U64(u64),
    /// An `f64`, compared bit for bit.
    F64(f64),
    /// A `vec2u`.
    Vec2U([u32; 2]),
    /// A `vec2i`.
    Vec2I([i32; 2]),
    /// A `vec2f`, compared bit for bit.
    Vec2F([f32; 2]),
    /// The variant `.1` of the category `.0`.
    Variant(&'static str, &'static str),
    /// An `f32` that is any NaN.
    Nan,
    /// An `f32` within `.1` of `.0`.
    Near(f32, f32),
}

impl CaseValue {
    /// Returns the type of the value.
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::U32(_) => ValueType::U32,
            Self::I32(_) => ValueType::I32,
            Self::F32(_) | Self::Nan | Self::Near(..) => ValueType::F32,
            Self::U64(_) => ValueType::U64,
            Self::F64(_) => ValueType::F64,
            Self::Vec2U(_) => ValueType::Vec2U,
            Self::Vec2I(_) => ValueType::Vec2I,
            Self::Vec2F(_) => ValueType::Vec2F,
            Self::Variant(category, _) => ValueType::Category((*category).to_owned()),
        }
    }
}

/// Returns a node case on both backends with no configuration and no context.
pub(crate) const fn case(
    name: &'static str,
    inputs: &'static [(&'static str, CaseValue)],
    outputs: &'static [(&'static str, CaseValue)],
) -> KnownAnswer {
    full_case(name, CaseScope::Both, "", inputs, CaseContext::NONE, outputs)
}

/// Returns a node case on the host alone, with no configuration and no context.
pub(crate) const fn host_case(
    name: &'static str,
    inputs: &'static [(&'static str, CaseValue)],
    outputs: &'static [(&'static str, CaseValue)],
) -> KnownAnswer {
    full_case(name, CaseScope::Host, "", inputs, CaseContext::NONE, outputs)
}

/// Returns a node case on both backends with `config` lines.
pub(crate) const fn config_case(
    name: &'static str,
    config: &'static str,
    inputs: &'static [(&'static str, CaseValue)],
    outputs: &'static [(&'static str, CaseValue)],
) -> KnownAnswer {
    full_case(name, CaseScope::Both, config, inputs, CaseContext::NONE, outputs)
}

/// Returns a node case on the host alone, with `config` lines.
pub(crate) const fn host_case_with_config(
    name: &'static str,
    config: &'static str,
    inputs: &'static [(&'static str, CaseValue)],
    outputs: &'static [(&'static str, CaseValue)],
) -> KnownAnswer {
    full_case(name, CaseScope::Host, config, inputs, CaseContext::NONE, outputs)
}

/// Returns a node case with every field given.
pub(crate) const fn full_case(
    name: &'static str,
    scope: CaseScope,
    config: &'static str,
    inputs: &'static [(&'static str, CaseValue)],
    context: CaseContext,
    outputs: &'static [(&'static str, CaseValue)],
) -> KnownAnswer {
    KnownAnswer {
        name,
        scope,
        form: CaseForm::Node(NodeCase {
            config,
            inputs,
            context,
            outputs,
            stream_after: None,
        }),
    }
}

/// Returns a pass case on both backends.
pub(crate) const fn pass_case(name: &'static str, case: PassCase) -> KnownAnswer {
    KnownAnswer {
        name,
        scope: CaseScope::Both,
        form: CaseForm::Pass(case),
    }
}
