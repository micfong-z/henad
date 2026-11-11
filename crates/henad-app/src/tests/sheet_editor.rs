//! Tests of the sheet editor in the Editor tab: a sheet opens by the `kind` in its header, previews paused at tick 0,
//! applies and saves its canonical text, and the New model dialog opens a sheet starter under a name. The Card pane
//! reads each line as its sentence with its notes, red chips and "otherwise", the help popovers, the words rule and the
//! label-in-name rule hold over every starter, a note edit installs nothing, and Open in Editor in the Results tab
//! opens the sheet a sweep recorded.

use std::path::Path;
use std::sync::Arc;

use henad_compute::entry::ModelEntry;
use henad_core::diagnostic::DiagnosticSeverity;
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::spec::{BlockSpec, SweepSpec};
use henad_core::provenance::{CodeOrigin, ModelCode};
use henad_explore::handle::SweepOutput;
use henad_explore::model_file::ModelFile;
use henad_explore::progress::NoProgress;
use henad_explore::result_set::ResultSet;
use henad_explore::sweep::{Provenance, SweepOptions, run_spec};
use henad_graph::sheet::check::check;
use henad_graph::sheet::document::{
    LineKind, MadeOf, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetProperty, SheetValue, Slot, StartRule,
    StartsAs,
};
use henad_graph::sheet::sentence::{A_RANDOM_NUMBER, A_VALUE, BY_CHANCE, CANCEL, CLOSE_HELP, CREATE, STARTER_CHOICES};
use henad_graph::sheet::{ParsedSheet, starters};

use crate::state::{AppState, PreviewRun};
use crate::tests::panes::{
    Seen, assert_control_holds, assert_controls_hold_their_words, assert_no_forbidden_word, shows,
};
use crate::tests::support::{
    Harness, Node, ScratchFolder, assert_built, edited, forbidden_words, headless_app, life_with_a_skipped_line,
    settle_preview, starter_text, step_past, tick_zero_stat, top, wait_for_snapshot, wait_until,
};
use crate::ui::dock::Tab;
use crate::ui::editor::preview::{PREVIEW_AGENTS, PREVIEW_SIDE, PreviewState};
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::card_pane::{CardLine, LineNote, NoteColor, card_lines};
use crate::ui::editor::sheet::new_model::starter_choices;
use crate::ui::editor::sheet::sheet_pane::starts_text;
use crate::ui::editor::{
    ApplyOutcome, EditorModel, apply, edit_model, new_grid_rule, open_text, preview_running_text, save_name,
};
use crate::ui::playback::{press_build, run_refusal, tick_counter};
use crate::ui::results::store::{ResultsSource, ResultsStore};
use crate::ui::results::{open_folder, open_in_editor, poll};

/// Opens `text` in the Editor as Open model opens a picked file.
fn open(app: &mut AppState, text: &str) {
    open_text(&mut app.editor, text.to_owned(), Some("opened.toml".to_owned()));
}

/// Sheet in the Editor.
fn sheet(app: &AppState) -> &SheetEditor {
    app.editor.model.sheet().expect("a sheet")
}

#[test]
fn every_sheet_starter_opens_as_a_sheet_previews_at_tick_zero_plays_and_applies() {
    let Some(mut app) = headless_app() else {
        return;
    };
    for starter in starters() {
        open(&mut app, starter.text);
        assert!(
            matches!(app.editor.model, EditorModel::Sheet(_)),
            "{} opened as {:?}",
            starter.file_name,
            app.editor.model
        );
        assert!(
            !sheet(&app).has_errors(),
            "{}: {:?}",
            starter.file_name,
            sheet(&app).lowering_problems()
        );
        settle_preview(&mut app);
        let document = ParsedSheet::parse(starter.text)
            .expect("the starter reads")
            .into_document();
        let side = |cells: u32| cells.min(PREVIEW_SIDE);
        let run = match document.world.made_of {
            MadeOf::Agents => PreviewRun::Agents {
                count: document.agents[0].count.min(PREVIEW_AGENTS),
            },
            _ => PreviewRun::Grid {
                width: side(document.world.width),
                height: side(document.world.height),
            },
        };
        assert_eq!(app.preview(), Some(run), "{}", starter.file_name);
        assert_eq!(app.editor.preview.state, PreviewState::Running(run));
        assert_eq!(app.loaded_id(), Some(document.sheet.id.as_str()));
        app.wait_for_build();
        assert_built(&app, &format!("{}: the preview", starter.file_name));
        assert!(!app.sim_running, "{}: the preview waits for Play", starter.file_name);
        wait_for_snapshot(&mut app, |tick| tick == 0);

        // Play, as Playback's button presses it.
        app.play();
        wait_for_snapshot(&mut app, |tick| tick > 0);

        apply(&mut app);
        assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied), "{}", starter.file_name);
        assert_eq!(app.preview(), None, "{}: Apply replaces the preview", starter.file_name);
        assert_eq!(app.loaded_id(), Some(document.sheet.id.as_str()));
        app.wait_for_build();
        assert_built(&app, &format!("{}: the applied sheet", starter.file_name));
        step_past(&mut app, 0);
        // The next starter's preview replaces nothing past tick 0.
        app.offload_simulation();
    }
}

#[test]
fn applying_the_life_sheet_keeps_the_example_model_of_that_name() {
    let Some(mut app) = headless_app() else {
        return;
    };
    let example = app
        .models
        .get("game_of_life")
        .expect("the example Game of Life")
        .clone();
    assert_eq!(example.source().origin(), CodeOrigin::Compiled);
    open(&mut app, starter_text("game_of_life.toml"));
    settle_preview(&mut app);
    app.wait_for_build();
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    assert_eq!(app.selected_model.as_deref(), Some("sheet_game_of_life"));
    assert!(app.selection_is_loaded());
    let installed = app.models.get("sheet_game_of_life").expect("the sheet is installed");
    assert!(app.editor.is_applied(installed.source()));
    assert_eq!(app.loaded_id(), Some("sheet_game_of_life"));
    app.wait_for_build();

    // The example keeps its id, its name and its compiled code beside the sheet, and builds that code.
    let kept = app.models.get("game_of_life").expect("the example stays in the set");
    assert_eq!(kept.name(), example.name());
    assert_eq!(kept.source(), example.source());
    assert_eq!(kept.source().origin(), CodeOrigin::Compiled);
    assert_eq!(
        app.models.get("sheet_game_of_life").map(ModelEntry::name),
        Some(example.name())
    );
    app.select_model("game_of_life");
    press_build(&mut app);
    assert_eq!(app.loaded_id(), Some("game_of_life"));
    app.wait_for_build();
    assert_built(&app, "the example");
    assert_eq!(
        app.loaded_entry().map(|entry| entry.source().origin()),
        Some(CodeOrigin::Compiled)
    );
    step_past(&mut app, 0);
}

#[test]
fn save_writes_the_canonical_text_and_the_saved_file_reopens_to_the_same_document() {
    let Some(mut app) = headless_app() else {
        return;
    };
    for starter in starters() {
        open(&mut app, starter.text);
        let saved = app.editor.model.file_text();
        assert_eq!(saved, starter.text, "{} is canonical", starter.file_name);
        let document = sheet(&app).document().clone();
        assert_eq!(saved, document.to_text().expect("the sheet writes"));

        open(&mut app, &saved);
        assert_eq!(sheet(&app).document(), &document, "{} reopened", starter.file_name);
    }

    // A hand-edited sheet saves as its canonical text.
    let hand_edited = edited(
        starter_text("game_of_life.toml"),
        "id = \"sheet_game_of_life\"\nname",
        "id    =   \"sheet_game_of_life\"\nname",
    );
    open(&mut app, &hand_edited);
    assert_eq!(app.editor.model.file_text(), starter_text("game_of_life.toml"));

    app.editor.file_name = None;
    assert_eq!(save_name(&app.editor), "sheet_game_of_life.toml");
}

#[test]
fn a_sheet_with_a_skipped_line_opens_and_previews() {
    let Some(mut app) = headless_app() else {
        return;
    };
    open(&mut app, &life_with_a_skipped_line());
    let report = sheet(&app).report();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(report.is_skipped("l2"), "line 2 is skipped");
    assert!(!sheet(&app).has_errors());
    settle_preview(&mut app);
    assert!(app.preview().is_some(), "a skipped line leaves the preview running");
    app.wait_for_build();
    assert_built(&app, "the preview");
}

#[test]
fn a_sheet_with_a_checker_error_opens_in_the_sheet_editor() {
    let Some(mut app) = headless_app() else {
        return;
    };
    // A parameter default outside its range is an error of the checker. The reader reads it.
    let text = edited(starter_text("game_of_life.toml"), "default = 0.3\n", "default = 7\n");
    open(&mut app, &text);
    let sheet = sheet(&app);
    assert!(sheet.report().has_errors(), "{:?}", sheet.report().diagnostics);
    assert!(sheet.has_errors());
    assert!(
        sheet
            .lowering_problems()
            .iter()
            .all(|problem| problem.severity == DiagnosticSeverity::Error && problem.line.is_some()),
        "{:?}",
        sheet.lowering_problems()
    );
    settle_preview(&mut app);
    assert_eq!(app.preview(), None, "a sheet with an error builds no preview");
    apply(&mut app);
    assert!(
        matches!(&app.editor.outcome, Some(ApplyOutcome::Refused(problems)) if !problems.is_empty()),
        "{:?}",
        app.editor.outcome
    );
}

#[test]
fn a_sheet_whose_text_does_not_read_opens_in_the_code_view_with_its_problem() {
    let Some(mut app) = headless_app() else {
        return;
    };
    let text = format!("{}\n[mystery]\nkey = 1\n", starter_text("game_of_life.toml"));
    open(&mut app, &text);
    assert_eq!(app.editor.model.rule_text(), Some(text.as_str()));
    assert!(
        matches!(&app.editor.outcome, Some(ApplyOutcome::Unread(problems)) if problems.len() == 1),
        "{:?}",
        app.editor.outcome
    );

    // A graph file still opens on the canvas, and a grid rule in the code view.
    open(&mut app, henad_graph::starters::starters()[0].text);
    assert!(app.editor.model.graph().is_some());
    open(&mut app, &crate::ui::editor::life_starter());
    assert!(app.editor.model.rule_text().is_some());
    assert_eq!(app.editor.outcome, None);
}

#[test]
fn a_sheet_preview_waits_at_tick_zero_until_play() {
    let Some(mut app) = headless_app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle_preview(&mut app);
    app.wait_for_build();
    assert!(!app.sim_running, "the preview of a sheet waits for Play");
    let alive = tick_zero_stat(&mut app, "Alive");
    assert!(alive > 0.0, "the start is drawn at tick 0");
    assert_eq!(
        tick_counter(&app).as_deref(),
        Some("Tick 0 (a tick is one step of time)"),
        "Playback explains a tick in the sheet's words"
    );
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        app.sim_thread
            .as_mut()
            .and_then(|thread| thread.take_snapshot())
            .is_none_or(|snapshot| snapshot.tick == 0),
        "the paused preview stepped"
    );

    // Play, as Playback's button presses it.
    app.play();
    let tick = wait_for_snapshot(&mut app, |tick| tick > 0).tick;
    assert!(tick > 0);
    assert!(tick_counter(&app).is_some_and(|counter| counter.ends_with("(a tick is one step of time)")));

    // A graph's preview still plays once it is built.
    crate::ui::editor::new_graph_model(&mut app, "sir.toml");
    settle_preview(&mut app);
    app.wait_for_build();
    assert!(app.sim_running, "the preview of a graph plays");
    assert!(!app.runs_editor_sheet());
}

#[test]
fn the_sheet_preview_line_names_play_and_the_graph_line_is_unchanged() {
    let run = PreviewRun::Grid {
        width: 256,
        height: 256,
    };
    assert_eq!(
        preview_running_text(run, false),
        "Preview at 256 × 256 cells starts at tick 0. Press Play to run it, or Apply to run the model at its own size."
    );
    assert_eq!(
        preview_running_text(run, true),
        "Preview runs at 256 × 256 cells from tick 0. Press Apply to run the model at its own size."
    );
}

#[test]
fn a_rebuild_replays_the_same_start() {
    let Some(mut app) = headless_app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle_preview(&mut app);
    let first = tick_zero_stat(&mut app, "Alive");
    app.play();
    wait_for_snapshot(&mut app, |tick| tick > 0);

    // Build replays the preview's start while the Editor holds the sheet.
    assert!(app.previews_editor_sheet());
    press_build(&mut app);
    assert!(app.preview().is_some(), "Build kept the preview");
    assert!(!app.sim_running, "the replayed start waits for Play");
    assert_eq!(tick_zero_stat(&mut app, "Alive"), first);

    // Once applied, Build twice at one seed draws one start, and another seed another.
    apply(&mut app);
    let start_at = |app: &mut AppState, seed: u64| {
        app.seed = Some(seed);
        press_build(app);
        tick_zero_stat(app, "Alive")
    };
    let seven = start_at(&mut app, 7);
    assert_eq!(start_at(&mut app, 7), seven);
    assert_ne!(start_at(&mut app, 8), seven, "the seed draws the start");
}

/// Run to tick drives the preview of the Editor's sheet: Run is enabled while the preview shows, runs the preview to
/// the tick, and replays its start for a tick behind the current one. Once another model is selected, Run waits for
/// Build again.
#[test]
fn run_to_tick_drives_the_sheet_preview() {
    let Some(mut app) = headless_app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle_preview(&mut app);
    app.wait_for_build();
    let first = tick_zero_stat(&mut app, "Alive");
    assert_eq!(run_refusal(&app, None, 0), None);

    app.run_to(5);
    assert_eq!(wait_for_snapshot(&mut app, |tick| tick >= 5).tick, 5);
    assert!(app.previews_editor_sheet());
    assert_eq!(run_refusal(&app, None, 5), None);

    app.run_to(2);
    assert!(app.previews_editor_sheet(), "a tick behind replays the preview");
    app.wait_for_build();
    assert_eq!(wait_for_snapshot(&mut app, |tick| tick >= 2).tick, 2);

    press_build(&mut app);
    assert_eq!(
        tick_zero_stat(&mut app, "Alive"),
        first,
        "the replay drew the same start"
    );

    app.select_model("boids");
    assert!(run_refusal(&app, None, 0).is_some_and(|reason| reason.starts_with("Selected model not loaded")));
}

#[test]
fn build_builds_another_selected_model_instead_of_the_sheet_preview() {
    let Some(mut app) = headless_app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle_preview(&mut app);
    app.wait_for_build();
    assert!(app.build_replays_preview());

    let selection = app.selected_model.clone().expect("a selected model");
    assert_ne!(selection, "boids");

    // Choosing another model in the Model tab leaves the preview running, and Build then builds that model.
    app.select_model("boids");
    assert!(app.previews_editor_sheet(), "selecting builds nothing");
    assert!(!app.build_replays_preview());
    app.select_model(&selection);
    assert!(
        app.build_replays_preview(),
        "the selection at the preview's load replays it again"
    );
    app.select_model("boids");
    press_build(&mut app);
    assert_eq!(app.preview(), None, "Build replaced the preview");
    assert_eq!(app.loaded_id(), Some("boids"));
    app.wait_for_build();
    assert!(app.selection_is_loaded() && app.fault.is_none());
}

/// Cover property of the forest fire `document`.
fn cover(document: &mut SheetDocument) -> &mut SheetProperty {
    document
        .cells
        .iter_mut()
        .flatten()
        .find(|property| property.id == "cover")
        .expect("the cover property")
}

#[test]
fn the_cells_table_reads_each_start_in_the_wording_table_words() {
    let mut document = ParsedSheet::parse(starter_text("forest_fire.toml"))
        .expect("the starter reads")
        .into_document();
    assert_eq!(starts_text(&document.clone(), cover(&mut document)), BY_CHANCE);

    let mut all_trees = StartsAs::new(StartRule::Value);
    all_trees.value = Slot::Filled(SheetValue::Chip(SheetChip::State("tree".to_owned())));
    cover(&mut document).starts_as = Some(all_trees);
    assert_eq!(starts_text(&document.clone(), cover(&mut document)), "All Tree");

    let mut random = StartsAs::new(StartRule::Value);
    random.value = Slot::Filled(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::RandomNumber))));
    cover(&mut document).starts_as = Some(random);
    assert_eq!(starts_text(&document.clone(), cover(&mut document)), A_RANDOM_NUMBER);

    let mut number = StartsAs::new(StartRule::Value);
    number.value = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(2.0)));
    cover(&mut document).starts_as = Some(number);
    assert_eq!(starts_text(&document.clone(), cover(&mut document)), A_VALUE);
}

#[test]
fn the_new_model_dialog_offers_every_starter_with_its_description() {
    let choices = starter_choices();
    assert_eq!(choices[0].name, "Blank", "Blank comes first");
    assert_eq!(choices[0].file_name, None);
    let offered: Vec<&str> = choices.iter().filter_map(|choice| choice.file_name).collect();
    let mut agent_starters = 0;
    for starter in starters() {
        let document = ParsedSheet::parse(starter.text)
            .expect("the starter reads")
            .into_document();
        agent_starters += usize::from(document.world.made_of == MadeOf::Agents);
        assert!(offered.contains(&starter.file_name), "{}", starter.file_name);
    }
    assert_eq!(agent_starters, 4, "the four agent starters are offered");
    for choice in &choices {
        assert!(choice.description.is_some(), "{} has a description", choice.name);
    }
}

#[test]
fn new_model_opens_a_dialog_and_create_opens_the_chosen_sheet() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.click("New model");
    let nodes = harness.settle();
    assert!(
        nodes
            .iter()
            .any(|node| node.role == egui::accesskit::Role::Dialog && node.label == "New model"),
        "the dialog has a dialog role"
    );
    let dialog = harness.app.editor.new_model.clone().expect("the dialog is open");
    let names: Vec<&str> = dialog.choices().iter().map(|choice| choice.name.as_str()).collect();
    // Blank, the four grid starters, then the four agent starters, each in the wording table's order.
    let expected: Vec<&str> = STARTER_CHOICES.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, expected);
    // The world question under Blank holds radio buttons of its own.
    let mut radios: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.role == egui::accesskit::Role::RadioButton)
        .filter(|node| node.label != "Cells on a grid" && node.label != "Agents in open space")
        .collect();
    radios.sort_by(|a, b| top(a).total_cmp(&top(b)));
    let radios: Vec<&str> = radios.iter().map(|node| node.label.as_str()).collect();
    assert_eq!(radios, names, "Start from lists the sheet starters");
    let labels: Vec<&str> = nodes.iter().map(|node| node.label.as_str()).collect();
    for text in [
        "Start from",
        "Conway's Life. A dead cell with 3 live neighbours comes alive.",
        "Cancel",
        "Create",
    ] {
        assert!(
            nodes.iter().any(|node| node.label == text || node.value == text),
            "{text} is missing from {labels:?}"
        );
    }
    let name = |nodes: &[Node]| {
        nodes
            .iter()
            .find(|node| node.role == egui::accesskit::Role::TextInput)
            .map(|node| node.value.clone())
    };
    assert_eq!(
        name(&nodes).as_deref(),
        Some("Untitled model"),
        "Blank is named Untitled model"
    );

    harness.click("Majority vote");
    assert_eq!(name(&harness.settle()).as_deref(), Some("Majority vote"));

    // A typed name stays when the starter changes.
    harness.click_name_field();
    harness.replace_text("My life");
    harness.click("Game of Life");
    assert_eq!(name(&harness.settle()).as_deref(), Some("My life"));

    harness.click("Create");
    harness.settle();
    assert!(harness.app.editor.new_model.is_none(), "Create closes the dialog");
    let document = harness
        .app
        .editor
        .model
        .sheet()
        .expect("Create opened a sheet")
        .document();
    assert_eq!(document.sheet.id, "sheet_game_of_life");
    assert_eq!(document.sheet.name, "My life");
    let labels = harness.labels();
    for line in [
        "Line 1: Alive neighbours = count of the 8 cells around me whose Status is Alive",
        "Line 2: Become Alive from Dead, when Alive neighbours is 3",
        "Line 3: Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3",
    ] {
        assert!(labels.iter().any(|label| label == line), "{line} is missing");
    }

    // Cancel closes the dialog and leaves the Editor alone, and so does Escape.
    harness.click("New model");
    harness.click("Cancel");
    harness.settle();
    assert!(harness.app.editor.new_model.is_none());
    assert_eq!(
        harness
            .app
            .editor
            .model
            .sheet()
            .map(|sheet| sheet.document().sheet.name.as_str()),
        Some("My life")
    );
    harness.click("New model");
    harness.press(egui::Key::Escape, egui::Modifiers::NONE);
    harness.settle();
    assert!(harness.app.editor.new_model.is_none());
}

#[test]
fn enter_in_the_name_field_creates_the_model() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.click("New model");
    harness.click("Forest fire");
    harness.click_name_field();
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);
    harness.settle();
    let sheet = harness.app.editor.model.sheet().expect("Enter opened a sheet");
    assert_eq!(sheet.document().sheet.name, "Forest fire");
    assert_eq!(
        sheet.text(),
        starter_text("forest_fire.toml"),
        "an unchanged name keeps the starter"
    );
}

#[test]
fn new_node_graph_lists_the_six_graph_starters_from_the_header_and_the_file_menu() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let graph_names: Vec<String> = henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters())
        .map(|starter| {
            henad_graph::GraphFile::parse(starter.text)
                .expect("reads")
                .document()
                .name
                .clone()
        })
        .collect();
    assert_eq!(graph_names.len(), 6);

    harness.click("New node graph");
    let labels = harness.labels();
    for name in &graph_names {
        assert!(labels.contains(name), "the header menu lacks {name}: {labels:?}");
    }
    harness.press(egui::Key::Escape, egui::Modifiers::NONE);
    harness.settle();

    harness.click("File");
    let labels = harness.labels();
    let entries: Vec<&String> = labels
        .iter()
        .filter(|label| {
            label.ends_with("New model") || label.ends_with("New node graph") || label.ends_with("New grid rule")
        })
        .collect();
    assert_eq!(
        entries.len(),
        6,
        "the File menu and the header each hold the three: {entries:?}"
    );
    assert!(!labels.iter().any(|label| label.contains("New graph model")));
    // Two widgets read New node graph now: the header's menu and the File menu's submenu. The submenu is the second.
    let nodes = harness.settle();
    let submenu = nodes
        .iter()
        .filter(|node| node.label.ends_with("New node graph") && node.bounds.is_some())
        .max_by(|a, b| top(a).total_cmp(&top(b)))
        .and_then(|node| node.bounds)
        .expect("the File menu's New node graph")
        .center();
    harness.click_at(submenu);
    let labels = harness.labels();
    for name in &graph_names {
        assert!(labels.contains(name), "the File menu lacks {name}: {labels:?}");
    }
    harness.click("Boids Flocking");
    harness.settle();
    assert!(
        harness.app.editor.model.graph().is_some(),
        "the File menu opened the graph starter"
    );
}

#[test]
fn the_empty_editor_names_every_way_in() {
    assert_eq!(
        crate::ui::editor::EMPTY_EDITOR,
        "Editor is empty. Press New model, New node graph, New grid rule or Open model to start."
    );
}

// The Card pane's lines: sentences, notes, red chips and "otherwise"

#[test]
fn the_card_pane_reads_the_life_lines_as_their_sentences() {
    let document = ParsedSheet::parse(starter_text("game_of_life.toml"))
        .expect("the starter reads")
        .into_document();
    let sentences = henad_graph::sheet::sentence::Sentences::new(&document);
    let lines: Vec<(usize, String)> = card_lines(&document, &sentences, &check(&document), 0)
        .into_iter()
        .map(|line| (line.number, line.sentence))
        .collect();
    assert_eq!(
        lines,
        [
            (
                1,
                "Alive neighbours = count of the 8 cells around me whose Status is Alive".to_owned()
            ),
            (2, "Become Alive from Dead, when Alive neighbours is 3".to_owned()),
            (
                3,
                "Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3".to_owned()
            ),
        ]
    );
    assert_eq!(sentences.card_heading(&document.cards[0]), "Each tick, every cell");

    let vote = ParsedSheet::parse(starter_text("vote.toml"))
        .expect("the vote reads")
        .into_document();
    let sentences = henad_graph::sheet::sentence::Sentences::new(&vote);
    let lines = card_lines(&vote, &sentences, &check(&vote), 0);
    assert_eq!(
        lines[2].sentence,
        "otherwise (when line 2 does not apply) become A from any state, always"
    );
}

/// Returns the lines of card `card` of the sheet `text` as the Card pane shows them.
fn lines_of(text: &str, card: usize) -> Vec<CardLine> {
    let document = ParsedSheet::parse(text).expect("the sheet reads").into_document();
    let sentences = henad_graph::sheet::sentence::Sentences::new(&document);
    card_lines(&document, &sentences, &check(&document), card)
}

/// Returns Game of Life with line 2 moved above line 1. Line 1 then uses Alive neighbours from below it.
fn life_with_a_result_below() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l2\", \"l1\", \"l3\"]",
    )
}

/// Returns Game of Life with an old state on line 2 that Status lacks, an error of the checker at that line.
fn life_with_an_error_at_line_two() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "start_state = \"dead\"\n",
        "start_state = \"zombie\"\n",
    )
}

#[test]
fn every_line_of_the_life_starter_reads_its_sentence_by_accessible_name() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    let nodes = harness.settle();
    let mut lines: Vec<&Node> = nodes
        .iter()
        .filter(|node| {
            node.role == egui::accesskit::Role::Group
                && (node.label.starts_with("Line ") || node.label.starts_with("Clear line "))
        })
        .collect();
    lines.sort_by(|a, b| top(a).total_cmp(&top(b)));
    let labels: Vec<&str> = lines.iter().map(|node| node.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Line 1: Alive neighbours = count of the 8 cells around me whose Status is Alive",
            "Line 2: Become Alive from Dead, when Alive neighbours is 3",
            "Line 3: Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3",
            "Clear line 1: Become Dead from any state, always",
        ]
    );
    let named =
        |role: egui::accesskit::Role, label: &str| nodes.iter().any(|node| node.role == role && node.label == label);
    for (role, label) in [
        (egui::accesskit::Role::Group, "Sheet"),
        (egui::accesskit::Role::Group, "Card"),
        (egui::accesskit::Role::List, "Lines"),
        (egui::accesskit::Role::List, "Lines of Clear"),
        (egui::accesskit::Role::Button, "Alive neighbours"),
        (egui::accesskit::Role::Button, "Alive"),
        (egui::accesskit::Role::Button, "any state"),
        (egui::accesskit::Role::Button, "Clear line 1: choose when, now always"),
        (egui::accesskit::Role::ComboBox, "Line 2: comparison"),
        (egui::accesskit::Role::Button, "Help: lines"),
        (egui::accesskit::Role::Button, "Help: World"),
    ] {
        assert!(named(role, label), "no {role:?} named {label}");
    }
    let texts = harness.accessible_texts();
    for text in [
        "Each tick, every cell",
        "3 lines",
        "1 line",
        henad_graph::sheet::sentence::LAW,
    ] {
        assert!(texts.iter().any(|known| known == text), "{text} is missing");
    }
}

#[test]
fn a_skipped_line_shows_its_note_under_it() {
    let lines = lines_of(&life_with_a_skipped_line(), 0);
    assert_eq!(
        lines[1].notes,
        [LineNote {
            text: "Not run yet: choose the old state".to_owned(),
            color: NoteColor::Amber,
            action: None,
            description: None,
        }]
    );
    assert!(lines[0].notes.is_empty() && lines[2].notes.is_empty());
    assert!(
        lines[1]
            .controls()
            .iter()
            .any(|control| control.name == "Line 2: choose the old state"),
        "the empty old state is a blank"
    );

    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(&life_with_a_skipped_line());
    let nodes = harness.settle();
    let top_of = |text: &str| {
        nodes
            .iter()
            .find(|node| node.label.starts_with(text) || node.value == text)
            .map(top)
            .unwrap_or_else(|| panic!("{text} is missing"))
    };
    let note = top_of("Not run yet: choose the old state");
    assert!(
        top_of("Line 2: ") < note && note < top_of("Line 3: "),
        "the note sits under line 2"
    );
}

#[test]
fn a_checker_error_shows_at_its_line() {
    let lines = lines_of(&life_with_an_error_at_line_two(), 0);
    assert!(
        lines[1]
            .notes
            .iter()
            .any(|note| note.color == NoteColor::Red && note.text.contains("zombie")),
        "{:?}",
        lines[1].notes
    );

    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(&life_with_an_error_at_line_two());
    let nodes = harness.settle();
    let error = nodes
        .iter()
        .filter(|node| node.value.contains("zombie") && node.role == egui::accesskit::Role::Label)
        .map(top)
        .collect::<Vec<_>>();
    let line = |prefix: &str| {
        nodes
            .iter()
            .find(|node| node.label.starts_with(prefix))
            .map(top)
            .expect("the line")
    };
    assert_eq!(
        error.len(),
        1,
        "the error shows once, under its line and not in the list below"
    );
    assert!(line("Line 2: ") < error[0] && error[0] < line("Line 3: "));
}

/// Returns Game of Life with a line table `l9` that no lines list names. The card never shows it.
fn life_with_an_unlisted_line() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "# Lines of Clear",
        "[card.line.l9]\nkind = \"become\"\nproperty = \"status\"\nnew_state = \"dead\"\nstart_state = \"alive\"\n\n\
         # Lines of Clear",
    )
}

#[test]
fn an_error_at_a_line_the_card_does_not_show_stays_in_the_list() {
    let text = life_with_an_unlisted_line();
    let document = ParsedSheet::parse(&text).expect("the sheet reads").into_document();
    let messages: Vec<String> = check(&document)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.line.as_deref() == Some("l9"))
        .map(|diagnostic| diagnostic.diagnostic.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("listed in no lines list")),
        "{messages:?}"
    );

    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(&text);
    let nodes = harness.settle();
    for message in &messages {
        let shown = nodes
            .iter()
            .filter(|node| node.role == egui::accesskit::Role::Label && node.value.contains(message.as_str()))
            .count();
        assert_eq!(shown, 1, "{message} shows {shown} times");
    }
}

#[test]
fn a_result_used_above_its_line_is_red_with_its_tooltip() {
    let lines = lines_of(&life_with_a_result_below(), 0);
    assert_eq!(lines[0].sentence, "Become Alive from Dead, when Alive neighbours is 3");
    assert_eq!(
        lines[0]
            .red_chips
            .iter()
            .map(|(_, tooltip)| tooltip.as_str())
            .collect::<Vec<_>>(),
        ["Alive neighbours is below this line. Move this line under it, or choose another value."]
    );
    assert_eq!(
        lines[0].notes,
        [LineNote {
            text: "Uses Alive neighbours, which is below this line".to_owned(),
            color: NoteColor::Red,
            action: None,
            description: None,
        }]
    );
    assert!(lines[2].red_chips.is_empty(), "line 3 sits below the result it uses");
}

#[test]
fn chips_name_the_property_when_two_share_a_state_name() {
    let mut document = ParsedSheet::parse(starter_text("game_of_life.toml"))
        .expect("the starter reads")
        .into_document();
    let mut mood = SheetProperty::new("mood", "Mood", henad_graph::sheet::document::PropertyType::States);
    mood.states = vec![
        henad_graph::sheet::document::SheetState::new("calm", "Calm", [0, 0, 255, 255]),
        henad_graph::sheet::document::SheetState::new("lively", "Alive", [255, 0, 0, 255]),
    ];
    document.cells.as_mut().expect("cells").push(mood);
    let sentences = henad_graph::sheet::sentence::Sentences::new(&document);
    let lines = card_lines(&document, &sentences, &check(&document), 0);
    let chips: Vec<String> = lines[1].controls().into_iter().map(|control| control.name).collect();
    assert_eq!(chips[..2], ["Status Alive".to_owned(), "Dead".to_owned()]);
    assert_eq!(
        lines[1].swatches.iter().map(|(_, color)| *color).collect::<Vec<_>>(),
        [
            egui::Color32::from_rgb(0x00, 0xe6, 0x76),
            egui::Color32::from_rgb(0x15, 0x15, 0x15)
        ],
        "each state chip carries its state's colour"
    );
}

/// Inside a Repeat the first time a change applies decides, so the "otherwise" of a line whose higher line sits in its
/// own Repeat says the higher line wins at the same time. A line outside the Repeat, whose higher lines share it, says
/// the first time round decides between them.
#[test]
fn otherwise_inside_a_repeat_says_the_higher_line_wins_at_the_same_time() {
    let text = edited(
        starter_text("random_walk.toml"),
        "[[parameter]]",
        "[[agents.property]]\nid = \"status\"\nname = \"Status\"\ntype = \"states\"\nstates = [\n  \
         { id = \"s\", name = \"S\", color = \"#E8E8F0\" },\n  { id = \"a\", name = \"A\", color = \"#3DD58C\" },\n\
         ]\n\n[agents.property.starts_as]\nrule = \"value\"\nvalue = { state = \"s\" }\n\n[[parameter]]",
    );
    let text = edited(&text, "next_line = 3", "next_line = 6");
    let text = edited(&text, "lines = [\"l1\"]", "lines = [\"l1\", \"l5\"]");
    let text = edited(&text, "lines = [\"l2\"]", "lines = [\"l2\", \"l3\", \"l4\"]");
    let become_line = |id: &str, key: u32| {
        format!(
            "\n[card.line.{id}]\nkind = \"become\"\nkey = {key}\nproperty = \"status\"\nnew_state = \"a\"\n\
             start_state = \"s\"\n\n[card.line.{id}.condition]\nkind = \"chance\"\nchance = 0.5\n"
        )
    };
    let text = format!(
        "{text}{}{}{}",
        become_line("l3", 3),
        become_line("l4", 4),
        become_line("l5", 5)
    );
    let lines = lines_of(&text, 0);
    assert_eq!(lines[3].otherwise, ["l3"]);
    assert_eq!(
        lines[3].otherwise_tooltip,
        "Line 3 also changes Status for a walker that is S. When both apply at the same time, line 3 wins."
    );
    assert_eq!(lines[4].otherwise, ["l3", "l4"]);
    assert_eq!(
        lines[4].otherwise_tooltip,
        "Lines 3 and 4 also change Status for a walker that is S. When more than one applies, the highest line wins. \
         Between lines in one Repeat, the first time round that one applies decides."
    );
}

#[test]
fn otherwise_carries_its_partner_as_a_description() {
    let lines = lines_of(starter_text("forest_fire.toml"), 0);
    assert_eq!(lines[2].otherwise, ["l2"]);
    let tooltip = "Line 2 also changes Cover for a cell that is Tree. When both apply, line 2 wins.";
    assert_eq!(lines[2].otherwise_tooltip, tooltip);

    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("forest_fire.toml"));
    let nodes = harness.settle();
    let otherwise: Vec<&Node> = nodes
        .iter()
        .filter(|node| {
            node.role == egui::accesskit::Role::Label && node.label == "otherwise (when line 2 does not apply)"
        })
        .collect();
    assert_eq!(otherwise.len(), 1, "{otherwise:?}");
    assert_eq!(otherwise[0].description, tooltip);
    assert!(
        nodes.iter().any(|node| node.label
            == "Line 3: otherwise (when line 2 does not apply) become Burning from Tree, with chance Lightning chance"),
        "line 3 reads its otherwise"
    );
}

// Help popovers, the words rule and the label-in-name rule

#[test]
fn a_help_popover_opens_and_closes() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.click("Help: World");
    let nodes = harness.settle();
    assert!(
        nodes
            .iter()
            .any(|node| node.role == egui::accesskit::Role::Dialog && node.label == "World help"),
        "the popover has a dialog role"
    );
    let texts = harness.accessible_texts();
    for text in [
        henad_graph::sheet::sentence::SheetSection::World.help(),
        "Example:",
        "One tick is 1 day",
    ] {
        assert!(texts.iter().any(|known| known == text), "{text} is missing");
    }
    harness.click("Close help");
    let texts = harness.accessible_texts();
    assert!(
        !texts.iter().any(|known| known == "Close help"),
        "Close help closed the popover"
    );
}

#[test]
fn each_lines_help_button_opens_its_own_popover() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let two_tick_cards = edited(
        starter_text("game_of_life.toml"),
        "runs = \"once\"",
        "runs = \"each_tick\"",
    );
    harness.open_sheet(&two_tick_cards);
    let nodes = harness.settle();
    let mut buttons: Vec<egui::Pos2> = nodes
        .iter()
        .filter(|node| node.role == egui::accesskit::Role::Button && node.label == "Help: lines")
        .filter_map(|node| node.bounds.map(|bounds| bounds.center()))
        .collect();
    // The lower button first. Its popover opens below it and leaves the upper button free to press.
    buttons.sort_by(|a, b| b.y.total_cmp(&a.y));
    assert_eq!(buttons.len(), 2, "each card that runs every tick has a Lines help");
    let lines_help = |harness: &mut Harness| -> Vec<egui::Rect> {
        harness
            .settle()
            .into_iter()
            .filter(|node| node.role == egui::accesskit::Role::Dialog && node.label == "Lines help")
            .filter_map(|node| node.bounds)
            .collect()
    };
    harness.click_at(buttons[0]);
    assert_eq!(lines_help(&mut harness).len(), 1, "the first button opens a popover");
    // A press on the second button opens its own popover. A shared one would close instead.
    harness.click_at(buttons[1]);
    let open = lines_help(&mut harness);
    assert_eq!(open.len(), 1, "the second button opens its own popover");
    assert!(
        open[0].top() >= buttons[1].y,
        "the popover sits below the second button"
    );
}

#[test]
fn the_sheet_editor_never_shows_a_forbidden_word() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let mut seen = Seen::default();
    for starter in starters() {
        harness.open_sheet(starter.text);
        seen.look(&mut harness);
        let made_of = ParsedSheet::parse(starter.text)
            .expect("the starter reads")
            .document()
            .world
            .made_of;
        let helps = henad_graph::sheet::sentence::SheetSection::shown(made_of)
            .iter()
            .map(|section| section.help_name())
            .chain(["Help: lines".to_owned()]);
        for help in helps {
            harness.click(&help);
            seen.look(&mut harness);
            harness.press(egui::Key::Escape, egui::Modifiers::NONE);
        }
    }
    for text in [life_with_a_skipped_line(), life_with_a_result_below()] {
        harness.open_sheet(&text);
        seen.look(&mut harness);
    }
    harness.click("New model");
    assert!(shows(&mut harness, "Start from"), "the New model dialog was drawn");
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

#[test]
fn every_accessible_name_holds_its_visible_words() {
    // Every section's help button, the last at the foot of a long sheet, takes a click.
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    for text in starters()
        .iter()
        .map(|starter| starter.text.to_owned())
        .chain([life_with_a_skipped_line(), life_with_a_result_below()])
    {
        harness.open_sheet(&text);
        assert_controls_hold_their_words(&mut harness);
    }
    // Each help popover and the New model dialog hold controls whose visible words are fixed.
    harness.open_sheet(starter_text("game_of_life.toml"));
    for help in HELP_BUTTONS {
        harness.click(help);
        let nodes = harness.settle();
        assert_control_holds(&nodes, CLOSE_HELP);
        harness.press(egui::Key::Escape, egui::Modifiers::NONE);
    }
    harness.click("New model");
    let nodes = harness.settle();
    for visible in [CREATE, CANCEL]
        .into_iter()
        .chain(starter_choices().iter().map(|choice| choice.name.as_str()))
    {
        assert_control_holds(&nodes, visible);
    }
}

/// Accessible names of the "?" buttons of the Game of Life sheet, each of which opens a help popover.
const HELP_BUTTONS: [&str; 8] = [
    "Help: Purpose",
    "Help: World",
    "Help: Cells",
    "Help: Parameters",
    "Help: Measures",
    "Help: Colours",
    "Help: Buttons",
    "Help: lines",
];

#[test]
fn the_world_tabs_show_no_forbidden_word_from_a_sheet() {
    for starter in starters() {
        let entry = entry_of(starter.text);
        let mut texts = vec![entry.name().to_owned(), entry.description().to_owned()];
        texts.extend(
            entry
                .param_descriptors()
                .iter()
                .filter(|param| !param.is_hidden())
                .map(|param| param.label().to_owned()),
        );
        texts.extend(entry.stat_descriptors().iter().map(|stat| stat.label().to_owned()));
        texts.extend(
            entry
                .action_descriptors()
                .iter()
                .map(|action| action.label().to_owned()),
        );
        texts.push(tick_counter_words(entry.metadata()));
        let found = forbidden_words(&texts);
        assert!(found.is_empty(), "{}: {found:?}", starter.file_name);
    }
}

/// Returns the words of the tick counter that the run of a sheet with `metadata` shows at tick 4, one tick long.
fn tick_counter_words(metadata: &henad_core::metadata::ModelMetadata) -> String {
    let elapsed = metadata.time.as_ref().map(|scale| {
        crate::ui::time_axis::elapsed_text(
            4,
            crate::ui::time_axis::RunTime {
                unit: scale.unit,
                tick_length: 1.0,
            },
        )
    });
    henad_graph::sheet::sentence::tick_counter(4, elapsed.as_deref())
}

// A note edit, Edit model and the sheet a sweep recorded

/// Returns the Game of Life sheet with a note on line 2. The note leaves the lowered graph alone.
fn noted_life() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "start_state = \"dead\"\n",
        "start_state = \"dead\"\nnote = \"Birth\"\n",
    )
}

/// Returns the Game of Life sheet on a 16 by 16 grid.
fn small_life() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "width = 1024\nheight = 1024\n",
        "width = 16\nheight = 16\n",
    )
}

/// Returns the entry of the model file `text`, which must build.
fn entry_of(text: &str) -> ModelEntry {
    ModelFile::parse(text)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("the model file does not build: {error}"))
}

/// Returns a sweep of model `id` over Initial density for 2 steps.
fn density_sweep(id: &str) -> SweepSpec {
    let mut spec = SweepSpec::new(id);
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param(
            "initial_density",
            LevelSpec::Values(vec!["0.2".to_owned(), "0.4".to_owned()]),
        )],
        design_seed: None,
    }];
    spec.run.steps = 2;
    spec
}

/// Sweeps `entry` over Initial density into `folder` on the app's device.
fn sweep_into(app: &AppState, entry: &ModelEntry, folder: &Path) {
    drop(std::fs::remove_dir_all(folder));
    let options = SweepOptions::new(Provenance::new(henad_core::build_info!(), Vec::new()));
    let output = SweepOutput::Directory(folder.to_owned());
    run_spec(
        entry,
        app.gpu_ctx.as_ref(),
        &density_sweep(entry.id()),
        output,
        &options,
        &mut NoProgress,
    )
    .expect("the sweep runs");
}

/// Opens `folder` in the Results tab, and waits until its results show.
fn open_results(app: &mut AppState, folder: &Path) {
    open_folder(app, folder.to_owned());
    let ctx = egui::Context::default();
    wait_until("the folder's results to show", || {
        poll(&ctx, app);
        app.results
            .store()
            .is_some_and(|store| store.source == ResultsSource::Folder(folder.to_owned()))
    });
}

#[test]
fn a_note_edit_does_not_reinstall_the_model() {
    let Some(mut app) = headless_app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    apply(&mut app);
    app.wait_for_build();
    step_past(&mut app, 0);
    let id = sheet(&app).document().sheet.id.clone();
    let before = app.models.get(&id).expect("the sheet is installed").source().clone();
    assert_eq!(before.author().map(ModelCode::text), Some(sheet(&app).text()));

    open(&mut app, &noted_life());
    let noted_text = sheet(&app).text().to_owned();
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Unchanged));
    assert!(app.pending_rebuild.is_none() && !app.is_building(), "nothing rebuilds");
    assert!(
        app.snapshot.as_ref().is_some_and(|snapshot| snapshot.tick > 0),
        "the run kept going"
    );
    let held = app.models.get(&id).expect("the sheet is installed").source().clone();
    assert_eq!(held, before, "the code is unchanged");
    assert_eq!(
        held.author().map(ModelCode::text),
        Some(noted_text.as_str()),
        "the set's entry holds the sheet as it now reads"
    );

    // Edit model opens the sheet the selected model was built from, note included.
    new_grid_rule(&mut app);
    edit_model(&mut app);
    let opened = sheet(&app);
    assert_eq!(opened.text(), noted_text);
    assert_eq!(app.focus_request, Some(Tab::Editor));
}

#[test]
fn open_in_editor_opens_the_sheet_a_sweep_recorded_or_else_its_graph() {
    let Some(mut app) = headless_app() else {
        return;
    };
    let text = small_life();
    let document = ParsedSheet::parse(&text).expect("the sheet reads").into_document();

    // A sweep run in the app records the entry it started with.
    let entry = entry_of(&text);
    let plan = Arc::new(density_sweep(entry.id()).plan(&entry.schema()).expect("a valid spec"));
    app.results.begin_sweep(plan, &entry, None);
    assert!(app.results.store().is_some_and(ResultsStore::records_editable_code));
    new_grid_rule(&mut app);
    app.focus_request = None;
    open_in_editor(&mut app);
    assert_eq!(sheet(&app).document(), &document);
    assert_eq!(app.focus_request, Some(Tab::Editor));

    // A folder records the sheet in its manifest.
    let folder = ScratchFolder::new("sheet-sweep");
    sweep_into(&app, &entry, &folder.0);
    open_results(&mut app, &folder.0);
    // The sheet's id is its own, so a folder of a sheet the app has not applied finds no model of the set, and never
    // the compiled example of the same name.
    let store = app.results.store().expect("the folder's results");
    assert_eq!(store.model_id, "sheet_game_of_life");
    assert!(app.models.get(&store.model_id).is_none());
    assert!(!store.schema_matches && !store.code_matches);
    assert!(
        store
            .replay_refusal()
            .is_some_and(|reason| reason.contains("sheet_game_of_life")),
        "{:?}",
        store.replay_refusal()
    );
    new_grid_rule(&mut app);
    open_in_editor(&mut app);
    assert_eq!(sheet(&app).document(), &document, "the folder's sheet opens");

    // A folder swept from the recorded graph alone records no sheet, and its graph opens on the canvas.
    let set = ResultSet::open_dir(&folder.0, 0).expect("the folder reads");
    let code = set
        .manifest()
        .model
        .code
        .as_ref()
        .expect("the folder records the code")
        .to_code();
    let graph_entry = entry_of(code.text());
    assert_eq!(graph_entry.source().author(), None);
    let graph_folder = ScratchFolder::new("graph-sweep");
    sweep_into(&app, &graph_entry, &graph_folder.0);
    open_results(&mut app, &graph_folder.0);
    open_in_editor(&mut app);
    assert!(app.editor.model.graph().is_some(), "the graph opens on the canvas");
    let opened = ModelFile::parse(&app.editor.model.file_text()).expect("the opened graph reads");
    assert_eq!(opened.code(), code);

    // A compiled model records no code to open.
    let sir = app.models.get("sir").expect("SIR is registered").clone();
    let mut spec = SweepSpec::new("sir");
    spec.run.steps = 2;
    let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
    app.results.begin_sweep(plan, &sir, None);
    let store = app.results.store().expect("the sweep's results");
    assert!(!store.records_editable_code() && store.editable_model().is_none());
}
