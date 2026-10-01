//! Whole generations against a temporary `OUT_DIR`: what they write, what they leave alone and what Cargo watches.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::support::{STEP_SHADER, Scratch};
use crate::ShaderBuild;

/// Returns every file under `directory` with its modification time.
fn modification_times(directory: &Path) -> BTreeMap<PathBuf, SystemTime> {
    let mut times = BTreeMap::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        for item in std::fs::read_dir(&current).expect("the directory can be read") {
            let path = item.expect("the directory can be read").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let modified = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .expect("the file has a modification time");
                times.insert(path, modified);
            }
        }
    }
    times
}

#[test]
fn a_second_build_reruns_nothing() {
    let scratch = Scratch::new("second_build");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path gpu_vote::state\n\nconst VOTED: u32 = 1u;\n",
    );
    let out_dir = scratch.out_dir();

    let report = ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&out_dir)
        .expect("the first build generates");
    let root = std::path::absolute(scratch.root()).expect("the root has an absolute path");
    assert_eq!(report.watched, [root], "a discovered build watches its root alone");
    assert!(report.watched.iter().all(|path| !path.starts_with(&out_dir)));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let first = modification_times(&out_dir);
    for name in [
        "shader_bindings.rs",
        "binding_decls.rs",
        "henad_wgsl/henad/dispatch.wgsl",
        "henad_wgsl/henad/reduce_tree.wgsl",
    ] {
        assert!(first.contains_key(&out_dir.join(name)), "{name} was not written");
    }

    // A file system that records whole seconds would hide a rewrite within the same second.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&out_dir)
        .expect("the second build generates");
    assert_eq!(modification_times(&out_dir), first, "the second build rewrote a file");

    let explicit = ShaderBuild::new(scratch.root())
        .entry_point("gpu_vote/step.wgsl")
        .generate_in(&out_dir)
        .expect("the explicit build generates");
    let root = std::path::absolute(scratch.root()).expect("the root has an absolute path");
    assert_eq!(explicit.watched, [root.clone(), root.join("gpu_vote/step.wgsl")]);
    assert!(explicit.watched.iter().all(|path| !path.starts_with(&out_dir)));
}

#[test]
fn a_shader_added_between_builds_is_generated() {
    let scratch = Scratch::new("added_shader");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect("the first build generates");
    assert!(!scratch.generated("shader_bindings.rs").contains("pub mod tally"));

    scratch.write("gpu_vote/tally.wgsl", STEP_SHADER);
    ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect("the second build generates");
    let bindings = scratch.generated("shader_bindings.rs");
    assert!(bindings.contains("pub mod tally"), "the new shader has no module");
    assert!(
        bindings.contains("GpuVoteTally"),
        "the new shader has no ShaderEntry variant"
    );
    assert!(
        scratch
            .generated("binding_decls.rs")
            .contains("pub const GPU_VOTE_TALLY")
    );
}

#[test]
fn a_crate_without_shaders_builds() {
    let scratch = Scratch::new("no_shaders");
    scratch.write("lib.rs", "pub fn vote() {}\n");
    let build = ShaderBuild::discover(scratch.root()).expect("an empty root is fine");
    assert!(build.entries.is_empty());
    build.generate_in(&scratch.out_dir()).expect("an empty root generates");

    assert_eq!(scratch.generated("shader_bindings.rs"), "");
    let decls = scratch.generated("binding_decls.rs");
    assert!(decls.contains("pub const SHARED_WGSL_FNV1A64: u64 = 0x"), "{decls}");
    assert!(decls.contains("pub mod bindings {\n}\n"), "{decls}");
    assert!(!decls.contains("use super"), "{decls}");
}

#[test]
fn shaders_removed_and_restored_are_generated_again() {
    let scratch = Scratch::new("removed_and_restored");
    let generate = || {
        ShaderBuild::discover(scratch.root())
            .expect("the shaders are found")
            .generate_in(&scratch.out_dir())
            .expect("the build generates");
    };
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    generate();
    let with_shaders = scratch.generated("shader_bindings.rs");
    assert!(with_shaders.contains("pub mod gpu_vote"));

    std::fs::remove_dir_all(scratch.root().join("gpu_vote")).expect("the shaders can be removed");
    generate();
    assert_eq!(scratch.generated("shader_bindings.rs"), "");

    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    generate();
    assert_eq!(
        scratch.generated("shader_bindings.rs"),
        with_shaders,
        "the bindings stayed empty"
    );
}
