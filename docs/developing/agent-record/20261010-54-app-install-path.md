---
date: 2026-10-10
title: "App install path"
description: "Milestone M5 of #49. The app pins the entry its live simulation was built from, installs a model set by id through ModelSet::replace, carries the selection's values and scheduled actions to new code by id, and asks before a run past tick 0 is rebuilt. The Sweep tab forgets the replaced model's columns, and the Results tab warns on runs of other code and refuses to resume a folder of other runtime or plugin code."
icon: material/swap-horizontal
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#49"
state: "install path, pinned entry, Rebuild model modal, Sweep tab and Results invalidation built, reviewed and fixed, check.sh and check_hot.sh passing with HENAD_REQUIRE_GPU=1 after the review fixes, the app checked through the egui MCP server, uncommitted on top of f2d7760"
baseline_commit: f2d7760
delta_state: "uncommitted on `49/m5-app-install-path`, 7 new files and 17 changed files"
---

# App install path

> Milestone M5 of #49 (dynamic model loading), decisions 1, 2 and 10 of its plan.
> A host that changes a model's code hands the app a model set, and the app has to follow without a restart.
> `AppState` now keeps the `ModelEntry` its live simulation was built from, so the panels, pacing, the run details, a fault's model name and live edits describe the running code after the set's entry under its id is replaced.
> `AppState::install_models` replaces each entry by id through `ModelSet::replace`, so a runtime or plugin entry can shadow a built-in id.
> The selected model keeps its values, seed and scheduled actions when its declarations are unchanged, and otherwise gets them carried to the new code by parameter id and action id.
> A loaded run at tick 0 is rebuilt at once, and a run past tick 0 opens the Rebuild model modal, whose Cancel keeps the old run going and leaves "Model code changed. Press Build to apply." in the Parameters tab.
> The Sweep tab forgets the model's stat columns and, when its declarations changed, its draft, and the Results tab compares its runs with the new code, so Open on a run of replaced code warns.
> Nothing in the app calls `install_models` yet. M6's editor and M8's reload are its callers, and the tests drive it.
> Two reviews followed, and the session fixed what they found: a rebuild now checks the new code against the device and the Seed field before it tears the old run down, the modal counts against the running code, a run to a tick counts as a run past tick 0, an install of unchanged code leaves the tabs alone, and Resume sweep is disabled for a folder of other runtime or plugin code.

## State before

`49/m5-app-install-path` branched from `49-dynamic-loading` at `f2d7760`, which holds rounds 1 and 2 as placeholder commits: D1, M1 (#50), M3 (#52), D2 and M2 (#51).
M2 gave `ModelSet::replace` and code identity on `ModelSource` (origin, `ModelCode`, `code_hash`), and its record left the install path to this milestone.
D2 gave the app a dev host whose warm-up returns no model set, so nothing in the app learns of new code yet.

`AppState` named the loaded model by id alone (`loaded_model: Option<String>`), and `loaded_entry()` looked it up in the set.
After a replace, research-code-map.md's app-integration §1 and sweeps-cli-provenance §1.2 list what goes stale:

- `selection_is_loaded()` compared ids, so `send_live_param` sent indices of the new descriptors to the old state.
- The Parameters panel, pacing, the run details and a fault's model name read the new entry while the old code ran.
- The selection's positional `param_values` and its schedule no longer matched the new declarations, and Build was disabled.
- The Sweep tab built a model's stat columns once per id and kept them for good, and its drafts held one row per parameter of the old code.
- An in-app sweep's Results store recorded no builds, so Open replayed a run through the new code without a warning.
- `OpenedRun` cleared its modified mark on a rebuild from the same values under other code.

## What was done

### The pinned entry (`state.rs`)

- The private `loaded: Option<ModelEntry>` replaces `loaded_model`.
  It is the entry of the `RunSetup` the simulation was built from, `loaded_entry()` returns it, and the new `loaded_id()` returns its id.
- `selection_is_loaded()` compares the id and the `ModelSource`.
  The new `code_pending()` holds for the same id under another source.
  Live edits, action buttons, Run to and `HenadApp::logic`'s second run to a passed target all read `selection_is_loaded`, and so none of them reaches old code with the new code's indices.
- `reset_simulation` builds through a new private `load(Option<RunSetup>)`, which takes the schedule from the setup, pins the setup's entry and records `loaded_values`, `loaded_seed` and `loaded_schedule` from it.
  It resets `pending_reload` only for the selected model.
  A failed build names the setup's model in the fault modal through the new private `show_fault`.
- `select_model` on the loaded model restores the values and schedule it runs with, carried by id to the set's entry for that id (`remap_values`, `remap_schedule`), since the set can hold other code by then.
- `OpenedRun` records its `source`, the source of the entry that built it at open.
  `settle_opened_run` takes the setup being built and marks the run modified when the setup's source or values differ from the run's, so a rebuild under other code marks it and a rebuild of the run's own code and values clears it.

### The install path (`state/install.rs`)

`state/install.rs` is a child module of `state`, and reaches `AppState`'s private `loaded` and `load`.

- `install_models(&ModelSet)` replaces each entry by id, or appends it.
  A replacement with the same `ModelSource` and the same `schema_hash` changes nothing else.
  For each other replaced id, a selected model whose `schema_hash` changed has its `param_values`, `pending_reload` marks and schedule carried to the new entry, and keeps all three when the hash matches.
  It calls `SweepPanel::forget_model` and `ResultsPanel::model_replaced` for each such id, and settles the loaded model once at the end.
- `remap_values` carries values by parameter id.
  A choice is carried by its option's name, so reordered options keep their meaning.
  A parameter new to the code, or a value its new descriptor rejects (`check_value`), takes its default, and the count of those is returned.
  `remap_schedule` carries each scheduled action to its action's new index and drops an entry whose action is gone.
- A loaded model whose set entry now has another source is rebuilt at once while `run_at_start` holds and `rebuild_refusal` returns no reason.
  `run_at_start` reads no snapshot past tick 0, no play and no pending run to a tick.
  The snapshot lags the run, and an opened run on its way to a tick still reads tick 0.
- Otherwise `pending_rebuild` holds a `PendingRebuild` with the model id, the number of the run's values that the new code does not take, and the number of the run's scheduled actions it drops.
  Both are counted against the loaded entry, its `loaded_values` and its `loaded_schedule`, and so stay right across a Cancel and a second install.
- `rebuild_loaded` rebuilds a selected model as Build does, from the values of the next build, which the install carried already.
  A model loaded while another one is selected is rebuilt from the values, seed and actions it runs with, carried by id (`carried_setup`), and the selection stays.
- `rebuild_refusal` returns `build_refusal` for the selected model, and for another model the carried setup's `SetupError` or the device's shortfalls at its values.
  `rebuild_loaded` checks it before any teardown and leaves the run alone while it returns a reason.
- An install whose replacement has the loaded model's own source leaves the run alone and closes any pending modal.

### The Rebuild model modal (`ui/rebuild.rs`)

The modal draws after the fault modal in `HenadApp::ui`.
Its title is "Rebuild model", its body "The model will be rebuilt from tick 0. The current run will be lost.", and below it one line per kind of loss: "2 parameters will use their defaults." ("1 parameter will use its default.") and "1 scheduled action will be removed.".
Cancel comes first and Rebuild second, in the destructive style of the Sweep tab's modals (`destructive_button`, MCS red, square corners).
Rebuild is disabled while `AppState::rebuild_refusal` returns a reason, with the reason as its tooltip.
Cancel, Escape and a click beside the modal clear `pending_rebuild`, and the run keeps going.

### The Parameters and Playback panels

`params.rs`'s banner logic moved into `notice_content`, which returns a `ParamsNotice` with a `ParamsNoticeTone` that `notice` maps to the theme's colours, so a test can read the banner.
A new branch before "Selected model not loaded" shows "Reload needed" with "Model code changed. Press Build to apply." while `code_pending()` holds.
Run's disabled reason reads "Model code changed. Press Build first." in the same state.
Playback's `build_refusal` moved to `AppState::build_refusal`, with `NO_SELECTED_MODEL` and a shared `too_large_message`, so the rebuild path refuses what Build refuses.

### The Sweep tab

- `SweepPanel::forget_model(id, declarations_changed)` drops the id's `ColumnsBuild` and the last check, and the draft when the declarations changed.
  The next frame starts a column build of the new code.
- `KeptDraft` holds the entry its session started with.
  A session's Save spec writes the draft against that entry's schema, and Edit sweep restores the kept panel values only while the current entry declares the same schema.
  Both paired the kept draft with the set's entry by position before.

### The Results tab

- `ResultsStore` keeps a private `RecordedModel`: the engine and model builds the sessions recorded, the schema hash and the replay flag, from a folder's manifest or from the entry an in-app sweep ran.
  `compare_builds` reads it in place of the `ResultSet`, which the store consumes.
- `ResultsStore::model_changed(Option<&ModelEntry>)` recomputes `schema_matches`, `changed_builds`, `unidentified_builds`, `replays_exactly` and `code_matches`, and `ResultsPanel::model_replaced(id, entry)` calls it for a store of that model.
  The table's existing warnings then show beside Open: "Model build changed since this sweep. The replay might differ." or "Model parameters, stats or actions changed since this sweep. The replay might differ.".
- A folder read on the `henad-results` thread compares again with the current set when it lands, since the set can change while the thread reads.
- `ResultsStore::code_matches` holds whether the app's entry runs the code every session of a folder ran its runs with, the rule a resume sets for runtime and plugin code.
  `RecordedModel` keeps a folder's manifest for it, and `model_changed` recomputes it.
  Resume sweep is disabled while it fails, with "Model code changed since this sweep ran" (`code_refusal`) as its reason, in place of a resume that `ResumeError::CodeChanged` ends.
  henad-explore gains `Manifest::code_matches`, the body of `ResultSet::code_matches`, which now calls it.
  The store holds no `ResultSet`.
- `replay_warnings` is `pub(crate)` for the tests.

### No caller yet

D2's dev host returns no model set, so nothing in the app calls `install_models`.
`state/install.rs` carries a module-level `#![cfg_attr(not(test), expect(dead_code, reason = ..))]`, and `forget_model` and `model_replaced` carry the same attribute.
Rust's dead-code lint is transitive, so `PendingRebuild`'s construction and the helpers only `install_models` reaches fall under the module's expectation.
The first caller turns each expectation unfulfilled, an error under `-D warnings`, and so removes them.

### Tests

henad-app gains `src/tests/` with `support.rs` (`EditedSir`, a grid model under SIR's id with an infection rate capped at 0.5, no recovery rate, a new vaccination rate, one stat fewer and a new action ahead of the outbreak; `set_of`; `recoded`, which gives an entry runtime code and counts its builds through `wrap_factory`; `step_past`) and `install.rs`.
lib.rs's three `requested_threads` tests moved to `src/tests/threads.rs`, since a crate root lists one `mod tests`.

| Test | What it holds |
|---|---|
| `the_panels_describe_the_running_entry` | After Cancel, the loaded entry, the run details' `model_name`, `model_source` and `params`, and a fault's model name are the old code's. A live edit by a new index is not sent. Picking the model again carries the running values to the new code |
| `an_opened_run_is_modified_after_its_source_changes` | An opened run at tick 0 is rebuilt under new code and marked modified, and installing its own code again clears the mark |
| `install_keeps_values_when_the_schema_matches` | Values, pending marks, seed and schedule unchanged, no build of an unloaded model, and a loaded one rebuilt at tick 0 from the same values |
| `install_remaps_values_by_id_when_the_schema_changes` | Values and pending marks by id, a rejected value and a new parameter at their defaults, the outbreak moved to index 1, and an action the code dropped removed on the way back |
| `a_choice_is_carried_by_the_name_of_its_option` | Choices by option name, a removed option at its default, and a dropped schedule entry counted |
| `results_flag_runs_of_replaced_code` | An in-app sweep's store warns of a changed model build, then of changed declarations, and not for its own code or another model's |
| `install_works_for_every_model_type` | SIR, Boids, Virus on a Network, GPU Game of Life and GPU Boids, the GPU ones on the headless device: a run at tick 0 rebuilt at once under the new entry, and a run past tick 0 waiting on the modal, then rebuilt from tick 0 |
| `a_loaded_run_past_tick_zero_asks_before_rebuilding` | The modal's counts, unchanged after Cancel and a second install that edits the kernel alone, the old run untouched until Rebuild, and a model loaded while another one is selected rebuilt from its running values with the selection kept |
| `cancel_keeps_the_old_run_and_flags_the_code_change` | After Cancel the old run steps on, the banner reads "Model code changed", and Build builds the new code |
| `an_install_during_a_run_to_asks_first` | A run to tick 50 whose first snapshot has not arrived waits on the modal and keeps its target |
| `a_rebuild_the_new_code_refuses_leaves_the_old_run_alone` | A run at tick 0 with an invalid Seed field is not torn down: the modal shows the reason, `rebuild_loaded` does nothing, and a valid field lets it rebuild |
| `install_forgets_the_sweep_tab_columns_of_the_model` (`ui/sweep/mod.rs`) | An install of the same code keeps everything. The replaced model's columns and the last check are dropped, its draft kept for unchanged declarations and dropped for changed ones, and another model's left alone |
| `a_folder_store_compares_its_runs_with_a_replaced_entry` (`ui/results/store.rs`) | A store read from a sweep's files follows runtime code, changed declarations, no entry and the original code, and refuses a resume of other runtime code with its reason |
| `the_modal_names_what_the_rebuild_drops` (`ui/rebuild.rs`) | The modal's count lines in the singular and the plural |

Setting `selection_is_loaded` back to an id comparison fails `the_panels_describe_the_running_entry` and `cancel_keeps_the_old_run_and_flags_the_code_change`.
Dropping the run-to check from `run_at_start` fails `an_install_during_a_run_to_asks_first`, and dropping the refusal check before the rebuild at tick 0 fails `a_rebuild_the_new_code_refuses_leaves_the_old_run_alone`.
Each was checked by hand and reverted.

### Docs

- `docs/guide/app.md`: a Model code changes section before Model build and simulation errors (the new code replacing the model of its id, the running run keeping its code, the dialog with Rebuild and Cancel, Rebuild disabled when the new code cannot build, values carried by id, the Sweep and Results tabs), the Reload needed row of the banner table, the (modified) mark, the Results warning for a sweep run in the same window, and Resume sweep disabled for a folder of other runtime or plugin code.
- `docs/guide/hot-reload.md`: one line under What a reload covers pointing to that section.
- `AGENTS.md`: the test-module rule names henad-app's `src/tests/`, the panel file list gains `rebuild.rs`, and the henad-app paragraph describes the pinned entry, `load`, `build_refusal`, the remapped selection, `OpenedRun::source`, the install path with `rebuild_refusal` and its keep-list, and the Resume refusal. henad-explore's paragraph names `Manifest::code_matches`.
  Lines the edits left short are reflowed to the file's 100 columns.
- `CHANGELOG.md`: one Added line.

### Edited tree

```text
.
├── AGENTS.md, CHANGELOG.md, zensical.toml        ~ install path, Added line, nav #54
├── crates/henad-explore/src/
│   ├── output/manifest.rs                        ~ Manifest::code_matches
│   └── result_set.rs                             ~ ResultSet::code_matches calls it
├── crates/henad-app/src/
│   ├── lib.rs                                    ~ rebuild modal drawn, mod tests, inline tests moved out
│   ├── state.rs                                  ~ pinned loaded entry, load, selection_is_loaded by source,
│   │                                               code_pending, loaded_id, OpenedRun::source, select_model remap,
│   │                                               build_refusal and too_large_message from Playback
│   ├── state/
│   │   └── install.rs                            + install_models, rebuild_loaded, rebuild_refusal,
│   │                                               PendingRebuild, remap_values, remap_schedule
│   ├── tests/
│   │   ├── mod.rs                                +
│   │   ├── support.rs                            + EditedSir and helpers
│   │   ├── install.rs                            + eleven tests
│   │   └── threads.rs                            + requested_threads tests, moved from lib.rs
│   └── ui/
│       ├── mod.rs                                ~ mod rebuild
│       ├── rebuild.rs                            + Rebuild model modal, test
│       ├── params.rs                             ~ notice_content, ParamsNotice, Model code changed
│       ├── playback.rs                           ~ build_refusal moved to AppState, Run's reason
│       ├── sweep/mod.rs                          ~ forget_model, Save spec and Edit sweep on the kept entry, test
│       ├── sweep/session.rs                      ~ KeptDraft::entry
│       ├── results/mod.rs                        ~ model_replaced, folder load compared on landing,
│       │                                           Resume disabled by code_refusal
│       ├── results/store.rs                      ~ RecordedModel, model_changed, code_matches, test
│       └── results/table.rs                      ~ replay_warnings pub(crate)
└── docs/
    ├── guide/app.md                              ~ Model code changes
    ├── guide/hot-reload.md                       ~ link to it
    └── developing/agent-record/
        └── 20261010-54-app-install-path.md       +
```

## State after

Everything is uncommitted on `49/m5-app-install-path`, on top of `f2d7760`, in the worktree `.claude/worktrees/49-m5`.
Nothing was committed, pushed or tagged.

All checks below ran on the final tree, after the review fixes.
The implementer's runs before the review passed as well.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: the wasm32 typechecks, formatting of the workspace and the excluded packages, packaging, cargo-deny, clippy, 1278 Rust and doc tests with none failed and 6 ignored as before, the README program, the docs with `-D warnings`, the scripts' tests and the web build.
  The first run stopped on `a_location_survives_a_panic_on_a_rayon_worker`, the known flaky test, alone, and the rerun passed whole.
  The one warning is the future-incompatibility note on naga, wgpu, wgpu-core and wgpu-hal, as on `master`.
  `schema_hashes_are_unchanged_since_0_2_0`, the CLI goldens and JSON-line tests, the tutorial's `snippets`, `parity` and `shaders` tests, `the_readme_program_matches_the_example`, the 0.2.0 fixture tests and the byte-identity sweep tests are among the passed tests.
- `HENAD_REQUIRE_GPU=1 ./scripts/check_hot.sh` passes: hot-host's app self-check ("the dev host built, loaded and checked 12 models"), every check of `hot-check`, the twenty reload cycles at 11.0 MiB, and `hot-check relink`.
- `uv run zensical build` reports no issues, and the guide's new anchor `#model-code-changes` renders.
- The app ran on the main monitor before and after the review fixes, a release build with `--features inspection` and `EGUI_INSPECTION=1`, driven through the egui MCP server and ended with `pkill` each time.
  Before the fixes, Build built SIR and enabled Seed outbreak, Play, Step, Run and Offload with no banner.
  Run to tick 50 stopped at tick 50, and Run to tick 20, behind it, rebuilt and stopped at tick 20.
  A one-run sweep from the Sweep tab finished, its run in the Results tab's Runs view opened through Open at end as "Sweep run 0: config 0, replicate 0" with no "(modified)" and no replay warning, and stepped to tick 1000.
  The About window showed Henad 0.3.0, the commit `f2d77601 (2026-10-10), modified`, and the Models row `henad-models 0.3.0 (sources d0e4575fd52ff0f8)`.
  After the fixes, which move Build's refusal into `AppState`, Build, Run to tick 50 and Run to tick 20 behaved the same, the About window showed the same Models row, and a one-run sweep finished into the Results tab.
  A row of the Runs table did not take a synthesized click in that run, the window being occluded and never painting, so Open was not pressed again.
  The fixes leave the table and `open_run` alone, and the tests open runs.
  The Rebuild model modal has no caller in the app yet, and the tests cover it.
- The scratch directory `/Users/micfong/.claude/jobs/3144150c/tmp/m5/` held the check, docs and app logs alone, and is deleted.
  No template copy was built: the template names no item that changed.

Proposed commit: `feat: app install path (pinned entry, remap by id, rebuild warning)`.

M5 has no benchmark gate, and no per-tick path changed.
`selection_is_loaded` compares two `ModelSource`s where it compared two strings, a few times per frame.

## Issues found & future directions

- **Rebuild of the selected model is Build.**
  It builds from the values of the next build, so a reload-only edit pending in the Parameters tab is applied with the new code.
  A model loaded while another one is selected rebuilds from the values it runs with instead, since the panel holds another model's values.
- **Code identity is source equality.**
  A replacement with the loaded model's own source is no change for the run, and opens no modal.
  The tabs skip it only when its declarations match too.
  Two plugin builds of a crate without a source hash compare equal, and M8 refuses such entries, as design.md §1.5 (c) step 6 lists.
- **The modal counts against the run.**
  For the selected model, Rebuild builds from the panel values, and a value edited in the panel after the install takes the place of a default the modal counted.
- **Live edits wait for Build after Cancel, even with unchanged declarations.**
  `selection_is_loaded` compares sources, so a code change stops live edits and actions until the next build, whether or not the parameters changed.
- **The Results store keeps the opening entry's plan and descriptors.**
  `model_changed` recomputes the comparison alone.
  A folder opened while the set lacked its model keeps its "does not include model" refusal after an install adds the model, until it is opened again, and a search's replay parses its values with the descriptors of the opening entry.
- **`OpenedRun::source` is the source at open time,** not the build the sweep recorded.
  The Results tab's warning covers a run of other code at open.
- **No removal.** `install_models` only replaces and appends.
  An id an earlier plugin load added and a new load lacks stays in the set, which design.md §1.5 (c) step 7 leaves to M8 with `remove`.
- **A device error at build time ends the old run,** as the plan keeps it.
  The caller compiles first, and a build that fails on the device is reported by the fault modal after teardown.

### Deviations from the plan

- `install_models` takes `&ModelSet`.
  clippy's `needless_pass_by_value` refuses an owned set it only iterates.
- The modal adds a line for dropped scheduled actions beside the parameters line.
- Rebuild is disabled, with the reason, while the new code cannot build: Build's refusal for the selected model, and the carried setup's refusal or the device's shortfalls for another model.
  A run at tick 0 that cannot be rebuilt shows the modal instead of a rebuild.
- A run playing or running to a tick counts as past tick 0, whatever its last snapshot reads.
- Resume sweep is disabled for a folder of other runtime or plugin code, through a new public `Manifest::code_matches` in henad-explore.
  The plan names only the Results warning.
- `KeptDraft` keeps its entry, and Save spec and Edit sweep of a session follow it, which research-code-map.md's app-integration §1 lists as stale after a replace and the plan does not name.
- A folder read on its thread compares again with the current set when it lands.
- The tests sit in `src/tests/install.rs` with a shared `support.rs`, since the Sweep tab's test needs `EditedSir` as well, and lib.rs's inline tests moved to `src/tests/threads.rs`.
- Five tests beyond the plan's list: `a_choice_is_carried_by_the_name_of_its_option`, `an_install_during_a_run_to_asks_first`, `a_rebuild_the_new_code_refuses_leaves_the_old_run_alone`, `a_folder_store_compares_its_runs_with_a_replaced_entry` and `the_modal_names_what_the_rebuild_drops`.
- The device-lost notice of design.md §2 M5 is not here, as the plan cuts it.

### Questions for the maintainer

- Should Rebuild of the selected model build from the panel values, as Build does, or from the values the run had, as the rebuild of a model loaded while another one is selected does?
- Cancel comes before Rebuild, as in the Sweep tab's Abort and Replace results modals.
  Is that the order you want here?
- After Cancel, live edits stop until the next build.
  Should a code change that leaves the declarations alone keep live edits flowing to the old run?
- A run at tick 0 that the new code cannot rebuild, for a size the device cannot hold or an invalid Seed field, opens the modal with Rebuild disabled.
  Would the Reload needed banner alone suit you better there?

### Review findings

Two reviews read the first version of this milestone.
The session fixed each of these.

- A rebuild under new code that tore the old run down before it checked the device or the Seed field.
  The tick-0 rebuild, `rebuild_loaded` and the modal now go through `rebuild_refusal`, and Rebuild for a model loaded while another one is selected is disabled with its reason instead of closing the modal silently.
- Modal counts taken from the set entry an install replaced, so a Cancel then a kernel-only edit read no count while the rebuild still dropped the run's values.
  The counts now come from the loaded entry and the run's values and actions.
- A run to a tick whose first snapshot had not arrived, which read as tick 0 and was rebuilt without asking, losing its target.
  A pending run to a tick or a play now counts as past tick 0.
- Resume sweep enabled for a folder of runtime or plugin code that the app's entry no longer runs, which `ResumeError::CodeChanged` would end.
  The store records `code_matches`, and Resume is disabled with its reason.
- An install of the same source that still dropped the model's stat columns and the last check, so a GPU model was built again for its columns.
  It is skipped when its declarations match too, since a source without a hash can hide a change of declarations.
- Semicolons and trailing "which" clauses in new doc comments.
- `Notice` and `NoticeTone`, generic names, now `ParamsNotice` and `ParamsNoticeTone`.
- The CHANGELOG line's "(no caller yet)", a development note the release body would carry.
- Lines the AGENTS.md edits left short, reflowed to 100 columns.

No finding was rejected.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
