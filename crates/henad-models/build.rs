//! Generates Rust bindings for the models' WGSL from the shaders themselves.

// --8<-- [start:shader_build]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad_build::ShaderBuild::discover("src")?.generate()?;
    Ok(())
}
// --8<-- [end:shader_build]
