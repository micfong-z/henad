//! Conversion of a sweep draft to a sweep spec and back, and the TOML of a spec file.

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use henad_core::explore::plan::ModelSchema;
use henad_core::explore::seed::SeedScheme;
use henad_core::explore::spec::{BlockSpec, MeasureSettings, RunSettings, SeedSettings, SweepSpec};
use henad_core::explore::stop::{Comparison, StopSpec};
use henad_core::explore::value::{format_value, parse_value};
use henad_core::params::ParamValue;
use henad_explore::exec::Concurrency;
use henad_explore::spec_file::{ExecutionTable, LoadedSpec, SpecFile};
use henad_explore::sweep::SpecSource;

use crate::ui::text::describe_error;

use super::levels::{levels_text, row_levels};
use super::messages::{design_seed_message, root_seed_message};
use super::{
    ActionDraft, DesignTableDraft, DraftDesign, DraftIssue, DraftSite, ExploreMode, FactorDraft, NO_SUCH_ACTION,
    NO_TABLE, SPEC_TABLE_NAME, StopDraft, SweepDraft,
};

impl SweepDraft {
    /// Returns the spec the draft writes, with each parameter it does not vary at its value in `panel_values`.
    ///
    /// # Errors
    ///
    /// Returns every [`DraftIssue`] of text that does not parse, a design table that is missing, an action the model
    /// does not declare, or a timeout that is not a [`Duration`](std::time::Duration). Everything else is checked by
    /// [`Self::check`].
    pub fn to_spec(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Result<SweepSpec, Vec<DraftIssue>> {
        let mut issues = Vec::new();
        let mut spec = SweepSpec::new(self.model_id.clone());

        spec.actions = self.declared_actions(schema);
        for (position, action) in self.actions.iter().enumerate() {
            if schema.actions.get(action.action_index).is_none() {
                issues.push(DraftIssue::new(DraftSite::Action(position), NO_SUCH_ACTION));
            }
        }

        let root = parse_seed_text(&self.root_seed_text).unwrap_or_else(|message| {
            issues.push(DraftIssue::new(DraftSite::Seed, root_seed_message(&message)));
            0
        });
        spec.seeds = SeedSettings {
            root,
            scheme: if self.common_random_numbers {
                SeedScheme::Common
            } else {
                SeedScheme::Independent
            },
        };

        match self.mode {
            ExploreMode::Sweep => spec.blocks = self.blocks(schema, panel_values, &mut issues),
            ExploreMode::Search => {
                let (params, actions) = self.varied_factors(schema, &mut issues);
                let space = params.into_iter().map(|(_, factor)| factor).chain(actions).collect();
                spec.search = Some(self.search.to_spec(space));
            }
        }
        spec.fixed = self.fixed_values(schema, panel_values);
        let timeout = self.timeout().unwrap_or_else(|issue| {
            issues.push(issue);
            None
        });
        spec.run = RunSettings {
            steps: self.steps,
            warmup: self.warmup,
            replicates: self.replicates,
            stop: self.stop.as_ref().map(|stop| StopSpec {
                column: stop.column.clone(),
                comparison: Comparison {
                    comparator: stop.comparator,
                    threshold: stop.threshold,
                },
                min_tick: stop.min_tick,
            }),
            timeout,
        };
        spec.measure = MeasureSettings {
            stats_every: self.stats_every,
            series_every: self.series_every,
            default_reducers: self.default_reducers,
            reducers: self.reducers.clone(),
        };

        if issues.is_empty() { Ok(spec) } else { Err(issues) }
    }

    /// Returns the spec file of the draft as TOML, its concurrency and memory budgets included.
    ///
    /// # Errors
    ///
    /// Returns the issues of [`Self::to_spec`], and the issue of each value a spec file rejects: a window that ends
    /// before it starts, or a threshold that is not finite. Returns the reason that [`Self::from_spec_file`]
    /// reports for any other file it would fail to read back.
    pub fn to_toml(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Result<String, Vec<DraftIssue>> {
        let refused: Vec<DraftIssue> = self.stop_issue().into_iter().chain(self.output_issues()).collect();
        if !refused.is_empty() {
            return Err(refused);
        }
        let loaded = LoadedSpec {
            spec: self.to_spec(schema, panel_values)?,
            spec_source: SpecSource::default(),
            execution: ExecutionTable {
                concurrent: self.concurrency,
                memory: self.memory_budget,
                gpu_memory: self.gpu_memory_budget,
            },
        };
        let text = loaded
            .to_toml()
            .map_err(|error| vec![DraftIssue::new(DraftSite::Sweep, describe_error(&error))])?;
        SpecFile::parse(&text)
            .map_err(|error| describe_error(&error))
            .and_then(|file| Self::from_spec_file(file, schema))
            .map_err(|message| vec![DraftIssue::new(DraftSite::Sweep, message)])?;
        Ok(text)
    }

    /// Reads a draft from `file` over `schema`'s model, with the Parameters tab values the file sets.
    ///
    /// # Errors
    ///
    /// Returns the reason for a file that is not a valid spec, or for a spec that [`Self::from_spec`] rejects.
    pub fn from_spec_file(file: SpecFile, schema: &ModelSchema<'_>) -> Result<(Self, Vec<Option<ParamValue>>), String> {
        let execution = file.execution;
        let spec = file.into_spec().map_err(|error| describe_error(&error))?;
        let (mut draft, panel_values) = Self::from_spec(&spec, execution.concurrent, schema)?;
        draft.memory_budget = execution.memory;
        draft.gpu_memory_budget = execution.gpu_memory;
        Ok((draft, panel_values))
    }

    /// Reads a draft from `spec` over `schema`'s model.
    ///
    /// Returns the draft, and the value of each parameter the spec holds at one value, in descriptor order. A
    /// parameter the spec varies in every configuration is `None`, and keeps its Parameters tab value.
    ///
    /// # Errors
    ///
    /// Returns the reason for a spec of another model, a value or action the model rejects, or blocks the tab cannot
    /// edit. The tab edits one block, or the blocks [`DraftDesign::VaryEachAlone`] writes.
    pub fn from_spec(
        spec: &SweepSpec,
        concurrency: Concurrency,
        schema: &ModelSchema<'_>,
    ) -> Result<(Self, Vec<Option<ParamValue>>), String> {
        if spec.model != schema.id {
            return Err(format!(
                "Spec is for {}, but the selected model is {}",
                spec.model, schema.id
            ));
        }
        let mut draft = Self::new(schema);
        let mut panel_values = vec![None; schema.params.len()];
        for (id, text) in &spec.fixed {
            let index = param_index(schema, id)?;
            panel_values[index] = Some(read_value(schema, index, text)?);
        }
        for action in &spec.actions {
            let action_index = schema
                .actions
                .iter()
                .position(|declared| declared.id() == action.id)
                .ok_or_else(|| format!("{} has no action {}", schema.id, action.id))?;
            draft.actions.push(ActionDraft {
                action_index,
                name: action.name.clone(),
                tick: action.tick,
                vary_tick: false,
                ticks_text: action.tick.to_string(),
            });
        }

        draft.replicates = spec.run.replicates;
        draft.root_seed_text = spec.seeds.root.to_string();
        draft.common_random_numbers = spec.seeds.scheme == SeedScheme::Common;
        draft.steps = spec.run.steps;
        draft.warmup = spec.run.warmup;
        draft.stats_every = spec.measure.stats_every;
        draft.series_every = spec.measure.series_every;
        draft.stop = spec.run.stop.as_ref().map(|stop| StopDraft {
            column: stop.column.clone(),
            comparator: stop.comparison.comparator,
            threshold: stop.comparison.threshold,
            min_tick: stop.min_tick,
        });
        draft.timeout_s = spec.run.timeout.map(|timeout| timeout.as_secs_f64());
        draft.default_reducers = spec.measure.default_reducers;
        draft.reducers.clone_from(&spec.measure.reducers);
        draft.concurrency = concurrency;

        match spec.blocks.as_slice() {
            [] => {}
            [block] => draft.read_block(block, schema)?,
            blocks => draft.read_vary_each_alone(blocks, schema, &mut panel_values)?,
        }
        if let Some(search) = &spec.search {
            draft.mode = ExploreMode::Search;
            draft.search.read_spec(search);
            for factor in &search.space {
                draft.mark_varied(&factor.target, &factor.levels, schema)?;
            }
        }
        Ok((draft, panel_values))
    }

    /// Returns the blocks the draft's design writes, noting an issue for each varied value that does not parse.
    ///
    /// Called for a draft in [`ExploreMode::Sweep`] alone. In Search mode, [`Self::factor_design`] reads as a sampled
    /// design.
    pub(super) fn blocks(
        &self,
        schema: &ModelSchema<'_>,
        panel_values: &[ParamValue],
        issues: &mut Vec<DraftIssue>,
    ) -> Vec<BlockSpec> {
        if self.design == DraftDesign::Table {
            return self.table_blocks(issues);
        }
        let design = self.factor_design();
        let (params, actions) = self.varied_factors(schema, issues);
        if self.design == DraftDesign::VaryEachAlone {
            return vary_each_alone_blocks(schema, panel_values, &params, actions);
        }
        if params.is_empty() && actions.is_empty() && !design.is_sampled() {
            return Vec::new();
        }
        let design_seed = match self.design_seed_text.trim() {
            "" => None,
            _ if !design.is_sampled() => None,
            text => match parse_seed_text(text) {
                Ok(seed) => Some(seed),
                Err(message) => {
                    issues.push(DraftIssue::new(DraftSite::DesignSeed, design_seed_message(&message)));
                    None
                }
            },
        };
        vec![BlockSpec {
            design,
            factors: params.into_iter().map(|(_, factor)| factor).chain(actions).collect(),
            design_seed,
        }]
    }

    /// Returns the block of the draft's design table, or notes an issue when the draft has no design table.
    pub(super) fn table_blocks(&self, issues: &mut Vec<DraftIssue>) -> Vec<BlockSpec> {
        let Some(table) = &self.table else {
            issues.push(DraftIssue::missing(DraftSite::Design, NO_TABLE));
            return Vec::new();
        };
        vec![BlockSpec {
            design: DesignKind::Table {
                text: table.text.clone(),
            },
            factors: Vec::new(),
            design_seed: None,
        }]
    }

    /// Returns a factor for each varied parameter with its index, and one for each varied action tick, noting an
    /// issue for each factor whose values do not parse.
    pub(super) fn varied_factors(
        &self,
        schema: &ModelSchema<'_>,
        issues: &mut Vec<DraftIssue>,
    ) -> (Vec<(usize, FactorSpec)>, Vec<FactorSpec>) {
        let mut params = Vec::new();
        let draws_ranges = self.draws_ranges();
        for (index, (factor, descriptor)) in self.factors.iter().zip(schema.params).enumerate() {
            if factor.vary {
                match row_levels(&factor.levels_text, draws_ranges, || "Enter values".to_owned()) {
                    Ok(levels) => params.push((index, FactorSpec::param(descriptor.id(), levels))),
                    Err(issue) => issues.push(DraftIssue::at(DraftSite::Factor(index), issue)),
                }
            }
        }
        let mut actions = Vec::new();
        for (position, action) in self.actions.iter().enumerate() {
            if action.vary_tick {
                match row_levels(&action.ticks_text, draws_ranges, || "Enter ticks".to_owned()) {
                    Ok(levels) => actions.push(FactorSpec::action(action.name.clone(), levels)),
                    Err(issue) => issues.push(DraftIssue::at(DraftSite::Action(position), issue)),
                }
            }
        }
        (params, actions)
    }

    /// Returns the Parameters tab value of each parameter no block sets, as `(id, text)` pairs.
    pub(super) fn fixed_values(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Vec<(String, String)> {
        let table_columns = self.table_columns();
        schema
            .params
            .iter()
            .zip(panel_values)
            .zip(&self.factors)
            .filter(|((descriptor, _), factor)| match &table_columns {
                Some(columns) => !columns.iter().any(|column| column == descriptor.id()),
                None => !factor.vary,
            })
            .map(|((descriptor, value), _)| (descriptor.id().to_owned(), format_value(descriptor.kind(), value)))
            .collect()
    }

    /// Reads the design and factors of a spec's only block.
    pub(super) fn read_block(&mut self, block: &BlockSpec, schema: &ModelSchema<'_>) -> Result<(), String> {
        self.design = match &block.design {
            DesignKind::Factorial => DraftDesign::EveryCombination,
            DesignKind::Zip => DraftDesign::Zip,
            DesignKind::Random { samples } => {
                self.samples = *samples;
                DraftDesign::UniformRandom
            }
            DesignKind::LatinHypercube { samples } => {
                self.samples = *samples;
                DraftDesign::LatinHypercube
            }
            DesignKind::Table { text } => {
                self.table = Some(DesignTableDraft {
                    file_name: SPEC_TABLE_NAME.to_owned(),
                    text: text.clone(),
                });
                DraftDesign::Table
            }
        };
        if let Some(seed) = block.design_seed {
            self.design_seed_text = seed.to_string();
        }
        for factor in &block.factors {
            self.mark_varied(&factor.target, &factor.levels, schema)?;
        }
        Ok(())
    }

    /// Reads blocks that [`DraftDesign::VaryEachAlone`] writes, and the Parameters tab value at which they hold each
    /// varied parameter outside its own block.
    pub(super) fn read_vary_each_alone(
        &mut self,
        blocks: &[BlockSpec],
        schema: &ModelSchema<'_>,
        panel_values: &mut [Option<ParamValue>],
    ) -> Result<(), String> {
        let refusal = || {
            format!(
                "Spec holds {} blocks. Load a spec with one block or a One at a time design.",
                blocks.len()
            )
        };
        let param_ids: Vec<&str> = blocks[0]
            .factors
            .iter()
            .map_while(|factor| match &factor.target {
                FactorTarget::Param(id) => Some(id.as_str()),
                FactorTarget::Action(_) => None,
            })
            .collect();
        if blocks.len() < param_ids.len() {
            return Err(refusal());
        }
        let mut baselines: Vec<Option<&str>> = vec![None; param_ids.len()];
        let mut action_names: Vec<&str> = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            if block.design != DesignKind::Factorial || block.design_seed.is_some() {
                return Err(refusal());
            }
            let own_action = usize::from(index >= param_ids.len());
            if block.factors.len() != param_ids.len() + own_action {
                return Err(refusal());
            }
            let (param_factors, action_factors) = block.factors.split_at(param_ids.len());
            for (position, (factor, id)) in param_factors.iter().zip(&param_ids).enumerate() {
                if !matches!(&factor.target, FactorTarget::Param(target) if target == id) {
                    return Err(refusal());
                }
                if position == index {
                    continue;
                }
                let LevelSpec::Values(values) = &factor.levels else {
                    return Err(refusal());
                };
                let [value] = values.as_slice() else {
                    return Err(refusal());
                };
                match baselines[position] {
                    Some(baseline) if baseline != value => return Err(refusal()),
                    _ => baselines[position] = Some(value),
                }
            }
            for factor in action_factors {
                match &factor.target {
                    FactorTarget::Action(name) if !action_names.contains(&name.as_str()) => {
                        action_names.push(name);
                    }
                    _ => return Err(refusal()),
                }
            }
        }

        self.design = DraftDesign::VaryEachAlone;
        for (index, block) in blocks.iter().enumerate() {
            // A parameter's own block varies it at its position, and an action's block ends with the action.
            let own_factor = &block.factors[index.min(param_ids.len())];
            self.mark_varied(&own_factor.target, &own_factor.levels, schema)?;
        }
        for (id, baseline) in param_ids.iter().zip(baselines) {
            if let Some(text) = baseline {
                let index = param_index(schema, id)?;
                panel_values[index] = Some(read_value(schema, index, text)?);
            }
        }
        Ok(())
    }

    /// Marks the parameter or action `target` varied over `levels`.
    pub(super) fn mark_varied(
        &mut self,
        target: &FactorTarget,
        levels: &LevelSpec,
        schema: &ModelSchema<'_>,
    ) -> Result<(), String> {
        match target {
            FactorTarget::Param(id) => {
                let index = param_index(schema, id)?;
                self.factors[index] = FactorDraft {
                    vary: true,
                    levels_text: levels_text(levels),
                };
            }
            FactorTarget::Action(name) => {
                let action = self
                    .actions
                    .iter_mut()
                    .find(|action| action.name == *name)
                    .ok_or_else(|| format!("Spec varies the tick of action {name} but never runs it"))?;
                action.vary_tick = true;
                action.ticks_text = levels_text(levels);
            }
        }
        Ok(())
    }
}

/// Returns the blocks of [`DraftDesign::VaryEachAlone`]: one per varied parameter, then one per varied action tick.
///
/// Every block lists every varied parameter. Outside its own block a parameter takes its value in `panel_values`, and
/// an action its fixed tick.
pub(super) fn vary_each_alone_blocks(
    schema: &ModelSchema<'_>,
    panel_values: &[ParamValue],
    params: &[(usize, FactorSpec)],
    actions: Vec<FactorSpec>,
) -> Vec<BlockSpec> {
    let baseline = |index: usize| {
        let descriptor = &schema.params[index];
        let value = panel_values
            .get(index)
            .cloned()
            .unwrap_or_else(|| descriptor.kind().default_value());
        FactorSpec::param(
            descriptor.id(),
            LevelSpec::Values(vec![format_value(descriptor.kind(), &value)]),
        )
    };
    let baselines = |own_index: Option<usize>| -> Vec<FactorSpec> {
        params
            .iter()
            .map(|(index, factor)| {
                if Some(*index) == own_index {
                    factor.clone()
                } else {
                    baseline(*index)
                }
            })
            .collect()
    };
    let factorial_block = |factors| BlockSpec {
        design: DesignKind::Factorial,
        factors,
        design_seed: None,
    };
    params
        .iter()
        .map(|(index, _)| factorial_block(baselines(Some(*index))))
        .chain(actions.into_iter().map(|action| {
            let mut factors = baselines(None);
            factors.push(action);
            factorial_block(factors)
        }))
        .collect()
}

/// Reads a seed typed as a whole number from 0 to `u64::MAX`.
pub(super) fn parse_seed_text(text: &str) -> Result<u64, String> {
    let text = text.trim();
    text.parse()
        .ok()
        .ok_or_else(|| format!("'{text}' is not an integer from 0 to {}", u64::MAX))
}

pub(super) fn param_index(schema: &ModelSchema<'_>, id: &str) -> Result<usize, String> {
    schema
        .params
        .iter()
        .position(|descriptor| descriptor.id() == id)
        .ok_or_else(|| format!("{} has no parameter {id}", schema.id))
}

pub(super) fn read_value(schema: &ModelSchema<'_>, index: usize, text: &str) -> Result<ParamValue, String> {
    parse_value(schema.params[index].kind(), text).map_err(|error| describe_error(&error))
}
