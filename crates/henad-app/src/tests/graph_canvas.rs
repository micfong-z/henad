//! Tests of the graph canvas on a headless egui context with synthetic input: the canvas follows the document, every
//! gesture reaches it as an edit command, and the egui-snarl traps of record #62 stay guarded.

use egui::epaint::ClippedShape;
use egui::{Context, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, vec2};
use henad_graph::GraphFile;
use henad_graph::document::{Pass, World};
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::value::{InputValue, Literal, PortRef};

use crate::ui::editor::graph::canvas::pin_key;
use crate::ui::editor::graph::{GraphEdit, GraphEditor, GraphRequest, graph_ui};
use crate::ui::mcs;

/// Frames a test runs after a change, long enough for egui's sizing passes to settle.
const SETTLE_FRAMES: usize = 4;

/// Returns node `number`'s id.
pub(super) fn n(number: u32) -> NodeId {
    NodeId::new(number).expect("an id")
}

/// An egui context that draws a [`GraphEditor`] of a starter, driven by synthetic input.
pub(super) struct Harness {
    pub(super) context: Context,
    pub(super) graph: GraphEditor,
    time: f64,
    /// Modifiers held in the coming frames.
    pub(super) modifiers: Modifiers,
}

impl Harness {
    /// Returns a harness over the starter `name`, its tick pass open and arranged, and every node in view.
    pub(super) fn new(name: &str) -> Self {
        let text = henad_graph::starters::starter(name).expect("a starter").text;
        let file = GraphFile::parse(text).expect("the starter reads");
        let mut harness = Self {
            context: Context::default(),
            graph: GraphEditor::new(&file),
            time: 0.0,
            modifiers: Modifiers::NONE,
        };
        harness.settle();
        // The starters place some nodes edge to edge, where a pin of one node covers a pin of the next.
        harness.graph.request(GraphRequest::Arrange);
        harness.settle();
        harness.settle_framing();
        harness
    }

    /// Runs frames until the framing of the open canvas has settled, or panics after twenty.
    pub(super) fn settle_framing(&mut self) {
        for _ in 0..20 {
            let scope = self.scope();
            if self.graph.canvas(&scope).is_some_and(|canvas| canvas.framing.is_none()) {
                return;
            }
            self.frame(Vec::new());
        }
        panic!("the framing never settled");
    }

    /// Returns the nodes selected on the open canvas.
    pub(super) fn selected(&self) -> Vec<NodeId> {
        self.graph
            .canvas(&self.scope())
            .expect("a canvas")
            .selected(&self.context)
    }

    /// Runs one frame with `events`, and returns its output.
    pub(super) fn frame_output(&mut self, mut events: Vec<Event>) -> FullOutput {
        self.time += 0.016;
        events.insert(0, Event::ModifiersChanged(self.modifiers));
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let graph = &mut self.graph;
        let mut output = self.context.run_ui(input, |ui| graph_ui(ui, graph));
        output.textures_delta.clear();
        output
    }

    /// Runs one frame with `events`.
    pub(super) fn frame(&mut self, events: Vec<Event>) {
        drop(self.frame_output(events));
    }

    /// Runs frames with no input.
    pub(super) fn settle(&mut self) {
        for _ in 0..SETTLE_FRAMES {
            self.frame(Vec::new());
        }
    }

    pub(super) fn scope(&self) -> Scope {
        self.graph.scope().clone()
    }

    /// Returns the screen position of a canvas position.
    pub(super) fn screen(&self, canvas: Pos2) -> Pos2 {
        self.graph.canvas(&self.scope()).expect("a canvas").transform * canvas
    }

    /// Returns the screen position of the top of node `id`, a point on its header that holds no widget.
    pub(super) fn node_grip(&self, id: NodeId) -> Pos2 {
        let rect = self
            .graph
            .canvas(&self.scope())
            .and_then(|canvas| canvas.current_rect(id))
            .expect("the node is drawn");
        self.screen(rect.center_top() + vec2(0.0, 4.0))
    }

    /// Returns the screen position of the centre of a pin of node `id`.
    pub(super) fn pin(&self, id: NodeId, output: bool, index: usize) -> Pos2 {
        let canvas = self.graph.canvas(&self.scope()).expect("a canvas");
        let key = pin_key(canvas.canvas_id, id, output, index);
        let rect = self
            .context
            .data(|data| data.get_temp::<Rect>(key))
            .expect("the pin is drawn");
        self.screen(rect.center())
    }

    /// Returns the index of the input `port` of node `id` on the canvas.
    pub(super) fn input_index(&self, id: NodeId, port: &str) -> usize {
        let canvas = self.graph.canvas(&self.scope()).expect("a canvas");
        let view = canvas.snarl.get_node(canvas.nodes[&id]).expect("a view");
        view.inputs
            .iter()
            .position(|input| input.port == port)
            .expect("the port")
    }

    pub(super) fn button(&self, position: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed,
            modifiers: self.modifiers,
        }
    }

    /// Drags the primary button from `start` to `end` in screen points.
    pub(super) fn drag(&mut self, start: Pos2, end: Pos2) {
        self.frame(vec![Event::PointerMoved(start)]);
        self.frame(vec![self.button(start, true)]);
        for step in 1..=6 {
            let reached = start + (end - start) * (step as f32 / 6.0);
            self.frame(vec![Event::PointerMoved(reached)]);
        }
        self.frame(vec![self.button(end, false)]);
        self.settle();
    }

    /// Presses the primary button on `pin` and moves far enough to start a wire, and leaves the button down.
    pub(super) fn start_wire(&mut self, pin: Pos2) {
        self.frame(vec![Event::PointerMoved(pin)]);
        self.frame(vec![self.button(pin, true)]);
        for step in 1..=3 {
            self.frame(vec![Event::PointerMoved(pin + vec2(0.0, step as f32 * 10.0))]);
        }
    }

    /// Clicks the primary button with no modifier.
    pub(super) fn click(&mut self, position: Pos2) {
        self.frame(vec![Event::PointerMoved(position)]);
        self.frame(vec![self.button(position, true)]);
        self.frame(vec![self.button(position, false)]);
        self.settle();
    }

    /// Presses `key` with `modifiers`.
    pub(super) fn press(&mut self, key: Key, modifiers: Modifiers) {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![event(true), event(false)]);
        self.settle();
    }

    /// Returns a screen position on the canvas background, in its bottom right corner.
    pub(super) fn background(&self) -> Pos2 {
        self.graph
            .canvas(&self.scope())
            .expect("a canvas")
            .canvas_rect
            .right_bottom()
            - vec2(12.0, 12.0)
    }

    /// Returns the value of the input `port` of node `id` in the document.
    pub(super) fn input(&self, id: NodeId, port: &str) -> Option<InputValue> {
        let pass = self.graph.open_pass().expect("a pass");
        let node = pass.body.node(id)?;
        node.inputs.get(port).cloned()
    }

    /// Returns whether the open body holds node `id`.
    pub(super) fn holds(&self, id: NodeId) -> bool {
        self.graph.open_pass().expect("a pass").body.node(id).is_some()
    }

    /// Clicks the secondary button at `position`, which opens a menu of the canvas.
    pub(super) fn secondary_click(&mut self, position: Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: position,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(position)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.settle();
    }

    /// Returns the screen rectangle of the one widget whose accessible label or value is `label`.
    pub(super) fn accessible(&mut self, label: &str) -> Rect {
        self.accessible_with_role(label, None)
    }

    /// Returns the screen rectangle of the one widget of role `role`, when set, whose accessible label or value is
    /// `label`.
    pub(super) fn accessible_with_role(&mut self, label: &str, role: Option<egui::accesskit::Role>) -> Rect {
        self.context.enable_accesskit();
        self.frame(Vec::new());
        let output = self.frame_output(Vec::new());
        let update = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("an AccessKit update");
        let found: Vec<Rect> = update
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label) || node.value() == Some(label))
            .filter(|(_, node)| role.is_none_or(|role| node.role() == role))
            .filter_map(|(_, node)| node.bounds())
            .map(|rect| {
                Rect::from_min_max(
                    Pos2::new(rect.x0 as f32, rect.y0 as f32),
                    Pos2::new(rect.x1 as f32, rect.y1 as f32),
                )
            })
            .collect();
        assert_eq!(found.len(), 1, "one widget reads {label}: {found:?}");
        found[0]
    }

    /// Runs `request` now and lets the canvas follow.
    pub(super) fn run(&mut self, request: GraphRequest) {
        self.graph.request(request);
        self.settle();
    }
}

/// Returns the link to the output `port` of node `node`.
pub(super) fn link(node: NodeId, port: &str) -> InputValue {
    InputValue::Link(PortRef {
        node,
        port: Some(port.to_owned()),
    })
}

#[test]
fn the_canvas_shows_every_node_and_wire_of_the_open_pass() {
    let harness = Harness::new("sir.toml");
    let scope = harness.scope();
    assert_eq!(scope, Scope::pass("step"), "a starter opens on its tick pass");
    let canvas = harness.graph.canvas(&scope).expect("a canvas");
    let body = &harness.graph.open_pass().expect("a pass").body;
    assert_eq!(canvas.snarl.nodes().count(), body.nodes.len());
    let links = body
        .nodes
        .iter()
        .flat_map(|node| node.inputs.values())
        .filter(|value| matches!(value, InputValue::Link(_)))
        .count();
    assert_eq!(canvas.snarl.wires().count(), links);
    assert_eq!(canvas.document_wires(body).len(), links);
}

#[test]
fn a_wire_dragged_between_two_pins_runs_one_connect() {
    let mut harness = Harness::new("sir.toml");
    // Clear `b` of n15, then wire n12's output into it.
    harness.run(GraphRequest::Edit(crate::ui::editor::graph::GraphEdit::one(
        EditCommand::Disconnect {
            scope: Scope::pass("step"),
            node: n(15),
            port: "b".to_owned(),
        },
    )));
    let input = harness.input_index(n(15), "b");
    let (source, dest) = (harness.pin(n(12), true, 0), harness.pin(n(15), false, input));

    harness.drag(source, dest);

    assert_eq!(harness.input(n(15), "b"), Some(link(n(12), "out")));
    assert!(harness.graph.history().can_undo());
}

#[test]
fn connecting_an_input_that_holds_a_wire_replaces_it() {
    let mut harness = Harness::new("sir.toml");
    let input = harness.input_index(n(15), "b");
    assert!(
        matches!(harness.input(n(15), "b"), Some(InputValue::Link(_))),
        "the starter wires `b`"
    );
    let (source, dest) = (harness.pin(n(12), true, 0), harness.pin(n(15), false, input));

    harness.drag(source, dest);

    assert_eq!(harness.input(n(15), "b"), Some(link(n(12), "out")));
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let pin_node = canvas.nodes[&n(15)];
    let into_b = canvas
        .snarl
        .wires()
        .filter(|(_, dest)| dest.node == pin_node && dest.input == input)
        .count();
    assert_eq!(
        into_b, 1,
        "egui-snarl takes a second wire into one input, and the canvas follows the document"
    );
}

#[test]
fn a_command_drag_from_an_input_drops_its_wire_in_the_document() {
    let mut harness = Harness::new("sir.toml");
    let input = harness.input_index(n(20), "value");
    assert!(harness.input(n(20), "value").is_some());
    let pin = harness.pin(n(20), false, input);
    let away = pin + vec2(-60.0, 160.0);

    harness.modifiers = Modifiers::COMMAND;
    harness.drag(pin, away);
    harness.modifiers = Modifiers::NONE;
    harness.settle();

    assert_eq!(
        harness.input(n(20), "value"),
        None,
        "the document follows through the lost-wire comparison"
    );
}

#[test]
fn a_node_dragged_by_its_header_moves_in_one_undo_step() {
    let mut harness = Harness::new("vote.toml");
    let node = harness.graph.open_pass().expect("a pass").body.nodes[0].id;
    let before = harness.graph.builder().editor().node_layout(node).expect("placed").pos;
    let depth = harness.graph.history().undo_depth();
    let grip = harness.node_grip(node);

    harness.frame(vec![Event::PointerMoved(grip)]);
    harness.frame(vec![harness.button(grip, true)]);
    for step in 1..=8 {
        harness.frame(vec![Event::PointerMoved(grip + vec2(step as f32 * 5.0, 0.0))]);
        assert_eq!(
            harness.graph.history().undo_depth(),
            depth,
            "nothing is committed while the button is down"
        );
    }
    harness.frame(vec![harness.button(grip + vec2(40.0, 0.0), false)]);
    harness.settle();

    let after = harness.graph.builder().editor().node_layout(node).expect("placed").pos;
    assert!(after[0] > before[0], "{before:?} to {after:?}");
    assert_eq!(harness.graph.history().undo_depth(), depth + 1, "the drag was one step");
    harness.run(GraphRequest::Undo);
    let undone = harness.graph.builder().editor().node_layout(node).expect("placed").pos;
    assert_eq!(undone, before);
}

#[test]
fn a_plain_click_selects_a_node_and_delete_removes_it() {
    let mut harness = Harness::new("sir.toml");
    harness.click(harness.node_grip(n(18)));
    let selected = harness
        .graph
        .canvas(&harness.scope())
        .expect("a canvas")
        .selected(&harness.context);
    assert_eq!(selected, [n(18)]);

    harness.click(harness.node_grip(n(16)));
    let selected = harness
        .graph
        .canvas(&harness.scope())
        .expect("a canvas")
        .selected(&harness.context);
    assert_eq!(selected, [n(16)], "a second plain click replaces the selection");

    harness.frame(vec![Event::PointerMoved(harness.background())]);
    harness.press(Key::Delete, Modifiers::NONE);
    assert!(!harness.holds(n(16)));
    harness.press(Key::Z, Modifiers::COMMAND);
    assert!(harness.holds(n(16)), "undo brings the node back");
}

#[test]
fn a_keyboard_undo_during_a_wire_drag_waits_for_the_release() {
    let mut harness = Harness::new("sir.toml");
    harness.run(GraphRequest::AddNode {
        op: "math.sin".to_owned(),
        pos: [0.0, 600.0],
        region: None,
        wire: None,
    });
    let added = n(52);
    assert!(harness.holds(added));
    harness.settle();
    harness.start_wire(harness.pin(added, true, 0));

    // The undo removes the node under the wire. Run while the button is down, egui-snarl would panic on its next
    // draw.
    let undo = |pressed| Event::Key {
        key: Key::Z,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    };
    harness.frame(vec![undo(true), undo(false)]);
    harness.frame(Vec::new());
    assert!(harness.holds(added), "the undo waits while the button is down");

    let background = harness.background();
    harness.frame(vec![harness.button(background, false)]);
    harness.settle();
    assert!(!harness.holds(added));
}

#[test]
fn a_node_with_an_error_draws_the_red_stroke() {
    let mut harness = Harness::new("sir.toml");
    harness.run(GraphRequest::AddNode {
        op: "math.sin".to_owned(),
        pos: [0.0, 600.0],
        region: None,
        wire: None,
    });
    harness.run(GraphRequest::FrameAll);
    let shapes: Vec<ClippedShape> = harness.frame_output(Vec::new()).shapes;
    let red = shapes
        .iter()
        .filter(|clipped| matches!(&clipped.shape, Shape::Rect(rect) if rect.stroke.color == mcs::RED_500))
        .count();
    assert_eq!(red, 1, "one node has an error");
    assert_eq!(harness.graph.issue_counts(&Scope::pass("step")).0, 1);
}

#[test]
fn framing_a_node_centres_it_on_the_canvas() {
    let mut harness = Harness::new("sir.toml");
    harness.run(GraphRequest::Frame {
        scope: Scope::pass("counts"),
        node: n(31),
    });
    assert_eq!(harness.scope(), Scope::pass("counts"));
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let rect = canvas.current_rect(n(31)).expect("drawn");
    let centre = canvas.transform * rect.center();
    assert!((centre - canvas.canvas_rect.center()).length() < 1.0, "{centre:?}");
}

#[test]
fn copied_nodes_paste_with_fresh_ids_and_their_wire() {
    let mut harness = Harness::new("sir.toml");
    harness.graph.request(GraphRequest::Copy(vec![n(12), n(13)]));
    let output = harness.frame_output(Vec::new());
    let text = output
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("the copy reached the clipboard");
    assert!(GraphFile::parse(&text).is_ok(), "the clipboard holds a model file");

    let pointer = harness.background() - vec2(200.0, 200.0);
    harness.frame(vec![Event::PointerMoved(pointer)]);
    harness.frame(vec![Event::Paste(text)]);
    harness.settle();

    let body = &harness.graph.open_pass().expect("a pass").body;
    let pasted: Vec<NodeId> = body
        .nodes
        .iter()
        .filter(|node| node.id.number() >= 52)
        .map(|node| node.id)
        .collect();
    assert_eq!(pasted, [n(52), n(53)]);
    let base = body.node(n(53)).and_then(|node| node.inputs.get("base"));
    assert_eq!(
        base,
        Some(&InputValue::Link(PortRef {
            node: n(52),
            port: None
        }))
    );
    let editor = harness.graph.builder().editor();
    let corner = pasted
        .iter()
        .filter_map(|id| editor.node_layout(*id))
        .map(|layout| layout.pos)
        .reduce(|left, right| [left[0].min(right[0]), left[1].min(right[1])])
        .expect("placed");
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let expected = canvas.transform.inverse() * pointer;
    assert!(
        (corner[0] - f64::from(expected.x)).abs() < 1.0 && (corner[1] - f64::from(expected.y)).abs() < 1.0,
        "the pasted nodes sit at {corner:?}, and the pointer at {expected:?}"
    );
}

/// Returns the text of every text shape in `shapes`, nested shapes included.
pub(super) fn texts(shapes: &[ClippedShape]) -> Vec<String> {
    fn collect(shape: &Shape, texts: &mut Vec<String>) {
        match shape {
            Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, texts);
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn a_body_opened_again_after_its_canvas_went_shows_no_stale_selection() {
    let mut harness = Harness::new("sir.toml");
    // A node deleted and another added puts the new node on the freed pin, out of body order.
    harness.run(GraphRequest::Delete(vec![n(18)]));
    harness.run(GraphRequest::AddNode {
        op: "math.sin".to_owned(),
        pos: [0.0, 600.0],
        region: None,
        wire: None,
    });
    let added = n(52);
    harness.run(GraphRequest::FrameAll);
    harness.settle_framing();
    harness.click(harness.node_grip(added));
    assert_eq!(harness.selected(), [added]);

    // Removing the pass drops its canvas, and Undo brings the pass back.
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::RemovePass(
        "step".to_owned(),
    ))));
    assert!(harness.graph.canvas(&Scope::pass("step")).is_none());
    harness.run(GraphRequest::Undo);
    harness.run(GraphRequest::Open(Scope::pass("step")));
    assert_eq!(harness.scope(), Scope::pass("step"));
    let selected = harness.selected();
    assert!(
        selected.is_empty(),
        "the new canvas shows {selected:?} as selected from the old canvas's state"
    );
}

#[test]
fn a_pass_named_after_a_renamed_pass_gets_a_canvas_of_its_own() {
    let mut harness = Harness::new("sir.toml");
    let step = harness.graph.canvas(&Scope::pass("step")).expect("a canvas").canvas_id;
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::RenamePass {
        from: "step".to_owned(),
        to: "spread".to_owned(),
    })));
    let spread = harness
        .graph
        .canvas(&Scope::pass("spread"))
        .expect("a canvas")
        .canvas_id;
    assert_eq!(spread, step, "the canvas follows its pass");

    let domain = harness.graph.open_pass().expect("a pass").domain;
    let pass = Pass::new("step", henad_graph::document::PassStage::Tick, domain);
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::AddPass(pass))));
    harness.run(GraphRequest::Open(Scope::pass("step")));
    let fresh = harness.graph.canvas(&Scope::pass("step")).expect("a canvas").canvas_id;
    assert_ne!(fresh, spread, "two canvases share one egui state");
}

#[test]
fn two_drags_of_one_field_are_two_undo_steps() {
    let mut harness = Harness::new("sir.toml");
    let start = harness.graph.text();
    let literal = |value: f32| {
        GraphRequest::Edit(GraphEdit {
            commands: vec![EditCommand::SetLiteral {
                scope: Scope::pass("step"),
                node: n(12),
                port: "a".to_owned(),
                literal: Literal::F32(value),
            }],
            coalesce: Some("literal pass.step n12 a".to_owned()),
        })
    };
    let background = harness.background();
    let mut ends = Vec::new();
    // Each drag sends one edit per frame under one key while the button is down, as the field does.
    for values in [[0.5, 0.6], [0.7, 0.8]] {
        harness.frame(vec![Event::PointerMoved(background)]);
        harness.frame(vec![harness.button(background, true)]);
        for value in values {
            harness.graph.request(literal(value));
            harness.frame(Vec::new());
        }
        harness.frame(vec![harness.button(background, false)]);
        harness.settle();
        ends.push(harness.graph.text());
    }
    assert_eq!(harness.input(n(12), "a"), Some(InputValue::Literal(Literal::F32(0.8))));

    harness.run(GraphRequest::Undo);
    assert_eq!(
        harness.graph.text(),
        ends[0],
        "undo steps back over the second drag alone"
    );
    harness.run(GraphRequest::Undo);
    assert_eq!(harness.graph.text(), start);
}

#[test]
fn a_dragged_world_field_shows_its_value_before_the_release() {
    let mut harness = Harness::new("sir.toml");
    let mut world = harness.graph.document().world;
    let World::Grid(grid) = &mut world else {
        panic!("the SIR starter is a grid model");
    };
    assert_ne!(grid.width, 777);
    grid.width = 777;
    let width = |harness: &Harness| match harness.graph.document().world {
        World::Grid(grid) => grid.width,
        _ => 0,
    };

    let background = harness.background();
    harness.frame(vec![Event::PointerMoved(background)]);
    harness.frame(vec![harness.button(background, true)]);
    harness.graph.request(GraphRequest::Edit(GraphEdit {
        commands: vec![EditCommand::SetWorld(world)],
        coalesce: Some("world".to_owned()),
    }));
    let output = harness.frame_output(Vec::new());
    assert_ne!(width(&harness), 777, "the edit waits for the release");
    assert!(
        texts(&output.shapes).iter().any(|text| text == "777"),
        "the Size field shows the document's width during the drag"
    );

    harness.frame(vec![harness.button(background, false)]);
    harness.settle();
    assert_eq!(width(&harness), 777);
}

/// The world table and a parameter's default keep a value the file holds outside their range: the frames after a load,
/// and the frames that open the parameter's form, leave the document as it was read.
#[test]
fn the_world_panel_keeps_a_value_outside_its_range() {
    let cases = [
        (
            "sir.toml",
            [("width = 1024", "width = 20000"), ("default = 0.3", "default = 1.5")],
            Some("Infection Rate"),
        ),
        (
            "boids.toml",
            [("count = 50000", "count = 2000000"), ("width = 1000.0", "width = 0.5")],
            None,
        ),
    ];
    for (name, edits, parameter) in cases {
        let mut text = henad_graph::starters::starter(name).expect("a starter").text.to_owned();
        for (old, new) in edits {
            assert!(text.contains(old), "{name} holds {old}");
            text = text.replacen(old, new, 1);
        }
        let file = GraphFile::parse(&text).expect("the file reads");
        let mut harness = Harness {
            context: Context::default(),
            graph: GraphEditor::new(&file),
            time: 0.0,
            modifiers: Modifiers::NONE,
        };
        let read = harness.graph.text();
        harness.settle();
        assert!(harness.graph.text() == read, "{name}: a frame changed the file");
        if let Some(parameter) = parameter {
            // The parameter's chips on the canvas read its label too, and its form's header sits in the side panel
            // at the left.
            harness.context.enable_accesskit();
            let output = harness.frame_output(Vec::new());
            let update = output.platform_output.accesskit_update.expect("an AccessKit update");
            let header = update
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some(parameter))
                .filter_map(|(_, node)| node.bounds())
                .min_by(|a, b| a.x0.total_cmp(&b.x0))
                .expect("the parameter's header");
            harness.click(Pos2::new(
                header.x0 as f32 + 4.0,
                f64::midpoint(header.y0, header.y1) as f32,
            ));
            assert!(
                harness.graph.text() == read,
                "{name}: the parameter's form changed the file"
            );
        }
    }
}

#[test]
fn accessible_bounds_of_a_node_follow_the_canvas_transform() {
    let mut harness = Harness::new("sir.toml");
    harness.run(GraphRequest::Frame {
        scope: Scope::pass("step"),
        node: n(12),
    });
    harness.settle_framing();
    harness.context.enable_accesskit();
    harness.frame(Vec::new());
    let output = harness.frame_output(Vec::new());
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let local = canvas.current_rect(n(12)).expect("drawn");
    let shown = canvas.transform * local;
    assert!(
        !shown.intersects(local),
        "the test needs a transform that moves the node off its own canvas rectangle: {local:?} and {shown:?}"
    );
    let update = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("an AccessKit update");
    let bounds: Vec<Rect> = update
        .nodes
        .iter()
        .filter(|(_, node)| node.label() == Some("Subtract") || node.value() == Some("Subtract"))
        .filter_map(|(_, node)| node.bounds())
        .map(|rect| {
            Rect::from_min_max(
                Pos2::new(rect.x0 as f32, rect.y0 as f32),
                Pos2::new(rect.x1 as f32, rect.y1 as f32),
            )
        })
        .collect();
    assert_eq!(bounds.len(), 1, "one accessible node reads Subtract: {bounds:?}");
    assert!(
        shown.expand(1.0).contains_rect(bounds[0]),
        "the Subtract label of n12 sits at {:?}, and the node is drawn at {shown:?}",
        bounds[0]
    );
}
