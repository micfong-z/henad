//! The sheet edit commands: the author format's three typical edits built by commands, the outline moves, renames and
//! new ids, line numbers, id minting, automatic names, the commands behind the card's offers, deletes that leave
//! skipped lines, and random command sequences over every sheet.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::authoring::primitives::rng::xorshift64;
use henad_core::metadata::TimeUnit;

use super::sheet_document::{sheet_text, sheets};
use super::sheet_writer::assert_hunks;
use crate::keys::KeyStream;
use crate::sheet::check;
use crate::sheet::document::{
    Around, BlockedState, ButtonStepRule, CardRuns, ChipRelation, CompareTest, Edges, LineKind, LookSummary, Marks,
    MeasureRule, PaintedShape, ParameterType, PropertyType, ShapeKind, SheetChip, SheetDocument, SheetLine,
    SheetLiteral, SheetTime, SheetValue, Slot, StartRule, StartsAs, StartsAsRow, SummaryKind,
};
use crate::sheet::edit::{
    IdSpace, LineField, MoveDirection, STATE_COLORS, SheetBuilder, SheetEdit, SheetEditError, SheetItem,
    automatic_name, automatic_summary_name, mint_id, unit_in_text,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{ConditionItem, Sentences};
use crate::sheet::{ParsedSheet, starters};

#[test]
fn a_new_property_type_clears_every_reference_and_keeps_the_place() {
    let mut builder = builder_of("game_of_life.toml");
    run(
        &mut builder,
        SheetEdit::SetPropertyType {
            property: "status".to_owned(),
            property_type: PropertyType::Number,
        },
    );
    let document = builder.document();
    let status = &document.cells.as_ref().expect("cells")[0];
    assert_eq!(status.id, "status", "the property keeps its place and its id");
    assert_eq!(status.property_type, PropertyType::Number);
    assert!(status.states.is_empty() && status.starts_as.is_none());
    assert!(document.measures.is_empty(), "the count of Alive goes with the states");
    assert!(
        !document.colors.contains_key("cells"),
        "a colour by state no longer fits a number"
    );
    // The Count line's "whose" test and each Become's new state named the property or its states.
    let Some(SheetValue::Phrase(whose)) = line(&builder, "l1").whose.value() else {
        panic!("the Count line keeps its test");
    };
    assert!(whose.left.is_blank() && whose.right.is_blank(), "{whose:?}");
    assert_eq!(line(&builder, "l2").new_state, None);
    assert_eq!(line(&builder, "l2").start_state, None);

    // Back to States: the property colours the cells by state again, with no states until some are added.
    run(
        &mut builder,
        SheetEdit::SetPropertyType {
            property: "status".to_owned(),
            property_type: PropertyType::States,
        },
    );
    let rule = builder.document().colors.get("cells").expect("a colour rule");
    assert_eq!(rule.property.as_deref(), Some("status"));
    // The same type again changes nothing.
    let before = builder.document().clone();
    run(
        &mut builder,
        SheetEdit::SetPropertyType {
            property: "status".to_owned(),
            property_type: PropertyType::States,
        },
    );
    assert!(builder.document() == &before);
    assert!(
        builder
            .apply(SheetEdit::SetPropertyType {
                property: "nothing".to_owned(),
                property_type: PropertyType::Number,
            })
            .is_err()
    );
}

/// Reads `text`, which must read.
fn read(text: &str) -> SheetDocument {
    ParsedSheet::parse(text)
        .unwrap_or_else(|error| panic!("the sheet does not read: {error}\n{text}"))
        .into_document()
}

/// Writes `document`, which must write.
fn write(document: &SheetDocument) -> String {
    document
        .to_text()
        .unwrap_or_else(|error| panic!("the sheet does not write: {error}"))
}

/// Returns a builder of the sheet `file_name`, with its keys seeded.
fn builder_of(file_name: &str) -> SheetBuilder {
    SheetBuilder::new(read(sheet_text(file_name))).with_key_seed(12)
}

/// Runs `edit` on `builder`, which must accept it, and returns the ids it made.
fn run(builder: &mut SheetBuilder, edit: SheetEdit) -> Vec<String> {
    let shown = format!("{edit:?}");
    builder
        .apply(edit)
        .unwrap_or_else(|error| panic!("{shown} is refused: {error}"))
}

/// Returns a chip value.
fn chip(chip: SheetChip) -> Slot {
    Slot::Filled(SheetValue::Chip(chip))
}

/// Returns a phrase value.
fn phrase(line: SheetLine) -> Slot {
    Slot::Filled(SheetValue::Phrase(Box::new(line)))
}

/// Returns the line `line_id` of `builder`'s sheet.
fn line<'a>(builder: &'a SheetBuilder, line_id: &str) -> &'a SheetLine {
    builder
        .document()
        .line(line_id)
        .map(|(_, line)| line)
        .unwrap_or_else(|| panic!("no line {line_id}"))
}

/// Returns the condition phrase of line `line_id`.
fn condition<'a>(builder: &'a SheetBuilder, line_id: &str) -> &'a SheetLine {
    match line(builder, line_id).condition.value() {
        Some(SheetValue::Phrase(phrase)) => phrase,
        other => panic!("line {line_id} has no condition phrase: {other:?}"),
    }
}

/// Returns the outline of the card `card` as text: each line id, with the lines inside a block in brackets.
fn outline(document: &SheetDocument, card: usize) -> String {
    fn walk(card: &crate::sheet::document::SheetCard, ids: &[String]) -> String {
        let parts: Vec<String> = ids
            .iter()
            .map(|id| match card.line_tables.get(id) {
                Some(line) if matches!(line.kind, LineKind::When | LineKind::Repeat) => {
                    format!("{id}[{}]", walk(card, &line.lines))
                }
                _ => id.clone(),
            })
            .collect();
        parts.join(" ")
    }
    let card = &document.cards[card];
    walk(card, &card.lines)
}

/// Adds the parameter Waning rate of the author format's diff to `builder`, as the New parameter dialog and the
/// parameter's row on the sheet would.
fn add_waning_rate(builder: &mut SheetBuilder) {
    let made = run(
        builder,
        SheetEdit::AddParameter {
            name: "Waning rate".to_owned(),
            parameter_type: ParameterType::Number,
            min: 0.0,
            max: 1.0,
            default: 0.01,
            unit: Some("per day".to_owned()),
        },
    );
    assert_eq!(made, ["waning_rate"]);
    run(
        builder,
        SheetEdit::SetParameter {
            parameter: "waning_rate".to_owned(),
            min: 0.0,
            max: 1.0,
            default: 0.01,
            step: Some(0.001),
            unit: Some("per day".to_owned()),
            means: Some("Rate at which a recovered cell loses its immunity".to_owned()),
        },
    );
}

#[test]
fn a_new_waning_line_built_by_commands_changes_four_hunks() {
    let old = sheet_text("sir.toml");
    let mut builder = builder_of("sir.toml");
    add_waning_rate(&mut builder);
    // "+ Chance", then "Become, at a rate": a Become whose condition is a rate with its blanks empty. The key is the
    // one the reference writer minted for the author format's diff.
    let mut become_line = SheetLine::new(LineKind::Become);
    become_line.key = Some(1_585_830_067);
    become_line.condition = phrase(SheetLine::new(LineKind::Rate));
    let made = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(become_line),
        },
    );
    assert_eq!(made, ["l5"], "line 4 sits in the New outbreak card");
    let rate = condition(&builder, "l5");
    assert_eq!(
        (rate.rate_unit, rate.rate_unit_from_tick),
        (Some(TimeUnit::Day), true),
        "a new rate takes the unit of one tick, marked"
    );
    run(
        &mut builder,
        SheetEdit::SetField {
            line: "l5".to_owned(),
            phrase: String::new(),
            field: LineField::NewState(Some(("health".to_owned(), "susceptible".to_owned()))),
        },
    );
    run(
        &mut builder,
        SheetEdit::SetField {
            line: "l5".to_owned(),
            phrase: String::new(),
            field: LineField::StartState(Some("recovered".to_owned())),
        },
    );
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: "l5".to_owned(),
            path: "condition.rate".to_owned(),
            value: chip(SheetChip::Param("waning_rate".to_owned())),
        },
    );
    let rate = condition(&builder, "l5");
    assert_eq!(
        (rate.rate_unit, rate.rate_unit_from_tick),
        (Some(TimeUnit::Day), false),
        "the parameter's unit replaces the mark"
    );
    let new = write(builder.document());
    assert_hunks(
        old,
        &new,
        &[
            (&["next_line = 5"], &["next_line = 6"]),
            (
                &[],
                &[
                    "[[parameter]]",
                    "id = \"waning_rate\"",
                    "name = \"Waning rate\"",
                    "type = \"number\"",
                    "min = 0",
                    "max = 1",
                    "default = 0.01",
                    "step = 0.001",
                    "unit = \"per day\"",
                    "means = \"Rate at which a recovered cell loses its immunity\"",
                ],
            ),
            (
                &["lines = [\"l1\", \"l2\", \"l3\"]"],
                &["lines = [\"l1\", \"l2\", \"l3\", \"l5\"]"],
            ),
            (
                &[],
                &[
                    "# Become Susceptible from Recovered, at rate Waning rate per day",
                    "[card.line.l5]",
                    "kind = \"become\"",
                    "key = 1585830067",
                    "property = \"health\"",
                    "new_state = \"susceptible\"",
                    "start_state = \"recovered\"",
                    "",
                    "[card.line.l5.condition]",
                    "kind = \"rate\"",
                    "rate = { param = \"waning_rate\" }",
                    "rate_unit = \"day\"",
                ],
            ),
        ],
    );
    assert_eq!(read(&new), *builder.document());
}

#[test]
fn a_steer_reorder_by_two_moves_changes_one_line() {
    let old = sheet_text("boids.toml");
    let mut builder = builder_of("boids.toml");
    for _ in 0..2 {
        run(
            &mut builder,
            SheetEdit::MoveLine {
                line: "l5".to_owned(),
                direction: MoveDirection::Up,
            },
        );
    }
    assert_hunks(
        old,
        &write(builder.document()),
        &[(
            &["lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]"],
            &["lines = [\"l1\", \"l2\", \"l5\", \"l3\", \"l4\", \"l6\", \"l7\"]"],
        )],
    );
}

#[test]
fn a_parameter_rename_by_command_changes_its_name_and_the_comment_that_shows_it() {
    let old = sheet_text("boids.toml");
    let mut builder = builder_of("boids.toml");
    run(
        &mut builder,
        SheetEdit::Rename {
            item: SheetItem::Parameter("cohesion".to_owned()),
            name: "Pull to centre".to_owned(),
        },
    );
    assert_hunks(
        old,
        &write(builder.document()),
        &[
            (&["name = \"Cohesion\""], &["name = \"Pull to centre\""]),
            (
                &["# Steer Velocity by Flockmates' centre, strength Cohesion"],
                &["# Steer Velocity by Flockmates' centre, strength Pull to centre"],
            ),
        ],
    );
}

#[test]
fn a_rename_changes_one_name_and_the_comments_that_show_it() {
    let old = sheet_text("sir.toml");
    let mut builder = builder_of("sir.toml");
    run(
        &mut builder,
        SheetEdit::Rename {
            item: SheetItem::Parameter("infection_rate".to_owned()),
            name: "Transmission rate".to_owned(),
        },
    );
    assert_hunks(
        old,
        &write(builder.document()),
        &[
            (&["name = \"Infection rate\""], &["name = \"Transmission rate\""]),
            (
                &["# Become Infected from Susceptible, at rate Infection rate per day for each infected neighbour"],
                &["# Become Infected from Susceptible, at rate Transmission rate per day for each infected neighbour"],
            ),
        ],
    );
    let refused = builder.apply(SheetEdit::Rename {
        item: SheetItem::Parameter("recovery_rate".to_owned()),
        name: "transmission RATE".to_owned(),
    });
    assert!(matches!(refused, Err(SheetEditError::Taken(_))), "{refused:?}");
}

#[test]
fn an_average_with_its_automatic_name_follows_a_rename_of_its_property() {
    let rename = |builder: &mut SheetBuilder, item: SheetItem, name: &str| {
        run(
            builder,
            SheetEdit::Rename {
                item,
                name: name.to_owned(),
            },
        );
    };
    let position = || SheetItem::Property("position".to_owned());
    let measure = |builder: &SheetBuilder| builder.document().measures[0].name.clone();

    let mut builder = builder_of("random_walk.toml");
    assert_eq!(measure(&builder), "Average of Position");
    rename(&mut builder, position(), "Place");
    assert_eq!(measure(&builder), "Average of Place");
    assert_eq!(builder.document().measures[0].id, "average_of_position");

    // A measure named by hand keeps its name.
    let mut builder = builder_of("random_walk.toml");
    rename(
        &mut builder,
        SheetItem::Measure("average_of_position".to_owned()),
        "Centre",
    );
    rename(&mut builder, position(), "Place");
    assert_eq!(measure(&builder), "Centre");

    // A follower whose new name another measure holds keeps its old name.
    let mut builder = builder_of("random_walk.toml");
    run(
        &mut builder,
        SheetEdit::AddMeasure {
            name: "Average of place".to_owned(),
            rule: MeasureRule::EventCount,
            over: None,
            property: None,
            state: None,
            event: None,
            value: Slot::Blank,
            color: None,
        },
    );
    rename(&mut builder, position(), "Place");
    assert_eq!(measure(&builder), "Average of Position");
}

#[test]
fn an_edit_that_would_keep_a_number_that_is_not_finite_is_refused() {
    let mut builder = builder_of("sir.toml");
    let before = builder.document().clone();
    let refusals = [
        builder.apply(SheetEdit::SetSlot {
            line: "l3".to_owned(),
            path: "condition.rate".to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(f64::INFINITY))),
        }),
        builder.apply(SheetEdit::SetTime {
            time: Some(SheetTime::new(f64::INFINITY, TimeUnit::Day)),
        }),
        builder.apply(SheetEdit::SetSlot {
            line: "l3".to_owned(),
            path: "condition.rate".to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Percent(f64::NAN))),
        }),
    ];
    let reasons: Vec<String> = refusals
        .into_iter()
        .map(|refusal| match refusal {
            Err(SheetEditError::Invalid(reason)) => reason,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        reasons,
        [
            "that number is too large to keep",
            "that number is too large to keep",
            "that number cannot be worked out",
        ]
    );
    assert_eq!(builder.document(), &before, "a refused edit changes nothing");
    let parameter = builder.apply(SheetEdit::SetParameter {
        parameter: "recovery_rate".to_owned(),
        min: 0.0,
        max: f64::INFINITY,
        default: 0.1,
        step: None,
        unit: None,
        means: None,
    });
    assert!(
        matches!(&parameter, Err(SheetEditError::Invalid(reason)) if reason == "that number is too large to keep"),
        "{parameter:?}"
    );
    assert!(builder.document().to_text().is_ok());
}

#[test]
fn change_id_rewrites_every_reference_and_nothing_else() {
    let cases: [(&str, SheetItem, &str, &str, &str); 5] = [
        (
            "sir.toml",
            SheetItem::Parameter("initial_infected".to_owned()),
            "starting_share",
            "\"initial_infected\"",
            "\"starting_share\"",
        ),
        (
            "sir.toml",
            SheetItem::Property("health".to_owned()),
            "wellbeing",
            "\"health\"",
            "\"wellbeing\"",
        ),
        (
            "sir.toml",
            SheetItem::Button("new_outbreak".to_owned()),
            "outbreak",
            "\"new_outbreak\"",
            "\"outbreak\"",
        ),
        (
            "vote.toml",
            SheetItem::State {
                property: "opinion".to_owned(),
                state: "b".to_owned(),
            },
            "choice_b",
            "\"b\"",
            "\"choice_b\"",
        ),
        (
            "ants.toml",
            SheetItem::Parameter("trail_falloff".to_owned()),
            "fade",
            "trail_falloff",
            "fade",
        ),
    ];
    for (file_name, item, id, old_text, new_text) in cases {
        let old = sheet_text(file_name);
        let mut builder = builder_of(file_name);
        run(
            &mut builder,
            SheetEdit::ChangeId {
                item: item.clone(),
                id: id.to_owned(),
            },
        );
        assert_eq!(
            write(builder.document()),
            old.replace(old_text, new_text),
            "{file_name}: {item:?}"
        );
    }
    // A kind of agent's new id reaches its card, its colour rule and the tick card named after it.
    let old = sheet_text("boids.toml");
    let mut builder = builder_of("boids.toml");
    run(
        &mut builder,
        SheetEdit::ChangeId {
            item: SheetItem::AgentKind("bird".to_owned()),
            id: "flier".to_owned(),
        },
    );
    let expected = old
        .replace("\"bird\"", "\"flier\"")
        .replace("[color.bird]", "[color.flier]")
        .replace("\"bird_tick\"", "\"flier_tick\"");
    assert_eq!(write(builder.document()), expected);
    let mut builder = builder_of("sir.toml");
    let taken = builder.apply(SheetEdit::ChangeId {
        item: SheetItem::Parameter("initial_infected".to_owned()),
        id: "health".to_owned(),
    });
    assert!(matches!(taken, Err(SheetEditError::Taken(_))), "{taken:?}");
    let reserved = builder.apply(SheetEdit::ChangeId {
        item: SheetItem::Parameter("initial_infected".to_owned()),
        id: "tick_length".to_owned(),
    });
    assert_eq!(reserved, Err(SheetEditError::Reserved("tick_length".to_owned())));
    let malformed = builder.apply(SheetEdit::ChangeId {
        item: SheetItem::Parameter("initial_infected".to_owned()),
        id: "Initial".to_owned(),
    });
    assert!(matches!(malformed, Err(SheetEditError::Invalid(_))), "{malformed:?}");
}

#[test]
fn a_deleted_line_number_is_never_minted_again() {
    let mut builder = builder_of("game_of_life.toml");
    let first = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(SheetLine::new(LineKind::Become)),
        },
    );
    assert_eq!(first, ["l5"]);
    run(&mut builder, SheetEdit::DeleteLine { line: "l5".to_owned() });
    let second = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(SheetLine::new(LineKind::Become)),
        },
    );
    assert_eq!(second, ["l6"]);
    assert_eq!(builder.document().sheet.next_line, 7);
    let text = write(builder.document());
    assert!(!text.contains("[card.line.l5]"), "{text}");
}

#[test]
fn id_minting_follows_its_known_answers() {
    let never = |_: &str| false;
    let known = [
        ("Infection rate", "infection_rate"),
        ("  Wind speed (m/s) ", "wind_speed_m_s"),
        ("3 by 3 block", "x3_by_3_block"),
        ("Über-density", "ber_density"),
        ("!!!", "x"),
        ("A", "a"),
    ];
    for (name, id) in known {
        assert_eq!(mint_id(name, IdSpace::Item, never).as_deref(), Ok(id), "{name}");
    }
    let long = "A very long name of a parameter that goes on and on past the limit";
    let minted = mint_id(long, IdSpace::Item, never).unwrap_or_else(|error| panic!("{error}"));
    assert!(minted.len() <= 48 && !minted.ends_with('_'), "{minted}");
    let clashing = mint_id(long, IdSpace::Item, |id| id == minted).unwrap_or_else(|error| panic!("{error}"));
    assert!(clashing.len() <= 48 && clashing.ends_with("_2"), "{clashing}");
    for (name, space, id) in [
        ("Grid width", IdSpace::Item, "grid_width"),
        ("Tick length", IdSpace::Item, "tick_length"),
        ("Henad speed", IdSpace::Item, "henad_speed"),
        ("Count", IdSpace::Item, "count"),
        ("L2", IdSpace::Card, "l2"),
        ("Cells", IdSpace::Thing, "cells"),
        ("Any state", IdSpace::State, "any_state"),
    ] {
        assert_eq!(
            mint_id(name, space, never),
            Err(SheetEditError::Reserved(id.to_owned())),
            "{name}"
        );
    }
    assert_eq!(mint_id("Count", IdSpace::State, never).as_deref(), Ok("count"));
    // A suffix that makes a reserved id is refused as the base would be.
    assert_eq!(mint_id("Henad", IdSpace::Item, never).as_deref(), Ok("henad"));
    assert_eq!(
        mint_id("Henad", IdSpace::Item, |id| id == "henad"),
        Err(SheetEditError::Reserved("henad_2".to_owned()))
    );

    // Through the commands: a parameter named as a property takes `_2`, and a reserved name is refused.
    let mut builder = builder_of("game_of_life.toml");
    let property = run(
        &mut builder,
        SheetEdit::AddProperty {
            thing: "cells".to_owned(),
            name: "Infection rate".to_owned(),
            property_type: PropertyType::Number,
        },
    );
    assert_eq!(property, ["infection_rate"]);
    let add_parameter = |name: &str| SheetEdit::AddParameter {
        name: name.to_owned(),
        parameter_type: ParameterType::Number,
        min: 0.0,
        max: 1.0,
        default: 0.1,
        unit: None,
    };
    assert_eq!(run(&mut builder, add_parameter("Infection rate")), ["infection_rate_2"]);
    assert_eq!(
        builder.apply(add_parameter("Grid width")),
        Err(SheetEditError::Reserved("grid_width".to_owned()))
    );
    assert!(matches!(
        builder.apply(add_parameter("infection RATE")),
        Err(SheetEditError::Taken(_))
    ));
}

#[test]
fn every_starter_id_follows_the_minting_rule() {
    for (file_name, text) in sheets() {
        let document = read(text);
        let mut items: BTreeSet<String> = BTreeSet::new();
        let mut things: BTreeSet<String> = BTreeSet::new();
        let mut buttons: BTreeSet<String> = BTreeSet::new();
        let check_id = |taken: &mut BTreeSet<String>, space: IdSpace, name: &str, id: &str| {
            let minted = mint_id(name, space, |held| taken.contains(held))
                .unwrap_or_else(|error| panic!("{file_name}: '{name}': {error}"));
            assert_eq!(minted, id, "{file_name}: the id of '{name}'");
            taken.insert(minted);
        };
        for kind in &document.agents {
            check_id(&mut things, IdSpace::Thing, &kind.name, &kind.id);
        }
        let properties = document
            .cells
            .iter()
            .flatten()
            .chain(document.agents.iter().flat_map(|kind| &kind.properties))
            .chain(document.ground.iter().flat_map(|ground| &ground.properties));
        for property in properties {
            check_id(&mut items, IdSpace::Item, &property.name, &property.id);
            let mut states = BTreeSet::new();
            for state in &property.states {
                check_id(&mut states, IdSpace::State, &state.name, &state.id);
            }
        }
        for event in &document.events {
            check_id(&mut items, IdSpace::Item, &event.name, &event.id);
        }
        for parameter in &document.parameters {
            check_id(&mut items, IdSpace::Item, &parameter.name, &parameter.id);
        }
        for measure in &document.measures {
            check_id(&mut items, IdSpace::Item, &measure.name, &measure.id);
        }
        for button in &document.buttons {
            check_id(&mut buttons, IdSpace::Button, &button.name, &button.id);
        }
        let mut number = 0;
        for card in &document.cards {
            let expected = match card.runs {
                CardRuns::EachTick if card.every == "cells" => "cell_tick".to_owned(),
                CardRuns::EachTick => format!("{}_tick", card.every),
                _ => document
                    .buttons
                    .iter()
                    .find(|button| {
                        button
                            .does
                            .iter()
                            .any(|step| step.card.as_deref() == Some(card.id.as_str()))
                    })
                    .map_or_else(
                        || panic!("{file_name}: no button runs {}", card.id),
                        |button| button.id.clone(),
                    ),
            };
            assert_eq!(card.id, expected, "{file_name}: the card over {}", card.every);
            for id in CardRelations::new(&document, card).order() {
                number += 1;
                assert_eq!(
                    *id,
                    format!("l{number}"),
                    "{file_name}: lines are numbered in creation order"
                );
            }
        }
        assert_eq!(
            document.sheet.next_line,
            number + 1,
            "{file_name}: next_line follows the highest line"
        );
    }
}

#[test]
fn a_seeded_key_stream_gives_its_known_first_keys() {
    let mut stream = KeyStream::seeded(1);
    let keys = [stream.next_key(), stream.next_key(), stream.next_key()];
    assert_eq!(keys, [605_988_705, 180_871_622, 1_319_607_719]);
}

#[test]
fn every_outline_move_matches_the_prototype() {
    let mut builder = builder_of("game_of_life.toml");
    let add = |builder: &mut SheetBuilder, inside: Option<&str>, kind: LineKind| -> String {
        let made = run(
            builder,
            SheetEdit::AddLine {
                card: "cell_tick".to_owned(),
                inside: inside.map(str::to_owned),
                line: Box::new(SheetLine::new(kind)),
            },
        );
        made.into_iter().next().unwrap_or_default()
    };
    assert_eq!(add(&mut builder, None, LineKind::When), "l5");
    assert_eq!(add(&mut builder, Some("l5"), LineKind::Become), "l6");
    assert_eq!(add(&mut builder, Some("l5"), LineKind::When), "l7");
    assert_eq!(add(&mut builder, Some("l7"), LineKind::Become), "l8");
    let shape = |builder: &SheetBuilder| outline(builder.document(), 0);
    assert_eq!(shape(&builder), "l1 l2 l3 l5[l6 l7[l8]]");
    let steps: [(&str, MoveDirection, &str); 8] = [
        // A line just above a When moves into it, at its start.
        ("l3", MoveDirection::Down, "l1 l2 l5[l3 l6 l7[l8]]"),
        // The first line inside a When moves up out of it.
        ("l3", MoveDirection::Up, "l1 l2 l3 l5[l6 l7[l8]]"),
        // The last line inside a When moves down out of it, into the When around it.
        ("l8", MoveDirection::Down, "l1 l2 l3 l5[l6 l7[] l8]"),
        // A line just below a When moves into it, at its end.
        ("l8", MoveDirection::Up, "l1 l2 l3 l5[l6 l7[l8]]"),
        // A When moves with its lines and swaps with the line beside it.
        ("l7", MoveDirection::Up, "l1 l2 l3 l5[l7[l8] l6]"),
        ("l5", MoveDirection::Up, "l1 l2 l5[l7[l8] l6] l3"),
        ("l7", MoveDirection::Down, "l1 l2 l5[l6 l7[l8]] l3"),
        ("l5", MoveDirection::Down, "l1 l2 l3 l5[l6 l7[l8]]"),
    ];
    let move_line = |builder: &mut SheetBuilder, line_id: &str, direction: MoveDirection, expected: &str| {
        run(
            builder,
            SheetEdit::MoveLine {
                line: line_id.to_owned(),
                direction,
            },
        );
        assert_eq!(shape(builder), expected, "{line_id} {direction:?}");
    };
    for (line_id, direction, expected) in steps {
        move_line(&mut builder, line_id, direction, expected);
    }
    // A When swaps with a When beside it, and never moves into it, at the top of the card and inside a When.
    assert_eq!(add(&mut builder, None, LineKind::When), "l9");
    assert_eq!(add(&mut builder, Some("l5"), LineKind::When), "l10");
    assert_eq!(shape(&builder), "l1 l2 l3 l5[l6 l7[l8] l10[]] l9[]");
    let when_steps: [(&str, MoveDirection, &str); 6] = [
        ("l9", MoveDirection::Up, "l1 l2 l3 l9[] l5[l6 l7[l8] l10[]]"),
        ("l9", MoveDirection::Down, "l1 l2 l3 l5[l6 l7[l8] l10[]] l9[]"),
        ("l5", MoveDirection::Down, "l1 l2 l3 l9[] l5[l6 l7[l8] l10[]]"),
        ("l5", MoveDirection::Up, "l1 l2 l3 l5[l6 l7[l8] l10[]] l9[]"),
        ("l10", MoveDirection::Up, "l1 l2 l3 l5[l6 l10[] l7[l8]] l9[]"),
        ("l7", MoveDirection::Up, "l1 l2 l3 l5[l6 l7[l8] l10[]] l9[]"),
    ];
    for (line_id, direction, expected) in when_steps {
        move_line(&mut builder, line_id, direction, expected);
    }
    for (line_id, direction) in [("l1", MoveDirection::Up), ("l9", MoveDirection::Down)] {
        let refused = builder.apply(SheetEdit::MoveLine {
            line: line_id.to_owned(),
            direction,
        });
        assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    }
    for empty in ["l9", "l10"] {
        run(&mut builder, SheetEdit::DeleteLine { line: empty.to_owned() });
    }
    // Deleting a When keeps its lines one level up, in its place.
    run(&mut builder, SheetEdit::DeleteLine { line: "l5".to_owned() });
    assert_eq!(shape(&builder), "l1 l2 l3 l6 l7[l8]");
    run(&mut builder, SheetEdit::DeleteLine { line: "l7".to_owned() });
    assert_eq!(shape(&builder), "l1 l2 l3 l6 l8");
    assert_eq!(read(&write(builder.document())), *builder.document());
}

#[test]
fn a_change_added_inside_a_when_starts_always_and_takes_the_tested_state() {
    let mut builder = builder_of("game_of_life.toml");
    let mut when = SheetLine::new(LineKind::When);
    let mut test = SheetLine::new(LineKind::Compare);
    test.left = chip(SheetChip::My("status".to_owned()));
    test.test = Some(CompareTest::Is);
    test.right = chip(SheetChip::State("alive".to_owned()));
    when.condition = phrase(test);
    run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(when),
        },
    );
    run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: Some("l5".to_owned()),
            line: Box::new(SheetLine::new(LineKind::Become)),
        },
    );
    let added = line(&builder, "l6");
    assert_eq!(condition(&builder, "l6").kind, LineKind::Always);
    assert_eq!(added.property.as_deref(), Some("status"));
    assert_eq!(added.start_state.as_deref(), Some("alive"));
    let refused = builder.apply(SheetEdit::AddLine {
        card: "cell_tick".to_owned(),
        inside: Some("l1".to_owned()),
        line: Box::new(SheetLine::new(LineKind::Become)),
    });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
}

#[test]
fn automatic_names_follow_their_lines_until_renamed() {
    let mut builder = builder_of("forest_fire.toml");
    let add_chance = |builder: &mut SheetBuilder, chance: Slot| -> String {
        let mut line = SheetLine::new(LineKind::Chance);
        line.chance = chance;
        run(
            builder,
            SheetEdit::AddLine {
                card: "cell_tick".to_owned(),
                inside: None,
                line: Box::new(line),
            },
        )
        .into_iter()
        .next()
        .unwrap_or_default()
    };
    let name = |builder: &SheetBuilder, line_id: &str| line(builder, line_id).name.clone().unwrap_or_default();
    let empty = add_chance(&mut builder, Slot::Blank);
    assert_eq!(name(&builder, &empty), "Happens");
    let lightning = add_chance(&mut builder, chip(SheetChip::Param("lightning_chance".to_owned())));
    assert_eq!(name(&builder, &lightning), "Lightning happens");
    let again = add_chance(&mut builder, chip(SheetChip::Param("lightning_chance".to_owned())));
    assert_eq!(name(&builder, &again), "Lightning happens 2");
    let percent = add_chance(
        &mut builder,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Percent(30.0))),
    );
    assert_eq!(name(&builder, &percent), "30% chance happens");
    let typed = add_chance(
        &mut builder,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.3))),
    );
    assert_eq!(name(&builder, &typed), "0.3 chance happens");
    // The empty line follows its blank once it is filled.
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: empty.clone(),
            path: "chance".to_owned(),
            value: chip(SheetChip::Param("growth_chance".to_owned())),
        },
    );
    assert_eq!(name(&builder, &empty), "Growth happens");
    // A name typed by hand stays through later edits, and frees its automatic name for the next line.
    run(
        &mut builder,
        SheetEdit::RenameResult {
            line: lightning.clone(),
            name: "Strike".to_owned(),
        },
    );
    assert_eq!(name(&builder, &again), "Lightning happens");
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: lightning.clone(),
            path: "chance".to_owned(),
            value: chip(SheetChip::Param("growth_chance".to_owned())),
        },
    );
    assert_eq!(name(&builder, &lightning), "Strike");
    let taken = builder.apply(SheetEdit::RenameResult {
        line: typed,
        name: "strike".to_owned(),
    });
    assert!(matches!(taken, Err(SheetEditError::Taken(_))), "{taken:?}");
    // A count's name follows the state it counts.
    run(
        &mut builder,
        SheetEdit::Rename {
            item: SheetItem::State {
                property: "cover".to_owned(),
                state: "burning".to_owned(),
            },
            name: "On fire".to_owned(),
        },
    );
    assert_eq!(name(&builder, "l1"), "On fire neighbours");
    // The starters' names are their automatic names where a line has one.
    for starter in starters() {
        let document = read(starter.text);
        for card in &document.cards {
            for line in card.line_tables.values() {
                if let Some(automatic) = automatic_name(&document, line) {
                    assert_eq!(line.name.as_deref(), Some(automatic.as_str()), "{}", starter.file_name);
                }
            }
        }
    }
}

#[test]
fn the_condition_offers_of_the_final_editor_are_one_command_each() {
    let mut builder = builder_of("vote.toml");
    // "is at most" on a comparison keeps its blanks and sets its test.
    run(
        &mut builder,
        SheetEdit::ChooseCondition {
            line: "l2".to_owned(),
            item: ConditionItem::AtMost,
        },
    );
    let test = condition(&builder, "l2");
    assert_eq!(test.test, Some(CompareTest::AtMost));
    assert!(!test.left.is_blank() && !test.right.is_blank());
    // The outside form, with its upper number only, rewrites to "is more than".
    run(
        &mut builder,
        SheetEdit::ChooseCondition {
            line: "l2".to_owned(),
            item: ConditionItem::Outside,
        },
    );
    let outside = condition(&builder, "l2");
    assert_eq!(outside.test, Some(CompareTest::Outside));
    assert!(outside.left.is_blank() && outside.low.is_blank() && outside.high.is_blank());
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: "l2".to_owned(),
            path: "condition.left".to_owned(),
            value: chip(SheetChip::Result {
                line: "l1".to_owned(),
                part: None,
                property: None,
            }),
        },
    );
    let refused = builder.apply(SheetEdit::UseOneSide { line: "l2".to_owned() });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: "l2".to_owned(),
            path: "condition.high".to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(4.0))),
        },
    );
    let sentences = |builder: &SheetBuilder| Sentences::new(builder.document()).line("l2", &[]);
    assert_eq!(
        sentences(&builder),
        "Become B from any state, when B in my block is less than ... or more than 4"
    );
    run(&mut builder, SheetEdit::UseOneSide { line: "l2".to_owned() });
    assert_eq!(
        sentences(&builder),
        "Become B from any state, when B in my block is more than 4"
    );
    // A When cannot be always.
    let mut when = SheetLine::new(LineKind::When);
    when.condition = phrase(SheetLine::new(LineKind::Compare));
    let when_id = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(when),
        },
    );
    let refused = builder.apply(SheetEdit::ChooseCondition {
        line: when_id[0].clone(),
        item: ConditionItem::Always,
    });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
}

#[test]
fn a_count_moves_into_for_each_and_a_when_folds_into_its_change() {
    let mut builder = builder_of("sir.toml");
    // Line 2 at rate Infected neighbours: the offer moves the count into "for each" and empties the rate.
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: "l2".to_owned(),
            path: "condition.tries".to_owned(),
            value: Slot::Blank,
        },
    );
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: "l2".to_owned(),
            path: "condition.rate".to_owned(),
            value: chip(SheetChip::Result {
                line: "l1".to_owned(),
                part: None,
                property: None,
            }),
        },
    );
    run(&mut builder, SheetEdit::UseCountForEach { line: "l2".to_owned() });
    let rate = condition(&builder, "l2");
    assert!(rate.rate.is_blank());
    assert_eq!(
        rate.tries,
        chip(SheetChip::Result {
            line: "l1".to_owned(),
            part: None,
            property: None
        })
    );
    let refused = builder.apply(SheetEdit::UseCountForEach { line: "l3".to_owned() });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    // "Move it into line N": a When around one change gives the change its condition and goes.
    let mut when = SheetLine::new(LineKind::When);
    let mut test = SheetLine::new(LineKind::Compare);
    test.left = chip(SheetChip::My("health".to_owned()));
    test.test = Some(CompareTest::Is);
    test.right = chip(SheetChip::State("recovered".to_owned()));
    when.condition = phrase(test.clone());
    let when_id = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(when),
        },
    );
    let inner = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: Some(when_id[0].clone()),
            line: Box::new(SheetLine::new(LineKind::Become)),
        },
    );
    // A change with a condition of its own keeps it: the offer is refused.
    run(
        &mut builder,
        SheetEdit::ChooseCondition {
            line: inner[0].clone(),
            item: ConditionItem::WithChance,
        },
    );
    let refused = builder.apply(SheetEdit::MoveConditionInto {
        when: when_id[0].clone(),
    });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    run(
        &mut builder,
        SheetEdit::ChooseCondition {
            line: inner[0].clone(),
            item: ConditionItem::Always,
        },
    );
    run(
        &mut builder,
        SheetEdit::MoveConditionInto {
            when: when_id[0].clone(),
        },
    );
    assert_eq!(outline(builder.document(), 0), format!("l1 l2 l3 {}", inner[0]));
    assert_eq!(*condition(&builder, &inner[0]), test);
}

#[test]
fn a_delete_leaves_skipped_lines_and_never_an_error() {
    let deletes: [(&str, SheetItem, &[&str]); 7] = [
        ("sir.toml", SheetItem::Parameter("infection_rate".to_owned()), &["l2"]),
        (
            "game_of_life.toml",
            SheetItem::Property("status".to_owned()),
            &["l1", "l2", "l3", "l4"],
        ),
        (
            "sir.toml",
            SheetItem::State {
                property: "health".to_owned(),
                state: "infected".to_owned(),
            },
            &["l1", "l2", "l3", "l4"],
        ),
        (
            "ants.toml",
            SheetItem::Parameter("trail_falloff".to_owned()),
            &["l3", "l4"],
        ),
        (
            "forest_fire.toml",
            SheetItem::Parameter("growth_chance".to_owned()),
            &[],
        ),
        // A start on the ground and a start with a state value fall back on the first state left.
        (
            "ants.toml",
            SheetItem::State {
                property: "ground".to_owned(),
                state: "nest".to_owned(),
            },
            &[],
        ),
        (
            "ants.toml",
            SheetItem::State {
                property: "cargo".to_owned(),
                state: "searching".to_owned(),
            },
            &[],
        ),
    ];
    for (file_name, item, skipped) in deletes {
        let mut builder = builder_of(file_name);
        run(&mut builder, SheetEdit::Delete { item: item.clone() });
        let text = write(builder.document());
        let report = ParsedSheet::parse(&text)
            .unwrap_or_else(|error| panic!("{file_name}: {error}"))
            .check();
        let errors: Vec<String> = report.errors().map(|error| error.diagnostic.message.clone()).collect();
        assert!(errors.is_empty(), "{file_name}: {item:?}: {errors:?}");
        for line_id in skipped {
            assert!(
                report.is_skipped(line_id),
                "{file_name}: {item:?}: {line_id} still runs"
            );
        }
    }
    // "All tree", the usual start of a States property, falls back on the first state left, Empty.
    let mut builder = builder_of("forest_fire.toml");
    let mut all_trees = StartsAs::new(StartRule::Value);
    all_trees.value = chip(SheetChip::State("tree".to_owned()));
    run(
        &mut builder,
        SheetEdit::SetStartsAs {
            property: "cover".to_owned(),
            starts_as: Some(Box::new(all_trees)),
        },
    );
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::State {
                property: "cover".to_owned(),
                state: "tree".to_owned(),
            },
        },
    );
    let start = builder
        .document()
        .cells
        .as_ref()
        .and_then(|cells| cells[0].starts_as.clone())
        .map(|starts| starts.value);
    assert_eq!(start, Some(chip(SheetChip::State("empty".to_owned()))));
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    // A deleted line's result leaves a blank, and the line that used it is skipped.
    let mut builder = builder_of("game_of_life.toml");
    run(&mut builder, SheetEdit::DeleteLine { line: "l1".to_owned() });
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(report.is_skipped("l2") && report.is_skipped("l3"));
    // A deleted button takes the card it runs.
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::Button("clear".to_owned()),
        },
    );
    assert!(builder.document().cards.iter().all(|card| card.id != "clear"));
    assert!(!check::check(builder.document()).has_errors());
}

#[test]
fn colours_follow_their_states_and_new_states_take_the_defaults() {
    let mut builder = builder_of("sir.toml");
    let green = [0x14, 0xAE, 0x52, 0xFF];
    run(
        &mut builder,
        SheetEdit::Recolor {
            item: SheetItem::State {
                property: "health".to_owned(),
                state: "recovered".to_owned(),
            },
            color: green,
        },
    );
    let measure = |builder: &SheetBuilder, id: &str| {
        builder
            .document()
            .measures
            .iter()
            .find(|measure| measure.id == id)
            .map(|measure| measure.color)
    };
    assert_eq!(measure(&builder, "recovered"), Some(green));
    assert_eq!(measure(&builder, "infected"), Some([0xE4, 0x37, 0x48, 0xFF]));

    let mut builder = builder_of("game_of_life.toml");
    let made = run(
        &mut builder,
        SheetEdit::AddProperty {
            thing: "cells".to_owned(),
            name: "Health".to_owned(),
            property_type: PropertyType::States,
        },
    );
    for name in ["Susceptible", "Infected", "Recovered"] {
        run(
            &mut builder,
            SheetEdit::AddState {
                property: made[0].clone(),
                name: name.to_owned(),
                color: None,
            },
        );
    }
    let colors: Vec<_> = builder
        .document()
        .cells
        .iter()
        .flatten()
        .find(|property| property.id == "health")
        .map(|property| property.states.iter().map(|state| state.color).collect())
        .unwrap_or_default();
    assert_eq!(colors, STATE_COLORS[..3]);
    let measure = run(
        &mut builder,
        SheetEdit::AddMeasure {
            name: "Infected".to_owned(),
            rule: MeasureRule::CountInState,
            over: Some("cells".to_owned()),
            property: Some("health".to_owned()),
            state: Some("infected".to_owned()),
            event: None,
            value: Slot::Blank,
            color: None,
        },
    );
    assert_eq!(
        builder.document().measures.last().map(|known| known.color),
        Some(STATE_COLORS[1])
    );
    assert_eq!(measure, ["infected"]);
}

#[test]
fn a_rate_unit_comes_from_the_tick_until_chosen() {
    let mut builder = builder_of("sir.toml");
    let mut rate = SheetLine::new(LineKind::Rate);
    rate.rate = chip(SheetChip::Param("recovery_rate".to_owned()));
    let made = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(rate),
        },
    );
    let added = line(&builder, &made[0]);
    assert_eq!(
        (added.rate_unit, added.rate_unit_from_tick),
        (Some(TimeUnit::Day), true)
    );
    assert!(added.key.is_some(), "a line that draws takes a key");
    let key = added.key;
    run(
        &mut builder,
        SheetEdit::SetField {
            line: made[0].clone(),
            phrase: String::new(),
            field: LineField::RateUnit(Some(TimeUnit::Week)),
        },
    );
    let added = line(&builder, &made[0]);
    assert_eq!(
        (added.rate_unit, added.rate_unit_from_tick),
        (Some(TimeUnit::Week), false)
    );
    assert_eq!(added.key, key, "the key stays for the life of the line");
    run(&mut builder, SheetEdit::DrawNewNumbers { line: made[0].clone() });
    assert_ne!(line(&builder, &made[0]).key, key);

    // With no length of one tick, a parameter's unit sets one tick to 1 of that unit, and a parameter with no unit
    // takes the rate's.
    let mut builder = builder_of("sir.toml");
    run(&mut builder, SheetEdit::SetTime { time: None });
    let made = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(SheetLine::new(LineKind::Rate)),
        },
    );
    assert_eq!(line(&builder, &made[0]).rate_unit, None);
    run(
        &mut builder,
        SheetEdit::SetParameter {
            parameter: "recovery_rate".to_owned(),
            min: 0.0,
            max: 5.0,
            default: 0.1,
            step: None,
            unit: Some("per week per infected neighbour".to_owned()),
            means: None,
        },
    );
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: made[0].clone(),
            path: "rate".to_owned(),
            value: chip(SheetChip::Param("recovery_rate".to_owned())),
        },
    );
    assert_eq!(line(&builder, &made[0]).rate_unit, Some(TimeUnit::Week));
    assert_eq!(builder.document().time, Some(SheetTime::new(1.0, TimeUnit::Week)));
}

#[test]
fn a_parameter_with_no_unit_takes_the_rate_unit() {
    // Put into a rate, the parameter takes the rate's unit and leaves the rate's unit and its mark alone.
    let mut builder = builder_of("sir.toml");
    run(
        &mut builder,
        SheetEdit::AddParameter {
            name: "Plain".to_owned(),
            parameter_type: ParameterType::Number,
            min: 0.0,
            max: 1.0,
            default: 0.5,
            unit: None,
        },
    );
    let made = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(SheetLine::new(LineKind::Rate)),
        },
    );
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: made[0].clone(),
            path: "rate".to_owned(),
            value: chip(SheetChip::Param("plain".to_owned())),
        },
    );
    let plain_unit = |builder: &SheetBuilder| {
        builder
            .document()
            .parameters
            .iter()
            .find(|parameter| parameter.id == "plain")
            .and_then(|parameter| parameter.unit.clone())
    };
    assert_eq!(plain_unit(&builder).as_deref(), Some("per day"));
    assert_eq!(
        (
            line(&builder, &made[0]).rate_unit,
            line(&builder, &made[0]).rate_unit_from_tick
        ),
        (Some(TimeUnit::Day), true)
    );

    // A unit chosen by hand for a rate that reads a parameter with no unit gives the parameter that unit.
    let mut builder = builder_of("sir.toml");
    run(
        &mut builder,
        SheetEdit::SetParameter {
            parameter: "recovery_rate".to_owned(),
            min: 0.0,
            max: 1.0,
            default: 0.1,
            step: None,
            unit: None,
            means: None,
        },
    );
    let recovery_unit = |builder: &SheetBuilder| {
        builder
            .document()
            .parameters
            .iter()
            .find(|parameter| parameter.id == "recovery_rate")
            .and_then(|parameter| parameter.unit.clone())
    };
    assert_eq!(recovery_unit(&builder), None);
    run(
        &mut builder,
        SheetEdit::SetField {
            line: "l3".to_owned(),
            phrase: "condition".to_owned(),
            field: LineField::RateUnit(Some(TimeUnit::Week)),
        },
    );
    assert_eq!(recovery_unit(&builder).as_deref(), Some("per week"));
}

#[test]
fn a_parameter_unit_names_its_unit_of_time_as_the_final_editor_reads_it() {
    let cases: [(&str, Option<TimeUnit>); 9] = [
        ("per day", Some(TimeUnit::Day)),
        ("Per day", Some(TimeUnit::Day)),
        ("per days", Some(TimeUnit::Day)),
        ("PER   Week", Some(TimeUnit::Week)),
        ("per daylight", None),
        ("perday", None),
        ("super day", None),
        ("people per hour per infected neighbour", Some(TimeUnit::Hour)),
        ("per year, then per day", Some(TimeUnit::Year)),
    ];
    for (text, unit) in cases {
        assert_eq!(unit_in_text(text), unit, "{text}");
    }
}

#[test]
fn a_starts_as_commit_is_one_edit_and_keeps_its_key() {
    let mut builder = builder_of("game_of_life.toml");
    let key = builder
        .document()
        .cells
        .as_ref()
        .and_then(|cells| cells[0].starts_as.as_ref()?.key);
    assert!(key.is_some());
    let mut starts = StartsAs::new(StartRule::ByChance);
    starts.otherwise = Some("dead".to_owned());
    starts.rows.push(StartsAsRow::new(
        "alive",
        Slot::Filled(SheetValue::Literal(SheetLiteral::Percent(30.0))),
    ));
    run(
        &mut builder,
        SheetEdit::SetStartsAs {
            property: "status".to_owned(),
            starts_as: Some(Box::new(starts)),
        },
    );
    let stored = builder
        .document()
        .cells
        .as_ref()
        .and_then(|cells| cells[0].starts_as.clone());
    assert_eq!(stored.as_ref().and_then(|starts| starts.key), key);
    let text = write(builder.document());
    assert!(text.contains("chance = { percent = 30 }"), "{text}");
    assert!(!check::check(builder.document()).has_errors());

    // A start on the ground draws nothing and takes no key.
    let mut builder = builder_of("ants.toml");
    let mut on_ground = StartsAs::new(StartRule::OnGround);
    on_ground.property = Some("ground".to_owned());
    on_ground.state = Some("food".to_owned());
    run(
        &mut builder,
        SheetEdit::SetStartsAs {
            property: "position".to_owned(),
            starts_as: Some(Box::new(on_ground)),
        },
    );
    let position = builder
        .document()
        .agents
        .iter()
        .flat_map(|kind| &kind.properties)
        .find(|property| property.id == "position")
        .and_then(|property| property.starts_as.clone());
    assert_eq!(position.map(|starts| starts.key), Some(None));
    assert!(!check::check(builder.document()).has_errors());

    // A start whose template holds a key that a line already holds takes a fresh one.
    let mut builder = builder_of("sir.toml");
    let held = line(&builder, "l2").key;
    let mut starts = StartsAs::new(StartRule::ByChance);
    starts.key = held;
    starts.otherwise = Some("susceptible".to_owned());
    run(
        &mut builder,
        SheetEdit::SetStartsAs {
            property: "health".to_owned(),
            starts_as: Some(Box::new(starts)),
        },
    );
    let key = builder
        .document()
        .cells
        .as_ref()
        .and_then(|cells| cells[0].starts_as.as_ref()?.key);
    assert!(key.is_some() && key != held, "{key:?}");
    assert!(!check::check(builder.document()).has_errors());
}

/// Returns the key of the one draw of the start of the ants' Position in `document`, lowered.
fn ant_start_draw_key(document: &SheetDocument) -> crate::value::ConfigValue {
    let file = crate::sheet::SheetFile::parse(&write(document))
        .unwrap_or_else(|error| panic!("the sheet does not lower: {error}"));
    let start = file
        .lowered()
        .document()
        .passes
        .iter()
        .find(|pass| pass.id == "start_ant")
        .expect("the start of the ants");
    let mut draws = start.body.nodes.iter().filter(|node| node.op == "rand.float");
    let draw = draws.next().expect("the start draws a nest cell");
    assert!(draws.next().is_none(), "the start draws once");
    draw.config["key"].clone()
}

/// A start on a ground state painted on two cells draws one of them. The checker warns while it holds no key, a
/// second nest cell painted by a command gives it one, and its draws then keep their key across a new id of Position.
#[test]
fn a_start_on_two_painted_cells_takes_a_key() {
    let mut builder = builder_of("ants.toml");
    let mut painted = builder
        .document()
        .ground
        .as_ref()
        .and_then(|ground| ground.properties.first()?.starts_as.clone())
        .expect("the ants' ground is painted");
    let mut nest = PaintedShape::new(ShapeKind::Cell, "nest");
    nest.center = Some([0.125, 0.875]);
    painted.shapes.push(nest);
    let mut unkeyed = builder.document().clone();
    if let Some(ground) = unkeyed.ground.as_mut() {
        ground.properties[0].starts_as = Some(painted.clone());
    }
    let warnings = check::check(&unkeyed)
        .warnings()
        .map(|warning| warning.diagnostic.message.clone())
        .collect::<Vec<_>>();
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("the start of property 'position' draws and holds no key")),
        "{warnings:#?}"
    );
    run(
        &mut builder,
        SheetEdit::SetStartsAs {
            property: "ground".to_owned(),
            starts_as: Some(Box::new(painted)),
        },
    );
    let position = |document: &SheetDocument, id: &str| {
        document
            .agents
            .iter()
            .flat_map(|kind| &kind.properties)
            .find(|property| property.id == id)
            .and_then(|property| property.starts_as.as_ref()?.key)
    };
    assert!(position(builder.document(), "position").is_some());
    let report = check::check(builder.document());
    assert!(!report.has_errors());
    assert_eq!(report.warnings().count(), 0);
    let key = ant_start_draw_key(builder.document());
    run(
        &mut builder,
        SheetEdit::ChangeId {
            item: SheetItem::Property("position".to_owned()),
            id: "place".to_owned(),
        },
    );
    assert!(position(builder.document(), "place").is_some());
    assert_eq!(ant_start_draw_key(builder.document()), key);
}

/// Returns a number below `bound` from `rng`.
fn below(rng: &mut u64, bound: usize) -> usize {
    *rng = xorshift64(*rng);
    usize::try_from(*rng >> 33).unwrap_or(0) % bound.max(1)
}

/// Returns one of `choices` from `rng`.
fn pick<'a, T>(rng: &mut u64, choices: &'a [T]) -> Option<&'a T> {
    if choices.is_empty() {
        None
    } else {
        choices.get(below(rng, choices.len()))
    }
}

/// Returns a random value for a slot of `document`.
fn random_value(rng: &mut u64, document: &SheetDocument, lines: &[String]) -> Slot {
    let parameters: Vec<String> = document
        .parameters
        .iter()
        .map(|parameter| parameter.id.clone())
        .collect();
    let properties: Vec<String> = document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .map(|property| property.id.clone())
        .collect();
    let states: Vec<String> = document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .flat_map(|property| property.states.iter().map(|state| state.id.clone()))
        .collect();
    match below(rng, 9) {
        0 => Slot::Blank,
        1 => Slot::Filled(SheetValue::Literal(SheetLiteral::Number(below(rng, 100) as f64 / 10.0))),
        2 => Slot::Filled(SheetValue::Literal(SheetLiteral::Percent(below(rng, 100) as f64))),
        3 => pick(rng, &parameters).map_or(Slot::Blank, |id| chip(SheetChip::Param(id.clone()))),
        4 => pick(rng, &properties).map_or(Slot::Blank, |id| chip(SheetChip::My(id.clone()))),
        5 => pick(rng, &states).map_or(Slot::Blank, |id| chip(SheetChip::State(id.clone()))),
        6 => pick(rng, lines).map_or(Slot::Blank, |id| {
            chip(SheetChip::Result {
                line: id.clone(),
                part: None,
                property: None,
            })
        }),
        7 => {
            let kinds = [LineKind::Chance, LineKind::Rate, LineKind::Compare, LineKind::Always];
            let mut inner = SheetLine::new(kinds[below(rng, kinds.len())]);
            if inner.kind == LineKind::Compare {
                inner.test = Some(CompareTest::Is);
            }
            phrase(inner)
        }
        _ => Slot::Filled(SheetValue::List(vec![SheetValue::Literal(SheetLiteral::Number(2.0))])),
    }
}

/// Returns a random edit of `document`, mostly of things it holds and sometimes of things it does not.
#[expect(clippy::too_many_lines, reason = "one arm per kind of edit reads best as one table")]
fn random_edit(rng: &mut u64, document: &SheetDocument) -> SheetEdit {
    let lines: Vec<String> = document
        .cards
        .iter()
        .flat_map(|card| card.line_tables.keys().cloned())
        .chain(["l999".to_owned()])
        .collect();
    let cards: Vec<String> = document.cards.iter().map(|card| card.id.clone()).collect();
    let properties: Vec<String> = document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
        .map(|property| property.id.clone())
        .collect();
    let mut items: Vec<SheetItem> = properties.iter().map(|id| SheetItem::Property(id.clone())).collect();
    items.extend(
        document
            .parameters
            .iter()
            .map(|known| SheetItem::Parameter(known.id.clone())),
    );
    items.extend(
        document
            .measures
            .iter()
            .map(|known| SheetItem::Measure(known.id.clone())),
    );
    items.extend(document.events.iter().map(|known| SheetItem::Event(known.id.clone())));
    items.extend(document.buttons.iter().map(|known| SheetItem::Button(known.id.clone())));
    items.extend(cards.iter().map(|id| SheetItem::Card(id.clone())));
    for property in document.cells.iter().flatten() {
        items.extend(property.states.iter().map(|state| SheetItem::State {
            property: property.id.clone(),
            state: state.id.clone(),
        }));
    }
    let things: Vec<String> = ["cells".to_owned(), "ground".to_owned()]
        .into_iter()
        .chain(document.agents.iter().map(|kind| kind.id.clone()))
        .collect();
    let names = [
        "Speed",
        "Infection rate",
        "Alive",
        "Grid width",
        "  ",
        "Ünïcode name",
        "Sick",
        "A b c",
    ];
    let name = (*pick(rng, &names).unwrap_or(&"Name")).to_owned();
    let line = pick(rng, &lines).cloned().unwrap_or_default();
    let item = pick(rng, &items)
        .cloned()
        .unwrap_or(SheetItem::Parameter("nothing".to_owned()));
    let paths = [
        "condition",
        "condition.rate",
        "condition.chance",
        "condition.tries",
        "condition.left",
        "condition.right",
        "condition.low",
        "condition.high",
        "whose.right",
        "value",
        "rate",
        "chance",
        "items.0",
        "left",
    ];
    match below(rng, 29) {
        0 => SheetEdit::AddProperty {
            thing: pick(rng, &things).cloned().unwrap_or_default(),
            name,
            property_type: [PropertyType::States, PropertyType::Number][below(rng, 2)],
        },
        1 => SheetEdit::AddState {
            property: pick(rng, &properties).cloned().unwrap_or_default(),
            name,
            color: None,
        },
        2 => SheetEdit::AddParameter {
            name,
            parameter_type: ParameterType::Number,
            min: 0.0,
            max: 1.0,
            default: 0.5,
            unit: [None, Some("per day".to_owned())][below(rng, 2)].clone(),
        },
        3 => SheetEdit::AddMeasure {
            name,
            rule: MeasureRule::Average,
            over: Some("cells".to_owned()),
            property: None,
            state: None,
            event: None,
            value: Slot::Blank,
            color: None,
        },
        4 => SheetEdit::AddButton {
            name,
            rule: [ButtonStepRule::StartAgain, ButtonStepRule::RunOnce][below(rng, 2)],
            thing: pick(rng, &things).cloned().unwrap_or_default(),
        },
        5 => SheetEdit::AddCard {
            every: pick(rng, &things).cloned().unwrap_or_default(),
        },
        6 => SheetEdit::Rename { item, name },
        7 => SheetEdit::ChangeId {
            item,
            id: ["fresh_id", "health", "count", "status_2"][below(rng, 4)].to_owned(),
        },
        8 => SheetEdit::Recolor {
            item,
            color: STATE_COLORS[below(rng, STATE_COLORS.len())],
        },
        9 => SheetEdit::Delete { item },
        10 => SheetEdit::SetPropertyType {
            property: pick(rng, &properties).cloned().unwrap_or_default(),
            property_type: [PropertyType::States, PropertyType::Number, PropertyType::WholeNumber][below(rng, 3)],
        },
        11 | 12 => {
            let kinds = [
                LineKind::Become,
                LineKind::Set,
                LineKind::When,
                LineKind::Count,
                LineKind::Chance,
                LineKind::Rate,
                LineKind::Compare,
                LineKind::Calculate,
                LineKind::AllOf,
            ];
            let mut new_line = SheetLine::new(kinds[below(rng, kinds.len())]);
            if new_line.kind == LineKind::Count {
                new_line.around = Some(Around::Cells8);
            }
            let blocks: Vec<String> = document
                .cards
                .iter()
                .flat_map(|card| &card.line_tables)
                .filter(|(_, known)| known.kind == LineKind::When)
                .map(|(id, _)| id.clone())
                .collect();
            let inside = if below(rng, 2) == 0 {
                pick(rng, &blocks).cloned()
            } else {
                None
            };
            let card = inside
                .as_ref()
                .and_then(|block| document.line(block).map(|(index, _)| document.cards[index].id.clone()))
                .or_else(|| pick(rng, &cards).cloned())
                .unwrap_or_default();
            SheetEdit::AddLine {
                card,
                inside,
                line: Box::new(new_line),
            }
        }
        13 | 14 => SheetEdit::SetSlot {
            line,
            path: (*pick(rng, &paths).unwrap_or(&"condition")).to_owned(),
            value: random_value(rng, document, &lines),
        },
        15 => SheetEdit::SetField {
            line,
            phrase: String::new(),
            field: match below(rng, 5) {
                0 => LineField::Test(Some(
                    [CompareTest::Is, CompareTest::OneOf, CompareTest::Outside][below(rng, 3)],
                )),
                1 => LineField::RateUnit(Some(TimeUnit::Week)),
                2 => LineField::Formula(Some("1 + 2".to_owned())),
                3 => LineField::Property(pick(rng, &properties).cloned()),
                _ => LineField::StartState(Some("any_state".to_owned())),
            },
        },
        16 => SheetEdit::ChooseCondition {
            line,
            item: ConditionItem::ALL[below(rng, ConditionItem::ALL.len())],
        },
        17 => SheetEdit::UseOneSide { line },
        18 => SheetEdit::UseCountForEach { line },
        19 => SheetEdit::MoveConditionInto { when: line },
        20 => SheetEdit::RenameResult { line, name },
        21 => SheetEdit::SetNote {
            line,
            note: Some("A note \"quoted\"\nover two lines".to_owned()),
        },
        22 | 23 => SheetEdit::MoveLine {
            line,
            direction: [MoveDirection::Up, MoveDirection::Down][below(rng, 2)],
        },
        24 => SheetEdit::DeleteLine { line },
        26 => SheetEdit::AddSummary {
            line,
            summary: [SummaryKind::Average, SummaryKind::Total, SummaryKind::Count][below(rng, 3)],
        },
        27 => SheetEdit::DeleteSummary {
            line,
            index: below(rng, 3),
        },
        28 => SheetEdit::ChooseRowCondition {
            line,
            row: below(rng, 3),
            item: ConditionItem::ALL[below(rng, ConditionItem::ALL.len())],
        },
        _ => match below(rng, 7) {
            0 => SheetEdit::DrawNewNumbers { line },
            4 => SheetEdit::SetAgentKind {
                kind: document.agents.first().map(|kind| kind.id.clone()).unwrap_or_default(),
                count: [0, 5, 50][below(rng, 3)],
                max_count: [10, 100][below(rng, 2)],
            },
            5 => SheetEdit::SetMarks {
                property: pick(rng, &properties).cloned().unwrap_or_default(),
                marks: [None, Some(Marks::Largest), Some(Marks::AddUp)][below(rng, 3)],
                kept_per_tick: [
                    Slot::Blank,
                    Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.9))),
                ][below(rng, 2)]
                .clone(),
            },
            6 => {
                let ground_states: Vec<(String, String)> = document
                    .ground
                    .iter()
                    .flat_map(|ground| &ground.properties)
                    .flat_map(|property| {
                        property
                            .states
                            .iter()
                            .map(|state| (property.id.clone(), state.id.clone()))
                    })
                    .chain([("nothing".to_owned(), "nothing".to_owned())])
                    .collect();
                let (property, state) = pick(rng, &ground_states).cloned().unwrap_or_default();
                SheetEdit::SetBlocked {
                    property,
                    state,
                    blocked: below(rng, 2) == 0,
                }
            }
            1 => SheetEdit::SetTime {
                time: [None, Some(SheetTime::new(2.0, TimeUnit::Hour))][below(rng, 2)],
            },
            2 => SheetEdit::SetWorld {
                width: 64,
                height: 32,
                edges: Edges::Walls,
            },
            _ => SheetEdit::SetPurpose {
                purpose: Some("A purpose".to_owned()),
            },
        },
    }
}

#[test]
fn every_edit_leaves_a_document_that_reads_back() {
    let mut rng = 0x7368_6565_7465_6469_u64;
    let mut accepted = 0_usize;
    for (file_name, text) in sheets() {
        for _ in 0..12 {
            let mut builder = SheetBuilder::new(read(text)).with_key_seed(rng);
            for _ in 0..30 {
                let edit = random_edit(&mut rng, builder.document());
                let shown = format!("{edit:?}");
                if builder.apply(edit).is_err() {
                    continue;
                }
                accepted += 1;
                let written = write(builder.document());
                let again = ParsedSheet::parse(&written).unwrap_or_else(|error| {
                    panic!("{file_name}: after {shown} the text does not read: {error}\n{written}")
                });
                assert_eq!(again.document(), builder.document(), "{file_name}: after {shown}");
                let checked = catch_unwind(AssertUnwindSafe(|| check::check(builder.document())));
                assert!(
                    checked.is_ok(),
                    "{file_name}: the checker panics after {shown}\n{written}"
                );
                let keys: Vec<u32> = builder
                    .document()
                    .cards
                    .iter()
                    .flat_map(|card| card.line_tables.values())
                    .filter_map(|line| line.key)
                    .collect();
                let distinct: BTreeSet<u32> = keys.iter().copied().collect();
                assert_eq!(
                    distinct.len(),
                    keys.len(),
                    "{file_name}: two lines share a key after {shown}"
                );
            }
        }
    }
    assert!(accepted > 1000, "only {accepted} edits were accepted");
}

#[test]
fn a_new_button_and_card_take_their_ids() {
    let mut builder = builder_of("game_of_life.toml");
    let made = run(
        &mut builder,
        SheetEdit::AddButton {
            name: "Plant glider".to_owned(),
            rule: ButtonStepRule::RunOnce,
            thing: "cells".to_owned(),
        },
    );
    assert_eq!(made, ["plant_glider"]);
    assert!(
        builder
            .document()
            .cards
            .iter()
            .any(|card| card.id == "plant_glider" && card.runs == CardRuns::Once)
    );
    // A button named as a card takes `_2`, since its card shares the card namespace.
    let made = run(
        &mut builder,
        SheetEdit::AddButton {
            name: "Cell tick".to_owned(),
            rule: ButtonStepRule::RunOnce,
            thing: "cells".to_owned(),
        },
    );
    assert_eq!(made, ["cell_tick_2"]);
    let refused = builder.apply(SheetEdit::AddCard {
        every: "cells".to_owned(),
    });
    assert!(matches!(refused, Err(SheetEditError::Taken(_))), "{refused:?}");
    run(
        &mut builder,
        SheetEdit::ChangeId {
            item: SheetItem::Button("plant_glider".to_owned()),
            id: "glider".to_owned(),
        },
    );
    let document = builder.document();
    assert!(document.cards.iter().any(|card| card.id == "glider"));
    assert!(
        document.buttons.iter().any(
            |button| button.id == "glider" && button.does.iter().any(|step| step.card.as_deref() == Some("glider"))
        )
    );
    assert!(!check::check(document).has_errors());
    // A button that starts again mints against the buttons alone, and a card of the same id that it does not run
    // keeps its id when the button takes a new one.
    let mut builder = builder_of("game_of_life.toml");
    let made = run(
        &mut builder,
        SheetEdit::AddButton {
            name: "Cell tick".to_owned(),
            rule: ButtonStepRule::StartAgain,
            thing: "cells".to_owned(),
        },
    );
    assert_eq!(made, ["cell_tick"]);
    run(
        &mut builder,
        SheetEdit::ChangeId {
            item: SheetItem::Button("cell_tick".to_owned()),
            id: "reset".to_owned(),
        },
    );
    let document = builder.document();
    assert!(document.cards.iter().any(|card| card.id == "cell_tick"));
    assert!(document.cards.iter().all(|card| card.id != "reset"));
    assert!(document.buttons.iter().any(|button| button.id == "reset"));
    assert!(!check::check(document).has_errors());
}

#[test]
fn the_agents_take_a_count_up_to_their_largest_number() {
    let mut builder = builder_of("random_walk.toml");
    let set = |count: u32, max_count: u32| SheetEdit::SetAgentKind {
        kind: "walker".to_owned(),
        count,
        max_count,
    };
    run(&mut builder, set(500, 2000));
    let walker = &builder.document().agents[0];
    assert_eq!((walker.count, walker.max_count), (500, 2000));
    assert!(write(builder.document()).contains("count = 500\nmax_count = 2000\n"));
    for refused in [set(0, 10), set(11, 10)] {
        assert!(matches!(builder.apply(refused), Err(SheetEditError::Invalid(_))));
    }
    let missing = builder.apply(SheetEdit::SetAgentKind {
        kind: "bird".to_owned(),
        count: 1,
        max_count: 1,
    });
    assert!(matches!(missing, Err(SheetEditError::Missing(_))));
    assert_eq!(
        builder.document().agents[0].count,
        500,
        "a refused edit changes nothing"
    );
}

#[test]
fn a_ground_property_takes_its_marks_and_fading_and_comes_and_goes_with_the_ground() {
    let mut builder = builder_of("ants.toml");
    let kept = Slot::Filled(SheetValue::Chip(SheetChip::Param("trail_kept_per_tick".to_owned())));
    run(
        &mut builder,
        SheetEdit::SetMarks {
            property: "to_food".to_owned(),
            marks: Some(Marks::AddUp),
            kept_per_tick: kept.clone(),
        },
    );
    let written = write(builder.document());
    assert!(written.contains("id = \"to_food\"\nname = \"To food\"\ntype = \"number\"\nmarks = \"add_up\""));
    let lowered = crate::sheet::SheetFile::parse(&written).expect("the sheet lowers");
    assert!(
        lowered.lowered().to_toml().contains("combine = \"sum\""),
        "add up lowers to a sum"
    );

    // No marks rule takes the fading with it, since a property that fades names its marks rule.
    run(
        &mut builder,
        SheetEdit::SetMarks {
            property: "to_home".to_owned(),
            marks: None,
            kept_per_tick: kept.clone(),
        },
    );
    let ground = builder.document().ground.as_ref().expect("a ground");
    let to_home = &ground.properties[2];
    assert_eq!((to_home.marks, to_home.kept_per_tick.is_blank()), (None, true));
    for (property, error) in [
        ("ground", "a States property"),
        ("cargo", "an agent property"),
        ("nothing", "none"),
    ] {
        let refused = builder.apply(SheetEdit::SetMarks {
            property: property.to_owned(),
            marks: Some(Marks::Largest),
            kept_per_tick: Slot::Blank,
        });
        assert!(refused.is_err(), "marks on {error} are refused");
    }

    // A blocked state is held once, and letting in a state that was not blocked changes nothing.
    let blocked = |builder: &SheetBuilder| builder.document().ground.as_ref().expect("a ground").blocked.clone();
    let block = |state: &str, kept_out: bool| SheetEdit::SetBlocked {
        property: "ground".to_owned(),
        state: state.to_owned(),
        blocked: kept_out,
    };
    run(&mut builder, block("food", true));
    run(&mut builder, block("food", true));
    assert_eq!(
        blocked(&builder),
        [
            BlockedState::new("ground", "obstacle"),
            BlockedState::new("ground", "food")
        ]
    );
    run(&mut builder, block("obstacle", false));
    run(&mut builder, block("nest", false));
    assert_eq!(blocked(&builder), [BlockedState::new("ground", "food")]);
    assert!(
        builder.apply(block("lava", true)).is_err(),
        "a state the property lacks"
    );
    assert!(
        builder
            .apply(SheetEdit::SetBlocked {
                property: "to_food".to_owned(),
                state: "open".to_owned(),
                blocked: true,
            })
            .is_err(),
        "a Number property has no states to block"
    );

    // The ground comes with its first property and goes with its last.
    let mut walk = builder_of("random_walk.toml");
    let made = run(
        &mut walk,
        SheetEdit::AddProperty {
            thing: "ground".to_owned(),
            name: "Trail".to_owned(),
            property_type: PropertyType::Number,
        },
    );
    assert!(walk.document().ground.is_some());
    run(
        &mut walk,
        SheetEdit::Delete {
            item: SheetItem::Property(made[0].clone()),
        },
    );
    assert_eq!(walk.document().ground, None);
    assert_eq!(write(walk.document()), sheet_text("random_walk.toml"));
}

/// Returns the ids and names of the summaries of line `line_id` in `builder`.
fn summaries_of(builder: &SheetBuilder, line_id: &str) -> Vec<(String, String)> {
    line(builder, line_id)
        .summaries
        .iter()
        .map(|summary| (summary.id.clone(), summary.name.clone()))
        .collect()
}

/// Returns a look at the birds within a distance that starts with one average of nothing, as the editor adds it.
fn new_look() -> SheetLine {
    let mut look = SheetLine::new(LineKind::LookAround);
    look.around = Some(Around::AgentsWithin);
    look.agent_kind = Some("bird".to_owned());
    look.include_me = Some(false);
    let mut summary = LookSummary::new("", "");
    summary.summary = Some(SummaryKind::Average);
    look.summaries = vec![summary];
    look
}

#[test]
fn a_summary_follows_its_automatic_name_and_its_new_id_reaches_every_reference() {
    let mut builder = builder_of("boids.toml");
    let made = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "bird_tick".to_owned(),
            inside: None,
            line: Box::new(new_look()),
        },
    );
    let look = made[0].clone();
    assert_eq!(
        summaries_of(&builder, &look),
        [("average".to_owned(), "average".to_owned())]
    );
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: look.clone(),
            path: "summaries.0.value".to_owned(),
            value: chip(SheetChip::Their("velocity".to_owned())),
        },
    );
    assert_eq!(
        summaries_of(&builder, &look),
        [("average_velocity".to_owned(), "average velocity".to_owned())]
    );
    run(
        &mut builder,
        SheetEdit::AddSummary {
            line: look.clone(),
            summary: SummaryKind::Average,
        },
    );
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: look.clone(),
            path: "summaries.1.value".to_owned(),
            value: chip(SheetChip::Relation(ChipRelation::OffsetToThem)),
        },
    );
    assert_eq!(
        summaries_of(&builder, &look)[1],
        ("centre".to_owned(), "centre".to_owned())
    );

    // A Steer reads the centre. The total away from them is the push, and the Steer follows the new id.
    let mut steer = SheetLine::new(LineKind::Steer);
    steer.property = Some("velocity".to_owned());
    steer.value = chip(SheetChip::Result {
        line: look.clone(),
        part: Some("centre".to_owned()),
        property: None,
    });
    let steer = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "bird_tick".to_owned(),
            inside: None,
            line: Box::new(steer),
        },
    )[0]
    .clone();
    let mut summaries = line(&builder, &look).summaries.clone();
    summaries[1].summary = Some(SummaryKind::Total);
    summaries[1].value = chip(SheetChip::Relation(ChipRelation::OffsetAwayFromThem));
    run(
        &mut builder,
        SheetEdit::SetField {
            line: look.clone(),
            phrase: String::new(),
            field: LineField::Summaries(summaries),
        },
    );
    assert_eq!(summaries_of(&builder, &look)[1], ("push".to_owned(), "push".to_owned()));
    assert_eq!(
        line(&builder, &steer).value,
        chip(SheetChip::Result {
            line: look.clone(),
            part: Some("push".to_owned()),
            property: None,
        }),
        "the Steer reads the summary by its new id"
    );
}

#[test]
fn a_summary_named_by_hand_keeps_its_name_and_a_removed_summary_blanks_its_users() {
    let mut builder = builder_of("boids.toml");
    let mut look = new_look();
    look.summaries[0].value = chip(SheetChip::Their("velocity".to_owned()));
    let mut push = LookSummary::new("", "");
    push.summary = Some(SummaryKind::Total);
    push.value = chip(SheetChip::Relation(ChipRelation::OffsetAwayFromThem));
    look.summaries.push(push);
    let look = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "bird_tick".to_owned(),
            inside: None,
            line: Box::new(look),
        },
    )[0]
    .clone();
    let mut steer = SheetLine::new(LineKind::Steer);
    steer.property = Some("velocity".to_owned());
    steer.value = chip(SheetChip::Result {
        line: look.clone(),
        part: Some("push".to_owned()),
        property: None,
    });
    let steer = run(
        &mut builder,
        SheetEdit::AddLine {
            card: "bird_tick".to_owned(),
            inside: None,
            line: Box::new(steer),
        },
    )[0]
    .clone();

    // A name typed by hand stays, and so does its id. A second average of nothing takes "average 2".
    let mut summaries = line(&builder, &look).summaries.clone();
    summaries[0].name = "flock speed".to_owned();
    run(
        &mut builder,
        SheetEdit::SetField {
            line: look.clone(),
            phrase: String::new(),
            field: LineField::Summaries(summaries),
        },
    );
    for _ in 0..2 {
        run(
            &mut builder,
            SheetEdit::AddSummary {
                line: look.clone(),
                summary: SummaryKind::Average,
            },
        );
    }
    assert_eq!(
        summaries_of(&builder, &look),
        [
            ("average_velocity".to_owned(), "flock speed".to_owned()),
            ("push".to_owned(), "push".to_owned()),
            ("average".to_owned(), "average".to_owned()),
            ("average_2".to_owned(), "average 2".to_owned()),
        ]
    );

    // Removing the push leaves the Steer's value blank, and the last summary stays.
    run(
        &mut builder,
        SheetEdit::DeleteSummary {
            line: look.clone(),
            index: 1,
        },
    );
    assert!(line(&builder, &steer).value.is_blank());
    for _ in 0..2 {
        run(
            &mut builder,
            SheetEdit::DeleteSummary {
                line: look.clone(),
                index: 0,
            },
        );
    }
    assert!(matches!(
        builder.apply(SheetEdit::DeleteSummary { line: look, index: 0 }),
        Err(SheetEditError::Invalid(_))
    ));
    assert!(matches!(
        builder.apply(SheetEdit::AddSummary {
            line: "l3".to_owned(),
            summary: SummaryKind::Count,
        }),
        Err(SheetEditError::Invalid(_))
    ));
}

/// Returns the summary id that the value of line `line_id` in `builder` reads.
fn part_read_by(builder: &SheetBuilder, line_id: &str) -> Option<String> {
    match &line(builder, line_id).value {
        Slot::Filled(SheetValue::Chip(SheetChip::Result { part, .. })) => part.clone(),
        _ => None,
    }
}

#[test]
fn a_summary_that_takes_another_summarys_name_leaves_every_reference_on_its_own_summary() {
    // Boids' l4 reads summary 0, the average velocity, and l5 reads summary 1, the centre.
    let mut builder = builder_of("boids.toml");
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: "l1".to_owned(),
            path: "summaries.0.value".to_owned(),
            value: chip(SheetChip::Relation(ChipRelation::OffsetToThem)),
        },
    );
    let summaries = summaries_of(&builder, "l1");
    assert_eq!(
        summaries[..2],
        [
            ("centre_2".to_owned(), "centre 2".to_owned()),
            ("centre".to_owned(), "centre".to_owned()),
        ],
        "the centre keeps its name, and the edited summary takes the next number"
    );
    assert_eq!(part_read_by(&builder, "l4").as_deref(), Some("centre_2"));
    assert_eq!(part_read_by(&builder, "l5").as_deref(), Some("centre"));

    // Swapping the two values swaps the two ids, and each reference still follows its own summary.
    let mut builder = builder_of("boids.toml");
    let mut summaries = line(&builder, "l1").summaries.clone();
    let first = summaries[0].value.clone();
    summaries[0].value = summaries[1].value.clone();
    summaries[1].value = first;
    run(
        &mut builder,
        SheetEdit::SetField {
            line: "l1".to_owned(),
            phrase: String::new(),
            field: LineField::Summaries(summaries),
        },
    );
    assert_eq!(
        summaries_of(&builder, "l1")[..2],
        [
            ("centre".to_owned(), "centre".to_owned()),
            ("average_velocity".to_owned(), "average velocity".to_owned()),
        ]
    );
    assert_eq!(part_read_by(&builder, "l4").as_deref(), Some("centre"));
    assert_eq!(part_read_by(&builder, "l5").as_deref(), Some("average_velocity"));
}

#[test]
fn a_count_summary_takes_an_id_that_no_summary_holds() {
    let mut builder = builder_of("boids.toml");
    for _ in 0..2 {
        run(
            &mut builder,
            SheetEdit::AddSummary {
                line: "l1".to_owned(),
                summary: SummaryKind::Count,
            },
        );
    }
    let summaries = summaries_of(&builder, "l1");
    assert_eq!(
        summaries[summaries.len() - 2..],
        [
            ("count_summary".to_owned(), "count".to_owned()),
            ("count_2".to_owned(), "count 2".to_owned()),
        ]
    );
}

#[test]
fn the_automatic_summary_names_follow_round_1() {
    let document = read(sheet_text("boids.toml"));
    let summary = |kind: SummaryKind, value: Slot| {
        let mut summary = LookSummary::new("", "");
        summary.summary = Some(kind);
        summary.value = value;
        automatic_summary_name(&document, &summary)
    };
    let relation = |relation: ChipRelation| chip(SheetChip::Relation(relation));
    assert_eq!(summary(SummaryKind::Count, Slot::Blank), "count");
    assert_eq!(summary(SummaryKind::Total, Slot::Blank), "total");
    assert_eq!(
        summary(SummaryKind::Largest, chip(SheetChip::Their("velocity".to_owned()))),
        "largest velocity"
    );
    assert_eq!(
        summary(SummaryKind::Average, relation(ChipRelation::OffsetToThem)),
        "centre"
    );
    assert_eq!(
        summary(SummaryKind::Total, relation(ChipRelation::OffsetAwayFromThem)),
        "push"
    );
    assert_eq!(
        summary(SummaryKind::Average, relation(ChipRelation::DistanceToThem)),
        "average distance to them"
    );
    // Every committed look at agents names its summaries as the editor does, so a rebuild mints the same ids.
    for (file_name, text) in sheets() {
        let document = read(text);
        for card in &document.cards {
            for line in card.line_tables.values() {
                if line.around != Some(Around::AgentsWithin) {
                    continue;
                }
                for summary in &line.summaries {
                    assert_eq!(
                        summary.name,
                        automatic_summary_name(&document, summary),
                        "{file_name}: {}",
                        summary.id
                    );
                }
            }
        }
    }
}

#[test]
fn a_choose_row_takes_its_condition_from_choose_when() {
    let mut builder = builder_of("ants.toml");
    let choose = builder
        .document()
        .cards
        .iter()
        .flat_map(|card| &card.line_tables)
        .find(|(_, line)| line.kind == LineKind::Choose)
        .map(|(id, _)| id.clone())
        .expect("the ants choose their next cell");
    run(
        &mut builder,
        SheetEdit::ChooseRowCondition {
            line: choose.clone(),
            row: 1,
            item: ConditionItem::Always,
        },
    );
    let Some(SheetValue::Phrase(condition)) = line(&builder, &choose).rows[1].condition.value() else {
        panic!("the row holds a condition");
    };
    assert_eq!(condition.kind, LineKind::Always);
    // A filled value one past the last row adds a row with an empty condition.
    let rows = line(&builder, &choose).rows.len();
    run(
        &mut builder,
        SheetEdit::SetSlot {
            line: choose.clone(),
            path: format!("row.{rows}.value"),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0))),
        },
    );
    assert_eq!(line(&builder, &choose).rows.len(), rows + 1);
    assert!(line(&builder, &choose).rows[rows].condition.is_blank());
    assert!(matches!(
        builder.apply(SheetEdit::ChooseRowCondition {
            line: choose,
            row: 9,
            item: ConditionItem::Is,
        }),
        Err(SheetEditError::Missing(_))
    ));
}

#[test]
fn a_new_id_for_an_item_the_sheet_does_not_hold_is_refused_and_changes_nothing() {
    // Each id but the last two is held by an item of another kind: Carrying food is a measure and a state of Cargo,
    // Delivery an event, Momentum a parameter, Ant tick a card, Reset colony a button and Cargo a property.
    let state = |property: &str, state: &str| SheetItem::State {
        property: property.to_owned(),
        state: state.to_owned(),
    };
    let cases = [
        (SheetItem::Property("delivery".to_owned()), "property 'delivery'"),
        (state("cargo", "nest"), "state 'nest'"),
        (state("heading", "searching"), "state 'searching'"),
        (state("nothing", "searching"), "property 'nothing'"),
        (
            SheetItem::Parameter("carrying_food".to_owned()),
            "parameter 'carrying_food'",
        ),
        (SheetItem::Measure("delivery".to_owned()), "measure 'delivery'"),
        (SheetItem::Event("momentum".to_owned()), "event 'momentum'"),
        (SheetItem::Button("ant_tick".to_owned()), "button 'ant_tick'"),
        (SheetItem::Card("reset_colony".to_owned()), "card 'reset_colony'"),
        (SheetItem::AgentKind("cargo".to_owned()), "kind of agent 'cargo'"),
        (SheetItem::Parameter("nothing".to_owned()), "parameter 'nothing'"),
        (SheetItem::Card("nothing".to_owned()), "card 'nothing'"),
    ];
    let old = sheet_text("ants.toml");
    let mut builder = builder_of("ants.toml");
    for (item, named) in cases {
        let held = match &item {
            SheetItem::State { state, .. } => state.clone(),
            SheetItem::Property(id)
            | SheetItem::Parameter(id)
            | SheetItem::Measure(id)
            | SheetItem::Event(id)
            | SheetItem::Button(id)
            | SheetItem::Card(id)
            | SheetItem::AgentKind(id) => id.clone(),
        };
        // A new id, and the id the item would hold already: neither changes an item the sheet does not hold.
        for id in ["fresh_id".to_owned(), held] {
            let refused = builder.apply(SheetEdit::ChangeId {
                item: item.clone(),
                id: id.clone(),
            });
            assert_eq!(
                refused,
                Err(SheetEditError::Missing(format!("the sheet has no {named}"))),
                "{item:?} to '{id}'"
            );
            assert_eq!(write(builder.document()), old, "{item:?} to '{id}'");
        }
    }
}
