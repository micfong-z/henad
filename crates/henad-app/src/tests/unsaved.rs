//! Tests of the unsaved edits the Editor tracks per model, and of the dialog that asks before New model, New grid rule,
//! New node graph, Open model, Open in Editor or Edit model replaces a model with unsaved edits, and before the window
//! closes.

use egui::accesskit::Role;
use egui::{Key, Modifiers, ViewportCommand, ViewportEvent, ViewportId};
use henad_graph::document::World;
use henad_graph::edit::EditCommand;
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::{
    CANCEL, CONVERT, CONVERT_HEADING, DISCARD_CHANGES, MODEL_ACTIONS, SAVE_MODEL, unsaved_changes,
};

use crate::native::guard_close;
use crate::tests::panes::{apply_sheet_edit, sheet_mut};
use crate::tests::support::{Harness, Node, starter_text};
use crate::ui::editor::expert::{convert_sheet, swap_counterpart};
use crate::ui::editor::graph::{GraphEdit, GraphRequest};
use crate::ui::editor::{
    EditorModel, Replacement, apply, edit_model, life_starter, new_graph_model, receive_open, replace_model,
    save_model, save_name,
};
use crate::ui::files::{DialogFile, OpenResult, SaveResult, SaveTarget};

/// Sets the purpose of the sheet the Editor shows, an edit that leaves its code alone.
fn edit_purpose(harness: &mut Harness, purpose: &str) {
    apply_sheet_edit(
        sheet_mut(harness),
        SheetEdit::SetPurpose {
            purpose: Some(purpose.to_owned()),
        },
    );
}

/// Returns the dialog of a settled frame, by its accessible name, `None` while no dialog asks.
fn asking(harness: &mut Harness) -> Option<String> {
    harness
        .settle()
        .into_iter()
        .find(|node| node.role == Role::Dialog && node.label.ends_with("unsaved changes."))
        .map(|node| node.label)
}

/// Clicks the button named `label` inside the dialog named `dialog`.
fn click_in(harness: &mut Harness, dialog: &str, label: &str) {
    let nodes = harness.settle();
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.dialog == dialog && node.label == label && node.role == Role::Button)
        .collect();
    assert_eq!(found.len(), 1, "one {label} in {dialog}: {found:?}");
    let position = found[0].bounds.expect("bounds").center();
    harness.click_at(position);
    harness.settle();
}

/// The dialog asks before a replacement of a model with unsaved edits, names the model, and Cancel keeps it. Apply
/// leaves the edits unsaved, Save model saves the model and then replaces it, Discard changes replaces it at once, and
/// a save canceled in its own dialog keeps the model. A model never edited since it was opened or made, a graph whose
/// canvas has drawn included, is replaced without asking.
#[test]
fn the_editor_asks_before_replacing_a_model_with_unsaved_edits() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    assert!(
        harness.app.editor.unsaved_names().is_empty(),
        "an opened sheet holds no edit"
    );
    harness.click("New grid rule");
    assert_eq!(asking(&mut harness), None);
    assert!(
        harness.app.editor.model.rule_text().is_some(),
        "the rule replaced the sheet at once"
    );

    // A graph's canvas draws, and places and frames its nodes, without an edit.
    new_graph_model(&mut harness.app, "sir.toml");
    harness.settle();
    harness.settle();
    assert!(
        harness.app.editor.unsaved_names().is_empty(),
        "a drawn graph holds no edit"
    );

    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "Cells live and die by their neighbours.");
    apply(&mut harness.app);
    harness.app.wait_for_build();
    assert_eq!(
        harness.app.editor.unsaved_names(),
        ["Game of Life"],
        "Apply saves nothing"
    );
    let sentence = unsaved_changes(&["Game of Life".to_owned()]);
    assert_eq!(sentence, "Game of Life has unsaved changes.");

    harness.click("New grid rule");
    assert_eq!(asking(&mut harness).as_deref(), Some(sentence.as_str()));
    let nodes = harness.settle();
    let mut buttons: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.dialog == sentence && node.role == Role::Button)
        .collect();
    buttons.sort_by(|a, b| {
        let left = |node: &Node| node.bounds.map_or(0.0, |rect| rect.left());
        left(a).total_cmp(&left(b))
    });
    let labels: Vec<&str> = buttons.iter().map(|node| node.label.as_str()).collect();
    assert_eq!(labels, [CANCEL, DISCARD_CHANGES, SAVE_MODEL], "the primary action last");
    click_in(&mut harness, &sentence, CANCEL);
    assert_eq!(asking(&mut harness), None);
    assert!(harness.app.editor.model.sheet().is_some(), "Cancel keeps the sheet");

    // Save model saves the sheet first, and the rule replaces it once the save reports.
    harness.click("New grid rule");
    click_in(&mut harness, &sentence, SAVE_MODEL);
    let requested = harness.app.saves_requested.clone().unwrap_or_default();
    assert_eq!(requested, [(SaveTarget::Model, "opened.toml".to_owned())]);
    assert!(
        harness.app.editor.model.sheet().is_some(),
        "the sheet waits for its save"
    );
    harness.app.finish_save(SaveResult::Saved("life.toml".to_owned()));
    assert_eq!(harness.app.editor.model.rule_text(), Some(life_starter().as_str()));

    // Discard changes replaces an edited rule at once.
    let EditorModel::Rule(text) = &mut harness.app.editor.model else {
        panic!("the Editor holds the rule");
    };
    text.push_str("\n# An edit.\n");
    harness.click("New grid rule");
    let sentence = asking(&mut harness).expect("the dialog asks");
    assert_eq!(sentence, "life_rule.toml has unsaved changes.");
    click_in(&mut harness, &sentence, DISCARD_CHANGES);
    assert_eq!(harness.app.editor.model.rule_text(), Some(life_starter().as_str()));
    assert!(harness.app.editor.unsaved_names().is_empty());

    // A save canceled in its own dialog keeps the model and its edits, and replaces nothing.
    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "A second purpose.");
    replace_model(&mut harness.app, Replacement::NewGridRule);
    click_in(&mut harness, &sentence_of("Game of Life"), SAVE_MODEL);
    harness.app.finish_save(SaveResult::Canceled);
    assert!(harness.app.editor.model.sheet().is_some());
    assert_eq!(harness.app.editor.unsaved_names(), ["Game of Life"]);
    assert_eq!(asking(&mut harness), None);

    // Edit model asks too, as Open in Editor does, and opens the applied sheet without the later edit.
    assert_eq!(harness.app.selected_model.as_deref(), Some("sheet_game_of_life"));
    edit_model(&mut harness.app);
    assert!(asking(&mut harness).is_some());
    click_in(&mut harness, &sentence_of("Game of Life"), DISCARD_CHANGES);
    let sheet = harness
        .app
        .editor
        .model
        .sheet()
        .expect("the Editor holds the applied sheet");
    assert_eq!(
        sheet.document().sheet.purpose.as_deref(),
        Some("Cells live and die by their neighbours.")
    );
    assert!(harness.app.editor.unsaved_names().is_empty());
}

/// A close request of the window with unsaved edits waits, and the dialog asks about them. Cancel keeps the app open,
/// a save canceled in its own dialog keeps it open with the edits, and Save model saves the model and then closes the
/// window.
#[test]
fn a_close_request_with_unsaved_edits_asks_and_saves_before_closing() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "Cells live and die by their neighbours.");
    let sentence = sentence_of("Game of Life");

    assert!(harness.app.hold_close(), "the window closed over unsaved edits");
    assert_eq!(asking(&mut harness).as_deref(), Some(sentence.as_str()));
    click_in(&mut harness, &sentence, CANCEL);
    assert_eq!(asking(&mut harness), None);
    assert!(!harness.app.closing, "Cancel closed the window");
    assert!(harness.app.editor.model.sheet().is_some());

    assert!(harness.app.hold_close());
    click_in(&mut harness, &sentence, SAVE_MODEL);
    harness.app.finish_save(SaveResult::Canceled);
    assert!(!harness.app.closing, "a canceled save closed the window");
    assert_eq!(harness.app.editor.unsaved_names(), ["Game of Life"]);
    assert_eq!(asking(&mut harness), None);

    assert!(harness.app.hold_close());
    click_in(&mut harness, &sentence, SAVE_MODEL);
    assert!(!harness.app.closing, "the window closed before the save reported");
    harness.app.finish_save(SaveResult::Saved("life.toml".to_owned()));
    assert!(harness.app.closing, "the saved model's window stayed open");
    assert!(harness.app.editor.unsaved_names().is_empty());
    assert!(!harness.app.hold_close(), "the closing window held its close request");
}

/// A close request of the window without unsaved edits closes it at once, and Discard changes closes a window whose
/// model holds unsaved edits.
#[test]
fn a_close_request_closes_at_once_without_edits_and_after_discard_with_them() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    assert!(
        !harness.app.hold_close(),
        "a model without edits held the close request"
    );
    assert_eq!(asking(&mut harness), None);

    edit_purpose(&mut harness, "A purpose nobody saves.");
    let sentence = sentence_of("Game of Life");
    assert!(harness.app.hold_close());
    assert!(!harness.app.closing);
    click_in(&mut harness, &sentence, DISCARD_CHANGES);
    assert!(harness.app.closing, "Discard changes kept the window open");
    assert!(!harness.app.hold_close());
}

/// Runs one frame of [`guard_close`] alone, and returns the commands the frame sent to the window.
///
/// `key` is a key pressed and released in the frame with its modifiers, and the frame requests the window's close
/// when `close_requested` holds.
fn guard_frame(harness: &mut Harness, key: Option<(Key, Modifiers)>, close_requested: bool) -> Vec<ViewportCommand> {
    let mut input = egui::RawInput::default();
    if let Some((key, modifiers)) = key {
        input.events = [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            })
            .to_vec();
    }
    if close_requested {
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .events
            .push(ViewportEvent::Close);
    }
    let app = &mut harness.app;
    let output = harness.context.run_ui(input, |ui| guard_close(ui.ctx(), app));
    output
        .viewport_output
        .get(&ViewportId::ROOT)
        .map(|viewport| viewport.commands.clone())
        .unwrap_or_default()
}

/// Cmd+Q on macOS, and Ctrl+Q elsewhere, requests the window's close, which waits while the Editor holds unsaved
/// edits, as a press of the window's close button does. A plain Q and Ctrl+Q on macOS request nothing.
#[test]
fn the_quit_shortcut_requests_a_close_that_unsaved_edits_hold() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "A purpose nobody saves.");

    // egui-winit sets `command` with Cmd on macOS and with Ctrl elsewhere, and `mac_cmd` only with Cmd on macOS.
    for modifiers in [Modifiers::NONE, Modifiers::CTRL] {
        let commands = guard_frame(&mut harness, Some((Key::Q, modifiers)), false);
        assert!(commands.is_empty(), "Q with {modifiers:?} sent {commands:?}");
    }
    for modifiers in [
        Modifiers::MAC_CMD | Modifiers::COMMAND,
        Modifiers::CTRL | Modifiers::COMMAND,
    ] {
        assert_eq!(
            guard_frame(&mut harness, Some((Key::Q, modifiers)), false),
            [ViewportCommand::Close],
            "Q with {modifiers:?}"
        );
    }
    assert_eq!(asking(&mut harness), None);

    assert_eq!(
        guard_frame(&mut harness, None, true),
        [ViewportCommand::CancelClose],
        "the close request went through over unsaved edits"
    );
    assert_eq!(
        asking(&mut harness).as_deref(),
        Some(sentence_of("Game of Life").as_str())
    );
}

/// Returns the sentence of the dialog for the one model `name`.
fn sentence_of(name: &str) -> String {
    unsaved_changes(&[name.to_owned()])
}

/// The model held beside the one shown keeps its own unsaved edits across a conversion and a swap, the dialog names
/// both models, and Save model saves each in turn before the replacement runs.
#[test]
fn the_sheet_beside_its_copy_keeps_its_unsaved_edits_and_saves_in_turn() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "Cells live and die by their neighbours.");
    convert_sheet(&mut harness.app);
    assert!(harness.app.editor.model.graph().is_some(), "the copy shows");
    assert!(!harness.app.editor.is_unsaved(), "a copy just made holds no edit");
    assert_eq!(
        harness.app.editor.unsaved_names(),
        ["Game of Life"],
        "the sheet beside it does"
    );

    swap_counterpart(&mut harness.app.editor);
    assert!(harness.app.editor.is_unsaved(), "the edits follow the sheet");
    let sheet = harness.app.editor.model.sheet().expect("the sheet shows");
    assert_eq!(sheet.document().sheet.name, "Game of Life");
    swap_counterpart(&mut harness.app.editor);
    let copy = harness
        .app
        .editor
        .model
        .graph()
        .expect("the copy")
        .document()
        .name
        .clone();

    // An edit of the copy's world.
    let graph = harness.app.editor.model.graph_mut().expect("the copy shows");
    let mut world = graph.document().world;
    let World::Grid(grid) = &mut world else {
        panic!("Game of Life is a grid");
    };
    grid.width += 1;
    graph.request(GraphRequest::Edit(GraphEdit {
        commands: vec![EditCommand::SetWorld(world)],
        coalesce: None,
    }));
    harness.settle();
    assert!(harness.app.editor.is_unsaved(), "the copy holds the edit");

    replace_model(&mut harness.app, Replacement::NewGridRule);
    let sentence = asking(&mut harness).expect("the dialog asks");
    assert_eq!(sentence, unsaved_changes(&[copy, "Game of Life".to_owned()]));
    click_in(&mut harness, &sentence, SAVE_MODEL);
    harness.app.finish_save(SaveResult::Saved("copy.toml".to_owned()));
    assert!(
        harness.app.editor.model.graph().is_some(),
        "the copy waits for the sheet's save"
    );
    let requested = harness.app.saves_requested.clone().unwrap_or_default();
    assert_eq!(requested, [(SaveTarget::Counterpart, "opened.toml".to_owned())]);
    harness.app.finish_save(SaveResult::Saved("life.toml".to_owned()));
    assert!(
        harness.app.editor.model.rule_text().is_some(),
        "both saved, the rule replaced them"
    );
}

/// Converts the sheet the Editor shows through the model menu and the conversion dialog.
fn convert_through_the_dialog(harness: &mut Harness) {
    harness.click(MODEL_ACTIONS);
    harness.click(CONVERT_HEADING);
    harness.click(CONVERT);
    harness.settle();
}

/// A second conversion of a sheet whose copy beside it holds unsaved edits asks about the copy alone, which the new
/// copy replaces. Cancel keeps the copy, and Save model saves it before the conversion runs. The sheet's own edits stay
/// with the sheet.
#[test]
fn a_second_conversion_asks_before_it_drops_an_edited_copy() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    convert_through_the_dialog(&mut harness);
    let copy = harness
        .app
        .editor
        .model
        .graph()
        .expect("the copy shows")
        .document()
        .name
        .clone();
    let graph = harness.app.editor.model.graph_mut().expect("the copy shows");
    let mut world = graph.document().world;
    let World::Grid(grid) = &mut world else {
        panic!("Game of Life is a grid");
    };
    grid.width += 1;
    graph.request(GraphRequest::Edit(GraphEdit {
        commands: vec![EditCommand::SetWorld(world)],
        coalesce: None,
    }));
    harness.settle();
    swap_counterpart(&mut harness.app.editor);
    edit_purpose(&mut harness, "A purpose the conversion keeps.");
    assert_eq!(
        harness.app.editor.unsaved_names(),
        ["Game of Life".to_owned(), copy.clone()]
    );

    convert_through_the_dialog(&mut harness);
    let sentence = sentence_of(&copy);
    assert_eq!(
        asking(&mut harness).as_deref(),
        Some(sentence.as_str()),
        "the copy alone is named"
    );
    click_in(&mut harness, &sentence, CANCEL);
    assert!(
        harness.app.editor.model.sheet().is_some(),
        "Cancel keeps the sheet shown"
    );
    assert_eq!(
        harness.app.editor.unsaved_names(),
        ["Game of Life".to_owned(), copy.clone()],
        "Cancel keeps the edited copy"
    );

    convert_through_the_dialog(&mut harness);
    click_in(&mut harness, &sentence, SAVE_MODEL);
    let requested = harness.app.saves_requested.clone().unwrap_or_default();
    assert_eq!(requested.len(), 1);
    assert_eq!(
        requested[0].0,
        SaveTarget::Counterpart,
        "the copy is saved, and the sheet is not"
    );
    assert!(
        harness.app.editor.model.sheet().is_some(),
        "the conversion waits for the save"
    );
    harness.app.finish_save(SaveResult::Saved("copy.toml".to_owned()));
    harness.settle();
    assert!(harness.app.editor.model.graph().is_some(), "the new copy shows");
    assert_eq!(
        harness.app.editor.unsaved_names(),
        ["Game of Life".to_owned()],
        "the sheet beside the new copy keeps its edits"
    );
}

/// Returns the result of an Open model dialog that picked the Life starter as `life.toml`.
fn picked_life() -> OpenResult {
    OpenResult::Files(vec![DialogFile {
        name: "life.toml".to_owned(),
        bytes: life_starter().into_bytes(),
        path: None,
    }])
}

/// A model edited while the Open model dialog is open asks before the picked file replaces it, and Discard changes
/// opens the file. A model whose edits were discarded as the dialog opened is replaced at once. A model edited while
/// the save of the unsaved changes dialog is open asks again once the save reports.
#[test]
fn edits_made_while_a_file_dialog_is_open_ask_before_they_are_replaced() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    harness.app.editor.note_open_started();
    edit_purpose(&mut harness, "An edit made while the dialog is open.");
    receive_open(&mut harness.app, picked_life());
    let sentence = sentence_of("Game of Life");
    assert_eq!(asking(&mut harness).as_deref(), Some(sentence.as_str()));
    assert!(harness.app.editor.model.sheet().is_some(), "the edited sheet waits");
    click_in(&mut harness, &sentence, DISCARD_CHANGES);
    assert_eq!(harness.app.editor.model.rule_text(), Some(life_starter().as_str()));
    assert_eq!(harness.app.editor.file_name.as_deref(), Some("life.toml"));

    // Edits discarded as the dialog opened are not asked about again.
    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "An edit discarded before the dialog opens.");
    harness.app.editor.note_open_started();
    receive_open(&mut harness.app, picked_life());
    assert_eq!(asking(&mut harness), None);
    assert!(
        harness.app.editor.model.rule_text().is_some(),
        "the file replaced the sheet at once"
    );

    // An edit made while the save of the unsaved changes dialog is open asks again.
    harness.open_sheet(starter_text("game_of_life.toml"));
    edit_purpose(&mut harness, "A first edit.");
    replace_model(&mut harness.app, Replacement::NewGridRule);
    click_in(&mut harness, &sentence, SAVE_MODEL);
    edit_purpose(&mut harness, "A second edit made while the save is open.");
    harness.app.finish_save(SaveResult::Saved("life_sheet.toml".to_owned()));
    assert_eq!(asking(&mut harness).as_deref(), Some(sentence.as_str()));
    assert!(harness.app.editor.model.sheet().is_some(), "the second edit is kept");
}

/// A save that reports after the model it saved was replaced names no model. One that reports after the model moved
/// beside the model shown names the model it saved.
#[test]
fn a_late_save_names_the_model_it_saved() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starter_text("game_of_life.toml"));
    save_model(&mut harness.app);
    replace_model(&mut harness.app, Replacement::NewGridRule);
    assert!(
        harness.app.editor.model.rule_text().is_some(),
        "the rule replaced the clean sheet"
    );
    harness.app.finish_save(SaveResult::Saved("my_sheet.toml".to_owned()));
    assert_eq!(
        harness.app.editor.file_name, None,
        "the rule takes no name of the sheet's save"
    );
    assert_ne!(save_name(&harness.app.editor), "my_sheet.toml");

    // The copy is saved, and the sheet is shown again before the save reports.
    harness.open_sheet(starter_text("game_of_life.toml"));
    convert_sheet(&mut harness.app);
    save_model(&mut harness.app);
    swap_counterpart(&mut harness.app.editor);
    harness.app.finish_save(SaveResult::Saved("copy.toml".to_owned()));
    assert_eq!(
        harness.app.editor.file_name.as_deref(),
        Some("opened.toml"),
        "the sheet keeps its name"
    );
    let copy = harness.app.editor.counterpart.as_ref().expect("the copy beside");
    assert_eq!(
        copy.file_name.as_deref(),
        Some("copy.toml"),
        "the copy takes the name it was saved under"
    );
}
