//! The wording table against fixed copies of its words, and the sentences of the starters and test sheets.
//!
//! The literals of [`every_wording_table_entry_equals_the_final_words`] hold every word the sheet editor shows, word
//! for word. A template is called with the example the table gives, such as line 2 or "Alive neighbours".

use henad_core::metadata::TimeUnit;

use crate::sheet::ParsedSheet;
use crate::sheet::document::{
    Around, CompareTest, LineKind, LookSummary, PropertyType, SheetChip, SheetDocument, SheetLine, SheetProperty,
    SheetValue, Slot, SummaryKind,
};
use crate::sheet::sentence::{self, AddButton, AddItem, Blank, ConditionItem, ConditionKind, DrawKind, LineSelect};
use crate::sheet::sentence::{COMPARISONS, Sentences, SheetSection, StartsField};

use super::sheet_document::sheets;

/// Reads the sheet `file_name` of [`sheets`].
fn document(file_name: &str) -> SheetDocument {
    let text = sheets()
        .into_iter()
        .find(|(name, _)| *name == file_name)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no sheet {file_name}"));
    ParsedSheet::parse(text)
        .unwrap_or_else(|error| panic!("{file_name} does not read: {error}"))
        .document()
        .clone()
}

/// Returns the sentence of every line of card `card` of `document` in display order, the lines inside a When or
/// Repeat after it, each with the "otherwise" lines `otherwise` gives it by line number.
fn card_sentences(document: &SheetDocument, card: usize, otherwise: &[(usize, &[usize])]) -> Vec<String> {
    let card = &document.cards[card];
    let mut order = Vec::new();
    let mut stack: Vec<&str> = card.lines.iter().rev().map(String::as_str).collect();
    while let Some(id) = stack.pop() {
        order.push(id);
        if let Some(line) = card.line_tables.get(id) {
            stack.extend(line.lines.iter().rev().map(String::as_str));
        }
    }
    let sentences = Sentences::new(document);
    order
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let higher = otherwise
                .iter()
                .find(|(number, _)| *number == index + 1)
                .map_or(&[][..], |(_, lines)| lines);
            sentences.line(id, higher)
        })
        .collect()
}

/// Checks each pair of an entry of the final words and the wording table's text, and reports every mismatch at once.
fn assert_entries(entries: &[(&str, String)]) {
    let mismatches: Vec<String> = entries
        .iter()
        .filter(|(expected, actual)| expected != actual)
        .map(|(expected, actual)| format!("expected {expected:?}\n     got {actual:?}"))
        .collect();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// Returns `text` as an owned string, for a fixed entry beside a template's output.
fn fixed(text: &str) -> String {
    text.to_owned()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one entry per row of the wording table reads best as one table"
)]
fn every_wording_table_entry_equals_the_final_words() {
    let mut entries: Vec<(&str, String)> = vec![
        // The card's frame.
        ("Each tick, every cell", sentence::card_header("cell")),
        ("1 line", sentence::line_count(1)),
        ("3 lines", sentence::line_count(3)),
        (
            "Everyone reads the world as it was at the start of the tick. All changes land together at the end of \
             it. Whatever no line changes stays as it was.",
            fixed(sentence::LAW),
        ),
        ("Help: lines", fixed(sentence::HELP_LINES_NAME)),
        ("+ Repeat...", fixed(sentence::REPEAT_BUTTON)),
        (
            "These add a line at the end of the card.",
            fixed(sentence::ADD_AT_END_NOTE),
        ),
        ("+ Add inside line 4", sentence::add_inside_label(4)),
        ("Add a line inside line 4", sentence::add_inside_name(4)),
        ("line 2", sentence::line_word(None, 2)),
        ("Line 2", sentence::line_prefix(None, 2)),
        ("Clear line 1", sentence::line_word(Some("Clear"), 1)),
        ("Clear line 1", sentence::line_prefix(Some("Clear"), 1)),
        (
            "Line 2: Become Alive from Dead, when Alive neighbours is 3",
            sentence::line_name("Line 2", "Become Alive from Dead, when Alive neighbours is 3"),
        ),
        ("Line 2 actions", sentence::line_actions_name("Line 2")),
        ("Rename line 2", sentence::rename_line("line 2")),
        ("Move line 2 up", sentence::move_line_up("line 2")),
        ("Move line 2 down", sentence::move_line_down("line 2")),
        ("Delete line 2", sentence::delete_line("line 2")),
        (
            "Delete Clear line 1",
            sentence::delete_line(&sentence::line_word(Some("Clear"), 1)),
        ),
        ("Delete. Lines inside stay.", fixed(sentence::DELETE_WHEN_TOOLTIP)),
        ("Draw new numbers", fixed(sentence::DRAW_NEW_NUMBERS)),
        (
            "Another result is already called Alive neighbours.",
            sentence::result_name_taken("Alive neighbours"),
        ),
        ("Line 2: new name", sentence::rename_field_name("Line 2")),
        (
            "Lines of Clear: run once, every cell, when pressed",
            sentence::button_card_heading("Clear", "cell"),
        ),
        ("No lines yet. Add a change below.", fixed(sentence::BUTTON_CARD_EMPTY)),
        ("Lines", sentence::line_list_name(None)),
        ("Lines of Clear", sentence::line_list_name(Some("Clear"))),
        (
            "Nothing changes yet. Add a line below.",
            fixed(sentence::BLANK_MODEL_CARD),
        ),
        // Units on a rate line.
        ("second", fixed(sentence::rate_unit_word(TimeUnit::Second))),
        ("minute", fixed(sentence::rate_unit_word(TimeUnit::Minute))),
        ("hour", fixed(sentence::rate_unit_word(TimeUnit::Hour))),
        ("day", fixed(sentence::rate_unit_word(TimeUnit::Day))),
        ("week", fixed(sentence::rate_unit_word(TimeUnit::Week))),
        ("month", fixed(sentence::rate_unit_word(TimeUnit::Month))),
        ("year", fixed(sentence::rate_unit_word(TimeUnit::Year))),
        // Comparisons.
        (
            "is less than 2 or more than 3",
            sentence::outside_words("", "2", "3").trim_start().to_owned(),
        ),
        // Conditions and "Choose when".
        ("always", fixed(sentence::ALWAYS)),
        ("Line 2: choose when, now always", sentence::always_name("Line 2")),
        ("No condition. Choose to add one.", fixed(sentence::ALWAYS_TOOLTIP)),
        ("when", fixed(sentence::condition_head(ConditionKind::When, false))),
        (
            "with chance",
            fixed(sentence::condition_head(ConditionKind::Chance, false)),
        ),
        ("at rate", fixed(sentence::condition_head(ConditionKind::Rate, false))),
        (
            "Line 2: change the kind of condition",
            sentence::condition_arrow_name("Line 2"),
        ),
        ("Change the kind of condition", fixed(sentence::CONDITION_ARROW_TOOLTIP)),
        (
            "When ...",
            format!("{} ...", sentence::condition_head(ConditionKind::When, true)),
        ),
        (
            "When with chance ...",
            format!("{} ...", sentence::condition_head(ConditionKind::Chance, true)),
        ),
        (
            "When at rate ...",
            format!("{} ...", sentence::condition_head(ConditionKind::Rate, true)),
        ),
        ("Choose when", fixed(sentence::CHOOSE_WHEN)),
        (
            "Line 4 already gives a condition. Choose always to use only that one.",
            sentence::when_lead(4),
        ),
        ("From lines above", fixed(sentence::FROM_LINES_ABOVE)),
        ("when Crowded", sentence::result_condition_item("Crowded", false)),
        (
            "line 1: count of the 8 cells around me whose Status is Alive",
            sentence::result_description(1, "count of the 8 cells around me whose Status is Alive"),
        ),
        ("Conditions", fixed(sentence::CONDITIONS)),
        ("Or a new condition", fixed(sentence::OR_A_NEW_CONDITION)),
        ("otherwise", fixed(sentence::OTHERWISE)),
        ("when line 2 does not apply", sentence::otherwise_description(&[2])),
        (
            "otherwise, when line 2 does not apply",
            sentence::otherwise_item_name(&[2]),
        ),
        ("when ... is ...", ConditionItem::Is.label(false)),
        ("when ... is at least ...", ConditionItem::AtLeast.label(false)),
        ("when ... is at most ...", ConditionItem::AtMost.label(false)),
        (
            "when ... is less than ... or more than ...",
            ConditionItem::Outside.label(false),
        ),
        ("with chance ...", ConditionItem::WithChance.label(false)),
        ("at rate ... per ...", ConditionItem::AtRate.label(false)),
        ("When with chance ...", ConditionItem::WithChance.label(true)),
        ("When at rate ... per ...", ConditionItem::AtRate.label(true)),
        ("always", ConditionItem::Always.label(false)),
        (
            "The same as a value. The line offers is not, is more than and others.",
            fixed(ConditionItem::Is.description().unwrap_or_default()),
        ),
        (
            "The value or more, such as 1 or more",
            fixed(ConditionItem::AtLeast.description().unwrap_or_default()),
        ),
        (
            "The value or less, such as 4 or less",
            fixed(ConditionItem::AtMost.description().unwrap_or_default()),
        ),
        (
            "Outside a range, such as less than 2 or more than 3",
            fixed(ConditionItem::Outside.description().unwrap_or_default()),
        ),
        // Blanks and their accessible names.
        ("Line 2: unit of time", LineSelect::UnitOfTime.name("Line 2")),
        ("Line 2: comparison", LineSelect::Comparison.name("Line 2")),
        ("Line 2: which cells", LineSelect::WhichCells.name("Line 2")),
        ("Line 2: total or average", LineSelect::TotalOrAverage.name("Line 2")),
        ("Line 2: operation", LineSelect::Operation.name("Line 2")),
        ("unit", fixed(sentence::UNIT_PLACEHOLDER)),
        ("for each ...", fixed(sentence::FOR_EACH_BLANK_WORDS)),
        ("Line 2: for each, optional", sentence::for_each_optional_name("Line 2")),
        (
            "Optional: make the rate grow with a count",
            sentence::for_each_optional_tooltip(DrawKind::Rate),
        ),
        (
            "Optional: make the chance grow with a count",
            sentence::for_each_optional_tooltip(DrawKind::Chance),
        ),
        (
            "Line 2: change or remove for each",
            sentence::for_each_change_name("Line 2"),
        ),
        ("Line 3: add another value", sentence::add_value_name("Line 3")),
        (
            "Line 2: add for each infected neighbour",
            sentence::add_for_each_name("Line 2", &sentence::for_each_words("Infected neighbours")),
        ),
        (
            "Line 2: use a rate for each infected neighbour",
            sentence::use_for_each_name("Line 2", DrawKind::Rate, "infected neighbour"),
        ),
        (
            "Line 5: use is more than 0 instead",
            sentence::use_instead_name("Line 5", "is more than 0"),
        ),
        ("from", fixed(sentence::FROM_WORD)),
        ("Only cells in this state now change", sentence::from_tooltip("cell")),
        ("Status Alive", sentence::shared_state_name("Status", "Alive", true)),
        ("Alive", sentence::shared_state_name("Status", "Alive", false)),
        // Pickers.
        ("Choose a state", Blank::State.picker_title()),
        ("Choose what to count", Blank::Count.picker_title()),
        ("Choose the old state", Blank::OldState.picker_title()),
        ("Choose when", Blank::When.picker_title()),
        ("Type a number, then press Enter.", fixed(sentence::NUMBER_BOX_HINT)),
        (
            "Type a chance such as 0.3 or 30%, then press Enter.",
            fixed(sentence::CHANCE_BOX_HINT),
        ),
        ("Use", fixed(sentence::USE_BUTTON)),
        ("This cell's properties", fixed(sentence::THIS_CELLS_PROPERTIES)),
        ("my Food", sentence::my_property_item("Food")),
        ("this cell's Food", sentence::this_cells_property("Food")),
        ("Parameters", fixed(sentence::PARAMETERS)),
        ("New parameter...", fixed(sentence::NEW_PARAMETER)),
        ("States", fixed(sentence::STATES)),
        ("any state", fixed(sentence::ANY_STATE)),
        ("Properties", fixed(sentence::PROPERTIES)),
        ("Numbers, separated by commas", fixed(sentence::LIST_GROUP)),
        (
            "Type one or more numbers, such as 2, 3, then press Enter.",
            fixed(sentence::LIST_HINT),
        ),
        ("Remove from list", fixed(sentence::REMOVE_FROM_LIST)),
        (
            "The rate grows with what you choose. With for each infected neighbour, a cell with 2 infected \
             neighbours gets twice the rate.",
            fixed(sentence::for_each_lead(DrawKind::Rate)),
        ),
        (
            "Each one counted is a separate try with this chance.",
            fixed(sentence::for_each_lead(DrawKind::Chance)),
        ),
        ("Remove for each", fixed(sentence::REMOVE_FOR_EACH)),
        (
            "No number result above this line yet.",
            fixed(sentence::NO_NUMBER_RESULT),
        ),
        ("No property yet.", fixed(sentence::NO_PROPERTY)),
        ("No Number property yet.", fixed(sentence::NO_NUMBER_PROPERTY)),
        (
            "No States property yet. Add one under Cells on the sheet.",
            fixed(sentence::NO_STATES_PROPERTY),
        ),
        (
            "No Number property yet. Add one under Cells on the sheet.",
            fixed(sentence::NO_NUMBER_PROPERTY_ON_SHEET),
        ),
        (
            "No yes or no result above this line yet. Make one with + Yes or no above this line.",
            fixed(sentence::NO_YES_OR_NO_RESULT),
        ),
        (
            "No states yet. Add a States property under Cells on the sheet.",
            fixed(sentence::NO_STATES),
        ),
        (
            "A chance is from 0 to 1, or from 0% to 100%.",
            fixed(sentence::CHANCE_OUT_OF_RANGE),
        ),
        ("A rate cannot be below 0.", fixed(sentence::RATE_BELOW_ZERO)),
        ("Type one number, such as 3 or 0.25.", fixed(sentence::TYPE_ONE_NUMBER)),
        (
            "For a list such as 2, 3, choose is one of the numbers or is none of the numbers first.",
            fixed(sentence::LIST_IN_ONE_NUMBER),
        ),
        ("Choose a colour", fixed(sentence::CHOOSE_A_COLOR)),
        (
            "Black, Dark grey, Grey, White, Red, Orange, Yellow, Brown, Green, Dark green, Blue, Purple",
            sentence::COLOR_NAMES.join(", "),
        ),
        // Add menus.
        (
            "Or a yes or no that several lines share",
            fixed(sentence::SHARED_DRAW_GROUP),
        ),
        (
            "Become Alive from Dead, when ...",
            AddItem::Become
                .pattern(Some(("Alive", "Dead")))
                .trim_start_matches("Change a state: ")
                .to_owned(),
        ),
        (
            "Change a state: Become a new state from an old state, when ...",
            AddItem::Become.pattern(None),
        ),
        // Result names.
        ("Alive neighbours", sentence::count_result_name("Alive", false)),
        ("B in my block", sentence::count_result_name("B", true)),
        ("Count", fixed(sentence::COUNT_RESULT_EMPTY)),
        ("Total Food around", sentence::total_result_name("Food", false)),
        ("Average Food around", sentence::total_result_name("Food", true)),
        (
            "Recovery happens",
            sentence::happens_result_name(sentence::happens_base("Recovery rate")),
        ),
        (
            "Lightning happens",
            sentence::happens_result_name(sentence::happens_base("Lightning chance")),
        ),
        (
            "Lightning f happens",
            sentence::happens_result_name(sentence::happens_base("Lightning f")),
        ),
        ("0.1 per day", sentence::typed_rate_base("0.1", TimeUnit::Day)),
        ("0.3 chance", sentence::typed_chance_base("0.3")),
        (
            "30% chance happens",
            sentence::happens_result_name(&sentence::typed_chance_base("30%")),
        ),
        ("Happens", fixed(sentence::HAPPENS)),
        ("Test", fixed(sentence::TEST_RESULT_EMPTY)),
        ("Result", fixed(sentence::CALCULATE_RESULT_EMPTY)),
        ("All of", fixed(sentence::ALL_OF_RESULT)),
        ("Any of", fixed(sentence::ANY_OF_RESULT)),
        (
            "Alive neighbours 2",
            sentence::repeated_result_name("Alive neighbours", 2),
        ),
        (
            "Alive neighbours 3",
            sentence::repeated_result_name("Alive neighbours", 3),
        ),
        // Notes under a line.
        (
            "Not run yet: choose a state",
            sentence::not_run_choose(Blank::State.want()),
        ),
        (
            "Not run yet: choose the old state",
            sentence::not_run_choose(Blank::OldState.want()),
        ),
        (
            "Not run yet: choose the upper number",
            sentence::not_run_choose(Blank::UpperNumber.want()),
        ),
        (
            "Not run yet: choose unit of time",
            sentence::not_run_choose(LineSelect::UnitOfTime.words()),
        ),
        (
            "Uses Alive neighbours, which is below this line",
            sentence::uses_result_below("Alive neighbours"),
        ),
        (
            "Uses Infection happens from inside line 2. Only lines inside line 2 can use it. Choose another value.",
            sentence::uses_result_inside("Infection happens", 2),
        ),
        (
            "Not run yet: Alive neighbours on line 1 is not finished",
            sentence::not_run_result_unfinished("Alive neighbours", 1),
        ),
        (
            "Not run yet: set how long one tick is, under World on the sheet",
            fixed(sentence::NOT_RUN_TICK),
        ),
        (
            "Not run yet: line 4 is not finished",
            sentence::not_run_when_unfinished(4),
        ),
        (
            "Alive neighbours is below this line. Move this line under it, or choose another value.",
            sentence::result_below_tooltip("Alive neighbours"),
        ),
        (
            "Infection happens belongs to the lines inside a When this line is not in. Choose another value.",
            sentence::result_inside_tooltip("Infection happens"),
        ),
        (
            "For one side only, a plain comparison reads better.",
            fixed(sentence::ONE_SIDED_RANGE),
        ),
        (
            "Use is more than 0 instead",
            sentence::use_instead_label("is more than 0"),
        ),
        (
            "Use is less than 2 instead",
            sentence::use_instead_label("is less than 2"),
        ),
        (
            "Infection rate is per day on the sheet, but this line reads it per week.",
            sentence::unit_mismatch("Infection rate", TimeUnit::Day, TimeUnit::Week),
        ),
        (
            "Infected neighbours is a count, not a rate. Did you mean a rate for each infected neighbour?",
            sentence::count_used_as_draw("Infected neighbours", DrawKind::Rate, "infected neighbour"),
        ),
        (
            "Infected neighbours is a count, not a chance. Did you mean a chance for each infected neighbour?",
            sentence::count_used_as_draw("Infected neighbours", DrawKind::Chance, "infected neighbour"),
        ),
        (
            "Use a rate for each infected neighbour",
            sentence::use_for_each_label(DrawKind::Rate, "infected neighbour"),
        ),
        (
            "Should this rate grow with the number of Infected neighbours?",
            sentence::rate_grow_prompt("Infected neighbours"),
        ),
        (
            "Add for each infected neighbour",
            sentence::add_for_each_label("infected neighbour"),
        ),
        (
            "None of lines 2 and 3 applies when B in my block is 4 or 5. Those cells keep their Opinion.",
            sentence::gap_note(&[2, 3], "B in my block", &[4, 5], "Opinion", "cell"),
        ),
        ("4 to 8", sentence::count_values_words(&[4, 5, 6, 7, 8])),
        (
            "Lines 2 and 3 cover every value of B in my block. A cell ends the tick A when it is 0 to 4 and B when \
             it is 5 to 9.",
            sentence::coverage_note(
                &[2, 3],
                "B in my block",
                &[("A", &[0, 1, 2, 3, 4]), ("B", &[5, 6, 7, 8, 9])],
                "cell",
            ),
        ),
        (
            "Lines 2 and 3 change different cells (Dead and Alive), so their order does not matter.",
            sentence::order_note(&[2, 3], &["Dead", "Alive"], "cell"),
        ),
        (
            "Lines 2, 3 and 4 change different cells (Susceptible, Infected and Recovered), so their order does not \
             matter.",
            sentence::order_note(&[2, 3, 4], &["Susceptible", "Infected", "Recovered"], "cell"),
        ),
        (
            "Line 3 already says from Alive, so this When repeats it.",
            sentence::when_repeats_note(&[3], "Alive"),
        ),
        (
            "Lines 3 and 4 already say from Alive, so this When repeats it.",
            sentence::when_repeats_note(&[3, 4], "Alive"),
        ),
        ("Remove this When", fixed(sentence::REMOVE_THIS_WHEN)),
        // The two names below differ from the table. Each holds its button's words, as every accessible name does.
        (
            "Line 2: remove this When and keep the lines inside",
            sentence::remove_when_name("Line 2"),
        ),
        (
            "Only line 5 is inside. Its own when can hold this condition.",
            sentence::only_one_inside_note(5),
        ),
        ("Move it into line 5", sentence::move_into_label(5)),
        ("Line 4: move it into line 5", sentence::move_into_name("Line 4", 5)),
        (
            "Only when line 4 holds: my Health is Recovered.",
            sentence::only_when_note(4, Some("my Health is Recovered")),
        ),
        ("Only when line 4 holds.", sentence::only_when_note(4, None)),
        (
            "Worked out for every cell, whatever line 4 says. Only lines inside line 4 can use it.",
            sentence::worked_out_everywhere_note(4, "cell"),
        ),
        ("Nothing inside yet.", fixed(sentence::NOTHING_INSIDE)),
        // "otherwise".
        (
            "otherwise (when line 2 does not apply)",
            sentence::otherwise_words(&[2]),
        ),
        (
            "otherwise (when lines 2 and 5 do not apply)",
            sentence::otherwise_words(&[2, 5]),
        ),
        (
            "Line 2 also changes Cover for a cell that is Tree. When both apply, line 2 wins.",
            sentence::otherwise_tooltip(&[2], "Cover", Some("Tree"), "cell", false),
        ),
        (
            "Lines 2 and 5 also change Cover for a cell that is Tree. When more than one applies, the highest line \
             wins.",
            sentence::otherwise_tooltip(&[2, 5], "Cover", Some("Tree"), "cell", false),
        ),
        (
            "Line 2 also changes Status for an agent that is S. When both apply at the same time, line 2 wins.",
            sentence::otherwise_tooltip(&[2], "Status", Some("S"), "agent", true),
        ),
        (
            "Lines 2 and 5 also change Status for an agent that is S. When more than one applies at the same time, \
             the highest line wins.",
            sentence::otherwise_tooltip(&[2, 5], "Status", Some("S"), "agent", true),
        ),
        (
            "Line 6 now starts with otherwise. Lines 2 and 5 also change Cover for a cell that is Tree. When more \
             than one applies, the highest line wins.",
            sentence::otherwise_notice("Line 6", &[2, 5], "Cover", Some("Tree"), "cell", false),
        ),
        // Rates and chances.
        (
            "a Susceptible cell becomes Infected",
            sentence::become_subject("cell", Some("Susceptible"), Some("Infected")),
        ),
        (
            "an Infected cell becomes Recovered",
            sentence::become_subject("cell", Some("Infected"), Some("Recovered")),
        ),
        (
            "a cell becomes Infected",
            sentence::become_subject("cell", None, Some("Infected")),
        ),
        (
            "a cell changes",
            sentence::become_subject("cell", Some("Susceptible"), None),
        ),
        (
            "an ant changes",
            sentence::become_subject("ant", Some("Searching"), None),
        ),
        (
            "a Searching ant becomes Carrying",
            sentence::become_subject("ant", Some("Searching"), Some("Carrying")),
        ),
        ("a cell takes the new value", sentence::set_subject("cell")),
        (
            "the lines inside line 4 apply to a cell",
            sentence::when_subject(4, "cell"),
        ),
        ("Only birds in this state now change", sentence::from_tooltip("bird")),
        (
            "None of lines 2 and 3 applies when Near is 4. Those birds keep their Mood.",
            sentence::gap_note(&[2, 3], "Near", &[4], "Mood", "bird"),
        ),
        (
            "the result Recovery happens is yes for a cell",
            sentence::result_subject("Recovery happens", "cell"),
        ),
        ("Each tick (1 day)", sentence::each_tick(Some("1 day"))),
        ("Each tick", sentence::each_tick(None)),
        ("infected neighbour", sentence::for_each_words("Infected neighbours")),
        ("of B in my block", sentence::for_each_words("B in my block")),
        ("B neighbour", sentence::for_each_words("B neighbours")),
        (
            "One tick is now 1 day. Change it on the sheet.",
            sentence::tick_set_notice(TimeUnit::Day),
        ),
        // Errors on a rate in a button's card, a second agent kind, lines this version cannot run and the ground.
        (
            "A button's card has no duration, so it takes chances, not rates.",
            fixed(sentence::RATE_IN_BUTTON_CARD),
        ),
        (
            "One kind of agent per model in this version.",
            fixed(sentence::ONE_AGENT_KIND),
        ),
        ("Repeat is not supported yet.", fixed(sentence::NOT_YET_REPEAT)),
        (
            "Keep between, When, Repeat, Leave, Record and Move to inside a Repeat are not supported yet.",
            fixed(sentence::NOT_YET_REPEAT_CHANGE),
        ),
        (
            "Walls around agents that move by an arrow are not supported yet.",
            fixed(sentence::NOT_YET_AGENT_WALLS),
        ),
        (
            "Blocked ground under agents that move by an arrow is not supported yet.",
            fixed(sentence::NOT_YET_BLOCKED_MOVE_BY),
        ),
        (
            "Agents can start only on a state the ground paints on single cells.",
            fixed(sentence::ON_GROUND_CELLS),
        ),
        (
            "The cell ahead needs agents that move to cells.",
            fixed(sentence::AHEAD_NEEDS_CELLS),
        ),
        (
            "A Set inside a Repeat sets one value every time, so its value cannot read a line inside the Repeat.",
            fixed(sentence::SET_IN_REPEAT),
        ),
        (
            "Line 3 sets Velocity. A card that sets a property cannot also steer it.",
            sentence::set_already("Line 3", "Velocity"),
        ),
        (
            "Line 3 steers Velocity. A card that steers a property cannot also set it.",
            sentence::steered_already("Line 3", "Velocity"),
        ),
        (
            "Line 2 moves by an arrow. A card that moves by an arrow cannot also move to a place.",
            sentence::moved_by_already("Line 2"),
        ),
        (
            "Line 2 moves to a place. A card that moves to a place cannot also move by an arrow.",
            sentence::moved_to_already("Line 2"),
        ),
        ("line 3, Flockmates", sentence::look_name(&[(3, Some("Flockmates"))])),
        (
            "lines 1 and 2, Flockmates and Too close",
            sentence::look_name(&[(1, Some("Flockmates")), (2, Some("Too close"))]),
        ),
        ("line 4", sentence::look_name(&[(4, None)])),
        (
            "Line 1, Flockmates: looks at too many birds at 1,000,000 birds. Use a shorter distance or fewer birds.",
            sentence::look_costs_too_much("line 1, Flockmates", "birds", 1_000_000),
        ),
        (
            "Each tick, every bird: too much work at 1,000,000 birds. Use fewer birds.",
            sentence::card_costs_too_much("Each tick, every bird", "birds", 1_000_000),
        ),
        (
            "Line 1: too much work for one walker. Set a smaller maximum for Steps per tick.",
            sentence::line_costs_too_much("line 1", "walker", &sentence::smaller_maximum("Steps per tick")),
        ),
        (
            "Line 2, Wander: too much work for one walker. Repeat fewer times.",
            sentence::line_costs_too_much("line 2, Wander", "walker", sentence::FEWER_TIMES),
        ),
        (
            "Each tick, every bird: too much work for one bird. Use fewer lines on this card.",
            sentence::line_costs_too_much("Each tick, every bird", "bird", sentence::FEWER_LINES),
        ),
        (
            "Starting 20,000 persons takes too much work. Use fewer persons.",
            sentence::start_costs_too_much("persons", 20_000),
        ),
        ("or", fixed(sentence::COMPETING_WORD)),
        (
            "Lines 3 and 4 compete. A cell leaves Infected at the sum of their rates, and each line takes its share.",
            sentence::competing_tooltip(&[3, 4], "Infected", "cell"),
        ),
        (
            "This tick is long for this rate. A shorter tick follows the rate more closely.",
            fixed(sentence::LONG_TICK),
        ),
        ("Change to a rate", fixed(sentence::CHANGE_TO_RATE)),
        ("Change to a chance", fixed(sentence::CHANGE_TO_CHANCE)),
        (
            "Lines 3 and 5 use chances per tick. They do not follow the time scale.",
            sentence::chances_per_tick_note(&[3, 5]),
        ),
        // The Lines help.
        (
            "Every cell follows these lines each tick, from the top. A line either works out a result, which lines \
             below it can use, or changes one of my properties. Everyone reads the world as it was at the start of \
             the tick, and all changes land together at the end of it. Whatever no line changes stays as it was. \
             When two changes to one property both apply, the higher line wins, and the lower one shows \
             \"otherwise\".",
            fixed(sentence::LINES_HELP),
        ),
        (
            "Every agent of this kind follows these lines each tick, from the top. A line either works out a result, \
             which lines below it can use, or changes one of my properties. Everyone reads the world as it was at the \
             start of the tick, and all changes land together at the end of it. Whatever no line changes stays as it \
             was. When two changes to one property both apply, the higher line wins, and the lower one shows \
             \"otherwise\". Inside a Repeat, the first time a change applies decides, so a lower line that applies \
             earlier wins.",
            fixed(sentence::LINES_HELP_AGENTS),
        ),
        (
            "Alive neighbours = count of the 8 cells around me whose Status is Alive",
            fixed(sentence::LINES_HELP_EXAMPLE),
        ),
        ("Example:", fixed(sentence::HELP_EXAMPLE_LABEL)),
        ("Close help", fixed(sentence::CLOSE_HELP)),
        ("Lines", fixed(sentence::LINES_HELP_HEADING)),
        ("World help", sentence::help_dialog_name(SheetSection::World.heading())),
        ("Lines help", sentence::help_dialog_name(sentence::LINES_HELP_HEADING)),
        (
            "pass, stage, lane, domain, IR, shader, WGSL, kernel, buffer, node, port, wire, category, variant, seed, \
             stream, accumulator, region, compile",
            sentence::FORBIDDEN_WORDS.join(", "),
        ),
    ];

    let labels = [
        "+ Look around",
        "+ Chance",
        "+ Yes or no",
        "+ Calculate",
        "+ Change",
        "+ When, for several lines",
    ];
    let tooltips = [
        "Count or total of the cells around me (neighbours)",
        "Change a state by chance or at a rate",
        "Compare values, or join yes or no results",
        "Work out a number from two numbers",
        "Change a state or a number",
        "Give several lines one shared condition",
    ];
    let headings = [
        "Look at the cells around me (neighbours)",
        "Change a state by chance",
        "Make a yes or no result",
        "Work out a number",
        "Change one of my properties",
        "Several lines that share one condition",
    ];
    for (index, button) in AddButton::ALL.into_iter().enumerate() {
        entries.push((labels[index], fixed(button.label())));
        entries.push((tooltips[index], fixed(button.tooltip())));
        entries.push((headings[index], fixed(button.heading())));
    }

    // Each add menu's items, as "label: pattern", with the Become patterns' states in brackets.
    let items = [
        "Count: count of the cells around me (8, 4 or a 3 by 3 block) whose ... is ...",
        "Total or average: total of ... over the cells around me (8, 4 or a 3 by 3 block)",
        "Become, with chance: Become [new] from [old], with chance ..., optionally for each ...",
        "Become, at a rate: Become [new] from [old], at rate ... per day, optionally for each ...",
        "With chance: a yes or no: with chance ...",
        "At a rate: a yes or no: at rate ... per day",
        "Compare: ... is ..., is at least ..., is more than ... and others",
        "All of: all of ..., ...",
        "Any of: any of ..., ...",
        "Calculate: ... plus ...",
        "Become: Change a state: Become [new] from [old], when ...",
        "Set: Change a number: Set ... to ..., when ...",
        "When: When ...: then add the lines that share it",
    ];
    let written: Vec<String> = AddButton::ALL
        .iter()
        .flat_map(|button| button.items())
        .map(|item| format!("{}: {}", item.label(), item.pattern(Some(("[new]", "[old]")))))
        .collect();
    for (expected, actual) in items.iter().zip(written) {
        entries.push((expected, actual));
    }
    let inside: Vec<&str> = AddButton::ADD_INSIDE_ORDER
        .iter()
        .map(|button| button.label())
        .collect();
    entries.push((
        "+ Change, + When, for several lines, + Chance, + Look around, + Yes or no, + Calculate",
        inside.join(", "),
    ));

    // The comparison select, in order, and with a States property on the left.
    let comparisons = [
        "is",
        "is not",
        "is at least",
        "is at most",
        "is more than",
        "is less than",
        "is one of the numbers",
        "is none of the numbers",
    ];
    for (expected, test) in comparisons.iter().zip(COMPARISONS) {
        entries.push((expected, fixed(sentence::comparison_words(test, false))));
    }
    entries.push((
        "is one of the states",
        fixed(sentence::comparison_words(CompareTest::OneOf, true)),
    ));
    entries.push((
        "is none of the states",
        fixed(sentence::comparison_words(CompareTest::NotOneOf, true)),
    ));

    // Blanks: visible words, then accessible names.
    let words = [
        ("state", Blank::State),
        ("old state", Blank::OldState),
        ("property", Blank::Property),
        ("number property", Blank::NumberProperty),
        ("number", Blank::Number),
        ("value", Blank::Value),
        ("chance", Blank::Chance),
        ("rate", Blank::Rate),
        ("count", Blank::Count),
        ("result", Blank::YesOrNo),
        ("results", Blank::YesOrNoList),
        ("when", Blank::When),
    ];
    for (expected, blank) in words {
        entries.push((expected, fixed(blank.word())));
    }
    let names = [
        ("Line 2: choose a state", Blank::State),
        ("Line 2: choose the old state", Blank::OldState),
        ("Line 2: choose a property", Blank::Property),
        ("Line 2: choose a number property", Blank::NumberProperty),
        ("Line 2: choose a value", Blank::Value),
        ("Line 2: choose a number", Blank::Number),
        ("Line 2: choose a list of numbers", Blank::NumberList),
        ("Line 2: choose a list of states", Blank::StateList),
        ("Line 2: choose a chance", Blank::Chance),
        ("Line 2: choose a rate", Blank::Rate),
        ("Line 2: choose what to count", Blank::Count),
        ("Line 2: choose a yes or no result", Blank::YesOrNo),
        ("Line 2: choose yes or no results", Blank::YesOrNoList),
        ("Line 2: choose the lower number", Blank::LowerNumber),
        ("Line 2: choose the upper number", Blank::UpperNumber),
        ("Line 2: choose when", Blank::When),
    ];
    for (expected, blank) in names {
        entries.push((expected, blank.name("Line 2")));
    }
    entries.extend(rate_note_entries());
    entries.extend(sheet_entries());
    entries.extend(dialog_and_world_entries());
    assert_entries(&entries);
}

/// Returns the entries of the rate and chance notes, each template called with the table's example numbers.
fn rate_note_entries() -> Vec<(&'static str, String)> {
    let susceptible = sentence::become_subject("cell", Some("Susceptible"), Some("Infected"));
    vec![
        (
            "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. A rate is not a chance. Henad \
             turns it into one as 1 − e^(−0.1 × 1 day).",
            sentence::rate_note(
                "1 day",
                &sentence::become_subject("cell", Some("Infected"), Some("Recovered")),
                "0.0952",
                "0.1",
            ),
        ),
        (
            "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour, and \
             0.451 with 2. A rate is not a chance. Henad turns it into one as 1 − e^(−0.3 × n × 1 day), where n is \
             Infected neighbours.",
            sentence::rate_for_each_note("1 day", &susceptible, "Infected neighbours", "0.259", "0.451", "0.3"),
        ),
        (
            "with chance 0.259 when B in my block is 1, and 0.451 when it is 2",
            sentence::count_chances("B in my block", "0.259", "0.451"),
        ),
        (
            "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour, and \
             0.451 with 2. A rate is not a chance. Henad turns it into one as 1 − e^(−r × 1 day), where r is 0.3 \
             times Infected neighbours.",
            sentence::worked_out_rate_note(
                "1 day",
                &susceptible,
                "Infected neighbours",
                "0.259",
                "0.451",
                "0.3 times Infected neighbours",
            ),
        ),
        (
            "Each tick (1 day), a Susceptible cell becomes Infected with a chance that grows with r, where r is \
             Infected neighbours plus 0.3. A rate is not a chance. Henad turns it into one as 1 − e^(−r × 1 day).",
            sentence::growing_rate_note("1 day", &susceptible, "Infected neighbours plus 0.3", None),
        ),
        (
            "Each tick (1 day), a Susceptible cell becomes Infected with a chance that grows with r and n, where r is \
             ... and n is .... A rate is not a chance. Henad turns it into one as 1 − e^(−r × n × 1 day).",
            sentence::growing_rate_note("1 day", &susceptible, "...", Some("...")),
        ),
        (
            "Each tick, a cell becomes B with chance 0.2 with 1 B neighbour, and 0.36 with 2. Each one counted is a \
             separate try: 1 − (1 − 0.2)^n, where n is B neighbours.",
            sentence::chance_for_each_note(
                None,
                &sentence::become_subject("cell", None, Some("B")),
                "B neighbours",
                "0.2",
                "0.36",
            ),
        ),
        (
            "Each tick, a cell becomes B with a chance that grows with n, where p is ... and n is .... Each one \
             counted is a separate try: 1 − (1 − p)^n.",
            sentence::growing_chance_note(None, &sentence::become_subject("cell", None, Some("B")), "...", "..."),
        ),
        // The restart notice, the Integer kind, and the notes of competing rates, the long tick and the mean stay.
        (
            "Changes restart the world from tick 0.",
            fixed(sentence::RESTART_NOTICE),
        ),
        ("Integer", fixed(sentence::INTEGER_KIND)),
        (
            "A number with no fraction, such as 3 or -2.",
            fixed(sentence::INTEGER_KIND_TOOLTIP),
        ),
        (
            "Each tick (1 day), an Infected cell leaves Infected with chance 0.0952: Recovered 0.0476, Dead 0.0476. \
             A rate is not a chance. Henad turns it into one as 1 − e^(−(0.05 + 0.05) × 1 day).",
            sentence::competing_note(
                "1 day",
                "Infected",
                "0.0952",
                &[("Recovered", "0.0476"), ("Dead", "0.0476")],
                &["0.05", "0.05"],
                "cell",
            ),
        ),
        ("up to 0.91 with 8", sentence::long_tick_chance("0.91", 8)),
        (
            "On average a cell stays Infected 10.5 days at this tick length. It approaches 1/0.1 = 10 days as the \
             tick shrinks.",
            sentence::mean_stay_note("Infected", "10.5 days", "0.1", "10 days", "cell"),
        ),
    ]
}

/// Returns the entries of the sheet, each template called with the table's example.
#[expect(
    clippy::too_many_lines,
    reason = "one entry per row of the wording table reads best as one table"
)]
fn sheet_entries() -> Vec<(&'static str, String)> {
    let headings = [
        "Purpose",
        "World",
        "Cells",
        "Parameters",
        "Measures",
        "Colours",
        "Buttons",
    ];
    let mut entries: Vec<(&'static str, String)> = headings
        .iter()
        .zip(SheetSection::ALL)
        .map(|(expected, section)| (*expected, fixed(section.heading())))
        .collect();
    entries.push(("Help: Purpose", SheetSection::Purpose.help_name()));
    entries.push(("Help: Buttons", SheetSection::Buttons.help_name()));
    let units = [
        "one step (no unit)",
        "seconds",
        "minutes",
        "hours",
        "days",
        "weeks",
        "months",
        "years",
    ];
    let choices = std::iter::once(None).chain(TimeUnit::ALL.map(Some));
    for (expected, unit) in units.iter().zip(choices) {
        entries.push((expected, fixed(sentence::tick_unit_choice(unit))));
    }
    let suggestions = [
        "per second",
        "per minute",
        "per hour",
        "per day",
        "per week",
        "per month",
        "per year",
    ];
    for (expected, unit) in suggestions.iter().zip(TimeUnit::ALL) {
        entries.push((expected, sentence::unit_suggestion(unit)));
    }
    entries.push(("Name, Kind, Starts as, Changed by", sentence::CELLS_COLUMNS.join(", ")));
    entries.push(("Name, Value, Min, Max, Unit", sentence::PARAMETER_COLUMNS.join(", ")));
    entries.push((
        "Grey, Red, Orange, Yellow, Green, Blue, Purple",
        sentence::SHADE_COLORS.join(", "),
    ));
    entries.extend([
        // Purpose and World.
        ("What is this model for?", fixed(sentence::PURPOSE_PLACEHOLDER)),
        ("Cells on a grid", fixed(sentence::CELLS_ON_A_GRID)),
        ("Size", fixed(sentence::SIZE_LABEL)),
        ("by", fixed(sentence::SIZE_BY)),
        ("Width in cells", fixed(sentence::WIDTH_NAME)),
        ("Height in cells", fixed(sentence::HEIGHT_NAME)),
        ("Edges", fixed(sentence::EDGES_LABEL)),
        ("Wrap around", fixed(sentence::WRAP_AROUND)),
        ("Walls", fixed(sentence::WALLS)),
        (
            "The left edge joins the right edge, and the top edge joins the bottom.",
            fixed(sentence::WRAP_SENTENCE),
        ),
        (
            "Cells on an edge have fewer cells around them.",
            fixed(sentence::WALLS_SENTENCE),
        ),
        (
            "Cells on the left edge are neighbours of cells on the right edge, and the top edge joins the bottom.",
            fixed(sentence::WRAP_TOOLTIP),
        ),
        ("One tick is", fixed(sentence::ONE_TICK_IS)),
        ("Length of one tick", fixed(sentence::TICK_LENGTH_NAME)),
        ("Unit of one tick", fixed(sentence::TICK_UNIT_NAME)),
        // The property form and its checks.
        ("+ Add property", fixed(sentence::ADD_PROPERTY)),
        ("New property", fixed(sentence::NEW_PROPERTY)),
        ("Name", fixed(sentence::NAME_LABEL)),
        ("Kind", fixed(sentence::KIND_LABEL)),
        ("Such as Status or Health", fixed(sentence::PROPERTY_NAME_PLACEHOLDER)),
        ("Property name", fixed(sentence::PROPERTY_NAME_FIELD)),
        ("States", fixed(sentence::STATES_KIND)),
        ("Number", fixed(sentence::NUMBER_KIND)),
        (
            "One of a few named states, such as Dead and Alive. Give it at least two.",
            fixed(sentence::STATES_KIND_DESCRIPTION),
        ),
        (
            "Holds a number, such as an age or an amount of food.",
            fixed(sentence::NUMBER_KIND_DESCRIPTION),
        ),
        ("+ Add state", fixed(sentence::ADD_STATE)),
        ("State 1 colour: Dark grey", sentence::state_color_name(1, "Dark grey")),
        ("State 1 name", sentence::state_name_field(1)),
        ("Remove state 1", sentence::remove_state_name(1)),
        ("Done", fixed(sentence::DONE)),
        ("Cancel", fixed(sentence::CANCEL)),
        ("Type a name for the property.", fixed(sentence::PROPERTY_NAME_MISSING)),
        (
            "A property called Status already exists.",
            sentence::property_name_taken("Status"),
        ),
        (
            "Give the property at least two states.",
            fixed(sentence::TOO_FEW_STATES),
        ),
        (
            "Two states have the same name. Give each state its own name.",
            fixed(sentence::STATE_NAMES_REPEATED),
        ),
        ("Edit property Status", sentence::edit_property_heading("Status")),
        ("Delete property", fixed(sentence::DELETE_PROPERTY)),
        // Starts as.
        ("All [state]", sentence::starts_all("[state]")),
        ("By chance", fixed(sentence::BY_CHANCE)),
        ("with chance", fixed(sentence::WITH_CHANCE)),
        ("×", fixed(sentence::REMOVE_ROW_LABEL)),
        (
            "Status starts as, row 1: state",
            sentence::starts_field_name(&sentence::starts_row_place("Status", 1), StartsField::State),
        ),
        ("+ Add row", fixed(sentence::ADD_ROW)),
        ("Add row to Status starts as", sentence::add_row_name("Status")),
        (
            "Remove row 1 of Status starts as",
            sentence::remove_row_name(1, "Status"),
        ),
        ("otherwise", fixed(sentence::OTHERWISE)),
        ("Status starts as, otherwise", sentence::starts_otherwise_name("Status")),
        ("A value", fixed(sentence::A_VALUE)),
        ("Every cell starts at", fixed(sentence::STARTS_AT)),
        ("A random number", fixed(sentence::A_RANDOM_NUMBER)),
        ("A random number between", fixed(sentence::RANDOM_NUMBER_BETWEEN)),
        ("and", fixed(sentence::RANDOM_NUMBER_AND)),
        (
            "Status starts as, row 1: chance",
            sentence::starts_field_name(&sentence::starts_row_place("Status", 1), StartsField::Chance),
        ),
        (
            "Food starts as: number",
            sentence::starts_field_name(&sentence::starts_place("Food"), StartsField::Number),
        ),
        (
            "Food starts as, low end: number",
            sentence::starts_field_name(&sentence::starts_low_place("Food"), StartsField::Number),
        ),
        (
            "Food starts as, high end: number",
            sentence::starts_field_name(&sentence::starts_high_place("Food"), StartsField::Number),
        ),
        ("Type a chance such as 0.3 or 30%.", fixed(sentence::TYPE_A_CHANCE)),
        ("Type one number, such as 3 or 0.25.", fixed(sentence::TYPE_ONE_NUMBER)),
        (
            "A chance is from 0 to 1, or from 0% to 100%.",
            fixed(sentence::CHANCE_OUT_OF_RANGE),
        ),
        ("Use a parameter", fixed(sentence::USE_A_PARAMETER)),
        (
            "Status starts as, row 1: use a parameter",
            sentence::starts_field_name(&sentence::starts_row_place("Status", 1), StartsField::UseParameter),
        ),
        ("Choose a parameter", fixed(sentence::CHOOSE_A_PARAMETER)),
        ("New parameter...", fixed(sentence::NEW_PARAMETER)),
        ("Choose a chance", Blank::Chance.picker_title()),
        ("Choose a number", Blank::Number.picker_title()),
        ("Start again to see it", fixed(sentence::START_AGAIN_TO_SEE)),
        // Changed by and the sheet of a blank model.
        ("lines 2, 3", sentence::changed_by(&[2, 3])),
        ("line 2", sentence::changed_by(&[2])),
        ("no line yet", sentence::changed_by(&[])),
        (
            "Every cell has the properties listed here. Add the first one to start.",
            fixed(sentence::SHEET_EMPTY),
        ),
        // Parameters on the sheet.
        (
            "Name of Infection rate",
            sentence::sheet_parameter_name_field("Infection rate"),
        ),
        (
            "Infection rate value slider",
            sentence::value_slider_name("Infection rate"),
        ),
        ("Infection rate value", sentence::value_field_name("Infection rate")),
        (
            "Delete parameter Infection rate",
            sentence::delete_parameter_name("Infection rate"),
        ),
        ("Infection rate min", sentence::parameter_min_name("Infection rate")),
        ("Infection rate max", sentence::parameter_max_name("Infection rate")),
        ("Infection rate unit", sentence::parameter_unit_name("Infection rate")),
        ("none", fixed(sentence::UNIT_FIELD_PLACEHOLDER)),
        ("A parameter needs a name.", fixed(sentence::PARAMETER_NAME_MISSING)),
        (
            "A parameter called Infection rate already exists.",
            sentence::parameter_name_taken("Infection rate"),
        ),
        (
            "The world is trying 0.0005. The model keeps 0.0001.",
            sentence::sheet_trying_value("0.0005", "0.0001"),
        ),
        ("+ Add parameter", fixed(sentence::ADD_PARAMETER)),
        // Measures.
        ("Plot Alive", sentence::plot_state("Alive")),
        ("+ Add measure", fixed(sentence::ADD_MEASURE)),
        ("New measure", fixed(sentence::NEW_MEASURE)),
        ("Number of cells in a state", fixed(sentence::COUNT_MEASURE)),
        (
            "Number of cells whose [property] is [state]",
            sentence::count_measure("[property]", "[state]"),
        ),
        ("Average of a number", fixed(sentence::AVERAGE_MEASURE)),
        (
            "Average of [number property]",
            sentence::average_measure("[number property]"),
        ),
        ("Plotted", fixed(sentence::PLOTTED)),
        ("Colour of Alive: Blue", sentence::measure_color_name("Alive", "Blue")),
        (
            "Colour of recovered: Green",
            sentence::color_button_name("recovered", "Green"),
        ),
        ("Remove measure Alive", sentence::remove_measure_name("Alive")),
        ("Choose a property and a state.", fixed(sentence::MEASURE_NEEDS_STATE)),
        (
            "Choose a number property.",
            fixed(sentence::MEASURE_NEEDS_NUMBER_PROPERTY),
        ),
        (
            "That measure is already plotted.",
            fixed(sentence::MEASURE_ALREADY_PLOTTED),
        ),
        // Colours.
        ("Colour cells by", fixed(sentence::COLOR_CELLS_BY)),
        ("Shade", fixed(sentence::SHADE)),
        (
            "from [value] (dark) to [value] (light)",
            sentence::shade_range("[value]", "[value]"),
        ),
        ("Darkest at value", fixed(sentence::DARKEST_NAME)),
        ("Lightest at value", fixed(sentence::LIGHTEST_NAME)),
        // Buttons on the sheet.
        ("+ Add button", fixed(sentence::ADD_BUTTON)),
        ("Such as Clear", fixed(sentence::BUTTON_NAME_PLACEHOLDER)),
        ("Button name", fixed(sentence::BUTTON_NAME_FIELD)),
        ("Does", fixed(sentence::BUTTON_DOES)),
        ("Start again", fixed(sentence::START_AGAIN)),
        ("every property", fixed(sentence::EVERY_PROPERTY)),
        ("Start again for", fixed(sentence::START_AGAIN_FOR)),
        ("Run once, every cell", sentence::run_once("cell")),
        ("Edit button Clear", sentence::edit_button_name("Clear")),
        ("Delete button Clear", sentence::delete_button_name("Clear")),
        ("Type a name for the button.", fixed(sentence::BUTTON_NAME_MISSING)),
        (
            "A button called Clear already exists.",
            sentence::button_name_taken("Clear"),
        ),
    ]);
    entries
}

/// Returns the entries of the dialogs, the top bar, the world tabs, the help popovers, the Diagram and the
/// conversion, each template called with the table's example.
#[expect(
    clippy::too_many_lines,
    reason = "one entry per row of the wording table reads best as one table"
)]
fn dialog_and_world_entries() -> Vec<(&'static str, String)> {
    let starters = [
        ("Blank", "An empty grid of cells to build on."),
        (
            "Game of Life",
            "Conway's Life. A dead cell with 3 live neighbours comes alive.",
        ),
        (
            "SIR epidemic",
            "An epidemic that spreads to neighbours at a rate per day.",
        ),
        ("Majority vote", "Each cell takes the opinion of most of its block."),
        (
            "Forest fire",
            "Trees grow, lightning strikes, and fire spreads to neighbours.",
        ),
    ];
    let mut entries: Vec<(&'static str, String)> = Vec::new();
    for ((name, description), (written_name, written_description)) in starters.iter().zip(sentence::STARTER_CHOICES) {
        entries.push((name, fixed(written_name)));
        entries.push((description, fixed(written_description)));
    }
    let helps = [
        (
            "Say in a sentence what the model is for and what to look for when it runs.",
            "Spread of a disease through a town. Look for waves of infection.",
        ),
        (
            "The grid the cells live on: its size, what happens at its edges, and how much time one tick stands for. \
             A tick is one step of time, and every cell follows the card once per tick. Leave one tick as one step \
             (no unit) when time has no unit, as in Game of Life. With Wrap around, a cell on the left edge has \
             neighbours on the right edge.",
            "One tick is 1 day",
        ),
        (
            "A property is something every cell has. A States property is one of a few named states, such as Dead or \
             Alive. A Number property holds a number. Starts as says what every cell holds when the run starts.",
            "Status: States Dead, Alive. Starts as Alive with chance 0.3, otherwise Dead",
        ),
        (
            "A parameter is a named number with a slider. Lines can use it. The value here is the one the model \
             keeps. The slider beside the world tries other values in the run without changing the model.",
            "Infection rate = 0.3 per day",
        ),
        (
            "A measure is a number worked out from all the cells every tick, drawn on the plot under the world. Tick \
             Plot beside a state to count the cells in it.",
            "Number of cells whose Status is Alive",
        ),
        (
            "Choose which property colours the cells in the world. A States property uses the colours of its states. \
             A Number property is shaded from dark to light.",
            "Colour cells by Status",
        ),
        (
            "A button appears beside the world. Start again redraws how cells start. Run once changes every cell one \
             time, with its own lines.",
            "Clear: Become Dead from any state, always",
        ),
    ];
    for ((help, example), section) in helps.iter().zip(SheetSection::ALL) {
        entries.push((help, fixed(section.help())));
        entries.push((example, fixed(section.help_example())));
    }
    entries.extend([
        // The New model dialog.
        ("New model", fixed(sentence::NEW_MODEL)),
        ("Start from", fixed(sentence::START_FROM)),
        ("What is the world made of?", fixed(sentence::WORLD_QUESTION)),
        ("Agents in open space", fixed(sentence::AGENTS_IN_OPEN_SPACE)),
        ("Untitled model", fixed(sentence::UNTITLED_MODEL)),
        ("Create", fixed(sentence::CREATE)),
        // The New parameter dialog.
        ("New parameter", fixed(sentence::NEW_PARAMETER_HEADING)),
        (
            "A named number with a slider. Lines that use it change as the slider moves.",
            fixed(sentence::NEW_PARAMETER_LEAD),
        ),
        ("Parameter name", fixed(sentence::PARAMETER_NAME_FIELD)),
        ("Such as Infection rate", fixed(sentence::RATE_PARAMETER_PLACEHOLDER)),
        ("Such as Initial density", fixed(sentence::CHANCE_PARAMETER_PLACEHOLDER)),
        ("Such as Step size", fixed(sentence::NUMBER_PARAMETER_PLACEHOLDER)),
        ("Default", fixed(sentence::DEFAULT_LABEL)),
        ("Default value", fixed(sentence::DEFAULT_NAME)),
        ("Min", fixed(sentence::MIN_LABEL)),
        ("Max", fixed(sentence::MAX_LABEL)),
        ("Unit", fixed(sentence::UNIT_LABEL)),
        ("Optional, such as per day", fixed(sentence::DIALOG_UNIT_PLACEHOLDER)),
        ("Type a name.", fixed(sentence::DIALOG_NAME_MISSING)),
        (
            "Default, Min and Max must be numbers.",
            fixed(sentence::LIMITS_NOT_NUMBERS),
        ),
        ("Min must be below Max.", fixed(sentence::MIN_NOT_BELOW_MAX)),
        (
            "Default must be between Min and Max.",
            fixed(sentence::DEFAULT_OUTSIDE_LIMITS),
        ),
        (
            "A chance stays between 0 and 1. Set Min and Max inside that range.",
            fixed(sentence::CHANCE_LIMITS_OUTSIDE),
        ),
        ("Create parameter", fixed(sentence::CREATE_PARAMETER)),
        // The top bar.
        (
            "Model name: Game of Life. Rename",
            sentence::model_name_button("Game of Life"),
        ),
        ("Model name", fixed(sentence::MODEL_NAME_FIELD)),
        ("Undo", fixed(sentence::UNDO)),
        ("Redo", fixed(sentence::REDO)),
        ("Diagram", fixed(sentence::DIAGRAM)),
        ("Draw the lines as boxes and arrows", fixed(sentence::DIAGRAM_TOOLTIP)),
        // The world tabs.
        ("Tick 4 (a tick is one step of time)", sentence::tick_counter(4, None)),
        ("Tick 4 (4 days)", sentence::tick_counter(4, Some("4 days"))),
        (
            "Trying this value. The model keeps 0.0001.",
            sentence::trying_value("0.0001"),
        ),
        ("Reset", fixed(sentence::RESET)),
        (
            "Reset Lightning f to 0.0001",
            sentence::reset_name("Lightning f", "0.0001"),
        ),
        ("Keep", fixed(sentence::KEEP)),
        (
            "Keep 0.0005 for Lightning f",
            sentence::keep_name("0.0005", "Lightning f"),
        ),
        ("Make it the value the model keeps", fixed(sentence::KEEP_TOOLTIP)),
        (
            "Tick Plot beside a state under Measures to plot the number of cells in it.",
            fixed(sentence::PLOT_EMPTY),
        ),
        (
            "Add a property on the sheet to see the world.",
            fixed(sentence::WORLD_EMPTY),
        ),
        // The Diagram.
        ("Read-only. Edit the lines to change it.", fixed(sentence::DIAGRAM_NOTE)),
        ("Diagram of the card's 3 lines, read-only", sentence::diagram_name(3)),
        ("Inside line 4", sentence::inside_line_frame(4)),
        // Conversion to a node graph.
        (
            "Fix the problems on the card to convert.",
            fixed(sentence::CONVERT_NEEDS_FIXES),
        ),
        (
            "Some lines do not run yet. Finish or delete them to convert.",
            fixed(sentence::CONVERT_NEEDS_FINISHED_LINES),
        ),
        ("Convert to node graph", fixed(sentence::CONVERT_HEADING)),
        ("Game of Life (expert)", sentence::expert_copy_name("Game of Life")),
        ("Convert", fixed(sentence::CONVERT)),
        ("Edit as node graph", fixed(sentence::EDIT_AS_NODE_GRAPH)),
        ("Open Game of Life", sentence::open_original("Game of Life")),
        (
            "Converted to node graph: Game of Life (expert)",
            sentence::converted_link("Game of Life (expert)"),
        ),
        ("From the author model", fixed(sentence::FROM_AUTHOR_MODEL)),
    ]);
    let [first, second] = sentence::convert_text("Game of Life");
    entries.push((
        "Game of Life will be copied to a new model, Game of Life (expert), as a node graph.\nGame of Life stays as it \
         is.",
        first,
    ));
    entries.push((
        "In the node graph you can edit every node Henad runs.\nThe copy has no sheet or cards, so lines, fire counts \
         and \"otherwise\" will not appear in it.\nChanges to the copy cannot be turned back into lines.",
        second,
    ));
    entries
}

#[test]
fn the_game_of_life_starter_reads_the_final_sentences() {
    let game = document("game_of_life.toml");
    assert_eq!(
        card_sentences(&game, 0, &[]),
        [
            "Alive neighbours = count of the 8 cells around me whose Status is Alive",
            "Become Alive from Dead, when Alive neighbours is 3",
            "Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3",
        ]
    );
    assert_eq!(card_sentences(&game, 1, &[]), ["Become Dead from any state, always"]);
    let sentences = Sentences::new(&game);
    assert_eq!(sentences.card_heading(&game.cards[0]), "Each tick, every cell");
    assert_eq!(
        sentences.card_heading(&game.cards[1]),
        "Lines of Clear: run once, every cell, when pressed"
    );
}

#[test]
fn the_starters_read_the_final_sentences_with_their_otherwise() {
    let sir = document("sir.toml");
    assert_eq!(
        card_sentences(&sir, 0, &[]),
        [
            "Infected neighbours = count of the 8 cells around me whose Health is Infected",
            "Become Infected from Susceptible, at rate Infection rate per day for each infected neighbour",
            "Become Recovered from Infected, at rate Recovery rate per day",
        ]
    );
    assert_eq!(
        card_sentences(&sir, 1, &[]),
        ["Become Infected from Susceptible, with chance Initial infected"]
    );
    let vote = document("vote.toml");
    assert_eq!(
        card_sentences(&vote, 0, &[(3, &[2])]),
        [
            "B in my block = count of the 3 by 3 block including me whose Opinion is B",
            "Become B from any state, when B in my block is at least 5",
            "otherwise (when line 2 does not apply) become A from any state, always",
        ]
    );
    let fire = document("forest_fire.toml");
    assert_eq!(
        card_sentences(&fire, 0, &[(3, &[2])]),
        [
            "Burning neighbours = count of the 4 cells around me whose Cover is Burning",
            "Become Burning from Tree, when Burning neighbours is at least 1",
            "otherwise (when line 2 does not apply) become Burning from Tree, with chance Lightning chance",
            "Become Empty from Burning, always",
            "Become Tree from Empty, with chance Growth chance",
        ]
    );
}

#[test]
fn agent_lines_read_round_1_words_in_the_final_conventions() {
    let ants = document("ants.toml");
    let sentences = card_sentences(&ants, 0, &[(16, &[11, 14])]);
    assert_eq!(
        sentences[2],
        "Leave (Trail falloff times Home way's amount plus my Reward held) on To home, when my Cargo is Searching"
    );
    assert_eq!(
        sentences[4],
        "Way = depending on my Cargo: Searching -> Food way, Carrying food -> Home way"
    );
    assert_eq!(
        sentences[6],
        "Next cell = choose, first that applies: Ahead, when Way's amount is 0 and Ahead is open, with chance \
         Momentum; a random open neighbouring cell, with chance Random step chance; otherwise Way"
    );
    assert_eq!(
        sentences[8],
        "When my Cargo is Searching and Next cell's Ground is Food:"
    );
    assert_eq!(sentences[9], "Become Carrying food from any state, always");
    assert_eq!(sentences[14], "Record a Delivery, always");
    assert_eq!(
        sentences[15],
        "otherwise (when lines 11 and 14 do not apply) set Reward held to 0, always"
    );
    let boids = document("boids.toml");
    let bird = card_sentences(&boids, 0, &[]);
    assert_eq!(
        bird[0],
        "Flockmates = look at birds within Visual range: average of their Velocity (average velocity), average of \
         offset to them (centre)"
    );
    assert_eq!(bird[5], "Keep Velocity's length between Min speed and Max speed");
    assert_eq!(bird[6], "Move by my new Velocity");
    assert_eq!(
        card_sentences(&boids, 1, &[]),
        ["Set Velocity to a random direction of length (length of my Velocity), always"]
    );
    let opinion = document("bounded_confidence.toml");
    assert_eq!(
        card_sentences(&opinion, 0, &[])[0],
        "Like-minded = look at persons within Hearing range including me, only those whose Opinion is within \
         Confidence of my Opinion: average of their Opinion (average opinion)"
    );
    let walk = document("random_walk.toml");
    assert_eq!(
        card_sentences(&walk, 0, &[]),
        [
            "Repeat Steps per tick times:",
            "Move by a random direction of length Step size",
        ]
    );
}

#[test]
fn a_count_reads_in_the_singular_after_for_each() {
    for (name, words) in [
        ("Infected neighbours", "infected neighbour"),
        ("B neighbours", "B neighbour"),
        ("SIR cells", "SIR cell"),
        ("B in my block", "of B in my block"),
        ("my Food", "of my Food"),
        ("Neighbours", "neighbour"),
        ("Grass", "of Grass"),
    ] {
        assert_eq!(sentence::for_each_words(name), words, "{name}");
    }
}

#[test]
fn a_total_or_average_line_reads_over_its_cells() {
    let mut game = document("game_of_life.toml");
    game.cells
        .as_mut()
        .expect("Game of Life has cells")
        .push(SheetProperty::new("food", "Food", PropertyType::Number));
    let card = &mut game.cards[0];
    for (id, name, around, kind) in [
        ("l1", "Average Food around", Around::Cells8, SummaryKind::Average),
        ("l2", "Total Food around", Around::Block3x3, SummaryKind::Total),
    ] {
        let mut summary = LookSummary::new("summary", name);
        summary.summary = Some(kind);
        summary.value = Slot::Filled(SheetValue::Chip(SheetChip::Their("food".to_owned())));
        let line = card.line_tables.get_mut(id).expect("the line exists");
        *line = SheetLine::new(LineKind::LookAround);
        line.name = Some(name.to_owned());
        line.around = Some(around);
        line.summaries = vec![summary];
    }
    let sentences = card_sentences(&game, 0, &[]);
    assert_eq!(
        sentences[0],
        "Average Food around = average of Food over the 8 cells around me"
    );
    assert_eq!(
        sentences[1],
        "Total Food around = total of Food over the 3 by 3 block including me"
    );
    let text = game.to_text().expect("the sheet writes");
    assert!(
        text.contains("\n# Average Food around = average of Food over the 8 cells around me\n"),
        "{text}"
    );
}

#[test]
fn an_empty_total_or_average_value_is_a_number_property_blank() {
    use crate::sheet::sentence::SentencePart;

    let mut game = document("game_of_life.toml");
    let mut summary = LookSummary::new("summary", "Total around");
    summary.summary = Some(SummaryKind::Total);
    let line = game.cards[0].line_tables.get_mut("l1").expect("line 1");
    *line = SheetLine::new(LineKind::LookAround);
    line.name = Some("Total around".to_owned());
    line.around = Some(Around::Cells8);
    line.summaries = vec![summary];
    let sentences = Sentences::new(&game);
    let parts = sentences.line_parts("l1", &[]);
    assert!(parts.contains(&SentencePart::Blank(Blank::NumberProperty)), "{parts:?}");
    assert!(!parts.contains(&SentencePart::Blank(Blank::Property)), "{parts:?}");
    assert_eq!(Blank::NumberProperty.name("Line 1"), "Line 1: choose a number property");
}

#[test]
fn no_fixed_wording_entry_holds_a_forbidden_word() {
    let fixed_entries = [
        sentence::LAW,
        sentence::ADD_AT_END_NOTE,
        sentence::BUTTON_CARD_EMPTY,
        sentence::BLANK_MODEL_CARD,
        sentence::ALWAYS_TOOLTIP,
        sentence::CONDITION_ARROW_TOOLTIP,
        sentence::NO_YES_OR_NO_RESULT,
        sentence::NO_STATES,
        sentence::LINES_HELP,
        sentence::LINES_HELP_AGENTS,
        sentence::LINES_HELP_AGENTS_EXAMPLE,
        sentence::AGENT_WALLS_SENTENCE,
        sentence::AGENT_WRAP_TOOLTIP,
        sentence::WORLD_HELP_AGENTS,
        sentence::LINES_HELP_HEADING,
        sentence::FOR_EACH_BLANK_WORDS,
        sentence::FROM_WORD,
        sentence::RATE_IN_BUTTON_CARD,
        sentence::NOT_YET_REPEAT,
        sentence::NOT_YET_REPEAT_CHANGE,
        sentence::NOT_YET_AGENT_WALLS,
        sentence::NOT_YET_BLOCKED_MOVE_BY,
        sentence::LINE_TOO_LONG,
        sentence::LINE_NUMBER_TOO_LARGE,
        sentence::SHEET_TOO_LARGE,
        sentence::NAME_RESERVED,
        sentence::PREVIEW_WAITS,
        sentence::ON_GROUND_CELLS,
        sentence::AHEAD_NEEDS_CELLS,
        sentence::SET_IN_REPEAT,
        sentence::LONG_TICK,
        sentence::NOT_RUN_TICK,
        sentence::RESTART_NOTICE,
        sentence::INTEGER_KIND_TOOLTIP,
        sentence::WRAP_SENTENCE,
        sentence::WALLS_SENTENCE,
        sentence::WRAP_TOOLTIP,
        sentence::SHEET_EMPTY,
        sentence::STATES_KIND_DESCRIPTION,
        sentence::NUMBER_KIND_DESCRIPTION,
        sentence::STATE_NAMES_REPEATED,
        sentence::NEW_PARAMETER_LEAD,
        sentence::CHANCE_LIMITS_OUTSIDE,
        sentence::KEEP_TOOLTIP,
        sentence::PLOT_EMPTY,
        sentence::WORLD_EMPTY,
        sentence::DIAGRAM_TOOLTIP,
        sentence::DIAGRAM_NOTE,
    ];
    let mut texts: Vec<String> = fixed_entries.iter().map(|text| (*text).to_owned()).collect();
    texts.push(sentence::help_dialog_name(sentence::LINES_HELP_HEADING));
    texts.push(sentence::name_reserved("Count"));
    texts.push(sentence::empty_field("Health", "a chance"));
    for section in SheetSection::ALL {
        texts.push(sentence::help_dialog_name(section.heading()));
        texts.push(section.help().to_owned());
        texts.push(section.help_example().to_owned());
    }
    for (name, description) in sentence::STARTER_CHOICES {
        texts.push(name.to_owned());
        texts.push(description.to_owned());
    }
    for button in AddButton::ALL {
        texts.push(button.tooltip().to_owned());
        texts.push(button.heading().to_owned());
        for item in button.items() {
            texts.push(item.pattern(None));
        }
    }
    for button in AddButton::AGENTS {
        texts.push(button.label().to_owned());
        texts.push(button.tooltip_for(true).to_owned());
        texts.push(button.heading_for(true).to_owned());
        for item in button.items_for(true) {
            texts.push(item.label().to_owned());
            texts.push(item.pattern(None));
        }
    }
    for select in LineSelect::ALL {
        texts.push(select.name("Line 2"));
    }
    for blank in Blank::ALL {
        texts.push(blank.name("Line 2"));
    }
    for item in ConditionItem::ALL {
        texts.extend(item.description().map(str::to_owned));
    }
    for text in &texts {
        for word in text.split(|c: char| !c.is_alphanumeric()) {
            assert!(
                !sentence::is_forbidden_word(word),
                "{text:?} holds the forbidden word {word:?}"
            );
        }
    }
}

#[test]
fn the_parts_of_a_life_line_name_each_chip_select_and_word() {
    use crate::sheet::sentence::{ChipKind, SentenceChip, SentencePart};

    let game = document("game_of_life.toml");
    let sentences = Sentences::new(&game);
    let chip = |kind: ChipKind, name: &str| {
        SentencePart::Chip(SentenceChip {
            kind,
            lead: "",
            name: name.to_owned(),
        })
    };
    let state = |id: &str, name: &str| {
        chip(
            ChipKind::State {
                property: Some("status".to_owned()),
                state: id.to_owned(),
            },
            name,
        )
    };
    let text = |words: &str| SentencePart::Text(words.to_owned());
    assert_eq!(
        sentences.line_parts("l1", &[]),
        [
            text("Alive neighbours = count of "),
            SentencePart::Select(LineSelect::WhichCells, "the 8 cells around me".to_owned()),
            text(" whose "),
            chip(ChipKind::Property("status".to_owned()), "Status"),
            text(" is "),
            state("alive", "Alive"),
        ]
    );
    assert_eq!(
        sentences.line_parts("l2", &[]),
        [
            text("Become "),
            state("alive", "Alive"),
            text(" "),
            SentencePart::From,
            text(" "),
            state("dead", "Dead"),
            text(", "),
            SentencePart::ConditionWord("when"),
            text(" "),
            chip(ChipKind::Result("l1".to_owned()), "Alive neighbours"),
            text(" "),
            SentencePart::Select(LineSelect::Comparison, "is".to_owned()),
            text(" "),
            chip(ChipKind::Literal, "3"),
        ]
    );
    let line_three = sentences.line_parts("l3", &[]);
    assert_eq!(
        line_three[line_three.len() - 3..],
        [chip(ChipKind::Literal, "2"), text(", "), chip(ChipKind::Literal, "3")]
    );
    assert_eq!(
        sentences.line_parts("l4", &[]),
        [
            text("Become "),
            state("dead", "Dead"),
            text(" "),
            SentencePart::From,
            text(" "),
            chip(ChipKind::AnyState, "any state"),
            text(", "),
            SentencePart::Always,
        ]
    );
}

#[test]
fn otherwise_blanks_and_the_empty_for_each_are_parts_of_their_own() {
    use crate::sheet::sentence::{SentencePart, sentence_text};

    let fire = document("forest_fire.toml");
    let sentences = Sentences::new(&fire);
    let parts = sentences.line_parts("l3", &[2]);
    assert_eq!(parts.first(), Some(&SentencePart::Otherwise(vec![2])));
    assert_eq!(parts.get(1), Some(&SentencePart::Text(" become ".to_owned())));
    assert_eq!(parts.last(), Some(&SentencePart::ForEachBlank(DrawKind::Chance)));
    assert_eq!(
        sentence_text(&parts),
        "otherwise (when line 2 does not apply) become Burning from Tree, with chance Lightning chance"
    );

    let mut game = document("game_of_life.toml");
    let line = game.cards[0].line_tables.get_mut("l2").expect("line 2");
    line.start_state = None;
    line.condition = Slot::Blank;
    let sentences = Sentences::new(&game);
    let parts = sentences.line_parts("l2", &[]);
    assert!(parts.contains(&SentencePart::Blank(Blank::OldState)), "{parts:?}");
    assert_eq!(parts.last(), Some(&SentencePart::Blank(Blank::When)));
    assert_eq!(sentence_text(&parts), "Become Alive from ..., ...");
}

#[test]
fn no_line_of_a_test_sheet_holds_two_adjacent_word_parts() {
    use crate::sheet::sentence::SentencePart;

    for (file_name, _) in sheets() {
        let sheet = document(file_name);
        let sentences = Sentences::new(&sheet);
        for card in &sheet.cards {
            for id in card.line_tables.keys() {
                let parts = sentences.line_parts(id, &[]);
                assert!(
                    parts
                        .windows(2)
                        .all(|pair| !matches!(pair, [SentencePart::Text(_), SentencePart::Text(_)])),
                    "{file_name} {id} splits its words: {parts:?}"
                );
            }
        }
    }
}

#[test]
fn a_typed_number_reads_as_its_known_answers() {
    use crate::sheet::document::SheetLiteral;

    let read = sentence::read_typed_number;
    assert_eq!(read("3"), Some(SheetLiteral::Number(3.0)));
    assert_eq!(read(" -0.25 "), Some(SheetLiteral::Number(-0.25)));
    assert_eq!(read(".5"), Some(SheetLiteral::Number(0.5)));
    assert_eq!(read("1e-3"), Some(SheetLiteral::Number(0.001)));
    assert_eq!(read("30%"), Some(SheetLiteral::Percent(30.0)));
    assert_eq!(read("2.5 %"), Some(SheetLiteral::Percent(2.5)));
    for text in ["", "%", "abc", "2, 3", "inf", "-inf", "NaN", "1e400", "3 4", "30%%"] {
        assert_eq!(read(text), None, "{text:?}");
    }
    // A percent shows as typed, and stands for its number divided by 100.
    let percent = read("30%").expect("a percent");
    assert_eq!(sentence::literal_words(percent), "30%");
    assert_eq!(percent.number(), Some(0.3));
    assert_eq!(
        sentence::world_side_out_of_range(16384),
        "Type a size from 1 to 16384 cells."
    );
}

#[test]
fn every_control_of_a_life_line_is_placed_at_the_slot_or_field_it_shows() {
    use crate::sheet::sentence::{FieldPlace, PartPlace, PlaceTarget};

    let game = document("game_of_life.toml");
    let sentences = Sentences::new(&game);
    let places = |line_id: &str| -> Vec<(String, Option<PartPlace>)> {
        sentences
            .placed_parts(line_id, &[])
            .into_iter()
            .filter(|placed| {
                !matches!(
                    placed.part,
                    sentence::SentencePart::Text(_) | sentence::SentencePart::From
                )
            })
            .map(|placed| (placed.part.text(), placed.place))
            .collect()
    };
    let slot = |path: &str, want: Blank| {
        Some(PartPlace {
            target: PlaceTarget::Slot(path.to_owned()),
            want: Some(want),
        })
    };
    let field = |phrase: &str, field: FieldPlace, want: Option<Blank>| {
        Some(PartPlace {
            target: PlaceTarget::Field {
                phrase: phrase.to_owned(),
                field,
            },
            want,
        })
    };
    assert_eq!(
        places("l1"),
        [
            ("the 8 cells around me".to_owned(), field("", FieldPlace::Around, None)),
            ("Status".to_owned(), slot("whose.left", Blank::Property)),
            ("Alive".to_owned(), slot("whose.right", Blank::State)),
        ]
    );
    assert_eq!(
        places("l2"),
        [
            ("Alive".to_owned(), field("", FieldPlace::NewState, Some(Blank::State))),
            (
                "Dead".to_owned(),
                field("", FieldPlace::StartState, Some(Blank::OldState))
            ),
            ("when".to_owned(), None),
            ("Alive neighbours".to_owned(), slot("condition.left", Blank::Value)),
            ("is".to_owned(), field("condition", FieldPlace::Test, None)),
            ("3".to_owned(), slot("condition.right", Blank::Number)),
        ]
    );
    let line_three = places("l3");
    let list_item = |index: usize| {
        Some(PartPlace {
            target: PlaceTarget::ListItem {
                slot: "condition.right".to_owned(),
                index,
            },
            want: Some(Blank::NumberList),
        })
    };
    assert_eq!(
        line_three[line_three.len() - 2..],
        [("2".to_owned(), list_item(0)), ("3".to_owned(), list_item(1))]
    );
    assert_eq!(
        places("l4").last(),
        Some(&("always".to_owned(), slot("condition", Blank::When)))
    );
}

#[test]
fn a_new_count_wants_a_property_then_a_state_and_an_empty_calculate_its_two_values() {
    use crate::sheet::sentence::{FieldPlace, PlaceTarget, SentencePart, sentence_text};

    let mut game = document("game_of_life.toml");
    let line = game.cards[0].line_tables.get_mut("l1").expect("line 1");
    let mut whose = SheetLine::new(LineKind::Compare);
    whose.test = Some(CompareTest::Is);
    line.whose = Slot::Filled(SheetValue::Phrase(Box::new(whose)));
    line.name = Some("Count".to_owned());
    let mut calculate = SheetLine::new(LineKind::Calculate);
    calculate.name = Some("Result".to_owned());
    game.cards[0].line_tables.insert("l9".to_owned(), calculate);
    game.cards[0].lines.push("l9".to_owned());
    let sentences = Sentences::new(&game);
    let parts = sentences.line_parts("l1", &[]);
    assert_eq!(
        sentence_text(&parts),
        "Count = count of the 8 cells around me whose ... is ..."
    );
    assert!(parts.contains(&SentencePart::Blank(Blank::Property)), "{parts:?}");
    assert!(parts.contains(&SentencePart::Blank(Blank::State)), "{parts:?}");
    let placed = sentences.placed_parts("l9", &[]);
    let targets: Vec<PlaceTarget> = placed
        .iter()
        .filter_map(|placed| placed.place.as_ref().map(|place| place.target.clone()))
        .collect();
    let field = |field: FieldPlace| PlaceTarget::Field {
        phrase: String::new(),
        field,
    };
    assert_eq!(
        targets,
        [
            field(FieldPlace::FirstOperand),
            field(FieldPlace::Operation),
            field(FieldPlace::SecondOperand)
        ]
    );
    assert_eq!(sentences.line("l9", &[]), "Result = ... ... ...");
    assert_eq!(
        sentence::binary_formula("l1 / 2"),
        Some(("l1".to_owned(), '/', "2".to_owned()))
    );
    assert_eq!(sentence::binary_formula("-l1"), None);
    assert_eq!(sentences.formula_operand("l1").name, "Count");
}

#[test]
fn the_words_for_each_are_a_part_placed_at_its_count() {
    use crate::sheet::sentence::{PartPlace, PlaceTarget, SentencePart, sentence_text};

    let text = super::sheet_document::sheet_text("sir_chance.toml");
    let sir = ParsedSheet::parse(text).expect("the sheet reads").into_document();
    let sentences = Sentences::new(&sir);
    let placed = sentences.placed_parts("l2", &[]);
    let tries = Some(PartPlace {
        target: PlaceTarget::Slot("condition.tries".to_owned()),
        want: Some(Blank::Count),
    });
    let for_each = placed
        .iter()
        .find(|placed| placed.part == SentencePart::ForEach(DrawKind::Chance))
        .expect("the words for each");
    assert_eq!(for_each.place, tries);
    assert_eq!(placed.last().map(|placed| placed.place.clone()), Some(tries));
    let parts: Vec<SentencePart> = placed.into_iter().map(|placed| placed.part).collect();
    assert_eq!(
        sentence_text(&parts),
        "Become Infected from Susceptible, with chance Infection chance for each infected neighbour"
    );
}

#[test]
fn a_formula_inside_a_slot_places_no_value_and_no_operation() {
    use crate::sheet::sentence::{FieldPlace, PlaceTarget, SentencePart, sentence_text};

    let text = super::sheet_document::sheet_text("sir_chance.toml");
    let mut sir = ParsedSheet::parse(text).expect("the sheet reads").into_document();
    let line = sir.cards[0].line_tables.get_mut("l2").expect("line 2");
    let Slot::Filled(SheetValue::Phrase(condition)) = &mut line.condition else {
        panic!("line 2 has a chance");
    };
    let mut calculate = SheetLine::new(LineKind::Calculate);
    calculate.formula = Some("infection_chance / 2".to_owned());
    condition.chance = Slot::Filled(SheetValue::Phrase(Box::new(calculate)));
    let sentences = Sentences::new(&sir);
    let placed = sentences.placed_parts("l2", &[]);
    let operand_places: Vec<&PlaceTarget> = placed
        .iter()
        .filter_map(|placed| placed.place.as_ref().map(|place| &place.target))
        .filter(|target| {
            matches!(
                target,
                PlaceTarget::Field {
                    field: FieldPlace::FirstOperand | FieldPlace::Operation | FieldPlace::SecondOperand,
                    ..
                }
            )
        })
        .collect();
    assert!(operand_places.is_empty(), "{operand_places:?}");
    let parts: Vec<SentencePart> = placed.into_iter().map(|placed| placed.part).collect();
    assert_eq!(
        sentence_text(&parts),
        "Become Infected from Susceptible, with chance (Infection chance divided by 2) for each infected neighbour"
    );
}

#[test]
fn the_banner_of_a_copy_reads_as_the_expert_design_writes_it() {
    assert_eq!(sentence::converted_date("2026-11-07"), "7 Nov 2026");
    assert_eq!(sentence::converted_date("2026-13-07"), "2026-13-07");
    assert_eq!(sentence::converted_date("today"), "today");
    assert_eq!(
        sentence::converted_banner("Game of Life", "2026-11-07"),
        "Node graph converted from Game of Life on 7 Nov 2026. Frames show the line each node came from."
    );
    assert_eq!(sentence::node_edits(2, 0, 1), "2 nodes added, 1 changed");
    assert_eq!(sentence::node_edits(0, 0, 1), "1 node changed");
    assert_eq!(sentence::node_edits(0, 3, 1), "3 nodes removed, 1 changed");
    assert_eq!(sentence::node_edits(0, 2, 0), "2 nodes removed");
    assert_eq!(sentence::node_edits(1, 3, 0), "1 node added, 3 removed");
    assert_eq!(sentence::node_edits(0, 0, 0), "no node changed");
    assert_eq!(
        sentence::edited_banner("Game of Life", "2026-11-07", &sentence::node_edits(2, 0, 1)),
        "Node graph converted from Game of Life on 7 Nov 2026. Edited since: 2 nodes added, 1 changed."
    );
    assert_eq!(
        sentence::original_changed_banner("Game of Life"),
        "Game of Life has changed since this conversion. Convert it again for a new node graph."
    );
    assert_eq!(
        sentence::original_not_beside("Game of Life"),
        "Game of Life is not beside this file. Frames still show the line each node came from."
    );
}

#[test]
fn a_comparison_of_a_copy_reads_in_full_sentences() {
    assert_eq!(
        sentence::compare_running("Game of Life", "Game of Life (expert)", 200, 1),
        "Running Game of Life and Game of Life (expert) for 200 ticks at seed 1."
    );
    assert_eq!(
        sentence::runs_match("Game of Life", "Game of Life (expert)", 200),
        "Game of Life (expert) matches Game of Life at every tick from 0 to 200."
    );
    assert_eq!(
        sentence::runs_differ("Game of Life", "Game of Life (expert)", 1, "Alive", "366946", "359603"),
        "Game of Life (expert) first differs from Game of Life at tick 1: Alive is 366946 against 359603."
    );
    assert_eq!(
        sentence::runs_share_no_measure("Game of Life", "Game of Life (expert)"),
        "Game of Life (expert) and Game of Life share no measure to compare."
    );
    assert_eq!(
        sentence::runs_do_not_replay("Boids", "Boids (expert)"),
        "Boids does not replay exactly. Two runs at one seed can differ, so compare Boids (expert) with Boids in the \
         plot."
    );
    assert_eq!(sentence::compare_failed("no adapter"), "Comparison failed: no adapter");
    assert_eq!(sentence::compare_series("Alive", "Game of Life"), "Alive, Game of Life");
}

/// The notes under a card that runs on agents name the agents, as the card's heading does. Game of Life's card, set to
/// run on ants, keeps its order note.
#[test]
fn the_notes_of_an_agent_card_name_its_agents() {
    use crate::sheet::relations::CardRelations;

    let ants = document("ants.toml");
    let mut game = document("game_of_life.toml");
    game.agents.push(ants.agents[0].clone());
    let card = game
        .cards
        .iter_mut()
        .find(|card| card.every == "cells")
        .expect("the cell card");
    card.every = ants.agents[0].id.clone();
    let sentences = Sentences::new(&game);
    let card = game
        .cards
        .iter()
        .find(|card| card.every == "ant")
        .expect("the ant card");
    let notes: Vec<String> = CardRelations::new(&game, card)
        .notes(&sentences, |_| true)
        .into_iter()
        .map(|note| note.text)
        .collect();
    assert!(
        notes
            .iter()
            .any(|note| note.starts_with("Lines 2 and 3 change different ants (")),
        "{notes:?}"
    );
    assert!(notes.iter().all(|note| !note.contains("cells")), "{notes:?}");
}
