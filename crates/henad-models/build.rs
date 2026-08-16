//! Generates Rust bindings for the models' WGSL from the shaders themselves.

use std::path::PathBuf;

use wgsl_bindgen::{RustWgslTypeMap, WgslBindgenOptionBuilder, WgslShaderSourceType, WgslTypeSerializeStrategy};

/// Every shader a model hands the engine, relative to `src`.
const ENTRY_POINTS: &[&str] = &[
    "gpu_game_of_life/step.wgsl",
    "gpu_game_of_life/display.wgsl",
    "gpu_game_of_life/reduce.wgsl",
    "gpu_sir/step.wgsl",
    "gpu_sir/display.wgsl",
    "gpu_sir/reduce.wgsl",
    "gpu_boids/step.wgsl",
    "gpu_boids/reduce.wgsl",
    "gpu_ants/step.wgsl",
    "gpu_ants/merge.wgsl",
    "gpu_ants/display.wgsl",
    "gpu_ants/reduce.wgsl",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let src = manifest.join("src");
    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    // `shared/` lives with the engine that owns the prelude, one crate down.
    let compute_gpu = manifest.join("../henad-compute/src/gpu");

    let mut builder = WgslBindgenOptionBuilder::default();
    builder
        .workspace_root(&src)
        .additional_scan_dir((None, compute_gpu.to_string_lossy().as_ref()))
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(RustWgslTypeMap)
        .shader_source_type(WgslShaderSourceType::EmbedSource)
        .output(out.join("shader_bindings.rs"));
    for entry in ENTRY_POINTS {
        builder.add_entry_point(src.join(entry).to_string_lossy().into_owned());
    }
    builder.build()?.generate()?;

    emit_binding_decls(&src, &out)?;

    Ok(())
}

/// Emits each shader's `@group(0)` declarations, in `@binding` order.
///
/// `wgsl_bindgen` keeps binding names only in doc comments, so the engine cannot read them. This
/// reads them off the declaration lines instead, which is enough because everything needed (index,
/// name, address space, access) is on the line itself. Only the *type* needs the imports resolved,
/// which is why this does not have to compose the module first. A `@binding` line that does not
/// match the expected shape fails the build rather than being skipped.
fn emit_binding_decls(src: &std::path::Path, out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::fmt::Write as _;

    let mut rust = String::from(
        "/// Each shader's `@group(0)` declarations, in `@binding` order.\npub mod bindings {\n    use henad_core::authoring::binding::{BindingDecl, BindingKind};\n",
    );
    for entry in ENTRY_POINTS {
        let path = src.join(entry);
        let source = std::fs::read_to_string(&path)?;

        let mut decls: Vec<(u32, String, String)> = Vec::new();
        for line in source.lines().filter(|l| l.trim_start().starts_with("@group(0)")) {
            let index: u32 = line
                .split("@binding(")
                .nth(1)
                .and_then(|rest| rest.split(')').next())
                .ok_or_else(|| format!("{entry}: no @binding index in {line:?}"))?
                .trim()
                .parse()?;
            let name = line
                .rsplit_once(':')
                .and_then(|(head, _)| head.rsplit_once('>').or_else(|| head.rsplit_once(' ')))
                .map(|(_, name)| name.trim().to_owned())
                .filter(|n| !n.is_empty())
                .ok_or_else(|| format!("{entry}: no binding name in {line:?}"))?;
            let kind = if line.contains("var<uniform>") {
                "BindingKind::Uniform".to_owned()
            } else if line.contains("var<storage") {
                format!("BindingKind::Storage {{ read_only: {} }}", !line.contains("read_write"))
            } else if line.contains("texture_storage") {
                "BindingKind::StorageTexture".to_owned()
            } else {
                return Err(format!("{entry}: unrecognised binding kind in {line:?}").into());
            };
            decls.push((index, name, kind));
        }
        decls.sort_by_key(|(i, _, _)| *i);
        for (slot, (index, _, _)) in decls.iter().enumerate() {
            assert_eq!(
                *index as usize, slot,
                "{entry}: @binding indices must be 0..n with no gaps"
            );
        }

        let konst = entry.replace(['/', '.'], "_").to_uppercase().replace("_WGSL", "");
        writeln!(rust, "    pub const {konst}: &[BindingDecl] = &[")?;
        for (_, name, kind) in decls {
            writeln!(rust, "        BindingDecl {{ name: \"{name}\", kind: {kind} }},")?;
        }
        rust.push_str("    ];\n");
    }
    rust.push_str("}\n");
    std::fs::write(out.join("binding_decls.rs"), rust)?;
    Ok(())
}
