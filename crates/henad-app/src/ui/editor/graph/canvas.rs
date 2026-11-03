//! Canvas of one body: a persistent egui-snarl [`Snarl`] kept in step with the body of the document, and its viewer.
//!
//! The document is the truth. Each gesture on the canvas becomes a [`GraphRequest`], which the editor runs as
//! henad-graph edit commands once the primary button is up and the canvas has drawn. [`BodyCanvas::sync`] then brings
//! the `Snarl` to the document by difference: nodes that left are removed, new nodes inserted, and the rest updated in
//! place, so egui-snarl's selection and sizes stay with their nodes.
//!
//! A *pin* is a socket as egui-snarl numbers it, by position. A node's pins follow the order of its kind's resolved
//! ports, outer outputs before inner outputs, and the views below map each pin back to a port name.

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
use henad_graph::catalogue::{ConfigType, InputDefault, SocketType, effective_config, lookup};
use henad_graph::check::PassReport;
use henad_graph::document::{GraphDocument, Pass, PassStage, Stat};
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::editor::EditorRegion;
use henad_graph::ids::NodeId;
use henad_graph::node::{Node, NodeBody};
use henad_graph::types::{Rate, ValueType};
use henad_graph::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};

use super::quick_add::{WireEnd, offered_kinds};
use super::sockets::{rate_shape, type_color};
use super::view_pass::can_view;
use super::{DroppedWire, GraphEdit, GraphRequest, NodeIssues};
use crate::icons::material_design_icons::MDI_ALERT;
use crate::ui::mcs;

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

/// Node value of a canvas [`Snarl`]: what the viewer draws, and the document node it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    /// Id of the node in the document.
    pub id: NodeId,
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

/// A titled frame behind some nodes: an `[[editor.frame]]` of the file, or a region with its body.
#[derive(Debug, Clone)]
pub struct CanvasFrame {
    /// Text on the frame.
    pub title: String,
    /// Members of the frame.
    pub members: Vec<NodeId>,
    /// Frames around this one.
    pub depth: usize,
    /// Fill, stroke and title colours.
    pub colors: [Color32; 3],
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
    /// [`Snarl`] node of each document node.
    pub nodes: BTreeMap<NodeId, PinNode>,
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
        let mut views: BTreeMap<NodeId, NodeView> = BTreeMap::new();
        let mut order: Vec<NodeId> = Vec::with_capacity(body.nodes.len());
        for node in &body.nodes {
            if views.contains_key(&node.id) {
                continue;
            }
            let node_issues = issues.get(&node.id).cloned().unwrap_or_default();
            views.insert(node.id, node_view(node, body, document, editor, report, node_issues));
            order.push(node.id);
        }

        let gone: Vec<NodeId> = self
            .nodes
            .keys()
            .filter(|id| !views.contains_key(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(pin_node) = self.nodes.remove(&id) {
                self.snarl.remove_node(pin_node);
                self.rects.remove(&pin_node);
            }
        }
        for (index, id) in order.iter().enumerate() {
            let Some(view) = views.get(id) else { continue };
            let position = editor.node_layout(*id).map_or_else(
                || pos2(index as f32 * 30.0, index as f32 * 30.0),
                |layout| pos2(layout.pos[0] as f32, layout.pos[1] as f32),
            );
            if let Some(&pin_node) = self.nodes.get(id) {
                if let Some(info) = self.snarl.get_node_info_mut(pin_node) {
                    if info.value != *view {
                        info.value = view.clone();
                    }
                    info.pos = position;
                }
            } else {
                let pin_node = self.snarl.insert_node(position, view.clone());
                self.nodes.insert(*id, pin_node);
            }
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
        self.frames = frames_of(scope, document, editor, body);
    }

    /// Returns the pins of every wire of `body` whose two ends the canvas shows.
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
                let Some(&source) = self.nodes.get(&link.node) else {
                    continue;
                };
                let Some(source_view) = self.snarl.get_node(source) else {
                    continue;
                };
                let output = match &link.port {
                    Some(port) => source_view.outputs.iter().position(|known| known.port == *port),
                    None => (!source_view.outputs.is_empty()).then_some(0),
                };
                if let Some(output) = output {
                    wires.insert((OutPinId { node: source, output }, InPinId { node: dest, input }));
                }
            }
        }
        wires
    }

    /// Returns the rectangle of the node `id` at its current position.
    pub fn current_rect(&self, id: NodeId) -> Option<Rect> {
        let pin_node = *self.nodes.get(&id)?;
        let position = self.snarl.get_node_info(pin_node)?.pos;
        Some(self.rects.get(&pin_node).map_or_else(
            || Rect::from_min_size(position, UNDRAWN_NODE_SIZE),
            |drawn| drawn.rect.translate(position - drawn.position),
        ))
    }

    /// Returns the rectangle around every node, `None` for an empty canvas.
    pub fn all_rect(&self) -> Option<Rect> {
        self.nodes
            .keys()
            .filter_map(|id| self.current_rect(*id))
            .reduce(Rect::union)
    }

    /// Returns the document nodes egui-snarl holds selected, in id order.
    pub fn selected(&self, ctx: &egui::Context) -> Vec<NodeId> {
        let mut selected: Vec<NodeId> = get_selected_nodes(self.canvas_id, ctx)
            .iter()
            .filter_map(|pin_node| self.snarl.get_node(*pin_node))
            .map(|view| view.id)
            .collect();
        selected.sort_unstable();
        selected
    }

    /// Returns one [`EditCommand::Move`] per node whose position on the canvas differs from its placement in `editor`.
    pub fn moved(&self, editor: &EditorRegion) -> Vec<EditCommand> {
        let mut moves = Vec::new();
        for (id, pin_node) in &self.nodes {
            let Some(info) = self.snarl.get_node_info(*pin_node) else {
                continue;
            };
            let placed = editor.node_layout(*id).map(|layout| layout.pos);
            let pos = [f64::from(info.pos.x), f64::from(info.pos.y)];
            let differs =
                placed.is_none_or(|placed| (placed[0] - pos[0]).abs() > 0.01 || (placed[1] - pos[1]).abs() > 0.01);
            if differs {
                moves.push(EditCommand::Move { node: *id, pos });
            }
        }
        moves
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

/// Returns the view of `node` of `body`.
fn node_view(
    node: &Node,
    body: &NodeBody,
    document: &GraphDocument,
    editor: &EditorRegion,
    report: Option<&PassReport>,
    issues: NodeIssues,
) -> NodeView {
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

    let mut outputs = Vec::new();
    if let Some(def) = def {
        let ports = def
            .resolved_outputs(node, document)
            .into_iter()
            .map(|output| (output, false))
            .chain(
                def.resolved_inner_outputs(node)
                    .into_iter()
                    .map(|output| (output, true)),
            );
        for (resolved, inner) in ports {
            let typed = report.and_then(|report| {
                let table = if inner { &report.inner_outputs } else { &report.outputs };
                let ports = table.get(&node.id)?;
                ports.get(&resolved.name)
            });
            outputs.push(OutputView {
                value_type: typed.and_then(|typed| typed.value_type.clone()).or(resolved.declared),
                rate: typed.map_or(Rate::Element, |typed| typed.rate),
                port: resolved.name,
                inner,
            });
        }
    }

    let mut settings = Vec::new();
    if let Some(def) = def {
        for (key, value) in effective_config(def, node) {
            if key == "key" {
                continue;
            }
            settings.push(format!("{key}: {}", config_text(&value)));
        }
    }
    if let Some(region) = node.region {
        let depth = body.region_path(node.id).len();
        settings.push(format!("in region {region}, depth {depth}"));
    }
    NodeView {
        id: node.id,
        op: node.op.clone(),
        title,
        semantics: def.map_or("", |def| def.semantics),
        inputs,
        outputs,
        settings,
        issues,
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

/// Returns the frames of the canvas of `scope`: the editor's frames, then one per region, outermost first.
fn frames_of(scope: &Scope, document: &GraphDocument, editor: &EditorRegion, body: &NodeBody) -> Vec<CanvasFrame> {
    let canvas = scope.canvas();
    let mut frames = Vec::new();
    for frame in editor.frames() {
        if frame.scope != canvas {
            continue;
        }
        frames.push(CanvasFrame {
            title: frame.title,
            members: frame.members,
            depth: 0,
            colors: hue_colors(&frame.color),
        });
    }
    for node in &body.nodes {
        if !lookup(&node.op, document.library).is_some_and(|def| def.is_region) {
            continue;
        }
        let mut members = Vec::new();
        let mut stack = vec![node.id];
        let mut seen = BTreeSet::new();
        while let Some(region) = stack.pop() {
            if !seen.insert(region) {
                continue;
            }
            for member in body.region_members(region) {
                members.push(member.id);
                stack.push(member.id);
            }
        }
        if members.is_empty() {
            continue;
        }
        frames.push(CanvasFrame {
            title: format!("Body of {}", node.id),
            members,
            depth: 1 + body.region_path(node.id).len(),
            colors: [mcs::GRAY_800, mcs::GRAY_500, mcs::GRAY_300],
        });
    }
    frames.sort_by_key(|frame| frame.depth);
    frames
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

    /// Draws the quick-add list and returns the kind picked.
    fn quick_add(&mut self, ui: &mut Ui, wire: Option<&WireEnd>) -> Option<&'static str> {
        let Some(pass) = self.context.pass else {
            ui.weak("Nodes can be added to passes only.");
            return None;
        };
        ui.set_max_width(POPUP_WIDTH);
        let field = ui.add(
            TextEdit::singleline(self.query)
                .hint_text("Search nodes")
                .desired_width(POPUP_WIDTH),
        );
        if !field.has_focus() && self.query.is_empty() {
            field.request_focus();
        }
        let kinds = offered_kinds(self.context.document, pass, wire, self.query);
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
        !node.settings.is_empty()
    }

    fn show_body(
        &mut self,
        node: PinNode,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let Some(view) = snarl.get_node(node) else { return };
        ui.vertical(|ui| {
            for line in &view.settings {
                ui.add(
                    Label::new(RichText::new(line).small().weak())
                        .selectable(false)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
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
        let placed: HashMap<NodeId, (PinNode, Pos2)> = snarl
            .nodes_pos_ids()
            .map(|(pin_node, position, view)| (view.id, (pin_node, position)))
            .collect();
        let current = |id: &NodeId| -> Option<Rect> {
            let (pin_node, position) = *placed.get(id)?;
            Some(self.rects.get(&pin_node).map_or_else(
                || Rect::from_min_size(position, UNDRAWN_NODE_SIZE),
                |drawn| drawn.rect.translate(position - drawn.position),
            ))
        };
        for frame in self.frames {
            let Some(rect) = frame.members.iter().filter_map(current).reduce(Rect::union) else {
                continue;
            };
            let rect = Rect::from_min_max(
                rect.min - vec2(FRAME_MARGIN, FRAME_MARGIN + FRAME_TITLE_HEIGHT),
                rect.max + vec2(FRAME_MARGIN, FRAME_MARGIN),
            );
            let [fill, stroke, text] = frame.colors;
            painter.rect(
                rect,
                CornerRadius::ZERO,
                fill,
                Stroke::new(1.0, stroke),
                StrokeKind::Inside,
            );
            painter.text(
                rect.left_top() + vec2(6.0, 4.0),
                Align2::LEFT_TOP,
                &frame.title,
                FontId::proportional(11.0),
                text,
            );
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
        if let Some(op) = self.quick_add(ui, None) {
            self.requests.push(GraphRequest::AddNode {
                op: op.to_owned(),
                pos: [f64::from(pos.x), f64::from(pos.y)],
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
        if let Some(op) = self.quick_add(ui, Some(&dropped.end)) {
            self.requests.push(GraphRequest::AddNode {
                op: op.to_owned(),
                pos: [f64::from(pos.x), f64::from(pos.y)],
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
        if !view.inputs.is_empty() {
            ui.menu_button("Inputs", |ui| self.input_menus(ui, &view));
        }
        if lookup(&view.op, self.context.document.library).is_some_and(|def| {
            def.config
                .iter()
                .any(|key| matches!(key.value, ConfigType::Bool | ConfigType::Word(_) | ConfigType::LaneId))
                || def
                    .config
                    .iter()
                    .any(|key| matches!(key.value, ConfigType::CategoryId | ConfigType::PaletteId))
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
    let mut viewer = GraphViewer {
        canvas_id: canvas.canvas_id,
        context,
        frames: &canvas.frames,
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
