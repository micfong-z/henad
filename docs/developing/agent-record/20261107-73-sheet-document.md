---
date: 2026-11-07
title: "Sheet document"
description: "S1a of the #37 redesign, the first milestone after the gate. henad-core gains TimeUnit, the one unit of time of the workspace. henad-graph gains the sheet module: SheetDocument, the whole author format as plain data with half-built lines stored as blank slots, and ParsedSheet, a reader that checks the header and the sheet version first, refuses an unknown key in every table and records the place of every table and key. The three header checks of the workspace become one ModelHeader in model_text.rs."
icon: material/file-document-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s1a-sheet-document, four unsigned [draft] commits and this record after 5967809; the full check at 4bac972 equals the baseline at be338ec"
baseline_commit: be338ec
delta_state: "committed on `37-node-editor` by fast-forward: `feat: time units in henad-core [draft]`, `feat: sheet document and reader [draft]`, `fix: s1a review findings [draft]`, `feat: percent literal in sheets [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet document

> S0, the gate of the #37 redesign, sent the design to code: its verdict is go, every round C finding and every open item of the three change lists has a disposition, and S1a may start from `be338ec`.
> Its baseline at `be338ec` fails two steps of the full check: `cargo deny` on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on the budget test of record #68 alone.
> S1a builds the first layer of the sheet, the author document a researcher builds on the sheet and its cards.
> henad-core gains `TimeUnit`, second to year, the one unit of time that the sheet's `[time]`, a rate's unit and the later time scale share.
> henad-graph gains `henad_graph::sheet`: `SheetDocument`, the whole author format as plain public data, and `ParsedSheet`, its reader.
> The reader checks the header and `[sheet] version` first, refuses an unknown key in every table, reads an absent slot as blank, and records the place of every table and key.
> The header checks of the graph reader and henad-explore's two readers become one `ModelHeader` in `model_text.rs`, whose message lists the kinds it reads with "or".
> A depth limit that the plan did not have keeps a deeply nested sheet from overflowing the stack.
> After round D's addendum, a follow-up commit adds the percent literal `{ percent = 30 }` of D110, a literal standing for 0.3 wherever a number can.

## State before

S0 wrote the gate on 2026-11-07.

- **Verdict: go.** The study's rules applied to round C send the design to code, and no "goes back" rule holds.
  Every round C finding is fixed in the final specification and the prototype, or settled by a decision row from D80 to D102.
  Round D, on the smaller model, was still running, and the gate lets S1a, S3 and S6 start before its addendum.
- **Baseline at `be338ec`.** The full check of the plan's section 2.3 exits 0 on every step but `deny` (1) and `test` (101).
  `cargo deny` reports RUSTSEC-2026-0370, 0371 and 0372, the unmaintained `zvariant`, `zbus_names` and `zvariant_derive`.
  The one failing test is `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`, the budget test of record #68.
- **Scope changes for S1a.** The gate's section 11 adds the explicit `always` and `any_state` (D81), the outside test (D82) and `rate_unit_from_tick` (D83) to the stored form, and changes the copied examples: D81's values written in, Game of Life's Clear a Run once card (D98), and SIR's button renamed New outbreak (D97).

In the code at `be338ec`:

- Three places read a model file's header: the graph reader (`reader.rs`, with its own `HeaderTable` and `[graph] library`), and `ModelFile::parse` and `GridRuleFile::parse` in henad-explore (one `HeaderTable` taking the allowed kinds).
  Their messages differed: "this reader only reads 'graph'" against "this build only reads 'gpu_grid' and 'graph'".
- No module read a sheet, and no type named a unit of time.

## What was done

### `TimeUnit` in henad-core

`henad_core::metadata::TimeUnit` holds second, minute, hour, day, week, month and year.
`as_str` and `parse` name a unit in a file, `plural` is the word the sheet's Time line shows ("seconds" to "years", as the gate's wording table 10.12 has them), and `days` converts one unit to days, a month 30.44 and a year 365.25.
`time_unit_factors_follow_their_known_answers` holds a week at 7 days, a month at 30.44, a year at 365.25, an hour at 1/24, the minute and the second to their reciprocals, `parse` against `as_str` for every unit, and `parse` refusing "fortnight", "days" and "Day".
The facade re-exports it at the root with the rest of `henad_core::metadata`, and `facade_paths.rs` names it.

### One header check

`model_text.rs` holds `ModelHeader`, `HeaderProblem` and `read_header`.
`ModelHeader::read` reads `format`, `format_version`, `kind`, `[graph] library` and `[sheet] version`, ignoring every other key, so each reader parses its header once.
`ModelHeader::check(kinds)` refuses another format, another version or a kind outside `kinds`, and joins the kinds as a list ending in "or": "the file contains a model of kind 'gpu_agents', but this reader only reads 'gpu_grid', 'graph' or 'sheet'".
The graph reader checks `[graph] library` after it, and the sheet reader `[sheet] version`.
`read_header(text)` returns the kind of any TOML text, a file its reader refuses included, for the app's open dispatch in S13a.
It reads `kind` alone, so a header key of the wrong type cannot hide the kind.
henad-explore's `HeaderTable` is gone, and its dispatch test now expects "'gpu_grid' or 'graph'".
Every other test of the graph reader and of `model_file.rs` passes unchanged.

### The sheet document

`sheet/document.rs` holds the schema of the author format, every type `#[non_exhaustive]` with a constructor.

- `SheetDocument`: `sheet` (`SheetHeader`), `world` (`SheetWorld` with `MadeOf` and `Edges`), `time` (`SheetTime` with a `TimeUnit`), `cells`, `agents` (`AgentKind`), `ground` (`SheetGround` with `BlockedState`), `events`, `parameters` (`SheetParameter` with a free-text `unit`), `measures` (`SheetMeasure`, `MeasureRule`), `colors` (`ColorRule`, `ColorShade`, `ShadeScale`), `buttons` (`SheetButton`, `ButtonStep`) and `cards` (`SheetCard`, `CardRuns`).
  `SheetDocument::line(id)` finds a line and its card.
  Its `Debug` prints the id and the counts of cards and lines.
- A property holds its `PropertyType`, `SheetState`s with colours as four bytes, `Marks`, `kept_per_tick` and a `StartsAs` (`StartRule`, rows, `PaintedShape`s).
- A line is one `SheetLine`: a `LineKind` and every field and slot that any kind holds, a slot being a `Slot` (`Blank` or `Filled`).
  A filled slot holds a `SheetValue`: a `SheetLiteral`, a `SheetChip` (`Param`, `Result` with `part` and `property`, `My`, `MyNew`, `Their`, `Relation`, `State`), a phrase (`Box<SheetLine>`), or a `List` of literals and chips, which holds "is one of the states".
- The gate's stored forms: `LineKind::Always` for `{ kind = "always" }`, `start_state` holding `any_state`, `CompareTest::Outside` with `low` and `high`, and `rate_unit_from_tick`.
- Every enum has `ALL`, `as_str` and `parse` from one private macro, which also implements the crate-private `StoredName` the reader reads names through.

### The reader

`ParsedSheet::parse` returns the document, the text, `SheetPlaces` and the `LineEnds` map, with `document`, `into_document`, `text`, `places` and `place`.

1. A file past `MAX_SHEET_BYTES` is refused before parsing.
2. `ModelHeader` checks the header with the kinds `["sheet"]`, then `[sheet] version`: "the file is sheet version 2, and this build reads sheet versions up to 1", placed on the `2`, and "sheet versions count from 1" for 0. A newer file holding a new key is refused on its version, not on the key.
3. The text is parsed into a span tree, and a file whose tables and lists nest past `MAX_NESTING` (24) is refused at the deepest one.
4. The typed read: every table `deny_unknown_fields`, an unknown key's message cut to "unknown field `mystery`" without serde's list of the 50 keys a line can hold.
5. The conversion checks enum values, time units ("the time unit 'fortnight' is not one of second, minute, hour, day, week, month or year"), colours, finite numbers and the forms of values, each placed on its bytes.
   A table in a slot is a phrase when it holds `kind` and a chip otherwise. A phrase refuses a chip key, a chip refuses a line key, a chip names one thing, and `part` and `property` sit beside `result` alone.
   A line table without `kind` is refused at its header, and a card past `MAX_LINES_PER_CARD` at its table.
   A datetime reads as "a sheet holds no dates or times".
6. The places come from the span tree: `SheetPlaces::root` is a `TablePlaces` tree (`table`, `keys`, `tables`) that follows the TOML structure, and `SheetPlaces::line(card_index, line_id)` reaches a line table.

More than one `[[agents]]` table reads. Which keys belong to which line kind is left to the checker of S5.

### The percent literal

Round D's addendum to the gate (section 14, row D110) gives S1a a stored form for a number typed as a percent, and `feat: percent literal in sheets [draft]` adds it.
`{ percent = 30 }` reads as `SheetLiteral::Percent(30.0)` wherever a number literal can stand, in a slot, a list or a start's row, as an inline or a standard table, and `SheetLiteral::number` returns 0.3 for it, the number the checker of S5 and the lowering of S7 will read.
The reader keeps the number as typed, so the writer of S4 can write "30%" back, and refuses a percent holding another key, a value that is no number or a number that is not finite, each on its bytes.

### Limits

`sheet/limits.rs` holds `MAX_SHEET_BYTES` (1 MiB, as a graph file), `MAX_NESTING` (24), `MAX_LINES_PER_CARD` (256), `MAX_NODES_PER_LINE` and `MAX_SHEET_NODES` (999 each) and `MAX_SHEET_ID_BYTES` (48).
They sit outside `limits.rs`, so `library.lock` stays as it is (D41).
The reader enforces the first three.

### Test sheets and tests

The four examples of the author-file design are copied into `crates/henad-graph/src/tests/sheets/`, with the gate's changes: `{ kind = "always" }` on every condition-less Become, Set and Record (boids line 8, ants lines 10, 11, 13 to 16), `start_state = "any_state"` on the ants' lines 10 and 13, Game of Life's Clear a `run_once` button whose card `clear` holds line 4, "Become Dead from any state, always", with `next_line` 5, and SIR's `seed_outbreak` renamed `new_outbreak`, "New outbreak", on the button and its card.
The other comments stay as the author-file design's checker script wrote them, and S4 rewrites every comment.

`crates/henad-graph/src/tests/sheet_document.rs` holds fourteen tests.

| Test | What it holds |
| --- | --- |
| `every_sheet_reads` | The four sheets read, every line table has a place at its header, the ants' chip in `[[card.line.l7.row.condition.items]]` reads with its place, and no sheet nests deeper than half the limit (the ants reach 12) |
| `every_table_reads_its_values` | A value of every table kind, from the ants' painted ground to boids' relation chip and SIR's rate unit |
| `a_sheet_with_an_unknown_key_is_refused_on_the_key` | 13 tables, chips and inline records, each with `\n` and `\r\n` line ends |
| `a_newer_sheet_version_is_refused_on_its_key` | Versions 2 and 0, with a new key in the file, placed on line 6 |
| `the_header_check_names_three_kinds_with_or` | Three, two and one kinds, the sheet reader refusing a graph, another format and another version, and `read_header` reading the kind beside a header key of the wrong type |
| `an_unknown_time_unit_is_refused_on_its_key` | `tick_unit` and `rate_unit`, and every unit reading back |
| `a_blank_slot_reads_as_blank` | 33 deletions over each slot kind of the Game of Life and SIR lines and their phrases, a key deleted at a time, each filled before and empty after |
| `the_stored_forms_of_the_editor_read` | The outside test with a blank `high`, the tick's unit mark, a list of state chips, a half-built line, two agent kinds |
| `a_percent_reads_as_typed_wherever_a_number_can_stand` | Percents in SIR's start row, in a rate as an inline table and in a chance as a standard table, in Game of Life's compare slot and list, with their places, the number each stands for, and a percent kept apart from its decimal |
| `malformed_values_are_refused_on_their_bytes` | 14 malformed values, a datetime in a slot among them, each placed |
| `a_malformed_percent_is_refused_on_its_key` | A string, a boolean, NaN, a chip key and `kind` beside `percent`, and `percent` in a line table, each placed |
| `colours_and_sizes_follow_their_rules` | A lower-case colour with alpha, a card past the limit, a file past the size limit |
| `the_sheet_reader_never_panics_on_the_mutation_corpus` | 1600 random edits in the style of `tests/mutation.rs` (lines removed, copied, swapped, values replaced, percents among the values, lines inserted, truncation) and every key line deleted in turn, at least a quarter reading |
| `nesting_past_the_limit_is_refused_without_a_panic` | Phrases nested to the limit, one past it, past a header's reach and past TOML's, in standard and inline tables |


### Docs and topic files

- `agents/henad-graph-sheet.md` (new) holds the sheet module, with its row in `AGENTS.md`'s topic table.
- `agents/henad-graph.md` points to it and describes the header check's new home.
- `agents/henad-explore.md` names `ModelHeader` in both readers and the "or" message.
- `agents/henad-core.md` holds `TimeUnit`.
- `agents/facade-and-tutorial.md` names `TimeUnit` in `named_only` and `sheet` among the facade's graph modules.
- One CHANGELOG line under Unreleased.

Sizes after the change, the rebase onto S3 and the percent literal: `agents/henad-graph.md` 28,072 characters, `agents/henad-graph-sheet.md` 7,611, `agents/henad-explore.md` 28,323.

### Edited tree

```
AGENTS.md                                    topic row for henad-graph-sheet.md
CHANGELOG.md                                 sheets, the percent literal, TimeUnit, ModelHeader
agents/
├── facade-and-tutorial.md                   TimeUnit in named_only, sheet among the graph modules
├── henad-core.md                            TimeUnit
├── henad-explore.md                         ModelHeader in ModelFile and GridRuleFile, the "or" message
├── henad-graph-sheet.md                     new: the sheet module
└── henad-graph.md                           pointer to the sheet file, the header check
crates/
├── henad/
│   ├── src/lib.rs                           TimeUnit at the root, sheet in henad::graph
│   └── tests/facade_paths.rs                TimeUnit, ParsedSheet, SheetFileError, SheetDocument
├── henad-core/src/metadata.rs               TimeUnit and its known answers
├── henad-explore/src/
│   ├── model_file.rs                        HeaderTable replaced by ModelHeader
│   └── tests/model_file.rs                  the dispatch message with "or"
└── henad-graph/src/
    ├── lib.rs                               pub mod sheet
    ├── model_text.rs                        ModelHeader, HeaderProblem, read_header
    ├── reader.rs                            the header through ModelHeader, check_library
    ├── sheet/
    │   ├── mod.rs                           new: module doc, KIND_SHEET, SHEET_VERSION, SheetFileError
    │   ├── document.rs                      new: SheetDocument and its tables, SheetLiteral::Percent
    │   ├── reader.rs                        new: ParsedSheet, SheetPlaces, TablePlaces, the percent
    │   └── limits.rs                        new: the sheet's limits
    └── tests/
        ├── mod.rs                           sheet_document
        ├── sheet_document.rs                new: the reader's tests
        └── sheets/                          new: game_of_life, sir, boids and ants as sheets
```

### Deviations from the plan

- **The examples are not copied unchanged.** The gate's section 11 already says so: D81's values, D97's name and D98's Clear are written in.
- **`MAX_NESTING` is new.** The mutation test overflowed a test thread's 2 MiB stack: each nested phrase is a large frame of the typed read, and a debug build overflowed between 30 and 40 levels, where TOML's own limit sits near 80. The reader now parses the text into a cheap span tree first and refuses a file nested past 24 levels before the typed read. The ants reach 12, and a sheet at exactly 24 reads in debug and release builds.
- **`MAX_LINES_PER_CARD` is 256**, a number the plan leaves open. The reader enforces it with the file size and the depth, as size bounds of untrusted text. The id length and the node counts wait for the checker and the lowering.
- **The places come from a second parse.** `SheetPlaces` is one `TablePlaces` tree built from a span tree of the whole text, in place of hand-written place structs per table. It follows the TOML structure, so a key the document does not name still has a place.
- **The reader's entry point is `ParsedSheet`.** The plan names no type for S1a. S7's `SheetFile` can hold one.
- **`ModelHeader` reads the version keys of both kinds**, `[graph] library` and `[sheet] version`, so each reader parses its header once. henad-explore now reads `format_version` as an `i64`, as the graph reader did, so a negative version reports the version message where it was a type error.
- **The header message is one for every reader**: "this reader only reads" with the list. The graph reader's message already had that form, and `ModelFile`'s said "this build only reads ... and ...".
- **The facade's `henad::graph` lists `sheet`**, as the facade rule asks of every public module of henad-graph, and its doc names `ParsedSheet`.
- **The reader takes any line key on any kind.** The plan's typed read refuses unknown keys, and S5's checker holds keys to kinds, as the author-file design's checker script never did either.
- **`branches` is a map by state id.** The file order is not kept, since the canonical writer orders branches by the property's states.
- **Enum values are `Format` errors.** A misspelt value is placed on its bytes with the list of values it can take, where serde's own message would be a `Toml` error.
- **Colours read in either case.** The author-file design's checker script accepts upper case alone. The canonical writer of S4 writes upper case, so a lower-case colour takes its canonical form at the first save.

### Review

A review of the two commits found two "should fix" findings and five nits, and `fix: s1a review findings [draft]` fixes all seven.
`read_header` deserialized the whole `ModelHeader`, so a header key of the wrong type, such as `format_version = "1"` or `[sheet] version = "one"`, hid a kind that was present.
It now reads a private struct holding `kind` alone, and `the_header_check_names_three_kinds_with_or` holds the kind beside four wrong-typed keys.
This record named the design sources by their gitignored file paths, and now names them by design and section.
A datetime in a slot reported the TOML crate's private key, "unknown field `$__toml_private_datetime`".
The reader now reports "a sheet holds no dates or times", and `malformed_values_are_refused_on_their_bytes` holds the message and its place.
The header's kind list and the reader's value list now share the crate-private `or_list`.
Four doc comments lost their trailing "which" clauses, the `TimeUnit` doc no longer names the time scale of S2, the topic file no longer phrases the migration hook as a plan, the blank-slot test is described as covering each slot kind, and two hard-wrapped paragraphs of the topic files were rewrapped.

## Decisions adopted for review

The rows of the plan's section 6 and the gate's sections 7 and 14.6 that S1a builds, as written there.

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D7 | The kind's name | the author-file design, section 7, question 1 | (a) `sheet` | S1a |
| D9 | Where the code lives | the author-file design, section 7, question 3 | (a) `henad_graph::sheet`, no new dependency edge | S1a |
| D16 | The file extension | the author-file design, section 7, question 10 | (a) `.toml` | S1a |
| D14 | Where a rate's unit lives | the rates design, section 3 ("A parameter carries its unit ... so one parameter cannot be read in two units", the basis of its question 3 (a)) against the author-file design's question 8 (a) and round A's F8 | **Conflict.** The line holds its unit, takes its parameter's unit when one is put in, and warns on a mismatch. Both are stored and the line's governs, as the prototype researchers used. The rates design's section 3 would make the parameter's unit the only one, shown on the line as plain text. The prototype's form is taken because round B tested it and the sentence "at rate [r] per [unit]" of answer 6b names the unit on the line. One parameter can then be read in two units on two lines, the case the rates design's section 3 rules out, and the mismatch warning is the only guard | S1a, S21, S25 |
| D39 | How a half-built line is stored and run | both drafts (P1) | An absent slot key, read as blank and written as nothing. Line-local problems are skip warnings in the final specification's words, and the run skips the line and every line that reads it. File-level problems are errors | S1a, S5, S7 |
| D40 | A migration hook and the sheet version | foundations draft (P2), vertical draft (P2), against row AF1 and author-file section 5 | **Departure** from AF1 and author-file section 5, which ask for the migration hook. No hook until a version 2 exists, since a hook with no migration has no caller (`AGENTS.md`, "Two real examples before an abstraction"), and `[sheet] version` stays 1 until 0.4 ships. The version check of section 5 is built (S1a), so a newer file is still refused on its key | S1a |
| D41 | Where the sheet's limits live | the henad-graph recon | `sheet/limits.rs`, so `library.lock` stays as it is | S1a |
| D42 | Sheet ids and reserved ids | the henad-graph recon | Sheet ids at most 48 bytes, and the IR's reserved parameter ids plus `tick_length` refused by the sheet checker | S1a, S5 |
| D81 | "always" and "any state" in the sheet | final specification 8.3 and 8.4, against author-file 2.4 and D39 | **Conflict.** The final specification's, which D39 already implies. Every condition slot holds `{ kind = "always" }` for always, and `start_state` holds the reserved id `any_state` for any state. An absent `condition` or `start_state` is a blank (D39). `any_state` joins the ids that minting refuses (S12) and that the checker refuses as a state id (S5). A When whose condition is always is a checker error, since a When cannot be always. The four author-file examples are copied with these values written in wherever a line has no condition or a Become has no `start_state` (S1a), and the sheets written from round 1's answers take them too (S1b) | S1a, S1b, S4, S5, S7, S12 |
| D82 | The condition "is less than ... or more than ..." in the sheet | round B's B19, round C C1, final specification 8.4 and 8.10 item 2 | **Plan's choice** of the stored form, which no design source has. A compare phrase with `test = "outside"`, `left`, `low` and `high`, allowed in a condition slot only, as the page allows it, while a Compare line keeps its eight comparisons. It holds when `left` is below `low` or above `high`, counts as one test of its value for "otherwise", and lowers to two compares joined by an or. A blank `low` or `high` is a blank (D39), and the editor shows the one-sided offer of 8.10 item 2 | S1a, S4, S5, S7, S12, S19 |
| D83 | A rate's unit taken from the tick | round C C5, final specification 8.3, 8.9 and 12 (`unitFromTick`) | **Plan's choice** of the key. A rate phrase writes `rate_unit_from_tick = true` beside `rate_unit` while the unit came from the tick and was not chosen by hand, and writes nothing otherwise. The lowering reads `rate_unit` alone, so the mark stays outside the code identity. A new rate takes the tick's unit when one tick has a length, a parameter put into the rate replaces the unit with its own and clears the mark, and a unit chosen by hand clears it | S1a, S4, S12, S25 |
| D97 | "Seed outbreak" | final specification 4 and 17 ("Builder") | Renamed "New outbreak", id `new_outbreak`, on the button and its Run once card in `sir.toml` and `sir_chance.toml`, since "seed" is a forbidden word | S1a, S1b |
| D98 | Game of Life's Clear button | final specification 7.7 and 16, the prototype's notes, "Left out", against the author-file design's Game of Life example (`set_everywhere`) | The final specification's: Clear is a Run once button whose card holds "Become Dead from any state, always". `set_everywhere` stays in the schema for the ground, which has no card (the ants' Reset colony), and no grid control writes it. Without this, S16b's rebuild of Game of Life cannot equal the starter | S1a |
| D110 | A number typed as a percent | round D D8 part 1, the round D change list's D8, final specification 3, 8.5 and 12 (`pct`), against author-file 2.3 (a literal is a number, true or false, or a list of numbers) | **Conflict.** The final specification's: a number typed with "%" keeps showing as typed ("30%") on its chip, in the Number box when its picker reopens, in result names ("30% chance happens"), in sentences and in a Starts as field, and a note prints the decimal. **Plan's choice** of the stored form, which no design source has: a percent literal `{ percent = 30 }`, allowed wherever a number literal is, its number written under author-file 4.2's number rule. A percent can stand in any number slot, and D83's form, a key beside one slot, would need a key per slot. The checker applies D75's ranges to the number divided by 100, and the lowering writes that quotient. "30%" and 0.3 lower to the same IR. A parameter's default, min and max stay plain numbers, since the New parameter dialog keeps the decimal (section 10.14). S1a takes the literal in a follow-up commit before S4 starts | S1a, S4, S5, S7, S12, S16a, S19 |

What S1a built of each: D14 stores both units, the parameter's free-text `unit` and the line's `rate_unit`, and leaves the mismatch warning to S21. D39 reads an absent slot as `Slot::Blank`, and the skip rules wait for S5. D42 holds the 48-byte limit as `MAX_SHEET_ID_BYTES`, which the checker of S5 enforces. D81 reads `{ kind = "always" }` as `LineKind::Always` and keeps `any_state` as a string. The checker rules of D81 and D82 wait for S5. D110 reads the percent literal as `SheetLiteral::Percent` with `SheetLiteral::number`, and its ranges, sentences and lowering wait for S5, S4 and S7.

## Benchmark commands left for the maintainer

None.
The reader runs when a sheet opens or saves, outside every tick, so the milestone has no performance question.

## State after

- `37-node-editor` holds four unsigned `[draft]` commits after `5967809`, fast-forwarded from `redesign/s1a-sheet-document`: `feat: time units in henad-core [draft]`, `feat: sheet document and reader [draft]`, `fix: s1a review findings [draft]` and `feat: percent literal in sheets [draft]`, then this record and its `zensical.toml` entry.
  The branch was rebased onto `5967809`, the tip after S3, before the merge.
  `AGENTS.md`'s topic table conflicted in its list, and the merge keeps both rows, S3's `agents/henad-graph-wgsl.md` and S1a's `agents/henad-graph-sheet.md`.
  `agents/henad-graph.md` merged without a conflict.
- Before the rebase, in the worktree, with `HENAD_REQUIRE_GPU=1`:
  - `cargo fmt --all -- --check` passes.
  - `cargo clippy -p henad-core -p henad-graph -p henad-explore -p henad --all-targets --all-features -- -D warnings -W clippy::all` passes, and `-W missing_debug_implementations` over henad-graph lists nothing.
  - `cargo test -p henad-core -p henad-graph -p henad-explore -p henad --all-targets --all-features` passes: henad-core 294, henad-explore 293, henad-graph 186 with 1 ignored, and the facade's 6. The doc tests of the three crates pass, the sheet module's example among them.
  - `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph -p henad-core -p henad-explore --no-deps --all-features` and `./scripts/check_packaging.sh` pass.
  - `cargo check --lib --target wasm32-unknown-unknown` passes for henad-core, henad-graph and henad-explore with every feature, and for the facade with none and with every feature. henad-cli, henad-app and henad-models typecheck with every target.
- After the percent literal, `cargo fmt`, `cargo clippy -p henad-graph -p henad-core --all-targets --all-features -- -D warnings -W clippy::all` and the tests of both crates pass with `HENAD_REQUIRE_GPU=1`: henad-core 294, henad-graph 193 with 1 ignored, and their doc tests.
- The full check of the plan's section 2.3 at `4bac972`, the branch tip before this record, run from the worktree in three chunks with `HENAD_REQUIRE_GPU=1`, equals the baseline at `be338ec`.
  Every step exits 0 but `deny` (1) and `test` (101), the two the baseline fails.
  `cargo deny` reports RUSTSEC-2026-0370, 0371 and 0372 alone, with bans, licences and sources ok.
  The workspace tests pass 1682 with 2 ignored, and the one failure is the baseline's budget test of record #68, so the list of failing tests holds nothing the baseline's lacks.
  The doc tests, the README program, the docs build, the scripts' tests and the web build pass.

## Issues found & future directions

- **The stack and the nesting limit.** 24 levels hold in a debug test thread, about 20 of them phrase tables. The wasm32 stack was not measured at the limit. A sheet built by the editor nests far less, and the limit can drop if a browser run overflows.
- **Three parses per read.** The header, the span tree and the typed read each parse the whole text. A sheet holds at most 1 MiB, and the reader runs on open and save. If S13a reparses on every edit, a measurement there decides whether the span tree should come from the typed read.
- **Comments in the test sheets.** Their generated comments keep the wording before the final specification ("Become Alive if Dead"), apart from the lines added here. S4 rewrites every comment with the Rust writer.
- **Round D.** The gate's section 14 reopened S1a's stored form with one row, D110, and `feat: percent literal in sheets [draft]` takes it before S4. The test sheets hold no percent, since no example types one, and S1b's sheets written from round 1's answers write none either.
- **Merging beside S3.** S3 moved two sections of `agents/henad-graph.md` into a new file, and S1a edits the module paragraph and the reader paragraph of the same file. The hunks merged apart, and `AGENTS.md`'s topic table conflicted only in its list.
- **Keys per kind.** The reader takes any line key on any kind. S5's checker needs the per-kind key table of author-file section 2.4 to refuse a `test` on a Become.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
