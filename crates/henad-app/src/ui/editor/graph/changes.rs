//! Changes of a converted copy since its conversion, as Show changes lists them in the side panel and marks them on
//! the canvas.
//!
//! henad-graph's `changes_since_conversion` groups the nodes added, removed and changed by the frame that holds them,
//! and the nodes in no frame by canvas. The list names each node and opens it on its canvas, and the canvas marks an
//! added or changed node with a badge and a stroke. A removed node is listed alone, by the name its kind has in the
//! original's lowered graph. The canvas no longer holds it.

use std::collections::BTreeMap;

use egui::{Color32, RichText, Ui};
use henad_graph::document::GraphDocument;
use henad_graph::edit::Scope;
use henad_graph::ids::NodeId;
use henad_graph::node::NodeBody;
use henad_graph::sheet::ChangeGroup;
use henad_graph::sheet::sentence::{CHANGES_HEADING, IN_NO_FRAME, NODE_ADDED, NODE_CHANGED, NODE_REMOVED};

use super::display::display_name;
use super::{GraphEditor, GraphRequest};
use crate::ui::mcs;

/// Change of one node of a copy that the canvas marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeMark {
    /// The expert added the node after the conversion.
    Added,
    /// The node differs from the one the conversion wrote.
    Changed,
}

impl ChangeMark {
    /// Word of the mark's badge.
    pub fn word(self) -> &'static str {
        match self {
            Self::Added => NODE_ADDED,
            Self::Changed => NODE_CHANGED,
        }
    }

    /// Fill, text and stroke colours of the mark: green for an added node and yellow for a changed one, as the state
    /// mapping of [`mcs`] names them.
    pub fn colors(self) -> [Color32; 3] {
        match self {
            Self::Added => [mcs::GREEN_900, mcs::GREEN_200, mcs::GREEN_500],
            Self::Changed => [mcs::YELLOW_900, mcs::YELLOW_200, mcs::YELLOW_500],
        }
    }
}

/// Changes of a copy since its conversion: the nodes added, removed and changed by frame, and the name of each removed
/// node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyChanges {
    /// Nodes added, removed and changed, by frame.
    pub groups: Vec<ChangeGroup>,
    /// Name of each removed node, such as "Is (n2001)", from the original's lowered graph.
    pub removed_names: BTreeMap<NodeId, String>,
}

impl CopyChanges {
    /// Returns the changes `groups` with the name of each removed node read from `lowered`, the graph the original
    /// lowers to.
    pub fn new(groups: Vec<ChangeGroup>, lowered: &GraphDocument) -> Self {
        let bodies: Vec<&NodeBody> = lowered
            .passes
            .iter()
            .map(|pass| &pass.body)
            .chain(lowered.functions.iter().map(|function| &function.body))
            .collect();
        let removed_names = groups
            .iter()
            .flat_map(|group| group.removed.iter().copied())
            .filter_map(|node| {
                let found = bodies.iter().find_map(|body| body.node(node))?;
                Some((node, format!("{} ({node})", kind_name(&found.op))))
            })
            .collect();
        Self { groups, removed_names }
    }

    /// Returns whether no node was added, removed or changed.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Returns the display name of the node kind `kind`, else the kind itself.
fn kind_name(kind: &str) -> String {
    display_name(kind).map_or_else(|| kind.to_owned(), |name| name.name.to_owned())
}

/// Returns the body whose canvas the editor region names `canvas`, such as `pass.tick_cells`, `None` for a name that
/// names no body.
pub fn canvas_scope(canvas: &str) -> Option<Scope> {
    if let Some(id) = canvas.strip_prefix("pass.") {
        Some(Scope::Pass(id.to_owned()))
    } else {
        canvas
            .strip_prefix("function.")
            .map(|id| Scope::Function(id.to_owned()))
    }
}

/// Returns the mark of each node of the body `scope` that `groups` lists as added or changed.
pub fn marks_of(groups: &[ChangeGroup], scope: &Scope) -> BTreeMap<NodeId, ChangeMark> {
    let canvas = scope.canvas();
    let mut marks = BTreeMap::new();
    for group in groups.iter().filter(|group| group.scope == canvas) {
        marks.extend(group.added.iter().map(|node| (*node, ChangeMark::Added)));
        marks.extend(group.changed.iter().map(|node| (*node, ChangeMark::Changed)));
    }
    marks
}

/// Returns the name the list gives `node` of the body `scope` of the copy: its label, else the display name of its
/// kind, then its id.
fn node_name(graph: &GraphEditor, scope: Option<&Scope>, node: NodeId) -> String {
    let label = graph.editor_region().node_layout(node).and_then(|layout| layout.label);
    let kind = scope
        .and_then(|scope| graph.body(scope))
        .and_then(|body| body.node(node))
        .map(|found| kind_name(&found.op));
    match label.or(kind) {
        Some(name) => format!("{name} ({node})"),
        None => node.to_string(),
    }
}

/// Draws the changes the editor shows, each group under its frame's title, and pushes a request that opens a listed
/// node on its canvas.
pub fn changes_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    ui.strong(CHANGES_HEADING);
    for group in &graph.shown_changes.groups {
        ui.add_space(4.0);
        let title = group.title.as_deref().unwrap_or(IN_NO_FRAME);
        ui.label(RichText::new(title).strong());
        let scope = canvas_scope(&group.scope);
        if let Some(Scope::Pass(id) | Scope::Function(id)) = &scope {
            ui.weak(id);
        }
        for (word, nodes) in [(NODE_ADDED, &group.added), (NODE_CHANGED, &group.changed)] {
            for node in nodes {
                let text = format!("{word}: {}", node_name(graph, scope.as_ref(), *node));
                if ui.link(text).on_hover_text("Show this node").clicked()
                    && let Some(scope) = &scope
                {
                    requests.push(GraphRequest::Frame {
                        scope: scope.clone(),
                        node: *node,
                    });
                }
            }
        }
        for node in &group.removed {
            let name = graph
                .shown_changes
                .removed_names
                .get(node)
                .cloned()
                .unwrap_or_else(|| node.to_string());
            ui.label(format!("{NODE_REMOVED}: {name}"));
        }
    }
}
