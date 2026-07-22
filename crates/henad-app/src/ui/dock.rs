//! Tab definitions, the default layout, and dispatch to each panel module.

use egui_dock::{DockState, NodeIndex, TabViewer};

use crate::icons::material_design_icons::{
    MDI_CHART_LINE, MDI_CHIP, MDI_COG_OUTLINE, MDI_CUBE_OUTLINE, MDI_GAUGE, MDI_PLAY_CIRCLE_OUTLINE, MDI_SPEEDOMETER,
    MDI_TABLE, MDI_TUNE,
};
use crate::state::AppState;
use crate::ui;

/// A dockable panel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tab {
    Viewport,
    Playback,
    Pacing,
    Model,
    Params,
    Stats,
    Charts,
    Performance,
    System,
}

impl Tab {
    /// Every tab that exists, in View-menu order.
    pub const ALL: [Self; 9] = [
        Self::Viewport,
        Self::Playback,
        Self::Pacing,
        Self::Model,
        Self::Params,
        Self::Stats,
        Self::Charts,
        Self::Performance,
        Self::System,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Viewport => "Viewport",
            Self::Playback => "Playback",
            Self::Pacing => "Pacing",
            Self::Model => "Model",
            Self::Params => "Parameters",
            Self::Stats => "Statistics",
            Self::Charts => "Charts",
            Self::Performance => "Performance",
            Self::System => "System",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Viewport => MDI_CUBE_OUTLINE,
            Self::Playback => MDI_PLAY_CIRCLE_OUTLINE,
            Self::Pacing => MDI_SPEEDOMETER,
            Self::Model => MDI_COG_OUTLINE,
            Self::Params => MDI_TUNE,
            Self::Stats => MDI_TABLE,
            Self::Charts => MDI_CHART_LINE,
            Self::Performance => MDI_GAUGE,
            Self::System => MDI_CHIP,
        }
    }
}

/// Default layout, and what "Reset layout" restores.
pub fn default_dock_state() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let surface = dock.main_surface_mut();

    let [viewport, model] = surface.split_left(NodeIndex::root(), 0.2, vec![Tab::Model]);
    let [_, params] = surface.split_below(model, 0.2, vec![Tab::Params]);
    let [_, playback] = surface.split_below(params, 0.5, vec![Tab::Playback]);
    surface.split_below(playback, 0.35, vec![Tab::Pacing]);

    let [_, perf] = surface.split_right(viewport, 0.7, vec![Tab::Performance, Tab::System]);
    let [_, stats] = surface.split_below(perf, 0.25, vec![Tab::Stats]);
    surface.split_below(stats, 0.4, vec![Tab::Charts]);

    dock
}

impl TabViewer for AppState {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{}  {}", tab.icon(), tab.title()).into()
    }

    // Keyed on the variant so renaming a tab doesn't reset stored layouts.
    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Viewport => ui::viewport::viewport_ui(ui, self),
            Tab::Playback => ui::playback::playback_ui(ui, self),
            Tab::Pacing => ui::pacing::pacing_ui(ui, self),
            Tab::Model => ui::model::model_ui(ui, self),
            Tab::Params => ui::params::params_ui(ui, self),
            Tab::Stats => ui::stats::stats_ui(ui, self),
            Tab::Charts => ui::charts::charts_ui(ui, self),
            Tab::Performance => ui::performance::performance_ui(ui, self),
            Tab::System => ui::system::system_ui(ui, self),
        }
    }

    /// The viewport's paint callback covers its own rect, so the fill under it is wasted.
    fn clear_background(&self, tab: &Self::Tab) -> bool {
        *tab != Tab::Viewport
    }
}

/// Adds `tab` if it is closed, removes it if it is open.
pub fn toggle_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(indices) = dock.find_tab(&tab) {
        dock.remove_tab(indices);
    } else {
        dock.push_to_focused_leaf(tab);
    }
}

#[cfg(test)]
mod tests {
    use super::{Tab, default_dock_state, toggle_tab};

    #[test]
    fn default_layout_contains_every_tab() {
        let dock = default_dock_state();
        for tab in Tab::ALL {
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} missing from default layout");
        }
    }

    #[test]
    fn default_layout_hides_nothing_except_system() {
        let dock = default_dock_state();
        let leaves: Vec<_> = dock.iter_leaves().collect();

        assert_eq!(leaves.len(), Tab::ALL.len() - 1, "expected one leaf per tab bar System");
        for (_, leaf) in leaves {
            let shared = leaf.tabs == vec![Tab::Performance, Tab::System];
            assert!(
                leaf.tabs.len() == 1 || shared,
                "a panel starts out hidden behind a sibling tab: {:?}",
                leaf.tabs
            );
        }
    }

    #[test]
    fn system_starts_behind_performance() {
        let dock = default_dock_state();
        let (_, leaf) = dock
            .iter_leaves()
            .find(|(_, leaf)| leaf.tabs.contains(&Tab::System))
            .expect("System is in the default layout");

        assert_eq!(
            leaf.tabs.get(leaf.active.0),
            Some(&Tab::Performance),
            "Performance should be the tab in front, not System"
        );
    }

    #[test]
    fn toggling_every_tab_off_and_on_is_a_round_trip() {
        // Removing a tab can collapse its leaf and shift sibling indices.
        let mut dock = default_dock_state();

        for tab in Tab::ALL {
            toggle_tab(&mut dock, tab);
            assert!(dock.find_tab(&tab).is_none(), "{tab:?} still present after closing");
        }

        for tab in Tab::ALL {
            toggle_tab(&mut dock, tab);
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} missing after reopening");
        }
    }
}
