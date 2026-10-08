//! Checks that a sweep's manifest records the builds `henad-cli` ran, and that a resume under the same binary warns
//! of no build change.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// Directory under the system's temporary directory, removed with its contents on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("henad-cli-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Runs `henad-cli` with `arguments` and returns its output.
///
/// # Panics
///
/// Panics when the command does not run or exits with a failure.
fn run(arguments: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_henad-cli"))
        .args(arguments)
        .output()
        .expect("henad-cli runs");
    assert!(
        output.status.success(),
        "henad-cli {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Returns the arguments of a two-step Game of Life sweep of `replicates` replicates into `dir`.
fn sweep_arguments<'a>(dir: &'a Path, replicates: &'a str) -> Vec<&'a str> {
    vec![
        "game_of_life",
        "--set",
        "grid_width=8",
        "--set",
        "grid_height=8",
        "--steps",
        "2",
        "--reps",
        replicates,
        "--out",
        dir.to_str().expect("a UTF-8 path"),
    ]
}

#[test]
fn a_manifest_from_the_cli_entry_records_its_host() {
    let scratch = Scratch::new("manifest-host");
    run(&sweep_arguments(&scratch.0, "1"));
    let text = std::fs::read_to_string(scratch.0.join("manifest.json")).expect("the manifest is written");
    let manifest: Value = serde_json::from_str(&text).expect("the manifest is JSON");
    let session = &manifest["sessions"][0];
    let host = &session["host"];
    assert_eq!(host["name"], "henad-cli");
    assert!(
        host["commit"].as_str().is_some_and(|commit| !commit.is_empty()) || host["source_hash"].is_string(),
        "the host records a commit or a source hash: {host}"
    );
    assert_eq!(session["engine"]["name"], "henad");
    assert_eq!(session["engine"], manifest["engine"]);
    assert_eq!(session["model_source"]["name"], "henad-models");
    assert!(
        session["model_source"]["source_hash"].is_string(),
        "henad-models stamps its source hash"
    );
    assert_eq!(manifest["model"]["replays_exactly"], true);

    let resumed = run(&[sweep_arguments(&scratch.0, "2"), vec!["--resume", "--json"]].concat());
    let warnings: Vec<Value> = String::from_utf8_lossy(&resumed.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| line["kind"] == "explore_warning")
        .collect();
    assert_eq!(
        warnings,
        Vec::<Value>::new(),
        "a resume by the same binary warns of nothing"
    );
}
