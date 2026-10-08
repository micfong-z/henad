//! Whole generations against a temporary `OUT_DIR`: what they write, what they leave alone and what Cargo watches.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::support::{STEP_SHADER, Scratch};
use crate::{ShaderBuild, ShaderBuildError};

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
    assert!(report.bound, "the first build runs the pass");

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
    let second = ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&out_dir)
        .expect("the second build generates");
    assert!(!second.bound, "the stamp matches, and the second build skips the pass");
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
fn a_nested_entry_names_its_path_with_forward_slashes() {
    let scratch = Scratch::new("nested_entry");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect("the build generates");
    let decls = scratch.generated("binding_decls.rs");
    assert!(
        decls.contains(
            "\"henad-build read another number of @group(0) bindings from gpu_vote/step.wgsl than naga composed\""
        ),
        "{decls}"
    );
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

/// An entry point that calls a function from the module `gpu_vote::state`.
const CALLING_SHADER: &str = "\
#import henad::dispatch::{WORKGROUP, linear_index}
#import gpu_vote::state::voted

@group(0) @binding(0) var<storage, read_write> state: array<u32>;

@compute
@workgroup_size(WORKGROUP)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    state[linear_index(lid, wid, 1u)] = voted() + 11u;
}
";

#[test]
fn an_edited_shader_or_module_is_generated_again() {
    let scratch = Scratch::new("edited");
    let module =
        |value: u32| format!("#define_import_path gpu_vote::state\n\nfn voted() -> u32 {{\n    return {value}u;\n}}\n");
    scratch.write("gpu_vote/step.wgsl", CALLING_SHADER);
    scratch.write("gpu_vote/state.wgsl", &module(3));
    let generate = || {
        ShaderBuild::discover(scratch.root())
            .expect("the shaders are found")
            .generate_in(&scratch.out_dir())
            .expect("the build generates")
    };
    assert!(generate().bound);
    let first = scratch.generated("shader_bindings.rs");
    assert!(first.contains("+ 11u") && first.contains("return 3u;"), "{first}");
    assert!(!generate().bound, "nothing changed");

    // The same files now contain different text.
    scratch.write("gpu_vote/step.wgsl", &CALLING_SHADER.replace("11u", "13u"));
    assert!(generate().bound, "an edited entry point reruns the pass");
    let edited = scratch.generated("shader_bindings.rs");
    assert!(edited.contains("+ 13u") && !edited.contains("+ 11u"), "{edited}");

    scratch.write("gpu_vote/state.wgsl", &module(7));
    assert!(generate().bound, "an edited module reruns the pass");
    let edited = scratch.generated("shader_bindings.rs");
    assert!(
        edited.contains("return 7u;") && !edited.contains("return 3u;"),
        "{edited}"
    );
}

#[test]
fn a_shared_module_henad_core_no_longer_holds_is_removed() {
    let scratch = Scratch::new("stale_shared");
    scratch.write("gpu_vote/step.wgsl", STEP_SHADER);
    let stale = scratch.out_dir().join("henad_wgsl/henad/renamed.wgsl");
    std::fs::create_dir_all(stale.parent().expect("a file has a parent")).expect("the directory can be created");
    std::fs::write(&stale, "const OLD: u32 = 1u;\n").expect("the file can be written");
    ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect("the build generates");
    assert!(!stale.exists(), "the copy of a removed shared module stays importable");
    assert!(scratch.out_dir().join("henad_wgsl/henad/dispatch.wgsl").is_file());
}

/// Returns an entry point that imports `path` as `noise` and calls its `value`.
fn importing_shader(path: &str) -> String {
    format!(
        "#import henad::dispatch::{{WORKGROUP, linear_index}}\n#import \"{path}\" as noise\n\n\
         @group(0) @binding(0) var<storage, read_write> state: array<u32>;\n\n\
         @compute\n@workgroup_size(WORKGROUP)\n\
         fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {{\n    \
         state[linear_index(lid, wid, 1u)] = noise::value();\n}}\n"
    )
}

/// Returns a module whose `value` returns `value`.
fn noise_module(value: u32) -> String {
    format!("fn value() -> u32 {{\n    return {value}u;\n}}\n")
}

#[test]
fn a_file_imported_from_outside_the_root_is_watched_and_hashed() {
    let scratch = Scratch::new("outside_root");
    scratch.write("gpu_vote/step.wgsl", &importing_shader("../../shared/noise"));
    let shared = scratch.path().join("shared/noise.wgsl");
    std::fs::create_dir_all(shared.parent().expect("a file has a parent")).expect("the directory can be created");
    std::fs::write(&shared, noise_module(3)).expect("the file can be written");
    let generate = || {
        ShaderBuild::discover(scratch.root())
            .expect("the shaders are found")
            .generate_in(&scratch.out_dir())
            .expect("the build generates")
    };

    let report = generate();
    let shared_path = std::path::absolute(&shared).expect("the file has an absolute path");
    assert!(
        report.watched.contains(&shared_path),
        "the file outside the root is not watched: {:?}",
        report.watched
    );
    assert!(scratch.generated("shader_bindings.rs").contains("return 3u;"));
    assert!(!generate().bound, "nothing changed");

    std::fs::write(&shared, noise_module(5)).expect("the file can be written");
    assert!(generate().bound, "an edit outside the root reruns the pass");
    assert!(scratch.generated("shader_bindings.rs").contains("return 5u;"));

    std::fs::write(
        &shared,
        format!(
            "@group(0) @binding(1) var<storage, read> votes: array<u32>;\n\n{}",
            noise_module(5)
        ),
    )
    .expect("the file can be written");
    match ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
    {
        Err(ShaderBuildError::ModuleBinding { path, line: 1, .. }) => assert_eq!(path, shared_path),
        other => panic!("expected a refused binding outside the root, got {other:?}"),
    }
}

#[test]
fn a_file_without_the_wgsl_extension_is_hashed() {
    let scratch = Scratch::new("other_extension");
    scratch.write("gpu_vote/step.wgsl", &importing_shader("noise.inc"));
    scratch.write("gpu_vote/noise.inc", &noise_module(3));
    let generate = || {
        ShaderBuild::discover(scratch.root())
            .expect("the shaders are found")
            .generate_in(&scratch.out_dir())
            .expect("the build generates")
    };
    let report = generate();
    let root = std::path::absolute(scratch.root()).expect("the root has an absolute path");
    assert_eq!(
        report.watched,
        [root],
        "a file under the root is watched through the root"
    );
    assert!(!generate().bound, "nothing changed");
    scratch.write("gpu_vote/noise.inc", &noise_module(5));
    assert!(generate().bound, "an edit to an imported file reruns the pass");
    assert!(scratch.generated("shader_bindings.rs").contains("return 5u;"));

    // An entry point added through `ShaderBuild::entry_point` can have another extension too.
    let explicit = Scratch::new("other_extension_entry");
    explicit.write("gpu_vote/step.shader", STEP_SHADER);
    let generate = || {
        ShaderBuild::new(explicit.root())
            .entry_point("gpu_vote/step.shader")
            .generate_in(&explicit.out_dir())
            .expect("the build generates")
    };
    assert!(generate().bound);
    assert!(!generate().bound, "nothing changed");
    explicit.write("gpu_vote/step.shader", &STEP_SHADER.replace("= 1u;", "= 9u;"));
    assert!(generate().bound, "an edit to the entry point reruns the pass");
    assert!(explicit.generated("shader_bindings.rs").contains("= 9u;"));
}

#[test]
fn a_composer_panic_is_an_error() {
    let scratch = Scratch::new("nested_missing");
    scratch.write("gpu_vote/step.wgsl", CALLING_SHADER);
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path gpu_vote::state\n#import gpu_vote::missing::LOST\n\n\
         fn voted() -> u32 {\n    return LOST;\n}\n",
    );
    let error = ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect_err("an import inside a module that does not resolve fails the build");
    assert!(matches!(error, ShaderBuildError::Compose { .. }), "{error}");
    assert!(
        error
            .to_string()
            .contains("state.wgsl: Cannot find import `gpu_vote::missing::LOST`"),
        "{error}"
    );

    let scratch = Scratch::new("unparsable");
    scratch.write(
        "gpu_vote/step.wgsl",
        &format!("#import gpu_vote::state::{{voted,\n{STEP_SHADER}"),
    );
    let error = ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect_err("an import that does not parse fails the build");
    assert!(matches!(error, ShaderBuildError::Compose { .. }), "{error}");
    assert!(error.to_string().contains("failed to parse imports"), "{error}");

    let scratch = Scratch::new("cycle");
    scratch.write("gpu_vote/step.wgsl", CALLING_SHADER);
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path gpu_vote::state\n#import gpu_vote::other::other\n\n\
         fn voted() -> u32 {\n    return 1u;\n}\n",
    );
    scratch.write(
        "gpu_vote/other.wgsl",
        "#define_import_path gpu_vote::other\n#import gpu_vote::state::voted\n\n\
         fn other() -> u32 {\n    return voted();\n}\n",
    );
    let error = ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
        .expect_err("a cycle of imports fails the build");
    let message = error.to_string();
    assert!(matches!(error, ShaderBuildError::Compose { .. }), "{message}");
    assert!(message.contains("cycle") && message.contains("other.wgsl"), "{message}");
}
