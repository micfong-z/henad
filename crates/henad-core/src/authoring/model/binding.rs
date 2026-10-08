//! Binding declarations a shader makes at `@group(0)`, read from the WGSL at build time.
//!
//! The engine reads a pass's declarations in `@binding` order and resolves each name itself. A name
//! in [`RESERVED`] is a resource the engine owns. Any other name is the [`BufferSpec`] label of one of the
//! model's own buffers, with an optional `_in` or `_out` suffix. The suffix is a naming convention, and the
//! access mode picks the side the name resolves to.
//!
//! [`BufferSpec`]: crate::authoring::model::gpu_agent_model::BufferSpec

/// Kind of resource a binding declares, as the engine needs it for a layout entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// A storage buffer, `var<storage, read>` when `read_only` and `var<storage, read_write>` otherwise.
    Storage {
        /// Whether the shader can only read the buffer.
        read_only: bool,
    },
    /// A uniform block, `var<uniform>`.
    Uniform,
    /// A storage texture, `texture_storage_2d`.
    StorageTexture,
}

impl BindingKind {
    /// Returns whether this counts against `max_storage_buffers_per_shader_stage`.
    ///
    /// A uniform and a storage texture each count against their own limit.
    pub fn is_storage_buffer(self) -> bool {
        matches!(self, Self::Storage { .. })
    }
}

/// One `@group(0) @binding(n)` declaration. Its position in a pass's slice is `n`.
#[derive(Clone, Copy, Debug)]
pub struct BindingDecl {
    /// Variable name the shader gives the binding.
    pub name: &'static str,
    /// Kind of resource it binds.
    pub kind: BindingKind,
}

/// Returns the buffer label that `decl` refers to, and whether the pass writes it.
///
/// Returns `None` for a reserved name, which the engine resolves without consulting the model's buffers.
pub fn buffer_target(decl: &BindingDecl) -> Option<(&'static str, bool)> {
    if RESERVED.contains(&decl.name) {
        return None;
    }
    let label = decl
        .name
        .strip_suffix("_in")
        .or_else(|| decl.name.strip_suffix("_out"))
        .unwrap_or(decl.name);
    let writes = !matches!(decl.kind, BindingKind::Storage { read_only: true });
    Some((label, writes))
}

/// Names that the engine resolves itself.
///
/// | Name | Resource |
/// |---|---|
/// | `params` | The pass's own uniform block |
/// | `dims` | Grid and display texture size, for a grid model |
/// | `output` | The display texture |
/// | `cell_start`, `sorted` | The neighbour index |
/// | `counters` | The persistent counters, and a grid model's reduce totals |
/// | `partials` | The reduction's leaf output |
///
/// A model cannot label a buffer with one of these names, or with a label ending in `_in` or `_out`. A binding
/// named after such a label would resolve to the engine's own resource, or to the label without its suffix. Both
/// engines panic at construction on a reserved or suffixed label.
pub const RESERVED: &[&str] = &[
    "params",
    "dims",
    "output",
    "cell_start",
    "sorted",
    "counters",
    "partials",
];
