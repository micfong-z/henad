//! [`henad_core::authoring::model::field::FieldLayer`] implementations.

pub mod ca;
pub mod scalar;

pub use ca::{CaField, GRID_INIT_SEED, grid_init_rng};
pub use scalar::ScalarField;
