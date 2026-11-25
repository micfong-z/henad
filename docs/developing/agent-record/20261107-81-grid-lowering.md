---
date: 2026-11-07
title: "Grid lowering"
description: "S7 of the #37 redesign. henad-graph gains SheetFile, which reads, checks and lowers a grid sheet to the graph it runs as, and the lowering under sheet/lower: node ids in one block per line, keys composed per draw with V18 after lowering, an explicit line table, rows in display order only where two changes can both apply, and the starts, buttons, measures, colours, Counts, Yes or no lines, Becomes, Sets and When blocks of a grid. Game of Life and Majority vote lower, check with no warning and compile, and the line kinds a later milestone lowers are refused at their line in the author's words."
icon: material/layers-triple-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s7-grid-lowering, two unsigned [draft] commits and this record after 8ae998d; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 8ae998d
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sheet files and the grid lowering [draft]`, `fix: s7 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Grid lowering

> S7 makes a grid sheet run.
> `henad_graph::sheet::SheetFile::parse` reads a sheet, checks it, lowers it to a library 1 graph through `GraphDocument`'s constructors and reads that graph back, and the sheet's code identity is the lowered graph's.
> Line `l<k>` owns the node ids `n(1000k + 1)` to `n(1000k + 999)`, the sheet owns `n1` to `n999` in a fixed walk, and `LineMap` records every node's line or part of the sheet and its slot path, so no id depends on another line's place.
> Each draw takes a key composed from its owner's key and its slot path, written on the draw itself, and two draws with one key are an error naming both owners.
> Starts, buttons, measures, colours, Counts, Yes or no lines, Becomes, Sets and When blocks lower, Becomes of one property to one `cat.transition` whose rows follow display order only where two lines can both apply.
> Game of Life and Majority vote lower, check with no warning and compile, a blinker and a glider behave on the host, and chances, rates, Look around, Calculate, Choose, Depending on, Random number, Repeat and worlds of agents are each refused at their table in the author's words ("Chances are not supported yet.").

## State before

- `37-node-editor` at `8ae998d` (S5 merged), with S12 planned beside S7.
- S1a's reader, S4's writer and wording table and S5's checker and `CardRelations` held a sheet from text to its skip warnings, and nothing turned a sheet into a graph.
- The round 1 models existed as hand-lowered graphs in henad-models' `tests/fixtures/lowered/round1/`, stand-ins for a compiler that did not exist.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Sheet files

`crates/henad-graph/src/sheet/file.rs` holds `SheetFile`, re-exported as `henad_graph::sheet::SheetFile`.
`SheetFile::parse(text)` returns the reader's errors, then the checker's errors as the new `SheetFileError::Invalid`, then the lowering's problems, also `Invalid`, each placed at its table through the checker's `span_of`, and reads the lowered document back through `GraphFile::from_document`.
It keeps the `ParsedSheet` (document, text, places and line ends), the canonical text (`to_toml`), the checker's warnings placed in the text (`warnings`, skip warnings included), the lowered `GraphFile` (`lowered`) and the `LineMap` (`line_map`).
`code()` is the lowered graph's `code()`: the graph's `name` is `[sheet] name`, both descriptions stay empty, and nothing else of the sheet's text enters the identity (D8).
`SheetFileError` gains `diagnostics()`, every diagnostic it holds, and its `Display` writes each on its own line.
`Invalid` holds a `SheetErrors`, a list that `SheetErrors::new` builds from a first error and the rest, so it is never empty.

### The lowering

`crates/henad-graph/src/sheet/lower/` is private.
`lower(&document, &report)` builds the document in a fixed order: the grid, a category and a lane per cells property (state ids as variant names, state names as labels, `u8` storage for states, `f32` for Number, `i32` for Whole number, `bool` for Yes/No), a `[[param]]` per parameter (`u32` for a Whole number from 0, `f32` otherwise), then the passes in stack order.

- `start_cells`, the starts: a value, "All [state]", a random number between two values (`rand.float(high - low) + low`), and by chance with one draw per cell, `rand.chance` for one row and `rand.float(1)` against running sums for more, chosen through a `logic.select` chain ending in the "otherwise" state, as the prototype's start does.
- `tick_<card>`, the card that runs each tick, or an empty `tick_cells` without one, since a grid runs exactly one tick pass.
- `colours`, only when the colour rule needs a view pass: none for the first States property, `view.color` through the category for another, and `view.ramp` over 0 to 1 into a palette for a shaded number.
- `measures`, one `stat.count_by` per property shared by its counts, `stat.mean`, `stat.sum`, and a spread as two means and `f64` arithmetic, each `[[stat]]` labelled with the measure's name, its results column (D13).
- Per button an `[[action]]`: Start again runs `start_cells` when it names every property with a start and otherwise a disabled init pass `restart_<button>`, and Set everywhere and Run once write the action pass `button_<button>`.
  A rerun keys its draws on the press count, so each press draws a new layout, and a rebuild at one seed replays the start (D73).

`ids.rs` numbers the nodes after they are written with temporary ids (R4, D44): line `l<k>` takes `n(1000k + j)` in its own emission order, and the sheet takes `n1` upward in a walk of the passes, each pass's sheet nodes ordered by a `SheetKey` (the shared `lane.get` of each property, the parts, the changes, the measures).
A line past `MAX_NODES_PER_LINE` nodes, and a sheet past `MAX_SHEET_NODES` of its own, is refused with the limit named.
`table.rs` holds `LineMap`, `NodeOrigin` and `NodeOwner`, public through `henad_graph::sheet`: each written node id with its owner (`Line(id)` or `Sheet`) and a slot path such as `condition.right.2` or `cells.status.starts_as`, never computed by division.
`keys.rs` derives a missing key from `<sheet id>/<owner id>` and composes each draw's key from its owner's key and the low 32 bits of FNV-1a 64 over its slot path (R3, D12, D45), and `clashes` checks the composed keys after lowering (V18).

`card.rs` lowers a card's lines in display order and leaves out every line the checker skips, its dependents included (D39, D84).
A Count over cells is a `grid.count` per counted state when it tests their States property against states, and a `grid.fold` with a `count` accumulator, `lane.get_at` for "their" and a `region.put` otherwise.
Comparisons lower to `cmp.*`, "one of" to an or of equalities, "within" to `abs(a - b) <= tolerance`, and "outside" to an or of two compares (D82).
All of, any of and none of fold with `logic.and`, `logic.or` and `logic.not`, and "always" is the literal `true` (D81).
A When's condition is joined by `logic.and` into each change inside.
The Becomes of one property lower to one `cat.transition`, a change from any state taking a row per state, and the Sets of one property to a `logic.select` chain from the bottom up, with rows in a stable topological sort: display order where `CardRelations::can_both_apply`, line id order otherwise.
`value.rs` types the values: a count is `u32`, a typed number takes the type it meets when that type holds it exactly, other number pairs meet in `f32`, and a percent is its number divided by 100 (D110).

### Interim refusals

Each line holding a kind a later milestone lowers, as its own kind or as a phrase in any slot, gives one error at its line table in the author's words, new in the wording module: `NOT_YET_CHANCE`, `NOT_YET_RATE`, `NOT_YET_LOOK_AROUND`, `NOT_YET_CALCULATE`, `NOT_YET_CHOOSE`, `NOT_YET_DEPENDING_ON`, `NOT_YET_RANDOM_NUMBER` and `NOT_YET_REPEAT` (D38).
A world of agents gives `NOT_YET_AGENTS` at `[world]`.
Record on a cells card stays an error (D66), as do the agent line kinds and the starts that place agents or paint the ground.

### Tests

`crates/henad-graph/src/tests/sheet_lower.rs`, on the host evaluator:

- `game_of_life_and_vote_lower_check_and_compile`: no sheet warning, no graph diagnostic, `GridProgram::compile` (composition and naga validation, no device), an entry, and each starter its own canonical text.
- `the_lowered_vote_counts_its_block_and_lets_line_2_win`: the 3 by 3 count and the rows `l2_a`, `l2_b`, `l3_a`, `l3_b`.
- `lowering_is_deterministic`, `lowered_ids_sit_in_their_line_blocks`, `the_line_map_covers_every_written_node`.
- `two_draws_with_one_key_are_an_error_naming_both`, `a_start_with_no_key_takes_one_from_the_sheet_and_property_ids`, `a_percent_lowers_to_its_number_divided_by_100`, `a_measure_is_a_stat_named_after_it`.
- One test per identity row of the author format for grids: renaming a result, reordering two independent lines, and editing a note, `purpose` or a comment keep the code; renaming a parameter, a state or the model, changing a default or a colour, drawing new numbers and reordering two changes that can both apply change it.
- `the_life_sheet_blinker_has_period_2` and `the_life_sheet_glider_moves_one_cell_diagonally_every_4_ticks`, on a 32 by 32 wrapping grid whose tick-0 lanes the test plants (D74).
- `a_rebuild_at_one_seed_replays_the_same_start` and `a_start_again_button_draws_a_new_layout_each_press`.
- `a_skipped_line_and_its_dependents_are_left_out`, `each_interim_refusal_gives_one_located_error` (the seven line kinds and a chance phrase, Repeat and the agents world), `a_second_card_that_runs_each_tick_is_refused`, `a_grid_sheet_with_no_property_lowers_to_nothing`.
- `the_largest_grid_sheet_lowers_under_half_the_node_limit`: a card of `MAX_LINES_PER_CARD` (256) Count and Become lines over four properties lowers to 656 nodes, under 8,192, and checks.
- `the_kinds_sheet_runs_its_lines_on_the_host`: the new test sheet `src/tests/sheets/kinds.toml`, canonical, holds a Count of each form, an all of holding a none of, two Sets of one property, the outside test, a Set inside a When on an any of, and Becomes with one of, within and "my new", and one tick on a planted 8 by 8 world equals a Rust reference cell by cell.
- `the_sheet_lowering_never_panics_on_the_mutation_corpus`, over every sheet and the kinds sheet.
- `keys.rs` and `value.rs` hold inline tests of the derived key, the composed keys and the typing of numbers.

### Docs and topic files

- `agents/henad-graph-sheet.md`: the module list, and new paragraphs on sheet files, the lowering, ids, the line table, keys, starts and buttons, cards and their values, the interim refusals, and the tests. 30,986 characters after the change.
- `AGENTS.md`: the sheet topic file's row names the lowering.
- `crates/henad-graph/src/sheet/mod.rs`: the module doc names `SheetFile` and `LineMap`.
- One CHANGELOG line under Unreleased.

### Edited tree

```
AGENTS.md                                        sheet topic row
CHANGELOG.md                                     sheet lowering
agents/
└── henad-graph-sheet.md                         sheet files, the lowering, tests
crates/henad-graph/src/
├── sheet/
│   ├── mod.rs                                   SheetFile, LineMap, SheetFileError::Invalid, module doc
│   ├── file.rs                                  new: SheetFile
│   ├── document.rs                              SheetLine::slot_values, moved from the checker
│   ├── sentence.rs                              the interim refusals' words
│   ├── check/
│   │   ├── mod.rs                               Place, Thing and span_of visible to the sheet module
│   │   └── lines.rs                             reads SheetLine::slot_values
│   └── lower/                                   new
│       ├── mod.rs                               lower, Builder, Site, the numbering in finish
│       ├── ids.rs                               line blocks and the sheet's walk
│       ├── table.rs                             LineMap, NodeOrigin, NodeOwner
│       ├── keys.rs                              derived and composed keys, V18
│       ├── value.rs                             Operand and the number types
│       ├── world.rs                             grid, categories, lanes, parameters
│       ├── start.rs                             start_cells and restart passes
│       ├── card.rs                              tick pass, lines, changes, refusals
│       ├── measures.rs                          measures pass and stats
│       ├── colours.rs                           colours pass
│       └── buttons.rs                           actions and action passes
└── tests/
    ├── mod.rs                                   sheet_lower
    ├── sheet_lower.rs                           new
    ├── sheet_sentence.rs                        the interim refusals' words
    └── sheets/
        └── kinds.toml                           new: every line kind the grid lowering writes
```

### Deviations from the plan

- **One feature commit instead of two.** The framework and the line kinds share `card.rs`, `value.rs` and their tests, so `feat: sheet files and the grid lowering [draft]` holds both planned commits.
- **The key clash test plants its clash on two starts**, as `two_draws_with_one_key_are_an_error_naming_both`, since no line draws while chances are refused. One start stores the key the other derives, which the checker cannot see. The check is the same for lines, and S10's chance lines reach it.
- **`SheetFile` holds the `ParsedSheet`**, which holds the plan's `SheetPlaces` and `LineEnds`, and its accessors return them.
- **A grid sheet with no cells property lowers to its world alone**, no lane and no pass, with the checker's `SHEET_EMPTY` warning. The graph's checker then reports that a grid needs a lane, so an empty sheet opens and does not build.
- **The interim refusals hold whatever the line's skip**, so a half-built chance line is refused as a finished one is. A line refused is refused for its kind, which the author fixes the same way either way.
- **`the_largest_grid_sheet_lowers_under_half_the_node_limit` builds its card in the test**: 256 lines of the kinds Game of Life uses, over four properties, since one property holds at most `MAX_TRANSITION_RULES` (64) rows. A card of long lists can lower to more, and the graph checker's limits refuse it.
- **A button holds Start again steps or the other steps, never both**, and a property written twice in one button and a card run by two buttons are errors. An action's reruns and its action pass have no order a button's steps could set, and a card's lines can sit in one action pass alone.
- **More than one card that runs each tick over the cells is an error**, since a grid runs one tick pass (V29).
- **The shaded colour** takes the palette of the hand-lowered bounded confidence over 0 to 1. The schema holds no ramp or range, and no grid starter shades.
  A shaded rule with `scale = "log"` or `decades` is an error at `[color.cells]`, since only a linear shading lowers.
- **A random number start fits a Number property only**, as SPEC 7.3 offers it. On a Whole number property it is an error at the property, since a draw truncated toward zero never reaches the high end.
- **The pass id `colours` and the module `colours.rs` keep the British spelling** that the plan (D44) and the author-file design give the pass. AGENTS.md asks for American spelling in identifiers and string literals, and the hand-lowered round 1 fixtures also name their view passes `colour_<thing>`. The maintainer decides whether the pass becomes `colors`.
- **`[time]` is not lowered.** The tick length parameter and its time scale come with rates (S21, D4).
- **Starts on a grid** that place agents or paint the ground, and colour rules that shade a state, are technical errors, since they belong to the ground of agents.

## State after

- A grid sheet of starts, buttons, measures, colours, Counts, Yes or no lines, Becomes, Sets and When blocks lowers to a graph that checks and compiles, with a table from every node to its line, and its code identity follows the author format's identity table.
- Game of Life and Majority vote lower with no warning, and their lowered graphs compile.
- Every other kind of line is refused at its table in the author's words until its milestone lowers it.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph --all-targets --all-features -- -D warnings -W clippy::all`; `cargo check --workspace --all-targets`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph --all-targets --all-features` (328 passed, 1 ignored, after the review fix); `cargo test -p henad-graph --doc` (6 passed); `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features`; `cargo check -p henad-graph --all-features --lib --target wasm32-unknown-unknown`.
  The full check runs at the merge point.
- The merge check found the branch already based on the tip of `37-node-editor` at `8ae998d`, so the rebase changed nothing and met no conflict.
- The full check of plan section 2.3 ran on the branch at `d5091fb`, with `HENAD_REQUIRE_GPU=1`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models with the baseline's message (`boids.toml: 14816908000000 units in one submission`), and its log shows no other test binary that failed or aborted.
  The henad-graph tests pass in that run, 328 with 1 ignored.
  No baseline test now passes, and the merge check needed no fix.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.

## Issues found & future directions

1. S10 lifts the refusals of chances, Look around, Calculate, Choose, Depending on and Random number, each with a test that the refusal is gone, and its chance lines reach the key clash check through lines.
2. S11 maps the lowered graph's diagnostics to author lines through `LineMap` and wires `SheetFile` into `ModelFile`, the CLI and the facade. A sheet with no property fails its build there with the graph checker's message, which S11 may word for the author.
3. The sheet's own nodes all sit in `n1` to `n999`, the restart passes' copies of the starts included. A sheet with many properties and buttons that restart subsets could reach that limit before its lines do.
4. The lowered Game of Life matches the hand-lowered round 1 graph in its tick pass but draws its start with a composed key, so its tick-0 lanes differ from the hand-lowered fixture's by design. S14's device tests compare the lowered sheets with the graph starters by behaviour.
5. The rows of a `cat.transition` count a change from any state once per state, so a property of many states changed by many any-state lines reaches `MAX_TRANSITION_RULES` early, and the lowering then refuses the card.
6. The "otherwise" rule tries values with an allowance of 10^-9 and the lowered comparisons run in `f32` with none, as the gate notes. A typed number that `u32` holds exactly compares a count exactly.
7. Every result line lowers, whether or not a kept line reads it. A Count written before the line that will use it, or one whose only reader is skipped, then gives the graph checker one technical warning per node, such as "Neighborhood fold n2001 reaches nothing the pass keeps, and will be dropped". S11 filters these warnings or words them for the author when it maps diagnostics to lines.
8. The log scale of a shaded colour is refused until a log ramp lowers.

## Review

A review agent read the S7 commit and reported four findings to fix and five nits, and `fix: s7 review findings [draft]` fixes all of them but the two nits written down instead.
A shaded colour with `scale = "log"` or `decades` lowered silently to a linear ramp, and is now an error at `[color.cells]` (`a_shading_on_a_log_scale_is_refused`).
A random number start on a Whole number property truncated its draw, so "between 1 and 6" never gave 6, and it is now an error at the property, as SPEC 7.3 offers that start for a Number property alone (`a_random_number_start_of_a_whole_number_is_refused`, with `a_random_number_start_draws_between_its_values` holding the Number case).
`SheetFileError::diagnostic` indexed a public `Invalid(Vec<CodeDiagnostic>)` that a caller could build empty, and `Invalid` now holds a `SheetErrors`, which only `SheetErrors::new(first, rest)` builds (`a_sheet_error_list_holds_its_first_error_first`).
The paths without tests gained them on Game of Life with a random Number `level` and a States `mood` added: Set everywhere and a property set twice by one button, Start again on some properties (the disabled `restart_randomise` pass changes `status`, leaves `level` alone and replays), average, total and spread against a Rust reference on planted lanes, `view.color` for a second States property and `view.ramp` into `shaded_level`, and both node limits refused with the limit named, the line's through a `one_of` list of 600 numbers and the sheet's through 150 spread measures.
The comment nits are fixed: the trailing "which" clauses are split, "the line kinds a later version lowers" reads "the line kinds this version does not lower", and the `SheetFile` doc names what enters the identity and what stays outside it.
`LineMap` writes its own `Debug` with its length, and `SheetFileError` is documented as the reason a sheet cannot be read, checked or lowered.
The record's three text slips are corrected.
Two nits are written down without a code change: the British `colours` pass id follows the plan and waits for the maintainer (deviations), and results that no kept line reads still lower, left to S11's diagnostic mapping (issue 7).
After the fix, `cargo fmt --all -- --check`, `cargo clippy -p henad-graph --all-targets --all-features -- -D warnings -W clippy::all`, `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph --all-targets --all-features` (328 passed, 1 ignored), `cargo test -p henad-graph --doc` (6 passed), rustdoc with `-D warnings`, `cargo check --workspace --all-targets` and the wasm32 typecheck of henad-graph pass.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D8 | What the code identity hashes | the author-file design, section 7, question 2 | (a) The lowered IR's whole semantic text | S7 |
| D12 | Keys | the author-file design, section 7, question 6 | (a) Minted at random and stored, derived from the sheet and line ids with a warning when missing | S7, S12 |
| D13 | A measure's name is its CSV column | the author-file design, section 7, question 7 | (a) Yes | S7 |
| D38 | Line kinds whose lowering lands later | the plan | A located error in the author's words until the lowering exists, each lifted with a test that the refusal is gone | S7, S10, S17, S21 |
| D39 | How a half-built line is stored and run | both drafts (P1) | An absent slot key, read as blank and written as nothing. Line-local problems are skip warnings in the final specification's words, and the run skips the line and every line that reads it. File-level problems are errors | S1a, S5, S7 |
| D44 | Node ids and their lines | the recon of henad-graph | `n(1000k + j)` ids for stable text, with an explicit line table in place of division by 1000 | S7 |
| D45 | Draws and keys | the recon of henad-graph | Draws lowered inline with final composed keys, and no generated functions that draw | S7 |
| D66 | Record on a grid card | the recon of the engines | Stays refused | S7 |
| D73 | The World pane's controls kept as the dock has them | the specification's "World", the round A change list's F15, F20 and F22, the prototype's notes | **Plan's choice** within D56. The plot's whole run (F15) is the Charts tab's Unlimited history. The sticky run bar (F20) is the Playback tab, which never scrolls away. The legend stays on the Charts tab and current values on the Stats tab. The world's Start again is Playback's Build, which replays at the same seed, and a sheet button's Start again draws a new layout each press, as the prototype's notes say. The 0 label on the left axis (F22) is built | S7, S9, S13a, S14 |

From the gate, the rows whose "Built in" names S7, with the final specification named so and change lists named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D81 | "always" and "any state" in the sheet | final specification 8.3 and 8.4, against author-file 2.4 and D39 | **Conflict.** The final specification's. Every condition slot holds `{ kind = "always" }` for always, and `start_state` holds the reserved id `any_state` for any state. An absent `condition` or `start_state` is a blank (D39). `any_state` joins the ids that minting refuses (S12) and that the checker refuses as a state id (S5). A When whose condition is always is a checker error | S1a, S1b, S4, S5, S7, S12 |
| D82 | The condition "is less than ... or more than ..." in the sheet | the round B change list's B19, round C C1, final specification 8.4 and 8.10 item 2 | **Plan's choice** of the stored form: a compare phrase with `test = "outside"`, `left`, `low` and `high`, allowed in a condition slot only. It holds when `left` is below `low` or above `high`, counts as one test of its value for "otherwise", and lowers to two compares joined by an or. A blank `low` or `high` is a blank (D39) | S1a, S4, S5, S7, S12, S19 |
| D84 | Results worked out inside a When | the round B change list's B1, final specification 8.6, against author-file 2.5 | **Conflict.** The final specification's: a result worked out inside a When is open only to the lines after it inside that When, nested Whens included. A line elsewhere that uses one is a skip warning in the words of the wording table, its chip red, and the lowering leaves it and its dependents out (D39) | S5, S7, S19 |
| D110 | A number typed as a percent | round D D8 part 1, final specification 3, 8.5 and 12, against author-file 2.3 | **Conflict.** The final specification's: a number typed with "%" shows as typed. **Plan's choice** of the stored form `{ percent = 30 }`. The checker applies D75's ranges to the number divided by 100, and the lowering writes that quotient. "30%" and 0.3 lower to the same graph | S1a, S4, S5, S7, S12, S16a, S19 |

## Benchmark commands left for the maintainer

The two commands below need S11's sheet support in `henad-cli` before they can run.
The lowered Game of Life's tick pass writes the node kinds of the graph starter, a `grid.count` and a `cat.transition`, and the lowered vote's a transition of four rows where the vote starter selects with `logic.select`, so the first measurement belongs to S11 or S14, interleaved on a cooled machine as `agents/performance.md` asks:

```bash
cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 3
cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 3
```

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
