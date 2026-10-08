//! The files a shader build writes to `OUT_DIR`, each only when its bytes change.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::primitives::wgsl::{SHARED_WGSL_FNV1A64, SHARED_WGSL_MODULES};
use henad_core::explore::fingerprint::Fnv1a64;
use wgsl_bindgen::bevy_util::{DependencyTree, DependencyTreeError};
use wgsl_bindgen::{
    AdditionalScanDirectory, RustWgslTypeMap, SourceFilePath, WgslBindgenOptionBuilder, WgslShaderSourceType,
    WgslTypeSerializeStrategy,
};

use crate::binding_lines::read_bindings;
use crate::paths::{components, constant_name};
use crate::{ShaderBuildError, binding_lines, paths};

/// Directory under `OUT_DIR` that holds the shared modules, below it at `henad/<module>.wgsl`.
const SHARED_DIRECTORY: &str = "henad_wgsl";
/// File that holds the hash of the inputs the last `wgsl_bindgen` pass read.
const STAMP: &str = "shader_bindings.stamp";
/// Release of `wgsl_bindgen` that the workspace pins. A new release regenerates every binding.
///
/// `scripts/check_packaging.sh` checks that it equals the pin.
const WGSL_BINDGEN_VERSION: &str = "0.23.3";
/// Source of this file, which sets every option of the `wgsl_bindgen` pass. An edit to it regenerates every binding.
const GENERATOR_SOURCE: &str = include_str!("output.rs");

/// Result of one generation.
#[derive(Debug)]
pub(crate) struct Generated {
    /// Files outside the shader root that the entry points import, for Cargo to watch.
    pub(crate) outside_root: Vec<PathBuf>,
    /// Whether the `wgsl_bindgen` pass ran. A stamp that matches skips it.
    pub(crate) bound: bool,
}

/// Writes the shared modules, `shader_bindings.rs` and `binding_decls.rs` to `out_dir`.
///
/// `entries` are relative to `root`, and `sources` holds every `.wgsl` file under it with its text. The
/// `wgsl_bindgen` pass is skipped when the hash of its inputs matches the stamp the last pass left. The inputs cover
/// every file that an entry point reaches through its imports, including a file outside the root or without the
/// `.wgsl` extension.
pub(crate) fn generate(
    root: &Path,
    entries: &[PathBuf],
    sources: &[(PathBuf, String)],
    out_dir: &Path,
) -> Result<Generated, ShaderBuildError> {
    let shared = out_dir.join(SHARED_DIRECTORY);
    write_shared_modules(&shared)?;

    let bindings_path = out_dir.join("shader_bindings.rs");
    let stamp_path = out_dir.join(STAMP);
    let mut generated = Generated {
        outside_root: Vec::new(),
        bound: false,
    };
    if entries.is_empty() {
        // `wgsl_bindgen` rejects an empty list, and its output for an empty list would not compile.
        write_if_changed(&bindings_path, "")?;
        // A stamp left by shaders that were since removed would match those shaders when they are restored. The pass
        // would then be skipped and leave the empty bindings.
        if stamp_path.exists() {
            std::fs::remove_file(&stamp_path).map_err(|source| ShaderBuildError::Io {
                path: stamp_path.clone(),
                source,
            })?;
        }
    } else {
        let imported = imported_files(root, entries, sources, &shared)?;
        let stamp = format!("{:016x}\n", input_hash(root, entries, sources, &imported));
        let current = std::fs::read_to_string(&stamp_path).is_ok_and(|text| text == stamp);
        if !current || !bindings_path.is_file() {
            write_if_changed(&bindings_path, &bind(root, entries, &shared)?)?;
            write_if_changed(&stamp_path, &stamp)?;
            generated.bound = true;
        }
        let normal_root = normalize(root);
        generated.outside_root = imported
            .into_iter()
            .map(|(path, _)| path)
            .filter(|path| !path.starts_with(&normal_root))
            .collect();
    }

    write_if_changed(
        &out_dir.join("binding_decls.rs"),
        &binding_decls(root, entries, sources)?,
    )?;
    Ok(generated)
}

/// Writes the shared modules under `shared`, and removes any `.wgsl` file there that matches no shared module.
///
/// A copy of a module that henad-core has since renamed or removed would otherwise stay importable.
fn write_shared_modules(shared: &Path) -> Result<(), ShaderBuildError> {
    let current: Vec<PathBuf> = SHARED_WGSL_MODULES
        .iter()
        .map(|module| {
            shared
                .join(module.import_path.replace("::", "/"))
                .with_extension("wgsl")
        })
        .collect();
    if shared.is_dir() {
        let mut pending = vec![shared.to_path_buf()];
        while let Some(directory) = pending.pop() {
            let io_error = |source| ShaderBuildError::Io {
                path: directory.clone(),
                source,
            };
            for item in std::fs::read_dir(&directory).map_err(io_error)? {
                let path = item.map_err(io_error)?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "wgsl") && !current.contains(&path) {
                    std::fs::remove_file(&path).map_err(|source| ShaderBuildError::Io { path, source })?;
                }
            }
        }
    }
    for (path, module) in current.iter().zip(SHARED_WGSL_MODULES) {
        write_if_changed(path, module.source)?;
    }
    Ok(())
}

/// Returns the files that `wgsl_bindgen` reads for `entries` and that `sources` does not hold, each with its text: a
/// file outside the root, or a file without the `.wgsl` extension. Each path is absolute, with no `.` or `..`
/// component.
///
/// # Errors
///
/// Returns [`ShaderBuildError::Compose`] for an import that does not resolve, [`ShaderBuildError::ModuleBinding`] for
/// an imported file that declares a binding and is not an entry point, and [`ShaderBuildError::ReservedImport`] for a
/// module whose import path makes it `henad` or a name the generated bindings use.
fn imported_files(
    root: &Path,
    entries: &[PathBuf],
    sources: &[(PathBuf, String)],
    shared: &Path,
) -> Result<Vec<(PathBuf, String)>, ShaderBuildError> {
    let entry_paths: Vec<SourceFilePath> = entries
        .iter()
        .map(|entry| SourceFilePath::new(root.join(entry)))
        .collect();
    let scan = AdditionalScanDirectory {
        module_import_root: None,
        directory: shared.to_string_lossy().into_owned(),
    };
    let tree = catch_composer_panic(|| {
        DependencyTree::try_build(root.to_path_buf(), None, entry_paths, vec![scan]).map_err(|error| {
            let message = match &error {
                DependencyTreeError::ImportPathNotFound { src, .. } => format!("{}: {error}", src.name()),
                DependencyTreeError::SourceNotFound { .. } => error.to_string(),
            };
            ShaderBuildError::Compose { message }
        })
    })?;

    refuse_cycles(&tree)?;

    let (normal_root, normal_shared) = (normalize(root), normalize(shared));
    let held: BTreeSet<&Path> = sources.iter().map(|(path, _)| path.as_path()).collect();
    let mut imported = Vec::new();
    for file in tree.parsed_files() {
        let path = normalize(&file.file_path);
        if path.starts_with(&normal_shared) {
            continue;
        }
        let relative = path.strip_prefix(&normal_root).ok();
        if !relative.is_some_and(|relative| entries.iter().any(|entry| entry == relative)) {
            binding_lines::refuse_bindings(&path, &file.content)?;
        }
        if let Some(module) = &file.module_name {
            paths::check_module_name(&path, module)?;
        }
        if !relative.is_some_and(|relative| held.contains(relative)) {
            imported.push((path, file.content.clone()));
        }
    }
    imported.sort();
    Ok(imported)
}

/// Returns an error that lists the files of an import cycle in `tree`.
///
/// `wgsl_bindgen` follows a cycle until the stack overflows. The build script then aborts with no message.
fn refuse_cycles(tree: &DependencyTree) -> Result<(), ShaderBuildError> {
    let files = tree.parsed_files();
    let index_of = |path: &SourceFilePath| files.iter().position(|file| file.file_path == *path);
    /// State of one file in the walk.
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        /// Not reached yet.
        Unseen,
        /// On the path the walk follows, at this depth.
        OnPath(usize),
        /// Walked, with every file it imports.
        Done,
    }
    let mut marks = vec![Mark::Unseen; files.len()];
    for start in 0..files.len() {
        if marks[start] != Mark::Unseen {
            continue;
        }
        // The files on the path, each with the position of its next import to follow.
        let mut path = vec![(start, 0)];
        marks[start] = Mark::OnPath(0);
        while let Some(top) = path.last_mut() {
            let (file, next) = *top;
            top.1 += 1;
            let Some(dependency) = files[file].direct_dependencies.get_index(next) else {
                marks[file] = Mark::Done;
                path.pop();
                continue;
            };
            let Some(dependency) = index_of(dependency) else {
                continue;
            };
            match marks[dependency] {
                Mark::Unseen => {
                    marks[dependency] = Mark::OnPath(path.len());
                    path.push((dependency, 0));
                }
                Mark::OnPath(depth) => {
                    let cycle: Vec<String> = path[depth..]
                        .iter()
                        .chain(std::iter::once(&(dependency, 0)))
                        .map(|&(member, _)| normalize(&files[member].file_path).display().to_string())
                        .collect();
                    return Err(ShaderBuildError::Compose {
                        message: format!("the imports form a cycle: {}", cycle.join(" imports ")),
                    });
                }
                Mark::Done => {}
            }
        }
    }
    Ok(())
}

/// Returns the result of `compose`, or [`ShaderBuildError::Compose`] with its message when it panics.
///
/// `wgsl_bindgen` panics on an import inside an imported module that does not resolve, and on an import it cannot
/// parse. The panic hook still prints the panic before the error is returned.
fn catch_composer_panic<T>(compose: impl FnOnce() -> Result<T, ShaderBuildError>) -> Result<T, ShaderBuildError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(compose)).unwrap_or_else(|payload| {
        let text = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_owned()))
            .unwrap_or_else(|| "`wgsl_bindgen` panicked".to_owned());
        let message = missing_import(&text).unwrap_or(text);
        Err(ShaderBuildError::Compose { message })
    })
}

/// Returns a message with the file and the import for a panic caused by an import that does not resolve, or `None`
/// for any other panic.
///
/// The file and the import are read from the `Debug` text of the error that the panic carries.
fn missing_import(text: &str) -> Option<String> {
    let quoted = |marker: &str| {
        let start = text.find(marker)? + marker.len();
        text[start..].split('"').next()
    };
    let import = quoted("ImportPathNotFound { path: \"")?;
    let file = quoted("NamedSource { name: \"")?;
    Some(format!("{file}: Cannot find import `{import}` in this scope"))
}

/// Returns `path` with each `.` component dropped and each `..` component applied to the one before it, without
/// reading the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(normal.components().next_back(), Some(Component::Normal(_))) => {
                normal.pop();
            }
            _ => normal.push(component),
        }
    }
    normal
}

/// Returns the bindings `wgsl_bindgen` generates for `entries`, composed against the shared modules in `shared`.
fn bind(root: &Path, entries: &[PathBuf], shared: &Path) -> Result<String, ShaderBuildError> {
    let compose = |error: wgsl_bindgen::WgslBindgenError| ShaderBuildError::Compose {
        message: error.to_string(),
    };
    let mut builder = WgslBindgenOptionBuilder::default();
    builder
        .workspace_root(root)
        .additional_scan_dir((None, shared.to_string_lossy().as_ref()))
        // This crate prints every rerun line itself. With this option on, the pass would print its own rerun lines in
        // the double-colon form of Cargo 1.77, including lines for the copies of the shared modules under `OUT_DIR`.
        .emit_rerun_if_change(false)
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(RustWgslTypeMap)
        .shader_source_type(WgslShaderSourceType::EmbedSource);
    for entry in entries {
        builder.add_entry_point(root.join(entry).to_string_lossy().into_owned());
    }
    catch_composer_panic(|| builder.build().map_err(compose)?.generate_string().map_err(compose))
}

/// Returns the hash of the `wgsl_bindgen` pass inputs: the henad-build and `wgsl_bindgen` releases, this file's
/// source, the root, the entry points, every `.wgsl` file under the root, every other file that the entry points reach
/// through their imports, and the shared modules.
fn input_hash(root: &Path, entries: &[PathBuf], sources: &[(PathBuf, String)], imported: &[(PathBuf, String)]) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write_str(env!("CARGO_PKG_VERSION"));
    hasher.write_str(WGSL_BINDGEN_VERSION);
    hasher.write_str(GENERATOR_SOURCE);
    hasher.write_u64(SHARED_WGSL_FNV1A64);
    hasher.write_str(&root.to_string_lossy());
    hasher.write_u64(entries.len() as u64);
    for entry in entries {
        hasher.write_str(&entry.to_string_lossy());
    }
    for files in [sources, imported] {
        hasher.write_u64(files.len() as u64);
        for (path, source) in files {
            hasher.write_str(&path.to_string_lossy());
            hasher.write_str(source);
        }
    }
    hasher.finish()
}

/// Returns the text of `binding_decls.rs`: the shared modules' hash, each entry's `@group(0)` declarations and the
/// assertions checking each list against the length of its generated layout.
fn binding_decls(root: &Path, entries: &[PathBuf], sources: &[(PathBuf, String)]) -> Result<String, ShaderBuildError> {
    let mut constants = String::new();
    let mut assertions = String::new();
    for entry in entries {
        let path = root.join(entry);
        let source = match sources.iter().find(|(file, _)| file == entry) {
            Some((_, source)) => source.clone(),
            None => std::fs::read_to_string(&path).map_err(|source| ShaderBuildError::Io {
                path: path.clone(),
                source,
            })?,
        };
        let bindings = read_bindings(&path, &source)?;
        let names = components(entry).unwrap_or_default();
        let constant = constant_name(&names);

        constants.push_str(&format!("    pub const {constant}: &[BindingDecl] = &[\n"));
        for binding in &bindings {
            let kind = match binding.kind {
                BindingKind::Storage { read_only } => format!("BindingKind::Storage {{ read_only: {read_only} }}"),
                BindingKind::Uniform => "BindingKind::Uniform".to_owned(),
                BindingKind::StorageTexture => "BindingKind::StorageTexture".to_owned(),
            };
            constants.push_str(&format!(
                "        BindingDecl {{ name: \"{}\", kind: {kind} }},\n",
                binding.name
            ));
        }
        constants.push_str("    ];\n");

        if !bindings.is_empty() {
            let module = names.join("::");
            // The path uses forward slashes on every platform. Writing the literal through `Debug` escapes any other
            // special characters.
            let message = format!(
                "henad-build read another number of @group(0) bindings from {} than naga composed",
                entry.to_string_lossy().replace('\\', "/")
            );
            assertions.push_str(&format!(
                "const _: () = ::core::assert!(\n    bindings::{constant}.len() == \
                 super::shader_bindings::{module}::WgpuBindGroup0::LAYOUT_DESCRIPTOR.entries.len(),\n    \
                 {message:?},\n);\n"
            ));
        }
    }

    let mut text = format!(
        "// Generated by henad-build {}. Changes made to this file will not be saved.\n\n\
         /// FNV-1a hash of the shared WGSL these shaders were composed against.\n\
         pub const SHARED_WGSL_FNV1A64: u64 = 0x{SHARED_WGSL_FNV1A64:016x};\n\n\
         /// Each shader's `@group(0)` declarations, in `@binding` order.\n\
         pub mod bindings {{\n",
        env!("CARGO_PKG_VERSION")
    );
    if !entries.is_empty() {
        text.push_str("    use super::{BindingDecl, BindingKind};\n\n");
    }
    text.push_str(&constants);
    text.push_str("}\n");
    if !assertions.is_empty() {
        text.push('\n');
        text.push_str(&assertions);
    }
    Ok(text)
}

/// Writes `contents` to `path`, creating its directory, unless the file already holds those bytes.
pub(crate) fn write_if_changed(path: &Path, contents: &str) -> Result<(), ShaderBuildError> {
    if std::fs::read(path).is_ok_and(|current| current == contents.as_bytes()) {
        return Ok(());
    }
    let io_error = |source| ShaderBuildError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    std::fs::write(path, contents).map_err(io_error)
}
