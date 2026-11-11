//! The code and schema identity of grid sheets: each edit of the author format's identity table keeps or changes the
//! model code and the schema hash as the table says.

use henad_core::explore::fingerprint::schema_hash;

use super::sheet_support::{code, edited, entry_of, life, life_with_line, lowered, sheet_text, vote};
use crate::sheet::{KIND_SHEET, SHEET_CODE_FORMAT, SHEET_VERSION};

/// Returns the schema hash of `text`, a sheet of cells or of agents that must lower and compile.
fn schema(text: &str) -> u64 {
    schema_hash(&entry_of(&lowered(text)).schema())
}

/// Returns whether the code and the schema hash of `text` differ from those of `base`, in that order.
fn changes(base: &str, text: &str) -> (bool, bool) {
    (code(text) != code(base), schema(text) != schema(base))
}

/// Text of the SIR starter, which has rates, three parameters and three measures.
fn sir() -> &'static str {
    sheet_text("sir.toml")
}

/// Text of the boids starter, which has three properties and two Steers of one property.
fn boids() -> &'static str {
    sheet_text("boids.toml")
}

/// Returns `text` with the parts `first` and `second` swapped, where `first` comes before `second` and each occurs
/// once.
fn swapped(text: &str, first: &str, second: &str) -> String {
    assert_eq!(text.matches(first).count(), 1, "{first:?} is not unique");
    assert_eq!(text.matches(second).count(), 1, "{second:?} is not unique");
    text.replacen(second, "\u{0}", 1)
        .replacen(first, second, 1)
        .replacen('\u{0}', first, 1)
}

#[test]
fn renaming_a_result_keeps_the_code() {
    let renamed = edited(life(), "name = \"Alive neighbours\"", "name = \"Live neighbours\"");
    assert_eq!(changes(life(), &renamed), (false, false));
}

#[test]
fn reordering_two_independent_lines_keeps_the_code() {
    let reordered = edited(
        life(),
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l3\", \"l2\"]",
    );
    assert_eq!(
        lowered(&reordered).lowered().to_toml(),
        lowered(life()).lowered().to_toml()
    );
}

#[test]
fn editing_a_note_purpose_or_comment_keeps_the_code() {
    let noted = edited(
        life(),
        "start_state = \"dead\"\n",
        "start_state = \"dead\"\nnote = \"Birth\"\n",
    );
    assert_eq!(code(&noted), code(life()));
    let purpose = edited(life(), "Conway's Game of Life.", "John Conway's Game of Life.");
    assert_eq!(code(&purpose), code(life()));
    let comment = edited(
        life(),
        "# Each tick, every cell\n",
        "# Each tick, every cell, by hand\n",
    );
    assert_eq!(code(&comment), code(life()));
}

#[test]
fn a_lowered_sheet_records_its_canonical_text_outside_the_code() {
    let noted = edited(
        life(),
        "start_state = \"dead\"\n",
        "start_state = \"dead\"\nnote = \"Birth\"\n",
    );
    let (plain, noted) = (lowered(life()), lowered(&noted));
    for file in [&plain, &noted] {
        let author = file.lowered().author().expect("the lowered graph records the sheet");
        assert_eq!((author.format(), author.text()), (SHEET_CODE_FORMAT, file.to_toml()));
        let source = file.lowered().source();
        assert_eq!(source.author(), Some(author));
        assert_eq!(source.code(), Some(&file.code()));
    }
    assert_eq!(
        plain.lowered().source(),
        noted.lowered().source(),
        "a note leaves the source equal"
    );
    assert!(!plain.lowered().source().same_author(&noted.lowered().source()));
    assert!(noted.to_toml().contains("note = \"Birth\""));
    // The graph's own text and a graph read from the code carry no author text.
    assert!(!plain.lowered().to_toml().contains("henad-sheet"));
    let replayed = crate::GraphFile::from_code(&plain.code()).expect("the recorded code reads");
    assert_eq!(replayed.author(), None);
    assert_eq!(replayed.source(), plain.lowered().source());
    assert_eq!(SHEET_CODE_FORMAT, format!("henad-{KIND_SHEET}/{SHEET_VERSION}"));
}

#[test]
fn renaming_a_parameter_changes_the_code() {
    let renamed = edited(life(), "name = \"Initial density\"", "name = \"Starting density\"");
    assert_eq!(changes(life(), &renamed), (true, false));
}

#[test]
fn renaming_a_state_changes_the_code() {
    let renamed = edited(life(), "name = \"Alive\", color", "name = \"Living\", color");
    assert_eq!(changes(life(), &renamed), (true, false));
}

#[test]
fn renaming_the_model_changes_the_code() {
    let renamed = edited(life(), "name = \"Game of Life\"", "name = \"Life\"");
    assert_eq!(changes(life(), &renamed), (true, false));
}

#[test]
fn changing_a_default_changes_the_code() {
    let changed = edited(life(), "default = 0.3", "default = 0.4");
    assert_eq!(changes(life(), &changed), (true, true));
}

#[test]
fn changing_a_colour_changes_the_code() {
    let changed = edited(life(), "color = \"#151515\"", "color = \"#101010\"");
    assert_eq!(changes(life(), &changed), (true, false));
}

#[test]
fn drawing_new_numbers_changes_the_code() {
    let changed = edited(life(), "key = 2929187291", "key = 2929187292");
    assert_eq!(changes(life(), &changed), (true, false));
}

#[test]
fn reordering_two_changes_that_can_both_apply_changes_the_code() {
    let reordered = edited(
        vote(),
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l3\", \"l2\"]",
    );
    assert_ne!(code(&reordered), code(vote()));
}

#[test]
fn renaming_a_measure_changes_the_code_and_the_schema() {
    let renamed = edited(
        sir(),
        "id = \"infected\"\nname = \"Infected\"",
        "id = \"infected\"\nname = \"Ill\"",
    );
    assert_eq!(changes(sir(), &renamed), (true, true));
}

#[test]
fn changing_a_parameter_id_changes_the_code_and_the_schema() {
    let changed = sir().replace("recovery_rate", "recovery_speed");
    assert_eq!(changes(sir(), &changed), (true, true));
}

/// A measure's id names no node, and its stat takes the measure's name.
#[test]
fn changing_a_measure_id_keeps_the_code_and_the_schema() {
    let changed = edited(
        sir(),
        "id = \"infected\"\nname = \"Infected\"",
        "id = \"ill\"\nname = \"Infected\"",
    );
    assert_eq!(changes(sir(), &changed), (false, false));
}

/// Returns the `[[parameter]]` table of SIR's rate `id` named `name`, with its default, unit and meaning.
fn sir_rate(id: &str, name: &str, default: &str, means: &str) -> String {
    format!(
        "[[parameter]]\nid = \"{id}\"\nname = \"{name}\"\ntype = \"number\"\nmin = 0\nmax = 5\ndefault = {default}\n\
         step = 0.01\nunit = \"per day\"\nmeans = \"{means}\"\n"
    )
}

/// Returns the `[[measure]]` table of SIR's count of the state `id` named `name` and coloured `color`.
fn sir_count(id: &str, name: &str, color: &str) -> String {
    format!(
        "[[measure]]\nid = \"{id}\"\nname = \"{name}\"\ncolor = \"{color}\"\nrule = \"count_in_state\"\n\
         over = \"cells\"\nproperty = \"health\"\nstate = \"{id}\"\n"
    )
}

#[test]
fn reordering_parameters_changes_the_code_and_the_schema() {
    let reordered = swapped(
        sir(),
        &sir_rate(
            "infection_rate",
            "Infection rate",
            "0.3",
            "Rate at which one infected neighbour infects a susceptible cell",
        ),
        &sir_rate(
            "recovery_rate",
            "Recovery rate",
            "0.1",
            "Rate at which an infected cell recovers",
        ),
    );
    assert_eq!(changes(sir(), &reordered), (true, true));
}

#[test]
fn reordering_measures_changes_the_code_and_the_schema() {
    let reordered = swapped(
        sir(),
        &sir_count("infected", "Infected", "#E43748"),
        &sir_count("recovered", "Recovered", "#808080"),
    );
    assert_eq!(changes(sir(), &reordered), (true, true));
}

#[test]
fn reordering_properties_changes_the_code_and_keeps_the_schema() {
    let heading = "[[agents.property]]\nid = \"heading\"\nname = \"Heading\"\ntype = \"heading\"\n\n";
    let reordered = edited(boids(), heading, "");
    let reordered = edited(
        &reordered,
        "[[parameter]]\nid = \"visual_range\"",
        &format!("{heading}[[parameter]]\nid = \"visual_range\""),
    );
    assert_eq!(changes(boids(), &reordered), (true, false));
}

#[test]
fn changing_a_minimum_or_maximum_changes_the_code_and_the_schema() {
    let minimum = edited(
        sir(),
        "min = 0\nmax = 1\ndefault = 0.01",
        "min = 0.001\nmax = 1\ndefault = 0.01",
    );
    assert_eq!(changes(sir(), &minimum), (true, true));
    let maximum = edited(
        sir(),
        "min = 0\nmax = 5\ndefault = 0.1",
        "min = 0\nmax = 4\ndefault = 0.1",
    );
    assert_eq!(changes(sir(), &maximum), (true, true));
}

#[test]
fn changing_a_step_changes_the_code_and_keeps_the_schema() {
    let changed = edited(sir(), "step = 0.001", "step = 0.002");
    assert_eq!(changes(sir(), &changed), (true, false));
}

#[test]
fn changing_a_unit_or_means_keeps_the_code_and_the_schema() {
    let unit = edited(
        sir(),
        "unit = \"per day\"\nmeans = \"Rate at which an infected cell recovers\"",
        "unit = \"per sick day\"\nmeans = \"Rate at which an infected cell recovers\"",
    );
    assert_eq!(changes(sir(), &unit), (false, false));
    let means = edited(
        sir(),
        "means = \"Rate at which an infected cell recovers\"",
        "means = \"Rate at which an infected cell gets better\"",
    );
    assert_eq!(changes(sir(), &means), (false, false));
}

/// The mark of a rate whose unit followed the tick's stays outside the code.
#[test]
fn marking_a_rate_unit_as_the_ticks_keeps_the_code_and_the_schema() {
    let marked = edited(
        sir(),
        "rate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\n",
        "rate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\nrate_unit_from_tick = true\n",
    );
    assert!(lowered(&marked).to_toml().contains("rate_unit_from_tick = true"));
    assert_eq!(changes(sir(), &marked), (false, false));
}

/// A new tick length is a new default, bounds and step of the hidden `tick_length` parameter, so the schema changes
/// with the code. A new tick unit converts the rates and keeps the schema.
#[test]
fn changing_the_time_scale_changes_the_code() {
    let length = edited(sir(), "tick_length = 1\n", "tick_length = 0.5\n");
    assert_eq!(changes(sir(), &length), (true, true));
    let unit = edited(sir(), "tick_unit = \"day\"", "tick_unit = \"week\"");
    assert_eq!(changes(sir(), &unit), (true, false));
}

/// Steers of one property add up in line id order, so a reorder of them leaves the lowered graph alone.
#[test]
fn reordering_steers_of_one_property_keeps_the_lowered_graph() {
    let reordered = edited(
        boids(),
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]",
        "lines = [\"l1\", \"l2\", \"l5\", \"l3\", \"l4\", \"l6\", \"l7\"]",
    );
    assert_ne!(lowered(&reordered).to_toml(), lowered(boids()).to_toml());
    assert_eq!(
        lowered(&reordered).lowered().to_toml(),
        lowered(boids()).lowered().to_toml()
    );
    assert_eq!(changes(boids(), &reordered), (false, false));
}

#[test]
fn drawing_new_numbers_on_a_chance_changes_the_code() {
    let fire = sheet_text("forest_fire.toml");
    let changed = edited(fire, "key = 3688405984", "key = 3688405985");
    assert_ne!(code(&changed), code(fire));
}

#[test]
fn renaming_a_chance_result_keeps_the_code() {
    let table = "[card.line.l5]\nkind = \"chance\"\nname = \"Spark\"\nkey = 11\nchance = 0.5";
    let renamed = table.replace("Spark", "Flash");
    assert_eq!(code(&life_with_line(table)), code(&life_with_line(&renamed)));
}
