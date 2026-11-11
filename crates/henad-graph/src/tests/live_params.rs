//! Live and reload-only parameters of compiled graphs: each sheet starter's entry marks reload-only exactly the
//! parameters its build reads (the starts, the reach of a look at agents, the tick length and the sizes), and every
//! other one live.

use henad_compute::entry::ModelEntry;

use super::sheet_support::{entry_of, lowered, sheet_text};
use crate::sheet::starters;

/// Returns the entry of the sheet starter `file_name`, lowered and compiled.
fn starter_entry(file_name: &str) -> ModelEntry {
    entry_of(&lowered(sheet_text(file_name)))
}

/// Returns the ids of the reload-only parameters of `entry`, in its order.
fn reload_only(entry: &ModelEntry) -> Vec<&str> {
    entry
        .param_descriptors()
        .iter()
        .filter(|descriptor| !descriptor.is_live())
        .map(|descriptor| descriptor.id())
        .collect()
}

/// Each starter marks its sizes, the parameters its starts read, the reach of its looks at agents and its tick length
/// reload-only, and leaves every parameter its lines read live.
#[test]
fn each_sheet_starter_marks_what_its_build_reads_reload_only() {
    let cases: [(&str, &[&str]); 8] = [
        ("game_of_life.toml", &["grid_width", "grid_height", "initial_density"]),
        (
            "sir.toml",
            &["grid_width", "grid_height", "tick_length", "initial_infected"],
        ),
        ("vote.toml", &["grid_width", "grid_height", "initial_share_of_b"]),
        ("forest_fire.toml", &["grid_width", "grid_height", "initial_trees"]),
        ("random_walk.toml", &["num_agents", "world_width", "world_height"]),
        (
            "boids.toml",
            &[
                "num_agents",
                "world_width",
                "world_height",
                "visual_range",
                "protected_range",
                "max_speed",
                "min_speed",
            ],
        ),
        (
            "bounded_confidence.toml",
            &["num_agents", "world_width", "world_height", "hearing_range"],
        ),
        (
            "ants.toml",
            &["num_agents", "world_width", "world_height", "site_reward"],
        ),
    ];
    assert_eq!(cases.len(), starters().len(), "every starter has a case");
    for (file_name, expected) in cases {
        let entry = starter_entry(file_name);
        assert_eq!(reload_only(&entry), expected, "{file_name}");
    }
}

/// The SIR starter's rates are live, the parameters an edit of the running world reaches.
#[test]
fn the_sir_starters_rates_are_live() {
    let entry = starter_entry("sir.toml");
    for id in ["infection_rate", "recovery_rate"] {
        let descriptor = entry
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id() == id)
            .unwrap_or_else(|| panic!("no parameter {id}"));
        assert!(descriptor.is_live(), "{id}");
    }
}
