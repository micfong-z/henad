//! Random command sequences over every sheet: each command a builder accepts leaves a document whose written text reads
//! back to it, that the checker takes without a panic, and in which no two lines share a key.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::metadata::TimeUnit;

use super::sheet_support::{chip, phrase, read, result, sheets, write};
use super::support::below;
use crate::sheet::ParsedSheet;
use crate::sheet::check;
use crate::sheet::document::{
    Around, ButtonStepRule, CompareTest, Edges, LineKind, Marks, MeasureRule, ParameterType, PropertyType, SheetChip,
    SheetDocument, SheetLine, SheetLiteral, SheetTime, SheetValue, Slot, SummaryKind,
};
use crate::sheet::edit::{LineField, LineSide, MoveDirection, STATE_COLORS, SheetBuilder, SheetEdit, SheetItem};
use crate::sheet::sentence::ConditionItem;

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
        6 => pick(rng, lines).map_or(Slot::Blank, |id| chip(result(id))),
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
    match below(rng, 32) {
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
        29 => SheetEdit::InsertLine {
            anchor: line,
            side: [LineSide::Above, LineSide::Below][below(rng, 2)],
            line: Box::new(SheetLine::new(
                [LineKind::Become, LineKind::When, LineKind::Calculate][below(rng, 3)],
            )),
        },
        30 => SheetEdit::DuplicateLine { line },
        31 => SheetEdit::DeleteRow {
            line,
            index: below(rng, 3),
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
