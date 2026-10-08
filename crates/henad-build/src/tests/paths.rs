//! The Rust names that a shader's path becomes, and the rejected paths.

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

    // `aB` and `ab` give the variants `AB` and `Ab`, and the same constant `AB`.
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

    // Only the first component can clash with a name that the generated code uses.
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

#[test]
fn a_path_whose_variant_is_no_identifier_is_refused() {
    for (file, variant) in [("self_.wgsl", "Self"), ("__.wgsl", ""), ("_1.wgsl", "1")] {
        let scratch = Scratch::new("variant");
        scratch.write(file, STEP_SHADER);
        match ShaderBuild::discover(scratch.root()) {
            Err(error @ ShaderBuildError::InvalidName { .. }) => {
                let message = error.to_string();
                let ShaderBuildError::InvalidName { path, component } = error else {
                    unreachable!()
                };
                assert_eq!(component, variant, "{file}");
                assert_eq!(path, scratch.root().join(file));
                assert!(message.contains("`ShaderEntry` variant"), "{message}");
                assert!(!message.contains("``"), "{message}");
            }
            other => panic!("{file}: expected an invalid name, got {other:?}"),
        }
    }

    // A module gets no variant, and can keep the name `self_`.
    let scratch = Scratch::new("variant_module");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    scratch.write("self_.wgsl", "#define_import_path self_\n\nconst VOTED: u32 = 1u;\n");
    ShaderBuild::discover(scratch.root()).expect("a module is never a variant");
}

#[test]
fn a_directive_in_a_block_comment_makes_no_module() {
    let scratch = Scratch::new("commented_directive");
    scratch.write(
        "gpu_vote/step.wgsl",
        &format!("/*\n#define_import_path gpu_vote::step\n*/\n{STEP_SHADER}"),
    );
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path gpu_vote::state\n/*\n#define_import_path henad::state\n*/\nconst VOTED: u32 = 1u;\n",
    );
    let build = ShaderBuild::discover(scratch.root()).expect("the shaders are found");
    assert_eq!(build.entries, [PathBuf::from("gpu_vote/step.wgsl")]);
    let report = build.generate_in(&scratch.out_dir()).expect("the build generates");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_module_imported_under_a_generated_name_is_refused() {
    let scratch = Scratch::new("generated_import");
    scratch.write(
        "gpu_vote/step.wgsl",
        &STEP_SHADER
            .replace("@group(0)", "#import std::VOTED\n\n@group(0)")
            .replace("= 1u;", "= VOTED;"),
    );
    scratch.write(
        "gpu_vote/std.wgsl",
        "#define_import_path gpu_vote::std\n\nconst VOTED: u32 = 1u;\n",
    );
    let build = ShaderBuild::discover(scratch.root()).expect("the file paths are free");
    match build.generate_in(&scratch.out_dir()) {
        Err(error @ ShaderBuildError::ReservedImport { .. }) => {
            let message = error.to_string();
            let ShaderBuildError::ReservedImport {
                path,
                import_path,
                name,
            } = error
            else {
                unreachable!()
            };
            assert_eq!((import_path.as_str(), name.as_str()), ("std", "std"));
            assert!(path.ends_with("gpu_vote/std.wgsl"), "{}", path.display());
            assert!(message.contains("the import path `std`"), "{message}");
        }
        other => panic!("expected a reserved import, got {other:?}"),
    }
}

#[test]
fn a_quoted_import_is_checked_under_the_name_the_bindings_give_it() {
    for (import_path, file, name) in [
        ("\"std.inc\"", "gpu_vote/std.inc", "std"),
        ("\"./std.inc\"", "gpu_vote/std.inc", "std"),
        ("\"std\"", "gpu_vote/std.wgsl", "std"),
        ("\"../shared/core.inc\"", "shared/core.inc", "core"),
        ("\"Henad.inc\"", "gpu_vote/Henad.inc", "Henad"),
    ] {
        let scratch = Scratch::new("quoted_import");
        scratch.write(
            "gpu_vote/step.wgsl",
            &STEP_SHADER
                .replace("@group(0)", &format!("#import {import_path} as noise\n\n@group(0)"))
                .replace("= 1u;", "= noise::value();"),
        );
        let module = "#define_import_path noise\n\nfn value() -> u32 {\n    return 1u;\n}\n";
        scratch.write(file, module);
        let build = ShaderBuild::discover(scratch.root()).expect("the file paths are free");
        match build.generate_in(&scratch.out_dir()) {
            Err(error @ ShaderBuildError::ReservedImport { .. }) => {
                let message = error.to_string();
                let ShaderBuildError::ReservedImport {
                    path,
                    import_path: found_import_path,
                    name: found_name,
                } = error
                else {
                    unreachable!()
                };
                assert_eq!((found_import_path.as_str(), found_name.as_str()), (import_path, name));
                assert!(path.ends_with(file), "{import_path}: {}", path.display());
                let reason = if name == "Henad" {
                    "shared modules"
                } else {
                    "at their root"
                };
                assert!(message.contains(reason), "{import_path}: {message}");
            }
            other => panic!("{import_path}: expected a reserved import, got {other:?}"),
        }
    }

    // A quoted import whose file stem is free generates.
    let scratch = Scratch::new("quoted_import_free");
    scratch.write(
        "gpu_vote/step.wgsl",
        &STEP_SHADER
            .replace("@group(0)", "#import \"std_noise.inc\" as noise\n\n@group(0)")
            .replace("= 1u;", "= noise::value();"),
    );
    scratch.write("gpu_vote/std_noise.inc", "fn value() -> u32 {\n    return 1u;\n}\n");
    ShaderBuild::discover(scratch.root())
        .expect("the file paths are free")
        .generate_in(&scratch.out_dir())
        .expect("a free stem generates");
}

#[cfg(unix)]
#[test]
fn a_linked_directory_is_walked_once() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("linked");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    // Two links back to ancestors would make a walk that follows them unbounded.
    symlink(".", scratch.root().join("again")).expect("a symlink can be made");
    symlink("..", scratch.root().join("gpu_vote/up")).expect("a symlink can be made");
    let shared = scratch.path().join("shared");
    std::fs::create_dir_all(&shared).expect("the directory can be created");
    std::fs::write(shared.join("tally.wgsl"), STEP_SHADER).expect("the file can be written");
    symlink("../shared", scratch.root().join("linked")).expect("a symlink can be made");
    symlink("../shared", scratch.root().join("same")).expect("a symlink can be made");

    let build = ShaderBuild::discover(scratch.root()).expect("the walk ends");
    assert_eq!(
        build.entries,
        [PathBuf::from("gpu_vote/step.wgsl"), PathBuf::from("linked/tally.wgsl")],
        "each directory is walked once, under its first path"
    );
    build.generate_in(&scratch.out_dir()).expect("the build generates");
    assert!(scratch.generated("binding_decls.rs").contains("pub const LINKED_TALLY"));
}
