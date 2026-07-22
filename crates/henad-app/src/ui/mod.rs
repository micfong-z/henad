pub mod charts;
pub mod dock;
pub mod menu_bar;
pub mod model;
pub mod pacing;
pub mod params;
pub mod performance;
pub mod playback;
pub mod stats;
pub mod system;
pub mod viewport;

pub fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KB", bytes as f64 / (1u64 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
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
