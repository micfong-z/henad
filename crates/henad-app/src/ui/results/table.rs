//! Runs view: every run in a table, and a strip that opens the selected run in the viewport.

use std::collections::BTreeSet;

use egui_extras::{Column, TableBuilder};
use henad_core::explore::outcome::{RunOutcome, RunStatus, StopReason};
use web_time::Instant;

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CONTENT_COPY, MDI_FAST_FORWARD, MDI_MENU_DOWN, MDI_MENU_UP, MDI_PLAY_BOX_OUTLINE,
};
use crate::state::OpenAt;
use crate::ui::results::ResultsRequest;
use crate::ui::results::plot::{format_significant, labeled_combo, refresh_due, text_width};
use crate::ui::results::store::{ResultsStore, RunsColumn, RunsFilter, RunsSort};

/// Height of a row of the table.
const ROW_HEIGHT: f32 = 18.0;

/// Least width of any column.
const MIN_COLUMN_WIDTH: f32 = 48.0;

/// Settings of the Runs view.
#[derive(Debug, Default)]
pub struct TableView {
    pub filter: RunsFilter,
    sort: RunsSort,
    cache: Option<RowOrder>,
}

/// Order of the table's rows, with the settings and the store revision it was sorted from.
#[derive(Debug)]
struct RowOrder {
    revision: u64,
    filter: RunsFilter,
    sort: RunsSort,
    selected_configs: BTreeSet<u64>,
    positions: Vec<usize>,
    sorted_at: Instant,
}

impl RunsFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All runs",
            Self::Failed => "Failed runs",
            Self::SelectedConfigs => "Selected configurations",
        }
    }
}

/// Returns the name the table gives `status`.
pub fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Ok => "OK",
        RunStatus::NonFinite => "Not finite",
        RunStatus::Panicked => "Panicked",
        RunStatus::GpuError => "GPU error",
        RunStatus::Refused => "Refused",
        RunStatus::ShapeError => "Shape error",
        RunStatus::TimedOut => "Timed out",
    }
}

pub fn table_ui(
    ui: &mut egui::Ui,
    store: &ResultsStore,
    view: &mut TableView,
    selected_configs: &BTreeSet<u64>,
    selected_run: Option<u64>,
    request: &mut Option<ResultsRequest>,
) {
    ui.horizontal_wrapped(|ui| {
        labeled_combo(ui, "Show", "henad_results_runs_filter", view.filter.label(), |ui| {
            for filter in [RunsFilter::All, RunsFilter::Failed, RunsFilter::SelectedConfigs] {
                ui.selectable_value(&mut view.filter, filter, filter.label());
            }
        });
        let clearable = view.filter == RunsFilter::SelectedConfigs && !selected_configs.is_empty();
        if ui
            .add_enabled(clearable, egui::Button::new("Clear selection"))
            .on_hover_text("Deselect all configurations")
            .clicked()
        {
            *request = Some(ResultsRequest::ClearSelection);
        }
    });
    let columns = columns(store);
    let sort = view.sort;
    let positions = view.positions(ui.ctx(), store, selected_configs);
    let mut clicked_header = None;
    let widths: Vec<f32> = columns
        .iter()
        .map(|(column, title)| min_column_width(ui, *column, title))
        .collect();
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let mut table = TableBuilder::new(ui)
            .id_salt("henad_results_runs")
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for &width in &widths {
            table = table.column(Column::auto().at_least(width).clip(true));
        }
        table
            .header(ROW_HEIGHT + 4.0, |mut header| {
                for (column, title) in &columns {
                    header.col(|ui| {
                        let marker = match sort {
                            RunsSort {
                                column: sorted,
                                descending,
                            } if sorted == *column => {
                                if descending {
                                    MDI_MENU_DOWN
                                } else {
                                    MDI_MENU_UP
                                }
                            }
                            _ => "",
                        };
                        if ui.button(header_text(title, marker)).clicked() {
                            clicked_header = Some(*column);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, positions.len(), |mut row| {
                    let outcome = &store.runs()[positions[row.index()]];
                    let run_id = outcome.run.run_id;
                    row.set_selected(selected_run == Some(run_id));
                    for (column, _) in &columns {
                        row.col(|ui| cell(ui, store, outcome, *column));
                    }
                    if row.response().clicked() {
                        *request = Some(ResultsRequest::SelectRun(run_id));
                    }
                });
            });
    });
    if let Some(column) = clicked_header {
        view.sort = RunsSort {
            column,
            descending: sort.column == column && !sort.descending,
        };
    }
}

/// Returns every column of the table with its title: the run's ids and outcome, each axis, then each output.
fn columns(store: &ResultsStore) -> Vec<(RunsColumn, String)> {
    let mut columns: Vec<(RunsColumn, String)> = [
        (RunsColumn::Run, "Run"),
        (RunsColumn::Config, store.config_noun()),
        (RunsColumn::Replicate, "Rep"),
        (RunsColumn::Seed, "Seed"),
        (RunsColumn::Status, "Status"),
        (RunsColumn::Ticks, "Ticks"),
        (RunsColumn::Time, "Time"),
    ]
    .into_iter()
    .map(|(column, title)| (column, title.to_owned()))
    .collect();
    let axes = store.axes().iter().enumerate();
    columns.extend(axes.map(|(index, axis)| (RunsColumn::Axis(index), axis.label.clone())));
    let outputs = 0..store.reducer_columns().len();
    columns.extend(outputs.map(|index| (RunsColumn::Output(index), store.output_label(index))));
    columns
}

/// Returns the text of the header button of the column `title` names, `marker` its sort arrow or empty.
///
/// The arrow leads. A header cut off at the edge of the tab still shows the sort.
fn header_text(title: &str, marker: &str) -> String {
    if marker.is_empty() {
        title.to_owned()
    } else {
        format!("{marker} {title}")
    }
}

/// Returns the least width of `column`, headed `title`.
///
/// The header button fits with either sort arrow, and the Seed column fits a seed of as many digits as
/// [`u64::MAX`], 20, with the header's padding to spare.
fn min_column_width(ui: &egui::Ui, column: RunsColumn, title: &str) -> f32 {
    let padding = 2.0 * ui.spacing().button_padding.x;
    let header = [MDI_MENU_UP, MDI_MENU_DOWN]
        .map(|marker| text_width(ui, &header_text(title, marker), egui::TextStyle::Button))
        .into_iter()
        .fold(0.0, f32::max)
        + padding;
    let cell = if column == RunsColumn::Seed {
        let digits = u64::MAX.to_string().len();
        // Digits of a proportional font can differ in width.
        ('0'..='9')
            .map(|digit| text_width(ui, &digit.to_string().repeat(digits), egui::TextStyle::Body))
            .fold(0.0, f32::max)
            + padding
    } else {
        0.0
    };
    header.max(cell).max(MIN_COLUMN_WIDTH).ceil()
}

/// Draws the cell of `outcome` in `column`.
fn cell(ui: &mut egui::Ui, store: &ResultsStore, outcome: &RunOutcome, column: RunsColumn) {
    let run = &outcome.run;
    let text = match column {
        RunsColumn::Status => {
            let mut text = egui::RichText::new(status_label(outcome.status));
            if outcome.status.is_failure() {
                text = text.color(ui.visuals().error_fg_color);
            }
            // A selectable label takes the click that selects the row.
            let label = ui.add(egui::Label::new(text).selectable(false));
            if let Some(detail) = status_detail(outcome) {
                label.on_hover_text(detail);
            }
            return;
        }
        RunsColumn::Run => run.run_id.to_string(),
        RunsColumn::Config => run.config_id.to_string(),
        RunsColumn::Replicate => run.rep.to_string(),
        RunsColumn::Seed => run.seed.to_string(),
        RunsColumn::Ticks => outcome.ticks.to_string(),
        RunsColumn::Time => format_time(outcome.wall_ms),
        RunsColumn::Axis(axis) => store
            .config_level(run.config_id, axis)
            .and_then(|level| store.axes().get(axis)?.levels.get(level).cloned())
            .unwrap_or_default(),
        RunsColumn::Output(output) => outcome
            .reducers
            .get(output)
            .copied()
            .flatten()
            .map_or_else(String::new, format_significant),
    };
    ui.add(egui::Label::new(text).selectable(false));
}

/// Returns the note of `outcome`, or the tick a stop condition ended it on.
fn status_detail(outcome: &RunOutcome) -> Option<String> {
    outcome.note.clone().or_else(|| {
        (outcome.stop_reason == StopReason::Condition).then(|| format!("Stopped at tick {}", outcome.ticks))
    })
}

/// Returns `milliseconds` as a short duration.
fn format_time(milliseconds: f64) -> String {
    if !milliseconds.is_finite() {
        String::new()
    } else if milliseconds < 10_000.0 {
        format!("{milliseconds:.0} ms")
    } else {
        format!("{:.1} s", milliseconds / 1000.0)
    }
}

impl TableView {
    /// Returns the positions of the listed runs in order.
    ///
    /// The order is sorted again at once when the settings change, and at most once per [`REFRESH_INTERVAL`] while
    /// the store keeps changing. In between, runs that arrived since the last sort are not listed yet, and `ctx`
    /// repaints once the interval ends.
    ///
    /// [`REFRESH_INTERVAL`]: crate::ui::results::plot::REFRESH_INTERVAL
    fn positions(&mut self, ctx: &egui::Context, store: &ResultsStore, selected_configs: &BTreeSet<u64>) -> &[usize] {
        let (filter, sort) = (self.filter, self.sort);
        let stale = self.cache.as_ref().is_some_and(|order| {
            let settings_changed = order.filter != filter
                || order.sort != sort
                || (filter == RunsFilter::SelectedConfigs && order.selected_configs != *selected_configs);
            if settings_changed || order.revision == store.revision() {
                return settings_changed;
            }
            refresh_due(ctx, order.sorted_at)
        });
        if stale {
            self.cache = None;
        }
        let order = self.cache.get_or_insert_with(|| RowOrder {
            revision: store.revision(),
            filter,
            sort,
            selected_configs: selected_configs.clone(),
            positions: store.row_order(filter, selected_configs, sort),
            sorted_at: Instant::now(),
        });
        &order.positions
    }
}

/// Draws the strip for run `run_id`: its name, Open, Open at end and Copy command.
///
/// The buttons are disabled with the store's [`ResultsStore::replay_refusal`] when the runs cannot replay.
pub fn detail_strip(ui: &mut egui::Ui, store: &ResultsStore, run_id: u64, request: &mut Option<ResultsRequest>) {
    let Some(outcome) = store.run(run_id) else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!("Run {run_id}"));
        ui.weak(store.config_label(outcome.run.config_id));
    });
    let refusal = store.replay_refusal();
    let replays = refusal.is_none();
    let refusal_text = refusal.unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        let open = ui
            .add_enabled(replays, egui::Button::new(format!("{MDI_PLAY_BOX_OUTLINE} Open")))
            .on_hover_text("Build this run at tick 0 in the Viewport tab")
            .on_disabled_hover_text(refusal_text);
        if open.clicked() {
            *request = Some(ResultsRequest::OpenRun {
                run_id,
                start: OpenAt::Start,
            });
        }
        let end = ui
            .add_enabled(replays, egui::Button::new(format!("{MDI_FAST_FORWARD} Open at end")))
            .on_hover_text(format!("Build this run and step to its last tick, {}", outcome.ticks))
            .on_disabled_hover_text(refusal_text);
        if end.clicked() {
            let start = if outcome.ticks == 0 {
                OpenAt::Start
            } else {
                OpenAt::Tick(outcome.ticks)
            };
            *request = Some(ResultsRequest::OpenRun { run_id, start });
        }
        let copy = ui
            .add_enabled(replays, egui::Button::new(format!("{MDI_CONTENT_COPY} Copy command")))
            .on_hover_text("Copy henad-cli command to replay this run")
            .on_disabled_hover_text(refusal_text);
        if copy.clicked() {
            *request = Some(ResultsRequest::CopyCommand(run_id));
        }
    });
    if let Some(refusal) = refusal {
        ui.colored_label(ui.visuals().warn_fg_color, format!("{MDI_ALERT} {refusal}"));
    } else if !store.schema_matches {
        let noun = if store.is_search() { "search" } else { "sweep" };
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{MDI_ALERT} Model parameters changed since this {noun}. The replay might differ."),
        );
    }
}

#[cfg(test)]
mod tests {
    use egui::{Align, Button, Label, Layout, TextWrapMode, Widget, vec2};

    use super::{ROW_HEIGHT, header_text, min_column_width};
    use crate::icons::material_design_icons::{MDI_MENU_DOWN, MDI_MENU_UP};
    use crate::ui::results::store::RunsColumn;

    /// Returns the width `widget` takes untruncated, laid out in a table cell `width` wide.
    fn intrinsic_width(ui: &mut egui::Ui, width: f32, widget: impl Widget) -> f32 {
        let layout = Layout::left_to_right(Align::Center);
        ui.allocate_ui_with_layout(vec2(width, ROW_HEIGHT), layout, |ui| {
            ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
            ui.add(widget).intrinsic_size().map_or(f32::INFINITY, |size| size.x)
        })
        .inner
    }

    #[test]
    fn a_full_seed_and_a_sorted_header_fit_their_columns() {
        let context = egui::Context::default();
        crate::init::setup_custom_fonts(&context);
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let seed_width = min_column_width(ui, RunsColumn::Seed, "Seed");
            let padding = 2.0 * ui.spacing().button_padding.x;
            for seed in [u64::MAX, 10_000_000_000_000_000_000, 17_777_777_777_777_777_777] {
                let label = Label::new(seed.to_string()).selectable(false);
                let width = intrinsic_width(ui, seed_width, label);
                assert!(
                    width + padding <= seed_width,
                    "seed {seed} takes {width} of {seed_width}, with {padding} to spare"
                );
            }
            let columns = [
                (RunsColumn::Seed, "Seed"),
                (RunsColumn::Replicate, "Rep"),
                (RunsColumn::Output(0), "Infected (max)"),
                (RunsColumn::Output(1), "Recovered (first <= 10)"),
            ];
            for (column, title) in columns {
                let column_width = min_column_width(ui, column, title);
                for marker in ["", MDI_MENU_UP, MDI_MENU_DOWN] {
                    let button = Button::new(header_text(title, marker));
                    let width = intrinsic_width(ui, column_width, button);
                    assert!(
                        width <= column_width,
                        "{title} {marker} takes {width} of {column_width}"
                    );
                }
            }
        });
        output.drop_without_applying_deltas();
    }
}
