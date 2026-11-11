//! Tests of the unsaved edits the Editor tracks per model, and of the dialog that asks before New model, New grid rule,
//! New node graph, Open model, Open in Editor or Edit model replaces a model with unsaved edits.

use egui::accesskit::Role;
use henad_graph::document::World;
use henad_graph::edit::EditCommand;
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::{CANCEL, DISCARD_CHANGES, SAVE_MODEL, unsaved_changes};
use henad_graph::sheet::starters;

use crate::tests::support::{Harness, Node};
use crate::ui::editor::expert::{convert_sheet, swap_counterpart};
use crate::ui::editor::graph::{GraphEdit, GraphRequest};
use crate::ui::editor::{EditorModel, Replacement, apply, edit_model, life_starter, new_graph_model, replace_model};
use crate::ui::files::{SaveResult, SaveTarget};

/// Text of the Game of Life sheet starter.
fn life_sheet() -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == "game_of_life.toml")
        .expect("the Game of Life sheet")
        .text
}

/// Sets the purpose of the sheet the Editor shows, an edit that leaves its code alone.
fn edit_purpose(harness: &mut Harness, purpose: &str) {
    let EditorModel::Sheet(sheet) = &mut harness.app.editor.model else {
        panic!("the Editor holds a sheet");
    };
    sheet
        .edit(false, true, |builder| {
            builder
                .apply(SheetEdit::SetPurpose {
                    purpose: Some(purpose.to_owned()),
                })
                .map(drop)
        })
        .expect("the purpose is set");
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
    harness.open_sheet(life_sheet());
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

    harness.open_sheet(life_sheet());
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
    harness.open_sheet(life_sheet());
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
    let EditorModel::Sheet(sheet) = &harness.app.editor.model else {
        panic!("the Editor holds the applied sheet");
    };
    assert_eq!(
        sheet.document().sheet.purpose.as_deref(),
        Some("Cells live and die by their neighbours.")
    );
    assert!(harness.app.editor.unsaved_names().is_empty());
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
    harness.open_sheet(life_sheet());
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
    let EditorModel::Sheet(sheet) = &harness.app.editor.model else {
        panic!("the sheet shows");
    };
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
    let EditorModel::Graph(graph) = &mut harness.app.editor.model else {
        panic!("the copy shows");
    };
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
