//! The `.wgsl` files under a shader root, and the Rust names their paths become.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use heck::ToPascalCase as _;

use crate::ShaderBuildError;

/// Name reserved for the shared modules' import root, in any case.
const RESERVED: &str = "henad";

/// Names the generated bindings use at their root, beside the shader modules. A shader whose path starts with one
/// would shadow it.
const GENERATED_NAMES: &[&str] = &[
    "wgpu",
    "bytemuck",
    "std",
    "core",
    "alloc",
    "_root",
    "ShaderEntry",
    "layout_asserts",
    "bytemuck_impls",
];

/// Rust's strict and reserved keywords, in the 2024 edition.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
    "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract",
    "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// Returns every `.wgsl` file under `root`, relative to it, in sorted order.
///
/// # Errors
///
/// Returns [`ShaderBuildError::Io`] for a directory that cannot be read.
pub(crate) fn wgsl_files(root: &Path) -> Result<Vec<PathBuf>, ShaderBuildError> {
    let mut files = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let directory = root.join(&relative);
        let io_error = |source| ShaderBuildError::Io {
            path: directory.clone(),
            source,
        };
        for item in std::fs::read_dir(&directory).map_err(io_error)? {
            let item = item.map_err(io_error)?;
            let path = relative.join(item.file_name());
            if root.join(&path).is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "wgsl") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Returns whether `source` declares an import path, which makes its file a module and not an entry point.
pub(crate) fn is_module(source: &str) -> bool {
    source
        .lines()
        .any(|line| line.trim_start().starts_with("#define_import_path"))
}

/// Returns whether `source` declares an import path under the reserved root.
pub(crate) fn defines_reserved_path(source: &str) -> bool {
    source.lines().any(|line| {
        line.trim_start()
            .strip_prefix("#define_import_path")
            .and_then(|path| path.trim().split("::").next())
            .is_some_and(|root| root.eq_ignore_ascii_case(RESERVED))
    })
}

/// Returns an error when the last component of `root` is the reserved name, in any case.
pub(crate) fn check_root(root: &Path) -> Result<(), ShaderBuildError> {
    let absolute = std::path::absolute(root).map_err(|source| ShaderBuildError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if absolute.file_name().is_some_and(is_reserved) {
        return Err(ShaderBuildError::ReservedName {
            path: root.to_path_buf(),
            name: RESERVED.to_owned(),
        });
    }
    Ok(())
}

/// Returns an error for the first of `files` named `henad.wgsl` or lying in a directory named `henad`, in any case,
/// or whose path starts with a name the generated bindings use.
pub(crate) fn check_reserved(root: &Path, files: &[PathBuf]) -> Result<(), ShaderBuildError> {
    for file in files {
        let reserved = |name: &str| ShaderBuildError::ReservedName {
            path: root.join(file),
            name: name.to_owned(),
        };
        let reserved_directory = file
            .parent()
            .is_some_and(|parent| parent.components().any(|component| is_reserved(component.as_os_str())));
        let reserved_file = file.file_stem().is_some_and(is_reserved);
        if reserved_directory || reserved_file {
            return Err(reserved(RESERVED));
        }
        let first = file.with_extension("");
        let first = first
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str());
        if let Some(name) = first.filter(|name| GENERATED_NAMES.contains(name)) {
            return Err(reserved(name));
        }
    }
    Ok(())
}

/// Returns whether `name` is the reserved name, in any case.
fn is_reserved(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| name.eq_ignore_ascii_case(RESERVED))
}

/// Returns an error when a component of `file`, a `.wgsl` path relative to `root`, is no Rust identifier or is a
/// keyword.
pub(crate) fn check_components(root: &Path, file: &Path) -> Result<(), ShaderBuildError> {
    let invalid = |component: String| ShaderBuildError::InvalidName {
        path: root.join(file),
        component,
    };
    let names = components(file).ok_or_else(|| invalid(file.display().to_string()))?;
    match names.into_iter().find(|name| !is_identifier(name)) {
        Some(name) => Err(invalid(name)),
        None => Ok(()),
    }
}

/// Returns an error when two of `entries` give one module path, `ShaderEntry` variant or binding constant, or one
/// entry's module path holds another's.
pub(crate) fn check_collisions(root: &Path, entries: &[PathBuf]) -> Result<(), ShaderBuildError> {
    let mut names: BTreeMap<(&'static str, String), &PathBuf> = BTreeMap::new();
    // Modules that hold another entry's module, each with the first entry inside it.
    let mut parents: BTreeMap<String, &PathBuf> = BTreeMap::new();
    for entry in entries {
        let Some(components) = components(entry) else {
            continue;
        };
        let collision = |first: &PathBuf, kind, name| ShaderBuildError::NameCollision {
            first: root.join(first),
            second: root.join(entry),
            kind,
            name,
        };
        let module = components.join("::");
        for (kind, name) in [
            ("module", module.clone()),
            ("variant", variant_name(&components)),
            ("constant", constant_name(&components)),
        ] {
            if let Some(first) = names.insert((kind, name.clone()), entry) {
                return Err(collision(first, kind, name));
            }
        }
        if let Some(first) = parents.get(&module) {
            return Err(collision(first, "module", module));
        }
        for count in 1..components.len() {
            let parent = components[..count].join("::");
            if let Some(first) = names.get(&("module", parent.clone())) {
                return Err(collision(first, "module", parent));
            }
            parents.entry(parent).or_insert(entry);
        }
    }
    Ok(())
}

/// Returns the components of `file`, a `.wgsl` path, with the final `.wgsl` removed, or `None` for a component that
/// is not plain Unicode or a path that is not a normal relative one.
pub(crate) fn components(file: &Path) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for component in file.with_extension("").components() {
        match component {
            Component::Normal(name) => names.push(name.to_str()?.to_owned()),
            _ => return None,
        }
    }
    Some(names)
}

/// Name of the binding constant for an entry whose path has `components`, as in `GPU_VOTE_STEP`.
pub(crate) fn constant_name(components: &[String]) -> String {
    components
        .iter()
        .map(|component| component.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Name of the `ShaderEntry` variant `wgsl_bindgen` gives an entry whose path has `components`, as in `GpuVoteStep`.
fn variant_name(components: &[String]) -> String {
    components.join("_").to_pascal_case()
}

/// Returns whether `name` is an ASCII Rust identifier and no keyword.
fn is_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    let starts_well = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    starts_well
        && name != "_"
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !KEYWORDS.contains(&name)
}
