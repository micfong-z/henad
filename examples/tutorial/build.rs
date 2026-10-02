//! Generates Rust bindings for the tutorial's WGSL from the shaders themselves, and stamps the build of its models.

fn main() -> Result<(), henad_build::ShaderBuildError> {
    henad_build::stamp_commit();
    henad_build::ShaderBuild::discover("src")?.generate()
}
