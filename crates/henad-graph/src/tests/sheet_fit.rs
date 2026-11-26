//! The checker's reading of one value in one place of a line: the kind it gives, and why a place refuses it, which a
//! card's picker shows on each value it offers.

use super::sheet_support::{boids_with, read, sheet_text};
use crate::sheet::check::{ValueKind, check, place_refusal, value_kind};
use crate::sheet::document::{LineKind, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetValue, Slot};
use crate::sheet::sentence::{self, PlaceTarget};
use crate::sheet::{ParsedSheet, SheetFile};

/// Returns the chip of the parameter `id`.
fn param(id: &str) -> SheetValue {
    SheetValue::Chip(SheetChip::Param(id.to_owned()))
}

/// Returns the chip of the part `part` of the result of line `line`.
fn part(line: &str, part: &str) -> SheetValue {
    SheetValue::Chip(SheetChip::Result {
        line: line.to_owned(),
        part: Some(part.to_owned()),
        property: None,
    })
}

/// Returns a random number between `low` and `high`.
fn random_number(low: f64, high: f64) -> SheetValue {
    let mut phrase = SheetLine::new(LineKind::RandomNumber);
    phrase.low = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(low)));
    phrase.high = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(high)));
    SheetValue::Phrase(Box::new(phrase))
}

/// Returns the boids starter's document.
fn boids() -> SheetDocument {
    read(sheet_text("boids.toml"))
}

/// Returns the place of the slot `path`.
fn slot(path: &str) -> PlaceTarget {
    PlaceTarget::Slot(path.to_owned())
}

#[test]
fn a_value_gives_the_kind_the_checker_reads() {
    let boids = boids();
    let random_direction = SheetValue::Phrase(Box::new(SheetLine::new(LineKind::RandomDirection)));
    for (value, kind) in [
        (part("l1", "centre"), ValueKind::Arrow),
        (part("l1", "average_velocity"), ValueKind::Arrow),
        (param("visual_range"), ValueKind::Number),
        (SheetValue::Chip(SheetChip::My("velocity".to_owned())), ValueKind::Arrow),
        (
            SheetValue::Chip(SheetChip::MyNew("velocity".to_owned())),
            ValueKind::Arrow,
        ),
        (random_number(1.0, 2.0), ValueKind::Number),
        (random_direction, ValueKind::Arrow),
        (SheetValue::Literal(SheetLiteral::Bool(true)), ValueKind::YesNo),
    ] {
        assert_eq!(value_kind(&boids, "l7", &value), Some(kind), "{value:?}");
    }
}

#[test]
fn the_distance_of_a_look_at_agents_takes_a_typed_number_or_a_parameter_alone() {
    let boids = boids();
    let distance = slot("distance");
    assert_eq!(place_refusal(&boids, "l1", &distance, &param("protected_range")), None);
    assert_eq!(
        place_refusal(&boids, "l1", &distance, &SheetValue::Literal(SheetLiteral::Number(5.0))),
        None
    );
    let refusal = place_refusal(&boids, "l1", &distance, &random_number(20.0, 60.0)).expect("the random distance");
    assert_eq!(refusal.message, sentence::DISTANCE_NOT_FIXED);
    assert_eq!(refusal.kinds, None);
    assert_eq!(refusal.reason("a random number"), sentence::DISTANCE_NOT_FIXED);
    // The checker refuses the random distance on the sheet itself, so the lowering never meets it.
    let text = sheet_text("boids.toml").replace(
        "distance = { param = \"visual_range\" }",
        "distance = { kind = \"random_number\", low = 20, high = 60 }",
    );
    let parsed = ParsedSheet::parse(&text).expect("the sheet reads");
    let report = parsed.check();
    let errors: Vec<(&str, Option<&str>)> = report
        .errors()
        .map(|error| (error.diagnostic.message.as_str(), error.line.as_deref()))
        .collect();
    assert_eq!(errors, [(sentence::DISTANCE_NOT_FIXED, Some("l1"))]);
    assert!(SheetFile::lowering_problems(&parsed, &report).is_empty());
}

#[test]
fn a_value_of_another_kind_names_both_kinds_and_the_line_that_needs_it() {
    let moving = read(&boids_with("[card.line.l9]\nkind = \"move\"\nmode = \"to\""));
    let refusal = place_refusal(&moving, "l9", &slot("value"), &part("l1", "centre")).expect("a vector refused");
    assert_eq!(refusal.kinds, Some((ValueKind::Place, ValueKind::Arrow)));
    assert_eq!(
        refusal.reason("Flockmates' centre"),
        "Flockmates' centre is a vector, and Move to needs a cell."
    );
    // Set Velocity takes a vector, and a strength a number.
    let boids = boids();
    let refusal = place_refusal(&boids, "l8", &slot("value"), &param("visual_range")).expect("a number refused");
    assert_eq!(
        refusal.reason("Visual range"),
        "Visual range is a number, and Set Velocity needs a vector."
    );
    let refusal = place_refusal(&boids, "l3", &slot("strength"), &part("l1", "centre")).expect("a vector refused");
    assert_eq!(
        refusal.reason("Flockmates' centre"),
        "Flockmates' centre is a vector. Choose a number here."
    );
    assert_eq!(place_refusal(&boids, "l3", &slot("strength"), &param("cohesion")), None);
    // A Steer of Velocity takes a vector, a part of a look above among them.
    assert_eq!(place_refusal(&boids, "l3", &slot("value"), &part("l1", "centre")), None);
    // A field takes no value through the trial.
    let field = PlaceTarget::Field {
        phrase: String::new(),
        field: sentence::FieldPlace::Property,
    };
    assert_eq!(place_refusal(&boids, "l3", &field, &param("cohesion")), None);
    // The sheet as it is has no error the trial could take for the value's.
    assert!(!check(&boids).has_errors());
}
