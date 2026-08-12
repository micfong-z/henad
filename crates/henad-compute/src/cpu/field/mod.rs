//! [`henad_core::authoring::field::FieldLayer`] implementations.

pub mod ca;
pub mod scalar;

pub use ca::{CaField, GRID_INIT_SEED};
pub use scalar::ScalarField;
