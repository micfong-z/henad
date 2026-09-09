//! Writing a run's results out. Shared by the headless runner and the app.

pub mod state;
pub mod stats_csv;

pub use stats_csv::{StatsWriteError, StatsWriter};
