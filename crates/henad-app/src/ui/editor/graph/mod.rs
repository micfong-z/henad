//! The graph editor: a graph model's document, edited through henad-graph's commands on a canvas and in forms.
//!
//! A *body* is the nodes of one pass, shown on a canvas of its own. Every gesture becomes a [`GraphRequest`]. The
//! requests of a frame wait while the primary button is down and run after the canvas has drawn, since egui-snarl
//! panics when a node under a wire in flight leaves its graph. An edit runs on a copy of the document, so a command
//! that is refused changes nothing, and its message shows under the canvas.
//!
//! Undo keeps the canonical text of the file before each edit, `[editor]` included. The *revision* counts the
//! document's changes, and a canvas whose revision is behind is brought to the document before it draws. The compiled
//! program of the latest revision is kept for the generated WGSL view and the preview.

pub mod arrange;
pub mod canvas;
pub mod history;
pub mod passes;
pub mod quick_add;
pub mod sockets;
pub mod view_pass;
pub mod wgsl_view;
pub mod world;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Button, CentralPanel, Event, Frame, Id, Key, KeyboardShortcut, Margin, Modifiers, Panel, ScrollArea};
use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};
use henad_core::params::ParamDescriptor;
use henad_graph::GraphFile;
use henad_graph::catalogue::lookup;
use henad_graph::check::CheckReport;
use henad_graph::document::{GraphDocument, Pass, PassStage, World};
use henad_graph::edit::{EditCommand, EditError, GraphBuilder, Scope};
use henad_graph::ids::NodeId;
use henad_graph::node::NodeBody;
use henad_graph::value::PortRef;
use henad_graph::wgsl::GridProgram;

use self::canvas::{BodyCanvas, CanvasContext, Framing, show_canvas};
use self::history::History;
use self::quick_add::{WireEnd, wire_port};
use self::view_pass::{ViewTarget, view_file};
use crate::icons::material_design_icons::{
    MDI_CODE_BRACES, MDI_CONTENT_COPY, MDI_DELETE_OUTLINE, MDI_EYE_OFF_OUTLINE, MDI_EYE_OUTLINE,
    MDI_FIT_TO_SCREEN_OUTLINE, MDI_REDO, MDI_SITEMAP_OUTLINE, MDI_UNDO,
};
use crate::ui::sweep::layout::add_button;

/// Default width in points of the side panel that holds the passes and the world.
const SIDEBAR_WIDTH: f32 = 220.0;

/// Default width in points of the side panel that holds the generated WGSL.
const WGSL_WIDTH: f32 = 420.0;

/// Distance in canvas points a paste moves its nodes when the pointer is off the canvas.
const PASTE_OFFSET: [f64; 2] = [40.0, 40.0];

/// Number of documents loaded so far, which keeps the egui state of each document's canvases apart.
static LOADS: AtomicU64 = AtomicU64::new(0);

/// Problems the checker found at one node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeIssues {
    /// Messages of the errors.
    pub errors: Vec<String>,
    /// Messages of the warnings.
    pub warnings: Vec<String>,
}

/// Commands of one gesture, run together: all of them change the document, or none does.
#[derive(Debug, Clone)]
pub struct GraphEdit {
    /// Commands in order.
    pub commands: Vec<EditCommand>,
    /// Coalesce key of the gesture, under which consecutive edits make one undo step.
    pub coalesce: Option<String>,
}

impl GraphEdit {
    /// Returns the edit of one command, an undo step of its own.
    pub fn one(command: EditCommand) -> Self {
        Self {
            commands: vec![command],
            coalesce: None,
        }
    }
}

/// End of a wire dropped on the canvas, which a node added from the dropped wire menu takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedWire {
    /// Node at the fixed end.
    pub node: NodeId,
    /// Port at the fixed end.
    pub port: String,
    /// Dragged end, with its type.
    pub end: WireEnd,
}

/// Request of a gesture in the graph editor, run once the canvas has drawn and the primary button is up.
#[derive(Debug, Clone)]
pub enum GraphRequest {
    /// Runs an edit.
    Edit(GraphEdit),
    /// Adds a node of kind `op` at `pos` on the open canvas, wired to a dropped wire if there is one.
    AddNode {
        /// Kind of the node.
        op: String,
        /// Position on the canvas.
        pos: [f64; 2],
        /// Wire the node takes.
        wire: Option<DroppedWire>,
    },
    /// Restores the text before the last edit.
    Undo,
    /// Restores the text the last undo replaced.
    Redo,
    /// Copies nodes of the open body to the clipboard as model-file text.
    Copy(Vec<NodeId>),
    /// Copies nodes of the open body, then removes them.
    Cut(Vec<NodeId>),
    /// Pastes model-file text into the open body, at `at` on the canvas when set.
    Paste {
        /// Copied text.
        text: String,
        /// Canvas position of the pasted nodes' top left.
        at: Option<[f64; 2]>,
    },
    /// Removes nodes of the open body.
    Delete(Vec<NodeId>),
    /// Places the nodes of the open body in layers from left to right.
    Arrange,
    /// Opens a body on the canvas.
    Open(Scope),
    /// Opens a body and brings one of its nodes into view.
    Frame {
        /// Body of the node.
        scope: Scope,
        /// Node to show.
        node: NodeId,
    },
    /// Brings every node of the open body into view.
    FrameAll,
    /// Shows the output `port` of `node` of the open pass in the Viewport, through the preview.
    View {
        /// The node.
        node: NodeId,
        /// Output port.
        port: String,
    },
    /// Returns the preview to the model's own view passes.
    StopViewing,
    /// Shows or hides the generated WGSL beside the canvas.
    ShowWgsl(bool),
}

/// Line under the canvas about the last gesture: a refusal, or a note such as a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphStatus {
    /// Text of the line.
    pub message: String,
    /// Whether the line reports a refusal, which shows in the theme's warning colour.
    pub warning: bool,
}

impl GraphStatus {
    /// Returns the status of a refusal.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            warning: true,
        }
    }

    /// Returns the status of a gesture that ran, in the theme's text colour.
    pub fn note(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            warning: false,
        }
    }
}

/// Program of one revision of the document: the compiled model, or the problems that stop it compiling.
pub type CompiledProgram = Result<GridProgram, Vec<CodeDiagnostic>>;

/// A graph model under edit.
pub struct GraphEditor {
    builder: GraphBuilder,
    history: History,
    /// Body on the canvas.
    scope: Scope,
    /// Canvas of each body opened so far.
    canvases: BTreeMap<Scope, BodyCanvas>,
    /// Report of the last check.
    report: CheckReport,
    /// Problem of a document whose canonical text does not read back, which no command should make.
    read_error: Option<CodeDiagnostic>,
    /// Problems of each pass's nodes, by pass id.
    issues: BTreeMap<String, BTreeMap<NodeId, NodeIssues>>,
    /// Number of changes of the document so far.
    revision: u64,
    /// Number of the load, which salts the egui ids of the canvases.
    load: u64,
    /// Number of canvases made so far, which gives each canvas an egui id of its own.
    canvases_made: u64,
    /// Requests waiting for the primary button to go up.
    pending: Vec<GraphRequest>,
    /// Whether the primary button was down at the last [`Self::settle`], so the next one ends a press.
    pressed: bool,
    /// Widget with keyboard focus at the last [`Self::settle`].
    focused: Option<Id>,
    /// Program of the document, with the revision it was compiled at.
    program: Option<(u64, CompiledProgram)>,
    /// Output shown in the Viewport in place of the model's view passes.
    viewing: Option<ViewTarget>,
    /// Number of changes of [`Self::viewing`] so far.
    view_changes: u64,
    /// Whether the generated WGSL shows beside the canvas.
    pub show_wgsl: bool,
    /// Line about the last refused edit or the last copy, `None` once another edit runs.
    pub status: Option<GraphStatus>,
}

/// Prints the graph's id, the open body and the revision.
impl std::fmt::Debug for GraphEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEditor")
            .field("id", &self.builder.document().id)
            .field("scope", &self.scope)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl GraphEditor {
    /// Returns an editor of `file`, with every body that has no placement arranged and the first tick pass open.
    pub fn new(file: &GraphFile) -> Self {
        let mut builder = GraphBuilder::from_file(file);
        place_unplaced(&mut builder);
        let document = builder.document();
        let scope = document
            .passes
            .iter()
            .find(|pass| pass.stage == PassStage::Tick)
            .or_else(|| document.passes.first())
            .map_or_else(|| Scope::pass(""), |pass| Scope::pass(&pass.id));
        let mut editor = Self {
            builder,
            history: History::default(),
            scope,
            canvases: BTreeMap::new(),
            report: CheckReport::default(),
            read_error: None,
            issues: BTreeMap::new(),
            revision: 0,
            load: LOADS.fetch_add(1, Ordering::Relaxed),
            canvases_made: 0,
            pending: Vec::new(),
            pressed: false,
            focused: None,
            program: None,
            viewing: None,
            view_changes: 0,
            show_wgsl: false,
            status: None,
        };
        editor.refresh();
        editor
    }

    /// Builder that holds the document and its editor region.
    #[cfg(test)]
    pub fn builder(&self) -> &GraphBuilder {
        &self.builder
    }

    /// Document under edit.
    pub fn document(&self) -> &GraphDocument {
        self.builder.document()
    }

    /// Returns the canonical text of the file, `[editor]` included.
    pub fn text(&self) -> String {
        self.builder.to_toml()
    }

    /// Body on the canvas.
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// Report of the last check.
    #[cfg(test)]
    pub fn report(&self) -> &CheckReport {
        &self.report
    }

    /// Returns whether the document holds an error, which stops it compiling.
    pub fn has_errors(&self) -> bool {
        self.read_error.is_some() || self.report.has_errors()
    }

    /// Returns the key of what the preview shows: the load, the revision and the changes of the viewed output. A
    /// preview built under another key is out of date.
    pub fn preview_key(&self) -> [u64; 3] {
        [self.load, self.revision, self.view_changes]
    }

    /// Output shown in the Viewport in place of the model's view passes.
    pub fn viewing(&self) -> Option<&ViewTarget> {
        self.viewing.as_ref()
    }

    /// Returns the program of the document as it is now, compiled on first use after each change.
    pub fn program(&mut self) -> &CompiledProgram {
        let revision = self.revision;
        let stale = self.program.as_ref().is_none_or(|(compiled, _)| *compiled != revision);
        if stale {
            let program = self
                .builder
                .to_file()
                .map_err(|error| vec![error.diagnostic().clone()])
                .and_then(|file| GridProgram::compile(&file));
            self.program = Some((revision, program));
        }
        match &self.program {
            Some((_, program)) => program,
            None => unreachable!("the program was compiled above"),
        }
    }

    /// Returns the program the preview runs: the document's, or its view file's while an output is viewed.
    ///
    /// A viewed output that the document no longer allows to show is dropped, and the status names the reason.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the program compiling.
    pub fn preview_program(&mut self) -> CompiledProgram {
        if let Some(target) = self.viewing.clone() {
            match view_file(&self.builder, &self.report, &target) {
                Ok(file) => return GridProgram::compile(&file),
                Err(message) => {
                    self.status = Some(GraphStatus::warning(format!(
                        "Stopped viewing {}. {message}",
                        target.name()
                    )));
                    self.set_viewing(None);
                }
            }
        }
        self.program().clone()
    }

    /// Replaces the output shown in the Viewport.
    fn set_viewing(&mut self, viewing: Option<ViewTarget>) {
        if self.viewing != viewing {
            self.viewing = viewing;
            self.view_changes += 1;
        }
    }

    /// History of the edits.
    #[cfg(test)]
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Returns the canvas of `scope`, `None` before it is first shown.
    #[cfg(test)]
    pub fn canvas(&self, scope: &Scope) -> Option<&BodyCanvas> {
        self.canvases.get(scope)
    }

    /// Returns every problem of the file in file order: the read problem of a broken document, then the report's.
    pub fn diagnostics(&self) -> impl Iterator<Item = &CodeDiagnostic> {
        self.read_error.iter().chain(&self.report.diagnostics)
    }

    /// Returns the number of errors and warnings in the pass `id`.
    pub fn pass_issue_counts(&self, id: &str) -> (usize, usize) {
        self.report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.pass.as_deref() == Some(id))
            .fold((0, 0), |(errors, warnings), diagnostic| {
                if diagnostic.severity == DiagnosticSeverity::Error {
                    (errors + 1, warnings)
                } else {
                    (errors, warnings + 1)
                }
            })
    }

    /// Returns the pass on the canvas, `None` for a function.
    pub fn open_pass(&self) -> Option<&Pass> {
        match &self.scope {
            Scope::Pass(id) => self.document().passes.iter().find(|pass| pass.id == *id),
            _ => None,
        }
    }

    /// Returns the body of `scope`.
    fn body(&self, scope: &Scope) -> Option<&NodeBody> {
        let document = self.document();
        match scope {
            Scope::Pass(id) => document
                .passes
                .iter()
                .find(|pass| pass.id == *id)
                .map(|pass| &pass.body),
            Scope::Function(id) => document
                .functions
                .iter()
                .find(|function| function.id == *id)
                .map(|function| &function.body),
            _ => None,
        }
    }

    /// Runs `request` now, as the editor runs it once the primary button is up.
    pub fn run(&mut self, ctx: &egui::Context, request: GraphRequest) {
        match request {
            GraphRequest::Edit(edit) => {
                self.edit(edit.commands, edit.coalesce.as_deref());
            }

            GraphRequest::AddNode { op, pos, wire } => self.add_node(&op, pos, wire.as_ref()),
            GraphRequest::Undo => {
                let current = self.text();
                if let Some(text) = self.history.undo(current) {
                    self.restore(&text);
                }
            }
            GraphRequest::Redo => {
                let current = self.text();
                if let Some(text) = self.history.redo(current) {
                    self.restore(&text);
                }
            }
            GraphRequest::Copy(nodes) => {
                self.copy(ctx, &nodes);
            }
            GraphRequest::Cut(nodes) => {
                if self.copy(ctx, &nodes) {
                    self.delete(&nodes);
                }
            }
            GraphRequest::Paste { text, at } => self.paste(&text, at),
            GraphRequest::Delete(nodes) => self.delete(&nodes),
            GraphRequest::Arrange => {
                let Some(body) = self.body(&self.scope) else { return };
                let sizes: BTreeMap<NodeId, [f64; 2]> = self
                    .canvases
                    .get(&self.scope)
                    .map(|canvas| {
                        canvas
                            .nodes
                            .keys()
                            .filter_map(|id| {
                                let size = canvas.current_rect(*id)?.size();
                                Some((*id, [f64::from(size.x), f64::from(size.y)]))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let moves = arrange::arrange(body, [0.0, 0.0], &sizes)
                    .into_iter()
                    .map(|(node, pos)| EditCommand::Move { node, pos })
                    .collect();
                if self.edit(moves, None) {
                    self.canvas_mut().framing = Some(Framing::All);
                }
            }
            GraphRequest::Open(scope) => self.open(scope),
            GraphRequest::Frame { scope, node } => {
                self.open(scope);
                self.canvas_mut().framing = Some(Framing::Node(node));
            }
            GraphRequest::FrameAll => self.canvas_mut().framing = Some(Framing::All),
            GraphRequest::View { node, port } => self.view(node, port),
            GraphRequest::StopViewing => {
                self.set_viewing(None);
                self.status = None;
            }
            GraphRequest::ShowWgsl(show) => self.show_wgsl = show,
        }
    }

    /// Shows the output `port` of `node` of the open pass in the Viewport, when the document allows it.
    fn view(&mut self, node: NodeId, port: String) {
        let Scope::Pass(pass) = &self.scope else {
            return;
        };
        let target = ViewTarget {
            pass: pass.clone(),
            node,
            port,
        };
        match view_file(&self.builder, &self.report, &target) {
            Ok(_) => {
                self.status = None;
                self.set_viewing(Some(target));
            }
            Err(message) => self.status = Some(GraphStatus::warning(message)),
        }
    }

    /// Opens `scope` on the canvas, when the document holds it.
    fn open(&mut self, scope: Scope) {
        if self.body(&scope).is_some() {
            self.scope = scope;
        }
    }

    /// Returns the canvas of the open body, made when first asked for.
    ///
    /// egui-snarl keeps the selection, the transform and the node sizes in egui's memory under the canvas's id. A new
    /// canvas takes an id no canvas had before, so a body opened again after its canvas was dropped, or a pass that
    /// takes a renamed pass's id, starts from a fresh state.
    fn canvas_mut(&mut self) -> &mut BodyCanvas {
        let made = &mut self.canvases_made;
        let load = self.load;
        self.canvases.entry(self.scope.clone()).or_insert_with(|| {
            *made += 1;
            BodyCanvas::new(Id::new("henad_graph_canvas").with(load).with(*made))
        })
    }

    /// Runs `commands` as one edit, and returns whether they ran. A refused command changes nothing and sets the
    /// status to its message.
    pub fn edit(&mut self, commands: Vec<EditCommand>, coalesce: Option<&str>) -> bool {
        if commands.is_empty() {
            return true;
        }
        let mut draft = self.builder.clone();
        let mut renamed = None;
        for command in commands {
            if let EditCommand::RenamePass { from, to } = &command {
                renamed = Some((from.clone(), to.clone()));
            }
            if let Err(error) = draft.apply(command) {
                self.status = Some(GraphStatus::warning(edit_message(&error)));
                self.history.end_gesture();
                self.mark_canvases();
                return false;
            }
        }
        self.history.record(self.text(), coalesce);
        self.builder = draft;
        if let Some((from, to)) = renamed {
            let old = Scope::pass(&from);
            if let Some(canvas) = self.canvases.remove(&old) {
                self.canvases.insert(Scope::pass(&to), canvas);
            }
            if self.scope == old {
                self.scope = Scope::pass(&to);
            }
        }
        self.status = None;
        self.refresh();
        true
    }

    /// Adds a node of kind `op` at `pos`, and wires it to `wire` when the kind has a socket that takes it.
    fn add_node(&mut self, op: &str, pos: [f64; 2], wire: Option<&DroppedWire>) {
        let scope = self.scope.clone();
        let mut draft = self.builder.clone();
        let made = draft.apply(EditCommand::AddNode {
            scope: scope.clone(),
            op: op.to_owned(),
            region: None,
            pos: Some(pos),
        });
        let node = match made.map(|made| made.first().copied()) {
            Ok(Some(node)) => node,
            Ok(None) => return,
            Err(error) => {
                self.status = Some(GraphStatus::warning(edit_message(&error)));
                return;
            }
        };
        let def = lookup(op, draft.document().library);
        let connect = wire.zip(def).and_then(|(wire, def)| {
            let port = wire_port(def, &wire.end)?.to_owned();
            Some(match wire.end {
                WireEnd::Output(_) => EditCommand::Connect {
                    scope: scope.clone(),
                    node,
                    port,
                    source: PortRef {
                        node: wire.node,
                        port: Some(wire.port.clone()),
                    },
                },
                WireEnd::Input(_) => EditCommand::Connect {
                    scope: scope.clone(),
                    node: wire.node,
                    port: wire.port.clone(),
                    source: PortRef { node, port: Some(port) },
                },
            })
        });
        if let Some(connect) = connect
            && let Err(error) = draft.apply(connect)
        {
            self.status = Some(GraphStatus::warning(edit_message(&error)));
        }
        self.history.record(self.text(), None);
        self.builder = draft;
        self.refresh();
    }

    /// Copies `nodes` of the open body to the clipboard, and returns whether it did.
    fn copy(&mut self, ctx: &egui::Context, nodes: &[NodeId]) -> bool {
        if nodes.is_empty() {
            return false;
        }
        match self.builder.copy(&self.scope, nodes) {
            Ok(text) => {
                ctx.copy_text(text);
                self.status = Some(GraphStatus::note(match nodes.len() {
                    1 => "Copied 1 node.".to_owned(),
                    count => format!("Copied {count} nodes."),
                }));
                true
            }
            Err(error) => {
                self.status = Some(GraphStatus::warning(edit_message(&error)));
                false
            }
        }
    }

    /// Removes `nodes` of the open body in one edit.
    fn delete(&mut self, nodes: &[NodeId]) {
        let commands = nodes
            .iter()
            .map(|node| EditCommand::RemoveNode {
                scope: self.scope.clone(),
                node: *node,
            })
            .collect();
        self.edit(commands, None);
    }

    /// Pastes `text` into the open body, with the top left of its nodes at `at` when set.
    fn paste(&mut self, text: &str, at: Option<[f64; 2]>) {
        let offset = at
            .and_then(|at| {
                let file = GraphFile::parse(text).ok()?;
                let editor = file.editor()?;
                let document = file.document();
                let bodies = document
                    .passes
                    .iter()
                    .map(|pass| &pass.body)
                    .chain(document.functions.iter().map(|function| &function.body));
                let corner = bodies
                    .flat_map(|body| body.nodes.iter())
                    .filter_map(|node| editor.node_layout(node.id))
                    .map(|layout| layout.pos)
                    .reduce(|left, right| [left[0].min(right[0]), left[1].min(right[1])])?;
                Some([at[0] - corner[0], at[1] - corner[1]])
            })
            .unwrap_or(PASTE_OFFSET);
        self.edit(
            vec![EditCommand::Paste {
                scope: self.scope.clone(),
                text: text.to_owned(),
                offset,
            }],
            None,
        );
    }

    /// Replaces the document with the one `text` holds, a snapshot of the history.
    fn restore(&mut self, text: &str) {
        match GraphFile::parse(text) {
            Ok(file) => {
                self.builder = GraphBuilder::from_file(&file);
                self.status = None;
                self.refresh();
            }
            Err(error) => self.status = Some(GraphStatus::warning(format!("Undo failed: {error}"))),
        }
    }

    /// Marks every canvas as behind the document, so each is brought to it before it next draws.
    fn mark_canvases(&mut self) {
        for canvas in self.canvases.values_mut() {
            canvas.revision = u64::MAX;
        }
    }

    /// Checks the document again after a change, and opens another body when the open one is gone.
    fn refresh(&mut self) {
        self.revision += 1;
        self.mark_canvases();
        match self.builder.to_file() {
            Ok(file) => {
                self.report = file.check();
                self.read_error = None;
            }
            Err(error) => {
                self.report = CheckReport::default();
                self.read_error = Some(error.diagnostic().clone());
            }
        }
        self.issues.clear();
        for diagnostic in &self.report.diagnostics {
            let (Some(pass), Some(node)) = (&diagnostic.pass, &diagnostic.node) else {
                continue;
            };
            let Ok(node) = node.parse::<NodeId>() else { continue };
            let issues = self.issues.entry(pass.clone()).or_default().entry(node).or_default();
            if diagnostic.severity == DiagnosticSeverity::Error {
                issues.errors.push(diagnostic.message.clone());
            } else {
                issues.warnings.push(diagnostic.message.clone());
            }
        }
        if self.body(&self.scope).is_none() {
            let first = self.document().passes.first().map(|pass| Scope::pass(&pass.id));
            if let Some(first) = first {
                self.scope = first;
            }
        }
        let known: Vec<Scope> = self.canvases.keys().cloned().collect();
        for scope in known {
            if self.body(&scope).is_none() {
                self.canvases.remove(&scope);
            }
        }
    }

    /// Brings the open canvas to the document when it is behind.
    fn sync_open_canvas(&mut self) {
        let revision = self.revision;
        if self.canvas_mut().revision == revision {
            return;
        }
        let scope = self.scope.clone();
        let Some(mut canvas) = self.canvases.remove(&scope) else {
            return;
        };
        if let Some(body) = self.body(&scope) {
            let pass_id = match &scope {
                Scope::Pass(id) => id.as_str(),
                _ => "",
            };
            let empty = BTreeMap::new();
            let issues = self.issues.get(pass_id).unwrap_or(&empty);
            let report = self.report.passes.iter().find(|pass| pass.id == pass_id);
            canvas.sync(
                &scope,
                self.builder.document(),
                self.builder.editor(),
                body,
                report,
                issues,
            );
            canvas.revision = revision;
        }
        self.canvases.insert(scope, canvas);
    }

    /// Runs the requests that wait, and `requests`, once the primary button is up. While it is down they wait.
    ///
    /// A drag that ended commits its moves first, then any wire egui-snarl dropped without a viewer callback.
    ///
    /// A press of the primary button and a change of keyboard focus each end the gesture of the edits before them, and
    /// the edits released by a press make one gesture. Two drags of one field are two undo steps.
    pub fn settle(&mut self, ctx: &egui::Context, requests: Vec<GraphRequest>) {
        self.pending.extend(requests);
        let focused = ctx.memory(|memory| memory.focused());
        if focused != self.focused {
            self.focused = focused;
            self.history.end_gesture();
        }
        if ctx.input(|input| input.pointer.primary_down()) {
            self.pressed = true;
            return;
        }
        let released = std::mem::take(&mut self.pressed);
        if released {
            self.history.end_gesture();
        }
        let mut gesture = Vec::new();
        if let (Some(canvas), Some(body)) = (self.canvases.get(&self.scope), self.body(&self.scope))
            && canvas.revision == self.revision
        {
            gesture.extend(canvas.moved(self.builder.editor()));
            gesture.extend(canvas.lost_wires(&self.scope, body));
        }
        if !gesture.is_empty() {
            self.edit(gesture, None);
        }
        for request in std::mem::take(&mut self.pending) {
            self.run(ctx, request);
        }
        if released {
            self.history.end_gesture();
        }
        self.sync_open_canvas();
    }

    /// Queues `request`, which runs once the canvas has drawn and the primary button is up.
    pub fn request(&mut self, request: GraphRequest) {
        self.pending.push(request);
    }

    /// Returns the commands of the edits that wait for the primary button, the most recent first.
    fn pending_commands(&self) -> impl Iterator<Item = &EditCommand> {
        self.pending
            .iter()
            .rev()
            .filter_map(|request| match request {
                GraphRequest::Edit(edit) => Some(edit.commands.iter().rev()),
                _ => None,
            })
            .flatten()
    }

    /// Returns the world the document will hold once the waiting edits run, as a field dragged now shows it.
    pub fn shown_world(&self) -> World {
        self.pending_commands()
            .find_map(|command| match command {
                EditCommand::SetWorld(world) => Some(*world),
                _ => None,
            })
            .unwrap_or(self.document().world)
    }

    /// Returns the parameter `param` as the document will hold it once the waiting edits run, as a field dragged now
    /// shows it.
    pub fn shown_param<'a>(&'a self, param: &'a ParamDescriptor) -> &'a ParamDescriptor {
        self.pending_commands()
            .find_map(|command| match command {
                EditCommand::SetParam(pending) if pending.id() == param.id() => Some(pending),
                _ => None,
            })
            .unwrap_or(param)
    }
}

/// Returns the message the editor shows for a refused edit, with its first letter in upper case and a full stop.
fn edit_message(error: &EditError) -> String {
    sentence(&error.to_string())
}

/// Returns `text` with its first letter in upper case and a full stop.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut message: String = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    if !message.ends_with('.') {
        message.push('.');
    }
    message
}

/// Places every body of `builder` that holds no placement in layers, and the nodes of other bodies that have none to
/// the right of their placed nodes.
fn place_unplaced(builder: &mut GraphBuilder) {
    let document = builder.document();
    let bodies = document
        .passes
        .iter()
        .map(|pass| &pass.body)
        .chain(document.functions.iter().map(|function| &function.body));
    let mut moves = Vec::new();
    for body in bodies {
        let editor = builder.editor();
        let placed: Vec<[f64; 2]> = body
            .nodes
            .iter()
            .filter_map(|node| editor.node_layout(node.id))
            .map(|layout| layout.pos)
            .collect();
        if placed.len() == body.nodes.len() {
            continue;
        }
        let origin = placed
            .iter()
            .map(|pos| pos[0])
            .reduce(f64::max)
            .map_or([0.0, 0.0], |right| {
                [right + arrange::NODE_SIZE[0] + arrange::COLUMN_GAP, 0.0]
            });
        for (node, pos) in arrange::arrange(body, origin, &BTreeMap::new()) {
            if editor.node_layout(node).is_none() {
                moves.push(EditCommand::Move { node, pos });
            }
        }
    }
    for command in moves {
        // A move of a node the document holds is never refused.
        if let Err(error) = builder.apply(command) {
            log::warn!("placing a node failed: {error}");
        }
    }
}

/// Draws the graph editor: the passes and the world beside the canvas, and the canvas's toolbar.
pub fn graph_ui(ui: &mut egui::Ui, graph: &mut GraphEditor) {
    let mut requests = Vec::new();
    Panel::left("henad_graph_sidebar")
        .resizable(true)
        .default_size(SIDEBAR_WIDTH)
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("henad_graph_sidebar_scroll")
                .show(ui, |ui| {
                    passes::passes_ui(ui, graph, &mut requests);
                    ui.add_space(8.0);
                    world::world_ui(ui, graph, &mut requests);
                });
        });
    Panel::top("henad_graph_toolbar")
        .resizable(false)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(4, 4)))
        .show(ui, |ui| toolbar_ui(ui, graph, &mut requests));
    if graph.show_wgsl {
        Panel::right("henad_graph_wgsl")
            .resizable(true)
            .default_size(WGSL_WIDTH.min(ui.available_width() / 2.0))
            .show(ui, |ui| wgsl_view::wgsl_ui(ui, graph));
    }
    CentralPanel::no_frame().show(ui, |ui| {
        if graph.body(&graph.scope).is_none() {
            ui.weak("The model has no pass. Add a pass in the Passes panel.");
            return;
        }
        let pointer_on_canvas = graph
            .canvases
            .get(&graph.scope)
            .is_some_and(|canvas| ui.rect_contains_pointer(canvas.canvas_rect));
        if pointer_on_canvas {
            requests.extend(keyboard_requests(ui.ctx(), graph));
        }
        if !ui.ctx().input(|input| input.pointer.primary_down()) {
            graph.sync_open_canvas();
        }
        let scope = graph.scope.clone();
        graph.canvas_mut();
        let Some(mut canvas) = graph.canvases.remove(&scope) else {
            return;
        };
        let document = graph.builder.document();
        let pass = match &scope {
            Scope::Pass(id) => document.passes.iter().find(|pass| pass.id == *id),
            _ => None,
        };
        let context = CanvasContext {
            scope: &scope,
            pass,
            document,
        };
        requests.extend(show_canvas(ui, &mut canvas, context));
        graph.canvases.insert(scope, canvas);
    });
    graph.settle(ui.ctx(), requests);
}

/// Returns the requests of the keys pressed over the canvas: undo, redo, delete, copy, cut and paste.
///
/// Keys reach the canvas only while no text field has keyboard focus.
fn keyboard_requests(ctx: &egui::Context, graph: &GraphEditor) -> Vec<GraphRequest> {
    if ctx.memory(|memory| memory.focused().is_some()) {
        return Vec::new();
    }
    let selected = graph
        .canvases
        .get(&graph.scope)
        .map(|canvas| canvas.selected(ctx))
        .unwrap_or_default();
    let pointer = graph.canvases.get(&graph.scope).and_then(|canvas| {
        let screen = ctx.input(|input| input.pointer.hover_pos())?;
        let graph_pos = canvas.transform.inverse() * screen;
        Some([f64::from(graph_pos.x), f64::from(graph_pos.y)])
    });
    let mut requests = Vec::new();
    ctx.input_mut(|input| {
        if input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
            || input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
        {
            requests.push(GraphRequest::Redo);
        }
        if input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)) {
            requests.push(GraphRequest::Undo);
        }
        if !selected.is_empty()
            && (input.consume_key(Modifiers::NONE, Key::Delete) || input.consume_key(Modifiers::NONE, Key::Backspace))
        {
            requests.push(GraphRequest::Delete(selected.clone()));
        }
        for event in &input.events {
            match event {
                Event::Copy if !selected.is_empty() => requests.push(GraphRequest::Copy(selected.clone())),
                Event::Cut if !selected.is_empty() => requests.push(GraphRequest::Cut(selected.clone())),
                Event::Paste(text) => requests.push(GraphRequest::Paste {
                    text: text.clone(),
                    at: pointer,
                }),
                _ => {}
            }
        }
    });
    requests
}

/// Draws the toolbar above the canvas: the open body, Undo, Redo, Copy, Delete, Arrange, Frame all, the WGSL switch,
/// and the output shown in the Viewport with Stop viewing.
fn toolbar_ui(ui: &mut egui::Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    let selected = graph
        .canvases
        .get(&graph.scope)
        .map(|canvas| canvas.selected(ui.ctx()))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        if let Some(pass) = graph.open_pass() {
            ui.strong(&pass.id);
            ui.weak(match pass.stage {
                PassStage::Init => "init pass",
                PassStage::Tick => "tick pass",
                PassStage::View => "view pass",
                PassStage::Stats => "stats pass",
                PassStage::Action => "action pass",
                _ => "pass",
            });
            ui.separator();
        }
        let buttons = [
            (
                MDI_UNDO,
                "Undo",
                graph.history.can_undo(),
                "Undo last edit",
                GraphRequest::Undo,
            ),
            (
                MDI_REDO,
                "Redo",
                graph.history.can_redo(),
                "Redo last undone edit",
                GraphRequest::Redo,
            ),
            (
                MDI_CONTENT_COPY,
                "Copy",
                !selected.is_empty(),
                "Copy selected nodes as model-file text",
                GraphRequest::Copy(selected.clone()),
            ),
            (
                MDI_DELETE_OUTLINE,
                "Delete",
                !selected.is_empty(),
                "Delete selected nodes",
                GraphRequest::Delete(selected.clone()),
            ),
            (
                MDI_SITEMAP_OUTLINE,
                "Arrange",
                true,
                "Place nodes in columns from left to right",
                GraphRequest::Arrange,
            ),
            (
                MDI_FIT_TO_SCREEN_OUTLINE,
                "Frame all",
                true,
                "Bring every node into view",
                GraphRequest::FrameAll,
            ),
        ];
        for (icon, name, enabled, tooltip, request) in buttons {
            let button = add_button(ui, enabled, Button::new(icon), name).on_hover_text(tooltip);
            if button.clicked() {
                requests.push(request);
            }
        }
        ui.separator();
        let wgsl = ui
            .selectable_label(graph.show_wgsl, format!("{MDI_CODE_BRACES} WGSL"))
            .on_hover_text("Show WGSL generated for open pass");
        if wgsl.clicked() {
            requests.push(GraphRequest::ShowWgsl(!graph.show_wgsl));
        }
        if let Some(target) = graph.viewing() {
            ui.separator();
            ui.label(format!(
                "{MDI_EYE_OUTLINE} Viewing {} of pass {}",
                target.name(),
                target.pass
            ));
            let stop = add_button(ui, true, Button::new(MDI_EYE_OFF_OUTLINE), "Stop viewing")
                .on_hover_text("Show model's own view in viewport");
            if stop.clicked() {
                requests.push(GraphRequest::StopViewing);
            }
        }
    });
}
