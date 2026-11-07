//! The Sheet pane: what a sheet model holds, read-only, one section per heading of
//! [`SheetSection`](henad_graph::sheet::sentence::SheetSection), each heading with its "?" help.
//!
//! Purpose, World (with the length of one tick), Cells (each property with its kind, its start and the lines that
//! change it), Parameters, Measures, Colours and Buttons.

use egui::RichText;
use henad_graph::sheet::document::{
    ButtonStepRule, CardRuns, ColorRuleKind, Edges, LineKind, MadeOf, PropertyType, SheetChip, SheetDocument,
    SheetProperty, SheetValue, StartRule,
};
use henad_graph::sheet::format_number;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    A_RANDOM_NUMBER, A_VALUE, AGENTS_IN_OPEN_SPACE, BY_CHANCE, CELLS_COLUMNS, CELLS_ON_A_GRID, COLOR_CELLS_BY,
    EDGES_LABEL, NUMBER_KIND, ONE_TICK_IS, SIZE_BY, SIZE_LABEL, START_AGAIN, STATES_KIND, SheetSection, WALLS,
    WALLS_SENTENCE, WRAP_AROUND, WRAP_SENTENCE, changed_by, run_once, starts_all, tick_unit_choice,
};

use super::help::{Help, help_button};
use crate::ui::kv_grid;

/// Draws every section of `document`, in the order of [`SheetSection::ALL`], each heading with its help.
pub fn sheet_pane(ui: &mut egui::Ui, document: &SheetDocument) {
    for section in SheetSection::ALL {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(section.heading()).strong());
            help_button(ui, Help::Section(section));
        });
        match section {
            SheetSection::Purpose => purpose(ui, document),
            SheetSection::World => world(ui, document),
            SheetSection::Cells => cells(ui, document),
            SheetSection::Parameters => parameters(ui, document),
            SheetSection::Measures => measures(ui, document),
            SheetSection::Colours => colours(ui, document),
            SheetSection::Buttons => buttons(ui, document),
            _ => {}
        }
    }
}

/// Draws what the model is for.
fn purpose(ui: &mut egui::Ui, document: &SheetDocument) {
    if let Some(purpose) = &document.sheet.purpose {
        ui.label(purpose);
    }
}

/// Draws the world's kind, size and edges, and the length of one tick.
fn world(ui: &mut egui::Ui, document: &SheetDocument) {
    let world = &document.world;
    ui.label(match world.made_of {
        MadeOf::Agents => AGENTS_IN_OPEN_SPACE,
        _ => CELLS_ON_A_GRID,
    });
    ui.label(format!("{SIZE_LABEL} {} {SIZE_BY} {}", world.width, world.height));
    let (edges, sentence) = match world.edges {
        Edges::Walls => (WALLS, WALLS_SENTENCE),
        _ => (WRAP_AROUND, WRAP_SENTENCE),
    };
    ui.label(format!("{EDGES_LABEL} {edges}"));
    ui.weak(sentence);
    ui.label(tick_text(document));
}

/// Returns the length of one tick as the World section reads it: "One tick is one step (no unit)", "One tick is 1
/// day" or "One tick is 0.5 days".
pub fn tick_text(document: &SheetDocument) -> String {
    match &document.time {
        None => format!("{ONE_TICK_IS} {}", tick_unit_choice(None)),
        Some(time) => {
            let length = format_number(time.tick_length).unwrap_or_default();
            let unit = if time.tick_length == 1.0 {
                time.tick_unit.as_str()
            } else {
                time.tick_unit.plural()
            };
            format!("{ONE_TICK_IS} {length} {unit}")
        }
    }
}

/// Draws the Cells table: each property's name, kind, start and the lines that change it.
fn cells(ui: &mut egui::Ui, document: &SheetDocument) {
    let Some(properties) = &document.cells else {
        return;
    };
    egui::Grid::new("henad_sheet_cells").num_columns(4).show(ui, |ui| {
        for column in CELLS_COLUMNS {
            ui.weak(column);
        }
        ui.end_row();
        for property in properties {
            ui.label(&property.name);
            ui.label(kind_text(property));
            ui.label(starts_text(property));
            ui.label(changed_by(&changing_lines(document, &property.id)));
            ui.end_row();
        }
    });
}

/// Returns the kind of `property`: its states for a States property, such as "States Dead, Alive", or "Number".
fn kind_text(property: &SheetProperty) -> String {
    match property.property_type {
        PropertyType::States => {
            let names: Vec<&str> = property.states.iter().map(|state| state.name.as_str()).collect();
            format!("{STATES_KIND} {}", names.join(", "))
        }
        _ => NUMBER_KIND.to_owned(),
    }
}

/// Returns how `property` starts: "All Dead", "By chance", "A value" or "A random number".
///
/// A start of another rule returns an empty string. Only the ground or the agents of an agent world take one.
pub fn starts_text(property: &SheetProperty) -> String {
    let Some(starts) = &property.starts_as else {
        return String::new();
    };
    match starts.rule {
        StartRule::ByChance => BY_CHANCE.to_owned(),
        StartRule::Value => match starts.value.value() {
            Some(SheetValue::Chip(SheetChip::State(state))) => match state_name(property, state) {
                Some(name) => starts_all(name),
                None => A_VALUE.to_owned(),
            },
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber => A_RANDOM_NUMBER.to_owned(),
            _ => A_VALUE.to_owned(),
        },
        _ => String::new(),
    }
}

/// Returns the name of state `state_id` of `property`.
fn state_name<'a>(property: &'a SheetProperty, state_id: &str) -> Option<&'a str> {
    property
        .states
        .iter()
        .find(|state| state.id == state_id)
        .map(|state| state.name.as_str())
}

/// Returns the numbers of the lines that change property `property_id` on the cards that run each tick.
fn changing_lines(document: &SheetDocument, property_id: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    for card in document.cards.iter().filter(|card| card.runs == CardRuns::EachTick) {
        let relations = CardRelations::new(document, card);
        for &line_id in relations.order() {
            let changes = relations.line(line_id).is_some_and(|line| {
                matches!(line.kind, LineKind::Become | LineKind::Set) && line.property.as_deref() == Some(property_id)
            });
            if changes && let Some(number) = relations.number(line_id) {
                numbers.push(number);
            }
        }
    }
    numbers
}

/// Draws each parameter's name and value, with its unit.
fn parameters(ui: &mut egui::Ui, document: &SheetDocument) {
    if document.parameters.is_empty() {
        return;
    }
    kv_grid(ui, "henad_sheet_parameters").show(ui, |ui, rows| {
        for parameter in &document.parameters {
            ui.label(&parameter.name);
            let value = format_number(parameter.default).unwrap_or_default();
            match &parameter.unit {
                Some(unit) => ui.label(format!("{value} {unit}")),
                None => ui.label(value),
            };
            rows.end_row(ui);
        }
    });
}

/// Draws each measure's name.
fn measures(ui: &mut egui::Ui, document: &SheetDocument) {
    for measure in &document.measures {
        ui.label(&measure.name);
    }
}

/// Draws the property that colours the cells.
fn colours(ui: &mut egui::Ui, document: &SheetDocument) {
    let Some(rule) = document.colors.get("cells") else {
        return;
    };
    let property = rule.property.as_deref().and_then(|id| {
        document
            .cells
            .iter()
            .flatten()
            .find(|property| property.id == id)
            .map(|property| property.name.as_str())
    });
    if let (ColorRuleKind::ByState | ColorRuleKind::Shaded, Some(property)) = (rule.rule, property) {
        ui.label(format!("{COLOR_CELLS_BY} {property}"));
    }
}

/// Draws each button's name and what it does.
fn buttons(ui: &mut egui::Ui, document: &SheetDocument) {
    for button in &document.buttons {
        let does: Vec<String> = button
            .does
            .iter()
            .map(|step| match step.rule {
                ButtonStepRule::StartAgain => START_AGAIN.to_owned(),
                ButtonStepRule::RunOnce => run_once("cell"),
                _ => SET_EVERYWHERE.to_owned(),
            })
            .collect();
        ui.label(format!("{}: {}", button.name, does.join(", ")));
    }
}

/// What a button step does that sets one property everywhere. No control of a grid sheet writes such a step.
const SET_EVERYWHERE: &str = "Set everywhere";
