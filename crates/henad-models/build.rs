//! Generates Rust bindings for the models' WGSL from the shaders themselves, and records a hash of the models'
//! sources.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad_build::stamp_source_hash();
    henad_build::ShaderBuild::discover("src")?.generate()?;
    Ok(())
}
