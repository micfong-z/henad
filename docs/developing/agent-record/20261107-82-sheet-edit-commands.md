---
date: 2026-11-07
title: "Sheet edit commands"
description: "S12 of the #37 redesign. henad-graph gains sheet::edit, every edit the sheet editor makes as a command on SheetDocument with no user interface: SheetBuilder::apply runs a SheetEdit on a copy, mints ids from names and line numbers that never come back, keeps result names automatic and unique, clears every reference a delete leaves so the lines that used it are skipped, and builds the final editor's offers as one command each. The graph builder's key stream moves to a public KeyStream that both builders share."
icon: material/pencil-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s12-sheet-edit, rebased onto S7's record d5091fb: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 8ae998d
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sheet edit commands and a shared key stream [draft]`, `fix: s12 review findings [draft]`, `fix: s12 merge check [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet edit commands

> S12 gives the sheet its edit commands, the library half of R5.
> `henad_graph::sheet::edit` holds one `SheetEdit` per edit the sheet editor makes, and `SheetBuilder::apply` runs each on a copy, so a refused command changes nothing.
> New items take ids minted from their names, a rename changes the name alone, and Change id rewrites every reference.
> A new line takes the next line number, never one a deleted line held, and a line that draws takes a fresh key and keeps it.
> A delete never refuses: it clears every reference to what it deletes, and the lines that used it are skipped.
> Result names follow the final editor's automatic names until the author types one, and stay unique across the sheet.
> The outline moves, the outside form, the one-sided rewrite, the fold offers, "Use a rate for each", the tick's unit with its mark, the percent literal and the Starts as commit are each one command.
> The graph builder's key stream becomes the public `KeyStream`, which both builders share.

## State before

- `37-node-editor` at `8ae998d` (S5 merged), with S7 open in a parallel worktree.
- S1a's reader, S4's writer and wording table and S5's checker read, wrote and checked a sheet, but nothing edited one: every test that changed a sheet changed its `SheetDocument` fields by hand, as the writer's three diffs of the author format did.
- `GraphBuilder` held its own `xorshift64` state and drew its keys from `clipboard::entropy`.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### The shared key stream

`crates/henad-graph/src/keys.rs` is public as `henad_graph::keys`.
`KeyStream` is an `xorshift64` stream: `from_entropy` seeds it from a `RandomState` hash of a counter, the code that was `clipboard::entropy`, and `seeded(seed)` through `mix_seed`, as `GraphBuilder::with_key_seed` did.
`next_key` returns the high 32 bits of the next state, and `next_key_not_in` skips the keys a closure calls taken.
`GraphBuilder` holds a `KeyStream` in place of its `key_state`, and its `mint_key` calls `next_key_not_in`, so it draws the same keys as before and every graph edit test passes unchanged.
`KeyStream::seeded(1)` returns 605988705, 180871622 and 1319607719 first, which `a_seeded_key_stream_gives_its_known_first_keys` holds so a rebuild test can name its keys.
The facade adds `keys` to `henad::graph`.

### The builder and its commands

`crates/henad-graph/src/sheet/edit/` is public as `henad_graph::sheet::edit`.
`SheetBuilder::new(document)` seeds its keys from entropy and `with_key_seed` from a seed.
`apply(SheetEdit)` runs the command on a copy and returns the ids it made, or a `SheetEditError`: `Missing`, `Invalid`, `Taken` (a name another item of the same kind holds, case aside, or an id) or `Reserved`.
A command checks the structure it changes and leaves meaning to the checker, as `GraphBuilder` does.

- **The sheet:** `SetSheetName`, `SetPurpose`, `SetWorld`, `SetTime`.
- **Items:** `AddProperty` (the first States property of a kind of thing with no colour rule colours it by state, as the final editor chooses the first property made), `AddState` (the next of `STATE_COLORS`, the final editor's dark grey, blue, orange, purple, yellow and on, F14), `AddParameter`, `SetParameter` (min, max, default, step, unit and meaning as one edit), `AddMeasure` (a count in a state takes the state's colour, otherwise the next of `MEASURE_COLORS`), `AddEvent`, `AddButton` (start again, or run once with a card of the button's id), `SetButtonSteps`, `AddCard` (`cell_tick` or `<agent kind id>_tick`), `SetStartsAs`, `SetColorRule`.
  `suggested_state_color` holds the final editor's usual colours for state names, red for Infected and on, for the property form of S16a.
- **Any item:** `Rename`, `ChangeId`, `Recolor` and `Delete` over a `SheetItem` (property, state, parameter, measure, event, button, card, agent kind).
  A recoloured state takes along each measure drawn in its old colour that counts it (F11).
- **Lines:** `AddLine` (a line template at the end of a card or of a When or Repeat), `SetSlot` and `SetField` (a `LineField`) at a slot path, `ChooseCondition` (a `ConditionItem` of "Choose when"), `UseOneSide`, `UseCountForEach`, `MoveConditionInto`, `RenameResult`, `SetNote`, `MoveLine`, `DeleteLine` and `DrawNewNumbers`.
  "Remove this When" is `DeleteLine` on the When, and "Add for each" is `SetSlot` of `tries`.

A slot path is the author format's keys joined by dots, such as `condition.rate`, `whose.right`, `items.2` or `row.0.condition`, each step but the last into the phrase a slot holds.

### Ids and line numbers

`mint_id(name, space, is_taken)` lower-cases the name, turns each run of characters other than `a-z` and `0-9` into one `_`, puts `x` before a leading digit, cuts the id to 48 bytes, and takes `_2`, `_3` and so on against a clash.
"Infection rate" mints `infection_rate`, and `infection_rate_2` beside a property of that id.
An id reserved in its `IdSpace` is refused: "Grid width" mints `grid_width`, which the IR holds, and minting returns `Reserved("grid_width")`.
The spaces follow the checker's namespaces, with `any_state` the one reserved state id (D81), and a button that runs once mints against the cards too, since its card takes its id.
`ChangeId` checks the id grammar, the length, the reservation and the namespace, then rewrites every reference.
A card of the button's id that the button runs follows the button, and an agent kind's tick card follows the kind.
A suffixed id that is reserved, such as `henad_2` when `henad` is taken, is refused as a reserved base is.

A new line takes `l<next_line>`, and `next_line` grows past it, so a deleted line's number is never minted again.
After a command, each line it touched that draws gets a key no other draw holds, a template's own key kept when free.
Each start it set that draws, or that holds a key another draw holds, gets a fresh key too, and a start on the ground draws nothing and takes none.
A line keeps its key while an edit takes its draw away, and `DrawNewNumbers` mints a new one.

### References and deletes

`references.rs` walks every place a sheet names an id: slots, lists, items, branches, summaries, rows, the line fields, formulas, starts, blocked states, measures, colour rules, button steps and cards.
A rename rewrites each of them, and a clear blanks a slot, drops a list item, empties a field or a formula that names the target, and drops a table about the target, such as a measure of a deleted state or a colour rule over a deleted property.
A state chip belongs to the property its slot is about: a Become's property, the left side of a comparison, a start's own property or a Set's property, and otherwise to the one property that holds a state of that id.
Formulas are rewritten with the formula parser's lexing, so numbers and the function names stay as they are.

A delete leaves a blank where a reference was.
In most slots of a card line the checker reads a blank as a skip warning, and a line that used a deleted item is skipped and never refused (D39).
A start that named a deleted state takes the first state its property still holds: its `base`, its `otherwise`, the state chip of its value ("All tree"), and the ground state of a start on the ground.
A blank outside the card lines is a checker error, as are a few blanks in agent lines, and issue 3 lists the deletes that leave one.
A deleted When or Repeat leaves its lines one level up in its place, a deleted card takes its lines, and a deleted button takes each card that runs once and that no other button runs.

### The outline moves

The moves follow the final editor's outline, the round A change list's F4.
The first line inside a When moves up out of it and the last moves down out of it.
A line just below or above a When moves into it, at its end or its start.
A When moves with its lines and swaps with the line or block beside it, and never moves into another When.
A move past the top or the bottom of the card is refused.

`AddLine` inside a When gives a Become or Set with a blank condition "always", and a Become with no old state takes the state the nearest When tests as "my X is S", as the final editor does.

### The final editor's offers

- **`ChooseCondition`** keeps the condition's values when the form stays: "is", "is at least" and "is at most" share the comparison form and set its test (C1, D82).
  Any other form starts blank, and a When refuses always.
- **`UseOneSide`** turns "is less than 2 or more than ..." into "is less than 2", and "... or more than 4" into "is more than 4" (D82).
- **`UseCountForEach`** moves a count chosen as a chance or rate into "for each" and empties the chance or rate, the button "Use a rate for each infected neighbour" (C4).
- **`MoveConditionInto`** copies a When's condition into the one change inside it and removes the When, and refuses when the change has a condition other than blank or always, as the final specification offers it only then.
- **The tick's unit (D83):** a rate put in by `AddLine`, `SetSlot` or `ChooseCondition` with no unit takes the tick's unit and the mark `rate_unit_from_tick`.
  A parameter put into a rate with no unit or a marked one gives the rate the first "per day" its Unit text names ("per day per infected neighbour" names day) and clears the mark.
  The Unit text is read as the prototype's `paramPerUnit` reads it: case aside, one or more spaces after "per", a plural "s", and a word boundary at both ends.
  A unit chosen by hand clears the mark and gives a parameter with no unit in the rate the unit "per week", as the prototype does when the author picks a unit.
  With no length of one tick on the sheet, either sets one tick to 1 of the unit.
- **Percents (D110):** a typed percent is a `SheetLiteral::Percent` in `SetSlot` or `SetStartsAs`, and names a result "30% chance happens".
- **Starts as (D111):** `SetStartsAs` replaces a property's start as one command, keeping its key.

### Automatic names

`names.rs` holds `automatic_name(document, line)`, the final editor's result names from the wording table: "Alive neighbours" or "B in my block" for a Count, "Total Food around" for a Total or average, "Recovery happens" for a chance or rate of the parameter Recovery rate, "Infection happens" for a rate of a Calculate that holds Infection rate, "0.1 per day happens" or "30% chance happens" for a typed number, a Compare's or a Calculate's sentence, "All of" and "Any of", and the empty forms "Count", "Happens", "Test" and "Result".
`Sentences::result_words` gives the sentence of a Compare and a Calculate.
After every command, each result line that follows its automatic name takes it again (D85): a name the edit left alone follows when it was its old automatic name, a " 2" after it included, and a new line or a name the edit set follows when it is empty or its new automatic name.
Names typed by hand are taken first, and the followers take " 2", " 3" in display order, unique case aside.
No flag is stored, as D85 chose.

### Starters

Every committed sheet already follows the minting rule, so no starter id was renamed.
`every_starter_id_follows_the_minting_rule` mints every property, state, parameter, measure, event, button and agent kind id from its name in file order, derives each card id from its kind of thing or its button, and finds the line ids `l1` upward in display order across the cards, with `next_line` one past the highest.

### Tests

`crates/henad-graph/src/tests/sheet_edit.rs`:

- The author format's three edits built by commands from the sheets, each producing the exact canonical text of its diff with S4's words: the waning line in SIR through `AddParameter`, `SetParameter`, `AddLine` with the diff's key, `SetField` and `SetSlot`, which passes through the tick's unit and its mark; Cohesion's Steer moved above Separation's by two `MoveLine`; Cohesion renamed.
- `every_outline_move_matches_the_prototype`: into, out of and around a When nested in a When, a When swapping with a When beside it at the top of the card and inside a When, the refused moves at the top and the bottom, and deleting a When.
- `a_rename_changes_one_name_and_the_comments_that_show_it`, `change_id_rewrites_every_reference_and_nothing_else` (a parameter, a property, a button with its card, a state, a parameter inside formulas and an agent kind, each the old text with the id replaced, and the refusals of a taken, reserved and malformed id), `a_deleted_line_number_is_never_minted_again`, `id_minting_follows_its_known_answers`, `every_starter_id_follows_the_minting_rule`, `a_seeded_key_stream_gives_its_known_first_keys`.
- A change added inside a When, the automatic names, the condition offers, "Use a rate for each" and "Move it into line N" with its refusal, deletes that leave skipped lines and no error (with the forest fire's "All tree" start and the ants' start on the nest), colours, rate units and their mark, a parameter with no unit taking a unit chosen by hand, `unit_in_text`'s known answers, a Starts as commit with a percent, a start on the ground with no key and a start whose template key is taken, and new buttons and cards with a start-again button's new id.
- `every_edit_leaves_a_document_that_reads_back`: twelve sequences of thirty random commands on each of the nine sheets, more than a thousand of them accepted, after each of which the canonical text reads back to the same document, the checker does not panic, and no two lines share a key.

An inline test in `references.rs` holds the formula rewrite.

### Docs and topic files

- `agents/henad-graph-sheet.md`: the module list and six paragraphs on the edit commands, ids, references, lines, rates and names, and the test file. 31,213 characters after the change.
  At the merge point S7's lowering had already grown the file, and the two together held 40,919 characters, past the 40k limit of `AGENTS.md`. The merge check moved the edit commands into `agents/henad-graph-sheet-edit.md` (9,826 characters), leaving 32,171 in the sheet file.
- `agents/henad-graph-edit.md`: the shared key stream. 8,403 characters.
- `agents/henad-graph.md`: `keys.rs` in the module list.
- `agents/facade-and-tutorial.md`: `keys` among the graph modules, and the new paths of the facade path test.
- `AGENTS.md`: a row for `agents/henad-graph-sheet-edit.md`, beside the sheet topic file's row.
- `crates/henad/src/lib.rs`: `keys` in `henad::graph`, and its module doc names `SheetBuilder` and `KeyStream`.
- `crates/henad/tests/facade_paths.rs`: `SheetBuilder`, `SheetEdit` and `KeyStream` through the facade.
- One CHANGELOG line under Unreleased.

### Edited tree

```
AGENTS.md                                        sheet edit topic row
CHANGELOG.md                                     sheet edit commands, KeyStream
agents/
├── facade-and-tutorial.md                       keys, facade paths
├── henad-graph-edit.md                          the shared key stream
├── henad-graph-sheet.md                         module list, sheet/edit/
├── henad-graph-sheet-edit.md                    new: edit commands, ids, references, lines, rates, names, tests
└── henad-graph.md                               keys.rs
crates/
├── henad/
│   ├── src/lib.rs                               keys in henad::graph
│   └── tests/facade_paths.rs                    SheetBuilder, SheetEdit, KeyStream
└── henad-graph/src/
    ├── lib.rs                                   pub mod keys
    ├── keys.rs                                  new: KeyStream
    ├── edit/
    │   ├── mod.rs                               GraphBuilder takes its keys from KeyStream
    │   └── clipboard.rs                         entropy moved to keys.rs
    ├── sheet/
    │   ├── mod.rs                               pub mod edit, module doc
    │   ├── sentence.rs                          Sentences::result_words
    │   ├── check/
    │   │   ├── mod.rs                           crate-visible reserved ids, holds_draw, is_result_kind
    │   │   ├── declarations.rs                  reserved id lists crate-visible
    │   │   └── lines.rs                         holds_draw, is_result_kind crate-visible
    │   └── edit/                                new
    │       ├── mod.rs                           SheetBuilder, SheetEdit, LineField, SheetItem, mint_id, colours
    │       ├── names.rs                         automatic_name, names kept up to date and unique
    │       ├── references.rs                    every reference, renamed or cleared, formula rewrite
    │       └── lines.rs                         slot paths, phrase depth, outline moves
    └── tests/
        ├── mod.rs                               sheet_edit
        ├── sheet_edit.rs                        new
        └── sheet_writer.rs                      assert_hunks shared
```

### Deviations from the plan

- **`sheet/edit/` is a directory** of four files instead of one `edit.rs`, which would have held about 3,400 lines.
- **`AddLine` takes a whole line as its template** instead of a kind alone, so an add menu item can start its line as the final editor does ("Become, at a rate" starts with a rate already chosen). The builder fills what the final editor fills, inside a When and for a new rate, and keeps a template's key when no other draw holds it, which lets the waning edit reproduce the author format's key.
- **"Remove this When" and "Add for each" have no commands of their own.** They are `DeleteLine` on the When and `SetSlot` of `tries`, each already one command.
- **A delete clears its references** instead of leaving ids that name nothing. The checker reads a parameter, property or state that does not exist as an error and a blank as a skip warning, so only a clear leaves the line skipped and never refused, as D39 asks. A formula that names the deleted item is emptied, a start's `otherwise` or `base` falls back to the property's first remaining state, and a Depending on branch that names a deleted state is dropped.
- **No starter id was renamed**, since every committed sheet already follows the minting rule. "Creation order" is read as file order, table by table. The sheet's own id is outside the rule: the vote's `vote` and the ants' `ants` differ from what their names mint, and no reference holds a sheet id.
- **The minting rule's open points** are settled here: the leading letter before a digit is `x`, a name with no letter or digit mints `x`, an id is cut to the sheet's 48 bytes before its suffix, and a reserved id is refused, never suffixed.
- **A name another item of the same kind holds is refused**, case aside, which is the final editor's form check ("A parameter called X already exists.") moved into the library, with technical messages.
- **`AddButton` makes a button that starts again or runs once.** No grid control writes `set_everywhere` (D98), and a button with other steps takes `SetButtonSteps`. An agent kind is neither added nor deleted, since a sheet holds one (D43), and agents on the sheet pane come with S26.
- **Keys of hand-written lines.** A line with no key that an edit touches takes a random key at that edit, where the author format derives one from the sheet and line ids. S7 lowers a keyless line with the derived key, and the two differ only for a hand-written file with a missing key.
- **A start on the ground takes no key**, as the checker counts it no draw.
- **Two automatic names have no row in the wording table.** A Total or average with no property reads "Total ... around" with `BLANK_TEXT`, and a typed rate with no unit reads "0.1 per unit happens" with `UNIT_PLACEHOLDER`, as the prototype names it. A none of has no automatic name.
- **The three diffs of the author format** are built by commands in new tests beside the writer's tests, which keep building them by hand as tests of the writer.

## State after

- Every edit the sheet editor makes is a command, from adding a property to moving a line, and every command leaves a sheet that reads back from its canonical text and checks without a panic.
- S16a, S16b, S19, S22 and S25 drive the card and the sheet pane through these commands, one command per undo step.
- `KeyStream` is the one source of draw keys for graphs and sheets.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph -p henad --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad --all-targets --all-features` (henad-graph 306 passed, 1 ignored after the review, and the facade's tests); `cargo test -p henad-graph --doc` (9 passed); `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` for henad-graph and the facade; `cargo check --all-features --lib --target wasm32-unknown-unknown` for henad-graph and the facade; `cargo clippy -p henad-graph -- -W missing_debug_implementations` reports nothing.
  The full check runs at the merge point.
- The merge rebased S12 onto `d5091fb` (S7 merged, with its record #81).
  The lists of `AGENTS.md`, `CHANGELOG.md`, `sheet/mod.rs` (its module doc and its module list) and `src/tests/mod.rs` conflicted and keep both sides, as does the module paragraph of `agents/henad-graph-sheet.md`.
  The joined topic file held 40,919 characters, and `fix: s12 merge check [draft]` moved the edit commands into `agents/henad-graph-sheet-edit.md`.
- The full check at the merge point, with `HENAD_REQUIRE_GPU=1`: the native, wasm32 and facade typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372 alone, and the workspace tests exit 101 on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, both as in the baseline.

## Issues found & future directions

1. S19 maps each add menu item (`AddItem`) to its `AddLine` template, the one place left that decides how a new line starts.
2. A state chip whose slot names no property, of a state id two properties share, is neither renamed nor cleared, since its property cannot be known. The checker reports what it then names. No committed sheet holds one.
3. Some deletes leave checker errors. Deleting every property, state, parameter, event and button of each of the nine sheets one at a time finds these:
   - a parameter in a start's chance or value or in `kept_per_tick` (the initial density of Game of Life, Vote, SIR, SIR by chance and the forest fire, the site reward and the trail kept per tick of the ants), which leaves a blank outside the card lines;
   - the ground property of the ants, which leaves the start on the ground with no property;
   - Boids' velocity, max speed and min speed, the ants' `to_food` and `to_home`, the random walk's position and bounded confidence's opinion, whose blanks fall in agent slots the checker treats as errors;
   - the ants' event `delivery` ("Record needs an event") and property `cargo` ("l7 is no place").

   A clear also drops a Depending on branch that named a deleted state, and the checker then reports the missing branch as an error.
   A parameter's default as a literal in a start, a start anywhere after its ground property goes, and blank-tolerant agent slots (S28) would remove these errors.
4. Each command rebuilds the sentences of the whole sheet twice to keep the names up to date. No command was timed, and an editor on a large sheet may want to time `apply` before S16a draws a sheet on every frame.
5. The unit in a parameter's Unit text is read as the first "per second" to "per year", case aside, with a plural "s" and a word boundary at both ends, as the final editor reads it. "Per days" names day, and "per daylight" names none.
6. S7 and S12 share no file but the lists, and no starter id changed, so whichever merges second only reruns its tests.

## Review

The review returned seven should-fixes and five nits, and all twelve held.
Some deletes left checker errors instead of skipped lines, against the module doc and this record.
A start whose value is a state chip, such as the forest fire's "All tree", now falls back on the first state its property still holds, as `base` and `otherwise` already did, and a start on the ground falls back on the first state its ground property still holds.
`a_delete_leaves_skipped_lines_and_never_an_error` holds the forest fire's "All tree" with Tree deleted and the ants with Nest and Searching deleted.
Deleting every item of every sheet one at a time found the deletes that still leave errors: blanks outside the card lines, a start on a deleted ground property, and agent slots the checker treats as errors.
They are issue 3, and the module doc, `references.rs` and the topic file now say that the checker reads most blanks of a card line as a skip and a blank outside the card lines as an error.
`starts_draws` counted a start on the ground as a draw and gave the ants' position a key the checker and the starter do not hold.
It no longer does, and a test holds an on-the-ground `SetStartsAs` at no key.
`SetSlot` wrote "per <unit>" into a parameter with no unit put into a rate, a rule neither the final specification's 8.9 nor D83 holds.
That rule now sits where the prototype applies it, in `LineField::RateUnit`, and `a_parameter_with_no_unit_takes_the_rate_unit_chosen_by_hand` holds both commands.
`unit_in_text` searched case-sensitively for "per <unit>", with no word boundaries.
It now reads the Unit text as the prototype's `paramPerUnit` regular expression does, and `a_parameter_unit_names_its_unit_of_time_as_the_final_editor_reads_it` holds nine known answers, "Per day", "per days" and "per daylight" among them.
Issue 5 is corrected.
`ChangeId` of a button renamed any card of the button's old id, so a start-again button "Cell tick" renamed the Game of Life tick card.
A card now follows only when the button runs it, and `a_new_button_and_card_take_their_ids` holds the case.
The outline test had no step where two Whens swap.
`every_outline_move_matches_the_prototype` now swaps two Whens at the top of the card in both directions, and two Whens inside a When.
Of the nits, `MoveConditionInto` refuses a change whose condition is neither blank nor always, `mint_id` refuses a suffixed id that is reserved (`henad_2`), and a touched start whose key another draw holds takes a fresh key.
The `Rename` doc says that references keep the id and that names following their automatic names take the new words, and the "so" clauses of the module doc and the trailing "which" clauses of `KeyStream::seeded` and `LineField::RateUnit` are split into sentences.
`set_field` grew past clippy's line limit, and the choice of a rate unit moved into `rate_unit_chosen`.
After the fixes: fmt, clippy over henad-graph and the facade with `-D warnings`, the Debug lint, the henad-graph and facade tests with `HENAD_REQUIRE_GPU=1` (306 passed, 1 ignored in henad-graph), its doc tests (9 passed), rustdoc of henad-graph and the facade, and the wasm32 typechecks of henad-graph and the facade pass.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D12 | Keys | the author-file design, section 7, question 6 | (a) Minted at random and stored, derived from the sheet and line ids with a warning when missing | S7, S12 |
| D39 | How a half-built line is stored and run | both drafts (P1) | An absent slot key, read as blank and written as nothing. Line-local problems are skip warnings in the final specification's words, and the run skips the line and every line that reads it. File-level problems are errors | S1a, S5, S7 |
| D69 | Key minting | the foundations draft | One public `KeyStream` with the graph builder and the sheet builder as its two callers | S12 |

D39 does not name S12, and S12 builds the delete that leaves lines skipped under it.

From the gate, the rows whose "Built in" names S12, with the final specification named so and change lists named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D81 | "always" and "any state" in the sheet | final specification 8.3 and 8.4, against author-file 2.4 and D39 | **Conflict.** The final specification's. Every condition slot holds `{ kind = "always" }` for always, and `start_state` holds the reserved id `any_state` for any state. An absent `condition` or `start_state` is a blank (D39). `any_state` joins the ids that minting refuses (S12) and that the checker refuses as a state id (S5). A When whose condition is always is a checker error | S1a, S1b, S4, S5, S7, S12 |
| D82 | The condition "is less than ... or more than ..." in the sheet | the round B change list's B19, round C C1, final specification 8.4 and 8.10 item 2 | **Plan's choice** of the stored form: a compare phrase with `test = "outside"`, `left`, `low` and `high`, allowed in a condition slot only, while a Compare line keeps its eight comparisons. It holds when `left` is below `low` or above `high`, counts as one test of its value for "otherwise", and lowers to two compares joined by an or. A blank `low` or `high` is a blank (D39), and the editor shows the one-sided offer of 8.10 item 2 | S1a, S4, S5, S7, S12, S19 |
| D83 | A rate's unit taken from the tick | round C C5, final specification 8.3, 8.9 and 12 (`unitFromTick`) | **Plan's choice** of the key. A rate phrase writes `rate_unit_from_tick = true` beside `rate_unit` while the unit came from the tick and was not chosen by hand, and writes nothing otherwise. The lowering reads `rate_unit` alone, so the mark stays outside the code identity. A new rate takes the tick's unit when one tick has a length, a parameter put into the rate replaces the unit with its own and clears the mark, and a unit chosen by hand clears it | S1a, S4, S12, S25 |
| D85 | Result names | the round B change list's B1, the round A change list's F19, final specification 8.6, superseding D35 | The final specification's automatic names, unique across the model with " 2" and " 3". D35's fixed "Happens" is void, and "Happens" shows only while the blank is empty. **Plan's choice** for storage: no stored flag. An edit renames a result only while its stored name equals the automatic name of the line as it was before the edit, so a name typed by hand stays. The naming function lives in henad-graph beside the edit commands | S12, S19 |
| D110 | A number typed as a percent | round D D8 part 1, final specification 3, 8.5 and 12, against author-file 2.3 | **Conflict.** The final specification's: a number typed with "%" keeps showing as typed ("30%") on its chip, in the Number box when its picker reopens, in result names ("30% chance happens"), in sentences and in a Starts as field, and a note prints the decimal. **Plan's choice** of the stored form `{ percent = 30 }`, allowed wherever a number literal is. The checker applies D75's ranges to the number divided by 100, and the lowering writes that quotient. A parameter's default, min and max stay plain numbers | S1a, S4, S5, S7, S12, S16a, S19 |
| D111 | Starts as numbers | round D D8 part 2, final specification 7.3 and 13, against plan S16a and the wording table's "[blank]" | A Starts as chance, value, low end or high end is a plain text field with a "Use a parameter" button beside it. The field commits on Enter or on losing focus, only when the value changed, as one `SheetEdit` and one undo step, and typing alone changes nothing. Text it cannot read, or a chance outside 0 to 1, keeps the old value and shows a red message under the row, which is no undo step | S16a, S16b (the undo step) |

The gate's scope differences also give S12 the commands of D84's scope (none: the When scope is the checker's) and of C4's count note ("Use a rate for each", built as `UseCountForEach`).

## Benchmark commands left for the maintainer

None.
S12 changes no engine, no lowering and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
