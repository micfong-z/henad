//! The traits a model author implements.
//!
//! One per topology, plus [`field`], the grid layer an [`agent_model::AgentModel`] can sit over.
//! Each is const metadata plus pure functions. The engine that drives them lives in
//! `henad-compute`.
//!
//! Not to be confused with [`crate::model`], which is the interface the *runner* drives.

pub mod agent_model;
pub mod field;
pub mod gpu_grid_model;
pub mod grid_model;
