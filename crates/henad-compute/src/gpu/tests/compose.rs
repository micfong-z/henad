//! Tests of [`crate::gpu::compose`] against this crate's own shaders.

use crate::gpu::compose::{WgslFile, compose_wgsl};

/// Each compute entry point of this crate: its path below `src/gpu`, its source, and the `SHADER_STRING` henad-build
/// generated from it.
const ENTRIES: &[(&str, &str, &str)] = &[
    (
        "primitives/hash_count.wgsl",
        include_str!("../primitives/hash_count.wgsl"),
        crate::shader_bindings::primitives::hash_count::SHADER_STRING,
    ),
    (
        "primitives/hash_scatter.wgsl",
        include_str!("../primitives/hash_scatter.wgsl"),
        crate::shader_bindings::primitives::hash_scatter::SHADER_STRING,
    ),
    (
        "primitives/scan.wgsl",
        include_str!("../primitives/scan.wgsl"),
        crate::shader_bindings::primitives::scan::SHADER_STRING,
    ),
    (
        "primitives/scan_add.wgsl",
        include_str!("../primitives/scan_add.wgsl"),
        crate::shader_bindings::primitives::scan_add::SHADER_STRING,
    ),
    (
        "primitives/reduce.wgsl",
        include_str!("../primitives/reduce.wgsl"),
        crate::shader_bindings::primitives::reduce::SHADER_STRING,
    ),
    (
        "grid_dims.wgsl",
        include_str!("../grid_dims.wgsl"),
        crate::shader_bindings::grid_dims::SHADER_STRING,
    ),
    (
        "tests/parity.wgsl",
        include_str!("parity.wgsl"),
        crate::shader_bindings::tests::parity::SHADER_STRING,
    ),
];

/// Every compute shader of this crate composes at run time to the text henad-build embedded at build time.
///
/// `wgsl_bindgen` writes the text into a raw string literal that opens with a line break, so `SHADER_STRING` is the
/// composed text after one `\n`.
#[test]
fn runtime_composition_matches_build_time_shader_strings() {
    for &(path, source, built) in ENTRIES {
        let composed = compose_wgsl(WgslFile { path, text: source }, &[])
            .unwrap_or_else(|problems| panic!("{path} does not compose: {problems:#?}"));
        assert_eq!(format!("\n{}", composed.text()), built, "{path}");
    }
}
