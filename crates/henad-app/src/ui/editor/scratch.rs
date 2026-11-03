//! Scratch canvas of the egui-snarl spike, a throwaway node editor behind the `snarl-spike` feature.
//!
//! A *body* is one canvas: a document of nodes keyed by Henad node id, with a [`Snarl`] kept in step as its view. Every
//! gesture becomes a [`ScratchCommand`] that changes both. The readouts beside the canvas exist for the egui inspection
//! server, which reads them through the accessibility tree.
//!
//! A *region* is a node pair, an input half and an output half, with a frame behind its members. The frame is painted
//! from the node rectangles the last frame recorded, moved by any change of node position since.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use egui::emath::TSTransform;
use egui::epaint::Shadow;
use egui::{
    Align2, CentralPanel, Color32, CornerRadius, FontId, Frame, Id, Label, Margin, Painter, Panel, Pos2, Rect,
    ScrollArea, Stroke, StrokeKind, Style, TextEdit, Ui, pos2, vec2,
};
use egui_snarl::ui::{
    AnyPins, BackgroundPattern, Grid, PinInfo, PinPlacement, PinWireInfo, SelectionStyle, SnarlPin, SnarlStyle,
    SnarlViewer, SnarlWidget, WireStyle, get_selected_nodes,
};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use web_time::Instant;

use crate::ui::mcs;

/// Number of plain nodes the stress graph holds, without the region halves.
pub const STRESS_NODES: usize = 300;

/// Number of regions the stress graph holds. Each region adds two halves to the graph.
pub const STRESS_REGIONS: usize = 10;

/// Number of nodes the stress graph holds, the region halves included.
pub const STRESS_TOTAL_NODES: usize = STRESS_NODES + 2 * STRESS_REGIONS;

/// Number of frames the timing readouts average over.
const TIMING_WINDOW: usize = 120;

/// Number of commands the log readout keeps.
const LOG_LENGTH: usize = 14;

/// Largest graph whose pin positions the readouts list.
const PIN_READOUT_LIMIT: usize = 16;

/// Margin in graph points between a region frame and its members.
const REGION_MARGIN: f32 = 14.0;

/// Height in graph points of the title strip on top of a region frame.
const REGION_TITLE_HEIGHT: f32 = 18.0;

/// Graph points left around a framed node.
const FRAMING_MARGIN: f32 = 100.0;

/// Size assumed for a node that has not been drawn yet.
const UNDRAWN_NODE_SIZE: egui::Vec2 = vec2(120.0, 60.0);

/// Lowest zoom of the canvas.
const MIN_SCALE: f32 = 0.2;

/// Highest zoom of the canvas.
const MAX_SCALE: f32 = 2.0;

// Socket colours, which are data colours and not MCS.
const SOCKET_FLOAT: Color32 = Color32::from_rgb(0x6b, 0xa8, 0xd8);
const SOCKET_INTEGER: Color32 = Color32::from_rgb(0x7f, 0xc0, 0x8a);

/// Role of a node in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeRole {
    /// A node that belongs to no region as a half.
    Plain,
    /// One half of the region of this id.
    RegionHalf(String),
}

/// One node of a [`ScratchBody`] document.
#[derive(Clone, Debug)]
pub struct DocumentNode {
    /// Text of the node's header.
    pub title: String,
    /// Name of each input socket.
    pub inputs: Vec<&'static str>,
    /// Name of each output socket.
    pub outputs: Vec<&'static str>,
    /// Value the hover popup shows.
    pub value: f32,
    /// Innermost region around the node.
    pub region: Option<String>,
    /// Graph position the last [`ScratchCommand::Move`] committed.
    pub position: Pos2,
    /// Role of the node.
    pub role: NodeRole,
}

/// One region of a [`ScratchBody`] document.
#[derive(Clone, Debug)]
pub struct RegionNodes {
    /// Text on the frame.
    pub title: String,
    /// Node id of the input half.
    pub input: String,
    /// Node id of the output half.
    pub output: String,
}

/// One wire of a [`ScratchBody`] document.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireRef {
    /// Node id at the output end.
    pub source: String,
    /// Output socket index at the source.
    pub output: usize,
    /// Node id at the input end.
    pub dest: String,
    /// Input socket index at the dest.
    pub input: usize,
}

/// An edit of a body's document and its [`Snarl`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScratchCommand {
    /// Adds a node at a graph position, wired to a pending wire end if one is given.
    AddNode {
        /// Graph position of the node.
        position: Pos2,
        /// Output socket that feeds the new node's first input.
        feed: Option<(String, usize)>,
        /// Input socket the new node's first output feeds.
        consumer: Option<(String, usize)>,
    },
    /// Removes a node. A region half removes its whole region and leaves the members in the region around it.
    RemoveNode(String),
    /// Adds a wire, replacing the wire the input holds.
    Connect(WireRef),
    /// Removes a wire.
    Disconnect(WireRef),
    /// Commits the graph positions of the nodes one drag moved.
    Move(Vec<(String, Pos2)>),
    /// Changes the innermost region around a node.
    SetRegion {
        /// Node id.
        node: String,
        /// Region id, `None` for no region.
        region: Option<String>,
    },
}

/// Node value of a [`Snarl`]: what the viewer draws, and the Henad node id it belongs to.
#[derive(Clone, Debug)]
pub struct NodeView {
    /// Henad node id, the reverse of [`ScratchBody::node_ids`].
    pub id: String,
    /// Text of the node's header.
    pub title: String,
    /// Name of each input socket.
    pub inputs: Vec<&'static str>,
    /// Name of each output socket.
    pub outputs: Vec<&'static str>,
    /// Value the hover popup shows.
    pub value: f32,
}

/// Rectangle a node was last drawn in, with the position it was drawn at.
#[derive(Clone, Copy, Debug)]
pub struct DrawnRect {
    /// Rectangle in graph points, node frame included.
    pub rect: Rect,
    /// Node position when the rectangle was drawn.
    pub position: Pos2,
}

/// A region frame ready to paint.
#[derive(Clone, Debug)]
pub struct RegionFrame {
    /// Region id.
    pub id: String,
    /// Text on the frame.
    pub title: String,
    /// Number of regions around this region.
    pub depth: usize,
    /// Rectangle in graph points.
    pub rect: Rect,
}

/// One canvas: a persistent [`Snarl`] under one egui id, with the document it shows.
#[derive(Debug)]
pub struct ScratchBody {
    /// Egui id of the canvas, under which egui-snarl keeps the transform, selection, draw order and node sizes.
    pub canvas_id: Id,
    /// View of the document.
    pub snarl: Snarl<NodeView>,
    /// Nodes by Henad node id.
    pub nodes: BTreeMap<String, DocumentNode>,
    /// Wires.
    pub wires: BTreeSet<WireRef>,
    /// Regions by id, nested through the `region` of their halves.
    pub regions: BTreeMap<String, RegionNodes>,
    /// [`Snarl`] node of each Henad node id.
    pub node_ids: HashMap<String, NodeId>,
    /// Rectangle of each node in the last frame.
    pub rects: HashMap<NodeId, DrawnRect>,
    /// Transform of the canvas in the last frame.
    pub transform: TSTransform,
    /// Number of nodes created so far, which numbers the next one.
    created: usize,
}

impl ScratchBody {
    /// Returns an empty body under the egui id derived from `salt`.
    pub fn new(salt: &str) -> Self {
        Self {
            canvas_id: Id::new("henad_scratch_canvas").with(salt),
            snarl: Snarl::new(),
            nodes: BTreeMap::new(),
            wires: BTreeSet::new(),
            regions: BTreeMap::new(),
            node_ids: HashMap::new(),
            rects: HashMap::new(),
            transform: TSTransform::IDENTITY,
            created: 0,
        }
    }

    /// Returns the seed graph: four wired nodes, a free node, and a region holding a nested region.
    pub fn seed(salt: &str) -> Self {
        let mut body = Self::new(salt);
        body.insert("n1", "lane.get", &[], &["out"], 0.25, pos2(0.0, 0.0));
        body.insert("n2", "rand.float", &["max"], &["out"], 0.5, pos2(0.0, 120.0));
        body.insert("n3", "math.add", &["a", "b"], &["out"], 0.75, pos2(220.0, 40.0));
        body.insert("n4", "lane.set", &["value"], &[], 0.75, pos2(440.0, 40.0));
        body.insert("n5", "math.sin", &["a"], &["out"], 0.1, pos2(220.0, 200.0));
        body.apply_connect("n1", 0, "n3", 0);
        body.apply_connect("n2", 0, "n3", 1);
        body.apply_connect("n3", 0, "n4", 0);

        body.insert_region("r1", "Repeat r1", pos2(0.0, 340.0), pos2(800.0, 340.0), None);
        body.insert("n6", "math.mul", &["a", "b"], &["out"], 2.0, pos2(200.0, 340.0));
        body.set_region("n6", Some("r1"));
        body.insert_region("r2", "Repeat r2", pos2(380.0, 340.0), pos2(600.0, 420.0), Some("r1"));
        body.insert("n7", "math.abs", &["a"], &["out"], 3.0, pos2(440.0, 420.0));
        body.set_region("n7", Some("r2"));
        body.created = 7;
        body
    }

    /// Returns a graph of [`STRESS_NODES`] plain nodes in columns, wired column to column, with [`STRESS_REGIONS`]
    /// regions.
    pub fn stress(salt: &str) -> Self {
        const ROWS: usize = 20;
        let mut body = Self::new(salt);
        for index in 0..STRESS_NODES {
            let column = index / ROWS;
            let row = index % ROWS;
            let id = format!("n{index}");
            let position = pos2(column as f32 * 240.0, row as f32 * 110.0);
            let value = index as f32 * 0.01;
            body.insert(&id, "math.add", &["a", "b"], &["out"], value, position);
            if column > 0 {
                let upper = format!("n{}", index - ROWS);
                let lower = format!("n{}", (index - ROWS).max(1) - 1);
                body.apply_connect(&upper, 0, &id, 0);
                body.apply_connect(&lower, 0, &id, 1);
            }
        }
        for region in 0..STRESS_REGIONS {
            let id = format!("r{region}");
            let first = region * 30;
            let left = (first / ROWS) as f32 * 240.0;
            let top = (first % ROWS) as f32 * 110.0;
            let title = format!("Repeat {id}");
            body.insert_region(
                &id,
                &title,
                pos2(left - 200.0, top - 60.0),
                pos2(left + 480.0, top - 60.0),
                None,
            );
            for member in first..first + 5 {
                body.set_region(&format!("n{member}"), Some(&id));
            }
        }
        body.created = STRESS_NODES;
        body
    }

    /// Inserts a node into the document and the [`Snarl`].
    fn insert(
        &mut self,
        id: &str,
        kind: &str,
        inputs: &[&'static str],
        outputs: &[&'static str],
        value: f32,
        position: Pos2,
    ) {
        self.insert_node(
            id,
            DocumentNode {
                title: format!("{id} {kind}"),
                inputs: inputs.to_vec(),
                outputs: outputs.to_vec(),
                value,
                region: None,
                position,
                role: NodeRole::Plain,
            },
        );
    }

    /// Inserts `node` under `id`, keeping the document, the [`Snarl`] and the id map in step.
    pub fn insert_node(&mut self, id: &str, node: DocumentNode) {
        let view = NodeView {
            id: id.to_owned(),
            title: node.title.clone(),
            inputs: node.inputs.clone(),
            outputs: node.outputs.clone(),
            value: node.value,
        };
        let node_id = self.snarl.insert_node(node.position, view);
        self.node_ids.insert(id.to_owned(), node_id);
        self.nodes.insert(id.to_owned(), node);
    }

    /// Inserts a region's two halves, inside the region `parent` if one is given.
    fn insert_region(&mut self, id: &str, title: &str, input_at: Pos2, output_at: Pos2, parent: Option<&str>) {
        let input = format!("{id}.in");
        let output = format!("{id}.out");
        for (half, position, inputs, outputs) in [
            (&input, input_at, vec!["count"], vec!["k"]),
            (&output, output_at, vec!["put"], vec!["acc"]),
        ] {
            self.insert_node(
                half,
                DocumentNode {
                    title: format!("{half} {title}"),
                    inputs,
                    outputs,
                    value: 0.0,
                    region: parent.map(str::to_owned),
                    position,
                    role: NodeRole::RegionHalf(id.to_owned()),
                },
            );
        }
        self.regions.insert(
            id.to_owned(),
            RegionNodes {
                title: title.to_owned(),
                input,
                output,
            },
        );
    }

    /// Sets the region of the node `id`, which the document holds.
    fn set_region(&mut self, id: &str, region: Option<&str>) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.region = region.map(str::to_owned);
        }
    }

    /// Adds a wire to both sides, replacing the wire the input holds.
    fn apply_connect(&mut self, source: &str, output: usize, dest: &str, input: usize) {
        let held: Vec<WireRef> = self
            .wires
            .iter()
            .filter(|wire| wire.dest == dest && wire.input == input)
            .cloned()
            .collect();
        for wire in held {
            self.apply_disconnect(&wire);
        }
        if let (Some(&source_node), Some(&dest_node)) = (self.node_ids.get(source), self.node_ids.get(dest)) {
            let out_pin = OutPinId {
                node: source_node,
                output,
            };
            let in_pin = InPinId { node: dest_node, input };
            self.snarl.connect(out_pin, in_pin);
            self.wires.insert(WireRef {
                source: source.to_owned(),
                output,
                dest: dest.to_owned(),
                input,
            });
        }
    }

    /// Removes a wire from both sides.
    fn apply_disconnect(&mut self, wire: &WireRef) {
        if let Some((out_pin, in_pin)) = self.wire_pins(wire) {
            self.snarl.disconnect(out_pin, in_pin);
        }
        self.wires.remove(wire);
    }

    /// Returns the [`Snarl`] pins of `wire`.
    fn wire_pins(&self, wire: &WireRef) -> Option<(OutPinId, InPinId)> {
        let source = *self.node_ids.get(&wire.source)?;
        let dest = *self.node_ids.get(&wire.dest)?;
        Some((
            OutPinId {
                node: source,
                output: wire.output,
            },
            InPinId {
                node: dest,
                input: wire.input,
            },
        ))
    }

    /// Removes one node from both sides and from the rectangle cache.
    fn remove_one(&mut self, id: &str) {
        if let Some(node_id) = self.node_ids.remove(id) {
            self.snarl.remove_node(node_id);
            self.rects.remove(&node_id);
        }
        self.nodes.remove(id);
        self.wires.retain(|wire| wire.source != id && wire.dest != id);
    }

    /// Applies `command` to the document and the [`Snarl`], and returns the line the log shows.
    pub fn apply(&mut self, command: ScratchCommand) -> String {
        match command {
            ScratchCommand::AddNode {
                position,
                feed,
                consumer,
            } => {
                self.created += 1;
                let id = format!("n{}", self.created);
                let value = self.created as f32 * 0.5;
                self.insert(&id, "math.add", &["a", "b"], &["out"], value, position);
                if let Some((source, output)) = feed {
                    self.apply_connect(&source, output, &id, 0);
                }
                if let Some((dest, input)) = consumer {
                    self.apply_connect(&id, 0, &dest, input);
                }
                format!("AddNode {id}")
            }
            ScratchCommand::RemoveNode(id) => {
                let role = self.nodes.get(&id).map(|node| node.role.clone());
                if let Some(NodeRole::RegionHalf(region)) = role {
                    self.remove_region(&region);
                    format!("RemoveNode {id}, region {region}")
                } else {
                    self.remove_one(&id);
                    format!("RemoveNode {id}")
                }
            }
            ScratchCommand::Connect(wire) => {
                let line = format!(
                    "Connect {}.{} to {}.{}",
                    wire.source, wire.output, wire.dest, wire.input
                );
                self.apply_connect(&wire.source, wire.output, &wire.dest, wire.input);
                line
            }
            ScratchCommand::Disconnect(wire) => {
                let line = format!(
                    "Disconnect {}.{} from {}.{}",
                    wire.source, wire.output, wire.dest, wire.input
                );
                self.apply_disconnect(&wire);
                line
            }
            ScratchCommand::Move(moves) => {
                let mut names = Vec::with_capacity(moves.len());
                for (id, position) in moves {
                    if let Some(node) = self.nodes.get_mut(&id) {
                        node.position = position;
                    }
                    names.push(id);
                }
                format!("Move {}", names.join(" "))
            }
            ScratchCommand::SetRegion { node, region } => {
                self.set_region(&node, region.as_deref());
                format!("SetRegion {node} {}", region.as_deref().unwrap_or("none"))
            }
        }
    }

    /// Removes a region's halves, and leaves its members in the region around it.
    fn remove_region(&mut self, region: &str) {
        let Some(nodes) = self.regions.remove(region) else {
            return;
        };
        let parent = self.nodes.get(&nodes.input).and_then(|node| node.region.clone());
        for node in self.nodes.values_mut() {
            if node.region.as_deref() == Some(region) {
                node.region.clone_from(&parent);
            }
        }
        self.remove_one(&nodes.input);
        self.remove_one(&nodes.output);
    }

    /// Returns the rectangle of the node `id` at its current position.
    pub fn current_rect(&self, id: &str) -> Option<Rect> {
        let node_id = *self.node_ids.get(id)?;
        let position = self.snarl.get_node_info(node_id)?.pos;
        Some(self.rects.get(&node_id).map_or_else(
            || Rect::from_min_size(position, UNDRAWN_NODE_SIZE),
            |drawn| drawn.rect.translate(position - drawn.position),
        ))
    }

    /// Returns every region frame, outermost first, leaving the node `excluded` out of the rectangles.
    pub fn region_frames(&self, excluded: Option<&str>) -> Vec<RegionFrame> {
        let mut members: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut parents: HashMap<&str, Option<&str>> = HashMap::new();
        for (id, node) in &self.nodes {
            match &node.role {
                NodeRole::RegionHalf(region) => {
                    members.entry(region).or_default().push(id);
                    if self.regions.get(region).is_some_and(|nodes| nodes.input == *id) {
                        parents.insert(region, node.region.as_deref());
                        if let Some(parent) = &node.region {
                            children.entry(parent).or_default().push(region);
                        }
                    }
                }
                NodeRole::Plain => {
                    if let Some(region) = &node.region
                        && excluded != Some(id.as_str())
                    {
                        members.entry(region).or_default().push(id);
                    }
                }
            }
        }

        let mut frames = Vec::with_capacity(self.regions.len());
        let roots: Vec<&str> = self
            .regions
            .keys()
            .map(String::as_str)
            .filter(|region| parents.get(region).copied().flatten().is_none())
            .collect();
        for root in roots {
            self.push_frames(root, 0, &members, &children, &mut frames);
        }
        frames.sort_by_key(|frame| frame.depth);
        frames
    }

    /// Pushes the frames of `region` and the regions inside it, and returns the rectangle of its own frame.
    fn push_frames(
        &self,
        region: &str,
        depth: usize,
        members: &HashMap<&str, Vec<&str>>,
        children: &HashMap<&str, Vec<&str>>,
        frames: &mut Vec<RegionFrame>,
    ) -> Rect {
        let mut rect = Rect::NOTHING;
        for member in members.get(region).into_iter().flatten() {
            if let Some(member_rect) = self.current_rect(member) {
                rect = rect.union(member_rect);
            }
        }
        for child in children.get(region).into_iter().flatten() {
            rect = rect.union(self.push_frames(child, depth + 1, members, children, frames));
        }
        let rect = Rect::from_min_max(
            rect.min - vec2(REGION_MARGIN, REGION_MARGIN + REGION_TITLE_HEIGHT),
            rect.max + vec2(REGION_MARGIN, REGION_MARGIN),
        );
        let title = self
            .regions
            .get(region)
            .map_or_else(String::new, |nodes| nodes.title.clone());
        frames.push(RegionFrame {
            id: region.to_owned(),
            title,
            depth,
            rect,
        });
        rect
    }

    /// Returns the region a drop of the node `id` lands in: the deepest frame around the node's centre, that frame
    /// computed without the node.
    pub fn drop_target(&self, id: &str) -> Option<String> {
        let center = self.current_rect(id)?.center();
        self.region_frames(Some(id))
            .into_iter()
            .filter(|frame| frame.rect.contains(center))
            .max_by_key(|frame| frame.depth)
            .map(|frame| frame.id)
    }

    /// Returns whether `region` holds `inner` at any depth, or equals it.
    fn region_holds(&self, region: &str, inner: &str) -> bool {
        let mut current = Some(inner.to_owned());
        while let Some(held) = current {
            if held == region {
                return true;
            }
            current = self
                .regions
                .get(&held)
                .and_then(|nodes| self.nodes.get(&nodes.input))
                .and_then(|node| node.region.clone());
        }
        false
    }

    /// Returns the commands of the drag that just ended: one [`ScratchCommand::Move`] over every node whose position
    /// differs from the document, then a [`ScratchCommand::SetRegion`] for each plain node dropped into a region.
    ///
    /// A member leaves its region through the node menu. A drag only stretches its frame.
    pub fn drag_commands(&self) -> Vec<ScratchCommand> {
        let mut moves = Vec::new();
        for (node_id, position, view) in self.snarl.nodes_pos_ids() {
            debug_assert_eq!(self.node_ids.get(&view.id), Some(&node_id), "the id map is one to one");
            if self.nodes.get(&view.id).is_some_and(|node| node.position != position) {
                moves.push((view.id.clone(), position));
            }
        }
        if moves.is_empty() {
            return Vec::new();
        }
        let mut commands = Vec::with_capacity(1 + moves.len());
        for (id, _) in &moves {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if node.role != NodeRole::Plain {
                continue;
            }
            if let Some(target) = self.drop_target(id) {
                let current = node.region.as_deref();
                let joins = current != Some(target.as_str()) && !current.is_some_and(|c| self.region_holds(&target, c));
                if joins {
                    commands.push(ScratchCommand::SetRegion {
                        node: id.clone(),
                        region: Some(target),
                    });
                }
            }
        }
        commands.insert(0, ScratchCommand::Move(moves));
        commands
    }

    /// Returns [`ScratchCommand::Disconnect`] for each document wire the [`Snarl`] no longer holds.
    ///
    /// A command-drag from an input socket drops the input's wires in egui-snarl without a viewer callback.
    pub fn lost_wires(&self) -> Vec<ScratchCommand> {
        let held: HashSet<(OutPinId, InPinId)> = self.snarl.wires().collect();
        self.wires
            .iter()
            .filter(|wire| self.wire_pins(wire).is_some_and(|pins| !held.contains(&pins)))
            .map(|wire| ScratchCommand::Disconnect(wire.clone()))
            .collect()
    }
}

/// Returns the transform that shows `target` at the centre of `canvas`, zoomed to fit with [`FRAMING_MARGIN`].
pub fn framing_transform(target: Rect, canvas: Rect) -> TSTransform {
    let view = target.expand(FRAMING_MARGIN);
    let fit = canvas.size() / view.size();
    let scaling = fit.min_elem().clamp(MIN_SCALE, MAX_SCALE.min(1.0));
    TSTransform {
        scaling,
        translation: canvas.center().to_vec2() - target.center().to_vec2() * scaling,
    }
}

/// Pin that records its rectangle in egui memory as it draws.
struct RecordedPin {
    /// Shape and colours.
    info: PinInfo,
    /// Key of the rectangle, `None` to record nothing.
    key: Option<Id>,
}

impl SnarlPin for RecordedPin {
    fn draw(self, snarl_style: &SnarlStyle, style: &Style, rect: Rect, painter: &Painter) -> PinWireInfo {
        if let Some(key) = self.key {
            painter.ctx().data_mut(|data| data.insert_temp(key, rect));
        }
        self.info.draw(snarl_style, style, rect, painter)
    }
}

/// Returns the key under which a pin's rectangle is recorded.
pub fn pin_key(canvas_id: Id, node: NodeId, output: bool, index: usize) -> Id {
    canvas_id.with(("pin", node, output, index))
}

/// Request of the node menu that changes the view and not the document.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ViewRequest {
    Frame(String),
    ToggleMark(String),
}

/// Viewer of one frame of a body.
struct ScratchViewer<'a> {
    canvas_id: Id,
    record_pins: bool,
    marked: &'a BTreeSet<String>,
    frames: &'a [RegionFrame],
    drop_highlight: Option<&'a str>,
    /// Rectangle to bring into view this frame, with the canvas rectangle.
    framing: Option<(Rect, Rect)>,
    rects: &'a mut HashMap<NodeId, DrawnRect>,
    /// Commands of the frame's gestures, applied after the canvas is drawn.
    commands: Vec<ScratchCommand>,
    requests: Vec<ViewRequest>,
    transform: TSTransform,
}

impl ScratchViewer<'_> {
    /// Returns the document wire between two pins.
    fn wire_ref(source: &OutPin, dest: &InPin, snarl: &Snarl<NodeView>) -> WireRef {
        WireRef {
            source: snarl[source.id.node].id.clone(),
            output: source.id.output,
            dest: snarl[dest.id.node].id.clone(),
            input: dest.id.input,
        }
    }
}

impl SnarlViewer<NodeView> for ScratchViewer<'_> {
    fn title(&mut self, node: &NodeView) -> String {
        node.title.clone()
    }

    fn node_frame(
        &mut self,
        default: Frame,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<NodeView>,
    ) -> Frame {
        if self.marked.contains(&snarl[node].id) {
            default.stroke(Stroke::new(2.0, mcs::RED_500))
        } else {
            default
        }
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        // A selectable label takes the press, and the node would not drag by its title.
        ui.add(Label::new(&snarl[node].title).selectable(false));
    }

    fn inputs(&mut self, node: &NodeView) -> usize {
        node.inputs.len()
    }

    fn outputs(&mut self, node: &NodeView) -> usize {
        node.outputs.len()
    }

    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<NodeView>) -> impl SnarlPin + 'static {
        let view = &snarl[pin.id.node];
        let name = view.inputs.get(pin.id.input).copied().unwrap_or("");
        ui.add(Label::new(name).selectable(false))
            .on_hover_text(format!("{} {name}: {:.3}", view.id, view.value));
        RecordedPin {
            info: PinInfo::circle().with_fill(SOCKET_FLOAT),
            key: self
                .record_pins
                .then(|| pin_key(self.canvas_id, pin.id.node, false, pin.id.input)),
        }
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<NodeView>) -> impl SnarlPin + 'static {
        let view = &snarl[pin.id.node];
        let name = view.outputs.get(pin.id.output).copied().unwrap_or("");
        ui.add(Label::new(name).selectable(false))
            .on_hover_text(format!("{} {name}: {:.3}", view.id, view.value));
        RecordedPin {
            info: PinInfo::square().with_fill(SOCKET_INTEGER),
            key: self
                .record_pins
                .then(|| pin_key(self.canvas_id, pin.id.node, true, pin.id.output)),
        }
    }

    fn final_node_rect(&mut self, node: NodeId, rect: Rect, _ui: &mut Ui, snarl: &mut Snarl<NodeView>) {
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
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let view = &snarl[node];
        ui.label(format!("{} value: {:.3}", view.id, view.value));
    }

    fn connect(&mut self, source: &OutPin, dest: &InPin, snarl: &mut Snarl<NodeView>) {
        self.commands
            .push(ScratchCommand::Connect(Self::wire_ref(source, dest, snarl)));
    }

    fn disconnect(&mut self, source: &OutPin, dest: &InPin, snarl: &mut Snarl<NodeView>) {
        self.commands
            .push(ScratchCommand::Disconnect(Self::wire_ref(source, dest, snarl)));
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<NodeView>) {
        for remote in &pin.remotes {
            let dest = snarl.in_pin(*remote);
            self.commands
                .push(ScratchCommand::Disconnect(Self::wire_ref(pin, &dest, snarl)));
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<NodeView>) {
        for remote in &pin.remotes {
            let source = snarl.out_pin(*remote);
            self.commands
                .push(ScratchCommand::Disconnect(Self::wire_ref(&source, pin, snarl)));
        }
    }

    fn draw_background(
        &mut self,
        background: Option<&BackgroundPattern>,
        viewport: &Rect,
        snarl_style: &SnarlStyle,
        style: &Style,
        painter: &Painter,
        _snarl: &Snarl<NodeView>,
    ) {
        if let Some(background) = background {
            background.draw(viewport, snarl_style, style, painter);
        }
        for frame in self.frames {
            let fill = if frame.depth == 0 { mcs::GRAY_900 } else { mcs::GRAY_800 };
            let stroke = if self.drop_highlight == Some(frame.id.as_str()) {
                Stroke::new(2.0, mcs::BLUE_500)
            } else {
                Stroke::new(1.0, mcs::GRAY_500)
            };
            painter.rect(frame.rect, CornerRadius::ZERO, fill, stroke, StrokeKind::Inside);
            painter.text(
                frame.rect.left_top() + vec2(6.0, 4.0),
                Align2::LEFT_TOP,
                &frame.title,
                FontId::proportional(11.0),
                mcs::GRAY_300,
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
        if ui.button("Add math.add").clicked() {
            self.commands.push(ScratchCommand::AddNode {
                position: pos,
                feed: None,
                consumer: None,
            });
            ui.close();
        }
    }

    fn has_dropped_wire_menu(&mut self, _src_pins: AnyPins<'_>, _snarl: &mut Snarl<NodeView>) -> bool {
        true
    }

    fn show_dropped_wire_menu(&mut self, pos: Pos2, ui: &mut Ui, src_pins: AnyPins<'_>, snarl: &mut Snarl<NodeView>) {
        if ui.button("Add wired math.add").clicked() {
            let (feed, consumer) = match src_pins {
                AnyPins::Out(pins) => (pins.first().map(|pin| (snarl[pin.node].id.clone(), pin.output)), None),
                AnyPins::In(pins) => (None, pins.first().map(|pin| (snarl[pin.node].id.clone(), pin.input))),
            };
            self.commands.push(ScratchCommand::AddNode {
                position: pos,
                feed,
                consumer,
            });
            ui.close();
        }
    }

    fn has_node_menu(&mut self, _node: &NodeView) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeView>,
    ) {
        let id = snarl[node].id.clone();
        if ui.button("Frame").clicked() {
            self.requests.push(ViewRequest::Frame(id.clone()));
            ui.close();
        }
        if ui.button("Mark error").clicked() {
            self.requests.push(ViewRequest::ToggleMark(id.clone()));
            ui.close();
        }
        if ui.button("Move out of region").clicked() {
            self.commands.push(ScratchCommand::SetRegion {
                node: id.clone(),
                region: None,
            });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.commands.push(ScratchCommand::RemoveNode(id));
            ui.close();
        }
    }
}

/// Which body the canvas shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActiveBody {
    /// The pass body.
    #[default]
    Pass,
    /// The function body.
    Function,
}

/// State of the scratch canvas.
#[derive(Debug)]
pub struct ScratchCanvas {
    /// Pass body.
    pub pass: ScratchBody,
    /// Function body.
    pub function: ScratchBody,
    /// Body on the canvas.
    pub active: ActiveBody,
    /// Node ids the error stroke marks.
    pub marked: BTreeSet<String>,
    /// Text of the Target input field.
    pub target: String,
    /// Node waiting to be framed on the next frame.
    pub framing: Option<String>,
    /// Whether wires are axis-aligned with square corners.
    pub axis_aligned: bool,
    /// Whether nodes keep the window shadow.
    pub node_shadow: bool,
    /// Whether the canvas repaints every frame, for the timing readouts.
    pub continuous: bool,
    /// Whether the canvas reads shift as held. The inspection server's modifiers never reach `InputState::modifiers`.
    pub hold_shift: bool,
    /// Whether the canvas reads command as held.
    pub hold_command: bool,
    /// Whether a modifier toggle was on in the last frame.
    was_holding: bool,
    /// Rectangle of the canvas in the last frame.
    pub canvas_rect: Rect,
    /// Last commands applied, newest first.
    pub log: VecDeque<String>,
    /// Number of times the stress graph was loaded, which keeps each load's egui state apart.
    stress_loads: usize,
    /// Time in milliseconds of the last canvas draws.
    show_times: VecDeque<f32>,
    /// Time in milliseconds between the last frames.
    frame_intervals: VecDeque<f32>,
    /// Time in milliseconds eframe spent on the last frame, tessellation and painting included.
    frame_costs: VecDeque<f32>,
}

impl Default for ScratchCanvas {
    fn default() -> Self {
        let mut function = ScratchBody::new("function");
        function.insert("f1", "fn.input", &[], &["x"], 1.0, pos2(0.0, 0.0));
        function.insert("f2", "fn.output", &["x"], &[], 1.0, pos2(240.0, 0.0));
        function.apply_connect("f1", 0, "f2", 0);
        function.created = 2;
        Self {
            pass: ScratchBody::seed("pass"),
            function,
            active: ActiveBody::Pass,
            marked: BTreeSet::new(),
            target: "n3".to_owned(),
            framing: None,
            axis_aligned: false,
            node_shadow: false,
            continuous: false,
            hold_shift: false,
            hold_command: false,
            was_holding: false,
            canvas_rect: Rect::ZERO,
            log: VecDeque::new(),
            stress_loads: 0,
            show_times: VecDeque::new(),
            frame_intervals: VecDeque::new(),
            frame_costs: VecDeque::new(),
        }
    }
}

impl ScratchCanvas {
    /// Returns the body on the canvas.
    pub fn body(&self) -> &ScratchBody {
        match self.active {
            ActiveBody::Pass => &self.pass,
            ActiveBody::Function => &self.function,
        }
    }

    /// Returns the body on the canvas.
    pub fn body_mut(&mut self) -> &mut ScratchBody {
        match self.active {
            ActiveBody::Pass => &mut self.pass,
            ActiveBody::Function => &mut self.function,
        }
    }

    /// Records the time eframe spent on the last frame.
    pub fn record_frame_cost(&mut self, seconds: Option<f32>) {
        if let Some(seconds) = seconds {
            push_sample(&mut self.frame_costs, seconds * 1000.0);
        }
    }

    /// Applies `command` to the body on the canvas and logs it.
    pub fn run_command(&mut self, command: ScratchCommand) {
        let line = self.body_mut().apply(command);
        self.log.push_front(line);
        self.log.truncate(LOG_LENGTH);
    }

    /// Loads the stress graph as a new body under a new egui id.
    pub fn load_stress(&mut self) {
        self.stress_loads += 1;
        self.pass = ScratchBody::stress(&format!("stress{}", self.stress_loads));
        self.active = ActiveBody::Pass;
        self.marked.clear();
        self.show_times.clear();
        self.frame_intervals.clear();
        self.frame_costs.clear();
    }
}

/// Pushes `sample` to the window of the last [`TIMING_WINDOW`] samples.
fn push_sample(samples: &mut VecDeque<f32>, sample: f32) {
    samples.push_back(sample);
    if samples.len() > TIMING_WINDOW {
        samples.pop_front();
    }
}

/// Returns the mean and the worst of `samples`, zero for no samples.
fn mean_and_worst(samples: &VecDeque<f32>) -> (f32, f32) {
    let sum: f32 = samples.iter().sum();
    let worst = samples.iter().copied().fold(0.0, f32::max);
    (sum / samples.len().max(1) as f32, worst)
}

/// Returns the canvas style: square corners on nodes, headers, the selection and axis-aligned wires.
pub fn canvas_style(style: &Style, axis_aligned: bool, node_shadow: bool) -> SnarlStyle {
    let shadow = if node_shadow {
        style.visuals.window_shadow
    } else {
        Shadow::NONE
    };
    let node_frame = Frame::window(style).corner_radius(CornerRadius::ZERO).shadow(shadow);
    SnarlStyle {
        node_frame: Some(node_frame),
        header_frame: Some(node_frame.shadow(Shadow::NONE)),
        select_style: Some(SelectionStyle {
            margin: Margin::same(4),
            rounding: CornerRadius::ZERO,
            fill: mcs::BLUE_900,
            stroke: Stroke::new(1.0, mcs::BLUE_500),
        }),
        wire_style: Some(if axis_aligned {
            WireStyle::AxisAligned { corner_radius: 0.0 }
        } else {
            WireStyle::Bezier5
        }),
        bg_frame: Some(Frame::canvas(style).corner_radius(CornerRadius::ZERO).inner_margin(0)),
        bg_pattern: Some(BackgroundPattern::Grid(Grid::new(vec2(50.0, 50.0), 0.0))),
        pin_placement: Some(PinPlacement::Edge),
        collapsible: Some(false),
        min_scale: Some(MIN_SCALE),
        max_scale: Some(MAX_SCALE),
        ..SnarlStyle::new()
    }
}

/// Draws the toolbar, the readouts and the canvas of the scratch canvas.
pub fn scratch_canvas_ui(ui: &mut Ui, scratch: &mut ScratchCanvas) {
    Panel::top("henad_scratch_toolbar")
        .resizable(false)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 4)))
        .show(ui, |ui| toolbar_ui(ui, scratch));
    Panel::right("henad_scratch_readouts")
        .default_size(280.0)
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("henad_scratch_readouts_scroll")
                .show(ui, |ui| readouts_ui(ui, scratch));
        });
    CentralPanel::no_frame().show(ui, |ui| canvas_ui(ui, scratch));
}

/// Draws the toolbar.
fn toolbar_ui(ui: &mut Ui, scratch: &mut ScratchCanvas) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Add node").clicked() {
            let body = scratch.body();
            let position =
                body.transform.inverse() * scratch.canvas_rect.center() + vec2(body.created as f32 * 8.0, 0.0);
            scratch.run_command(ScratchCommand::AddNode {
                position,
                feed: None,
                consumer: None,
            });
        }
        if ui.button(format!("Load {STRESS_TOTAL_NODES} nodes")).clicked() {
            scratch.load_stress();
        }
        ui.selectable_value(&mut scratch.active, ActiveBody::Pass, "Pass body");
        ui.selectable_value(&mut scratch.active, ActiveBody::Function, "Function body");
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Target");
        ui.add(TextEdit::singleline(&mut scratch.target).desired_width(60.0));
        if ui.button("Frame target").clicked() {
            scratch.framing = Some(scratch.target.clone());
        }
        if ui.button("Mark target").clicked() {
            let target = scratch.target.clone();
            if !scratch.marked.remove(&target) {
                scratch.marked.insert(target);
            }
        }
        ui.checkbox(&mut scratch.axis_aligned, "Axis-aligned wires");
        ui.checkbox(&mut scratch.node_shadow, "Node shadow");
        ui.checkbox(&mut scratch.continuous, "Continuous repaint");
        ui.checkbox(&mut scratch.hold_shift, "Hold shift");
        ui.checkbox(&mut scratch.hold_command, "Hold command");
    });
}

/// Draws the readouts.
fn readouts_ui(ui: &mut Ui, scratch: &mut ScratchCanvas) {
    let body = scratch.body();
    ui.label(format!("Nodes: {}", body.nodes.len()));
    ui.label(format!("Wires: {}", body.wires.len()));
    ui.label(format!(
        "Transform: scale {:.3}, translation {:.1} {:.1}",
        body.transform.scaling, body.transform.translation.x, body.transform.translation.y
    ));
    let selected = get_selected_nodes(body.canvas_id, ui.ctx());
    let selected: Vec<&str> = selected
        .iter()
        .filter_map(|node| body.snarl.get_node(*node))
        .map(|view| view.id.as_str())
        .collect();
    ui.label(format!("Selected: {}", selected.join(" ")));

    let (show_mean, show_worst) = mean_and_worst(&scratch.show_times);
    ui.label(format!("Canvas draw: {show_mean:.2} ms mean, {show_worst:.2} ms worst"));
    let (cost_mean, cost_worst) = mean_and_worst(&scratch.frame_costs);
    ui.label(format!("Frame CPU: {cost_mean:.2} ms mean, {cost_worst:.2} ms worst"));
    let (interval_mean, interval_worst) = mean_and_worst(&scratch.frame_intervals);
    ui.label(format!(
        "Frame interval: {interval_mean:.2} ms mean, {interval_worst:.2} ms worst"
    ));

    ui.separator();
    ui.label("Regions");
    for frame in body.region_frames(None) {
        let members: Vec<&str> = body
            .nodes
            .iter()
            .filter(|(_, node)| node.role == NodeRole::Plain && node.region.as_deref() == Some(frame.id.as_str()))
            .map(|(id, _)| id.as_str())
            .collect();
        ui.label(format!(
            "Region {} depth {}: {}",
            frame.id,
            frame.depth,
            members.join(" ")
        ));
    }

    ui.separator();
    ui.label("Diagnostics");
    let marked: Vec<String> = scratch.marked.iter().cloned().collect();
    for id in marked {
        if ui.button(format!("Problem at {id}")).clicked() {
            scratch.framing = Some(id);
        }
    }

    ui.separator();
    ui.label("Log");
    for line in &scratch.log {
        ui.label(line);
    }

    let body = scratch.body();
    if body.nodes.len() <= PIN_READOUT_LIMIT {
        ui.separator();
        ui.label("Pins in screen points");
        for (id, node) in &body.nodes {
            let Some(&node_id) = body.node_ids.get(id) else {
                continue;
            };
            let sides = [(false, node.inputs.len(), "in"), (true, node.outputs.len(), "out")];
            for (output, count, side) in sides {
                for index in 0..count {
                    let key = pin_key(body.canvas_id, node_id, output, index);
                    if let Some(rect) = ui.ctx().data(|data| data.get_temp::<Rect>(key)) {
                        let center = body.transform * rect.center();
                        ui.label(format!("Pin {id} {side} {index}: {:.1} {:.1}", center.x, center.y));
                    }
                }
            }
        }
    }
}

/// Draws the canvas and applies the commands of the frame.
fn canvas_ui(ui: &mut Ui, scratch: &mut ScratchCanvas) {
    if scratch.continuous {
        ui.ctx().request_repaint();
    }
    push_sample(
        &mut scratch.frame_intervals,
        ui.ctx().input(|input| input.unstable_dt) * 1000.0,
    );

    // The input state keeps its modifiers between frames, so a released toggle writes them back once.
    let holding = scratch.hold_shift || scratch.hold_command;
    if holding || scratch.was_holding {
        ui.ctx().input_mut(|input| {
            input.modifiers.shift = scratch.hold_shift;
            input.modifiers.command = scratch.hold_command;
            input.modifiers.mac_cmd = scratch.hold_command && cfg!(target_os = "macos");
        });
    }
    scratch.was_holding = holding;

    let canvas_rect = ui.available_rect_before_wrap();
    scratch.canvas_rect = canvas_rect;
    let snarl_style = canvas_style(ui.style(), scratch.axis_aligned, scratch.node_shadow);
    let framing_target = scratch.framing.take();
    let pointer_down = ui.ctx().input(|input| input.pointer.primary_down());

    let moving: Vec<String> = if pointer_down {
        scratch
            .body()
            .drag_commands()
            .into_iter()
            .filter_map(|command| match command {
                ScratchCommand::SetRegion { node, .. } => Some(node),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let drop_highlight = moving.first().and_then(|id| scratch.body().drop_target(id));

    let started = Instant::now();
    let marked = std::mem::take(&mut scratch.marked);
    let (commands, requests) = {
        let body = scratch.body_mut();
        let frames = body.region_frames(None);
        let framing = framing_target
            .as_deref()
            .and_then(|id| body.current_rect(id))
            .map(|target| (target, canvas_rect));
        let mut viewer = ScratchViewer {
            canvas_id: body.canvas_id,
            record_pins: body.nodes.len() <= PIN_READOUT_LIMIT,
            marked: &marked,
            frames: &frames,
            drop_highlight: drop_highlight.as_deref(),
            framing,
            rects: &mut body.rects,
            commands: Vec::new(),
            requests: Vec::new(),
            transform: body.transform,
        };
        SnarlWidget::new()
            .id(body.canvas_id)
            .style(snarl_style)
            .show(&mut body.snarl, &mut viewer, ui);
        let ScratchViewer {
            commands,
            requests,
            transform,
            ..
        } = viewer;
        body.transform = transform;
        (commands, requests)
    };
    scratch.marked = marked;
    push_sample(&mut scratch.show_times, started.elapsed().as_secs_f32() * 1000.0);

    for lost in scratch.body().lost_wires() {
        scratch.run_command(lost);
    }
    for command in commands {
        scratch.run_command(command);
    }
    for request in requests {
        match request {
            ViewRequest::Frame(id) => scratch.framing = Some(id),
            ViewRequest::ToggleMark(id) => {
                if !scratch.marked.remove(&id) {
                    scratch.marked.insert(id);
                }
            }
        }
    }
    if !ui.ctx().input(|input| input.pointer.primary_down()) {
        for command in scratch.body().drag_commands() {
            scratch.run_command(command);
        }
    }
}
