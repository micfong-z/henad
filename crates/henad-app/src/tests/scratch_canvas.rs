//! Tests of the scratch canvas, which pin how egui-snarl 0.12 treats node ids, node drags and the pointer under a
//! transform.

use egui::epaint::{ClippedShape, RectShape};
use egui::{
    Context, CornerRadius, Event, FullOutput, Modifiers, PointerButton, Popup, Pos2, RawInput, Rect, Shape, Ui, pos2,
    vec2,
};
use egui_snarl::ui::get_selected_nodes;
use egui_snarl::{InPinId, OutPinId, Snarl};

use crate::ui::editor::scratch::{
    ActiveBody, DocumentNode, NodeRole, STRESS_REGIONS, STRESS_TOTAL_NODES, ScratchBody, ScratchCanvas, ScratchCommand,
    WireRef, framing_transform, pin_key, scratch_canvas_ui,
};
use crate::ui::mcs;

/// Frames a test runs after a change, long enough for egui's sizing passes to settle.
const SETTLE_FRAMES: usize = 4;

/// An egui context that draws a [`ScratchCanvas`], driven by synthetic input.
struct CanvasHarness {
    context: Context,
    canvas: ScratchCanvas,
    time: f64,
    /// Modifiers held in the coming frames.
    modifiers: Modifiers,
    /// Function that draws the canvas in each frame.
    draw: fn(&mut Ui, &mut ScratchCanvas),
}

impl CanvasHarness {
    fn new() -> Self {
        let mut harness = Self {
            context: Context::default(),
            canvas: ScratchCanvas::default(),
            time: 0.0,
            modifiers: Modifiers::NONE,
            draw: scratch_canvas_ui,
        };
        harness.settle();
        harness
    }

    /// Runs one frame with `events`.
    fn frame(&mut self, events: Vec<Event>) {
        drop(self.frame_shapes(events));
    }

    /// Runs one frame with `events`, and returns the shapes it painted.
    fn frame_shapes(&mut self, events: Vec<Event>) -> Vec<ClippedShape> {
        self.frame_output(events, self.draw).shapes
    }

    /// Runs one frame with `events` that `draw` paints, and returns the output of the frame.
    fn frame_output(
        &mut self,
        mut events: Vec<Event>,
        mut draw: impl FnMut(&mut Ui, &mut ScratchCanvas),
    ) -> FullOutput {
        self.time += 0.016;
        events.insert(0, Event::ModifiersChanged(self.modifiers));
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let canvas = &mut self.canvas;
        let mut output = self.context.run_ui(input, |ui| draw(ui, canvas));
        output.textures_delta.clear();
        output
    }

    /// Runs frames with no input.
    fn settle(&mut self) {
        for _ in 0..SETTLE_FRAMES {
            self.frame(Vec::new());
        }
    }

    /// Returns the screen position of a graph position.
    fn screen(&self, graph: Pos2) -> Pos2 {
        self.canvas.body().transform * graph
    }

    /// Returns the screen position of the top of the node `id`, a point on its frame that holds no pin.
    fn node_grip(&self, id: &str) -> Pos2 {
        let rect = self.canvas.body().current_rect(id).expect("the node exists");
        self.screen(rect.center_top() + vec2(0.0, 6.0))
    }

    fn button_event(&self, position: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed,
            modifiers: self.modifiers,
        }
    }

    /// Drags the primary button from `start` to `end` in screen points, in `steps` moves.
    fn drag(&mut self, start: Pos2, end: Pos2, steps: usize) {
        self.frame(vec![Event::PointerMoved(start)]);
        self.frame(vec![self.button_event(start, true)]);
        for step in 1..=steps {
            let reached = start + (end - start) * (step as f32 / steps as f32);
            self.frame(vec![Event::PointerMoved(reached)]);
        }
        self.frame(vec![self.button_event(end, false)]);
        self.settle();
    }

    /// Returns the screen position of the centre of a pin of the node `id`.
    fn pin(&self, id: &str, output: bool, index: usize) -> Pos2 {
        let body = self.canvas.body();
        let key = pin_key(body.canvas_id, body.node_ids[id], output, index);
        let rect = self
            .context
            .data(|data| data.get_temp::<Rect>(key))
            .expect("the pin is drawn");
        self.screen(rect.center())
    }

    /// Presses the primary button on `pin` and moves far enough to start a wire, and leaves the button down.
    fn start_wire(&mut self, pin: Pos2) {
        self.frame(vec![Event::PointerMoved(pin)]);
        self.frame(vec![self.button_event(pin, true)]);
        for step in 1..=3 {
            self.frame(vec![Event::PointerMoved(pin + vec2(0.0, step as f32 * 10.0))]);
        }
    }

    /// Clicks the primary button with the modifiers the harness holds.
    fn click(&mut self, position: Pos2) {
        self.frame(vec![Event::PointerMoved(position)]);
        self.frame(vec![self.button_event(position, true)]);
        self.frame(vec![self.button_event(position, false)]);
        self.settle();
    }

    /// Returns a screen position on the canvas background, in a corner of the canvas.
    fn background(&self) -> Pos2 {
        self.canvas.canvas_rect.right_bottom() - vec2(12.0, 12.0)
    }

    /// Clicks with shift held, which selects a node.
    fn shift_click(&mut self, position: Pos2) {
        self.modifiers = Modifiers::SHIFT;
        self.frame(vec![Event::PointerMoved(position)]);
        self.frame(vec![self.button_event(position, true)]);
        self.frame(vec![self.button_event(position, false)]);
        self.modifiers = Modifiers::NONE;
        self.settle();
    }

    /// Returns the Henad ids of the selected nodes, sorted.
    fn selected(&self) -> Vec<String> {
        let body = self.canvas.body();
        let mut selected: Vec<String> = get_selected_nodes(body.canvas_id, &self.context)
            .iter()
            .map(|node| body.snarl[*node].id.clone())
            .collect();
        selected.sort();
        selected
    }

    /// Returns the lines of the command log that start with `prefix`.
    fn logged(&self, prefix: &str) -> Vec<String> {
        self.canvas
            .log
            .iter()
            .filter(|line| line.starts_with(prefix))
            .cloned()
            .collect()
    }
}

/// Returns a plain document node.
fn plain_node(title: &str, position: Pos2) -> DocumentNode {
    DocumentNode {
        title: title.to_owned(),
        inputs: vec!["a"],
        outputs: vec!["out"],
        value: 0.0,
        region: None,
        position,
        role: NodeRole::Plain,
    }
}

#[test]
fn removing_and_inserting_reuses_the_node_id() {
    let mut snarl = Snarl::<u8>::new();
    let first = snarl.insert_node(Pos2::ZERO, 1);
    let second = snarl.insert_node(Pos2::ZERO, 2);
    snarl.remove_node(first);
    assert_eq!(snarl.insert_node(Pos2::ZERO, 3), first);
    assert_ne!(first, second);
}

#[test]
fn a_node_inserted_in_the_frame_of_a_removal_inherits_the_selection() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n5"));
    assert_eq!(harness.selected(), ["n5"]);

    let body = harness.canvas.body_mut();
    body.apply(ScratchCommand::RemoveNode("n5".to_owned()));
    body.apply(ScratchCommand::AddNode {
        position: pos2(900.0, 900.0),
        feed: None,
        consumer: None,
    });
    harness.settle();

    assert_eq!(harness.selected(), ["n8"], "the new node sits on the freed id");
}

#[test]
fn a_frame_between_removal_and_insertion_clears_the_selection() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n5"));

    harness
        .canvas
        .body_mut()
        .apply(ScratchCommand::RemoveNode("n5".to_owned()));
    harness.settle();
    harness.canvas.body_mut().apply(ScratchCommand::AddNode {
        position: pos2(900.0, 900.0),
        feed: None,
        consumer: None,
    });
    harness.settle();

    assert!(harness.selected().is_empty());
}

#[test]
fn a_snarl_rebuilt_under_one_id_moves_the_selection_to_another_node() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n5"));

    // Undo by rebuild: the same document inserted in another order, under the same egui id.
    let canvas_id = harness.canvas.pass.canvas_id;
    let old = std::mem::replace(&mut harness.canvas.pass, ScratchBody::new("rebuilt"));
    harness.canvas.pass.canvas_id = canvas_id;
    for (id, node) in old.nodes.iter().rev() {
        harness.canvas.pass.insert_node(id, node.clone());
    }
    harness.settle();

    assert_eq!(harness.selected().len(), 1);
    assert_ne!(harness.selected(), ["n5"]);
}

#[test]
fn a_reused_node_id_draws_one_frame_at_the_old_size() {
    let mut harness = CanvasHarness::new();
    let body = harness.canvas.body_mut();
    let long = "a title much longer than any other node title on this canvas";
    body.insert_node("wide", plain_node(long, pos2(900.0, 0.0)));
    harness.settle();
    let wide_rect = harness.canvas.body().current_rect("wide").expect("drawn");

    let body = harness.canvas.body_mut();
    body.apply(ScratchCommand::RemoveNode("wide".to_owned()));
    body.insert_node("narrow", plain_node("x", pos2(900.0, 0.0)));
    harness.frame(Vec::new());
    let first_frame = harness.canvas.body().current_rect("narrow").expect("drawn");
    harness.settle();
    let settled = harness.canvas.body().current_rect("narrow").expect("drawn");

    assert!(
        (first_frame.width() - wide_rect.width()).abs() < 1.0,
        "the size stays under the node id"
    );
    assert!(settled.width() < wide_rect.width() / 2.0);
}

#[test]
fn a_drag_commits_one_move_when_it_ends() {
    let mut harness = CanvasHarness::new();
    let start = harness.node_grip("n5");
    let before = harness.canvas.body().nodes["n5"].position;

    harness.frame(vec![Event::PointerMoved(start)]);
    harness.frame(vec![harness.button_event(start, true)]);
    for step in 1..=10 {
        harness.frame(vec![Event::PointerMoved(start + vec2(step as f32 * 4.0, 0.0))]);
        assert!(
            harness.logged("Move").is_empty(),
            "nothing is committed while the button is down"
        );
        assert_eq!(harness.canvas.body().nodes["n5"].position, before);
    }
    harness.frame(vec![harness.button_event(start + vec2(40.0, 0.0), false)]);
    harness.settle();

    assert_eq!(harness.logged("Move"), ["Move n5"]);
    assert_ne!(harness.canvas.body().nodes["n5"].position, before);
}

#[test]
fn a_node_drags_by_its_title() {
    let mut harness = CanvasHarness::new();
    let rect = harness.canvas.body().current_rect("n5").expect("drawn");
    let title = harness.screen(rect.right_top() + vec2(-30.0, 14.0));

    harness.drag(title, title + vec2(40.0, 0.0), 6);

    assert_eq!(harness.logged("Move"), ["Move n5"]);
}

#[test]
fn a_drag_of_a_selected_node_moves_the_whole_selection_in_one_command() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n1"));
    harness.shift_click(harness.node_grip("n2"));
    let start = harness.node_grip("n2");

    harness.drag(start, start + vec2(30.0, 30.0), 6);

    assert_eq!(harness.logged("Move"), ["Move n1 n2"]);
}

/// Returns the rectangles among `shapes` that fill with `fill`.
fn filled_rects(shapes: &[ClippedShape], fill: egui::Color32) -> Vec<RectShape> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect) if rect.fill == fill => Some(rect.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_selection_and_the_region_frames_paint_square_corners() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n5"));

    let shapes = harness.frame_shapes(Vec::new());

    let selection = filled_rects(&shapes, mcs::BLUE_900);
    assert_eq!(selection.len(), 1);
    assert_eq!(selection[0].corner_radius, CornerRadius::ZERO);
    let frames = filled_rects(&shapes, mcs::GRAY_800);
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.corner_radius == CornerRadius::ZERO)
            .count(),
        frames.len()
    );
    assert!(!frames.is_empty());
}

#[test]
fn a_held_shift_reaches_the_canvas_through_input_state() {
    let mut harness = CanvasHarness::new();
    harness.canvas.hold_shift = true;
    let grip = harness.node_grip("n5");

    harness.frame(vec![Event::PointerMoved(grip)]);
    harness.frame(vec![harness.button_event(grip, true)]);
    harness.frame(vec![harness.button_event(grip, false)]);
    harness.settle();

    assert_eq!(harness.selected(), ["n5"]);
}

#[test]
fn a_plain_click_selects_nothing() {
    let mut harness = CanvasHarness::new();
    let grip = harness.node_grip("n5");

    harness.frame(vec![Event::PointerMoved(grip)]);
    harness.frame(vec![harness.button_event(grip, true)]);
    harness.frame(vec![harness.button_event(grip, false)]);
    harness.settle();

    assert!(harness.selected().is_empty());
}

#[test]
fn a_command_drag_from_an_input_drops_its_wire_without_a_viewer_callback() {
    let mut harness = CanvasHarness::new();
    let wire = WireRef {
        source: "n3".to_owned(),
        output: 0,
        dest: "n4".to_owned(),
        input: 0,
    };
    assert!(harness.canvas.body().wires.contains(&wire));

    // The input pin sits on the left edge of the node, level with its only row.
    let rect = harness.canvas.body().current_rect("n4").expect("drawn");
    let pin = harness.screen(pos2(rect.left(), rect.bottom() - 16.0));
    harness.modifiers = Modifiers::COMMAND;
    harness.drag(pin, pin + vec2(-40.0, 150.0), 6);
    harness.modifiers = Modifiers::NONE;
    harness.settle();

    assert!(
        !harness.canvas.body().wires.contains(&wire),
        "the document follows through the reconcile pass"
    );
    assert_eq!(harness.logged("Disconnect"), ["Disconnect n3.0 from n4.0"]);
}

#[test]
fn framing_centres_the_node_on_the_canvas() {
    let target = Rect::from_min_size(pos2(5000.0, -3000.0), vec2(120.0, 60.0));
    let canvas = Rect::from_min_size(pos2(40.0, 80.0), vec2(800.0, 600.0));

    let transform = framing_transform(target, canvas);

    assert!(((transform * target.center()) - canvas.center()).length() < 0.01);
    assert!(transform.scaling <= 1.0);
}

#[test]
fn the_frame_target_request_moves_the_node_into_view() {
    let mut harness = CanvasHarness::new();
    harness.canvas.framing = Some("n7".to_owned());
    harness.settle();

    let rect = harness.canvas.body().current_rect("n7").expect("drawn");
    let screen = harness.screen(rect.center());
    assert!(harness.context.content_rect().contains(screen));
}

#[test]
fn region_frames_nest_and_follow_their_members_without_lag() {
    let mut harness = CanvasHarness::new();
    let frames = harness.canvas.body().region_frames(None);
    assert_eq!(
        frames.iter().map(|frame| frame.id.as_str()).collect::<Vec<_>>(),
        ["r1", "r2"]
    );
    let (outer, inner) = (frames[0].rect, frames[1].rect);
    assert!(outer.contains_rect(inner));
    let member = harness.canvas.body().current_rect("n7").expect("drawn");
    assert!(inner.contains_rect(member));

    // The position alone changes, as in the frame after a drag delta, and the frame follows it before any redraw.
    let node = harness.canvas.body().node_ids["n7"];
    harness
        .canvas
        .body_mut()
        .snarl
        .get_node_info_mut(node)
        .expect("a node")
        .pos += vec2(0.0, 500.0);
    let moved = harness.canvas.body().region_frames(None);
    assert!((moved[1].rect.bottom() - inner.bottom() - 500.0).abs() < 1.0);
    assert!(moved[0].rect.contains_rect(moved[1].rect));
}

#[test]
fn a_node_dropped_into_a_frame_joins_the_region_and_the_menu_takes_it_out() {
    let mut harness = CanvasHarness::new();
    assert_eq!(harness.canvas.body().nodes["n5"].region, None);
    let frames = harness.canvas.body().region_frames(None);
    let inner = frames[1].rect;

    let grip = harness.node_grip("n5");
    let rect = harness.canvas.body().current_rect("n5").expect("drawn");
    let offset = harness.screen(inner.center()) - harness.screen(rect.center());
    harness.drag(grip, grip + offset, 8);

    assert_eq!(harness.canvas.body().nodes["n5"].region.as_deref(), Some("r2"));
    assert_eq!(harness.logged("SetRegion"), ["SetRegion n5 r2"]);

    harness.canvas.run_command(ScratchCommand::SetRegion {
        node: "n5".to_owned(),
        region: None,
    });
    assert_eq!(harness.canvas.body().nodes["n5"].region, None);
}

#[test]
fn a_member_dragged_past_its_frame_stays_in_the_region() {
    let mut harness = CanvasHarness::new();
    let grip = harness.node_grip("n6");
    harness.drag(grip, grip + vec2(0.0, 600.0), 8);

    assert_eq!(harness.canvas.body().nodes["n6"].region.as_deref(), Some("r1"));
    let frames = harness.canvas.body().region_frames(None);
    let member = harness.canvas.body().current_rect("n6").expect("drawn");
    assert!(frames[0].rect.contains_rect(member), "the frame stretches");
}

#[test]
fn deleting_a_region_half_removes_the_region_and_keeps_its_members() {
    let mut harness = CanvasHarness::new();
    harness
        .canvas
        .run_command(ScratchCommand::RemoveNode("r2.out".to_owned()));
    harness.settle();

    let body = harness.canvas.body();
    assert!(!body.nodes.contains_key("r2.in"));
    assert!(!body.regions.contains_key("r2"));
    assert_eq!(body.nodes["n7"].region.as_deref(), Some("r1"));
}

#[test]
fn each_body_keeps_its_own_transform_and_selection() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n5"));
    harness.canvas.framing = Some("n1".to_owned());
    harness.settle();
    let pass_transform = harness.canvas.pass.transform;

    harness.canvas.active = ActiveBody::Function;
    harness.settle();
    assert!(harness.selected().is_empty());

    harness.canvas.active = ActiveBody::Pass;
    harness.settle();
    assert_eq!(harness.canvas.pass.transform, pass_transform);
    assert_eq!(harness.selected(), ["n5"]);
}

#[test]
fn the_stress_graph_holds_every_node_and_a_frame_per_region() {
    let body = ScratchBody::stress("test");
    assert_eq!(body.snarl.nodes().count(), STRESS_TOTAL_NODES);
    assert_eq!(body.region_frames(None).len(), STRESS_REGIONS);
}

#[test]
fn the_stress_button_names_the_nodes_the_graph_holds() {
    let mut harness = CanvasHarness::new();
    harness.context.enable_accesskit();

    let output = harness.frame_output(Vec::new(), scratch_canvas_ui);

    let update = output.platform_output.accesskit_update.expect("accesskit is on");
    let count = ScratchBody::stress("test").snarl.nodes().count();
    let expected = format!("Load {count} nodes");
    assert!(
        update.nodes.iter().any(|(_, node)| node.label() == Some(&expected)),
        "no button reads {expected}"
    );
}

/// Draws the canvas with the modifier write that makes a plain click select.
///
/// On a frame that releases a click with no modifier held, this writes shift and command before the canvas draws and
/// restores the modifiers after it.
fn draw_with_plain_click(ui: &mut Ui, canvas: &mut ScratchCanvas) {
    let restore = ui.ctx().input_mut(|input| {
        let is_plain_click =
            input.pointer.primary_released() && input.pointer.could_any_button_be_click() && input.modifiers.is_none();
        let held = input.modifiers;
        if is_plain_click {
            input.modifiers.shift = true;
            input.modifiers.command = true;
            input.modifiers.mac_cmd = cfg!(target_os = "macos");
        }
        is_plain_click.then_some(held)
    });
    scratch_canvas_ui(ui, canvas);
    if let Some(held) = restore {
        ui.ctx().input_mut(|input| input.modifiers = held);
    }
}

#[test]
fn a_modifier_write_makes_a_plain_click_select() {
    let mut harness = CanvasHarness::new();
    harness.draw = draw_with_plain_click;

    harness.click(harness.node_grip("n5"));
    assert_eq!(harness.selected(), ["n5"]);

    harness.click(harness.node_grip("n2"));
    assert_eq!(
        harness.selected(),
        ["n2"],
        "a second plain click replaces the selection"
    );

    harness.shift_click(harness.node_grip("n5"));
    assert_eq!(
        harness.selected(),
        ["n2", "n5"],
        "a real shift-click adds to the selection"
    );
}

#[test]
fn a_background_click_deselects_only_with_the_modifier_write() {
    let mut harness = CanvasHarness::new();
    harness.shift_click(harness.node_grip("n5"));

    harness.click(harness.background());
    assert_eq!(
        harness.selected(),
        ["n5"],
        "a plain background click leaves the selection"
    );

    harness.draw = draw_with_plain_click;
    harness.click(harness.background());
    assert!(harness.selected().is_empty());
}

#[test]
fn a_modifier_write_leaves_a_node_drag_as_a_move() {
    let mut harness = CanvasHarness::new();
    harness.draw = draw_with_plain_click;
    harness.click(harness.node_grip("n2"));

    let grip = harness.node_grip("n5");
    harness.drag(grip, grip + vec2(30.0, 30.0), 6);

    assert_eq!(harness.logged("Move"), ["Move n5"]);
    assert_eq!(harness.selected(), ["n2"]);
}

#[test]
fn a_modifier_write_leaves_a_wire_drag_connecting() {
    let mut harness = CanvasHarness::new();
    harness.draw = draw_with_plain_click;
    let (source, dest) = (harness.pin("n5", true, 0), harness.pin("n7", false, 0));

    harness.drag(source, dest, 6);

    assert_eq!(harness.logged("Connect"), ["Connect n5.0 to n7.0"]);
}

/// Double-clicks the background with `gap_frames` frames after each release, and returns whether the view moved.
fn double_click_moves_the_view(gap_frames: usize) -> bool {
    let mut harness = CanvasHarness::new();
    harness.draw = draw_with_plain_click;
    harness.canvas.framing = Some("n7".to_owned());
    harness.settle();
    let framed = harness.canvas.body().transform;
    let background = harness.background();

    harness.frame(vec![Event::PointerMoved(background)]);
    for _ in 0..2 {
        harness.frame(vec![harness.button_event(background, true)]);
        harness.frame(vec![harness.button_event(background, false)]);
        for _ in 0..gap_frames {
            harness.frame(Vec::new());
        }
    }
    harness.settle();

    harness.canvas.body().transform != framed
}

#[test]
fn a_modifier_write_leaves_a_background_double_click_centring() {
    assert!(double_click_moves_the_view(2));
}

#[test]
fn a_modifier_write_loses_a_double_click_whose_second_press_follows_at_once() {
    assert!(!double_click_moves_the_view(0));
}

#[test]
fn snarl_connect_takes_a_second_wire_and_the_connect_command_replaces_it() {
    let mut harness = CanvasHarness::new();
    let body = harness.canvas.body_mut();
    let dest = InPinId {
        node: body.node_ids["n5"],
        input: 0,
    };
    for source in ["n1", "n2"] {
        let out_pin = OutPinId {
            node: body.node_ids[source],
            output: 0,
        };
        body.snarl.connect(out_pin, dest);
    }
    assert_eq!(body.snarl.in_pin(dest).remotes.len(), 2, "one input holds two wires");
    body.snarl.drop_inputs(dest);

    for source in ["n1", "n2"] {
        body.apply(ScratchCommand::Connect(WireRef {
            source: source.to_owned(),
            output: 0,
            dest: "n5".to_owned(),
            input: 0,
        }));
    }

    let held = OutPinId {
        node: body.node_ids["n2"],
        output: 0,
    };
    assert_eq!(body.snarl.in_pin(dest).remotes, [held]);
    let document: Vec<&str> = body
        .wires
        .iter()
        .filter(|wire| wire.dest == "n5")
        .map(|wire| wire.source.as_str())
        .collect();
    assert_eq!(document, ["n2"]);
}

#[test]
#[should_panic(expected = "no entry found for key")]
fn removing_the_node_of_an_output_wire_in_flight_panics() {
    let mut harness = CanvasHarness::new();
    harness.start_wire(harness.pin("n5", true, 0));

    harness
        .canvas
        .body_mut()
        .apply(ScratchCommand::RemoveNode("n5".to_owned()));
    harness.frame(Vec::new());
}

#[test]
#[should_panic(expected = "no entry found for key")]
fn removing_the_node_of_an_input_wire_in_flight_panics() {
    let mut harness = CanvasHarness::new();
    harness.start_wire(harness.pin("n5", false, 0));

    harness
        .canvas
        .body_mut()
        .apply(ScratchCommand::RemoveNode("n5".to_owned()));
    harness.frame(Vec::new());
}

#[test]
#[should_panic(expected = "no entry found for key")]
fn removing_the_far_node_of_a_command_dragged_wire_panics() {
    let mut harness = CanvasHarness::new();
    let pin = harness.pin("n4", false, 0);
    harness.modifiers = Modifiers::COMMAND | Modifiers::SHIFT;
    harness.start_wire(pin);

    harness
        .canvas
        .body_mut()
        .apply(ScratchCommand::RemoveNode("n3".to_owned()));
    harness.frame(Vec::new());
}

#[test]
fn removing_another_node_during_a_wire_drag_keeps_the_wire() {
    let mut harness = CanvasHarness::new();
    let dest = harness.pin("n7", false, 0);
    harness.start_wire(harness.pin("n5", true, 0));

    harness
        .canvas
        .body_mut()
        .apply(ScratchCommand::RemoveNode("n2".to_owned()));
    harness.frame(vec![Event::PointerMoved(dest)]);
    harness.frame(vec![harness.button_event(dest, false)]);
    harness.settle();

    assert_eq!(harness.logged("Connect"), ["Connect n5.0 to n7.0"]);
}

#[test]
fn a_new_canvas_id_drops_the_wire_in_flight() {
    let mut harness = CanvasHarness::new();
    let dest = harness.pin("n7", false, 0);
    harness.start_wire(harness.pin("n5", true, 0));

    let body = harness.canvas.body_mut();
    body.canvas_id = body.canvas_id.with("rebuilt");
    body.apply(ScratchCommand::RemoveNode("n5".to_owned()));
    harness.frame(vec![Event::PointerMoved(dest)]);
    harness.frame(vec![harness.button_event(dest, false)]);
    harness.settle();

    assert!(harness.logged("Connect").is_empty());
}

/// Draws the canvas, then runs the command in `pending` once the primary button is up.
fn draw_with_held_removal(ui: &mut Ui, canvas: &mut ScratchCanvas, pending: &mut Option<ScratchCommand>) {
    scratch_canvas_ui(ui, canvas);
    if !ui.ctx().input(|input| input.pointer.primary_down())
        && let Some(command) = pending.take()
    {
        canvas.run_command(command);
    }
}

#[test]
fn an_edit_held_until_the_button_is_up_and_run_after_show_survives_a_wire() {
    let mut harness = CanvasHarness::new();
    harness.start_wire(harness.pin("n5", true, 0));
    let release = harness.button_event(harness.background(), false);
    let mut pending = Some(ScratchCommand::RemoveNode("n5".to_owned()));

    let _down = harness.frame_output(Vec::new(), |ui, canvas| {
        draw_with_held_removal(ui, canvas, &mut pending);
    });
    assert!(pending.is_some(), "the edit waits while the button is down");

    let _released = harness.frame_output(vec![release], |ui, canvas| {
        draw_with_held_removal(ui, canvas, &mut pending);
    });
    assert!(pending.is_none());
    harness.settle();

    assert!(!harness.canvas.body().nodes.contains_key("n5"));
}

#[test]
#[should_panic(expected = "no entry found for key")]
fn an_edit_before_show_on_the_release_frame_panics() {
    let mut harness = CanvasHarness::new();
    harness.start_wire(harness.pin("n5", true, 0));
    let release = harness.button_event(harness.background(), false);

    let _output = harness.frame_output(vec![release], |ui, canvas| {
        if !ui.ctx().input(|input| input.pointer.primary_down()) {
            canvas.run_command(ScratchCommand::RemoveNode("n5".to_owned()));
        }
        scratch_canvas_ui(ui, canvas);
    });
}

#[test]
fn removing_the_node_of_a_wire_held_by_its_dropped_menu_survives() {
    let mut harness = CanvasHarness::new();
    let background = harness.background();
    harness.drag(harness.pin("n5", true, 0), background, 6);
    assert!(Popup::is_any_open(&harness.context), "the dropped wire menu is open");

    harness
        .canvas
        .body_mut()
        .apply(ScratchCommand::RemoveNode("n5".to_owned()));
    harness.settle();

    assert!(!harness.canvas.body().nodes.contains_key("n5"));
}
