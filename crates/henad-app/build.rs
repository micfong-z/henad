//! Generates Rust bindings for this crate's WGSL from the shader itself.

use std::path::PathBuf;

use wgsl_bindgen::{RustWgslTypeMap, WgslBindgenOptionBuilder, WgslShaderSourceType, WgslTypeSerializeStrategy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("src/ui");
    let out = PathBuf::from(std::env::var("OUT_DIR")?);

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
