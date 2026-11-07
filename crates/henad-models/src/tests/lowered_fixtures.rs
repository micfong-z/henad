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

/// Code hash of each graph starter and corpus file at `be338ec`, the commit before the `hidden` and `tick_unit` keys,
/// and of each round 1 fixture as it was committed.
const CODE_HASHES: [(&str, u64); 18] = [
    ("game_of_life.toml", 0xfa47_7f66_5e31_10fc),
    ("vote.toml", 0x833a_c5c0_973b_caa3),
    ("sir.toml", 0x60a2_c398_4952_d75a),
    ("boids.toml", 0xa740_66c9_71f5_06fb),
    ("ants.toml", 0x26f3_a3e1_b6b9_7d39),
    ("random_walk.toml", 0x4110_ce50_9d6d_690c),
    ("gpu_ants.toml", 0x3f14_3fec_2c4c_d3fb),
    ("gpu_boids.toml", 0x7c72_367d_7f98_5f83),
    ("gpu_sir_exact.toml", 0x31cb_5f69_2798_aa13),
    ("repeat_trials.toml", 0x93ba_6c97_8fa2_3d0d),
    ("round1/ants.toml", 0x7c2a_e559_020a_06fe),
    ("round1/boids.toml", 0x52a6_215c_f96a_e781),
    ("round1/bounded_confidence.toml", 0x1124_1648_facf_bb3e),
    ("round1/forest_fire.toml", 0xe16c_f3be_3370_c94a),
    ("round1/game_of_life.toml", 0x50fb_4d77_3997_0a0c),
    ("round1/random_walk.toml", 0x5cbd_7773_7407_28aa),
    ("round1/sir.toml", 0x8b45_75b8_3102_c8ac),
    ("round1/vote.toml", 0x9108_5e3d_8aa1_0f9a),
];

/// Code hash of each rates fixture before its tick length gained the two keys.
const RATES_CODE_HASHES: [(&str, u64); 3] = [
    ("sir_rates.toml", 0xb12b_b567_4b90_ff76),
    ("sird_competing.toml", 0x5863_22c0_973d_ba52),
    ("sird_higher_wins.toml", 0x000e_3446_c63f_418f),
];

/// The two keys are written only when set. Every file that sets neither keeps its code, so its folders still resume,
/// and the rates fixtures with the keys taken out hash as they did before.
#[test]
fn graph_codes_are_unchanged_by_the_new_keys() {
    let starters = henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters())
        .map(|starter| (starter.file_name.to_owned(), starter.text.to_owned()));
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphs");
    let mut graphs: Vec<(String, String)> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (
                path.file_name().expect("a file name").to_string_lossy().into_owned(),
                text,
            )
        })
        .collect();
    graphs.sort();
    let round_1 = lowered_fixtures("round1")
        .into_iter()
        .map(|(name, text)| (format!("round1/{name}"), text));
    let files: Vec<(String, String)> = starters.chain(graphs).chain(round_1).collect();
    assert_eq!(files.len(), CODE_HASHES.len());
    for ((name, text), (recorded_name, recorded)) in files.iter().zip(CODE_HASHES) {
        assert_eq!(name, recorded_name);
        let file = GraphFile::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(!file.to_toml().contains("tick_unit"), "{name}");
        assert_eq!(file.code().hash(), recorded, "{name}'s code moved");
    }

    let keys = "hidden = true\ntick_unit = \"day\"\n";
    for ((name, text), (recorded_name, recorded)) in lowered_fixtures("rates").iter().zip(RATES_CODE_HASHES) {
        assert_eq!(name, recorded_name);
        assert_eq!(text.matches(keys).count(), 1, "{name} sets the keys on its tick length");
        let file = GraphFile::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_ne!(file.code().hash(), recorded, "{name}: the keys are part of the code");
        let without = GraphFile::parse(&text.replace(keys, "")).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            without.code().hash(),
            recorded,
            "{name}'s code moved apart from the keys"
        );
    }
}
