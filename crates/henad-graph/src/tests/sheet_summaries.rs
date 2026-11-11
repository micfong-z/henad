//! The summaries of a Look around under edits: automatic names that follow their property and summary kind, names typed
//! by hand, new ids that reach every reference, removed summaries that blank their users, and the names of round 1.

use super::sheet_support::{add_line, builder_of, chip, line, read, run, set_field, set_slot, sheet_text, sheets};
use crate::sheet::document::{
    Around, ChipRelation, LineKind, LookSummary, SheetChip, SheetLine, SheetValue, Slot, SummaryKind,
};
use crate::sheet::edit::{LineField, SheetBuilder, SheetEdit, SheetEditError, automatic_summary_name};

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
    let made = add_line(&mut builder, "bird_tick", None, new_look());
    let look = made[0].clone();
    assert_eq!(
        summaries_of(&builder, &look),
        [("average".to_owned(), "average".to_owned())]
    );
    set_slot(
        &mut builder,
        &look,
        "summaries.0.value",
        chip(SheetChip::Their("velocity".to_owned())),
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
    set_slot(
        &mut builder,
        &look,
        "summaries.1.value",
        chip(SheetChip::Relation(ChipRelation::OffsetToThem)),
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
    let steer = add_line(&mut builder, "bird_tick", None, steer)[0].clone();
    let mut summaries = line(&builder, &look).summaries.clone();
    summaries[1].summary = Some(SummaryKind::Total);
    summaries[1].value = chip(SheetChip::Relation(ChipRelation::OffsetAwayFromThem));
    set_field(&mut builder, &look, LineField::Summaries(summaries));
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
    let look = add_line(&mut builder, "bird_tick", None, look)[0].clone();
    let mut steer = SheetLine::new(LineKind::Steer);
    steer.property = Some("velocity".to_owned());
    steer.value = chip(SheetChip::Result {
        line: look.clone(),
        part: Some("push".to_owned()),
        property: None,
    });
    let steer = add_line(&mut builder, "bird_tick", None, steer)[0].clone();

    // A name typed by hand stays, and so does its id. A second average of nothing takes "average 2".
    let mut summaries = line(&builder, &look).summaries.clone();
    summaries[0].name = "flock speed".to_owned();
    set_field(&mut builder, &look, LineField::Summaries(summaries));
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
    set_slot(
        &mut builder,
        "l1",
        "summaries.0.value",
        chip(SheetChip::Relation(ChipRelation::OffsetToThem)),
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
    set_field(&mut builder, "l1", LineField::Summaries(summaries));
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
