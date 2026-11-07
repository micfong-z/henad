//! Tests of the graph editor in the Editor tab: a starter opens as a graph, files open by their kind, every edit runs
//! through the edit commands with snapshot undo, and Apply reads the graph's canonical text.

use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_graph::GraphFile;
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::value::InputValue;

use crate::state::AppState;
use crate::tests::support::step_past;
use crate::ui::dock::Tab;
use crate::ui::editor::graph::world::param_with;
use crate::ui::editor::graph::{GraphEdit, GraphEditor, GraphRequest, GraphStatus};
use crate::ui::editor::{
    ApplyOutcome, EditorModel, apply, editor_ui, new_graph_model, new_grid_rule, receive_open, save_name,
};
use crate::ui::files::{DialogFile, OpenResult};

/// Returns node `number`'s id.
fn n(number: u32) -> NodeId {
    NodeId::new(number).expect("an id")
}

/// Returns an editor of the starter `name`.
fn editor(name: &str) -> GraphEditor {
    let text = henad_graph::starters::starter(name).expect("a starter").text;
    GraphEditor::new(&GraphFile::parse(text).expect("the starter reads"))
}

/// Runs `request` on `graph` as the canvas runs it once the button is up.
fn run(graph: &mut GraphEditor, request: GraphRequest) {
    graph.run(&egui::Context::default(), request);
}

/// Returns an app over the example models on a headless device, or `None` to skip on a machine without one.
fn app(compute: bool) -> Option<AppState> {
    AppState::headless(henad_models::example_models(), compute)
}

/// Returns an opened file named `name` holding `text`.
fn opened(name: &str, text: &str) -> OpenResult {
    OpenResult::Files(vec![DialogFile {
        name: name.to_owned(),
        bytes: text.as_bytes().to_vec(),
        path: None,
    }])
}

#[test]
fn new_graph_model_opens_a_starter_on_its_tick_pass() {
    let Some(mut app) = app(false) else {
        return;
    };
    app.editor.model = EditorModel::Rule("an earlier text".to_owned());
    new_graph_model(&mut app, "sir.toml");

    let graph = app.editor.model.graph().expect("a graph opened");
    assert!(app.editor.model.rule_text().is_none(), "the grid rule's text went");
    assert_eq!(graph.scope(), &Scope::pass("step"));
    assert_eq!(graph.document().id, "node_sir");
    assert!(!graph.report().has_errors(), "a starter checks");
    assert_eq!(app.focus_request, Some(Tab::Editor));
    assert_eq!(save_name(&app.editor), "node_sir.toml");
    let saved = GraphFile::parse(&app.editor.model.file_text()).expect("the saved text reads");
    assert_eq!(saved.to_toml(), graph.text(), "a save writes the canonical text");

    new_grid_rule(&mut app);
    assert!(app.editor.model.graph().is_none(), "a grid rule replaces the graph");
}

#[test]
fn a_file_opens_on_the_canvas_or_as_text_by_whether_it_reads_as_a_graph() {
    let Some(mut app) = app(false) else {
        return;
    };
    let text = henad_graph::starters::starter("vote.toml").expect("a starter").text;
    receive_open(&mut app, opened("vote.toml", text));
    assert!(app.editor.model.graph().is_some(), "a graph opens on the canvas");
    assert_eq!(app.editor.file_name.as_deref(), Some("vote.toml"));
    assert_eq!(app.editor.file_status.as_deref(), Some("Opened vote.toml"));

    let broken = text.replace("[graph]", "[graph]\nunknown_key = 1");
    receive_open(&mut app, opened("broken.toml", &broken));
    assert!(
        app.editor.model.graph().is_none(),
        "a graph that does not read opens as text"
    );
    assert_eq!(app.editor.model.file_text(), broken);
}

#[test]
fn apply_installs_the_graph_and_builds_it_at_its_own_size() {
    let Some(mut app) = app(true) else {
        return;
    };
    new_graph_model(&mut app, "sir.toml");
    apply(&mut app);
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Applied),
        "{:?}",
        app.editor.outcome
    );
    let installed = app.models.get("node_sir").expect("the graph was installed").clone();
    assert_eq!(
        installed.source().code().map(henad_core::provenance::ModelCode::format),
        Some(henad_graph::CODE_FORMAT)
    );
    assert_eq!(app.selected_model.as_deref(), Some("node_sir"));
    assert_eq!(app.loaded_id(), Some("node_sir"), "nothing ran, so the graph was built");
    assert_eq!(app.preview(), None, "Apply builds no preview");
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none(), "the build failed");
    assert_eq!(
        &app.loaded_values[..2],
        &[ParamValue::U32(1024), ParamValue::U32(1024)],
        "the graph runs at its own grid size"
    );
    assert!(crate::ui::editor::can_edit(&installed));
    assert_eq!(
        crate::ui::editor::editable_graph(&installed).map(|file| file.code()),
        GraphFile::parse(&app.editor.model.file_text())
            .ok()
            .map(|file| file.code()),
        "Edit model opens the code the graph runs"
    );
}

#[test]
fn a_graph_edit_past_tick_zero_waits_on_the_rebuild_dialog() {
    let Some(mut app) = app(true) else {
        return;
    };
    new_graph_model(&mut app, "sir.toml");
    apply(&mut app);
    step_past(&mut app, 0);
    let running = app.loaded_entry().expect("the graph runs").source().clone();
    let graph = app.editor.model.graph_mut().expect("a graph");
    run(
        graph,
        GraphRequest::Edit(GraphEdit::one(EditCommand::SetLiteral {
            scope: Scope::pass("step"),
            node: n(12),
            port: "a".to_owned(),
            literal: henad_graph::value::Literal::F32(0.9),
        })),
    );
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::AwaitingRebuild));
    assert!(app.pending_rebuild.is_some());
    assert_eq!(
        app.loaded_entry().map(|entry| entry.source()),
        Some(&running),
        "the run keeps its code until Rebuild"
    );
}

#[test]
fn a_refused_edit_changes_nothing_and_names_the_problem() {
    let mut graph = editor("sir.toml");
    let before = graph.text();
    let refused = graph.edit(vec![EditCommand::RemoveParam("infection_rate".to_owned())], None);
    assert!(!refused);
    assert_eq!(graph.text(), before);
    assert!(!graph.history().can_undo());
    let status = graph.status.clone().expect("a message");
    assert!(status.warning, "a refusal shows as a warning");
    let status = status.message;
    assert!(
        status.starts_with("The parameter 'infection_rate' is still named by"),
        "{status}"
    );
    assert!(status.ends_with('.'), "{status}");
}

#[test]
fn undo_and_redo_restore_the_canonical_text() {
    let mut graph = editor("sir.toml");
    let start = graph.text();
    run(
        &mut graph,
        GraphRequest::AddNode {
            op: "math.sin".to_owned(),
            pos: [10.0, 10.0],
            region: None,
            wire: None,
        },
    );
    let added = graph.text();
    assert_ne!(added, start);
    run(&mut graph, GraphRequest::Delete(vec![n(52)]));
    run(&mut graph, GraphRequest::Undo);
    assert_eq!(graph.text(), added);
    run(&mut graph, GraphRequest::Undo);
    assert_eq!(graph.text(), start);
    run(&mut graph, GraphRequest::Redo);
    assert_eq!(graph.text(), added);
}

#[test]
fn edits_under_one_key_make_one_undo_step() {
    let mut graph = editor("sir.toml");
    let start = graph.text();
    for value in [0.5_f32, 0.6, 0.7] {
        let command = EditCommand::SetLiteral {
            scope: Scope::pass("step"),
            node: n(12),
            port: "a".to_owned(),
            literal: henad_graph::value::Literal::F32(value),
        };
        graph.edit(vec![command], Some("literal n12 a"));
    }
    run(&mut graph, GraphRequest::Undo);
    assert_eq!(graph.text(), start);
    assert!(!graph.history().can_undo());
}

#[test]
fn a_node_added_from_a_dropped_wire_takes_the_wire() {
    let mut graph = editor("sir.toml");
    run(
        &mut graph,
        GraphRequest::AddNode {
            op: "logic.not".to_owned(),
            pos: [0.0, 0.0],
            region: None,
            wire: Some(crate::ui::editor::graph::DroppedWire {
                node: n(17),
                port: "out".to_owned(),
                end: crate::ui::editor::graph::quick_add::WireEnd::Output(Some(henad_graph::types::ValueType::Bool)),
            }),
        },
    );
    let pass = graph.open_pass().expect("a pass");
    let added = pass.body.node(n(52)).expect("the node was added");
    assert!(
        matches!(added.inputs.get("a"), Some(InputValue::Link(link)) if link.node == n(17)),
        "{:?}",
        added.inputs
    );
}

#[test]
fn renaming_or_removing_the_open_pass_keeps_a_pass_open() {
    let mut graph = editor("sir.toml");
    run(
        &mut graph,
        GraphRequest::Edit(GraphEdit::one(EditCommand::RenamePass {
            from: "step".to_owned(),
            to: "spread".to_owned(),
        })),
    );
    assert_eq!(graph.scope(), &Scope::pass("spread"));
    run(
        &mut graph,
        GraphRequest::Edit(GraphEdit::one(EditCommand::RemovePass("spread".to_owned()))),
    );
    assert_eq!(graph.scope(), &Scope::pass("seed"), "the first pass opens");
}

#[test]
fn issue_counts_follow_the_checker_per_pass() {
    let mut graph = editor("sir.toml");
    assert_eq!(graph.issue_counts(&Scope::pass("step")), (0, 0));
    run(
        &mut graph,
        GraphRequest::Edit(GraphEdit::one(EditCommand::Disconnect {
            scope: Scope::pass("step"),
            node: n(15),
            port: "b".to_owned(),
        })),
    );
    let (errors, _) = graph.issue_counts(&Scope::pass("step"));
    assert_eq!(errors, 1);
    assert_eq!(graph.issue_counts(&Scope::pass("counts")), (0, 0));
    let problem = graph
        .diagnostics()
        .find(|diagnostic| diagnostic.node.as_deref() == Some("n15"))
        .expect("the problem sits at the node");
    assert_eq!(problem.pass.as_deref(), Some("step"));
}

#[test]
fn a_new_parameter_binds_to_an_input() {
    let mut graph = editor("sir.toml");
    let ran = graph.edit(
        vec![
            EditCommand::AddParam(ParamDescriptor::new(
                "spread_rate",
                "Spread rate",
                ParamKind::F32 {
                    min: 0.0,
                    max: 1.0,
                    default: 0.4,
                    step: None,
                },
            )),
            EditCommand::BindParam {
                scope: Scope::pass("step"),
                node: n(12),
                port: "b".to_owned(),
                param: "spread_rate".to_owned(),
            },
        ],
        None,
    );
    assert!(ran, "{:?}", graph.status);
    assert!(!graph.report().has_errors());
    let node = graph.open_pass().expect("a pass").body.node(n(12)).expect("a node");
    assert_eq!(node.inputs.get("b"), Some(&InputValue::Param("spread_rate".to_owned())));
}

#[test]
fn a_label_or_range_edit_keeps_the_tick_length_hidden() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../henad-models/tests/fixtures/lowered/rates/sir_rates.toml");
    let text = std::fs::read_to_string(&path).expect("the fixture reads");
    let mut graph = GraphEditor::new(&GraphFile::parse(&text).expect("the fixture reads as a graph"));
    assert!(!graph.report().has_errors());
    let tick_length = |graph: &GraphEditor| {
        graph
            .document()
            .params
            .iter()
            .find(|param| param.id() == "tick_length")
            .expect("the tick length")
            .clone()
    };

    let param = tick_length(&graph);
    let relabelled = param_with(&param, "One step is (days)", param.kind().clone());
    assert!(
        graph.edit(vec![EditCommand::SetParam(relabelled)], None),
        "{:?}",
        graph.status
    );
    let ParamKind::F32 { min, default, step, .. } = tick_length(&graph).kind().clone() else {
        panic!("the tick length is an f32");
    };
    let widened = ParamKind::F32 {
        min,
        max: 60.0,
        default,
        step,
    };
    let param = tick_length(&graph);
    assert!(
        graph.edit(
            vec![EditCommand::SetParam(param_with(&param, param.label(), widened))],
            None
        ),
        "{:?}",
        graph.status
    );

    let param = tick_length(&graph);
    assert_eq!(param.label(), "One step is (days)");
    assert!(param.is_hidden(), "the edits keep `hidden`");
    assert!(
        !graph.report().has_errors(),
        "{:?}",
        graph.report().errors().collect::<Vec<_>>()
    );
    assert!(graph.text().contains("tick_unit = \"day\""));
}

#[test]
fn arrange_places_the_open_pass_in_one_undo_step() {
    let mut graph = editor("sir.toml");
    let start = graph.text();
    run(&mut graph, GraphRequest::Arrange);
    assert_ne!(graph.text(), start);
    let code = |graph: &GraphEditor| graph.builder().to_file().expect("reads").code();
    run(&mut graph, GraphRequest::Undo);
    assert_eq!(graph.text(), start);
    assert!(!graph.history().can_undo());
    run(&mut graph, GraphRequest::Redo);
    assert_eq!(
        code(&graph),
        GraphFile::parse(&start).expect("reads").code(),
        "Arrange leaves the code alone"
    );
}

#[test]
fn a_graph_without_placement_opens_arranged() {
    let text = henad_graph::starters::starter("game_of_life.toml")
        .expect("a starter")
        .text;
    let file = GraphFile::parse(text).expect("reads");
    let bare = GraphFile::parse(&file.semantic_text()).expect("the semantic text reads");
    assert!(bare.editor().is_none());
    let graph = GraphEditor::new(&bare);
    for pass in &graph.document().passes {
        for node in &pass.body.nodes {
            assert!(
                graph.builder().editor().node_pos(node.id).is_some(),
                "{} has no place",
                node.id
            );
        }
    }
    assert!(!graph.history().can_undo(), "opening makes no undo step");
}

#[test]
fn a_converted_copy_opens_arranged_with_its_labels() {
    let text = henad_graph::sheet::starters()[0].text;
    let sheet = henad_graph::sheet::SheetFile::parse(text).expect("the Game of Life sheet lowers");
    let options = henad_graph::sheet::ConvertOptions::new(sheet.document(), "2026-11-07", |_, _| false);
    let copy = henad_graph::sheet::convert(&sheet, &options).expect("the sheet converts");
    let labelled = copy.editor().expect("the copy has an editor region");
    let graph = GraphEditor::new(&copy);
    let editor = graph.builder().editor();
    for pass in &graph.document().passes {
        let mut seen: Vec<[f64; 2]> = Vec::new();
        for node in &pass.body.nodes {
            let pos = editor
                .node_pos(node.id)
                .unwrap_or_else(|| panic!("{} of {} has no place", node.id, pass.id));
            assert!(
                !seen.contains(&pos),
                "{} of {} shares the place {pos:?} with another node",
                node.id,
                pass.id
            );
            seen.push(pos);
            let label = |editor: &henad_graph::editor::EditorRegion| editor.node_layout(node.id)?.label;
            assert_eq!(label(editor), label(labelled), "{} keeps its label", node.id);
        }
    }
    assert!(!graph.history().can_undo(), "opening makes no undo step");
}

/// Returns the colours of the text shapes in `shapes` that read `text`, nested shapes included.
fn text_colors(shapes: &[egui::epaint::ClippedShape], text: &str) -> Vec<egui::Color32> {
    fn collect(shape: &egui::Shape, text: &str, colors: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                colors.extend(shape.override_text_color);
                colors.extend(shape.galley.job.sections.iter().map(|section| section.format.color));
                colors.push(shape.fallback_color);
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text, colors);
                }
            }
            _ => {}
        }
    }
    let mut colors = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, text, &mut colors);
    }
    colors
}

#[test]
fn a_copy_confirmation_shows_in_the_text_colour() {
    let Some(mut app) = app(false) else {
        return;
    };
    new_graph_model(&mut app, "sir.toml");
    let ctx = egui::Context::default();
    let graph = app.editor.model.graph_mut().expect("a graph");
    graph.run(&ctx, GraphRequest::Copy(vec![n(12)]));
    assert_eq!(graph.status, Some(GraphStatus::note("Copied 1 node.")));

    let mut shapes = Vec::new();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            ..egui::RawInput::default()
        };
        let mut output = ctx.run_ui(input, |ui| editor_ui(ui, &mut app));
        output.textures_delta.clear();
        shapes = std::mem::take(&mut output.shapes);
    }
    let colors = text_colors(&shapes, "Copied 1 node.");
    assert!(!colors.is_empty(), "the confirmation is drawn");
    let warning = ctx.global_style().visuals.warn_fg_color;
    assert!(
        !colors.contains(&warning),
        "a copy confirmation shows in the warning colour"
    );
}
