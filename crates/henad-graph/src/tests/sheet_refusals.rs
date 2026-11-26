//! Tests that each choice [`refusals`] refuses is one the sheet's build refuses with the same words, and that each
//! choice it lets through adds none of those refusals.

use super::sheet_support::{builder_of, phrase, read, sheet_text, write};
use crate::sheet::document::{
    Around, CardRuns, CellWhich, ChooseRow, CompareTest, Edges, LineKind, LineMode, LookSummary, MadeOf, PickRule,
    PropertyType, SheetChip, SheetDocument, SheetLine, SheetValue, Slot, StartRule, StartsAs, SummaryKind,
};
use crate::sheet::edit::{SheetBuilder, SheetEdit};
use crate::sheet::relations::CardRelations;
use crate::sheet::{SheetFile, SheetFileError, refusals, sentence};

/// The sheets of agents the tests change.
const AGENT_SHEETS: [&str; 5] = [
    "boids.toml",
    "random_walk.toml",
    "ants.toml",
    "bounded_confidence.toml",
    "mobile_sir.toml",
];

/// Returns the errors of the check and the lowering of `document`, none for a document that lowers.
fn errors(document: &SheetDocument) -> Vec<String> {
    match SheetFile::parse(&write(document)) {
        Ok(_) => Vec::new(),
        Err(SheetFileError::Invalid(errors)) => errors.as_slice().iter().map(|error| error.message.clone()).collect(),
        Err(other) => panic!("the sheet does not read: {other}"),
    }
}

/// Returns whether `message` is one of the refusals [`refusals`] gives.
fn is_refusal(message: &str) -> bool {
    [
        sentence::LOOK_IN_REPEAT,
        sentence::NOT_YET_REPEAT_CHANGE,
        sentence::NOT_YET_AGENT_WALLS,
        sentence::NOT_YET_BLOCKED_MOVE_BY,
        sentence::AHEAD_NEEDS_CELLS,
    ]
    .contains(&message)
        || message.contains("This card cannot also")
}

/// Asserts that applying a choice gave `after`, the errors of the changed sheet, where `before` are the errors of the
/// sheet before it: the refusal `expected` among them, alone or after the words of its line ("line 8: "), or no
/// refusal that `before` lacks for `None`.
fn assert_agrees(choice: &str, expected: Option<&str>, before: &[String], after: &[String]) {
    if let Some(message) = expected {
        assert!(
            after
                .iter()
                .any(|error| error == message || error.ends_with(&format!(": {message}"))),
            "{choice}: greyed with {message:?}, and the build gave {after:#?}"
        );
    } else {
        let added: Vec<&String> = after
            .iter()
            .filter(|error| is_refusal(error) && !before.contains(error))
            .collect();
        assert!(
            added.is_empty(),
            "{choice}: offered, and the build refused it with {added:#?}"
        );
    }
}

/// Returns the line each item of an add menu on a card over the agents `every` adds, by its words, as the editor makes
/// it with its blanks empty.
fn new_agent_lines(every: &str) -> Vec<(&'static str, SheetLine)> {
    let compare = || {
        let mut compare = SheetLine::new(LineKind::Compare);
        compare.test = Some(CompareTest::Is);
        compare
    };
    let look = |count: bool| {
        let mut line = SheetLine::new(if count { LineKind::Count } else { LineKind::LookAround });
        line.around = Some(Around::AgentsWithin);
        line.agent_kind = Some(every.to_owned());
        line.include_me = Some(false);
        if count {
            line.whose = phrase(compare());
        } else {
            let mut summary = LookSummary::new("", "");
            summary.summary = Some(SummaryKind::Average);
            line.summaries = vec![summary];
        }
        line
    };
    let mode = |kind: LineKind, mode: LineMode| {
        let mut line = SheetLine::new(kind);
        line.mode = Some(mode);
        line
    };
    let mut pick = SheetLine::new(LineKind::PickCell);
    pick.around = Some(Around::Cells8);
    pick.pick = Some(PickRule::Most);
    pick.open_only = Some(true);
    let mut here = SheetLine::new(LineKind::LookAtCell);
    here.which = Some(CellWhich::Here);
    let mut choose = SheetLine::new(LineKind::Choose);
    choose.rows = vec![ChooseRow::default()];
    let mut become_with_chance = SheetLine::new(LineKind::Become);
    become_with_chance.condition = phrase(SheetLine::new(LineKind::Chance));
    vec![
        ("Count", look(true)),
        ("Look around", look(false)),
        ("Pick a cell", pick),
        ("The cell here or ahead", here),
        ("Become, with chance", become_with_chance),
        ("With chance", SheetLine::new(LineKind::Chance)),
        ("Random number", SheetLine::new(LineKind::RandomNumber)),
        ("Random direction", SheetLine::new(LineKind::RandomDirection)),
        ("Compare", compare()),
        ("All of", SheetLine::new(LineKind::AllOf)),
        ("Any of", SheetLine::new(LineKind::AnyOf)),
        ("Calculate", SheetLine::new(LineKind::Calculate)),
        ("Choose", choose),
        ("Depending on", SheetLine::new(LineKind::DependingOn)),
        ("Become", SheetLine::new(LineKind::Become)),
        ("Set", SheetLine::new(LineKind::Set)),
        ("Steer", mode(LineKind::Steer, LineMode::By)),
        ("Keep between", SheetLine::new(LineKind::KeepBetween)),
        ("Move by", mode(LineKind::Move, LineMode::By)),
        ("Move to", mode(LineKind::Move, LineMode::To)),
        ("Leave", SheetLine::new(LineKind::Leave)),
        ("Record", SheetLine::new(LineKind::Record)),
        ("When", SheetLine::new(LineKind::When)),
        ("Repeat", SheetLine::new(LineKind::Repeat)),
    ]
}

/// Returns the index and id of the card of `document` that runs each tick over agents.
fn agent_card(document: &SheetDocument) -> (usize, String) {
    document
        .cards
        .iter()
        .enumerate()
        .find(|(_, card)| card.runs == CardRuns::EachTick && card.every != "cells")
        .map(|(index, card)| (index, card.id.clone()))
        .expect("a card over agents")
}

/// Applies `edit` to a copy of `builder`, and returns the document it leaves.
fn applied(builder: &SheetBuilder, edit: SheetEdit) -> SheetDocument {
    let mut copy = builder.clone();
    let shown = format!("{edit:?}");
    copy.apply(edit)
        .unwrap_or_else(|error| panic!("{shown} is refused: {error}"));
    copy.document().clone()
}

#[test]
fn every_line_offered_inside_a_repeat_runs_there_and_every_greyed_one_is_refused() {
    let builder = builder_of("random_walk.toml");
    let document = builder.document();
    let (card_index, card) = agent_card(document);
    let relations = CardRelations::new(document, &document.cards[card_index]);
    let repeat = relations
        .order()
        .iter()
        .find(|id| relations.line(id).is_some_and(|line| line.kind == LineKind::Repeat))
        .map(|id| (*id).to_owned())
        .expect("the walkers' Repeat");
    let before = errors(document);
    let every = document.cards[card_index].every.clone();
    let mut greyed = 0;
    for (item, line) in new_agent_lines(&every) {
        // A Move to below the walkers' Move by is refused for that first, as the lowering refuses it.
        let expected = refusals::change(document, &relations, &line, None)
            .or_else(|| refusals::in_repeat(&line).map(str::to_owned));
        greyed += usize::from(expected.is_some());
        let after = errors(&applied(
            &builder,
            SheetEdit::AddLine {
                card: card.clone(),
                inside: Some(repeat.clone()),
                line: Box::new(line),
            },
        ));
        assert_agrees(item, expected.as_deref(), &before, &after);
    }
    assert_eq!(greyed, 9, "three looks and six changes");
}

#[test]
fn every_move_offered_on_an_agent_card_runs_and_every_greyed_one_is_refused() {
    let mut greyed = 0;
    for file in AGENT_SHEETS {
        let builder = builder_of(file);
        let document = builder.document();
        let (card_index, card) = agent_card(document);
        let relations = CardRelations::new(document, &document.cards[card_index]);
        let before = errors(document);
        for (item, line) in new_agent_lines(&document.cards[card_index].every) {
            let expected = refusals::change(document, &relations, &line, None);
            greyed += usize::from(expected.is_some());
            let after = errors(&applied(
                &builder,
                SheetEdit::AddLine {
                    card: card.clone(),
                    inside: None,
                    line: Box::new(line),
                },
            ));
            assert_agrees(&format!("{file}: {item}"), expected.as_deref(), &before, &after);
        }
    }
    // Move to beside the Move by of the walkers and of mobile SIR, Move by beside the ants' Move to.
    assert!(greyed >= 3, "{greyed}");
}

#[test]
fn a_move_switched_to_the_other_mode_runs_unless_its_switch_is_greyed() {
    let mut checked = 0;
    for file in AGENT_SHEETS {
        let builder = builder_of(file);
        let document = builder.document();
        let before = errors(document);
        for card in &document.cards {
            let relations = CardRelations::new(document, card);
            for id in relations.order() {
                let Some(line) = relations.line(id).filter(|line| line.kind == LineKind::Move) else {
                    continue;
                };
                let mut switched = line.clone();
                switched.mode = Some(match line.mode {
                    Some(LineMode::To) => LineMode::By,
                    _ => LineMode::To,
                });
                let expected = refusals::change(document, &relations, &switched, Some(id));
                // The value of the other mode is of another type, so the switched line holds none and skips.
                switched.value = Slot::Blank;
                let mut changed = document.clone();
                for card in &mut changed.cards {
                    if let Some(known) = card.line_tables.get_mut(*id) {
                        *known = switched.clone();
                    }
                }
                let after = errors(&changed);
                assert_agrees(
                    &format!("{file}: {id} to {:?}", switched.mode),
                    expected.as_deref(),
                    &before,
                    &after,
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 3, "{checked}");
}

#[test]
fn walls_a_blocked_state_and_the_cell_ahead_are_greyed_exactly_where_the_build_refuses_them() {
    for file in AGENT_SHEETS {
        let builder = builder_of(file);
        let document = builder.document();
        let before = errors(document);
        let world = document.world;
        if world.edges == Edges::Wrap {
            let after = errors(&applied(
                &builder,
                SheetEdit::SetWorld {
                    width: world.width,
                    height: world.height,
                    edges: Edges::Walls,
                },
            ));
            assert_agrees(&format!("{file}: walls"), refusals::walls(document), &before, &after);
        }
        let (_, card) = agent_card(document);
        let mut ahead = SheetLine::new(LineKind::LookAtCell);
        ahead.which = Some(CellWhich::Ahead);
        let after = errors(&applied(
            &builder,
            SheetEdit::AddLine {
                card: card.clone(),
                inside: None,
                line: Box::new(ahead),
            },
        ));
        assert_agrees(
            &format!("{file}: the cell ahead"),
            refusals::cell_ahead(document),
            &before,
            &after,
        );
    }
    assert!(refusals::walls(&read(sheet_text("random_walk.toml"))).is_some());
    assert!(refusals::cell_ahead(&read(sheet_text("random_walk.toml"))).is_some());
    assert!(refusals::cell_ahead(&read(sheet_text("ants.toml"))).is_none());
}

#[test]
fn a_blocked_state_is_greyed_while_a_line_moves_agents_by_a_vector() {
    // The walkers move by a vector over a ground made on the sheet, and the ants move to places over theirs.
    let mut walk = builder_of("random_walk.toml");
    walk.apply(SheetEdit::AddProperty {
        thing: "ground".to_owned(),
        name: "Terrain".to_owned(),
        property_type: PropertyType::States,
    })
    .expect("a ground property");
    for name in ["Open", "Rock"] {
        walk.apply(SheetEdit::AddState {
            property: "terrain".to_owned(),
            name: name.to_owned(),
            color: None,
        })
        .expect("a state");
    }
    let mut start = StartsAs::new(StartRule::Value);
    start.value = Slot::Filled(SheetValue::Chip(SheetChip::State("open".to_owned())));
    walk.apply(SheetEdit::SetStartsAs {
        property: "terrain".to_owned(),
        starts_as: Some(Box::new(start)),
    })
    .expect("a start");
    assert!(errors(walk.document()).is_empty(), "{:#?}", errors(walk.document()));
    let ants = builder_of("ants.toml");
    for (name, builder, property, state) in [
        ("random walk", &walk, "terrain", "rock"),
        ("ants", &ants, "ground", "food"),
    ] {
        let document = builder.document();
        assert_eq!(document.world.made_of, MadeOf::Agents);
        let before = errors(document);
        let after = errors(&applied(
            builder,
            SheetEdit::SetBlocked {
                property: property.to_owned(),
                state: state.to_owned(),
                blocked: true,
            },
        ));
        assert_agrees(
            &format!("{name}: blocked {state}"),
            refusals::blocked_state(document),
            &before,
            &after,
        );
    }
    assert!(refusals::blocked_state(walk.document()).is_some());
    assert!(refusals::blocked_state(ants.document()).is_none());
}
