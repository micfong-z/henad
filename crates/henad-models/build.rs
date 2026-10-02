//! Generates Rust bindings for the models' WGSL from the shaders themselves, and records a hash of the models'
//! sources.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad_build::stamp_source_hash();
    // --8<-- [start:shader_build]
    henad_build::ShaderBuild::discover("src")?.generate()?;
    // --8<-- [end:shader_build]
    Ok(())
}
