//! The files a shader build writes to `OUT_DIR`, each only when its bytes change.

use std::path::{Path, PathBuf};

use henad_core::authoring::model::binding::BindingKind;
use henad_core::authoring::primitives::wgsl::{SHARED_WGSL_FNV1A64, SHARED_WGSL_MODULES};
use henad_core::explore::fingerprint::Fnv1a64;
use wgsl_bindgen::{RustWgslTypeMap, WgslBindgenOptionBuilder, WgslShaderSourceType, WgslTypeSerializeStrategy};

use crate::ShaderBuildError;
use crate::binding_lines::read_bindings;
use crate::paths::{components, constant_name};

/// Directory under `OUT_DIR` that holds the shared modules, below it at `henad/<module>.wgsl`.
const SHARED_DIRECTORY: &str = "henad_wgsl";
/// File that holds the hash of the inputs the last `wgsl_bindgen` pass read.
const STAMP: &str = "shader_bindings.stamp";
/// Release of `wgsl_bindgen` the workspace's pin names. A new one regenerates every binding.
///
/// `scripts/check_packaging.sh` holds it equal to the pin.
const WGSL_BINDGEN_VERSION: &str = "0.23.3";
/// Source of this file, which sets every option of the `wgsl_bindgen` pass. An edit to it regenerates every binding.
const GENERATOR_SOURCE: &str = include_str!("output.rs");

/// Writes the shared modules, `shader_bindings.rs` and `binding_decls.rs` to `out_dir`.
///
/// `entries` are relative to `root`, and `sources` holds every `.wgsl` file under it with its text. The
/// `wgsl_bindgen` pass is skipped when the hash of its inputs matches the stamp the last pass left.
pub(crate) fn generate(
    root: &Path,
    entries: &[PathBuf],
    sources: &[(PathBuf, String)],
    out_dir: &Path,
) -> Result<(), ShaderBuildError> {
    let shared = out_dir.join(SHARED_DIRECTORY);
    for module in SHARED_WGSL_MODULES {
        let path = shared
            .join(module.import_path.replace("::", "/"))
            .with_extension("wgsl");
        write_if_changed(&path, module.source)?;
    }

    let bindings_path = out_dir.join("shader_bindings.rs");
    let stamp_path = out_dir.join(STAMP);
    if entries.is_empty() {
        // `wgsl_bindgen` refuses an empty list, and its output for one would not compile.
        write_if_changed(&bindings_path, "")?;
        // A stamp left from shaders since removed would match their return, and skip the pass over empty bindings.
        if stamp_path.exists() {
            std::fs::remove_file(&stamp_path).map_err(|source| ShaderBuildError::Io {
                path: stamp_path.clone(),
                source,
            })?;
        }
    } else {
        let stamp = format!("{:016x}\n", input_hash(root, entries, sources));
        let current = std::fs::read_to_string(&stamp_path).is_ok_and(|text| text == stamp);
        if !current || !bindings_path.is_file() {
            write_if_changed(&bindings_path, &bind(root, entries, &shared)?)?;
            write_if_changed(&stamp_path, &stamp)?;
        }
    }

    write_if_changed(
        &out_dir.join("binding_decls.rs"),
        &binding_decls(root, entries, sources)?,
    )
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
        // Every rerun line is this crate's own. Left on, the pass names the copies of the shared modules under
        // `OUT_DIR`. Each is written during a run, and every later build finds it newer and runs the script again.
        .emit_rerun_if_change(false)
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(RustWgslTypeMap)
        .shader_source_type(WgslShaderSourceType::EmbedSource);
    for entry in entries {
        builder.add_entry_point(root.join(entry).to_string_lossy().into_owned());
    }
    builder.build().map_err(compose)?.generate_string().map_err(compose)
}

/// Returns the hash of what the `wgsl_bindgen` pass reads: the generator's releases and this file's source, the root,
/// the entry points, every `.wgsl` file under the root and the shared modules.
fn input_hash(root: &Path, entries: &[PathBuf], sources: &[(PathBuf, String)]) -> u64 {
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
    hasher.write_u64(sources.len() as u64);
    for (path, source) in sources {
        hasher.write_str(&path.to_string_lossy());
        hasher.write_str(source);
    }
    hasher.finish()
}

/// Returns the text of `binding_decls.rs`: the shared modules' hash, each entry's `@group(0)` declarations and the
/// assertions holding each list to the length of its generated layout.
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
            assertions.push_str(&format!(
                "const _: () = ::core::assert!(\n    bindings::{constant}.len() == \
                 super::shader_bindings::{module}::WgpuBindGroup0::LAYOUT_DESCRIPTOR.entries.len(),\n    \
                 \"henad-build read another number of @group(0) bindings from {} than naga composed\",\n);\n",
                entry.display()
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
