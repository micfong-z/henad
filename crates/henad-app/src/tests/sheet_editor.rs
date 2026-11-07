//! Tests of the sheet editor in the Editor tab: a sheet opens by the `kind` in its header, previews paused at tick 0,
//! applies and saves its canonical text, and the New model dialog opens a sheet starter under a name.

use henad_core::diagnostic::DiagnosticSeverity;
use henad_graph::sheet::check::check;
use henad_graph::sheet::document::{
    LineKind, MadeOf, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetProperty, SheetValue, Slot, StartRule,
    StartsAs,
};
use henad_graph::sheet::sentence::{A_RANDOM_NUMBER, A_VALUE, BY_CHANCE, CANCEL, CLOSE_HELP, CREATE, FORBIDDEN_WORDS};
use henad_graph::sheet::{ParsedSheet, starters};
use web_time::Instant;

use crate::state::{AppState, PreviewRun};
use crate::tests::support::step_past;
use crate::ui::editor::preview::{DEBOUNCE, PREVIEW_AGENTS, PREVIEW_SIDE, PreviewState, poll_at};
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::card_pane::{CardLine, LineNote, NoteColor, card_lines};
use crate::ui::editor::sheet::new_model::{new_model_modal, starter_choices};
use crate::ui::editor::sheet::sheet_pane::starts_text;
use crate::ui::editor::{ApplyOutcome, EditorModel, apply, editor_ui, open_text, preview_running_text, save_name};
use crate::ui::menu_bar::file_menu;
use crate::ui::playback::{press_build, tick_counter};

/// Returns an app over the example models on a headless device that runs compute shaders, or `None` to skip on a
/// machine without a device.
fn app() -> Option<AppState> {
    AppState::headless(henad_models::example_models(), true)
}

/// Returns the text of the sheet starter whose file is named `file_name`.
fn starter_text(file_name: &str) -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .expect("a sheet starter")
        .text
}

/// Opens `text` in the Editor as Open model opens a picked file.
fn open(app: &mut AppState, text: &str) {
    open_text(&mut app.editor, text.to_owned(), Some("opened.toml".to_owned()));
}

/// Sheet in the Editor.
fn sheet(app: &AppState) -> &SheetEditor {
    app.editor.model.sheet().expect("a sheet")
}

/// Polls the preview once now and once after the debounce, as two frames that far apart would.
fn settle(app: &mut AppState) {
    let ctx = egui::Context::default();
    let now = Instant::now();
    poll_at(&ctx, app, now);
    poll_at(&ctx, app, now + DEBOUNCE);
}

/// Returns Game of Life with the old state of line 2 left blank. Line 2 is then skipped.
fn life_with_a_skipped_line() -> String {
    let text = starter_text("game_of_life.toml");
    let blanked = text.replacen(
        "new_state = \"alive\"\nstart_state = \"dead\"\n",
        "new_state = \"alive\"\n",
        1,
    );
    assert_ne!(blanked, text, "the old state of line 2 was found");
    blanked
}

#[test]
fn every_sheet_starter_opens_as_a_sheet_previews_at_tick_zero_plays_and_applies() {
    let Some(mut app) = app() else {
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
        settle(&mut app);
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
        assert!(
            app.sim_thread.is_some() && app.fault.is_none(),
            "{}: the preview failed to build",
            starter.file_name
        );
        assert!(!app.sim_running, "{}: the preview waits for Play", starter.file_name);
        wait_for_snapshot(&mut app, |tick| tick == 0);

        // Play, as Playback's button presses it.
        app.sim_thread.as_mut().expect("a run").play();
        app.sim_running = true;
        wait_for_snapshot(&mut app, |tick| tick > 0);

        apply(&mut app);
        assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied), "{}", starter.file_name);
        assert_eq!(app.preview(), None, "{}: Apply replaces the preview", starter.file_name);
        assert_eq!(app.loaded_id(), Some(document.sheet.id.as_str()));
        app.wait_for_build();
        assert!(
            app.sim_thread.is_some() && app.fault.is_none(),
            "{}: the applied sheet failed to build",
            starter.file_name
        );
        step_past(&mut app, 0);
        // The next starter's preview replaces nothing past tick 0.
        app.offload_simulation();
    }
}

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

#[test]
fn apply_installs_and_builds_the_sheet() {
    let Some(mut app) = app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle(&mut app);
    app.wait_for_build();
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    assert_eq!(app.preview(), None, "Apply replaces the preview");
    assert_eq!(app.selected_model.as_deref(), Some("game_of_life"));
    assert!(app.selection_is_loaded());
    let installed = app.models.get("game_of_life").expect("the sheet is installed");
    assert!(app.editor.is_applied(installed.source()));
    app.wait_for_build();
    assert!(
        app.sim_thread.is_some() && app.fault.is_none(),
        "the applied sheet failed to build"
    );
    step_past(&mut app, 0);
}

#[test]
fn save_writes_the_canonical_text_and_the_saved_file_reopens_to_the_same_document() {
    let Some(mut app) = app() else {
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
    let edited =
        starter_text("game_of_life.toml").replace("id = \"game_of_life\"\nname", "id    =   \"game_of_life\"\nname");
    open(&mut app, &edited);
    assert_eq!(app.editor.model.file_text(), starter_text("game_of_life.toml"));

    app.editor.file_name = None;
    assert_eq!(save_name(&app.editor), "game_of_life.toml");
}

#[test]
fn a_sheet_with_a_skipped_line_opens_and_previews() {
    let Some(mut app) = app() else {
        return;
    };
    open(&mut app, &life_with_a_skipped_line());
    let report = sheet(&app).report();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(report.is_skipped("l2"), "line 2 is skipped");
    assert!(!sheet(&app).has_errors());
    settle(&mut app);
    assert!(app.preview().is_some(), "a skipped line leaves the preview running");
    app.wait_for_build();
    assert!(
        app.sim_thread.is_some() && app.fault.is_none(),
        "the preview failed to build"
    );
}

#[test]
fn a_sheet_with_a_checker_error_opens_in_the_sheet_editor() {
    let Some(mut app) = app() else {
        return;
    };
    // A parameter default outside its range is an error of the checker. The reader reads it.
    let text = starter_text("game_of_life.toml").replace("default = 0.3\n", "default = 7\n");
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
    settle(&mut app);
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
    let Some(mut app) = app() else {
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

/// Returns the value of the stat `label` in the loaded run's snapshot at tick 0, or panics after ten seconds.
fn tick_zero_stat(app: &mut AppState, label: &str) -> f64 {
    app.wait_for_build();
    let snapshot = wait_for_snapshot(app, |tick| tick == 0);
    snapshot
        .stats
        .iter()
        .find(|entry| entry.label() == label)
        .map(|entry| entry.value().scalar())
        .expect("the stat is published")
}

/// Takes the loaded run's snapshots until one whose tick passes `accept`, keeps it as the app's snapshot and returns
/// it, or panics after ten seconds.
fn wait_for_snapshot(app: &mut AppState, accept: impl Fn(u64) -> bool) -> &henad_compute::snapshot::Snapshot {
    let thread = app.sim_thread.as_mut().expect("a model is loaded");
    for _ in 0..1000 {
        if let Some(snapshot) = thread.take_snapshot()
            && accept(snapshot.tick)
        {
            return app.snapshot.insert(snapshot);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no snapshot of the loaded model passed the test");
}

#[test]
fn a_sheet_preview_waits_at_tick_zero_until_play() {
    let Some(mut app) = app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle(&mut app);
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
    app.sim_thread.as_mut().expect("a run").play();
    app.sim_running = true;
    let tick = wait_for_snapshot(&mut app, |tick| tick > 0).tick;
    assert!(tick > 0);
    assert!(tick_counter(&app).is_some_and(|counter| counter.ends_with("(a tick is one step of time)")));

    // A graph's preview still plays once it is built.
    crate::ui::editor::new_graph_model(&mut app, "sir.toml");
    settle(&mut app);
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
    let Some(mut app) = app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle(&mut app);
    let first = tick_zero_stat(&mut app, "Alive");
    app.sim_thread.as_mut().expect("a run").play();
    app.sim_running = true;
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

#[test]
fn build_builds_another_selected_model_instead_of_the_sheet_preview() {
    let Some(mut app) = app() else {
        return;
    };
    open(&mut app, starter_text("game_of_life.toml"));
    settle(&mut app);
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
    assert_eq!(starts_text(cover(&mut document)), BY_CHANCE);

    let mut all_trees = StartsAs::new(StartRule::Value);
    all_trees.value = Slot::Filled(SheetValue::Chip(SheetChip::State("tree".to_owned())));
    cover(&mut document).starts_as = Some(all_trees);
    assert_eq!(starts_text(cover(&mut document)), "All Tree");

    let mut random = StartsAs::new(StartRule::Value);
    random.value = Slot::Filled(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::RandomNumber))));
    cover(&mut document).starts_as = Some(random);
    assert_eq!(starts_text(cover(&mut document)), A_RANDOM_NUMBER);

    let mut number = StartsAs::new(StartRule::Value);
    number.value = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(2.0)));
    cover(&mut document).starts_as = Some(number);
    assert_eq!(starts_text(cover(&mut document)), A_VALUE);
}

/// An egui context that draws the File menu, the Editor tab and the New model dialog over an app, driven by synthetic
/// input.
struct Harness {
    context: egui::Context,
    app: AppState,
    time: f64,
}

/// Returns the top of `node` on the screen.
fn top(node: &Node) -> f32 {
    node.bounds.map_or(0.0, |rect| rect.top())
}

/// One accessible node of a frame.
#[derive(Debug, Clone)]
struct Node {
    label: String,
    value: String,
    description: String,
    role: egui::accesskit::Role,
    bounds: Option<egui::Rect>,
}

impl Harness {
    /// Returns a harness over the example models, or `None` to skip on a machine without a device.
    fn new() -> Option<Self> {
        let harness = Self {
            context: egui::Context::default(),
            app: app()?,
            time: 0.0,
        };
        harness.context.enable_accesskit();
        Some(harness)
    }

    /// Runs one frame with `events`, and returns its accessible nodes.
    fn frame(&mut self, events: Vec<egui::Event>) -> Vec<Node> {
        self.time += 0.016;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            time: Some(self.time),
            events,
            ..egui::RawInput::default()
        };
        let app = &mut self.app;
        let mut output = self.context.run_ui(input, |ui| {
            egui::MenuBar::new().ui(ui, |ui| file_menu(ui, app));
            editor_ui(ui, app);
            new_model_modal(ui.ctx(), app);
        });
        output.textures_delta.clear();
        let update = output.platform_output.accesskit_update.expect("an AccessKit update");
        update
            .nodes
            .iter()
            .map(|(_, node)| Node {
                // A submenu's button ends with its arrow.
                label: node.label().unwrap_or_default().trim_end_matches(" ⏵").to_owned(),
                value: node.value().unwrap_or_default().to_owned(),
                description: node.description().unwrap_or_default().to_owned(),
                role: node.role(),
                bounds: node.bounds().map(|rect| {
                    egui::Rect::from_min_max(
                        egui::pos2(rect.x0 as f32, rect.y0 as f32),
                        egui::pos2(rect.x1 as f32, rect.y1 as f32),
                    )
                }),
            })
            .collect()
    }

    /// Runs a few frames with no input, and returns the accessible nodes of the last.
    fn settle(&mut self) -> Vec<Node> {
        for _ in 0..3 {
            self.frame(Vec::new());
        }
        self.frame(Vec::new())
    }

    /// Clicks the Name field of the New model dialog.
    fn click_name_field(&mut self) {
        let nodes = self.settle();
        let field = nodes
            .iter()
            .find(|node| node.role == egui::accesskit::Role::TextInput)
            .and_then(|node| node.bounds)
            .expect("the Name field")
            .center();
        self.click_at(field);
    }

    /// Clicks the primary button at `position`.
    fn click_at(&mut self, position: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![egui::Event::PointerMoved(position)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
    }

    /// Returns the centre of the one node whose label ends with `label`.
    fn find(&mut self, label: &str) -> egui::Pos2 {
        let nodes = self.settle();
        let found: Vec<&Node> = nodes
            .iter()
            .filter(|node| node.label.ends_with(label) && node.bounds.is_some())
            .collect();
        assert_eq!(found.len(), 1, "one widget reads {label}: {found:?}");
        found[0].bounds.expect("bounds").center()
    }

    /// Clicks the one widget whose label ends with `label`.
    fn click(&mut self, label: &str) {
        let position = self.find(label);
        self.click_at(position);
    }

    /// Presses `key` with `modifiers`.
    fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![event(true), event(false)]);
    }

    /// Returns the label and the value of every node of a settled frame. A label's text is its node's value.
    fn labels(&mut self) -> Vec<String> {
        self.settle()
            .into_iter()
            .flat_map(|node| [node.label, node.value])
            .filter(|text| !text.is_empty())
            .collect()
    }
}

#[test]
fn the_new_model_dialog_offers_every_grid_starter_and_no_agent_starter() {
    let offered: Vec<&str> = starter_choices().iter().map(|choice| choice.file_name).collect();
    let mut agent_starters = 0;
    for starter in starters() {
        let document = ParsedSheet::parse(starter.text)
            .expect("the starter reads")
            .into_document();
        let cells = document.world.made_of == MadeOf::Cells;
        agent_starters += usize::from(!cells);
        assert_eq!(offered.contains(&starter.file_name), cells, "{}", starter.file_name);
    }
    assert!(agent_starters > 0, "an agent starter is held out");
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
    assert_eq!(names, ["Game of Life", "Majority vote", "Forest fire"]);
    let mut radios: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.role == egui::accesskit::Role::RadioButton)
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
        Some("Game of Life"),
        "Name follows the starter"
    );

    harness.click("Majority vote");
    assert_eq!(name(&harness.settle()).as_deref(), Some("Majority vote"));

    // A typed name stays when the starter changes.
    harness.click_name_field();
    harness.press(egui::Key::A, egui::Modifiers::COMMAND);
    harness.frame(vec![egui::Event::Text("My life".to_owned())]);
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
    assert_eq!(document.sheet.id, "game_of_life");
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

/// Returns the lines of card `card` of the sheet `text` as the Card pane shows them.
fn lines_of(text: &str, card: usize) -> Vec<CardLine> {
    let document = ParsedSheet::parse(text).expect("the sheet reads").into_document();
    let sentences = henad_graph::sheet::sentence::Sentences::new(&document);
    card_lines(&document, &sentences, &check(&document), card)
}

/// Returns Game of Life with line 2 moved above line 1. Line 1 then uses Alive neighbours from below it.
fn life_with_a_result_below() -> String {
    let text = starter_text("game_of_life.toml");
    let moved = text.replacen(
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l2\", \"l1\", \"l3\"]",
        1,
    );
    assert_ne!(moved, text, "the lines of the card were found");
    moved
}

/// Returns Game of Life with an old state on line 2 that Status lacks, an error of the checker at that line.
fn life_with_an_error_at_line_two() -> String {
    let text = starter_text("game_of_life.toml");
    let broken = text.replacen("start_state = \"dead\"\n", "start_state = \"zombie\"\n", 1);
    assert_ne!(broken, text, "the old state of line 2 was found");
    broken
}

/// Returns the words of `text`, split at every character that is not a letter or a digit.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
}

/// Returns whether `text` belongs to a control that opens or creates a node graph, which the words rule exempts.
fn opens_a_node_graph(text: &str) -> bool {
    text.contains(crate::ui::editor::NEW_NODE_GRAPH)
}

/// Returns every forbidden word in `texts`, with the text that holds it.
fn forbidden_words(texts: &[String]) -> Vec<(String, String)> {
    texts
        .iter()
        .filter(|text| !opens_a_node_graph(text))
        .flat_map(|text| {
            words(text)
                .filter(|word| {
                    FORBIDDEN_WORDS
                        .iter()
                        .any(|forbidden| forbidden.eq_ignore_ascii_case(word))
                })
                .map(|word| (word.to_owned(), text.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

impl Harness {
    /// Returns the label, the value and the description of every node of a settled frame.
    fn accessible_texts(&mut self) -> Vec<String> {
        self.settle()
            .into_iter()
            .flat_map(|node| [node.label, node.value, node.description])
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Opens the sheet `text` in the Editor and draws it.
    fn open_sheet(&mut self, text: &str) {
        open(&mut self.app, text);
        self.settle();
    }
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
    let text = starter_text("game_of_life.toml");
    let added = text.replacen(
        "# Lines of Clear",
        "[card.line.l9]\nkind = \"become\"\nproperty = \"status\"\nnew_state = \"dead\"\nstart_state = \"alive\"\n\n\
         # Lines of Clear",
        1,
    );
    assert_ne!(added, text, "the Clear card was found");
    added
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
    let text = starter_text("game_of_life.toml");
    let two_tick_cards = text.replacen("runs = \"once\"", "runs = \"each_tick\"", 1);
    assert_ne!(two_tick_cards, text, "the Clear card was found");
    harness.open_sheet(&two_tick_cards);
    let nodes = harness.settle();
    let buttons: Vec<egui::Pos2> = nodes
        .iter()
        .filter(|node| node.role == egui::accesskit::Role::Button && node.label == "Help: lines")
        .filter_map(|node| node.bounds.map(|bounds| bounds.center()))
        .collect();
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
    let mut texts = Vec::new();
    for starter in starters() {
        harness.open_sheet(starter.text);
        texts.extend(harness.accessible_texts());
        for help in HELP_BUTTONS {
            harness.click(help);
            texts.extend(harness.accessible_texts());
            harness.press(egui::Key::Escape, egui::Modifiers::NONE);
        }
    }
    for text in [life_with_a_skipped_line(), life_with_a_result_below()] {
        harness.open_sheet(&text);
        texts.extend(harness.accessible_texts());
    }
    harness.click("New model");
    texts.extend(harness.accessible_texts());
    assert!(
        texts.iter().any(|text| text == "Start from"),
        "the New model dialog was drawn"
    );
    let found = forbidden_words(&texts);
    assert!(found.is_empty(), "forbidden words: {found:?}");
}

#[test]
fn every_accessible_name_holds_its_visible_words() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    for text in starters()
        .iter()
        .map(|starter| starter.text.to_owned())
        .chain([life_with_a_skipped_line(), life_with_a_result_below()])
    {
        harness.open_sheet(&text);
        let nodes = harness.settle();
        let document = ParsedSheet::parse(&text).expect("the sheet reads").into_document();
        let sentences = henad_graph::sheet::sentence::Sentences::new(&document);
        let report = check(&document);
        for card in 0..document.cards.len() {
            for line in card_lines(&document, &sentences, &report, card) {
                for control in line.controls() {
                    assert!(
                        nodes.iter().any(|node| node.label == control.name),
                        "{} has no control named {}",
                        line.label(),
                        control.name
                    );
                    if control.select {
                        continue;
                    }
                    let visible = control.visible.trim_end_matches("...").trim().to_lowercase();
                    assert!(
                        control.name.to_lowercase().contains(&visible),
                        "{:?} does not hold its visible words {:?}",
                        control.name,
                        control.visible
                    );
                }
            }
        }
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

/// Asserts that a button, a radio button or a check box of `nodes` shows `visible` and that its name holds it.
fn assert_control_holds(nodes: &[Node], visible: &str) {
    use egui::accesskit::Role;

    let named = nodes.iter().any(|node| {
        matches!(node.role, Role::Button | Role::RadioButton | Role::CheckBox)
            && node.label.to_lowercase().contains(&visible.to_lowercase())
    });
    assert!(named, "no control's name holds its visible words {visible:?}");
}

#[test]
fn the_world_tabs_show_no_forbidden_word_from_a_sheet() {
    for starter in starters() {
        let entry = henad_explore::model_file::ModelFile::parse(starter.text)
            .and_then(|file| file.entry())
            .expect("the starter has an entry");
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
