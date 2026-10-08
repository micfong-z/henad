//! Response view: an output against one varied parameter or action tick, with whiskers for its spread.

use std::ops::RangeInclusive;

use egui::{Color32, Id, Shape, Stroke};
use egui_plot::{
    Legend, Line, Plot, PlotBounds, PlotGeometry, PlotItem, PlotItemBase, PlotPoint, PlotTransform, Points,
    uniform_grid_spacer,
};
use henad_core::explore::summary::ReplicateSummary;
use web_time::Instant;

use crate::ui::results::plot::{config_color, labeled_combo, level_formatter, refresh_due};
use crate::ui::results::store::{ResponseLine, ResponseQuery, ResultsAxis, ResultsStore};
use crate::ui::show_plot;

/// Maximum number of levels an axis can have for its points to be joined into lines.
const MAX_JOINED_LEVELS: usize = 32;

/// Width of a whisker's stem and caps, in points.
const WHISKER_WIDTH: f32 = 1.0;

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

/// Lines of one response plot, with the query, the error bars and the store revision they were computed from.
#[derive(Debug)]
struct ResponseCache {
    revision: u64,
    computed_at: Instant,
    query: ResponseQuery,
    error_bars: ErrorBars,
    lines: Vec<PlottedLine>,
}

/// One line of a response plot in plot coordinates.
#[derive(Debug, Clone, PartialEq)]
struct PlottedLine {
    /// Name of the line in the legend.
    name: String,
    /// Id the line's points and whiskers share, so the legend hides them together.
    id: Id,
    /// Mean at each level of the x axis that has a mean.
    means: Vec<PlotPoint>,
    /// Ends of each whisker segment, the stem and then the low and high caps of each whisker in turn.
    whiskers: Vec<[PlotPoint; 2]>,
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
    let x_axis = &axes[view.x_axis];
    let query = ResponseQuery {
        x_axis: view.x_axis,
        output: view.output,
        group_axis: view.group_axis,
        pins: view.pins.clone(),
    };
    let error_bars = view.error_bars;
    let lines = view.lines(ui.ctx(), store, &query, error_bars);
    // Lines come in the order of their group's levels. A legend sorted by name lists level 10 before level 2.
    let mut plot = Plot::new("henad_results_response")
        .legend(Legend::default().follow_insertion_order(true))
        .x_axis_label(x_axis.label.as_str())
        .y_axis_label(store.output_label(query.output));
    if !x_axis.numeric {
        plot = plot
            .x_axis_formatter(level_formatter(&x_axis.levels))
            .x_grid_spacer(uniform_grid_spacer(|_| [1.0, 5.0, 10.0]));
    }
    let joined = x_axis.levels.len() <= MAX_JOINED_LEVELS;
    show_plot(ui, plot, |plot_ui| {
        for (position, line) in lines.iter().enumerate() {
            let color = config_color(position);
            if joined {
                plot_ui.line(
                    Line::new(line.name.as_str(), line.means.as_slice())
                        .color(color)
                        .width(1.5)
                        .id(line.id),
                );
            }
            plot_ui.points(
                Points::new(line.name.as_str(), line.means.as_slice())
                    .color(color)
                    .radius(3.5)
                    .id(line.id),
            );
            if !line.whiskers.is_empty() {
                plot_ui.add(Whiskers::new(&line.whiskers, color, line.id));
            }
        }
    });
}

/// Returns the lines of `query` over `store` in plot coordinates, with the whiskers that `error_bars` selects.
fn plotted_lines(store: &ResultsStore, query: &ResponseQuery, error_bars: ErrorBars) -> Vec<PlottedLine> {
    let axes = store.axes();
    let half_width = whisker_half_width(&axes[query.x_axis]);
    let group_axis = query.group_axis.and_then(|axis| axes.get(axis));
    store
        .response(query)
        .iter()
        .map(|line| {
            let name = match (group_axis, line.group_level) {
                (Some(axis), Some(level)) => format!("{} {}", axis.label, axis.levels[level]),
                _ => store.output_label(query.output),
            };
            plotted_line(line, name, error_bars, half_width)
        })
        .collect()
}

/// Returns `line`, named `name`, in plot coordinates, with the whiskers that `error_bars` selects and their caps
/// `half_width` either side of the stem.
fn plotted_line(line: &ResponseLine, name: String, error_bars: ErrorBars, half_width: f64) -> PlottedLine {
    let means = line
        .points
        .iter()
        .filter_map(|point| Some(PlotPoint::new(point.x, point.summary.mean?)))
        .collect();
    let whiskers = line
        .points
        .iter()
        .filter_map(|point| Some((point.x, whisker(point.summary, error_bars)?)))
        .flat_map(|(x, (low, high))| {
            let (left, right) = (x - half_width, x + half_width);
            [
                [PlotPoint::new(x, low), PlotPoint::new(x, high)],
                [PlotPoint::new(left, low), PlotPoint::new(right, low)],
                [PlotPoint::new(left, high), PlotPoint::new(right, high)],
            ]
        })
        .collect();
    PlottedLine {
        id: Id::new(("henad_results_response_line", name.as_str())),
        name,
        means,
        whiskers,
    }
}

/// Whiskers of one line as a single plot item, drawn from segments computed once per cache entry.
///
/// The whiskers take the id of their line and stay out of the legend.
struct Whiskers<'a> {
    base: PlotItemBase,
    id: Id,
    segments: &'a [[PlotPoint; 2]],
    color: Color32,
}

impl<'a> Whiskers<'a> {
    fn new(segments: &'a [[PlotPoint; 2]], color: Color32, id: Id) -> Self {
        Self {
            base: PlotItemBase::new(String::new()),
            id,
            segments,
            color,
        }
    }
}

impl PlotItem for Whiskers<'_> {
    fn shapes(&self, _ui: &egui::Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        // A highlighted item doubles its width, as a highlighted line does.
        let width = if self.highlighted() {
            2.0 * WHISKER_WIDTH
        } else {
            WHISKER_WIDTH
        };
        let stroke = Stroke::new(width, self.color);
        shapes.extend(self.segments.iter().map(|[start, end]| {
            let ends = [transform.position_from_point(start), transform.position_from_point(end)];
            Shape::line_segment(ends, stroke)
        }));
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        self.color
    }

    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let mut bounds = PlotBounds::NOTHING;
        for point in self.segments.iter().flatten() {
            bounds.extend_with(point);
        }
        bounds
    }

    fn base(&self) -> &PlotItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut PlotItemBase {
        &mut self.base
    }

    fn id(&self) -> Id {
        self.id
    }
}

/// Returns the low and high ends of the whisker that `error_bars` selects, or `None` when there is no whisker to draw.
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

/// Draws a combo for each axis outside `free` that holds it at one level or at any level, into `pins`.
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
    /// Clamps the settings to the axes and outputs of `store`.
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

    /// Returns the lines of `query` with the whiskers that `error_bars` selects, computed again when `query` or
    /// `error_bars` changes, and at most once per [`REFRESH_INTERVAL`] while the store changes.
    ///
    /// `ctx` repaints once a pending refresh is due.
    ///
    /// [`REFRESH_INTERVAL`]: crate::ui::results::plot::REFRESH_INTERVAL
    fn lines(
        &mut self,
        ctx: &egui::Context,
        store: &ResultsStore,
        query: &ResponseQuery,
        error_bars: ErrorBars,
    ) -> &[PlottedLine] {
        let stale = self.cache.as_ref().is_some_and(|cache| {
            let settings_changed = cache.query != *query || cache.error_bars != error_bars;
            if settings_changed || cache.revision == store.revision() {
                return settings_changed;
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
            error_bars,
            lines: plotted_lines(store, query, error_bars),
        });
        &cache.lines
    }
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Id, Rect, Shape, pos2};
    use egui_plot::{PlotItem as _, PlotPoint, PlotTransform};
    use henad_core::explore::summary::ReplicateSummary;

    use super::{ErrorBars, Whiskers, plotted_line};
    use crate::ui::results::store::{ResponseLine, ResponsePoint};

    /// Returns the point at `x` of level `level`, with a mean of `mean` and a confidence interval of `ci95`.
    fn point(x: f64, level: usize, mean: Option<f64>, ci95: Option<(f64, f64)>) -> ResponsePoint {
        ResponsePoint {
            x,
            level,
            summary: ReplicateSummary {
                n: 2,
                mean,
                standard_deviation: mean.map(|_| 1.0),
                ci95,
            },
        }
    }

    #[test]
    fn a_plotted_line_holds_three_segments_per_whisker() {
        let line = ResponseLine {
            group_level: None,
            points: vec![
                point(1.0, 0, Some(5.0), Some((4.0, 6.0))),
                point(2.0, 1, Some(7.0), None),
                point(3.0, 2, None, None),
            ],
        };
        let plotted = plotted_line(&line, "Infected, max".to_owned(), ErrorBars::ConfidenceInterval, 0.25);

        assert_eq!(
            plotted.means,
            [PlotPoint::new(1.0, 5.0), PlotPoint::new(2.0, 7.0)],
            "a level with no mean is left out"
        );
        assert_eq!(
            plotted.whiskers,
            [
                [PlotPoint::new(1.0, 4.0), PlotPoint::new(1.0, 6.0)],
                [PlotPoint::new(0.75, 4.0), PlotPoint::new(1.25, 4.0)],
                [PlotPoint::new(0.75, 6.0), PlotPoint::new(1.25, 6.0)],
            ],
            "a level with no interval draws no whisker"
        );
        let deviations = plotted_line(&line, "Infected, max".to_owned(), ErrorBars::StandardDeviation, 0.25);
        assert_eq!(deviations.whiskers.len(), 6, "both levels with a mean have a deviation");
        assert_eq!(deviations.id, plotted.id, "the id follows the name");
        let hidden = plotted_line(&line, "Infected, max".to_owned(), ErrorBars::Hidden, 0.25);
        assert!(hidden.whiskers.is_empty());
    }

    #[test]
    fn whiskers_draw_one_segment_each_within_their_bounds() {
        let segments = [
            [PlotPoint::new(1.0, 4.0), PlotPoint::new(1.0, 6.0)],
            [PlotPoint::new(0.75, 4.0), PlotPoint::new(1.25, 4.0)],
            [PlotPoint::new(0.75, 6.0), PlotPoint::new(1.25, 6.0)],
        ];
        let id = Id::new("line");
        let whiskers = Whiskers::new(&segments, Color32::RED, id);
        assert_eq!(whiskers.id(), id, "the whiskers hide with their line");
        assert!(!whiskers.allow_hover());
        let bounds = whiskers.bounds();
        assert_eq!((bounds.min(), bounds.max()), ([0.75, 4.0], [1.25, 6.0]));

        let transform = PlotTransform::new(Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)), bounds, false);
        let context = egui::Context::default();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let mut shapes = Vec::new();
            whiskers.shapes(ui, &transform, &mut shapes);
            assert_eq!(shapes.len(), segments.len());
            for (shape, [start, end]) in shapes.iter().zip(&segments) {
                let Shape::LineSegment { points, stroke } = shape else {
                    panic!("{shape:?} is not a line segment");
                };
                let expected = [transform.position_from_point(start), transform.position_from_point(end)];
                assert_eq!(*points, expected);
                assert_eq!(stroke.color, Color32::RED);
            }
        });
        output.drop_without_applying_deltas();
    }
}
