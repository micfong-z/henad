//! Sweep specs, as a spec file or the command line writes them.

use std::time::Duration;

use crate::explore::design::DesignKind;
use crate::explore::factor::FactorSpec;
use crate::explore::reducer::ReducerSpec;
use crate::explore::search::SearchSpec;
use crate::explore::seed::SeedScheme;
use crate::explore::stop::StopSpec;

/// Prefix of the column that holds an action's tick, as in `action.second_wave`.
pub const ACTION_COLUMN_PREFIX: &str = "action.";

/// A sweep as written, with parameter values still in text.
///
/// [`SweepSpec::plan`] checks it against a model and lists its configs.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepSpec {
    /// Id of the model to run.
    pub model: String,
    /// Values every config shares, as `(id, value)` pairs in the form `--set` takes.
    pub fixed: Vec<(String, String)>,
    pub run: RunSettings,
    pub measure: MeasureSettings,
    pub seeds: SeedSettings,
    /// Actions every run fires, in the order two at one tick fire.
    pub actions: Vec<ActionSpec>,
    /// Blocks whose configs the sweep runs, in order. An empty list runs the fixed values alone.
    pub blocks: Vec<BlockSpec>,
    /// Search that picks the configs to run in place of blocks, `None` for a sweep.
    ///
    /// Each candidate of a search runs [`RunSettings::replicates`] times, over the fixed values and actions above.
    pub search: Option<SearchSpec>,
}

impl SweepSpec {
    /// Returns a spec for `model` with default settings and no blocks.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            fixed: Vec::new(),
            run: RunSettings::default(),
            measure: MeasureSettings::default(),
            seeds: SeedSettings::default(),
            actions: Vec::new(),
            blocks: Vec::new(),
            search: None,
        }
    }
}

/// A model action a sweep fires in every run, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec {
    /// Id of the action the model declares.
    pub id: String,
    /// Name factors and output columns give the action, unique within a spec.
    pub name: String,
    /// Tick the action fires at, where no block varies it.
    ///
    /// Tick 0 fires before the first step, and any later tick after the step that reaches it.
    pub tick: u64,
}

impl ActionSpec {
    /// Returns action `id` at `tick`, named by its id.
    pub fn new(id: impl Into<String>, tick: u64) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            tick,
        }
    }

    /// Returns the name of the column that holds the action's tick, as in `action.second_wave`.
    pub fn column_name(&self) -> String {
        format!("{ACTION_COLUMN_PREFIX}{}", self.name)
    }
}

/// Factors combined under one design, as written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockSpec {
    pub design: DesignKind,
    /// Factors of the block. A table design takes its factors from its table, and has none here.
    pub factors: Vec<FactorSpec>,
    /// Seed of a sampled design's draws, [`design_seed`] of the root and the block when `None`.
    ///
    /// [`design_seed`]: crate::explore::seed::design_seed
    pub design_seed: Option<u64>,
}

/// Length and end of each run, and the number of runs per config.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSettings {
    /// Ticks stepped after the warm-up.
    pub steps: u64,
    /// Ticks stepped before the first sample.
    pub warmup: u64,
    /// Runs of each config, each with its own seed.
    pub replicates: u64,
    /// Condition that ends a run at the first sample where it holds.
    pub stop: Option<StopSpec>,
    /// Wall-clock time after which a run is abandoned, checked between slices of steps. On a GPU track, a run's
    /// clock counts its share of the time the sweep spends on the tracks.
    ///
    /// Note that a timed-out run depends on the machine, so no plan or results hash covers the timeout.
    pub timeout: Option<Duration>,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            steps: 1000,
            warmup: 0,
            replicates: 1,
            stop: None,
            timeout: None,
        }
    }
}

/// Ticks a run samples, and the values it keeps from them.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureSettings {
    /// Ticks between two samples, counted from the end of the warm-up.
    pub stats_every: u64,
    /// Ticks between two rows of the series, a multiple of `stats_every`.
    ///
    /// The final sample is always a row. A value of 0 keeps no series at all.
    pub series_every: u64,
    /// Whether every column other than a histogram bucket gets the [`ReducerKind::DEFAULTS`] reducers.
    ///
    /// [`ReducerKind::DEFAULTS`]: crate::explore::reducer::ReducerKind::DEFAULTS
    pub default_reducers: bool,
    /// Reducers added after the defaults.
    pub reducers: Vec<ReducerSpec>,
}

impl Default for MeasureSettings {
    fn default() -> Self {
        Self {
            stats_every: 1,
            series_every: 1,
            default_reducers: true,
            reducers: Vec::new(),
        }
    }
}

/// Root seed, and the scheme that derives each run's seed from it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeedSettings {
    pub root: u64,
    pub scheme: SeedScheme,
}
