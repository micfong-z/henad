//! Time series over the recorded `StatsHistory`, plus per-snapshot vector and histogram plots.

use crate::state::AppState;
use crate::ui::show_plot;
use henad_core::view::StatValue;

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
    let Some(history) = &app.stats_history else {
        return;
    };

    if history.is_empty() {
        return;
    }

    let plot = egui_plot::Plot::new("stats_plot")
        .height(200.0)
        .show_axes(true)
        .legend(egui_plot::Legend::default());
    show_plot(ui, plot, |plot_ui| {
        let filled = history.len();

        for (col, desc) in history.descriptors().iter().enumerate() {
            let [r, g, b, _] = desc.color;
            let color = egui::Color32::from_rgb(r, g, b);

            let points: Vec<[f64; 2]> = (0..filled)
                .filter_map(|j| {
                    let (value, tick) = history.get(col, j)?;
                    Some([(tick) as f64, value])
                })
                .collect();

            plot_ui.line(egui_plot::Line::new(desc.label, points).color(color).width(1.5));
        }
    });

    // The latest snapshot's vector and histogram stats get a plot each.
    if let Some(snap) = &app.snapshot {
        for stat in &snap.stats {
            match &stat.value {
                StatValue::Vector2D { x, y } => {
                    vector_arrow_chart(ui, stat.label, *x, *y, stat.color);
                }
                StatValue::Histogram { edges, counts } => {
                    histogram_chart(ui, stat.label, edges, counts, stat.color);
                }
                StatValue::Scalar(_) => {}
            }
        }
    }
}

fn vector_arrow_chart(ui: &mut egui::Ui, label: &str, x: f64, y: f64, color: [u8; 4]) {
    let [r, g, b, _] = color;
    let arrow_color = egui::Color32::from_rgb(r, g, b);
    let mag = x.hypot(y);

    ui.separator();
    ui.label(format!("{label}: ({x:.2}, {y:.2})  |{mag:.2}|"));

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
        if mag > 1e-6 {
            let n = 64;
            let circle: Vec<[f64; 2]> = (0..=n)
                .map(|i| {
                    let angle = std::f64::consts::TAU * i as f64 / n as f64;
                    [mag * angle.cos(), mag * angle.sin()]
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
