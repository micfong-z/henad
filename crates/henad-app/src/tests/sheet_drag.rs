//! Tests of dragging lines on the Card pane by their gutter: a drop runs the moves of the line menu as one step of
//! undo, a line goes into a When, to its end and out after it, a When moves with its lines but not into another When,
//! no line goes to another card and no look goes inside a Repeat, Escape and a click change nothing, the pane scrolls
//! at its edge, and every slot a drop offers is one the line menu's moves reach.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2};
use henad_graph::sheet::document::{Around, LineKind, SheetCard, SheetDocument, SheetLine};
use henad_graph::sheet::edit::{MoveDirection, SheetBuilder, SheetEdit};
use henad_graph::sheet::sentence::{
    AddButton, DROP_INTO_BLOCK, DROP_ON_OTHER_CARD, LOOK_IN_REPEAT, add_inside_label, drag_line_tooltip,
};

use crate::tests::panes::{
    Seen, apply_sheet_edit, assert_no_forbidden_word, document_of, line_text, named, open_starter, sheet, sheet_mut,
    shows, starter_document, text, text_and_code,
};
use crate::tests::support::Harness;
use crate::ui::editor::sheet::card_drag::{DropMove, DropSlot, slot_of};

/// Returns the row of the line whose accessible name starts with `prefix`, such as "Line 3:".
fn row(harness: &mut Harness, prefix: &str) -> Rect {
    harness
        .settle()
        .into_iter()
        .find(|node| node.role == Role::Group && node.label.starts_with(prefix))
        .and_then(|node| node.bounds)
        .unwrap_or_else(|| panic!("no row starts with {prefix}"))
}

/// Returns the bounds of the one control named `name`, such as a block's add row.
fn control(harness: &mut Harness, name: &str) -> Rect {
    let found = named(harness, name);
    assert_eq!(found.len(), 1, "one control is named {name}");
    found[0].bounds.expect("bounds")
}

/// Returns a point in the gutter of the row `row`, the handle that drags its line.
fn gutter(row: Rect) -> Pos2 {
    pos2(row.left() + 12.0, row.top() + 16.0)
}

/// Returns a point in the upper half of the row `row`, over the gap above it.
fn upper_half(row: Rect) -> Pos2 {
    pos2(row.left() + 80.0, row.top() + 3.0)
}

/// Returns the event of the primary button at `position`, pressed or released.
fn button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// Presses the primary button at `start` and moves the pointer to `end` in steps, holding the button.
fn drag_to(harness: &mut Harness, start: Pos2, end: Pos2) {
    harness.frame(vec![Event::PointerMoved(start)]);
    harness.frame(vec![button(start, true)]);
    for step in 1..=10 {
        let at = start + (end - start) * step as f32 / 10.0;
        harness.frame(vec![Event::PointerMoved(at)]);
    }
    harness.frame(vec![Event::PointerMoved(end)]);
}

/// Releases the primary button at `at` and settles.
fn release(harness: &mut Harness, at: Pos2) {
    harness.frame(vec![button(at, false)]);
    harness.frame(vec![Event::PointerGone]);
    harness.settle();
}

/// Drags from `start` to `end` and drops there.
fn drop_at(harness: &mut Harness, start: Pos2, end: Pos2) {
    drag_to(harness, start, end);
    release(harness, end);
}

/// Returns the slot of line `line_id` in the sheet in the harness's Editor.
fn slot_in(harness: &Harness, line_id: &str) -> DropSlot {
    slot_of(sheet(harness).document(), line_id).expect("the line")
}

/// Returns the slot in block `parent` before line `before`.
fn drop_slot(parent: Option<&str>, before: Option<&str>) -> DropSlot {
    DropSlot {
        parent: parent.map(str::to_owned),
        before: before.map(str::to_owned),
    }
}

/// Returns the canonical text of `document` after `edits`.
fn text_after(document: &SheetDocument, edits: Vec<SheetEdit>) -> String {
    let mut builder = SheetBuilder::new(document.clone()).with_key_seed(0);
    for edit in edits {
        builder.apply(edit).expect("the edit applies");
    }
    text_and_code(builder.document()).0
}

/// Returns the edit that moves line `line` one step in `direction`.
fn move_line(line: &str, direction: MoveDirection) -> SheetEdit {
    SheetEdit::MoveLine {
        line: line.to_owned(),
        direction,
    }
}

#[test]
fn a_dropped_line_moves_as_the_line_menu_moves_it_in_one_step_of_undo() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let starter = text(&harness);
    let moved = text_after(
        &starter_document("game_of_life.toml"),
        vec![move_line("l3", MoveDirection::Up), move_line("l3", MoveDirection::Up)],
    );
    let sentence = line_text(&mut harness, "Line 3");
    let start = gutter(row(&mut harness, "Line 3:"));
    let end = upper_half(row(&mut harness, "Line 1:"));
    drop_at(&mut harness, start, end);
    assert_eq!(text(&harness), moved, "the drop runs the two moves of Move line 3 up");
    assert_eq!(line_text(&mut harness, "Line 1"), sentence);
    sheet_mut(&mut harness).undo(false);
    assert_eq!(text(&harness), starter, "one undo takes the whole drop back");
    assert!(!sheet(&harness).can_undo());
    sheet_mut(&mut harness).redo(false);
    assert_eq!(text(&harness), moved);
}

#[test]
fn a_line_drops_into_a_when_to_its_end_and_out_after_it() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "ants.toml");
    let starter = text(&harness);
    // Line 16 goes between lines 13 and 14, inside the When of line 12.
    let start = gutter(row(&mut harness, "Line 16:"));
    let end = upper_half(row(&mut harness, "Line 14:"));
    drag_to(&mut harness, start, end);
    seen.look(&mut harness);
    release(&mut harness, end);
    assert_eq!(slot_in(&harness, "l16"), drop_slot(Some("l12"), Some("l14")));
    let inside = text(&harness);
    // Now line 14, it goes to the end of the When, above the When's add row.
    let start = gutter(row(&mut harness, "Line 14:"));
    let add_row = control(&mut harness, &add_inside_label(12));
    drop_at(&mut harness, start, pos2(add_row.center().x, add_row.top()));
    assert_eq!(slot_in(&harness, "l16"), drop_slot(Some("l12"), None));
    // Now line 16, the last inside the When, it goes out under the When's add row, back where it started.
    let start = gutter(row(&mut harness, "Line 16:"));
    let add_row = control(&mut harness, &add_inside_label(12));
    drop_at(&mut harness, start, pos2(add_row.center().x, add_row.bottom()));
    assert_eq!(slot_in(&harness, "l16"), drop_slot(None, None));
    assert_eq!(text(&harness), starter);
    // Each drop is one step of undo.
    sheet_mut(&mut harness).undo(false);
    sheet_mut(&mut harness).undo(false);
    assert_eq!(text(&harness), inside);
    assert_no_forbidden_word(&seen);
}

#[test]
fn a_when_moves_with_its_lines_but_not_into_another_when() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "ants.toml");
    let starter = text(&harness);
    // The When of line 12 over the place between lines 10 and 11, inside the When of line 9.
    let start = gutter(row(&mut harness, "Line 12:"));
    let end = upper_half(row(&mut harness, "Line 11:"));
    drag_to(&mut harness, start, end);
    assert!(shows(&mut harness, DROP_INTO_BLOCK), "the copy says why");
    seen.look(&mut harness);
    release(&mut harness, end);
    assert_eq!(text(&harness), starter, "a refused drop changes nothing");
    assert!(!sheet(&harness).can_undo());
    // The When of line 9 to the top of the card takes lines 10 and 11 with it.
    let moves = (0..8).map(|_| move_line("l9", MoveDirection::Up)).collect();
    let moved = text_after(&starter_document("ants.toml"), moves);
    let start = gutter(row(&mut harness, "Line 9:"));
    let end = upper_half(row(&mut harness, "Line 1:"));
    drag_to(&mut harness, start, end);
    seen.look(&mut harness);
    release(&mut harness, end);
    assert_eq!(slot_in(&harness, "l9"), drop_slot(None, Some("l1")));
    assert_eq!(slot_in(&harness, "l10"), drop_slot(Some("l9"), Some("l11")));
    assert_eq!(text(&harness), moved);
    assert_no_forbidden_word(&seen);
}

#[test]
fn a_when_dropped_over_its_own_lines_stays_and_says_nothing() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "ants.toml");
    let starter = text(&harness);
    let start = gutter(row(&mut harness, "Line 12:"));
    let end = upper_half(row(&mut harness, "Line 14:"));
    drag_to(&mut harness, start, end);
    for refusal in [DROP_INTO_BLOCK, DROP_ON_OTHER_CARD, LOOK_IN_REPEAT] {
        assert!(
            !shows(&mut harness, refusal),
            "a drag over its own lines refuses nothing"
        );
    }
    release(&mut harness, end);
    assert_eq!(text(&harness), starter);
    assert!(!sheet(&harness).can_undo());
}

#[test]
fn a_line_does_not_go_to_another_card_and_a_look_does_not_go_inside_a_repeat() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    let starter = text(&harness);
    let start = gutter(row(&mut harness, "Line 3:"));
    let clear = row(&mut harness, "Clear line 1:");
    let end = pos2(clear.left() + 80.0, clear.center().y);
    drag_to(&mut harness, start, end);
    assert!(shows(&mut harness, DROP_ON_OTHER_CARD), "the copy says why");
    seen.look(&mut harness);
    release(&mut harness, end);
    assert_eq!(text(&harness), starter);
    // A Count added after the Repeat of the random walk stays out of it.
    open_starter(&mut harness, "random_walk.toml");
    let mut count = SheetLine::new(LineKind::Count);
    count.around = Some(Around::AgentsWithin);
    let card = sheet(&harness).document().cards[0].id.clone();
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::AddLine {
            card,
            inside: None,
            line: Box::new(count),
        },
    );
    let added = text(&harness);
    let start = gutter(row(&mut harness, "Line 3:"));
    let end = upper_half(row(&mut harness, "Line 2:"));
    drag_to(&mut harness, start, end);
    assert!(shows(&mut harness, LOOK_IN_REPEAT), "the copy says why");
    seen.look(&mut harness);
    release(&mut harness, end);
    assert_eq!(text(&harness), added);
    assert_no_forbidden_word(&seen);
}

#[test]
fn escape_and_a_click_on_the_gutter_change_nothing() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let starter = text(&harness);
    let start = gutter(row(&mut harness, "Line 3:"));
    harness.click_at(start);
    harness.settle();
    assert_eq!(text(&harness), starter, "a click moves nothing");
    let end = upper_half(row(&mut harness, "Line 1:"));
    drag_to(&mut harness, start, end);
    harness.press(Key::Escape, Modifiers::NONE);
    release(&mut harness, end);
    assert_eq!(text(&harness), starter, "Escape ends the drag");
    assert!(!sheet(&harness).can_undo());
}

#[test]
fn the_gutter_says_how_to_move_its_line() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let start = gutter(row(&mut harness, "Line 3:"));
    harness.frame(vec![Event::PointerMoved(start)]);
    for _ in 0..60 {
        harness.frame(Vec::new());
    }
    assert!(shows(&mut harness, &drag_line_tooltip(AddButton::Change.group())));
}

#[test]
fn the_card_pane_scrolls_while_a_line_is_dragged_at_its_edge() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    open_starter(&mut harness, "ants.toml");
    let starter = text(&harness);
    let pane = harness
        .settle()
        .into_iter()
        .find(|node| node.role == Role::Group && node.label == "Card")
        .and_then(|node| node.bounds)
        .expect("the Card pane");
    let first = row(&mut harness, "Line 2:");
    let start = gutter(row(&mut harness, "Line 1:"));
    let end = pos2(first.left() + 80.0, pane.bottom() - 6.0);
    drag_to(&mut harness, start, end);
    for _ in 0..30 {
        harness.frame(vec![Event::PointerMoved(end)]);
    }
    let scrolled = row(&mut harness, "Line 2:");
    assert!(
        scrolled.top() < first.top() - 100.0,
        "the pane scrolls under a pointer at its edge: {first:?} to {scrolled:?}"
    );
    release(&mut harness, end);
    assert_ne!(
        slot_in(&harness, "l1"),
        drop_slot(None, Some("l2")),
        "the line moves down the card"
    );
    sheet_mut(&mut harness).undo(false);
    assert_eq!(text(&harness), starter);
}

/// Returns every slot of `card`: each place in the card's top level and inside each When or Repeat.
fn slots_of(card: &SheetCard) -> Vec<DropSlot> {
    let lists = std::iter::once((None, &card.lines)).chain(
        card.line_tables
            .iter()
            .filter(|(_, line)| line.kind.is_block())
            .map(|(id, line)| (Some(id.as_str()), &line.lines)),
    );
    lists
        .flat_map(|(parent, list)| {
            (0..=list.len()).map(move |index| drop_slot(parent, list.get(index).map(String::as_str)))
        })
        .collect()
}

/// Returns whether line `line_id` of `card` lies inside the block `block_id`, directly or deeper.
fn is_inside_block(card: &SheetCard, line_id: &str, block_id: &str) -> bool {
    let parent_of = |id: &str| {
        card.line_tables
            .iter()
            .find(|(_, line)| line.lines.iter().any(|inner| inner == id))
            .map(|(parent, _)| parent.clone())
    };
    std::iter::successors(parent_of(line_id), |id| parent_of(id)).any(|parent| parent == block_id)
}

#[test]
fn every_slot_a_drop_offers_is_reached_by_the_moves_of_the_line_menu() {
    let mut documents: Vec<SheetDocument> = henad_graph::sheet::starters()
        .iter()
        .map(|starter| document_of(starter.text))
        .collect();
    // The ants with a When inside the When of line 9, so a block sits inside a block.
    let mut ants = SheetBuilder::new(starter_document("ants.toml")).with_key_seed(0);
    let card = ants.document().cards[0].id.clone();
    ants.apply(SheetEdit::AddLine {
        card,
        inside: Some("l9".to_owned()),
        line: Box::new(SheetLine::new(LineKind::When)),
    })
    .expect("a nested When");
    documents.push(ants.into_document());
    let mut checked = 0;
    for document in &documents {
        for card in &document.cards {
            for line_id in card.line_tables.keys() {
                let block = card.line_tables[line_id].kind.is_block();
                let here = slot_of(document, line_id).expect("the line");
                let planned = DropMove::planned(document, &card.id, line_id);
                for target in slots_of(card) {
                    // A slot right above the line, at the line, or inside it leaves the line where it is.
                    let inside = target
                        .parent
                        .as_deref()
                        .is_some_and(|parent| parent == line_id || is_inside_block(card, parent, line_id));
                    if target == here || target.before.as_deref() == Some(line_id.as_str()) || inside {
                        continue;
                    }
                    let drop = planned.iter().find(|drop| *drop.slot() == target).cloned();
                    // A When or Repeat stays among the lines beside it and the blocks around it.
                    let reachable = !block
                        || target
                            .parent
                            .as_deref()
                            .is_none_or(|parent| is_inside_block(card, line_id, parent));
                    assert_eq!(drop.is_some(), reachable, "{line_id} to {target:?} on {}", card.id);
                    let Some(drop) = drop else {
                        continue;
                    };
                    let mut builder = SheetBuilder::new(document.clone()).with_key_seed(0);
                    drop.run(&mut builder).expect("the moves reach the slot");
                    assert_eq!(slot_of(builder.document(), line_id), Some(target));
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 500, "{checked} drops checked");
}
