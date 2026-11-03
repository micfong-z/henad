//! Tests of regions on the graph canvas over a headless egui context: a Repeat draws as two halves inside a frame,
//! frames nest, a selection wraps in a Repeat, a node joins a region when dropped in its frame and leaves through the
//! node menu, and the Repeat controls edit the accumulators and the bound.

use std::collections::HashSet;

use egui::epaint::ClippedShape;
use egui::{Event, Key, Modifiers, Rect, Shape, vec2};
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::node::{Accumulator, Combine, OP_REGION_PUT, OP_REGION_REPEAT};
use henad_graph::types::ValueType;
use henad_graph::value::{ConfigValue, InputValue, Literal};

use super::graph_canvas::{Harness, n};
use crate::ui::editor::graph::{GraphEdit, GraphRequest};
use crate::ui::mcs;

/// Nodes of the SIR starter's step pass that a test wraps: the infection chance, the draw and the decisions.
const WRAPPED: [u32; 7] = [12, 13, 14, 15, 16, 17, 18];

fn step() -> Scope {
    Scope::pass("step")
}

/// Returns the harness of the SIR starter with [`WRAPPED`] wrapped in a Repeat, and the Repeat's id.
fn wrapped() -> (Harness, NodeId) {
    let mut harness = Harness::new("sir.toml");
    harness.run(GraphRequest::WrapInRepeat(WRAPPED.map(n).to_vec()));
    let repeat = repeat_of(&harness, n(14));
    (harness, repeat)
}

/// Returns the region that holds node `id` in the open pass.
fn repeat_of(harness: &Harness, id: NodeId) -> NodeId {
    harness
        .graph
        .open_pass()
        .and_then(|pass| pass.body.node(id))
        .and_then(|node| node.region)
        .expect("the node sits in a region")
}

/// Returns the region of node `id` in the open pass, `None` outside every region.
fn region(harness: &Harness, id: NodeId) -> Option<NodeId> {
    harness.graph.open_pass()?.body.node(id)?.region
}

/// Returns the canvas rectangle of the frame of `region`.
fn frame(harness: &Harness, region: NodeId) -> Rect {
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    canvas
        .region_frame_rects(&HashSet::new())
        .into_iter()
        .find(|frame| frame.region == region)
        .expect("the region has a frame")
        .rect
}

/// Returns the errors of the document.
fn errors(harness: &Harness) -> Vec<String> {
    harness
        .graph
        .diagnostics()
        .filter(|diagnostic| diagnostic.severity == henad_core::diagnostic::DiagnosticSeverity::Error)
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

/// Returns whether `shapes` hold a rectangle stroked in `color`.
fn holds_stroke(shapes: &[ClippedShape], color: egui::Color32) -> bool {
    fn holds(shape: &Shape, color: egui::Color32) -> bool {
        match shape {
            Shape::Rect(rect) => rect.stroke.color == color && rect.stroke.width > 1.0,
            Shape::Vec(shapes) => shapes.iter().any(|shape| holds(shape, color)),
            _ => false,
        }
    }
    shapes.iter().any(|clipped| holds(&clipped.shape, color))
}

#[test]
fn a_repeat_draws_as_two_halves_inside_its_frame() {
    let (harness, repeat) = wrapped();
    assert!(errors(&harness).is_empty(), "{:#?}", errors(&harness));
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let body = &harness.graph.open_pass().expect("a pass").body;
    assert_eq!(
        canvas.snarl.nodes().count(),
        body.nodes.len() + 1,
        "the Repeat takes two canvas nodes"
    );
    let links = body
        .nodes
        .iter()
        .flat_map(|node| node.inputs.values())
        .filter(|value| matches!(value, InputValue::Link(_)))
        .count();
    assert_eq!(
        canvas.snarl.wires().count(),
        links,
        "every wire shows, the Repeat's included"
    );

    let input_half = canvas.current_rect(repeat).expect("the input half is drawn");
    let output_half = canvas.output_half_rect(repeat).expect("the output half is drawn");
    let frame = frame(&harness, repeat);
    assert!(frame.contains_rect(input_half) && frame.contains_rect(output_half));
    for member in body.region_contents(repeat) {
        let rect = canvas.current_rect(member).expect("a member is drawn");
        assert!(frame.contains_rect(rect), "{member} sits in the frame");
        assert!(
            input_half.max.x < rect.min.x && rect.max.x < output_half.min.x,
            "{member} sits between the halves: {input_half:?} {rect:?} {output_half:?}"
        );
    }
    let input_view = canvas.snarl.get_node(canvas.nodes[&repeat]).expect("a view");
    let output_view = canvas.snarl.get_node(canvas.output_halves[&repeat]).expect("a view");
    assert!(
        input_view.outputs.iter().all(|output| output.inner),
        "the input half shows inner outputs"
    );
    assert!(output_view.inputs.is_empty());
    assert_eq!(
        output_view
            .outputs
            .iter()
            .map(|output| output.port.as_str())
            .collect::<Vec<_>>(),
        ["out", "out_2"],
        "the output half shows the accumulators"
    );
}

#[test]
fn the_repeat_button_wraps_the_selected_nodes() {
    let mut harness = Harness::new("sir.toml");
    let grip = harness.node_grip(n(14));
    harness.click(grip);
    harness.modifiers = Modifiers::SHIFT;
    let grip = harness.node_grip(n(15));
    harness.click(grip);
    harness.modifiers = Modifiers::NONE;
    assert_eq!(harness.selected(), [n(14), n(15)]);
    let before = harness.graph.text();

    let button = harness.accessible("Repeat");
    harness.click(button.center());

    let repeat = repeat_of(&harness, n(14));
    assert_eq!(region(&harness, n(15)), Some(repeat));
    let body = &harness.graph.open_pass().expect("a pass").body;
    assert_eq!(body.node(repeat).map(|node| node.op.as_str()), Some(OP_REGION_REPEAT));
    assert!(errors(&harness).is_empty(), "{:#?}", errors(&harness));
    harness.run(GraphRequest::Undo);
    assert_eq!(
        harness.graph.text(),
        before,
        "one undo unwraps the nodes and takes the halves away"
    );
}

#[test]
fn a_nested_frame_paints_inside_the_frame_around_it() {
    let (mut harness, inner) = wrapped();
    harness.run(GraphRequest::WrapInRepeat(vec![inner]));
    let outer = repeat_of(&harness, inner);
    assert_eq!(region(&harness, n(14)), Some(inner), "the members stay in their region");
    let (inner_frame, outer_frame) = (frame(&harness, inner), frame(&harness, outer));
    assert!(
        outer_frame.contains_rect(inner_frame) && outer_frame != inner_frame,
        "{outer_frame:?} holds {inner_frame:?}"
    );
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let depths: Vec<usize> = canvas
        .frames
        .iter()
        .filter(|frame| frame.region.is_some())
        .map(|frame| frame.depth)
        .collect();
    assert_eq!(depths, [1, 2], "the outermost frame paints first");
    assert!(errors(&harness).is_empty(), "{:#?}", errors(&harness));
}

#[test]
fn a_node_dropped_in_a_frame_joins_the_region_and_the_menu_takes_it_out() {
    let (mut harness, repeat) = wrapped();
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::SetRegion {
        scope: step(),
        node: n(16),
        region: None,
    })));
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::Move {
        node: n(16),
        pos: [-600.0, -600.0],
    })));
    harness.run(GraphRequest::FrameAll);
    harness.settle_framing();
    assert_eq!(region(&harness, n(16)), None);

    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let node = canvas.current_rect(n(16)).expect("drawn");
    let target = frame(&harness, repeat);
    let start = harness.node_grip(n(16));
    let end = start + (harness.screen(target.center()) - harness.screen(node.center()));
    harness.frame(vec![Event::PointerMoved(start)]);
    harness.frame(vec![harness.button(start, true)]);
    let mut highlighted = false;
    for step in 1..=6 {
        let reached = start + (end - start) * (step as f32 / 6.0);
        let output = harness.frame_output(vec![Event::PointerMoved(reached)]);
        highlighted |= holds_stroke(&output.shapes, mcs::BLUE_500);
    }
    assert!(highlighted, "the target frame takes a blue stroke during the drag");
    harness.frame(vec![harness.button(end, false)]);
    harness.settle();
    assert_eq!(
        region(&harness, n(16)),
        Some(repeat),
        "the dropped node joins the region"
    );

    let grip = harness.node_grip(n(16));
    harness.secondary_click(grip);
    let button = harness.accessible("Move out of region");
    harness.click(button.center());
    assert_eq!(region(&harness, n(16)), None, "the node menu takes it out");
    harness.settle();
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let node = canvas.current_rect(n(16)).expect("drawn");
    assert!(
        !node.intersects(frame(&harness, repeat)),
        "the node leaves the frame: {node:?} against {:?}",
        frame(&harness, repeat)
    );
}

#[test]
fn a_member_moved_out_of_its_region_is_drawn_outside_the_frame() {
    let (mut harness, repeat) = wrapped();
    let before = frame(&harness, repeat);
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    assert!(before.contains_rect(canvas.current_rect(n(14)).expect("drawn")));
    let grip = harness.node_grip(n(14));
    harness.secondary_click(grip);
    let button = harness.accessible("Move out of region");
    harness.click(button.center());
    harness.settle();
    assert_eq!(region(&harness, n(14)), None, "the node menu takes it out");
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let node = canvas.current_rect(n(14)).expect("drawn");
    let after = frame(&harness, repeat);
    assert!(!node.intersects(after), "{node:?} against {after:?}");
    assert!(
        node.min.x > after.max.x,
        "the node sits right of the frame: {node:?} {after:?} {before:?}"
    );
    harness.run(GraphRequest::Undo);
    assert_eq!(
        region(&harness, n(14)),
        Some(repeat),
        "one undo restores the region and the place"
    );
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    assert!(frame(&harness, repeat).contains_rect(canvas.current_rect(n(14)).expect("drawn")));
}

#[test]
fn a_member_dragged_inside_its_frame_stays_in_its_region() {
    let (mut harness, repeat) = wrapped();
    let start = harness.node_grip(n(14));
    harness.drag(start, start + vec2(30.0, 20.0));
    assert_eq!(region(&harness, n(14)), Some(repeat));
    assert!(harness.graph.history().can_undo(), "the drag is a move");
}

#[test]
fn dragging_the_output_half_records_its_place() {
    let (mut harness, repeat) = wrapped();
    let before = harness.graph.text();
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let rect = canvas.output_half_rect(repeat).expect("drawn");
    let placed = harness
        .graph
        .builder()
        .editor()
        .node_layout(repeat)
        .and_then(|layout| layout.output_pos)
        .expect("the wrap places the output half");
    let start = harness.screen(rect.center_top() + vec2(0.0, 4.0));
    harness.drag(start, start + vec2(80.0, 40.0));
    let moved = harness
        .graph
        .builder()
        .editor()
        .node_layout(repeat)
        .and_then(|layout| layout.output_pos)
        .expect("placed");
    assert!(
        moved[0] > placed[0] + 10.0 && moved[1] > placed[1] + 5.0,
        "{placed:?} to {moved:?}"
    );
    harness.run(GraphRequest::Undo);
    assert_eq!(harness.graph.text(), before, "one undo step");
}

#[test]
fn a_constant_count_sets_the_bound() {
    let (mut harness, repeat) = wrapped();
    let max = |harness: &Harness| {
        harness
            .graph
            .open_pass()
            .and_then(|pass| pass.body.node(repeat))
            .and_then(|node| node.config.get("max"))
            .and_then(ConfigValue::as_integer)
    };
    for (count, bound) in [(7, 7), (0, 1), (100_000, 65_536)] {
        harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::SetLiteral {
            scope: step(),
            node: repeat,
            port: "count".to_owned(),
            literal: Literal::U32(count),
        })));
        assert_eq!(max(&harness), Some(bound), "count {count}");
    }
}

#[test]
fn a_bound_out_of_range_is_drawn_and_left_for_the_checker() {
    let (mut harness, repeat) = wrapped();
    harness.run(GraphRequest::Frame {
        scope: step(),
        node: repeat,
    });
    harness.settle_framing();
    let max = |harness: &Harness| {
        harness
            .graph
            .open_pass()
            .and_then(|pass| pass.body.node(repeat))
            .and_then(|node| node.config.get("max"))
            .and_then(ConfigValue::as_integer)
    };
    for bound in [100_000, 0] {
        let depth = harness.graph.history().undo_depth();
        harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::SetConfig {
            scope: step(),
            node: repeat,
            key: "max".to_owned(),
            value: Some(ConfigValue::Integer(bound)),
        })));
        harness.settle();
        assert_eq!(max(&harness), Some(bound), "drawing max {bound} edits nothing");
        assert_eq!(harness.graph.history().undo_depth(), depth + 1, "one edit");
        assert!(!errors(&harness).is_empty(), "the checker reports max {bound}");
    }
}

#[test]
fn the_repeat_controls_add_rename_and_remove_an_accumulator() {
    let (mut harness, repeat) = wrapped();
    harness.run(GraphRequest::Frame {
        scope: step(),
        node: repeat,
    });
    harness.settle_framing();
    let accumulators = |harness: &Harness| -> Vec<Accumulator> {
        harness
            .graph
            .open_pass()
            .and_then(|pass| pass.body.node(repeat))
            .and_then(|node| node.accumulators().ok())
            .unwrap_or_default()
    };

    let add = harness.accessible(&format!(
        "{} Add accumulator",
        crate::icons::material_design_icons::MDI_PLUS
    ));
    harness.click(add.center());
    assert_eq!(
        accumulators(&harness).last(),
        Some(&Accumulator::new("acc", ValueType::F32, Combine::Sum))
    );

    let field = harness.accessible_with_role("acc", Some(egui::accesskit::Role::TextInput));
    harness.click(field.center());
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text("speed".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    assert_eq!(
        accumulators(&harness)
            .last()
            .map(|accumulator| accumulator.name.as_str()),
        Some("speed")
    );

    let removes = harness.graph.text().matches("name = ").count();
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::RemoveAccumulator {
        scope: step(),
        node: repeat,
        name: "speed".to_owned(),
    })));
    assert_eq!(harness.graph.text().matches("name = ").count(), removes - 1);
    assert_eq!(accumulators(&harness).len(), 2);
}

#[test]
fn quick_add_in_a_frame_adds_the_node_to_the_region() {
    let (mut harness, repeat) = wrapped();
    harness.run(GraphRequest::FrameAll);
    harness.settle_framing();
    let corner = frame(&harness, repeat).min + vec2(4.0, 4.0);
    let position = harness.screen(corner);
    harness.secondary_click(position);
    harness.frame(vec![Event::Text("Accumulate".to_owned())]);
    harness.settle();
    let item = harness.accessible_with_role("Accumulate", Some(egui::accesskit::Role::Button));
    harness.click(item.center());
    let body = &harness.graph.open_pass().expect("a pass").body;
    let put = body
        .nodes
        .iter()
        .filter(|node| node.op == OP_REGION_PUT)
        .max_by_key(|node| node.id)
        .expect("a put");
    assert_eq!(put.region, Some(repeat), "{}", harness.graph.text());
}

#[test]
fn quick_add_in_a_radius_fold_offers_no_region_and_the_add_is_refused() {
    let mut harness = Harness::new("boids.toml");
    harness.run(GraphRequest::FrameAll);
    harness.settle_framing();
    let corner = frame(&harness, n(25)).min + vec2(4.0, 4.0);
    let position = harness.screen(corner);
    harness.secondary_click(position);
    harness.frame(vec![Event::Text("repeat".to_owned())]);
    harness.settle();
    let offered = super::graph_canvas::texts(&harness.frame_output(Vec::new()).shapes);
    assert!(
        offered.iter().any(|text| text == "No node matches."),
        "a radius fold holds no Repeat: {offered:?}"
    );

    let before = harness.graph.text();
    harness.run(GraphRequest::AddNode {
        op: OP_REGION_REPEAT.to_owned(),
        pos: [0.0, 600.0],
        region: Some(n(25)),
        wire: None,
    });
    assert_eq!(harness.graph.text(), before, "the add is refused");
    assert!(harness.graph.status.as_ref().is_some_and(|status| status.warning));
}
