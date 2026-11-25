//! Tests of the fire counts beside the card's lines: the number of cells each change line applied to in the latest
//! counted tick of the sheet's preview, shown while it plays and kept while it is paused.

use egui::accesskit::Role;
use henad_graph::sheet::SheetFile;
use henad_graph::sheet::document::{SheetLiteral, SheetValue, Slot};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::{add_value_name, fire_count, for_each_optional_name};

use crate::tests::panes::{sheet, sheet_mut};
use crate::tests::support::{Harness, Node, edited, settle_preview, starter_text, top, wait_until};

/// Takes the preview's snapshots until one holds the counts of a step, keeps it as the app's snapshot and returns the
/// tick of its counts, or panics after [`DEADLINE`](crate::tests::support::DEADLINE).
fn wait_for_counts(harness: &mut Harness) -> u64 {
    let app = &mut harness.app;
    let mut counted = 0;
    wait_until("a snapshot of the preview that holds counts", || {
        if let Some(snapshot) = app.sim_thread.as_mut().expect("a preview runs").take_snapshot() {
            counted = snapshot.fire_counts.tick;
            app.snapshot = Some(snapshot);
        }
        counted > 0
    });
    counted
}

/// Returns the fire count labels of a settled frame, each with its description, top to bottom.
fn count_labels(harness: &mut Harness) -> Vec<Node> {
    let mut labels: Vec<Node> = harness
        .settle()
        .into_iter()
        .filter(|node| node.role == Role::Label && node.description.contains(" applied to "))
        .collect();
    labels.sort_by(|a, b| top(a).total_cmp(&top(b)));
    labels
}

/// Returns the bounds of the control named `name` in `nodes`, or panics.
fn control_bounds(nodes: &[Node], name: &str) -> egui::Rect {
    nodes
        .iter()
        .find(|node| node.role == Role::Button && node.label == name)
        .and_then(|node| node.bounds)
        .unwrap_or_else(|| panic!("no control named {name}"))
}

/// Returns the bounds of the line whose accessible label starts with `prefix` in `nodes`, or panics.
fn line_bounds(nodes: &[Node], prefix: &str) -> egui::Rect {
    nodes
        .iter()
        .find(|node| node.role == Role::Group && node.label.starts_with(prefix))
        .and_then(|node| node.bounds)
        .unwrap_or_else(|| panic!("no line labelled {prefix}"))
}

/// Game of Life and SIR show a count beside each of their two change lines while the preview plays, the counts of the
/// published snapshot in the wording of the card, and keep them while the preview is paused.
#[test]
fn the_change_lines_show_their_counts_while_playing_and_keep_them_when_paused() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    // A narrow window wraps line 3 onto a second row.
    harness.screen = egui::vec2(700.0, 1400.0);
    for name in ["game_of_life.toml", "sir.toml"] {
        let text = starter_text(name);
        let file = SheetFile::parse(text).expect("the starter lowers");
        harness.open_sheet(text);
        settle_preview(&mut harness.app);
        harness.app.wait_for_build();
        assert!(
            count_labels(&mut harness).is_empty(),
            "{name}: no count before a tick runs"
        );
        let before = line_bounds(&harness.settle(), "Line 3: ");

        // Play, as Playback's button presses it.
        harness.app.play();
        let tick = wait_for_counts(&mut harness);
        let counts = harness
            .app
            .snapshot
            .as_ref()
            .map(|snapshot| file.fire_lines().counts_by_line(&snapshot.fire_counts.counts))
            .expect("a snapshot");
        assert_eq!(
            counts.keys().map(String::as_str).collect::<Vec<_>>(),
            ["l2", "l3"],
            "{name}"
        );
        let shown = count_labels(&mut harness);
        let descriptions: Vec<&str> = shown.iter().map(|node| node.description.as_str()).collect();
        let expected: Vec<String> = [("Line 2", "l2"), ("Line 3", "l3")]
            .iter()
            .map(|(prefix, line)| {
                let count = counts[*line];
                let cells = if count == 1 { "cell" } else { "cells" };
                format!("{prefix} applied to {} {cells} in tick {tick}.", fire_count(count))
            })
            .collect();
        assert_eq!(descriptions, expected, "{name}");
        for (node, line) in shown.iter().zip(["l2", "l3"]) {
            let words = fire_count(counts[line]);
            assert!(
                node.label == words || node.value == words,
                "{name}: the count of {line} reads {words}: {node:?}"
            );
        }

        // The count takes no room of its own, so the order note under line 3 stays below the line's sentence, inside
        // the line's row.
        let nodes = harness.settle();
        let line_3 = line_bounds(&nodes, "Line 3: ");
        // The column of counts is kept before the first count, so the line does not move when the counts appear.
        assert_eq!(line_3, before, "{name}: line 3 keeps its place when the counts appear");
        let note = nodes
            .iter()
            .find(|node| node.value.starts_with("Lines 2 and 3 change different cells"))
            .and_then(|node| node.bounds)
            .expect("the order note");
        // The last control of line 3: Game of Life's "+" after its list, SIR's optional "for each".
        let last_name = if name == "game_of_life.toml" {
            add_value_name("Line 3")
        } else {
            for_each_optional_name("Line 3")
        };
        let last = control_bounds(&nodes, &last_name);
        assert!(
            last.top() > line_3.top() + 30.0,
            "{name}: line 3 wraps, {last:?} in {line_3:?}"
        );
        assert!(
            line_3.contains_rect(note)
                && note.top() >= last.bottom()
                && note.top() >= shown[1].bounds.expect("a count").bottom(),
            "{name}: the note at {note:?} overlaps the sentence of line 3 at {line_3:?}"
        );

        // Pause, then let any snapshot in flight land.
        harness.app.pause_simulation();
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(snapshot) = harness
            .app
            .sim_thread
            .as_mut()
            .and_then(|thread| thread.take_snapshot())
        {
            harness.app.snapshot = Some(snapshot);
        }
        let paused: Vec<String> = count_labels(&mut harness)
            .into_iter()
            .map(|node| node.description)
            .collect();
        assert_eq!(paused.len(), 2, "{name}: {paused:?}");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let later: Vec<String> = count_labels(&mut harness)
            .into_iter()
            .map(|node| node.description)
            .collect();
        assert_eq!(later, paused, "{name}: the counts stay while paused");
        harness.app.offload_simulation();
    }
}

/// A sheet whose code differs from the running sheet's shows no counts, as after an edit before its preview builds.
#[test]
fn a_sheet_shows_no_counts_of_a_run_of_other_code() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let text = starter_text("game_of_life.toml");
    harness.open_sheet(text);
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    harness.app.play();
    wait_for_counts(&mut harness);
    assert_eq!(count_labels(&mut harness).len(), 2);

    harness.open_sheet(&edited(text, "right = 3\n", "right = 4\n"));
    assert!(
        count_labels(&mut harness).is_empty(),
        "the run is the sheet before the edit"
    );
    harness.app.offload_simulation();
}

/// Sets slot `path` of line `line` of the sheet open in the harness to `value`, as one edit in the Card pane.
fn set_slot(harness: &mut Harness, line: &str, path: &str, value: Slot) {
    sheet_mut(harness)
        .edit(true, false, |builder| {
            builder
                .apply(SheetEdit::SetSlot {
                    line: line.to_owned(),
                    path: path.to_owned(),
                    value,
                })
                .map(drop)
        })
        .expect("the edit is accepted");
}

/// An edit in the Card pane hides the counts of the run before it, and a blank that skips line 2 leaves the card its
/// column of counts, so line 3 keeps its size.
#[test]
fn an_edit_hides_the_counts_and_a_blank_keeps_the_column() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    // A narrow window wraps line 3 onto a second row.
    harness.screen = egui::vec2(700.0, 1400.0);
    harness.open_sheet(starter_text("game_of_life.toml"));
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    harness.app.play();
    wait_for_counts(&mut harness);
    assert_eq!(count_labels(&mut harness).len(), 2);
    // The sentence of line 3 keeps its rows while the last control stays where it was in the line.
    let sentence = |nodes: &[Node]| {
        let line = line_bounds(nodes, "Line 3: ");
        let last = control_bounds(nodes, &add_value_name("Line 3"));
        (line.width(), last.translate(-line.min.to_vec2()))
    };
    let counting = sentence(&harness.settle());

    let four = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(4.0)));
    set_slot(&mut harness, "l2", "condition.right", four);
    assert!(
        count_labels(&mut harness).is_empty(),
        "the run is the sheet before the edit"
    );
    assert_eq!(sentence(&harness.settle()), counting);

    set_slot(&mut harness, "l2", "condition.right", Slot::Blank);
    let file = sheet(&harness)
        .file()
        .expect("a blank skips its line and the sheet lowers");
    assert_eq!(file.fire_lines().lines(), ["l3"]);
    assert!(count_labels(&mut harness).is_empty());
    assert_eq!(
        sentence(&harness.settle()),
        counting,
        "line 3 keeps its sentence's rows while line 2 is blank"
    );
    harness.app.offload_simulation();
}
