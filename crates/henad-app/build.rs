//! Generates Rust bindings for this crate's WGSL from the shader itself, and stamps the binary
//! with the commit it was built from.

use std::path::{Path, PathBuf};
use std::process::Command;

use wgsl_bindgen::{RustWgslTypeMap, WgslBindgenOptionBuilder, WgslShaderSourceType, WgslTypeSerializeStrategy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let ui = manifest.join("src/ui");
    let out = PathBuf::from(std::env::var("OUT_DIR")?);

    emit_commit_stamp(&manifest);

    WgslBindgenOptionBuilder::default()
        .workspace_root(&ui)
        .add_entry_point(ui.join("agents.wgsl").to_string_lossy().into_owned())
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(RustWgslTypeMap)
        .shader_source_type(WgslShaderSourceType::EmbedSource)
        .output(out.join("shader_bindings.rs"))
        .build()?
        .generate()?;

    Ok(())
}

fn emit_commit_stamp(manifest: &Path) {
    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git").args(args).current_dir(manifest).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };

    let commit = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_default();
    let date = git(&["log", "-1", "--format=%cd", "--date=short"]).unwrap_or_default();
    println!("cargo:rustc-env=HENAD_COMMIT={commit}");
    println!("cargo:rustc-env=HENAD_COMMIT_DATE={date}");

    // A commit rewrites HEAD, or the branch ref it points at. Only track a path that is there.
    // Naming a missing one reruns this script on every build instead.
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) else {
        return;
    };
    let branch = git(&["symbolic-ref", "-q", "HEAD"]).map(|reference| git_dir.join(reference));
    for path in [Some(git_dir.join("HEAD")), branch].into_iter().flatten() {
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
