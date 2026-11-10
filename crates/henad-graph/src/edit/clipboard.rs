//! Copy and paste of nodes as model-file text.

use std::collections::BTreeMap;

use super::{EditError, GraphBuilder, Scope};
use crate::catalogue::{Effect, lookup};
use crate::document::Pass;
use crate::file::GraphFile;
use crate::ids::NodeId;
use crate::node::{Node, NodeBody};
use crate::value::{ConfigValue, InputValue};

/// Returns `nodes` with every wire to a node outside them dropped, and every region outside them cleared.
fn detached(nodes: Vec<Node>) -> Vec<Node> {
    let kept: Vec<NodeId> = nodes.iter().map(|node| node.id).collect();
    nodes
        .into_iter()
        .map(|mut node| {
            node.inputs
                .retain(|_, value| !matches!(value, InputValue::Link(link) if !kept.contains(&link.node)));
            if node.region.is_some_and(|region| !kept.contains(&region)) {
                node.region = None;
            }
            node
        })
        .collect()
}

impl GraphBuilder {
    /// Returns the model-file text of the nodes `nodes` of `scope`, which [`super::EditCommand::Paste`] reads.
    ///
    /// The text is a whole graph file: the declarations of the document, one pass or function of the copied body's
    /// kind holding the nodes, their placement, and each frame that holds one of them with only those members. A wire
    /// from a node outside the copy is dropped, and so is a region outside it. A paste adds a frame on its canvas for
    /// each frame of the text, every key kept, around the pasted nodes.
    ///
    /// # Errors
    ///
    /// Returns [`EditError::Missing`] for a scope or a node the document does not hold.
    pub fn copy(&self, scope: &Scope, nodes: &[NodeId]) -> Result<String, EditError> {
        for node in nodes {
            self.require_node(scope, *node)?;
        }
        let selected = |body: &NodeBody| -> Vec<Node> {
            detached(
                body.nodes
                    .iter()
                    .filter(|node| nodes.contains(&node.id))
                    .cloned()
                    .collect(),
            )
        };
        let mut document = self.document.clone();
        document.stats.clear();
        for action in &mut document.actions {
            action.runs.clear();
        }
        match scope {
            Scope::Pass(id) => {
                let mut pass: Pass = document
                    .passes
                    .iter()
                    .find(|pass| pass.id == *id)
                    .cloned()
                    .ok_or_else(|| EditError::Missing(format!("the file declares no {scope}")))?;
                pass.body.nodes = selected(&pass.body);
                pass.draw_order.retain(|node| nodes.contains(node));
                document.passes = vec![pass];
                document.functions.clear();
            }
            Scope::Function(id) => {
                let mut function = document
                    .functions
                    .iter()
                    .find(|function| function.id == *id)
                    .cloned()
                    .ok_or_else(|| EditError::Missing(format!("the file declares no {scope}")))?;
                function.body.nodes = selected(&function.body);
                document.functions = vec![function];
                document.passes.clear();
            }
        }
        let editor = self.editor.copy_of(&scope.canvas(), nodes);
        let mut text = crate::writer::semantic_text(&document);
        text.push('\n');
        text.push_str(&editor.to_text());
        Ok(text)
    }

    /// Pastes the nodes of `text` into `scope`, moved by `offset`, and returns their new ids.
    pub(super) fn paste(&mut self, scope: &Scope, text: &str, offset: [f64; 2]) -> Result<Vec<NodeId>, EditError> {
        let file = GraphFile::parse(text).map_err(EditError::Paste)?;
        let source = file
            .document()
            .passes
            .first()
            .map(|pass| (&pass.body, pass.draw_order.clone()))
            .or_else(|| {
                file.document()
                    .functions
                    .first()
                    .map(|function| (&function.body, Vec::new()))
            })
            .ok_or_else(|| EditError::Invalid("the pasted text holds no node".to_owned()))?;
        self.body(scope)?;
        let mut renamed: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut sorted: Vec<&Node> = source.0.nodes.iter().collect();
        sorted.sort_by_key(|node| node.id);
        for node in &sorted {
            if let std::collections::btree_map::Entry::Vacant(entry) = renamed.entry(node.id) {
                entry.insert(self.mint_id()?);
            }
        }
        let mut pasted = Vec::with_capacity(sorted.len());
        for node in sorted {
            let Some(&id) = renamed.get(&node.id) else { continue };
            if pasted.iter().any(|known: &Node| known.id == id) {
                continue;
            }
            let mut copy = node.clone();
            copy.id = id;
            copy.region = node.region.and_then(|region| renamed.get(&region).copied());
            copy.inputs = node
                .inputs
                .iter()
                .filter_map(|(port, value)| {
                    let kept = match value {
                        InputValue::Link(link) => {
                            let mut link = link.clone();
                            link.node = *renamed.get(&link.node)?;
                            InputValue::Link(link)
                        }
                        InputValue::Param(param) => {
                            self.document.param_position(param)?;
                            value.clone()
                        }
                        InputValue::Literal(_) => value.clone(),
                    };
                    Some((port.clone(), kept))
                })
                .collect();
            if lookup(&copy.op, self.document.library).is_some_and(|def| def.config.iter().any(|key| key.name == "key"))
            {
                copy.config
                    .insert("key".to_owned(), ConfigValue::Integer(i64::from(self.mint_key())));
            }
            if let Some(mut layout) = file.editor().and_then(|editor| editor.node_layout(node.id)) {
                layout.pos = [layout.pos[0] + offset[0], layout.pos[1] + offset[1]];
                layout.output_pos = layout.output_pos.map(|pos| [pos[0] + offset[0], pos[1] + offset[1]]);
                self.editor.place_node(id, &layout);
            }
            pasted.push(copy);
        }
        let streams: Vec<NodeId> = source
            .1
            .iter()
            .filter_map(|node| renamed.get(node).copied())
            .filter(|node| {
                pasted.iter().any(|known| {
                    known.id == *node
                        && lookup(&known.op, self.document.library)
                            .is_some_and(|def| def.has_effect(Effect::DrawsStream))
                })
            })
            .collect();
        let made: Vec<NodeId> = pasted.iter().map(|node| node.id).collect();
        self.body_mut(scope)?.nodes.extend(pasted);
        if let Some(source) = file.editor() {
            self.editor.push_renamed_frames(source, &scope.canvas(), &renamed);
        }
        if let Scope::Pass(pass) = scope
            && let Some(pass) = self.document.passes.iter_mut().find(|known| known.id == *pass)
        {
            pass.draw_order.extend(streams);
        }
        Ok(made)
    }
}
