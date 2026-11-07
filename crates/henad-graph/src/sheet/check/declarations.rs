//! Tables outside the cards: the header, the world, the time scale, ids and their namespaces, properties and how they
//! start, parameters, measures, colour rules and buttons.

use std::collections::{HashMap, HashSet};

use super::lines::{Context, Draw, LineState, ValueType, Want, line_number, value_draws, value_rates};
use super::{Checker, Place, Thing};
use crate::ids::is_id;
use crate::sheet::SHEET_VERSION;
use crate::sheet::document::{
    ButtonStepRule, CardRuns, ColorRuleKind, MadeOf, MeasureRule, PropertyType, SheetDocument, SheetProperty, StartRule,
};
use crate::sheet::format_number;
use crate::sheet::limits::MAX_SHEET_ID_BYTES;
use crate::sheet::sentence::{ANY_STATE_ID, Blank, ONE_AGENT_KIND, SHEET_EMPTY};

/// Ids a sheet item cannot take: the words of a formula, the parts of a result and the chips of mine.
pub(crate) const RESERVED_IDS: [&str; 9] = [
    "my",
    "my_new",
    "length",
    "direction",
    "smaller",
    "larger",
    "amount",
    "open",
    "count",
];

/// Parameter ids the lowered model holds of its own: the world's parameters and the length of one tick.
pub(crate) const RESERVED_PARAMETER_IDS: [&str; 6] = [
    "grid_width",
    "grid_height",
    "num_agents",
    "world_width",
    "world_height",
    "tick_length",
];

/// Prefix of the ids the lowered model holds of its own.
pub(crate) const RESERVED_PREFIX: &str = "henad_";

/// Checks every table outside the cards.
pub(super) fn check(checker: &mut Checker<'_>) {
    check_header(checker);
    check_world(checker);
    check_ids(checker);
    check_properties(checker);
    check_parameters(checker);
    check_measures(checker);
    check_colors(checker);
    check_buttons(checker);
}

/// Checks the `[sheet]` table and the `[time]` table.
fn check_header(checker: &mut Checker<'_>) {
    let header = &checker.document.sheet;
    if !(1..=SHEET_VERSION).contains(&header.version) {
        checker.error(
            format!(
                "the sheet is version {}, and this build reads versions 1 to {SHEET_VERSION}",
                header.version
            ),
            &Place::Sheet,
        );
    }
    claim_grammar(checker, "sheet", &header.id, &Place::Sheet);
    if header.next_line < 1 {
        checker.error("next_line counts from 1", &Place::Sheet);
    }
    if let Some(time) = &checker.document.time
        && !(time.tick_length.is_finite() && time.tick_length > 0.0)
    {
        checker.error("one tick lasts a number of units above 0", &Place::Time);
    }
}

/// Checks the `[world]` table and the kinds of thing it holds.
fn check_world(checker: &mut Checker<'_>) {
    let document = checker.document;
    let world = &document.world;
    if world.width == 0 || world.height == 0 {
        checker.error("the world is at least 1 cell wide and 1 cell high", &Place::World);
    }
    match world.made_of {
        MadeOf::Cells => {
            if !document.agents.is_empty() || document.ground.is_some() {
                checker.error("a world of cells has no agents and no ground", &Place::World);
            }
            if document.cells.as_ref().is_none_or(Vec::is_empty) {
                checker.warning(SHEET_EMPTY, &Place::World);
            }
        }
        MadeOf::Agents => {
            if document.cells.is_some() {
                checker.error(
                    "a world of agents has no [cells], and its cells are the ground",
                    &Place::World,
                );
            }
        }
    }
    for (index, kind) in document.agents.iter().enumerate() {
        let place = Place::AgentKind(index);
        if index > 0 {
            checker.error(ONE_AGENT_KIND, &place);
        }
        if !(1..=kind.max_count).contains(&kind.count) {
            checker.error(
                format!("the agents '{}' need a count from 1 to max_count", kind.id),
                &place,
            );
        }
    }
}

/// Checks the grammar and the length of `id`, the id of a `what` placed at `place`, and returns whether both hold.
fn claim_grammar(checker: &mut Checker<'_>, what: &str, id: &str, place: &Place) -> bool {
    if !is_id(id) {
        checker.error(format!("the {what} id '{id}' does not follow the id grammar"), place);
        return false;
    }
    if id.len() > MAX_SHEET_ID_BYTES {
        checker.error(
            format!(
                "the {what} id '{id}' holds {} bytes, and a sheet id holds at most {MAX_SHEET_ID_BYTES}",
                id.len()
            ),
            place,
        );
        return false;
    }
    true
}

/// Checks the id of a summary of a Look around line placed at `place`: its grammar, its length, and whether it is a
/// reserved word.
///
/// A chip and a formula name a summary as a part of its line's result, beside the parts `count`, `amount` and `open`.
pub(super) fn claim_summary(checker: &mut Checker<'_>, id: &str, place: &Place) {
    if claim_grammar(checker, "summary", id, place) && (RESERVED_IDS.contains(&id) || line_number(id).is_some()) {
        checker.error(format!("the summary id '{id}' is reserved"), place);
    }
}

/// A namespace of sheet ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Namespace {
    /// Properties, parameters, measures and events. The lowered model holds them side by side.
    Item,
    /// Kinds of thing.
    Thing,
    /// Buttons.
    Button,
    /// Cards.
    Card,
}

/// Ids claimed so far, each with what holds it, by namespace and id.
struct Claims<'a> {
    /// What holds each id, such as "parameter".
    seen: HashMap<(Namespace, &'a str), &'static str>,
}

impl<'a> Claims<'a> {
    /// Claims `id` in `namespace` for a `what` placed at `place`, and checks its grammar, its length, whether it is
    /// reserved and whether another item holds it.
    fn claim(
        &mut self,
        checker: &mut Checker<'_>,
        id: &'a str,
        what: &'static str,
        namespace: Namespace,
        place: &Place,
    ) {
        if !claim_grammar(checker, what, id, place) {
            return;
        }
        let reserved = RESERVED_IDS.contains(&id)
            || line_number(id).is_some()
            || namespace == Namespace::Item
                && (RESERVED_PARAMETER_IDS.contains(&id) || id.starts_with(RESERVED_PREFIX))
            || namespace == Namespace::Thing && matches!(id, "cells" | "ground");
        if reserved {
            checker.error(format!("the {what} id '{id}' is reserved"), place);
        }
        match self.seen.get(&(namespace, id)) {
            Some(other) => checker.error(format!("the {what} id '{id}' is also the id of a {other}"), place),
            None => {
                self.seen.insert((namespace, id), what);
            }
        }
    }
}

/// Checks every id and namespace: grammar, length, reserved ids and ids held twice.
fn check_ids(checker: &mut Checker<'_>) {
    let document = checker.document;
    let mut claims = Claims { seen: HashMap::new() };
    for (index, kind) in document.agents.iter().enumerate() {
        claims.claim(
            checker,
            &kind.id,
            "kind of thing",
            Namespace::Thing,
            &Place::AgentKind(index),
        );
    }
    for (thing, properties) in thing_properties(document) {
        for (index, property) in properties.iter().enumerate() {
            let place = Place::Property(thing, index);
            claims.claim(checker, &property.id, "property", Namespace::Item, &place);
            check_states(checker, property, &place);
        }
    }
    for (index, parameter) in document.parameters.iter().enumerate() {
        claims.claim(
            checker,
            &parameter.id,
            "parameter",
            Namespace::Item,
            &Place::Parameter(index),
        );
    }
    for (index, measure) in document.measures.iter().enumerate() {
        claims.claim(checker, &measure.id, "measure", Namespace::Item, &Place::Measure(index));
    }
    for (index, event) in document.events.iter().enumerate() {
        claims.claim(checker, &event.id, "event", Namespace::Item, &Place::Event(index));
    }
    for (index, button) in document.buttons.iter().enumerate() {
        claims.claim(checker, &button.id, "button", Namespace::Button, &Place::Button(index));
    }
    for (index, card) in document.cards.iter().enumerate() {
        claims.claim(checker, &card.id, "card", Namespace::Card, &Place::Card(index));
    }
}

/// Checks the state ids of `property`: grammar, length, the reserved `any_state` and ids held twice.
fn check_states(checker: &mut Checker<'_>, property: &SheetProperty, place: &Place) {
    let mut ids = HashSet::new();
    for state in &property.states {
        if !claim_grammar(checker, "state", &state.id, place) {
            continue;
        }
        if state.id == ANY_STATE_ID {
            checker.error(format!("the state id '{ANY_STATE_ID}' is reserved"), place);
        }
        if !ids.insert(state.id.as_str()) {
            checker.error(
                format!("property '{}' has two states with the id '{}'", property.id, state.id),
                place,
            );
        }
    }
}

/// Returns each kind of thing of `document` with its properties: the cells, each kind of agent, then the ground.
fn thing_properties(document: &SheetDocument) -> Vec<(Thing, &[SheetProperty])> {
    let mut out: Vec<(Thing, &[SheetProperty])> = Vec::new();
    if let Some(cells) = &document.cells {
        out.push((Thing::Cells, cells));
    }
    for (index, kind) in document.agents.iter().enumerate() {
        out.push((Thing::Agents(index), &kind.properties));
    }
    if let Some(ground) = &document.ground {
        out.push((Thing::Ground, &ground.properties));
    }
    out
}

/// Returns the id of the kind of thing `thing`.
fn thing_id(document: &SheetDocument, thing: Thing) -> &str {
    match thing {
        Thing::Cells => "cells",
        Thing::Agents(index) => document.agents.get(index).map_or("", |kind| kind.id.as_str()),
        Thing::Ground => "ground",
    }
}

/// Returns whether `id` names a kind of thing of `document`.
fn is_thing(document: &SheetDocument, id: &str) -> bool {
    id == "cells" && document.cells.is_some()
        || id == "ground" && document.ground.is_some()
        || document.agents.iter().any(|kind| kind.id == id)
}

/// Checks every property: its type, its states, its marks and how it starts.
fn check_properties(checker: &mut Checker<'_>) {
    let document = checker.document;
    for (thing, properties) in thing_properties(document) {
        let thing_id = thing_id(document, thing);
        for (index, property) in properties.iter().enumerate() {
            check_property(checker, property, thing_id, &Place::Property(thing, index));
        }
    }
}

/// Checks `property`, a property of the kind of thing `thing`.
fn check_property<'a>(checker: &mut Checker<'a>, property: &'a SheetProperty, thing: &'a str, place: &Place) {
    let id = property.id.as_str();
    let kind = property.property_type;
    if (kind == PropertyType::States) == property.states.is_empty() {
        checker.error(
            format!("property '{id}': a States property, and only one, lists states"),
            place,
        );
    }
    if matches!(kind, PropertyType::Position | PropertyType::Heading) && matches!(thing, "cells" | "ground") {
        checker.error(format!("property '{id}': only agents have a {}", kind.as_str()), place);
    }
    let context = Context::outside(place.clone(), thing);
    if property.marks.is_some() || !property.kept_per_tick.is_blank() {
        if thing != "ground" || kind != PropertyType::Number {
            checker.error(
                format!("property '{id}': marks and fading belong to a Number property of the ground"),
                place,
            );
        }
        if property.marks.is_none() {
            checker.error(format!("property '{id}' fades and names no marks rule"), place);
        }
        checker.slot(
            &property.kept_per_tick,
            &context,
            Want::Blank(Blank::Number),
            Some(ValueType::Number),
            None,
            &mut LineState::default(),
        );
    }
    check_start(checker, property, thing, place);
}

/// Checks how `property`, a property of the kind of thing `thing`, starts.
fn check_start<'a>(checker: &mut Checker<'a>, property: &'a SheetProperty, thing: &'a str, place: &Place) {
    let id = property.id.as_str();
    let kind = property.property_type;
    let context = Context::outside(place.clone(), thing);
    let Some(start) = &property.starts_as else {
        if kind != PropertyType::Heading {
            checker.error(format!("property '{id}' does not say how it starts"), place);
        }
        return;
    };
    let mut draws = false;
    match start.rule {
        StartRule::Value => {
            checker.slot(
                &start.value,
                &context,
                Want::Blank(Blank::Value),
                None,
                Some(id),
                &mut LineState::default(),
            );
            draws = start.value.value().is_some_and(value_draws);
            if start.value.value().is_some_and(value_rates) {
                checker.error(
                    format!("property '{id}' starts with a rate, and a start takes chances"),
                    place,
                );
            }
        }
        StartRule::ByChance => {
            draws = true;
            if kind != PropertyType::States {
                checker.error(format!("property '{id}': by chance starts a States property"), place);
            }
            for row in &start.rows {
                match row.state.as_deref() {
                    Some(state) if checker.has_state(id, state) => {}
                    state => checker.error(
                        format!(
                            "property '{id}' starts by chance in '{}', which is no state of it",
                            state.unwrap_or("")
                        ),
                        place,
                    ),
                }
                if row.chance.value().is_some_and(value_rates) {
                    checker.error(
                        format!("property '{id}' starts with a rate, and a start takes chances"),
                        place,
                    );
                }
                checker.slot(
                    &row.chance,
                    &context,
                    Want::Blank(Blank::Chance),
                    Some(ValueType::Number),
                    None,
                    &mut LineState::default(),
                );
                checker.draw_range(&row.chance, &context, Draw::Chance);
            }
            check_state_field(checker, id, start.otherwise.as_deref(), "otherwise", place);
        }
        StartRule::Painted => {
            check_state_field(checker, id, start.base.as_deref(), "base", place);
            for shape in &start.shapes {
                check_state_field(checker, id, Some(&shape.state), "shape", place);
            }
        }
        StartRule::Anywhere => {
            draws = true;
            if kind != PropertyType::Position {
                checker.error(format!("property '{id}': anywhere starts a Position"), place);
            }
        }
        StartRule::OnGround => {
            if kind != PropertyType::Position {
                checker.error(format!("property '{id}': on the ground starts a Position"), place);
            }
            let ground = start.property.as_deref().unwrap_or("");
            if checker.property_of(ground, "ground").is_none() {
                checker.error(
                    format!("property '{id}' starts on '{ground}', which is no ground property"),
                    place,
                );
            } else {
                check_state_field(checker, ground, start.state.as_deref(), "state", place);
            }
        }
    }
    checker.key(start.key, format!("the start of property '{id}'"), draws, place);
}

/// Checks that `state`, the field `field` of a start, is a state of property `property`.
fn check_state_field(checker: &mut Checker<'_>, property: &str, state: Option<&str>, field: &str, place: &Place) {
    match state {
        Some(state) if checker.has_state(property, state) => {}
        Some(state) => checker.error(format!("the {field} '{state}' is no state of '{property}'"), place),
        None => checker.error(format!("the start of '{property}' names no {field} state"), place),
    }
}

/// Checks every parameter's default against its range.
fn check_parameters(checker: &mut Checker<'_>) {
    let document = checker.document;
    for (index, parameter) in document.parameters.iter().enumerate() {
        let (min, max, default) = (parameter.min, parameter.max, parameter.default);
        if !(min <= default && default <= max) {
            let number = |value: f64| format_number(value).unwrap_or_else(|| value.to_string());
            checker.error(
                format!(
                    "the default {} of parameter '{}' is outside its range, {} to {}",
                    number(default),
                    parameter.id,
                    number(min),
                    number(max)
                ),
                &Place::Parameter(index),
            );
        }
    }
}

/// Checks every measure: what it counts or reads, and over which kind of thing.
fn check_measures(checker: &mut Checker<'_>) {
    let document = checker.document;
    for (index, measure) in document.measures.iter().enumerate() {
        let place = Place::Measure(index);
        let id = measure.id.as_str();
        if measure.rule == MeasureRule::EventCount {
            match measure.event.as_deref() {
                Some(event) if checker.events.contains(event) => {}
                event => checker.error(
                    format!(
                        "measure '{id}' counts the event '{}', which does not exist",
                        event.unwrap_or("")
                    ),
                    &place,
                ),
            }
            continue;
        }
        let Some(over) = measure.over.as_deref().filter(|over| is_thing(document, over)) else {
            checker.error(
                format!(
                    "measure '{id}' is over '{}', which is no kind of thing",
                    measure.over.as_deref().unwrap_or("")
                ),
                &place,
            );
            continue;
        };
        if measure.rule == MeasureRule::CountInState {
            let property = measure.property.as_deref().unwrap_or("");
            if checker.property_of(property, over).is_none() {
                checker.error(
                    format!("measure '{id}' counts '{property}', which is no property of '{over}'"),
                    &place,
                );
            } else {
                match measure.state.as_deref() {
                    Some(state) if checker.has_state(property, state) => {}
                    state => checker.error(
                        format!(
                            "measure '{id}' counts the state '{}', which is no state of '{property}'",
                            state.unwrap_or("")
                        ),
                        &place,
                    ),
                }
            }
        } else {
            let context = Context::outside(place.clone(), over);
            checker.slot(
                &measure.value,
                &context,
                Want::Blank(Blank::Value),
                None,
                None,
                &mut LineState::default(),
            );
        }
    }
}

/// Checks every colour rule against the kind of thing it colours.
fn check_colors(checker: &mut Checker<'_>) {
    let document = checker.document;
    for (thing, rule) in &document.colors {
        let place = Place::Color(thing.clone());
        if !is_thing(document, thing) {
            checker.error(format!("the color rule of '{thing}' colors no kind of thing"), &place);
            continue;
        }
        match rule.rule {
            ColorRuleKind::ByState => {
                let property = rule.property.as_deref().unwrap_or("");
                let states = checker
                    .property_of(property, thing)
                    .filter(|found| found.property_type == PropertyType::States);
                if states.is_none() {
                    checker.error(
                        format!("the color rule of '{thing}' reads '{property}', which is no States property of it"),
                        &place,
                    );
                } else if let Some(state) = rule.shade_state.as_deref()
                    && !checker.has_state(property, state)
                {
                    checker.error(
                        format!("the color rule of '{thing}' shades '{state}', which is no state"),
                        &place,
                    );
                }
                for shade in &rule.shades {
                    if checker.property_of(&shade.property, thing).is_none() {
                        checker.error(
                            format!(
                                "the color rule of '{thing}' shades '{}', which is no property of it",
                                shade.property
                            ),
                            &place,
                        );
                    }
                }
            }
            ColorRuleKind::ByHeading => {
                if matches!(thing.as_str(), "cells" | "ground") {
                    checker.error(
                        format!("the color rule of '{thing}' reads a heading, which only agents have"),
                        &place,
                    );
                }
            }
            ColorRuleKind::Shaded => {
                if let Some(property) = rule.property.as_deref()
                    && checker.property_of(property, thing).is_none()
                {
                    checker.error(
                        format!("the color rule of '{thing}' shades '{property}', which is no property of it"),
                        &place,
                    );
                }
            }
        }
    }
}

/// Checks every button's steps.
fn check_buttons(checker: &mut Checker<'_>) {
    let document = checker.document;
    for (index, button) in document.buttons.iter().enumerate() {
        let place = Place::Button(index);
        let id = button.id.as_str();
        for step in &button.does {
            match step.rule {
                ButtonStepRule::StartAgain => {
                    if let Some(thing) = step.thing.as_deref()
                        && !is_thing(document, thing)
                    {
                        checker.error(
                            format!("button '{id}' starts '{thing}' again, which is no kind of thing"),
                            &place,
                        );
                    }
                    for property in &step.properties {
                        if !checker.properties.contains_key(property.as_str()) {
                            checker.error(
                                format!("button '{id}' starts '{property}' again, which is no property"),
                                &place,
                            );
                        }
                    }
                }
                ButtonStepRule::SetEverywhere => {
                    let property = step.property.as_deref().unwrap_or("");
                    match checker.properties.get(property).copied() {
                        None => checker.error(format!("button '{id}' sets '{property}', which is no property"), &place),
                        Some((_, owner)) => {
                            let context = Context::outside(place.clone(), owner);
                            checker.slot(
                                &step.value,
                                &context,
                                Want::Blank(Blank::Value),
                                None,
                                Some(property),
                                &mut LineState::default(),
                            );
                        }
                    }
                }
                ButtonStepRule::RunOnce => {
                    let card = step.card.as_deref().unwrap_or("");
                    let runs_once = checker
                        .cards
                        .get(card)
                        .is_some_and(|found| found.runs == CardRuns::Once);
                    if !runs_once {
                        checker.error(
                            format!("button '{id}' runs '{card}', which is no card that runs once"),
                            &place,
                        );
                    }
                }
            }
        }
    }
}
