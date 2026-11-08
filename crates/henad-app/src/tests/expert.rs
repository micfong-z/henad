//! Tests of the expert mode: the Diagram of a card, the conversion of a sheet to a node graph copy, the copy's banner
//! and the original's link line, display names, port names by origin and frame titles that fit.

use std::path::PathBuf;

use egui::accesskit::Role;
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::sheet::ConvertRefusal;
use henad_graph::sheet::sentence::{
    CONVERT, CONVERT_HEADING, CONVERT_NEEDS_FINISHED_LINES, CONVERT_NEEDS_FIXES, DIAGRAM, EDIT_AS_NODE_GRAPH,
    MODEL_ACTIONS, Sentences, convert_text, converted_banner, converted_link, diagram_name, open_original,
};
use henad_graph::sheet::starters;
use henad_graph::value::Literal;

use crate::tests::support::{Harness, forbidden_words};
use crate::ui::editor::expert::{CopyState, convert_refusal, copy_banner, find_original, open_original_of, today};
use crate::ui::editor::graph::arrange::NODE_SIZE;
use crate::ui::editor::sheet::diagram::diagram_boxes;
use crate::ui::editor::{EditorModel, open_text};

/// Title of the frame of line 2 in the copy of Game of Life.
const LINE_2: &str = "Line 2: Become Alive from Dead, when Alive neighbours is 3";

/// Returns Game of Life with the old state of line 2 left blank. Line 2 is then skipped.
fn life_with_a_skipped_line() -> String {
    starter_text("game_of_life.toml").replacen(
        "new_state = \"alive\"\nstart_state = \"dead\"\n",
        "new_state = \"alive\"\n",
        1,
    )
}

/// Returns Game of Life with line 2 changing to a state Status does not hold, which the check reports as an error.
fn life_with_an_error() -> String {
    starter_text("game_of_life.toml").replacen("new_state = \"alive\"", "new_state = \"zombie\"", 1)
}

/// Opens Game of Life in the Editor of `harness` and converts it through the model menu and the dialog.
fn convert_life(harness: &mut Harness) {
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.click(MODEL_ACTIONS);
    harness.click(CONVERT_HEADING);
    harness.click(CONVERT);
    harness.settle();
}

/// Returns the text of the sheet starter whose file is named `file_name`.
fn starter_text(file_name: &str) -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .expect("a sheet starter")
        .text
}

#[test]
fn the_graph_view_draws_one_box_per_line_and_edits_nothing() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.click(DIAGRAM);
    let nodes = harness.settle();
    let sheet = harness.app.editor.model.sheet().expect("a sheet");
    assert!(sheet.diagram, "the toggle turns the Diagram on");
    let document = sheet.document();
    let boxes = diagram_boxes(document, &Sentences::new(document), sheet.report(), 0);
    assert_eq!(boxes.len(), 3, "one box per line of the card that runs each tick");
    assert_eq!(boxes[1].reads, [("l1".to_owned(), "Alive neighbours".to_owned())]);
    assert_eq!(boxes[0].result.as_deref(), Some("Alive neighbours"));
    let image = nodes
        .iter()
        .find(|node| node.label == diagram_name(3))
        .expect("the Diagram of the card's 3 lines");
    assert_eq!(image.role, Role::Image);
    let card = nodes
        .iter()
        .find(|group| group.label == "Card" && group.role == Role::Group)
        .and_then(|group| group.bounds)
        .expect("the Card pane");
    let controls: Vec<&str> = nodes
        .iter()
        .filter(|node| {
            matches!(
                node.role,
                Role::TextInput | Role::ComboBox | Role::CheckBox | Role::Slider | Role::SpinButton
            ) || node.label.starts_with("Line ")
                || node.label.starts_with("Clear line")
                || node.label.starts_with("+ ")
        })
        .filter(|node| node.bounds.is_some_and(|bounds| card.intersects(bounds)))
        .map(|node| node.label.as_str())
        .collect();
    assert!(controls.is_empty(), "the Diagram edits nothing: {controls:?}");
    let texts = harness.accessible_texts();
    assert_eq!(forbidden_words(&texts), Vec::new());
    harness.click(DIAGRAM);
    let lines = harness.settle();
    assert!(
        lines.iter().any(|node| node.label.starts_with("Line 2: ")),
        "the toggle returns to the lines"
    );
}

#[test]
fn the_diagram_frames_the_lines_inside_each_when() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("ants.toml"));
    let sheet = harness.app.editor.model.sheet().expect("a sheet");
    let document = sheet.document();
    let sentences = Sentences::new(document);
    let boxes = diagram_boxes(document, &sentences, sheet.report(), 0);
    let whens: Vec<&str> = boxes
        .iter()
        .filter(|line| line.when)
        .map(|line| line.id.as_str())
        .collect();
    assert!(!whens.is_empty(), "the ants' card holds a When");
    for when in whens {
        assert!(
            boxes.iter().any(|line| line.parent.as_deref() == Some(when)),
            "a line sits inside {when}"
        );
    }
    harness.click(DIAGRAM);
    let nodes = harness.settle();
    assert!(
        nodes
            .iter()
            .any(|node| node.role == Role::Image && node.label.starts_with("Diagram of the card's")),
        "the ants' card draws as an image"
    );
}

#[test]
fn the_conversion_modal_names_no_preset() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.click(MODEL_ACTIONS);
    harness.click(CONVERT_HEADING);
    let texts = harness.accessible_texts();
    for paragraph in convert_text("Game of Life") {
        for line in paragraph.lines() {
            assert!(
                texts.iter().any(|text| text == line),
                "the dialog reads {line}: {texts:?}"
            );
        }
    }
    assert!(
        !texts.iter().any(|text| text.to_lowercase().contains("preset")),
        "the app has no presets"
    );
    assert!(texts.iter().any(|text| text == CONVERT_HEADING));
    // The dialog is exempt by place: its texts are left out, and every other text keeps to the words rule.
    let outside = harness.accessible_texts_outside(CONVERT_HEADING);
    let [first, second] = convert_text("Game of Life");
    for line in first.lines().chain(second.lines()) {
        assert!(
            !outside.iter().any(|text| text == line),
            "{line} lies inside the dialog"
        );
    }
    assert_eq!(forbidden_words(&outside), Vec::new());
    harness.click("Cancel");
    assert!(harness.app.editor.convert.is_none());
    assert!(harness.app.editor.model.sheet().is_some(), "Cancel keeps the sheet");
}

/// Returns whether the one node of `harness` named `label` is disabled, after checking that a pointer resting on it
/// shows `tooltip`.
fn disabled_with_tooltip(harness: &mut Harness, label: &str, tooltip: &str) -> bool {
    let nodes = harness.settle();
    let found: Vec<_> = nodes.iter().filter(|node| node.label == label).collect();
    assert_eq!(found.len(), 1, "one control reads {label}: {found:?}");
    let disabled = found[0].disabled;
    let centre = found[0].bounds.expect("bounds").center();
    harness.frame(vec![egui::Event::PointerMoved(centre)]);
    // A tooltip shows once the pointer has rested past egui's delay.
    for _ in 0..60 {
        harness.frame(Vec::new());
    }
    let texts = harness.accessible_texts();
    assert!(
        texts.iter().any(|text| text == tooltip),
        "{label} shows the tooltip {tooltip}: {texts:?}"
    );
    disabled
}

#[test]
fn convert_is_disabled_with_its_tooltip() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    for (text, refusal, tooltip) in [
        (life_with_an_error(), ConvertRefusal::Errors, CONVERT_NEEDS_FIXES),
        (
            life_with_a_skipped_line(),
            ConvertRefusal::SkippedLines,
            CONVERT_NEEDS_FINISHED_LINES,
        ),
    ] {
        harness.open_sheet(&text);
        let sheet = harness.app.editor.model.sheet().expect("a sheet");
        assert_eq!(convert_refusal(sheet), Some(refusal));
        assert_eq!(refusal.tooltip(), tooltip);
        harness.click(MODEL_ACTIONS);
        assert!(
            disabled_with_tooltip(&mut harness, CONVERT_HEADING, tooltip),
            "Convert to node graph is disabled"
        );
        harness.click(CONVERT_HEADING);
        harness.settle();
        assert!(
            harness.app.editor.convert.is_none(),
            "a disabled Convert opens no dialog"
        );
        harness.press(egui::Key::Escape, egui::Modifiers::NONE);
        harness.click(DIAGRAM);
        assert!(
            disabled_with_tooltip(&mut harness, EDIT_AS_NODE_GRAPH, tooltip),
            "Edit as node graph is disabled"
        );
        harness.click(EDIT_AS_NODE_GRAPH);
        assert!(
            harness.app.editor.convert.is_none(),
            "a disabled Edit as node graph opens no dialog"
        );
        assert!(harness.app.editor.model.sheet().is_some());
        harness.click(DIAGRAM);
    }

    // The same controls are enabled on a sheet that converts.
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.click(MODEL_ACTIONS);
    let nodes = harness.settle();
    let convert = nodes
        .iter()
        .find(|node| node.label == CONVERT_HEADING)
        .expect("Convert to node graph");
    assert!(!convert.disabled, "Convert to node graph is enabled");
}

#[test]
fn the_words_rule_exempts_whole_controls_and_still_reports_a_bare_node() {
    let found = |text: &str| forbidden_words(&[text.to_owned()]);
    for exempt in [
        CONVERT_HEADING.to_owned(),
        EDIT_AS_NODE_GRAPH.to_owned(),
        format!("\u{f0420}  {}", crate::ui::editor::NEW_NODE_GRAPH),
        converted_link("Game of Life (expert)"),
    ] {
        assert_eq!(found(&exempt), Vec::new(), "{exempt} is exempt");
    }
    for reported in ["node", "node graph", "Henad runs every node", "Edit as node graph now"] {
        let words = found(reported);
        assert!(
            words.iter().any(|(word, _)| word == "node"),
            "{reported} is reported: {words:?}"
        );
    }
}

#[test]
fn converting_game_of_life_opens_the_copy_with_frames_titled_by_line_and_the_banner() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    let graph = harness.app.editor.model.graph().expect("the copy opens in the Editor");
    assert_eq!(graph.document().name, "Game of Life (expert)");
    assert_eq!(graph.document().id, "game_of_life_expert");
    assert_eq!(
        harness.app.editor.file_name.as_deref(),
        Some("game_of_life_expert.toml")
    );
    let frames = graph.editor_region().frames();
    assert!(frames.iter().any(|frame| frame.title == LINE_2), "{frames:?}");
    let origin = graph.editor_region().model_origin().expect("an origin record");
    assert_eq!(origin.file, "opened.toml");
    assert_eq!(origin.converted, today());
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::Unedited);
    assert_eq!(banner.text(), converted_banner("Game of Life", &today()));
    let texts = harness.accessible_texts();
    assert!(texts.contains(&banner.text()), "the banner shows: {texts:?}");
    assert!(texts.contains(&open_original("Game of Life")));

    // Every frame of a canvas keeps its members together, apart from every other frame. A member of the open canvas
    // takes the rectangle it is drawn in, and one of another canvas the size the layout assumes.
    let graph = harness.app.editor.model.graph().expect("the copy");
    let editor = graph.editor_region();
    let open = graph.canvas(graph.scope()).expect("the open canvas");
    let open_scope = graph.scope().canvas();
    let frames = editor.frames();
    assert!(
        frames.iter().filter(|frame| frame.scope == open_scope).count() > 1,
        "the open canvas holds frames to compare"
    );
    for (index, frame) in frames.iter().enumerate() {
        let rect = |members: &[NodeId]| {
            members
                .iter()
                .filter_map(|member| {
                    let drawn = (frame.scope == open_scope)
                        .then(|| open.current_rect(*member))
                        .flatten();
                    drawn.or_else(|| {
                        let pos = editor.node_pos(*member)?;
                        let [width, height] = NODE_SIZE;
                        Some(egui::Rect::from_min_size(
                            egui::pos2(pos[0] as f32, pos[1] as f32),
                            egui::vec2(width as f32, height as f32),
                        ))
                    })
                })
                .reduce(egui::Rect::union)
        };
        let own = rect(&frame.members).expect("placed members");
        for other in frames.iter().skip(index + 1).filter(|other| other.scope == frame.scope) {
            let theirs = rect(&other.members).expect("placed members");
            assert!(
                !own.intersects(theirs),
                "{} and {} share rows and columns",
                frame.title,
                other.title
            );
        }
    }
}

#[test]
fn editing_a_node_of_the_copy_changes_the_banner_and_a_new_node_joins_no_frame() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    let EditorModel::Graph(graph) = &mut harness.app.editor.model else {
        panic!("the copy opens in the Editor");
    };
    let frames_before = graph.editor_region().frames();
    let n2001 = NodeId::new(2001).expect("an id");
    assert!(graph.edit(
        vec![EditCommand::SetLiteral {
            scope: Scope::pass("tick_cell_tick"),
            node: n2001,
            port: "b".to_owned(),
            literal: Literal::U32(6),
        }],
        None,
    ));
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::Edited("1 node changed".to_owned()));
    assert!(
        banner.text().ends_with("Edited since: 1 node changed."),
        "{}",
        banner.text()
    );

    let EditorModel::Graph(graph) = &mut harness.app.editor.model else {
        panic!("the copy stays in the Editor");
    };
    assert!(graph.edit(
        vec![EditCommand::AddNode {
            scope: Scope::pass("tick_cell_tick"),
            op: "cmp.eq".to_owned(),
            region: None,
            pos: Some([0.0, 0.0]),
        }],
        None,
    ));
    assert_eq!(
        graph.editor_region().frames(),
        frames_before,
        "a node added after conversion joins no frame"
    );
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::Edited("1 node added, 1 changed".to_owned()));
}

#[test]
fn open_original_returns_to_the_sheet_and_its_line_opens_the_copy() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    harness.click(&open_original("Game of Life"));
    let sheet = harness.app.editor.model.sheet().expect("the sheet is back");
    assert_eq!(sheet.document().sheet.name, "Game of Life");
    assert_eq!(harness.app.editor.file_name.as_deref(), Some("opened.toml"));
    let link = converted_link("Game of Life (expert)");
    let texts = harness.accessible_texts();
    assert!(texts.contains(&link), "{texts:?}");
    assert_eq!(forbidden_words(&texts), Vec::new());
    harness.click(&link);
    let graph = harness.app.editor.model.graph().expect("the copy is back");
    assert_eq!(graph.document().name, "Game of Life (expert)");

    // An edit of the sheet since the conversion changes the banner.
    harness.click(&open_original("Game of Life"));
    let EditorModel::Sheet(sheet) = &mut harness.app.editor.model else {
        panic!("the sheet is back");
    };
    sheet
        .edit(false, true, |builder| {
            builder
                .apply(henad_graph::sheet::edit::SheetEdit::SetSheetName {
                    name: "Life".to_owned(),
                })
                .map(drop)
        })
        .expect("a new name");
    harness.click(&converted_link("Game of Life (expert)"));
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::OriginalChanged);
}

/// A folder under the system's temporary folder, removed when dropped.
struct ScratchFolder(PathBuf);

impl ScratchFolder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("henad-app-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("a scratch folder");
        Self(path)
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[test]
fn a_sheet_with_a_file_has_its_copy_written_beside_it_and_found_again() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let folder = ScratchFolder::new("convert-beside");
    let sheet_path = folder.0.join("life.toml");
    let text = starter_text("game_of_life.toml");
    std::fs::write(&sheet_path, text).expect("the sheet is written");
    // A file of the first copy's name is taken, and the copy takes the next.
    std::fs::write(folder.0.join("game_of_life_expert.toml"), "taken").expect("a taken file");
    open_text(&mut harness.app.editor, text.to_owned(), Some("life.toml".to_owned()));
    harness.app.editor.file_path = Some(sheet_path);
    harness.settle();
    harness.click(MODEL_ACTIONS);
    harness.click(CONVERT_HEADING);
    harness.click(CONVERT);
    let copy_path = folder.0.join("game_of_life_expert_2.toml");
    assert_eq!(harness.app.editor.file_path.as_ref(), Some(&copy_path));
    assert_eq!(
        harness.app.editor.file_status.as_deref(),
        Some("Saved game_of_life_expert_2.toml")
    );
    let copy_text = std::fs::read_to_string(&copy_path).expect("the copy is written");

    // The copy opened from its file finds the sheet beside it.
    open_text(
        &mut harness.app.editor,
        copy_text,
        Some("game_of_life_expert_2.toml".to_owned()),
    );
    harness.app.editor.file_path = Some(copy_path.clone());
    find_original(&mut harness.app.editor);
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::Unedited);
    open_original_of(&mut harness.app.editor);
    let sheet = harness
        .app
        .editor
        .model
        .sheet()
        .expect("the sheet opens from beside the copy");
    assert_eq!(sheet.document().sheet.id, "game_of_life");
    assert_eq!(harness.app.editor.file_name.as_deref(), Some("life.toml"));

    // Without the sheet beside it, the copy says so and offers nothing to open.
    std::fs::remove_file(folder.0.join("life.toml")).expect("the sheet is removed");
    let copy_text = std::fs::read_to_string(&copy_path).expect("the copy reads");
    open_text(&mut harness.app.editor, copy_text, None);
    harness.app.editor.file_path = Some(copy_path);
    find_original(&mut harness.app.editor);
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::NotFound);
    assert!(!banner.opens_original());
}

#[test]
fn every_catalogue_kind_has_a_display_name() {
    use std::collections::BTreeSet;

    use crate::ui::editor::graph::display::{DISPLAY_NAMES, display_name};

    let kinds: BTreeSet<&str> = henad_graph::catalogue::definitions().map(|def| def.id).collect();
    for kind in &kinds {
        assert!(display_name(kind).is_some(), "{kind} has a display name");
    }
    let listed: BTreeSet<&str> = DISPLAY_NAMES.iter().map(|entry| entry.kind).collect();
    assert_eq!(listed, kinds, "the table lists the catalogue's kinds and no other");
    let names: BTreeSet<&str> = DISPLAY_NAMES.iter().map(|entry| entry.name).collect();
    assert_eq!(names.len(), DISPLAY_NAMES.len(), "no two kinds share a display name");
}

#[test]
fn quick_add_finds_a_node_by_its_display_name_and_by_its_kind() {
    use crate::ui::editor::graph::quick_add::offered_kinds;

    let text = henad_graph::starters::starter("game_of_life.toml")
        .expect("a starter")
        .text;
    let file = henad_graph::GraphFile::parse(text).expect("the starter reads");
    let pass = file
        .document()
        .passes
        .iter()
        .find(|pass| pass.stage == henad_graph::document::PassStage::Tick)
        .expect("a tick pass");
    let ids = |query: &str| -> Vec<&'static str> {
        offered_kinds(file.document(), Some(pass), None, query, false)
            .into_iter()
            .map(|def| def.id)
            .collect()
    };
    assert!(ids("count around me").contains(&"grid.count"));
    assert!(ids("grid.count").contains(&"grid.count"));
    assert!(ids("is at most").contains(&"cmp.le"));
    assert!(ids("decide").contains(&"cmp.eq"), "the category matches too");
}

#[test]
fn the_copy_names_nodes_by_display_name_and_become_ports_by_line() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    let graph = harness.app.editor.model.graph().expect("the copy");
    let canvas = graph.canvas(graph.scope()).expect("the open canvas");
    let views: Vec<&crate::ui::editor::graph::canvas::NodeView> = canvas
        .nodes
        .values()
        .filter_map(|pin| canvas.snarl.get_node(*pin))
        .collect();
    let count = views
        .iter()
        .find(|view| view.op == "grid.count")
        .expect("the count of line 1");
    assert_eq!(count.title, "Alive neighbours");
    assert_eq!(count.subtitle, "Count around me · grid.count");
    let compare = views
        .iter()
        .find(|view| view.op == "cmp.eq")
        .expect("line 2's comparison");
    assert_eq!((compare.title.as_str(), compare.subtitle.as_str()), ("Is", "cmp.eq"));
    let become_node = views
        .iter()
        .find(|view| view.op == "cat.transition")
        .expect("the Become of lines 2 and 3");
    let inputs: Vec<&str> = become_node.inputs.iter().map(|input| input.label.as_str()).collect();
    assert!(
        inputs.contains(&"Line 2 when") && inputs.contains(&"Line 3 when"),
        "{inputs:?}"
    );
    let outputs: Vec<&str> = become_node.outputs.iter().map(|output| output.label.as_str()).collect();
    assert!(outputs.contains(&"Line 2 fired"), "{outputs:?}");
    assert!(
        become_node.inputs.iter().any(|input| input.port == "rule.l2.when"),
        "the port keeps its name"
    );
}

#[test]
fn a_frame_is_at_least_as_wide_as_its_title() {
    use crate::ui::editor::graph::canvas::fit_title;

    let narrow = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
    let fitted = fit_title(narrow, 300.0);
    assert!(fitted.width() >= 300.0);
    assert_eq!((fitted.min, fitted.height()), (narrow.min, narrow.height()));
    let wide = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(500.0, 50.0));
    assert_eq!(fit_title(wide, 300.0), wide);
}
