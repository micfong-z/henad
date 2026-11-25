//! Tests of functions and expressions in the graph editor over a headless egui context: a call opens its function
//! under a breadcrumb on a canvas of its own, a selection becomes a function and a call ungroups, a port and a node
//! that names it are added together, a function is imported from a picked file, the region rules hold in the menus
//! and on a drop, and the expression editor renames an input.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use henad_graph::GraphFile;
use henad_graph::edit::{EditCommand, GraphBuilder, PortSide, Scope};
use henad_graph::node::{OP_FUNCTION_CALL, OP_FUNCTION_OUTPUT, OP_REGION_REPEAT};
use henad_graph::types::ValueType;
use henad_graph::value::{ConfigValue, InputValue, PortRef};

use crate::tests::support::{CanvasHarness, link, n};
use crate::ui::editor::graph::canvas::{can_add, can_join};
use crate::ui::editor::graph::{GraphEdit, GraphRequest};

fn clamp_speed() -> Scope {
    Scope::Function("clamp_speed".to_owned())
}

/// Returns the canonical text's errors of the document under edit.
fn errors(harness: &CanvasHarness) -> Vec<String> {
    let file = GraphFile::parse(&harness.graph.text()).expect("the text reads");
    file.check().errors().map(|error| error.message.clone()).collect()
}

/// Returns the accessible role of each widget whose label or value is `label`.
fn roles_of(harness: &mut CanvasHarness, label: &str) -> Vec<Role> {
    harness
        .accessible_nodes()
        .into_iter()
        .filter(|node| node.label == label || node.value == label)
        .map(|node| node.role)
        .collect()
}

#[test]
fn a_call_opens_its_function_under_a_breadcrumb_on_a_canvas_of_its_own() {
    let mut harness = CanvasHarness::new("boids.toml");
    assert_eq!(harness.scope(), Scope::pass("flock"));
    let in_pass = roles_of(&mut harness, "flock");
    let pass_canvas = harness.graph.canvas(&Scope::pass("flock")).expect("a canvas").canvas_id;
    harness.run(GraphRequest::OpenCall(n(49)));
    assert_eq!(harness.scope(), clamp_speed());
    assert_eq!(harness.graph.trail(), [Scope::pass("flock")]);
    let canvas = harness.graph.canvas(&clamp_speed()).expect("the function's canvas");
    assert_ne!(canvas.canvas_id, pass_canvas, "each body keeps its own egui state");
    assert_eq!(canvas.nodes.len(), 17, "every node of the function's body is drawn");
    let sqrt = canvas.snarl.get_node(canvas.nodes[&n(105)]).expect("the square root");
    assert_eq!(
        sqrt.outputs[0].value_type,
        Some(ValueType::F32),
        "the function's report types its pins"
    );
    // In the pass, its name shows in the toolbar and in the Name field of the open pass. In the function, the only
    // label that reads it is the breadcrumb's link.
    assert!(in_pass.contains(&Role::TextInput));
    let in_function = roles_of(&mut harness, "flock");
    assert!(
        in_function.contains(&Role::Label) && !in_function.contains(&Role::TextInput),
        "the breadcrumb names the pass: {in_function:?}"
    );
    harness.run(GraphRequest::OpenParent(0));
    assert_eq!(harness.scope(), Scope::pass("flock"));
    assert!(harness.graph.trail().is_empty());
}

#[test]
fn a_problem_in_a_function_shows_on_its_node_and_counts_for_the_function() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::Open(clamp_speed()));
    assert!(
        harness.graph.trail().is_empty(),
        "the panel opens a function with no breadcrumb"
    );
    assert_eq!(harness.graph.issue_counts(&clamp_speed()), (0, 0));
    harness.run(GraphRequest::Edit(GraphEdit::one(EditCommand::Disconnect {
        scope: clamp_speed(),
        node: n(105),
        port: "a".to_owned(),
    })));
    assert!(harness.graph.issue_counts(&clamp_speed()).0 > 0);
    let canvas = harness.graph.canvas(&clamp_speed()).expect("a canvas");
    let sqrt = canvas.snarl.get_node(canvas.nodes[&n(105)]).expect("the square root");
    assert!(!sqrt.issues.errors.is_empty(), "the error sits on the node");
    let diagnostic = harness
        .graph
        .diagnostics()
        .find(|diagnostic| diagnostic.node.as_deref() == Some("n105"))
        .expect("a diagnostic at the node")
        .clone();
    assert_eq!(harness.graph.diagnostic_scope(&diagnostic), Some(clamp_speed()));
}

#[test]
fn a_selection_becomes_a_function_and_its_call_ungroups() {
    let mut harness = CanvasHarness::new("sir.toml");
    let before = harness.graph.document().passes[1].body.nodes.len();
    harness.run(GraphRequest::MakeFunction([12, 13, 15, 16, 17].map(n).to_vec()));
    assert_eq!(
        harness.graph.status.as_ref().map(|status| status.message.as_str()),
        Some("Made function 'function'. Rename it in the Functions panel.")
    );
    assert!(errors(&harness).is_empty(), "{:?}", errors(&harness));
    let call = harness
        .graph
        .open_pass()
        .and_then(|pass| pass.body.nodes.iter().find(|node| node.op == OP_FUNCTION_CALL))
        .map(|node| node.id)
        .expect("a call in the pass");
    assert_eq!(harness.graph.document().passes[1].body.nodes.len(), before - 4);
    harness.run(GraphRequest::Ungroup(call));
    assert!(!harness.holds(call));
    assert_eq!(harness.graph.document().passes[1].body.nodes.len(), before);
    assert!(errors(&harness).is_empty(), "{:?}", errors(&harness));
    harness.run(GraphRequest::Undo);
    assert!(harness.holds(call), "an ungroup is one undo step");
}

#[test]
fn an_added_output_comes_with_its_node_and_names_the_links_to_each_call() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::AddFunctionPort {
        function: "clamp_speed".to_owned(),
        side: PortSide::Output,
    });
    let function = &harness.graph.document().functions[0];
    assert_eq!(function.outputs.len(), 2);
    assert!(function.body.nodes.iter().any(|node| {
        node.op == OP_FUNCTION_OUTPUT && node.config.get("port").and_then(ConfigValue::as_str) == Some("output")
    }));
    let readers: Vec<InputValue> = harness
        .graph
        .open_pass()
        .expect("flock")
        .body
        .nodes
        .iter()
        .flat_map(|node| node.inputs.values().cloned())
        .filter(|value| matches!(value, InputValue::Link(PortRef { node, .. }) if *node == n(49)))
        .collect();
    assert!(!readers.is_empty() && readers.iter().all(|value| *value == link(n(49), "out")));
}

#[test]
fn one_undo_takes_back_a_port_added_in_the_open_function() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::Open(clamp_speed()));
    let before = harness.graph.text();
    let depth = harness.graph.history().undo_depth();
    for side in [PortSide::Input, PortSide::Output] {
        harness.run(GraphRequest::AddFunctionPort {
            function: "clamp_speed".to_owned(),
            side,
        });
    }
    assert_eq!(harness.graph.history().undo_depth(), depth + 2, "each port is one edit");
    let function = &harness.graph.document().functions[0];
    let editor = harness.graph.editor_region();
    assert!(
        function
            .body
            .nodes
            .iter()
            .all(|node| editor.node_layout(node.id).is_some()),
        "the port's node is placed when it is made"
    );
    harness.run(GraphRequest::Undo);
    harness.run(GraphRequest::Undo);
    assert_eq!(harness.graph.text(), before, "two undos take both ports back");
    assert!(harness.graph.history().can_redo());
}

#[test]
fn one_undo_takes_back_an_import_with_no_placement() {
    let mut harness = CanvasHarness::new("boids.toml");
    // The ants starter's [editor] region holds frames and no node placement.
    let ants = henad_graph::starters::starter("ants.toml").expect("ants").text;
    harness.graph.set_import_source("ants.toml".to_owned(), ants.to_owned());
    let before = harness.graph.text();
    let depth = harness.graph.history().undo_depth();
    harness.run(GraphRequest::Import {
        function: "decay".to_owned(),
        id: None,
    });
    harness.run(GraphRequest::Open(Scope::Function("decay".to_owned())));
    assert_eq!(
        harness.graph.history().undo_depth(),
        depth + 1,
        "opening the function moves nothing"
    );
    harness.run(GraphRequest::Undo);
    assert_eq!(harness.graph.text(), before, "one undo takes the import back");
}

#[test]
fn a_function_imports_from_a_picked_file_and_records_its_origin() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness
        .graph
        .set_import_source("notes.toml".to_owned(), "no graph".to_owned());
    assert!(harness.graph.status.as_ref().is_some_and(|status| status.warning));
    assert!(harness.graph.import_source().is_none());
    let ants = henad_graph::starters::starter("ants.toml").expect("ants").text;
    harness.graph.set_import_source("ants.toml".to_owned(), ants.to_owned());
    let source = harness.graph.import_source().expect("the picked file");
    assert!(source.functions.contains(&"decay".to_owned()), "{:?}", source.functions);
    harness.run(GraphRequest::Import {
        function: "decay".to_owned(),
        id: Some("fade".to_owned()),
    });
    assert_eq!(
        harness.graph.status.as_ref().map(|status| status.message.as_str()),
        Some("Imported function 'fade' from ants.toml.")
    );
    assert!(
        harness
            .graph
            .document()
            .functions
            .iter()
            .any(|function| function.id == "fade")
    );
    let origin = harness
        .graph
        .editor_region()
        .function_origin("fade")
        .expect("an origin");
    assert_eq!(origin.name, "ants.toml");
    harness.run(GraphRequest::Open(Scope::Function("fade".to_owned())));
    assert_eq!(harness.scope(), Scope::Function("fade".to_owned()));
    harness.run(GraphRequest::CloseImport);
    assert!(harness.graph.import_source().is_none());
}

#[test]
fn a_function_scope_wraps_nodes_in_a_repeat() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::Open(clamp_speed()));
    harness.run(GraphRequest::WrapInRepeat(vec![n(104), n(105)]));
    let body = &harness.graph.document().functions[0].body;
    let repeat = body
        .node(n(105))
        .and_then(|node| node.region)
        .expect("the node is wrapped");
    assert_eq!(body.node(repeat).map(|node| node.op.as_str()), Some(OP_REGION_REPEAT));
    assert!(errors(&harness).is_empty(), "{:?}", errors(&harness));
}

/// A grid whose Repeat `n1` holds the Repeat `n2`, which holds the Repeat `n3`, beside a Repeat `n4`, two neighborhood
/// folds `n5` and `n7`, and a node `n6` outside every region.
const NESTED: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_nested"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 8
height = 8

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "region.repeat"

[[pass.node]]
id = "n2"
op = "region.repeat"
region = "n1"

[[pass.node]]
id = "n3"
op = "region.repeat"
region = "n2"

[[pass.node]]
id = "n4"
op = "region.repeat"

[[pass.node]]
id = "n5"
op = "grid.fold"

[[pass.node]]
id = "n6"
op = "math.add"

[[pass.node]]
id = "n7"
op = "grid.fold"
"#;

#[test]
fn regions_join_as_library_one_nests_them_three_deep() {
    let file = GraphFile::parse(NESTED).expect("the text reads");
    let body = &file.document().passes[0].body;
    assert!(can_join(body, n(6), n(3)), "a plain node joins any region");
    assert!(can_join(body, n(4), n(2)), "a Repeat nests three deep");
    assert!(!can_join(body, n(4), n(3)), "a Repeat does not nest four deep");
    assert!(can_join(body, n(5), n(1)), "a Repeat holds a neighborhood fold");
    assert!(can_join(body, n(4), n(5)), "a neighborhood fold holds a Repeat");
    assert!(
        !can_join(body, n(7), n(5)),
        "a neighborhood fold holds no neighborhood fold"
    );
    assert!(!can_join(body, n(1), n(4)), "a region of depth 3 joins nothing");
    assert!(can_add(body, OP_REGION_REPEAT, None), "a Repeat sits at the top");
    assert!(can_add(body, "math.add", Some(n(3))), "a plain node goes in any region");
    assert!(
        can_add(body, OP_REGION_REPEAT, Some(n(2))),
        "a new Repeat nests three deep"
    );
    assert!(
        !can_add(body, OP_REGION_REPEAT, Some(n(3))),
        "a new Repeat does not nest four deep"
    );
    assert!(
        !can_add(body, "grid.fold", Some(n(5))),
        "a neighborhood fold takes no new neighborhood fold"
    );

    let boids = henad_graph::starters::starter("boids.toml").expect("boids").text;
    let mut builder = GraphBuilder::from_file(&GraphFile::parse(boids).expect("reads"));
    let repeat = builder
        .add_node(&Scope::pass("flock"), OP_REGION_REPEAT, &[], &[])
        .expect("a Repeat");
    let body = &builder.document().passes[1].body;
    assert!(!can_join(body, n(25), repeat), "a radius fold sits in no region");
    assert!(!can_join(body, repeat, n(25)), "a radius fold holds no region");
    assert!(can_join(body, n(37), n(25)), "a plain node joins a radius fold");
    assert!(
        !can_add(body, OP_REGION_REPEAT, Some(n(25))),
        "a radius fold takes no new Repeat"
    );
}

#[test]
fn the_node_menu_offers_no_region_a_radius_fold_cannot_join() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::AddNode {
        op: OP_REGION_REPEAT.to_owned(),
        pos: [0.0, 600.0],
        region: None,
        wire: None,
    });
    harness.settle_framing();
    harness.secondary_click(harness.node_grip(n(25)));
    let menu = super::graph_canvas::texts(&harness.frame_output(Vec::new()).shapes);
    assert!(
        menu.iter().any(|text| text == "Delete"),
        "the node menu is open: {menu:?}"
    );
    assert!(
        !menu.iter().any(|text| text == "Move into region"),
        "a radius fold joins no region: {menu:?}"
    );
}

#[test]
fn the_expression_editor_renames_an_input_and_moves_its_wire() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::Frame {
        scope: Scope::pass("flock"),
        node: n(35),
    });
    harness.settle_framing();
    let field = harness.accessible_with_role("count_f", Some(Role::TextInput));
    harness.click(field.center());
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![egui::Event::Text("count_float".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
    let node = harness
        .graph
        .open_pass()
        .and_then(|pass| pass.body.node(n(35)))
        .expect("the expression")
        .clone();
    let inputs = node.config.get("inputs").expect("inputs");
    assert!(format!("{inputs:?}").contains("count_float"), "{inputs:?}");
    assert!(
        matches!(node.inputs.get("count_float"), Some(InputValue::Link(source)) if source.node == n(37)),
        "the wire moves to the renamed input: {:?}",
        node.inputs
    );
    assert!(!node.inputs.contains_key("count_f"));
}

#[test]
fn the_expression_editor_refuses_the_name_of_another_input() {
    let mut harness = CanvasHarness::new("boids.toml");
    harness.run(GraphRequest::Frame {
        scope: Scope::pass("flock"),
        node: n(35),
    });
    harness.settle_framing();
    let before = harness.graph.text();
    let field = harness.accessible_with_role("count_f", Some(Role::TextInput));
    harness.click(field.center());
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![egui::Event::Text("count".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
    assert_eq!(harness.graph.text(), before, "the rename is refused");
    assert_eq!(
        harness.graph.status.as_ref().map(|status| status.message.as_str()),
        Some("Node n35 already has the input 'count'.")
    );
}
