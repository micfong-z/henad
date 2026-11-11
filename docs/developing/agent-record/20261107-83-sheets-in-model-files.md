---
date: 2026-11-07
title: "Sheets in model files"
description: "S11 of the #37 redesign. ModelFile gains a third kind, Sheet, with sheet() and lowered_graph(), and builds a sheet as the graph it lowers to. SheetFile::place_graph_diagnostic places every finding about the lowered graph at the table of the sheet its node came from, with no pass and no node. henad-cli runs a sheet with --model-file and prints a model file's graph with --print-lowered, and the facade names KIND_SHEET and the sheet types."
icon: material/file-document-check-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s11-sheet-model-files, rebased onto S12's record cf3ce8c: two unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 31422d4
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sheets in model files, the CLI and the facade [draft]`, `fix: s11 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheets in model files

> S11 takes a grid sheet from henad-graph to the places a researcher meets a model file.
> `henad_explore::model_file::ModelFile` reads `kind = "sheet"` as a third kind, `Sheet(Box<SheetFile>)`, and builds it as the graph it lowers to, under that graph's code.
> `SheetFile::place_graph_diagnostic` places a finding of the graph checker, the host evaluator or the WGSL compiler at the line table its node was lowered from, through the `LineMap`, and clears its pass and node (D46).
> `henad-cli --model-file crates/henad-graph/sheets/game_of_life.toml` runs the Game of Life sheet, a broken copy prints the author line and no node or pass, and `--print-lowered` prints the graph a model file builds as.
> The facade names `KIND_SHEET` and the sheet types, and the model file, command line and API reference pages describe sheets.

## State before

- The branch was cut from `37-node-editor` at `31422d4` (S7 merged). At the merge point it rebased onto the tip of `37-node-editor`, `cf3ce8c`, which holds S12.
  S10 had not merged by then, so S11 merges before S10, against the order this record's issue 4 first assumed.
- `SheetFile::parse` read, checked and lowered a grid sheet, and its `LineMap` named the line or part of the sheet behind each node of the lowered graph.
- `ModelFile` read two kinds, `gpu_grid` and `graph`, and refused `sheet` with "this reader only reads 'gpu_grid' or 'graph'", so no host could run a sheet.
- The graph checker's diagnostics carried a pass, a node and a place in the graph's own text, which for a lowered sheet is a text the researcher never wrote.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Placing graph findings in the sheet

`SheetFile::place_graph_diagnostic(diagnostic)` (`crates/henad-graph/src/sheet/file.rs`) maps a diagnostic of the lowered graph back into the sheet (R2, D46).

- A node the `LineMap` owns by a line is placed at that line's `[card.line.<id>]` table, found through the card whose `line_tables` hold the line, since line ids are unique in the file.
- A node the sheet owns is placed by the first two segments of its slot path: `cells.<property>` at the property, `measure.<id>`, `button.<id>`, `card.<id>` and `color.<thing>` at their tables, any other path at `[sheet]`.
- A node the map does not know, such as a copy normalisation makes, is placed at `[sheet]`.
- A diagnostic with a pass and no node is placed at the card of a `tick_<card>` pass or the button of a `button_<id>` or `restart_<id>` pass, otherwise at `[sheet]`.

The old span, line and column are replaced, and `pass` and `node` are cleared, so henad-cli's `located` prints no "in node ... of pass ..." note.
`SheetFile::parse_planted`, crate-private and `#[cfg(test)]`, runs a closure on the lowered document before it is read back, and the tests plant a node the checker refuses through it.

### The third kind of model file

`crates/henad-explore/src/model_file.rs`:

- `ModelFile::Sheet(Box<SheetFile>)`, read when the header's kind is `sheet` (the header check now takes `gpu_grid`, `graph` and `sheet`, and a refused kind's message lists "'gpu_grid', 'graph' or 'sheet'").
- `ModelFileError::Sheet(Vec<CodeDiagnostic>)`, and `From<SheetFileError>`: `Toml` to `Toml`, `Format` to `Format`, `Invalid` (the checker's or the lowering's errors) to `Sheet`.
- `sheet()`, the sheet, and `lowered_graph()`, the graph the model builds as: a graph file's own graph, a sheet's lowered graph, `None` for a grid rule.
  `graph()` stays `None` for a sheet, so henad-models' corpus loaders never meet one.
- `id` is `[sheet] id`, which is also the lowered graph's target id, `to_toml` the sheet's canonical text, and `code` the lowered graph's code.
- `entry` of a graph and of a sheet call the one private `graph_entry`, which takes a placer: the identity for a graph, `place_graph_diagnostic` for a sheet, whose errors then come as `ModelFileError::Sheet`.
- `warnings` of a sheet are the sheet checker's, already placed.
- A grid sheet with no property lowers to a graph with no lane and no pass, whose checker errors are in IR words ("a grid model needs a lane of cells"). `entry` of such a sheet returns its `SHEET_EMPTY` warning, raised to an error at `[world]`, before the graph is built.
- The size limit of a sheet is the reader's `MAX_SHEET_BYTES` (1 MiB), reached through `SheetFile::parse` and documented on `ModelFile::parse`.

The crate doc and the `model-files` feature comment name the three kinds.

### The command line

`crates/henad-cli/src/lib.rs`:

- A sheet needs no code of its own in `--model-file`: `ModelFile` places its problems, and `located` prints them at the author's line.
  The crate doc names the three kinds and gains a sheet run and a `--print-lowered` line among its examples.
- `--print-lowered` (`Mode::PrintLowered`, checked first) requires `--model-file`, takes exactly one (`parse_args` refuses another count, exit 2), and conflicts with a model id, `--list`, `--params`, `--info`, `--json`, the exports, `--stats-every`, `--merge`, every sweep flag and every run flag (`--steps`, `--warmup`, `--global-warmup`, `--seed`, `--reps`, `--set`, `--act`, `--threads`).
  It returns before the model set is touched or a device acquired, prints `lowered_graph`'s canonical text on stdout and the warnings on stderr, and refuses a grid rule with exit 1.
- `read_model_file` and `load_error` hold what `load_model_file` and `print_lowered` share.
- `existing_invocations_keep_their_mode` expects `Mode::PrintLowered` of a documented line holding the new flag, since no line written before it can.

### The facade

`crates/henad/src/lib.rs` re-exports `henad_graph::sheet::KIND_SHEET` in `henad::graph` beside `KIND_GRAPH`, the `rules` doc names the three kinds a `ModelFile` holds and says what its code is for each, and the `graph` doc names `SheetFile`.
`crates/henad/tests/facade_paths.rs` gains `SheetTypes` (`SheetFile`, `SheetErrors`, `LineMap`, `NodeOrigin`, `NodeOwner`, `SheetPlaces`, `TablePlaces`), `GRAPH_KINDS` and `sheet_starters_build`, in the `named_only` module beside `graph_starters_check`.

### Tests

- `crates/henad-graph/src/tests/sheet_lower.rs`: `a_graph_diagnostic_is_placed_at_its_author_line` plants an unknown op on a node of line `l2`, takes the checker's error on it (pass `tick_cell_tick`), and places it at `[card.line.l2]` with no pass or node, then a node the sheet owns at `[[cells.property]]` and a pass alone at its card and its button. `an_unknown_node_is_placed_on_the_sheet` plants node `n900000`, and places it, a pass of no card and a bare warning at `[sheet]`.
- `crates/henad-explore/src/tests/sheet_file.rs`, new: `every_sheet_starter_reads_checks_and_builds_an_entry` over `sheet::starters()` (no graph, no warning, the entry's code hash the file's, the canonical text read back to the same code), `a_sheet_error_comes_back_placed_at_its_line` (a checker error at `[card.line.l2]` and the rate refusal of an added line at `[card.line.l5]`, neither naming a pass or node, an unknown key, a newer sheet version), `a_sheet_warning_comes_back_placed_at_its_line`, `a_result_no_line_reads_gives_no_warning_about_the_graph`, `a_sheet_with_no_property_is_refused_in_its_own_words` and `a_sheet_is_held_to_its_size_limit` (past the limit refused with the limit named, at the limit read).
- `crates/henad-explore/src/tests/model_file.rs`: `the_header_dispatches_on_the_kind` expects the three kinds in a refused kind's message, reads the Game of Life sheet as a sheet, and reads the same text marked as a graph through the graph's reader.
- `crates/henad-cli/tests/model_file.rs`: `a_sheet_model_file_runs_and_prints_its_problems_at_its_lines` reads `../henad-graph/sheets/game_of_life.toml` by path, runs it for 20 steps where a device is, and refuses a broken copy at `broken.toml:95:1` with neither "node" nor "pass" in the output, and a copy with no property at its `[world]` line with no "lane" either. `print_lowered_prints_the_graph_a_sheet_lowers_to` reads the printed text back as a canonical graph with the sheet's code and id, prints a graph file's own text, refuses a grid rule, and holds the flag's conflicts, `--set`, `--act`, `--steps` and `--threads` among them.
- Unchanged and passing: every graph `ModelFile` test, the tests that read graph starters by name, `facade_paths` and `library_lock`.

### Docs and topic files

- `docs/reference/model-files.md`: the three kinds at the top and in the header table, and a Sheets section holding `crates/henad-graph/sheets/game_of_life.toml` whole, the format of a sheet, problems at their lines, the code identity of a sheet and its limits.
- `docs/reference/cli.md`: `--print-lowered` in the flag table, a Sheets subsection under Model files with the real error of a broken copy, and the manifest's code of a graph or sheet.
- `docs/reference/api.md`: henad-graph's row, the `model-files` feature and the `henad::graph` row name sheets and `KIND_SHEET`.
- `agents/henad-explore.md` (the third kind, `lowered_graph`, the size limit, the warnings, the tests; 31,335 characters), `agents/henad-cli.md` (author lines, `--print-lowered`, the tests), `agents/facade-and-tutorial.md` (`KIND_SHEET`, `SheetTypes`), `agents/henad-graph-sheet.md` (the mapping and its tests; 33,188 characters).
- `AGENTS.md`: a sheet run beside the graph starter run in the command list.
- One CHANGELOG line under Unreleased.

### Edited tree

```
AGENTS.md                                        a sheet run in the command list
CHANGELOG.md                                     sheets in model files
agents/
├── facade-and-tutorial.md                       KIND_SHEET, SheetTypes
├── henad-cli.md                                 author lines, --print-lowered
├── henad-explore.md                             the third kind, lowered_graph, the size limit
└── henad-graph-sheet.md                         place_graph_diagnostic and its tests
crates/
├── henad/
│   ├── src/lib.rs                               KIND_SHEET, the rules and graph docs
│   └── tests/facade_paths.rs                    SheetTypes, GRAPH_KINDS, sheet_starters_build
├── henad-cli/
│   ├── src/lib.rs                               --print-lowered, read_model_file, load_error, crate doc
│   └── tests/model_file.rs                      the sheet run and --print-lowered
├── henad-explore/
│   ├── Cargo.toml                               the feature comment names three kinds
│   └── src/
│       ├── lib.rs                               the crate doc names three kinds
│       ├── model_file.rs                        ModelFile::Sheet, ModelFileError::Sheet, graph_entry
│       └── tests/
│           ├── mod.rs                           sheet_file
│           ├── model_file.rs                    the header dispatch over three kinds
│           └── sheet_file.rs                    new
└── henad-graph/src/
    ├── sheet/
    │   ├── mod.rs                               the module doc names the mapping
    │   └── file.rs                              place_graph_diagnostic, parse_planted
    └── tests/
        └── sheet_lower.rs                       the two mapping tests
docs/reference/
├── api.md                                       the sheet module
├── cli.md                                       sheets, author lines, --print-lowered
└── model-files.md                               three kinds, the Sheets section
```

### Deviations from the plan

- **One commit.** The record goes into the repository at the merge step, so the branch holds `feat: sheets in model files, the CLI and the facade [draft]` and the review's `fix: s11 review findings [draft]`, and no record commit until the merge step.
- **The lowered graph's warnings are left out of a sheet's warnings.** The author-file design forwards them mapped, and S7's issue 7 asked S11 to filter or word them. The graph checker has two warnings, and each message names a node of the graph ("Neighborhood fold n2001 reaches nothing the pass keeps, and will be dropped", and a Repeat whose count can pass its `max`). The first comes from a result no kept line reads, which costs nothing and asks nothing of the author. The second needs Repeat, which S20 lowers, and S20 can give it a sheet warning in the wording table's words. `lowered_graph()` and `--print-lowered` keep both within reach.
- **A node the sheet owns and a pass alone are placed at a table of their own.** The plan places a diagnostic at its line, or at `[sheet]` for an unknown node. A node of a start, a measure, a colour rule, a button step or a card's shared change sits in the `LineMap` under the sheet, so its slot path names its table, and a pass names its card or button. Both fall back to `[sheet]`.
- **`lowered_graph()` returns a graph file's own graph too**, the graph the model builds as, so `--print-lowered` prints a graph file's canonical text and refuses only a grid rule.
- **The size limit already existed.** The reader holds a sheet to `MAX_SHEET_BYTES`, so `ModelFile` reaches it through `SheetFile::parse`, and `a_sheet_is_held_to_its_size_limit` holds it at the model file.
- **The sheet starter check in `facade_paths.rs` is a named function**, as `graph_starters_check` is, since that test builds no entry outside its `named_only` module. henad-explore's `every_sheet_starter_reads_checks_and_builds_an_entry` runs the same check.
- **The command lines in the docs and the crate doc name the model id `game_of_life`** where they run the sheet, so `existing_invocations_keep_their_mode` reads them as the benchmark mode it already knows.

## State after

- A grid sheet is a model file: it reads, checks, lowers and builds through `ModelFile`, under the lowered graph's code, and every problem comes back at a table of the sheet.
- `henad-cli --model-file` runs a sheet and prints its problems at the author's lines with no pass and no node, and `--print-lowered` prints the graph.
- The facade names `KIND_SHEET` and the sheet types, and the reference pages describe sheets.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph -p henad-explore -p henad-cli -p henad --all-targets --all-features -- -D warnings -W clippy::all`; `cargo check --workspace --all-targets`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-explore -p henad-cli -p henad --all-targets --all-features --no-fail-fast` (henad-graph 330 passed and 1 ignored, henad-explore 304, henad-cli 32 in the library and 12 in `tests/model_file.rs`, every other binary passing); the doc tests of the four crates and `cargo test -p henad --doc --features example-models,app`; `RUSTDOCFLAGS="-D warnings" cargo doc` over the four crates; `./scripts/check_packaging.sh`; `cargo check --lib --target wasm32-unknown-unknown` of henad-graph and henad-explore with every feature and of the facade with and without features.
  The full check runs at the merge point.

### Merge check

- The rebase onto `cf3ce8c` (S12) conflicted in four files, each resolved by keeping both milestones' text.
  `CHANGELOG.md` keeps S12's sheet edit commands line and S11's line after it.
  `agents/facade-and-tutorial.md` names `keys` and `KIND_SHEET` in the `graph` sentence, and `SheetBuilder`, `SheetEdit`, `KeyStream`, `SheetTypes`, `GRAPH_KINDS` and `sheet_starters_build` in the `named_only` sentence, which ends "Keep them." as the review asked.
  The module doc of `crates/henad-graph/src/sheet/mod.rs` names `SheetFile::place_graph_diagnostic` and S12's `edit` module.
  The `graph` doc of `crates/henad/src/lib.rs` keeps S12's sentences on `SheetBuilder` and `KeyStream`, then S11's on `SheetFile` and the `sheet` kind, and the `pub use` list holds S12's `keys` beside S11's `KIND_SHEET`.
- The full check with `HENAD_REQUIRE_GPU=1` on the rebased branch: every step exits 0 but `cargo deny`, which reports only the baseline's RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests, whose one failure is the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`.
  The failing test list holds no name outside the baseline's, and the test log shows one failed target, henad-models' library, with that one test.
  No fix was needed.

## Issues found & future directions

1. The app's Apply already goes through `ModelFile::parse(...).and_then(entry)`, so a sheet's text in the Editor's code view would now build. S13a gives a sheet its own editor and dispatches open, save and Apply on the kind.
2. A graph checker error on a lowered sheet keeps the graph checker's words, and can name a node such as `n2001`. Only its place and its pass and node fields change. One case an author reaches is handled: a sheet with no property, which S7's issue 2 handed to S11, is refused at `[world]` in the words of its own warning before the graph is built. Every other graph checker error found so far points to a lowering fault rather than an author's mistake. If one shows up in practice, the lowering is fixed, or the error gains wording in the wording table.
3. `--print-lowered` prints the graph a sheet lowers to with no `[editor]` region. The conversion to a node graph (S23) writes one with each line's frame.
4. S11 merged before S10, so S10's merge step carries what follows. The merge with S10 conflicts in three files, checked on a scratch merge of the two branches. `CHANGELOG.md`: S10 rewrites the sheet lowering line, so S10's two lines are kept and S11's line follows them. `agents/henad-graph-sheet.md` conflicts in its text as well as in lists: the tests paragraph takes S10's text, then S11's two sentences on the mapping tests (37,282 characters after the merge). `crates/henad-graph/src/tests/sheet_lower.rs` conflicts in its list of tests only, and keeps both sides. With those resolutions the tests and clippy of henad-graph, henad-explore and henad-cli pass on the merge. S10's merge step also adds a row to the limits of a sheet in `docs/reference/model-files.md` for S10's `sheet::limits::MAX_FORMULA_DEPTH`, "Operations nested in one formula | 16 deep".
5. The sheet starters' ids match example models (`game_of_life`), so `--model-file sheets/game_of_life.toml` replaces the example model of that id and prints a note. That is the intended `--model-file` behaviour, and a sweep of it records the runtime code.

## Review

A reviewer with a fresh context found six problems, all fixed in `fix: s11 review findings [draft]`.
`a_sheet_error_comes_back_placed_at_its_line` planted a chance, which S10 lowers, and asserted `NOT_YET_CHANCE`, which S10 deletes, so the test target would not have compiled after the rebase onto S10. It now adds a rate line `l5` to the tick card and asserts `NOT_YET_RATE` at `[card.line.l5]`, a refusal that holds until rates lower at S25, and passes on a scratch merge with S10's branch.
A sheet with no property reached the author in graph words ("a grid model needs a lane of cells", "a grid model has exactly one tick pass, and this file has 0"). `ModelFile::entry` now returns the sheet's `SHEET_EMPTY` warning as the one error, at `[world]`, in words the wording table already holds for a blank model. `a_sheet_with_no_property_is_refused_in_its_own_words` and the henad-cli sheet test hold it, the second asserting no "node", "pass" or "lane" in the output. Issue 2 and the two reference pages now say that a graph checker error on a lowered sheet keeps the checker's wording.
The benchmark section had dropped record #81's two commands, and now holds them.
`--print-lowered` accepted and ignored the run flags. It now conflicts with `--steps`, `--warmup`, `--global-warmup`, `--seed`, `--reps`, `--set`, `--act`, `--threads` and `--stats-every`, and the CLI test refuses four of them.
The writing nits are fixed: the summary paragraphs of `ModelFile::to_toml` and `ModelFile::warnings`, the trailing "which" clauses in the grid rule refusal of `--print-lowered` and in the `warnings` doc, the verb agreement on the model files page, and "Keep them." in the facade topic file.
The limits row for S10's `MAX_FORMULA_DEPTH` waits for S10's merge step, since the constant does not exist on this branch, and issue 4 names it.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D46 | IR words in diagnostics | the foundations draft (P10) | Mapped diagnostics drop their pass and node, and `--print-lowered` keeps the IR within reach | S11 |

The gate adds no row whose "Built in" names S11.

## Benchmark commands for the maintainer

Record #81 (S7) left two commands that needed S11's sheet support in `henad-cli`, and they run from S11 on.
They compare the lowered Game of Life sheet with the graph starter it matches node for node, a `grid.count` and a `cat.transition` in the tick pass, at 4096 by 4096.
Run them interleaved on a cooled machine, as `agents/performance.md` asks:

```bash
cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 3
cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 3
```

The sheet exposes `grid_width` and `grid_height` as the graph starter does.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
