//! Plan of a sweep or search, as the Plan panel lists it: its totals, its varied and held values, and its problems.
//!
//! The panel beside the form, the Plan section that replaces it on a narrow tab, and the plan of a running session all
//! draw a [`PlanSummary`] through [`plan_ui`].

use std::path::Path;

use egui::{Label, RichText, Sense};
use henad_core::explore::factor::LevelSpec;
use henad_core::explore::plan::{ModelSchema, Plan};
use henad_core::explore::search::{Aggregate, Goal};
use henad_core::helpers::fmt_bytes;
use henad_core::params::ParamValue;
use henad_explore::exec::Concurrency;

use crate::icons::material_design_icons::MDI_ALERT;
use crate::ui::kv_grid;
use crate::ui::params::display_value;
use crate::ui::plural;
use crate::ui::results::plot::format_significant;
use crate::ui::results::store::output_label;
use crate::ui::sweep::builder::design_label;
use crate::ui::sweep::draft::{
    DraftAlgorithm, DraftDesign, DraftMode, LevelCount, LevelPreview, LevelPreviews, MAX_DRAFT_RUNS,
    MAX_MEMORY_SERIES_BYTES, StopDraft, SweepDraft, TickSource, comparator_words, parse_levels,
};
use crate::ui::sweep::layout::{Reveal, SweepSection, issue_color, issue_icon};
use crate::ui::sweep::search::algorithm_label;
use crate::ui::sweep::session::SweepSession;
use crate::ui::sweep::{CheckSummary, DraftCheck, IssueLine, SweepRequest};

/// Value of a row that needs a plan, while the draft has none.
pub const NOT_COUNTED: &str = "Unknown";

/// Most varied ticks of an action the Actions row lists one by one.
const MAX_LISTED_TICKS: usize = 4;

/// One row of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub label: &'static str,
    pub value: String,
    /// Text shown on hover over the value.
    pub tooltip: Option<String>,
    /// Whether the value nears a limit of the app.
    pub warn: bool,
    /// Section the label opens, `None` for a label that opens none.
    pub section: Option<SweepSection>,
}

impl PlanRow {
    fn new(label: &'static str, value: impl Into<String>, section: Option<SweepSection>) -> Self {
        Self {
            label,
            value: value.into(),
            tooltip: None,
            warn: false,
            section,
        }
    }
}

/// Plan of a sweep or search, with every text the Plan panel draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanSummary {
    pub rows: Vec<PlanRow>,
    /// Each varied parameter or action tick, with its values.
    pub varied: Vec<(String, String)>,
    /// Each parameter held at one value, with that value, as in "Grid Width 1024".
    pub held: Vec<String>,
    pub problems: Vec<IssueLine>,
    pub warnings: Vec<String>,
    /// Runs of the plan, `None` until counted.
    pub runs: Option<u64>,
}

impl PlanSummary {
    /// Returns the plan of the draft `check` holds, a draft of `schema`'s model named `model_name`, with the
    /// problems and warnings of `summary`.
    pub(super) fn for_draft(
        check: &DraftCheck,
        schema: &ModelSchema<'_>,
        model_name: &str,
        summary: &CheckSummary,
    ) -> Self {
        let draft = &check.draft;
        let planned = check.result.as_ref().ok();
        let (param_counts, tick_counts) = level_counts(&check.level_previews);
        let mut rows = vec![PlanRow::new("Model", model_name, None)];
        rows.extend(design_rows(check));
        rows.push(PlanRow::new(
            "Replicates",
            format!(
                "{}, {}",
                draft.replicates,
                if draft.common_random_numbers {
                    "common random numbers"
                } else {
                    "independent seeds"
                }
            ),
            Some(SweepSection::Seeds),
        ));
        let runs = planned.map(|planned| planned.counts().2);
        rows.push(runs_row(runs));
        rows.extend(run_rows(draft, schema, &check.level_previews.1));
        let in_memory = draft.holds_results_in_memory();
        rows.push(series_row(
            draft.series_every,
            planned.map(|planned| planned.series_bytes),
            in_memory,
        ));
        let mut results = results_row(draft.output_folder().map(Path::new));
        if !in_memory && draft.output_folder().is_none() {
            results.value = "No folder selected".to_owned();
        }
        rows.push(results);
        rows.push(PlanRow::new(
            "Concurrent runs",
            concurrency_text(draft.concurrency),
            Some(SweepSection::Execution),
        ));
        rows.extend(budget_rows(draft));

        Self {
            rows,
            varied: varied_lines(draft, schema, &param_counts, &tick_counts),
            held: held_values(draft, schema, &check.panel_values),
            problems: summary.lines.clone(),
            warnings: summary.warnings.clone(),
            runs,
        }
    }

    /// Returns the plan of a session that resumes a folder, read from the plan it runs.
    pub fn for_resumed(session: &SweepSession) -> Self {
        let plan: &Plan = session.plan();
        let mut rows = vec![PlanRow::new("Model", session.model_name.as_str(), None)];
        let runs = if let Some(search_plan) = session.search_plan() {
            rows.push(PlanRow::new(
                "Evaluations",
                search_plan.search().max_evaluations.to_string(),
                None,
            ));
            search_plan.run_count()
        } else {
            rows.push(PlanRow::new("Configurations", plan.configs().len().to_string(), None));
            plan.run_count()
        };
        rows.push(PlanRow::new("Replicates", plan.replicates().to_string(), None));
        let mut runs_row = runs_row(Some(runs));
        runs_row.section = None;
        rows.push(runs_row);
        let run = plan.run_settings();
        rows.push(PlanRow::new("Steps per run", steps_text(run.steps, run.warmup), None));
        rows.push(PlanRow::new(
            "Samples per run",
            samples_text(run.steps, plan.measure_settings().stats_every),
            None,
        ));
        let mut results = results_row(session.output_dir.as_deref());
        results.section = None;
        rows.push(results);
        let execution = &session.execution;
        rows.push(PlanRow::new(
            "Concurrent runs",
            concurrency_text(execution.concurrency),
            None,
        ));
        let budgets = [
            ("Memory budget", execution.memory_budget),
            ("GPU memory budget", execution.gpu_memory_budget),
        ];
        rows.extend(budgets.into_iter().filter_map(|(label, bytes)| {
            let mut row = PlanRow::new(label, fmt_bytes(bytes?), None);
            row.tooltip = Some(format!("Recorded by the {}", session.noun()));
            Some(row)
        }));
        Self {
            rows,
            runs: Some(runs),
            ..Self::default()
        }
    }

    /// Returns the summary line of the Plan section: the runs, or that they are not counted yet.
    pub fn section_summary(&self) -> String {
        match self.runs {
            Some(runs) => format!("{runs} {}", plural(runs, "run")),
            None => NOT_COUNTED.to_owned(),
        }
    }
}

/// Returns the rows of a search's method, budget and objective, or of a sweep's design and configurations.
fn design_rows(check: &DraftCheck) -> Vec<PlanRow> {
    let draft = &check.draft;
    if draft.mode == DraftMode::Search {
        return vec![
            PlanRow::new(
                "Method",
                algorithm_label(draft.search.algorithm),
                Some(SweepSection::Search),
            ),
            PlanRow::new(
                "Evaluations",
                format!(
                    "{} in batches of {}",
                    draft.search.max_evaluations, draft.search.batch_size
                ),
                Some(SweepSection::Search),
            ),
            objective_row(draft),
        ];
    }
    let (param_counts, tick_counts) = level_counts(&check.level_previews);
    let configurations = check.result.as_ref().ok().map_or_else(
        || NOT_COUNTED.to_owned(),
        |planned| configurations_text(draft, planned.plan.configs().len() as u64, &param_counts, &tick_counts),
    );
    vec![
        PlanRow::new("Design", design_label(draft.design), Some(SweepSection::Design)),
        PlanRow::new("Configurations", configurations, Some(SweepSection::Design)),
    ]
}

/// Returns the number of values of each parameter row, then of each action row, from their `previews`.
fn level_counts((params, ticks): &LevelPreviews) -> (Vec<Option<LevelCount>>, Vec<Option<LevelCount>>) {
    let counts = |previews: &[Option<LevelPreview>]| {
        previews
            .iter()
            .map(|preview| preview.as_ref().map(LevelPreview::count))
            .collect()
    };
    (counts(params), counts(ticks))
}

/// Returns the rows of a run's length, its stop condition, its timeout, its actions and its outputs.
///
/// `tick_previews` holds the values of each action row's varied ticks, as [`SweepDraft::level_previews`] gives them.
fn run_rows(draft: &SweepDraft, schema: &ModelSchema<'_>, tick_previews: &[Option<LevelPreview>]) -> [PlanRow; 6] {
    [
        PlanRow::new(
            "Steps per run",
            steps_text(draft.steps, draft.warmup),
            Some(SweepSection::RunLength),
        ),
        PlanRow::new(
            "Samples per run",
            samples_text(draft.steps, draft.stats_every),
            Some(SweepSection::Outputs),
        ),
        PlanRow::new(
            "Stop when",
            draft.stop.as_ref().map_or_else(|| "Never".to_owned(), stop_text),
            Some(SweepSection::RunLength),
        ),
        PlanRow::new(
            "Timeout",
            draft.timeout_s.map_or_else(|| "None".to_owned(), seconds_text),
            Some(SweepSection::RunLength),
        ),
        PlanRow::new(
            "Actions",
            actions_text(draft, schema, tick_previews),
            Some(SweepSection::Actions),
        ),
        PlanRow::new(
            "Outputs per run",
            draft.output_names(schema).len().to_string(),
            Some(SweepSection::Outputs),
        ),
    ]
}

/// Returns the Objective row of a search, or the Grid row of a Pattern Space Exploration.
fn objective_row(draft: &SweepDraft) -> PlanRow {
    let search = &draft.search;
    if search.algorithm == DraftAlgorithm::PatternSpace {
        let settings = &search.pattern_space;
        let automatic = match (settings.x_axis.is_automatic(), settings.y_axis.is_automatic()) {
            (true, true) => ", automatic ranges",
            (true, false) => ", automatic range on X axis",
            (false, true) => ", automatic range on Y axis",
            (false, false) => "",
        };
        return PlanRow::new(
            "Grid",
            format!("{} × {} cells{automatic}", settings.x_axis.cells, settings.y_axis.cells),
            Some(SweepSection::Search),
        );
    }
    let objective = if search.objective_column.is_empty() {
        "Not selected".to_owned()
    } else {
        let goal = match search.goal {
            Goal::Maximize => "maximized",
            Goal::Minimize => "minimized",
        };
        let aggregate = match search.aggregate {
            Aggregate::Median => "median",
            Aggregate::Mean => "mean",
        };
        format!("{}, {goal}, {aggregate}", output_label(&search.objective_column))
    };
    PlanRow::new("Objective", objective, Some(SweepSection::Search))
}

/// Returns the Runs row for `runs`, `None` while not counted, warning from half the app's limit.
fn runs_row(runs: Option<u64>) -> PlanRow {
    let mut row = PlanRow::new(
        "Runs",
        runs.map_or_else(|| NOT_COUNTED.to_owned(), |runs| runs.to_string()),
        Some(SweepSection::Seeds),
    );
    if runs.is_some_and(|runs| runs >= MAX_DRAFT_RUNS / 2) {
        row.warn = true;
        row.tooltip = Some(format!("Near the app's limit of {MAX_DRAFT_RUNS} runs"));
    }
    row
}

/// Returns the Series row for series written every `series_every` ticks, taking `bytes` once counted, and warning
/// from half the memory limit while the results stay `in_memory`.
fn series_row(series_every: u64, bytes: Option<u64>, in_memory: bool) -> PlanRow {
    let mut row = PlanRow::new("Series", NOT_COUNTED, Some(SweepSection::Outputs));
    if series_every == 0 {
        row.value = "None".to_owned();
        return row;
    }
    if let Some(bytes) = bytes {
        row.value = format!("About {}", fmt_bytes(bytes));
        if in_memory && bytes >= MAX_MEMORY_SERIES_BYTES / 2 {
            row.warn = true;
            row.tooltip = Some(format!(
                "Series near the memory limit of {}",
                fmt_bytes(MAX_MEMORY_SERIES_BYTES)
            ));
        }
    }
    row
}

/// Returns a row for each memory budget of `draft`, which only a loaded spec file sets.
fn budget_rows(draft: &SweepDraft) -> Vec<PlanRow> {
    let budgets = [
        ("Memory budget", draft.memory_budget),
        ("GPU memory budget", draft.gpu_memory_budget),
    ];
    budgets
        .into_iter()
        .filter_map(|(label, bytes)| {
            let mut row = PlanRow::new(label, fmt_bytes(bytes?), Some(SweepSection::Execution));
            row.tooltip = Some("From loaded spec file".to_owned());
            Some(row)
        })
        .collect()
}

/// Returns the Results row for results written to `folder`, or held in memory for `None`.
fn results_row(folder: Option<&Path>) -> PlanRow {
    let mut row = PlanRow::new("Results", "In memory", Some(SweepSection::Execution));
    if let Some(folder) = folder {
        row.value = folder_name(folder);
        row.tooltip = Some(folder.display().to_string());
    }
    row
}

/// Returns the last part of `folder`'s path, or the whole path when it has none.
pub fn folder_name(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Returns the text of a run of `steps` measured ticks after `warmup` ticks of warm-up.
fn steps_text(steps: u64, warmup: u64) -> String {
    match warmup {
        0 => steps.to_string(),
        warmup => format!("{steps} after {warmup} of warm-up"),
    }
}

/// Returns the samples a run of `steps` measured ticks takes at one every `stats_every` ticks, the first and the last
/// included, `None` for no sampling.
///
/// A count past `u64::MAX` saturates.
pub fn sample_count(steps: u64, stats_every: u64) -> Option<u64> {
    (stats_every > 0).then(|| {
        (steps / stats_every)
            .saturating_add(1)
            .saturating_add(u64::from(!steps.is_multiple_of(stats_every)))
    })
}

/// Returns the number of samples a run takes as the plan shows it, [`NOT_COUNTED`] for no sampling.
fn samples_text(steps: u64, stats_every: u64) -> String {
    sample_count(steps, stats_every).map_or_else(|| NOT_COUNTED.to_owned(), |samples| samples.to_string())
}

/// Returns the condition of `stop`, as in "Infected is at most 0, from tick 10".
pub fn stop_text(stop: &StopDraft) -> String {
    let mut text = format!(
        "{} is {} {}",
        stop.column,
        comparator_words(stop.comparator),
        format_significant(stop.threshold)
    );
    if stop.min_tick > 0 {
        text.push_str(&format!(", from tick {}", stop.min_tick));
    }
    text
}

/// Returns `seconds` as in "600 s".
pub fn seconds_text(seconds: f64) -> String {
    format!("{} s", format_significant(seconds))
}

/// Returns `concurrency` as the Concurrent runs row shows it. A browser runs one run at a time.
pub fn concurrency_text(concurrency: Concurrency) -> String {
    if cfg!(target_arch = "wasm32") {
        return "1".to_owned();
    }
    match concurrency {
        Concurrency::Auto => "Auto".to_owned(),
        Concurrency::Fixed(count) => count.to_string(),
    }
}

/// Returns the actions of `draft`, one per line, each with its tick or ticks, as in "Seed outbreak at 0, 100 or 200".
///
/// `tick_previews` holds the values of each action row's varied ticks, as [`SweepDraft::level_previews`] gives them.
/// Ticks with an issue read as typed.
fn actions_text(draft: &SweepDraft, schema: &ModelSchema<'_>, tick_previews: &[Option<LevelPreview>]) -> String {
    if draft.actions.is_empty() {
        return "None".to_owned();
    }
    let actions: Vec<String> = draft
        .actions
        .iter()
        .enumerate()
        .map(|(position, action)| {
            let label = action.label(schema);
            match draft.tick_source(action) {
                TickSource::Fixed => format!("{label} at {}", action.tick),
                TickSource::Table => format!("{label}, tick from design table"),
                TickSource::Varied => match tick_previews.get(position).and_then(Option::as_ref) {
                    Some(preview) => format!("{label} at {}", ticks_text(preview)),
                    None => format!("{label} at {}", action.ticks_text.trim()),
                },
            }
        })
        .collect();
    actions.join("\n")
}

/// Returns the ticks of `preview` as the Actions row reads them: "0, 100 or 200", "one of 11 ticks from 0 to 100", or
/// "any tick from 0 to 400" for ticks drawn from a range.
fn ticks_text(preview: &LevelPreview) -> String {
    match preview {
        LevelPreview::Listed { count, values, .. } if *count <= MAX_LISTED_TICKS => either_text(values),
        LevelPreview::Listed { count, values, last } => format!(
            "one of {count} ticks from {} to {last}",
            values.first().map_or("", String::as_str)
        ),
        LevelPreview::Drawn { min, max } => format!("any tick from {min} to {max}"),
    }
}

/// Returns `values` joined as alternatives, as in "0, 100 or 200".
fn either_text(values: &[String]) -> String {
    match values {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// Returns the configurations of a sweep of `draft` with `configs` in all, as its design combines them: "5 × 3 =
/// 15", "5 zipped", "5 + 3 = 8", "20 samples" or "12 rows of sir-design.csv".
fn configurations_text(
    draft: &SweepDraft,
    configs: u64,
    param_counts: &[Option<LevelCount>],
    tick_counts: &[Option<LevelCount>],
) -> String {
    let listed: Vec<String> = param_counts
        .iter()
        .chain(tick_counts)
        .filter_map(|count| match count {
            Some(LevelCount::Listed(count)) => Some(count.to_string()),
            Some(LevelCount::Drawn) | None => None,
        })
        .collect();
    match draft.design {
        DraftDesign::EveryCombination if listed.len() > 1 => format!("{} = {configs}", listed.join(" × ")),
        DraftDesign::VaryEachAlone if listed.len() > 1 => format!("{} = {configs}", listed.join(" + ")),
        DraftDesign::EveryCombination | DraftDesign::VaryEachAlone => configs.to_string(),
        DraftDesign::Zip => format!("{configs} zipped"),
        DraftDesign::LatinHypercube | DraftDesign::UniformRandom => format!("{configs} {}", plural(configs, "sample")),
        DraftDesign::Table => {
            let file = draft.table.as_ref().map_or("", |table| table.file_name.as_str());
            format!("{configs} {} of {file}", plural(configs, "row"))
        }
    }
}

/// Returns the draft's varied parameters and action ticks, each with a short text of its values.
fn varied_lines(
    draft: &SweepDraft,
    schema: &ModelSchema<'_>,
    param_counts: &[Option<LevelCount>],
    tick_counts: &[Option<LevelCount>],
) -> Vec<(String, String)> {
    let search = draft.mode == DraftMode::Search;
    if !search && draft.design == DraftDesign::Table {
        let columns = draft.table.as_ref().map(|table| table.columns()).unwrap_or_default();
        return schema
            .params
            .iter()
            .filter(|descriptor| columns.iter().any(|column| column == descriptor.id))
            .map(|descriptor| (descriptor.label.to_owned(), "From design table".to_owned()))
            .collect();
    }
    let params = schema
        .params
        .iter()
        .zip(&draft.factors)
        .zip(param_counts)
        .filter(|((_, factor), _)| factor.vary)
        .map(|((descriptor, factor), count)| {
            (
                descriptor.label.to_owned(),
                levels_summary(&factor.levels_text, *count, search),
            )
        });
    let ticks = draft
        .actions
        .iter()
        .zip(tick_counts)
        .filter(|(action, _)| action.vary_tick)
        .map(|(action, count)| {
            (
                format!("{} tick", action.label(schema)),
                levels_summary(&action.ticks_text, *count, search),
            )
        });
    params.chain(ticks).collect()
}

/// Returns a short text of the values `text` lists, `count` of them when counted, for a search when `search` is set.
fn levels_summary(text: &str, count: Option<LevelCount>, search: bool) -> String {
    if text.trim().is_empty() {
        return "no values yet".to_owned();
    }
    let Ok(levels) = parse_levels(text) else {
        return text.trim().to_owned();
    };
    match levels {
        LevelSpec::Values(values) if values.len() <= 4 => values.join(", "),
        LevelSpec::Values(values) => format!(
            "{} values, {} to {}",
            values.len(),
            values.first().map_or("", String::as_str),
            values.last().map_or("", String::as_str)
        ),
        LevelSpec::Range { min, max, step: None } if search => format!("any value from {min} to {max}"),
        LevelSpec::Range { min, max, step: None } => format!("{min} to {max}, drawn"),
        LevelSpec::Range {
            min,
            max,
            step: Some(_),
        } => match count {
            Some(LevelCount::Listed(count)) => format!("{min} to {max}, {count} {}", plural(count as u64, "value")),
            _ => format!("{min} to {max}"),
        },
        LevelSpec::All => "all options".to_owned(),
    }
}

/// Returns each parameter the draft holds at its Parameters tab value in `panel_values`, as in "Grid Width 1024".
fn held_values(draft: &SweepDraft, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> Vec<String> {
    let table_columns = match (&draft.mode, &draft.design, &draft.table) {
        (DraftMode::Sweep, DraftDesign::Table, Some(table)) => Some(table.columns()),
        (DraftMode::Sweep, DraftDesign::Table, None) => Some(Vec::new()),
        _ => None,
    };
    schema
        .params
        .iter()
        .zip(&draft.factors)
        .zip(panel_values)
        .filter(|((descriptor, factor), _)| match &table_columns {
            Some(columns) => !columns.iter().any(|column| column == descriptor.id),
            None => !factor.vary,
        })
        .map(|((descriptor, _), value)| format!("{} {}", descriptor.label, display_value(descriptor, value)))
        .collect()
}

/// Draws `summary`, with `caption` after the title during a session.
///
/// While `interactive` is set, each row label and each problem opens its section through `request`.
pub fn plan_ui(
    ui: &mut egui::Ui,
    summary: &PlanSummary,
    caption: Option<&str>,
    interactive: bool,
    request: &mut Option<SweepRequest>,
) {
    ui.horizontal(|ui| {
        ui.strong("Plan");
        if let Some(caption) = caption {
            ui.weak(caption);
        }
    });
    plan_body(ui, summary, interactive, request);
}

/// Draws `summary` without its title, as the Plan section does under its own header.
///
/// While `interactive` is set, each row label and each problem opens its section through `request`.
pub fn plan_body(ui: &mut egui::Ui, summary: &PlanSummary, interactive: bool, request: &mut Option<SweepRequest>) {
    kv_grid(ui, "henad_sweep_plan_grid").show(ui, |ui, grid_rows| {
        for row in &summary.rows {
            let label = match row.section {
                Some(section) if interactive => {
                    let label = ui
                        .add(Label::new(row.label).sense(Sense::click()))
                        .on_hover_text(format!("Open {} section", section.title()));
                    if label.clicked() {
                        *request = Some(SweepRequest::Reveal(Reveal::section(section)));
                    }
                    label
                }
                _ => ui.label(row.label),
            };
            let text = if row.warn {
                RichText::new(&row.value).color(ui.visuals().warn_fg_color)
            } else {
                RichText::new(&row.value)
            };
            let value = ui.add(Label::new(text).wrap()).labelled_by(label.id);
            if let Some(tooltip) = &row.tooltip {
                value.on_hover_text(tooltip);
            }
            grid_rows.end_row(ui);
        }
    });
    if !summary.varied.is_empty() {
        ui.add_space(4.0);
        ui.strong("Varied");
        for (label, values) in &summary.varied {
            ui.weak(format!("{label}  {values}"));
        }
    }
    if !summary.held.is_empty() {
        ui.add_space(4.0);
        ui.strong("Fixed");
        ui.weak(summary.held.join(", "));
    }
    if !summary.problems.is_empty() {
        ui.add_space(4.0);
        ui.strong("Problems");
        for line in &summary.problems {
            let text =
                RichText::new(format!("{} {}", issue_icon(line.kind), line.text)).color(issue_color(ui, line.kind));
            if interactive {
                let link = ui.add(Label::new(text).sense(Sense::click()));
                if link.on_hover_text("Show row").clicked() {
                    *request = Some(SweepRequest::Reveal(line.reveal()));
                }
            } else {
                ui.label(text);
            }
        }
    }
    if !summary.warnings.is_empty() {
        ui.add_space(4.0);
        ui.strong("Warnings");
        for warning in &summary.warnings {
            ui.colored_label(ui.visuals().warn_fg_color, format!("{MDI_ALERT} {warning}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use henad_compute::cpu::sim_thread::WakeFn;
    use henad_compute::entry::ModelEntry;
    use henad_core::params::ParamValue;
    use henad_models::example_models;

    use super::{NOT_COUNTED, PlanSummary, budget_rows, either_text, samples_text, steps_text};
    use crate::ui::sweep::draft::{DesignTableDraft, DraftAlgorithm, DraftDesign, DraftMode, GridAxis, SweepDraft};
    use crate::ui::sweep::{CheckSummary, SweepPanel};

    fn sir() -> ModelEntry {
        example_models().get("sir").cloned().expect("SIR is registered")
    }

    fn default_values(entry: &ModelEntry) -> Vec<ParamValue> {
        entry
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect()
    }

    /// Returns the plan of the draft that `edit` makes of a new SIR draft.
    fn plan_of(edit: impl FnOnce(&mut crate::ui::sweep::draft::SweepDraft, &[&str])) -> PlanSummary {
        let entry = sir();
        let schema = entry.schema();
        let panel_values = default_values(&entry);
        let ids: Vec<&str> = schema.params.iter().map(|descriptor| descriptor.id).collect();
        let mut panel = SweepPanel::default();
        edit(panel.draft_mut(&schema), &ids);
        let check = panel.cached_check(&schema, &panel_values);
        let summary = CheckSummary::new(check, &entry, &schema);
        PlanSummary::for_draft(check, &schema, entry.name(), &summary)
    }

    /// Returns the plan of the draft `panel` holds for `entry`'s model, checked against the model's defaults.
    fn plan_of_panel(panel: &mut SweepPanel, entry: &ModelEntry) -> PlanSummary {
        let schema = entry.schema();
        let check = panel.cached_check(&schema, &default_values(entry));
        let summary = CheckSummary::new(check, entry, &schema);
        PlanSummary::for_draft(check, &schema, entry.name(), &summary)
    }

    fn value<'a>(plan: &'a PlanSummary, label: &str) -> &'a str {
        plan.rows
            .iter()
            .find(|row| row.label == label)
            .map_or_else(|| panic!("no {label} row"), |row| row.value.as_str())
    }

    fn index(ids: &[&str], id: &str) -> usize {
        ids.iter()
            .position(|candidate| *candidate == id)
            .expect("SIR declares the parameter")
    }

    #[test]
    fn the_plan_lists_each_action_on_its_own_line_and_numbers_a_repeat() {
        let plan = plan_of(|draft, _| {
            let entry = sir();
            let schema = entry.schema();
            draft.add_action(&schema, 0, 0);
            draft.add_action(&schema, 0, 100);
            draft.actions[1].vary_tick = true;
            draft.actions[1].ticks_text = "100, 200".to_owned();
        });
        assert_eq!(
            value(&plan, "Actions"),
            "Seed outbreak at 0\nSeed outbreak 2 at 100 or 200"
        );
        assert_eq!(
            plan.varied,
            [("Seed outbreak 2 tick".to_owned(), "100, 200".to_owned())]
        );
    }

    #[test]
    fn a_ready_plan_counts_its_configurations_as_its_design_combines_them() {
        let plan = plan_of(|draft, ids| {
            draft.replicates = 3;
            draft.warmup = 100;
            let infection = index(ids, "infection_rate");
            let recovery = index(ids, "recovery_rate");
            draft.factors[infection].vary = true;
            draft.factors[infection].levels_text = "0.1:0.5:0.1".to_owned();
            draft.factors[recovery].vary = true;
            draft.factors[recovery].levels_text = "0.02, 0.05, 0.1".to_owned();
        });
        assert_eq!(value(&plan, "Design"), "Every combination");
        assert_eq!(value(&plan, "Configurations"), "5 × 3 = 15");
        assert_eq!(value(&plan, "Replicates"), "3, common random numbers");
        assert_eq!(value(&plan, "Runs"), "45");
        assert_eq!(value(&plan, "Steps per run"), "1000 after 100 of warm-up");
        assert_eq!(value(&plan, "Stop when"), "Never");
        assert_eq!(value(&plan, "Results"), "In memory");
        assert_eq!(plan.runs, Some(45));
        assert_eq!(plan.section_summary(), "45 runs");
        assert_eq!(
            plan.varied,
            [
                ("Infection Rate".to_owned(), "0.1 to 0.5, 5 values".to_owned()),
                ("Recovery Rate".to_owned(), "0.02, 0.05, 0.1".to_owned()),
            ]
        );
        assert!(
            plan.held.iter().all(|held| !held.starts_with("Infection Rate")),
            "a varied parameter is not held"
        );
        assert!(plan.problems.is_empty());

        let alone = plan_of(|draft, ids| {
            draft.design = DraftDesign::VaryEachAlone;
            let infection = index(ids, "infection_rate");
            let recovery = index(ids, "recovery_rate");
            draft.factors[infection].vary = true;
            draft.factors[infection].levels_text = "0.1:0.5:0.1".to_owned();
            draft.factors[recovery].vary = true;
            draft.factors[recovery].levels_text = "0.02, 0.05, 0.1".to_owned();
        });
        assert_eq!(value(&alone, "Configurations"), "5 + 3 = 8");
    }

    #[test]
    fn a_plan_with_problems_leaves_its_totals_uncounted_and_lists_the_problems() {
        let plan = plan_of(|draft, ids| {
            let infection = index(ids, "infection_rate");
            draft.factors[infection].vary = true;
            draft.factors[infection].levels_text = "0.x".to_owned();
        });
        assert_eq!(value(&plan, "Configurations"), NOT_COUNTED);
        assert_eq!(value(&plan, "Runs"), NOT_COUNTED);
        assert_eq!(value(&plan, "Series"), NOT_COUNTED);
        assert_eq!(plan.runs, None);
        assert_eq!(plan.section_summary(), NOT_COUNTED);
        assert_eq!(plan.problems.len(), 1);
        assert_eq!(plan.problems[0].text, "Infection Rate: '0.x' is not a number");
    }

    #[test]
    fn a_search_plan_names_its_method_budget_and_objective() {
        let plan = plan_of(|draft, ids| {
            draft.mode = DraftMode::Search;
            let infection = index(ids, "infection_rate");
            draft.factors[infection].vary = true;
            draft.factors[infection].levels_text = "0:1".to_owned();
        });
        assert_eq!(value(&plan, "Method"), "Random search");
        assert_eq!(value(&plan, "Evaluations"), "200 in batches of 16");
        assert_eq!(value(&plan, "Objective"), "Susceptible, maximum, maximized, median");
        assert_eq!(value(&plan, "Runs"), "200");
        assert_eq!(
            plan.varied,
            [("Infection Rate".to_owned(), "any value from 0 to 1".to_owned())]
        );

        let pattern_plan = |bounded_y: bool| {
            plan_of(|draft, _| {
                draft.mode = DraftMode::Search;
                draft.search.algorithm = DraftAlgorithm::PatternSpace;
                if bounded_y {
                    draft.search.set_automatic_range(GridAxis::Y, false);
                }
            })
        };
        assert_eq!(value(&pattern_plan(false), "Grid"), "20 × 20 cells, automatic ranges");
        assert_eq!(
            value(&pattern_plan(true), "Grid"),
            "20 × 20 cells, automatic range on X axis"
        );
    }

    #[test]
    fn run_lengths_count_every_sample_and_name_the_warm_up() {
        assert_eq!(steps_text(1000, 0), "1000");
        assert_eq!(steps_text(1000, 100), "1000 after 100 of warm-up");
        assert_eq!(samples_text(1000, 1), "1001");
        assert_eq!(samples_text(1000, 10), "101");
        assert_eq!(samples_text(1005, 10), "102", "the last tick is sampled too");
        assert_eq!(samples_text(1000, 0), NOT_COUNTED);
        assert_eq!(
            samples_text(u64::MAX, 1),
            u64::MAX.to_string(),
            "a count past the largest integer saturates"
        );
        assert_eq!(
            either_text(&["0".to_owned(), "100".to_owned(), "200".to_owned()]),
            "0, 100 or 200"
        );
        assert_eq!(either_text(&["7".to_owned()]), "7");
    }

    /// Returns the Actions row of the plan of a SIR draft with one action at tick 50, which `edit` changes.
    fn actions_row(edit: impl FnOnce(&mut crate::ui::sweep::draft::SweepDraft)) -> String {
        let plan = plan_of(|draft, _| {
            let entry = sir();
            draft.add_action(&entry.schema(), 0, 50);
            edit(draft);
        });
        value(&plan, "Actions").to_owned()
    }

    #[test]
    fn the_actions_row_reads_varied_ticks_as_words() {
        let vary = |ticks: &'static str| {
            move |draft: &mut crate::ui::sweep::draft::SweepDraft| {
                draft.actions[0].vary_tick = true;
                draft.actions[0].ticks_text = ticks.to_owned();
            }
        };
        assert_eq!(actions_row(|_| {}), "Seed outbreak at 50");
        assert_eq!(actions_row(vary("0:30:10")), "Seed outbreak at 0, 10, 20 or 30");
        assert_eq!(
            actions_row(vary("0:100:10")),
            "Seed outbreak at one of 11 ticks from 0 to 100"
        );
        assert_eq!(
            actions_row(|draft| {
                vary("0:400")(draft);
                draft.mode = DraftMode::Search;
            }),
            "Seed outbreak at any tick from 0 to 400"
        );
    }

    #[test]
    fn a_range_searched_past_the_listed_values_reads_as_words() {
        let plan = plan_of(|draft, _| {
            let entry = sir();
            draft.add_action(&entry.schema(), 0, 50);
            draft.actions[0].vary_tick = true;
            draft.actions[0].ticks_text = "0:2000000".to_owned();
            draft.mode = DraftMode::Search;
        });
        assert_eq!(value(&plan, "Actions"), "Seed outbreak at any tick from 0 to 2000000");
        assert_eq!(
            plan.varied,
            [(
                "Seed outbreak tick".to_owned(),
                "any value from 0 to 2000000".to_owned()
            )]
        );
    }

    #[test]
    fn a_design_table_sets_the_tick_the_actions_row_reads() {
        let under_table = |text: &str| {
            let text = text.to_owned();
            actions_row(move |draft| {
                draft.actions[0].vary_tick = true;
                draft.actions[0].ticks_text = "20, 40".to_owned();
                draft.design = DraftDesign::Table;
                draft.table = Some(DesignTableDraft {
                    file_name: "design.csv".to_owned(),
                    text,
                });
            })
        };
        assert_eq!(
            under_table("infection_rate,action.seed_outbreak\n0.1,20\n"),
            "Seed outbreak, tick from design table"
        );
        assert_eq!(
            under_table("infection_rate\n0.1\n"),
            "Seed outbreak at 50",
            "a table without the action's column leaves its tick"
        );
    }

    #[test]
    fn the_outputs_per_run_count_each_part_of_a_vector_stat() {
        let entry = example_models().get("boids").cloned().expect("boids is registered");
        let mut panel = SweepPanel::default();
        assert_eq!(
            value(&plan_of_panel(&mut panel, &entry), "Outputs per run"),
            "8",
            "each stat counts as one column until a build reports"
        );
        let (sender, woken) = flume::bounded(1);
        let wake: WakeFn = Arc::new(move || {
            sender.try_send(()).ok();
        });
        panel.learn_stat_columns(&entry, &wake);
        woken
            .recv_timeout(Duration::from_secs(60))
            .expect("the build of boids reports its columns");
        panel.learn_stat_columns(&entry, &wake);
        assert_eq!(
            value(&plan_of_panel(&mut panel, &entry), "Outputs per run"),
            "16",
            "four defaults of Average Speed, and of each of Average Velocity's x, y and magnitude"
        );
    }

    #[test]
    fn the_plan_lists_each_loaded_budget() {
        let mut draft = SweepDraft::new(&sir().schema());
        assert!(budget_rows(&draft).is_empty(), "a draft without budgets lists none");
        draft.memory_budget = Some(4 << 30);
        draft.gpu_memory_budget = Some(2 << 30);
        let rows: Vec<(&str, String)> = budget_rows(&draft)
            .into_iter()
            .map(|row| (row.label, row.value))
            .collect();
        assert_eq!(
            rows,
            [
                ("Memory budget", "4.0 GB".to_owned()),
                ("GPU memory budget", "2.0 GB".to_owned())
            ]
        );
    }
}
