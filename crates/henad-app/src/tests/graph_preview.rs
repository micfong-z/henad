//! Tests of the graph editor's live preview, its generated WGSL view and View in viewport: the preview rebuilds from
//! tick 0 after each valid edit at a small size, takes the Viewport only from a run at tick 0 or another preview, and
//! gives way to Apply and Build.

use std::sync::Arc;

use henad_compute::entry::ModelState;
use henad_compute::fault::{BUILDING, Fault};
use henad_compute::gpu::GpuContext;
use henad_compute::simulation::RunSetup;
use henad_core::action::Schedule;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_graph::GraphFile;
use henad_graph::document::World;
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::wgsl::ShaderRole;
use web_time::Instant;

use crate::state::{AppState, PreviewRun};
use crate::tests::support::step_past;
use crate::ui::editor::graph::{GraphEdit, GraphEditor, GraphRequest};
use crate::ui::editor::preview::{DEBOUNCE, PREVIEW_AGENTS, PREVIEW_SIDE, PreviewState, poll_at, preview_values};
use crate::ui::editor::{ApplyOutcome, apply, new_graph_model, new_grid_rule};
use crate::ui::playback::{PREVIEW_APPLY, PREVIEW_ORPHANED, preview_hover};

/// Returns node `number`'s id.
fn n(number: u32) -> NodeId {
    NodeId::new(number).expect("an id")
}

/// Returns an app over the example models on a headless device that runs compute shaders, with the SIR starter in the
/// Editor, or `None` to skip on a machine without a device.
fn app_with_sir() -> Option<AppState> {
    let mut app = AppState::headless(henad_models::example_models(), true)?;
    new_graph_model(&mut app, "sir.toml");
    Some(app)
}

/// Graph in the Editor.
fn graph(app: &mut AppState) -> &mut GraphEditor {
    app.editor.graph.as_deref_mut().expect("a graph")
}

/// Polls the preview once now and once after the debounce, as two frames that far apart would.
fn settle(app: &mut AppState) {
    let ctx = egui::Context::default();
    let now = Instant::now();
    poll_at(&ctx, app, now);
    poll_at(&ctx, app, now + DEBOUNCE);
}

/// Runs an edit that adds the parameter `spread_rate` and binds the infection term of the step to it.
fn bind_new_parameter(app: &mut AppState) {
    let ran = graph(app).edit(
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
    assert!(ran, "{:?}", graph(app).status);
}

/// Returns the parameter ids of the loaded entry.
fn loaded_params(app: &AppState) -> Vec<String> {
    app.loaded_entry()
        .expect("a model is loaded")
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.id().to_owned())
        .collect()
}

#[test]
fn a_new_graph_model_previews_at_the_preview_size() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    let selected = app.selected_model.clone();
    let values = app.param_values.clone();
    let ctx = egui::Context::default();
    let now = Instant::now();
    poll_at(&ctx, &mut app, now);
    assert!(app.editor.preview.is_due(), "the preview waits for the debounce");
    assert_eq!(app.preview(), None);

    poll_at(&ctx, &mut app, now + DEBOUNCE);
    let side = PREVIEW_SIDE.min(1024);
    let run = PreviewRun::Grid {
        width: side,
        height: side,
    };
    assert_eq!(app.preview(), Some(run));
    assert_eq!(app.editor.preview.state, PreviewState::Running(run));
    assert_eq!(app.loaded_id(), Some("node_sir"));
    assert!(app.models.get("node_sir").is_none(), "a preview installs nothing");
    assert_eq!(app.selected_model, selected, "a preview leaves the selection alone");
    assert_eq!(
        app.param_values, values,
        "a preview leaves the next build's values alone"
    );
    assert!(!app.selection_is_loaded());
    assert_eq!(&app.loaded_values[..2], &[ParamValue::U32(side), ParamValue::U32(side)]);
    app.wait_for_build();
    assert!(
        app.sim_thread.is_some() && app.fault.is_none(),
        "the preview failed to build"
    );
    assert!(app.sim_running, "the preview plays");
    step_past(&mut app, 0);
}

#[test]
fn binding_a_parameter_restarts_the_preview() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    settle(&mut app);
    step_past(&mut app, 0);
    assert!(!loaded_params(&app).contains(&"spread_rate".to_owned()));

    bind_new_parameter(&mut app);
    let ctx = egui::Context::default();
    let now = Instant::now();
    poll_at(&ctx, &mut app, now);
    assert!(
        !loaded_params(&app).contains(&"spread_rate".to_owned()),
        "the preview waits for the edits to pause"
    );
    poll_at(&ctx, &mut app, now + DEBOUNCE);
    assert!(loaded_params(&app).contains(&"spread_rate".to_owned()));
    assert!(app.snapshot.is_none(), "the preview restarts from tick 0");
    app.wait_for_build();
    step_past(&mut app, 0);
}

#[test]
fn a_preview_whose_build_fails_at_once_stays_paused_with_nothing_loaded() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    let entry = graph(&mut app).program().clone().expect("the starter compiles").entry();
    let refused = entry.wrap_factory(|_| {
        Arc::new(
            |_: &[ParamValue], _: Option<u64>, _: Option<&GpuContext>| -> Result<ModelState, Fault> {
                Err(Fault::refused(BUILDING, "the device refused the preview"))
            },
        )
    });
    let (values, run) = preview_values(&refused);
    let setup = RunSetup::from_parts(&refused, &values, None, Schedule::default()).expect("a setup");

    // A browser builds inline, and the build's fault arrives before `load_preview` returns.
    app.inline_builds = true;
    app.load_preview(setup, run);
    assert!(
        matches!(app.editor.preview.state, PreviewState::Faulted(_)),
        "{:?}",
        app.editor.preview.state
    );
    assert!(app.fault.is_none(), "the preview's fault opened the fault dialog");
    assert_eq!(app.preview(), None);
    assert!(!app.has_simulation(), "the failed build left a run");
    assert!(!app.sim_running, "Playback shows a run that plays with nothing loaded");
}

#[test]
fn another_document_clears_the_state_of_the_preview() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    let fault = Fault::refused("running on the GPU", "the device refused the step");
    app.editor.show_preview_fault(&fault);
    assert!(matches!(app.editor.preview.state, PreviewState::Faulted(_)));

    // The next document has a checker error, so no preview replaces the state.
    let mut broken = GraphEditor::new(
        &GraphFile::parse(henad_graph::starters::starter("sir.toml").expect("a starter").text).expect("reads"),
    );
    assert!(broken.edit(
        vec![EditCommand::Disconnect {
            scope: Scope::pass("step"),
            node: n(15),
            port: "b".to_owned(),
        }],
        None
    ));
    assert!(broken.has_errors(), "the disconnected input is an error");
    let file = GraphFile::parse(&broken.text()).expect("the broken document reads");
    app.editor.load_graph(&file, None);
    settle(&mut app);
    assert_eq!(
        app.editor.preview.state,
        PreviewState::Idle,
        "the fault of the earlier document shows over this one"
    );

    app.editor.show_preview_fault(&fault);
    new_grid_rule(&mut app);
    assert_eq!(app.editor.preview.state, PreviewState::Idle);
}

#[test]
fn a_preview_that_outlived_its_graph_reports_to_the_fault_dialog() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    settle(&mut app);
    app.wait_for_build();
    assert!(app.preview().is_some());
    assert_eq!(preview_hover(&app), PREVIEW_APPLY);

    new_grid_rule(&mut app);
    assert!(app.preview().is_some(), "the preview outlives its graph");
    assert_eq!(
        preview_hover(&app),
        PREVIEW_ORPHANED,
        "Apply now builds the grid rule, not the preview's graph"
    );
    app.report_fault(Fault::refused("running on the GPU", "the device refused the step"));
    assert!(app.fault.is_some(), "the fault reached nothing the Editor draws");
    assert_eq!(app.editor.preview.state, PreviewState::Idle);
    assert_eq!(app.preview(), None, "the faulted preview is offloaded");
}

#[test]
fn an_invalid_edit_leaves_the_preview_running() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    settle(&mut app);
    app.wait_for_build();
    let running = app.loaded_entry().expect("the preview runs").source().clone();
    let ran = graph(&mut app).edit(
        vec![EditCommand::Disconnect {
            scope: Scope::pass("step"),
            node: n(15),
            port: "b".to_owned(),
        }],
        None,
    );
    assert!(ran && graph(&mut app).has_errors());
    settle(&mut app);
    assert!(!app.editor.preview.is_due());
    assert_eq!(app.loaded_entry().map(|entry| entry.source()), Some(&running));
    assert!(app.sim_thread.is_some(), "the preview kept running");
    assert!(matches!(app.editor.preview.state, PreviewState::Running(_)));
}

#[test]
fn an_edit_during_a_full_size_run_past_tick_zero_leaves_that_run_going() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    step_past(&mut app, 0);
    bind_new_parameter(&mut app);
    settle(&mut app);
    assert_eq!(app.editor.preview.state, PreviewState::Waiting);
    assert_eq!(app.preview(), None, "the full-size run keeps going");
    assert_eq!(app.loaded_values[0], ParamValue::U32(1024));
    assert!(app.pending_rebuild.is_none(), "a preview opens no Rebuild model dialog");

    // The run ends, and the preview takes the Viewport.
    app.offload_simulation();
    settle(&mut app);
    assert!(matches!(app.editor.preview.state, PreviewState::Running(_)));
    assert!(loaded_params(&app).contains(&"spread_rate".to_owned()));
}

#[test]
fn a_build_after_the_edit_replaces_the_waiting_preview() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    apply(&mut app);
    step_past(&mut app, 0);
    bind_new_parameter(&mut app);
    settle(&mut app);
    assert_eq!(app.editor.preview.state, PreviewState::Waiting);

    app.reset_simulation();
    app.wait_for_build();
    settle(&mut app);
    assert_eq!(app.editor.preview.state, PreviewState::Idle);
    assert!(!app.editor.preview.is_due());
    assert_eq!(app.preview(), None, "the Build keeps the Viewport");
}

#[test]
fn a_run_at_tick_zero_gives_way_to_the_preview() {
    let Some(mut app) = AppState::headless(henad_models::example_models(), true) else {
        return;
    };
    app.select_model("gpu_game_of_life");
    app.reset_simulation();
    app.wait_for_build();
    assert!(app.run_at_start());
    new_graph_model(&mut app, "sir.toml");
    settle(&mut app);
    assert_eq!(app.loaded_id(), Some("node_sir"));
    assert!(app.preview().is_some());
    assert_eq!(
        app.selected_model.as_deref(),
        Some("gpu_game_of_life"),
        "the selection stays"
    );
}

#[test]
fn apply_replaces_a_running_preview_with_the_full_size_run() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    settle(&mut app);
    step_past(&mut app, 0);
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    assert_eq!(app.preview(), None);
    assert!(app.pending_rebuild.is_none());
    assert_eq!(&app.loaded_values[..2], &[ParamValue::U32(1024), ParamValue::U32(1024)]);
    assert!(app.selection_is_loaded());
    app.wait_for_build();
    settle(&mut app);
    assert_eq!(app.preview(), None, "an unchanged document builds no preview");
}

#[test]
fn an_agents_graph_previews_at_the_preview_count_and_applies_at_its_own() {
    let Some(mut app) = AppState::headless(henad_models::example_models(), true) else {
        return;
    };
    new_graph_model(&mut app, "boids.toml");
    settle(&mut app);
    assert_eq!(app.preview(), Some(PreviewRun::Agents { count: PREVIEW_AGENTS }));
    assert_eq!(app.loaded_id(), Some("node_boids"));
    assert_eq!(app.loaded_values[0], ParamValue::U32(PREVIEW_AGENTS));
    app.wait_for_build();
    assert!(
        app.sim_thread.is_some() && app.fault.is_none(),
        "the preview failed to build"
    );
    let shaders = graph(&mut app).program().clone().expect("the starter compiles");
    assert!(
        shaders
            .shaders()
            .iter()
            .any(|shader| shader.passes().iter().any(|pass| pass == "flock")),
        "the WGSL view finds the shader of the tick pass"
    );
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    assert_eq!(app.preview(), None);
    assert_eq!(app.loaded_values[0], ParamValue::U32(50000));
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none());
}

#[test]
fn moving_a_node_leaves_the_preview_running() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    settle(&mut app);
    let run = app.preview().expect("the preview runs");
    let key = graph(&mut app).preview_key();
    assert!(graph(&mut app).edit(
        vec![EditCommand::Move {
            node: n(12),
            pos: [400.0, 300.0],
        }],
        None
    ));
    assert_eq!(graph(&mut app).preview_key(), key, "placement is outside the code");
    let ctx = egui::Context::default();
    poll_at(&ctx, &mut app, Instant::now());
    assert!(!app.editor.preview.is_due(), "no preview waits to rebuild");
    assert_eq!(app.preview(), Some(run));
    bind_new_parameter(&mut app);
    assert_ne!(graph(&mut app).preview_key(), key, "an edit of the code moves the key");
}

#[test]
fn a_preview_keeps_a_side_below_the_limit() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    let mut world = graph(&mut app).document().world;
    if let World::Grid(grid) = &mut world {
        grid.width = 100;
    }
    assert!(graph(&mut app).edit(vec![EditCommand::SetWorld(world)], None));
    settle(&mut app);
    assert_eq!(
        app.preview(),
        Some(PreviewRun::Grid {
            width: 100,
            height: PREVIEW_SIDE.min(1024)
        })
    );
}

#[test]
fn the_wgsl_view_reads_the_shaders_of_the_open_pass() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    let program = graph(&mut app).program().clone().expect("the starter compiles");
    let step: Vec<_> = program
        .shaders()
        .iter()
        .filter(|shader| shader.passes().iter().any(|pass| pass == "step"))
        .collect();
    assert_eq!(step.len(), 1);
    assert_eq!(step[0].role(), &ShaderRole::Step);
    assert!(step[0].text().contains("n13_out"), "values are named by their node");
    assert!(
        program
            .shaders()
            .iter()
            .all(|shader| !shader.passes().iter().any(|pass| pass == "seed")),
        "an init pass runs on the host"
    );
}

#[test]
fn view_in_viewport_previews_a_view_pass_of_the_output() {
    let Some(mut app) = app_with_sir() else {
        return;
    };
    settle(&mut app);
    let text = graph(&mut app).text();
    let ctx = egui::Context::default();
    graph(&mut app).run(
        &ctx,
        GraphRequest::View {
            node: n(13),
            port: "out".to_owned(),
        },
    );
    assert_eq!(graph(&mut app).status, None);
    assert_eq!(
        graph(&mut app).viewing().map(|target| target.name()).as_deref(),
        Some("n13.out")
    );
    assert_eq!(graph(&mut app).text(), text, "viewing leaves the document alone");
    let program = graph(&mut app).preview_program().expect("the view file compiles");
    let display = program
        .shaders()
        .iter()
        .find(|shader| *shader.role() == ShaderRole::Display)
        .expect("a display shader");
    assert_eq!(display.passes(), ["view_in_viewport"]);
    let before = app.loaded_entry().expect("the preview runs").source().clone();
    settle(&mut app);
    assert_ne!(
        app.loaded_entry().map(|entry| entry.source()),
        Some(&before),
        "the preview restarted with the view pass"
    );

    // A draw cannot run in a view pass.
    graph(&mut app).run(
        &ctx,
        GraphRequest::View {
            node: n(14),
            port: "out".to_owned(),
        },
    );
    assert!(graph(&mut app).status.is_some(), "the draw was refused");
    assert_eq!(
        graph(&mut app).viewing().map(|target| target.name()).as_deref(),
        Some("n13.out")
    );

    // A removed node stops the view at the next preview.
    graph(&mut app).run(
        &ctx,
        GraphRequest::Edit(GraphEdit::one(EditCommand::RemoveNode {
            scope: Scope::pass("step"),
            node: n(13),
        })),
    );
    drop(graph(&mut app).preview_program());
    assert!(graph(&mut app).viewing().is_none());
    let status = graph(&mut app)
        .status
        .clone()
        .map(|status| status.message)
        .unwrap_or_default();
    assert!(status.starts_with("Stopped viewing n13.out."), "{status}");

    graph(&mut app).run(
        &ctx,
        GraphRequest::View {
            node: n(11),
            port: "count".to_owned(),
        },
    );
    graph(&mut app).run(&ctx, GraphRequest::StopViewing);
    assert!(graph(&mut app).viewing().is_none());
}

/// Repeated trials with `max` trials per cell each tick on a grid of 1024 by 1024 cells. Each cell's cost stays under
/// the per-element limit, and the submission budget refuses the model at its own size.
fn heavy_trials(max: u32) -> String {
    format!(
        r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_heavy_trials"
name = "Heavy trials"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"
id = "node_heavy_trials"

[grid]
width = 1024
height = 1024
boundary = "torus"

[[category]]
id = "outcome"
variants = [
  {{ name = "Miss", label = "Miss", color = [26, 26, 46, 255] }},
  {{ name = "Hit", label = "Hit", color = [242, 166, 59, 255] }},
]

[[lane]]
id = "outcome"
domain = "cells"
type = "cat:outcome"
initial = {{ cat = "outcome.Miss" }}

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = {max}
config.acc = [{{ name = "hit", type = "bool", combine = "or" }}]
in.count = {{ u32 = {max} }}

[[pass.node]]
id = "n2"
op = "rand.chance"
config.key = 2734176385
in.p = {{ f32 = 0.001 }}
region = "n1"

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "hit"
in.value = "n2"
region = "n1"

[[pass.node]]
id = "n4"
op = "logic.select"
in.cond = "n1.hit"
in.if_true = {{ cat = "outcome.Hit" }}
in.if_false = {{ cat = "outcome.Miss" }}

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "outcome"
in.value = "n4"
"#
    )
}

#[test]
fn the_submission_budget_refuses_apply_at_full_size_and_lets_the_preview_run() {
    let Some(mut app) = AppState::headless(henad_models::example_models(), true) else {
        return;
    };
    let file = henad_graph::GraphFile::parse(&heavy_trials(4000)).expect("the file reads");
    let report = file.check();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    app.editor.load_graph(&file, None);

    apply(&mut app);
    match &app.editor.outcome {
        Some(ApplyOutcome::Faulted(problems)) => {
            assert!(problems[0].message.contains("one submission would run"), "{problems:?}");
        }
        other => panic!("the model was not refused: {other:?}"),
    }
    assert!(!app.has_simulation(), "nothing was built");

    settle(&mut app);
    assert!(
        matches!(app.editor.preview.state, PreviewState::Running(_)),
        "{:?}",
        app.editor.preview.state
    );
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none());
}

#[test]
fn every_starter_previews_within_the_device_limits_and_the_work_budget() {
    let starters = henad_graph::starters::starters().iter();
    for starter in starters.chain(henad_graph::starters::agent_starters()) {
        let file = henad_graph::GraphFile::parse(starter.text).expect("a starter reads");
        let entry = crate::ui::editor::graph::GraphProgram::compile(&file)
            .expect("a starter compiles")
            .entry();
        let (values, _) = crate::ui::editor::preview::preview_values(&entry);
        let shortfalls = entry.shortfalls(&values, &wgpu::Limits::default());
        assert!(shortfalls.is_empty(), "{}: {shortfalls:?}", starter.file_name);
    }
}
