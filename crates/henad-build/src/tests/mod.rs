//! Tests of the path checks, the binding reader, a whole generation and the build stamps.
//!
//! Each shader test runs [`crate::ShaderBuild`] against a temporary shader root and `OUT_DIR`. None runs Cargo, which
//! would build `wgsl_bindgen`'s tree again in its own target directory. The stamp tests run git in scratch
//! repositories, and one packages the engine's crates through `cargo package --no-verify` without building them.

mod binding_lines;
mod generate;
mod paths;
mod stamp;
mod support;
