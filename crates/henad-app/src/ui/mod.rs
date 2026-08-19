pub mod agent_layer;
pub mod charts;
pub mod dock;
pub mod fault;
pub mod menu_bar;
pub mod model;
pub mod pacing;
pub mod params;
pub mod performance;
pub mod playback;
pub mod stats;
pub mod system;
pub mod viewport;

pub fn banner(ui: &mut egui::Ui, icon: &str, color: egui::Color32, title: &str, detail: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, icon);
        ui.colored_label(color, title);
    });
    ui.label(detail);
    ui.separator();
}

const KV_GRID_SPACING: [f32; 2] = [16.0, 4.0];

/// Two-column key/value grid, full panel width.
///
/// `Grid` sizes to its content, so without pinning the columns the striping stops short.
pub fn kv_grid(ui: &egui::Ui, id: &str) -> egui::Grid {
    let col_width = ((ui.available_width() - KV_GRID_SPACING[0]) / 2.0).max(0.0);
    egui::Grid::new(id)
        .num_columns(2)
        .striped(true)
        .spacing(KV_GRID_SPACING)
        .min_col_width(col_width)
}
