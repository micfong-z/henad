#define_import_path gpu_virus_network::node_state

// Mirrors `virus_network::{SUSCEPTIBLE, INFECTED, RESISTANT}`, which also index the palette.
const SUSCEPTIBLE: u32 = 0u;
const INFECTED: u32 = 1u;
const RESISTANT: u32 = 2u;

// `state_bits` packs each node's state into 2 bits, 16 nodes to a word.
const STATE_BITS: u32 = 2u;
const STATES_PER_WORD: u32 = 16u;
