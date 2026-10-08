//! The traits a model author implements.
//!
//! There is one trait per topology and backend, plus [`field`], the grid layer an [`agent_model::AgentModel`]
//! can sit over. Each trait is const metadata plus pure functions. The engine that drives them lives in
//! `henad-compute`.
//!
//! Note that [`crate::model`] is a different module. It holds the interface the *runner* drives.

pub mod agent_model;
pub mod binding;
pub mod field;
pub mod gpu_agent_model;
pub mod gpu_grid_model;
pub mod grid_model;
pub mod network_model;
