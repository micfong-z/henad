//! Starters: graph files a new graph model begins from, embedded in the crate.
//!
//! Each starter is an example model ported to one GPU target, with the model id `node_<model>`, or a model of its own
//! such as the random walk. A starter reads and checks with no error.

/// A graph file a new graph model begins from.
#[derive(Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Starter {
    /// Name of the file in the crate's `starters` directory, such as `sir.toml`.
    pub file_name: &'static str,
    /// Text of the file.
    pub text: &'static str,
}

/// Prints the file name and the byte length of the text.
impl std::fmt::Debug for Starter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Starter")
            .field("file_name", &self.file_name)
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

/// Starters in the order an editor lists them.
const STARTERS: [Starter; 3] = [
    Starter {
        file_name: "game_of_life.toml",
        text: include_str!("../starters/game_of_life.toml"),
    },
    Starter {
        file_name: "vote.toml",
        text: include_str!("../starters/vote.toml"),
    },
    Starter {
        file_name: "sir.toml",
        text: include_str!("../starters/sir.toml"),
    },
];

/// Starters of the agents topology, in the order an editor lists them.
const AGENT_STARTERS: [Starter; 3] = [
    Starter {
        file_name: "boids.toml",
        text: include_str!("../starters/boids.toml"),
    },
    Starter {
        file_name: "ants.toml",
        text: include_str!("../starters/ants.toml"),
    },
    Starter {
        file_name: "random_walk.toml",
        text: include_str!("../starters/random_walk.toml"),
    },
];

/// Returns every starter of the grid topology, in the order an editor lists them. The starters of agents are in
/// [`agent_starters`].
pub fn starters() -> &'static [Starter] {
    &STARTERS
}

/// Returns every starter of the agents topology, in the order an editor lists them after the grid starters.
///
/// Each compiles to WGSL through [`crate::wgsl::AgentProgram`].
pub fn agent_starters() -> &'static [Starter] {
    &AGENT_STARTERS
}

/// Returns the starter whose file is named `file_name`, such as `sir.toml`, of either topology.
pub fn starter(file_name: &str) -> Option<&'static Starter> {
    STARTERS
        .iter()
        .chain(&AGENT_STARTERS)
        .find(|starter| starter.file_name == file_name)
}
