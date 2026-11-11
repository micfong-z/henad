//! Tests of the expert mode: the Diagram of a card, the conversion of a sheet to a node graph copy, the copy's banner
//! and the original's link line, display names, port names by origin and frame titles that fit.

use egui::accesskit::Role;
use henad_graph::edit::{EditCommand, Scope};
use henad_graph::ids::NodeId;
use henad_graph::sheet::ConvertRefusal;
use henad_graph::sheet::sentence::{
    CONVERT, CONVERT_HEADING, CONVERT_NEEDS_FINISHED_LINES, CONVERT_NEEDS_FIXES, DIAGRAM, EDIT_AS_NODE_GRAPH,
    LINES_VIEW, MODEL_ACTIONS, Sentences, convert_text, converted_banner, converted_link, diagram_name, open_original,
};
use henad_graph::value::Literal;

use crate::tests::panes::{apply_sheet_edit, sheet, sheet_mut, shows_notice};
use crate::tests::support::{
    Harness, ScratchFolder, edited, forbidden_words, life_with_a_skipped_line, n, starter_text, wait_within,
};
use crate::ui::editor::expert::{CopyState, convert_refusal, copy_banner, find_original, open_original_of, today};
use crate::ui::editor::graph::arrange::NODE_SIZE;
use crate::ui::editor::sheet::diagram::diagram_boxes;
use crate::ui::editor::{FileStatus, open_text};

/// Title of the frame of line 2 in the copy of Game of Life.
const LINE_2: &str = "Line 2: Become Alive from Dead, when Alive neighbours is 3";

/// Returns Game of Life with line 2 changing to a state Status does not hold, which the check reports as an error.
fn life_with_an_error() -> String {
    edited(
        starter_text("game_of_life.toml"),
        "new_state = \"alive\"",
        "new_state = \"zombie\"",
    )
}

/// Opens Game of Life in the Editor of `harness` and converts it through the model menu and the dialog.
fn convert_life(harness: &mut Harness) {
    convert_starter(harness, "game_of_life.toml");
}

/// Opens the sheet starter `file_name` in the Editor of `harness` and converts it through the model menu and the
/// dialog.
fn convert_starter(harness: &mut Harness, file_name: &str) {
    harness.open_sheet(starter_text(file_name));
    convert_open_sheet(harness);
}

/// Converts the sheet open in the Editor of `harness` through the model menu and the dialog.
fn convert_open_sheet(harness: &mut Harness) {
    harness.click(MODEL_ACTIONS);
    harness.click(CONVERT_HEADING);
    harness.click(CONVERT);
    harness.settle();
}

#[test]
fn the_graph_view_draws_one_box_per_line_and_edits_nothing() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.click(DIAGRAM);
    let nodes = harness.settle();
    let sheet = sheet(&harness);
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

/// Lines and Diagram are the two segments of one toggle, each a button on the keyboard path that a key presses.
#[test]
fn both_segments_of_the_card_toggle_take_the_focus_and_a_key() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    let nodes = harness.settle();
    for segment in [LINES_VIEW, DIAGRAM] {
        assert!(
            nodes
                .iter()
                .any(|node| node.role == Role::Button && node.label == segment),
            "{segment} is a button"
        );
    }
    harness.click(DIAGRAM);
    assert!(sheet(&harness).diagram);
    // Tab walks the keyboard path from the start until it reaches Lines.
    for _ in 0..200 {
        if harness.focused == LINES_VIEW {
            break;
        }
        harness.press(egui::Key::Tab, egui::Modifiers::NONE);
    }
    assert_eq!(harness.focused, LINES_VIEW, "Lines is on the keyboard path");
    harness.press(egui::Key::Space, egui::Modifiers::NONE);
    harness.settle();
    assert!(!sheet(&harness).diagram, "Space on Lines returns to the lines");
    harness.press(egui::Key::Tab, egui::Modifiers::NONE);
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);
    harness.settle();
    assert_eq!(harness.focused, DIAGRAM);
    assert!(sheet(&harness).diagram, "Enter on Diagram shows the Diagram");
}

#[test]
fn the_diagram_frames_the_lines_inside_each_when() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("ants.toml"));
    let sheet = sheet(&harness);
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
        let sheet = sheet(&harness);
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
    assert_eq!(graph.document().id, "sheet_game_of_life_expert");
    assert_eq!(
        harness.app.editor.file_name.as_deref(),
        Some("sheet_game_of_life_expert.toml")
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
    assert!(
        shows_notice(&mut harness, &banner.text()),
        "the banner shows as a notice: {texts:?}"
    );
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
    let graph = harness
        .app
        .editor
        .model
        .graph_mut()
        .expect("the copy opens in the Editor");
    let frames_before = graph.editor_region().frames();
    let n2001 = n(2001);
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

    let graph = harness
        .app
        .editor
        .model
        .graph_mut()
        .expect("the copy stays in the Editor");
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
    apply_sheet_edit(
        sheet_mut(&mut harness),
        henad_graph::sheet::edit::SheetEdit::SetSheetName {
            name: "Life".to_owned(),
        },
    );
    harness.click(&converted_link("Game of Life (expert)"));
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::OriginalChanged);
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
    std::fs::write(folder.0.join("sheet_game_of_life_expert.toml"), "taken").expect("a taken file");
    open_text(&mut harness.app.editor, text.to_owned(), Some("life.toml".to_owned()));
    harness.app.editor.file_path = Some(sheet_path);
    harness.settle();
    convert_open_sheet(&mut harness);
    let copy_path = folder.0.join("sheet_game_of_life_expert_2.toml");
    assert_eq!(harness.app.editor.file_path.as_ref(), Some(&copy_path));
    assert_eq!(
        harness.app.editor.file_status.as_ref().map(FileStatus::text),
        Some("Saved sheet_game_of_life_expert_2.toml")
    );
    let copy_text = std::fs::read_to_string(&copy_path).expect("the copy is written");

    // The copy opened from its file finds the sheet beside it.
    open_text(
        &mut harness.app.editor,
        copy_text,
        Some("sheet_game_of_life_expert_2.toml".to_owned()),
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
    assert_eq!(sheet.document().sheet.id, "sheet_game_of_life");
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

/// Returns `copy`, the converted Game of Life, with the birth rule B36/S23 of High Life: line 2's comparison becomes an
/// either of two new comparisons, the count is 3 and the count is 6.
fn highlife(copy: &henad_graph::GraphFile) -> henad_graph::GraphFile {
    use henad_graph::node::Node as GraphNode;
    use henad_graph::value::{InputValue, PortRef};

    let mut document = copy.document().clone();
    let tick = document
        .passes
        .iter_mut()
        .find(|pass| pass.id == "tick_cell_tick")
        .expect("the tick pass");
    let count = PortRef {
        node: n(1001),
        port: None,
    };
    let equals = |number: u32, value: u32| {
        let mut node = GraphNode::new(n(number), "cmp.eq");
        node.inputs.insert("a".to_owned(), InputValue::Link(count.clone()));
        node.inputs
            .insert("b".to_owned(), InputValue::Literal(Literal::U32(value)));
        node
    };
    tick.body.nodes.push(equals(9001, 3));
    tick.body.nodes.push(equals(9002, 6));
    let birth = tick
        .body
        .nodes
        .iter_mut()
        .find(|node| node.id == n(2001))
        .expect("line 2's comparison");
    *birth = GraphNode::new(n(2001), "logic.or");
    for (port, source) in [("a", 9001), ("b", 9002)] {
        birth.inputs.insert(
            port.to_owned(),
            InputValue::Link(PortRef {
                node: n(source),
                port: None,
            }),
        );
    }
    henad_graph::GraphFile::from_document(&document, copy.editor()).expect("the edited copy reads")
}

/// Converts Game of Life in `harness` and gives the copy the birth rule of High Life, as an expert's edit on the canvas
/// would, keeping the sheet as the copy's counterpart.
fn convert_to_highlife(harness: &mut Harness) {
    convert_life(harness);
    let graph = harness
        .app
        .editor
        .model
        .graph_mut()
        .expect("the copy opens in the Editor");
    let edited = highlife(&graph.to_file().expect("the copy reads"));
    *graph = crate::ui::editor::graph::GraphEditor::new(&edited);
    harness.settle();
}

#[test]
fn show_changes_lists_a_highlife_edit_under_the_frame_of_line_2_and_marks_it_on_the_canvas() {
    use henad_graph::sheet::sentence::{CHANGES_HEADING, IN_NO_FRAME, NODE_ADDED, NODE_CHANGED, SHOW_CHANGES};

    use crate::ui::editor::graph::changes::{ChangeMark, marks_of};

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_to_highlife(&mut harness);
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert_eq!(banner.state, CopyState::Edited("2 nodes added, 1 changed".to_owned()));
    let texts = harness.accessible_texts();
    assert!(
        !texts.iter().any(|text| text == CHANGES_HEADING),
        "the changes wait for Show changes"
    );

    harness.click(SHOW_CHANGES);
    assert!(harness.app.editor.show_changes);
    let texts = harness.accessible_texts();
    for expected in [
        CHANGES_HEADING,
        LINE_2,
        IN_NO_FRAME,
        "Changed: Either (n2001)",
        "Added: Is (n9001)",
    ] {
        assert!(texts.iter().any(|text| text == expected), "{expected} shows: {texts:?}");
    }
    let graph = harness.app.editor.model.graph().expect("the copy");
    let line_2 = graph
        .shown_changes
        .groups
        .iter()
        .find(|group| group.title.as_deref() == Some(LINE_2))
        .expect("the group of line 2");
    assert_eq!(line_2.changed, [n(2001)], "one node under line 2");
    let marks = marks_of(&graph.shown_changes.groups, &Scope::pass("tick_cell_tick"));
    assert_eq!(marks.get(&n(2001)), Some(&ChangeMark::Changed));
    assert_eq!(marks.get(&n(9002)), Some(&ChangeMark::Added));
    assert_eq!(marks.len(), 3);
    assert!(
        texts.iter().filter(|text| *text == NODE_ADDED).count() == 2 && texts.iter().any(|text| text == NODE_CHANGED),
        "the canvas badges the marked nodes: {texts:?}"
    );

    // A listed node opens on its canvas.
    harness.click("Added: Is (n9002)");
    let graph = harness.app.editor.model.graph().expect("the copy");
    assert_eq!(graph.scope(), &Scope::pass("tick_cell_tick"));

    harness.click(SHOW_CHANGES);
    let texts = harness.accessible_texts();
    assert!(
        !texts.iter().any(|text| text == CHANGES_HEADING),
        "the toggle hides the list"
    );
    assert!(
        harness
            .app
            .editor
            .model
            .graph()
            .is_some_and(|graph| graph.shown_changes.is_empty())
    );
}

#[test]
fn an_unedited_copy_offers_no_show_changes() {
    use henad_graph::sheet::sentence::SHOW_CHANGES;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert!(!banner.shows_changes());
    assert!(banner.changes.is_empty());
    let texts = harness.accessible_texts();
    assert!(!texts.iter().any(|text| text == SHOW_CHANGES), "{texts:?}");
}

#[test]
fn the_notes_of_the_author_model_show_under_its_section() {
    use henad_graph::sheet::sentence::FROM_AUTHOR_MODEL;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_starter(&mut harness, "sir.toml");
    let graph = harness.app.editor.model.graph().expect("the copy opens");
    let notes = graph.editor_region().notes();
    assert!(notes.len() >= 2, "the purpose and the parameters' meanings: {notes:?}");
    let texts = harness.accessible_texts();
    assert!(texts.iter().any(|text| text == FROM_AUTHOR_MODEL));
    for note in &notes {
        assert!(texts.contains(&note.title), "{} shows", note.title);
        assert!(texts.contains(&note.text), "{} shows", note.text);
    }
    assert!(
        texts.iter().any(|text| text.starts_with("A spatial SIR epidemic.")),
        "the purpose shows"
    );
}

#[test]
fn add_category_on_a_converted_sir_adds_a_state_with_its_colour_and_the_copy_still_checks() {
    use henad_graph::sheet::edit::STATE_COLORS;

    use crate::ui::editor::graph::world::{next_variant_color, variant_color_name};

    let Some(mut harness) = Harness::new() else {
        return;
    };
    // A tall screen keeps the forms under the categories above the Editor's list of problems.
    harness.screen = egui::vec2(1600.0, 1600.0);
    convert_starter(&mut harness, "sir.toml");
    let graph = harness.app.editor.model.graph().expect("the copy opens");
    let category = graph.document().categories.first().expect("SIR's states").clone();
    let color = next_variant_color(&category);
    assert!(
        category.variants.iter().all(|variant| variant.color != color),
        "a new state takes a colour no state of its category has"
    );

    let categories = format!("Categories ({})", graph.document().categories.len());
    harness.click(&categories);
    let field = |harness: &mut Harness, hint: &str| {
        let nodes = harness.settle();
        nodes
            .iter()
            .filter(|node| node.role == Role::TextInput)
            .find(|node| node.label == hint)
            .and_then(|node| node.bounds)
            .unwrap_or_else(|| panic!("the field {hint}"))
            .center()
    };
    let type_into = |harness: &mut Harness, hint: &str, text: &str| {
        let position = field(harness, hint);
        harness.click_at(position);
        harness.frame(vec![egui::Event::Text(text.to_owned())]);
        harness.settle();
    };

    // A state added to SIR's category takes the next colour of the palette.
    type_into(&mut harness, &format!("New variant of {}", category.id), "waned");
    harness.click("Add variant");
    let graph = harness.app.editor.model.graph().expect("the copy");
    let states = &graph.document().categories[0];
    let waned = states
        .variants
        .iter()
        .find(|variant| variant.name == "waned")
        .expect("the new state")
        .clone();
    assert_eq!(waned.color, color);
    assert!(!graph.has_errors(), "{:?}", graph.report().diagnostics);

    // Its colour changes through the palette.
    harness.click(&variant_color_name(&waned));
    harness.click("Purple");
    let graph = harness.app.editor.model.graph().expect("the copy");
    let purple = graph.document().categories[0]
        .variants
        .iter()
        .find(|variant| variant.name == "waned")
        .expect("the new state")
        .color;
    assert_ne!(purple, waned.color, "the colour changed");
    assert_eq!(
        variant_color_name(&graph.document().categories[0].variants[3]),
        "Colour of waned: Purple"
    );

    // A category of its own starts with its first state in the first palette colour.
    type_into(&mut harness, "New category id", "mood");
    type_into(&mut harness, "First variant", "calm");
    harness.click("Add category");
    let graph = harness.app.editor.model.graph().expect("the copy");
    let mood = graph
        .document()
        .categories
        .iter()
        .find(|category| category.id == "mood")
        .expect("the new category");
    assert_eq!(mood.variants.len(), 1);
    assert_eq!(mood.variants[0].color, STATE_COLORS[0]);
    assert!(!graph.has_errors(), "{:?}", graph.report().diagnostics);

    // The copy is edited with no node changed, and the banner offers no empty list of changes.
    let banner = copy_banner(&mut harness.app.editor).expect("a banner");
    assert!(matches!(banner.state, CopyState::Edited(_)), "{:?}", banner.state);
    assert!(banner.changes.is_empty());
    assert!(!banner.shows_changes());
    let texts = harness.accessible_texts();
    assert!(
        !texts
            .iter()
            .any(|text| text == henad_graph::sheet::sentence::SHOW_CHANGES),
        "{texts:?}"
    );
}

#[test]
fn a_removed_node_is_listed_by_the_name_of_its_kind() {
    use henad_graph::sheet::sentence::SHOW_CHANGES;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    let graph = harness
        .app
        .editor
        .model
        .graph_mut()
        .expect("the copy opens in the Editor");
    let removed = graph.edit(
        vec![EditCommand::RemoveNode {
            scope: Scope::pass("tick_cell_tick"),
            node: n(2001),
        }],
        None,
    );
    assert!(removed, "line 2's comparison is removed");
    harness.settle();
    harness.click(SHOW_CHANGES);
    let texts = harness.accessible_texts();
    assert!(texts.iter().any(|text| text == "Removed: Is (n2001)"), "{texts:?}");
}

/// Draws frames of `harness` until both runs of its comparison finished or one failed, and returns the outcome.
///
/// Panics after two minutes, or when a run failed.
fn wait_for_comparison(harness: &mut Harness) -> crate::ui::editor::compare::CompareOutcome {
    let mut outcome = None;
    wait_within("the end of both runs", std::time::Duration::from_secs(120), || {
        harness.frame(Vec::new());
        let comparison = harness.app.editor.comparison.as_ref().expect("a comparison runs");
        assert_eq!(comparison.failure, None, "the runs finish");
        outcome = comparison.outcome();
        outcome.is_some()
    });
    outcome.expect("the outcome that ended the wait")
}

#[test]
fn an_unedited_copy_matches_its_sheet_at_every_tick_of_compare_runs() {
    use henad_graph::sheet::sentence::{CLOSE_COMPARISON, COMPARE_RUNS, runs_match};

    use crate::ui::editor::compare::{COMPARE_TICKS, CompareOutcome};

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    harness.click(COMPARE_RUNS);
    let outcome = wait_for_comparison(&mut harness);
    assert_eq!(outcome, CompareOutcome::Match { ticks: COMPARE_TICKS });
    let text = runs_match("Game of Life", "Game of Life (expert)", COMPARE_TICKS);
    assert_eq!(
        harness
            .app
            .editor
            .comparison
            .as_ref()
            .map(|comparison| comparison.text()),
        Some(text.clone())
    );
    let texts = harness.accessible_texts();
    assert!(texts.contains(&text), "the comparison shows its line: {texts:?}");
    harness.click(CLOSE_COMPARISON);
    assert!(harness.app.editor.comparison.is_none());
}

/// An unedited copy of bounded confidence, whose looks add up floating-point numbers in the order the agents are
/// found, names no tick of a difference, as two runs of the sheet at one seed already differ.
#[test]
fn compare_runs_of_a_model_that_does_not_replay_exactly_names_no_first_difference() {
    use henad_graph::sheet::sentence::{COMPARE_RUNS, runs_do_not_replay};

    use crate::ui::editor::compare::CompareOutcome;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_starter(&mut harness, "bounded_confidence.toml");
    harness.click(COMPARE_RUNS);
    let outcome = wait_for_comparison(&mut harness);
    assert_eq!(outcome, CompareOutcome::Inexact);
    let text = runs_do_not_replay("Bounded confidence", "Bounded confidence (expert)");
    let texts = harness.accessible_texts();
    assert!(texts.contains(&text), "the comparison shows its line: {texts:?}");
}

#[test]
fn an_edit_of_the_copy_drops_the_comparison_of_the_copy_before_it() {
    use henad_graph::document::{Category, Variant};
    use henad_graph::sheet::edit::STATE_COLORS;
    use henad_graph::sheet::sentence::{COMPARE_RUNS, runs_match};

    use crate::ui::editor::compare::COMPARE_TICKS;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_life(&mut harness);
    harness.click(COMPARE_RUNS);
    wait_for_comparison(&mut harness);
    let text = runs_match("Game of Life", "Game of Life (expert)", COMPARE_TICKS);
    assert!(harness.accessible_texts().contains(&text));

    // Moving a node changes no code, and the comparison stays.
    let graph = harness
        .app
        .editor
        .model
        .graph_mut()
        .expect("the copy opens in the Editor");
    let moved = graph.edit(
        vec![EditCommand::Move {
            node: n(2001),
            pos: [10.0, 10.0],
        }],
        None,
    );
    assert!(moved);
    harness.settle();
    assert!(harness.app.editor.comparison.is_some(), "a move keeps the comparison");

    let graph = harness
        .app
        .editor
        .model
        .graph_mut()
        .expect("the copy opens in the Editor");
    let mood = Category::new("mood", vec![Variant::new("calm", STATE_COLORS[0])]);
    assert!(graph.edit(vec![EditCommand::AddCategory(mood)], None));
    harness.settle();
    assert!(harness.app.editor.comparison.is_none(), "an edit drops the comparison");
    let texts = harness.accessible_texts();
    assert!(!texts.contains(&text), "the old line is gone: {texts:?}");
    assert!(
        texts.iter().any(|shown| shown == COMPARE_RUNS),
        "Compare runs runs again"
    );
}

#[test]
fn compare_runs_names_the_first_tick_at_which_a_highlife_edit_differs() {
    use henad_graph::sheet::sentence::COMPARE_RUNS;

    use crate::ui::editor::compare::CompareOutcome;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    convert_to_highlife(&mut harness);
    harness.click(COMPARE_RUNS);
    let outcome = wait_for_comparison(&mut harness);
    let CompareOutcome::Differ {
        tick,
        measure,
        value,
        copy_value,
    } = outcome
    else {
        panic!("the edit changes the run: {outcome:?}");
    };
    assert_eq!(measure, "Alive");
    assert!(tick >= 1, "tick 0 is the start both share");
    assert_ne!(value, copy_value);
    let text = harness
        .app
        .editor
        .comparison
        .as_ref()
        .map(|comparison| comparison.text())
        .expect("a comparison");
    assert!(
        text.starts_with(&format!(
            "Game of Life (expert) first differs from Game of Life at tick {tick}: Alive is "
        )),
        "{text}"
    );

    // Opening the sheet hides the comparison, and the copy shows it again.
    harness.click(&open_original("Game of Life"));
    let texts = harness.accessible_texts();
    assert!(!texts.contains(&text), "sheet mode shows no comparison");
    assert_eq!(forbidden_words(&texts), Vec::new());
    harness.click(&converted_link("Game of Life (expert)"));
    assert!(harness.accessible_texts().contains(&text));
}
