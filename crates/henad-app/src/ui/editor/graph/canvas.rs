//! Canvas of one body: a persistent egui-snarl [`Snarl`] kept in step with the body of the document, and its viewer.
//!
//! The document is the truth. Each gesture on the canvas becomes a [`GraphRequest`], which the editor runs as
//! henad-graph edit commands once the primary button is up and the canvas has drawn. [`BodyCanvas::sync`] then brings
//! the `Snarl` to the document by difference: nodes that left are removed, new nodes inserted, and the rest updated in
//! place, so egui-snarl's selection and sizes stay with their nodes.
//!
//! A *pin* is a socket as egui-snarl numbers it, by position. A node's pins follow the order of its kind's resolved
//! ports, and the views below map each pin back to a port name.
//!
//! A region draws as two canvas nodes with a frame behind its body: the *input half* carries the region's inputs and
//! its inner outputs, and the *output half* its accumulator outputs. The frame is painted from the rectangles of the
//! halves and the body cached in the last frame, and nested frames paint inside the frames around them. A node dropped
//! with its centre in a region's frame joins that region, and the node menu moves it into a region or out of one.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use egui::emath::TSTransform;
use egui::epaint::Shadow;
use egui::{
    Align2, Button, Color32, CornerRadius, DragValue, FontId, Frame, Id, Label, LayerId, Margin, Painter, Pos2, Rect,
    RichText, ScrollArea, Stroke, StrokeKind, Style, TextEdit, Ui, pos2, vec2,
};
use egui_snarl::ui::{
    AnyPins, BackgroundPattern, Grid, PinInfo, PinPlacement, SelectionStyle, SnarlPin, SnarlStyle, SnarlViewer,
    SnarlWidget, WireStyle, get_selected_nodes,
};
use egui_snarl::{InPin, InPinId, NodeId as PinNode, OutPin, OutPinId, Snarl};
use henad_core::params::ParamKind;
use henad_graph::catalogue::{ConfigType, InputDefault, NodeDef, SocketType, effective_config, lookup};
use henad_graph::check::PassReport;
use henad_graph::document::{GraphDocument, Pass, PassStage, Stat};
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::editor::EditorRegion;
use henad_graph::ids::NodeId;
use henad_graph::limits::{MAX_ACCUMULATORS, MAX_REGION_DEPTH, MAX_REPEAT_ITERATIONS};
use henad_graph::node::{
    Accumulator, Combine, Node, NodeBody, OP_EXPRESSION, OP_FUNCTION_CALL, OP_FUNCTION_OUTPUT, OP_REGION_REPEAT,
};
use henad_graph::types::{Rate, ValueType};
use henad_graph::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};

use super::arrange::{COLUMN_GAP, CanvasKey, NODE_SIZE};
use super::quick_add::{WireEnd, offered_kinds};
use super::sockets::{rate_shape, type_color};
use super::view_pass::can_view;
use super::{DroppedWire, GraphEdit, GraphRequest, NodeIssues};
use crate::icons::material_design_icons::{MDI_ALERT, MDI_CLOSE, MDI_PLUS};
use crate::ui::mcs;
use crate::ui::sweep::layout::add_button;

/// Margin in canvas points between a frame and the nodes inside it.
const FRAME_MARGIN: f32 = 14.0;

/// Height in canvas points of the title strip on top of a frame.
const FRAME_TITLE_HEIGHT: f32 = 18.0;

/// Canvas points left around the nodes a framing shows.
const FRAMING_MARGIN: f32 = 80.0;

/// Size in canvas points assumed for a node that has not been drawn yet.
const UNDRAWN_NODE_SIZE: egui::Vec2 = vec2(160.0, 80.0);

/// Lowest zoom of the canvas.
pub const MIN_SCALE: f32 = 0.2;

/// Highest zoom of the canvas.
pub const MAX_SCALE: f32 = 2.0;

/// Most kinds the quick-add list shows at once.
const QUICK_ADD_LIMIT: usize = 80;

/// Width in points of the hover popup and the quick-add list.
const POPUP_WIDTH: f32 = 320.0;

/// Kind of a neighborhood fold.
const OP_GRID_FOLD: &str = "grid.fold";

/// Kinds of the regions library 1 nests.
const REGION_OPS: [&str; 3] = [OP_REGION_REPEAT, OP_GRID_FOLD, "agent.radius_fold"];

/// Width in points of an expression's text field on its node.
const EXPRESSION_WIDTH: f32 = 260.0;

/// Value an input of a node holds, as the canvas shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum InputShown {
    /// A wire from another node.
    Wire,
    /// A constant, edited in place.
    Literal(Literal),
    /// A binding to a parameter.
    Param {
        /// Id of the parameter.
        id: String,
        /// Label of the parameter.
        label: String,
    },
    /// Nothing.
    Empty,
}

/// One input pin of a node.
#[derive(Debug, Clone, PartialEq)]
pub struct InputView {
    /// Port name, dotted when configuration makes it.
    pub port: String,
    /// Value the input holds.
    pub shown: InputShown,
    /// Type of the value, `None` when unknown.
    pub value_type: Option<ValueType>,
    /// Rate of the value.
    pub rate: Rate,
    /// Whether the file has to write the input.
    pub required: bool,
}

/// One output pin of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputView {
    /// Port name.
    pub port: String,
    /// Type of the value, `None` when unknown.
    pub value_type: Option<ValueType>,
    /// Rate of the value.
    pub rate: Rate,
    /// Whether only the region's own body reads the output.
    pub inner: bool,
}

/// Part of a document node that one canvas node draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePart {
    /// A node that is no region, whole.
    Whole,
    /// The input half of a region: its inputs and its inner outputs.
    RegionInput,
    /// The output half of a region: its accumulator outputs.
    RegionOutput,
}

/// Node value of a canvas [`Snarl`]: what the viewer draws, and the document node it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    /// Id of the node in the document.
    pub id: NodeId,
    /// Part of the node the canvas node draws.
    pub part: NodePart,
    /// Kind of the node.
    pub op: String,
    /// Text of the header: the placement's label, or the kind's name.
    pub title: String,
    /// Written semantics of the kind, empty for an unknown kind.
    pub semantics: &'static str,
    /// Input pins.
    pub inputs: Vec<InputView>,
    /// Output pins.
    pub outputs: Vec<OutputView>,
    /// Lines of configuration the body shows, as `key: value`.
    pub settings: Vec<String>,
    /// Problems the checker found at the node.
    pub issues: NodeIssues,
}

/// Rectangle a node was last drawn in, with the position it was drawn at.
#[derive(Debug, Clone, Copy)]
pub struct DrawnRect {
    /// Rectangle in canvas points, node frame included.
    pub rect: Rect,
    /// Node position when the rectangle was drawn.
    pub position: Pos2,
}

/// A frame behind some canvas nodes: a titled `[[editor.frame]]` of the file, or a region with its halves and its body.
#[derive(Debug, Clone)]
pub struct CanvasFrame {
    /// Text on the frame, empty for a region.
    pub title: String,
    /// Canvas nodes inside the frame.
    pub members: Vec<PinNode>,
    /// Number of region frames around this one, 0 for a frame of the file.
    pub depth: usize,
    /// Fill, stroke and title colours.
    pub colors: [Color32; 3],
    /// Region whose body the frame holds, `None` for a frame of the file.
    pub region: Option<NodeId>,
}

/// Rectangle of a region's frame on the canvas.
#[derive(Debug, Clone, Copy)]
pub struct RegionFrameRect {
    /// Index of the frame in [`BodyCanvas::frames`].
    pub index: usize,
    /// Region of the frame.
    pub region: NodeId,
    /// Number of region frames around the frame, itself included.
    pub depth: usize,
    /// Rectangle in canvas points.
    pub rect: Rect,
}

/// Nodes a framing brings into view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// One node.
    Node(NodeId),
    /// Every node of the body.
    All,
}

/// The canvas of one body.
#[derive(Debug)]
pub struct BodyCanvas {
    /// Egui id of the canvas, under which egui-snarl keeps the transform, the selection and the node sizes.
    pub canvas_id: Id,
    /// View of the body.
    pub snarl: Snarl<NodeView>,
    /// [`Snarl`] node of each document node, the input half for a region.
    pub nodes: BTreeMap<NodeId, PinNode>,
    /// [`Snarl`] node of the output half of each region.
    pub output_halves: BTreeMap<NodeId, PinNode>,
    /// Position each canvas node took at the last sync.
    placed: HashMap<PinNode, Pos2>,
    /// Rectangle of each node in the last frame.
    pub rects: HashMap<PinNode, DrawnRect>,
    /// Transform of the canvas in the last frame.
    pub transform: TSTransform,
    /// Rectangle of the canvas on screen in the last frame.
    pub canvas_rect: Rect,
    /// Frames behind the nodes.
    pub frames: Vec<CanvasFrame>,
    /// Revision of the document the canvas shows.
    pub revision: u64,
    /// Nodes to bring into view on the next frames.
    pub framing: Option<Framing>,
    /// Rectangle the framing of the last frame showed, while a framing holds.
    framed: Option<Rect>,
    /// Text of the quick-add search field.
    pub query: String,
    /// Canvas position where the open menu was opened, `None` while no menu is open.
    menu_origin: Option<Pos2>,
}

impl BodyCanvas {
    /// Returns an empty canvas under the egui id `canvas_id`, which brings every node into view on its first frame.
    pub fn new(canvas_id: Id) -> Self {
        Self {
            canvas_id,
            snarl: Snarl::new(),
            nodes: BTreeMap::new(),
            output_halves: BTreeMap::new(),
            placed: HashMap::new(),
            rects: HashMap::new(),
            transform: TSTransform::IDENTITY,
            canvas_rect: Rect::ZERO,
            frames: Vec::new(),
            revision: u64::MAX,
            framing: Some(Framing::All),
            framed: None,
            query: String::new(),
            menu_origin: None,
        }
    }

    /// Brings the [`Snarl`] to `body`, the body of `scope` in `document`, placed by `editor` and typed by `report`.
    ///
    /// Note that a removal while a wire is in flight from or to the removed node makes egui-snarl panic on its next
    /// draw. The caller runs no sync while the primary button is down.
    pub fn sync(
        &mut self,
        scope: &Scope,
        document: &GraphDocument,
        editor: &EditorRegion,
        body: &NodeBody,
        report: Option<&PassReport>,
        issues: &BTreeMap<NodeId, NodeIssues>,
    ) {
        let mut views: BTreeMap<CanvasKey, NodeView> = BTreeMap::new();
        let mut order: Vec<CanvasKey> = Vec::with_capacity(body.nodes.len());
        for node in &body.nodes {
            if views.contains_key(&CanvasKey::main(node.id)) {
                continue;
            }
            let node_issues = issues.get(&node.id).cloned().unwrap_or_default();
            let (main, output) = node_views(node, body, document, editor, report, node_issues);
            views.insert(CanvasKey::main(node.id), main);
            order.push(CanvasKey::main(node.id));
            if let Some(output) = output {
                views.insert(CanvasKey::output(node.id), output);
                order.push(CanvasKey::output(node.id));
            }
        }

        let held: Vec<(CanvasKey, PinNode)> = self
            .nodes
            .iter()
            .map(|(id, pin_node)| (CanvasKey::main(*id), *pin_node))
            .chain(
                self.output_halves
                    .iter()
                    .map(|(id, pin_node)| (CanvasKey::output(*id), *pin_node)),
            )
            .collect();
        for (key, pin_node) in held {
            let known = views.get(&key).is_some_and(|view| {
                self.snarl
                    .get_node(pin_node)
                    .is_some_and(|shown| shown.part == view.part)
            });
            if !known {
                self.canvas_nodes_mut(key.output_half).remove(&key.node);
                self.snarl.remove_node(pin_node);
                self.rects.remove(&pin_node);
                self.placed.remove(&pin_node);
            }
        }
        for (index, key) in order.iter().enumerate() {
            let Some(view) = views.get(key) else { continue };
            let fallback = pos2(index as f32 * 30.0, index as f32 * 30.0);
            let position = if key.output_half {
                output_half_position(key.node, body, editor)
            } else {
                editor
                    .node_pos(key.node)
                    .map_or(fallback, |pos| pos2(pos[0] as f32, pos[1] as f32))
            };
            let existing = self.canvas_nodes(key.output_half).get(&key.node).copied();
            let pin_node = if let Some(pin_node) = existing {
                if let Some(info) = self.snarl.get_node_info_mut(pin_node) {
                    if info.value != *view {
                        info.value = view.clone();
                    }
                    info.pos = position;
                }
                pin_node
            } else {
                let pin_node = self.snarl.insert_node(position, view.clone());
                self.canvas_nodes_mut(key.output_half).insert(key.node, pin_node);
                pin_node
            };
            self.placed.insert(pin_node, position);
        }

        let wanted = self.document_wires(body);
        let held: HashSet<(OutPinId, InPinId)> = self.snarl.wires().collect();
        let mut extra: Vec<&(OutPinId, InPinId)> = held.difference(&wanted).collect();
        extra.sort_by_key(|wire| wire_order(wire));
        for wire in extra {
            self.snarl.disconnect(wire.0, wire.1);
        }
        let mut missing: Vec<&(OutPinId, InPinId)> = wanted.difference(&held).collect();
        missing.sort_by_key(|wire| wire_order(wire));
        for wire in missing {
            self.snarl.connect(wire.0, wire.1);
        }
        self.frames = self.frames_of(scope, document, editor, body);
    }

    /// Returns the map of the canvas nodes of document nodes, or of output halves when `output_half` holds.
    fn canvas_nodes(&self, output_half: bool) -> &BTreeMap<NodeId, PinNode> {
        if output_half { &self.output_halves } else { &self.nodes }
    }

    /// Returns the map of the canvas nodes of document nodes, or of output halves when `output_half` holds, for
    /// writing.
    fn canvas_nodes_mut(&mut self, output_half: bool) -> &mut BTreeMap<NodeId, PinNode> {
        if output_half {
            &mut self.output_halves
        } else {
            &mut self.nodes
        }
    }

    /// Returns the canvas nodes of the document node `id`: the node, or a region's two halves.
    pub fn pins_of(&self, id: NodeId) -> impl Iterator<Item = PinNode> + '_ {
        self.nodes
            .get(&id)
            .into_iter()
            .chain(self.output_halves.get(&id))
            .copied()
    }

    /// Returns the frames of the canvas of `scope`: the editor's frames, then one per region, outermost first.
    fn frames_of(
        &self,
        scope: &Scope,
        document: &GraphDocument,
        editor: &EditorRegion,
        body: &NodeBody,
    ) -> Vec<CanvasFrame> {
        let canvas = scope.canvas();
        let mut frames = Vec::new();
        for frame in editor.frames() {
            if frame.scope != canvas {
                continue;
            }
            frames.push(CanvasFrame {
                title: frame.title,
                members: frame.members.iter().flat_map(|member| self.pins_of(*member)).collect(),
                depth: 0,
                colors: hue_colors(&frame.color),
                region: None,
            });
        }
        for node in &body.nodes {
            if !lookup(&node.op, document.library).is_some_and(|def| def.is_region) {
                continue;
            }
            let members: Vec<PinNode> = std::iter::once(node.id)
                .chain(body.region_contents(node.id))
                .flat_map(|member| self.pins_of(member))
                .collect();
            let depth = 1 + body.region_path(node.id).len();
            frames.push(CanvasFrame {
                title: String::new(),
                members,
                depth,
                colors: region_colors(depth),
                region: Some(node.id),
            });
        }
        frames.sort_by_key(|frame| frame.depth);
        frames
    }

    /// Returns the pins of every wire of `body` whose two ends the canvas shows.
    ///
    /// A link to a region's inner output leaves its input half, and a link to an accumulator its output half.
    pub fn document_wires(&self, body: &NodeBody) -> HashSet<(OutPinId, InPinId)> {
        let mut wires = HashSet::new();
        for node in &body.nodes {
            let Some(&dest) = self.nodes.get(&node.id) else {
                continue;
            };
            let Some(dest_view) = self.snarl.get_node(dest) else {
                continue;
            };
            for (input, view) in dest_view.inputs.iter().enumerate() {
                let Some(InputValue::Link(link)) = node.inputs.get(&view.port) else {
                    continue;
                };
                if let Some(output) = self.output_pin(link) {
                    wires.insert((output, InPinId { node: dest, input }));
                }
            }
        }
        wires
    }

    /// Returns the output pin that `link` reads, `None` when the canvas does not show it.
    fn output_pin(&self, link: &PortRef) -> Option<OutPinId> {
        let candidates = self.pins_of(link.node).filter_map(|pin_node| {
            let view = self.snarl.get_node(pin_node)?;
            let output = match &link.port {
                Some(port) => view.outputs.iter().position(|known| known.port == *port)?,
                None => (!view.outputs.is_empty() && view.part != NodePart::RegionInput).then_some(0)?,
            };
            Some(OutPinId { node: pin_node, output })
        });
        let mut candidates = candidates;
        candidates.next()
    }

    /// Returns the rectangle of the node `id` at its current position, the input half for a region.
    pub fn current_rect(&self, id: NodeId) -> Option<Rect> {
        self.pin_rect(*self.nodes.get(&id)?)
    }

    /// Returns the rectangle of the canvas node `pin_node` at its current position.
    pub fn pin_rect(&self, pin_node: PinNode) -> Option<Rect> {
        let position = self.snarl.get_node_info(pin_node)?.pos;
        Some(drawn_rect(self.rects.get(&pin_node), position))
    }

    /// Returns the rectangle the node `id` was last drawn in, the input half for a region, `None` before it is drawn.
    pub fn drawn_rect(&self, id: NodeId) -> Option<Rect> {
        let pin_node = self.nodes.get(&id)?;
        self.rects.get(pin_node).map(|drawn| drawn.rect)
    }

    /// Returns the rectangle the output half of the region `id` was last drawn in, `None` before it is drawn.
    pub fn drawn_output_half_rect(&self, id: NodeId) -> Option<Rect> {
        let pin_node = self.output_halves.get(&id)?;
        self.rects.get(pin_node).map(|drawn| drawn.rect)
    }

    /// Returns the rectangle of the output half of the region `id` at its current position.
    pub fn output_half_rect(&self, id: NodeId) -> Option<Rect> {
        self.pin_rect(*self.output_halves.get(&id)?)
    }

    /// Returns the rectangle around every canvas node, `None` for an empty canvas.
    pub fn all_rect(&self) -> Option<Rect> {
        self.nodes
            .values()
            .chain(self.output_halves.values())
            .filter_map(|pin_node| self.pin_rect(*pin_node))
            .reduce(Rect::union)
    }

    /// Returns the canvas rectangle around the canvas nodes of `ids`, as `[min, max]`, `None` when none is shown.
    pub fn bounds_of(&self, ids: &[NodeId]) -> Option<[[f64; 2]; 2]> {
        let rect = ids
            .iter()
            .flat_map(|id| self.pins_of(*id))
            .filter_map(|pin_node| self.pin_rect(pin_node))
            .reduce(Rect::union)?;
        Some([
            [f64::from(rect.min.x), f64::from(rect.min.y)],
            [f64::from(rect.max.x), f64::from(rect.max.y)],
        ])
    }

    /// Returns the document nodes egui-snarl holds selected, each once, in id order. Either half of a region selects
    /// the region.
    pub fn selected(&self, ctx: &egui::Context) -> Vec<NodeId> {
        let mut selected: Vec<NodeId> = get_selected_nodes(self.canvas_id, ctx)
            .iter()
            .filter_map(|pin_node| self.snarl.get_node(*pin_node))
            .map(|view| view.id)
            .collect();
        selected.sort_unstable();
        selected.dedup();
        selected
    }

    /// Returns one [`EditCommand::Move`] per node whose position on the canvas differs from its placement in `editor`,
    /// and one [`EditCommand::MoveRegionOutput`] per output half moved since the last sync.
    pub fn moved(&self, editor: &EditorRegion) -> Vec<EditCommand> {
        let mut moves = Vec::new();
        for (id, pin_node) in &self.nodes {
            let Some(info) = self.snarl.get_node_info(*pin_node) else {
                continue;
            };
            let placed = editor.node_pos(*id);
            let pos = [f64::from(info.pos.x), f64::from(info.pos.y)];
            let differs =
                placed.is_none_or(|placed| (placed[0] - pos[0]).abs() > 0.01 || (placed[1] - pos[1]).abs() > 0.01);
            if differs {
                moves.push(EditCommand::Move { node: *id, pos });
            }
        }
        for (id, pin_node) in &self.output_halves {
            if let Some(pos) = self.moved_since_sync(*pin_node) {
                moves.push(EditCommand::MoveRegionOutput {
                    node: *id,
                    pos: [f64::from(pos.x), f64::from(pos.y)],
                });
            }
        }
        moves
    }

    /// Returns the position of `pin_node` when it differs from the one the last sync gave it.
    fn moved_since_sync(&self, pin_node: PinNode) -> Option<Pos2> {
        let pos = self.snarl.get_node_info(pin_node)?.pos;
        let placed = self.placed.get(&pin_node)?;
        ((*placed - pos).length() > 0.01).then_some(pos)
    }

    /// Returns the rectangle of each region frame at the current positions, drawn without the canvas nodes in
    /// `without`. A frame left with no member is left out.
    pub fn region_frame_rects(&self, without: &HashSet<PinNode>) -> Vec<RegionFrameRect> {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(index, frame)| {
                let region = frame.region?;
                let rect = frame
                    .members
                    .iter()
                    .filter(|pin_node| !without.contains(pin_node))
                    .filter_map(|pin_node| self.pin_rect(*pin_node))
                    .reduce(Rect::union)?;
                Some(RegionFrameRect {
                    index,
                    region,
                    depth: frame.depth,
                    rect: frame_rect(rect, &frame.title),
                })
            })
            .collect()
    }

    /// Returns the region that each node moved since the last sync joins when it is dropped where it is now, with the
    /// index of the region's frame in [`Self::frames`].
    ///
    /// A node joins the deepest region whose frame, drawn without the moved canvas nodes, holds the node's centre. It
    /// joins no region around it, which the node menu's Move out of region reaches, and a node whose region moves with
    /// it stays in that region.
    pub fn regions_entered(&self, body: &NodeBody) -> Vec<(NodeId, NodeId, usize)> {
        let moved: Vec<(PinNode, NodeId)> = self
            .nodes
            .iter()
            .chain(&self.output_halves)
            .filter(|(_, pin_node)| self.moved_since_sync(**pin_node).is_some())
            .map(|(id, pin_node)| (*pin_node, *id))
            .collect();
        if moved.is_empty() {
            return Vec::new();
        }
        let moved_pins: HashSet<PinNode> = moved.iter().map(|(pin_node, _)| *pin_node).collect();
        let moved_ids: BTreeSet<NodeId> = moved.iter().map(|(_, id)| *id).collect();
        let frames = self.region_frame_rects(&moved_pins);
        let mut entered = Vec::new();
        let mut seen = BTreeSet::new();
        for (pin_node, id) in moved {
            if !seen.insert(id) {
                continue;
            }
            let path = body.region_path(id);
            if path.iter().any(|region| moved_ids.contains(region)) {
                continue;
            }
            let Some(center) = self.pin_rect(pin_node).map(|rect| rect.center()) else {
                continue;
            };
            let target = frames
                .iter()
                .filter(|frame| {
                    frame.rect.contains(center)
                        && frame.region != id
                        && !body.region_path(frame.region).contains(&id)
                        && can_join(body, id, frame.region)
                })
                .max_by_key(|frame| frame.depth);
            if let Some(frame) = target
                && !path.contains(&frame.region)
            {
                entered.push((id, frame.region, frame.index));
            }
        }
        entered
    }

    /// Returns a [`EditCommand::Disconnect`] for each document wire of `body` the [`Snarl`] no longer holds.
    ///
    /// A command-drag from a socket drops its wires in egui-snarl without a viewer callback.
    pub fn lost_wires(&self, scope: &Scope, body: &NodeBody) -> Vec<EditCommand> {
        let held: HashSet<(OutPinId, InPinId)> = self.snarl.wires().collect();
        let mut lost: Vec<EditCommand> = self
            .document_wires(body)
            .into_iter()
            .filter(|wire| !held.contains(wire))
            .filter_map(|(_, input)| {
                let view = self.snarl.get_node(input.node)?;
                Some(EditCommand::Disconnect {
                    scope: scope.clone(),
                    node: view.id,
                    port: view.inputs.get(input.input)?.port.clone(),
                })
            })
            .collect();
        lost.sort_by_key(|command| format!("{command:?}"));
        lost
    }
}

/// Returns the sort key of a wire, which keeps the order the [`Snarl`] receives wires the same from run to run.
fn wire_order(wire: &(OutPinId, InPinId)) -> [usize; 4] {
    [wire.0.node.0, wire.0.output, wire.1.node.0, wire.1.input]
}

/// Returns the views of `node` of `body`: the whole node, or a region's input half and its output half.
fn node_views(
    node: &Node,
    body: &NodeBody,
    document: &GraphDocument,
    editor: &EditorRegion,
    report: Option<&PassReport>,
    issues: NodeIssues,
) -> (NodeView, Option<NodeView>) {
    let def = lookup(&node.op, document.library);
    let label = editor.node_layout(node.id).and_then(|layout| layout.label);
    let title = label.unwrap_or_else(|| def.map_or_else(|| node.op.clone(), |def| def.label.to_owned()));
    let output_type = |source: &PortRef| -> (Option<ValueType>, Rate) {
        let Some(report) = report else {
            return (None, Rate::Element);
        };
        let ports = report
            .outputs
            .get(&source.node)
            .into_iter()
            .chain(report.inner_outputs.get(&source.node));
        for ports in ports {
            let found = match &source.port {
                Some(port) => ports.get(port),
                None => ports.values().next(),
            };
            if let Some(port) = found {
                return (port.value_type.clone(), port.rate);
            }
        }
        (None, Rate::Element)
    };

    let mut inputs = Vec::new();
    if let Some(def) = def {
        for resolved in def.resolved_inputs(node, document) {
            let socket_type = resolved.declared.clone().or_else(|| match resolved.value_type {
                Some(SocketType::Fixed(class)) => class.value_type(),
                _ => None,
            });
            inputs.push(input_view(
                &resolved.name,
                node.inputs.get(&resolved.name),
                socket_type,
                resolved.default == InputDefault::Required,
                document,
                &output_type,
            ));
        }
    }
    for (port, value) in &node.inputs {
        if !inputs.iter().any(|known| known.port == *port) {
            inputs.push(input_view(port, Some(value), None, false, document, &output_type));
        }
    }

    let outputs = def.map_or_else(Vec::new, |def| output_views(def, node, document, report));

    let is_region = def.is_some_and(|def| def.is_region);
    let settings = settings_lines(def, node, body);
    let semantics = def.map_or("", |def| def.semantics);
    if !is_region {
        let view = NodeView {
            id: node.id,
            part: NodePart::Whole,
            op: node.op.clone(),
            title,
            semantics,
            inputs,
            outputs,
            settings,
            issues,
        };
        return (view, None);
    }
    let (inner, outer): (Vec<OutputView>, Vec<OutputView>) = outputs.into_iter().partition(|output| output.inner);
    let output_half = NodeView {
        id: node.id,
        part: NodePart::RegionOutput,
        op: node.op.clone(),
        title: format!("{title} output"),
        semantics,
        inputs: Vec::new(),
        outputs: outer,
        settings: Vec::new(),
        issues: NodeIssues::default(),
    };
    let input_half = NodeView {
        id: node.id,
        part: NodePart::RegionInput,
        op: node.op.clone(),
        title,
        semantics,
        inputs,
        outputs: inner,
        settings,
        issues,
    };
    (input_half, Some(output_half))
}

/// Returns the output pins of `node` of kind `def`, outer outputs before inner outputs, typed by `report`.
fn output_views(def: &NodeDef, node: &Node, document: &GraphDocument, report: Option<&PassReport>) -> Vec<OutputView> {
    let ports = def
        .resolved_outputs(node, document)
        .into_iter()
        .map(|output| (output, false))
        .chain(
            def.resolved_inner_outputs(node)
                .into_iter()
                .map(|output| (output, true)),
        );
    ports
        .map(|(resolved, inner)| {
            let typed = report.and_then(|report| {
                let table = if inner { &report.inner_outputs } else { &report.outputs };
                table.get(&node.id)?.get(&resolved.name)
            });
            OutputView {
                value_type: typed.and_then(|typed| typed.value_type.clone()).or(resolved.declared),
                rate: typed.map_or(Rate::Element, |typed| typed.rate),
                port: resolved.name,
                inner,
            }
        })
        .collect()
}

/// Returns the lines of configuration the body of `node`, of kind `def`, shows as `key: value`, and its region.
///
/// A region leaves out the keys its controls show: its accumulators, and a Repeat's `max`.
fn settings_lines(def: Option<&NodeDef>, node: &Node, body: &NodeBody) -> Vec<String> {
    let mut settings = Vec::new();
    if let Some(def) = def {
        for (key, value) in effective_config(def, node) {
            let controlled = key == "acc" || (key == "max" && node.op == OP_REGION_REPEAT);
            let expression = node.op == OP_EXPRESSION && matches!(key.as_str(), "text" | "inputs" | "outputs");
            if key == "key" || (def.is_region && controlled) || expression {
                continue;
            }
            settings.push(format!("{key}: {}", config_text(&value)));
        }
    }
    if let Some(region) = node.region {
        let depth = body.region_path(node.id).len();
        settings.push(format!("in region {region}, depth {depth}"));
    }
    settings
}

/// Returns the position of the output half of the region `region`: its placement, or one column right of the nodes
/// inside the region, level with the input half.
fn output_half_position(region: NodeId, body: &NodeBody, editor: &EditorRegion) -> Pos2 {
    let layout = editor.node_layout(region);
    if let Some(pos) = layout.as_ref().and_then(|layout| layout.output_pos) {
        return pos2(pos[0] as f32, pos[1] as f32);
    }
    let start = layout.map_or([0.0, 0.0], |layout| layout.pos);
    let right = body
        .region_contents(region)
        .into_iter()
        .filter_map(|member| editor.node_layout(member))
        .map(|member| member.pos[0])
        .fold(start[0], f64::max);
    pos2((right + NODE_SIZE[0] + COLUMN_GAP) as f32, start[1] as f32)
}

/// Returns the rectangle of a canvas node at `position`, from the rectangle it was `drawn` in, or of an assumed size
/// for a node not drawn yet.
fn drawn_rect(drawn: Option<&DrawnRect>, position: Pos2) -> Rect {
    drawn.map_or_else(
        || Rect::from_min_size(position, UNDRAWN_NODE_SIZE),
        |drawn| drawn.rect.translate(position - drawn.position),
    )
}

/// Returns the rectangle of a frame around the canvas nodes in `members`, with room for a title strip when `title`
/// is not empty.
fn frame_rect(members: Rect, title: &str) -> Rect {
    let top = if title.is_empty() {
        FRAME_MARGIN
    } else {
        FRAME_MARGIN + FRAME_TITLE_HEIGHT
    };
    Rect::from_min_max(
        members.min - vec2(FRAME_MARGIN, top),
        members.max + vec2(FRAME_MARGIN, FRAME_MARGIN),
    )
}

/// Returns the fill, stroke and title colours of a region's frame `depth` region frames deep, counting itself. A
/// deeper frame takes a lighter fill.
fn region_colors(depth: usize) -> [Color32; 3] {
    match depth {
        0 | 1 => [mcs::GRAY_900, mcs::GRAY_700, mcs::GRAY_300],
        2 => [mcs::GRAY_800, mcs::GRAY_600, mcs::GRAY_300],
        _ => [mcs::GRAY_700, mcs::GRAY_500, mcs::GRAY_200],
    }
}

/// Returns the view of the input `port` holding `value`, of the socket type `socket_type` when known.
fn input_view(
    port: &str,
    value: Option<&InputValue>,
    socket_type: Option<ValueType>,
    required: bool,
    document: &GraphDocument,
    output_type: &dyn Fn(&PortRef) -> (Option<ValueType>, Rate),
) -> InputView {
    let (shown, value_type, rate) = match value {
        Some(InputValue::Link(link)) => {
            let (value_type, rate) = output_type(link);
            (InputShown::Wire, value_type.or(socket_type), rate)
        }
        Some(InputValue::Literal(literal)) => (
            InputShown::Literal(literal.clone()),
            Some(literal.value_type()),
            Rate::Constant,
        ),
        Some(InputValue::Param(id)) => {
            let descriptor = document.params.iter().find(|param| param.id() == id);
            let value_type = descriptor.map(|param| match param.kind() {
                ParamKind::F32 { .. } => ValueType::F32,
                ParamKind::U32 { .. } | ParamKind::Choice { .. } => ValueType::U32,
                ParamKind::Bool { .. } => ValueType::Bool,
            });
            let label = descriptor.map_or_else(|| id.clone(), |param| param.label().to_owned());
            (InputShown::Param { id: id.clone(), label }, value_type, Rate::Parameter)
        }
        Some(_) | None => (InputShown::Empty, socket_type, Rate::Constant),
    };
    InputView {
        port: port.to_owned(),
        shown,
        value_type,
        rate,
        required,
    }
}

/// Returns `value` on one short line.
fn config_text(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Bool(flag) => flag.to_string(),
        ConfigValue::Integer(number) => number.to_string(),
        ConfigValue::Float(number) => format!("{number}"),
        ConfigValue::String(text) => text.clone(),
        ConfigValue::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .map(|item| match item.as_table() {
                    Some(table) => table
                        .get("id")
                        .or_else(|| table.get("name"))
                        .map_or_else(|| "{..}".to_owned(), config_text),
                    None => config_text(item),
                })
                .collect();
            names.join(", ")
        }
        _ => "{..}".to_owned(),
    }
}

/// Returns the text of `literal` as the canvas shows a constant it cannot edit in place.
pub fn literal_text(literal: &Literal) -> String {
    match literal {
        Literal::Bool(flag) => flag.to_string(),
        Literal::U32(number) => number.to_string(),
        Literal::I32(number) => number.to_string(),
        Literal::F32(number) => format!("{number}"),
        Literal::U64(number) => format!("{number:#x}"),
        Literal::F64(number) => format!("{number}"),
        Literal::Vec2U([x, y]) => format!("({x}, {y})"),
        Literal::Vec2I([x, y]) => format!("({x}, {y})"),
        Literal::Vec2F([x, y]) => format!("({x}, {y})"),
        Literal::Category(name) | Literal::Layer(name) | Literal::ChoiceOption(name) => name.to_string(),
        _ => "?".to_owned(),
    }
}

/// Returns the fill, stroke and title colours of a frame in the MCS hue `hue`, gray for a name MCS does not hold.
fn hue_colors(hue: &str) -> [Color32; 3] {
    match hue {
        "red" => [mcs::RED_950, mcs::RED_700, mcs::RED_200],
        "orange" => [mcs::ORANGE_950, mcs::ORANGE_700, mcs::ORANGE_200],
        "yellow" => [mcs::YELLOW_950, mcs::YELLOW_700, mcs::YELLOW_200],
        "green" => [mcs::GREEN_950, mcs::GREEN_700, mcs::GREEN_200],
        "blue" => [mcs::BLUE_950, mcs::BLUE_700, mcs::BLUE_200],
        "purple" => [mcs::PURPLE_950, mcs::PURPLE_700, mcs::PURPLE_200],
        _ => [mcs::GRAY_900, mcs::GRAY_700, mcs::GRAY_300],
    }
}

/// Returns the transform that shows `target` at the centre of `canvas`, zoomed to fit with [`FRAMING_MARGIN`].
pub fn framing_transform(target: Rect, canvas: Rect) -> TSTransform {
    let view = target.expand(FRAMING_MARGIN);
    let fit = canvas.size() / view.size();
    let scaling = fit.min_elem().clamp(MIN_SCALE, 1.0);
    TSTransform {
        scaling,
        translation: canvas.center().to_vec2() - target.center().to_vec2() * scaling,
    }
}

/// Returns the canvas style: square corners on nodes, headers and the selection, and no space before the title.
pub fn canvas_style(style: &Style) -> SnarlStyle {
    let node_frame = Frame::window(style)
        .corner_radius(CornerRadius::ZERO)
        .shadow(Shadow::NONE);
    SnarlStyle {
        node_frame: Some(node_frame),
        header_frame: Some(node_frame),
        header_drag_space: Some(egui::Vec2::ZERO),
        select_style: Some(SelectionStyle {
            margin: Margin::same(4),
            rounding: CornerRadius::ZERO,
            fill: mcs::BLUE_900,
            stroke: Stroke::new(1.0, mcs::BLUE_500),
        }),
        wire_style: Some(WireStyle::Bezier5),
        bg_frame: Some(Frame::canvas(style).corner_radius(CornerRadius::ZERO).inner_margin(0)),
        bg_pattern: Some(BackgroundPattern::Grid(Grid::new(vec2(50.0, 50.0), 0.0))),
        bg_pattern_stroke: Some(Stroke::new(1.0, mcs::GRAY_900)),
        pin_placement: Some(PinPlacement::Edge),
        collapsible: Some(false),
        min_scale: Some(MIN_SCALE),
        max_scale: Some(MAX_SCALE),
        ..SnarlStyle::new()
    }
}

/// Pin of the canvas. A test build records each pin's rectangle in egui's memory, where the tests find the pins.
struct GraphPin {
    /// Shape and colours.
    info: PinInfo,
    /// Key of the recorded rectangle.
    #[cfg(test)]
    key: Id,
}

impl GraphPin {
    /// Returns a circle with the style's colours, for a pin whose node or port the view lacks.
    fn plain(_canvas_id: Id) -> Self {
        Self {
            info: PinInfo::circle(),
            #[cfg(test)]
            key: _canvas_id.with("unknown pin"),
        }
    }
}

impl SnarlPin for GraphPin {
    fn draw(
        self,
        snarl_style: &SnarlStyle,
        style: &Style,
        rect: Rect,
        painter: &Painter,
    ) -> egui_snarl::ui::PinWireInfo {
        #[cfg(test)]
        painter.ctx().data_mut(|data| data.insert_temp(self.key, rect));
        self.info.draw(snarl_style, style, rect, painter)
    }
}

/// Returns the key under which a test build records the rectangle of a pin, in canvas points.
#[cfg(test)]
pub fn pin_key(canvas_id: Id, node: NodeId, output: bool, index: usize) -> Id {
    canvas_id.with(("pin", node, output, index))
}

/// Inputs of one frame of the canvas, borrowed from the editor.
pub struct CanvasContext<'a> {
    /// Body the canvas shows.
    pub scope: &'a Scope,
    /// Nodes of the body.
    pub body: &'a NodeBody,
    /// Pass of the body, `None` for a function.
    pub pass: Option<&'a Pass>,
    /// Document the body belongs to.
    pub document: &'a GraphDocument,
}

/// Viewer of one frame of a canvas.
struct GraphViewer<'a> {
    canvas_id: Id,
    context: CanvasContext<'a>,
    frames: &'a [CanvasFrame],
    /// Indices in `frames` of the regions a dragged node would join on release.
    drop_targets: Vec<usize>,
    /// Region, depth and canvas rectangle of each region frame painted this frame.
    region_rects: Vec<(NodeId, usize, Rect)>,
    /// Rectangle to bring into view this frame, with the canvas rectangle.
    framing: Option<(Rect, Rect)>,
    rects: &'a mut HashMap<PinNode, DrawnRect>,
    query: &'a mut String,
    /// Canvas position where the open menu was opened.
    menu_origin: &'a mut Option<Pos2>,
    /// Whether a menu of the canvas showed this frame.
    menu_shown: bool,
    /// Requests of the frame's gestures, run after the canvas is drawn.
    requests: Vec<GraphRequest>,
    transform: TSTransform,
}

impl GraphViewer<'_> {
    /// Returns the canvas position where the open menu was opened: the pointer on the menu's first frame.
    ///
    /// egui-snarl passes the menu's top left, which egui moves to keep a tall menu on screen.
    fn menu_origin(&mut self, ui: &Ui, fallback: Pos2) -> Pos2 {
        self.menu_shown = true;
        let to_canvas = self.transform.inverse();
        *self.menu_origin.get_or_insert_with(|| {
            ui.ctx()
                .input(|input| input.pointer.latest_pos())
                .map_or(fallback, |pointer| to_canvas * pointer)
        })
    }

    fn edit(&mut self, commands: Vec<EditCommand>, coalesce: Option<String>) {
        self.requests.push(GraphRequest::Edit(GraphEdit { commands, coalesce }));
    }

    /// Returns the moves that place node `id` of `body`, with every node inside it, right of the frame of `region`,
    /// keeping their places relative to each other. Returns none when no canvas node of them comes within
    /// `FRAME_MARGIN` of the frame drawn without them, or when the frame holds nothing else.
    fn moves_out_of_frame(
        &self,
        snarl: &Snarl<NodeView>,
        body: &NodeBody,
        id: NodeId,
        region: NodeId,
    ) -> Vec<EditCommand> {
        let mut leaving: BTreeSet<NodeId> = body.region_contents(id).into_iter().collect();
        leaving.insert(id);
        let moving: Vec<(PinNode, &NodeView, Pos2)> = snarl
            .nodes_ids_data()
            .filter(|(_, node)| leaving.contains(&node.value.id))
            .map(|(pin_node, node)| (pin_node, &node.value, node.pos))
            .collect();
        let moving_pins: HashSet<PinNode> = moving.iter().map(|(pin_node, _, _)| *pin_node).collect();
        let rect_of = |pin_node: PinNode| {
            let position = snarl.get_node_info(pin_node)?.pos;
            Some(drawn_rect(self.rects.get(&pin_node), position))
        };
        let Some(frame) = self
            .frames
            .iter()
            .find(|frame| frame.region == Some(region))
            .and_then(|frame| {
                frame
                    .members
                    .iter()
                    .filter(|pin_node| !moving_pins.contains(pin_node))
                    .filter_map(|pin_node| rect_of(*pin_node))
                    .reduce(Rect::union)
                    .map(|members| frame_rect(members, &frame.title))
            })
        else {
            return Vec::new();
        };
        let Some(bounds) = moving
            .iter()
            .filter_map(|(pin_node, _, _)| rect_of(*pin_node))
            .reduce(Rect::union)
        else {
            return Vec::new();
        };
        // A node within a margin of the frame's edge still reads as a member.
        if !bounds.intersects(frame.expand(FRAME_MARGIN)) {
            return Vec::new();
        }
        let shift = frame.max.x + COLUMN_GAP as f32 - bounds.min.x;
        moving
            .into_iter()
            .map(|(_, view, pos)| {
                let pos = [f64::from(pos.x + shift), f64::from(pos.y)];
                if view.part == NodePart::RegionOutput {
                    EditCommand::MoveRegionOutput { node: view.id, pos }
                } else {
                    EditCommand::Move { node: view.id, pos }
                }
            })
            .collect()
    }

    /// Returns the region of the deepest region frame painted this frame that holds the canvas position `pos`.
    fn region_at(&self, pos: Pos2) -> Option<NodeId> {
        self.region_rects
            .iter()
            .filter(|(_, _, rect)| rect.contains(pos))
            .max_by_key(|(_, depth, _)| *depth)
            .map(|(region, _, _)| *region)
    }

    /// Returns the port names of the wire between two pins.
    fn wire_ports(source: &OutPin, dest: &InPin, snarl: &Snarl<NodeView>) -> Option<(NodeId, String, PortRef)> {
        let source_view = snarl.get_node(source.id.node)?;
        let dest_view = snarl.get_node(dest.id.node)?;
        let output = source_view.outputs.get(source.id.output)?;
        let input = dest_view.inputs.get(dest.id.input)?;
        Some((
            dest_view.id,
            input.port.clone(),
            PortRef {
                node: source_view.id,
                port: Some(output.port.clone()),
            },
        ))
    }

    fn disconnect_input(&mut self, dest: InPinId, snarl: &Snarl<NodeView>) {
        let Some(view) = snarl.get_node(dest.node) else { return };
        let Some(input) = view.inputs.get(dest.input) else {
            return;
        };
        let command = EditCommand::Disconnect {
            scope: self.context.scope.clone(),
            node: view.id,
            port: input.port.clone(),
        };
        self.edit(vec![command], None);
    }

    /// Draws the quick-add list of the kinds the top of the body, or the body of `region` when set, takes, and
    /// returns the kind picked. A region kind that cannot nest in `region` is left out.
    fn quick_add(&mut self, ui: &mut Ui, wire: Option<&WireEnd>, region: Option<NodeId>) -> Option<&'static str> {
        ui.set_max_width(POPUP_WIDTH);
        let field = ui.add(
            TextEdit::singleline(self.query)
                .hint_text("Search nodes")
                .desired_width(POPUP_WIDTH),
        );
        if !field.has_focus() && self.query.is_empty() {
            field.request_focus();
        }
        let body = self.context.body;
        let kinds: Vec<_> = offered_kinds(
            self.context.document,
            self.context.pass,
            wire,
            self.query,
            region.is_some(),
        )
        .into_iter()
        .filter(|def| can_add(body, def.id, region))
        .collect();
        if kinds.is_empty() {
            ui.weak("No node matches.");
            return None;
        }
        let mut picked = None;
        ScrollArea::vertical()
            .id_salt("henad_graph_quick_add")
            .max_height(320.0)
            .show(ui, |ui| {
                let mut group = "";
                for def in kinds.iter().take(QUICK_ADD_LIMIT) {
                    if def.group != group {
                        group = def.group;
                        ui.label(RichText::new(group).small().weak());
                    }
                    let first_sentence = def.semantics.split(". ").next().unwrap_or_default();
                    if ui
                        .add(Button::new(def.label).frame(false))
                        .on_hover_text(format!("{}\n{first_sentence}", def.id))
                        .clicked()
                    {
                        picked = Some(def.id);
                    }
                }
            });
        if picked.is_some() {
            self.query.clear();
        }
        picked
    }

    /// Draws the inline editor of a constant input, and requests the edit of a change.
    fn literal_editor(&mut self, ui: &mut Ui, view: &mut NodeView, index: usize) {
        let Some(input) = view.inputs.get_mut(index) else {
            return;
        };
        let InputShown::Literal(literal) = &mut input.shown else {
            return;
        };
        let changed = match literal {
            Literal::F32(number) => ui.add(DragValue::new(number).speed(0.01)).changed(),
            Literal::U32(number) => ui.add(DragValue::new(number).speed(0.2)).changed(),
            Literal::I32(number) => ui.add(DragValue::new(number).speed(0.2)).changed(),
            Literal::Bool(flag) => ui.checkbox(flag, "").changed(),
            Literal::Category(name) => {
                let mut picked = None;
                if let Some(category) = self
                    .context
                    .document
                    .categories
                    .iter()
                    .find(|category| category.id == name.owner)
                {
                    ui.menu_button(name.member.clone(), |ui| {
                        for variant in &category.variants {
                            if ui.button(&variant.label).clicked() {
                                picked = Some(variant.name.clone());
                                ui.close();
                            }
                        }
                    });
                } else {
                    ui.label(name.to_string());
                }
                if let Some(member) = picked {
                    *name = MemberName {
                        owner: name.owner.clone(),
                        member,
                    };
                    true
                } else {
                    false
                }
            }
            _ => {
                ui.label(literal_text(literal));
                false
            }
        };
        if changed {
            let command = EditCommand::SetLiteral {
                scope: self.context.scope.clone(),
                node: view.id,
                port: input.port.clone(),
                literal: literal.clone(),
            };
            let key = format!("literal {} {} {}", self.context.scope.canvas(), view.id, input.port);
            self.edit(vec![command], Some(key));
        }
    }

    /// Draws the node menu's entries for each input: bind to a parameter, set a constant, or clear.
    fn input_menus(&mut self, ui: &mut Ui, view: &NodeView) {
        let scope = self.context.scope.clone();
        let document = self.context.document;
        for input in &view.inputs {
            ui.menu_button(&input.port, |ui| {
                ui.menu_button("Bind to parameter", |ui| {
                    if document.params.is_empty() {
                        ui.weak("Add a parameter in the World panel first.");
                    }
                    for param in &document.params {
                        if ui.button(param.label()).on_hover_text(param.id()).clicked() {
                            let command = EditCommand::BindParam {
                                scope: scope.clone(),
                                node: view.id,
                                port: input.port.clone(),
                                param: param.id().to_owned(),
                            };
                            self.edit(vec![command], None);
                            ui.close();
                        }
                    }
                });
                let constant = input
                    .value_type
                    .as_ref()
                    .and_then(Literal::zero_of)
                    .or_else(|| first_variant(document, input.value_type.as_ref()));
                if let Some(constant) = constant
                    && ui.button("Set constant").clicked()
                {
                    let command = EditCommand::SetLiteral {
                        scope: scope.clone(),
                        node: view.id,
                        port: input.port.clone(),
                        literal: constant,
                    };
                    self.edit(vec![command], None);
                    ui.close();
                }
                let held = !matches!(input.shown, InputShown::Empty);
                if ui.add_enabled(held, Button::new("Clear")).clicked() {
                    let command = EditCommand::Disconnect {
                        scope: scope.clone(),
                        node: view.id,
                        port: input.port.clone(),
                    };
                    self.edit(vec![command], None);
                    ui.close();
                }
            });
        }
    }

    /// Draws the node menu's entries for each configuration key with a short list of values.
    fn setting_menus(&mut self, ui: &mut Ui, view: &NodeView) {
        let document = self.context.document;
        let Some(def) = lookup(&view.op, document.library) else {
            return;
        };
        for key in def.config {
            let choices: Vec<String> = match key.value {
                ConfigType::Bool => vec!["false".to_owned(), "true".to_owned()],
                ConfigType::Word(words) => words.iter().map(|word| (*word).to_owned()).collect(),
                ConfigType::LaneId => document.lanes.iter().map(|lane| lane.id.clone()).collect(),
                ConfigType::CategoryId => document.categories.iter().map(|category| category.id.clone()).collect(),
                ConfigType::PaletteId => document
                    .palettes
                    .iter()
                    .map(|palette| palette.id.clone())
                    .chain(document.categories.iter().map(|category| category.id.clone()))
                    .collect(),
                ConfigType::FunctionId => document.functions.iter().map(|function| function.id.clone()).collect(),
                ConfigType::InterfacePort => {
                    let Scope::Function(id) = self.context.scope else {
                        continue;
                    };
                    let Some(function) = document.functions.iter().find(|function| function.id == *id) else {
                        continue;
                    };
                    let ports = if view.op == OP_FUNCTION_OUTPUT {
                        &function.outputs
                    } else {
                        &function.inputs
                    };
                    ports.iter().map(|port| port.port.clone()).collect()
                }
                _ => continue,
            };
            ui.menu_button(key.name, |ui| {
                for choice in choices {
                    if ui.button(&choice).clicked() {
                        let value = match key.value {
                            ConfigType::Bool => ConfigValue::Bool(choice == "true"),
                            _ => ConfigValue::String(choice),
                        };
                        let command = EditCommand::SetConfig {
                            scope: self.context.scope.clone(),
                            node: view.id,
                            key: key.name.to_owned(),
                            value: Some(value),
                        };
                        self.edit(vec![command], None);
                        ui.close();
                    }
                }
            });
        }
    }
}

impl GraphViewer<'_> {
    /// Draws a text field of the canvas whose edit commits when it loses focus, and returns the text then, when it
    /// differs from `current`.
    fn commit_text(
        &self,
        ui: &mut Ui,
        key: impl std::hash::Hash + std::fmt::Debug,
        current: &str,
        multiline: bool,
    ) -> Option<String> {
        let field_id = self.canvas_id.with(key);
        let mut text = ui
            .data(|data| data.get_temp::<String>(field_id))
            .unwrap_or_else(|| current.to_owned());
        let field = if multiline {
            TextEdit::multiline(&mut text)
                .id(field_id)
                .code_editor()
                .desired_rows(2)
                .desired_width(EXPRESSION_WIDTH)
        } else {
            TextEdit::singleline(&mut text)
                .id(field_id)
                .font(egui::TextStyle::Small)
                .desired_width(72.0)
        };
        let response = ui.add(field);
        if response.changed() {
            ui.data_mut(|data| data.insert_temp(field_id, text.clone()));
        }
        if response.lost_focus() {
            ui.data_mut(|data| data.remove::<String>(field_id));
            return (text != current).then_some(text);
        }
        None
    }

    /// Draws the node menu's entries of the call `id`: Open function and Ungroup.
    fn call_menus(&mut self, ui: &mut Ui, id: NodeId) {
        if ui.button("Open function").clicked() {
            self.requests.push(GraphRequest::OpenCall(id));
            ui.close();
        }
        if ui
            .button("Ungroup")
            .on_hover_text("Replace call with copy of function's nodes")
            .clicked()
        {
            self.requests.push(GraphRequest::Ungroup(id));
            ui.close();
        }
    }

    /// Draws the row of the input at `index` of `inputs`, the inputs of the expression `node`: its name, its type and
    /// Remove. A rename to the name of another input is refused.
    fn expression_input_row(&mut self, ui: &mut Ui, node: &Node, inputs: &[(String, ValueType)], index: usize) {
        let id = node.id;
        let scope = self.context.scope.clone();
        let set = |value: ConfigValue| EditCommand::SetConfig {
            scope: scope.clone(),
            node: id,
            key: "inputs".to_owned(),
            value: Some(value),
        };
        let (name, value_type) = &inputs[index];
        let renamed = self.commit_text(ui, ("expression input", id, index), name, false);
        let taken = |renamed: &str| {
            inputs
                .iter()
                .enumerate()
                .any(|(other, (known, _))| other != index && known == renamed)
        };
        if let Some(renamed) = renamed.as_deref().filter(|renamed| taken(renamed)) {
            self.requests.push(GraphRequest::Refuse(format!(
                "Node {id} already has the input '{renamed}'."
            )));
        } else if let Some(renamed) = renamed {
            let mut list = inputs.to_vec();
            list[index].0.clone_from(&renamed);
            let mut commands = vec![set(expression_input_list(&list))];
            commands.extend(moved_input(&scope, node, name, &renamed));
            self.edit(commands, None);
        }
        let mut picked = None;
        ui.menu_button(RichText::new(value_type.to_string()).small(), |ui| {
            for choice in value_types(self.context.document) {
                if ui.button(choice.to_string()).clicked() {
                    picked = Some(choice);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Type of the input");
        if let Some(choice) = picked {
            let mut list = inputs.to_vec();
            list[index].1 = choice;
            self.edit(vec![set(expression_input_list(&list))], None);
        }
        let remove = add_button(
            ui,
            true,
            Button::new(RichText::new(MDI_CLOSE).small()).frame(false),
            "Remove input",
        );
        if remove.on_hover_text("Remove input").clicked() {
            let mut list = inputs.to_vec();
            list.remove(index);
            let mut commands = vec![set(expression_input_list(&list))];
            if node.inputs.contains_key(name) {
                commands.push(EditCommand::Disconnect {
                    scope: scope.clone(),
                    node: id,
                    port: name.clone(),
                });
            }
            self.edit(commands, None);
        }
    }

    /// Draws the editor of the expression `id`: its text, its named inputs with their types, and its outputs.
    fn expression_editor(&mut self, ui: &mut Ui, id: NodeId) {
        let Some(node) = self.context.body.node(id) else {
            return;
        };
        let scope = self.context.scope.clone();
        let set = |key: &str, value: ConfigValue| EditCommand::SetConfig {
            scope: scope.clone(),
            node: id,
            key: key.to_owned(),
            value: Some(value),
        };
        let text = node
            .config
            .get("text")
            .and_then(ConfigValue::as_str)
            .unwrap_or_default();
        if let Some(changed) = self.commit_text(ui, ("expression text", id), text, true) {
            self.edit(vec![set("text", ConfigValue::String(changed))], None);
        }
        let inputs = expression_inputs(node);
        ui.label(RichText::new("Inputs").small().weak());
        for index in 0..inputs.len() {
            ui.horizontal(|ui| self.expression_input_row(ui, node, &inputs, index));
        }
        let add = add_button(
            ui,
            true,
            Button::new(RichText::new(format!("{MDI_PLUS} Add input")).small()),
            "Add input",
        );
        if add.on_hover_text("Add input of type f32").clicked() {
            let taken = |candidate: &str| inputs.iter().any(|(known, _)| known == candidate);
            let name = std::iter::once("x".to_owned())
                .chain((2..).map(|number| format!("x_{number}")))
                .find(|candidate| !taken(candidate))
                .unwrap_or_default();
            let mut list = inputs.clone();
            list.push((name, ValueType::F32));
            self.edit(vec![set("inputs", expression_input_list(&list))], None);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Outputs").small().weak());
            let outputs: Vec<&str> = match node.config.get("outputs") {
                Some(ConfigValue::Array(items)) => items.iter().filter_map(ConfigValue::as_str).collect(),
                _ => Vec::new(),
            };
            let current = outputs.join(", ");
            if let Some(changed) = self.commit_text(ui, ("expression outputs", id), &current, false) {
                let list = changed
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(|name| ConfigValue::String(name.to_owned()))
                    .collect();
                self.edit(vec![set("outputs", ConfigValue::Array(list))], None);
            }
        })
        .response
        .on_hover_text("Names the text assigns, separated by commas. Each is an output of the node.");
    }

    /// Draws the controls of the region `id` in its input half: a Repeat's bound `max`, and the accumulators with
    /// their names, types and combines.
    fn region_controls(&mut self, ui: &mut Ui, id: NodeId) {
        let Some(region) = self.context.body.node(id) else {
            return;
        };
        if region.op == OP_REGION_REPEAT {
            self.max_control(ui, id, region);
        }
        if region.config.contains_key("acc") {
            let accumulators = region.accumulators().unwrap_or_default();
            self.accumulator_controls(ui, id, &accumulators);
        }
    }

    /// Draws the bound `max` of the Repeat `region`.
    fn max_control(&mut self, ui: &mut Ui, id: NodeId, region: &Node) {
        let scope = self.context.scope.clone();
        let max_now = region.config.get("max").and_then(ConfigValue::as_integer).unwrap_or(1);
        let mut max = max_now;
        ui.horizontal(|ui| {
            ui.add(Label::new(RichText::new("Max").small()).selectable(false))
                .on_hover_text("Largest number of iterations. A larger count runs this many.");
            ui.add(
                DragValue::new(&mut max)
                    .range(1..=MAX_REPEAT_ITERATIONS as i64)
                    .clamp_existing_to_range(false)
                    .speed(0.2),
            );
        });
        if max != max_now {
            let command = EditCommand::SetConfig {
                scope: scope.clone(),
                node: id,
                key: "max".to_owned(),
                value: Some(ConfigValue::Integer(max)),
            };
            self.edit(vec![command], Some(format!("max {} {id}", scope.canvas())));
        }
    }

    /// Draws the accumulator rows of the region `id`, from `accumulators`, and Add accumulator.
    fn accumulator_controls(&mut self, ui: &mut Ui, id: NodeId, accumulators: &[Accumulator]) {
        let scope = self.context.scope.clone();
        for (index, accumulator) in accumulators.iter().enumerate() {
            ui.horizontal(|ui| self.accumulator_row(ui, id, index, accumulator));
        }
        if accumulators.len() < MAX_ACCUMULATORS
            && ui
                .add(Button::new(
                    RichText::new(format!("{MDI_PLUS} Add accumulator")).small(),
                ))
                .on_hover_text("Add value carried through iterations")
                .clicked()
        {
            let name = unused_accumulator_name(accumulators);
            let command = EditCommand::AddAccumulator {
                scope,
                node: id,
                accumulator: Accumulator::new(name, ValueType::F32, Combine::Sum),
            };
            self.edit(vec![command], None);
        }
    }

    /// Draws one accumulator of the region `id`: its name, its type, its combine and its removal.
    fn accumulator_row(&mut self, ui: &mut Ui, id: NodeId, index: usize, accumulator: &Accumulator) {
        let scope = self.context.scope.clone();
        let field_id = self.canvas_id.with(("accumulator name", id, index));
        let mut name = ui
            .data(|data| data.get_temp::<String>(field_id))
            .unwrap_or_else(|| accumulator.name.clone());
        let field = ui.add(
            TextEdit::singleline(&mut name)
                .id(field_id)
                .desired_width(72.0)
                .font(egui::TextStyle::Small),
        );
        if field.changed() {
            ui.data_mut(|data| data.insert_temp(field_id, name.clone()));
        }
        if field.lost_focus() {
            ui.data_mut(|data| data.remove::<String>(field_id));
            if name != accumulator.name {
                let command = EditCommand::RenameAccumulator {
                    scope: scope.clone(),
                    node: id,
                    from: accumulator.name.clone(),
                    to: name,
                };
                self.edit(vec![command], None);
            }
        }
        let mut changed: Option<Accumulator> = None;
        ui.menu_button(RichText::new(accumulator.value_type.to_string()).small(), |ui| {
            for value_type in value_types(self.context.document) {
                if ui.button(value_type.to_string()).clicked() {
                    let combine = if accumulator.combine.applies_to(&value_type) {
                        accumulator.combine
                    } else {
                        Combine::Carry
                    };
                    changed = Some(Accumulator::new(accumulator.name.clone(), value_type, combine));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Type of the value");
        ui.menu_button(RichText::new(accumulator.combine.as_str()).small(), |ui| {
            for combine in Combine::ALL {
                if combine.applies_to(&accumulator.value_type) && ui.button(combine.as_str()).clicked() {
                    changed = Some(Accumulator::new(
                        accumulator.name.clone(),
                        accumulator.value_type.clone(),
                        combine,
                    ));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Rule that combines the values of Accumulate nodes");
        if let Some(accumulator) = changed {
            let command = EditCommand::SetAccumulator {
                scope: scope.clone(),
                node: id,
                accumulator,
            };
            self.edit(vec![command], None);
        }
        let remove = add_button(
            ui,
            true,
            Button::new(RichText::new(MDI_CLOSE).small()).frame(false),
            "Remove accumulator",
        );
        if remove.on_hover_text("Remove accumulator").clicked() {
            let command = EditCommand::RemoveAccumulator {
                scope,
                node: id,
                name: accumulator.name.clone(),
            };
            self.edit(vec![command], None);
        }
    }

    /// Draws the node menu's entries that move the node `id` into a region of the body, or out of its region.
    ///
    /// A node moved out of its region is also placed right of the region's frame, with every node inside it, when it
    /// would otherwise still be drawn inside the frame and read as a member.
    fn region_menus(&mut self, ui: &mut Ui, id: NodeId, snarl: &Snarl<NodeView>) {
        let body = self.context.body;
        let document = self.context.document;
        let Some(node) = body.node(id) else { return };
        let contents = body.region_contents(id);
        let targets: Vec<(NodeId, String)> = body
            .nodes
            .iter()
            .filter(|other| {
                other.id != id
                    && Some(other.id) != node.region
                    && !contents.contains(&other.id)
                    && lookup(&other.op, document.library).is_some_and(|def| def.is_region)
                    && can_join(body, id, other.id)
            })
            .map(|other| {
                let label = lookup(&other.op, document.library).map_or(other.op.as_str(), |def| def.label);
                (other.id, format!("{label} {}", other.id))
            })
            .collect();
        let scope = self.context.scope.clone();
        if !targets.is_empty() {
            ui.menu_button("Move into region", |ui| {
                for (region, label) in targets {
                    if ui.button(label).clicked() {
                        let command = EditCommand::SetRegion {
                            scope: scope.clone(),
                            node: id,
                            region: Some(region),
                        };
                        self.edit(vec![command], None);
                        ui.close();
                    }
                }
            });
        }
        let parent = node
            .region
            .and_then(|region| body.node(region))
            .map(|region| region.region);
        let can_leave = parent.is_some_and(|parent| parent.is_none_or(|parent| can_join(body, id, parent)));
        if let Some(region) = node.region
            && can_leave
            && ui
                .button("Move out of region")
                .on_hover_text(format!("Move node out of region {region}"))
                .clicked()
        {
            let parent = body.node(region).and_then(|known| known.region);
            let mut commands = vec![EditCommand::SetRegion {
                scope,
                node: id,
                region: parent,
            }];
            commands.extend(self.moves_out_of_frame(snarl, body, id, region));
            self.edit(commands, None);
            ui.close();
        }
    }
}

/// Returns whether node `node` of `body`, with every node inside it, can move into the region `region`.
///
/// A node that is no region joins any region. A region joins as library 1 nests regions: a Repeat holds a Repeat or a
/// neighborhood fold, a neighborhood fold holds a Repeat, and a radius fold holds no region and sits in none. The
/// regions around the deepest one inside the moved node number at most [`MAX_REGION_DEPTH`] after the move.
pub fn can_join(body: &NodeBody, node: NodeId, region: NodeId) -> bool {
    let (Some(moved), Some(target)) = (body.node(node), body.node(region)) else {
        return false;
    };
    if !REGION_OPS.contains(&moved.op.as_str()) {
        return true;
    }
    let base = body.region_path(node).len();
    let inside = body
        .region_contents(node)
        .into_iter()
        .filter(|member| {
            body.node(*member)
                .is_some_and(|known| REGION_OPS.contains(&known.op.as_str()))
        })
        .map(|member| body.region_path(member).len() - base)
        .max()
        .unwrap_or(0);
    nests(body, &moved.op, target, inside)
}

/// Returns whether a new node of the kind `op` can be added to the region `region` of `body`, or to the top of the
/// body when `region` is `None`, by the rules of [`can_join`].
pub fn can_add(body: &NodeBody, op: &str, region: Option<NodeId>) -> bool {
    let Some(region) = region else {
        return true;
    };
    let Some(target) = body.node(region) else {
        return false;
    };
    !REGION_OPS.contains(&op) || nests(body, op, target, 0)
}

/// Returns whether a region of the kind `op` can sit in the region `target` of `body`, with regions nested `inside`
/// levels deep inside it.
fn nests(body: &NodeBody, op: &str, target: &Node, inside: usize) -> bool {
    let allowed = matches!(
        (target.op.as_str(), op),
        (OP_REGION_REPEAT, OP_REGION_REPEAT | OP_GRID_FOLD) | (OP_GRID_FOLD, OP_REGION_REPEAT)
    );
    let depth = body.region_path(target.id).len() + 1 + 1 + inside;
    allowed && depth <= MAX_REGION_DEPTH
}

/// Returns the named inputs of the expression `node`, from its `config.inputs`, leaving out an entry that does not
/// read.
fn expression_inputs(node: &Node) -> Vec<(String, ValueType)> {
    let Some(ConfigValue::Array(items)) = node.config.get("inputs") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let ConfigValue::Table(table) = item else { return None };
            let name = table.get("name")?.as_str()?;
            let value_type = table.get("type")?.as_str()?.parse().ok()?;
            Some((name.to_owned(), value_type))
        })
        .collect()
}

/// Returns the `config.inputs` list of an expression that holds `inputs`.
fn expression_input_list(inputs: &[(String, ValueType)]) -> ConfigValue {
    ConfigValue::Array(
        inputs
            .iter()
            .map(|(name, value_type)| {
                ConfigValue::Table(BTreeMap::from([
                    ("name".to_owned(), ConfigValue::String(name.clone())),
                    ("type".to_owned(), ConfigValue::String(value_type.to_string())),
                ]))
            })
            .collect(),
    )
}

/// Returns the commands that move the value of the input `from` of `node` to the input `to`.
fn moved_input(scope: &Scope, node: &Node, from: &str, to: &str) -> Vec<EditCommand> {
    let Some(value) = node.inputs.get(from) else {
        return Vec::new();
    };
    let port = to.to_owned();
    let set = match value.clone() {
        InputValue::Link(source) => EditCommand::Connect {
            scope: scope.clone(),
            node: node.id,
            port,
            source,
        },
        InputValue::Literal(literal) => EditCommand::SetLiteral {
            scope: scope.clone(),
            node: node.id,
            port,
            literal,
        },
        InputValue::Param(param) => EditCommand::BindParam {
            scope: scope.clone(),
            node: node.id,
            port,
            param,
        },
        _ => return Vec::new(),
    };
    vec![
        set,
        EditCommand::Disconnect {
            scope: scope.clone(),
            node: node.id,
            port: from.to_owned(),
        },
    ]
}

/// Returns whether `view` is the input half of a region, whose body holds the region controls.
fn is_region_input(view: &NodeView) -> bool {
    view.part == NodePart::RegionInput
}

/// Returns `acc`, or `acc` with the first number from 2 that no accumulator in `taken` holds.
fn unused_accumulator_name(taken: &[Accumulator]) -> String {
    let free = |name: &str| !taken.iter().any(|known| known.name == name);
    if free("acc") {
        return "acc".to_owned();
    }
    (2..)
        .map(|number| format!("acc_{number}"))
        .find(|name| free(name))
        .unwrap_or_else(|| "acc".to_owned())
}

/// Returns the types an accumulator or a function port of a GPU pass takes: the scalars and vectors, then each category
/// of `document`.
pub fn value_types(document: &GraphDocument) -> Vec<ValueType> {
    let mut types = vec![
        ValueType::Bool,
        ValueType::U32,
        ValueType::I32,
        ValueType::F32,
        ValueType::Vec2U,
        ValueType::Vec2I,
        ValueType::Vec2F,
    ];
    types.extend(
        document
            .categories
            .iter()
            .map(|category| ValueType::Category(category.id.clone())),
    );
    types
}

/// Returns the first variant of the category `value_type` names, as a constant.
fn first_variant(document: &GraphDocument, value_type: Option<&ValueType>) -> Option<Literal> {
    let Some(ValueType::Category(id)) = value_type else {
        return None;
    };
    let category = document.categories.iter().find(|category| category.id == *id)?;
    let variant = category.variants.first()?;
    Some(Literal::Category(MemberName {
        owner: id.clone(),
        member: variant.name.clone(),
    }))
}

/// Returns the colour a badge or a stroke of `issues` takes: red for an error, orange for a warning alone.
fn issue_colors(issues: &NodeIssues) -> Option<[Color32; 3]> {
    if !issues.errors.is_empty() {
        Some([mcs::RED_900, mcs::RED_200, mcs::RED_500])
    } else if !issues.warnings.is_empty() {
        Some([mcs::ORANGE_900, mcs::ORANGE_200, mcs::ORANGE_500])
    } else {
        None
    }
}

impl SnarlViewer<NodeView> for GraphViewer<'_> {
    fn title(&mut self, node: &NodeView) -> String {
        node.title.clone()
    }

    fn node_frame(
        &mut self,
        default: Frame,
        node: PinNode,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<NodeView>,
    ) -> Frame {
        match snarl.get_node(node).and_then(|view| issue_colors(&view.issues)) {
            Some([_, _, stroke]) => default.stroke(Stroke::new(2.0, stroke)),
            None => default,
        }
    }

    fn show_header(
        &mut self,
        node: PinNode,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let Some(view) = snarl.get_node(node) else { return };
        // A selectable label takes the press, and the node would not drag by its title.
        ui.add(Label::new(RichText::new(&view.title).strong()).selectable(false));
        ui.add(Label::new(RichText::new(view.id.to_string()).small().weak()).selectable(false));
        if let Some([fill, text, _]) = issue_colors(&view.issues) {
            let count = view.issues.errors.len() + view.issues.warnings.len();
            let messages: Vec<&str> = view
                .issues
                .errors
                .iter()
                .chain(&view.issues.warnings)
                .map(String::as_str)
                .collect();
            let badge = Button::new(RichText::new(format!("{MDI_ALERT} {count}")).small().color(text))
                .fill(fill)
                .corner_radius(CornerRadius::ZERO);
            if ui.add(badge).on_hover_text(messages.join("\n")).clicked() {
                self.requests.push(GraphRequest::Frame {
                    scope: self.context.scope.clone(),
                    node: view.id,
                });
            }
        }
    }

    fn inputs(&mut self, node: &NodeView) -> usize {
        node.inputs.len()
    }

    fn outputs(&mut self, node: &NodeView) -> usize {
        node.outputs.len()
    }

    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<NodeView>) -> impl SnarlPin + 'static {
        let Some(view) = snarl.get_node_mut(pin.id.node) else {
            return GraphPin::plain(self.canvas_id);
        };
        let Some(input) = view.inputs.get(pin.id.input).cloned() else {
            return GraphPin::plain(self.canvas_id);
        };
        let mut name = RichText::new(&input.port);
        if input.required && input.shown == InputShown::Empty {
            name = name.color(ui.visuals().warn_fg_color);
        }
        let label = ui.add(Label::new(name).selectable(false));
        let hover = match &input.value_type {
            Some(value_type) => format!("{}: {value_type}", input.port),
            None => input.port.clone(),
        };
        label.on_hover_text(if input.required && input.shown == InputShown::Empty {
            format!("{hover}\nRequired. Connect a wire, set a constant or bind a parameter.")
        } else {
            hover
        });
        match &input.shown {
            InputShown::Literal(_) => self.literal_editor(ui, view, pin.id.input),
            InputShown::Param { id, label } => {
                Frame::NONE
                    .fill(mcs::GRAY_800)
                    .inner_margin(Margin::symmetric(4, 1))
                    .show(ui, |ui| {
                        ui.add(Label::new(RichText::new(label).small()).selectable(false));
                    })
                    .response
                    .on_hover_text(format!("Bound to parameter {id}"));
            }
            InputShown::Wire | InputShown::Empty => {}
        }
        GraphPin {
            info: PinInfo::default()
                .with_shape(rate_shape(input.rate))
                .with_fill(type_color(input.value_type.as_ref())),
            #[cfg(test)]
            key: pin_key(self.canvas_id, view.id, false, pin.id.input),
        }
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<NodeView>) -> impl SnarlPin + 'static {
        let Some(view) = snarl.get_node(pin.id.node) else {
            return GraphPin::plain(self.canvas_id);
        };
        #[cfg(test)]
        let output_node = view.id;
        let Some(output) = view.outputs.get(pin.id.output) else {
            return GraphPin::plain(self.canvas_id);
        };
        let text = if output.inner {
            RichText::new(&output.port).italics()
        } else {
            RichText::new(&output.port)
        };
        let hover = match &output.value_type {
            Some(value_type) => format!("{}: {value_type}", output.port),
            None => output.port.clone(),
        };
        let hover = if output.inner {
            format!("{hover}\nRead inside the region's body.")
        } else {
            hover
        };
        ui.add(Label::new(text).selectable(false)).on_hover_text(hover);
        GraphPin {
            info: PinInfo::default()
                .with_shape(rate_shape(output.rate))
                .with_fill(type_color(output.value_type.as_ref())),
            #[cfg(test)]
            key: pin_key(self.canvas_id, output_node, true, pin.id.output),
        }
    }

    fn has_body(&mut self, node: &NodeView) -> bool {
        !node.settings.is_empty() || is_region_input(node) || node.op == OP_EXPRESSION
    }

    fn show_body(
        &mut self,
        node: PinNode,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let Some(view) = snarl.get_node(node).cloned() else {
            return;
        };
        ui.vertical(|ui| {
            for line in &view.settings {
                ui.add(
                    Label::new(RichText::new(line).small().weak())
                        .selectable(false)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
            if is_region_input(&view) {
                self.region_controls(ui, view.id);
            }
            if view.op == OP_EXPRESSION {
                self.expression_editor(ui, view.id);
            }
        });
    }

    fn final_node_rect(&mut self, node: PinNode, rect: Rect, _ui: &mut Ui, snarl: &mut Snarl<NodeView>) {
        if let Some(info) = snarl.get_node_info(node) {
            self.rects.insert(
                node,
                DrawnRect {
                    rect,
                    position: info.pos,
                },
            );
        }
    }

    fn has_on_hover_popup(&mut self, _node: &NodeView) -> bool {
        true
    }

    fn show_on_hover_popup(
        &mut self,
        node: PinNode,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let Some(view) = snarl.get_node(node) else { return };
        ui.set_max_width(POPUP_WIDTH);
        ui.strong(format!("{} ({})", view.title, view.id));
        ui.weak(&view.op);
        if !view.semantics.is_empty() {
            ui.label(view.semantics);
        }
        for error in &view.issues.errors {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        for warning in &view.issues.warnings {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
    }

    fn connect(&mut self, source: &OutPin, dest: &InPin, snarl: &mut Snarl<NodeView>) {
        if let Some((node, port, link)) = Self::wire_ports(source, dest, snarl) {
            let command = EditCommand::Connect {
                scope: self.context.scope.clone(),
                node,
                port,
                source: link,
            };
            self.edit(vec![command], None);
        }
    }

    fn disconnect(&mut self, _source: &OutPin, dest: &InPin, snarl: &mut Snarl<NodeView>) {
        self.disconnect_input(dest.id, snarl);
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<NodeView>) {
        for remote in &pin.remotes {
            self.disconnect_input(*remote, snarl);
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<NodeView>) {
        self.disconnect_input(pin.id, snarl);
    }

    fn draw_background(
        &mut self,
        background: Option<&BackgroundPattern>,
        viewport: &Rect,
        snarl_style: &SnarlStyle,
        style: &Style,
        painter: &Painter,
        snarl: &Snarl<NodeView>,
    ) {
        if let Some(background) = background {
            background.draw(viewport, snarl_style, style, painter);
        }
        self.region_rects.clear();
        for (index, frame) in self.frames.iter().enumerate() {
            let rect = frame
                .members
                .iter()
                .filter_map(|pin_node| {
                    let position = snarl.get_node_info(*pin_node)?.pos;
                    Some(drawn_rect(self.rects.get(pin_node), position))
                })
                .reduce(Rect::union);
            let Some(rect) = rect.map(|rect| frame_rect(rect, &frame.title)) else {
                continue;
            };
            if let Some(region) = frame.region {
                self.region_rects.push((region, frame.depth, rect));
            }
            let [fill, stroke, text] = frame.colors;
            let stroke = if self.drop_targets.contains(&index) {
                Stroke::new(2.0, mcs::BLUE_500)
            } else {
                Stroke::new(1.0, stroke)
            };
            painter.rect(rect, CornerRadius::ZERO, fill, stroke, StrokeKind::Inside);
            if !frame.title.is_empty() {
                painter.text(
                    rect.left_top() + vec2(6.0, 4.0),
                    Align2::LEFT_TOP,
                    &frame.title,
                    FontId::proportional(11.0),
                    text,
                );
            }
        }
    }

    fn current_transform(&mut self, to_global: &mut TSTransform, _snarl: &mut Snarl<NodeView>) {
        if let Some((target, canvas)) = self.framing.take() {
            *to_global = framing_transform(target, canvas);
        }
        self.transform = *to_global;
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<NodeView>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: Pos2, ui: &mut Ui, _snarl: &mut Snarl<NodeView>) {
        let pos = self.menu_origin(ui, pos);
        let region = self.region_at(pos);
        if let Some(op) = self.quick_add(ui, None, region) {
            self.requests.push(GraphRequest::AddNode {
                op: op.to_owned(),
                pos: [f64::from(pos.x), f64::from(pos.y)],
                region,
                wire: None,
            });
            ui.close();
        }
    }

    fn has_dropped_wire_menu(&mut self, _src_pins: AnyPins<'_>, _snarl: &mut Snarl<NodeView>) -> bool {
        true
    }

    fn show_dropped_wire_menu(&mut self, pos: Pos2, ui: &mut Ui, src_pins: AnyPins<'_>, snarl: &mut Snarl<NodeView>) {
        let pos = self.menu_origin(ui, pos);
        // A node removed while this menu is open leaves its pins here, so every lookup takes `get_node`.
        let dropped = match src_pins {
            AnyPins::Out(pins) => pins.first().and_then(|pin| {
                let view = snarl.get_node(pin.node)?;
                let output = view.outputs.get(pin.output)?;
                Some(DroppedWire {
                    node: view.id,
                    port: output.port.clone(),
                    end: WireEnd::Output(output.value_type.clone()),
                })
            }),
            AnyPins::In(pins) => pins.first().and_then(|pin| {
                let view = snarl.get_node(pin.node)?;
                let input = view.inputs.get(pin.input)?;
                Some(DroppedWire {
                    node: view.id,
                    port: input.port.clone(),
                    end: WireEnd::Input(input.value_type.clone()),
                })
            }),
        };
        let Some(dropped) = dropped else {
            ui.close();
            return;
        };
        let region = self.region_at(pos);
        if let Some(op) = self.quick_add(ui, Some(&dropped.end), region) {
            self.requests.push(GraphRequest::AddNode {
                op: op.to_owned(),
                pos: [f64::from(pos.x), f64::from(pos.y)],
                region,
                wire: Some(dropped),
            });
            ui.close();
        }
    }

    fn has_node_menu(&mut self, _node: &NodeView) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: PinNode,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let Some(view) = snarl.get_node(node).cloned() else {
            ui.close();
            return;
        };
        if ui.button("Frame").clicked() {
            self.requests.push(GraphRequest::Frame {
                scope: self.context.scope.clone(),
                node: view.id,
            });
            ui.close();
        }
        if ui.button("Copy").clicked() {
            self.requests.push(GraphRequest::Copy(vec![view.id]));
            ui.close();
        }
        if view.op == OP_FUNCTION_CALL {
            self.call_menus(ui, view.id);
        }
        if !view.inputs.is_empty() {
            ui.menu_button("Inputs", |ui| self.input_menus(ui, &view));
        }
        if lookup(&view.op, self.context.document.library).is_some_and(|def| {
            def.config
                .iter()
                .any(|key| matches!(key.value, ConfigType::Bool | ConfigType::Word(_) | ConfigType::LaneId))
                || def.config.iter().any(|key| {
                    matches!(
                        key.value,
                        ConfigType::CategoryId
                            | ConfigType::PaletteId
                            | ConfigType::FunctionId
                            | ConfigType::InterfacePort
                    )
                })
        }) {
            ui.menu_button("Settings", |ui| self.setting_menus(ui, &view));
        }
        let outside_regions = self
            .context
            .pass
            .and_then(|pass| pass.body.node(view.id))
            .is_some_and(|node| node.region.is_none());
        let viewable: Vec<&OutputView> = if outside_regions {
            view.outputs
                .iter()
                .filter(|output| !output.inner && can_view(output.value_type.as_ref(), output.rate))
                .collect()
        } else {
            Vec::new()
        };
        if self.context.pass.is_some() && !viewable.is_empty() {
            ui.menu_button("View in viewport", |ui| {
                for output in viewable {
                    if ui.button(&output.port).clicked() {
                        self.requests.push(GraphRequest::View {
                            node: view.id,
                            port: output.port.clone(),
                        });
                        ui.close();
                    }
                }
            });
        }
        let in_stats = self.context.pass.is_some_and(|pass| pass.stage == PassStage::Stats);
        let tick_outputs: Vec<&OutputView> = view
            .outputs
            .iter()
            .filter(|output| output.rate == Rate::Tick && !output.inner)
            .collect();
        if in_stats && !tick_outputs.is_empty() {
            ui.menu_button("Add statistic", |ui| {
                for output in tick_outputs {
                    if ui.button(&output.port).clicked() {
                        let label = unused_label(self.context.document, &output.port);
                        let stat = Stat::new(
                            &label,
                            PortRef {
                                node: view.id,
                                port: Some(output.port.clone()),
                            },
                            [128, 128, 128, 255],
                        );
                        self.edit(vec![EditCommand::AddStat(stat)], None);
                        ui.close();
                    }
                }
            });
        }
        self.region_menus(ui, view.id, snarl);
        ui.separator();
        if ui.button("Delete").clicked() {
            self.requests.push(GraphRequest::Delete(vec![view.id]));
            ui.close();
        }
    }
}

/// Returns `wanted`, or `wanted` with the first number from 2 that makes it a statistic label the file does not hold.
fn unused_label(document: &GraphDocument, wanted: &str) -> String {
    let taken = |label: &str| document.stats.iter().any(|stat| stat.label == label);
    if !taken(wanted) {
        return wanted.to_owned();
    }
    (2..)
        .map(|number| format!("{wanted} {number}"))
        .find(|label| !taken(label))
        .unwrap_or_else(|| wanted.to_owned())
}

/// Moves the accessible bounds of each widget drawn on `layer` this pass from the layer's coordinates to the screen's.
///
/// egui writes a widget's bounds in the coordinates of its layer and ignores the layer's transform. Without the move,
/// an accessibility client such as a screen reader or the egui MCP server places a widget on a node where it would
/// sit at scale 1 with no pan. A layer with no transform, or a pass with AccessKit off, is left alone.
fn place_accessible_nodes(ctx: &egui::Context, layer: LayerId) {
    let Some(transform) = ctx.layer_transform_to_global(layer) else {
        return;
    };
    let ids: Vec<Id> = ctx.viewport(|viewport| {
        let pass = &viewport.this_pass;
        let Some(state) = &pass.accesskit_state else {
            return Vec::new();
        };
        pass.widgets
            .get_layer(layer)
            .map(|widget| widget.id)
            .filter(|id| state.nodes.contains_key(id))
            .collect()
    });
    for id in ids {
        ctx.accesskit_node_builder(id, |node| {
            if let Some(bounds) = node.bounds() {
                let local = Rect::from_min_max(
                    pos2(bounds.x0 as f32, bounds.y0 as f32),
                    pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                let screen = transform * local;
                node.set_bounds(egui::accesskit::Rect {
                    x0: screen.min.x.into(),
                    y0: screen.min.y.into(),
                    x1: screen.max.x.into(),
                    y1: screen.max.y.into(),
                });
            }
        });
    }
}

/// Draws `canvas` for one frame and returns the requests of its gestures.
///
/// A plain click selects: on a frame that releases a click with no modifier held, shift and command are written into
/// the input state for the draw and restored after it. egui-snarl selects only with shift held.
pub fn show_canvas(ui: &mut Ui, canvas: &mut BodyCanvas, context: CanvasContext<'_>) -> Vec<GraphRequest> {
    let canvas_rect = ui.available_rect_before_wrap();
    // A framing holds until the rectangle it shows stops changing. A node's first draws can size it differently.
    let framing = canvas.framing.and_then(|target| {
        let rect = match target {
            Framing::Node(node) => canvas.current_rect(node),
            Framing::All => canvas.all_rect(),
        };
        let drawn = match target {
            Framing::Node(node) => canvas
                .nodes
                .get(&node)
                .is_some_and(|pin_node| canvas.rects.contains_key(pin_node)),
            Framing::All => canvas
                .nodes
                .values()
                .all(|pin_node| canvas.rects.contains_key(pin_node)),
        };
        let settled = drawn && rect.is_some() && canvas.framed == rect;
        if settled || rect.is_none() {
            canvas.framing = None;
            canvas.framed = None;
        } else {
            canvas.framed = rect;
        }
        rect.map(|rect| (rect, canvas_rect))
    });
    let restore = ui.ctx().input_mut(|input| {
        let is_plain_click = input.pointer.primary_released()
            && input.pointer.could_any_button_be_click()
            && input.modifiers.is_none()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|position| canvas_rect.contains(position));
        let held = input.modifiers;
        if is_plain_click {
            input.modifiers.shift = true;
            input.modifiers.command = true;
            input.modifiers.mac_cmd = cfg!(target_os = "macos");
        }
        is_plain_click.then_some(held)
    });
    let style = canvas_style(ui.style());
    let drop_targets = if ui.ctx().input(|input| input.pointer.primary_down()) {
        canvas
            .regions_entered(context.body)
            .into_iter()
            .map(|(_, _, frame)| frame)
            .collect()
    } else {
        Vec::new()
    };
    let mut viewer = GraphViewer {
        canvas_id: canvas.canvas_id,
        context,
        frames: &canvas.frames,
        drop_targets,
        region_rects: Vec::new(),
        framing,
        rects: &mut canvas.rects,
        query: &mut canvas.query,
        menu_origin: &mut canvas.menu_origin,
        menu_shown: false,
        requests: Vec::new(),
        transform: canvas.transform,
    };
    SnarlWidget::new()
        .id(canvas.canvas_id)
        .style(style)
        .show(&mut canvas.snarl, &mut viewer, ui);
    place_accessible_nodes(ui.ctx(), LayerId::new(ui.layer_id().order, canvas.canvas_id));
    let GraphViewer {
        requests,
        transform,
        menu_shown,
        ..
    } = viewer;
    if !menu_shown {
        canvas.menu_origin = None;
    }
    canvas.transform = transform;
    canvas.canvas_rect = canvas_rect;
    if let Some(held) = restore {
        ui.ctx().input_mut(|input| input.modifiers = held);
    }
    requests
}
