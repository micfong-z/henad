---
date: 2026-11-07
title: "Preview parameters"
description: "S24 of the #37 redesign. The Parameters tab drives the Editor's preview: a value tried there runs in the preview while the model keeps its own, survives the preview's rebuilds, and ends with Reset, Keep or an edit of the value. L1 is built: a graph or sheet parameter the build does not read is live, and both GPU engines take it between steps by writing their uniform blocks again, so the tick and the cells stay and later ticks follow the new value."
icon: material/tune-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s24-preview-parameters at fafd1b8, where the merge rebase found the branch already on the tip: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: fafd1b8
delta_state: "committed on `37-node-editor` by fast-forward: `feat: live uniform parameters on GPU models [draft]`, `feat: parameters drive the preview [draft]`, `fix: s24 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Preview parameters

> S24 makes the Parameters tab drive the Editor's preview (D54) and builds L1 with the tried values of the final `SPEC.md` (D53, D88).
> While a preview runs and the selection is the one it loaded under, the tab draws the preview's parameters, leaves the selected model's values, seed and schedule alone, and presses the preview's buttons.
> A value set there is *tried*: the preview runs with it, every rebuild of the preview keeps it, and the tab shows "Trying this value. The model keeps 0.3." with Reset and Keep, while the Sheet pane shows "The world is trying 0.5. The model keeps 0.3." under the parameter.
> Keep makes the tried value the sheet's as one edit with its range widened, and an edit of the value on the sheet, a new model or an opened model ends the trial.
> A parameter the build reads (a start, a look's reach, the tick length, a size) is reload-only and rebuilds the preview at tick 0 with the tried value; every other graph or sheet parameter is now live.
> Both GPU engines take a live parameter between steps by writing the uniform blocks of their passes again, and on the device a SIR sheet keeps its tick and cells through an infection rate edit and then steps exactly as the host at the new rate.

## State before

- `37-node-editor` at `a3f0480` (S21 merged) when S24 started, and at `fafd1b8` (S20 and S16a merged) when it was rebased.
- Every GPU graph parameter was reload-only: `GridProgram::entry` and `AgentProgram::entry` marked the file's parameters `on_reload`, and both GPU engines returned `false` from `set_param`.
- The Params tab drew the selected entry, and a preview was never the selection, so a slider moved during a preview reached nothing (`ui/params.rs`, recon `henad-app.md` section 1.4).
- The preview built at each parameter's default (`preview_values`), and its sheet buttons sat disabled under the selected model's actions.
- `sheet::sentence` already held the wording table's tried-value words from S4: `trying_value`, `RESET`, `reset_name`, `KEEP`, `keep_name`, `KEEP_TOOLTIP` and `sheet_trying_value`, with nothing drawing them.
- S16a, merged during S24, gave the Sheet pane its editing (`SheetEditor::edit`, `PaneState`, `sheet_preview_past_start`, `Preview::acted`), and S16b, with the Parameters section editing and undo, is still to come.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Live parameters on the GPU engines (L1)

`ModelEntry::from_grid_spec` and `from_agent_spec` read each declared parameter's `is_live` and pass the flags to every build through the crate-private `with_live_params(Vec<bool>)`.
A model type's engine (`GpuGridState`, `GpuAgentState`) gets none, so every registered GPU model stays reload-only, as `register_gpu_*` declares it.
`set_param` rejects a reload-only index and an index past the parameters.
For a live one the grid engine stores the value and writes the uniform block of every step pass, its counting block included, the display and the reduce pass again through `pass_params_bytes`, in the `GridPassCtx` of the build (`uniform_ctx`), so the bytes equal those of a fresh build at the new value.
The agent engine computes every block but the actions' first through `run_params_bytes` at the build's `AgentRun`, and rejects the edit with the run unchanged when a block fails.
An action's block is written at each press from the current values, and the reruns and the stats read them too.
The writes go through `queue.write_buffer` between submissions, so the next step reads them and the tick and the buffers stay.
Each built pass now keeps its pass id and its counting uniform (`BuiltPass`, `EncodedPass`, which also keeps its invocations).
The public docs of `from_gpu_grid_spec` and `from_gpu_agent_spec` state what a live parameter may feed.

### Live and reload-only graph parameters

`wgsl/live.rs` holds `reload_only(&HostEvaluator)`, and both entries mark each of the graph's own parameters through `own_params`.
A parameter is reload-only when the host reads it at build: the tick length of the graph's `time`, a parameter an init pass reads or runs by, and one the reach or the cell size of a radius fold is computed from, found through the prelude and the element with regions flattened.
The sizes in front stay reload-only.
For the starters that gives, beside the sizes: Game of Life's Initial density, SIR's tick length and Initial infected, Vote's initial share, the forest fire's Initial trees, boids' two ranges and two speeds (the start draws a speed between them), bounded confidence's hearing range, and the ants' site reward (a start reads it).
Every rate, chance, weight and step size the lines read is live.

### The Parameters tab's preview mode (D54)

`AppState::params_drive_preview` holds while a preview runs or builds and the selection is the one `load_preview` recorded, and `params_ui` then draws `preview_params_ui`.
It draws the loaded entry's parameters but the hidden ones and the sizes (`preview::SIZE_PARAMS`), through the `value_widget` that the usual rows now share, then the entry's buttons, pressed into the preview (marking `Preview::acted` for S16a's start rule), and no Seed row, schedule or notice.
A slider move records a `TriedValue` in the `Preview` (id, value and the value the model keeps, read from the sheet or the graph by `preview::model_value`), and a move back to the kept value ends it.
A live parameter then goes to the running preview through `AppState::send_preview_param`, and a reload-only one schedules `Preview::rebuild_after`, a rebuild at tick 0 after the 250 ms debounce with the key kept.
`build_preview` puts every tried value in place of its default (`with_tried`), so a rebuild after any sheet edit, and Build's replay of a sheet's preview, keep them.
A reload-only slider's tooltip reads "Changes restart the world from tick 0." (`RESTART_NOTICE`, the plan's restart wording of section 10.21).

### Tried values (D88)

Under a tried value `trial_row` shows `trying_value` with Reset, named "Reset Initial density to 0.3", and Keep, named "Keep 0.86 for Initial density" with the tooltip "Make it the value the model keeps".
Keep shows only while the Editor holds the document the preview runs.
Reset ends the trial and sends or rebuilds at the kept value.
Keep runs `SheetEditor::keep_value`, one `SheetEdit::SetParameter` with the range widened to hold the value and the sheet's load number kept, so the preview starts nothing again; for a graph it runs one `EditCommand::SetParam`.
`preview::poll_at` ends a trial whose kept value changed, as a sheet edit of that value does, and the Editor's loads reset the `Preview` with its trials.
The Sheet pane takes the trials (`SheetTrial`) from `editor_ui` and shows `sheet_trying_value` under each tried parameter.
The parameters' `kv_grid` ends after a tried parameter, the line spans the pane, and the next rows start a new grid with the same minimum first-column width, so the values stay aligned and the stripes stay.
The line wraps at the visible right edge of the pane (its clip rectangle), since the pane's content is wider than the pane when the Cells table is.

### Tests

- henad-compute `gpu/tests/live_params.rs`: `a_live_edit_keeps_the_run_and_later_steps_read_the_new_value` (both engines, a reload-only and an out-of-range index rejected, the counting block holding the new value), `an_engine_without_live_parameters_rejects_every_edit` and `a_value_whose_blocks_fail_is_rejected_and_the_run_keeps_its_value`.
- henad-graph `tests/live_params.rs`: `each_sheet_starter_marks_what_its_build_reads_reload_only` (all eight starters) and `the_sir_starters_rates_are_live`.
- henad-models `sheet_grid.rs`: `a_live_rate_edit_keeps_the_tick_and_the_cells_and_later_ticks_follow_it_on_the_device`, 64 by 64 cells, 10 ticks at 0.3, an edit to 2.0 with the tick and every cell kept, then 30 ticks equal to the host evaluator at 2.0 and apart from a run that kept 0.3; Initial infected, the tick length and the grid width are rejected.
  The testing kit's `ApplyModes` check, which every sheet entry already runs, now holds the engines' acceptance to the new flags.
- henad-app `tests/preview_params.rs`: `a_params_tab_change_rebuilds_the_preview_with_the_value_and_keeps_it_across_an_edit` (Initial density 0.9 rebuilds at tick 0, survives a world-size edit, ends at an edit of the value), `a_live_parameter_reaches_the_running_preview_and_the_tick_keeps_counting`, `the_preview_mode_leaves_the_selected_model_alone`, `reset_and_keep_end_a_trial`, `keep_widens_the_range_to_hold_the_value`, `a_tried_value_shows_its_note_reset_keep_and_the_sheets_line` (the words, the names, no Seed, the New outbreak button, no forbidden word) and `a_tried_value_reaches_the_sheet_as_its_shortest_decimal` (`params::sheet_number`, shared by the sheet's line and Keep).

### UI checks (egui MCP, secondary display)

Under the lock of plan section 2.4, on the branch before the rebase, the window at (-1800, 60), 1600 by 960:

1. `01-life-density-tried.png`: Game of Life from New model; the Parameters tab shows only Initial density and the buttons Randomise and Clear. Dragging Initial density to 0.86 rebuilt the preview at tick 0 with 56286 of 65536 cells alive (19820 at 0.3), the restart tooltip under the pointer.
2. `02-life-trial-row.png`: "Trying this value. The model keeps 0.3." with Reset and Keep under the slider, and the sheet's line under Initial density.
3. `03-sir-running.png`: the SIR sheet's preview after Play, ticking under adaptive batching.
4. `04-sir-live-rate.png`: Infection rate dragged to 2.7 during the run; the tick went on from 74414 to 505527 with no rebuild and the trial row showing. The epidemic had already burnt out at that speed, so the new rate shows in the tick counter rather than the plot.
5. `05-fire-trying-line.png`: the forest fire with Lightning chance tried at 0.00032; "The world is trying 0.00032. The model keeps 1e-5." under the parameter, read whole from the tree, clipped at the pane's right edge.
6. `06-fire-kept.png`: after Keep, the sheet reads 0.00032, the trial row and line are gone, and the preview stayed at tick 0.

After the rebase onto S16a the check was started again under the lock: New model and Create worked, then the window stopped painting (the macOS session would not bring it to the front, likely a locked screen overnight) and no screenshot could be taken.
The app was quit, `app.ron` restored and the lock released.

After the review fixes, checks 1 to 6 ran again on the rebased branch (on `fafd1b8`, with the review fixes) under the lock, the window placed on G15F1 at (-1800, 60), 1600 by 960.
The macOS session was locked, so the window drew no frame a screenshot could take, and every check was read from the accessibility tree through the egui MCP server, with no screenshots.
The app still ran its frames and took the synthesized input.

1. Game of Life from New model: the Parameters tab held only Initial density, Randomise and Clear, and no Seed row.
   Dragging Initial density to 0.9 rebuilt the preview at tick 0 with 58798 of 65536 cells alive (19820 at 0.3), and hovering the slider showed "Changes restart the world from tick 0.".
2. "Trying this value. The model keeps 0.3." with Reset ("Reset Initial density to 0.3") and Keep ("Keep 0.9 for Initial density"), and on the sheet "The world is trying 0.9. The model keeps 0.3." on one line from x 782 to 1045, inside the pane's visible edge at about 1050.
3. and 4. The SIR epidemic from New model, Play, then Infection rate dragged to 2.7 during the run: the tick read 35434 before the drag, 36398 right after it and 38651 twenty frames later, with no rebuild, the trial row in the tab and the trying line under Infection rate on the sheet.
   The values of Infection rate and Recovery rate, in the grids before and after the line, both sit at x 983.
5. The forest fire, Lightning chance typed as 0.00032: "Trying this value. The model keeps 0.00001." in the tab, and on the sheet "The world is trying 0.00032. The model keeps 1e-5." wrapped onto two lines from x 782 to 1015, now whole inside the pane where the first check found it clipped.
6. Keep: the sheet's Lightning chance read 0.00032, the trial row, the trying line and the restart tooltip were gone, and the tick stayed 0.

The app was quit, `app.ron` restored from this milestone's backup and the lock released.

### Edited tree

```
CHANGELOG.md                                       live parameters and tried values
agents/
├── henad-app-editor.md                            build_preview takes the tried values
├── henad-app-sheet.md                             SheetTrial, the trying line, keep_value
├── henad-app.md                                   the Parameters tab's preview mode
├── henad-compute.md                               live parameters on the GPU engines
├── henad-graph-wgsl.md                            live and reload-only parameters
└── sim-runner.md                                  SetParam on the GPU loop
crates/henad-app/src/
├── state.rs                                       params_drive_preview, send_preview_param
├── tests/
│   ├── mod.rs                                     preview_params
│   └── preview_params.rs                          new
└── ui/
    ├── editor/
    │   ├── mod.rs                                 sheet_trials passed to the sheet editor
    │   ├── preview.rs                             TriedValue, SIZE_PARAMS, model_value, rebuild_after
    │   └── sheet/
    │       ├── mod.rs                             keep_value, sheet_ui takes the trials
    │       └── sheet_pane.rs                      SheetTrial, the trying line, the grid split at tried values
    └── params.rs                                  preview mode, value_widget, trial row, Keep, sheet_number
crates/henad-compute/src/
├── entry/mod.rs                                   live flags from the declarations, docs
├── simulation.rs                                  set_param's Errors names the agent hooks' rejection
└── gpu/
    ├── agent_engine.rs                            live set_param, uniform_blocks
    ├── agent_spec.rs                              AgentRun::tick doc
    ├── grid_engine.rs                             live set_param, rewrite_uniforms
    ├── grid_spec.rs                               GridPassCtx::tick doc
    └── tests/
        ├── live_params.rs                         new
        └── mod.rs                                 live_params
crates/henad-graph/src/
├── sheet/sentence.rs                              RESTART_NOTICE doc names the slider tooltip
├── tests/
│   ├── live_params.rs                             new
│   └── mod.rs                                     live_params
└── wgsl/
    ├── agents.rs                                  own_params on the entry
    ├── emit.rs                                    defined is pub(crate)
    ├── live.rs                                    new: reload_only
    └── mod.rs                                     own_params, live module
crates/henad-models/src/tests/sheet_grid.rs        the SIR live rate on the device
```

### Deviations from the plan

- **`math.powi` tables are live.**
  The plan lists `math.powi` tables among the reload-only sources.
  The table's length is a compile-time literal, and its values are recomputed on the host from the prelude's base at every write of the block (`FieldSource::Table`), so a parameter feeding the base changes the table correctly.
  The SIR starter's infection rate feeds exactly such a base (`e^(-r dt)` raised to the count), and the acceptance asks for it to be live.
- **A radius fold's reach is reload-only too.**
  The plan names `cell_size` sources.
  The reach decides the work a fold does, which the build's budget check and the walk caps assume, so a parameter feeding a fold's radius is reload-only as well.
  In every agent starter the reach and the cell size come from the same parameters.
- **The flags come from the declarations.**
  The plan has henad-graph mark each parameter and the engines take live ones; the engines read the mark from `EntryDeclarations.params`, the descriptors the panel and the testing kit already read, with no new spec field.
- **The tests sit in a new `tests/preview_params.rs`.**
  The plan names `tests/sheet_editor.rs`, which S16a rewrote while S24 was open; a separate file kept the two apart.
- **"Values kept per preview key" are kept per document.**
  The preview key changes at every edit since S16a, which would drop the values at the rebuild the acceptance asks them to survive.
  The tried values live in the `Preview` until the Editor loads another document, and each ends when the value it was tried against changes.
- **Keep has no Undo yet.**
  D88 makes Keep one sheet edit with Undo, and the undo stack arrives with S16b.
  `keep_value` is one `SheetEdit`, and S16b routes it through its undo when it lands. Keep keeps the preview's key, where `SheetEditor::edit` would start the preview again, since the preview already runs with the kept value.
- **The preview mode covers a graph's preview too.**
  A graph's Keep is one `EditCommand::SetParam`, which goes through the graph editor's undo and rebuilds its preview at the kept value.
- **Commit order.**
  The engine commit comes first, since the app's live path needs it; the plan lists the app commit first.
  The fix of the trying line's layout, made after the first UI check, was folded into the app commit during the rebase.
  The ants starter's case that S20 brought was first a separate `fix: s24 rebase onto sheet editing [draft]`, and the review folded it into the engine commit, so each commit passes its own tests.
- **S24 merges before S16b.**
  The gate's D88 builds the work in "S24, merged after S16b", and its S24 row says "The sheet line sits in `sheet_pane.rs`, so S24 merges after S16b".
  S16b had not merged when S24 was ready, and S24 was rebased onto S16a instead.
  Two consequences follow until S16b lands.
  Keep on a sheet is one `SheetEdit` with no Undo, where D88 asks for one with Undo, and the trying line sits in the read-only `parameters` of `sheet_pane.rs`, which S16b replaces.
  S16b takes over both: it keeps the trying line under each editable parameter row (`SheetTrial` is passed down already) and sends `SheetEditor::keep_value` through its undo stack, with a test that Undo restores the old value and range.
  Holding S24 until S16b, the other way out, is the orchestrator's call; this record names the departure in case S24 merges first.
- **A set_param rejection by the agent hooks reads as reload-only.**
  `Simulation::set_param` reports every `false` from the state as `SetupError::ReloadOnly`, and the agent engine now also returns `false` for a value at which a uniform block cannot be written.
  The `# Errors` section says so.
  A separate error variant would change henad-compute's public API for a case no starter reaches.

## State after

- A sheet's or graph's preview takes its parameter values from the Parameters tab, live where the build does not read them and by a rebuild at tick 0 otherwise, and the selected model is untouched.
- Tried values show in the tab and on the sheet in the wording table's words, and Reset and Keep end them.
- An applied graph or sheet takes its live parameters from the Parameters tab's usual rows too, through `send_live_param`, now that the engine accepts them.
- A sweep sets every value at build and sends no edit, so its runs are unchanged.
- The branch is rebased on `fafd1b8` with S16a and S20; S16b, S29 and later milestones are the open neighbours.
- `37-node-editor` holds the three unsigned commits, the feature, the engine change and the review's fixes, fast-forwarded with no merge-check fix.

### Merge check

- The merge rebase onto `37-node-editor` found the branch already on its tip, `fafd1b8`, so no commit was replayed and no conflict arose.
- The full check then ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.
- The rebase brought no change, so no egui MCP check was repeated.

## Issues found & future directions

- **S16b.** The Parameters section on the sheet replaces the read-only `parameters` of `sheet_pane.rs`, which draws the trying line; S16b keeps the line under each parameter row (`SheetTrial` is passed down already) and routes `keep_value` through its undo stack.
- **The Sheet pane is wider than its panel.** The Cells table and the trying line are clipped at the pane's right edge in a 1600-point window (the line reads whole in the tree). The pane's `ScrollArea` takes the width of its widest row; a wrapping Cells table would fix both.
- **Number formats differ.** The Params tab writes 0.00001 and the sheet 1e-5 for the same value, each following its own existing rule (`display_value`, `format_number`).
- **A live edit is a session action.** It does not reach a replay, as for the stock models' live sliders; a run with tried values is not reproducible from its seed alone.
- **The reach rule is coarse.** A parameter feeding both a fold's reach and a line's chance is reload-only for both uses. No starter has one.
- **The UI checks after the review read the tree only.** The session was locked, so no screenshot shows the rebased branch, and a look at the trying line's wrapping on screen remains for the maintainer.
- **The merge order against S16b** is open, as the deviation "S24 merges before S16b" says.

## Decisions adopted for review

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D53 | Live parameter sliders (L1) | recon `engine.md` section 2.10 | Built only if the final `SPEC.md` keeps "The parameter sliders, live", for uniform-only parameters | S24 |
| D54 | Which sliders drive the preview | recon `henad-app.md` | The Params tab's preview mode, with values kept per preview key | S24 |
| D88 | Tried parameter values and L1 | `CHANGES-B.md` B7, final `SPEC.md` 7.4, 10.6 and 11, updating D53 and D54 | The final `SPEC.md`'s tried values, on the Params tab's preview mode (D54): a value set there is tried by the run, and the sheet keeps its own. The tab shows "Trying this value. The model keeps 0.0001." with Reset and Keep, Keep is one sheet edit with Undo, the sheet's parameter row shows "The world is trying 0.0005. The model keeps 0.0001.", and a sheet edit of that value, a new model or an opened model clears it. L1 is built (D53), since the final text keeps sliders that change a running world (section 9). A uniform-only parameter changes the run live. A reload-only parameter rebuilds at tick 0 with the tried value, a **Departure** from 10.6, where a Starts as value tried after the first tick waits for Start again, kept for D52's single restart rule | S24, merged after S16b (see the deviation "S24 merges before S16b") |
| D100 | The prototype's departures after round B | `CHANGES-B.md` "Departures" (B1, B2, B3, B4, B7, B10, B19) | Kept as built, as D37 keeps round A's: "+ Chance" writes a Become, chance results named after what they draw on (D85), results scoped to their When (D84), the optional "for each ..." blank, the fold offers, "always" shown on the line, and the "less than ... or more than ..." condition (D82). B4 and B7 map as D86 and D88 say | S19, S22, S24, S25 |

S24 also takes four choices of its own, listed under the deviations: `math.powi` bases live, a fold's reach reload-only, tried values kept per document rather than per key, and the preview mode over a graph's preview with its Keep as a graph edit.

## Review

The review found one must-fix, three should-fixes and seven nits.
Every finding held, though the premise of one nit held only in part.
The agent engine's `set_param` doc opened with the stale "Rejects every live edit." line, now gone.
The merge order against S16b is written down as a deviation for the orchestrator, with what S16b must take over.
The UI checks ran again on the rebased branch, from the tree only, since the session was locked.
The reload-only tooltip "The world will start again from tick 0 with this value." was new text outside the wording table, and the slider now reuses `RESTART_NOTICE` ("Changes restart the world from tick 0.", section 10.21) instead of adding a departure.
`TRY_RESTARTS_TOOLTIP` is gone.
The ants starter's case was folded into the engine commit with a non-interactive autosquash, so the engine commit passes its own graph test.
The docs of `TrialChoice`, `EncodedPass::id` and `::invocations`, `BuiltPass::id`, `GpuGridEngine::uniform_ctx` and `AgentRun::tick` were reworded or completed, and the lines past the width in `wgsl/live.rs`, `agents/henad-compute.md` and `simulation.rs` rewrapped.
`params::sheet_number` replaces the copied widening in `sheet_trials` and in Keep, with a test.
`Simulation::set_param`'s `# Errors` names the agent hooks' rejection, recorded above as a deviation.
The nit on the per-parameter grids was half right: `kv_grid` gives every grid a half-width first column, so the values were already aligned unless a name is wider than that, but a one-row grid never stripes.
The parameters now share a grid that ends only after a tried parameter, so the stripes are back, and the trying line wraps at the pane's visible edge.
The first UI check had found that line clipped.
A single grid with the line in the value column, the reviewer's first suggestion, was tried first.
It wrapped the line into four or five lines about 70 points wide.
After the fixes, fmt and clippy with `-D warnings` over henad-compute, henad-graph, henad-models and henad-app, rustdoc with `-D warnings` over henad-compute and henad-graph, and the wasm32 typecheck of henad-compute, henad-graph and henad-app pass.
With `HENAD_REQUIRE_GPU=1`, henad-compute has 243 passed, the henad-graph library 432 passed and 1 ignored, the henad-app library 411 passed, and the henad-models library 189 passed, 1 failed and 1 ignored, the failure the baseline's budget test, with every integration test passing.

## Benchmark commands for the maintainer

A live edit costs one host evaluation of each pass's prelude and one `write_buffer` per block, once per edit and never per step, and a step encodes nothing new, so no step cost changes.
Nothing here needs measuring; a check that the stepping rate of an applied SIR sheet is unchanged, interleaved on a cooled machine as `agents/performance.md` asks, would be:

```bash
for rep in 1 2 3; do
  cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/sir.toml \
    --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 1 --json
done
```

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
