//! The hand-lowered references: round 1's models and the rates design's three SIR models, written by hand as graph
//! files.
//!
//! The files sit in `tests/fixtures/lowered/`, apart from `tests/fixtures/graphs/`, whose corpus loaders hold every
//! file to a stock model. `tests/fixtures/docs/lowered_fixture.md` holds the procedure they were written and checked
//! by.

use std::path::Path;

use henad_graph::GraphFile;
use henad_graph::model_text::read_header;

/// Returns the name and text of every file in `tests/fixtures/lowered/<directory>`, sorted by name.
fn lowered_fixtures(directory: &str) -> Vec<(String, String)> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lowered")
        .join(directory);
    let mut files: Vec<(String, String)> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
            (name, text)
        })
        .collect();
    files.sort();
    files
}

#[test]
fn every_lowered_fixture_reads() {
    let round_1 = lowered_fixtures("round1");
    let names: Vec<&str> = round_1.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ants.toml",
            "boids.toml",
            "bounded_confidence.toml",
            "forest_fire.toml",
            "game_of_life.toml",
            "random_walk.toml",
            "sir.toml",
            "vote.toml",
        ]
    );
    let rates = lowered_fixtures("rates");
    let names: Vec<&str> = rates.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["sir_rates.toml", "sird_competing.toml", "sird_higher_wins.toml"]
    );

    for (name, text) in round_1.iter().chain(&rates) {
        assert_eq!(read_header(text).as_deref(), Some("graph"), "{name}");
        let file = GraphFile::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let report = file.check();
        assert!(
            !report.has_errors(),
            "{name}: {}",
            report.errors().map(ToString::to_string).collect::<Vec<_>>().join("; ")
        );
    }
}
