---
date: 2026-11-07
title: "Editor model enum"
description: "The Editor tab holds its model as one enum, EditorModel with Empty, Rule and Graph, where it held a grid rule's text and an optional graph side by side. Every site that differs by kind calls a method of the enum, and the preview reads the model through preview_source and preview_program. No behaviour changes, and the app's tests change only in field access."
icon: material/code-braces-box
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s6-editor-model-enum, two unsigned [draft] commits and this record after f4088d7; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: f4088d7
delta_state: "committed on `37-node-editor` by fast-forward: `chore: one editor model enum [draft]`, `fix: s6 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Editor model enum

> S6 of the #37 redesign plan: groundwork for AF8 in henad-app, with no change in behaviour.
> `EditorPanel` held a grid rule's text in `text` and the graph under edit in `graph`, and each caller decided the kind by whether `graph` was `Some`.
> It now holds one `model: EditorModel`, an enum of `Empty`, `Rule(String)` and `Graph(Box<GraphEditor>)`, so a text and a graph are never held at once.
> Every caller that differs by kind calls a method of the enum: `is_empty`, `file_text`, `holds_preview_model` and `preview_source`, with `preview_program` beside the last.
> The preview, the fault routing, the playback hover, the menu bar and every Editor dispatch site go through it.
> The app's tests change only in how they reach the field, one new test holds each variant's answers, and another types into an empty Editor.
> The running app opens, edits, applies, saves and undoes a grid rule and a graph starter as before.

## State before

`37-node-editor` at `f4088d7` held rounds 1 to 3 of #37 and records #61 to #70.
`crates/henad-app/src/ui/editor/mod.rs` declared `EditorPanel` with two fields for the model:

- `text: String`, a grid rule's text as typed, empty while the Editor held a graph.
- `graph: Option<Box<GraphEditor>>`, the graph under edit.

`EditorPanel::is_empty` and `file_text` combined the two, and nine sites read `graph.is_some()` or `graph.as_deref()` to decide the kind: the bottom and central panels of `editor_ui`, `take_pick_import`, `graph_outcome_ui`, `receive_import`, `preview::poll_at` and `build_preview`, `AppState::show_fault` and `playback::preview_hover`.
`outcome_ui` and `move_cursor_to` read `text` instead, through `panel.text.is_empty()` and `char_index_at(&panel.text, ...)`.
The app's tests reached the fields directly: 24 uses in `src/tests/editor.rs`, 9 in `graph_editor.rs` and 1 in `graph_preview.rs`.

## What was done

### The enum

`EditorModel` sits in `ui/editor/mod.rs` beside `EditorPanel`, whose `text` and `graph` fields became one `model: EditorModel`.

- `Empty` is the default, the state of a fresh app.
- `Rule(String)` is a grid rule's text as typed. It may be empty or fail to read.
- `Graph(Box<GraphEditor>)` is the graph on the canvas.

Its methods:

| Method | Answer | Callers |
| --- | --- | --- |
| `is_empty` | `Empty`, or a `Rule` whose text is empty | header buttons, `save_model`, `outcome_ui`, the File menu |
| `file_text` | the graph's canonical text, the rule's text, or an empty text | `save_model`, `save_name`, `apply` |
| `holds_preview_model` | `Graph` alone | `AppState::show_fault`, `preview_hover` |
| `preview_source` | a `PreviewSource` of the preview key and the error flag, `None` with no preview | `preview::poll_at`, `build_preview` |
| `preview_program` | the compiled program the preview runs, `None` with no preview | `build_preview` |
| `rule_text` | the rule's text, `None` otherwise | `move_cursor_to` |
| `graph`, `graph_mut` | the graph, `None` otherwise | `editor_ui`, `graph_outcome_ui`, `receive_import` |

`EditorPanel::is_empty` and `file_text` moved to the enum, and their callers read `panel.model.is_empty()` and `panel.model.file_text()`.
`load` sets `Rule` and `load_graph` sets `Graph`, where each cleared the other field before.

The central panel matches on the model.
The `Empty` arm draws the code view over a fresh `String` each frame and replaces the model with a `Rule` once typing leaves the text non-empty, as typing into the empty field filled `text` before.

### Tests

- The app's existing tests changed only in how they reach the model: a write of `app.editor.text` became `app.editor.model = EditorModel::Rule(...)`, a read became `app.editor.model.file_text()`, and `app.editor.graph.as_deref()` and its kin became `app.editor.model.graph()` and `graph_mut()`.
  One test appended to the text with `push_str` and now sets a `Rule` of the text with the line appended.
  The canvas, function and region tests needed no change.
- `each_editor_model_answers_by_its_kind`, inline in `ui/editor/mod.rs`, holds each variant's answers: an empty text counts as no model, a rule's text reads back as written, and a grid starter's graph previews, compiles and round-trips its code through `file_text`.

### Live checks

On the secondary display under the UI lock, from the worktree, with `EGUI_INSPECTION=1 cargo run -p henad-app --features inspection`:

1. The Editor tab opened from View on a fresh app showed the empty code view with Save model and Apply disabled and "Editor is empty. Press New grid rule, New graph model or Open model to start."
2. Typing `# typed` into the empty field enabled Save model and Apply, and the list read "Press Apply to check and install the model.", so the `Empty` arm became a `Rule`.
3. New grid rule loaded the Life starter's 43 lines.
4. Apply built and ran the rule ("Model applied.").
5. An edit typed into line 1 made Apply refuse it ("Model not applied. Fix the problems below." and 1:8 key with no value, expected `=`).
6. Undo in the code field (Cmd+Z) restored the text, and Apply then read "Model code unchanged."
7. Save model suggested `life_rule.toml` and wrote the starter's text to a scratch folder.
8. New graph model, SIR Epidemic, opened the graph on the canvas and its preview ran at 256 × 256 cells.
9. Edges set to Walls and Apply installed `node_sir` at 1024 × 1024 ("Model applied.").
10. Save model suggested `node_sir.toml` and wrote the graph with `boundary = "bounded"`.
11. The graph toolbar's Undo returned Edges to Wrap around, the preview rebuilt at 256 × 256 cells, and the outcome read "Model applied. Press Build to run it." behind the preview.

One screenshot was taken per step, and the images stay outside the repository.
The native save dialog lies outside the egui accessibility tree, and the bare binary is not an application that computer use can be granted, so the two saves ran through System Events (Go to Folder and Return) into a scratch folder, deleted afterwards.
The egui MCP server's Cmd+Z took effect only with both `command` and `mac_cmd` set.

### Edited tree

```
agents/
└── henad-app-editor.md                      EditorModel, its methods, the preview through preview_source
crates/henad-app/src/
├── state.rs                                 show_fault calls holds_preview_model
├── tests/
│   ├── editor.rs                            field access through EditorModel, typing into an empty Editor
│   ├── graph_editor.rs                      field access through EditorModel
│   └── graph_preview.rs                     field access through graph_mut
└── ui/
    ├── editor/
    │   ├── mod.rs                           EditorModel, PreviewSource, every dispatch site, the enum's test
    │   └── preview.rs                       poll_at and build_preview over preview_source and preview_program
    ├── menu_bar.rs                          Save model calls model.is_empty
    └── playback.rs                          preview_hover calls holds_preview_model
```

### Deviations from the plan

- **`preview_source` returns the key and the error flag, and the program comes from a separate `preview_program`.**
  The recon proposed `preview_source()` returning a key, errors and a program.
  `GraphEditor::preview_program` compiles on each call and can stop viewing an output that no longer reads, so it takes `&mut self` and runs only when the preview builds, while `poll_at` reads the key every frame.
  A single method would compile every frame or hold a borrow of the editor.
- **Three accessors beyond the plan's four questions:** `rule_text`, `graph` and `graph_mut`, for the sites that belong to one kind (the code view's cursor, the graph's outcome list, the function import).
- **No CHANGELOG line.** Plan section 2.5 asks for one per user-visible milestone, and S6 changes nothing a user sees.
- **One test edit beyond plain field access.** `edit_model_shows_the_code_the_rule_runs` appended a line with `push_str` on the field, and now sets a `Rule` of the applied text with the line appended.

## State after

- `redesign/s6-editor-model-enum` holds two unsigned commits after `f4088d7`, `chore: one editor model enum [draft]` and `fix: s6 review findings [draft]`.
- In the worktree: `cargo fmt --all -- --check` passes, `cargo clippy -p henad-app --all-targets --all-features -- -D warnings -W clippy::all` passes, `HENAD_REQUIRE_GPU=1 cargo test -p henad-app --all-targets --all-features` runs 349 tests that pass, `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-app --no-deps --all-features` passes, and the wasm32 typechecks of henad-app and the facade with every feature pass.
- The merge check ran the full check of plan section 2.3 on the branch at `845f566`, which needed no rebase since `37-node-editor` was still at `f4088d7`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models, and its log shows no other test binary that failed or aborted.
  No baseline test now passes.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.
- `agents/henad-app-editor.md` describes the enum, at 25,487 characters.

## Issues found & future directions

- **S13a adds `Sheet(Box<SheetEditor>)`.** Each method then gains one arm, and `preview_source` and `preview_program` return the sheet's key and program, so `poll_at` and `build_preview` need no change.
  `PreviewSource::key` is the graph's `[u64; 3]`, and a sheet key either fits that shape or the field becomes a hash of its own.
- **`receive_open` still decides the kind with `GraphFile::parse`.** S13a's dispatch on the header kind replaces it.
- **`Empty` and an empty `Rule` read alike.** Both count as no model. A rule's text cleared by hand stays a `Rule` and is never set back to `Empty`, which no caller distinguishes.
- **`cargo doc --document-private-items` fails on henad-app at `f4088d7` already**, on an unresolved `STOP_WAIT` link in `state.rs`. CI's rustdoc runs without private items and passes.

## Review

The review returned one finding to fix and two nits, and all three held.
The `Empty` arm of `editor_ui` was the one new behaviour of S6 and no test drew it.
`typing_into_an_empty_editor_starts_a_grid_rule` in `src/tests/editor.rs` now runs `editor_ui` on a fresh app: a frame with no input leaves the model `Empty`, text typed into the focused code field turns it into a `Rule` of that text, and a second edit extends that `Rule`.
With the arm changed to drop the text, the test fails at the typed text.
The `EditorModel` doc lost its second paragraph, which narrated the design for callers, and "asks" became "calls" in the topic file, the record's description and summary, and the edited tree.
"State before" now counts nine sites that read `graph` and names `outcome_ui` and `move_cursor_to` apart, since both read `text`.
Live check 5 quotes the diagnostic as the app shows it, with the expected literal in backticks.
None of the fixes changes what the app shows, so the live checks were not repeated.
After the fixes, `cargo fmt --all -- --check` and clippy over henad-app pass, and `HENAD_REQUIRE_GPU=1 cargo test -p henad-app --all-targets --all-features` runs 349 tests that pass.

## Decisions adopted for review

None of the rows of plan section 6 names S6, and the gate added none for it.
The deviations above are the choices made in the maintainer's absence.

## Benchmark commands left for the maintainer

None. The change touches no hot path.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
