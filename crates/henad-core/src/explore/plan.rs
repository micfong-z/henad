//! Plans that check a sweep spec against a model and list every config and run.

use std::fmt;
use std::ops::Range;
use std::str::FromStr;

use crate::action::{ActionDescriptor, Schedule, Scheduled};
use crate::explore::design::{Block, DesignError, DesignKind, generate};
use crate::explore::design_csv::{DesignTableError, read_table};
use crate::explore::factor::{FactorError, FactorSlot, FactorTarget};
use crate::explore::fingerprint::{plan_hash, results_fingerprint, run_key, schema_hash};
use crate::explore::measure::{MeasureError, MeasurePlan};
use crate::explore::outcome::PlannedRun;
use crate::explore::replay::Replay;
use crate::explore::seed::design_seed;
use crate::explore::spec::{ActionSpec, BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec};
use crate::explore::stop::StopError;
use crate::explore::value::{ValueError, resolve_params};
use crate::params::{ParamDescriptor, ParamValue};
use crate::view::StatDescriptor;

/// Declarations of a model, as a plan checks a spec against them.
#[derive(Debug, Clone, Copy)]
pub struct ModelSchema<'a> {
    pub id: &'a str,
    pub params: &'a [ParamDescriptor],
    pub stats: &'a [StatDescriptor],
    pub actions: &'a [ActionDescriptor],
}

/// One set of parameter values and action ticks a sweep runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Index of the block the config comes from.
    pub block: usize,
    /// One value per parameter, in descriptor order.
    pub params: Vec<ParamValue>,
    /// Tick of each of the spec's actions, in spec order.
    pub action_ticks: Vec<u64>,
}

/// Design and configs of one block of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedBlock {
    pub design: DesignKind,
    /// Ids of the block's configs.
    pub configs: Range<u64>,
    /// Seed the block's configs were drawn from, `None` for a design that draws nothing.
    pub design_seed: Option<u64>,
}

/// Something in a plan that runs, though likely not as meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanWarning {
    /// Action `name`, due past the run's `last_tick` in `config_count` configs, at `latest_tick` at the latest.
    ActionAfterEnd {
        name: String,
        latest_tick: u64,
        last_tick: u64,
        config_count: u64,
    },
}

impl fmt::Display for PlanWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionAfterEnd {
                name,
                latest_tick,
                last_tick,
                config_count,
            } => {
                let configs = if *config_count == 1 { "config" } else { "configs" };
                write!(
                    f,
                    "action '{name}' is due after the last tick {last_tick} in {config_count} {configs} (latest tick \
                     {latest_tick}) and will not run there"
                )
            }
        }
    }
}

/// Share of a plan's runs, the runs whose id leaves remainder `index` when divided by `count`.
///
/// Written `INDEX/COUNT`, as in `0/4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    index: u64,
    count: u64,
}

impl Shard {
    /// Every run of a plan.
    pub const WHOLE: Self = Self { index: 0, count: 1 };

    /// Returns shard `index` of `count`.
    ///
    /// # Errors
    ///
    /// Returns [`ShardError`] for a `count` of 0, or an `index` not below `count`.
    pub fn new(index: u64, count: u64) -> Result<Self, ShardError> {
        if count == 0 {
            return Err(ShardError::ZeroCount);
        }
        if index >= count {
            return Err(ShardError::IndexPastCount { index, count });
        }
        Ok(Self { index, count })
    }

    pub fn index(self) -> u64 {
        self.index
    }

    pub fn count(self) -> u64 {
        self.count
    }

    /// Returns whether run `run_id` falls in the shard.
    pub fn contains(self, run_id: u64) -> bool {
        run_id % self.count == self.index
    }

    /// Returns the number of runs the shard takes of a plan of `plan_runs` runs.
    pub fn run_count(self, plan_runs: u64) -> u64 {
        if self.index < plan_runs {
            (plan_runs - 1 - self.index) / self.count + 1
        } else {
            0
        }
    }
}

impl Default for Shard {
    fn default() -> Self {
        Self::WHOLE
    }
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.index, self.count)
    }
}

impl FromStr for Shard {
    type Err = ShardError;

    /// Reads `INDEX/COUNT`.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (index, count) = raw
            .split_once('/')
            .and_then(|(index, count)| Some((index.trim().parse().ok()?, count.trim().parse().ok()?)))
            .ok_or_else(|| ShardError::BadText { raw: raw.to_owned() })?;
        Self::new(index, count)
    }
}

/// A shard that cannot be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShardError {
    /// Text not of the form `INDEX/COUNT`.
    BadText { raw: String },
    /// A shard count of 0.
    ZeroCount,
    /// An index not below the count.
    IndexPastCount { index: u64, count: u64 },
}

impl fmt::Display for ShardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadText { raw } => write!(f, "invalid shard '{raw}', expected INDEX/COUNT"),
            Self::ZeroCount => write!(f, "shard count must be at least 1"),
            Self::IndexPastCount { index, count } => {
                write!(f, "shard index {index} must be less than shard count {count}")
            }
        }
    }
}

impl std::error::Error for ShardError {}

/// A sweep spec checked against a model, with every config listed.
///
/// Runs are numbered config by config, so run `run_id` is replicate `run_id % replicates` of config
/// `run_id / replicates`.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    model: String,
    configs: Vec<Config>,
    blocks: Vec<PlannedBlock>,
    run_settings: RunSettings,
    measure: MeasureSettings,
    seeds: SeedSettings,
    actions: Vec<ActionSpec>,
    /// Index of each action in the model's declarations, in spec order.
    action_indices: Vec<usize>,
    warnings: Vec<PlanWarning>,
    run_count: u64,
    schema_hash: u64,
    results_fingerprint: u64,
    plan_hash: u64,
}

impl Plan {
    /// Id of the model the plan runs.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Every config, indexed by config id.
    pub fn configs(&self) -> &[Config] {
        &self.configs
    }

    pub fn config(&self, config_id: u64) -> Option<&Config> {
        self.configs.get(usize::try_from(config_id).ok()?)
    }

    pub fn blocks(&self) -> &[PlannedBlock] {
        &self.blocks
    }

    pub fn run_settings(&self) -> &RunSettings {
        &self.run_settings
    }

    pub fn measure_settings(&self) -> &MeasureSettings {
        &self.measure
    }

    pub fn seed_settings(&self) -> SeedSettings {
        self.seeds
    }

    /// Actions every run fires, in spec order, the order of [`Config::action_ticks`].
    pub fn actions(&self) -> &[ActionSpec] {
        &self.actions
    }

    pub fn warnings(&self) -> &[PlanWarning] {
        &self.warnings
    }

    pub fn replicates(&self) -> u64 {
        self.run_settings.replicates
    }

    /// Number of runs, every config times its replicates.
    pub fn run_count(&self) -> u64 {
        self.run_count
    }

    /// Returns run `run_id`, or `None` past the last run.
    pub fn run(&self, run_id: u64) -> Option<PlannedRun> {
        (run_id < self.run_count).then(|| self.planned_run(run_id))
    }

    /// Returns every run in order.
    pub fn runs(&self) -> impl Iterator<Item = PlannedRun> + '_ {
        (0..self.run_count).map(|run_id| self.planned_run(run_id))
    }

    /// Returns the runs of `shard` in order.
    pub fn runs_in_shard(&self, shard: Shard) -> impl Iterator<Item = PlannedRun> + '_ {
        (0..shard.run_count(self.run_count)).map(move |position| self.planned_run(shard.index + position * shard.count))
    }

    fn planned_run(&self, run_id: u64) -> PlannedRun {
        let config_id = run_id / self.run_settings.replicates;
        let rep = run_id % self.run_settings.replicates;
        PlannedRun {
            run_id,
            config_id,
            rep,
            seed: self.seeds.scheme.seed(self.seeds.root, config_id, rep),
        }
    }

    /// Returns the actions of `config` in the order they fire, by tick and then in spec order.
    ///
    /// # Panics
    ///
    /// Panics when `config` does not hold one tick per action of the plan.
    pub fn schedule(&self, config: &Config) -> Schedule {
        assert_eq!(
            config.action_ticks.len(),
            self.actions.len(),
            "a config holds one tick per action"
        );
        let mut entries: Vec<Scheduled> = self
            .actions
            .iter()
            .zip(&self.action_indices)
            .zip(&config.action_ticks)
            .map(|((action, &index), &tick)| Scheduled {
                index,
                id: action.id.clone(),
                tick,
            })
            .collect();
        entries.sort_by_key(|entry| entry.tick);
        Schedule::from_entries(entries)
    }

    /// Returns the [`Replay`] of run `run_id`, or `None` past the last run.
    pub fn replay(&self, run_id: u64) -> Option<Replay> {
        let run = self.run(run_id)?;
        let config = self.config(run.config_id)?;
        Some(Replay {
            model: self.model.clone(),
            params: config.params.clone(),
            seed: run.seed,
            schedule: self.schedule(config),
            ticks: self.run_settings.warmup + self.run_settings.steps,
            label: format!("Sweep run {run_id}: config {}, replicate {}", run.config_id, run.rep),
        })
    }

    /// Returns the key naming the results of `run`, from [`run_key`].
    ///
    /// # Panics
    ///
    /// Panics when the config of `run` is not in this plan.
    pub fn run_key(&self, run: &PlannedRun) -> u64 {
        let config = self
            .config(run.config_id)
            .expect("a planned run's config is in its plan");
        run_key(self.results_fingerprint, &config.params, &config.action_ticks, run.seed)
    }

    /// Hash of the model's declarations, from [`schema_hash`].
    pub fn schema_hash(&self) -> u64 {
        self.schema_hash
    }

    /// Hash of every setting that shapes the results of one run, from [`results_fingerprint`].
    pub fn results_fingerprint(&self) -> u64 {
        self.results_fingerprint
    }

    /// Hash of every setting that fixes the configs and their seeds, from [`plan_hash`].
    ///
    /// Note that the replicate count and the timeout are left out.
    pub fn plan_hash(&self) -> u64 {
        self.plan_hash
    }
}

impl SweepSpec {
    /// Checks the spec against `schema`, and lists its configs.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] for a spec naming another model, no replicates, settings [`MeasurePlan::check`]
    /// refuses, a reducer or stop condition over no stat label, an action the model does not declare or a name two
    /// actions share, a value or factor the model's parameters or the spec's actions refuse, a parameter both fixed
    /// and varied, a target varied twice in one block, or a block its design cannot combine.
    pub fn plan(&self, schema: &ModelSchema<'_>) -> Result<Plan, PlanError> {
        if self.model != schema.id {
            return Err(PlanError::WrongModel {
                spec_model: self.model.clone(),
                schema_model: schema.id.to_owned(),
            });
        }
        if self.run.replicates == 0 {
            return Err(PlanError::NoReplicates);
        }
        MeasurePlan::check(&self.run, &self.measure).map_err(PlanError::Measure)?;
        for reducer in &self.measure.reducers {
            reducer
                .check_label(schema.stats)
                .map_err(|error| PlanError::Measure(MeasureError::Reducer(error)))?;
        }
        if let Some(stop) = &self.run.stop {
            stop.check_label(schema.stats).map_err(PlanError::Stop)?;
        }
        let action_indices = self.resolve_actions(schema)?;
        let fixed = resolve_params(schema.params, &self.fixed).map_err(|error| match error {
            ValueError::UnknownParam { id, known } => PlanError::UnknownFixed { id, known },
            error => PlanError::Fixed(error),
        })?;
        let fixed_ticks: Vec<u64> = self.actions.iter().map(|action| action.tick).collect();

        let lone_block = BlockSpec::default();
        let block_specs = if self.blocks.is_empty() {
            std::slice::from_ref(&lone_block)
        } else {
            self.blocks.as_slice()
        };
        let mut configs = Vec::new();
        let mut blocks = Vec::with_capacity(block_specs.len());
        for (index, block_spec) in block_specs.iter().enumerate() {
            let block = self.resolve_block(index, block_spec, schema)?;
            let base = Config {
                block: index,
                params: fixed.clone(),
                action_ticks: fixed_ticks.clone(),
            };
            let start = configs.len() as u64;
            configs.extend(generate(&block, &base).map_err(|source| PlanError::Design { block: index, source })?);
            blocks.push(PlannedBlock {
                design: block.design.clone(),
                configs: start..configs.len() as u64,
                design_seed: block.design.is_sampled().then_some(block.design_seed),
            });
        }

        let run_count = (configs.len() as u64)
            .checked_mul(self.run.replicates)
            .ok_or(PlanError::TooManyRuns)?;
        let schema_hash = schema_hash(schema);
        let results_fingerprint = results_fingerprint(schema_hash, &self.run, &self.measure, &self.actions);
        let plan_hash = plan_hash(results_fingerprint, &self.seeds, &self.actions, &blocks, &configs);
        let warnings = late_actions(&self.actions, &configs, self.run.warmup + self.run.steps);
        Ok(Plan {
            model: self.model.clone(),
            configs,
            blocks,
            run_settings: self.run.clone(),
            measure: self.measure.clone(),
            seeds: self.seeds,
            actions: self.actions.clone(),
            action_indices,
            warnings,
            run_count,
            schema_hash,
            results_fingerprint,
            plan_hash,
        })
    }

    /// Returns the index of each action in the model's declarations.
    fn resolve_actions(&self, schema: &ModelSchema<'_>) -> Result<Vec<usize>, PlanError> {
        let mut indices = Vec::with_capacity(self.actions.len());
        for (position, action) in self.actions.iter().enumerate() {
            if self.actions[..position]
                .iter()
                .any(|earlier| earlier.name == action.name)
            {
                return Err(PlanError::DuplicateActionName {
                    name: action.name.clone(),
                });
            }
            let index = schema
                .actions
                .iter()
                .position(|declared| declared.id == action.id)
                .ok_or_else(|| PlanError::UnknownAction {
                    id: action.id.clone(),
                    known: schema.actions.iter().map(|declared| declared.id).collect(),
                })?;
            indices.push(index);
        }
        Ok(indices)
    }

    /// Resolves the factors of block `index` against the model's parameters and the spec's actions.
    fn resolve_block(
        &self,
        index: usize,
        block_spec: &BlockSpec,
        schema: &ModelSchema<'_>,
    ) -> Result<Block, PlanError> {
        let factors = if let DesignKind::Table { text } = &block_spec.design {
            if !block_spec.factors.is_empty() {
                return Err(PlanError::TableWithFactors { block: index });
            }
            read_table(text, schema.params, &self.actions)
                .map_err(|source| PlanError::Table { block: index, source })?
        } else {
            let mut factors = Vec::with_capacity(block_spec.factors.len());
            for (position, factor) in block_spec.factors.iter().enumerate() {
                if block_spec.factors[..position]
                    .iter()
                    .any(|earlier| earlier.target == factor.target)
                {
                    return Err(PlanError::VariedTwice {
                        target: factor.target.clone(),
                        block: index,
                    });
                }
                factors.push(
                    factor
                        .resolve(schema.params, &self.actions, &block_spec.design)
                        .map_err(|source| PlanError::Factor { block: index, source })?,
                );
            }
            factors
        };
        for factor in &factors {
            if let FactorSlot::Param(param) = factor.slot {
                let id = schema.params[param].id;
                if self.fixed.iter().any(|(fixed_id, _)| fixed_id == id) {
                    return Err(PlanError::FixedAndVaried {
                        id: id.to_owned(),
                        block: index,
                    });
                }
            }
        }
        Ok(Block {
            design: block_spec.design.clone(),
            factors,
            design_seed: block_spec
                .design_seed
                .unwrap_or_else(|| design_seed(self.seeds.root, index)),
        })
    }
}

/// Returns a warning for each action due after `last_tick` in some config.
fn late_actions(actions: &[ActionSpec], configs: &[Config], last_tick: u64) -> Vec<PlanWarning> {
    let mut warnings = Vec::new();
    for (position, action) in actions.iter().enumerate() {
        let late = configs
            .iter()
            .map(|config| config.action_ticks[position])
            .filter(|&tick| tick > last_tick);
        let (count, latest) = late.fold((0, 0), |(count, latest), tick| (count + 1, latest.max(tick)));
        if count > 0 {
            warnings.push(PlanWarning::ActionAfterEnd {
                name: action.name.clone(),
                latest_tick: latest,
                last_tick,
                config_count: count,
            });
        }
    }
    warnings
}

/// A sweep spec that cannot be planned against a model.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// A spec for model `spec_model`, checked against model `schema_model`.
    WrongModel { spec_model: String, schema_model: String },
    /// A replicate count of 0.
    NoReplicates,
    /// Settings that cannot measure a run, for the reason inside.
    Measure(MeasureError),
    /// A stop condition over no stat label, for the reason inside.
    Stop(StopError),
    /// An action id the model does not declare. `known` lists the ids it does.
    UnknownAction { id: String, known: Vec<&'static str> },
    /// A name two actions of the spec share.
    DuplicateActionName { name: String },
    /// A fixed value refused for the reason inside.
    Fixed(ValueError),
    /// A fixed value for an id no parameter has. `known` lists the ids the parameters do have.
    UnknownFixed { id: String, known: Vec<&'static str> },
    /// Parameter `id`, varied in block `block` and fixed as well.
    FixedAndVaried { id: String, block: usize },
    /// A target varied twice in block `block`.
    VariedTwice { target: FactorTarget, block: usize },
    /// A factor of block `block`, refused for the reason in `source`.
    Factor { block: usize, source: FactorError },
    /// A table design in block `block` that lists factors of its own.
    TableWithFactors { block: usize },
    /// The table of block `block`, refused for the reason in `source`.
    Table { block: usize, source: DesignTableError },
    /// Block `block`, whose design cannot combine its factors for the reason in `source`.
    Design { block: usize, source: DesignError },
    /// More runs than a 64-bit count holds.
    TooManyRuns,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongModel {
                spec_model,
                schema_model,
            } => {
                write!(f, "spec is for model '{spec_model}', expected '{schema_model}'")
            }
            Self::NoReplicates => write!(f, "replicates must be at least 1"),
            Self::Measure(_) => write!(f, "measurement settings"),
            Self::Stop(_) => write!(f, "stop condition"),
            Self::UnknownAction { id, known } if known.is_empty() => {
                write!(f, "unknown action '{id}' (model declares no actions)")
            }
            Self::UnknownAction { id, known } => {
                write!(f, "unknown action '{id}', expected one of {}", known.join(", "))
            }
            Self::DuplicateActionName { name } => {
                write!(f, "two actions are named '{name}', and a name must be unique")
            }
            Self::Fixed(_) => write!(f, "fixed values"),
            Self::UnknownFixed { id, known } => {
                write!(f, "unknown parameter '{id}', expected one of {}", known.join(", "))
            }
            Self::FixedAndVaried { id, block } => {
                write!(f, "parameter '{id}' is both fixed and varied in block {block}")
            }
            Self::VariedTwice { target, block } => write!(f, "block {block} varies {target} twice"),
            Self::TableWithFactors { block } => {
                write!(f, "block {block} reads its configs from a table, and takes no factors")
            }
            Self::Factor { block, .. } | Self::Table { block, .. } | Self::Design { block, .. } => {
                write!(f, "block {block}")
            }
            Self::TooManyRuns => write!(f, "plan has more runs than a 64-bit integer can hold"),
        }
    }
}

impl std::error::Error for PlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Measure(error) => Some(error),
            Self::Stop(error) => Some(error),
            Self::Fixed(error) => Some(error),
            Self::Factor { source, .. } => Some(source),
            Self::Table { source, .. } => Some(source),
            Self::Design { source, .. } => Some(source),
            Self::WrongModel { .. }
            | Self::NoReplicates
            | Self::UnknownAction { .. }
            | Self::DuplicateActionName { .. }
            | Self::UnknownFixed { .. }
            | Self::FixedAndVaried { .. }
            | Self::VariedTwice { .. }
            | Self::TableWithFactors { .. }
            | Self::TooManyRuns => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ModelSchema, PlanError, PlanWarning, Shard, ShardError};
    use crate::action::{ActionDescriptor, Scheduled};
    use crate::explore::design::{DesignError, DesignKind};
    use crate::explore::design_csv::DesignTableError;
    use crate::explore::factor::{FactorError, FactorSpec, FactorTarget, LevelSpec};
    use crate::explore::measure::MeasureError;
    use crate::explore::reducer::{ReducerError, ReducerKind, ReducerSpec};
    use crate::explore::seed::{SeedScheme, design_seed, run_seed};
    use crate::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
    use crate::explore::stop::{StopError, StopSpec};
    use crate::helpers::{choice_param, f32_param, u32_param};
    use crate::params::{ParamDescriptor, ParamValue};
    use crate::view::StatDescriptor;

    const COLOR: [u8; 4] = [0, 0, 0, 255];
    const STATS: &[StatDescriptor] = &[
        StatDescriptor::new("Infected", COLOR),
        StatDescriptor::new("Recovered", COLOR),
    ];
    const ACTIONS: &[ActionDescriptor] = &[
        ActionDescriptor::new("seed_outbreak", "Seed outbreak"),
        ActionDescriptor::new("vaccinate", "Vaccinate"),
    ];
    const NEIGHBORHOODS: &[&str] = &["moore", "von_neumann"];

    fn params() -> Vec<ParamDescriptor> {
        vec![
            f32_param("infection_rate", "Infection rate", 0.2, 0.0, 1.0, Some(0.01)),
            f32_param("recovery_rate", "Recovery rate", 0.05, 0.0, 1.0, None),
            u32_param("grid_width", "Grid width", 64, 1, 4096),
            choice_param("neighborhood", "Neighborhood", NEIGHBORHOODS, 0),
        ]
    }

    fn values(raw: &[&str]) -> LevelSpec {
        LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
    }

    fn block(design: DesignKind, factors: Vec<FactorSpec>) -> BlockSpec {
        BlockSpec {
            design,
            factors,
            design_seed: None,
        }
    }

    fn plan(spec: &SweepSpec) -> Result<super::Plan, PlanError> {
        let params = params();
        let schema = ModelSchema {
            id: "sir",
            params: &params,
            stats: STATS,
            actions: ACTIONS,
        };
        spec.plan(&schema)
    }

    /// Returns a spec with two actions, the second named `second_wave`.
    fn spec_with_actions() -> SweepSpec {
        let mut spec = SweepSpec::new("sir");
        spec.run.steps = 100;
        spec.run.replicates = 2;
        spec.actions = vec![
            ActionSpec::new("seed_outbreak", 50),
            ActionSpec {
                name: "second_wave".to_owned(),
                ..ActionSpec::new("seed_outbreak", 50)
            },
        ];
        spec
    }

    #[test]
    fn blocks_concatenate_and_number_configs_in_order() {
        let mut spec = SweepSpec::new("sir");
        spec.blocks = vec![
            block(
                DesignKind::Factorial,
                vec![FactorSpec::param("grid_width", values(&["16", "32"]))],
            ),
            block(
                DesignKind::Zip,
                vec![
                    FactorSpec::param("infection_rate", values(&["0.1", "0.2", "0.3"])),
                    FactorSpec::param("neighborhood", values(&["von_neumann", "moore", "1"])),
                ],
            ),
        ];
        let plan = plan(&spec).expect("a valid spec");
        let blocks: Vec<usize> = plan.configs().iter().map(|config| config.block).collect();
        assert_eq!(blocks, [0, 0, 1, 1, 1]);
        assert_eq!(plan.blocks()[0].configs, 0..2);
        assert_eq!(plan.blocks()[1].configs, 2..5);
        assert_eq!(plan.blocks()[1].design, DesignKind::Zip);
        let second = plan.config(3).expect("config 3 exists");
        assert_eq!(
            second.params,
            [
                ParamValue::F32(0.2),
                ParamValue::F32(0.05),
                ParamValue::U32(64),
                ParamValue::Choice(0)
            ],
            "a block leaves the parameters it does not vary at their fixed values"
        );
    }

    #[test]
    fn runs_are_numbered_config_by_config() {
        let mut spec = SweepSpec::new("sir");
        spec.run.replicates = 3;
        spec.seeds.root = 42;
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![FactorSpec::param("grid_width", values(&["16", "32"]))],
        )];
        let common = plan(&spec).expect("a valid spec");
        assert_eq!(common.run_count(), 6);
        let runs: Vec<(u64, u64, u64)> = common.runs().map(|run| (run.run_id, run.config_id, run.rep)).collect();
        assert_eq!(runs, [(0, 0, 0), (1, 0, 1), (2, 0, 2), (3, 1, 0), (4, 1, 1), (5, 1, 2)]);
        let fifth = common.run(4).expect("run 4 exists");
        let second = common.run(1).expect("run 1 exists");
        assert_eq!(fifth.seed, run_seed(42, 1), "common random numbers by default");
        assert_eq!(fifth.seed, second.seed);
        assert_ne!(common.run_key(&fifth), common.run_key(&second), "the configs differ");
        assert_eq!(common.run(6), None);

        spec.seeds.scheme = SeedScheme::Independent;
        let independent = plan(&spec).expect("a valid spec");
        assert_ne!(
            independent.run(4).map(|run| run.seed),
            independent.run(1).map(|run| run.seed)
        );
    }

    #[test]
    fn a_spec_with_no_blocks_runs_its_fixed_values() {
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![("grid_width".to_owned(), "128".to_owned())];
        let plan = plan(&spec).expect("a valid spec");
        assert_eq!(plan.configs().len(), 1);
        assert_eq!(plan.configs()[0].params[2], ParamValue::U32(128));
        assert_eq!(plan.blocks().len(), 1);
    }

    #[test]
    fn a_parameter_varied_twice_in_a_block_is_refused() {
        let mut spec = SweepSpec::new("sir");
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![
                FactorSpec::param("grid_width", values(&["16"])),
                FactorSpec::param("infection_rate", values(&["0.1"])),
                FactorSpec::param("grid_width", values(&["32"])),
            ],
        )];
        assert_eq!(
            plan(&spec),
            Err(PlanError::VariedTwice {
                target: FactorTarget::Param("grid_width".to_owned()),
                block: 0
            })
        );
        assert_eq!(
            plan(&spec).map_err(|error| error.to_string()),
            Err("block 0 varies parameter 'grid_width' twice".to_owned())
        );
        spec.blocks.insert(0, block(DesignKind::Factorial, Vec::new()));
        assert!(
            matches!(plan(&spec), Err(PlanError::VariedTwice { block: 1, .. })),
            "the error names the block"
        );

        let mut apart = SweepSpec::new("sir");
        apart.blocks = vec![
            block(
                DesignKind::Factorial,
                vec![FactorSpec::param("grid_width", values(&["16"]))],
            ),
            block(
                DesignKind::Factorial,
                vec![FactorSpec::param("grid_width", values(&["32"]))],
            ),
        ];
        assert!(plan(&apart).is_ok(), "two blocks can vary one parameter");
    }

    #[test]
    fn a_parameter_both_fixed_and_varied_is_refused() {
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![("grid_width".to_owned(), "128".to_owned())];
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![FactorSpec::param("grid_width", values(&["16", "32"]))],
        )];
        assert_eq!(
            plan(&spec),
            Err(PlanError::FixedAndVaried {
                id: "grid_width".to_owned(),
                block: 0
            })
        );
    }

    #[test]
    fn a_spec_the_model_refuses_is_refused() {
        let mut other_model = SweepSpec::new("boids");
        assert!(matches!(plan(&other_model), Err(PlanError::WrongModel { .. })));
        other_model.model = "sir".to_owned();

        let mut spec = other_model.clone();
        spec.run.replicates = 0;
        assert_eq!(plan(&spec), Err(PlanError::NoReplicates));

        let mut spec = other_model.clone();
        spec.measure.stats_every = 0;
        assert_eq!(plan(&spec), Err(PlanError::Measure(MeasureError::ZeroStatsEvery)));

        let mut spec = other_model.clone();
        spec.measure.reducers = vec![ReducerSpec {
            column: "Susceptible".to_owned(),
            kind: ReducerKind::Max,
        }];
        assert!(matches!(
            plan(&spec),
            Err(PlanError::Measure(MeasureError::Reducer(
                ReducerError::UnknownColumn { .. }
            )))
        ));

        let mut spec = other_model.clone();
        spec.fixed = vec![("grid_width".to_owned(), "0".to_owned())];
        assert!(matches!(plan(&spec), Err(PlanError::Fixed(_))));

        let mut spec = other_model.clone();
        spec.fixed = vec![("nope".to_owned(), "1".to_owned())];
        let error = plan(&spec).expect_err("the model has no parameter 'nope'");
        assert_eq!(
            error.to_string(),
            "unknown parameter 'nope', expected one of infection_rate, recovery_rate, grid_width, neighborhood"
        );

        let mut spec = other_model.clone();
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.1,
                    max: 0.5,
                    step: None,
                },
            )],
        )];
        assert!(
            matches!(
                plan(&spec),
                Err(PlanError::Factor {
                    block: 0,
                    source: FactorError::MissingStep { .. }
                })
            ),
            "a factorial over an F32 range needs a step"
        );

        let mut spec = other_model;
        spec.blocks = vec![block(
            DesignKind::Zip,
            vec![
                FactorSpec::param("grid_width", values(&["16", "32"])),
                FactorSpec::param("infection_rate", values(&["0.1"])),
            ],
        )];
        assert!(matches!(
            plan(&spec),
            Err(PlanError::Design {
                block: 0,
                source: DesignError::UnequalLengths { .. }
            })
        ));
    }

    #[test]
    fn an_action_tick_factor_moves_the_action() {
        let mut spec = spec_with_actions();
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![
                FactorSpec::param("grid_width", values(&["16", "32"])),
                FactorSpec::action("second_wave", values(&["10", "90"])),
            ],
        )];
        let plan = plan(&spec).expect("a valid spec");
        let ticks: Vec<Vec<u64>> = plan
            .configs()
            .iter()
            .map(|config| config.action_ticks.clone())
            .collect();
        assert_eq!(
            ticks,
            [[50, 10], [50, 90], [50, 10], [50, 90]],
            "the other action keeps its tick"
        );
        let run = plan.run(1).expect("run 1 exists");
        let moved = plan.run(3).expect("run 3 exists");
        assert_eq!(run.seed, moved.seed);
        assert_ne!(plan.run_key(&run), plan.run_key(&moved), "the tick is part of the key");
        assert!(plan.warnings().is_empty());
    }

    #[test]
    fn a_replay_holds_its_runs_config_seed_and_schedule() {
        let mut spec = spec_with_actions();
        spec.run.warmup = 5;
        spec.seeds.root = 9;
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![
                FactorSpec::param("grid_width", values(&["16", "32"])),
                FactorSpec::action("second_wave", values(&["10"])),
            ],
        )];
        let plan = plan(&spec).expect("a valid spec");
        assert_eq!(plan.model(), "sir");
        let run = plan.run(3).expect("run 3 exists");
        let config = plan.config(run.config_id).expect("the config exists");
        let replay = plan.replay(3).expect("run 3 exists");
        assert_eq!(replay.model, "sir");
        assert_eq!(replay.params, config.params);
        assert_eq!(replay.params[2], ParamValue::U32(32));
        assert_eq!(replay.seed, run.seed);
        assert_eq!(replay.schedule, plan.schedule(config));
        let ticks: Vec<u64> = replay.schedule.entries().iter().map(|entry| entry.tick).collect();
        assert_eq!(ticks, [10, 50]);
        assert_eq!(replay.ticks, 105, "the warm-up is part of the run");
        assert_eq!(replay.label, "Sweep run 3: config 1, replicate 1");
        assert_eq!(plan.replay(plan.run_count()), None);
    }

    #[test]
    fn a_schedule_orders_by_tick_then_spec_order() {
        let mut spec = spec_with_actions();
        spec.actions.push(ActionSpec {
            name: "vaccination".to_owned(),
            ..ActionSpec::new("vaccinate", 20)
        });
        spec.blocks = vec![block(
            DesignKind::Zip,
            vec![FactorSpec::action("second_wave", values(&["50", "0"]))],
        )];
        let plan = plan(&spec).expect("a valid spec");
        let order = |config_id: u64| {
            let config = plan.config(config_id).expect("the config exists");
            plan.schedule(config)
                .entries()
                .iter()
                .map(|Scheduled { index, tick, .. }| (*index, *tick))
                .collect::<Vec<_>>()
        };
        assert_eq!(order(0), [(1, 20), (0, 50), (0, 50)], "two at one tick keep spec order");
        assert_eq!(order(1), [(0, 0), (1, 20), (0, 50)]);
        assert_eq!(plan.schedule(&plan.configs()[1]).entries()[0].id, "seed_outbreak");
    }

    #[test]
    fn an_action_the_spec_cannot_fire_is_refused() {
        let mut spec = spec_with_actions();
        spec.actions[1].name = "seed_outbreak".to_owned();
        assert_eq!(
            plan(&spec),
            Err(PlanError::DuplicateActionName {
                name: "seed_outbreak".to_owned()
            })
        );

        let mut spec = spec_with_actions();
        spec.actions.push(ActionSpec::new("reset", 3));
        let error = plan(&spec).expect_err("no action 'reset'");
        assert_eq!(
            error.to_string(),
            "unknown action 'reset', expected one of seed_outbreak, vaccinate"
        );

        let mut spec = spec_with_actions();
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![FactorSpec::action("third_wave", values(&["10"]))],
        )];
        assert!(matches!(
            plan(&spec),
            Err(PlanError::Factor {
                source: FactorError::UnknownAction { .. },
                ..
            })
        ));

        let mut spec = spec_with_actions();
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![
                FactorSpec::action("second_wave", values(&["10"])),
                FactorSpec::action("second_wave", values(&["20"])),
            ],
        )];
        assert!(matches!(
            plan(&spec),
            Err(PlanError::VariedTwice {
                target: FactorTarget::Action(_),
                ..
            })
        ));
    }

    #[test]
    fn an_action_past_the_end_is_a_warning() {
        let mut spec = spec_with_actions();
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![FactorSpec::action("second_wave", values(&["100", "101", "150"]))],
        )];
        let plan = plan(&spec).expect("a late action still plans");
        assert_eq!(
            plan.warnings(),
            [PlanWarning::ActionAfterEnd {
                name: "second_wave".to_owned(),
                latest_tick: 150,
                last_tick: 100,
                config_count: 2,
            }],
            "tick 100 is the last tick, and fires after the last step"
        );
        let lone = PlanWarning::ActionAfterEnd {
            name: "second_wave".to_owned(),
            latest_tick: 150,
            last_tick: 100,
            config_count: 1,
        };
        assert_eq!(
            lone.to_string(),
            "action 'second_wave' is due after the last tick 100 in 1 config (latest tick 150) and will not run there"
        );
    }

    #[test]
    fn a_stop_over_an_unknown_label_is_refused() {
        let mut spec = SweepSpec::new("sir");
        spec.run.stop = Some(StopSpec::parse("Infected <= 0", 5).expect("a well-formed condition"));
        assert!(plan(&spec).is_ok());
        spec.run.stop = Some(StopSpec::parse("Susceptible <= 0", 5).expect("a well-formed condition"));
        assert!(matches!(
            plan(&spec),
            Err(PlanError::Stop(StopError::UnknownColumn { .. }))
        ));
    }

    #[test]
    fn a_sampled_block_records_its_design_seed() {
        let mut spec = spec_with_actions();
        spec.seeds.root = 42;
        let sampled = |design_seed| BlockSpec {
            design_seed,
            ..block(
                DesignKind::LatinHypercube { samples: 6 },
                vec![
                    FactorSpec::param(
                        "infection_rate",
                        LevelSpec::Range {
                            min: 0.1,
                            max: 0.9,
                            step: None,
                        },
                    ),
                    FactorSpec::action(
                        "second_wave",
                        LevelSpec::Range {
                            min: 0.0,
                            max: 100.0,
                            step: None,
                        },
                    ),
                ],
            )
        };
        spec.blocks = vec![block(DesignKind::Zip, Vec::new()), sampled(None), sampled(Some(7))];
        let plan = plan(&spec).expect("a valid spec");
        let seeds: Vec<Option<u64>> = plan.blocks().iter().map(|block| block.design_seed).collect();
        assert_eq!(seeds, [None, Some(design_seed(42, 1)), Some(7)]);
        assert_eq!(plan.configs().len(), 13);
        assert_eq!(plan.blocks()[2].configs, 7..13);
    }

    #[test]
    fn a_table_block_reads_its_configs() {
        let mut spec = spec_with_actions();
        spec.fixed = vec![("recovery_rate".to_owned(), "0.1".to_owned())];
        let table = "infection_rate,action.second_wave\n0.25,5\n0.5,6\n";
        spec.blocks = vec![block(DesignKind::Table { text: table.to_owned() }, Vec::new())];
        let table_plan = plan(&spec).expect("a valid spec");
        assert_eq!(table_plan.configs().len(), 2);
        assert_eq!(table_plan.configs()[1].params[0], ParamValue::F32(0.5));
        assert_eq!(
            table_plan.configs()[1].params[1],
            ParamValue::F32(0.1),
            "fixed values fill the rest"
        );
        assert_eq!(table_plan.configs()[1].action_ticks, [50, 6]);
        assert_eq!(table_plan.blocks()[0].design_seed, None);

        let mut listed = spec.clone();
        listed.blocks[0].factors = vec![FactorSpec::param("grid_width", values(&["16"]))];
        assert_eq!(plan(&listed), Err(PlanError::TableWithFactors { block: 0 }));

        let mut fixed = spec.clone();
        fixed.fixed = vec![("infection_rate".to_owned(), "0.1".to_owned())];
        assert!(matches!(plan(&fixed), Err(PlanError::FixedAndVaried { .. })));

        let mut unknown = spec;
        unknown.blocks[0].design = DesignKind::Table {
            text: "speed\n1\n".to_owned(),
        };
        assert!(matches!(
            plan(&unknown),
            Err(PlanError::Table {
                block: 0,
                source: DesignTableError::UnknownColumn { .. }
            })
        ));
    }

    #[test]
    fn shards_partition_the_runs() {
        let mut spec = SweepSpec::new("sir");
        spec.run.replicates = 3;
        spec.blocks = vec![block(
            DesignKind::Factorial,
            vec![FactorSpec::param("grid_width", values(&["16", "32", "48", "64", "80"]))],
        )];
        let plan = plan(&spec).expect("a valid spec");
        let every: Vec<u64> = plan.runs().map(|run| run.run_id).collect();
        assert_eq!(
            plan.runs_in_shard(Shard::WHOLE).collect::<Vec<_>>(),
            plan.runs().collect::<Vec<_>>()
        );
        for count in 1..=17 {
            let mut seen = BTreeSet::new();
            for index in 0..count {
                let shard = Shard::new(index, count).expect("a valid shard");
                let runs: Vec<u64> = plan.runs_in_shard(shard).map(|run| run.run_id).collect();
                assert_eq!(runs.len() as u64, shard.run_count(plan.run_count()), "shard {shard}");
                assert!(
                    runs.iter()
                        .all(|&run_id| run_id % count == index && shard.contains(run_id))
                );
                assert!(runs.windows(2).all(|pair| pair[0] < pair[1]), "in run order");
                for run_id in runs {
                    assert!(seen.insert(run_id), "run {run_id} is in one shard only");
                }
            }
            assert_eq!(
                seen.into_iter().collect::<Vec<_>>(),
                every,
                "{count} shards cover every run"
            );
        }
        let second_run = plan.runs_in_shard(Shard::new(1, 4).expect("a valid shard")).nth(1);
        assert_eq!(second_run, plan.run(5), "shard runs carry their plan's seeds");
    }

    #[test]
    fn a_shard_reads_as_index_over_count() {
        assert_eq!("2/5".parse::<Shard>(), Shard::new(2, 5));
        assert_eq!("0/1".parse::<Shard>(), Ok(Shard::WHOLE));
        assert_eq!(Shard::new(3, 8).map(|shard| shard.to_string()), Ok("3/8".to_owned()));
        assert_eq!("1/0".parse::<Shard>(), Err(ShardError::ZeroCount));
        assert_eq!(
            "4/4".parse::<Shard>(),
            Err(ShardError::IndexPastCount { index: 4, count: 4 })
        );
        for bad in ["", "1", "1/", "a/2", "-1/2", "1/2/3"] {
            assert_eq!(
                bad.parse::<Shard>(),
                Err(ShardError::BadText { raw: bad.to_owned() }),
                "{bad}"
            );
        }
        assert_eq!(Shard::WHOLE.run_count(0), 0);
        assert_eq!(Shard::new(5, 6).map(|shard| shard.run_count(5)), Ok(0));
    }
}
