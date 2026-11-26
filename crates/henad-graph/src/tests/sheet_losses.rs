//! What an edit takes away from a sheet besides the item it deletes or sets: the finder behind the question the editor
//! asks before such an edit.

use super::sheet_support::{builder_of, run};
use crate::sheet::document::{ColorRule, ColorRuleKind, LineKind, PropertyType, SheetLine, StartRule, StartsAs};
use crate::sheet::edit::{SheetEdit, SheetItem, SheetLosses};

/// Returns the delete of `item`.
fn delete(item: SheetItem) -> SheetEdit {
    SheetEdit::Delete { item }
}

/// Returns what `edit` takes away from the sheet `file_name`.
fn losses(file_name: &str, edit: &SheetEdit) -> SheetLosses {
    SheetLosses::of_edit(builder_of(file_name).document(), edit)
}

/// Returns `ids` as owned strings.
fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|&id| id.to_owned()).collect()
}

/// Deleting SIR's Health blanks every line that reads or changes it, removes the counts of its states and the colour
/// rule over it. A new kind for Health takes the same, and leaves out its own start, which it sets afresh.
#[test]
fn deleting_or_retyping_a_property_names_its_lines_measures_and_colours() {
    let deleted = losses("sir.toml", &delete(SheetItem::Property("health".to_owned())));
    assert_eq!(deleted.lines, ids(&["l1", "l2", "l3", "l4"]));
    assert_eq!(deleted.removed_measures, ids(&["susceptible", "infected", "recovered"]));
    assert_eq!(deleted.colors, [("cells".to_owned(), "health".to_owned())]);
    assert!(deleted.starts.is_empty() && deleted.deleted_lines.is_empty());
    let retyped = SheetEdit::SetPropertyType {
        property: "health".to_owned(),
        property_type: PropertyType::Number,
    };
    assert_eq!(losses("sir.toml", &retyped), deleted);

    // Velocity's averages keep their tables and lose their values.
    let velocity = losses("boids.toml", &delete(SheetItem::Property("velocity".to_owned())));
    assert_eq!(velocity.measures, ids(&["average_speed", "average_velocity"]));
    assert!(velocity.removed_measures.is_empty());
    // A heading deleted takes the colours of the walkers by heading with it.
    let heading = losses("random_walk.toml", &delete(SheetItem::Property("heading".to_owned())));
    assert_eq!(heading.colors, [("walker".to_owned(), "heading".to_owned())]);
}

/// A deleted state takes its lines, its count and its row of its property's start. A start on the ground that falls
/// back on another state counts as lost too, and so does a shape painted in the state.
#[test]
fn a_deleted_state_names_the_starts_and_shapes_that_held_it() {
    let state = |property: &str, state: &str| {
        delete(SheetItem::State {
            property: property.to_owned(),
            state: state.to_owned(),
        })
    };
    let infected = losses("sir.toml", &state("health", "infected"));
    assert_eq!(infected.lines, ids(&["l1", "l2", "l3", "l4"]));
    assert_eq!(infected.starts, ids(&["health"]));
    assert_eq!(infected.removed_measures, ids(&["infected"]));
    let nest = losses("ants.toml", &state("ground", "nest"));
    assert_eq!(nest.starts, ids(&["position"]), "the ants start on another state");
    assert_eq!(nest.shapes, [("ground".to_owned(), 1)]);
}

/// A parameter is lost by the starts and the shares kept that read it, as well as by lines, and a property by the
/// buttons that set it, the measures that add it up and the shades it draws.
#[test]
fn a_delete_names_the_starts_shares_buttons_measures_and_shades_that_read_the_item() {
    let parameter = |id: &str| delete(SheetItem::Parameter(id.to_owned()));
    let initial = losses("sir.toml", &parameter("initial_infected"));
    assert_eq!(initial.lines, ids(&["l4"]));
    assert_eq!(initial.starts, ids(&["health"]));
    let kept = losses("ants.toml", &parameter("trail_kept_per_tick"));
    assert_eq!(kept.kept, ids(&["to_food", "to_home"]));
    let to_food = losses("ants.toml", &delete(SheetItem::Property("to_food".to_owned())));
    assert_eq!(to_food.lines, ids(&["l1", "l4"]));
    assert_eq!(to_food.buttons, ids(&["reset_colony"]));
    assert_eq!(to_food.measures, ids(&["total_trail"]));
    assert_eq!(to_food.shades, [("ground".to_owned(), ids(&["to_food"]))]);
    let delivery = losses("ants.toml", &delete(SheetItem::Event("delivery".to_owned())));
    assert_eq!(delivery.lines, ids(&["l15"]));
    assert_eq!(delivery.removed_measures, ids(&["deliveries"]));
}

/// A new start of the ground drops its painted shapes, and the ants that start on a painted state lose their cells.
/// The ground's own start is what the edit sets, and is left out.
#[test]
fn a_new_start_names_the_shapes_it_drops_and_the_agents_that_start_on_them() {
    let all_open = SheetEdit::SetStartsAs {
        property: "ground".to_owned(),
        starts_as: Some(Box::new(StartsAs::new(StartRule::Value))),
    };
    let replaced = losses("ants.toml", &all_open);
    assert_eq!(replaced.shapes, [("ground".to_owned(), 4)]);
    assert_eq!(
        replaced.start_cells,
        [("position".to_owned(), "ground".to_owned(), "nest".to_owned())]
    );
    assert!(replaced.starts.is_empty());
    let by_chance = SheetEdit::SetStartsAs {
        property: "health".to_owned(),
        starts_as: Some(Box::new(StartsAs::new(StartRule::Value))),
    };
    assert!(losses("sir.toml", &by_chance).is_empty(), "no shape was painted");
}

/// New colours of the ground drop the shades of the rule they replace. The rule itself is what the edit sets.
#[test]
fn new_colours_name_the_shades_they_drop() {
    let none = SheetEdit::SetColorRule {
        thing: "ground".to_owned(),
        rule: None,
    };
    let dropped = losses("ants.toml", &none);
    assert_eq!(dropped.shades, [("ground".to_owned(), ids(&["to_food", "to_home"]))]);
    assert!(dropped.colors.is_empty());
    let mut by_cargo = ColorRule::new(ColorRuleKind::ByState);
    by_cargo.property = Some("cargo".to_owned());
    let ants = SheetEdit::SetColorRule {
        thing: "ant".to_owned(),
        rule: Some(by_cargo),
    };
    assert!(losses("ants.toml", &ants).is_empty(), "the ants' rule holds no shade");
}

/// A deleted line blanks the lines that read its result, a deleted button deletes the lines of its card, and a removed
/// summary blanks its readers but not its own look, whose summary takes its value with it.
#[test]
fn a_deleted_line_button_or_summary_names_the_lines_it_takes() {
    assert_eq!(
        losses("sir.toml", &SheetEdit::DeleteLine { line: "l1".to_owned() }).lines,
        ids(&["l2"])
    );
    let button = losses("sir.toml", &delete(SheetItem::Button("new_outbreak".to_owned())));
    assert_eq!(button.deleted_lines, ids(&["l4"]));
    let summary = SheetEdit::DeleteSummary {
        line: "l1".to_owned(),
        index: 1,
    };
    assert_eq!(losses("boids.toml", &summary).lines, ids(&["l5"]));
}

/// An edit that takes nothing away besides its item finds nothing: a rename, a new colour, a measure removed, a
/// parameter no line reads, a When whose lines move up a level, and a new property.
#[test]
fn an_edit_that_takes_nothing_else_away_finds_nothing() {
    let rename = SheetEdit::Rename {
        item: SheetItem::State {
            property: "health".to_owned(),
            state: "infected".to_owned(),
        },
        name: "Sick".to_owned(),
    };
    assert!(losses("sir.toml", &rename).is_empty());
    let recolor = SheetEdit::Recolor {
        item: SheetItem::State {
            property: "health".to_owned(),
            state: "infected".to_owned(),
        },
        color: [0, 0, 0, 255],
    };
    assert!(losses("sir.toml", &recolor).is_empty());
    assert!(losses("ants.toml", &delete(SheetItem::Measure("total_trail".to_owned()))).is_empty());
    assert!(losses("ants.toml", &SheetEdit::DeleteLine { line: "l9".to_owned() }).is_empty());
    let add = SheetEdit::AddProperty {
        thing: "cells".to_owned(),
        name: "Age".to_owned(),
        property_type: PropertyType::Number,
    };
    assert!(losses("sir.toml", &add).is_empty());
    let mut sir = builder_of("sir.toml");
    let mut unused = SheetLine::new(LineKind::Calculate);
    unused.formula = Some("2 * 3".to_owned());
    run(
        &mut sir,
        SheetEdit::AddLine {
            card: "cell_tick".to_owned(),
            inside: None,
            line: Box::new(unused),
        },
    );
    let lone = sir.document().cards[0].lines.last().cloned().expect("the new line");
    let lone = SheetEdit::DeleteLine { line: lone };
    assert!(
        SheetLosses::of_edit(sir.document(), &lone).is_empty(),
        "no line reads it"
    );
}

/// Several commands compared as one edit, as Done of the property form makes them: a rename of the property finds
/// nothing, and a removed state its lines, its count and its row of the start.
#[test]
fn several_commands_compare_as_one_edit() {
    let before = builder_of("sir.toml");
    let mut after = before.clone();
    run(
        &mut after,
        SheetEdit::Rename {
            item: SheetItem::Property("health".to_owned()),
            name: "Condition".to_owned(),
        },
    );
    assert!(SheetLosses::between(before.document(), after.document()).is_empty());
    run(
        &mut after,
        delete(SheetItem::State {
            property: "health".to_owned(),
            state: "recovered".to_owned(),
        }),
    );
    let lost = SheetLosses::between(before.document(), after.document());
    assert_eq!(lost.lines, ids(&["l3"]));
    assert_eq!(lost.removed_measures, ids(&["recovered"]));
}
