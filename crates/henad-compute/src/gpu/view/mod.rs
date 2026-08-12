//! What a GPU model hands the UI to draw.
//!
//! A grid publishes a [`display`] texture its own compute pass wrote. Agents publish their lane
//! buffers ([`agents`]) and are drawn in place. A composite model publishes both.

pub mod agents;
pub mod display;
