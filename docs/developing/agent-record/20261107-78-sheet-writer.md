---
date: 2026-11-07
title: "Sheet writer"
description: "S4 of the #37 redesign. henad-graph gains the canonical sheet writer, SheetDocument::to_text with its own number formatter and a generated comment above each card and line, and the wording table sheet::sentence, which holds every word the sheet editor shows in the final editor's wording and writes each line's sentence. Every committed sheet is now the writer's output, and four golden files hold the reference writer's text with the procedure that made them."
icon: material/file-document-edit-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s4-sheet-writer, five unsigned [draft] commits and this record after e5d5a67; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 35b5735
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sheet wording table and sentences [draft]`, `feat: canonical sheet writer [draft]`, `fix: s4 comment wording [draft]`, `fix: s4 review findings [draft]`, `fix: s4 merge check [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet writer

> S4 gives sheets a canonical text and the card its words.
> `SheetDocument::to_text` writes the one text of each document, and reading that text returns the same document.
> It follows the author format's canonical writing: the order of the sheet, a fixed key order per table, line tables in line id order, inline values and standard sub-tables by the schema, a number rule with its own formatter, and colours in upper case.
> A generated comment above each card holds its heading, and one above each line table holds the line's sentence.
> The sentences, and every other word the sheet editor shows, come from one wording table, `henad_graph::sheet::sentence`, built from the final editor's wording table.
> Every sheet committed by S1a and S1b is now the writer's output, and its comments read in the final words: "Become Alive from Dead", "is none of the numbers 2, 3", "for each infected neighbour".
> Four golden files hold the author format's reference writer's text for its four examples, without comments, and the Rust writer equals them byte for byte.
> "otherwise" is in the table but in no comment yet, since deciding it is S5's.

## State before

- `37-node-editor` at `dfab1fb` when the milestone started, rebased onto `35b5735` (S15 merged) before the record.
  S1a's reader read sheets into `SheetDocument`, and S1b's nine sheets (two starters, seven test sheets) held comments in the wording before the final editor ("Become Alive if Dead", "Clear (button), every cell once").
- No Rust code wrote a sheet.
  The four sheets copied from the author format's examples were canonical by the reference writer, a Python script outside the repository, and S1b had made its four sheets canonical with the same script.
- The graph writer held private text helpers (`key`, `color`, `strings`, `multiline_array`).
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### The wording table

`crates/henad-graph/src/sheet/sentence.rs` is public as `henad_graph::sheet::sentence`.
It holds every word the sheet editor shows, from the final editor's wording table, section by section:

- the card's frame: the header, the line count, the law, the add buttons with their tooltips and menu headings, the add-inside row, the line tools, the button card's heading and empty text, the list names, the blank model's card;
- the comparisons in the select's order, with "is one of the states" and "is none of the states" for a States property on the left, and the outside test;
- the conditions: "always" with its name and tooltip, the condition words on a change and on a When line, the arrow's name and tooltip, "Choose when" with its lead, groups, items and their grey lines, and the "otherwise" item;
- the blanks: an enum `Blank` with the visible word, what it wants, the accessible name and the picker title, and `LineSelect` for the selects inside a line, and the other line controls;
- the pickers: the Number box, the groups, the list blank, the "for each" picker, every empty-group message, the typed-number messages and the colour names;
- the add menus: `AddButton` and `AddItem`, each item's label and pattern, the Become patterns taking the model's states or "a new state" and "an old state";
- the result names' words;
- the notes under a line, rows 1 to 12, as templates taking line numbers and names;
- "otherwise": the words, the tooltip and the notice;
- the rate words: "Each tick (1 day)", the subjects, the singular count, the unit notice, and every rate and chance note as a template taking numbers already formatted, the worked-out rate and the chance with "for each" of round D included;
- the plan's own words for decisions the final editor lacks: a rate in a button's card, one agent kind, competing rates with their group note, the long tick, the mean stay, rate and chance conversion, the restart notice and the Integer kind;
- the Lines help with its example, and the forbidden words;
- around the card: the sheet (`SheetSection` with each heading, help text and example, World, Time, the Cells table, the property form and its checks, Starts as with its fields and "Use a parameter", Changed by, the blank model's sheet, Parameters, Measures, Colours, Buttons), the New model and New parameter dialogs, the top bar, the world tabs (tick counter, tried values, the empty plot and the blank model's world), the Diagram and the conversion to a node graph.
  Rows the wording table marks "Not built" are left out.

Fixed text is a constant and a template is a function taking what it names: a line number, the line word ("line 2", "Clear line 1") or the line prefix ("Line 2", "Clear line 1").

`Sentences` writes a card's heading and a line's sentence from a `SheetDocument`.
`line(line_id, otherwise)` takes the numbers of the higher lines that override the line, so the "otherwise" words come from the caller.
The sentences follow the final editor where a line kind is shared, and round 1's words for agent lines in the same conventions:

| Before | After |
| --- | --- |
| Become Alive if Dead | Become Alive from Dead |
| Become Carrying food | Become Carrying food from any state, always |
| when Alive neighbours is not one of 2, 3 | when Alive neighbours is none of the numbers 2, 3 |
| the 3 by 3 block, me included | the 3 by 3 block including me |
| persons within Hearing range, me included | persons within Hearing range including me |
| for each of Infected neighbours | for each infected neighbour |
| when I am Searching | when my Cargo is Searching |
| Trail falloff × Home way's amount + my Reward held | Trail falloff times Home way's amount plus my Reward held |
| Clear (button), every cell once | Lines of Clear: run once, every cell, when pressed |

A blank reads `...`, the prototype's plain-text form, and an unknown reference reads as its id.
A Compare line hides its name while the name equals its sentence.
A percent reads as typed ("30%").
A state that another States property also names reads "Status Alive".

### The canonical writer

`crates/henad-graph/src/sheet/writer.rs` holds `SheetDocument::to_text`, `SheetWriteError` and `format_number`.

- The writer builds a tree of records in the author format's key order and writes it as the reference writer does: inline keys before sub-tables, one blank line before each header, no header for a table with no inline key that holds tables.
- Line tables follow line id order by number, colour rules follow thing order (cells, agent kinds, ground), `branches` follows the property's state order.
- An absent field and a blank slot write nothing (D39's writing half), `rate_unit_from_tick` only when true (D83), a percent as `{ percent = N }` (D110), `any_state` and `{ kind = "always" }` as stored (D81), `test = "outside"` with `low` and `high` (D82).
- A phrase in a slot is a standard sub-table, in `items` an array of tables, and an inline table where TOML allows nothing else.
- `format_number` takes the shortest round-trip digits from Rust's `{:e}` and lays them out by hand: an integer within 2^53, plain notation from 10^-4 below 10^16, `1e-5` style outside, `-0` as `0`, and `None` for NaN and the infinities, which `to_text` reports as a `SheetWriteError` naming the key.
- A key TOML cannot read bare is quoted, and a control character in a comment becomes a space.

`model_text.rs` now holds the text helpers both writers share: `push_key`, `string_array`, `multiline_array` and `color_array`, moved from the graph writer and renamed, and the sheet's `color_hex`.

### The sheets

Every sheet S1a and S1b committed was read and written back by the Rust writer.
Only comments changed: the two starters and six of the seven test sheets, `random_walk.toml` already reading in the final words.
The golden files under `crates/henad-graph/src/tests/golden/` hold the reference writer's output for Game of Life, SIR, boids and ants, run on scratch copies of the sheets at `dfab1fb`, with comment lines removed.
`golden/README.md` holds the script's SHA-256, the commands, the `grep -v '^#'` and the rules the files follow, written out in full, and lists where the Rust writer goes further than the script.

### Tests

| Test | What it holds |
| --- | --- |
| `every_wording_table_entry_equals_the_final_words` | every built row of the final wording table, as literals copied in, against the module's text, each template called with the table's example |
| `the_game_of_life_starter_reads_the_final_sentences` | Game of Life's three sentences and Clear's, exactly as the final editor gives them, and both card headings |
| `the_starters_read_the_final_sentences_with_their_otherwise` | SIR's, the vote's and the forest fire's sentences as the final editor gives them, with "otherwise (when line 2 does not apply)" passed in |
| `agent_lines_read_round_1_words_in_the_final_conventions` | ants, boids, bounded confidence and random walk lines |
| `a_total_or_average_line_reads_over_its_cells` | a grid Total or average line, stored as a Look around over cells with one summary, reads "average of Food over the 8 cells around me", in its sentence and in its comment |
| `a_count_reads_in_the_singular_after_for_each` | the singular rule, its "of" form and its capitals |
| `no_fixed_wording_entry_holds_a_forbidden_word` | the fixed entries, patterns, names, grey lines, help texts and starter descriptions, the conversion words aside |
| `every_sheet_is_a_fixed_point` | read, write, byte for byte, over every committed sheet |
| `the_rust_writer_agrees_with_check_py_apart_from_comments` | the four golden files |
| `a_hand_edited_sheet_reads_back_to_its_canonical_text` | Game of Life with keys out of order, inline and standard tables swapped, `1_024`, `+3.0`, `3e-1`, `-0.0`, a lower-case colour with `ff` alpha, `\r\n` line ends and the author's comments |
| `unusual_values_read_back_to_the_same_document` | empty `[cells]` and `[ground]`, alpha, a quoted line id, mixed items, branches holding a phrase, a summary holding a phrase |
| `a_number_that_is_not_finite_is_refused` | NaN and an infinity, each named by its key |
| `numbers_follow_the_sheet_number_rule` | `1024`, `1`, `0.3`, `0.0005`, `1e-5`, `2.5e-7`, `-0` as `0`, 2^53, `1e16`, and the refusals |
| `every_line_sentence_equals_its_comment` | every card and line comment of every committed sheet against `Sentences` |
| `a_new_waning_line_changes_four_hunks`, `a_steer_reorder_changes_one_line`, `a_parameter_rename_changes_its_name_and_the_comment_that_shows_it` | the author format's three typical edits and their diffs, by editing the document, through a line diff in the test |
| `every_readable_mutation_writes_text_that_reads_back` | the reader's mutation corpus: every edit that reads writes, reads back to the same document, and is a fixed point |

### Docs and topic files

- `agents/henad-graph-sheet.md`: the module list, a paragraph on the writer, two on the wording table and `Sentences`, the Tests paragraph, and the test sheets' text now the writer's.
- `agents/henad-graph.md`: the shared helpers in `model_text.rs`.
- `crates/henad-graph/src/sheet/mod.rs`: the module doc names the writer and the doc test writes and reads back.
- One CHANGELOG line under Unreleased.
- `crates/henad/tests/facade_paths.rs`: `SheetWriteError` and `Sentences` through the facade.

Sizes after the change: `agents/henad-graph-sheet.md` 15,433 characters, `agents/henad-graph.md` 30,112.

### Edited tree

```
CHANGELOG.md                                     sheet writer and wording table
agents/
├── henad-graph-sheet.md                         writer, wording table, Sentences, tests
└── henad-graph.md                               shared text helpers
crates/
├── henad/tests/facade_paths.rs                  SheetWriteError, Sentences
└── henad-graph/
    ├── sheets/
    │   ├── game_of_life.toml                    rewritten by the writer
    │   └── vote.toml                            rewritten by the writer
    └── src/
        ├── model_text.rs                        push_key, string_array, multiline_array, color_array, color_hex
        ├── writer.rs                            helpers moved out
        ├── sheet/
        │   ├── mod.rs                           pub mod sentence, mod writer, re-exports
        │   ├── sentence.rs                      new: the wording table and Sentences
        │   └── writer.rs                        new: to_text, SheetWriteError, format_number
        └── tests/
            ├── mod.rs                           sheet_sentence, sheet_writer
            ├── sheet_document.rs                mutate and below shared
            ├── sheet_sentence.rs                new
            ├── sheet_writer.rs                  new
            ├── golden/                          new
            │   ├── README.md                    the procedure and the rules
            │   ├── ants.toml
            │   ├── boids.toml
            │   ├── game_of_life.toml
            │   └── sir.toml
            └── sheets/
                ├── ants.toml                    rewritten by the writer
                ├── boids.toml                   rewritten by the writer
                ├── bounded_confidence.toml      rewritten by the writer
                ├── forest_fire.toml             rewritten by the writer
                ├── sir.toml                     rewritten by the writer
                └── sir_chance.toml              rewritten by the writer
```

### Deviations from the plan

- **No "otherwise" in the comments.** S5 builds the rule in `relations.rs`, so the writer passes no partner and the vote's line 3, the forest fire's line 3 and the ants' line 16 lost the "Otherwise" their old comments held. The words exist (`otherwise_words`), `Sentences::line` takes the partners, and a test reads the vote and the forest fire with them passed in. S5 passes `relations::otherwise_of` into the writer's call, and `every_sheet_is_a_fixed_point` then asks it to rewrite those three comments.
- **The wording table leaves out rows the gate marks "Not built"**: the run bar, speed and run to tick, the pauses, the world notes after the first tick and the facilitator. The banner and link line of the conversion take words from the expert design that the gate does not quote, so S27 adds them. The number rule of the rate notes stays S21's, and the note templates take numbers already formatted. The agent starters' descriptions are S26's.
- **The wording test is named `every_wording_table_entry_equals_the_final_words`** and not after the gate's file, since tracked files name no gitignored file.
- **The golden files come from the repository's sheets at `dfab1fb`**, not from the design folder's examples. S1a changed those copies (D81, D97, D98), so the Rust writer could not reproduce the originals. The reference writer left their tables unchanged, so the golden files differ from the committed text in comments alone.
- **Two test files.** `sheet_sentence.rs` holds the wording table's tests beside the plan's `sheet_writer.rs`.
- **The moved helpers are renamed** under AGENTS.md's naming rule: `key` to `push_key`, `strings` to `string_array`, `color` to `color_array`. `color_hex` is new, since the sheet writes colours as `"#RRGGBB"` and the graph as four bytes.
- **Tests beyond the plan.** `unusual_values_read_back_to_the_same_document`, `a_number_that_is_not_finite_is_refused`, `every_readable_mutation_writes_text_that_reads_back`, `agent_lines_read_round_1_words_in_the_final_conventions`, `a_count_reads_in_the_singular_after_for_each` and `no_fixed_wording_entry_holds_a_forbidden_word`.
- **Where the Rust writer departs from the reference writer**, in forms no example holds: a list of chips stays on one line (the script spreads it over lines), `items` holding a literal is one inline array, a key TOML cannot read bare is quoted, and a control character in a comment becomes a space. `golden/README.md` lists them.
- **Word choices the final editor leaves open.** Agent neighbourhoods read "including me", as the 3 by 3 block does. A Calculate formula reads its operators as the final Calculate line's words, "to the power of" for `^`, with a call's parentheses left out. A When whose condition is always reads "When always:", which S5 makes an error.
- **Rebased in the worktree.** S15 merged during the milestone. The rebase onto `35b5735` met two list conflicts in `tests/mod.rs` and one in the graph writer, where S15's `resolution` key met the helper rename.
- **A third commit**, `fix: s4 comment wording [draft]`, rewords five comments after a self-review, and a fourth, `fix: s4 review findings [draft]`, holds the review's fixes.
- **The record is not committed here.** The orchestration asks for it in the design folder, numbered and committed at the merge step.

## State after

- `redesign/s4-sheet-writer` holds four commits after `35b5735`.
- A sheet reads and writes: `ParsedSheet::parse(text)?.document().to_text()` returns `text` for every committed sheet, and a hand-written sheet takes its canonical text.
- `henad_graph::sheet::sentence` holds the sheet editor's words, and `Sentences` writes each line's sentence in them.
- Checks run in the worktree on the rebased branch: `cargo fmt --all -- --check`, `cargo clippy -p henad-graph -p henad --all-targets --all-features -- -D warnings -W clippy::all`, `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph --all-targets --all-features` (225 passed, 1 ignored), `cargo test -p henad-graph --doc`, `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features`, the facade's `facade_paths` test, `./scripts/check_packaging.sh`, and `cargo check --lib --target wasm32-unknown-unknown` for henad-graph with every feature and for the facade with and without them.
  The full check of section 2.3 runs at the merge point.
- The merge check rebased the branch onto `37-node-editor` at `e5d5a67`, which had gained S2's time scale (record #77) since `35b5735`.
  The rebase met no textual conflict, but the build broke on one semantic conflict.
  S2's graph writer writes the tick length's `tick_unit` key through the old `key` helper, which S4 renames to `push_key` in `model_text.rs`.
  `fix: s4 merge check [draft]` renames that one call.
  S2's hidden flag and `tick_unit` key live in the graph file's `[[param]]` table, and the sheet keeps its own `[time]` table, which the sheet writer already writes, so nothing else of S2 reaches the sheet writer.
- The full check of plan section 2.3 ran on the rebased branch at `a304761`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models with the baseline's message (`boids.toml: 14816908000000 units in one submission`), and its log shows no other test binary that failed or aborted.
  The sheet writer and sentence tests of this milestone pass in that run.
  No baseline test now passes.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.

## Issues found & future directions

1. S5 wires `relations::otherwise_of` into the writer's comments, and its fixed-point test then rewrites the vote's, the forest fire's and the ants' "otherwise" lines.
2. S21 adds the number rule and the choice of note to the rate-note templates, S26 the agent starters' descriptions, and S27 the conversion banner and link line.
3. A result line with no name reads as its phrase alone. S12's automatic names fill the name, and a sheet that reaches the writer through the editor always holds one.
4. `sentence.rs` holds 3,125 lines, the table and the renderer together. Splitting `Sentences` into a file of its own would keep each half readable once S5 to S28 grow both.
5. The reference writer spreads a list of chips over lines and the Rust writer keeps it on one line. No example holds one, so nothing compares them. If the maintainer keeps the script as the reference, one of the two should change.
6. The hand-lowered fixtures of S1b (`crates/henad-models/tests/fixtures/lowered/round1/` and their procedure) keep round 1's words in their comments, such as "Become Alive if Dead". They are records of round 1's design and never reach a researcher, so S4 leaves them.

## Review

The review returned one must-fix, three should-fixes and two nits, and all six held.
The wording table had stopped at the card, while the gate gives S4 every row of its wording table and every row round D changed.
It now holds the rows of the sheet, the two dialogs, the top bar, the world tabs, the help popovers, the Diagram and the conversion, the rate and chance notes as templates (the worked-out rate and the chance with "for each" of round D among them), and the remaining words the plan adds, each with its entry in `every_wording_table_entry_equals_the_final_words`.
S5's warning for a grid sheet with no cells property can take `SHEET_EMPTY`.
A grid Total or average line, stored as a Look around over cells with one total or average summary, read in the agent form ("look at the 8 cells around me: average of their Food").
It now reads "average of Food over the 8 cells around me", and `a_total_or_average_line_reads_over_its_cells` holds the sentence and the writer's comment.
The ants test passed line 14 alone as the partner of line 16, and now passes lines 11 and 14, as the gate's Set rule gives them.
The record's sources no longer cite design files by path.
The topic files no longer name the reference script and stay within their wrap, the sentence module's group comment names its contents, the writer's module doc lost its parallel frame, and the CHANGELOG line lost its design jargon.
After the fixes: fmt, clippy over henad-graph and the facade, the henad-graph tests with `HENAD_REQUIRE_GPU=1` (225 passed, 1 ignored), its doc tests, rustdoc, the facade path test and the wasm32 typecheck of henad-graph pass.

## Decisions adopted for review

From the plan's section 6, as written there:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D11 | Generated comments | the author-file design, section 7, question 5 | (a) Each line's sentence above its table | S4 |
| D39 | How a half-built line is stored and run | both drafts (P1) | An absent slot key, read as blank and written as nothing. Line-local problems are skip warnings in the final specification's words, and the run skips the line and every line that reads it. File-level problems are errors | S1a, S5, S7 (S4 writes the blank as nothing) |

From the gate, the rows whose "Built in" names S4, as written there with the final specification named so:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D81 | "always" and "any state" in the sheet | final specification 8.3 and 8.4 against author-file 2.4 and D39 | **Conflict.** The final specification's. Every condition slot holds `{ kind = "always" }` for always, and `start_state` holds the reserved id `any_state` for any state. An absent `condition` or `start_state` is a blank (D39) | S1a, S1b, S4, S5, S7, S12 |
| D82 | The condition "is less than ... or more than ..." in the sheet | the round B change list's B19, round C C1, final specification 8.4 and 8.10 item 2 | **Plan's choice** of the stored form: a compare phrase with `test = "outside"`, `left`, `low` and `high`, in a condition slot only | S1a, S4, S5, S7, S12, S19 |
| D83 | A rate's unit taken from the tick | round C C5, final specification 8.3, 8.9 and 12 | **Plan's choice** of the key: `rate_unit_from_tick = true` beside `rate_unit` while the unit came from the tick, and nothing otherwise | S1a, S4, S12, S25 |
| D90 | The arrow form of Become | the round B change list's B8 and round C's C9 | Kept as the final specification builds it: "Become [state] from [old state]", the empty old-state blank reading "old state". The arrow form is not built | S4, S19 |
| D110 | A number typed as a percent | round D D8 part 1, final specification 3, 8.5 and 12, against author-file 2.3 | **Conflict.** The final specification's: a number typed with "%" shows as typed. **Plan's choice** of the stored form: `{ percent = 30 }`, its number written under author-file 4.2's number rule | S1a, S4, S5, S7, S12, S16a, S19 |

## Benchmark commands left for the maintainer

None.
S4 changes no engine, no lowering and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
