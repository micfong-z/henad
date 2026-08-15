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

    Ok(())
}
