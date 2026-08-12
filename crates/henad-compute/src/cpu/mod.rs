//! CPU engine machinery, the sibling of [`crate::gpu`].
//!
//! [`sim_thread`] drives a state off the UI thread, the two `*_engine` modules build one out of an
//! authoring trait, [`field`] holds the grid layers an agent model can sit over, and
//! [`primitives`] the chunking, scatter and lane machinery the engines share.

pub mod agent_engine;
pub mod field;
pub mod grid_engine;
pub mod primitives;
pub mod sim_thread;

pub use agent_engine::{AgentModelState, agent_model_param_descriptors};
pub use grid_engine::{GRID_INIT_SEED, GridModelState, grid_model_param_descriptors};
