//! CPU engine machinery, the sibling of [`crate::gpu`].
//!
//! [`sim_thread`] runs a state, on a thread of its own on native and inside the frame loop on the web. The three
//! `*_engine` modules each build one out of an authoring trait. [`field`] holds the grid layers an agent model can
//! sit over, and [`layout`] relaxes a network model's node positions. [`primitives`] holds the chunking, scatter,
//! lane and connected-component building blocks that the engines and models call.

pub mod agent_engine;
pub mod field;
pub mod grid_engine;
pub mod layout;
pub mod network_engine;
pub mod primitives;
pub mod sim_thread;

pub use agent_engine::{AgentModelState, agent_model_param_descriptors};
pub use grid_engine::{GRID_INIT_SEED, GridModelState, grid_model_param_descriptors};
