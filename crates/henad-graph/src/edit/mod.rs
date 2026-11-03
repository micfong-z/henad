//! Edit commands and the builder that applies them: one command per gesture of the node editor, and the same commands
//! for tests and the command line.
//!
//! A [`GraphBuilder`] holds a document and its editor region. [`GraphBuilder::apply`] runs one [`EditCommand`], which
//! changes the document's structure and leaves its meaning to the checker: a command refuses an id that does not
//! exist, a name outside its grammar or a delete that other tables still name, and accepts a wire of the wrong type.
//! A rename rewrites every reference, and a delete with references is refused with their list.
//!
//! A new node takes the number `max(highest id in the file + 1, [editor] next_id)`, and a node of a kind with a `key`
//! takes a fresh random key, distinct from every key in the file. A paste mints fresh ids and fresh keys.

mod clipboard;
mod declarations;

use std::collections::BTreeSet;
use std::fmt;

use henad_core::params::ParamDescriptor;

use crate::catalogue::{Effect, lookup};
use crate::document::{Action, Category, GraphDocument, Lane, Palette, Pass, PassStage, Stat, Variant, World};
use crate::editor::EditorRegion;
use crate::file::{GraphFile, GraphFileError};
use crate::ids::{NodeId, is_port_segment};
use crate::node::{Node, NodeBody};
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// A pass or a function, whose nodes form one body.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Scope {
    /// The pass with this id.
    Pass(String),
    /// The function with this id.
    Function(String),
}

impl Scope {
    /// Returns the pass `id`.
    pub fn pass(id: &str) -> Self {
        Self::Pass(id.to_owned())
    }

    /// Returns the name the editor region gives the canvas of the body, such as `pass.seed`.
    pub fn canvas(&self) -> String {
        match self {
            Self::Pass(id) => format!("pass.{id}"),
            Self::Function(id) => format!("function.{id}"),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass(id) => write!(f, "pass '{id}'"),
            Self::Function(id) => write!(f, "function '{id}'"),
        }
    }
}

/// One change to a graph, as one gesture of the editor makes it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EditCommand {
    /// Adds a node of kind `op` to `scope`, inside `region` when set, at `pos` on the canvas when set.
    AddNode {
        /// Body the node joins.
        scope: Scope,
        /// Kind of the node.
        op: String,
        /// Region the node joins, `None` for the top of the body.
        region: Option<NodeId>,
        /// Position on the canvas.
        pos: Option<[f64; 2]>,
    },
    /// Removes a node, every wire from it in its body, and its placement. The members of a region move to the region
    /// around it.
    RemoveNode {
        /// Body that holds the node.
        scope: Scope,
        /// Node to remove.
        node: NodeId,
    },
    /// Wires the output `source` to the input `port` of `node`.
    Connect {
        /// Body that holds both nodes.
        scope: Scope,
        /// Node whose input takes the wire.
        node: NodeId,
        /// Input port, dotted when configuration makes it.
        port: String,
        /// Output the wire reads.
        source: PortRef,
    },
    /// Clears the input `port` of `node`, whatever it holds.
    Disconnect {
        /// Body that holds the node.
        scope: Scope,
        /// Node whose input is cleared.
        node: NodeId,
        /// Input port.
        port: String,
    },
    /// Sets the input `port` of `node` to a constant.
    SetLiteral {
        /// Body that holds the node.
        scope: Scope,
        /// Node whose input is set.
        node: NodeId,
        /// Input port.
        port: String,
        /// Constant, whose floats are finite.
        literal: Literal,
    },
    /// Binds the input `port` of `node` to the parameter `param`.
    BindParam {
        /// Body that holds the node.
        scope: Scope,
        /// Node whose input is bound.
        node: NodeId,
        /// Input port.
        port: String,
        /// Id of a declared parameter.
        param: String,
    },
    /// Sets the configuration key `key` of `node`, or removes it when `value` is `None`.
    SetConfig {
        /// Body that holds the node.
        scope: Scope,
        /// Node whose configuration changes.
        node: NodeId,
        /// Configuration key.
        key: String,
        /// New value.
        value: Option<ConfigValue>,
    },
    /// Moves `node` into `region`, or out of every region when `region` is `None`.
    SetRegion {
        /// Body that holds the node.
        scope: Scope,
        /// Node that moves.
        node: NodeId,
        /// Region the node joins.
        region: Option<NodeId>,
    },
    /// Places `node` at `pos` on its canvas, which changes the editor region alone.
    Move {
        /// Node that moves.
        node: NodeId,
        /// Position on the canvas in points.
        pos: [f64; 2],
    },
    /// Draws a titled frame around `members` on the canvas of `scope`.
    Frame {
        /// Body whose canvas holds the frame.
        scope: Scope,
        /// Title.
        title: String,
        /// MCS hue name.
        color: String,
        /// Nodes inside the frame.
        members: Vec<NodeId>,
    },
    /// Pastes the nodes a [`GraphBuilder::copy`] text holds into `scope`, moved by `offset` on the canvas.
    Paste {
        /// Body the nodes join.
        scope: Scope,
        /// Copied text.
        text: String,
        /// Distance the pasted nodes move from their copied places.
        offset: [f64; 2],
    },
    /// Adds a category.
    AddCategory(Category),
    /// Adds a palette.
    AddPalette(Palette),
    /// Adds a lane.
    AddLane(Lane),
    /// Renames a lane and every reference to it.
    RenameLane {
        /// Current id.
        from: String,
        /// New id.
        to: String,
    },
    /// Removes a lane that nothing reads or writes.
    RemoveLane(String),
    /// Adds a variant at the end of a category.
    AddVariant {
        /// Id of the category.
        category: String,
        /// New variant.
        variant: Variant,
    },
    /// Renames a variant and every reference to it.
    RenameVariant {
        /// Id of the category.
        category: String,
        /// Current name.
        from: String,
        /// New name.
        to: String,
    },
    /// Removes a variant that nothing names.
    RemoveVariant {
        /// Id of the category.
        category: String,
        /// Name of the variant.
        name: String,
    },
    /// Adds a parameter at the end of the schema.
    AddParam(ParamDescriptor),
    /// Renames a parameter and every binding of it.
    RenameParam {
        /// Current id.
        from: String,
        /// New id.
        to: String,
    },
    /// Removes a parameter that nothing binds.
    RemoveParam(String),
    /// Adds a statistic at the end of the schema.
    AddStat(Stat),
    /// Relabels a statistic.
    RenameStat {
        /// Current label.
        from: String,
        /// New label.
        to: String,
    },
    /// Removes a statistic.
    RemoveStat(String),
    /// Adds an action.
    AddAction(Action),
    /// Renames an action and every pass of it.
    RenameAction {
        /// Current id.
        from: String,
        /// New id.
        to: String,
    },
    /// Removes an action that no pass belongs to.
    RemoveAction(String),
    /// Adds a pass at the end of its stage.
    AddPass(Pass),
    /// Renames a pass, every rerun of it and its canvas.
    RenamePass {
        /// Current id.
        from: String,
        /// New id.
        to: String,
    },
    /// Removes a pass and its nodes, when no action reruns it and no statistic reads it.
    RemovePass(String),
    /// Moves a pass to position `index` among the passes of its stage.
    MovePass {
        /// Id of the pass.
        id: String,
        /// New position within its stage, clamped to the last.
        index: usize,
    },
}

/// Reason an edit command is refused. A refused command changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// The command names something the document does not hold.
    Missing(String),
    /// The command breaks a rule of the document's structure, such as a duplicate id.
    Invalid(String),
    /// A delete of something that other tables still name.
    Referenced {
        /// What the command would delete.
        subject: String,
        /// Each place that names it.
        references: Vec<String>,
    },
    /// A paste of text that is not a graph file.
    Paste(GraphFileError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(message) | Self::Invalid(message) => f.write_str(message),
            Self::Referenced { subject, references } => {
                write!(f, "{subject} is still named by {}", references.join(", "))
            }
            Self::Paste(error) => write!(f, "the pasted text is no graph: {error}"),
        }
    }
}

impl std::error::Error for EditError {}

/// A graph under edit: its document, its editor region, and the source of the random keys it mints.
#[derive(Clone)]
pub struct GraphBuilder {
    document: GraphDocument,
    editor: EditorRegion,
    /// State of the `xorshift64` stream new keys come from, never 0.
    key_state: u64,
}

/// Prints the graph's id and the number of passes.
impl fmt::Debug for GraphBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphBuilder")
            .field("id", &self.document.id)
            .field("passes", &self.document.passes.len())
            .finish_non_exhaustive()
    }
}

impl GraphBuilder {
    /// Returns a builder of an empty graph with the model id `id` over `world`.
    pub fn new(id: &str, world: World) -> Self {
        Self::from_document(GraphDocument::new(id, world), None)
    }

    /// Returns a builder of the graph `file` holds.
    pub fn from_file(file: &GraphFile) -> Self {
        Self::from_document(file.document().clone(), file.editor())
    }

    /// Returns a builder of `document` and `editor`.
    pub fn from_document(document: GraphDocument, editor: Option<&EditorRegion>) -> Self {
        Self {
            document,
            editor: editor.cloned().unwrap_or_default(),
            key_state: clipboard::entropy(),
        }
    }

    /// Returns the builder with its keys drawn from `seed`, so a test mints the same keys every run.
    pub fn with_key_seed(mut self, seed: u64) -> Self {
        self.key_state = henad_core::authoring::primitives::rng::mix_seed(seed);
        self
    }

    /// Document under edit.
    pub fn document(&self) -> &GraphDocument {
        &self.document
    }

    /// Editor region under edit.
    pub fn editor(&self) -> &EditorRegion {
        &self.editor
    }

    /// Returns the canonical text of the graph and its editor region.
    pub fn to_toml(&self) -> String {
        let mut text = crate::writer::semantic_text(&self.document);
        text.push('\n');
        text.push_str(&self.editor.to_text());
        text
    }

    /// Returns the graph as a file, read back from its canonical text.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`GraphFile::parse`] for a document whose canonical text does not read back.
    pub fn to_file(&self) -> Result<GraphFile, GraphFileError> {
        GraphFile::from_document(&self.document, Some(&self.editor))
    }

    /// Runs `command`, and returns the ids of the nodes it makes, in id order.
    ///
    /// # Errors
    ///
    /// Returns an [`EditError`] for a command the document refuses. A refused command changes nothing.
    pub fn apply(&mut self, command: EditCommand) -> Result<Vec<NodeId>, EditError> {
        let mut draft = self.clone();
        let made = draft.run(command)?;
        *self = draft;
        Ok(made)
    }

    /// Adds a node of kind `op` at the top of `scope`, sets `config` and `inputs`, and returns its id.
    ///
    /// # Errors
    ///
    /// Returns the errors of the [`EditCommand::AddNode`], [`EditCommand::SetConfig`], [`EditCommand::Connect`],
    /// [`EditCommand::SetLiteral`] and [`EditCommand::BindParam`] it runs. On an error nothing changes.
    pub fn add_node(
        &mut self,
        scope: &Scope,
        op: &str,
        config: &[(&str, ConfigValue)],
        inputs: &[(&str, InputValue)],
    ) -> Result<NodeId, EditError> {
        let mut draft = self.clone();
        let made = draft.run(EditCommand::AddNode {
            scope: scope.clone(),
            op: op.to_owned(),
            region: None,
            pos: None,
        })?;
        let node = *made
            .first()
            .ok_or_else(|| EditError::Invalid("the node was not made".to_owned()))?;
        for (key, value) in config {
            draft.run(EditCommand::SetConfig {
                scope: scope.clone(),
                node,
                key: (*key).to_owned(),
                value: Some(value.clone()),
            })?;
        }
        for (port, value) in inputs {
            let port = (*port).to_owned();
            draft.run(match value.clone() {
                InputValue::Link(source) => EditCommand::Connect {
                    scope: scope.clone(),
                    node,
                    port,
                    source,
                },
                InputValue::Literal(literal) => EditCommand::SetLiteral {
                    scope: scope.clone(),
                    node,
                    port,
                    literal,
                },
                InputValue::Param(param) => EditCommand::BindParam {
                    scope: scope.clone(),
                    node,
                    port,
                    param,
                },
            })?;
        }
        *self = draft;
        Ok(node)
    }

    /// Runs `command` on the builder in place. The caller keeps a copy to restore on an error.
    fn run(&mut self, command: EditCommand) -> Result<Vec<NodeId>, EditError> {
        match command {
            EditCommand::AddNode { scope, op, region, pos } => self.add(&scope, &op, region, pos).map(|id| vec![id]),
            EditCommand::RemoveNode { scope, node } => self.remove(&scope, node).map(|()| Vec::new()),
            EditCommand::Connect {
                scope,
                node,
                port,
                source,
            } => {
                self.require_node(&scope, source.node)?;
                self.set_input(&scope, node, &port, InputValue::Link(source))
            }
            EditCommand::Disconnect { scope, node, port } => {
                let target = self.node_mut(&scope, node)?;
                target
                    .inputs
                    .remove(&port)
                    .map(|_| Vec::new())
                    .ok_or_else(|| EditError::Missing(format!("node {node} has no input `{port}`")))
            }
            EditCommand::SetLiteral {
                scope,
                node,
                port,
                literal,
            } => {
                if !is_finite(&literal) {
                    return Err(EditError::Invalid(format!("a literal of `{port}` must be finite")));
                }
                self.set_input(&scope, node, &port, InputValue::Literal(literal))
            }
            EditCommand::BindParam {
                scope,
                node,
                port,
                param,
            } => {
                if !self.document.params.iter().any(|known| known.id() == param) {
                    return Err(EditError::Missing(format!("the file declares no parameter '{param}'")));
                }
                self.set_input(&scope, node, &port, InputValue::Param(param))
            }
            EditCommand::SetConfig {
                scope,
                node,
                key,
                value,
            } => {
                if !is_port_segment(&key) {
                    return Err(EditError::Invalid(format!("'{key}' is no configuration key")));
                }
                let target = self.node_mut(&scope, node)?;
                match value {
                    Some(value) => target.config.insert(key, value),
                    None => target.config.remove(&key),
                };
                Ok(Vec::new())
            }
            EditCommand::SetRegion { scope, node, region } => self.set_region(&scope, node, region),
            EditCommand::Move { node, pos } => {
                if !self.all_nodes().any(|known| known.id == node) {
                    return Err(EditError::Missing(format!("the file holds no node {node}")));
                }
                self.editor.set_node_pos(node, pos);
                Ok(Vec::new())
            }
            EditCommand::Frame {
                scope,
                title,
                color,
                members,
            } => {
                for member in &members {
                    self.require_node(&scope, *member)?;
                }
                self.editor.add_frame(&scope.canvas(), &title, &color, &members);
                Ok(Vec::new())
            }
            EditCommand::Paste { scope, text, offset } => self.paste(&scope, &text, offset),
            other => self.run_declaration(other).map(|()| Vec::new()),
        }
    }

    /// Returns the body of `scope`.
    fn body(&self, scope: &Scope) -> Result<&NodeBody, EditError> {
        match scope {
            Scope::Pass(id) => self
                .document
                .passes
                .iter()
                .find(|pass| pass.id == *id)
                .map(|pass| &pass.body),
            Scope::Function(id) => self
                .document
                .functions
                .iter()
                .find(|function| function.id == *id)
                .map(|function| &function.body),
        }
        .ok_or_else(|| EditError::Missing(format!("the file declares no {scope}")))
    }

    /// Returns the body of `scope` for writing.
    fn body_mut(&mut self, scope: &Scope) -> Result<&mut NodeBody, EditError> {
        match scope {
            Scope::Pass(id) => self
                .document
                .passes
                .iter_mut()
                .find(|pass| pass.id == *id)
                .map(|pass| &mut pass.body),
            Scope::Function(id) => self
                .document
                .functions
                .iter_mut()
                .find(|function| function.id == *id)
                .map(|function| &mut function.body),
        }
        .ok_or_else(|| EditError::Missing(format!("the file declares no {scope}")))
    }

    /// Returns an error unless `scope` holds node `node`.
    fn require_node(&self, scope: &Scope, node: NodeId) -> Result<(), EditError> {
        self.body(scope)?
            .node(node)
            .map(|_| ())
            .ok_or_else(|| EditError::Missing(format!("the {scope} holds no node {node}")))
    }

    /// Returns node `node` of `scope` for writing.
    fn node_mut(&mut self, scope: &Scope, node: NodeId) -> Result<&mut Node, EditError> {
        let missing = EditError::Missing(format!("the {scope} holds no node {node}"));
        self.body_mut(scope)?
            .nodes
            .iter_mut()
            .find(|known| known.id == node)
            .ok_or(missing)
    }

    /// Returns every node of the document, in passes and functions.
    fn all_nodes(&self) -> impl Iterator<Item = &Node> {
        self.document
            .passes
            .iter()
            .map(|pass| &pass.body)
            .chain(self.document.functions.iter().map(|function| &function.body))
            .flat_map(|body| body.nodes.iter())
    }

    /// Sets input `port` of `node` to `value`.
    fn set_input(
        &mut self,
        scope: &Scope,
        node: NodeId,
        port: &str,
        value: InputValue,
    ) -> Result<Vec<NodeId>, EditError> {
        if !port.split('.').all(is_port_segment) {
            return Err(EditError::Invalid(format!("'{port}' is no port name")));
        }
        self.node_mut(scope, node)?.inputs.insert(port.to_owned(), value);
        Ok(Vec::new())
    }

    /// Returns a fresh node id, and records the next one in the editor region.
    fn mint_id(&mut self) -> Result<NodeId, EditError> {
        let highest = self.all_nodes().map(|node| node.id.number()).max().unwrap_or(0);
        let next = highest
            .checked_add(1)
            .map(|after| after.max(self.editor.next_id().unwrap_or(0)))
            .and_then(NodeId::new)
            .ok_or_else(|| EditError::Invalid("the file has no node number left".to_owned()))?;
        if self.editor.table().get("version").is_none() {
            self.editor
                .table_mut()
                .insert("version".to_owned(), toml::Value::Integer(1));
        }
        self.editor.set_next_id(next.number().saturating_add(1));
        Ok(next)
    }

    /// Returns a fresh random key, distinct from every key in the file.
    fn mint_key(&mut self) -> u32 {
        let taken: BTreeSet<i64> = self
            .all_nodes()
            .filter_map(|node| node.config.get("key").and_then(ConfigValue::as_integer))
            .collect();
        loop {
            self.key_state = henad_core::authoring::primitives::rng::xorshift64(self.key_state);
            let key = (self.key_state >> 32) as u32;
            if !taken.contains(&i64::from(key)) {
                return key;
            }
        }
    }

    /// Adds a node of kind `op` to `scope`.
    fn add(
        &mut self,
        scope: &Scope,
        op: &str,
        region: Option<NodeId>,
        pos: Option<[f64; 2]>,
    ) -> Result<NodeId, EditError> {
        let def = lookup(op, self.document.library)
            .ok_or_else(|| EditError::Missing(format!("node library {} has no kind `{op}`", self.document.library)))?;
        self.body(scope)?;
        if let Some(region) = region {
            self.require_region(scope, region)?;
        }
        let id = self.mint_id()?;
        let mut node = Node::new(id, op);
        node.region = region;
        if def.config.iter().any(|key| key.name == "key") {
            node.config
                .insert("key".to_owned(), ConfigValue::Integer(i64::from(self.mint_key())));
        }
        self.body_mut(scope)?.nodes.push(node);
        if def.has_effect(Effect::DrawsStream)
            && let Scope::Pass(pass) = scope
            && let Some(pass) = self.document.passes.iter_mut().find(|known| known.id == *pass)
        {
            pass.draw_order.push(id);
        }
        if let Some(pos) = pos {
            self.editor.set_node_pos(id, pos);
        }
        Ok(id)
    }

    /// Returns an error unless `region` is a region node of `scope`.
    fn require_region(&self, scope: &Scope, region: NodeId) -> Result<(), EditError> {
        let node = self
            .body(scope)?
            .node(region)
            .ok_or_else(|| EditError::Missing(format!("the {scope} holds no node {region}")))?;
        let is_region = lookup(&node.op, self.document.library).is_some_and(|def| def.is_region);
        if is_region {
            Ok(())
        } else {
            Err(EditError::Invalid(format!("node {region} is no region")))
        }
    }

    /// Removes node `node` of `scope`.
    fn remove(&mut self, scope: &Scope, node: NodeId) -> Result<(), EditError> {
        self.require_node(scope, node)?;
        let readers: Vec<String> = self
            .document
            .stats
            .iter()
            .filter(|stat| stat.from.node == node && matches!(scope, Scope::Pass(_)))
            .map(|stat| format!("the statistic '{}'", stat.label))
            .collect();
        if !readers.is_empty() {
            return Err(EditError::Referenced {
                subject: format!("node {node}"),
                references: readers,
            });
        }
        let body = self.body_mut(scope)?;
        let parent = body.node(node).and_then(|removed| removed.region);
        body.nodes.retain(|known| known.id != node);
        for other in &mut body.nodes {
            other
                .inputs
                .retain(|_, value| !matches!(value, InputValue::Link(link) if link.node == node));
            if other.region == Some(node) {
                other.region = parent;
            }
        }
        if let Scope::Pass(pass) = scope
            && let Some(pass) = self.document.passes.iter_mut().find(|known| known.id == *pass)
        {
            pass.draw_order.retain(|known| *known != node);
        }
        self.editor.remove_node(node);
        Ok(())
    }

    /// Moves `node` of `scope` into `region`.
    fn set_region(&mut self, scope: &Scope, node: NodeId, region: Option<NodeId>) -> Result<Vec<NodeId>, EditError> {
        self.require_node(scope, node)?;
        if let Some(region) = region {
            self.require_region(scope, region)?;
            let body = self.body(scope)?;
            if region == node || body.region_path(region).contains(&node) {
                return Err(EditError::Invalid(format!("node {node} cannot sit inside itself")));
            }
        }
        self.node_mut(scope, node)?.region = region;
        Ok(Vec::new())
    }

    /// Adds `pass` after the last pass of its stage, which keeps the passes in stack order.
    fn insert_pass(&mut self, pass: Pass) {
        let stage: PassStage = pass.stage;
        let position = self
            .document
            .passes
            .iter()
            .rposition(|known| known.stage <= stage)
            .map_or(0, |last| last + 1);
        self.document.passes.insert(position, pass);
    }
}

/// Returns whether every float of `literal` is finite.
fn is_finite(literal: &Literal) -> bool {
    match literal {
        Literal::F32(number) => number.is_finite(),
        Literal::F64(number) => number.is_finite(),
        Literal::Vec2F(pair) => pair.iter().all(|number| number.is_finite()),
        _ => true,
    }
}
