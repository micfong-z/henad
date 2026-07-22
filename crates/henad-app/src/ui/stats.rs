//! Latest stat values as a table, see [`charts`](crate::ui::charts) for time series.

use crate::icons::material_design_icons::{MDI_ARROW_TOP_RIGHT_THIN, MDI_CHART_HISTOGRAM, MDI_CIRCLE_SMALL};
use crate::state::AppState;
use henad_core::view::StatValue;

pub fn stats_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let Some(snap) = &app.snapshot else {
        ui.label("No simulation loaded.");
        return;
    };

    crate::ui::kv_grid(ui, "stats_grid").show(ui, |ui| {
        for stat in &snap.stats {
            let [r, g, b, _] = stat.color;
            let color = egui::Color32::from_rgb(r, g, b);
            ui.colored_label(color, stat.label);
            ui.horizontal(|ui| {
                let (text, data_type) = match &stat.value {
                    StatValue::Scalar(v) => (format!("{v:.0}"), "Scalar"),
                    StatValue::Vector2D { x, y } => {
                        let mag = x.hypot(*y);
                        (format!("({x:.1}, {y:.1}) |{mag:.1}|"), "Vector2D")
                    }
                    StatValue::Histogram { counts, .. } => {
                        let total: u64 = counts.iter().sum();
                        (format!("n={total}"), "Histogram")
                    }
                };

                let icon = match &stat.value {
                    StatValue::Scalar(_) => MDI_CIRCLE_SMALL,
                    StatValue::Vector2D { .. } => MDI_ARROW_TOP_RIGHT_THIN,
                    StatValue::Histogram { .. } => MDI_CHART_HISTOGRAM,
                };
                ui.colored_label(egui::Color32::GRAY, icon).on_hover_text(data_type);
                ui.colored_label(color, text);
            });
            ui.end_row();
        }
    });
}
