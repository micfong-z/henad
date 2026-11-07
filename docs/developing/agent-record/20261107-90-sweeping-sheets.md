---
date: 2026-11-07
title: "Sweeping sheets"
description: "S18 of the #37 redesign. A model built from a sheet records the sheet's canonical text as its author text, outside the code hash and outside ModelSource equality, and a results folder records it as model.author. A note edit resumes and records the new text, a lowered IR change still refuses, a merge keeps the lowest shard's text, and a folder replays from its recorded graph with no sheet reader. In the app, Open in Editor in the Results tab opens the recorded sheet, or the recorded graph, Edit model opens a sheet's model as the sheet, and a note edit applies without a rebuild."
icon: material/file-restore-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s18-sweeping-sheets after its merge rebase onto d7b04f2 (S23 and S13b merged): three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test, and the egui MCP checks were repeated on the rebased branch"
baseline_commit: e0dece6
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sheet text in results folders [draft]`, `feat: open a sweep's sheet from results [draft]`, `fix: s18 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sweeping sheets

> S18 keeps a sheet with the results it produced (row AF9, decision D10).
> `ModelSource` gains an author text, `with_author`, `author` and `same_author`, which stays outside `code_hash()` and outside equality through a hand-written `PartialEq` (D51).
> A sheet's lowered `GraphFile` carries the sheet's canonical text in the format `henad-sheet/1`, and every entry built from it records that text in its source.
> A sweep's manifest records the text as `model.author` beside the lowered graph's `model.code`, `FORMAT_VERSION` staying 1, and `ResultSet::author` reads it.
> A note edit resumes and records the new text, a change to the lowered IR refuses as before, a merge keeps the lowest shard's text, and a folder replays from the recorded graph with no sheet reader.
> In the app, Open in Editor in the Results tab opens the recorded sheet, or the recorded graph or grid rule when no sheet is recorded, and Edit model on the Model tab opens a sheet's model as the sheet (the Results half of AF8).
> A note edit applied in the Editor reads "Model code unchanged.", rebuilds nothing, and still replaces the set's entry, so the next sweep records the sheet as it now reads.

## State before

- `37-node-editor` at `e0dece6`, S17 merged, with S13b and S20 planned beside S18.
- A sheet's entry recorded `runtime:<sheet id>` and the lowered graph's code `henad-graph/1`, and nothing of the sheet itself.
  A results folder made from a sheet held the IR alone, and Edit model on a sheet's model opened the lowered graph on the canvas.
- `ModelSource` derived `PartialEq` over every field, and the app decides that a model changed with `==` (`state/install.rs` `same_code`, and `apply` in `ui/editor/mod.rs`).
- The Results tab had no way back to the Editor.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Author text in henad-core (D51)

`ModelSource` holds `author: Option<ModelCode>`.
`with_author(code)` sets it, `author()` reads it, and `same_author(&other)` compares the author texts alone.
`PartialEq` is written by hand: it destructures `Self` with `author: _` and compares the type path, the build, the origin and the code, so a field added later fails to compile until it is placed.
`code_hash()` is unchanged.
`model_source_equality_ignores_the_author_text` builds one source per field, each differing from a base in that field alone, through a struct literal that names every field, and asserts that every field but the author breaks equality.

### The sheet text on the lowered graph (AF9)

`henad_graph::sheet::SHEET_CODE_FORMAT` is `henad-sheet/1`, the kind and the sheet version.
`GraphFile` gains a private `author` field, `author()`, and a crate-private `with_author`.
`SheetFile::parse` records the canonical text on the lowered graph, and `GraphFile::source()` returns `ModelSource::runtime(..).with_author(..)` when the graph carries one.
Both compilers already put `file.source()` in the entry, so henad-compute and `ModelFile::entry` are unchanged.
`GraphFile::from_code`, `parse` and `from_document` carry no author, so the graph's own text and code never hold the sheet.

### The manifest (D10)

`ManifestModel` gains `author: Option<ManifestModelCode>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`, filled beside `code` in `sweep.rs` from `entry.source().author()`.
`ManifestModelCode::to_code` rebuilds a `ModelCode` with its hash recomputed, and `ResultSet::author` returns the recorded author text.
The model block is rebuilt from the current entry each session, so a resume records the text it ran, and a merge clones the lowest shard's manifest.
Neither compares the author text, so a note edit between sessions resumes and a merge of two shards whose sheets differ in a note goes ahead.

### The app

- `ui/editor/mod.rs`: `EditableModel` (`Sheet`, `Graph`, `Rule`) and `editable_model(author, code)`, which returns the sheet of a `henad-sheet/1` author text that reads, else the graph or grid rule of the code.
  `EditorPanel::load_model` loads one from no file, and `open_editable` also brings the Editor to the front.
  `edit_model` and `can_edit` go through `editable_model`, so a sheet's model opens as the sheet.
  `editable_graph` and `editable_text` are gone, and their two tests read through `editable_model`.
- `apply`: when the set's entry has the same source but another author text, the entry is replaced through `install_models`, whose `same_code` then passes over it.
  The outcome stays `Unchanged` and the run keeps going.
- `ui/results/store.rs`: `RecordedModel` keeps the code and the author text, from the entry a sweep in the app started with or from a folder's manifest.
  `ResultsStore::editable_model` and `records_editable_code`, the second a check of the recorded formats alone for the toolbar.
- `ui/results/mod.rs`: the toolbar shows `OPEN_IN_EDITOR`, "Open in Editor", with a pencil icon and the tooltip "Open model this sweep ran in Editor" ("search" for a search), while the store records a model the Editor reads.
  The press sends `ResultsRequest::OpenInEditor`, and `open_in_editor` opens the model or sets the status line to "Recorded model does not read".

### Tests

- henad-core: `model_source_equality_ignores_the_author_text`.
- henad-graph `src/tests/sheet_lower.rs`: `a_lowered_sheet_records_its_canonical_text_outside_the_code` (the author text equals `to_toml`, a note keeps the source equal and the author text apart, `from_code` carries none, the format constant spells the kind and version).
  The `SheetFile` doc example reads the author text from the source.
- henad-explore `src/tests/provenance.rs`, module `sheet_text` behind `model-files`, on the device, over the Game of Life sheet at 16 by 16 swept over Initial density: `the_manifest_records_the_sheet_text`, `a_note_edit_resumes_and_records_the_new_text`, `a_lowered_ir_change_refuses_to_resume` (a state colour, same schema hash, manifest untouched), `a_merge_keeps_the_lowest_shards_sheet_text`, and `a_sheet_folder_replays_from_its_recorded_code_alone` (the recorded code reads through `ModelFile::parse` as a graph, every run replays, and a sweep of that graph writes the same tables as the sheet's sweep).
- henad-app `src/tests/sheet_editor.rs`: `a_note_edit_does_not_reinstall_the_model` (Apply after a note reads `Unchanged`, nothing rebuilds, the run passes tick 0, the set's entry holds the new text, and Edit model opens the noted sheet) and `open_in_editor_opens_the_sheet_a_sweep_recorded_or_else_its_graph` (a sweep in the app, a folder read through `open_folder`, a folder swept from the recorded graph alone that opens on the canvas, and a compiled model that shows no button).

### UI check on G15F1

Under the UI lock, with `app.ron` set to (-1800, 60) at 1600 by 960 and the app run from the worktree with `EGUI_INSPECTION=1 cargo run -p henad-app --features inspection`.
The screenshots, named below, stay out of the repository.

1. `1-results-folder-open-in-editor.png`: a folder written by `henad-cli game_of_life --model-file crates/henad-graph/sheets/game_of_life.toml --vary initial_density=0.2:0.4:0.2 --steps 10`, opened with `--open`, shows "Open in Editor" beside Open results and the two configs' Alive series.
2. `2-folder-sheet-in-editor.png`: the press opens the Editor with the Game of Life sheet and its card, three numbered lines and the Clear card, the preview paused at tick 0.
3. `3-sweep-tab-sheet-over-initial-density.png`: after Apply, the Sweep tab varies Initial density over 0.2 and 0.4 for 20 steps, 2 runs.
4. `4-app-sweep-results-editor-holds-a-rule.png`: the sweep finished, Show results opened the current sweep, and the Editor was switched to the Life grid rule with New grid rule.
5. `5-app-sweep-sheet-in-editor.png`: Open in Editor replaced the rule with the swept sheet.
6. `6-edit-model-opens-the-sheet.png`: with the grid rule loaded again, Edit model on the Model tab opened the applied sheet.

The app was quit, `app.ron` restored from its backup, the backup deleted and the lock released.

### Docs and topic files

- `docs/guide/sweeps.md`: sweeping a sheet with `--model-file`, what a resume of a sheet accepts, and the sheet text a resume records (the plan's risk sentence).
- `docs/reference/model-files.md`: "The sheet in a results folder", the `model.author` block, the resume, merge and replay rules, Open in Editor and `ResultSet::author`.
- `docs/guide/app.md`: Open in Editor under Opening results.
- `agents/henad-core.md` (the author text and the hand-written equality), `agents/henad-graph-sheet.md` (the lowered graph's author text), `agents/henad-explore.md` (the manifest field and its tests), `agents/henad-app-sweep.md` (Open in Editor), `agents/henad-app-editor.md` (`editable_model` in place of `editable_graph` and `editable_text`), `agents/henad-app-sheet.md` (Edit model and the note-only Apply).
  Sizes after the change: henad-explore 33157, henad-graph-sheet 28865, henad-app-editor 26659, henad-core 16821, henad-app-sweep 11440, henad-app-sheet 10464 characters.
- `CHANGELOG.md`: one Added line, and `ManifestModel.author` with the equality change under the breaking changes, plus `author: None` for a `ManifestModel` literal in the migration notes.

### Edited tree

```
CHANGELOG.md                                     sheet text in results; ManifestModel.author breaking
agents/
├── henad-app-editor.md                          editable_model, EditableModel, load_model
├── henad-app-sheet.md                           Edit model opens the sheet, note-only Apply
├── henad-app-sweep.md                           Open in Editor
├── henad-core.md                                ModelSource author text, hand-written PartialEq
├── henad-explore.md                             ManifestModel.author, ResultSet::author, tests
└── henad-graph-sheet.md                         the lowered graph's author text
crates/henad-core/src/
└── provenance.rs                                author, with_author, same_author, PartialEq, test
crates/henad-graph/src/
├── file.rs                                      GraphFile author, source() with the author text
├── sheet/
│   ├── mod.rs                                   SHEET_CODE_FORMAT
│   └── file.rs                                  the lowered graph records the canonical text
└── tests/sheet_lower.rs                         the author text outside the code
crates/henad-explore/src/
├── output/manifest.rs                           ManifestModel.author, ManifestModelCode::to_code
├── result_set.rs                                ResultSet::author
├── sweep.rs                                     the manifest records the author text
└── tests/provenance.rs                          module sheet_text, five device tests
crates/henad-app/src/
├── tests/
│   ├── editor.rs                                rule_text through editable_model
│   ├── graph_editor.rs                          the graph through editable_model
│   └── sheet_editor.rs                          note-only Apply, Open in Editor
└── ui/
    ├── editor/mod.rs                            EditableModel, editable_model, load_model, open_editable,
    │                                            edit_model, the set's entry on a note-only Apply
    └── results/
        ├── mod.rs                               Open in Editor button, request, open_in_editor
        └── store.rs                             RecordedModel code and author, editable_model
docs/
├── guide/
│   ├── app.md                                   Open in Editor
│   └── sweeps.md                                sweeping a sheet, what a resume accepts
└── reference/model-files.md                     the sheet in a results folder
```

### Deviations from the plan

- **A note-only Apply replaces the set's entry.**
  With equality blind to the author text, Apply after a note edit read `Unchanged` and left the set's entry with the old text, so a sweep started next would have recorded the sheet without the note.
  `apply` now replaces the entry when `same_author` fails, through `install_models`, which passes over an entry of the same code.
  Nothing reinstalls or rebuilds, as D51 asks.
- **`editable_graph` and `editable_text` are replaced by `editable_model`.**
  Edit model, `can_edit` and Open in Editor share one function over an author text and a code, so a folder's recorded model and an entry open the same way.
  Both removed functions sat in the app's private `ui` module.
- **The explore tests sit in a `sheet_text` module of `provenance.rs`.**
  They build the entry through `ModelFile`, which only the `model-files` feature compiles, and the plan names `provenance.rs`.
  A fifth test, `a_sheet_folder_replays_from_its_recorded_code_alone`, holds the acceptance line about replays with no sheet reader.
- **Open in Editor also opens a recorded grid rule**, as Edit model does, and shows only while the store records a model the Editor reads, so a compiled model's results show no button.
- **`docs/guide/app.md`** gains a paragraph under Opening results, which the plan's docs list leaves out, since the other two pages link to it.
- **The UI check's folder came from henad-cli**, opened with `--open`, and the Sweep tab's sweep kept its results in memory.
  Choosing an output folder in the Sweep tab goes through a native folder dialog, which the egui MCP server cannot drive.
  Both paths opened the sheet.

### Review

A reviewer with a fresh context listed one should-fix and six nits.
Five held against the code and were fixed in `fix: s18 review findings [draft]`, and the merge nit is left for the merge point.
The should-fix was the resume paragraph of `docs/guide/sweeps.md`, which listed the edits that resume but left out a new order of two lines that cannot change the result, and closed on a "never" parallel frame.
The paragraph now says only the lowered graph's code decides a resume, links to "Code identity of a sheet" in `docs/reference/model-files.md` for the parts of a sheet outside the code, and states the manifest's latest text and the shared code in two plain sentences.
Two lines of `agents/henad-app-editor.md` and `agents/henad-graph-sheet.md` that S18 left past 100 columns are re-wrapped with the rest of their paragraphs.
This record's UI check no longer cites the plan's section or the screenshot folder, both of which sit outside the repository.
The comment of `model_source_equality_ignores_the_author_text` claimed a new field must join the `differing` list to compile.
Only the destructuring and the final literal force that, and the comment now says so and that a row is added by hand.
`docs/guide/app.md` now says Edit model opens the sheet, node graph or grid rule a model was built from, and lists Open in Editor among the ways the Editor tab opens.
The doc comment of `open_in_editor` holds its summary sentence alone, with the status-line note in a paragraph of its own.
No fix changed what the app shows, so the UI check was not repeated.
`cargo fmt`, clippy over henad-core and henad-app, their tests with `HENAD_REQUIRE_GPU=1` and a `zensical build` pass after the fixes.

The last nit concerns the merge point.
`git merge-tree` against `37-node-editor` at `f879ba2` reports conflicts in `CHANGELOG.md`, a list conflict kept on both sides, and in `docs/reference/model-files.md`, where S23's "A sheet converted to a graph model" and S18's "The sheet in a results folder" were both added after "Code identity of a sheet".
The rebase keeps both sections, S23's first, and then checks the `#opening-results` link and the `#code-identity-of-a-sheet` anchor `sweeps.md` now links to.

## State after

- A sheet's entry, the manifest of a sheet's sweep and the app's Results tab all hold the sheet's canonical text, outside every code comparison.
- `cargo fmt`, clippy with `-D warnings -W clippy::all` over henad-core, henad-graph, henad-explore and henad-app, their tests with `HENAD_REQUIRE_GPU=1` and `--all-features`, rustdoc with `-D warnings` over the three library crates and the wasm32 typecheck of the four crates pass.
  The full check runs at the merge point.

### Merge check

- The merge rebase onto `37-node-editor` at `d7b04f2` brought in S23 (conversion) and S13b (sheet and card panes), and conflicted in the first two commits.
  - `CHANGELOG.md` keeps both Added lines, S23's conversion and S18's sheet text in results.
  - `docs/reference/model-files.md` keeps both sections after "Code identity of a sheet", S23's "A sheet converted to a graph model" first and S18's "The sheet in a results folder" after it.
    The `#opening-results` link of the new section and the `#code-identity-of-a-sheet` anchor that `sweeps.md` links to both resolve.
  - `agents/henad-app-sheet.md` keeps S13b's wording of the list under the panes (the diagnostics the card does not draw, and a problem shown under its line on the card) and S18's Apply sentences (the author text in the entry's source, Edit model opening the sheet, and the note-only Apply).
  - `crates/henad-app/src/tests/sheet_editor.rs` keeps both milestones' imports, S13b's `check` beside S18's sweep and results imports, and both sets of tests the two milestones appended at the end of the file.
- The full check then ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
  No fix was needed.
- No baseline test passes now.
- The rebase brought in henad-app changes (S13b's sheet and card panes), so the egui MCP checks were repeated under the UI lock on G15F1, over a folder that henad-cli wrote from the Game of Life sheet.
  Open in Editor in the Results tab opened the recorded sheet in the new panes, with the card's chips, selects and the paused preview at tick 0.
  After Apply and New grid rule, Edit model on the Model tab replaced the Life rule with the applied sheet.
  The screenshots stay out of the repository.

## Issues found & future directions

- **A folder's runs replay through the app's entry of the same id.**
  A folder of a sheet whose model this app has not applied finds the compiled `game_of_life` example under that id, whose schema differs from the sheet's, and the Series legend of screenshot 1 names the raw parameter id `initial_density`.
  The app could build an entry from `model.code` for such a folder, which the recorded IR allows.
- **Open in Editor replaces the Editor's model without asking**, as Open model, New grid rule and Edit model do.
  No editor model tracks unsaved edits yet.
- **Opening the sheet starts its preview**, which takes the place of the applied model when nothing runs past tick 0.
  This is S13a's preview rule and unchanged here.
- **A lowered grid sheet shows Grid Width and Grid Height** in the Parameters and Sweep tabs, beside the sheet's own size, which the World section shows as fixed.
  That belongs to the lowering of the world (S7), and S24's preview parameters can settle it.
- **The manifest holds the latest session's sheet.**
  A resume after a note edit keeps no record of the earlier text.
  The code is equal in every case, and `sweeps.md` says so.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D10 | The sheet in a results folder | author-file, section 7, question 4 | (a) Recorded in the manifest and in `ModelSource`, outside the hash | S18 |
| D51 | `ModelSource` equality | recon of explore, the CLI and the docs | Leaves the author text out, so a note edit never reinstalls a model | S18 |

The words S18 shows, "Open in Editor", its tooltip and "Recorded model does not read", are in the Results tab, outside the wording table of the gate, and no researcher met them.

## Benchmark commands left for the maintainer

None: S18 changes no engine, no shader and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
