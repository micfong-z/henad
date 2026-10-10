//! Tab definitions, the default layout, and dispatch to each panel module.

use egui_dock::{DockState, Node, NodeIndex, NodePath, Split, TabViewer};

use crate::icons::material_design_icons::{
    MDI_CHART_BOX_OUTLINE, MDI_CHART_LINE, MDI_CHIP, MDI_CODE_BRACES, MDI_COG_OUTLINE, MDI_CUBE_OUTLINE, MDI_EXPORT,
    MDI_FLASK_OUTLINE, MDI_GAUGE, MDI_PLAY_CIRCLE_OUTLINE, MDI_SPEEDOMETER, MDI_TABLE, MDI_TUNE,
};
use crate::state::AppState;
use crate::ui;

/// A dockable panel.
///
/// Note that egui keys a tab's stored state on the variant's discriminant. A new tab goes last, so the others keep
/// theirs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tab {
    Viewport,
    Playback,
    Pacing,
    Model,
    Params,
    Sweep,
    Results,
    Stats,
    Charts,
    Export,
    Performance,
    System,
    Editor,
}

impl Tab {
    /// Every tab that exists, in View-menu order.
    pub const ALL: [Self; 13] = [
        Self::Viewport,
        Self::Playback,
        Self::Pacing,
        Self::Model,
        Self::Params,
        Self::Editor,
        Self::Sweep,
        Self::Results,
        Self::Stats,
        Self::Charts,
        Self::Export,
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
            Self::Sweep => "Sweep",
            Self::Results => "Results",
            Self::Stats => "Statistics",
            Self::Charts => "Charts",
            Self::Export => "Export",
            Self::Performance => "Performance",
            Self::System => "System",
            Self::Editor => "Editor",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Viewport => MDI_CUBE_OUTLINE,
            Self::Playback => MDI_PLAY_CIRCLE_OUTLINE,
            Self::Pacing => MDI_SPEEDOMETER,
            Self::Model => MDI_COG_OUTLINE,
            Self::Params => MDI_TUNE,
            Self::Sweep => MDI_FLASK_OUTLINE,
            Self::Results => MDI_CHART_BOX_OUTLINE,
            Self::Stats => MDI_TABLE,
            Self::Charts => MDI_CHART_LINE,
            Self::Export => MDI_EXPORT,
            Self::Performance => MDI_GAUGE,
            Self::System => MDI_CHIP,
            Self::Editor => MDI_CODE_BRACES,
        }
    }
}

/// Returns the default layout, as Reset layout restores it. Every tab but the Editor is open.
pub fn default_dock_state() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport, Tab::Sweep, Tab::Results]);
    let surface = dock.main_surface_mut();

    let [viewport, model] = surface.split_left(NodeIndex::root(), 0.2, vec![Tab::Model]);
    let [_, params] = surface.split_below(model, 0.38, vec![Tab::Params]);
    let [_, playback] = surface.split_below(params, 0.58, vec![Tab::Playback]);
    surface.split_below(playback, 0.5, vec![Tab::Pacing]);

    let [_, perf] = surface.split_right(viewport, 0.7, vec![Tab::Performance, Tab::System]);
    let [_, stats] = surface.split_below(perf, 0.25, vec![Tab::Stats]);
    surface.split_below(stats, 0.4, vec![Tab::Charts, Tab::Export]);

    dock
}

impl TabViewer for AppState {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            // The id is keyed on the variant. A title that changes with the sweep resets nothing.
            Tab::Sweep => self.sweep.tab_title().into(),
            _ => format!("{}  {}", tab.icon(), tab.title()).into(),
        }
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
            Tab::Sweep => ui::sweep::sweep_ui(ui, self),
            Tab::Results => ui::results::results_ui(ui, self),
            Tab::Stats => ui::stats::stats_ui(ui, self),
            Tab::Charts => ui::charts::charts_ui(ui, self),
            Tab::Export => ui::export::export_ui(ui, self),
            Tab::Performance => ui::performance::performance_ui(ui, self),
            Tab::System => ui::system::system_ui(ui, self),
            Tab::Editor => ui::editor::editor_ui(ui, self),
        }
    }

    /// The viewport's paint callback covers its own rect, so the fill under it is wasted.
    fn clear_background(&self, tab: &Self::Tab) -> bool {
        *tab != Tab::Viewport
    }

    /// The Sweep tab scrolls its own form between a fixed header and footer, and the Editor its code between a fixed
    /// header and diagnostics list. An outer scroll would carry them away.
    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        match tab {
            Tab::Sweep | Tab::Editor => [false, false],
            _ => [true, true],
        }
    }
}

/// Adds `tab` if it is closed, removes it if it is open.
pub fn toggle_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(indices) = dock.find_tab(&tab) {
        dock.remove_tab(indices);
    } else {
        open_tab(dock, tab);
    }
}

/// Brings `tab` to the front of its leaf, and opens it where it is closed.
pub fn focus_tab(dock: &mut DockState<Tab>, tab: Tab) {
    match dock.find_tab(&tab) {
        Some(path) => {
            if let Err(error) = dock.set_active_tab(path) {
                log::warn!("Could not bring {tab:?} to the front: {error}");
            }
        }
        None => open_tab(dock, tab),
    }
}

/// Share of the Viewport's leaf that the Viewport keeps when the Editor opens beside it.
const EDITOR_SPLIT: f32 = 0.5;

/// Opens `tab`, which is closed.
///
/// The Editor opens in a leaf of its own to the right of the Viewport's leaf, and the Viewport comes to the front of its
/// leaf, so the run stays in view while its code is edited. Any other tab, and the Editor while the Viewport is
/// closed, joins the focused leaf.
fn open_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if tab == Tab::Editor
        && let Some(viewport) = dock.find_tab(&Tab::Viewport)
    {
        if let Err(error) = dock.set_active_tab(viewport) {
            log::warn!("Could not bring {:?} to the front: {error}", Tab::Viewport);
        }
        let parent = viewport.node_path();
        let [_, editor] = dock.split(parent, Split::Right, EDITOR_SPLIT, Node::leaf(tab));
        dock.set_focused_node_and_surface(NodePath {
            surface: parent.surface,
            node: editor,
        });
        return;
    }
    dock.push_to_focused_leaf(tab);
}

#[cfg(test)]
mod tests {
    use egui_dock::Node;

    use super::{Tab, default_dock_state, focus_tab, toggle_tab};

    /// Tabs the default layout leaves closed.
    const CLOSED_BY_DEFAULT: [Tab; 1] = [Tab::Editor];

    #[test]
    fn default_layout_contains_every_tab_but_the_editor() {
        let dock = default_dock_state();
        for tab in Tab::ALL {
            let closed = CLOSED_BY_DEFAULT.contains(&tab);
            assert_eq!(
                dock.find_tab(&tab).is_some(),
                !closed,
                "{tab:?} is open or closed against the default layout"
            );
        }
        assert!(dock.find_tab(&Tab::Editor).is_none(), "the Editor starts closed");
    }

    /// Tabs that share a leaf in the default layout, with the front tab first.
    const STACKED: [&[Tab]; 3] = [
        &[Tab::Viewport, Tab::Sweep, Tab::Results],
        &[Tab::Charts, Tab::Export],
        &[Tab::Performance, Tab::System],
    ];

    #[test]
    fn nothing_shares_a_leaf_except_the_stacked_tabs() {
        let dock = default_dock_state();
        let leaves: Vec<_> = dock.iter_leaves().collect();
        let hidden: usize = STACKED.iter().map(|stack| stack.len() - 1).sum();

        assert_eq!(
            leaves.len(),
            Tab::ALL.len() - CLOSED_BY_DEFAULT.len() - hidden,
            "expected one leaf per open tab, bar the stacked ones"
        );
        for (_, leaf) in leaves {
            assert!(
                leaf.tabs.len() == 1 || STACKED.iter().any(|stack| leaf.tabs == *stack),
                "a panel starts out hidden behind a sibling tab: {:?}",
                leaf.tabs
            );
        }
    }

    #[test]
    fn the_first_of_each_stack_is_the_one_in_front() {
        let dock = default_dock_state();
        for stack in STACKED {
            let (_, leaf) = dock
                .iter_leaves()
                .find(|(_, leaf)| leaf.tabs == stack)
                .expect("every stack is in the default layout");

            assert_eq!(
                leaf.tabs.get(leaf.active.0),
                Some(&stack[0]),
                "{:?} should be the tab in front of {:?}",
                stack[0],
                &stack[1..]
            );
        }
    }

    #[test]
    fn sweep_and_results_come_to_the_front_of_the_viewport_leaf() {
        let mut dock = default_dock_state();
        for tab in [Tab::Results, Tab::Sweep, Tab::Viewport] {
            focus_tab(&mut dock, tab);
            let (_, leaf) = dock
                .iter_leaves()
                .find(|(_, leaf)| leaf.tabs.contains(&Tab::Viewport))
                .expect("Viewport is in the default layout");
            assert_eq!(leaf.tabs.get(leaf.active.0), Some(&tab), "{tab:?} is not in front");
            assert_eq!(leaf.tabs.len(), 3, "focusing {tab:?} moved a tab out of the stack");
        }
    }

    /// The Editor never stacks onto the Viewport, which would hide the run while its code is edited, and opens with the
    /// Viewport in front of the tabs stacked behind it.
    #[test]
    fn the_editor_opens_beside_the_viewport() {
        for open in [focus_tab, toggle_tab] {
            let mut dock = default_dock_state();
            focus_tab(&mut dock, Tab::Sweep);
            open(&mut dock, Tab::Editor);
            let editor = dock.find_tab(&Tab::Editor).expect("the Editor opened");
            let viewport = dock
                .find_tab(&Tab::Viewport)
                .expect("Viewport is in the default layout");
            let (_, leaf) = dock
                .iter_leaves()
                .find(|(_, leaf)| leaf.tabs.contains(&Tab::Editor))
                .expect("the Editor is in a leaf");
            assert_eq!(leaf.tabs, [Tab::Editor], "the Editor shares a leaf");
            let (_, leaf) = dock
                .iter_leaves()
                .find(|(_, leaf)| leaf.tabs.contains(&Tab::Viewport))
                .expect("the Viewport is in a leaf");
            assert_eq!(leaf.tabs, STACKED[0], "the Viewport's stack changed");
            assert_eq!(
                leaf.active, viewport.tab,
                "the Sweep tab stayed in front of the Viewport"
            );

            let parent = viewport.node.parent().expect("the Viewport's leaf was split");
            assert_eq!(viewport.node, parent.left(), "the Viewport is not the left half");
            assert_eq!(editor.node, parent.right(), "the Editor is not the right half");
            assert!(
                matches!(dock.main_surface()[parent], Node::Horizontal(_)),
                "the halves are not side by side"
            );
        }

        let mut dock = default_dock_state();
        toggle_tab(&mut dock, Tab::Viewport);
        focus_tab(&mut dock, Tab::Editor);
        assert!(
            dock.find_tab(&Tab::Editor).is_some(),
            "the Editor stayed closed without a Viewport"
        );
    }

    #[test]
    fn focusing_a_tab_brings_it_in_front_of_its_siblings() {
        let mut dock = default_dock_state();
        focus_tab(&mut dock, Tab::Export);
        let (_, leaf) = dock
            .iter_leaves()
            .find(|(_, leaf)| leaf.tabs.contains(&Tab::Export))
            .expect("Export is in the default layout");
        assert_eq!(leaf.tabs.get(leaf.active.0), Some(&Tab::Export));
    }

    #[test]
    fn focusing_a_closed_tab_opens_it() {
        let mut dock = default_dock_state();
        toggle_tab(&mut dock, Tab::Viewport);
        focus_tab(&mut dock, Tab::Viewport);
        assert!(dock.find_tab(&Tab::Viewport).is_some(), "Viewport stayed closed");
    }

    #[test]
    fn toggling_every_tab_off_and_on_is_a_round_trip() {
        // Removing a tab can collapse its leaf and shift sibling indices.
        let mut dock = default_dock_state();
        toggle_tab(&mut dock, Tab::Editor);

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
