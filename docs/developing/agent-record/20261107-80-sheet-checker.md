---
date: 2026-11-07
title: "Sheet checker"
description: "S5 of the #37 redesign. henad-graph gains the sheet checker, sheet::check with SheetReport and ParsedSheet::check, which holds a sheet to every rule of the author format and words a line that cannot run yet as a skip warning in the card's words, and sheet::relations, whose CardRelations gives a card's display order, line numbers, the final editor's otherwise rule and groups of competing rate lines. The writer's comments now carry otherwise, and the 25 mutation cases of the author format each have a test."
icon: material/clipboard-check-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s5-sheet-checker, two unsigned [draft] commits and this record after 3c59165; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 1fa6ed3
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sheet checker and the otherwise rule [draft]`, `fix: s5 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet checker

> S5 gives sheets a checker and the card its "otherwise".
> `henad_graph::sheet::check` holds a sheet to every rule of the author format: ids and their namespaces, references, value types, scopes, keys, one kind of agent, and the places where rates and out-of-range chances are refused.
> A problem of the file is an error, and a problem local to one line is a skip warning: the line does not run, nor does any line that uses its result, and the warning's message is the note the card shows under the line, from the wording table ("Not run yet: choose the old state", "Uses Alive neighbours, which is below this line").
> `henad_graph::sheet::relations` holds a card's display order and the final editor's "otherwise" rule, with the reading of an inline all of that the gate adds, so the vote, the forest fire and the ants now read "otherwise (when line 2 does not apply)" in their comments.
> Each of the 25 mutation cases of the author format has a named test, placed at its table and marked error, warning or skip.

## State before

- `37-node-editor` at `1fa6ed3` (S4 merged), with S9 open in a parallel worktree.
- S1a's reader read a sheet and checked its shape, S4's writer wrote its canonical text, and `sheet::sentence` held every word the editor shows.
  Nothing checked what a sheet means: a chip naming a deleted line, a state of another property or a rate with no time scale read and wrote without a word.
- `Sentences::line` took the numbers of the higher lines that override a line, and the writer passed none, so the vote's line 3, the forest fire's line 3 and the ants' line 16 had lost their "otherwise".
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Relations between lines

`crates/henad-graph/src/sheet/relations.rs` is public as `henad_graph::sheet::relations`.
`CardRelations::new(&document, &card)` holds one card's display order, the lines inside a When or Repeat right after the block line, with each line's number from 1 and the block that holds it (`parent`, `is_within`).
An id listed twice, or inside its own block, is taken once at its first place, so a malformed card never loops.

- **Collision.** Two changes collide when both are Become or both are Set, they change one property, and for Become both name the state they change from, `any_state` included.
- **`can_both_apply(a, b)`** is false when some value is tested by both lines in ways no value passes together.
  A line's tests are its own condition, the condition of each When around it, and a Become's named start state as "my property is that state".
  An all of contributes each of its items, nested all of included, and any of, none of, a result chip, a draw and a `within` test contribute nothing (gate decision D80).
  A test reads a property of mine or a result chip, with its part or property, against fixed numbers, percents or states (a state reads as its index), the outside test of D82 included.
  Two tests exclude each other when no tried value passes both: each listed number, its neighbours, the midpoints and the ends, whole numbers only for a States property or a Count, with the page's allowance of 10^-9.
- **`otherwise_of(line)`** returns the higher colliding lines in display order, and `otherwise_numbers` their numbers for the words.
- **`competing_groups()`** returns two or more Become lines of one property from one named start state, each condition drawing exactly one rate and no chance.
  `otherwise_of` leaves out a partner in the line's own group, since the group's lines read "or" against each other (D1).

The writer now passes `otherwise_numbers` into `Sentences::line` for every line comment.

### The checker

`crates/henad-graph/src/sheet/check/` is public as `henad_graph::sheet::check`:

- `mod.rs`: `check(&document)` returns a `SheetReport` with no places, in document order, and `ParsedSheet::check` places each diagnostic at its table through `SheetPlaces` (a line's at `[card.line.l2]`) and sorts the report by place.
  `SheetDiagnostic` holds the `CodeDiagnostic`, the card index, the line id and `skips_line`, and `SheetReport` answers `has_errors`, `errors`, `warnings`, `is_skipped`, `skip_note` and `skipped`.
- `declarations.rs`: the header, the world, the time scale, ids and namespaces, properties and how they start, parameters, measures, colour rules and buttons.
- `lines.rs`: cards, lines and the values in their slots, walked in each sentence's order, so the first problem of a line words its note.
- `formula.rs`: a parser of the Calculate formula that returns its references or the reason it does not parse.

**Errors**, problems of the file, in the graph checker's message style:

- the sheet version, the id grammar and the 48-byte limit for every id (D42), the reserved ids (`my`, `my_new`, the formula and part words, `l<digits>`; in the namespace of properties, parameters, measures and events `grid_width`, `grid_height`, `num_agents`, `world_width`, `world_height`, `tick_length` and `henad_*`; `cells` and `ground` for a kind of thing; `any_state` for a state, D81), and ids held twice in a namespace;
- line ids: their form, below `next_line`, one card each, one list each, reachable from the card, and every listed id held;
- a second kind of agent, "One kind of agent per model in this version." (D43);
- properties, starts, parameter defaults, measures, colour rules, buttons and a card's kind of thing;
- a name on a change, `always` as a line or outside a condition, a When that is always (D81), the outside test outside a condition (D82), a phrase of a change kind;
- unknown parameters and formula words, a state of another property, `their` and relations outside a look, a result from another card or of a change line, a wrong part, a value of the wrong type, a formula that does not parse, a key held twice;
- Count, Look around and Pick a cell inside a Repeat or in a card that runs once;
- a rate in a card that runs once, "A button's card has no duration, so it takes chances, not rates.", a rate inside a Repeat, and a rate in a start;
- D75's ranges, in the wording table's words: a literal chance outside 0 to 1 ("A chance is from 0 to 1, or from 0% to 100%.") and a negative literal rate ("A rate cannot be below 0."), a percent read as its number divided by 100 (D110), a parameter used as a chance whose range leaves 0 to 1 ("A chance stays between 0 and 1. Set Min and Max inside that range.") and one used as a rate whose minimum is below 0.

**Warnings that keep the line running**: a draw with no key, two result lines with one name, and the final editor's empty sheet, "Every cell has the properties listed here. Add the first one to start.", on a grid sheet with no cells property.

**Skip warnings** (D39), one per line, each built from its wording table entry:

| Problem | Note |
| --- | --- |
| A blank slot or select, the first in the sentence | "Not run yet: choose a state", "Not run yet: choose the old state", "Not run yet: choose unit of time" (D112's names) |
| A result of a line no card holds | read as a blank: "Not run yet: choose a value" |
| A result at or below its user | "Uses Alive neighbours, which is below this line" |
| A result inside a When or Repeat that does not hold the user (D84) | "Uses Alive neighbours from inside line 1. Only lines inside line 1 can use it. Choose another value." |
| `my_new` at or above the last change | "Uses my new Velocity, which is below this line" |
| A result of a skipped line above | "Not run yet: Alive neighbours on line 1 is not finished" |
| A rate with no `[time]` | "Not run yet: set how long one tick is, under World on the sheet" |
| A change, When or Repeat inside a skipped block | "Not run yet: line 12 is not finished" |

A blank or unusable value outside the cards, in a start, a measure or a button, is an error.

### The sheets

The writer rewrote three comments, as S4's record foresaw:

| Sheet | Line | Comment now |
| --- | --- | --- |
| `vote.toml` | 3 | otherwise (when line 2 does not apply) become A from any state, always |
| `forest_fire.toml` | 3 | otherwise (when line 2 does not apply) become Burning from Tree, with chance Lightning chance |
| `ants.toml` | 16 | otherwise (when lines 11 and 14 do not apply) set Reward held to 0, always |

Every committed sheet checks with no diagnostic at all.

### Tests

`crates/henad-graph/src/tests/sheet_mutation.rs`, one test per case, each a committed sheet edited in the test and checked for its message, its severity, whether it skips and the table it is placed at:

| Case | Sheet | Reported as |
| --- | --- | --- |
| a result below its user | Game of Life, lines reordered | skip at `[card.line.l2]` |
| a result of a change line | Game of Life | error at `[card.line.l2]` |
| an unknown parameter | forest fire | error at `[card.line.l3]` |
| a state of another property | ants | error at `[card.line.l3]` |
| "my new" above the last change | boids, lines reordered | skip at `[card.line.l7]` |
| a missing key | forest fire | warning at `[card.line.l3]` |
| a duplicate key | forest fire | error at `[card.line.l5]` |
| an unlisted line | Game of Life | error at `[card.line.l3]` |
| a line past `next_line` | Game of Life | error at `[card.line.l4]` |
| a rate with no `[time]` | SIR | skip at `[card.line.l2]` and `[card.line.l3]` |
| a number in a condition | Game of Life | error at `[card.line.l4]` |
| a Count in a button card | Game of Life | error at `[card.line.l1]` |
| a result from another card | Game of Life | error at `[card.line.l4]` |
| `their` outside a look | Game of Life | error at `[card.line.l2]` |
| an unknown formula word | ants | error at `[card.line.l3]` |
| a formula syntax error | ants | error at `[card.line.l3]` |
| a part of the wrong line | boids | error at `[card.line.l3]` |
| Leave on an agent property | ants | error at `[card.line.l3]` |
| Become on a Number | ants | error at `[card.line.l10]` |
| two parameters with one id | SIR | error at the second `[[parameter]]` |
| a parameter id of the line form | SIR | error at its `[[parameter]]` |
| a default outside its range | Game of Life | error at its `[[parameter]]` |
| an unknown state in a measure | Game of Life | error at its `[[measure]]` |
| a button running a tick card | Game of Life | error at its `[[button]]` |
| a Count inside a Repeat | random walk | error at `[card.line.l2]` |

Beyond the 25: `a_rate_in_a_run_once_card_is_an_error`, `a_chance_above_1_and_a_negative_rate_are_errors` (1.5, -0.1, 150%, a negative rate and a negative percent rate refused, 0, 1, 30% and 100% clean, 130% refused in a start) and `a_chance_parameter_ranging_past_1_is_an_error` (and a rate parameter with a negative minimum).
`the_sheet_checker_never_panics_on_the_mutation_corpus` runs the checker, both report forms, the relations of every card and the writer over the reader's mutation corpus and every sheet with each key deleted in turn.

`crates/henad-graph/src/tests/sheet_check.rs`:

| Test | What it holds |
| --- | --- |
| `every_sheet_checks_clean` | no diagnostic for any committed sheet, placed or not |
| `the_report_is_in_file_order_with_each_line_problem_at_its_table` | the sort by place, line problems at their headers, `skipped`, `warnings` |
| `otherwise_names_its_partner_and_skips_exclusive_tests` | none between "is 3" and "is none of the numbers 2, 3", "at most 4" and "at least 5", "less than 2" and "more than 3", "is one of the numbers 2, 3" and "is 4"; "otherwise (when line 3 does not apply)" between "is 3" and "at least 3", "is 3" and "is one of the numbers 2, 3", "at most 4" and "at least 4", "is not 3" and "is not 4" |
| `the_starters_read_otherwise_as_the_final_words_give_them` | Game of Life with none, the vote's line 3 with line 2 |
| `the_forest_fire_lightning_line_reads_otherwise_against_the_spread_line` | line 3 against line 2, and its sentence |
| `the_ants_set_lines_follow_the_recorded_set_rule` | line 14 with none, line 16 with lines 11 and 14, line 13 with none, and line 14 against line 11 once its When's all of becomes an any of |
| `two_rate_exits_form_one_group` | SIR with Recovered and Dead from Infected at rates: one group, no "otherwise" |
| `a_chance_line_beside_a_rate_line_keeps_otherwise` | the same with a chance on line 2: no group, "otherwise (when line 2 does not apply)" |
| `a_line_reading_a_skipped_line_is_skipped` | a Count with no state skips both Become lines of Game of Life |
| `a_line_inside_a_skipped_when_is_skipped` | the ants' When on line 12 with no condition skips lines 13 to 15 and not 16 |
| `a_result_inside_a_when_is_open_only_to_the_lines_inside_it` | D84 |
| `every_skip_note_equals_its_wording_table_entry` | each kind of skip against its `sheet::sentence` entry and the gate's literal words |
| `the_outside_test_stands_only_in_a_condition_and_counts_for_otherwise` | D82 |
| `a_when_that_is_always_and_the_reserved_ids_are_errors` | D81 and D42 |
| `a_second_agent_kind_is_refused` | D43, placed at the second `[[agents]]` |
| `a_grid_sheet_with_no_property_gets_the_empty_state_warning` | the warning alone, also with an empty card |

The formula parser has two inline tests of its own, and `every_line_sentence_equals_its_comment` now passes each line's "otherwise" numbers.

### Docs and topic files

- `agents/henad-graph-sheet.md`: the module list, a paragraph on the relations, one on the checker, the writer's comments with "otherwise", and the test files. 21,445 characters after the change.
- `AGENTS.md`: the sheet topic file's row names the writer, wording and checker.
- `crates/henad-graph/src/sheet/mod.rs`: the module doc names the checker and the relations.
- One CHANGELOG line under Unreleased.
- `crates/henad/tests/facade_paths.rs`: `SheetReport`, `SheetDiagnostic` and `CardRelations` through the facade.

### Edited tree

```
AGENTS.md                                        sheet topic row
CHANGELOG.md                                     sheet checker and relations
agents/
└── henad-graph-sheet.md                         relations, checker, writer comments, tests
crates/
├── henad/tests/facade_paths.rs                  SheetReport, SheetDiagnostic, CardRelations
└── henad-graph/
    ├── sheets/
    │   └── vote.toml                            line 3 reads otherwise
    └── src/
        ├── sheet/
        │   ├── mod.rs                           pub mod check, pub mod relations, module doc
        │   ├── relations.rs                     new: CardRelations, the otherwise rule, competing groups
        │   ├── writer.rs                        comments carry otherwise
        │   └── check/                           new
        │       ├── mod.rs                       check, SheetReport, SheetDiagnostic, ParsedSheet::check
        │       ├── declarations.rs              tables outside the cards
        │       ├── lines.rs                     cards, lines, values, skip notes
        │       └── formula.rs                   Calculate formula parser
        └── tests/
            ├── mod.rs                           sheet_check, sheet_mutation
            ├── sheet_check.rs                   new
            ├── sheet_mutation.rs                new
            ├── sheet_writer.rs                  comments with otherwise
            └── sheets/
                ├── ants.toml                    line 16 reads otherwise
                └── forest_fire.toml             line 3 reads otherwise
```

### Deviations from the plan

- **`sheet/check/` is a directory** of four files, as `check/` is for graphs, instead of one `check.rs`, which would have held about 2,700 lines.
- **Only skip notes and the plan's named errors take the wording table's words.** The file-level errors are technical messages in the graph checker's style ("the parameter 'lightning' does not exist"), since no wording table row covers them and only the CLI and a hand-written file meet them.
- **"my new" above the last change** has no words in the final specification. Its note reads `uses_result_below("my new Velocity")`, "Uses my new Velocity, which is below this line", and a `my_new` of a property no line changes reads as a blank.
- **Agent-only fields with no editor words yet** (a Steer or Move mode, a pick rule, `which`, an event, `agent_kind`, `include_me`) are errors when absent instead of skip warnings, as the author format's checker has them. Slots and the fields the card already words (property, states, `around`, `test`, `rate_unit`, `ranked_by`, `ground_property`) are skip warnings.
- **A result line with no name** is no error, which the author format's checker made one. A note names such a result by its sentence, and the editor's automatic names fill every name.
- **A result inside a Repeat** takes the When scope of D84 and its note, a skip warning, where the author format's checker gave an error.
- **A rate in a start** gets its own error beside the type error a rate phrase in a number slot already gets.
- **A card over `cells` is allowed in a grid world with no `[cells]` table**, so a blank model's empty card is no error.
- **The competing "or"** is computed (`competing_groups`, and `otherwise_of` leaves group partners out), and the writer writes no "or" in the comment of a group's lower line. No committed sheet has a group, and the card's words for one come with rates in the card.
- **The ants test asserts the gate's reading alone.** The plan offered two outcomes, one for "For Set it is always", which the final specification no longer says.
- **The "otherwise" test puts the higher change on line 3**, by adding a result line to Game of Life, so its strings are the gate's "otherwise (when line 3 does not apply)".

## State after

- A sheet read from its text, or a document built in memory, can be checked, and every problem is placed at its table and tied to its card and line.
- The skipped lines and their notes are known before any lowering, and `CardRelations` gives the order, the numbers and "otherwise" that the lowering and the card need.
- Every committed sheet checks clean and is a fixed point of the writer, its comments now carrying "otherwise".
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph -p henad --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph --all-targets --all-features` (279 passed, 1 ignored); `cargo test -p henad-graph --doc` (5 passed); `cargo test -p henad --all-features --test facade_paths` (4 passed); `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features`; `cargo check -p henad-graph --all-features --lib --target wasm32-unknown-unknown` and the same for the facade.
  The full check runs at the merge point.
- The merge check rebased the branch onto `37-node-editor` at `3c59165`, which had gained S9's time axis (record #79) since `1fa6ed3`.
  The rebase met no conflict, and S9 changed no file of henad-graph.
- The full check of plan section 2.3 ran on the rebased branch at `8ae998d`, with `HENAD_REQUIRE_GPU=1`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models with the baseline's message (`boids.toml: 14816908000000 units in one submission`), and its log shows no other test binary that failed or aborted.
  The henad-graph tests pass in that run, 283 with 1 ignored.
  No baseline test now passes, and the merge check needed no fix.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.

## Issues found & future directions

1. S7's lowering takes its row order from `CardRelations::otherwise_of` and leaves out the lines `SheetReport::skipped` names, so the card's word and the lowered order agree.
2. The card (S13b) draws `skip_note` under each line and the CLI (S11) prints every diagnostic with its place.
3. The card draws a competing group's "or", its tooltip and its group note with rates in the card (S25), and the writer's comment for a group's lower line could take the same word then.
4. Agent line words (S28) let the absent agent-only fields become skip warnings in the card's words.
5. Each check builds a `Sentences` for the whole document and a `CardRelations` per card. No check was timed, and an editor that checks on every edit may want to time one on the largest sheet the limits allow.
6. The "otherwise" rule tries values with an allowance of 10^-9, as the page does. The lowering's f32 comparisons have no allowance, which the gate notes leaves counts unchanged.

## Review

The review returned one must-fix, two should-fixes and five nits, and all eight held.
The Calculate formula parser had no depth limit, and a formula of 50,000 nested parentheses, 100 KB inside the 1 MiB file limit, overflowed the stack and ended the process.
`MAX_FORMULA_NESTING` in `sheet/limits.rs` now caps parentheses, calls and the right sides of `^` at 32 groups, and a deeper formula is the error "the formula nests groups more than 32 deep".
An inline test parses 100,000 open parentheses, 100,000 open calls and a chain of 100,000 powers, each refused, and 100,000 groups side by side, accepted.
S7's lowering parses the same formulas and should reuse this parser.
A result line inside a When inside a skipped Repeat ran, since only the direct parent was consulted.
Any line with a skipped Repeat among its blocks, at any depth, is now skipped with that Repeat's note, and a result inside a skipped When still runs.
`every_line_inside_a_skipped_repeat_is_skipped_at_any_depth` holds both on the Random walk with a When and a Calculate added inside its Repeat.
The ids of a Look around's summaries were checked for duplicates alone.
They now meet the id grammar, the 48-byte limit, and the reserved words and `l<digits>`, which keeps a summary named `count` from being shadowed by the built-in part, and `a_summary_id_follows_the_id_rules` holds the four cases.
Of the nits, the "otherwise" rule now tries a Look around's `count` part and a Count summary at whole numbers, as it does a Count line, and `a_count_part_or_summary_is_tried_at_whole_numbers` holds it.
The field doc and the topic file describe the order of a report without a text as a stable order, which lists the problems outside the cards first.
The topic file's final newline is back, the color rule messages spell "color", and a test no longer assigns a field twice.
After the fixes: fmt, clippy over henad-graph and the facade, the henad-graph tests with `HENAD_REQUIRE_GPU=1` (283 passed, 1 ignored), its doc tests, rustdoc, the facade path test and the wasm32 typecheck of henad-graph pass.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D1 | Competing rate changes out of one state | the rates design, section 10, question 1 | (a) Competing risks, written "or" with a bracket, an exception to answer 2a | S5, S21, S25 |
| D15 | The "otherwise" rule for Set | the author-file design, section 7, question 9 | (a) Pinned states exclude, extended by the round A change list's F10 exclusive tests. D32 takes precedence: this row holds only if the Set rule the gate quotes from the final specification agrees, and today's specification says "For Set it is always", in which case every later Set of a property reads "otherwise" and this row is void | S5 |
| D32 | Which "otherwise" rule | the author-file design, section 2.4, against the specification and the round A change list's F10 | **Conflict.** The final specification's rule, quoted in the gate by S0, implemented once in `sheet/relations.rs` and used by the lowering's row order. It takes precedence over D15, and S5's ants test follows it | S5 |
| D39 | How a half-built line is stored and run | both drafts (P1) | An absent slot key, read as blank and written as nothing. Line-local problems are skip warnings in the final specification's words, and the run skips the line and every line that reads it. File-level problems are errors | S1a, S5, S7 |
| D42 | Sheet ids and reserved ids | the recon of henad-graph | Sheet ids at most 48 bytes, and the IR's reserved parameter ids plus `tick_length` refused by the sheet checker | S1a, S5 |
| D43 | More than one agent kind | the recon of henad-graph | Refused, since `AgentsWorld` holds one population | S5, S17 |
| D75 | Input rules for chances and rates | the prototype's notes ("Typed chances must lie from 0 to 1 and rates may not be negative") | Built in the editor's fields and, for a hand-written file, as checker errors | S5, S16a, S19 |

From the gate, the rows whose "Built in" names S5, with the final specification named so and change lists named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D80 | The "otherwise" rule for Set, and tests inside an all of | final specification 8.7, with D15 and D32 | The final rule holds for Set as for Become: two Sets of one property collide unless some value is tested by both in ways no value passes together. "For Set it is always" is gone, so D15 holds and is not void. **Plan's choice** for a case the prototype cannot express: an inline all of in a change's condition or in an enclosing When contributes each of its items as a test the change needs, nested all of included, and any of, none of and a result chip contribute nothing. The ants' line 14 then reads no "otherwise", and line 16 reads "otherwise (when lines 11 and 14 do not apply)" | S5 |
| D81 | "always" and "any state" in the sheet | final specification 8.3 and 8.4, against author-file 2.4 and D39 | **Conflict.** The final specification's. Every condition slot holds `{ kind = "always" }` for always, and `start_state` holds the reserved id `any_state` for any state. An absent `condition` or `start_state` is a blank (D39). `any_state` joins the ids that minting refuses (S12) and that the checker refuses as a state id (S5). A When whose condition is always is a checker error | S1a, S1b, S4, S5, S7, S12 |
| D82 | The condition "is less than ... or more than ..." in the sheet | the round B change list's B19, round C C1, final specification 8.4 and 8.10 item 2 | **Plan's choice** of the stored form: a compare phrase with `test = "outside"`, `left`, `low` and `high`, allowed in a condition slot only. It holds when `left` is below `low` or above `high`, counts as one test of its value for "otherwise", and lowers to two compares joined by an or. A blank `low` or `high` is a blank (D39) | S1a, S4, S5, S7, S12, S19 |
| D84 | Results worked out inside a When | the round B change list's B1, final specification 8.6, against author-file 2.5 | **Conflict.** The final specification's: a result worked out inside a When is open only to the lines after it inside that When, nested Whens included. A line elsewhere that uses one is a skip warning in the words of the wording table, its chip red, and the lowering leaves it and its dependents out (D39) | S5, S7, S19 |
| D110 | A number typed as a percent | round D D8 part 1, final specification 3, 8.5 and 12, against author-file 2.3 | **Conflict.** The final specification's: a number typed with "%" shows as typed. **Plan's choice** of the stored form `{ percent = 30 }`. The checker applies D75's ranges to the number divided by 100, and the lowering writes that quotient | S1a, S4, S5, S7, S12, S16a, S19 |

## Benchmark commands left for the maintainer

None.
S5 changes no engine, no lowering and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
