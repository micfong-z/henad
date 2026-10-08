//! The layers a GPU model passes to the UI to draw.
//!
//! A grid publishes a [`display`] texture its own compute pass wrote. Agents publish their lane
//! buffers ([`agents`]) and are drawn in place. A composite model publishes both layers.

pub mod agents;
pub mod display;
