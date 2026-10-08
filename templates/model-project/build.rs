//! Generates bindings for the shaders under `src`, and stamps the build of this crate's models.

fn main() -> Result<(), henad_build::ShaderBuildError> {
    henad_build::stamp_commit();
    henad_build::ShaderBuild::discover("src")?.generate()?;
    Ok(())
}
