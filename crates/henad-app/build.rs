//! Generates Rust bindings for this crate's WGSL from the shader itself, and stamps the binary with the commit it
//! was built from and a hash of its sources.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad_build::stamp_commit();
    henad_build::ShaderBuild::discover("src/ui")?.generate()?;
    Ok(())
}
