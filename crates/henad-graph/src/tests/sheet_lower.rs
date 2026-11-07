//! The lowering of grid sheets: every grid sheet checked and compiled, ids, keys and the line table, the code identity
//! under the author format's edits, known answers on the host, the two ways a world starts again, skipped lines, the
//! refusals of the line kinds this version does not lower and the refusals lifted, chances and their power rule,
//! looks, calculations and choices, buttons, measures, colours, the node limits and a mutation corpus.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::action::action_seed;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::explore::fingerprint::fnv1a64;
use henad_core::params::ParamValue;

use super::sheet_document::{mutate, sheet_text, sheets};
use crate::document::PassStage;
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::limits::MAX_NODES_IN_FILE;
use crate::sheet::edit::lines::{parse_path, phrase_mut, value_mut};
use crate::sheet::limits::MAX_LINES_PER_CARD;
use crate::sheet::{
    KIND_SHEET, NodeOwner, SHEET_CODE_FORMAT, SHEET_VERSION, SheetFile, SheetFileError, sentence, starters,
};
use crate::wgsl::GridProgram;

/// Text of the Game of Life starter.
fn life() -> &'static str {
    starters()[0].text
}

/// Text of the Majority vote starter.
fn vote() -> &'static str {
    starters()[1].text
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Lowers `text`, which must lower.
fn lowered(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("the sheet does not lower: {error}"))
}

/// Returns the errors of lowering `text`, which must hold some.
fn refusals(text: &str) -> Vec<henad_core::diagnostic::CodeDiagnostic> {
    match SheetFile::parse(text) {
        Ok(_) => panic!("the sheet lowers:\n{text}"),
        Err(SheetFileError::Invalid(errors)) => errors.as_slice().to_vec(),
        Err(other) => panic!("the sheet does not read: {other}"),
    }
}

/// Returns the bytes of `text` that `diagnostic` points at.
fn placed_at<'a>(diagnostic: &henad_core::diagnostic::CodeDiagnostic, text: &'a str) -> &'a str {
    let span = diagnostic
        .span
        .clone()
        .unwrap_or_else(|| panic!("{diagnostic} has no place"));
    &text[span]
}

/// Game of Life on a wrapping grid `side` cells wide and high.
fn life_of_side(side: u32) -> String {
    let text = edited(life(), "width = 1024", &format!("width = {side}"));
    edited(&text, "height = 1024", &format!("height = {side}"))
}

/// Game of Life with line `table` added to the card that runs each tick as line `l5`.
fn life_with_line(table: &str) -> String {
    let text = edited(life(), "next_line = 5", "next_line = 6");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
    );
    edited(
        &text,
        "# Lines of Clear: run once, every cell, when pressed",
        &format!("{table}\n\n# Lines of Clear: run once, every cell, when pressed"),
    )
}

/// Every grid sheet: the grid starters, then the SIR epidemic with chances.
fn grid_sheets() -> Vec<(&'static str, &'static str)> {
    starters()
        .iter()
        .filter(|starter| starter.text.contains("made_of = \"cells\""))
        .map(|starter| (starter.file_name, starter.text))
        .chain(sheets().into_iter().filter(|(name, _)| *name == "sir_chance.toml"))
        .collect()
}

#[test]
fn every_grid_sheet_lowers_checks_and_compiles() {
    let names: Vec<&str> = grid_sheets().iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        ["game_of_life.toml", "vote.toml", "forest_fire.toml", "sir_chance.toml"]
    );
    for (file_name, text) in grid_sheets() {
        let file = lowered(text);
        assert!(file.warnings().is_empty(), "{file_name}: {:?}", file.warnings());
        let report = file.lowered().check();
        assert!(
            report.diagnostics.is_empty(),
            "{file_name}: {:#?}\n{}",
            report.diagnostics,
            file.lowered().to_toml()
        );
        let program =
            GridProgram::compile(file.lowered()).unwrap_or_else(|problems| panic!("{file_name}: {problems:#?}"));
        let entry = program.entry();
        assert_eq!(entry.id(), file.document().sheet.id);
        assert_eq!(file.code(), file.lowered().code());
        assert_eq!(file.to_toml(), text, "a sheet is its own canonical text");
    }
}

#[test]
fn the_lowered_vote_counts_its_block_and_lets_line_2_win() {
    let file = lowered(vote());
    let document = file.lowered().document();
    let tick = document
        .passes
        .iter()
        .find(|pass| pass.stage == PassStage::Tick)
        .expect("a tick pass");
    assert_eq!(tick.id, "tick_cell_tick");
    let count = tick
        .body
        .nodes
        .iter()
        .find(|node| node.op == "grid.count")
        .expect("a grid count");
    assert_eq!(file.line_map().line_of(count.id), Some("l1"));
    assert_eq!(count.config["include_center"], crate::value::ConfigValue::Bool(true));
    let transition = tick
        .body
        .nodes
        .iter()
        .find(|node| node.op == "cat.transition")
        .expect("a transition");
    let ids: Vec<&str> = transition.config["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .filter_map(|rule| rule.as_table()?.get("id")?.as_str())
        .collect();
    assert_eq!(
        ids,
        ["l2_a", "l2_b", "l3_a", "l3_b"],
        "a change from any state takes a row per state"
    );
}

#[test]
fn lowering_is_deterministic() {
    for text in [life().to_owned(), vote().to_owned(), kinds()] {
        let first = lowered(&text).lowered().to_toml();
        let second = lowered(&text).lowered().to_toml();
        assert_eq!(first, second);
    }
}

#[test]
fn lowered_ids_sit_in_their_line_blocks() {
    for text in [life().to_owned(), vote().to_owned(), kinds()] {
        let file = lowered(&text);
        for (id, origin) in file.line_map().iter() {
            match &origin.owner {
                NodeOwner::Line(line) => {
                    let k: u32 = line[1..].parse().expect("a line id");
                    assert!(
                        (1000 * k + 1..=1000 * k + 999).contains(&id.number()),
                        "{id} sits outside the block of {line}"
                    );
                }
                NodeOwner::Sheet => assert!(id.number() <= 999, "{id} is the sheet's and past n999"),
            }
        }
    }
}

#[test]
fn the_line_map_covers_every_written_node() {
    for text in [life().to_owned(), vote().to_owned(), kinds()] {
        let file = lowered(&text);
        let written: BTreeSet<_> = file
            .lowered()
            .document()
            .passes
            .iter()
            .flat_map(|pass| pass.body.nodes.iter().map(|node| node.id))
            .collect();
        let mapped: BTreeSet<_> = file.line_map().iter().map(|(id, _)| id).collect();
        assert_eq!(written, mapped);
        assert_eq!(file.line_map().len(), written.len());
        // Line 3 of Game of Life writes its two comparisons and their and, all from its condition.
        let paths: Vec<&str> = file
            .line_map()
            .nodes_of("l3")
            .filter_map(|id| file.line_map().origin(id))
            .map(|origin| origin.path.as_str())
            .collect();
        if text == life() {
            assert_eq!(paths, ["condition.right.0", "condition.right.1", "condition.right.1"]);
        }
    }
}

/// Paths of the nodes a line owns that name no slot: the line itself, the join with the condition of a When around
/// it, its share of a property's change chain, and the expression of a Calculate.
const LINE_PATHS_OF_NO_SLOT: [&str; 4] = ["", "when", "change", "formula"];

#[test]
fn every_line_map_path_of_a_line_reads_as_an_edit_command_slot_path() {
    let mut sheets: Vec<(&str, &str)> = starters()
        .iter()
        .map(|starter| (starter.file_name, starter.text))
        .collect();
    let values = values();
    sheets.push(("values.toml", &values));
    for (file_name, text) in sheets {
        let file = lowered(text);
        let mut slots = 0;
        for (id, origin) in file.line_map().iter() {
            let NodeOwner::Line(line_id) = &origin.owner else {
                continue;
            };
            if LINE_PATHS_OF_NO_SLOT.contains(&origin.path.as_str()) {
                continue;
            }
            let mut line = file
                .document()
                .cards
                .iter()
                .find_map(|card| card.line_tables.get(line_id))
                .unwrap_or_else(|| panic!("{file_name}: no line {line_id}"))
                .clone();
            let segments: Vec<&str> = origin.path.split('.').collect();
            // The `logic.select` of a Choose row sits at the row itself, which names no slot.
            if let ["row", row] = segments.as_slice() {
                let row: usize = row.parse().expect("a row index");
                assert!(row < line.rows.len(), "{file_name}: '{}' of {line_id}", origin.path);
                continue;
            }
            // The longest prefix that reads as a slot path. Past it, a path names an item of a list or a node's role.
            let steps = (1..=segments.len())
                .rev()
                .find_map(|end| parse_path(&segments[..end].join(".")).ok())
                .unwrap_or_else(|| panic!("{file_name}: {id} of {line_id} at '{}' names no slot", origin.path));
            let (last, inner) = steps.split_last().expect("a parsed path has a step");
            let phrase = phrase_mut(&mut line, inner)
                .unwrap_or_else(|error| panic!("{file_name}: '{}' of {line_id}: {error}", origin.path));
            assert!(
                value_mut(phrase, last).is_some(),
                "{file_name}: '{}' of {line_id} names an empty slot",
                origin.path
            );
            slots += 1;
        }
        assert!(slots > 0, "{file_name}");
    }
}

#[test]
fn two_draws_with_one_key_are_an_error_naming_both() {
    // The start of `mood` holds no key and takes one derived from the sheet and property ids. The start of `status`
    // stores that key, so both draw the same numbers, which the checker cannot see and the lowering refuses.
    let derived = (fnv1a64(b"game_of_life/mood") & 0xffff_ffff) as u32;
    let text = edited(life(), "key = 2929187291", &format!("key = {derived}"));
    let text = edited(
        &text,
        "[[parameter]]",
        "[[cells.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  { id = \"calm\", name = \
         \"Calm\", color = \"#202020\" },\n  { id = \"angry\", name = \"Angry\", color = \"#E04040\" },\n]\n\n\
         [cells.property.starts_as]\nrule = \"by_chance\"\notherwise = \"calm\"\n\n[[cells.property.starts_as.row]]\n\
         state = \"angry\"\nchance = 0.5\n\n[[parameter]]",
    );
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    let message = &errors[0].message;
    assert!(
        message.contains("property 'mood'") && message.contains("property 'status'"),
        "{message}"
    );
    let mood = text.find("[[cells.property]]\nid = \"mood\"").expect("the property");
    assert_eq!(
        errors[0].span.as_ref().map(|span| span.start),
        Some(mood),
        "{:?}",
        errors[0]
    );
}

#[test]
fn a_start_with_no_key_takes_one_from_the_sheet_and_property_ids() {
    let text = edited(life(), "key = 2929187291\n", "");
    let file = lowered(&text);
    assert_eq!(file.warnings().len(), 1, "{:?}", file.warnings());
    let derived = (fnv1a64(b"game_of_life/status") & 0xffff_ffff) as u32;
    let path = (fnv1a64(b"starts_as.row") & 0xffff_ffff) as u32;
    let key = crate::normal::compose_key(derived, path);
    assert!(
        file.lowered().to_toml().contains(&format!("config.key = {key}\n")),
        "{}",
        file.lowered().to_toml()
    );
}

#[test]
fn a_percent_lowers_to_its_number_divided_by_100() {
    let decimal = edited(life(), "chance = { param = \"initial_density\" }", "chance = 0.25");
    let percent = edited(
        life(),
        "chance = { param = \"initial_density\" }",
        "chance = { percent = 25 }",
    );
    assert_eq!(
        lowered(&decimal).lowered().to_toml(),
        lowered(&percent).lowered().to_toml()
    );
    let compared = edited(life(), "right = 3\n", "right = { percent = 300 }\n");
    assert_eq!(code(&compared), code(life()));
}

#[test]
fn a_measure_is_a_stat_named_after_it() {
    let file = lowered(life());
    let stats = &file.lowered().document().stats;
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].label, "Alive");
    assert_eq!(stats[0].from.port.as_deref(), Some("alive"));
}

/// Returns the code of `text`, which must lower.
fn code(text: &str) -> henad_core::provenance::ModelCode {
    lowered(text).code()
}

#[test]
fn renaming_a_result_keeps_the_code() {
    let renamed = edited(life(), "name = \"Alive neighbours\"", "name = \"Live neighbours\"");
    assert_eq!(code(&renamed), code(life()));
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
    assert_ne!(code(&renamed), code(life()));
}

#[test]
fn renaming_a_state_changes_the_code() {
    let renamed = edited(life(), "name = \"Alive\", color", "name = \"Living\", color");
    assert_ne!(code(&renamed), code(life()));
}

#[test]
fn renaming_the_model_changes_the_code() {
    let renamed = edited(life(), "name = \"Game of Life\"", "name = \"Life\"");
    assert_ne!(code(&renamed), code(life()));
}

#[test]
fn changing_a_default_changes_the_code() {
    let changed = edited(life(), "default = 0.3", "default = 0.4");
    assert_ne!(code(&changed), code(life()));
}

#[test]
fn changing_a_colour_changes_the_code() {
    let changed = edited(life(), "color = \"#151515\"", "color = \"#101010\"");
    assert_ne!(code(&changed), code(life()));
}

#[test]
fn drawing_new_numbers_changes_the_code() {
    let changed = edited(life(), "key = 2929187291", "key = 2929187292");
    assert_ne!(code(&changed), code(life()));
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

/// Parameter values of Game of Life at their defaults.
const LIFE_PARAMS: [ParamValue; 1] = [ParamValue::F32(0.3)];

/// Returns the alive cells of a Game of Life world `side` cells wide, as column and row.
fn alive(lanes: &HostLanes, side: u32) -> BTreeSet<(u32, u32)> {
    let status = lanes.column("status").expect("a status lane");
    (0..side * side)
        .filter(|&cell| status.get(cell as usize) == Some(HostValue::Category(1)))
        .map(|cell| (cell % side, cell / side))
        .collect()
}

/// Runs `ticks` ticks of the lowered Game of Life `file` on a world `side` cells wide from the alive cells `cells`, and
/// returns the alive cells after each tick, the planted ones first.
fn run_life(file: &SheetFile, side: u32, cells: &[(u32, u32)], ticks: u32) -> Vec<BTreeSet<(u32, u32)>> {
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    };
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let status = lanes.column_mut("status").expect("a status lane");
    for &(x, y) in cells {
        assert!(status.set((y * side + x) as usize, HostValue::Category(1)));
    }
    let mut states = vec![alive(&lanes, side)];
    let mut stream = 0;
    for tick in 0..ticks {
        evaluator
            .run_pass(
                "tick_cell_tick",
                &mut lanes,
                &LIFE_PARAMS,
                &RunContext::tick(Some(1), tick),
                &mut stream,
            )
            .expect("a tick runs");
        states.push(alive(&lanes, side));
    }
    states
}

#[test]
fn the_life_sheet_blinker_has_period_2() {
    let file = lowered(&life_of_side(32));
    let states = run_life(&file, 32, &[(15, 14), (15, 15), (15, 16)], 4);
    assert_eq!(states[1], BTreeSet::from([(14, 15), (15, 15), (16, 15)]));
    assert_eq!(states[2], states[0]);
    assert_eq!(states[3], states[1]);
    assert_eq!(states[4], states[0]);
}

#[test]
fn the_life_sheet_glider_moves_one_cell_diagonally_every_4_ticks() {
    let file = lowered(&life_of_side(32));
    let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let states = run_life(&file, 32, &glider, 8);
    for (period, shift) in [(4, 1), (8, 2)] {
        let moved: BTreeSet<(u32, u32)> = glider.iter().map(|&(x, y)| (x + shift, y + shift)).collect();
        assert_eq!(states[period], moved, "after {period} ticks");
    }
    assert!(states.iter().all(|state| state.len() == 5), "a glider keeps 5 cells");
}

/// Returns the lanes of the lowered Game of Life on a world `side` cells wide after its start at `seed`.
fn started(evaluator: &HostEvaluator, side: u32, seed: u64) -> HostLanes {
    let world = HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    };
    evaluator.init(world, &LIFE_PARAMS, Some(seed)).expect("the start runs")
}

#[test]
fn a_rebuild_at_one_seed_replays_the_same_start() {
    let file = lowered(&life_of_side(64));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let first = alive(&started(&evaluator, 64, 7), 64);
    let again = alive(&started(&evaluator, 64, 7), 64);
    let other = alive(&started(&evaluator, 64, 8), 64);
    assert_eq!(first, again);
    assert_ne!(first, other);
    // About 30% of 4096 cells start alive.
    assert!((1000..1500).contains(&first.len()), "{} alive", first.len());
}

#[test]
fn a_start_again_button_draws_a_new_layout_each_press() {
    let file = lowered(&life_of_side(64));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let press = |presses: u32| {
        let mut lanes = started(&evaluator, 64, 7);
        let mut stream = action_seed(Some(7));
        let mut layouts = Vec::new();
        for press in 1..=presses {
            evaluator
                .rerun("randomise", press, &mut lanes, &LIFE_PARAMS, Some(7), &mut stream)
                .expect("the press runs");
            layouts.push(alive(&lanes, 64));
        }
        layouts
    };
    let layouts = press(2);
    let start = alive(&started(&evaluator, 64, 7), 64);
    assert_ne!(layouts[0], start);
    assert_ne!(layouts[1], layouts[0]);
    assert_eq!(press(2), layouts, "the presses replay");
}

#[test]
fn a_skipped_line_and_its_dependents_are_left_out() {
    // Line 1 loses the state it counts, and lines 2 and 3 read its result.
    let text = edited(
        life(),
        "left = { their = \"status\" }\nright = { state = \"alive\" }\n",
        "left = { their = \"status\" }\n",
    );
    let file = lowered(&text);
    let skipped: Vec<&str> = ["l1", "l2", "l3"]
        .into_iter()
        .filter(|line| file.line_map().nodes_of(line).next().is_none())
        .collect();
    assert_eq!(skipped, ["l1", "l2", "l3"]);
    assert_eq!(file.warnings().len(), 3, "{:?}", file.warnings());
    let tick = &file.lowered().document().passes[1];
    assert_eq!(tick.stage, PassStage::Tick);
    assert!(tick.body.nodes.is_empty(), "{:?}", tick.body.nodes);
    assert!(GridProgram::compile(file.lowered()).is_ok());

    // Line 3 alone loses its numbers, and line 2 still runs.
    let text = edited(life(), "right = [2, 3]\n", "");
    let file = lowered(&text);
    assert!(file.line_map().nodes_of("l3").next().is_none());
    assert!(file.line_map().nodes_of("l2").next().is_some());
    let rules = file
        .lowered()
        .to_toml()
        .lines()
        .filter(|line| line.contains("phase = 0"))
        .count();
    assert_eq!(rules, 1 + 2, "line 2's row and the two rows of Clear");
}

/// Lines of the kinds the grid lowering refused before chances, looks, calculations and choices lowered, each as line
/// `l5` of Game of Life.
const LIFTED_LINES: [&str; 7] = [
    "[card.line.l5]\nkind = \"chance\"\nname = \"Spark\"\nkey = 11\nchance = 0.5",
    "[card.line.l5]\nkind = \"look_around\"\nname = \"Around\"\naround = \"cells_8\"\nsummaries = [\n  { id = \
     \"alive_around\", name = \"Alive around\", summary = \"count\" },\n]",
    "[card.line.l5]\nkind = \"calculate\"\nname = \"Double\"\nformula = \"l1 * 2\"",
    "[card.line.l5]\nkind = \"choose\"\nname = \"Pick\"\notherwise = 0\n\n[[card.line.l5.row]]\nvalue = 1\n\n\
     [card.line.l5.row.condition]\nkind = \"compare\"\ntest = \"is\"\nleft = { result = \"l1\" }\nright = 3",
    "[card.line.l5]\nkind = \"depending_on\"\nname = \"By status\"\nproperty = \"status\"\nbranches = { dead = 0, \
     alive = 1 }",
    "[card.line.l5]\nkind = \"random_number\"\nname = \"Noise\"\nkey = 13\nlow = 0\nhigh = 1",
    "[card.line.l5]\nkind = \"become\"\nkey = 14\nproperty = \"status\"\nnew_state = \"dead\"\nstart_state = \
     \"alive\"\n\n[card.line.l5.condition]\nkind = \"chance\"\nchance = 0.1",
];

#[test]
fn each_lifted_refusal_now_lowers_checks_and_compiles() {
    for table in LIFTED_LINES {
        let text = life_with_line(table);
        let file = lowered(&text);
        assert!(file.warnings().is_empty(), "{table}: {:?}", file.warnings());
        assert!(file.line_map().nodes_of("l5").next().is_some(), "{table}");
        let report = file.lowered().check();
        assert!(!report.has_errors(), "{table}: {:#?}", report.diagnostics);
        assert!(GridProgram::compile(file.lowered()).is_ok(), "{table}");
    }
}

#[test]
fn each_interim_refusal_gives_one_located_error() {
    let rate =
        life_with_line("[card.line.l5]\nkind = \"rate\"\nname = \"Spark\"\nkey = 12\nrate = 0.5\nrate_unit = \"day\"");
    let errors = refusals(&rate);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].message, sentence::NOT_YET_RATE);
    assert!(placed_at(&errors[0], &rate).starts_with("[card.line.l5]"));
    let repeat = edited(life(), "lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l1\", \"l5\"]");
    let repeat = edited(&repeat, "next_line = 5", "next_line = 6");
    let repeat = edited(
        &repeat,
        "# Become Alive from Dead",
        "[card.line.l5]\nkind = \"repeat\"\ntimes = 2\nlines = [\"l2\", \"l3\"]\n\n# Become Alive from Dead",
    );
    let errors = refusals(&repeat);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].message, sentence::NOT_YET_REPEAT);
}

#[test]
fn a_second_card_that_runs_each_tick_is_refused() {
    let text = edited(life(), "runs = \"once\"", "runs = \"each_tick\"");
    let text = edited(&text, "[[button]]\nid = \"clear\"", "[[button]]\nid = \"clear_button\"");
    let text = edited(
        &text,
        "rule = \"run_once\"\ncard = \"clear\"\n",
        "rule = \"start_again\"\nproperties = [\"status\"]\n",
    );
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("card 'clear'"), "{}", errors[0].message);
}

#[test]
fn a_grid_sheet_with_no_property_lowers_to_nothing() {
    let text = "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\nid = \
                \"blank\"\nname = \"Untitled model\"\nnext_line = 1\n\n[world]\nmade_of = \"cells\"\nwidth = 100\n\
                height = 100\nedges = \"wrap\"\n";
    let file = lowered(text);
    assert_eq!(
        file.warnings()
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>(),
        [sentence::SHEET_EMPTY]
    );
    let document = file.lowered().document();
    assert!(document.lanes.is_empty() && document.params.is_empty() && document.passes.is_empty());
    assert!(file.line_map().is_empty());
    // A graph with no lane cannot run, so its build reports the sheet's emptiness as the graph's.
    assert!(file.lowered().check().has_errors());
}

#[test]
fn the_largest_grid_sheet_lowers_under_half_the_node_limit() {
    let text = largest_grid_sheet();
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let lines: usize = file.document().cards[0].line_tables.len();
    assert_eq!(lines, MAX_LINES_PER_CARD);
    let nodes = file.line_map().len();
    assert!(nodes < MAX_NODES_IN_FILE / 2, "{nodes} nodes");
    let report = file.lowered().check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
}

/// Returns a grid sheet whose card holds [`MAX_LINES_PER_CARD`] lines of the kinds Game of Life uses, over four States
/// properties: a Count of each property's alive neighbours every four lines, each followed by Become lines reading
/// it, at most 48 Becomes a property.
fn largest_grid_sheet() -> String {
    let mut text = String::from(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\nid = \"largest\"\nname \
         = \"Largest\"\n",
    );
    text.push_str(&format!("next_line = {}\n\n", MAX_LINES_PER_CARD + 1));
    text.push_str("[world]\nmade_of = \"cells\"\nwidth = 64\nheight = 64\nedges = \"wrap\"\n\n");
    for property in 0..4 {
        text.push_str(&format!(
            "[[cells.property]]\nid = \"p{property}\"\nname = \"P{property}\"\ntype = \"states\"\nstates = [\n  {{ id = \
             \"dead\", name = \"Dead\", color = \"#151515\" }},\n  {{ id = \"alive\", name = \"Alive\", color = \
             \"#00E676\" }},\n]\n\n[cells.property.starts_as]\nrule = \"value\"\nvalue = {{ state = \"dead\" }}\n\n"
        ));
    }
    let ids: Vec<String> = (1..=MAX_LINES_PER_CARD).map(|k| format!("\"l{k}\"")).collect();
    text.push_str(&format!(
        "[[card]]\nid = \"cell_tick\"\nevery = \"cells\"\nruns = \"each_tick\"\nlines = [{}]\n\n",
        ids.join(", ")
    ));
    let mut count = 0;
    for k in 1..=MAX_LINES_PER_CARD {
        let property = (k - 1) / 4 % 4;
        if (k - 1) % 4 == 0 {
            count = k;
            text.push_str(&format!(
                "[card.line.l{k}]\nkind = \"count\"\nname = \"Around {k}\"\naround = \"cells_8\"\n\n[card.line.l{k}.whose]\n\
                 kind = \"compare\"\ntest = \"is\"\nleft = {{ their = \"p{property}\" }}\nright = {{ state = \"alive\" }}\n\n"
            ));
        } else {
            let (new_state, start_state) = if k % 2 == 0 {
                ("alive", "dead")
            } else {
                ("dead", "alive")
            };
            text.push_str(&format!(
                "[card.line.l{k}]\nkind = \"become\"\nproperty = \"p{property}\"\nnew_state = \"{new_state}\"\n\
                 start_state = \"{start_state}\"\n\n[card.line.l{k}.condition]\nkind = \"compare\"\ntest = \
                 \"not_one_of\"\nleft = {{ result = \"l{count}\" }}\nright = [2, 3]\n\n"
            ));
        }
    }
    text
}

/// A grid sheet over the line kinds of this lowering: Counts by `grid.count` and by a fold, an all of holding a none
/// of, two Sets of one property, the outside test, a Set inside a When on an any of, and Becomes with one of, within
/// and `my_new`.
fn kinds() -> String {
    include_str!("sheets/kinds.toml").to_owned()
}

#[test]
fn the_kinds_sheet_runs_its_lines_on_the_host() {
    let parsed = crate::sheet::ParsedSheet::parse(&kinds()).expect("the kinds sheet reads");
    assert_eq!(
        parsed.document().to_text().as_deref(),
        Ok(kinds().as_str()),
        "the kinds sheet is canonical"
    );
    let file = lowered(&kinds());
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let report = file.lowered().check();
    assert!(
        !report.has_errors(),
        "{:#?}\n{}",
        report.diagnostics,
        file.lowered().to_toml()
    );
    assert!(GridProgram::compile(file.lowered()).is_ok());
    let tick = &file.lowered().document().passes[1];
    assert!(tick.body.nodes.iter().any(|node| node.op == "grid.count"));
    assert!(tick.body.nodes.iter().any(|node| node.op == "grid.fold"));

    let side = 8u32;
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    };
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let cells = (side * side) as usize;
    let mut rng = 0x6b69_6e64_7300_0001u64;
    let mut status = vec![false; cells];
    let mut flag = vec![false; cells];
    for cell in 0..cells {
        rng = henad_core::authoring::primitives::rng::xorshift64(rng);
        status[cell] = rng.is_multiple_of(3);
        flag[cell] = (rng >> 8).is_multiple_of(2);
        assert!(
            lanes
                .column_mut("status")
                .expect("status")
                .set(cell, HostValue::Category(u32::from(status[cell])))
        );
        assert!(
            lanes
                .column_mut("flag")
                .expect("flag")
                .set(cell, HostValue::Bool(flag[cell]))
        );
        assert!(lanes.column_mut("level").expect("level").set(cell, HostValue::F32(7.0)));
    }
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(1), 0),
            &mut stream,
        )
        .expect("a tick runs");

    let at = |x: i64, y: i64| ((y.rem_euclid(side as i64)) * side as i64 + x.rem_euclid(side as i64)) as usize;
    for cell in 0..cells {
        let (x, y) = ((cell as u32 % side) as i64, (cell as u32 / side) as i64);
        let mut alive_around = 0u32;
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            alive_around += u32::from(status[at(x + dx, y + dy)]);
        }
        let mut flagged_around = 0u32;
        for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
            flagged_around += u32::from(flag[at(x + dx, y + dy)]);
        }
        let crowded = !(1..=3).contains(&alive_around);
        let level = if crowded { 5.0 } else { alive_around as f32 };
        let lone_flag = flagged_around >= 1 && !flag[cell];
        let new_flag = status[cell] || lone_flag || flag[cell];
        let new_status = if status[cell] {
            alive_around > 1
        } else {
            alive_around == 3 || level >= 5.0
        };
        assert_eq!(
            lanes.column("level").and_then(|lane| lane.get(cell)),
            Some(HostValue::F32(level)),
            "level at {cell}"
        );
        assert_eq!(
            lanes.column("flag").and_then(|lane| lane.get(cell)),
            Some(HostValue::Bool(new_flag)),
            "flag at {cell}"
        );
        assert_eq!(
            lanes.column("status").and_then(|lane| lane.get(cell)),
            Some(HostValue::Category(u32::from(new_status))),
            "status at {cell}"
        );
    }
}

#[test]
fn the_sheet_lowering_never_panics_on_the_mutation_corpus() {
    let mut rng = 0x6c6f_7765_7200_0007u64;
    let (mut cases, mut lowered_cases) = (0usize, 0usize);
    let texts: Vec<String> = sheets()
        .into_iter()
        .map(|(_, text)| text.to_owned())
        .chain([kinds()])
        .collect();
    for text in &texts {
        // Edits of a sheet that does not lower count toward the corpus, and not toward the share that lowers.
        let lowers = SheetFile::parse(text).is_ok();
        for edit in 0..200 {
            let mut edited = mutate(text, &mut rng);
            if edit % 3 == 0 {
                edited = mutate(&edited, &mut rng);
            }
            let outcome = exercise(&edited);
            if lowers {
                cases += 1;
                lowered_cases += usize::from(outcome);
            }
        }
        for (index, line) in text.lines().enumerate() {
            if line.contains(" = ") {
                let edited: Vec<&str> = text
                    .lines()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, line)| line)
                    .collect();
                let outcome = exercise(&edited.join("\n"));
                if lowers {
                    cases += 1;
                    lowered_cases += usize::from(outcome);
                }
            }
        }
    }
    assert!(
        lowered_cases * 5 > cases,
        "only {lowered_cases} of {cases} edits of the sheets that lower still lowered"
    );
}

/// Reads, checks and lowers `text`, and returns whether it lowered. A panic fails the test with the text.
fn exercise(text: &str) -> bool {
    let outcome = catch_unwind(AssertUnwindSafe(|| match SheetFile::parse(text) {
        Ok(file) => {
            let document = file.lowered().document();
            let nodes: usize = document.passes.iter().map(|pass| pass.body.nodes.len()).sum();
            assert_eq!(nodes, file.line_map().len());
            true
        }
        Err(error) => {
            assert!(!error.diagnostics().is_empty());
            false
        }
    }));
    outcome.unwrap_or_else(|_| panic!("the lowering panicked on:\n{text}"))
}

/// Two cells properties for a Game of Life sheet: `level`, a Number starting at a random number between 0 and 1, and
/// `mood`, a States property starting calm.
const EXTRA_PROPERTIES: &str = "[[cells.property]]\nid = \"level\"\nname = \"Level\"\ntype = \"number\"\n\n\
                                [cells.property.starts_as]\nrule = \"value\"\nkey = 77\n\n\
                                [cells.property.starts_as.value]\nkind = \"random_number\"\nlow = 0\nhigh = 1\n\n\
                                [[cells.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  \
                                { id = \"calm\", name = \"Calm\", color = \"#2060E0\" },\n  { id = \"excited\", name = \
                                \"Excited\", color = \"#E06020\" },\n]\n\n[cells.property.starts_as]\nrule = \"value\"\n\
                                value = { state = \"calm\" }\n\n";

/// Returns `text`, a Game of Life sheet, with [`EXTRA_PROPERTIES`] after its `status` property and `parts` before its
/// first card. `parts` holds measures, colours or buttons.
fn life_with(text: &str, parts: &str) -> String {
    let text = edited(text, "[[parameter]]", &format!("{EXTRA_PROPERTIES}[[parameter]]"));
    edited(
        &text,
        "# Each tick, every cell\n",
        &format!("{parts}# Each tick, every cell\n"),
    )
}

/// Returns the value of the `level` lane at every cell.
fn levels(lanes: &HostLanes) -> Vec<f32> {
    let column = lanes.column("level").expect("a level lane");
    (0..column.len())
        .map(|cell| match column.get(cell) {
            Some(HostValue::F32(value)) => value,
            other => panic!("level at {cell} is {other:?}"),
        })
        .collect()
}

#[test]
fn a_set_everywhere_button_writes_its_value_on_every_cell() {
    let wipe = "[[button]]\nid = \"wipe\"\nname = \"Wipe\"\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                \"status\"\nvalue = { state = \"dead\" }\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                \"level\"\nvalue = 0.5\n\n";
    let file = lowered(&life_with(&life_of_side(16), wipe));
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let pass = document
        .passes
        .iter()
        .find(|pass| pass.id == "button_wipe")
        .expect("the action pass");
    assert_eq!(pass.stage, PassStage::Action);
    assert_eq!(pass.action.as_deref(), Some("wipe"));
    let action = document
        .actions
        .iter()
        .position(|action| action.id == "wipe")
        .expect("the action");
    assert!(document.actions[action].runs.is_empty());

    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let mut lanes = started(&evaluator, 16, 7);
    assert!(!alive(&lanes, 16).is_empty(), "the start leaves some cells alive");
    let mut stream = action_seed(Some(7));
    evaluator
        .run_pass(
            "button_wipe",
            &mut lanes,
            &LIFE_PARAMS,
            &RunContext::action(Some(7), action, 1, 0),
            &mut stream,
        )
        .expect("the press runs");
    assert!(alive(&lanes, 16).is_empty());
    assert!(levels(&lanes).iter().all(|&level| level == 0.5));
}

#[test]
fn a_property_set_twice_by_one_button_is_refused() {
    let twice = "[[button]]\nid = \"wipe\"\nname = \"Wipe\"\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                 \"status\"\nvalue = { state = \"dead\" }\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                 \"status\"\nvalue = { state = \"alive\" }\n\n";
    let text = life_with(life(), twice);
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].message.contains("'status' is changed twice by one button"),
        "{}",
        errors[0].message
    );
    assert_eq!(placed_at(&errors[0], &text), "[[button]]");
    assert_eq!(
        errors[0].span.as_ref().map(|span| span.start),
        text.find("[[button]]\nid = \"wipe\""),
        "the error sits at the button"
    );
}

#[test]
fn a_start_again_button_on_some_properties_reruns_only_their_starts() {
    let file = lowered(&life_with(&life_of_side(64), ""));
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let randomise = document
        .actions
        .iter()
        .find(|action| action.id == "randomise")
        .expect("the action");
    assert_eq!(randomise.runs, ["restart_randomise"]);
    let restart = document
        .passes
        .iter()
        .find(|pass| pass.id == "restart_randomise")
        .expect("the restart pass");
    assert_eq!(restart.stage, PassStage::Init);
    assert_eq!(restart.enabled, crate::document::PassEnabled::Literal(false));
    let paths: BTreeSet<&str> = restart
        .body
        .nodes
        .iter()
        .filter_map(|node| file.line_map().origin(node.id))
        .map(|origin| origin.path.as_str())
        .collect();
    assert_eq!(paths, BTreeSet::from(["cells.status.starts_as"]));

    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let press = |presses: u32| {
        let mut lanes = started(&evaluator, 64, 7);
        let mut stream = action_seed(Some(7));
        let mut states = Vec::new();
        for press in 1..=presses {
            evaluator
                .rerun("randomise", press, &mut lanes, &LIFE_PARAMS, Some(7), &mut stream)
                .expect("the press runs");
            states.push((alive(&lanes, 64), levels(&lanes)));
        }
        states
    };
    let start = started(&evaluator, 64, 7);
    let (start_alive, start_levels) = (alive(&start, 64), levels(&start));
    assert!(
        start_levels.iter().any(|&level| level != start_levels[0]),
        "the levels start at random"
    );
    let states = press(2);
    assert_ne!(states[0].0, start_alive);
    assert_ne!(states[1].0, states[0].0);
    assert!(
        states.iter().all(|(_, levels)| *levels == start_levels),
        "the levels stay"
    );
    assert_eq!(press(2), states, "the presses replay");
}

#[test]
fn a_random_number_start_draws_between_its_values() {
    let file = lowered(&life_with(&life_of_side(64), ""));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let levels = levels(&started(&evaluator, 64, 7));
    assert!(levels.iter().all(|level| (0.0..1.0).contains(level)), "{levels:?}");
    let mean = levels.iter().map(|&level| f64::from(level)).sum::<f64>() / levels.len() as f64;
    assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
}

#[test]
fn a_random_number_start_of_a_whole_number_is_refused() {
    let text = life_with(life(), "");
    let text = edited(
        &text,
        "name = \"Level\"\ntype = \"number\"",
        "name = \"Level\"\ntype = \"whole_number\"",
    );
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].message.contains("starts a Number property only"),
        "{}",
        errors[0].message
    );
    assert_eq!(placed_at(&errors[0], &text), "[[cells.property]]");
    assert_eq!(
        errors[0].span.as_ref().map(|span| span.start),
        text.find("[[cells.property]]\nid = \"level\""),
        "the error sits at the property"
    );
}

/// Returns `value` as `f64`, for a measure's value.
fn number(value: Option<HostValue>) -> f64 {
    match value {
        Some(HostValue::F64(value)) => value,
        Some(HostValue::F32(value)) => f64::from(value),
        other => panic!("{other:?} is no number"),
    }
}

#[test]
fn the_average_total_and_spread_measures_match_a_reference() {
    let measures = "[[measure]]\nid = \"mean_level\"\nname = \"Mean level\"\ncolor = \"#2060E0\"\nrule = \
                    \"average\"\nover = \"cells\"\nvalue = { my = \"level\" }\n\n[[measure]]\nid = \"total_level\"\n\
                    name = \"Total level\"\ncolor = \"#E06020\"\nrule = \"total\"\nover = \"cells\"\nvalue = { my = \
                    \"level\" }\n\n[[measure]]\nid = \"spread_level\"\nname = \"Spread of level\"\ncolor = \
                    \"#20A060\"\nrule = \"spread\"\nover = \"cells\"\nvalue = { my = \"level\" }\n\n";
    let text = edited(
        &life_with(&life_of_side(8), ""),
        "[color.cells]",
        &format!("{measures}[color.cells]"),
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let labels: Vec<&str> = file
        .lowered()
        .document()
        .stats
        .iter()
        .map(|stat| stat.label.as_str())
        .collect();
    assert_eq!(labels, ["Alive", "Mean level", "Total level", "Spread of level"]);

    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: 8,
        height: 8,
        boundary: Boundary::Torus,
    };
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let planted: Vec<f32> = (0..64u16).map(|cell| f32::from(cell % 7) * 0.25 - 0.5).collect();
    for (cell, &level) in planted.iter().enumerate() {
        assert!(
            lanes
                .column_mut("level")
                .expect("level")
                .set(cell, HostValue::F32(level))
        );
    }
    let mut stream = 0;
    let outcome = evaluator
        .run_pass(
            "measures",
            &mut lanes,
            &LIFE_PARAMS,
            &RunContext::tick(Some(1), 0),
            &mut stream,
        )
        .expect("the measures run");
    let values: Vec<f64> = evaluator.document().stats[1..]
        .iter()
        .map(|stat| number(outcome.value(&stat.from)))
        .collect();

    let total: f64 = planted.iter().map(|&level| f64::from(level)).sum();
    let mean = total / 64.0;
    let mean_square = planted.iter().map(|&level| f64::from(level * level)).sum::<f64>() / 64.0;
    let spread = (mean_square - mean * mean).max(0.0).sqrt();
    for (found, expected) in values.iter().zip([mean, total, spread]) {
        assert!(
            (found - expected).abs() < 1e-9,
            "{values:?} against {mean}, {total}, {spread}"
        );
    }
    assert!(spread > 0.1, "the planted levels spread");
}

#[test]
fn colouring_by_a_second_states_property_writes_a_view_pass() {
    let text = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"by_state\"\nproperty = \"mood\"",
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let view = document
        .passes
        .iter()
        .find(|pass| pass.id == "colours")
        .expect("the view pass");
    assert_eq!(view.stage, PassStage::View);
    let color = view
        .body
        .nodes
        .iter()
        .find(|node| node.op == "view.color")
        .expect("a view.color");
    assert_eq!(
        color.config["palette"],
        crate::value::ConfigValue::String("mood".to_owned())
    );
    assert!(view.body.nodes.iter().all(|node| node.op != "view.ramp"));
    assert!(GridProgram::compile(file.lowered()).is_ok());

    // The first States property takes its colours without a view pass.
    let first = lowered(&life_with(life(), ""));
    assert!(
        first
            .lowered()
            .document()
            .passes
            .iter()
            .all(|pass| pass.id != "colours")
    );
}

#[test]
fn a_shaded_number_ramps_into_its_palette() {
    let text = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"shaded\"\nproperty = \"level\"",
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let palette = document
        .palettes
        .iter()
        .find(|palette| palette.id == "shaded_level")
        .expect("the palette");
    assert_eq!(palette.colors.len(), 8);
    let view = document
        .passes
        .iter()
        .find(|pass| pass.id == "colours")
        .expect("the view pass");
    let ramp = view
        .body
        .nodes
        .iter()
        .find(|node| node.op == "view.ramp")
        .expect("a view.ramp");
    assert_eq!(
        ramp.config["palette"],
        crate::value::ConfigValue::String("shaded_level".to_owned())
    );
    assert!(view.body.nodes.iter().any(|node| node.op == "view.color"));
    let report = file.lowered().check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    assert!(GridProgram::compile(file.lowered()).is_ok());
}

#[test]
fn a_shading_on_a_log_scale_is_refused() {
    let shaded = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"shaded\"\nproperty = \"level\"",
    );
    for keys in ["scale = \"log\"\ndecades = 3", "scale = \"log\"", "decades = 3"] {
        let text = edited(
            &shaded,
            "property = \"level\"\n",
            &format!("property = \"level\"\n{keys}\n"),
        );
        let errors = refusals(&text);
        assert_eq!(errors.len(), 1, "{keys}: {errors:?}");
        assert!(errors[0].message.contains("log scale"), "{}", errors[0].message);
        assert!(placed_at(&errors[0], &text).starts_with("[color.cells]"), "{keys}");
    }
    let linear = edited(
        &shaded,
        "property = \"level\"\n",
        "property = \"level\"\nscale = \"linear\"\n",
    );
    assert!(SheetFile::parse(&linear).is_ok());
}

#[test]
fn a_line_past_the_node_limit_of_a_line_is_refused_naming_the_limit() {
    let numbers: Vec<String> = (0..600).map(|number| number.to_string()).collect();
    let table = format!(
        "[card.line.l5]\nkind = \"compare\"\nname = \"Listed\"\ntest = \"one_of\"\nleft = {{ result = \"l1\" }}\n\
         right = [{}]",
        numbers.join(", ")
    );
    let text = life_with_line(&table);
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0]
            .message
            .contains(&format!("more than {} nodes", crate::sheet::limits::MAX_NODES_PER_LINE)),
        "{}",
        errors[0].message
    );
    assert!(placed_at(&errors[0], &text).starts_with("[card.line.l5]"));
}

#[test]
fn a_sheet_past_the_node_limit_of_the_sheet_is_refused_naming_the_limit() {
    let measures: String = (0..150)
        .map(|index| {
            format!(
                "[[measure]]\nid = \"spread_{index}\"\nname = \"Spread {index}\"\ncolor = \"#20A060\"\nrule = \
                 \"spread\"\nover = \"cells\"\nvalue = {{ my = \"level\" }}\n\n"
            )
        })
        .collect();
    let text = edited(
        &life_with(life(), ""),
        "[color.cells]",
        &format!("{measures}[color.cells]"),
    );
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0]
            .message
            .contains(&format!("more than {} nodes", crate::sheet::limits::MAX_SHEET_NODES)),
        "{}",
        errors[0].message
    );
}

#[test]
fn a_sheet_error_list_holds_its_first_error_first() {
    use henad_core::diagnostic::CodeDiagnostic;

    let errors = crate::sheet::SheetErrors::new(CodeDiagnostic::error("first"), [CodeDiagnostic::error("second")]);
    let error = SheetFileError::Invalid(errors);
    assert_eq!(error.diagnostic().message, "first");
    assert_eq!(error.diagnostics().len(), 2);
    assert_eq!(error.to_string().lines().count(), 2);
}

/// Returns Game of Life lowered with the node `id` of the tick pass made a node of an op the catalogue does not hold,
/// added to that pass when the lowering wrote no node `id`.
fn life_with_unknown_node(id: crate::ids::NodeId) -> SheetFile {
    SheetFile::parse_planted(life(), |document| {
        let pass = document
            .passes
            .iter_mut()
            .find(|pass| pass.stage == PassStage::Tick)
            .expect("Game of Life runs a tick pass");
        match pass.body.nodes.iter_mut().find(|node| node.id == id) {
            Some(node) => node.op = "grid.no_such_op".to_owned(),
            None => pass.body.nodes.push(crate::node::Node::new(id, "grid.no_such_op")),
        }
    })
    .unwrap_or_else(|error| panic!("the planted sheet does not lower: {error}"))
}

/// Returns the error that the graph checker reports on node `id` of `file`'s lowered graph.
fn graph_error_on(file: &SheetFile, id: crate::ids::NodeId) -> henad_core::diagnostic::CodeDiagnostic {
    let report = file.lowered().check();
    let node = id.to_string();
    report
        .errors()
        .find(|error| error.node.as_deref() == Some(node.as_str()))
        .cloned()
        .unwrap_or_else(|| panic!("no error on {node}: {:?}", report.errors().collect::<Vec<_>>()))
}

/// Returns the line of `text` that holds the first `needle`, counted from 1.
fn line_of(text: &str, needle: &str) -> u32 {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("the text holds no {needle:?}"));
    u32::try_from(text[..at].matches('\n').count() + 1).expect("a short text")
}

#[test]
fn a_graph_diagnostic_is_placed_at_its_author_line() {
    let id = lowered(life())
        .line_map()
        .nodes_of("l2")
        .next()
        .expect("line l2 owns a node");
    let file = life_with_unknown_node(id);
    let error = graph_error_on(&file, id);
    assert!(error.message.contains("grid.no_such_op"), "{error}");
    assert_eq!(error.pass.as_deref(), Some("tick_cell_tick"), "{error:?}");

    let placed = file.place_graph_diagnostic(error.clone());
    assert_eq!(
        (placed.pass.as_deref(), placed.node.as_deref()),
        (None, None),
        "{placed:?}"
    );
    assert_eq!(placed.message, error.message);
    assert!(placed_at(&placed, life()).starts_with("[card.line.l2]"), "{placed}");
    assert_eq!(placed.line, Some(line_of(life(), "[card.line.l2]")), "{placed}");

    // A node the sheet owns is placed at the part of the sheet its slot path names, here the count of each state that
    // the measure reads.
    let count = file
        .line_map()
        .iter()
        .find(|(_, origin)| origin.path == "cells.status.count")
        .map(|(id, _)| id)
        .expect("the count of the states is a node");
    let placed =
        file.place_graph_diagnostic(henad_core::diagnostic::CodeDiagnostic::error("a").with_node(count.to_string()));
    assert!(placed_at(&placed, life()).starts_with("[[cells.property]]"), "{placed}");

    // A finding with a pass and no node is placed at the card the pass runs.
    let placed =
        file.place_graph_diagnostic(henad_core::diagnostic::CodeDiagnostic::error("b").with_pass("tick_cell_tick"));
    assert_eq!(placed.pass, None);
    assert_eq!(
        placed.line,
        Some(line_of(life(), "[[card]]\nid = \"cell_tick\"")),
        "{placed}"
    );
    let placed =
        file.place_graph_diagnostic(henad_core::diagnostic::CodeDiagnostic::error("c").with_pass("restart_randomise"));
    assert_eq!(
        placed.line,
        Some(line_of(life(), "[[button]]\nid = \"randomise\"")),
        "{placed}"
    );
}

#[test]
fn an_unknown_node_is_placed_on_the_sheet() {
    let id = crate::ids::NodeId::new(900_000).expect("a node id");
    let file = life_with_unknown_node(id);
    assert!(file.line_map().origin(id).is_none());
    let placed = file.place_graph_diagnostic(graph_error_on(&file, id));
    assert_eq!(
        (placed.pass.as_deref(), placed.node.as_deref()),
        (None, None),
        "{placed:?}"
    );
    assert!(placed_at(&placed, life()).starts_with("[sheet]"), "{placed}");
    assert_eq!(placed.line, Some(line_of(life(), "[sheet]")));

    // So is a finding with a pass the sheet lowers from no card or button, and one with neither pass nor node.
    for diagnostic in [
        henad_core::diagnostic::CodeDiagnostic::error("a").with_pass("measures"),
        henad_core::diagnostic::CodeDiagnostic::warning("b"),
    ] {
        let placed = file.place_graph_diagnostic(diagnostic);
        assert_eq!(placed.line, Some(line_of(life(), "[sheet]")), "{placed}");
    }
}

/// Returns the number of cells of the `health` lane of `lanes` in each state of the SIR epidemic.
fn health_counts(lanes: &HostLanes) -> [usize; 3] {
    let health = lanes.column("health").expect("a health lane");
    let mut counts = [0; 3];
    for cell in 0..health.len() {
        match health.get(cell) {
            Some(HostValue::Category(state)) if state < 3 => counts[state as usize] += 1,
            other => panic!("cell {cell} holds {other:?}"),
        }
    }
    counts
}

#[test]
fn the_chance_sir_sheet_conserves_its_cells_every_tick() {
    let text = edited(sheet_text("sir_chance.toml"), "width = 1024", "width = 64");
    let text = edited(&text, "height = 1024", "height = 64");
    let file = lowered(&text);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: 64,
        height: 64,
        boundary: Boundary::Torus,
    };
    let mut lanes = evaluator.init(world, &[], Some(5)).expect("the start runs");
    let start = health_counts(&lanes);
    assert!(start[1] > 0, "the start infects some cells: {start:?}");
    // New outbreak runs its card once, and its chance infects more susceptible cells.
    let mut stream = 0;
    evaluator
        .run_pass(
            "button_new_outbreak",
            &mut lanes,
            &[],
            &RunContext::action(Some(5), 0, 1, 0),
            &mut stream,
        )
        .expect("the press runs");
    let pressed = health_counts(&lanes);
    assert_eq!(pressed.iter().sum::<usize>(), 64 * 64);
    assert!(pressed[1] > start[1], "{start:?} then {pressed:?}");
    let start = pressed;
    let mut peak = 0;
    for tick in 0..200 {
        evaluator
            .run_pass(
                "tick_cell_tick",
                &mut lanes,
                &[],
                &RunContext::tick(Some(5), tick),
                &mut stream,
            )
            .expect("a tick runs");
        let counts = health_counts(&lanes);
        assert_eq!(counts.iter().sum::<usize>(), 64 * 64, "tick {tick}: {counts:?}");
        peak = peak.max(counts[1]);
    }
    let end = health_counts(&lanes);
    assert!(
        peak > start[1] && end[2] > 0,
        "the epidemic spreads and recovers: {peak}, {end:?}"
    );
}

/// A grid sheet `side` cells wide and high whose only line sets Hit to yes with chance `chance` once for each of the
/// cells `around` whose Flag is yes. `chance` is a value as a sheet writes it, and a result `l2` is a Calculate of
/// 0.1 times 2.
fn power_sheet(side: u32, around: &str, chance: &str) -> String {
    format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\nid = \"power\"\nname = \
         \"Power\"\nnext_line = 4\n\n[world]\nmade_of = \"cells\"\nwidth = {side}\nheight = {side}\nedges = \"wrap\"\n\n\
         [[cells.property]]\nid = \"flag\"\nname = \"Flag\"\ntype = \"yes_no\"\n\n[cells.property.starts_as]\nrule = \
         \"value\"\nvalue = false\n\n[[cells.property]]\nid = \"hit\"\nname = \"Hit\"\ntype = \"yes_no\"\n\n\
         [cells.property.starts_as]\nrule = \"value\"\nvalue = false\n\n[[card]]\nid = \"cell_tick\"\nevery = \
         \"cells\"\nruns = \"each_tick\"\nlines = [\"l1\", \"l2\", \"l3\"]\n\n[card.line.l1]\nkind = \"count\"\nname = \
         \"Flagged around\"\naround = \"{around}\"\n\n[card.line.l1.whose]\nkind = \"compare\"\ntest = \"is\"\nleft = \
         {{ their = \"flag\" }}\nright = true\n\n[card.line.l2]\nkind = \"calculate\"\nname = \"Base chance\"\nformula = \
         \"0.1 * 2\"\n\n[card.line.l3]\nkind = \"set\"\nkey = 21\nproperty = \"hit\"\nvalue = true\n\n\
         [card.line.l3.condition]\nkind = \"chance\"\nchance = {chance}\ntries = {{ result = \"l1\" }}\n"
    )
}

/// Runs one tick of the lowered power sheet `file`, `side` cells wide, from the flags `flagged` places, and returns the
/// share of the cells `counted` keeps that Hit holds yes on.
fn hit_share(
    file: &SheetFile,
    side: u32,
    flagged: impl Fn(u32, u32) -> bool,
    counted: impl Fn(u32, u32) -> bool,
) -> f64 {
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    };
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    for cell in 0..side * side {
        let flag = flagged(cell % side, cell / side);
        assert!(
            lanes
                .column_mut("flag")
                .expect("flag")
                .set(cell as usize, HostValue::Bool(flag))
        );
    }
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(9), 0),
            &mut stream,
        )
        .expect("a tick runs");
    let hit = lanes.column("hit").expect("a hit lane");
    let (mut cells, mut hits) = (0u32, 0u32);
    for cell in 0..side * side {
        if counted(cell % side, cell / side) {
            cells += 1;
            hits += u32::from(hit.get(cell as usize) == Some(HostValue::Bool(true)));
        }
    }
    f64::from(hits) / f64::from(cells)
}

#[test]
fn once_for_each_follows_the_power_rule() {
    let side = 300;
    let everywhere = |_: u32, _: u32| true;
    for chance in ["0.2", "{ result = \"l2\" }"] {
        let power = if chance == "0.2" { "math.powi" } else { "math.pow" };
        for (around, neighbours) in [("cells_8", 8), ("cells_4", 4), ("block_3x3", 9)] {
            let file = lowered(&power_sheet(side, around, chance));
            assert!(file.warnings().is_empty(), "{:?}", file.warnings());
            let tick = &file.lowered().document().passes[1];
            let node = tick
                .body
                .nodes
                .iter()
                .find(|node| node.op == power)
                .unwrap_or_else(|| panic!("{around}, {chance}: no {power}"));
            if power == "math.powi" {
                assert_eq!(
                    node.config.get("max_exponent"),
                    Some(&crate::value::ConfigValue::Integer(neighbours)),
                    "{around}"
                );
            }
            // Every cell flagged: each cell counts all its neighbours, the bound of the power.
            let expected = 1.0 - 0.8f64.powi(neighbours as i32);
            let share = hit_share(&file, side, everywhere, everywhere);
            assert!(
                (share - expected).abs() < 0.006,
                "{around}, {chance}: {share} for {expected}"
            );
            // No cell flagged: no try, so no cell is hit.
            assert_eq!(
                hit_share(&file, side, |_, _| false, everywhere),
                0.0,
                "{around}, {chance}"
            );
        }
        // Every third row flagged: a cell of another row counts the 3 cells of the flagged row beside it.
        let file = lowered(&power_sheet(side, "cells_8", chance));
        let share = hit_share(&file, side, |_, y| y % 3 == 0, |_, y| y % 3 != 0);
        assert!((share - 0.488).abs() < 0.01, "{chance}: {share} for 0.488");
    }
}

#[test]
fn the_forest_fire_lowers_its_spread_line_before_its_lightning_line() {
    let file = lowered(sheet_text("forest_fire.toml"));
    let tick = &file.lowered().document().passes[1];
    let transition = tick
        .body
        .nodes
        .iter()
        .find(|node| node.op == "cat.transition")
        .expect("the cover's transition");
    let Some(crate::value::ConfigValue::Array(rules)) = transition.config.get("rules") else {
        panic!("the transition holds no rules");
    };
    let ids: Vec<&str> = rules
        .iter()
        .filter_map(|rule| match rule {
            crate::value::ConfigValue::Table(table) => table.get("id").and_then(crate::value::ConfigValue::as_str),
            _ => None,
        })
        .collect();
    let place = |id: &str| {
        ids.iter()
            .position(|rule| *rule == id)
            .unwrap_or_else(|| panic!("no rule {id}"))
    };
    assert!(place("l2") < place("l3"), "{ids:?}");
    // Both chances lower to a draw keyed by their own line.
    let draws = tick.body.nodes.iter().filter(|node| node.op == "rand.chance").count();
    assert_eq!(draws, 2);
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

#[test]
fn a_chance_inside_a_when_draws_with_its_lines_composed_key() {
    let text = edited(life(), "next_line = 5", "next_line = 7");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
    );
    let text = edited(
        &text,
        "# Lines of Clear: run once, every cell, when pressed",
        "[card.line.l5]\nkind = \"when\"\nkey = 31\nlines = [\"l6\"]\n\n[card.line.l5.condition]\nkind = \
         \"chance\"\nchance = 0.5\n\n[card.line.l6]\nkind = \"become\"\nkey = 32\nproperty = \"status\"\nnew_state = \
         \"dead\"\nstart_state = \"alive\"\n\n[card.line.l6.condition]\nkind = \"chance\"\nchance = 0.25\n\n\
         # Lines of Clear: run once, every cell, when pressed",
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let path = (fnv1a64(b"condition") & 0xffff_ffff) as u32;
    let mut keys: Vec<i64> = file.lowered().document().passes[1]
        .body
        .nodes
        .iter()
        .filter(|node| node.op == "rand.chance")
        .filter_map(|node| match node.config.get("key") {
            Some(crate::value::ConfigValue::Integer(key)) => Some(*key),
            _ => None,
        })
        .collect();
    keys.sort_unstable();
    let mut expected = vec![
        i64::from(crate::normal::compose_key(31, path)),
        i64::from(crate::normal::compose_key(32, path)),
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected, "one draw per chance, each keyed by its own line");
}

/// A grid sheet over the lines that work out values from the cells around: three Look arounds, two with an `only` test
/// and one of them able to leave out every neighbour, covering every summary, a Count with an `only` test, a Calculate
/// over their parts, a Choose, a Depending on, and Sets and a Become reading them.
fn values() -> String {
    include_str!("sheets/values.toml").to_owned()
}

/// Plants random statuses, flags and levels on a world `side` cells wide, runs one tick of the lowered values sheet
/// `file`, and returns the lanes with the planted statuses, flags and levels.
fn values_after_one_tick(file: &SheetFile, side: u32) -> (HostLanes, Vec<bool>, Vec<bool>, Vec<f32>) {
    let cells = (side * side) as usize;
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    };
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let mut rng = 0x7661_6c75_6573_0001u64;
    let (mut status, mut flag, mut level) = (vec![false; cells], vec![false; cells], vec![0.0f32; cells]);
    for cell in 0..cells {
        rng = henad_core::authoring::primitives::rng::xorshift64(rng);
        status[cell] = rng.is_multiple_of(3);
        flag[cell] = (rng >> 8).is_multiple_of(2);
        level[cell] = ((rng >> 16) % 1000) as f32 / 1000.0;
        assert!(
            lanes
                .column_mut("status")
                .expect("status")
                .set(cell, HostValue::Category(u32::from(status[cell])))
        );
        assert!(
            lanes
                .column_mut("flag")
                .expect("flag")
                .set(cell, HostValue::Bool(flag[cell]))
        );
        assert!(
            lanes
                .column_mut("level")
                .expect("level")
                .set(cell, HostValue::F32(level[cell]))
        );
    }
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(1), 0),
            &mut stream,
        )
        .expect("a tick runs");

    (lanes, status, flag, level)
}

#[test]
fn the_values_sheet_runs_its_lines_on_the_host() {
    let parsed = crate::sheet::ParsedSheet::parse(&values()).expect("the values sheet reads");
    assert_eq!(
        parsed.document().to_text().as_deref(),
        Ok(values().as_str()),
        "the values sheet is canonical"
    );
    let file = lowered(&values());
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let report = file.lowered().check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    assert!(GridProgram::compile(file.lowered()).is_ok());
    let tick = &file.lowered().document().passes[1];
    for op in ["grid.fold", "math.expr", "cat.switch", "logic.select"] {
        assert!(tick.body.nodes.iter().any(|node| node.op == op), "no {op}");
    }

    let side = 8u32;
    let cells = (side * side) as usize;
    let (lanes, status, flag, level) = values_after_one_tick(&file, side);

    let at = |x: i64, y: i64| ((y.rem_euclid(side as i64)) * side as i64 + x.rem_euclid(side as i64)) as usize;
    let near = |value: Option<HostValue>, expected: f32| match value {
        Some(HostValue::F32(found)) => (found - expected).abs() < 1e-4,
        _ => false,
    };
    for cell in 0..cells {
        let (x, y) = ((cell as u32 % side) as i64, (cell as u32 / side) as i64);
        let around: Vec<usize> = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
            .iter()
            .map(|&(dx, dy)| at(x + dx, y + dy))
            .collect();
        let total: f32 = around.iter().map(|&other| level[other]).sum();
        let top = around.iter().map(|&other| level[other]).fold(f32::MIN, f32::max);
        let low = around.iter().map(|&other| level[other]).fold(f32::MAX, f32::min);
        let beside: Vec<usize> = [(0, -1), (-1, 0), (1, 0), (0, 1)]
            .iter()
            .map(|&(dx, dy)| at(x + dx, y + dy))
            .filter(|&other| status[other])
            .collect();
        let alive_near = beside.len() as f32;
        let alive_level = if beside.is_empty() {
            0.0
        } else {
            beside.iter().map(|&other| level[other]).sum::<f32>() / alive_near
        };
        let any_flagged = beside.iter().any(|&other| flag[other]);
        let all_flagged = beside.iter().all(|&other| flag[other]);
        let spread = (top - low) / 2.0 + alive_near.max(1.0).powi(2) - level[cell];
        let band = if alive_level >= 0.5 {
            1
        } else if any_flagged {
            2
        } else {
            3
        };
        let score = if status[cell] { 10.0 } else { spread };
        let new_status = status[cell] || all_flagged;
        assert!(
            near(lanes.column("total").and_then(|lane| lane.get(cell)), total),
            "total at {cell}"
        );
        assert!(
            near(lanes.column("score").and_then(|lane| lane.get(cell)), score),
            "score at {cell}"
        );
        assert_eq!(
            lanes.column("band").and_then(|lane| lane.get(cell)),
            Some(HostValue::I32(band)),
            "band at {cell}"
        );
        assert_eq!(
            lanes.column("status").and_then(|lane| lane.get(cell)),
            Some(HostValue::Category(u32::from(new_status))),
            "status at {cell}"
        );
    }
}

#[test]
fn the_values_sheet_honors_each_only_and_gives_0_for_a_summary_of_no_cell() {
    let file = lowered(&values());
    let side = 8u32;
    let cells = (side * side) as usize;
    let (lanes, status, _, level) = values_after_one_tick(&file, side);
    let at = |x: i64, y: i64| ((y.rem_euclid(side as i64)) * side as i64 + x.rem_euclid(side as i64)) as usize;
    // Cells whose Count with an `only` differs from the same Count without it, and cells with and without a
    // neighbour of level at least 0.8, so each `only` and each empty summary is met at least once.
    let (mut trimmed, mut empty, mut filled) = (0, 0, 0);
    for cell in 0..cells {
        let (x, y) = ((cell as u32 % side) as i64, (cell as u32 / side) as i64);
        let around: Vec<usize> = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
            .iter()
            .map(|&(dx, dy)| at(x + dx, y + dy))
            .collect();
        let alive_around = around.iter().filter(|&&other| status[other]).count();
        let bright = around
            .iter()
            .filter(|&&other| status[other] && level[other] >= 0.5)
            .count();
        let high: Vec<f32> = around
            .iter()
            .map(|&other| level[other])
            .filter(|&other| other >= 0.8)
            .collect();
        let (high_mean, high_top, high_low) = if high.is_empty() {
            (0.0, 0.0, 0.0)
        } else {
            (
                high.iter().sum::<f32>() / high.len() as f32,
                high.iter().copied().fold(f32::MIN, f32::max),
                high.iter().copied().fold(f32::MAX, f32::min),
            )
        };
        trimmed += usize::from(bright != alive_around);
        empty += usize::from(high.is_empty());
        filled += usize::from(!high.is_empty());
        assert_eq!(
            lanes.column("bright").and_then(|lane| lane.get(cell)),
            Some(HostValue::I32(bright as i32)),
            "bright at {cell}"
        );
        for (lane, expected) in [("high_mean", high_mean), ("high_top", high_top), ("high_low", high_low)] {
            let found = lanes.column(lane).and_then(|column| column.get(cell));
            assert!(
                matches!(found, Some(HostValue::F32(value)) if (value - expected).abs() < 1e-4),
                "{lane} at {cell}: {found:?}, not {expected}"
            );
        }
    }
    assert!(trimmed > 0, "the Count's `only` never leaves a cell out");
    assert!(
        empty > 0 && filled > 0,
        "{empty} cells see no high level, {filled} see one"
    );
}

#[test]
fn a_formula_of_arrows_or_past_the_depth_limit_is_refused_at_its_line() {
    for formula in ["length(l1)", &vec!["l1"; 20].join(" + ")] {
        let text = life_with_line(&format!(
            "[card.line.l5]\nkind = \"calculate\"\nname = \"Worked out\"\nformula = \"{formula}\""
        ));
        let errors = refusals(&text);
        assert_eq!(errors.len(), 1, "{formula}: {errors:?}");
        assert!(placed_at(&errors[0], &text).starts_with("[card.line.l5]"), "{formula}");
    }
}

#[test]
fn a_random_number_line_draws_between_its_values() {
    let text =
        life_with_line("[card.line.l5]\nkind = \"random_number\"\nname = \"Noise\"\nkey = 13\nlow = 2\nhigh = 3");
    let text = edited(&text, "[[parameter]]", &format!("{EXTRA_PROPERTIES}[[parameter]]"));
    let text = edited(
        &text,
        "[card.line.l5]",
        "[card.line.l6]\nkind = \"set\"\nproperty = \"level\"\nvalue = { result = \"l5\" }\n\n\
         [card.line.l6.condition]\nkind = \"always\"\n\n[card.line.l5]",
    );
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\", \"l6\"]",
    );
    let text = edited(&text, "next_line = 6", "next_line = 7");
    let file = lowered(&text);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: 32,
        height: 32,
        boundary: Boundary::Torus,
    };
    let mut lanes = evaluator.init(world, &[], Some(3)).expect("the start runs");
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(3), 0),
            &mut stream,
        )
        .expect("a tick runs");
    let drawn = levels(&lanes);
    assert!(drawn.iter().all(|level| (2.0..3.0).contains(level)), "{drawn:?}");
    let mean = drawn.iter().sum::<f32>() / drawn.len() as f32;
    assert!((mean - 2.5).abs() < 0.05, "{mean}");
}
