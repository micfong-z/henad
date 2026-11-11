//! Time series over the recorded `StatsHistory`, plus per-snapshot vector and histogram plots.

use egui_plot::PlotResponse;
use henad_core::view::{StatValue, StatsHistory};
use henad_graph::sheet::sentence::PLOT_EMPTY;

use crate::state::AppState;
use crate::ui::show_plot;
use crate::ui::time_axis::{RunTime, axis_label, plot_x};

pub fn charts_ui(ui: &mut egui::Ui, app: &mut AppState) {
    history_controls(ui, app);

    egui::ScrollArea::vertical().show(ui, |ui| {
        stats_chart(ui, app);
    });
}

/// Draws the Unlimited history checkbox and the History length slider.
///
/// Unlimited history keeps the whole run for an export, and grows with the run.
fn history_controls(ui: &mut egui::Ui, app: &mut AppState) {
    let mut unlimited = app.history_capacity.is_none();
    let mut changed = ui.checkbox(&mut unlimited, "Unlimited history").changed();

    changed |= ui
        .add_enabled(
            !unlimited,
            egui::Slider::new(&mut app.history_len, 100..=1_000_000)
                .logarithmic(true)
                .text("History length"),
        )
        .changed();

    if changed {
        app.history_capacity = (!unlimited).then_some(app.history_len);
        if let Some(history) = &mut app.stats_history {
            history.resize(app.history_capacity);
        }
    }
}

fn stats_chart(ui: &mut egui::Ui, app: &AppState) {
    // A sheet with no measure plots nothing, and the tab says how to plot one.
    if app.runs_editor_sheet()
        && app
            .editor
            .model
            .sheet()
            .is_some_and(|sheet| sheet.document().measures.is_empty())
    {
        ui.weak(PLOT_EMPTY);
        return;
    }
    let Some(history) = &app.stats_history else {
        return;
    };

    if history.is_empty() {
        return;
    }

    stats_plot(ui, history, app.loaded_time());

    // The latest snapshot's vector and histogram stats get a plot each.
    if let Some(snapshot) = &app.snapshot {
        for stat in &snapshot.stats {
            match stat.value() {
                StatValue::Vector2D { x, y } => {
                    vector_arrow_chart(ui, stat.label(), *x, *y, stat.color());
                }
                StatValue::Histogram { edges, counts } => {
                    histogram_chart(ui, stat.label(), edges, counts, stat.color());
                }
                StatValue::Scalar(_) => {}
            }
        }
    }
}

/// Radius in points of the dot that a series of one sample draws.
const SINGLE_SAMPLE_RADIUS: f32 = 3.0;

/// Draws every stat of `history` over time, the x axis in ticks or, for a run with a time scale, in its unit.
///
/// The y range holds 0. A series of one sample, as at tick 0, draws as a dot in its colour, since a line through one
/// point draws nothing, and the x range then spans the tick of the sample, as [`single_sample_x_range`] returns it.
fn stats_plot(ui: &mut egui::Ui, history: &StatsHistory, time: Option<RunTime>) -> PlotResponse<()> {
    let mut plot = egui_plot::Plot::new("stats_plot")
        .height(200.0)
        .show_axes(true)
        .x_axis_label(axis_label(time.map(|time| time.unit)))
        .include_y(0.0)
        // In the order the model lists its stats, which for a sheet is the order of its measures.
        .legend(egui_plot::Legend::default().follow_insertion_order(true));
    if let Some([start, end]) = single_sample_x_range(history, time) {
        plot = plot.include_x(start).include_x(end);
    }
    show_plot(ui, plot, |plot_ui| {
        for (column, descriptor) in history.descriptors().iter().enumerate() {
            let [r, g, b, _] = descriptor.color();
            let color = egui::Color32::from_rgb(r, g, b);
            let points = stat_points(history, column, time);
            if points.len() == 1 {
                plot_ui.points(
                    egui_plot::Points::new(descriptor.label(), points)
                        .color(color)
                        .radius(SINGLE_SAMPLE_RADIUS),
                );
            } else {
                plot_ui.line(egui_plot::Line::new(descriptor.label(), points).color(color).width(1.5));
            }
        }
    })
}

/// Returns the x range of a plot of `history` while it holds one sample: from the sample's tick to the next tick, each
/// at [`plot_x`]. Returns `None` for any other number of samples.
///
/// Note that `egui_plot` widens an x range of zero width to the height of the y range. A count of 65536 cells would
/// then stretch the x axis of one sample at tick 0 from about -34000 to 34000.
fn single_sample_x_range(history: &StatsHistory, time: Option<RunTime>) -> Option<[f64; 2]> {
    if history.len() != 1 {
        return None;
    }
    let tick = history.tick(0)?;
    Some([plot_x(tick, time), plot_x(tick + 1, time)])
}

/// Returns the points of stat column `column` of `history`, each sample at [`plot_x`] of its tick.
fn stat_points(history: &StatsHistory, column: usize, time: Option<RunTime>) -> Vec<[f64; 2]> {
    (0..history.len())
        .filter_map(|position| {
            let (value, tick) = history.get(column, position)?;
            Some([plot_x(tick, time), value])
        })
        .collect()
}

fn vector_arrow_chart(ui: &mut egui::Ui, label: &str, x: f64, y: f64, color: [u8; 4]) {
    let [r, g, b, _] = color;
    let arrow_color = egui::Color32::from_rgb(r, g, b);
    let magnitude = x.hypot(y);

    ui.separator();
    ui.label(format!("{label}: ({x:.2}, {y:.2})  |{magnitude:.2}|"));

    let plot = egui_plot::Plot::new(format!("vec_{label}"))
        .height(ui.available_width())
        .data_aspect(1.0)
        .invert_y(true)
        .show_axes(true)
        .show_grid(true)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false);
    show_plot(ui, plot, |plot_ui| {
        // Arrow from origin to (x, y). The plot inverts the y-axis.
        plot_ui.arrows(
            egui_plot::Arrows::new(label, vec![[0.0, 0.0]], vec![[x, y]])
                .color(arrow_color)
                .tip_length(8.0),
        );
        // A reference circle sits at the current magnitude.
        if magnitude > 1e-6 {
            let segments = 64;
            let circle: Vec<[f64; 2]> = (0..=segments)
                .map(|segment| {
                    let angle = std::f64::consts::TAU * segment as f64 / segments as f64;
                    [magnitude * angle.cos(), magnitude * angle.sin()]
                })
                .collect();
            plot_ui.line(
                egui_plot::Line::new(format!("{label} Magnitude"), circle)
                    .color(egui::Color32::from_gray(60))
                    .width(0.5),
            );
        }
    });
}

fn histogram_chart(ui: &mut egui::Ui, label: &str, edges: &[f64], counts: &[u64], color: [u8; 4]) {
    if edges.len() < 2 || counts.is_empty() {
        return;
    }
    let [r, g, b, _] = color;
    let bar_color = egui::Color32::from_rgb(r, g, b);

    ui.separator();
    ui.label(label);

    let plot = egui_plot::Plot::new(format!("hist_{label}"))
        .height(120.0)
        .show_axes(true);
    show_plot(ui, plot, |plot_ui| {
        let bars: Vec<egui_plot::Bar> = edges
            .windows(2)
            .zip(counts.iter())
            .map(|(edge_pair, &count)| {
                let center = edge_pair[0].midpoint(edge_pair[1]);
                let width = edge_pair[1] - edge_pair[0];
                egui_plot::Bar::new(center, count as f64).width(width)
            })
            .collect();

        plot_ui.bar_chart(egui_plot::BarChart::new(label, bars).color(bar_color));
    });
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, Shape, vec2};
    use henad_core::metadata::TimeUnit;
    use henad_core::view::{StatDescriptor, StatEntry, StatValue, StatsHistory};

    use super::{single_sample_x_range, stat_points, stats_plot};
    use crate::ui::time_axis::RunTime;

    const HALF_DAY: RunTime = RunTime {
        unit: TimeUnit::Day,
        tick_length: 0.5,
    };

    const DAY: RunTime = RunTime {
        unit: TimeUnit::Day,
        tick_length: 1.0,
    };

    /// Returns a history of one stat, Infected, holding `samples`, each a tick and its value.
    fn history(samples: &[(u64, f64)]) -> StatsHistory {
        let color = [255, 0, 0, 255];
        let mut history = StatsHistory::new(vec![StatDescriptor::new("Infected", color)], None);
        for &(tick, value) in samples {
            history.push_entries(&[StatEntry::new("Infected", StatValue::Scalar(value), color)], tick);
        }
        history
    }

    /// Plot drawn for one frame: its texts, the number of circles it painted, the lowest y it shows, and the range of x
    /// it shows.
    struct DrawnPlot {
        texts: Vec<String>,
        circles: usize,
        lowest_y: f64,
        x_range: [f64; 2],
    }

    /// Adds the text of every text shape in `shape` to `texts` and counts its circles into `circles`.
    fn collect(shape: &Shape, texts: &mut Vec<String>, circles: &mut usize) {
        match shape {
            Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            Shape::Circle(_) => *circles += 1,
            Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, texts, circles);
                }
            }
            _ => {}
        }
    }

    /// Draws the stats plot of `history` for one frame.
    fn draw(history: &StatsHistory, time: Option<RunTime>) -> DrawnPlot {
        let context = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            ..egui::RawInput::default()
        };
        let mut lowest_y = f64::NAN;
        let mut x_range = [f64::NAN; 2];
        let output = context.run_ui(input, |ui| {
            let bounds = *stats_plot(ui, history, time).transform.bounds();
            lowest_y = bounds.min()[1];
            x_range = [bounds.min()[0], bounds.max()[0]];
        });
        let (mut texts, mut circles) = (Vec::new(), 0);
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut texts, &mut circles);
        }
        output.drop_without_applying_deltas();
        DrawnPlot {
            texts,
            circles,
            lowest_y,
            x_range,
        }
    }

    #[test]
    fn a_timed_run_plots_each_sample_at_its_tick_times_the_tick_length() {
        let history = history(&[(0, 120.0), (10, 150.0), (20, 180.0)]);
        assert_eq!(
            stat_points(&history, 0, None),
            [[0.0, 120.0], [10.0, 150.0], [20.0, 180.0]]
        );
        assert_eq!(
            stat_points(&history, 0, Some(HALF_DAY)),
            [[0.0, 120.0], [5.0, 150.0], [10.0, 180.0]]
        );
    }

    #[test]
    fn the_axis_reads_in_ticks_or_the_unit_and_the_range_holds_zero() {
        // The samples start after tick 0. The only "0" left is then the label on the left axis.
        let history = history(&[(10, 120.0), (20, 150.0), (30, 180.0)]);
        let untimed = draw(&history, None);
        assert!(untimed.texts.iter().any(|text| text == "Tick"), "{:?}", untimed.texts);
        assert!(!untimed.texts.iter().any(|text| text == "Day"), "{:?}", untimed.texts);
        assert!(untimed.lowest_y <= 0.0, "the range starts at {}", untimed.lowest_y);
        assert!(
            untimed.texts.iter().any(|text| text == "0"),
            "the left axis labels 0: {:?}",
            untimed.texts
        );

        let timed = draw(&history, Some(HALF_DAY));
        assert!(timed.texts.iter().any(|text| text == "Day"), "{:?}", timed.texts);
        assert!(!timed.texts.iter().any(|text| text == "Tick"), "{:?}", timed.texts);
        assert!(timed.lowest_y <= 0.0, "the range starts at {}", timed.lowest_y);
        assert!(
            timed.texts.iter().any(|text| text == "0"),
            "the left axis labels 0: {:?}",
            timed.texts
        );
    }

    #[test]
    fn a_series_of_one_sample_draws_as_a_dot() {
        let one = draw(&history(&[(0, 94.0)]), None).circles;
        let two = draw(&history(&[(0, 94.0), (1, 96.0)]), None).circles;
        // The legend paints the same circles for both, so the dot is the one more.
        assert_eq!(one, two + 1, "{one} circles with one sample, {two} with two");
    }

    /// One sample at tick 0 shows the tick it sits on, whatever the size of its values, with a time scale or without.
    /// Left to `egui_plot`, a range of zero width takes the height of the y range, and a count of 65536 cells stretched
    /// it to tens of thousands.
    #[test]
    fn one_sample_shows_the_tick_it_sits_on() {
        let one = history(&[(0, 65_536.0)]);
        assert_eq!(single_sample_x_range(&one, None), Some([0.0, 1.0]));
        assert_eq!(single_sample_x_range(&one, Some(DAY)), Some([0.0, 1.0]));
        assert_eq!(single_sample_x_range(&one, Some(HALF_DAY)), Some([0.0, 0.5]));
        for time in [None, Some(DAY)] {
            let [start, end] = draw(&one, time).x_range;
            assert!(
                (-0.1..0.0).contains(&start) && (1.0..1.1).contains(&end),
                "x from {start} to {end} with {time:?}"
            );
        }

        let two = history(&[(0, 65_536.0), (1, 65_000.0)]);
        assert_eq!(
            single_sample_x_range(&two, None),
            None,
            "two samples set their own range"
        );
    }
}
