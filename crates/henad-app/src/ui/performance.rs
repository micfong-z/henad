//! Live throughput and frame-cost readouts.

use crate::state::AppState;
use crate::ui::KvGridRows;
use henad_core::helpers::fmt_bytes;

/// Adds a label of `text` with its leading zeros dimmed.
fn padded_num_label(ui: &mut egui::Ui, text: &str) {
    let (zeros, significant) = split_leading_zeros(text);

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
    job.append(significant, 0.0, egui::TextFormat { color: normal, ..font });
    ui.label(job);
}

/// Splits the leading zeros from the significant digits. An all-zero value keeps its last digit.
fn split_leading_zeros(text: &str) -> (&str, &str) {
    let first_significant = text.find(|c: char| c != '0').unwrap_or(text.len() - 1);
    text.split_at(first_significant)
}

fn padded_row(ui: &mut egui::Ui, rows: &mut KvGridRows, label: &str, value: &str) {
    ui.label(label);
    padded_num_label(ui, value);
    rows.end_row(ui);
}

pub fn performance_ui(ui: &mut egui::Ui, app: &AppState) {
    if let Some(note) = &app.thread_pool_note {
        crate::ui::banner(
            ui,
            crate::icons::material_design_icons::MDI_ALERT,
            ui.visuals().warn_fg_color,
            "Thread pool failed to start",
            note,
        );
    }
    crate::ui::kv_grid(ui, "performance_grid").show(ui, |ui, rows| {
        match &app.snapshot {
            Some(snapshot) => {
                padded_row(ui, rows, "Tick", &format!("{:09}", snapshot.tick));
                padded_row(ui, rows, "TPS", &format!("{:09.0}", snapshot.actual_tps));
                rows.row(ui, "Population", format!("{}", snapshot.population));
                rows.row(ui, "Sim memory", fmt_bytes(snapshot.heap_bytes as u64));
            }
            None => rows.row(ui, "Simulation", "Not loaded"),
        }

        let dt = ui.ctx().input(|i| i.stable_dt);
        if dt > 0.0 {
            padded_row(ui, rows, "FPS", &format!("{:04.0}", 1.0 / dt));
        } else {
            rows.row(ui, "FPS", "-");
        }

        // Each time is padded to three integer digits and one decimal.
        let engine_ms = app.snapshot.as_ref().map_or(0.0, |snapshot| snapshot.engine_ms);
        padded_row(ui, rows, "Engine", &format!("{engine_ms:05.1} ms"));
        // Shows the last snapshot's own cost.
        let view_ms = app.snapshot.as_ref().map_or(0.0, |snapshot| snapshot.view_ms);
        padded_row(ui, rows, "Prepare view", &format!("{view_ms:05.1} ms"));

        padded_row(ui, rows, "Render", &format!("{:05.1} ms", app.timings.render_ms));
        padded_row(ui, rows, "UI", &format!("{:05.1} ms", app.timings.ui_ms));
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
