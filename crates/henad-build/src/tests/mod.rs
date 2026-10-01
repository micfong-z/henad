//! Tests of the path checks, the binding reader and a whole generation.
//!
//! Each runs [`crate::ShaderBuild`] against a temporary shader root and `OUT_DIR`. None runs Cargo, which would build
//! `wgsl_bindgen`'s tree again in a target directory of its own.

mod binding_lines;
mod generate;
mod paths;
mod support;
