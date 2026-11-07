//! Sheet starters: sheets a new model begins from, embedded in the crate.

use crate::starters::Starter;

/// Sheet starters in the order an editor lists them.
const SHEET_STARTERS: [Starter; 6] = [
    Starter {
        file_name: "game_of_life.toml",
        text: include_str!("../../sheets/game_of_life.toml"),
    },
    Starter {
        file_name: "vote.toml",
        text: include_str!("../../sheets/vote.toml"),
    },
    Starter {
        file_name: "forest_fire.toml",
        text: include_str!("../../sheets/forest_fire.toml"),
    },
    Starter {
        file_name: "random_walk.toml",
        text: include_str!("../../sheets/random_walk.toml"),
    },
    Starter {
        file_name: "boids.toml",
        text: include_str!("../../sheets/boids.toml"),
    },
    Starter {
        file_name: "bounded_confidence.toml",
        text: include_str!("../../sheets/bounded_confidence.toml"),
    },
];

/// Returns every sheet starter, in the order an editor lists them.
///
/// Each is a file in the crate's `sheets` directory that [`super::ParsedSheet::parse`] reads. The graph starters are in
/// [`crate::starters::starters`].
pub fn starters() -> &'static [Starter] {
    &SHEET_STARTERS
}
