//! Changes of a converted copy since its conversion: the nodes added, removed and changed, grouped by frame.
//!
//! The copy's nodes are compared by node id with the graph the sheet lowers to. One sheet always lowers to one graph. A
//! node *changed* when its kind, configuration, inputs, region or canvas differ.

use std::collections::BTreeMap;
use std::fmt;

use super::convert::frames;
use super::{SheetFile, sentence};
use crate::document::GraphDocument;
use crate::editor::FrameLayout;
use crate::file::GraphFile;
use crate::ids::NodeId;
use crate::node::Node;

/// Nodes of one frame that differ between a copy and the graph its sheet lowers to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ChangeGroup {
    /// Canvas of the frame, such as `pass.tick_cell_tick`.
    pub scope: String,
    /// Title of the frame, `None` for the nodes of the canvas that sit in no frame.
    pub title: Option<String>,
    /// Nodes the copy holds and the sheet's graph does not, in id order.
    pub added: Vec<NodeId>,
    /// Nodes the sheet's graph holds and the copy does not, in id order.
    pub removed: Vec<NodeId>,
    /// Nodes both hold that differ, in id order.
    pub changed: Vec<NodeId>,
}

impl ChangeGroup {
    /// Returns the group of the frame titled `title` on the canvas `scope`, with no node yet.
    fn empty(scope: &str, title: Option<String>) -> Self {
        Self {
            scope: scope.to_owned(),
            title,
            added: Vec::new(),
            removed: Vec::new(),
            changed: Vec::new(),
        }
    }
}

/// Reason the changes of a copy cannot be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangesError {
    /// The copy records no conversion.
    NoOrigin,
    /// The sheet's code no longer hashes to the code the copy records, so the model the sheet holds changed since the
    /// conversion or is another model.
    ///
    /// Note that an edit outside the code, such as a note, a comment or a parameter's meaning, leaves the code's hash
    /// alone.
    OriginalChanged {
        /// Name of the sheet the copy was converted from.
        name: String,
    },
}

/// Writes the words the copy shows.
impl fmt::Display for ChangesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOrigin => write!(f, "the model records no conversion"),
            Self::OriginalChanged { name } => write!(f, "{}", sentence::original_changed(name)),
        }
    }
}

impl std::error::Error for ChangesError {}

/// Returns the changes of `copy` since it was converted from `original`, one group per frame that holds a change.
///
/// Each node is listed under the first frame of its canvas that holds it in the copy, or else in a fresh conversion of
/// `original`, so a removed node is listed under the frame it sat in at conversion. A node in neither is listed in the
/// group of its canvas with no title. Groups follow the frames of the conversion, then the frames only the copy holds,
/// then each canvas's group with no title.
///
/// # Errors
///
/// Returns [`ChangesError::NoOrigin`] for a copy with no origin record, and [`ChangesError::OriginalChanged`] when the
/// hash of the code of `original` differs from the `code` the copy records.
pub fn changes_since_conversion(copy: &GraphFile, original: &SheetFile) -> Result<Vec<ChangeGroup>, ChangesError> {
    let origin = copy
        .editor()
        .and_then(|editor| editor.model_origin())
        .ok_or(ChangesError::NoOrigin)?;
    if origin.code != format!("{:016x}", original.code().hash()) {
        return Err(ChangesError::OriginalChanged { name: origin.name });
    }
    let converted = frames(original, &original.parsed().check());
    let copied = copy.editor().map(|editor| editor.frames()).unwrap_or_default();
    let mut order: Vec<FrameLayout> = converted.clone();
    for frame in &copied {
        if !order.iter().any(|known| same_frame(known, frame)) {
            order.push(frame.clone());
        }
    }
    let ours = nodes_by_id(copy.document());
    let theirs = nodes_by_id(original.lowered().document());
    let frame_of = |scope: &str, node: NodeId| -> Option<usize> {
        let holds = |frame: &FrameLayout| frame.scope == scope && frame.members.contains(&node);
        let found = copied
            .iter()
            .find(|frame| holds(frame))
            .or_else(|| converted.iter().find(|frame| holds(frame)))?;
        order.iter().position(|known| same_frame(known, found))
    };
    // Groups by frame position, then the groups with no frame by canvas.
    let mut framed: BTreeMap<usize, ChangeGroup> = BTreeMap::new();
    let mut unframed: BTreeMap<String, ChangeGroup> = BTreeMap::new();
    let mut record = |scope: &str, node: NodeId, kind: fn(&mut ChangeGroup) -> &mut Vec<NodeId>| {
        let group = match frame_of(scope, node) {
            Some(position) => framed
                .entry(position)
                .or_insert_with(|| ChangeGroup::empty(&order[position].scope, Some(order[position].title.clone()))),
            None => unframed
                .entry(scope.to_owned())
                .or_insert_with(|| ChangeGroup::empty(scope, None)),
        };
        kind(group).push(node);
    };
    for (id, (scope, node)) in &ours {
        match theirs.get(id) {
            None => record(scope, *id, |group| &mut group.added),
            Some((their_scope, their_node)) if their_scope != scope || their_node != node => {
                record(scope, *id, |group| &mut group.changed);
            }
            Some(_) => {}
        }
    }
    for (id, (scope, _)) in &theirs {
        if !ours.contains_key(id) {
            record(scope, *id, |group| &mut group.removed);
        }
    }
    Ok(framed.into_values().chain(unframed.into_values()).collect())
}

/// Returns whether `first` and `second` are one frame: one canvas and one title.
fn same_frame(first: &FrameLayout, second: &FrameLayout) -> bool {
    first.scope == second.scope && first.title == second.title
}

/// Returns every node of the passes and functions of `document` with its canvas, by id.
fn nodes_by_id(document: &GraphDocument) -> BTreeMap<NodeId, (String, &Node)> {
    let passes = document.passes.iter().flat_map(|pass| {
        pass.body
            .nodes
            .iter()
            .map(move |node| (format!("pass.{}", pass.id), node))
    });
    let functions = document.functions.iter().flat_map(|function| {
        function
            .body
            .nodes
            .iter()
            .map(move |node| (format!("function.{}", function.id), node))
    });
    passes
        .chain(functions)
        .map(|(scope, node)| (node.id, (scope, node)))
        .collect()
}
