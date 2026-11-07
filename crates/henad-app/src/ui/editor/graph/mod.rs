//! The graph editor: a graph model's document, edited through henad-graph's commands on a canvas and in forms.
//!
//! A *body* is the nodes of one pass, shown on a canvas of its own. Every gesture becomes a [`GraphRequest`]. The
//! requests of a frame wait while the primary button is down and run after the canvas has drawn, since egui-snarl
//! panics when a node under a wire in flight leaves its graph. An edit runs on a copy of the document, so a command
//! that is refused changes nothing, and its message shows under the canvas.
//!
//! Undo keeps the canonical text of the file before each edit, `[editor]` included. The *revision* counts the
//! document's changes, and a canvas whose revision is behind is brought to the document before it draws. The *code
//! revision* counts the changes of the code alone, which a move of a node leaves alone. The compiled program of the
//! latest code revision is kept for the generated WGSL view and the preview.

pub mod arrange;
pub mod canvas;
pub mod functions;
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
use henad_compute::entry::ModelEntry;
use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};
use henad_core::params::ParamDescriptor;
use henad_graph::GraphFile;
use henad_graph::catalogue::lookup;
use henad_graph::check::CheckReport;
use henad_graph::document::{GraphDocument, Pass, PassStage, World};
use henad_graph::edit::{EditCommand, EditError, GraphBuilder, PortSide, Scope};
use henad_graph::ids::NodeId;
use henad_graph::limits::MAX_REPEAT_ITERATIONS;
use henad_graph::node::{NodeBody, OP_FUNCTION_INPUT, OP_FUNCTION_OUTPUT, OP_REGION_REPEAT};
use henad_graph::types::ValueType;
use henad_graph::value::{ConfigValue, Literal, PortRef};
use henad_graph::wgsl::{AgentProgram, GeneratedShader, GridProgram};

use self::arrange::CanvasKey;
use self::canvas::{BodyCanvas, CanvasContext, Framing, can_add, show_canvas};
use self::history::History;
use self::quick_add::{WireEnd, wire_port};
use self::view_pass::{ViewTarget, view_file};
use crate::icons::material_design_icons::{
    MDI_CHEVRON_RIGHT, MDI_CODE_BRACES, MDI_CONTENT_COPY, MDI_DELETE_OUTLINE, MDI_EYE_OFF_OUTLINE, MDI_EYE_OUTLINE,
    MDI_FIT_TO_SCREEN_OUTLINE, MDI_FUNCTION_VARIANT, MDI_REDO, MDI_REPEAT, MDI_SITEMAP_OUTLINE, MDI_UNDO,
};
use crate::ui::sweep::layout::add_button;

/// Default width in points of the side panel that holds the passes and the world.
const SIDEBAR_WIDTH: f32 = 220.0;

/// Default width in points of the side panel that holds the generated WGSL.
const WGSL_WIDTH: f32 = 420.0;

/// Distance in canvas points a paste moves its nodes when the pointer is off the canvas.
const PASTE_OFFSET: [f64; 2] = [40.0, 40.0];

/// Number of graphs and sheets loaded so far. It keeps the egui state of each graph's canvases apart, and gives each
/// document a preview key of its own.
pub(crate) static LOADS: AtomicU64 = AtomicU64::new(0);

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
        /// Region the node joins, `None` for the top of the body.
        region: Option<NodeId>,
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
    /// Wraps nodes of the open pass in a new Repeat, placed around them as the canvas draws them.
    WrapInRepeat(Vec<NodeId>),
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
    /// Opens the function a call of the open body names, with the open body as its parent in the breadcrumb.
    OpenCall(NodeId),
    /// Opens the body at this position of the breadcrumb, and drops the bodies after it.
    OpenParent(usize),
    /// Moves nodes of the open body into a new function, with a call in their place.
    MakeFunction(Vec<NodeId>),
    /// Replaces a call of the open body with a copy of its function's body.
    Ungroup(NodeId),
    /// Adds a port to one side of a function, and a node of its body that names it.
    AddFunctionPort {
        /// Id of the function.
        function: String,
        /// Side of the port.
        side: PortSide,
    },
    /// Opens a dialog that picks a model file to import a function from.
    PickImport,
    /// Copies the function `function` of the picked file into the document, under `id` or its own id.
    Import {
        /// Id of the function in the picked file.
        function: String,
        /// Id in this document, `None` to keep its own.
        id: Option<String>,
    },
    /// Forgets the picked file.
    CloseImport,
    /// Shows a refused gesture's message in the status line.
    Refuse(String),
}

/// A model file picked to import functions from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSource {
    /// Name of the file.
    pub name: String,
    /// Text of the file.
    pub text: String,
    /// Ids of the file's functions, in file order.
    pub functions: Vec<String>,
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

/// A Repeat that a wrap made, whose halves move clear of the wrapped nodes once the canvas has drawn them.
#[derive(Debug, Clone)]
struct WrapFit {
    /// Body of the Repeat.
    scope: Scope,
    /// The Repeat.
    repeat: NodeId,
    /// Puts the wrap made.
    puts: Vec<NodeId>,
    /// Canvas rectangle of the wrapped nodes.
    bounds: egui::Rect,
    /// Sizes of the input half and the puts at the last frame, which have to hold for two frames.
    sizes: Vec<egui::Vec2>,
}

/// Compiled program of a graph model of either topology.
#[derive(Clone)]
pub enum GraphProgram {
    /// A model on a grid.
    Grid(GridProgram),
    /// A model of agents.
    Agents(AgentProgram),
}

/// Prints the model's id and the topology.
impl std::fmt::Debug for GraphProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (topology, id) = match self {
            Self::Grid(program) => ("grid", program.id()),
            Self::Agents(program) => ("agents", program.id()),
        };
        f.debug_struct("GraphProgram")
            .field("topology", &topology)
            .field("id", &id)
            .finish_non_exhaustive()
    }
}

impl GraphProgram {
    /// Compiles `file` through the compiler of its topology.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the file compiling.
    pub fn compile(file: &GraphFile) -> CompiledProgram {
        match file.document().world {
            World::Grid(_) => GridProgram::compile(file).map(Self::Grid),
            World::Agents(_) => AgentProgram::compile(file).map(Self::Agents),
            _ => Err(vec![CodeDiagnostic::error(
                "The Editor runs graphs on a grid or of agents.",
            )]),
        }
    }

    /// Generated shaders, in the order the engine runs them.
    pub fn shaders(&self) -> &[GeneratedShader] {
        match self {
            Self::Grid(program) => program.shaders(),
            Self::Agents(program) => program.shaders(),
        }
    }

    /// Returns the model entry that builds the program.
    pub fn entry(&self) -> ModelEntry {
        match self {
            Self::Grid(program) => program.entry(),
            Self::Agents(program) => program.entry(),
        }
    }
}

/// Program of one revision of the document: the compiled model, or the problems that stop it compiling.
pub type CompiledProgram = Result<GraphProgram, Vec<CodeDiagnostic>>;

/// A graph model under edit.
pub struct GraphEditor {
    builder: GraphBuilder,
    history: History,
    /// Body on the canvas.
    scope: Scope,
    /// Bodies the open function was entered from through its calls, outermost first, which the breadcrumb shows.
    trail: Vec<Scope>,
    /// Canvas of each body opened so far.
    canvases: BTreeMap<Scope, BodyCanvas>,
    /// Report of the last check.
    report: CheckReport,
    /// Problem of a document whose canonical text does not read back, which no command should make.
    read_error: Option<CodeDiagnostic>,
    /// Problems of each body's nodes, by body.
    issues: BTreeMap<Scope, BTreeMap<NodeId, NodeIssues>>,
    /// Number of changes of the document so far.
    revision: u64,
    /// Number of changes of the document's code so far, which placement leaves alone.
    code_revision: u64,
    /// Hash of the document's code at the last check, `None` while the document does not read back.
    code_hash: Option<u64>,
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
    /// Program of the document, with the code revision it was compiled at.
    program: Option<(u64, CompiledProgram)>,
    /// Output shown in the Viewport in place of the model's view passes.
    viewing: Option<ViewTarget>,
    /// Number of changes of [`Self::viewing`] so far.
    view_changes: u64,
    /// Whether the generated WGSL shows beside the canvas.
    pub show_wgsl: bool,
    /// Line about the last refused edit or the last copy, `None` once another edit runs.
    pub status: Option<GraphStatus>,
    /// Repeat of the last wrap, until its halves sit clear of the wrapped nodes.
    wrap_fit: Option<WrapFit>,
    /// Model file picked to import functions from.
    import: Option<ImportSource>,
    /// Whether a dialog that picks a file to import from waits to open.
    pick_import: bool,
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
            trail: Vec::new(),
            canvases: BTreeMap::new(),
            report: CheckReport::default(),
            read_error: None,
            issues: BTreeMap::new(),
            revision: 0,
            code_revision: 0,
            code_hash: None,
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
            wrap_fit: None,
            import: None,
            pick_import: false,
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

    /// Editor region of the document under edit.
    pub fn editor_region(&self) -> &henad_graph::editor::EditorRegion {
        self.builder.editor()
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

    /// Returns the key of what the preview shows: the load, the code revision and the changes of the viewed output. A
    /// preview built under another key is out of date, and a move of a node keeps the key.
    pub fn preview_key(&self) -> [u64; 3] {
        [self.load, self.code_revision, self.view_changes]
    }

    /// Output shown in the Viewport in place of the model's view passes.
    pub fn viewing(&self) -> Option<&ViewTarget> {
        self.viewing.as_ref()
    }

    /// Returns the program of the document as it is now, compiled on first use after each change.
    pub fn program(&mut self) -> &CompiledProgram {
        let revision = self.code_revision;
        let stale = self.program.as_ref().is_none_or(|(compiled, _)| *compiled != revision);
        if stale {
            let program = self
                .builder
                .to_file()
                .map_err(|error| vec![error.diagnostic().clone()])
                .and_then(|file| GraphProgram::compile(&file));
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
                Ok(file) => return GraphProgram::compile(&file),
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

    /// Returns the number of errors and warnings in the body `scope`.
    pub fn issue_counts(&self, scope: &Scope) -> (usize, usize) {
        self.report
            .diagnostics
            .iter()
            .filter(|diagnostic| self.diagnostic_scope(diagnostic).as_ref() == Some(scope))
            .fold((0, 0), |(errors, warnings), diagnostic| {
                if diagnostic.severity == DiagnosticSeverity::Error {
                    (errors + 1, warnings)
                } else {
                    (errors, warnings + 1)
                }
            })
    }

    /// Returns the body a diagnostic is in: its pass, or the function that holds its node.
    pub fn diagnostic_scope(&self, diagnostic: &CodeDiagnostic) -> Option<Scope> {
        if let Some(pass) = &diagnostic.pass {
            return Some(Scope::pass(pass));
        }
        let node: NodeId = diagnostic.node.as_ref()?.parse().ok()?;
        self.document()
            .functions
            .iter()
            .find(|function| function.body.node(node).is_some())
            .map(|function| Scope::Function(function.id.clone()))
    }

    /// Bodies the open function was entered from, outermost first.
    pub fn trail(&self) -> &[Scope] {
        &self.trail
    }

    /// Model file picked to import functions from.
    pub fn import_source(&self) -> Option<&ImportSource> {
        self.import.as_ref()
    }

    /// Returns whether a dialog that picks a file to import from waits to open, and clears the wait.
    pub fn take_pick_import(&mut self) -> bool {
        std::mem::take(&mut self.pick_import)
    }

    /// Takes the model file `text`, named `name`, as the file to import functions from, or names the reason it cannot.
    pub fn set_import_source(&mut self, name: String, text: String) {
        match GraphFile::parse(&text) {
            Ok(file) if file.document().functions.is_empty() => {
                self.status = Some(GraphStatus::warning(format!("{name} holds no function.")));
            }
            Ok(file) => {
                let functions = file
                    .document()
                    .functions
                    .iter()
                    .map(|function| function.id.clone())
                    .collect();
                self.status = None;
                self.import = Some(ImportSource { name, text, functions });
            }
            Err(error) => {
                self.status = Some(GraphStatus::warning(format!("{name} is no graph model. {error}")));
            }
        }
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

            GraphRequest::AddNode { op, pos, region, wire } => self.add_node(&op, pos, region, wire.as_ref()),
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
            GraphRequest::WrapInRepeat(nodes) => self.wrap(nodes),
            GraphRequest::Arrange => {
                let Some(body) = self.body(&self.scope) else { return };
                let sizes: BTreeMap<CanvasKey, [f64; 2]> = self
                    .canvases
                    .get(&self.scope)
                    .map(|canvas| {
                        let mains = canvas
                            .nodes
                            .keys()
                            .map(|id| (CanvasKey::main(*id), canvas.current_rect(*id)));
                        let outputs = canvas
                            .output_halves
                            .keys()
                            .map(|id| (CanvasKey::output(*id), canvas.output_half_rect(*id)));
                        mains
                            .chain(outputs)
                            .filter_map(|(key, rect)| {
                                let size = rect?.size();
                                Some((key, [f64::from(size.x), f64::from(size.y)]))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let library = self.document().library;
                let moves = arrange::arrange(body, library, [0.0, 0.0], &sizes)
                    .into_iter()
                    .map(|(key, pos)| place_command(key, pos))
                    .collect();
                if self.edit(moves, None) {
                    self.canvas_mut().framing = Some(Framing::All);
                }
            }
            GraphRequest::Open(scope) => {
                self.trail.clear();
                self.open(scope);
            }
            GraphRequest::OpenCall(node) => self.open_call(node),
            GraphRequest::OpenParent(index) => {
                if let Some(scope) = self.trail.get(index).cloned() {
                    self.trail.truncate(index);
                    self.open(scope);
                }
            }
            GraphRequest::MakeFunction(nodes) => self.make_function(nodes),
            GraphRequest::Ungroup(node) => {
                let command = EditCommand::Ungroup {
                    scope: self.scope.clone(),
                    node,
                };
                self.edit(vec![command], None);
            }
            GraphRequest::AddFunctionPort { function, side } => self.add_function_port(&function, side),
            GraphRequest::PickImport => self.pick_import = true,
            GraphRequest::Import { function, id } => self.import_function(function, id),
            GraphRequest::CloseImport => self.import = None,
            GraphRequest::Refuse(message) => self.status = Some(GraphStatus::warning(message)),
            GraphRequest::Frame { scope, node } => {
                if scope != self.scope {
                    self.trail.clear();
                }
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

    /// Wraps `nodes` of the open pass in a new Repeat, placed around them as the canvas draws them.
    fn wrap(&mut self, nodes: Vec<NodeId>) {
        let bounds = self
            .canvases
            .get(&self.scope)
            .and_then(|canvas| canvas.bounds_of(&nodes));
        let scope = self.scope.clone();
        let command = EditCommand::WrapInRepeat {
            scope: scope.clone(),
            nodes,
            bounds,
        };
        let mut draft = self.builder.clone();
        let made = match draft.apply(command) {
            Ok(made) => made,
            Err(error) => {
                self.status = Some(GraphStatus::warning(edit_message(&error)));
                self.history.end_gesture();
                self.mark_canvases();
                return;
            }
        };
        place_unplaced(&mut draft);
        self.history.record(self.text(), None);
        self.builder = draft;
        self.status = None;
        self.refresh();
        if let (Some(bounds), [repeat, puts @ ..]) = (bounds, made.as_slice()) {
            let [min, max] = bounds.map(|corner| egui::pos2(corner[0] as f32, corner[1] as f32));
            self.wrap_fit = Some(WrapFit {
                scope,
                repeat: *repeat,
                puts: puts.to_vec(),
                bounds: egui::Rect::from_min_max(min, max),
                sizes: Vec::new(),
            });
        }
    }

    /// Moves the halves of the last wrap's Repeat clear of the wrapped nodes and the puts, once the canvas has drawn
    /// them at one size for two frames. The moves join the wrap's undo step.
    fn fit_wrap(&mut self) {
        let Some(mut fit) = self.wrap_fit.take() else { return };
        if fit.scope != self.scope {
            return;
        }
        let Some(canvas) = self.canvases.get(&fit.scope) else {
            return;
        };
        let rects: Option<Vec<egui::Rect>> = std::iter::once(canvas.drawn_rect(fit.repeat))
            .chain(fit.puts.iter().map(|put| canvas.drawn_rect(*put)))
            .chain(std::iter::once(canvas.drawn_output_half_rect(fit.repeat)))
            .collect();
        let Some(rects) = rects else {
            self.wrap_fit = Some(fit);
            return;
        };
        let sizes: Vec<egui::Vec2> = rects.iter().map(|rect| rect.size()).collect();
        if sizes != fit.sizes {
            fit.sizes = sizes;
            self.wrap_fit = Some(fit);
            return;
        }
        let gap = arrange::COLUMN_GAP as f32;
        let mut moves = Vec::new();
        if let Some(input) = rects.first()
            && input.max.x > fit.bounds.min.x - gap
        {
            let pos = input.min - egui::vec2(input.max.x - (fit.bounds.min.x - gap), 0.0);
            moves.push(EditCommand::Move {
                node: fit.repeat,
                pos: [f64::from(pos.x), f64::from(pos.y)],
            });
        }
        let right = rects[1..rects.len() - 1]
            .iter()
            .map(|rect| rect.max.x)
            .fold(fit.bounds.max.x, f32::max);
        if let Some(output) = rects.last()
            && output.min.x < right + gap
        {
            moves.push(EditCommand::MoveRegionOutput {
                node: fit.repeat,
                pos: [f64::from(right + gap), f64::from(output.min.y)],
            });
        }
        if moves.is_empty() {
            return;
        }
        let mut draft = self.builder.clone();
        if moves.into_iter().all(|command| draft.apply(command).is_ok()) {
            self.builder = draft;
            self.refresh();
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

    /// Opens the function the call `node` of the open body names, with the open body last in the breadcrumb.
    fn open_call(&mut self, node: NodeId) {
        let function = self
            .body(&self.scope)
            .and_then(|body| body.node(node))
            .and_then(|call| call.config.get("function"))
            .and_then(ConfigValue::as_str)
            .map(|id| Scope::Function(id.to_owned()));
        let Some(function) = function.filter(|function| self.body(function).is_some()) else {
            self.status = Some(GraphStatus::warning(format!(
                "Node {node} calls no function the file declares."
            )));
            return;
        };
        let parent = self.scope.clone();
        self.trail.push(parent);
        self.open(function);
    }

    /// Moves `nodes` of the open body into a new function named `function`, or `function_<n>` when that is taken.
    fn make_function(&mut self, nodes: Vec<NodeId>) {
        let taken = |id: &str| self.document().functions.iter().any(|function| function.id == id);
        let id = std::iter::once("function".to_owned())
            .chain((2..).map(|number| format!("function_{number}")))
            .find(|id| !taken(id))
            .unwrap_or_default();
        let command = EditCommand::MakeFunction {
            scope: self.scope.clone(),
            nodes,
            id: id.clone(),
        };
        if self.edit(vec![command], None) {
            self.status = Some(GraphStatus::note(format!(
                "Made function '{id}'. Rename it in the Functions panel."
            )));
        }
    }

    /// Adds a port to `side` of the function `function`, with an unused name and type `f32`, and a node of its body
    /// that names it.
    fn add_function_port(&mut self, function: &str, side: PortSide) {
        let Some(declared) = self.document().functions.iter().find(|known| known.id == function) else {
            return;
        };
        let (ports, word, op) = match side {
            PortSide::Input => (&declared.inputs, "input", OP_FUNCTION_INPUT),
            _ => (&declared.outputs, "output", OP_FUNCTION_OUTPUT),
        };
        let taken = |name: &str| ports.iter().any(|port| port.port == name);
        let port = std::iter::once(word.to_owned())
            .chain((2..).map(|number| format!("{word}_{number}")))
            .find(|name| !taken(name))
            .unwrap_or_default();
        let scope = Scope::Function(function.to_owned());
        let mut draft = self.builder.clone();
        let added = draft
            .apply(EditCommand::AddFunctionPort {
                function: function.to_owned(),
                side,
                port: port.clone(),
                value_type: ValueType::F32,
            })
            .and_then(|_| draft.add_node(&scope, op, &[("port", ConfigValue::String(port))], &[]));
        match added {
            Ok(_) => {
                place_unplaced(&mut draft);
                self.history.record(self.text(), None);
                self.builder = draft;
                self.status = None;
                self.refresh();
            }
            Err(error) => self.status = Some(GraphStatus::warning(edit_message(&error))),
        }
    }

    /// Copies `function` of the picked file into the document, under `id` or its own id.
    fn import_function(&mut self, function: String, id: Option<String>) {
        let Some(source) = self.import.clone() else { return };
        let command = EditCommand::ImportFunction {
            text: source.text,
            source: source.name.clone(),
            function: function.clone(),
            id: id.clone(),
        };
        if self.edit(vec![command], None) {
            let id = id.unwrap_or(function);
            self.status = Some(GraphStatus::note(format!(
                "Imported function '{id}' from {}.",
                source.name
            )));
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
    ///
    /// A constant `count` of a Repeat also sets its `max`, clamped to the limit. A node the commands leave with no
    /// placement, such as one pasted or imported from a file with none, is placed as at load. Otherwise the canvas
    /// would report the place it draws the node at as a move, an edit of its own.
    pub fn edit(&mut self, commands: Vec<EditCommand>, coalesce: Option<&str>) -> bool {
        if commands.is_empty() {
            return true;
        }
        let commands = fill_repeat_max(commands, self.document());
        let mut draft = self.builder.clone();
        let mut renamed = Vec::new();
        for command in commands {
            match &command {
                EditCommand::RenamePass { from, to } => renamed.push((Scope::pass(from), Scope::pass(to))),
                EditCommand::RenameFunction { from, to } => {
                    renamed.push((Scope::Function(from.clone()), Scope::Function(to.clone())));
                }
                _ => {}
            }
            if let Err(error) = draft.apply(command) {
                self.status = Some(GraphStatus::warning(edit_message(&error)));
                self.history.end_gesture();
                self.mark_canvases();
                return false;
            }
        }
        place_unplaced(&mut draft);
        self.history.record(self.text(), coalesce);
        self.builder = draft;
        for (old, new) in renamed {
            if let Some(canvas) = self.canvases.remove(&old) {
                self.canvases.insert(new.clone(), canvas);
            }
            if self.scope == old {
                self.scope = new.clone();
            }
            for scope in &mut self.trail {
                if *scope == old {
                    *scope = new.clone();
                }
            }
        }
        self.status = None;
        self.refresh();
        true
    }

    /// Adds a node of kind `op` at `pos` in `region`, and wires it to `wire` when the kind has a socket that takes it.
    ///
    /// A new Repeat runs once, with no accumulator. A region kind that cannot nest in `region` is refused, by the rules
    /// of [`canvas::can_join`].
    fn add_node(&mut self, op: &str, pos: [f64; 2], region: Option<NodeId>, wire: Option<&DroppedWire>) {
        let scope = self.scope.clone();
        if let Some(region) = region
            && self.body(&scope).is_some_and(|body| !can_add(body, op, Some(region)))
        {
            self.status = Some(GraphStatus::warning(format!(
                "Region {region} cannot hold a node of kind {op}."
            )));
            return;
        }
        let mut draft = self.builder.clone();
        let made = draft.apply(EditCommand::AddNode {
            scope: scope.clone(),
            op: op.to_owned(),
            region,
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
        if op == OP_REGION_REPEAT {
            let defaults = [
                EditCommand::SetConfig {
                    scope: scope.clone(),
                    node,
                    key: "max".to_owned(),
                    value: Some(ConfigValue::Integer(1)),
                },
                EditCommand::SetConfig {
                    scope: scope.clone(),
                    node,
                    key: "acc".to_owned(),
                    value: Some(ConfigValue::Array(Vec::new())),
                },
                EditCommand::SetLiteral {
                    scope: scope.clone(),
                    node,
                    port: "count".to_owned(),
                    literal: Literal::U32(1),
                },
            ];
            for command in defaults {
                if let Err(error) = draft.apply(command) {
                    self.status = Some(GraphStatus::warning(edit_message(&error)));
                }
            }
        }
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
        place_unplaced(&mut draft);
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

    /// Replaces the document with the one `text` holds, a snapshot of the history, and places any node it leaves
    /// unplaced.
    fn restore(&mut self, text: &str) {
        match GraphFile::parse(text) {
            Ok(file) => {
                self.builder = GraphBuilder::from_file(&file);
                place_unplaced(&mut self.builder);
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
                let hash = file.code().hash();
                if self.code_hash != Some(hash) {
                    self.code_hash = Some(hash);
                    self.code_revision += 1;
                }
                self.report = file.check();
                self.read_error = None;
            }
            Err(error) => {
                self.code_hash = None;
                self.code_revision += 1;
                self.report = CheckReport::default();
                self.read_error = Some(error.diagnostic().clone());
            }
        }
        self.issues.clear();
        for diagnostic in &self.report.diagnostics {
            let (Some(scope), Some(node)) = (self.diagnostic_scope(diagnostic), &diagnostic.node) else {
                continue;
            };
            let Ok(node) = node.parse::<NodeId>() else { continue };
            let issues = self.issues.entry(scope).or_default().entry(node).or_default();
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
            self.trail.clear();
        }
        let trail = std::mem::take(&mut self.trail);
        self.trail = trail.into_iter().filter(|scope| self.body(scope).is_some()).collect();
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
            let empty = BTreeMap::new();
            let issues = self.issues.get(&scope).unwrap_or(&empty);
            let report = match &scope {
                Scope::Pass(id) => self.report.passes.iter().find(|pass| pass.id == *id),
                Scope::Function(id) => self.report.functions.iter().find(|function| function.id == *id),
                _ => None,
            };
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
            gesture.extend(
                canvas
                    .regions_entered(body)
                    .into_iter()
                    .map(|(node, region, _)| EditCommand::SetRegion {
                        scope: self.scope.clone(),
                        node,
                        region: Some(region),
                    }),
            );
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
        self.fit_wrap();
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

/// Returns the command that places the canvas node `key` at `pos`: a move of a node, or of a region's output half.
fn place_command(key: CanvasKey, pos: [f64; 2]) -> EditCommand {
    if key.output_half {
        EditCommand::MoveRegionOutput { node: key.node, pos }
    } else {
        EditCommand::Move { node: key.node, pos }
    }
}

/// Returns `commands` with a `max` after each constant `count` of a Repeat of `document`, the count clamped to
/// 1 to [`MAX_REPEAT_ITERATIONS`].
fn fill_repeat_max(commands: Vec<EditCommand>, document: &GraphDocument) -> Vec<EditCommand> {
    let is_repeat = |scope: &Scope, node: NodeId| {
        let body = match scope {
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
        };
        body.and_then(|body| body.node(node))
            .is_some_and(|known| known.op == OP_REGION_REPEAT)
    };
    let mut filled = Vec::with_capacity(commands.len());
    for command in commands {
        let max = match &command {
            EditCommand::SetLiteral {
                scope,
                node,
                port,
                literal: Literal::U32(count),
            } if port == "count" && is_repeat(scope, *node) => Some(EditCommand::SetConfig {
                scope: scope.clone(),
                node: *node,
                key: "max".to_owned(),
                value: Some(ConfigValue::Integer(i64::from(
                    (*count).clamp(1, MAX_REPEAT_ITERATIONS),
                ))),
            }),
            _ => None,
        };
        filled.push(command);
        filled.extend(max);
    }
    filled
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
        // An output half takes the layout's place only in a body placed from scratch. Otherwise it follows its body.
        let placed_none = placed.is_empty();
        let origin = placed
            .iter()
            .map(|pos| pos[0])
            .reduce(f64::max)
            .map_or([0.0, 0.0], |right| {
                [right + arrange::NODE_SIZE[0] + arrange::COLUMN_GAP, 0.0]
            });
        for (key, pos) in arrange::arrange(body, document.library, origin, &BTreeMap::new()) {
            let layout = editor.node_layout(key.node);
            let placed = if key.output_half {
                layout.is_some_and(|layout| layout.output_pos.is_some())
            } else {
                layout.is_some()
            };
            if !placed && (!key.output_half || placed_none) {
                moves.push(place_command(key, pos));
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
                    functions::functions_ui(ui, graph, &mut requests);
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
        if let Some(body) = graph.body(&scope) {
            let context = CanvasContext {
                scope: &scope,
                body,
                pass,
                document,
            };
            requests.extend(show_canvas(ui, &mut canvas, context));
        }
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

/// Draws the breadcrumb of the open body: the bodies it was entered from, each of which opens again, then its own
/// name and kind.
fn breadcrumb_ui(ui: &mut egui::Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    for (index, scope) in graph.trail().iter().enumerate() {
        let name = match scope {
            Scope::Pass(id) | Scope::Function(id) => id.as_str(),
            _ => "",
        };
        if ui.link(name).on_hover_text("Open this body").clicked() {
            requests.push(GraphRequest::OpenParent(index));
        }
        ui.weak(MDI_CHEVRON_RIGHT);
    }
    match graph.scope() {
        Scope::Function(id) => {
            ui.strong(id);
            ui.weak("function");
        }
        _ => {
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
            }
        }
    }
}

/// Draws the toolbar above the canvas: the breadcrumb of the open body, Undo, Redo, Copy, Delete, Repeat, Function,
/// Arrange, Frame all, the WGSL switch, and the output shown in the Viewport with Stop viewing.
fn toolbar_ui(ui: &mut egui::Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    let selected = graph
        .canvases
        .get(&graph.scope)
        .map(|canvas| canvas.selected(ui.ctx()))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        breadcrumb_ui(ui, graph, requests);
        ui.separator();
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
                MDI_REPEAT,
                "Repeat",
                !selected.is_empty(),
                "Wrap selected nodes in Repeat",
                GraphRequest::WrapInRepeat(selected.clone()),
            ),
            (
                MDI_FUNCTION_VARIANT,
                "Function",
                !selected.is_empty(),
                "Move selected nodes into new function",
                GraphRequest::MakeFunction(selected.clone()),
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
