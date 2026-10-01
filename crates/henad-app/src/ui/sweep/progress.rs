//! Progress of a sweep or search: the text of its progress bar, the grids of its status, its search and its result,
//! and the runs in progress.
//!
//! While a session runs, the Status grid counts its runs and names how they run. Once it ends, the Result grid takes
//! its place. A search adds a Search grid of its budget and its best candidate either way.

use std::path::Path;
use std::time::Duration;

use egui::containers::Sides;
use egui::{Label, ProgressBar, RichText, Ui};
use henad_core::explore::search::{SearchAlgorithm, SearchSpec};
use henad_core::helpers::fmt_bytes;
use henad_core::metadata::Backend;
use henad_explore::exec::{ActiveRun, ExecutionLayout};
use henad_explore::handle::SweepProgress;
use henad_explore::search_run::SearchUpdate;
use henad_explore::sweep::{SweepEnd, SweepReport};

use crate::icons::material_design_icons::MDI_ALERT;
use crate::ui::results::ResultsPanel;
use crate::ui::results::plot::format_precise;
use crate::ui::results::store::output_label;
use crate::ui::sweep::draft::{batch_estimate, capitalize, generation_estimate};
use crate::ui::sweep::layout::{IssueCount, SectionHeading, section, truncated_label};
use crate::ui::sweep::session::{SessionState, SweepSession};
use crate::ui::sweep::{SweepRequest, format_duration};
use crate::ui::{add_progress_bar, banner, kv_grid, mcs, plural};

/// Most runs in progress the tab lists one by one.
const MAX_ACTIVE_LINES: usize = 8;

/// Shortest time left the progress bar shows, in milliseconds. A shorter estimate says nothing useful.
const MIN_REMAINING_MS: u128 = 100;

/// Width of the bar of a run in progress, in points.
const RUN_BAR_WIDTH: f32 = 120.0;

/// Height of the bar of a run in progress, in points.
const RUN_BAR_HEIGHT: f32 = 10.0;

/// Salt of the id of the Runs in progress section.
const ACTIVE_RUNS_ID: &str = "henad_sweep_progress_active";

/// Returns the runs that have finished at `progress`, those waiting on an earlier run included, as the command line
/// counts them.
fn finished_runs(progress: &SweepProgress) -> u64 {
    progress.runs_done + progress.runs_waiting
}

/// Returns the share of the runs finished at `progress`, from 0 to 1.
pub fn finished_fraction(progress: &SweepProgress) -> f32 {
    match progress.runs_total {
        0 => 0.0,
        total => (finished_runs(progress) as f64 / total as f64) as f32,
    }
}

/// Returns the text of the progress bar of a session in `state` at `progress`.
pub fn bar_text(progress: &SweepProgress, state: SessionState) -> String {
    let (finished, total) = (finished_runs(progress), progress.runs_total);
    let runs = format!("{finished} of {total} {}", plural(total, "run"));
    match state {
        SessionState::Planning => "Building first configuration".to_owned(),
        SessionState::Paused => format!("Paused · {runs}"),
        SessionState::Running
        | SessionState::Finished
        | SessionState::Aborted
        | SessionState::Stopped
        | SessionState::Failed => {
            let mut text = runs;
            if progress.runs_failed > 0 {
                text.push_str(&format!(" · {} failed", progress.runs_failed));
            }
            if let Some(remaining) = progress
                .remaining
                .filter(|remaining| remaining.as_millis() >= MIN_REMAINING_MS)
            {
                text.push_str(&format!(" · about {} left", format_duration(remaining)));
            }
            text
        }
    }
}

/// End of a session, as the frozen progress bar shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionResult {
    pub text: String,
    /// Share of the plan's runs written, from 0 to 1.
    pub fraction: f32,
    /// Whether the session ended on an error of its own.
    pub failed: bool,
}

impl SessionResult {
    /// Returns the end of the sweep or search `noun` names, from its `report` or its `failure`, with the time of
    /// `progress`. That time leaves pauses out.
    pub fn new(noun: &str, report: Option<&SweepReport>, failure: Option<&str>, progress: &SweepProgress) -> Self {
        let noun = capitalize(noun);
        if let Some(failure) = failure {
            return Self {
                text: format!("{noun} failed: {failure}"),
                fraction: finished_fraction(progress),
                failed: true,
            };
        }
        let Some(report) = report else {
            return Self {
                text: format!("{noun} ended"),
                fraction: finished_fraction(progress),
                failed: false,
            };
        };
        let (rows, planned) = (report.counts.rows, report.outline.runs);
        let fraction = match planned {
            0 => 1.0,
            planned => (rows as f64 / planned as f64).min(1.0) as f32,
        };
        let elapsed = format_duration(progress.elapsed);
        let text = match report.end {
            SweepEnd::Aborted => {
                format!(
                    "{noun} aborted after {rows} of {planned} {}, in {elapsed}",
                    plural(planned, "run")
                )
            }
            SweepEnd::DeviceLost => format!(
                "{noun} stopped after {rows} of {planned} {}: GPU device lost",
                plural(planned, "run")
            ),
            SweepEnd::Complete | SweepEnd::Planned => format!(
                "{noun} finished: {rows} {}, {} failed, in {elapsed}",
                plural(rows, "run"),
                report.counts.failed
            ),
        };
        Self {
            text,
            fraction,
            failed: false,
        }
    }
}

/// Returns the text of the Abort modal: the runs an abort of the sweep at `progress` keeps, then each kind it drops.
pub fn abort_text(progress: &SweepProgress) -> String {
    let (done, waiting) = (progress.runs_done, progress.runs_waiting);
    let active = progress.active_runs.len() as u64;
    let mut text = format!("{done} finished {} will be kept in the results.", plural(done, "run"));
    if active > 0 {
        text.push_str(&format!(
            " {active} {} in progress will be discarded.",
            plural(active, "run")
        ));
    }
    if waiting > 0 {
        text.push_str(&format!(
            " {waiting} finished {} waiting for an earlier run will be discarded.",
            plural(waiting, "run")
        ));
    }
    text
}

/// Returns the sentence of the Abort modal on whether the sweep or search `noun` names can run the rest later. Only
/// one writing to a folder can.
pub fn resume_text(noun: &str, in_folder: bool) -> String {
    if in_folder {
        format!("Press Resume {noun} in the Results tab to run the rest later.")
    } else {
        format!("An aborted {noun} in memory cannot resume.")
    }
}

/// Button that follows a value in a grid of the progress view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowButton {
    /// Lists the failed runs in the Results tab.
    ShowFailedRuns,
    /// Selects the first run of the search candidate in the Results tab.
    ShowCandidate(u64),
}

impl RowButton {
    /// Returns the button's text, its tooltip and the request a click sends.
    fn parts(self) -> (&'static str, &'static str, SweepRequest) {
        match self {
            Self::ShowFailedRuns => (
                "Show failed runs",
                "List failed runs in the Results tab",
                SweepRequest::ShowFailedRuns,
            ),
            Self::ShowCandidate(candidate_id) => (
                "Show in Results",
                "Select candidate's first run in the Results tab",
                SweepRequest::ShowCandidate(candidate_id),
            ),
        }
    }
}

/// One row of a grid of the progress view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressRow {
    pub label: String,
    pub value: String,
    /// Whether the value counts failures, and is drawn in the error colour.
    pub failure: bool,
    /// Text shown on hover over the value.
    pub tooltip: Option<String>,
    pub button: Option<RowButton>,
}

impl ProgressRow {
    fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            failure: false,
            tooltip: None,
            button: None,
        }
    }
}

/// Returns the Failed row for `failed` runs. Once there are any, the row lists them in the Results tab.
fn failed_row(failed: u64) -> ProgressRow {
    let mut row = ProgressRow::new("Failed", failed.to_string());
    if failed > 0 {
        row.failure = true;
        row.button = Some(RowButton::ShowFailedRuns);
    }
    row
}

/// Execution a planned sweep settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedExecution {
    pub layout: ExecutionLayout,
    pub backend: Backend,
    /// Bytes the live runs are projected to hold together.
    pub projected_bytes: u64,
}

/// Returns the runs `execution` steps at once, as in "4, with 3 threads each" or "2 on the GPU".
pub fn runs_at_once_text(execution: &PlannedExecution) -> String {
    let layout = &execution.layout;
    match execution.backend {
        Backend::Gpu => format!("{} on the GPU", layout.gpu_tracks),
        Backend::Cpu => {
            let threads = layout.threads_per_lane as u64;
            let each = if layout.cpu_lanes == 1 { "" } else { " each" };
            format!(
                "{}, with {threads} {}{each}",
                layout.cpu_lanes,
                plural(threads, "thread")
            )
        }
    }
}

/// Returns the time a session in `state` has left at `progress`: an estimate, "Paused", or the event an estimate
/// waits for.
pub fn left_text(progress: &SweepProgress, state: SessionState) -> String {
    if state.is_paused() {
        return "Paused".to_owned();
    }
    match progress.remaining {
        Some(remaining) => format!("About {}", format_duration(remaining)),
        None => "Will appear after the first run finishes".to_owned(),
    }
}

/// Returns the Results row of a running `noun`, whose results go to `output_dir` or stay in memory for `None`.
pub fn running_results_text(noun: &str, output_dir: Option<&Path>) -> String {
    match output_dir {
        Some(dir) => dir.display().to_string(),
        None => format!("In memory. Save them once the {noun} ends."),
    }
}

/// Returns the Results row of an ended session whose results went to `output_dir`, or stayed in memory for `None`,
/// where `saved` says whether they were saved since.
pub fn ended_results_text(output_dir: Option<&Path>, saved: bool) -> String {
    match output_dir {
        Some(dir) => format!("Written to {}", dir.display()),
        None if saved => "Saved".to_owned(),
        None => "In memory, not saved".to_owned(),
    }
}

/// Inputs of the Status grid of a running session.
pub struct StatusInput<'a> {
    pub progress: &'a SweepProgress,
    pub state: SessionState,
    /// Execution of the sweep once planned, `None` while it plans.
    pub execution: Option<PlannedExecution>,
    /// Where the results go, as the Results row reads it.
    pub results: &'a str,
    /// Whether the session resumes a folder, keeping the runs the folder holds.
    pub resumed: bool,
}

/// Returns the rows of the Status grid of a running session.
pub fn status_rows(input: &StatusInput<'_>) -> Vec<ProgressRow> {
    let progress = input.progress;
    let (finished, total) = (finished_runs(progress), progress.runs_total);
    let running = progress.active_runs.len() as u64;
    let mut rows = vec![ProgressRow::new(
        "Finished",
        format!("{finished} of {total} {}", plural(total, "run")),
    )];
    if input.resumed {
        rows.push(ProgressRow::new("Kept from folder", progress.runs_skipped.to_string()));
    }
    rows.push(ProgressRow::new("Running", running.to_string()));
    if progress.runs_waiting > 0 {
        let mut waiting = ProgressRow::new("Waiting", progress.runs_waiting.to_string());
        waiting.tooltip =
            Some("Finished runs waiting for an earlier run to be written. Aborting will discard them.".to_owned());
        rows.push(waiting);
    }
    rows.push(ProgressRow::new(
        "Queued",
        total.saturating_sub(finished + running).to_string(),
    ));
    rows.push(failed_row(progress.runs_failed));
    rows.push(ProgressRow::new("Elapsed", format_duration(progress.elapsed)));
    rows.push(ProgressRow::new("Left", left_text(progress, input.state)));
    let (at_once, memory) = match &input.execution {
        Some(execution) => (
            runs_at_once_text(execution),
            format!("About {}", fmt_bytes(execution.projected_bytes)),
        ),
        None => ("Planning".to_owned(), "Planning".to_owned()),
    };
    rows.push(ProgressRow::new("Runs at once", at_once));
    rows.push(ProgressRow::new("Projected memory", memory));
    rows.push(ProgressRow::new("Results", input.results));
    rows
}

/// Runs an ended session wrote, and how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ending {
    pub state: SessionState,
    /// Runs written, the runs a resume kept included.
    pub written: u64,
    /// Runs of the whole plan.
    pub planned: u64,
    pub failed: u64,
}

impl Ending {
    /// Returns the end of a session that ended in `state`, read from its `report`, or from `progress` for a session
    /// that failed with none.
    pub fn new(state: SessionState, report: Option<&SweepReport>, progress: &SweepProgress) -> Self {
        match report {
            Some(report) => Self {
                state,
                written: report.counts.rows,
                planned: report.outline.runs,
                failed: report.counts.failed,
            },
            None => Self {
                state,
                written: progress.runs_done + progress.runs_skipped,
                planned: progress.runs_total + progress.runs_skipped,
                failed: progress.runs_failed,
            },
        }
    }
}

/// Returns the rows of the Result grid of a session that ended as `ending` after `elapsed` of running, with its
/// results at `results`.
pub fn result_rows(ending: &Ending, elapsed: Duration, results: &str) -> Vec<ProgressRow> {
    let runs = if ending.state == SessionState::Finished {
        format!("{} finished", ending.written)
    } else {
        format!("{} of {} finished", ending.written, ending.planned)
    };
    vec![
        ProgressRow::new("Runs", runs),
        failed_row(ending.failed),
        ProgressRow::new("Time", format!("{}, excluding pauses", format_duration(elapsed))),
        ProgressRow::new("Results", results),
    ]
}

/// Returns the note under the Result grid of the `noun` that ended as `ending`, `None` for one that finished or
/// failed.
///
/// The note says whether and where the runs it did not write can run. Only results in a folder can resume.
pub fn result_note(noun: &str, ending: &Ending, in_folder: bool) -> Option<String> {
    let left = ending.planned.saturating_sub(ending.written);
    match ending.state {
        // An abort or a lost device can land after the last run is written.
        SessionState::Aborted | SessionState::Stopped if left == 0 => {
            Some(format!("Every run finished before the {noun} ended."))
        }
        SessionState::Aborted if in_folder => Some(format!(
            "Press Resume {noun} in the Results tab to run the remaining {left}."
        )),
        SessionState::Aborted => Some(format!("An aborted {noun} in memory cannot resume.")),
        SessionState::Stopped if in_folder => Some(format!(
            "Press Resume {noun} in the Results tab to run the rest on a working device."
        )),
        SessionState::Stopped => Some(format!("A stopped {noun} in memory cannot resume.")),
        _ => None,
    }
}

/// Standing of a search, as the Search grid shows it.
pub struct SearchStanding<'a> {
    pub search: &'a SearchSpec,
    /// Update after the last batch the search was told, `None` before the first.
    pub update: Option<&'a SearchUpdate>,
    /// Generations of a genetic algorithm that finished before the last batch, the index of that batch's generation.
    pub generation: u64,
    /// Label and value of each parameter and tick of the best candidate, empty while the Results tab lacks them.
    pub best_values: &'a [(String, String)],
}

/// Returns the rows of the Search grid: the evaluations run of the budget, the generation of a genetic algorithm,
/// and the best candidate with its values, or the cells a Pattern Space Exploration filled.
pub fn search_rows(standing: &SearchStanding<'_>) -> Vec<ProgressRow> {
    let search = standing.search;
    let budget = search.max_evaluations;
    let genetic = match &search.algorithm {
        SearchAlgorithm::Genetic(settings) => Some(settings),
        _ => None,
    };
    // A hill climb or a genetic algorithm can ask for fewer candidates than a batch holds.
    let exact = matches!(
        search.algorithm,
        SearchAlgorithm::Random | SearchAlgorithm::PatternSpaceExploration(_)
    );
    let evaluations = match standing.update {
        Some(update) => {
            let batches = match &search.algorithm {
                SearchAlgorithm::PatternSpaceExploration(settings) => settings.batch_count(budget, search.batch_size),
                _ => batch_estimate(budget, search.batch_size, genetic),
            };
            let about = if exact { "" } else { "about " };
            format!(
                "{} of {budget}, batch {} of {about}{batches}",
                update.evaluations,
                update.batch + 1
            )
        }
        None => format!("0 of {budget}"),
    };
    let mut rows = vec![ProgressRow::new("Evaluations", evaluations)];
    if let Some(settings) = genetic {
        let current = standing.generation + 1;
        let value = match generation_estimate(budget, settings) {
            Some(total) => format!("{current} of about {total}"),
            None => current.to_string(),
        };
        rows.push(ProgressRow::new("Generation", value));
    }
    if let SearchAlgorithm::PatternSpaceExploration(settings) = &search.algorithm {
        // An automatic range places no evaluation in a cell until the initial samples fix it.
        let waiting = !settings.has_ranges() && standing.update.is_none_or(|update| update.pattern_settings.is_none());
        let value = if waiting {
            "Waiting for initial samples".to_owned()
        } else {
            let cells = u64::from(settings.x_axis.cells) * u64::from(settings.y_axis.cells);
            let filled = standing.update.map_or(0, |update| update.filled_cells);
            format!("{filled} of {cells}")
        };
        rows.push(ProgressRow::new("Cells filled", value));
        return rows;
    }
    let Some(best) = standing.update.and_then(|update| update.best.as_ref()) else {
        rows.push(ProgressRow::new("Best", "Will appear after the first batch"));
        return rows;
    };
    let value = format_precise(best.objective);
    let best_text = match &search.objective {
        Some(objective) => format!("{} {value}", output_label(&objective.column)),
        None => value,
    };
    rows.push(ProgressRow::new("Best", best_text));
    let mut candidate = ProgressRow::new(
        "Best candidate",
        format!(
            "{}, {} {}",
            best.candidate_id,
            best.replicate_count,
            plural(best.replicate_count, "replicate")
        ),
    );
    candidate.button = Some(RowButton::ShowCandidate(best.candidate_id));
    rows.push(candidate);
    rows.extend(
        standing
            .best_values
            .iter()
            .map(|(label, value)| ProgressRow::new(label.as_str(), value.as_str())),
    );
    rows
}

/// Returns the label of run in progress `active`, and its tooltip.
///
/// `values` are the values of its config as the Results tab holds them, `None` while it lacks them. A search names
/// its configs candidates, and learns a candidate's values once the candidate's batch ends.
pub fn active_run_label(active: &ActiveRun, search: bool, values: Option<&str>) -> (String, Option<&'static str>) {
    let noun = if search { "Candidate" } else { "Config" };
    let head = format!("{noun} {} · replicate {}", active.run.config_id, active.run.rep);
    match values {
        Some(values) if !values.is_empty() => (format!("{head}: {values}"), None),
        None if search => (head, Some("Values will appear once its batch ends")),
        _ => (head, None),
    }
}

/// Returns the text of the bar of run in progress `active`, as in "671 of 1000".
pub fn run_bar_text(active: &ActiveRun) -> String {
    format!("{} of {}", active.tick, active.end_tick)
}

/// Returns the share of its ticks that run in progress `active` has stepped, from 0 to 1.
fn run_fraction(active: &ActiveRun) -> f32 {
    match active.end_tick {
        0 => 0.0,
        end_tick => (active.tick as f64 / end_tick as f64).min(1.0) as f32,
    }
}

/// Everything the progress view reads.
pub struct ProgressView<'a> {
    pub session: &'a SweepSession,
    pub progress: &'a SweepProgress,
    pub state: SessionState,
    /// Results tab, holding the session's runs and the values of their configs.
    pub results: &'a ResultsPanel,
}

/// Draws the body of the tab while a session exists: its status or its result, its search, its runs in progress and
/// its warnings.
pub fn progress_ui(ui: &mut Ui, view: &ProgressView<'_>, request: &mut Option<SweepRequest>) {
    let session = view.session;
    let (progress, state) = (view.progress, view.state);
    let noun = session.noun();
    if let Some(failure) = &session.failure {
        let title = format!("{} failed", capitalize(noun));
        banner(ui, MDI_ALERT, ui.visuals().error_fg_color, &title, failure);
    }
    let output_dir = session.output_dir.as_deref();
    if state.is_running() {
        ui.strong("Status");
        let results = running_results_text(noun, output_dir);
        let input = StatusInput {
            progress,
            state,
            execution: session.outline.as_ref().map(|outline| PlannedExecution {
                layout: outline.layout,
                backend: outline.backend,
                projected_bytes: outline.projected_bytes,
            }),
            results: &results,
            resumed: session.is_resumed(),
        };
        progress_grid(ui, "henad_sweep_status_grid", &status_rows(&input), request);
    } else {
        ui.strong("Result");
        let ending = Ending::new(state, session.report.as_ref(), progress);
        let saved = view.results.files().is_some() && !view.results.holds_unsaved_files();
        let results = ended_results_text(output_dir, saved);
        let rows = result_rows(&ending, progress.elapsed, &results);
        progress_grid(ui, "henad_sweep_result_grid", &rows, request);
        if let Some(note) = result_note(noun, &ending, output_dir.is_some()) {
            ui.weak(note);
        }
    }
    search_block(ui, view, request);
    if state.is_running() {
        ui.separator();
        active_runs(ui, view);
    }
    for warning in &session.warnings {
        ui.colored_label(ui.visuals().warn_fg_color, format!("{MDI_ALERT} {warning}"));
    }
}

/// Draws the Search grid of a search, and nothing for a sweep.
fn search_block(ui: &mut Ui, view: &ProgressView<'_>, request: &mut Option<SweepRequest>) {
    let session = view.session;
    let Some(search_plan) = session.search_plan() else {
        return;
    };
    let update = session.latest_search_update.as_deref();
    let best_values = update
        .and_then(|update| update.best.as_ref())
        .zip(view.results.store())
        .and_then(|(best, store)| store.config_values(best.candidate_id))
        .unwrap_or_default();
    let standing = SearchStanding {
        search: search_plan.search(),
        update,
        generation: session.latest_generation,
        best_values: &best_values,
    };
    ui.add_space(4.0);
    ui.strong("Search");
    progress_grid(ui, "henad_sweep_search_grid", &search_rows(&standing), request);
}

/// Draws `rows` in a two-column grid of id `id`, each value labelled by its row's label.
fn progress_grid(ui: &mut Ui, id: &str, rows: &[ProgressRow], request: &mut Option<SweepRequest>) {
    kv_grid(ui, id).show(ui, |ui, grid_rows| {
        for row in rows {
            let label = ui.label(row.label.as_str());
            let mut text = RichText::new(row.value.as_str());
            if row.failure {
                text = text.color(ui.visuals().error_fg_color);
            }
            let value = match row.button {
                None => ui.add(Label::new(text).wrap()),
                Some(button) => {
                    ui.horizontal(|ui| {
                        let value = ui.label(text);
                        let (button_text, tooltip, pressed) = button.parts();
                        if ui.small_button(button_text).on_hover_text(tooltip).clicked() {
                            *request = Some(pressed);
                        }
                        value
                    })
                    .inner
                }
            };
            let value = value.labelled_by(label.id);
            if let Some(tooltip) = &row.tooltip {
                value.on_hover_text(tooltip);
            }
            grid_rows.end_row(ui);
        }
    });
}

/// Draws the Runs in progress section: a line and a bar for each run, [`MAX_ACTIVE_LINES`] at most.
fn active_runs(ui: &mut Ui, view: &ProgressView<'_>) {
    let active_runs = &view.progress.active_runs;
    let summary = format!("{} running", active_runs.len());
    let heading = SectionHeading {
        title: "Runs in progress",
        issues: IssueCount::default(),
        summary: &summary,
    };
    let search = view.session.search_plan().is_some();
    let store = view.results.store();
    section(ui, ACTIVE_RUNS_ID, heading, true, false, |ui, _| {
        if active_runs.is_empty() {
            ui.weak("None");
            return;
        }
        for active in active_runs.iter().take(MAX_ACTIVE_LINES) {
            let values = store.and_then(|store| store.config_values_text(active.run.config_id));
            let (text, tooltip) = active_run_label(active, search, values.as_deref());
            Sides::new().shrink_left().truncate().show(
                ui,
                |ui| {
                    let label = truncated_label(ui, text, None);
                    if let Some(tooltip) = tooltip {
                        label.on_hover_text(tooltip);
                    }
                },
                |ui| {
                    let bar = ProgressBar::new(run_fraction(active))
                        .desired_width(RUN_BAR_WIDTH)
                        .desired_height(RUN_BAR_HEIGHT)
                        .text(RichText::new(run_bar_text(active)).small())
                        .corner_radius(0);
                    add_progress_bar(ui, bar, mcs::BLUE_700);
                },
            );
        }
        if active_runs.len() > MAX_ACTIVE_LINES {
            ui.weak(format!("and {} more", active_runs.len() - MAX_ACTIVE_LINES));
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use henad_core::explore::outcome::PlannedRun;
    use henad_core::explore::search::genetic::GeneticSettings;
    use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
    use henad_core::explore::search::{Aggregate, Goal, Objective, RankingEntry, SearchAlgorithm, SearchSpec};
    use henad_core::metadata::Backend;
    use henad_explore::exec::{ActiveRun, ExecutionLayout};
    use henad_explore::handle::{SweepPhase, SweepProgress};
    use henad_explore::search_run::SearchUpdate;

    use super::{
        Ending, PlannedExecution, ProgressRow, RowButton, SearchStanding, SessionResult, StatusInput, abort_text,
        active_run_label, bar_text, ended_results_text, finished_fraction, result_note, result_rows, resume_text,
        run_bar_text, running_results_text, runs_at_once_text, search_rows, status_rows,
    };
    use crate::ui::sweep::session::SessionState;

    fn progress() -> SweepProgress {
        SweepProgress {
            phase: SweepPhase::Running,
            runs_total: 60,
            runs_skipped: 0,
            runs_done: 10,
            runs_waiting: 2,
            runs_failed: 0,
            elapsed: Duration::from_secs(4),
            remaining: Some(Duration::from_secs(16)),
            active_runs: Vec::new(),
        }
    }

    fn active(run_id: u64) -> ActiveRun {
        ActiveRun {
            run: PlannedRun {
                run_id,
                config_id: run_id,
                rep: 0,
                seed: 0,
            },
            tick: 3,
            end_tick: 10,
        }
    }

    /// Returns the value of the row labelled `label` in `rows`.
    fn value<'a>(rows: &'a [ProgressRow], label: &str) -> &'a str {
        rows.iter()
            .find(|row| row.label == label)
            .map_or_else(|| panic!("no {label} row"), |row| row.value.as_str())
    }

    fn labels(rows: &[ProgressRow]) -> Vec<&str> {
        rows.iter().map(|row| row.label.as_str()).collect()
    }

    #[test]
    fn the_abort_modal_names_every_run_an_abort_drops() {
        let mut progress = SweepProgress {
            runs_total: 15,
            runs_done: 0,
            runs_waiting: 7,
            remaining: None,
            active_runs: vec![active(0)],
            ..progress()
        };
        assert_eq!(
            abort_text(&progress),
            "0 finished runs will be kept in the results. 1 run in progress will be discarded. 7 finished runs \
             waiting for an earlier run will be discarded."
        );
        progress.runs_done = 1;
        progress.runs_waiting = 0;
        progress.active_runs.push(active(1));
        assert_eq!(
            abort_text(&progress),
            "1 finished run will be kept in the results. 2 runs in progress will be discarded."
        );
    }

    #[test]
    fn the_progress_bar_counts_finished_runs_and_hides_the_estimate_while_paused() {
        let mut progress = progress();
        assert_eq!(
            bar_text(&progress, SessionState::Running),
            "12 of 60 runs · about 16 s left"
        );
        assert_eq!(finished_fraction(&progress), 0.2);
        progress.runs_failed = 1;
        progress.remaining = Some(Duration::from_millis(40));
        assert_eq!(
            bar_text(&progress, SessionState::Running),
            "12 of 60 runs · 1 failed",
            "an estimate under 100 ms says nothing"
        );
        assert_eq!(bar_text(&progress, SessionState::Paused), "Paused · 12 of 60 runs");
        assert_eq!(
            bar_text(&progress, SessionState::Planning),
            "Building first configuration"
        );
        progress.runs_total = 0;
        assert_eq!(finished_fraction(&progress), 0.0);
    }

    #[test]
    fn a_failed_session_keeps_its_fraction_and_names_its_error() {
        let result = SessionResult::new("search", None, Some("Device lost"), &progress());
        assert_eq!(result.text, "Search failed: Device lost");
        assert_eq!(result.fraction, 0.2);
        assert!(result.failed);
    }

    #[test]
    fn only_a_sweep_in_a_folder_can_resume_after_an_abort() {
        assert_eq!(
            resume_text("sweep", true),
            "Press Resume sweep in the Results tab to run the rest later."
        );
        assert_eq!(
            resume_text("search", false),
            "An aborted search in memory cannot resume."
        );
    }

    #[test]
    fn the_status_grid_counts_the_runs_and_names_how_they_run() {
        let mut progress = SweepProgress {
            active_runs: vec![active(12), active(13), active(14), active(15)],
            ..progress()
        };
        let results = running_results_text("sweep", None);
        let execution = PlannedExecution {
            layout: ExecutionLayout {
                cpu_lanes: 4,
                threads_per_lane: 3,
                gpu_tracks: 0,
            },
            backend: Backend::Cpu,
            projected_bytes: 1_288_490_189,
        };
        let input = StatusInput {
            progress: &progress,
            state: SessionState::Running,
            execution: Some(execution),
            results: &results,
            resumed: false,
        };
        let rows = status_rows(&input);
        assert_eq!(
            labels(&rows),
            [
                "Finished",
                "Running",
                "Waiting",
                "Queued",
                "Failed",
                "Elapsed",
                "Left",
                "Runs at once",
                "Projected memory",
                "Results",
            ]
        );
        assert_eq!(value(&rows, "Finished"), "12 of 60 runs");
        assert_eq!(value(&rows, "Running"), "4");
        assert_eq!(value(&rows, "Queued"), "44");
        assert_eq!(value(&rows, "Left"), "About 16 s");
        assert_eq!(value(&rows, "Runs at once"), "4, with 3 threads each");
        assert_eq!(value(&rows, "Projected memory"), "About 1.2 GB");
        assert_eq!(value(&rows, "Results"), "In memory. Save them once the sweep ends.");
        let failed = rows.iter().find(|row| row.label == "Failed").expect("a Failed row");
        assert_eq!((failed.failure, failed.button), (false, None));

        progress.runs_waiting = 0;
        progress.runs_failed = 1;
        progress.runs_skipped = 30;
        progress.remaining = None;
        let input = StatusInput {
            progress: &progress,
            state: SessionState::Paused,
            execution: None,
            results: "/tmp/sweep-01",
            resumed: true,
        };
        let rows = status_rows(&input);
        assert_eq!(value(&rows, "Kept from folder"), "30");
        assert!(!labels(&rows).contains(&"Waiting"), "no runs wait");
        assert_eq!(value(&rows, "Left"), "Paused");
        assert_eq!(value(&rows, "Runs at once"), "Planning");
        let failed = rows.iter().find(|row| row.label == "Failed").expect("a Failed row");
        assert!(failed.failure);
        assert_eq!(failed.button, Some(RowButton::ShowFailedRuns));
    }

    #[test]
    fn runs_at_once_name_the_lanes_or_the_tracks() {
        let execution = |cpu_lanes, threads_per_lane, gpu_tracks, backend| PlannedExecution {
            layout: ExecutionLayout {
                cpu_lanes,
                threads_per_lane,
                gpu_tracks,
            },
            backend,
            projected_bytes: 0,
        };
        assert_eq!(
            runs_at_once_text(&execution(1, 12, 0, Backend::Cpu)),
            "1, with 12 threads"
        );
        assert_eq!(
            runs_at_once_text(&execution(12, 1, 0, Backend::Cpu)),
            "12, with 1 thread each"
        );
        assert_eq!(runs_at_once_text(&execution(0, 0, 2, Backend::Gpu)), "2 on the GPU");
    }

    #[test]
    fn the_result_grid_counts_what_an_ending_kept_and_what_can_run_later() {
        let finished = Ending {
            state: SessionState::Finished,
            written: 45,
            planned: 45,
            failed: 1,
        };
        let rows = result_rows(&finished, Duration::from_secs(12), "Saved");
        assert_eq!(value(&rows, "Runs"), "45 finished");
        assert_eq!(value(&rows, "Time"), "12 s, excluding pauses");
        assert_eq!(
            rows.iter().find(|row| row.label == "Failed").and_then(|row| row.button),
            Some(RowButton::ShowFailedRuns)
        );
        assert_eq!(result_note("sweep", &finished, true), None);

        let aborted = Ending {
            state: SessionState::Aborted,
            written: 39,
            planned: 300,
            failed: 0,
        };
        let rows = result_rows(&aborted, Duration::from_secs(12), "Saved");
        assert_eq!(value(&rows, "Runs"), "39 of 300 finished");
        assert_eq!(
            result_note("sweep", &aborted, true).as_deref(),
            Some("Press Resume sweep in the Results tab to run the remaining 261.")
        );
        assert_eq!(
            result_note("search", &aborted, false).as_deref(),
            Some("An aborted search in memory cannot resume.")
        );
        let stopped = Ending {
            state: SessionState::Stopped,
            ..aborted
        };
        assert_eq!(
            result_note("sweep", &stopped, true).as_deref(),
            Some("Press Resume sweep in the Results tab to run the rest on a working device.")
        );
        let aborted_after_the_last_run = Ending {
            written: 300,
            ..aborted
        };
        for in_folder in [true, false] {
            assert_eq!(
                result_note("search", &aborted_after_the_last_run, in_folder).as_deref(),
                Some("Every run finished before the search ended.")
            );
        }
        let stopped_after_the_last_run = Ending {
            state: SessionState::Stopped,
            ..aborted_after_the_last_run
        };
        assert_eq!(
            result_note("sweep", &stopped_after_the_last_run, true).as_deref(),
            Some("Every run finished before the sweep ended.")
        );

        let failed = Ending::new(SessionState::Failed, None, &progress());
        assert_eq!((failed.written, failed.planned), (10, 60), "waiting runs are dropped");
        assert_eq!(
            ended_results_text(Some(Path::new("/tmp/sweep-01")), false),
            "Written to /tmp/sweep-01"
        );
        assert_eq!(ended_results_text(None, false), "In memory, not saved");
        assert_eq!(ended_results_text(None, true), "Saved");
    }

    fn search(algorithm: SearchAlgorithm) -> SearchSpec {
        let objective = Objective {
            column: "Susceptible:max".to_owned(),
            goal: Goal::Maximize,
            aggregate: Aggregate::Median,
        };
        SearchSpec {
            objective: (!matches!(algorithm, SearchAlgorithm::PatternSpaceExploration(_))).then_some(objective),
            algorithm,
            max_evaluations: 100,
            batch_size: 16,
            space: Vec::new(),
        }
    }

    fn update(batch: u64, evaluations: u64, best: Option<RankingEntry>, filled_cells: u64) -> SearchUpdate {
        SearchUpdate {
            batch,
            evaluations,
            runs: evaluations * 2,
            evaluated: Vec::new(),
            best,
            generations: Vec::new(),
            landed_entries: Vec::new(),
            filled_cells,
            pattern_settings: None,
        }
    }

    #[test]
    fn the_search_grid_shows_the_budget_the_generation_and_the_best_candidate() {
        let genetic = search(SearchAlgorithm::Genetic(GeneticSettings::default()));
        let best = RankingEntry {
            candidate_id: 41,
            objective: 1_027_606.0,
            replicate_count: 2,
            failed_count: 0,
            evaluations: 1,
            first_batch: 1,
        };
        let told = update(2, 48, Some(best), 0);
        let best_values = [("Infection Rate".to_owned(), "0.32".to_owned())];
        let standing = SearchStanding {
            search: &genetic,
            update: Some(&told),
            generation: 1,
            best_values: &best_values,
        };
        let rows = search_rows(&standing);
        assert_eq!(
            labels(&rows),
            ["Evaluations", "Generation", "Best", "Best candidate", "Infection Rate"]
        );
        assert_eq!(value(&rows, "Evaluations"), "48 of 100, batch 3 of about 7");
        assert_eq!(value(&rows, "Generation"), "2 of about 3");
        assert_eq!(value(&rows, "Best"), "Susceptible, maximum 1,027,606");
        assert_eq!(value(&rows, "Best candidate"), "41, 2 replicates");
        assert_eq!(
            rows.iter()
                .find(|row| row.label == "Best candidate")
                .and_then(|row| row.button),
            Some(RowButton::ShowCandidate(41))
        );
        assert_eq!(value(&rows, "Infection Rate"), "0.32");

        let random = search(SearchAlgorithm::Random);
        let standing = SearchStanding {
            search: &random,
            update: None,
            generation: 0,
            best_values: &[],
        };
        let rows = search_rows(&standing);
        assert_eq!(value(&rows, "Evaluations"), "0 of 100");
        assert_eq!(value(&rows, "Best"), "Will appear after the first batch");

        let axis = |column: &str| PatternAxis::bounded(column, 0.0, 1.0, 20);
        let pattern = search(SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings::new(
            axis("Infected:max"),
            axis("Infected:argmax"),
        )));
        let told = update(0, 16, None, 9);
        let standing = SearchStanding {
            search: &pattern,
            update: Some(&told),
            generation: 0,
            best_values: &[],
        };
        let rows = search_rows(&standing);
        assert_eq!(labels(&rows), ["Evaluations", "Cells filled"]);
        assert_eq!(value(&rows, "Evaluations"), "16 of 100, batch 1 of 7");
        assert_eq!(value(&rows, "Cells filled"), "9 of 400");
        let cut = search(SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings {
            initial_samples: 50,
            ..PatternSpaceSettings::new(axis("Infected:max"), axis("Infected:argmax"))
        }));
        let standing = SearchStanding {
            search: &cut,
            update: Some(&told),
            generation: 0,
            best_values: &[],
        };
        assert_eq!(
            value(&search_rows(&standing), "Evaluations"),
            "16 of 100, batch 1 of 8",
            "the 50 initial samples end a batch of their own"
        );

        let automatic = |column: &str| PatternAxis::automatic(column, 20);
        let settings = PatternSpaceSettings::new(automatic("Infected:max"), automatic("Infected:argmax"));
        let pattern = search(SearchAlgorithm::PatternSpaceExploration(settings.clone()));
        let waiting = update(0, 16, None, 0);
        let ranged = SearchUpdate {
            pattern_settings: Some(settings.with_automatic_ranges(&[(0.0, 0.0), (10.0, 10.0)])),
            ..update(4, 80, None, 12)
        };
        let cells_filled = |update: &SearchUpdate| {
            let standing = SearchStanding {
                search: &pattern,
                update: Some(update),
                generation: 0,
                best_values: &[],
            };
            value(&search_rows(&standing), "Cells filled").to_owned()
        };
        assert_eq!(cells_filled(&waiting), "Waiting for initial samples");
        assert_eq!(cells_filled(&ranged), "12 of 400", "counted once the range is taken");
    }

    #[test]
    fn a_run_in_progress_names_its_config_and_its_values() {
        let run = ActiveRun {
            tick: 671,
            end_tick: 1000,
            ..active(3)
        };
        assert_eq!(
            active_run_label(&run, false, Some("Infection Rate 0.2, Recovery Rate 0.05")),
            (
                "Config 3 · replicate 0: Infection Rate 0.2, Recovery Rate 0.05".to_owned(),
                None
            )
        );
        assert_eq!(
            active_run_label(&run, false, Some("")),
            ("Config 3 · replicate 0".to_owned(), None),
            "a sweep that varies nothing"
        );
        assert_eq!(
            active_run_label(&run, true, None),
            (
                "Candidate 3 · replicate 0".to_owned(),
                Some("Values will appear once its batch ends")
            )
        );
        assert_eq!(run_bar_text(&run), "671 of 1000");
    }
}
