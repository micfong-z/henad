//! The lowering of grid sheets: every grid sheet checked and compiled, ids, keys and the line table, known answers on
//! the host, the two ways a world starts again, skipped lines, the refusals of the line kinds this version does not
//! lower and the refusals lifted, the node limits, the places of graph diagnostics and a mutation corpus.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::action::action_seed;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::explore::fingerprint::fnv1a64;

use super::sheet_support::{
    KINDS, LIFE_PARAMS, SHEET_HOSTILE, VALUES, alive, code, edited, host_of, life, life_with, life_with_line, lowered,
    only_refusal, placed_at, sheets, sized, started, vote,
};
use super::support::{line_of, mutations, node_id, torus, without_each_key};
use crate::document::PassStage;
use crate::host::{HostLanes, HostValue, RunContext};
use crate::ids::NodeId;
use crate::limits::MAX_NODES_IN_FILE;
use crate::sheet::edit::lines::{parse_path, phrase_mut, value_mut};
use crate::sheet::limits::MAX_LINES_PER_CARD;
use crate::sheet::{NodeOwner, SheetFile, SheetFileError, sentence, starters};
use crate::value::ConfigValue;
use crate::wgsl::GridProgram;

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
        [
            "game_of_life.toml",
            "sir.toml",
            "vote.toml",
            "forest_fire.toml",
            "sir_chance.toml"
        ]
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
    assert_eq!(count.config["include_center"], ConfigValue::Bool(true));
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
    for text in [life(), vote(), KINDS] {
        let first = lowered(text).lowered().to_toml();
        let second = lowered(text).lowered().to_toml();
        assert_eq!(first, second);
    }
}

#[test]
fn lowered_ids_sit_in_their_line_blocks() {
    for text in [life(), vote(), KINDS] {
        let file = lowered(text);
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
    for text in [life(), vote(), KINDS] {
        let file = lowered(text);
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
    sheets.push(("values.toml", VALUES));
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
    let derived = (fnv1a64(b"sheet_game_of_life/mood") & 0xffff_ffff) as u32;
    let text = edited(life(), "key = 2929187291", &format!("key = {derived}"));
    let text = edited(
        &text,
        "[[parameter]]",
        "[[cells.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  { id = \"calm\", name = \
         \"Calm\", color = \"#202020\" },\n  { id = \"angry\", name = \"Angry\", color = \"#E04040\" },\n]\n\n\
         [cells.property.starts_as]\nrule = \"by_chance\"\notherwise = \"calm\"\n\n[[cells.property.starts_as.row]]\n\
         state = \"angry\"\nchance = 0.5\n\n[[parameter]]",
    );
    let error = only_refusal(&text);
    let message = &error.message;
    assert!(
        message.contains("property 'mood'") && message.contains("property 'status'"),
        "{message}"
    );
    let mood = text.find("[[cells.property]]\nid = \"mood\"").expect("the property");
    assert_eq!(error.span.as_ref().map(|span| span.start), Some(mood), "{error:?}");
}

#[test]
fn a_start_with_no_key_takes_one_from_the_sheet_and_property_ids() {
    let text = edited(life(), "key = 2929187291\n", "");
    let file = lowered(&text);
    assert_eq!(file.warnings().len(), 1, "{:?}", file.warnings());
    let derived = (fnv1a64(b"sheet_game_of_life/status") & 0xffff_ffff) as u32;
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

/// Runs `ticks` ticks of the lowered Game of Life `file` on a world `side` cells wide from the alive cells `cells`, and
/// returns the alive cells after each tick, the planted ones first.
fn run_life(file: &SheetFile, side: u32, cells: &[(u32, u32)], ticks: u32) -> Vec<BTreeSet<(u32, u32)>> {
    let evaluator = host_of(file);
    let world = torus(side, side);
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
    let file = lowered(&sized(life(), 32));
    let states = run_life(&file, 32, &[(15, 14), (15, 15), (15, 16)], 4);
    assert_eq!(states[1], BTreeSet::from([(14, 15), (15, 15), (16, 15)]));
    assert_eq!(states[2], states[0]);
    assert_eq!(states[3], states[1]);
    assert_eq!(states[4], states[0]);
}

#[test]
fn the_life_sheet_glider_moves_one_cell_diagonally_every_4_ticks() {
    let file = lowered(&sized(life(), 32));
    let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let states = run_life(&file, 32, &glider, 8);
    for (period, shift) in [(4, 1), (8, 2)] {
        let moved: BTreeSet<(u32, u32)> = glider.iter().map(|&(x, y)| (x + shift, y + shift)).collect();
        assert_eq!(states[period], moved, "after {period} ticks");
    }
    assert!(states.iter().all(|state| state.len() == 5), "a glider keeps 5 cells");
}

#[test]
fn a_rebuild_at_one_seed_replays_the_same_start() {
    let file = lowered(&sized(life(), 64));
    let evaluator = host_of(&file);
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
    let file = lowered(&sized(life(), 64));
    let evaluator = host_of(&file);
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
    let repeat = edited(life(), "lines = [\"l1\", \"l2\", \"l3\"]", "lines = [\"l1\", \"l5\"]");
    let repeat = edited(&repeat, "next_line = 5", "next_line = 6");
    let repeat = edited(
        &repeat,
        "# Become Alive from Dead",
        "[card.line.l5]\nkind = \"repeat\"\ntimes = 2\nlines = [\"l2\", \"l3\"]\n\n# Become Alive from Dead",
    );
    let error = only_refusal(&repeat);
    assert_eq!(error.message, sentence::NOT_YET_REPEAT);
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
    let error = only_refusal(&text);
    assert!(error.message.contains("card 'clear'"), "{}", error.message);
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

#[test]
fn the_sheet_lowering_never_panics_on_the_mutation_corpus() {
    let mut rng = 0x6c6f_7765_7200_0007u64;
    let (mut cases, mut lowered_cases) = (0usize, 0usize);
    let texts: Vec<&str> = sheets().into_iter().map(|(_, text)| text).chain([KINDS]).collect();
    for text in texts {
        // Edits of a sheet that does not lower count toward the corpus, and not toward the share that lowers.
        let lowers = SheetFile::parse(text).is_ok();
        for edited in mutations(text, &mut rng, &SHEET_HOSTILE, 200).chain(without_each_key(text)) {
            let outcome = exercise(&edited);
            if lowers {
                cases += 1;
                lowered_cases += usize::from(outcome);
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
            let nodes: usize = document.passes.iter().map(|pass| pass.body.nodes.len()).sum::<usize>()
                + document
                    .functions
                    .iter()
                    .map(|function| function.body.nodes.len())
                    .sum::<usize>();
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

#[test]
fn a_line_past_the_node_limit_of_a_line_is_refused_in_words() {
    let numbers: Vec<String> = (0..600).map(|number| number.to_string()).collect();
    let table = format!(
        "[card.line.l5]\nkind = \"compare\"\nname = \"Listed\"\ntest = \"one_of\"\nleft = {{ result = \"l1\" }}\n\
         right = [{}]",
        numbers.join(", ")
    );
    let text = life_with_line(&table);
    let error = only_refusal(&text);
    assert_eq!(error.message, crate::sheet::sentence::LINE_TOO_LONG);
    assert!(placed_at(&error, &text).starts_with("[card.line.l5]"));
}

#[test]
fn a_sheet_past_the_node_limit_of_the_sheet_is_refused_in_words() {
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
    let error = only_refusal(&text);
    assert_eq!(error.message, crate::sheet::sentence::SHEET_TOO_LARGE);
}

#[test]
fn a_sheet_error_list_holds_its_first_error_first() {
    let errors = crate::sheet::SheetErrors::new(CodeDiagnostic::error("first"), [CodeDiagnostic::error("second")]);
    let error = SheetFileError::Invalid(errors);
    assert_eq!(error.diagnostic().message, "first");
    assert_eq!(error.diagnostics().len(), 2);
    assert_eq!(error.to_string().lines().count(), 2);
}

/// Returns Game of Life lowered with the node `id` of the tick pass made a node of an op the catalogue does not hold,
/// added to that pass when the lowering wrote no node `id`.
fn life_with_unknown_node(id: NodeId) -> SheetFile {
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
fn graph_error_on(file: &SheetFile, id: NodeId) -> CodeDiagnostic {
    let report = file.lowered().check();
    let node = id.to_string();
    report
        .errors()
        .find(|error| error.node.as_deref() == Some(node.as_str()))
        .cloned()
        .unwrap_or_else(|| panic!("no error on {node}: {:?}", report.errors().collect::<Vec<_>>()))
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
    let placed = file.place_graph_diagnostic(CodeDiagnostic::error("a").with_node(count.to_string()));
    assert!(placed_at(&placed, life()).starts_with("[[cells.property]]"), "{placed}");

    // A finding with a pass and no node is placed at the card the pass runs.
    let placed = file.place_graph_diagnostic(CodeDiagnostic::error("b").with_pass("tick_cell_tick"));
    assert_eq!(placed.pass, None);
    assert_eq!(
        placed.line,
        Some(line_of(life(), "[[card]]\nid = \"cell_tick\"")),
        "{placed}"
    );
    let placed = file.place_graph_diagnostic(CodeDiagnostic::error("c").with_pass("restart_randomise"));
    assert_eq!(
        placed.line,
        Some(line_of(life(), "[[button]]\nid = \"randomise\"")),
        "{placed}"
    );
}

#[test]
fn an_unknown_node_is_placed_on_the_sheet() {
    let id = node_id(900_000);
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
        CodeDiagnostic::error("a").with_pass("measures"),
        CodeDiagnostic::warning("b"),
    ] {
        let placed = file.place_graph_diagnostic(diagnostic);
        assert_eq!(placed.line, Some(line_of(life(), "[sheet]")), "{placed}");
    }
}
