//! Tests of the graph canvas on a headless egui context with synthetic input: the canvas follows the document, every
//! gesture reaches it as an edit command, the egui-snarl traps of record #62 stay guarded, and Frame all fits a wide
//! converted sheet.

use egui::epaint::ClippedShape;
use egui::{Event, Key, Modifiers, Pos2, Rect, Shape, vec2};
use henad_graph::GraphFile;
use henad_graph::document::{Pass, World};
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::value::{InputValue, Literal, PortRef};

use crate::tests::support::{CanvasHarness, link, n};
use crate::ui::editor::graph::{GraphEdit, GraphEditor, GraphRequest};
use crate::ui::mcs;

#[test]
fn the_canvas_shows_every_node_and_wire_of_the_open_pass() {
    let harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("vote.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
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
    let mut harness = CanvasHarness::new("sir.toml");
    let mut world = harness.graph.document().world;
    let World::Grid(grid) = &mut world else {
        panic!("the SIR starter is a grid model");
    };
    assert_ne!(grid.width, 777);
    grid.width = 777;
    let width = |harness: &CanvasHarness| match harness.graph.document().world {
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
        let mut harness = CanvasHarness::over(GraphEditor::new(&GraphFile::parse(&text).expect("the file reads")));
        let read = harness.graph.text();
        harness.settle();
        assert!(harness.graph.text() == read, "{name}: a frame changed the file");
        if let Some(parameter) = parameter {
            // The parameter's chips on the canvas read its label too, and its form's header sits in the side panel
            // at the left.
            let header = harness
                .accessible_nodes()
                .into_iter()
                .filter(|node| node.label == parameter)
                .filter_map(|node| node.bounds)
                .min_by(|a, b| a.left().total_cmp(&b.left()))
                .expect("the parameter's header");
            harness.click(Pos2::new(header.left() + 4.0, header.center().y));
            assert!(
                harness.graph.text() == read,
                "{name}: the parameter's form changed the file"
            );
        }
    }
}

#[test]
fn accessible_bounds_of_a_node_follow_the_canvas_transform() {
    let mut harness = CanvasHarness::new("sir.toml");
    harness.run(GraphRequest::Frame {
        scope: Scope::pass("step"),
        node: n(12),
    });
    harness.settle_framing();
    let nodes = harness.accessible_nodes();
    let canvas = harness.graph.canvas(&harness.scope()).expect("a canvas");
    let local = canvas.current_rect(n(12)).expect("drawn");
    let shown = canvas.transform * local;
    assert!(
        !shown.intersects(local),
        "the test needs a transform that moves the node off its own canvas rectangle: {local:?} and {shown:?}"
    );
    let bounds: Vec<Rect> = nodes
        .into_iter()
        .filter(|node| node.label == "Subtract" || node.value == "Subtract")
        .filter_map(|node| node.bounds)
        .collect();
    assert_eq!(bounds.len(), 1, "one accessible node reads Subtract: {bounds:?}");
    assert!(
        shown.expand(1.0).contains_rect(bounds[0]),
        "the Subtract label of n12 sits at {:?}, and the node is drawn at {shown:?}",
        bounds[0]
    );
}

/// Every canvas of every graph starter, drawn with the app's fonts and style, holds no node over another, so no pin
/// or port label hides under a neighbour.
#[test]
fn every_starter_opens_with_no_node_over_another() {
    for starter in henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters())
    {
        let mut harness = CanvasHarness::over(crate::tests::support::graph_editor_of(starter.file_name));
        crate::init::setup_custom_fonts(&harness.context);
        crate::init::setup_custom_styles(&harness.context);
        harness.settle();
        let document = harness.graph.document().clone();
        let scopes = document.passes.iter().map(|pass| Scope::pass(&pass.id)).chain(
            document
                .functions
                .iter()
                .map(|function| Scope::Function(function.id.clone())),
        );
        for scope in scopes {
            harness.graph.request(GraphRequest::Open(scope.clone()));
            harness.settle();
            let canvas = harness.graph.canvas(&scope).expect("a canvas");
            let rects: Vec<(String, Rect)> = canvas
                .nodes
                .keys()
                .filter_map(|id| Some((id.to_string(), canvas.drawn_rect(*id)?)))
                .chain(
                    canvas
                        .output_halves
                        .keys()
                        .filter_map(|id| Some((format!("{id} output"), canvas.drawn_output_half_rect(*id)?))),
                )
                .collect();
            assert_eq!(
                rects.len(),
                canvas.nodes.len() + canvas.output_halves.len(),
                "{} {scope:?}: every node is drawn",
                starter.file_name
            );
            for (index, (first, first_rect)) in rects.iter().enumerate() {
                for (second, second_rect) in &rects[index + 1..] {
                    assert!(
                        !first_rect.intersect(*second_rect).is_positive(),
                        "{} {scope:?}: {first} at {first_rect:?} lies over {second} at {second_rect:?}",
                        starter.file_name
                    );
                }
            }
        }
    }
}

#[test]
fn frame_all_fits_a_converted_sheet_wider_than_the_canvas() {
    // The frames of the SIR sheet converted to a node graph, 2974 by 992 canvas points, in the 547 point wide canvas of
    // a window 1280 by 800 points.
    let target = Rect::from_min_size(Pos2::ZERO, vec2(2974.0, 992.0));
    let canvas = Rect::from_min_size(Pos2::new(253.0, 120.0), vec2(547.0, 600.0));
    let transform = crate::ui::editor::graph::canvas::framing_transform(target, canvas);
    let shown = transform * target;
    assert!(canvas.contains_rect(shown), "{shown:?} fits in {canvas:?}");
}
