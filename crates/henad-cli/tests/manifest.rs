//! Checks that a sweep's manifest records the builds of the `henad-cli` binary that ran it, and that a resume under the
//! same binary prints no `build_changed` warning.

mod support;

use std::path::Path;

use serde_json::Value;
use support::{Scratch, manifest, run_ok, stdout};

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
    let out = scratch.path("sweep");
    run_ok(&sweep_arguments(&out, "1"));
    let manifest = manifest(&out);
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

    let resumed = run_ok(&[sweep_arguments(&out, "2"), vec!["--resume", "--json"]].concat());
    let warnings: Vec<Value> = stdout(&resumed)
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
