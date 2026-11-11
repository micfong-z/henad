---
date: 2026-11-07
title: "Grid line kinds"
description: "S10 of the #37 redesign. The grid lowering gains every remaining line kind of a cells card: chances, once for each of a count through the power rule, Look around over the cells with every summary, Calculate as one expression node, Choose, Depending on and Random number. S7's refusals of these kinds are lifted, each with a test, the forest fire becomes the third sheet starter, and the chance SIR, the forest fire, Game of Life and Majority vote lower, check with no warning and compile."
icon: material/dice-multiple-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s10-grid-line-kinds, rebased onto S11's record 03b34fa: two unsigned [draft] commits, a docs commit and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: cf3ce8c
delta_state: "committed on `37-node-editor` by fast-forward: `feat: lower chances, looks and choices in grid sheets [draft]`, `fix: s10 review findings [draft]`, `docs: formula depth limit of a sheet [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Grid line kinds

> S10 lowers the rest of a cells card.
> A chance is one `rand.chance` keyed by its line, and a chance once for each of a count holds with 1 − (1 − p)^n, through `math.powi` with `max_exponent` 8, 4 or 9 when p is fixed and n counts the cells around, and through `math.pow` otherwise.
> Look around over the cells is one `grid.fold` with an accumulator per summary under its `only` test, Calculate one `math.expr`, Choose a `logic.select` chain, Depending on a `cat.switch`, and Random number a scaled `rand.float`.
> S7's six interim refusals are lifted, each with a test, and their words leave the wording module.
> The forest fire moves into `crates/henad-graph/sheets/` and `starters()`, and every grid sheet (Game of Life, Majority vote, the forest fire and the chance SIR) lowers with no warning, checks and compiles.
> On the host the chance SIR keeps its cell count for 200 ticks, the power rule meets 1 − 0.8^n for n = 3, 4, 8 and 9 and 0 for n = 0, and a sheet of every summary, a Calculate, a Choose and a Depending on equals a Rust reference cell by cell.

## State before

- `37-node-editor` at `31422d4` (S7 merged), with S11 planned beside S10.
  The branch was rebased onto `cf3ce8c` (S12 merged) for the review fixes, since S12's slot path grammar is what the lowering's paths now follow.
- S7's lowering held starts, buttons, measures, colours, Counts, Yes or no lines, Becomes, Sets and When blocks, and refused chances, rates, Look around, Calculate, Choose, Depending on, Random number, Repeat and worlds of agents at their table in the author's words.
- The forest fire waited as test data in `src/tests/sheets/` for its chances, and `starters()` returned Game of Life and Majority vote.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Draws

`crates/henad-graph/src/sheet/lower/draw.rs` is new.
Each draw takes the key of the line that owns its site, stored or derived from `<sheet id>/<line id>`, composed with the low 32 bits of FNV-1a 64 over the site's slot path, and is recorded for S7's key clash check (V18).
The path is `condition` for a chance in a change's condition, `condition.items.0` for the first item of an all of, and empty for a Chance or Random number line itself.
A When's condition lowers once and its operand joins every change inside it, so a chance in a When's condition is written and keyed once.
The plan's risk for S10, a When's condition composed twice, does not arise.

A chance is one `rand.chance` over its chance in `f32`.
With `tries` ("once for each"), it holds with 1 − (1 − p)^n (row P): `math.sub`, the power, `math.sub`, then the draw.
The power is `math.powi` when p is a number, a percent or a parameter, so its base is fixed when the model is built as the node requires, and n is a grid Count, the `count` part of a grid Look around or one of its Count summaries.
Its `max_exponent` is the number of cells the count looks at: 8 for the 8 cells around, 4 for the 4, 9 for the 3 by 3 block.
Any other p or n takes `math.pow`, with n converted to `f32`.

A Random number is `rand.float(high - low) + low` in `f32`, the form of S7's random start.

### Looks

`crates/henad-graph/src/sheet/lower/look.rs` is new and takes S7's Count from `card.rs`, with `their` and the fold stack.
A Count with an `only` test now lowers it too, joined by `logic.and` into its test inside the fold, and skips the `grid.count` form.
A Look around over the cells is one `grid.fold` (`moore_row_major` or `von_neumann`, the centre for the 3 by 3 block, `boundary = "domain"`) with a `count` accumulator and one accumulator `summary_<id>` per summary: an `f32` `sum` for a total and an average, `max` for the largest, `min` for the smallest, a `bool` `or` for any and `and` for all.
A Count summary reads `count`.
Every `region.put` sits under the `only` test, and a summary's value reads the neighbour through `lane.get_at`.
The parts are kept by line id and part, `count` and each summary id, and a `{ result = "l2", part = "alive_level" }` chip reads them.
An average divides the total by `count` in `f32`, and an average, a largest and a smallest value of no neighbour are 0 through a `logic.select` on `count > 0`, instead of a division by zero and the identities of `min` and `max`.
A Look around standing as a phrase in a slot is an error, since it gives several values.

### Calculate

`crates/henad-graph/src/sheet/lower/calculate.rs` is new.
A formula lowers to one `math.expr` whose text is `out = <formula>`, fully parenthesised, with each distinct reference an `f32` input `v1`, `v2` in the order the formula first names it.
A reference resolves through the checker's `formula_chip` and the card's `chip`, so `my.x`, `my_new.x`, results, parts and parameters read as they do in a slot.
A number is written by `f32`'s `Debug`, which always holds a point or an exponent, so the expression reads every number as `f32`, and a number past the range of `f32` is an error at the line.
`^` lowers to `pow`, `smaller` to `min` and `larger` to `max`, and `length` and `direction` are an error on a grid, which holds no arrows.

The checker's formula parser (`sheet/check/formula.rs`) now builds a `FormulaTree` for the lowering through `parse`, and `references` keeps reading any formula for the checker.
The tree is built only by `parse`, since a long sum the checker reads for its references would otherwise nest one box per term and overflow the stack when dropped.
A formula nesting more than `MAX_FORMULA_DEPTH` (16, new in `sheet::limits`) operations does not parse for the lowering: `math.expr` reads at most 64 levels, and the parenthesised text spends up to three per operation.

### Choose and Depending on

Both sit in `card.rs`.
A Choose lowers its rows top down and builds a `logic.select` chain from the last row up over its "otherwise" value.
A row whose condition is the constant yes ends the chain, and a row whose condition is the constant no is left out.
Its slot paths are `row.<index>.value`, `row.<index>.condition` and `otherwise`, and a row's own `logic.select` sits at `row.<index>`.
A Depending on is one `cat.switch` over the read of my property, a `case.<state>` per state at `branches.<state>`.
The values of both share a type through `card::choice_type` over the new `value::shared_type`: one type is kept, numbers of several types meet in `f32`, and a typed number the known type cannot hold widens to `f32`.
A state chip in these slots is an error: the checker passes no governing property for them, so a checked sheet holds none.

### Refusals and starters

`card.rs` keeps the interim refusals of rate lines and Repeat (D38), an agents-only line kind, Record (D66), and a Count or Look around over agents within a distance.
The six lifted refusals' words, `NOT_YET_CHANCE`, `NOT_YET_LOOK_AROUND`, `NOT_YET_CALCULATE`, `NOT_YET_CHOOSE`, `NOT_YET_DEPENDING_ON` and `NOT_YET_RANDOM_NUMBER`, are removed from `sheet::sentence` and from the wording tests.
`forest_fire.toml` moves from `src/tests/sheets/` to `crates/henad-graph/sheets/` unchanged, and `starters()` returns Game of Life, Majority vote and the forest fire (section 4.1 of the plan, D64).

### Tests

In `crates/henad-graph/src/tests/sheet_lower.rs`, on the host evaluator:

- `every_grid_sheet_lowers_checks_and_compiles`, which replaces `game_of_life_and_vote_lower_check_and_compile`: the three starters and `sir_chance.toml`, each with no sheet warning, no graph diagnostic, `GridProgram::compile`, an entry and its own canonical text.
- `the_chance_sir_sheet_conserves_its_cells_every_tick`: on a 64 by 64 world, a New outbreak press through its action pass infects more cells, then Susceptible, Infected and Recovered add up to 4,096 every tick for 200 ticks, and the epidemic peaks and recovers.
- `once_for_each_follows_the_power_rule`: a 300 by 300 sheet whose Set draws with chance 0.2 once for each flagged neighbour.
  With every cell flagged the hit share meets 1 − 0.8^k within 0.006 for the 8 cells (0.832), the 4 cells (0.590) and the 3 by 3 block (0.866), with `max_exponent` 8, 4 and 9 checked on the `math.powi`.
  With no cell flagged no cell is hit.
  With every third row flagged the cells of the other rows count 3 and meet 0.488 within 0.01.
  The same runs with p from a Calculate (0.1 times 2), which takes `math.pow`.
- `the_forest_fire_lowers_its_spread_line_before_its_lightning_line`: line 2's row precedes line 3's in the cover's transition, as `relations.rs` orders two lines that can both apply, and the two chances lower to two draws.
- `drawing_new_numbers_on_a_chance_changes_the_code` and `renaming_a_chance_result_keeps_the_code`, the identity rows of the author format that involve a chance.
- `a_chance_inside_a_when_draws_with_its_lines_composed_key`: a When holding a chance and a Become inside it holding another lower to exactly two draws, keyed `compose_key(31, path)` and `compose_key(32, path)`.
- `each_lifted_refusal_now_lowers_checks_and_compiles`: S7's seven refused lines (a chance line, a Look around, a Calculate, a Choose, a Depending on, a Random number and a chance in a condition) lower with no warning, own nodes and check and compile.
  `each_interim_refusal_gives_one_located_error` keeps rate, Repeat and the agents world.
- `the_values_sheet_runs_its_lines_on_the_host`: the new test sheet `src/tests/sheets/values.toml`, canonical, holds two Look arounds covering every summary, one with an `only` test, a Calculate over their parts and `my.level`, a Choose on a part and a comparison, a Depending on of a result and a number, Sets of a Whole number and two Numbers, and a Become on an all summary.
  One tick on a planted 8 by 8 world equals a Rust reference cell by cell, an empty all included.
- `the_values_sheet_honors_each_only_and_gives_0_for_a_summary_of_no_cell` (review): the same sheet's Count of alive cells with an `only` on their level, and a third Look around whose `only` (level at least 0.8) can leave out every neighbour, its average, largest and smallest each Set to a Number.
  The Rust reference checks every cell, and the test requires at least one cell where the `only` changes the count, one with no high neighbour (0 expected) and one with some.
- `every_line_map_path_of_a_line_reads_as_an_edit_command_slot_path` (review): over the four grid sheets and `values.toml`, every path of a line-owned node either names no slot (`""`, `when`, `change`, `formula`, `row.<index>`) or has a longest prefix that S12's `parse_path` reads and that leads to a filled slot of that line.
- `a_formula_of_arrows_or_past_the_depth_limit_is_refused_at_its_line` and `a_random_number_line_draws_between_its_values`.
- `the_sheet_lowering_never_panics_on_the_mutation_corpus` now runs over the forest fire and the chance SIR as sheets that lower.

In `crates/henad-graph/src/sheet/check/formula.rs`, `a_formula_parses_to_its_tree` (precedence, left grouping of `-`, right grouping of `^`, `-x ^ 2` as `-(x ^ 2)`) and `a_formula_nesting_operations_past_the_depth_limit_does_not_parse` (a sum of 100,000 terms refused by `parse` and read by `references`).
In `crates/henad-graph/src/tests/sheet_document.rs`, `starters_returns_game_of_life_vote_and_the_forest_fire` replaces `starters_returns_game_of_life_and_vote`.

### Docs and topic files

- `agents/henad-graph-sheet.md`: the formula module and `MAX_FORMULA_DEPTH`, the forest fire among the starters, the keys of a line's draws, the slot path convention of the `LineMap`, looks, draws, Calculate, Choose and Depending on in the cards paragraph, the refusals left, and the tests. 37,485 characters after the review fixes, under the 40k split point.
- `agents/henad-graph-sheet-edit.md`: the lowering shares the slot path grammar, and `lines` is crate-visible for the test.
- `CHANGELOG.md`: one line under Unreleased for the grid line kinds and the removed words, the starters line naming the forest fire, and S7's line listing only the refusals left.

### Edited tree

```
CHANGELOG.md                                     grid line kinds, starters, refusals left
agents/
├── henad-graph-sheet.md                         looks, draws, Calculate, choices, slot paths, tests
└── henad-graph-sheet-edit.md                    the lowering shares the slot path grammar
crates/henad-graph/
├── sheets/
│   └── forest_fire.toml                         moved from src/tests/sheets/
└── src/
    ├── sheet/
    │   ├── limits.rs                            MAX_FORMULA_DEPTH
    │   ├── sentence.rs                          six lifted refusals' words removed
    │   ├── starters.rs                          the forest fire
    │   ├── check/
    │   │   ├── mod.rs                           formula module and formula_chip visible to the sheet module
    │   │   ├── formula.rs                       FormulaTree, parse, the depth limit
    │   │   └── lines.rs                         formula_chip visible to the sheet module
    │   ├── edit/
    │   │   ├── mod.rs                           lines module crate-visible
    │   │   └── lines.rs                         parse_path, phrase_mut, value_mut crate-visible
    │   └── lower/
    │       ├── mod.rs                           calculate, draw and look modules
    │       ├── buttons.rs                       button step paths from 0
    │       ├── card.rs                          new line kinds, parts, Choose, Depending on, choice_type, refusals left
    │       ├── look.rs                          new: Count, Look around, their
    │       ├── draw.rs                          new: chances, the power rule, Random number
    │       ├── calculate.rs                     new: formulas as math.expr
    │       └── value.rs                         shared_type
    └── tests/
        ├── sheet_document.rs                    three starters
        ├── sheet_lower.rs                       the tests above
        ├── sheet_sentence.rs                    six lifted refusals' words removed
        └── sheets/
            ├── forest_fire.toml                 moved to sheets/
            └── values.toml                      new: every summary, Calculate, Choose, Depending on, each only
```

### Deviations from the plan

- **`math.pow` beside `math.powi`.** The plan names `math.powi` for a grid count.
  `math.powi` takes a base fixed when the model is built, so a chance worked out per cell, such as a Calculate result, takes `math.pow`, as round 1's row P allows, and so does a count the lowering cannot bound by the cells around.
- **A Count's `only` test lowers.** The checker accepts a Count's `only`, and S7 left it unread.
  The Look around's `only` needed the same join, so a Count now reads it too.
- **The six lifted refusals' words are removed** from `sheet::sentence`, a public module, along with their wording test rows, since no code shows them any more.
  S11's plan text names no such word.
- **A formula depth limit and two formula errors.** `MAX_FORMULA_DEPTH` (16), a formula reading `length` or `direction` on a grid, and a number past `f32` are lowering errors at the line.
  The checker accepts these formulas, and an agents lowering may lift the arrow functions.
- **An empty average, largest or smallest is 0.** No design source fixes the value of a summary over no neighbour.
  The fold's identities would give the largest `f32` for a smallest, and an average would divide by zero.
- **The mutation corpus's share of edits that still lower is now one in five**, down from one in four.
  The forest fire and the chance SIR join the sheets that lower, and their random edits mostly break the TOML or a key, as the other sheets' do.
  No edit panics.
- **`look.rs`, `draw.rs` and `calculate.rs`** split the line kinds out of `card.rs`, where the plan names `card.rs` and `look.rs`, to keep `card.rs` near its S7 size.

## State after

- Every line kind of a cells card lowers but rates and Repeat: Count, Look around, Chance with or without "once for each", Random number, Compare, All of, Any of, None of, Calculate, Choose, Depending on, Become, Set and When.
- The forest fire is a sheet starter, and the four grid sheets lower with no warning, check and compile.
- Rates, Repeat and worlds of agents are refused at their table in the author's words until S21, S20 and S17.
- Checks run in the worktree after the review fixes: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph --all-targets --all-features -- -D warnings -W clippy::all`; `cargo check --workspace --all-targets --all-features`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph --all-targets --all-features` (365 passed, 1 ignored); `cargo test -p henad-graph --doc` (10 passed); `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features`; `cargo check -p henad-graph --all-features --lib --target wasm32-unknown-unknown`.
  The full check runs at the merge point.

### Merge check

- The rebase onto `03b34fa` (S11) conflicted in three files, each resolved by keeping both milestones' text, as S11's record foresaw.
  `CHANGELOG.md` keeps S10's rewritten sheet starters and sheet lowering lines, S11's sheets in model files line, then S10's line on every grid line kind.
  `agents/henad-graph-sheet.md` takes S10's text of the lowering tests paragraph, then S11's two sentences on the mapping tests.
  `crates/henad-graph/src/tests/sheet_lower.rs` keeps S11's mapping tests and S10's tests after them, and drops the `sheet` helper again in the review fix commit, since `sheet_document::sheet_text` replaces it.
- `docs/reference/model-files.md` gains the row "Operations nested in one formula | 16 deep" in the limits of a sheet, for `sheet::limits::MAX_FORMULA_DEPTH`, in `docs: formula depth limit of a sheet [draft]`, as S11's record asked of this merge.
- S11's tests that walk `sheet::starters()` (henad-explore's sheet file tests and the facade's `sheet_starters_build`) now meet the forest fire too, and pass.
  S11's `ModelFile` leaves the lowered graph's warnings out of a sheet's warnings.
  Issue 2 below therefore concerns the lowered graph alone, such as the graph `--print-lowered` prints.
- The full check with `HENAD_REQUIRE_GPU=1` on the rebased branch: every step exits 0 but `cargo deny`, which reports only the baseline's RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests, whose one failure is the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`.
  The failing test list holds no name outside the baseline's, and the test log shows one failed target, henad-models' library, with that one test.
  No fix was needed.

## Issues found & future directions

1. The device has not run a lowered chance, look or formula yet.
   `math.powi` reads a table on both backends, so its parity is exact, while `math.pow` and the `f32` division of an average are approximate in a shader.
   S14's device tests cover the grid sheets.
2. The new kinds write unused nodes the way S7's results do: a Look around summary no line reads still has its accumulator and `region.put`, and a Chance line nothing reads still draws.
   The graph checker reports such nodes as technical warnings.
   S11's diagnostic mapping filters them or words them for the author.
3. The checker refuses a state value in a Choose or Depending on, since it passes no governing property for these slots, so the lowering only meets numbers, yes or no values and property values there.
   The editor (S19) decides whether such a choice is offered, and if it is, the checker needs the property a choice changes as its governing property.
4. A grid Look around and a grid Count over the same cells each write their own fold.
   Fold fusion (F1) comes with S20.
5. A chance once for each of a count that is not a grid count, such as a Calculate or a typed number, takes `math.pow` with no bound on n.
   A negative or fractional n then gives a chance that no reading as separate tries explains.
   The editor never offers one, since the "for each" blank takes a count.
6. `forest_fire.toml` keeps round 1's 1024 by 1024 world and a lightning chance of 10^-5 (D64, D72), and the New model dialog lists it from S13a.
7. The sentence of a Count leaves out its `only` test: `values.toml`'s line 10 reads "count of the 8 cells around me whose Status is Alive", although the line counts only those whose Level is at least 0.5.
   The Look around's sentence names its `only`.
   The writer's sentences belong to S4's wording table, and S10 leaves them as they are.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D38 | Line kinds whose lowering lands later | the plan | A located error in the author's words until the lowering exists, each lifted with a test that the refusal is gone | S7, S10, S17, S21 |
| D64 | Which round 1 models ship as starters | round 1, section 14, answer 13a | All eight, forest fire and bounded confidence included, and the ants keep their slower trail with a note | S1b, S10, S17, S20 |

No row of the gate names S10.
The gate's section 8 changes S10's test strings to "otherwise (when line 2 does not apply)", and no S10 test reads an "otherwise" string: the forest fire's line 3 reads it in S5's and S4's tests.

## Review

A reviewer with a fresh context found three should-fix items and three nits, all fixed in `fix: s10 review findings [draft]` after a rebase onto `cf3ce8c`.
S12 had merged meanwhile, and its `parse_path` numbers rows and items from 0 and names a summary by index, where S10's lowering numbered rows and items from 1 and named a summary by id.
The lowering now follows S12's grammar (`items.0`, `row.0.condition`, `summaries.0.value`, `right.0` for a listed item, and `button.<id>.does.0` for a sheet node too), since nothing yet depended on the old paths and the draw keys they feed change code identity once sheets are saved.
Keeping summaries by id would have kept a draw's key across a reorder of summaries, but the author format already accepts that a reorder of Choose rows moves their keys, and one grammar for the module outweighs it.
A new test holds every line path of the grid sheets and `values.toml` to `parse_path`, which needed `sheet::edit::lines` and its `parse_path`, `PathStep`, `RowSlot`, `phrase_mut` and `value_mut` raised to crate visibility.
The second item removed the branch that gave a choice of states alone the first States property holding them, since the checker refuses a state chip in those slots and the branch could never run: `card::choice_type` now returns an error for a state.
The third item added the tests above for a Count's `only` and for a summary of no cell, and each fails when its behaviour is broken by hand (a NaN average, and a count that ignores the `only`).
The nits: "summarize" in a string literal, the topic file's final newline, its tests paragraph ("two starts") and keys paragraph reflowed, and the lowering tests' duplicate `sheet` helper replaced by `sheet_document::sheet_text`.

## Benchmark commands left for the maintainer

None.
S10 changes no engine and no hot loop, and the lowered forest fire and chance SIR have no hand-written graph of the same model to compare against in `crates/henad-graph/starters/`.
S14 compares the grid sheets with the graph starters on the device.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
