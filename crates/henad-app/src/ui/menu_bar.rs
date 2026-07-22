//! The menu bar above the docking area.

use crate::icons::material_design_icons::{MDI_RESTART, MDI_VIEW_DASHBOARD_OUTLINE};
use crate::ui::dock::{Tab, default_dock_state, toggle_tab};
use egui_dock::DockState;

pub fn menu_bar_panel(ctx: &egui::Context, dock: &mut DockState<Tab>) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            view_menu(ui, dock);
        });
    });
}

/// Tab visibility, and the only way to reopen a closed tab.
fn view_menu(ui: &mut egui::Ui, dock: &mut DockState<Tab>) {
    ui.menu_button(format!("{MDI_VIEW_DASHBOARD_OUTLINE}  View"), |ui| {
        for tab in Tab::ALL {
            let open = dock.find_tab(&tab).is_some();
            if ui
                .selectable_label(open, format!("{}  {}", tab.icon(), tab.title()))
                .clicked()
            {
                toggle_tab(dock, tab);
            }
        }
        ui.separator();
        if ui.button(format!("{MDI_RESTART}  Reset layout")).clicked() {
            *dock = default_dock_state();
        }
    });
}
