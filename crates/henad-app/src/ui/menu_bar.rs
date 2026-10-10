//! The menu bar above the docking area.

use crate::icons::material_design_icons::{
    MDI_BOOK_OPEN_VARIANT, MDI_CODE_BRACES, MDI_FILE_OUTLINE, MDI_FILE_PLUS_OUTLINE, MDI_FOLDER_OPEN_OUTLINE,
    MDI_GITHUB, MDI_INFORMATION_OUTLINE, MDI_OPEN_IN_NEW, MDI_RESTART, MDI_TRAY_ARROW_DOWN, MDI_VIEW_DASHBOARD_OUTLINE,
};
use crate::state::AppState;
use crate::ui::dock::{Tab, default_dock_state, toggle_tab};
use crate::ui::editor::{self, EMPTY_EDITOR};
use egui_dock::DockState;

pub fn menu_bar_panel(ui: &mut egui::Ui, dock: &mut DockState<Tab>, app: &mut AppState) {
    egui::Panel::top("menu_bar").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            file_menu(ui, app);
            view_menu(ui, dock);
            about_menu(ui, app);
        });
    });
}

/// Draws the File menu, whose entries fill the Editor or save its text. Each one that fills the Editor brings it to the
/// front.
fn file_menu(ui: &mut egui::Ui, app: &mut AppState) {
    ui.menu_button(format!("{MDI_FILE_OUTLINE}  File"), |ui| {
        if ui
            .button(format!("{MDI_FILE_PLUS_OUTLINE}  New grid rule"))
            .on_hover_text("Open Game of Life rule in Editor")
            .clicked()
        {
            editor::new_grid_rule(app);
            ui.close();
        }
        if ui
            .button(format!("{MDI_FOLDER_OPEN_OUTLINE}  Open model"))
            .on_hover_text("Open TOML model file in Editor")
            .clicked()
        {
            editor::open_model(app);
            ui.close();
        }
        let save = ui
            .add_enabled(
                !app.editor.text.is_empty(),
                egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN}  Save model")),
            )
            .on_hover_text("Save Editor text to TOML model file")
            .on_disabled_hover_text(EMPTY_EDITOR);
        if save.clicked() {
            editor::save_model(app);
            ui.close();
        }
    });
}

/// Draws the View menu. Its entries toggle each tab, and are the only way to reopen a closed tab.
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
        let product = &app.product;
        if let Some(url) = &product.source_url {
            link_button(ui, source_icon(url), "Source code", url);
        }
        if let Some(url) = &product.documentation_url {
            link_button(ui, MDI_BOOK_OPEN_VARIANT, "Documentation", url);
        }
        if product.source_url.is_some() || product.documentation_url.is_some() {
            ui.separator();
        }
        if ui
            .button(format!("{MDI_INFORMATION_OUTLINE}  About {}", product.name))
            .clicked()
        {
            app.about_open = true;
            ui.close();
        }
    });
}

/// Returns the icon of the Source code link to `url`: GitHub's mark for a link to GitHub, and braces otherwise.
fn source_icon(url: &str) -> &'static str {
    let address = url.split_once("://").map_or(url, |(_, address)| address);
    let host = address.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    let host = host.split(':').next().unwrap_or_default();
    let on_github = host.eq_ignore_ascii_case("github.com") || host.eq_ignore_ascii_case("www.github.com");
    if on_github { MDI_GITHUB } else { MDI_CODE_BRACES }
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

#[cfg(test)]
mod tests {
    use super::source_icon;
    use crate::icons::material_design_icons::{MDI_CODE_BRACES, MDI_GITHUB};

    /// The host decides. A path or a longer host containing `github.com` gets the braces.
    #[test]
    fn only_a_link_to_github_shows_its_mark() {
        assert_eq!(source_icon("https://github.com/micfong-z/henad"), MDI_GITHUB);
        assert_eq!(source_icon("https://www.GitHub.com/a/b"), MDI_GITHUB);
        assert_eq!(source_icon("https://gitlab.com/a/b"), MDI_CODE_BRACES);
        assert_eq!(source_icon("https://codeberg.org/a/github.com"), MDI_CODE_BRACES);
        assert_eq!(source_icon("https://github.com.example.org/a"), MDI_CODE_BRACES);
        assert_eq!(source_icon("https://git.example.ac.uk:8443/lab/model"), MDI_CODE_BRACES);
    }
}
