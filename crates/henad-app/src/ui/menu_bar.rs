//! The menu bar above the docking area.

use crate::icons::material_design_icons::{
    MDI_BOOK_OPEN_VARIANT, MDI_CODE_BRACES, MDI_FILE_DOCUMENT_PLUS_OUTLINE, MDI_FILE_OUTLINE, MDI_FILE_PLUS_OUTLINE,
    MDI_FOLDER_OPEN_OUTLINE, MDI_GITHUB, MDI_GRAPH_OUTLINE, MDI_INFORMATION_OUTLINE, MDI_OPEN_IN_NEW, MDI_RESTART,
    MDI_TRAY_ARROW_DOWN, MDI_VIEW_DASHBOARD_OUTLINE,
};
use crate::state::AppState;
use crate::ui::dock::{Layout, Tab, Workspace};
use crate::ui::editor::{self, EMPTY_EDITOR, NEW_MODEL_BUTTON, NEW_NODE_GRAPH};

/// Label of the View menu's checkbox that switches to the Editor layout when a model opens in the Editor.
const EDITOR_LAYOUT_ON_OPEN: &str = "Open models in the Editor layout";

pub fn menu_bar_panel(ui: &mut egui::Ui, workspace: &mut Workspace, app: &mut AppState) {
    egui::Panel::top("menu_bar").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            file_menu(ui, app);
            view_menu(ui, workspace);
            about_menu(ui, app);
        });
    });
}

/// Draws the File menu, whose entries fill the Editor or save its text. Each one that fills the Editor brings it to the
/// front. New model opens its dialog first.
pub(crate) fn file_menu(ui: &mut egui::Ui, app: &mut AppState) {
    ui.menu_button(format!("{MDI_FILE_OUTLINE}  File"), |ui| {
        if ui
            .button(format!("{MDI_FILE_DOCUMENT_PLUS_OUTLINE}  {NEW_MODEL_BUTTON}"))
            .on_hover_text("Start new model in Editor")
            .clicked()
        {
            editor::sheet::new_model::open_new_model(app);
            ui.close();
        }
        if ui
            .button(format!("{MDI_FILE_PLUS_OUTLINE}  New grid rule"))
            .on_hover_text("Open Game of Life rule in Editor")
            .clicked()
        {
            editor::new_grid_rule(app);
            ui.close();
        }
        ui.menu_button(format!("{MDI_GRAPH_OUTLINE}  {NEW_NODE_GRAPH}"), |ui| {
            if let Some(file_name) = editor::starter_menu(ui) {
                editor::new_graph_model(app, file_name);
                ui.close();
            }
        })
        .response
        .on_hover_text("Start node graph from an example in Editor");
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
                !app.editor.model.is_empty(),
                egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN}  Save model")),
            )
            .on_hover_text("Save Editor model to TOML model file")
            .on_disabled_hover_text(EMPTY_EDITOR);
        if save.clicked() {
            editor::save_model(app);
            ui.close();
        }
    });
}

/// Draws the View menu. Its first entries toggle each tab in the layout that shows, and are the only way to reopen a
/// closed tab. The layouts follow, then the checkbox that switches to the Editor layout as a model opens. Reset layout
/// comes last and resets the layout that shows.
fn view_menu(ui: &mut egui::Ui, workspace: &mut Workspace) {
    ui.menu_button(format!("{MDI_VIEW_DASHBOARD_OUTLINE}  View"), |ui| {
        for tab in Tab::ALL {
            let open = workspace.dock().find_tab(&tab).is_some();
            if ui
                .selectable_label(open, format!("{}  {}", tab.icon(), tab.title()))
                .clicked()
            {
                workspace.toggle(tab);
            }
        }
        ui.separator();
        for layout in Layout::ALL {
            if ui.radio(workspace.layout() == layout, layout.title()).clicked() {
                workspace.show(layout);
            }
        }
        ui.checkbox(&mut workspace.editor_layout_on_open, EDITOR_LAYOUT_ON_OPEN);
        ui.separator();
        if ui.button(format!("{MDI_RESTART}  Reset layout")).clicked() {
            workspace.reset();
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
    use egui::accesskit::{Role, Toggled};

    use super::{EDITOR_LAYOUT_ON_OPEN, source_icon, view_menu};
    use crate::icons::material_design_icons::{MDI_CODE_BRACES, MDI_GITHUB};
    use crate::ui::dock::{Layout, Tab, Workspace};

    /// One accessible node of a frame of the View menu.
    #[derive(Debug)]
    struct MenuNode {
        label: String,
        role: Role,
        toggled: Option<Toggled>,
        bounds: egui::Rect,
    }

    /// A context that draws the View menu over a workspace, driven by synthetic input.
    struct MenuHarness {
        context: egui::Context,
        workspace: Workspace,
        time: f64,
    }

    impl MenuHarness {
        fn new() -> Self {
            let context = egui::Context::default();
            context.enable_accesskit();
            Self {
                context,
                workspace: Workspace::default(),
                time: 0.0,
            }
        }

        /// Runs one frame with `events`, and returns its accessible nodes that have bounds.
        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<MenuNode> {
            self.time += 0.016;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 800.0))),
                time: Some(self.time),
                events,
                ..egui::RawInput::default()
            };
            let workspace = &mut self.workspace;
            let mut output = self.context.run_ui(input, |ui| {
                egui::MenuBar::new().ui(ui, |ui| view_menu(ui, workspace));
            });
            output.textures_delta.clear();
            let update = output.platform_output.accesskit_update.expect("an AccessKit update");
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let rect = node.bounds()?;
                    Some(MenuNode {
                        label: node.label().unwrap_or_default().to_owned(),
                        role: node.role(),
                        toggled: node.toggled(),
                        bounds: egui::Rect::from_min_max(
                            egui::pos2(rect.x0 as f32, rect.y0 as f32),
                            egui::pos2(rect.x1 as f32, rect.y1 as f32),
                        ),
                    })
                })
                .collect()
        }

        /// Opens the View menu, and returns the nodes of the frame that shows it.
        fn open(&mut self) -> Vec<MenuNode> {
            self.click("View");
            self.frame(Vec::new())
        }

        /// Clicks the one node labelled `label`, after an icon or on its own.
        fn click(&mut self, label: &str) {
            let nodes = self.frame(Vec::new());
            let after_icon = format!("  {label}");
            let found: Vec<&MenuNode> = nodes
                .iter()
                .filter(|node| node.label == label || node.label.ends_with(&after_icon))
                .collect();
            assert_eq!(found.len(), 1, "one node reads {label}: {found:?}");
            let position = found[0].bounds.center();
            let button = |pressed| egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![egui::Event::PointerMoved(position)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }
    }

    /// Returns whether `nodes` hold a node of `role` named `label` that is `toggled`.
    fn shows(nodes: &[MenuNode], role: Role, label: &str, toggled: Toggled) -> bool {
        nodes
            .iter()
            .any(|node| node.role == role && node.label == label && node.toggled == Some(toggled))
    }

    #[test]
    fn the_view_menu_switches_layouts_by_name() {
        let mut harness = MenuHarness::new();
        let nodes = harness.open();
        assert!(shows(&nodes, Role::RadioButton, "Default layout", Toggled::True));
        assert!(shows(&nodes, Role::RadioButton, "Editor layout", Toggled::False));
        assert!(shows(&nodes, Role::CheckBox, EDITOR_LAYOUT_ON_OPEN, Toggled::True));

        harness.click("Editor layout");
        assert_eq!(harness.workspace.layout(), Layout::Editor);
        let nodes = harness.open();
        assert!(shows(&nodes, Role::RadioButton, "Editor layout", Toggled::True));
        assert!(
            nodes
                .iter()
                .any(|node| node.label.ends_with(Tab::Editor.title()) && node.toggled == Some(Toggled::True)),
            "the menu does not list the Editor as open in the Editor layout"
        );

        harness.click(EDITOR_LAYOUT_ON_OPEN);
        assert!(!harness.workspace.editor_layout_on_open, "the checkbox stayed on");
        harness.open();
        harness.click("Default layout");
        assert_eq!(harness.workspace.layout(), Layout::Default);
    }

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
