//! The menu bar above the docking area.

use crate::icons::material_design_icons::{
    MDI_BOOK_OPEN_VARIANT, MDI_GITHUB, MDI_INFORMATION_OUTLINE, MDI_OPEN_IN_NEW, MDI_RESTART,
    MDI_VIEW_DASHBOARD_OUTLINE,
};
use crate::state::AppState;
use crate::ui::about::{DOCS_URL, SOURCE_URL};
use crate::ui::dock::{Tab, default_dock_state, toggle_tab};
use egui_dock::DockState;

pub fn menu_bar_panel(ui: &mut egui::Ui, dock: &mut DockState<Tab>, app: &mut AppState) {
    egui::Panel::top("menu_bar").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            view_menu(ui, dock);
            about_menu(ui, app);
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

fn about_menu(ui: &mut egui::Ui, app: &mut AppState) {
    ui.menu_button(format!("{MDI_INFORMATION_OUTLINE}  About"), |ui| {
        link_button(ui, MDI_GITHUB, "Source code", SOURCE_URL);
        link_button(ui, MDI_BOOK_OPEN_VARIANT, "Documentation", DOCS_URL);
        ui.separator();
        if ui.button(format!("{MDI_INFORMATION_OUTLINE}  About Henad")).clicked() {
            app.about_open = true;
            ui.close();
        }
    });
}

fn link_button(ui: &mut egui::Ui, icon: &str, label: &str, url: &str) {
    if ui
        .button(format!("{icon}  {label}  {MDI_OPEN_IN_NEW}"))
        .on_hover_text(url)
        .clicked()
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        ui.close();
    }
}
