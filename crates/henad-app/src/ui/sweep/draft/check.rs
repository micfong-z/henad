//! Check of a sweep draft: the issues its pre-pass finds, and the plan errors it places on the rows they concern.

use std::time::Duration;

use henad_core::explore::design::DesignError;
use henad_core::explore::factor::{FactorError, FactorTarget};
use henad_core::explore::measure::{MeasureError, MeasurePlan};
use henad_core::explore::plan::{ModelSchema, PlanError};
use henad_core::explore::reducer::{ReducerError, ReducerKind};
use henad_core::explore::search::SearchSpecError;
use henad_core::explore::search::genetic::GeneticSettings;
use henad_core::explore::search::genome::SearchSpaceError;
use henad_core::explore::search::hill_climb::HillClimbSettings;
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::spec::SweepSpec;
use henad_core::helpers::fmt_bytes;
use henad_core::params::ParamValue;
use henad_explore::search_run::{SearchPlan, SearchPlanError};

use crate::options::cli_phrase;
use crate::ui::plural;
use crate::ui::text::describe_error;

use super::levels::{estimated_configs, estimated_series_bytes};
use super::messages::{
    design_seed_message, factor_message, missing_output_message, root_seed_message, setting_message,
};
use super::spec::parse_seed_text;
use super::{
    AXIS_RANGE_MISSING, COLUMNS_PENDING, CROSSOVER_RATE_KEY, DraftAlgorithm, DraftDesign, DraftIssue, DraftPlan,
    DraftSite, ELITE_COUNT_KEY, ExploreMode, GENETIC_MUTATION_SCALE_KEY, GridAxis, HILL_CLIMB_MUTATION_SCALE_KEY,
    INITIAL_SAMPLES_KEY, IssueKind, MAX_DRAFT_RUNS, MAX_MEMORY_SERIES_BYTES, MAX_SETTING_CHECKS, MUTATION_RATE_KEY,
    NO_REPLICATES, NO_SAMPLE_EVERY, NO_SUCH_ACTION, NO_TABLE, NOT_FINITE_THRESHOLD, NOTHING_SEARCHED, PATIENCE_KEY,
    POPULATION_KEY, PSE_MUTATION_SCALE_KEY, REEVALUATE_FRACTION_KEY, SELECT_OUTPUT, SweepDraft, TOURNAMENT_SIZE_KEY,
};

impl SweepDraft {
    /// Returns every issue the draft's rows show before a plan is made, in the order the Sweep tab draws the rows.
    ///
    /// Each parameter row, action tick, seed and search field is checked, and every issue found is listed. The plan
    /// finds the rest one at a time. The issue of stat columns still being sampled comes last.
    pub fn issues(&self, schema: &ModelSchema<'_>) -> Vec<DraftIssue> {
        let mut issues = Vec::new();
        let mut row_issues = Vec::new();
        let actions = self.declared_actions(schema);
        let runs_table = self.runs_table();
        // Each varied row that lists its values adds its label and number of values, for a zip to compare.
        let mut listed: Vec<(String, usize)> = Vec::new();
        let mut varied_rows = 0;
        if !runs_table {
            for (index, descriptor) in schema.params.iter().enumerate() {
                if !self.factors.get(index).is_some_and(|factor| factor.vary) {
                    continue;
                }
                varied_rows += 1;
                match self.resolve_param(schema, &actions, index) {
                    Ok(factor) => {
                        if let Some(levels) = factor.levels() {
                            listed.push((descriptor.label().to_owned(), levels.len()));
                        }
                    }
                    Err(issue) => row_issues.push(DraftIssue::at(DraftSite::Factor(index), issue)),
                }
            }
        }
        if self.mode == ExploreMode::Search && varied_rows == 0 && !self.actions.iter().any(|action| action.vary_tick) {
            row_issues.push(DraftIssue::missing(DraftSite::Parameters, NOTHING_SEARCHED));
        }
        for (position, action) in self.actions.iter().enumerate() {
            let Some(declared) = schema.actions.get(action.action_index) else {
                row_issues.push(DraftIssue::new(DraftSite::Action(position), NO_SUCH_ACTION));
                continue;
            };
            if !action.vary_tick || runs_table {
                continue;
            }
            match self.resolve_ticks(schema, &actions, position) {
                Ok(factor) => {
                    if let Some(levels) = factor.levels() {
                        listed.push((format!("{} tick", declared.label()), levels.len()));
                    }
                }
                Err(issue) => row_issues.push(DraftIssue::at(DraftSite::Action(position), issue)),
            }
        }

        match self.mode {
            ExploreMode::Sweep if runs_table && self.table.is_none() => {
                issues.push(DraftIssue::missing(DraftSite::Design, NO_TABLE));
            }
            ExploreMode::Sweep if self.design == DraftDesign::Zip => issues.extend(zip_issue(&listed)),
            ExploreMode::Sweep => {}
            ExploreMode::Search => issues.extend(self.search_issues(schema)),
        }
        issues.append(&mut row_issues);

        if self.replicates == 0 {
            issues.push(DraftIssue::new(DraftSite::Replicates, NO_REPLICATES));
        }
        if let Err(message) = parse_seed_text(&self.root_seed_text) {
            issues.push(DraftIssue::new(DraftSite::Seed, root_seed_message(&message)));
        }
        let design_seed = self.design_seed_text.trim();
        if self.mode == ExploreMode::Sweep
            && self.design.is_sampled()
            && !design_seed.is_empty()
            && let Err(message) = parse_seed_text(design_seed)
        {
            issues.push(DraftIssue::new(DraftSite::DesignSeed, design_seed_message(&message)));
        }
        issues.extend(self.stop_issue());
        issues.extend(self.timeout().err());
        if self.stats_every == 0 {
            issues.push(DraftIssue::new(DraftSite::Sampling, NO_SAMPLE_EVERY));
        } else if !self.series_every.is_multiple_of(self.stats_every) {
            issues.push(DraftIssue::new(DraftSite::Sampling, self.series_cadence_message()));
        }
        issues.extend(self.output_issues());
        if self.results_in_folder && !cfg!(target_arch = "wasm32") && self.output_dir_text.trim().is_empty() {
            issues.push(DraftIssue::missing(DraftSite::Execution, "Select a folder"));
        }
        if self.columns_pending {
            issues.push(DraftIssue::missing(DraftSite::Outputs, COLUMNS_PENDING));
        }
        issues
    }

    /// Returns the issue of a stop condition whose threshold is not a finite number.
    ///
    /// The plan accepts any threshold. A spec file rejects a threshold that is not finite.
    pub(super) fn stop_issue(&self) -> Option<DraftIssue> {
        self.stop
            .as_ref()
            .filter(|stop| !stop.threshold.is_finite())
            .map(|_| DraftIssue::new(DraftSite::Stop, NOT_FINITE_THRESHOLD))
    }

    /// Returns the time limit of each run, `None` for no limit.
    ///
    /// A limit under [`MIN_TIMEOUT_SECONDS`](super::MIN_TIMEOUT_SECONDS) is accepted, 0 included, as in `henad-cli`.
    ///
    /// # Errors
    ///
    /// Returns the issue of a limit that is negative, not a number, or too large for a [`Duration`].
    pub(super) fn timeout(&self) -> Result<Option<Duration>, DraftIssue> {
        let Some(seconds) = self.timeout_s else {
            return Ok(None);
        };
        if let Ok(timeout) = Duration::try_from_secs_f64(seconds) {
            return Ok(Some(timeout));
        }
        let message = if seconds.is_nan() {
            "Seconds per run must be a number"
        } else if seconds < 0.0 {
            "Seconds per run must be at least 0"
        } else {
            "Seconds per run is too large"
        };
        Err(DraftIssue::new(DraftSite::Timeout, message))
    }

    /// Returns an issue for each output row whose window ends before it starts, or whose crossing threshold is not a
    /// finite number.
    ///
    /// The plan accepts a window in either order and any threshold. A spec file rejects a reversed window and a
    /// threshold that is not finite.
    pub(super) fn output_issues(&self) -> impl Iterator<Item = DraftIssue> + '_ {
        self.reducers.iter().enumerate().filter_map(|(position, reducer)| {
            let message = match reducer.kind {
                ReducerKind::WindowMean { start, end } if start > end => "From tick must be at most To tick",
                ReducerKind::FirstCrossing(comparison) if !comparison.threshold.is_finite() => NOT_FINITE_THRESHOLD,
                _ => return None,
            };
            Some(DraftIssue::new(DraftSite::Output(position), message))
        })
    }

    /// Returns the issue of a series every [`Self::series_every`] ticks, off the cadence of the samples.
    pub(super) fn series_cadence_message(&self) -> String {
        format!(
            "Series every must be 0 or a multiple of Sample every ({} {})",
            self.stats_every,
            plural(self.stats_every, "tick")
        )
    }

    /// Returns the issues of a search's objective or axes, and of its method's settings.
    pub(super) fn search_issues(&self, schema: &ModelSchema<'_>) -> Vec<DraftIssue> {
        let search = &self.search;
        let mut issues = Vec::new();
        for (site, column) in search.watched_columns() {
            if column.is_empty() {
                issues.push(DraftIssue::missing(site, SELECT_OUTPUT));
            } else if !self.records_output(column, schema) {
                issues.push(DraftIssue::new(site, missing_output_message(column)));
            }
            if let DraftSite::Axis(axis) = site {
                let settings = &search.pattern_space;
                let pattern_axis = match axis {
                    GridAxis::X => &settings.x_axis,
                    GridAxis::Y => &settings.y_axis,
                };
                issues.extend(axis_range_issue(pattern_axis).map(|issue| DraftIssue::at(site, issue)));
            }
        }
        issues.extend(self.setting_issues());
        issues
    }

    /// Returns an issue for each setting of the search's method that its check rejects.
    ///
    /// The check stops at the first setting it rejects. That setting is set to a value it accepts, and the check runs
    /// again, until it passes. A grid axis is checked with the rows of the axes.
    pub(super) fn setting_issues(&self) -> Vec<DraftIssue> {
        let search = &self.search;
        let refusals = match search.algorithm {
            DraftAlgorithm::Random => Vec::new(),
            DraftAlgorithm::HillClimb => every_refusal(search.hill_climb, HillClimbSettings::check, |settings, key| {
                let fallback = HillClimbSettings::default();
                match key {
                    PATIENCE_KEY => settings.patience = fallback.patience,
                    HILL_CLIMB_MUTATION_SCALE_KEY => settings.mutation_scale = fallback.mutation_scale,
                    _ => {}
                }
            }),
            DraftAlgorithm::Genetic => every_refusal(search.genetic, GeneticSettings::check, |settings, key| {
                let fallback = GeneticSettings::default();
                match key {
                    POPULATION_KEY => settings.population = fallback.population,
                    // An elite count of 0 is never rejected, whatever the population.
                    ELITE_COUNT_KEY => settings.elite_count = 0,
                    TOURNAMENT_SIZE_KEY => settings.tournament_size = fallback.tournament_size,
                    CROSSOVER_RATE_KEY => settings.crossover_rate = fallback.crossover_rate,
                    MUTATION_RATE_KEY => settings.mutation_rate = fallback.mutation_rate,
                    REEVALUATE_FRACTION_KEY => settings.reevaluate_fraction = fallback.reevaluate_fraction,
                    GENETIC_MUTATION_SCALE_KEY => settings.mutation_scale = fallback.mutation_scale,
                    _ => {}
                }
            }),
            DraftAlgorithm::PatternSpace => {
                let mut settings = search.pattern_space.clone();
                // The rows of the axes report their ranges, so the check here sees only a range it accepts. An
                // automatic range is left as it is. The check needs it to reject zero initial samples.
                for axis in [&mut settings.x_axis, &mut settings.y_axis] {
                    if !axis.is_automatic() {
                        axis.min = Some(0.0);
                        axis.max = Some(1.0);
                    }
                    axis.cells = axis.cells.max(1);
                }
                every_refusal(settings, PatternSpaceSettings::check, |settings, key| {
                    let fallback = PatternSpaceSettings::new(settings.x_axis.clone(), settings.y_axis.clone());
                    match key {
                        INITIAL_SAMPLES_KEY => settings.initial_samples = fallback.initial_samples,
                        PSE_MUTATION_SCALE_KEY => settings.mutation_scale = fallback.mutation_scale,
                        _ => {}
                    }
                })
            }
        };
        refusals
            .into_iter()
            .map(|(key, message)| DraftIssue::new(DraftSite::MethodSetting(key), message))
            .collect()
    }

    /// Returns the site of the row the search setting `key` belongs to.
    pub(super) fn setting_site(key: &'static str) -> DraftSite {
        if key.starts_with("pse.x_axis.") {
            DraftSite::Axis(GridAxis::X)
        } else if key.starts_with("pse.y_axis.") {
            DraftSite::Axis(GridAxis::Y)
        } else {
            DraftSite::MethodSetting(key)
        }
    }

    /// Returns the spec the draft writes and its plan.
    ///
    /// # Errors
    ///
    /// Returns every issue of [`Self::issues`] when there are any. Otherwise returns a single issue: a sweep or search
    /// of more than [`MAX_DRAFT_RUNS`] runs, a sweep or search held in memory whose series exceed
    /// [`MAX_MEMORY_SERIES_BYTES`], or the reason the model rejects the spec.
    pub fn check(&self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Result<DraftPlan, Vec<DraftIssue>> {
        let issues = self.issues(schema);
        if !issues.is_empty() {
            return Err(issues);
        }
        let spec = self.to_spec(schema, panel_values)?;
        let title = self.mode.title();
        let too_many = |runs: u64| {
            vec![DraftIssue::new(
                DraftSite::Sweep,
                format!(
                    "{title} of {runs} runs is over the app's limit of {MAX_DRAFT_RUNS}. Run it {}.",
                    cli_phrase(self.cli_command.as_deref())
                ),
            )]
        };
        let estimate = match self.mode {
            ExploreMode::Sweep => estimated_configs(&spec),
            ExploreMode::Search => self.search.max_evaluations,
        };
        let estimate = estimate.saturating_mul(spec.run.replicates);
        if estimate > MAX_DRAFT_RUNS {
            return Err(too_many(estimate));
        }
        let (plan, search_plan) = match self.mode {
            ExploreMode::Sweep => match spec.plan(schema) {
                Ok(plan) => (plan, None),
                Err(error) => return Err(vec![self.plan_issue(&error, &spec, schema)]),
            },
            ExploreMode::Search => match SearchPlan::new(&spec, schema) {
                Ok(search_plan) => ((**search_plan.base()).clone(), Some(search_plan)),
                Err(error) => return Err(vec![self.search_issue(&error, &spec, schema)]),
            },
        };
        // The plan checks each column against the stat labels alone. A sweep binds its reducers, its stop condition
        // and a search's outputs to the columns a build samples, and the check does the same once it has them.
        if let Some(columns) = &self.stat_columns {
            let measure = MeasurePlan::new(&spec.run, &spec.measure, columns.clone())
                .map_err(|error| vec![self.measure_issue(&error)])?;
            if let Some(search_plan) = &search_plan {
                search_plan
                    .watched_reducers(&measure)
                    .map_err(|error| vec![self.search_issue(&error, &spec, schema)])?;
            }
        }
        let run_count = search_plan.as_ref().map_or(plan.run_count(), SearchPlan::run_count);
        if run_count > MAX_DRAFT_RUNS {
            return Err(too_many(run_count));
        }
        let series_bytes = estimated_series_bytes(&plan, run_count, self.stat_layout(schema).len());
        if self.holds_results_in_memory() && series_bytes > MAX_MEMORY_SERIES_BYTES {
            return Err(vec![DraftIssue::new(
                DraftSite::Execution,
                memory_refusal(series_bytes),
            )]);
        }
        Ok(DraftPlan {
            spec,
            plan,
            search_plan,
            series_bytes,
        })
    }

    /// Returns the row of the parameter or action tick `target`.
    pub(super) fn target_site(&self, target: &FactorTarget, schema: &ModelSchema<'_>) -> DraftSite {
        match target {
            FactorTarget::Param(id) => schema
                .params
                .iter()
                .position(|descriptor| descriptor.id() == *id)
                .map_or(DraftSite::Sweep, DraftSite::Factor),
            FactorTarget::Action(name) => self.action_site(name),
        }
    }

    /// Returns the issue of a factor the plan or the search space rejects, placed on its row.
    pub(super) fn factor_issue(&self, error: &FactorError, schema: &ModelSchema<'_>) -> DraftIssue {
        let site = match error {
            FactorError::Level { id, .. } | FactorError::MissingStep { id } | FactorError::RangeOverOptions { id } => {
                self.target_site(&FactorTarget::Param(id.clone()), schema)
            }
            FactorError::BadTick { name, .. } => self.action_site(name),
            FactorError::Range { target, .. }
            | FactorError::NotWhole { target, .. }
            | FactorError::AllOverNumber { target }
            | FactorError::NoLevels { target } => self.target_site(target, schema),
            FactorError::UnknownParam { .. } | FactorError::UnknownAction { .. } => DraftSite::Sweep,
        };
        DraftIssue::new(site, factor_message(error))
    }

    /// Returns the issue of a search the model or its settings reject, placed on its row or section.
    pub(super) fn search_issue(
        &self,
        error: &SearchPlanError,
        spec: &SweepSpec,
        schema: &ModelSchema<'_>,
    ) -> DraftIssue {
        match error {
            SearchPlanError::Plan(source) => self.plan_issue(source, spec, schema),
            SearchPlanError::Settings(source @ SearchSpecError::Setting { key, .. }) => {
                DraftIssue::new(Self::setting_site(key), setting_message(source))
            }
            SearchPlanError::Settings(SearchSpecError::MissingObjective { .. }) => {
                DraftIssue::missing(DraftSite::Objective, SELECT_OUTPUT)
            }
            SearchPlanError::Settings(source) => DraftIssue::new(DraftSite::Search, setting_message(source)),
            SearchPlanError::Space(SearchSpaceError::NoFactors) => {
                DraftIssue::missing(DraftSite::Parameters, NOTHING_SEARCHED)
            }
            SearchPlanError::Space(SearchSpaceError::NoLevels { factor_index }) => {
                let site = spec
                    .search
                    .as_ref()
                    .and_then(|search| search.space.get(*factor_index))
                    .map_or(DraftSite::Search, |factor| self.target_site(&factor.target, schema));
                DraftIssue::new(site, "Enter values")
            }
            SearchPlanError::Space(SearchSpaceError::Factor(source)) => self.factor_issue(source, schema),
            SearchPlanError::UnknownColumn { column, .. } => {
                let site = self
                    .search
                    .watched_columns()
                    .into_iter()
                    .find(|(_, watched)| watched == column)
                    .map_or(DraftSite::Search, |(site, _)| site);
                DraftIssue::new(site, missing_output_message(column))
            }
            _ => DraftIssue::new(DraftSite::Search, describe_error(error)),
        }
    }

    /// Returns the issue of a plan error, placed on the row of the factor, action or output that it concerns.
    pub(super) fn plan_issue(&self, error: &PlanError, spec: &SweepSpec, schema: &ModelSchema<'_>) -> DraftIssue {
        match error {
            PlanError::Factor { source, .. } => self.factor_issue(source, schema),
            PlanError::Design { block, source } => {
                let site = match source {
                    DesignError::UnlistedLevels { factor_index } => spec
                        .blocks
                        .get(*block)
                        .and_then(|block| block.factors.get(*factor_index))
                        .map_or(DraftSite::Design, |factor| self.target_site(&factor.target, schema)),
                    _ => DraftSite::Design,
                };
                DraftIssue::new(site, describe_error(source))
            }
            PlanError::Table { source, .. } => DraftIssue::new(DraftSite::Design, describe_error(source)),
            PlanError::Measure(source) => self.measure_issue(source),
            PlanError::Stop(source) => DraftIssue::new(DraftSite::Stop, describe_error(source)),
            PlanError::NoReplicates => DraftIssue::new(DraftSite::Replicates, NO_REPLICATES),
            PlanError::DuplicateActionName { name } => DraftIssue::new(self.action_site(name), describe_error(error)),
            _ => DraftIssue::new(DraftSite::Sweep, describe_error(error)),
        }
    }

    /// Returns the issue of measurement settings the plan rejects, in the words of the tab's labels.
    pub(super) fn measure_issue(&self, error: &MeasureError) -> DraftIssue {
        match error {
            MeasureError::ZeroStatsEvery => DraftIssue::new(DraftSite::Sampling, NO_SAMPLE_EVERY),
            MeasureError::SeriesOffCadence { .. } => {
                DraftIssue::new(DraftSite::Sampling, self.series_cadence_message())
            }
            MeasureError::Reducer(source) => {
                let position = match source {
                    ReducerError::UnknownColumn { column, .. } | ReducerError::MissingKind { raw: column } => {
                        self.reducers.iter().position(|reducer| reducer.column == *column)
                    }
                    ReducerError::UnknownKind { raw }
                    | ReducerError::Comparison { raw, .. }
                    | ReducerError::BadWindow { raw } => self
                        .reducers
                        .iter()
                        .position(|reducer| reducer.kind.to_string() == *raw),
                };
                DraftIssue::new(
                    position.map_or(DraftSite::Sweep, DraftSite::Output),
                    describe_error(source),
                )
            }
            MeasureError::Stop(source) => DraftIssue::new(DraftSite::Stop, describe_error(source)),
            MeasureError::TooManyTicks { .. } => DraftIssue::new(DraftSite::RunLength, describe_error(error)),
            MeasureError::TimeoutTooLong { .. } => DraftIssue::new(DraftSite::Timeout, describe_error(error)),
        }
    }

    pub(super) fn action_site(&self, name: &str) -> DraftSite {
        self.actions
            .iter()
            .position(|action| action.name == name)
            .map_or(DraftSite::Sweep, DraftSite::Action)
    }
}

/// Returns the issue of a zip over rows `listed`, each a label and a number of values, when the numbers differ.
pub(super) fn zip_issue(listed: &[(String, usize)]) -> Option<DraftIssue> {
    let first = listed.first()?.1;
    if listed.iter().all(|(_, count)| *count == first) {
        return None;
    }
    let lists: Vec<String> = listed
        .iter()
        .map(|(label, count)| format!("{label} has {count}"))
        .collect();
    Some(DraftIssue::new(
        DraftSite::Design,
        format!("Use lists of equal length for Zip. {}.", lists.join(", ")),
    ))
}

/// Returns the kind and message of the issue of a grid axis's range, `None` for an automatic range or a range the
/// search accepts.
///
/// A range from 0 to 0, the range an axis's fields start with, is missing input.
pub(super) fn axis_range_issue(axis: &PatternAxis) -> Option<(IssueKind, String)> {
    let (min, max) = axis.range()?;
    if min == 0.0 && max == 0.0 {
        Some((IssueKind::Missing, AXIS_RANGE_MISSING.to_owned()))
    } else if !min.is_finite() {
        Some((IssueKind::Invalid, "Minimum must be a finite number".to_owned()))
    } else if !(max.is_finite() && max > min) {
        Some((IssueKind::Invalid, "Maximum must be greater than Minimum".to_owned()))
    } else {
        None
    }
}

/// Returns the key and message of every setting of `settings` that `check` rejects.
///
/// `repair` sets the rejected setting named by its key to a value `check` accepts, so the next check reaches the next
/// setting.
pub(super) fn every_refusal<S>(
    mut settings: S,
    check: impl Fn(&S) -> Result<(), SearchSpecError>,
    repair: impl Fn(&mut S, &str),
) -> Vec<(&'static str, String)> {
    let mut refusals = Vec::new();
    for _ in 0..MAX_SETTING_CHECKS {
        match check(&settings) {
            Err(error @ SearchSpecError::Setting { key, .. }) => {
                if refusals.iter().any(|(refused, _)| *refused == key) {
                    break;
                }
                refusals.push((key, setting_message(&error)));
                repair(&mut settings, key);
            }
            Err(_) | Ok(()) => break,
        }
    }
    refusals
}

/// Returns the issue of a sweep held in memory whose series take about `bytes`.
pub(super) fn memory_refusal(bytes: u64) -> String {
    let (size, limit) = (fmt_bytes(bytes), fmt_bytes(MAX_MEMORY_SERIES_BYTES));
    if cfg!(target_arch = "wasm32") {
        format!(
            "Series take about {size}, over the browser's limit of {limit}. Increase Series every, or run the sweep \
             in the desktop app."
        )
    } else {
        format!(
            "Series take about {size} in memory, over the limit of {limit}. Select In a folder under Results, or \
             increase Series every."
        )
    }
}
