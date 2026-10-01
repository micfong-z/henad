//! The Rust names a shader's path becomes, and the paths refused.

use std::path::PathBuf;

use super::support::{STEP_SHADER, Scratch};
use crate::{ShaderBuild, ShaderBuildError};

#[test]
fn a_path_that_is_no_rust_identifier_is_refused() {
    for (file, component) in [
        ("gpu-flock/step.wgsl", "gpu-flock"),
        ("gpu_flock/step-two.wgsl", "step-two"),
        ("type/step.wgsl", "type"),
        ("gpu_flock/mod.wgsl", "mod"),
        ("2d/step.wgsl", "2d"),
        ("gpu_flock/step.v2.wgsl", "step.v2"),
    ] {
        let scratch = Scratch::new("identifier");
        scratch.write(file, STEP_SHADER);
        match ShaderBuild::discover(scratch.root()) {
            Err(ShaderBuildError::InvalidName { path, component: found }) => {
                assert_eq!(found, component, "{file}");
                assert_eq!(path, scratch.root().join(file));
            }
            other => panic!("{file}: expected an invalid name, got {other:?}"),
        }
    }
}

#[test]
fn a_hyphenated_rust_file_and_a_shaderless_directory_are_accepted() {
    let scratch = Scratch::new("hyphenated");
    scratch.write("bin/my-model-cli.rs", "fn main() {}\n");
    scratch.write("my-assets/henad/logo.svg", "<svg/>\n");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path gpu_vote::state\n\nconst VOTED: u32 = 1u;\n",
    );

    let build = ShaderBuild::discover(scratch.root()).expect("only the .wgsl paths are checked");
    assert_eq!(
        build.entries,
        [PathBuf::from("gpu_vote/step.wgsl")],
        "a module is no entry point"
    );
    build.generate_in(&scratch.out_dir()).expect("the build generates");
    assert!(
        scratch
            .generated("binding_decls.rs")
            .contains("pub const GPU_VOTE_STEP")
    );
}

#[test]
fn two_shaders_with_one_constant_name_are_refused() {
    let scratch = Scratch::new("collision");
    scratch.write("gpu/vote_step.wgsl", STEP_SHADER);
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    match ShaderBuild::discover(scratch.root()) {
        Err(ShaderBuildError::NameCollision { first, second, .. }) => {
            assert_eq!(first, scratch.root().join("gpu/vote_step.wgsl"));
            assert_eq!(second, scratch.root().join("gpu_vote/step.wgsl"));
        }
        other => panic!("expected a collision, got {other:?}"),
    }

    // `aB` and `ab` give the variants `AB` and `Ab`, and the one constant `AB`.
    let entries = [PathBuf::from("aB.wgsl"), PathBuf::from("ab.wgsl")];
    match crate::paths::check_collisions(&scratch.root(), &entries) {
        Err(ShaderBuildError::NameCollision { kind, name, .. }) => {
            assert_eq!((kind, name.as_str()), ("constant", "AB"));
        }
        other => panic!("expected a constant collision, got {other:?}"),
    }

    let entries = [PathBuf::from("vote.wgsl"), PathBuf::from("vote/step.wgsl")];
    match crate::paths::check_collisions(&scratch.root(), &entries) {
        Err(ShaderBuildError::NameCollision { kind, name, .. }) => {
            assert_eq!((kind, name.as_str()), ("module", "vote"));
        }
        other => panic!("expected a module collision, got {other:?}"),
    }
}

#[test]
fn a_henad_folder_anywhere_under_the_root_is_refused() {
    let scratch = Scratch::new("reserved");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    scratch.write(
        "gpu_vote/henad/dispatch.wgsl",
        "#define_import_path henad::dispatch\n\nconst WORKGROUP: u32 = 64u;\n",
    );
    let shadowing = scratch.root().join("gpu_vote/henad/dispatch.wgsl");
    match ShaderBuild::discover(scratch.root()) {
        Err(ShaderBuildError::ReservedName { path, name }) => {
            assert_eq!((path, name.as_str()), (shadowing.clone(), "henad"));
        }
        other => panic!("expected a reserved name, got {other:?}"),
    }
    let explicit = ShaderBuild::new(scratch.root()).entry_point("gpu_vote/step.wgsl");
    match explicit.generate_in(&scratch.out_dir()) {
        Err(ShaderBuildError::ReservedName { path, name }) => {
            assert_eq!((path, name.as_str()), (shadowing.clone(), "henad"));
        }
        other => panic!("expected a reserved name, got {other:?}"),
    }

    for file in [
        "henad.wgsl",
        "Henad.wgsl",
        "gpu_vote/HENAD/dispatch.wgsl",
        "gpu_vote/hEnAd.wgsl",
    ] {
        let scratch = Scratch::new("reserved_file");
        scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
        scratch.write(file, "const WORKGROUP: u32 = 64u;\n");
        match ShaderBuild::discover(scratch.root()) {
            Err(error @ ShaderBuildError::ReservedName { .. }) => {
                assert!(error.to_string().contains("shared modules"), "{file}: {error}");
                assert_eq!(format!("{error:?}"), error.to_string(), "Debug shows the guidance");
            }
            other => panic!("{file}: expected a reserved name, got {other:?}"),
        }
    }

    for root in ["henad", "Henad"] {
        let scratch = Scratch::new("reserved_root");
        let root = scratch.root().join(root);
        std::fs::create_dir_all(&root).expect("the root can be created");
        assert!(matches!(
            ShaderBuild::discover(&root),
            Err(ShaderBuildError::ReservedName { .. })
        ));
    }
}

#[test]
fn a_path_starting_with_a_generated_name_is_refused() {
    for (file, reserved) in [
        ("wgpu.wgsl", "wgpu"),
        ("bytemuck/step.wgsl", "bytemuck"),
        ("std/step.wgsl", "std"),
        ("core.wgsl", "core"),
        ("alloc/step.wgsl", "alloc"),
        ("_root/step.wgsl", "_root"),
        ("ShaderEntry.wgsl", "ShaderEntry"),
        ("layout_asserts/step.wgsl", "layout_asserts"),
        ("bytemuck_impls.wgsl", "bytemuck_impls"),
    ] {
        let scratch = Scratch::new("generated_name");
        scratch.write(file, STEP_SHADER);
        match ShaderBuild::discover(scratch.root()) {
            Err(ShaderBuildError::ReservedName { path, name }) => {
                assert_eq!((path, name.as_str()), (scratch.root().join(file), reserved));
            }
            other => panic!("{file}: expected a reserved name, got {other:?}"),
        }
    }

    // Only the first component is the generated code's.
    let scratch = Scratch::new("generated_name_inside");
    scratch.write("gpu_vote/wgpu.wgsl", STEP_SHADER);
    scratch.write("gpu_vote/core/step.wgsl", STEP_SHADER);
    ShaderBuild::discover(scratch.root()).expect("a generated name below the first component is free");
}

#[test]
fn an_import_path_under_henad_elsewhere_draws_a_warning() {
    let scratch = Scratch::new("reserved_path");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path henad::state\n\nconst VOTED: u32 = 1u;\n",
    );
    let report = ShaderBuild::discover(scratch.root())
        .expect("resolution never reads the line")
        .generate_in(&scratch.out_dir())
        .expect("the build generates");
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("state.wgsl"), "{:?}", report.warnings);
}
