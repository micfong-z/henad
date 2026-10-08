//! Generates Rust bindings for this crate's WGSL from the shaders themselves, and records a hash of the crate's
//! sources.

/// Compute shaders, relative to `src/gpu`. The render shader in `view/` is not here, since it goes
/// through `include_wgsl!` and has no bindings to generate.
const ENTRY_POINTS: &[&str] = &[
    "primitives/hash_count.wgsl",
    "primitives/hash_scatter.wgsl",
    "primitives/scan.wgsl",
    "primitives/scan_add.wgsl",
    "primitives/reduce.wgsl",
    "grid_dims.wgsl",
    "tests/parity.wgsl",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad_build::stamp_source_hash();
    let build = ENTRY_POINTS
        .iter()
        .fold(henad_build::ShaderBuild::new("src/gpu"), |build, entry| {
            build.entry_point(entry)
        });
    build.generate()?;
    Ok(())
}
