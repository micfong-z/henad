//! Replays, each holding the build and schedule of one sweep run.

use crate::action::Schedule;
use crate::params::ParamValue;

/// One run of a sweep, as a live simulation rebuilds it.
///
/// A model built with `params` and `seed` follows the run tick for tick when the host fires the actions of `schedule`
/// due at tick 0 before the first step, and each later one after the step that reaches its tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    /// Id of the model the run steps.
    pub model: String,
    /// One value per parameter, in descriptor order.
    pub params: Vec<ParamValue>,
    pub seed: u64,
    pub schedule: Schedule,
    /// Tick the run ends on unless its stop condition holds sooner, the warm-up included.
    pub ticks: u64,
    /// Name of the run in the app, as in `Sweep run 12: config 3, replicate 0`.
    pub label: String,
}
