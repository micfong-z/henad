//! Tab definitions, the two layouts of the dock, and dispatch to each panel module.
//!
//! A *layout* is an arrangement of the tabs. The default layout is for running a model, sweeping and reading results,
//! and the Editor layout for writing a model and trying it. The app keeps a dock state per layout for the session.

use egui_dock::{DockState, Node, NodeIndex, NodePath, Split, SurfaceIndex, TabViewer};

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

// Splits of the Editor layout, each the share of the left or top side of its split.

/// Share of the width that the Editor's leaf takes.
const EDITOR_LEAF_SHARE: f32 = 0.625;
/// Share of the right column's height that the Viewport takes.
const VIEWPORT_LEAF_SHARE: f32 = 0.46;
/// Share of the height under the Viewport that Playback and Pacing take.
const PLAYBACK_LEAF_SHARE: f32 = 0.3;

/// Returns the Editor layout, as Reset layout restores it.
///
/// The Editor is in front of Sweep and Results on the left. The column on the right holds the Viewport, then Playback
/// in front of Pacing, then Charts in front of Parameters, Statistics and Model. Performance, System and Export are
/// closed.
pub fn editor_dock_state() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Editor, Tab::Sweep, Tab::Results]);
    let surface = dock.main_surface_mut();

    let [_, viewport] = surface.split_right(NodeIndex::root(), EDITOR_LEAF_SHARE, vec![Tab::Viewport]);
    let [_, playback] = surface.split_below(viewport, VIEWPORT_LEAF_SHARE, vec![Tab::Playback, Tab::Pacing]);
    surface.split_below(
        playback,
        PLAYBACK_LEAF_SHARE,
        vec![Tab::Charts, Tab::Params, Tab::Stats, Tab::Model],
    );

    dock
}

/// Arrangement of the tabs in the dock.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Layout {
    /// Arrangement for running a model, sweeping and reading results, with the Editor closed.
    #[default]
    Default,
    /// Arrangement for writing a model and trying it, with the Editor on most of the width.
    Editor,
}

impl Layout {
    /// Every layout, in View-menu order.
    pub const ALL: [Self; 2] = [Self::Default, Self::Editor];

    pub fn title(self) -> &'static str {
        match self {
            Self::Default => "Default layout",
            Self::Editor => "Editor layout",
        }
    }

    /// Returns the layout's dock state as the app starts it, and as Reset layout restores it.
    pub fn dock_state(self) -> DockState<Tab> {
        match self {
            Self::Default => default_dock_state(),
            Self::Editor => editor_dock_state(),
        }
    }
}

/// Dock state of each layout, and the layout that shows.
///
/// Each layout keeps its dock state while the other shows, so a tab moved or closed in one is still so after a visit
/// to the other. Nothing is stored across restarts, and the app starts in the default layout.
#[derive(Debug)]
pub struct Workspace {
    /// Layout that shows.
    layout: Layout,
    /// Dock state of the default layout.
    default: DockState<Tab>,
    /// Dock state of the Editor layout.
    editor: DockState<Tab>,
    /// Whether a request to bring the Editor to the front switches to the Editor layout first.
    pub editor_layout_on_open: bool,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            layout: Layout::Default,
            default: default_dock_state(),
            editor: editor_dock_state(),
            editor_layout_on_open: true,
        }
    }
}

impl Workspace {
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Dock state of the layout that shows.
    pub fn dock(&self) -> &DockState<Tab> {
        match self.layout {
            Layout::Default => &self.default,
            Layout::Editor => &self.editor,
        }
    }

    /// Mutable dock state of the layout that shows.
    pub fn dock_mut(&mut self) -> &mut DockState<Tab> {
        match self.layout {
            Layout::Default => &mut self.default,
            Layout::Editor => &mut self.editor,
        }
    }

    /// Shows `layout` as the user last left it.
    pub fn show(&mut self, layout: Layout) {
        self.layout = layout;
    }

    /// Puts the layout that shows back as it starts. The other layout keeps its changes.
    pub fn reset(&mut self) {
        *self.dock_mut() = self.layout.dock_state();
    }

    /// Adds `tab` to the layout that shows if it is closed there, and removes it if it is open.
    pub fn toggle(&mut self, tab: Tab) {
        let layout = self.layout;
        toggle_tab(self.dock_mut(), layout, tab);
    }

    /// Brings `tab` to the front of its leaf, and opens it where it is closed.
    ///
    /// A request for the Editor switches to the Editor layout first while [`Self::editor_layout_on_open`] holds. Every
    /// site that opens a model in the Editor makes that request.
    pub fn focus(&mut self, tab: Tab) {
        if tab == Tab::Editor && self.editor_layout_on_open {
            self.layout = Layout::Editor;
        }
        let layout = self.layout;
        focus_tab(self.dock_mut(), layout, tab);
    }
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

/// Adds `tab` to `dock`, a dock state of `layout`, if it is closed, and removes it if it is open.
fn toggle_tab(dock: &mut DockState<Tab>, layout: Layout, tab: Tab) {
    if let Some(indices) = dock.find_tab(&tab) {
        dock.remove_tab(indices);
    } else {
        open_tab(dock, layout, tab);
    }
}

/// Brings `tab` to the front of its leaf in `dock`, a dock state of `layout`, and opens it where it is closed.
fn focus_tab(dock: &mut DockState<Tab>, layout: Layout, tab: Tab) {
    match dock.find_tab(&tab) {
        Some(path) => {
            if let Err(error) = dock.set_active_tab(path) {
                log::warn!("Could not bring {tab:?} to the front: {error}");
            }
        }
        None => open_tab(dock, layout, tab),
    }
}

/// Share of the Viewport's leaf that the Viewport keeps when the Editor opens beside it in the default layout.
const EDITOR_SPLIT: f32 = 0.5;

/// Opens `tab`, which is closed in `dock`, a dock state of `layout`.
///
/// In the default layout the Editor opens in a leaf of its own to the right of the Viewport's leaf, and the Viewport
/// comes to the front of its leaf, so the run stays in view while its code is edited. In the Editor layout the Editor
/// opens in front of Sweep and Results, or in a leaf of its own on the left where both are closed. Any other tab joins
/// the focused leaf, as does the Editor where neither rule finds a place.
fn open_tab(dock: &mut DockState<Tab>, layout: Layout, tab: Tab) {
    let placed = tab == Tab::Editor
        && match layout {
            Layout::Default => open_editor_beside_viewport(dock),
            Layout::Editor => open_editor_in_its_leaf(dock),
        };
    if !placed {
        dock.push_to_focused_leaf(tab);
    }
}

/// Opens the Editor in a leaf of its own to the right of the Viewport's leaf, with the Viewport in front, and returns
/// whether the Viewport was open.
fn open_editor_beside_viewport(dock: &mut DockState<Tab>) -> bool {
    let Some(viewport) = dock.find_tab(&Tab::Viewport) else {
        return false;
    };
    if let Err(error) = dock.set_active_tab(viewport) {
        log::warn!("Could not bring {:?} to the front: {error}", Tab::Viewport);
    }
    let parent = viewport.node_path();
    let [_, editor] = dock.split(parent, Split::Right, EDITOR_SPLIT, Node::leaf(Tab::Editor));
    dock.set_focused_node_and_surface(NodePath {
        surface: parent.surface,
        node: editor,
    });
    true
}

/// Opens the Editor at the front of the leaf of Sweep or Results, or in a leaf of its own on the left of the main
/// surface where both are closed, and returns whether it found a place.
///
/// Note that an empty main surface leaves the Editor no place.
fn open_editor_in_its_leaf(dock: &mut DockState<Tab>) -> bool {
    let stack = dock.find_tab(&Tab::Sweep).or_else(|| dock.find_tab(&Tab::Results));
    if let Some(path) = stack
        && let Ok(leaf) = dock.leaf_mut(path.node_path())
    {
        leaf.insert_tab(0, Tab::Editor);
        dock.set_focused_node_and_surface(path.node_path());
        return true;
    }
    let surface = dock.main_surface_mut();
    if surface.root_node().is_none_or(Node::is_empty) {
        return false;
    }
    let [_, editor] = surface.split_left(NodeIndex::root(), EDITOR_LEAF_SHARE, vec![Tab::Editor]);
    dock.set_focused_node_and_surface(NodePath {
        surface: SurfaceIndex::main(),
        node: editor,
    });
    true
}

#[cfg(test)]
mod tests {
    use egui_dock::{DockState, Node, NodeIndex};

    use super::{Layout, Tab, Workspace, default_dock_state, editor_dock_state, focus_tab, toggle_tab};

    /// Returns the tabs of each leaf of `dock`.
    fn leaves(dock: &DockState<Tab>) -> Vec<Vec<Tab>> {
        dock.iter_leaves().map(|(_, leaf)| leaf.tabs.clone()).collect()
    }

    /// Returns the tab in front of the leaf that holds `tab`, `None` while `tab` is closed.
    fn front_of(dock: &DockState<Tab>, tab: Tab) -> Option<Tab> {
        let (_, leaf) = dock.iter_leaves().find(|(_, leaf)| leaf.tabs.contains(&tab))?;
        leaf.tabs.get(leaf.active.0).copied()
    }

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
            focus_tab(&mut dock, Layout::Default, tab);
            assert_eq!(front_of(&dock, Tab::Viewport), Some(tab), "{tab:?} is not in front");
            assert!(
                leaves(&dock).contains(&STACKED[0].to_vec()),
                "focusing {tab:?} moved a tab out of the stack"
            );
        }
    }

    /// The Editor never stacks onto the Viewport, which would hide the run while its code is edited, and opens with the
    /// Viewport in front of the tabs stacked behind it.
    #[test]
    fn the_editor_opens_beside_the_viewport() {
        for open in [focus_tab, toggle_tab] {
            let mut dock = default_dock_state();
            focus_tab(&mut dock, Layout::Default, Tab::Sweep);
            open(&mut dock, Layout::Default, Tab::Editor);
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
        toggle_tab(&mut dock, Layout::Default, Tab::Viewport);
        focus_tab(&mut dock, Layout::Default, Tab::Editor);
        assert!(
            dock.find_tab(&Tab::Editor).is_some(),
            "the Editor stayed closed without a Viewport"
        );
    }

    #[test]
    fn focusing_a_tab_brings_it_in_front_of_its_siblings() {
        let mut dock = default_dock_state();
        focus_tab(&mut dock, Layout::Default, Tab::Export);
        assert_eq!(front_of(&dock, Tab::Export), Some(Tab::Export));
    }

    #[test]
    fn focusing_a_closed_tab_opens_it() {
        let mut dock = default_dock_state();
        toggle_tab(&mut dock, Layout::Default, Tab::Viewport);
        focus_tab(&mut dock, Layout::Default, Tab::Viewport);
        assert!(dock.find_tab(&Tab::Viewport).is_some(), "Viewport stayed closed");
    }

    #[test]
    fn toggling_every_tab_off_and_on_is_a_round_trip() {
        // Removing a tab can collapse its leaf and shift sibling indices.
        let mut dock = default_dock_state();
        toggle_tab(&mut dock, Layout::Default, Tab::Editor);

        for tab in Tab::ALL {
            toggle_tab(&mut dock, Layout::Default, tab);
            assert!(dock.find_tab(&tab).is_none(), "{tab:?} still present after closing");
        }

        for tab in Tab::ALL {
            toggle_tab(&mut dock, Layout::Default, tab);
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} missing after reopening");
        }
    }

    // The Editor layout

    /// Tabs the Editor layout leaves closed.
    const CLOSED_IN_EDITOR_LAYOUT: [Tab; 3] = [Tab::Performance, Tab::System, Tab::Export];

    /// Tabs of each leaf of the Editor layout, with the front tab first.
    const EDITOR_LAYOUT_STACKS: [&[Tab]; 4] = [
        &[Tab::Editor, Tab::Sweep, Tab::Results],
        &[Tab::Viewport],
        &[Tab::Playback, Tab::Pacing],
        &[Tab::Charts, Tab::Params, Tab::Stats, Tab::Model],
    ];

    #[test]
    fn editor_layout_contains_every_tab_but_performance_system_and_export() {
        let dock = editor_dock_state();
        for tab in Tab::ALL {
            let closed = CLOSED_IN_EDITOR_LAYOUT.contains(&tab);
            assert_eq!(
                dock.find_tab(&tab).is_some(),
                !closed,
                "{tab:?} is open or closed against the Editor layout"
            );
        }
    }

    #[test]
    fn the_editor_layout_holds_its_four_stacks_and_nothing_else() {
        let found = leaves(&editor_dock_state());
        assert_eq!(
            found.len(),
            EDITOR_LAYOUT_STACKS.len(),
            "expected one leaf per stack: {found:?}"
        );
        for stack in EDITOR_LAYOUT_STACKS {
            assert!(
                found.iter().any(|leaf| leaf == stack),
                "{stack:?} is not a leaf of {found:?}"
            );
        }
    }

    #[test]
    fn the_editor_and_the_first_of_each_editor_layout_stack_are_in_front() {
        let dock = editor_dock_state();
        for stack in EDITOR_LAYOUT_STACKS {
            assert_eq!(
                front_of(&dock, stack[0]),
                Some(stack[0]),
                "{:?} should be the tab in front of {:?}",
                stack[0],
                &stack[1..]
            );
        }
    }

    /// The Editor takes the left of the window, and the column on the right holds the Viewport over Playback over
    /// Charts.
    #[test]
    fn the_editor_takes_the_left_and_the_world_the_right_column() {
        let dock = editor_dock_state();
        let surface = dock.main_surface();
        let root = NodeIndex::root();
        let column = root.right();
        let node_of = |tab: Tab| dock.find_tab(&tab).expect("the tab is open").node;

        assert!(
            matches!(surface[root], Node::Horizontal(_)),
            "the Editor is not beside the column"
        );
        assert_eq!(node_of(Tab::Editor), root.left(), "the Editor is not on the left");
        assert!(
            matches!(surface[column], Node::Vertical(_)),
            "the column is not stacked"
        );
        assert_eq!(node_of(Tab::Viewport), column.left(), "the Viewport is not at the top");
        assert_eq!(
            node_of(Tab::Playback),
            column.right().left(),
            "Playback is not under the Viewport"
        );
        assert_eq!(
            node_of(Tab::Charts),
            column.right().right(),
            "Charts is not at the bottom"
        );
    }

    #[test]
    fn sweep_and_results_come_to_the_front_of_the_editor_leaf() {
        let mut dock = editor_dock_state();
        for tab in [Tab::Results, Tab::Sweep, Tab::Editor] {
            focus_tab(&mut dock, Layout::Editor, tab);
            assert_eq!(front_of(&dock, Tab::Editor), Some(tab), "{tab:?} is not in front");
            assert_eq!(
                leaves(&dock),
                leaves(&editor_dock_state()),
                "focusing {tab:?} moved a tab"
            );
        }
    }

    /// The Editor closed by hand comes back in front of Sweep and Results, and in a leaf of its own on the left where
    /// both are closed too.
    #[test]
    fn the_editor_reopens_in_its_leaf_of_the_editor_layout() {
        for open in [focus_tab, toggle_tab] {
            let mut dock = editor_dock_state();
            focus_tab(&mut dock, Layout::Editor, Tab::Sweep);
            toggle_tab(&mut dock, Layout::Editor, Tab::Editor);
            open(&mut dock, Layout::Editor, Tab::Editor);
            assert_eq!(leaves(&dock), leaves(&editor_dock_state()), "the Editor left its stack");
            assert_eq!(
                front_of(&dock, Tab::Editor),
                Some(Tab::Editor),
                "the Editor is not in front"
            );
        }

        let mut dock = editor_dock_state();
        for tab in [Tab::Editor, Tab::Sweep, Tab::Results] {
            toggle_tab(&mut dock, Layout::Editor, tab);
        }
        focus_tab(&mut dock, Layout::Editor, Tab::Editor);
        let editor = dock.find_tab(&Tab::Editor).expect("the Editor opened");
        assert_eq!(editor.node, NodeIndex::root().left(), "the Editor is not on the left");
        assert_eq!(front_of(&dock, Tab::Editor), Some(Tab::Editor));
        assert!(
            leaves(&dock).contains(&vec![Tab::Editor]),
            "the Editor shares a leaf: {:?}",
            leaves(&dock)
        );

        let mut dock = editor_dock_state();
        for tab in Tab::ALL
            .into_iter()
            .filter(|tab| !CLOSED_IN_EDITOR_LAYOUT.contains(tab))
        {
            toggle_tab(&mut dock, Layout::Editor, tab);
        }
        focus_tab(&mut dock, Layout::Editor, Tab::Editor);
        assert_eq!(
            leaves(&dock),
            [[Tab::Editor]],
            "the Editor did not open on an empty layout"
        );
    }

    #[test]
    fn focusing_a_tab_of_the_editor_layout_brings_it_in_front_of_its_siblings() {
        let mut dock = editor_dock_state();
        for tab in [Tab::Pacing, Tab::Model] {
            focus_tab(&mut dock, Layout::Editor, tab);
            assert_eq!(front_of(&dock, tab), Some(tab), "{tab:?} is not in front");
        }
    }

    #[test]
    fn focusing_a_tab_the_editor_layout_closes_opens_it() {
        let mut dock = editor_dock_state();
        toggle_tab(&mut dock, Layout::Editor, Tab::Viewport);
        for tab in [Tab::Viewport, Tab::Export] {
            focus_tab(&mut dock, Layout::Editor, tab);
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} stayed closed");
        }
    }

    #[test]
    fn toggling_every_tab_of_the_editor_layout_off_and_on_is_a_round_trip() {
        let mut dock = editor_dock_state();
        for tab in CLOSED_IN_EDITOR_LAYOUT {
            toggle_tab(&mut dock, Layout::Editor, tab);
        }

        for tab in Tab::ALL {
            toggle_tab(&mut dock, Layout::Editor, tab);
            assert!(dock.find_tab(&tab).is_none(), "{tab:?} still present after closing");
        }

        for tab in Tab::ALL {
            toggle_tab(&mut dock, Layout::Editor, tab);
            assert!(dock.find_tab(&tab).is_some(), "{tab:?} missing after reopening");
        }
    }

    // Switching layouts

    #[test]
    fn a_switch_keeps_the_changes_of_each_layout() {
        let mut workspace = Workspace::default();
        workspace.toggle(Tab::Model);
        workspace.show(Layout::Editor);
        workspace.toggle(Tab::Charts);
        workspace.focus(Tab::Results);

        workspace.show(Layout::Default);
        assert!(
            workspace.dock().find_tab(&Tab::Model).is_none(),
            "Model reopened in the default layout"
        );
        assert!(
            workspace.dock().find_tab(&Tab::Charts).is_some(),
            "the Editor layout closed Charts here too"
        );

        workspace.show(Layout::Editor);
        assert!(
            workspace.dock().find_tab(&Tab::Charts).is_none(),
            "Charts reopened in the Editor layout"
        );
        assert!(
            workspace.dock().find_tab(&Tab::Model).is_some(),
            "the default layout closed Model here too"
        );
        assert_eq!(
            front_of(workspace.dock(), Tab::Editor),
            Some(Tab::Results),
            "the Editor layout forgot its front tab"
        );
    }

    #[test]
    fn a_request_for_the_editor_switches_layouts_only_while_the_checkbox_is_on() {
        let mut workspace = Workspace::default();
        assert_eq!(
            workspace.layout(),
            Layout::Default,
            "the app starts in the default layout"
        );
        assert!(workspace.editor_layout_on_open, "the checkbox starts on");
        for tab in Tab::ALL.into_iter().filter(|tab| *tab != Tab::Editor) {
            workspace.focus(tab);
            assert_eq!(
                workspace.layout(),
                Layout::Default,
                "a request for {tab:?} switched layouts"
            );
        }
        workspace.focus(Tab::Editor);
        assert_eq!(
            workspace.layout(),
            Layout::Editor,
            "a request for the Editor kept the default layout"
        );
        assert_eq!(front_of(workspace.dock(), Tab::Editor), Some(Tab::Editor));
        workspace.focus(Tab::Viewport);
        assert_eq!(
            workspace.layout(),
            Layout::Editor,
            "a request for the Viewport switched back"
        );
        workspace.show(Layout::Default);
        assert!(
            workspace.dock().find_tab(&Tab::Editor).is_none(),
            "the request opened the Editor in the default layout too"
        );

        let mut workspace = Workspace {
            editor_layout_on_open: false,
            ..Workspace::default()
        };
        workspace.focus(Tab::Editor);
        assert_eq!(
            workspace.layout(),
            Layout::Default,
            "the request switched with the checkbox off"
        );
        let editor = workspace.dock().find_tab(&Tab::Editor).expect("the Editor opened");
        let viewport = workspace.dock().find_tab(&Tab::Viewport).expect("the Viewport is open");
        assert_eq!(
            editor.node.parent(),
            viewport.node.parent(),
            "the Editor did not open beside the Viewport"
        );
    }

    #[test]
    fn reset_layout_resets_the_layout_that_shows_alone() {
        let mut workspace = Workspace::default();
        workspace.toggle(Tab::Model);
        workspace.show(Layout::Editor);
        workspace.toggle(Tab::Charts);
        workspace.toggle(Tab::Export);

        workspace.reset();
        assert_eq!(workspace.layout(), Layout::Editor, "Reset layout switched layouts");
        assert_eq!(leaves(workspace.dock()), leaves(&editor_dock_state()));
        assert_eq!(front_of(workspace.dock(), Tab::Editor), Some(Tab::Editor));

        workspace.show(Layout::Default);
        assert!(
            workspace.dock().find_tab(&Tab::Model).is_none(),
            "Reset layout reset the default layout too"
        );
        workspace.reset();
        assert_eq!(leaves(workspace.dock()), leaves(&default_dock_state()));
    }
}
