//! Generates Rust bindings for this crate's WGSL from the shaders themselves.

use std::path::PathBuf;

use wgsl_bindgen::{RustWgslTypeMap, WgslBindgenOptionBuilder, WgslShaderSourceType, WgslTypeSerializeStrategy};

/// Compute shaders, relative to `src/gpu`. The render shader in `view/` is not here, since it goes
/// through `include_wgsl!` and has no bindings to generate.
const ENTRY_POINTS: &[&str] = &[
    "primitives/hash_count.wgsl",
    "primitives/hash_scatter.wgsl",
    "primitives/scan.wgsl",
    "primitives/scan_add.wgsl",
    "primitives/reduce.wgsl",
    "shared/prelude.wgsl",
    "shared/dims.wgsl",
    "shared/space.wgsl",
    "shared/parity.wgsl",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gpu = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("src/gpu");
    let out = PathBuf::from(std::env::var("OUT_DIR")?);

    let mut builder = WgslBindgenOptionBuilder::default();
    builder
        .workspace_root(&gpu)
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(RustWgslTypeMap)
        .shader_source_type(WgslShaderSourceType::EmbedSource)
        .output(out.join("shader_bindings.rs"));
    for entry in ENTRY_POINTS {
        builder.add_entry_point(gpu.join(entry).to_string_lossy().into_owned());
    }
    builder.build()?.generate()?;

    Ok(())
}
