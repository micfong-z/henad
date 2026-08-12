//! Engine machinery that turns an authoring impl into something runnable.
//!
//! [`cpu`] and [`gpu`] are siblings, not a base and a specialisation. Each holds its own runner,
//! its own engines, and its own primitives. [`snapshot`] and [`runtime_info`] are shared, since
//! both backends publish through them.

pub mod cpu;
pub mod gpu;
pub mod runtime_info;
pub mod snapshot;

pub use cpu::primitives::lanes_macro::__lanes;
