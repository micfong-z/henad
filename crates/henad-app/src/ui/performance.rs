//! Live throughput and frame-cost readouts.

use crate::state::AppState;
use henad_core::helpers::fmt_bytes;

/// A label with dimmed leading zeros.
fn padded_num_label(ui: &mut egui::Ui, s: &str) {
    let (zeros, sig) = split_leading_zeros(s);

    let normal = ui.visuals().text_color();
    let faded = egui::Color32::from_gray(80);

    let font = egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    if !zeros.is_empty() {
        job.append(
            zeros,
            0.0,
            egui::TextFormat {
                color: faded,
                ..font.clone()
            },
        );
    }
    job.append(sig, 0.0, egui::TextFormat { color: normal, ..font });
    ui.label(job);
}

/// Splits padding from significant digits. An all-zero value keeps its last digit.
fn split_leading_zeros(s: &str) -> (&str, &str) {
    let first_sig = s.find(|c: char| c != '0').unwrap_or(s.len() - 1);
    s.split_at(first_sig)
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::WidgetText>) {
    ui.label(label);
    ui.label(value);
    ui.end_row();
}

fn padded_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(label);
    padded_num_label(ui, value);
    ui.end_row();
}

pub fn performance_ui(ui: &mut egui::Ui, app: &mut AppState) {
    crate::ui::kv_grid(ui, "performance_grid").show(ui, |ui| {
        match &app.snapshot {
            Some(snap) => {
                padded_row(ui, "Tick", &format!("{:09}", snap.tick));
                padded_row(ui, "TPS", &format!("{:09.0}", snap.actual_tps));
                row(ui, "Population", format!("{}", snap.population));
                row(ui, "Sim memory", fmt_bytes(snap.heap_bytes as u64));
            }
            None => row(ui, "Simulation", "Not loaded"),
        }

        let dt = ui.ctx().input(|i| i.stable_dt);
        if dt > 0.0 {
            padded_row(ui, "FPS", &format!("{:04.0}", 1.0 / dt));
        } else {
            row(ui, "FPS", "-");
        }

        // 3 integer digits, 1 point, and 1 decimal.
        let engine_ms = app.snapshot.as_ref().map_or(0.0, |s| s.engine_ms);
        padded_row(ui, "Engine", &format!("{engine_ms:05.1} ms"));

        padded_row(ui, "Render", &format!("{:05.1} ms", app.timings.render_ms));
        padded_row(ui, "UI", &format!("{:05.1} ms", app.timings.ui_ms));
    });
}

#[cfg(test)]
mod tests {
    use super::split_leading_zeros;

    #[test]
    fn pads_the_widths_each_counter_asks_for() {
        assert_eq!(split_leading_zeros(&format!("{:09}", 1042u64)), ("00000", "1042"));
        assert_eq!(split_leading_zeros(&format!("{:09.0}", 1798.4f64)), ("00000", "1798"));
        assert_eq!(split_leading_zeros(&format!("{:04.0}", 60.0f64)), ("00", "60"));
    }

    #[test]
    fn a_unit_suffix_rides_along_with_the_significant_digits() {
        assert_eq!(split_leading_zeros(&format!("{:05.1} ms", 0.4f64)), ("000", ".4 ms"));
        assert_eq!(split_leading_zeros(&format!("{:05.1} ms", 12.34f64)), ("0", "12.3 ms"));
        assert_eq!(split_leading_zeros(&format!("{:05.1} ms", 123.4f64)), ("", "123.4 ms"));
    }

    #[test]
    fn zero_keeps_one_significant_digit() {
        assert_eq!(split_leading_zeros(&format!("{:09}", 0u64)), ("00000000", "0"));
    }
}
