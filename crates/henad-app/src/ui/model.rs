//! Model selection.

use crate::state::AppState;

pub fn model_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let model_names: Vec<&str> = app.registry.iter().map(|m| m.name.as_str()).collect();
    let mut changed_model = false;

    egui::ComboBox::from_label("Select Model")
        .selected_text(model_names.get(app.selected_model).copied().unwrap_or("None"))
        .show_ui(ui, |ui| {
            for (i, name) in model_names.iter().enumerate() {
                if ui.selectable_value(&mut app.selected_model, i, *name).changed() {
                    changed_model = true;
                }
            }
        });

    if changed_model {
        app.load_default_params();
    }

    if let Some(entry) = app.registry.get(app.selected_model) {
        ui.separator();
        ui.label(entry.description.as_str());
    }
}
