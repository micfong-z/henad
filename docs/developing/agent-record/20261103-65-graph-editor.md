---
date: 2026-11-03
title: "Graph editor and live preview"
description: "Milestone E1 of #37. The Editor tab edits graph models on an egui-snarl canvas, Apply installs a grid graph at its own size, a live preview rebuilds from tick 0 after each valid edit at most 256 cells a side, and the editor shows each pass's generated WGSL and colours the Viewport by any element-rate output. Every E1 gate was met through the egui MCP server natively, and through Chrome on the release web build, where the round 2 audit fixes observed the restart after the binding through Playwright. A review then found eight defects, fixed with a test each."
icon: material/graph-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor with E1 as unsigned [draft] commits ac263ad to ac263ad on top of the G2 review record 260e00d; every E1 gate met natively and in Chrome, the restart after the binding in Chrome observed by the round 2 audit fixes (record #67); eight review findings fixed; HENAD_REQUIRE_GPU=1 check.sh lines passing apart from cargo deny's three advisories that master fails too"
baseline_commit: 260e00d
delta_state: "committed on `37-node-editor`: unsigned commits `ac263ad` to `ac263ad`, for the maintainer to reword and sign"
---

# Graph editor and live preview

> The Editor tab now edits a graph model on a canvas built on egui-snarl 0.12, beside a panel of its passes and its world.
> Apply installs a grid graph through the install path and builds it at its own size, with the Rebuild model dialog past tick 0, and a model past the submission budget is refused in the Editor.
> A live preview runs the document at most 256 cells a side and rebuilds it from tick 0 250 ms after each valid edit.
> It takes the Viewport only from no run, a run at tick 0 or another preview, and otherwise reads "Preview waits for the running model."
> A WGSL switch shows the shaders the compiler wrote for the open pass, and View in viewport colours the preview by any element-rate output through a temporary view pass.
> Every gate was met through the egui MCP server on the native app, and through Chrome on the release web build.
> The restart after the binding was observed in Chrome later, through Playwright, by the round 2 audit fixes (record #67).
> A review of the milestone found eight defects in the preview, the canvas state, undo and the side panel, each fixed with a test that fails without the fix.

## State before

Round 1 ended with records #61 to #64, and G2 (record #66) made grid graph files compile to WGSL and run, with a submission budget in `Demand`.
`ModelFile::entry` returned a GPU grid entry for a grid graph.
The Editor tab edited grid rules as text, and egui-snarl had been tried behind the `snarl-spike` feature (record #62), whose recommendation the maintainer took: adopt it.

E1 ran in two steps in the worktree `37-e1`, whose commits now sit on `37-node-editor`.
The first step replaced the spike with the graph editor: the canvas, the passes and world panels, quick-add, node menus, diagnostics on nodes, snapshot undo, copy and paste and Arrange, with henad-graph's three edits in place (`SetPassEnabled`, `SetParam`, `SetWorld`) and `EditorRegion::frames`.
Apply of a graph still read `ModelFileError::Unsupported`, since G2 had not landed in its base.
This record covers both steps, and the second step's work in detail.

## What was done

### Step 1: the editor (summary)

`ui/editor/graph/` holds `GraphEditor`, which edits the document through `GraphBuilder` and `EditCommand`s alone.
Every gesture is a `GraphRequest`, held while the primary button is down and run after the canvas has drawn, which keeps egui-snarl from panicking on a removal under a wire in flight.
Each body has one persistent `Snarl` synced by difference, so the selection and egui's per-node state stay on their nodes.
Undo keeps the canonical text before each edit, with coalesced keys for drags and typing.
`agents/henad-app-editor.md` describes the modules, and the scratch canvas and its feature are gone.

### Step 2: Apply, the preview, the WGSL view and View in viewport

After the rebase onto G2, `apply` installs a grid graph unchanged: `ModelFile::parse(..).entry()`, a one-entry set through `install_models`, the Rebuild model dialog for a run past tick 0, and fault claiming.
`apply_reads_the_graph_and_installs_nothing_until_graphs_compile` became `apply_installs_the_graph_and_builds_it_at_its_own_size`, and `a_graph_edit_past_tick_zero_waits_on_the_rebuild_dialog` joined it.
A model whose submission passes the budget at its own size is refused before anything is built, as `ApplyOutcome::Faulted` with the line of `Demand::shortfalls`.

**The preview** (`ui/editor/preview.rs`).
`HenadApp::logic` calls `preview::poll` every frame.
The graph's `preview_key` (its load, its revision and the changes of the viewed output) tells a new edit.
A valid one sets the preview due 250 ms later, and an invalid one leaves the running preview alone and drops a waiting one.
Once due, the preview builds when `AppState::preview_can_load` holds: nothing loaded and no build, a preview loaded, or a built run at tick 0 without the Rebuild model dialog.
Otherwise the Editor reads **Preview waits for the running model.**
A preview compiles the document, takes every parameter's default with each side of the grid at most `PREVIEW_SIDE` (256), checks `shortfalls` and the setup, and builds through `AppState::load_preview`.
That goes through the same `load` as Build, on the build thread natively and inline in a browser, and plays the run.

The app marks the run with `AppState::preview` (a `PreviewRun` with its size).
A preview is never the selection, so the Parameters tab, the next build's values, live edits and `select_model` leave it alone.
Apply replaces a preview at any tick and builds the model at its own size, and `settle_loaded` never opens the Rebuild model dialog over a preview.
A code fault of the preview's run goes to the Editor as the preview's state instead of the fault dialog, and Playback names the preview and its size.

**Generated WGSL** (`graph/wgsl_view.rs`).
`GraphEditor::program` compiles the document once per revision.
The toolbar's WGSL switch opens a panel of the `text` of every shader whose `passes` hold the open pass, as selectable monospace text that does not wrap.
An init pass reads that it runs on the host, and a switched-off pass that no shader runs it.

**View in viewport** (`graph/view_pass.rs`).
The node menu lists the outputs a viewport can show: element rate, a number, a flag or a category, on a node outside every region.
`view_file` works on a copy of the builder, so the document never changes.
It switches the model's view passes off and adds a palette of nine viridis stops and a view pass.
It pastes the node and everything it reads into that pass, regions whole, and ends in `view.ramp` and `view.color`.
A category shows in its own colours, a `u32` indexes the ramp, an `i32` and an `f32` go through `view.ramp`, and a flag is the ramp's two ends.
The file is checked before the view is taken, so a value that reads a draw is refused with the checker's reason.
The preview then runs the view file, and a later edit that removes the node stops the view with a status line.

### Measurements

**Preview limit.**
`henad-cli` swept the SIR starter over a design table of square sides, five replicates of 10 steps each, release build, Apple M4 Pro:

```sh
printf "grid_width,grid_height\n64,64\n128,128\n256,256\n512,512\n1024,1024\n2048,2048\n" > design.csv
cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/sir.toml \
  --design design.csv --reps 5 --steps 10 --out out
```

| Side | Median `build_ms` |
| --- | --- |
| 64 | 1.13 |
| 128 | 1.21 |
| 256 | 1.75 |
| 512 | 4.39 |
| 1024 | 14.93 |
| 2048 | 56.66 |

The build time stays near its floor up to 256 and grows with the cell count past it, from the host evaluator's init.
256 is the largest side within 1.6 times the floor, and it holds 65,536 cells, enough to read a pattern.
In Chrome on the release web build, a `PerformanceObserver` on long tasks recorded none (no task over 50 ms) while the 256 by 256 preview built inline, nor while Build built the model at 1024 by 1024.

**Demand at the preview size**, in work units of one submission against the budget of 6e11:

| Model | Preview (256 by 256) | Own size (1024 by 1024) |
| --- | --- | --- |
| `game_of_life.toml` | 5.47e7 | 8.76e8 |
| `vote.toml` | 5.05e7 | 8.08e8 |
| `sir.toml` | 1.13e8 | 1.82e9 |
| a 4000-trial Repeat on 1024 by 1024 | 1.17e11 | 1.88e12, refused |

`every_starter_previews_within_the_device_limits_and_the_work_budget` holds every starter's preview within `Limits::default()` and the budget.
`the_submission_budget_refuses_apply_at_full_size_and_lets_the_preview_run` holds the last row: Apply reads **Model build failed. Fix the problem below.** over "one submission would run ... past the budget", and the preview of the same file runs.

### Gates

| Gate | Native, egui MCP | Chrome, release wasm |
| --- | --- | --- |
| New graph model from the SIR starter | preview at 256 by 256 plays | same |
| Bind the infection rate to a new parameter, and the preview restarts | `spread_rate` added in the world panel, bound on Subtract n12 through Inputs, tick count back near 0 | first run: adding `spread_rate` restarted the preview, and the binding's own restart was not read. Record #67, through Playwright: the binding of n12's `b` to `spread_rate` restarted the preview, from tick 1,015,869 to 1,020 within 0.35 s |
| A full-size Build | Apply builds 1,048,576 cells at tick 0, "Model applied." | same |
| A type error shows a badge, and pressing it frames the node | count wired into a float input: red stroke, badge, three problems with pass and node; the badge press centres n12 | a float parameter bound to Greater n16's `u32` input: badge, problem, the press centres n16 |
| An edit during a full-size run past tick 0 leaves that run going | "Preview waits for the running model.", the full run kept stepping, Offload then let the preview in | same, with an edit of the grid's edges |
| The grid-rule tests in `src/tests/editor.rs` | pass unchanged | |

In the first Chrome run the binding's own restart was not observed: the restart read there came from adding `spread_rate`.
The round 2 audit fixes ran the gate again in Chrome through Playwright and observed it (record #67).
The WGSL panel and View in viewport were checked natively, and the WGSL panel in Chrome.
The Chrome extension's drags never reached egui-snarl's pins, so the Chrome type error came from a binding in the node menu.
The Chrome canvas also needed the other dock leaves closed before its nodes were large enough to drive, and View then Reset layout restored them.

### Tests added

`src/tests/graph_preview.rs` holds twelve tests, listed in `agents/henad-app-editor.md`: the preview size and its limit, the restart after a bound parameter, an invalid edit, a run past tick 0, a Build after the edit, a run at tick 0, Apply over a preview, the WGSL view, View in viewport, the submission budget and every starter's Demand.
The review fixes below added three more there, four in `graph_canvas.rs` and one in `graph_editor.rs`.

### Review fixes

A review of the milestone confirmed eight findings.
Each fix has a test, and each test was run with its fix reverted and failed at its own assertion.

| Finding | Fix | Test |
| --- | --- | --- |
| `load_preview` set `sim_running` and sent Play after `load_setup`, even when the build had already failed inside it (inline in a browser, or a build thread that did not start). The fault had offloaded the preview, so Playback showed Pause on a disabled button, Apply skipped the build, and the app repainted every frame. | Plays only while the preview is still loaded and a run or build exists. `AppState::inline_builds` (test builds only) runs a build inline natively, as a browser does. | `a_preview_whose_build_fails_at_once_stays_paused_with_nothing_loaded` |
| A canvas's egui id came from the load and the scope's name. A pass removed and brought back by Undo reopened under the old id, and egui-snarl's stale selection by pin index marked another node, which Delete, Copy and Cut then acted on (the record #62 trap). A new pass named after a renamed one shared its state. | Each canvas takes a new id from a per-editor count of canvases made. | `a_body_opened_again_after_its_canvas_went_shows_no_stale_selection`, `a_pass_named_after_a_renamed_pass_gets_a_canvas_of_its_own` |
| Two separate drags of one field, with no other edit between them, coalesced into one undo step, since nothing ended a gesture on release. | `settle` ends the gesture when the primary button goes up, before and after the batch it releases, and when keyboard focus moves. | `two_drags_of_one_field_are_two_undo_steps` |
| The world table and a parameter's range and default showed the document's value during a drag, which changes only on release, and jumped at the end. | The fields show the last waiting `SetWorld` or `SetParam` (`shown_world`, `shown_param`). | `a_dragged_world_field_shows_its_value_before_the_release` |
| The record claimed every gate in Chrome, but Chrome did not show the restart after the binding. | Reworded here, in the summary and in the frontmatter. | |
| "Copied 1 node." showed in the warning orange, which MCS keeps for warnings. | `status` is a `GraphStatus` with a `warning` flag, and a copy is a note in the text colour. | `a_copy_confirmation_shows_in_the_text_colour` |
| A Refused or Faulted preview state of one document stayed over the next document until that one previewed, which a document with a checker error never does. | `EditorPanel::load` and `load_graph` reset the preview. | `another_document_clears_the_state_of_the_preview` |
| A preview that outlived its graph (New grid rule, Open model or Edit model on a rule) kept the hover "Press Apply in the Editor to run the model at its own size.", and Apply would build the rule. Its code fault went to the Editor's preview state, which nothing drew, and reached only the log. | The hover reads "Editor holds another model. Press Offload to stop preview." without a graph, and the fault goes to the fault dialog. | `a_preview_that_outlived_its_graph_reports_to_the_fault_dialog` |

### Codebase structure

```text
crates/henad-app/src/
├── lib.rs                      # logic polls the preview each frame
├── state.rs                    # PreviewRun, preview, builds, load_preview, preview_can_load; review: inline_builds
├── state/build.rs              # review: an inline build in a test with inline_builds
├── state/install.rs            # settle_loaded leaves a preview alone
├── tests/
│   ├── graph_canvas.rs         # step 1
│   ├── graph_editor.rs         # Apply installs, Rebuild model past tick 0
│   ├── graph_preview.rs        # new: preview, WGSL view, View in viewport, budget
│   └── scratch_canvas.rs       # removed with the spike
└── ui/
    ├── editor/
    │   ├── mod.rs              # Apply over a preview, the preview's lines, show_preview_fault; review: load resets it
    │   ├── preview.rs          # new: Preview, PreviewState, poll, preview_values, PREVIEW_SIDE
    │   ├── scratch.rs          # removed with the spike
    │   └── graph/
    │       ├── mod.rs          # GraphEditor: program, preview_program, viewing, WGSL switch; review: GraphStatus,
    │       │                   #   a fresh egui id per canvas, gestures end on release, shown_world, shown_param
    │       ├── world.rs        # review: fields show the waiting edits
    │       ├── canvas.rs       # View in viewport in the node menu
    │       ├── view_pass.rs    # new: ViewTarget, can_view, view_file, RAMP
    │       ├── wgsl_view.rs    # new: the generated WGSL panel
    │       └── arrange.rs, history.rs, passes.rs, quick_add.rs, sockets.rs   # step 1
    ├── menu_bar.rs             # New graph model
    └── playback.rs             # preview_line; review: preview_hover
crates/henad-graph/src/edit/    # step 1: SetPassEnabled, SetParam, SetWorld
crates/henad-graph/src/editor.rs  # step 1: EditorRegion::frames
docs/guide/editor.md            # graph models, the preview, generated WGSL, View in viewport
agents/henad-app-editor.md      # graph editor, View in viewport, the live preview
agents/henad-app.md, agents/henad-graph.md, CHANGELOG.md, zensical.toml
```

### Decisions taken

- A preview is a loaded run that is not in the set, marked by `AppState::preview`, rather than an install. An install on every edit would open the Rebuild model dialog, reset the Sweep tab's columns and recompute the Results store each time.
- A preview counts as replaceable whatever document started it. The Editor holds one document, and a preview of an earlier one is an orphan.
- A Build started after the edit drops the waiting preview, so the user's own build keeps the Viewport at tick 0. Without it, a Build of the same model would be replaced by the preview at once.
- A build in progress that is no preview makes the preview wait, although its run is at tick 0. Otherwise an edit right after Apply would abandon the full-size build.
- The preview uses each parameter's default. The Parameters tab describes the selected model, which a preview never is.
- The preview's own problems show as its state under the canvas, in the Faulted form "Preview not built. Fix the problems below.". `ApplyOutcome` stays the outcome of the last Apply, and a preview that later builds would otherwise clear a real refusal.
- A preview plays once built.
- View in viewport computes the value in a view pass, from the lanes after the tick, and colours a float across 0 to 1.
- Review: an orphan preview keeps running, as decided above, and its hover names Offload. Offloading it when the Editor loads a grid rule would have changed that decision, which is the maintainer's to change.
- Review: the test of a build that fails inside `load_preview` needs a build that fails before the call returns. A test-only `AppState::inline_builds` runs the build inline natively, the path a browser takes. Making `std::thread::Builder::spawn` fail had no clean hook.
- Review: a change of keyboard focus also ends an undo gesture, so arrow-key edits of a focused field and a later drag of the same field stay apart.

## State after

`37-node-editor` holds E1 as unsigned `[draft]` commits `ac263ad` to `ac263ad`, on top of the G2 review record `260e00d`, nothing pushed.
The worktree branch `37-e1` that E1 started on no longer exists.
`HENAD_REQUIRE_GPU=1` runs of every `check.sh` line pass: the cargo checks and the three wasm32 checks, `cargo fmt`, `check_packaging.sh`, clippy with every feature, the workspace tests, the doc tests, the facade's README test, `cargo doc` with `-D warnings`, the scripts' tests and the default web build.
`cargo deny` stops at RUSTSEC-2026-0370, 0371 and 0372, unmaintained `zbus_names`, `zvariant` and `zvariant_derive`, which master fails too.
The release web build was built and served with trunk for the Chrome gates.
Neither `henad-app`'s public API nor any hand-written model changed.

The review fixes are two further unsigned `[draft]` commits: the fixes with their tests and topic file, then this record's update.
`HENAD_REQUIRE_GPU=1 cargo test -p henad-app --all-targets --all-features` passes 317 tests, and `cargo fmt`, clippy on henad-app with every target and feature and `-D warnings`, the wasm32 check of henad-app, its doc tests and its `cargo doc` with `-D warnings` pass.
The app was driven through the egui MCP server natively after the fixes.
Two drags of the grid width in the world panel, from 1024 to 1028 and then to 1034, were undone one at a time: one Undo returned 1028.
After New grid rule, the preview kept running, and the hover of Playback's preview line read **Editor holds another model. Press Offload to stop preview.**
The display of a world field during a drag and the colour of a copy's status were checked by their tests alone, since the MCP server's drag releases the button before it reads the tree.

## Issues found & future directions

- **A GPU preview runs out in a second.** The GPU runners take no target ticks per second, and the adaptive batching runs SIR at 256 by 256 at 20,000 to 70,000 ticks a second, so the preview shows the end of the epidemic almost at once. The stock GPU models behave the same at full size. A paced preview, or one that starts paused, is the maintainer's call.
- The Model and Parameters tabs describe the selected model while a preview runs, and the Parameters tab reads "Selected model not loaded" over the stock SIR's values. Playback's line names the preview, but the panels say nothing more.
- View in viewport has no control of the float range, and a value inside a region is not offered.
- "Model applied." stays in the list after later edits, as it did for grid rules.
- The Chrome extension's drag reaches no egui-snarl pin, so a wire cannot be drawn through it. Record #62 drew wires through Playwright.
- The float Repeat test against hand-chained copies stays ignored on Metal (record #66).
- A world field's value during a drag and the copy status's colour were checked over a headless egui context, not in the running app.
- Nothing removes egui-snarl's state of a dropped canvas from egui's temporary memory. It is a few small entries per canvas made, and no later canvas reads them.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
