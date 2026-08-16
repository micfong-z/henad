//! What a shader declares at `@group(0)`, extracted from the WGSL at build time.
//!
//! A pass no longer says which resource goes in which slot. The engine reads these declarations in
//! `@binding` order and resolves each `name` itself, so the slot index cannot disagree with the
//! shader that owns it.
//!
//! # Names
//!
//! Seven names are reserved for resources the engine owns.
//!
//! | name | resource |
//! |---|---|
//! | `params` | the pass's own uniform block |
//! | `dims` | grid and display texture size, for a grid model |
//! | `output` | the display texture |
//! | `cell_start`, `sorted` | the neighbour index |
//! | `counters` | the persistent counters, and a grid model's reduce totals |
//! | `partials` | the reduction's leaf output |
//!
//! Anything else names one of the model's own buffers by its [`BufferSpec`] label, with an optional
//! `_in` or `_out` suffix. Which side it resolves to comes from the access mode, not the suffix, so
//! a buffer read by one pass and written by another needs no naming trick.
//!
//! [`BufferSpec`]: crate::authoring::gpu_agent_model::BufferSpec

/// How a binding is declared, which is what the engine needs to build a layout entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Storage { read_only: bool },
    Uniform,
    StorageTexture,
}

impl BindingKind {
    /// Whether this counts against `max_storage_buffers_per_shader_stage`. A uniform and a storage
    /// texture each count against their own limit, not this one.
    pub fn is_storage_buffer(self) -> bool {
        matches!(self, Self::Storage { .. })
    }
}

/// One `@group(0) @binding(n)` declaration. Position in a pass's slice is `n`.
#[derive(Clone, Copy, Debug)]
pub struct BindingDecl {
    pub name: &'static str,
    pub kind: BindingKind,
}

/// The buffer label `name` refers to, and whether the pass writes it.
///
/// `None` for a reserved name, which the engine resolves without consulting the model's buffers.
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

/// Names the engine answers itself. A model cannot label a buffer with one of these.
pub const RESERVED: &[&str] = &[
    "params",
    "dims",
    "output",
    "cell_start",
    "sorted",
    "counters",
    "partials",
];
