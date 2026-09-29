//! Response view: an output against one varied parameter or action tick, with whiskers for its spread.

use egui_plot::{Legend, Line, Plot, Points, uniform_grid_spacer};
use henad_core::explore::summary::ReplicateSummary;
use web_time::Instant;

use crate::ui::results::plot::{config_color, labeled_combo, level_formatter, refresh_due};
use crate::ui::results::store::{ResponseLine, ResponseQuery, ResultsAxis, ResultsStore};
use crate::ui::show_plot;

/// Most levels an axis can have for its points to be joined into lines.
const MAX_JOINED_LEVELS: usize = 32;

/// Spread a whisker spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorBars {
    /// The 95% confidence interval of the mean.
    #[default]
    ConfidenceInterval,
    /// One sample standard deviation either side of the mean.
    StandardDeviation,
    Hidden,
}

impl ErrorBars {
    fn label(self) -> &'static str {
        match self {
            Self::ConfidenceInterval => "95% CI",
            Self::StandardDeviation => "SD",
            Self::Hidden => "None",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::ConfidenceInterval => "95% confidence interval (CI) of the mean",
            Self::StandardDeviation => "Mean ± one standard deviation (SD) of runs",
            Self::Hidden => "No error bars",
        }
    }
}

/// Settings of the Response view.
#[derive(Debug, Default)]
pub struct ResponseView {
    x_axis: usize,
    /// Index of the reducer column.
    output: usize,
    group_axis: Option<usize>,
    /// Level each axis is held at, `None` for any level.
    pins: Vec<Option<usize>>,
    error_bars: ErrorBars,
    cache: Option<ResponseCache>,
}

/// Lines of one response plot, with the query and the store revision they were computed from.
#[derive(Debug)]
struct ResponseCache {
    revision: u64,
    computed_at: Instant,
    query: ResponseQuery,
    lines: Vec<ResponseLine>,
}

pub fn response_ui(ui: &mut egui::Ui, store: &ResultsStore, view: &mut ResponseView) {
    let axes = store.axes();
    if axes.is_empty() {
        ui.weak("Vary at least one parameter to plot a response.");
        return;
    }
    if store.reducer_columns().is_empty() {
        ui.weak("Response will appear once the first run finishes.");
        return;
    }
    view.fit(store);
    controls(ui, store, view);
    let (output, error_bars) = (view.output, view.error_bars);
    let x_axis = &axes[view.x_axis];
    let group_axis = view.group_axis.and_then(|axis| axes.get(axis));
    let query = ResponseQuery {
        x_axis: view.x_axis,
        output,
        group_axis: view.group_axis,
        pins: view.pins.clone(),
    };
    let lines = view.lines(ui.ctx(), store, &query);
    let whisker_half_width = whisker_half_width(x_axis);
    // Lines come in the order of their group's levels. A legend sorted by name lists level 10 before level 2.
    let mut plot = Plot::new("henad_results_response")
        .legend(Legend::default().follow_insertion_order(true))
        .x_axis_label(x_axis.label.as_str())
        .y_axis_label(store.output_label(output));
    if !x_axis.numeric {
        plot = plot
            .x_axis_formatter(level_formatter(&x_axis.levels))
            .x_grid_spacer(uniform_grid_spacer(|_| [1.0, 5.0, 10.0]));
    }
    show_plot(ui, plot, |plot_ui| {
        for (position, line) in lines.iter().enumerate() {
            let color = config_color(position);
            let name = match (group_axis, line.group_level) {
                (Some(axis), Some(level)) => format!("{} {}", axis.label, axis.levels[level]),
                _ => store.output_label(output),
            };
            let means: Vec<[f64; 2]> = line
                .points
                .iter()
                .filter_map(|point| Some([point.x, point.summary.mean?]))
                .collect();
            if x_axis.levels.len() <= MAX_JOINED_LEVELS {
                plot_ui.line(Line::new(name.clone(), means.clone()).color(color).width(1.5));
            }
            plot_ui.points(Points::new(name, means).color(color).radius(3.5));
            for point in &line.points {
                let Some((low, high)) = whisker(point.summary, error_bars) else {
                    continue;
                };
                let (left, right) = (point.x - whisker_half_width, point.x + whisker_half_width);
                for segment in [
                    [[point.x, low], [point.x, high]],
                    [[left, low], [right, low]],
                    [[left, high], [right, high]],
                ] {
                    plot_ui.line(
                        Line::new("", segment.to_vec())
                            .color(color)
                            .width(1.0)
                            .allow_hover(false),
                    );
                }
            }
        }
    });
}

/// Returns the low and high ends of the whisker `error_bars` asks for, or `None` when there is none to draw.
fn whisker(summary: ReplicateSummary, error_bars: ErrorBars) -> Option<(f64, f64)> {
    match error_bars {
        ErrorBars::ConfidenceInterval => summary.ci95,
        ErrorBars::StandardDeviation => {
            let (mean, deviation) = (summary.mean?, summary.standard_deviation?);
            Some((mean - deviation, mean + deviation))
        }
        ErrorBars::Hidden => None,
    }
}

/// Returns half the width of a whisker's caps on `axis`, a fifth of the smallest gap between two levels.
fn whisker_half_width(axis: &ResultsAxis) -> f64 {
    let smallest_gap = axis
        .positions
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|gap| *gap > 0.0)
        .fold(f64::INFINITY, f64::min);
    if smallest_gap.is_finite() {
        smallest_gap * 0.2
    } else {
        0.1
    }
}

fn controls(ui: &mut egui::Ui, store: &ResultsStore, view: &mut ResponseView) {
    let axes = store.axes();
    ui.horizontal_wrapped(|ui| {
        let x_text = axes[view.x_axis].label.as_str();
        labeled_combo(ui, "X axis", "henad_results_response_x", x_text, |ui| {
            for (index, axis) in axes.iter().enumerate() {
                ui.selectable_value(&mut view.x_axis, index, axis.label.as_str());
            }
        });
        output_combo(ui, "henad_results_response_output", store, &mut view.output);
        let group_text = view.group_axis.map_or("None", |axis| axes[axis].label.as_str());
        labeled_combo(ui, "Group by", "henad_results_response_group", group_text, |ui| {
            ui.selectable_value(&mut view.group_axis, None, "None");
            for (index, axis) in axes.iter().enumerate().filter(|&(index, _)| index != view.x_axis) {
                ui.selectable_value(&mut view.group_axis, Some(index), axis.label.as_str());
            }
        });
        let bars_text = view.error_bars.label();
        labeled_combo(ui, "Error bars", "henad_results_response_error_bars", bars_text, |ui| {
            for bars in [
                ErrorBars::ConfidenceInterval,
                ErrorBars::StandardDeviation,
                ErrorBars::Hidden,
            ] {
                ui.selectable_value(&mut view.error_bars, bars, bars.label())
                    .on_hover_text(bars.tooltip());
            }
        })
        .on_hover_text(view.error_bars.tooltip());
    });
    let free = [Some(view.x_axis), view.group_axis];
    pins_row(ui, "henad_results_response_pin", axes, &mut view.pins, &free);
}

/// Draws the Output combo, picking a reducer column of `store` into `output`.
pub fn output_combo(ui: &mut egui::Ui, id: &str, store: &ResultsStore, output: &mut usize) {
    let selected_text = store.output_label(*output);
    labeled_combo(ui, "Output", id, &selected_text, |ui| {
        for index in 0..store.reducer_columns().len() {
            ui.selectable_value(output, index, store.output_label(index));
        }
    });
}

/// Draws a combo for each axis outside `free` that holds it at one level or at any, into `pins`.
pub fn pins_row(ui: &mut egui::Ui, id: &str, axes: &[ResultsAxis], pins: &mut [Option<usize>], free: &[Option<usize>]) {
    if axes.iter().enumerate().all(|(index, _)| free.contains(&Some(index))) {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (index, (axis, pin)) in axes.iter().zip(pins.iter_mut()).enumerate() {
            if free.contains(&Some(index)) {
                continue;
            }
            let text = pin.map_or("Any", |level| axis.levels[level].as_str());
            labeled_combo(ui, &format!("{} at", axis.label), (id, index), text, |ui| {
                ui.selectable_value(pin, None, "Any");
                for (level, name) in axis.levels.iter().enumerate() {
                    ui.selectable_value(pin, Some(level), name.as_str());
                }
            });
        }
    });
}

impl ResponseView {
    /// Brings the settings within the axes and outputs of `store`.
    fn fit(&mut self, store: &ResultsStore) {
        let axes = store.axes();
        self.x_axis = self.x_axis.min(axes.len() - 1);
        self.output = self.output.min(store.reducer_columns().len() - 1);
        self.group_axis = self.group_axis.filter(|&axis| axis < axes.len() && axis != self.x_axis);
        self.pins.resize(axes.len(), None);
        for (pin, axis) in self.pins.iter_mut().zip(axes) {
            *pin = pin.filter(|&level| level < axis.levels.len());
        }
    }

    /// Returns the lines of `query`, computed again when the query changes, and at most once per
    /// [`REFRESH_INTERVAL`] while the store changes.
    ///
    /// `ctx` repaints once a pending refresh is due.
    ///
    /// [`REFRESH_INTERVAL`]: crate::ui::results::plot::REFRESH_INTERVAL
    fn lines(&mut self, ctx: &egui::Context, store: &ResultsStore, query: &ResponseQuery) -> &[ResponseLine] {
        let stale = self.cache.as_ref().is_some_and(|cache| {
            if cache.query != *query || cache.revision == store.revision() {
                return cache.query != *query;
            }
            refresh_due(ctx, cache.computed_at)
        });
        if stale {
            self.cache = None;
        }
        let cache = self.cache.get_or_insert_with(|| ResponseCache {
            revision: store.revision(),
            computed_at: Instant::now(),
            query: query.clone(),
            lines: store.response(query),
        });
        &cache.lines
    }
}
