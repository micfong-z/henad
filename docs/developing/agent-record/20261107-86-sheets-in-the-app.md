---
date: 2026-11-07
title: "Sheets in the app"
description: "S13a of the #37 redesign. The Editor tab opens a sheet by the kind in its header, checker errors included, and shows it as a read-only Sheet pane and a Card pane of numbered sentences. Save model writes the canonical text and Apply installs the lowered graph. The preview waits paused at tick 0 until Play, Build replays its start at the same seed, and the tick counter says what a tick is. A New model dialog starts a sheet from Game of Life, Majority vote or Forest fire, and New graph model is now New node graph."
icon: material/file-document-plus-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s13a-sheets-in-app, rebased onto S14's record dc79b1b with one CHANGELOG list conflict kept on both sides: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: dc79b1b
delta_state: "committed on `37-node-editor` by fast-forward: `feat: open, save and apply sheets in the editor [draft]`, `feat: new model dialog and paused preview [draft]`, `fix: s13a review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheets in the app

> S13a brings a grid sheet into the app.
> The Editor tab opens a model file by the `kind` in its header, so a sheet with checker errors opens as a sheet, and a sheet that does not read opens in the code view with the reader's problem listed under it (D55).
> A sheet shows as two read-only panes, Sheet and Card, and the world stays in the dock's tabs (D56).
> Save model writes the sheet's canonical text, and Apply installs the graph it lowers to, unchanged.
> The preview of a sheet waits paused at tick 0 with its start drawn and runs on Play (D52), Playback's Build starts it again as it started before (D73), and the tick counter reads "Tick 0 (a tick is one step of time)".
> A New model dialog in the Editor header, the File menu and the Model tab starts a sheet from a starter under a name, and New graph model is renamed New node graph (D71).

## State before

- The branch was cut from `37-node-editor` at `dc79b1b`, with S6 (one editor model enum), S10 (every grid line kind) and S11 (sheets in model files) merged.
- `EditorModel` held `Empty`, `Rule` and `Graph`. `receive_open` opened a file that `GraphFile::parse` read on the canvas, and any other text in the code view, so a sheet opened as text and Apply could already build it through `ModelFile`.
- `AppState::load_preview` played every preview at once.
- The Editor header held New grid rule and a New graph model menu, the File menu the same two, and the Model tab New grid rule. No New model dialog existed.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### The sheet editor

`crates/henad-app/src/ui/editor/sheet/` is new.

- `mod.rs` holds `SheetEditor`: the `SheetDocument`, its canonical text, the `SheetReport` of `ParsedSheet::check` on that text, the `SheetFile` read, checked and lowered from it or the problems that stop it lowering, a load number and the compiled program.
  `SheetEditor::new(document)` writes the canonical text and checks and lowers that text, so every problem is placed in what Save model writes.
  `preview_key` is `[load, 0, 0]`, the load number taken from the graph editor's counter (`LOADS`, now `pub(crate)`), so a graph and a sheet never share a key.
  `program` compiles the lowered graph through `GraphProgram::compile` on first use and places each compiler problem through `SheetFile::place_graph_diagnostic`.
- `sheet_ui` lays the Sheet pane in a resizable left panel (40% of the tab's width by default, each pane at least 160 points) and the Card pane beside it, each under its heading in its own scroll area.
- `sheet_pane.rs` draws the seven sections of `SheetSection::ALL` as plain labels: the purpose, the world with its size, edges and edges sentence and "One tick is one step (no unit)", the Cells table (name, kind with the states, start, and `changed_by` over the Become and Set lines of the cards that run each tick), the parameters, the measures, "Colour cells by Status" and each button.
- `card_pane.rs` draws every card under `Sentences::card_heading`, and `card_lines` numbers each line in display order with its depth and its sentence, "otherwise" words included, through `Sentences::line` and `CardRelations::otherwise_numbers`.

### Open, save, Apply and faults

`crates/henad-app/src/ui/editor/mod.rs`:

- `EditorModel::Sheet(Box<SheetEditor>)`, with `sheet()`, `holds_preview_model` true for a sheet, `preview_source` and `preview_program` of the sheet, `preview_plays` (true for a graph only) and `place`.
- `open_text` dispatches on `model_text::read_header` (D55): `sheet` through `ParsedSheet::parse` alone into `EditorPanel::load_sheet`, `graph` through `GraphFile::parse` onto the canvas, anything else in the code view. A sheet whose text does not read opens in the code view with its problem as `ApplyOutcome::Unread` ("Model file has problems. Fix them, then press Apply."). `receive_open` calls it.
- `file_text` of a sheet is its canonical text, which Save model writes and Apply reads, and `save_name` suggests `<sheet id>.toml`, errors or not.
- The list under the panes (`sheet_outcome_ui`) shows the preview's state, the last Apply's outcome, every diagnostic of the sheet's check in the error or warn colour, and every lowering problem the check does not list.
- `EditorPanel::show_fault` and `show_preview_fault` pass a fault's problem through `EditorModel::place`, so a sheet places it through S11's mapping. `Preview::show_fault` became `show_problem`, which takes the placed problem.

### The paused preview and Build

- `AppState::load_preview` takes a `plays` flag, and `build_preview` passes `EditorModel::preview_plays`: a graph's preview still plays, and a sheet's waits at tick 0 (D52).
- `AppState::previews_editor_sheet` holds while the preview runs and the loaded id is the sheet's. `load_preview` records the selection, and `AppState::build_replays_preview` adds that the selection is still that one. Playback's Build then calls `press_build`, which runs `preview::rebuild_preview`: the same setup at the same seed, paused again at tick 0 (D73). Build is enabled then whatever the selected model's refusal, with the tooltip "Start world again from tick 0, as it started before". Once the Model tab selects another model, Build builds that model as it does behind a graph's preview.
- `AppState::runs_editor_sheet` holds for the sheet's preview or the source the Editor applied, and Playback's counter then reads `sheet::sentence::tick_counter` ("Tick 4 (a tick is one step of time)", or "Tick 4 (4 days)" through the new `time_axis::elapsed_text`).
- Playback's preview line adds "Press Play to run the preview." while a sheet's preview waits at tick 0.

### The New model dialog and the entry points

- `new_model.rs`: `NewModelDialog` in `EditorPanel::new_model`, drawn by `new_model_modal` from `lib.rs` beside the other modals. An `egui::Modal` whose content takes `Role::Dialog` and the label "New model" through `accesskit_node_builder`, the heading "New model", "Start from" with one radio per sheet starter and its description from the wording table, a Name field, Cancel and a blue 700 Create (D57). Name follows the chosen starter's name until the author types in it, and Enter in Name creates. `create_model` runs `SheetEdit::SetSheetName` when the name differs and opens the sheet.
- The Editor header and the File menu gain New model ahead of their other entries, and the Model tab gains it beside New grid rule (D71).
- New graph model is New node graph in the header (its widget name too) and the File menu, and still lists the six graph starters (D63). New grid rule stays in all three places.
- `EMPTY_EDITOR` reads "Editor is empty. Press New model, New node graph, New grid rule or Open model to start."

### Tests

`crates/henad-app/src/tests/sheet_editor.rs`, new:

- `every_sheet_starter_opens_as_a_sheet_previews_at_tick_zero_plays_and_applies`: each starter opens as `EditorModel::Sheet`, lowers, previews at its size capped at 256 under its own id, waits at tick 0, plays past it, and applies and builds.
- `the_card_pane_reads_the_life_lines_as_their_sentences`: Game of Life's three lines and heading, and the vote's line 3 "otherwise (when line 2 does not apply) become A from any state, always".
- `apply_installs_and_builds_the_sheet`, `save_writes_the_canonical_text_and_the_saved_file_reopens_to_the_same_document` (each starter byte for byte, a hand-spaced copy saved canonical, `save_name` without a file).
- `a_sheet_with_a_skipped_line_opens_and_previews` (line 2's old state blank), `a_sheet_with_a_checker_error_opens_in_the_sheet_editor` (a parameter default outside its range: no preview, Apply refused), `a_sheet_whose_text_does_not_read_opens_in_the_code_view_with_its_problem`.
- `a_sheet_preview_waits_at_tick_zero_until_play`: paused after the build, the start drawn, the counter's words, no step while paused, Play advances it, and a graph's preview still plays.
- `a_rebuild_replays_the_same_start`: Build on the preview gives the same Alive count at tick 0, and after Apply Build at seed 7 twice gives one count and seed 8 another.
- Over a `Harness` that draws the File menu, the Editor tab and the dialog with AccessKit on: `new_model_opens_a_dialog_and_create_opens_the_chosen_sheet`, `enter_in_the_name_field_creates_the_model`, `new_node_graph_lists_the_six_graph_starters_from_the_header_and_the_file_menu`. `the_empty_editor_names_every_way_in` holds `EMPTY_EDITOR`.
- `each_editor_model_answers_by_its_kind` in `ui/editor/mod.rs` covers the sheet variant, and `graph_preview.rs` passes `true` to `load_preview`.

### Checks in the running app

On G15F1 under the UI lock, the app run from the worktree with inspection, through the egui MCP server.
The screenshots, named below, stay out of the repository.

1. `shot0-start.png`: the Model tab shows New model beside New grid rule.
2. `shot1-dialog.png`: New model in the Model tab opened the dialog. The tree held a node of role Dialog named "New model". Game of Life was chosen, with its description under it, the other two starters below, Name "Game of Life", Cancel and a blue Create.
3. The first Create showed the Card pane squeezed to a column a word wide, since the Sheet pane took a fixed 340 points of a narrow Editor tab. The Sheet pane now takes 40% of the tab's width, and the app was restarted.
4. `shot2-sheet-tick0.png`: File, New model, Create. The Card pane held "Each tick, every cell" and lines 1 to 3 as the wording table gives them, then "Lines of Clear: run once, every cell, when pressed" and its line 1. The Viewport showed the random start, Playback read "Tick 0 (a tick is one step of time)" beside Play, Statistics read Alive 19820, and the Charts tab drew the tick-0 point.
5. `shot3-playing.png`: Play ran the preview to tick 148596. The Alive measure on the Charts tab fell from 19820 to 1874 and the Viewport showed still lifes and oscillators.
6. `shot4-build-replays.png`: Pause, then Build. The counter returned to "Tick 0 (a tick is one step of time)", Alive read 19820 again, the run waited paused, and Playback showed "Press Play to run the preview."
7. `shot5-file-new-node-graph.png`: File held New model, New grid rule, New node graph, Open model and Save model, and New node graph opened Game of Life, Vote, SIR Epidemic, Boids Flocking, Ant Foraging and Random Walk.
8. `shot6-applied.png`: Apply installed the sheet at 1024 by 1024 ("Model applied."), the Model tab selected Game of Life with Edit model, the Parameters tab showed Randomise and Clear, and the run waited at tick 0 with the counter in the sheet's words and Alive 314544.

The app was quit, `app.ron` restored from this milestone's backup and the lock released.

### Docs and topic files

- `docs/guide/app.md`: the File menu, the Model tab and the tab table name New model, and the Editor tab section gains a Sheets subsection (the dialog, the two panes, the paused preview, Build, the counter, Apply, Save model) above a Grid rules heading.
- `docs/guide/editor.md`: New model and New node graph in the list of ways in. The page is rewritten in S22.
- `agents/henad-app-sheet.md`, new, with its row in `AGENTS.md` (8,600 characters).
- `agents/henad-app-editor.md`: the sheet variant, `open_text`, the header, File menu and Model tab entries, `EMPTY_EDITOR`, the preview's `plays` flag and `show_problem` (26,370 characters).
- `agents/henad-app.md`: the `plays` flag, `runs_editor_sheet`, `previews_editor_sheet`, the File menu and the dialog in `lib.rs` (21,411 characters).
- Two CHANGELOG lines: the sheet editor under Added, the rename under Changed.

### Edited tree

```
AGENTS.md                                        the henad-app-sheet.md row
CHANGELOG.md                                     sheets in the app, New node graph
agents/
├── henad-app-editor.md                          sheet variant, open_text, entry points, plays flag
├── henad-app-sheet.md                           new
└── henad-app.md                                 plays flag, runs_editor_sheet, File menu
crates/henad-app/src/
├── lib.rs                                       draws the New model dialog
├── state.rs                                     load_preview's plays, runs_editor_sheet, previews_editor_sheet
├── tests/
│   ├── mod.rs                                   sheet_editor
│   ├── graph_preview.rs                         load_preview(.., true)
│   └── sheet_editor.rs                          new
└── ui/
    ├── editor/
    │   ├── mod.rs                               EditorModel::Sheet, open_text, header, EMPTY_EDITOR
    │   ├── preview.rs                           show_problem, plays, rebuild_preview
    │   ├── graph/mod.rs                         LOADS shared with sheets
    │   └── sheet/
    │       ├── mod.rs                           new: SheetEditor, sheet_ui
    │       ├── sheet_pane.rs                    new
    │       ├── card_pane.rs                     new
    │       └── new_model.rs                     new: the New model dialog
    ├── menu_bar.rs                              New model, New node graph
    ├── model.rs                                 New model
    ├── playback.rs                              press_build, sheet counter, preview line
    └── time_axis.rs                             elapsed_text
docs/guide/
├── app.md                                       New model, the Sheets subsection
└── editor.md                                    New model, New node graph
```

### Deviations from the plan

- **The dialog follows the gate, not the plan's list.** Gate section 11 replaces the plan's world question with "Agents in open space" disabled: the world question shows only under Blank, which arrives in S16a, so S13a's dialog has none. The Name field defaults to the chosen starter's name until the author types one ("Untitled model" only for Blank), so the plan's acceptance "the Name field defaults to Untitled model" is tested as "Name follows the starter, and a typed name stays". An empty name on Create takes "Untitled model". Cancel is always offered and Enter in Name creates, as the gate asks.
- **Build replays the preview while it is the Editor's sheet.** The plan says Playback's Build replays the start at the same seed. Build builds the selected model, and a preview is never the selection, so pressing Build behind a sheet's preview would have loaded some other model. While the preview of the Editor's sheet runs, Build now builds that preview again at the same seed and leaves it paused. After Apply, Build builds the applied sheet as for any model. `a_rebuild_replays_the_same_start` covers both.
- **Tick-0 cells are compared through the Alive measure.** A GPU run's cells never leave the device in the app, so `a_rebuild_replays_the_same_start` compares the Alive count at tick 0 and checks that another seed changes it. henad-graph's `a_rebuild_at_one_seed_replays_the_same_start` compares the cells themselves on the host evaluator.
- **A fault of the run lands at `[sheet]`.** The plan asks a fault to name the author line through S11's mapping. A `Fault` carries no node and no pass, so the mapping places it at `[sheet]`. A compiler problem of the preview does carry a node, and is placed at its line.
- **Save and reopen in the running app was not driven.** Save model opens the native save panel, which the egui MCP server cannot reach. `save_writes_the_canonical_text_and_the_saved_file_reopens_to_the_same_document` holds the bytes and the reopened document for every starter.
- **The skip notes are listed under the panes, not under their lines.** Notes under lines are S13b's, and S13a lists every diagnostic of the check in the list below the panes.
- **Commits.** The docs pages and the CHANGELOG lines went into the second commit, after the rename and the dialog they describe.

### Review

A reviewer with a fresh context listed one must-fix, three should-fix and five nits, and every one held against the code and was fixed in `fix: s13a review findings [draft]`.
The must-fix was a regression: behind a sheet's preview, Build replayed the preview even after the Model tab selected another model, so the selected model could never be built until Offload.
`load_preview` now records the selection in `AppState::preview_selection`, and `AppState::build_replays_preview` replays the preview only while that selection stands, with the usual Build tooltip and refusal otherwise.
`build_builds_another_selected_model_instead_of_the_sheet_preview` holds it.
The Cells table read a value start from `starts.state`, which only an on-ground start fills, so "All Tree" and "A random number" both read "A value".
`starts_text` now reads the `value` slot, and `the_cells_table_reads_each_start_in_the_wording_table_words` holds the four starts.
The deviation saying the start and button summaries lack words was wrong, since gate 10.12 gives them and `sheet::sentence` exports them.
The Sheet pane now takes `BY_CHANCE`, `starts_all`, `A_VALUE`, `A_RANDOM_NUMBER`, `COLOR_CELLS_BY`, `START_AGAIN` and `run_once`, and the deviation is struck.
A painted, anywhere or on-ground start, which no grid sheet holds, now reads as an empty cell instead of its raw rule id, and the `set_everywhere` step that no grid control writes (D98) keeps the pane's own "Set everywhere".
The per-starter test now checks the wait at tick 0, Play and Apply for all three starters.
Among the nits, a sheet that does not read opens with the new `ApplyOutcome::Unread` instead of a refused Apply, and a sheet's preview has its own line, which names Play.
The long doc line, the trailing "which" clauses and the topic file's citation of a gate section were fixed too.
The app was run again under the UI lock on G15F1: the Forest fire sheet showed the new preview line, "By chance", "Colour cells by Cover" and "Replant: Start again", and Build with Boids Flocking selected behind the preview loaded Boids at tick 0 with the plain counter (`S13a-shots/review-1-forest-fire-preview.png`, `review-2-build-builds-boids.png`).

## State after

- A sheet opens, previews paused at tick 0, plays, starts again as it started, applies and saves in the app, from Open model or from the New model dialog.
- The Editor header and the File menu read New model, New grid rule, New node graph, Open model and Save model, and the Model tab New model and New grid rule.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-app --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-app --all-targets --all-features` (374 passed); `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-app --no-deps --all-features`; `cargo check -p henad-app --all-features --lib --target wasm32-unknown-unknown`.
  The full check runs at the merge point.

### Merge check

- The branch was rebased from `dc79b1b` onto `dc79b1b`, the tip of `37-node-editor` after S14's merge.
  S14 touched no henad-app file, so the egui MCP checks were not repeated.
- One conflict came up, in `CHANGELOG.md` under Added, where S14's grid sheet oracles line and this milestone's sheets line meet.
  Both lines were kept, S14's first.
- The full check ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, with no new failing name and no aborted test binary in the log.
  henad-app's library tests pass, 377 of them.
- No baseline test passes now.

## Issues found & future directions

1. Run to tick behind a sheet's preview still reads "Selected model not loaded. Press Build first.", as it does behind a graph's preview, and Build there now replays the preview, so the advice goes round in a circle. A forward Run to tick on a preview needs no rebuild, and S24's preview mode of the Params tab is a natural place to let Run to tick drive the preview.
2. The Parameters tab behind a preview warns "Selected model not loaded" and shows the selected model's sliders, not the sheet's. S24 builds the preview mode (D54, D88).
3. The Cells table clips its last column in a narrow Sheet pane, and the Card pane's lines wrap in a narrow tab. S13b lays out the panes.
4. The preview line under the panes reads "Preview at 256 × 256 cells starts at tick 0. Press Play to run it, or Apply to run the model at its own size." since the review. S13b's forbidden-word and label tests should run over sheet mode too.
5. The sheet starters' ids match example models (`game_of_life`), so Apply of the Game of Life sheet replaces the example model of that id in the session, as `--model-file` does in henad-cli.
6. `SheetEditor::new` reads and checks the canonical text, and `SheetFile::parse` reads and checks it again. A sheet is small, and S16a's edits may want to keep one parse per edit.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D52 | Edits during a run (L2) | the engine recon, section 2.10, and the prototype's "World" | L2 is not built. A model waits paused at tick 0, edits redraw the start until the first tick, and after that a valid edit restarts the world from tick 0 with a notice | S13a, S16a |
| D55 | Open dispatch | the foundations draft | On the header kind through `read_header`, so a sheet with checker errors still opens in the sheet editor | S13a |
| D56 | Where the World pane lives | the app recon | In the dock's Viewport, Playback, Pacing, Params and Charts tabs, with the Sheet and Card panes in the Editor tab | S13a |
| D57 | The primary action's colour | the app recon, `agents/ui.md` | MCS blue 700 and the app's dark look, in place of the prototype's orange | S13a, S13b |
| D60 | The words rule's scope | the app recon | The Editor tab in sheet mode and the text a sheet produces in the World tabs, not the Model, Performance and System tabs | S13b |
| D61 | Autosave and the Facilitator panel | the app recon | Neither in the app | S13a |
| D63 | The graph starters | the author-file design, section 5 ("the maintainer's call"), the history recon | **Plan's choice**, no recommendation in the source. Kept as graph files, corpus and expert examples, listed under the renamed "New node graph" menu (D71) | S13a |
| D64 | Which round 1 models ship as starters | round 1 section 14, answer 13a | All eight, forest fire and bounded confidence included, and the ants keep their slower trail with a note | S1b, S10, S17, S20 |
| D71 | Entry points for New model and the graph starters | the app recon, section 3.3, which leaves it to the plan | **Plan's choice**, no recommendation in the source. "New model" opens the new dialog from the Editor header, the File menu and the Model tab. "New graph model" is renamed "New node graph" in the Editor header and the File menu and keeps the six graph starters. "New grid rule" stays in all three places. `EMPTY_EDITOR` names the three | S13a |
| D73 | The World pane's controls kept as the dock has them | the prototype's "World", round A's F15, F20 and F22, the prototype's notes | **Plan's choice** within D56. The plot's whole run (F15) is the Charts tab's Unlimited history. The sticky run bar (F20) is the Playback tab, which never scrolls away. The legend stays on the Charts tab and current values on the Stats tab. The world's Start again is Playback's Build, which replays at the same seed, and a sheet button's Start again draws a new layout each press, as the prototype's notes say. The 0 label on the left axis (F22) is built | S7, S9, S13a, S14 |

From the gate's section 7:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D96 | The words rule and the ways into the node graph | the final prototype spec, section 4 ("node" is forbidden), against D71 ("New node graph"), the expert design, section 5, and answer 8b | **Plan's choice.** The words rule exempts the controls that open or create a node graph: the "New node graph" menu in the Editor header and the File menu, "Convert to node graph", "Edit as node graph", the conversion modal, the converted copy's banner and the author model's line "Converted to node graph: X", as the final spec exempts the Facilitator's "Seed". Each forbidden-word test names these controls and checks every other text. Without it, S13b's test fails on the Editor header in sheet mode | S13a, S13b, S27 |

S13a builds D64's three grid starters into the dialog, D60 only as far as the panes draw the wording table's words, and D96 as the "New node graph" name that S13b's test exempts.

## Benchmark commands left for the maintainer

None. S13a changes no engine path, and the preview builds as the graph editor's does.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
