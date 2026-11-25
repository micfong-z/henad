//! A compute shader held as a value: its WGSL source and its `@group(0)` declarations.
//!
//! The GPU specs in [`super::grid_spec`] and [`super::agent_spec`] hold one per pass. A compiled model's shader borrows
//! its strings from the model's consts, and a shader composed at run time owns them.

use std::borrow::Cow;
use std::fmt;

use henad_core::authoring::model::binding::{BindingDecl, BindingKind, named_buffer_target};

/// One `@group(0) @binding(n)` declaration, whose position in [`ShaderSpec::bindings`] is `n`.
///
/// A [`BindingDecl`] names its binding for `'static`. This form also holds a name built at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShaderBinding {
    /// Variable name the shader gives the binding, which resolves it as the
    /// [`binding`](henad_core::authoring::model::binding) module describes.
    pub name: Cow<'static, str>,
    /// Kind of resource it binds.
    pub kind: BindingKind,
}

impl ShaderBinding {
    /// Creates the declaration of binding `name`.
    pub fn new(name: impl Into<Cow<'static, str>>, kind: BindingKind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }

    /// Returns the buffer label the binding refers to and whether the pass writes it, `None` for a reserved name.
    pub(crate) fn buffer_target(&self) -> Option<(&str, bool)> {
        named_buffer_target(&self.name, self.kind)
    }
}

impl From<&BindingDecl> for ShaderBinding {
    fn from(decl: &BindingDecl) -> Self {
        Self::new(decl.name, decl.kind)
    }
}

/// A compute shader's WGSL source, with its `@group(0)` declarations in `@binding` order.
#[derive(Clone)]
#[non_exhaustive]
pub struct ShaderSpec {
    /// WGSL source, with `main` as the entry point.
    pub source: Cow<'static, str>,
    /// Declarations of the shader's bindings.
    pub bindings: Vec<ShaderBinding>,
}

impl ShaderSpec {
    /// Creates a shader from its source and its declarations.
    pub fn new(source: impl Into<Cow<'static, str>>, bindings: Vec<ShaderBinding>) -> Self {
        Self {
            source: source.into(),
            bindings,
        }
    }

    /// Returns a shader whose source and binding names are a model's consts.
    pub(crate) fn borrowed(source: &'static str, bindings: &'static [BindingDecl]) -> Self {
        Self::new(source, bindings.iter().map(ShaderBinding::from).collect())
    }
}

/// Prints the source's length instead of the source.
impl fmt::Debug for ShaderSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShaderSpec")
            .field("source_len", &self.source.len())
            .field("bindings", &self.bindings)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::ShaderSpec;

    #[test]
    fn debug_marks_the_source_it_leaves_out() {
        let printed = format!("{:?}", ShaderSpec::new("fn main() {}", Vec::new()));
        assert_eq!(printed, "ShaderSpec { source_len: 12, bindings: [], .. }");
    }
}
