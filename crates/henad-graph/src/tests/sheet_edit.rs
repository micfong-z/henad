//! The sheet edit commands: the author format's three typical edits built by commands, the outline moves, renames and
//! new ids, line numbers, id minting, automatic names, the commands behind the card's offers, deletes that leave
//! skipped lines, colours, rate units, starts, buttons, cards, agents and the ground.

use std::collections::BTreeSet;

use henad_core::metadata::TimeUnit;

use super::sheet_support::{
    add_line, builder_of, chip, line, lowered, phrase, read, result, run, set_field, set_slot, sheet_text, sheets,
    write,
};
use super::sheet_writer::{RENAME_HUNKS, STEER_REORDER_HUNKS, WANING_HUNKS, assert_hunks};
use crate::keys::KeyStream;
use crate::sheet::check;
use crate::sheet::document::{
    BlockedState, ButtonStepRule, CardRuns, ChooseRow, ColorRuleKind, CompareTest, LineKind, Marks, MeasureRule,
    PaintedShape, ParameterType, PropertyType, ShapeKind, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetTime,
    SheetValue, Slot, StartRule, StartsAs, StartsAsRow,
};
use crate::sheet::edit::{
    IdSpace, LineField, LineSide, MoveDirection, STATE_COLORS, SheetBuilder, SheetEdit, SheetEditError, SheetItem,
    automatic_name, lines_losing_formulas, measures_losing_formulas, mint_id, unit_in_text,
};
use crate::sheet::limits::MAX_NESTING;
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{Blank, ConditionItem, SentencePart, Sentences};
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
    let made = add_line(&mut builder, "cell_tick", None, become_line);
    assert_eq!(made, ["l5"], "line 4 sits in the New outbreak card");
    let rate = condition(&builder, "l5");
    assert_eq!(
        (rate.rate_unit, rate.rate_unit_from_tick),
        (Some(TimeUnit::Day), true),
        "a new rate takes the unit of one tick, marked"
    );
    set_field(
        &mut builder,
        "l5",
        LineField::NewState(Some(("health".to_owned(), "susceptible".to_owned()))),
    );
    set_field(&mut builder, "l5", LineField::StartState(Some("recovered".to_owned())));
    set_slot(
        &mut builder,
        "l5",
        "condition.rate",
        chip(SheetChip::Param("waning_rate".to_owned())),
    );
    let rate = condition(&builder, "l5");
    assert_eq!(
        (rate.rate_unit, rate.rate_unit_from_tick),
        (Some(TimeUnit::Day), false),
        "the parameter's unit replaces the mark"
    );
    let new = write(builder.document());
    assert_hunks(old, &new, WANING_HUNKS);
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
    assert_hunks(old, &write(builder.document()), STEER_REORDER_HUNKS);
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
    assert_hunks(old, &write(builder.document()), RENAME_HUNKS);
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
            "that number is too large",
            "that number is too large",
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
        matches!(&parameter, Err(SheetEditError::Invalid(reason)) if reason == "that number is too large"),
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
fn a_kind_of_agent_takes_no_id_its_tick_card_cannot_follow() {
    // The tick card `<id>_tick` holds 5 bytes more than the kind's id, and a sheet id holds at most 48.
    let mut builder = builder_of("boids.toml");
    let longest = "a".repeat(43);
    run(
        &mut builder,
        SheetEdit::ChangeId {
            item: SheetItem::AgentKind("bird".to_owned()),
            id: longest.clone(),
        },
    );
    assert!(
        builder
            .document()
            .cards
            .iter()
            .any(|card| card.id == format!("{longest}_tick"))
    );
    assert!(!check::check(builder.document()).has_errors());
    let before = write(builder.document());
    let too_long = "b".repeat(44);
    let refused = builder.apply(SheetEdit::ChangeId {
        item: SheetItem::AgentKind(longest),
        id: too_long.clone(),
    });
    assert_eq!(
        refused,
        Err(SheetEditError::Invalid(format!(
            "the card id '{too_long}_tick' exceeds 48 bytes"
        )))
    );
    assert_eq!(write(builder.document()), before, "a refused id changes nothing");
}

#[test]
fn a_deleted_line_number_is_never_minted_again() {
    let mut builder = builder_of("game_of_life.toml");
    let first = add_line(&mut builder, "cell_tick", None, SheetLine::new(LineKind::Become));
    assert_eq!(first, ["l5"]);
    run(&mut builder, SheetEdit::DeleteLine { line: "l5".to_owned() });
    let second = add_line(&mut builder, "cell_tick", None, SheetLine::new(LineKind::Become));
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

/// A new parameter takes no id of a fixed column of `runs.csv`, whose sweep writes the parameters among those columns,
/// and no parameter changes its id to one. A property of the name takes the id, since no property is a column.
#[test]
fn a_parameter_takes_no_id_of_a_column_of_sweep_results() {
    let mut builder = builder_of("game_of_life.toml");
    let add_parameter = |name: &str| SheetEdit::AddParameter {
        name: name.to_owned(),
        parameter_type: ParameterType::Number,
        min: 0.0,
        max: 1.0,
        default: 0.1,
        unit: None,
    };
    assert_eq!(run(&mut builder, add_parameter("Seed")), ["seed_2"]);
    assert_eq!(run(&mut builder, add_parameter("Population")), ["population_2"]);
    let property = run(
        &mut builder,
        SheetEdit::AddProperty {
            thing: "cells".to_owned(),
            name: "Note".to_owned(),
            property_type: PropertyType::Number,
        },
    );
    assert_eq!(property, ["note"]);
    let report = check::check(builder.document());
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.diagnostic.message.contains("is reserved")),
        "{:?}",
        report.diagnostics
    );
    let reserved = builder.apply(SheetEdit::ChangeId {
        item: SheetItem::Parameter("seed_2".to_owned()),
        id: "seed".to_owned(),
    });
    assert_eq!(reserved, Err(SheetEditError::Reserved("seed".to_owned())));
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
fn a_line_added_inside_a_block_that_lists_itself_is_added() {
    // A file can list a block inside itself, or two blocks inside each other. The checker reports it, and the
    // editor still adds a line inside such a block.
    let block = |kind: LineKind, inner: &str| {
        let mut block = SheetLine::new(kind);
        block.lines = vec![inner.to_owned()];
        if kind == LineKind::Repeat {
            block.times = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(2.0)));
        }
        block
    };
    let cases = [
        vec![("l5", block(LineKind::Repeat, "l5"))],
        vec![("l5", block(LineKind::When, "l5"))],
        vec![
            ("l5", block(LineKind::When, "l6")),
            ("l6", block(LineKind::Repeat, "l5")),
        ],
    ];
    for blocks in cases {
        let mut document = read(sheet_text("game_of_life.toml"));
        let card = &mut document.cards[0];
        card.lines.push("l5".to_owned());
        for (id, line) in &blocks {
            card.line_tables.insert((*id).to_owned(), line.clone());
        }
        document.sheet.next_line = 7;
        assert!(check::check(&document).has_errors(), "{blocks:?}");
        let mut builder = SheetBuilder::new(document).with_key_seed(12);
        let made = add_line(&mut builder, "cell_tick", Some("l5"), SheetLine::new(LineKind::Become));
        assert_eq!(made, ["l7"], "{blocks:?}");
    }
}

#[test]
fn an_edit_the_builder_takes_leaves_a_text_that_reads_back() {
    // A value nested `depth` phrases deep in one of four ways, around a number or a list of one number.
    fn nested(shape: usize, depth: usize, list: bool) -> SheetValue {
        let number = SheetValue::Literal(SheetLiteral::Number(3.0));
        if depth == 0 {
            return if list { SheetValue::List(vec![number]) } else { number };
        }
        let inner = nested(shape, depth - 1, list);
        let phrase = match shape {
            0 => {
                let mut phrase = SheetLine::new(LineKind::Compare);
                phrase.test = Some(CompareTest::Is);
                phrase.left = Slot::Filled(inner);
                phrase
            }
            1 => {
                let mut phrase = SheetLine::new(LineKind::AllOf);
                phrase.items = vec![inner];
                phrase
            }
            2 => {
                let mut phrase = SheetLine::new(LineKind::Choose);
                phrase.rows = vec![ChooseRow {
                    value: Slot::Filled(inner),
                    condition: Slot::Blank,
                }];
                phrase
            }
            _ => {
                let mut phrase = SheetLine::new(LineKind::DependingOn);
                phrase.property = Some("status".to_owned());
                phrase.branches.insert("alive".to_owned(), inner);
                phrase
            }
        };
        SheetValue::Phrase(Box::new(phrase))
    }
    for (shape, list) in (0..4).flat_map(|shape| [(shape, false), (shape, true)]) {
        let mut taken = 0;
        let mut refused = 0;
        for depth in 0..=MAX_NESTING {
            let mut builder = builder_of("game_of_life.toml");
            let edit = SheetEdit::SetSlot {
                line: "l2".to_owned(),
                path: "condition".to_owned(),
                value: Slot::Filled(nested(shape, depth, list)),
            };
            if builder.apply(edit).is_err() {
                refused += 1;
                break;
            }
            taken += 1;
            let text = write(builder.document());
            if let Err(error) = ParsedSheet::parse(&text) {
                panic!("shape {shape} ({list}) at depth {depth} does not read back: {error}");
            }
        }
        assert!(taken > 0 && refused > 0, "shape {shape} ({list}): never refused");
    }
    // The text of a sheet holds at most the bytes the reader reads.
    let mut builder = builder_of("game_of_life.toml");
    let before = write(builder.document());
    let refused = builder.apply(SheetEdit::SetPurpose {
        purpose: Some("a".repeat(1_100_000)),
    });
    assert_eq!(
        refused,
        Err(SheetEditError::Invalid(
            "the sheet cannot be longer than 1048576 bytes".to_owned()
        ))
    );
    assert_eq!(write(builder.document()), before);
    run(
        &mut builder,
        SheetEdit::SetPurpose {
            purpose: Some("a".repeat(1000)),
        },
    );
}

#[test]
fn outline_moves_and_deletes_follow_the_when_blocks() {
    let mut builder = builder_of("game_of_life.toml");
    let add = |builder: &mut SheetBuilder, inside: Option<&str>, kind: LineKind| -> String {
        let made = add_line(builder, "cell_tick", inside, SheetLine::new(kind));
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
    add_line(&mut builder, "cell_tick", None, when);
    add_line(&mut builder, "cell_tick", Some("l5"), SheetLine::new(LineKind::Become));
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

/// Runs [`SheetEdit::InsertLine`] of a new line of `kind` beside line `anchor` on `side`, and returns the new id.
fn insert(builder: &mut SheetBuilder, anchor: &str, side: LineSide, kind: LineKind) -> String {
    let made = run(
        builder,
        SheetEdit::InsertLine {
            anchor: anchor.to_owned(),
            side,
            line: Box::new(SheetLine::new(kind)),
        },
    );
    made.into_iter().next().unwrap_or_default()
}

#[test]
fn a_line_goes_above_or_below_another_in_its_own_block() {
    let mut builder = builder_of("game_of_life.toml");
    let mut when = SheetLine::new(LineKind::When);
    let mut test = SheetLine::new(LineKind::Compare);
    test.left = chip(SheetChip::My("status".to_owned()));
    test.test = Some(CompareTest::Is);
    test.right = chip(SheetChip::State("alive".to_owned()));
    when.condition = phrase(test);
    add_line(&mut builder, "cell_tick", None, when);
    add_line(&mut builder, "cell_tick", Some("l5"), SheetLine::new(LineKind::Become));
    let shape = |builder: &SheetBuilder| outline(builder.document(), 0);
    assert_eq!(shape(&builder), "l1 l2 l3 l5[l6]");
    assert_eq!(insert(&mut builder, "l2", LineSide::Above, LineKind::Calculate), "l7");
    assert_eq!(shape(&builder), "l1 l7 l2 l3 l5[l6]");
    assert_eq!(insert(&mut builder, "l1", LineSide::Above, LineKind::Calculate), "l8");
    assert_eq!(shape(&builder), "l8 l1 l7 l2 l3 l5[l6]");
    // A change that goes beside a line inside a When takes the When's defaults, as one added at its end does.
    assert_eq!(insert(&mut builder, "l6", LineSide::Above, LineKind::Become), "l9");
    assert_eq!(shape(&builder), "l8 l1 l7 l2 l3 l5[l9 l6]");
    assert_eq!(condition(&builder, "l9").kind, LineKind::Always);
    assert_eq!(line(&builder, "l9").start_state.as_deref(), Some("alive"));
    // Below a When is after its lines, and above it is before the When.
    assert_eq!(insert(&mut builder, "l5", LineSide::Below, LineKind::Set), "l10");
    assert_eq!(insert(&mut builder, "l5", LineSide::Above, LineKind::Set), "l11");
    assert_eq!(shape(&builder), "l8 l1 l7 l2 l3 l11 l5[l9 l6] l10");
    assert!(
        condition_is_blank(&builder, "l10"),
        "a line outside the When takes no condition"
    );
    assert_eq!(insert(&mut builder, "l6", LineSide::Below, LineKind::Set), "l12");
    assert_eq!(shape(&builder), "l8 l1 l7 l2 l3 l11 l5[l9 l6 l12] l10");
    // Clear's card takes a line beside its own.
    let clear = builder.document().cards[1].lines[0].clone();
    let below_clear = insert(&mut builder, &clear, LineSide::Below, LineKind::Become);
    assert_eq!(builder.document().cards[1].lines, [clear, below_clear]);
    let refused = builder.apply(SheetEdit::InsertLine {
        anchor: "l99".to_owned(),
        side: LineSide::Below,
        line: Box::new(SheetLine::new(LineKind::Become)),
    });
    assert!(matches!(refused, Err(SheetEditError::Missing(_))), "{refused:?}");
    assert_eq!(read(&write(builder.document())), *builder.document());
}

/// Returns whether line `line_id` of `builder` has a blank condition.
fn condition_is_blank(builder: &SheetBuilder, line_id: &str) -> bool {
    line(builder, line_id).condition.is_blank()
}

#[test]
fn a_duplicate_copies_a_block_with_new_ids_keys_and_names_and_reads_its_own_copies() {
    let mut builder = builder_of("game_of_life.toml");
    add_line(&mut builder, "cell_tick", None, SheetLine::new(LineKind::When));
    // Line 6 sums the cells around, line 7 reads it and line 1, and line 8 compares line 6's total.
    let mut total = SheetLine::new(LineKind::LookAround);
    total.around = Some(crate::sheet::document::Around::Cells8);
    let mut summary = crate::sheet::document::LookSummary::new("total", "Total");
    summary.summary = Some(crate::sheet::document::SummaryKind::Total);
    summary.value = chip(SheetChip::My("status".to_owned()));
    total.summaries = vec![summary];
    add_line(&mut builder, "cell_tick", Some("l5"), total);
    let mut calculate = SheetLine::new(LineKind::Calculate);
    calculate.formula = Some("l6.total + l1".to_owned());
    add_line(&mut builder, "cell_tick", Some("l5"), calculate);
    run(
        &mut builder,
        SheetEdit::RenameResult {
            line: "l7".to_owned(),
            name: "Food way".to_owned(),
        },
    );
    let mut compare = SheetLine::new(LineKind::Compare);
    compare.left = chip(SheetChip::Result {
        line: "l6".to_owned(),
        part: Some("total".to_owned()),
        property: None,
    });
    compare.test = Some(CompareTest::Is);
    compare.right = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(3.0)));
    add_line(&mut builder, "cell_tick", Some("l5"), compare);
    let made = run(&mut builder, SheetEdit::DuplicateLine { line: "l5".to_owned() });
    assert_eq!(made, ["l9", "l10", "l11", "l12"]);
    assert_eq!(outline(builder.document(), 0), "l1 l2 l3 l5[l6 l7 l8] l9[l10 l11 l12]");
    assert_eq!(line(&builder, "l11").formula.as_deref(), Some("l10.total + l1"));
    assert_eq!(
        line(&builder, "l12").left.value(),
        Some(&SheetValue::Chip(SheetChip::Result {
            line: "l10".to_owned(),
            part: Some("total".to_owned()),
            property: None,
        }))
    );
    assert_eq!(line(&builder, "l11").name.as_deref(), Some("Food way 2"));
    assert_eq!(line(&builder, "l7").name.as_deref(), Some("Food way"));
    let names: Vec<String> = builder.document().cards[0]
        .line_tables
        .values()
        .filter_map(|line| line.name.as_deref().map(str::to_lowercase))
        .collect();
    let unique: BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    assert_eq!(read(&write(builder.document())), *builder.document());

    // A copy of a line that draws takes a key of its own, and a line copied twice numbers its name on.
    let mut builder = builder_of("ants.toml");
    let made = run(&mut builder, SheetEdit::DuplicateLine { line: "l7".to_owned() });
    assert_eq!(
        builder.document().cards[0].lines.iter().position(|id| *id == made[0]),
        Some(7)
    );
    assert_ne!(line(&builder, &made[0]).key, line(&builder, "l7").key);
    assert!(line(&builder, &made[0]).key.is_some());
    let again = run(&mut builder, SheetEdit::DuplicateLine { line: "l1".to_owned() });
    let third = run(&mut builder, SheetEdit::DuplicateLine { line: "l1".to_owned() });
    assert_eq!(line(&builder, &third[0]).name.as_deref(), Some("Food way 3"));
    assert_eq!(line(&builder, &again[0]).name.as_deref(), Some("Food way 2"));
    let refused = builder.apply(SheetEdit::DuplicateLine { line: "l99".to_owned() });
    assert!(matches!(refused, Err(SheetEditError::Missing(_))), "{refused:?}");
}

#[test]
fn a_choose_row_is_removed_and_the_last_row_stays() {
    let mut builder = builder_of("ants.toml");
    let delete = |builder: &mut SheetBuilder, line: &str, index: usize| {
        builder.apply(SheetEdit::DeleteRow {
            line: line.to_owned(),
            index,
        })
    };
    let second = line(&builder, "l7").rows[1].clone();
    let refused = delete(&mut builder, "l7", 2);
    assert!(matches!(refused, Err(SheetEditError::Missing(_))), "{refused:?}");
    delete(&mut builder, "l7", 0).expect("the first row goes");
    assert_eq!(line(&builder, "l7").rows, [second]);
    let refused = delete(&mut builder, "l7", 0);
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    let refused = delete(&mut builder, "l8", 0);
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    assert_eq!(read(&write(builder.document())), *builder.document());
}

#[test]
fn automatic_names_follow_their_lines_until_renamed() {
    let mut builder = builder_of("forest_fire.toml");
    let add_chance = |builder: &mut SheetBuilder, chance: Slot| -> String {
        let mut line = SheetLine::new(LineKind::Chance);
        line.chance = chance;
        add_line(builder, "cell_tick", None, line)
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
    set_slot(
        &mut builder,
        &empty,
        "chance",
        chip(SheetChip::Param("growth_chance".to_owned())),
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
    set_slot(
        &mut builder,
        &lightning,
        "chance",
        chip(SheetChip::Param("growth_chance".to_owned())),
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
    set_slot(&mut builder, "l2", "condition.left", chip(result("l1")));
    let refused = builder.apply(SheetEdit::UseOneSide { line: "l2".to_owned() });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    set_slot(
        &mut builder,
        "l2",
        "condition.high",
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(4.0))),
    );
    let sentences = |builder: &SheetBuilder| Sentences::new(builder.document()).line("l2", &[]);
    assert_eq!(
        sentences(&builder),
        "Become B from any state, when B in my block is less than … or more than 4"
    );
    run(&mut builder, SheetEdit::UseOneSide { line: "l2".to_owned() });
    assert_eq!(
        sentences(&builder),
        "Become B from any state, when B in my block is more than 4"
    );
    // A When cannot be always.
    let mut when = SheetLine::new(LineKind::When);
    when.condition = phrase(SheetLine::new(LineKind::Compare));
    let when_id = add_line(&mut builder, "cell_tick", None, when);
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
    set_slot(&mut builder, "l2", "condition.tries", Slot::Blank);
    set_slot(&mut builder, "l2", "condition.rate", chip(result("l1")));
    run(&mut builder, SheetEdit::UseCountForEach { line: "l2".to_owned() });
    let rate = condition(&builder, "l2");
    assert!(rate.rate.is_blank());
    assert_eq!(rate.tries, chip(result("l1")));
    let refused = builder.apply(SheetEdit::UseCountForEach { line: "l3".to_owned() });
    assert!(matches!(refused, Err(SheetEditError::Invalid(_))), "{refused:?}");
    // "Move it into line N": a When around one change gives the change its condition and goes.
    let mut when = SheetLine::new(LineKind::When);
    let mut test = SheetLine::new(LineKind::Compare);
    test.left = chip(SheetChip::My("health".to_owned()));
    test.test = Some(CompareTest::Is);
    test.right = chip(SheetChip::State("recovered".to_owned()));
    when.condition = phrase(test.clone());
    let when_id = add_line(&mut builder, "cell_tick", None, when);
    let inner = add_line(
        &mut builder,
        "cell_tick",
        Some(&when_id[0]),
        SheetLine::new(LineKind::Become),
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
fn a_delete_that_empties_a_list_leaves_a_blank_and_a_skipped_line() {
    let list = |item: SheetValue| Slot::Filled(SheetValue::List(vec![item]));
    // Line 2 becomes "when Alive neighbours is one of the numbers Birth count".
    let mut builder = builder_of("game_of_life.toml");
    let made = run(
        &mut builder,
        SheetEdit::AddParameter {
            name: "Birth count".to_owned(),
            parameter_type: ParameterType::WholeNumber,
            min: 0.0,
            max: 8.0,
            default: 3.0,
            unit: None,
        },
    );
    let parameter = made[0].clone();
    run(
        &mut builder,
        SheetEdit::SetField {
            line: "l2".to_owned(),
            phrase: "condition".to_owned(),
            field: LineField::Test(Some(CompareTest::OneOf)),
        },
    );
    let Some(SheetValue::Phrase(condition)) = line(&builder, "l2").condition.value() else {
        panic!("line 2 keeps its comparison");
    };
    assert_eq!(condition.right, list(SheetValue::Literal(SheetLiteral::Number(3.0))));
    set_slot(
        &mut builder,
        "l2",
        "condition.right",
        list(SheetValue::Chip(SheetChip::Param(parameter.clone()))),
    );
    assert!(!check::check(builder.document()).is_skipped("l2"));
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::Parameter(parameter),
        },
    );
    let Some(SheetValue::Phrase(condition)) = line(&builder, "l2").condition.value() else {
        panic!("line 2 keeps its comparison");
    };
    assert_eq!(condition.right, Slot::Blank);
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skip_note("l2"), Some("Not run yet: choose a list of numbers"));
    // A list of states loses its last state the same way.
    let mut builder = builder_of("game_of_life.toml");
    let mut compare = SheetLine::new(LineKind::Compare);
    compare.test = Some(CompareTest::NotOneOf);
    compare.left = chip(SheetChip::My("status".to_owned()));
    compare.right = list(SheetValue::Chip(SheetChip::State("alive".to_owned())));
    let mut become_dead = SheetLine::new(LineKind::Become);
    become_dead.property = Some("status".to_owned());
    become_dead.new_state = Some("dead".to_owned());
    become_dead.start_state = Some("any_state".to_owned());
    become_dead.condition = phrase(compare);
    let made = add_line(&mut builder, "cell_tick", None, become_dead);
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::State {
                property: "status".to_owned(),
                state: "alive".to_owned(),
            },
        },
    );
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skip_note(&made[0]), Some("Not run yet: choose a list of states"));
}

#[test]
fn a_depending_on_waits_for_a_value_for_each_state() {
    let number = |value: f64| SheetValue::Literal(SheetLiteral::Number(value));
    let mut depending = SheetLine::new(LineKind::DependingOn);
    depending.property = Some("status".to_owned());
    depending.branches.insert("dead".to_owned(), number(0.0));
    depending.branches.insert("alive".to_owned(), number(1.0));
    let mut builder = builder_of("game_of_life.toml");
    let made = add_line(&mut builder, "cell_tick", None, depending);
    let report = check::check(builder.document());
    assert!(
        !report.has_errors() && !report.is_skipped(&made[0]),
        "{:?}",
        report.diagnostics
    );
    // A new state of the property leaves the line waiting for its value.
    run(
        &mut builder,
        SheetEdit::AddState {
            property: "status".to_owned(),
            name: "Zombie".to_owned(),
            color: None,
        },
    );
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skip_note(&made[0]), Some("Not run yet: choose a value"));
    // So does a new Depending on as soon as its property is chosen.
    let made = add_line(&mut builder, "cell_tick", None, SheetLine::new(LineKind::DependingOn));
    set_field(&mut builder, &made[0], LineField::Property(Some("status".to_owned())));
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.skip_note(&made[0]), Some("Not run yet: choose a value"));
}

/// A delete names the lines whose formulas it empties when the card cannot write them again: a formula inside a slot,
/// as the ants' Leave amounts, or one of more than one operation on a Calculate line. A formula of one operation on a
/// Calculate line, which the card rebuilds, names no line, and neither does the length of one of my properties in a
/// slot, which a picker offers, or a line the delete takes with it.
#[test]
fn a_delete_names_the_lines_whose_formulas_the_card_cannot_write_again() {
    let ants = builder_of("ants.toml");
    let losing = |builder: &SheetBuilder, edit: SheetEdit| lines_losing_formulas(builder.document(), &edit);
    let delete = |item: SheetItem| SheetEdit::Delete { item };
    let parameter = |id: &str| delete(SheetItem::Parameter(id.to_owned()));
    let property = |id: &str| delete(SheetItem::Property(id.to_owned()));
    assert_eq!(losing(&ants, parameter("trail_falloff")), ["l3", "l4"]);
    assert_eq!(losing(&ants, property("reward_held")), ["l3", "l4"]);
    assert_eq!(losing(&ants, SheetEdit::DeleteLine { line: "l2".to_owned() }), ["l3"]);
    // Line 3 holds its own formula, which goes with it.
    assert_eq!(
        losing(&ants, SheetEdit::DeleteLine { line: "l3".to_owned() }),
        Vec::<String>::new()
    );
    // Line 8 sets a random direction of length (length of my Velocity), which a picker offers again.
    let mut boids = builder_of("boids.toml");
    assert_eq!(losing(&boids, property("velocity")), Vec::<String>::new());
    let mut length = SheetLine::new(LineKind::Calculate);
    length.formula = Some("min_speed * 2".to_owned());
    set_slot(&mut boids, "l8", "value.length", phrase(length));
    assert_eq!(
        losing(&boids, parameter("min_speed")),
        ["l8"],
        "no picker offers a formula of one operation in a slot"
    );

    // Two Calculate lines on the SIR card, of one operation and of two.
    let mut sir = builder_of("sir.toml");
    let calculate = |formula: &str| {
        let mut line = SheetLine::new(LineKind::Calculate);
        line.formula = Some(formula.to_owned());
        line
    };
    let one = add_line(&mut sir, "cell_tick", None, calculate("infection_rate * 2"));
    let two = add_line(
        &mut sir,
        "cell_tick",
        None,
        calculate("infection_rate * 2 + recovery_rate"),
    );
    assert_eq!(losing(&sir, parameter("infection_rate")), two);
    assert_eq!(losing(&sir, parameter("recovery_rate")), two);
    assert_eq!(losing(&sir, parameter("initial_infected")), Vec::<String>::new());
    run(&mut sir, parameter("infection_rate"));
    assert_eq!(
        line(&sir, &one[0]).formula,
        None,
        "the card rebuilds the formula of one operation"
    );
}

/// A delete names the measures whose formula it empties, since the Sheet pane writes none, the length of one of my
/// properties included. A measure the delete takes with it is left out.
#[test]
fn a_delete_names_the_measures_whose_formulas_it_empties() {
    let measures = |file_name: &str, edit: SheetEdit| measures_losing_formulas(builder_of(file_name).document(), &edit);
    let property = |id: &str| SheetEdit::Delete {
        item: SheetItem::Property(id.to_owned()),
    };
    // Total trail adds up my.to_food + my.to_home over the ground.
    assert_eq!(measures("ants.toml", property("to_food")), ["total_trail"]);
    assert_eq!(measures("ants.toml", property("to_home")), ["total_trail"]);
    assert_eq!(measures("ants.toml", property("reward_held")), Vec::<String>::new());
    // Average speed reads length(my.velocity). Average velocity reads Velocity through a chip, which holds no formula.
    assert_eq!(measures("boids.toml", property("velocity")), ["average_speed"]);
    let delete_measure = SheetEdit::Delete {
        item: SheetItem::Measure("total_trail".to_owned()),
    };
    assert_eq!(measures("ants.toml", delete_measure), Vec::<String>::new());
}

/// A removed summary of a look at agents names the lines whose formulas read it, and not the look itself, whose
/// summary takes its own value with it.
#[test]
fn a_removed_summary_names_the_lines_that_read_it_and_not_its_own_look() {
    let mut boids = builder_of("boids.toml");
    let mut centre = SheetLine::new(LineKind::Calculate);
    centre.formula = Some("visual_range * 2 + 1".to_owned());
    set_slot(&mut boids, "l1", "summaries.1.value", phrase(centre));
    let mut reads = SheetLine::new(LineKind::Calculate);
    reads.formula = Some("length(l1.average_velocity) * 2".to_owned());
    let reads = add_line(&mut boids, "bird_tick", None, reads);
    let remove = |index: usize| SheetEdit::DeleteSummary {
        line: "l1".to_owned(),
        index,
    };
    assert_eq!(lines_losing_formulas(boids.document(), &remove(0)), reads);
    assert_eq!(
        lines_losing_formulas(boids.document(), &remove(1)),
        Vec::<String>::new(),
        "the centre's own value goes with it"
    );
}

#[test]
fn a_delete_that_empties_a_formula_in_a_slot_leaves_the_slot_blank() {
    // Lines 3 and 4 of the ants leave an amount worked out from Trail falloff.
    let mut builder = builder_of("ants.toml");
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::Parameter("trail_falloff".to_owned()),
        },
    );
    let report = check::check(builder.document());
    for id in ["l3", "l4"] {
        assert_eq!(line(&builder, id).amount, Slot::Blank, "{id}");
        assert_eq!(report.skip_note(id), Some("Not run yet: choose a number"), "{id}");
    }
    // The blank is placed at the slot, so the card opens its picker there.
    let sentences = Sentences::new(builder.document());
    let blank = sentences
        .placed_parts("l3", &[])
        .into_iter()
        .find(|placed| placed.part == SentencePart::Blank(Blank::Number))
        .expect("line 3 shows a blank");
    assert!(blank.place.is_some(), "{blank:?}");
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

/// A States property colours agents coloured by their heading, as it colours agents with no colour rule, and a Heading
/// deleted or given another kind takes the agents' rule by heading with it. A rule by state stays.
#[test]
fn a_rule_by_heading_gives_way_to_states_and_goes_with_the_heading() {
    let rule_of = |builder: &SheetBuilder| {
        builder
            .document()
            .colors
            .get("bird")
            .map(|rule| (rule.rule, rule.property.clone()))
    };
    let mut builder = builder_of("boids.toml");
    assert_eq!(rule_of(&builder), Some((ColorRuleKind::ByHeading, None)));
    let made = run(
        &mut builder,
        SheetEdit::AddProperty {
            thing: "bird".to_owned(),
            name: "Mood".to_owned(),
            property_type: PropertyType::States,
        },
    );
    assert_eq!(rule_of(&builder), Some((ColorRuleKind::ByState, Some(made[0].clone()))));
    run(
        &mut builder,
        SheetEdit::AddProperty {
            thing: "bird".to_owned(),
            name: "Age".to_owned(),
            property_type: PropertyType::States,
        },
    );
    assert_eq!(
        rule_of(&builder),
        Some((ColorRuleKind::ByState, Some(made[0].clone()))),
        "a second States property leaves the first one's colours"
    );
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::Property("heading".to_owned()),
        },
    );
    assert_eq!(
        rule_of(&builder),
        Some((ColorRuleKind::ByState, Some(made[0].clone()))),
        "a rule by state reads no Heading"
    );

    let mut builder = builder_of("boids.toml");
    run(
        &mut builder,
        SheetEdit::Delete {
            item: SheetItem::Property("heading".to_owned()),
        },
    );
    assert_eq!(rule_of(&builder), None, "the agents have no Heading to colour them by");
    // The boids lower with no Heading.
    lowered(&write(builder.document()));

    let mut builder = builder_of("boids.toml");
    run(
        &mut builder,
        SheetEdit::SetPropertyType {
            property: "heading".to_owned(),
            property_type: PropertyType::Number,
        },
    );
    assert_eq!(rule_of(&builder), None);
}

#[test]
fn a_rate_unit_comes_from_the_tick_until_chosen() {
    let mut builder = builder_of("sir.toml");
    let mut rate = SheetLine::new(LineKind::Rate);
    rate.rate = chip(SheetChip::Param("recovery_rate".to_owned()));
    let made = add_line(&mut builder, "cell_tick", None, rate);
    let added = line(&builder, &made[0]);
    assert_eq!(
        (added.rate_unit, added.rate_unit_from_tick),
        (Some(TimeUnit::Day), true)
    );
    assert!(added.key.is_some(), "a line that draws takes a key");
    let key = added.key;
    set_field(&mut builder, &made[0], LineField::RateUnit(Some(TimeUnit::Week)));
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
    let made = add_line(&mut builder, "cell_tick", None, SheetLine::new(LineKind::Rate));
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
    set_slot(
        &mut builder,
        &made[0],
        "rate",
        chip(SheetChip::Param("recovery_rate".to_owned())),
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
    let made = add_line(&mut builder, "cell_tick", None, SheetLine::new(LineKind::Rate));
    set_slot(
        &mut builder,
        &made[0],
        "rate",
        chip(SheetChip::Param("plain".to_owned())),
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
    let file = lowered(&write(document));
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
    let file = lowered(&written);
    assert!(
        file.lowered().to_toml().contains("combine = \"sum\""),
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
    set_slot(
        &mut builder,
        &choose,
        &format!("row.{rows}.value"),
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0))),
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
