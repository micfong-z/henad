# henad-app

The app's options, entry points, state, build and install path, export and About window. The
Editor tab is in `henad-app-editor.md`, and the Sweep and Results tabs in `henad-app-sweep.md`.

**henad-app**: eframe/egui desktop+web GUI, as a library with the official binary over it. The
public API is `options.rs` and the two entry points. `AppOptions::new(models, product, host)`
takes the `ModelSet`, the product name (the window title and, on native, eframe's storage folder,
through `storage_id` where the name is no folder name)
and the host's `BuildInfo`, with setters for the icon, the source and documentation links, the
licence, the `cli_command` and an `AppOpening` (`Results` on native, `Run` or `Setup`, each with
an `OpenAt`, which `lib.rs` re-exports from the private `state`). The product's fields sit in the
crate-private `Product`, held as `AppState::product`, whose `Debug` prints the icon's byte length.
`Product::official` marks Henad's own app, and only the official binary sets it, through the
hidden `AppOptions::__official`. `native.rs` holds `run_native`, which returns `AppError` (its
`Debug` writes its `Display` and source, as `ShaderBuildError`'s does), and `results_folder`,
which `--open DIR` and `--open=DIR` go through. `web.rs` holds `start_web`, which starts the
wasm-bindgen-rayon pool at the crate-private `requested_threads` before it calls the options
closure, then looks up `the_canvas_id` and `loading_text`, and `init_web_logger`. A pool that
fails to start leaves `thread_pool_note`, which the Performance tab shows. wasm-bindgen-rayon and
the `init_thread_pool` re-export exist only with `target_feature = "atomics"`. A wasm32 build
without it, as docs.rs makes, has no pool and leaves a note of its own. A start failure is returned, a missing window
included, and the caller logs it once. In a window, it is also written into `loading_text` where
the page has one. Both entry points check the opening against the set before any device exists
(`AppOptions::check_opening`, `OpeningError`): the set has to hold the id, a run has to pass
`RunSetup::from_replay`, and a setup's entry has to declare the same `schema_hash`. `AppError` is
native only and `WebStartError` wasm only. henad-app re-exports no eframe item. `state`, `ui`, `HenadApp` and
`wgpu_configuration` are private. `HenadApp` (`lib.rs`) owns the `SimThread` and polls snapshots each frame.
`HenadApp::new` takes the options and hands the opening to `AppState::open`, and
`wgpu_configuration` takes `AppOptions::gpu_needs`, the set's `GpuNeeds`, for the device request.
henad-models and the binary's env_logger are optional behind the default
`example-models` feature, which the `[[bin]]` requires, and `main.rs` builds the official options over
`example_models()`: "Henad", the icon, the links, the licence, `cli_command("henad-cli")` and `__official()`. The
eframe app name "Henad" names the native storage folder, which left 0.2.0's `Henad-Engine` behind,
and a browser keeps eframe's one key per origin. `ui/` has one file per panel or window
(`menu_bar.rs`, `model.rs`, `playback.rs`, `pacing.rs`, `viewport.rs`, `stats.rs`,
`charts.rs`, `performance.rs`, `system.rs`, `fault.rs`, `rebuild.rs`, `about.rs`), and
`time_axis.rs` holds the time axis the Charts tab, the Results tab's Series view and Playback
share. `focus.rs` holds the keyboard focus order: `without_focus_stop` takes the divider of each
resizable panel (`panel_divider`, egui's id of the panel with "__resize") out of the order of Tab
and Shift+Tab, as a divider has no name and no key moves it, and `install` adds `TabWraps`, an
egui plugin that `HenadApp::new` and the tests' `Harness` install. Left to egui, Tab from the last
widget leaves nothing focused in its frame, and the first widget takes the focus in the next frame.
The plugin runs a second pass of that frame (`request_discard`), in which the first widget takes
the focus. egui passes no input events to the second pass, so a key or a click counts once, and a
frame whose Tab does not wrap takes one pass (`focus::tests`, `every_tab_stop_has_a_name`). The
Parameters tab, the Export tab and the Editor live in directories, `params/`, `export/` and
`editor/`. The Parameters tab's `mod.rs` draws the panel, its action buttons and its notice,
`values.rs` the widget and text of one value (`value_widget`, `display_value`, `sheet_number`;
an `f32` marked integer slides in whole steps, `parameter_slider_step`). An `f32` slider snaps
through `snapped`: the steps start at the minimum, a maximum off them is a step of its own, and no
value leaves the range. egui snaps a typed number from the minimum before the slider's setter sees
it, so the slider's parser keeps the number as typed for the setter, and a typed minimum or maximum
stays as typed (`a_slider_keeps_its_ends_and_never_leaves_its_range`, the values `--set` and a
sweep accept). A drag or a key reaches the setter on egui's steps already, and `snapped` only holds
it inside the range, so a maximum less than half a step past the last step is reached by typing
alone. The parser reads text such as "NaN" or "inf" as no number, and the value stays as it was.
`--set` refuses such text too. `seed.rs` draws the Seed row, `schedule.rs` the scheduled actions
and `trial.rs` the preview mode. The Export
tab's `mod.rs` draws the tab and writes the stat series and final state, `image.rs` captures the
viewport and `metadata.rs` builds the run details. Save stats (`history_csv`) and Record
(`start_recording`, `Recording::start`) give the `StatsWriter` the tick length of `loaded_time`,
which adds the `time` column `--export-stats` writes
(`tests/time_axis.rs`, `the_export_tab_writes_the_time_of_a_timed_model`). A `Recording` keeps
the id of the model it recorded, and Save recording names the file after that id and `to_tick`,
whatever is loaded then (`a_recording_is_named_after_the_run_it_recorded`). `files/` holds the file
dialogs: `save.rs` hands
bytes to a save dialog (several files go into one picked folder on native and download one by one in
a browser), and `open.rs` reads picked files, or takes a picked folder on native. Each outcome
carries a `SaveTarget` or `OpenTarget`, and `AppState::poll_saves` and `poll_opens` route it to
the panel that asked. `dock.rs` holds the tab definitions, the two layouts and the dispatch to each
panel. `Tab::Editor` is the last variant, since egui keys a tab's stored state on the discriminant,
and sits after Parameters in `Tab::ALL`, the View menu's order. A `Layout` is `Default` or `Editor`,
and `Layout::dock_state` builds its tree. `default_dock_state` stacks Sweep and Results behind
Viewport and leaves the Editor closed. `editor_dock_state` splits the width at `EDITOR_LEAF_SHARE`
(0.7): the Editor in front of Sweep and Results on the left, and a column of the Viewport
(`VIEWPORT_LEAF_SHARE`, 0.46), Playback in front of Pacing and Model (`PLAYBACK_LEAF_SHARE`, 0.3 of
the rest) and Charts in front of Parameters and Statistics, with Performance, System and Export
closed. The column is 384 points wide in a window of 1280, and each stack's tab bar holds three
tabs, about 380 points. A fourth tab in a stack would be cut off at that width. `HenadApp` holds a
`Workspace`: one `DockState` per layout for the session (nothing stored, the app starts in the
default layout), the layout that shows and `editor_layout_on_open`,
the View menu's checkbox (`menu_bar::EDITOR_LAYOUT_ON_OPEN`, "Open models in the Editor layout", on
at start). `Workspace::toggle` and `focus` open a closed tab through `open_tab`: the Editor of the
default layout in a leaf of its own split to the right of the Viewport's leaf (`EDITOR_SPLIT`) with
the Viewport brought to the front of its stack, the Editor of the Editor layout in front of the leaf
of Sweep or Results (else in a leaf of its own on the left at `EDITOR_LEAF_SHARE`), and any other
tab into the focused leaf. `Workspace::focus` of the Editor switches to the Editor layout first
while the checkbox holds, and every site that opens a model in the Editor sets `focus_request` to
the Editor. Nothing switches back. The View menu (`menu_bar::view_menu`) lists the tabs of the
layout that shows, then "Default layout" and "Editor layout" as radio buttons, the checkbox and
Reset layout (`Workspace::reset`, the layout that shows alone). Both layouts draw under one
`DockArea` id, so a tab body keeps its egui id across a switch, and `HenadApp::ui` copies the layout
into `AppState::layout` before the dock draws for the Sheet pane's stored width
(`henad-app-sheet.md`). `dock.rs` holds the eight tests of the default layout, their Editor layout
counterparts and the `Workspace` tests (`a_switch_keeps_the_changes_of_each_layout`,
`a_request_for_the_editor_switches_layouts_only_while_the_checkbox_is_on`,
`reset_layout_resets_the_layout_that_shows_alone`), and `menu_bar.rs`
`the_view_menu_switches_layouts_by_name`. The menu bar's File menu holds New model, New grid rule,
New node graph, Open model and Save model. `lib.rs` draws the New model, unsaved changes and replace
dialogs beside the fault and Rebuild model modals.
**Unsaved edits** of the Editor (`ui/editor/mod.rs`). Every load sets `saved_mark` to the model's
`edit_mark`, a save that reports sets it to the mark `saving` took at its start, and Apply leaves
it. `is_unsaved` holds for a non-empty model whose mark differs, and `Counterpart` keeps its own
marks, swapped with the model. Each save takes a number (`next_save`) in its `SaveTarget`
(`Model(n)` for the model shown, `Counterpart(n)` for the model beside it, as `SweepResults(n)`
carries a generation), and `saving` holds one `PendingSave` per save of the model, which move with
it in a swap and go with a replaced model. `save_finished` gives the file name and the save's edit
mark to the model, shown or beside, that holds the reporting save, and to no model once that model
is gone (`a_late_save_names_the_model_it_saved`). Two saves of one model, or of a model and the
model that replaced it, each name the model they saved, in the order they report
(`each_save_names_and_marks_the_model_it_saved`, which reports a chosen save through the test-only
`finish_save_of`). New model, New grid rule,
New node graph and Open model (header, File
menu, Model tab), Edit model of code the Editor did not apply, and Open in Editor go through
`replace_model(app, Replacement)`, which runs the action at once when `unsaved_models` of the
replacement is empty and otherwise sets `EditorPanel::ask`. Convert in the conversion dialog is
`Replacement::Convert`, which keeps the sheet shown and drops the copy beside it, so its dialog
names and saves the counterpart alone (`a_second_conversion_asks_before_it_drops_an_edited_copy`).
`unsaved_modal` (drawn from `lib.rs`) is a `Dialog` named by `sentence::unsaved_changes(names)`,
titled `UNSAVED_HEADING`, with Cancel, `DISCARD_CHANGES` and `SAVE_MODEL` (the primary, which Enter
presses). Save model saves the models `unsaved_models` names (`HeldModel::Shown`, then
`HeldModel::Beside`) in turn (`AfterSave`, `start_next_save`): `poll_saves` passes each outcome to
`save_finished` and `continue_replacement`, which waits for the numbered save it started, runs the
action through `replace_model` once both saved, so an edit made while a save dialog was open asks
again, and drops it on a save that saved nothing. A model replaced while the action waits takes the
action with it (`replace` clears `after_save`), so a later report of its save runs nothing
(`a_replacement_waiting_for_a_save_goes_with_its_model`). `open_model` records the models' edit
marks (`note_open_started`, `EditorPanel::open_marks`), and `receive_open` opens the picked file at
once unless a model was edited since, when `Replacement::OpenFile` (an `OpenedFile`, decoded first)
asks (`edits_made_while_a_file_dialog_is_open_ask_before_they_are_replaced`). A file dialog leaves
the app's window usable on Windows and Linux. In a test build `save_as` records each save in
`saves_requested`, and `finish_save` reports it (`src/tests/unsaved.rs`). On native a close request
of the window waits the same way: `HenadApp::logic` calls `native::guard_close`, which sends
`ViewportCommand::CancelClose` while `AppState::hold_close` holds, which asks through
`replace_model(app, Replacement::CloseWindow)`. Discard changes, or a Save model that saved both
models, runs `AppState::close_window`: `closing` set, which holds no later request, then
`ViewportCommand::Close` (`a_close_request_with_unsaved_edits_asks_and_saves_before_closing`,
`a_close_request_closes_at_once_without_edits_and_after_discard_with_them`). egui's default
`Options::quit_shortcuts` turns Cmd+Q, Ctrl+Q off macOS, into the same close request
(`the_quit_shortcut_requests_a_close_that_unsaved_edits_hold`). On macOS winit's default menu binds
Cmd+Q to a Quit item that sends `terminate:`, which ends the app at once, since winit's delegate
has no `applicationShouldTerminate:`. `run_native`'s app creator therefore runs
`remove_menu_quit` first, which removes the item whose action is `terminate:` and the separator
before it through objc2-app-kit's safe calls (`objc2` and `objc2-app-kit` are macOS-only
dependencies, versions already in the tree). Cmd+Q then reaches egui as a key press. No test can
run it: it needs `NSApplication` on the main thread. Quit in the Dock, a logout and an Apple Event
quit still send `terminate:` and end the app without the dialog. Vetoing them needs a declared
Objective-C method, which `unsafe_code = "deny"` rules out. A browser tab cannot
hold a close. `HenadApp::logic` mirrors whether `unsaved_names` is empty into `web.rs`'s
`UNSAVED_EDITS` each frame, and the `beforeunload` listener `start_web` adds once the app started
calls `preventDefault` and sets `returnValue` while it holds, so the browser asks before the page
closes or reloads. Only the wasm32 typecheck compiles it, and no test runs it.
`agent_layer.rs` (with `agents.wgsl`) is the instanced agent renderer drawn over the grid layer,
and `painted.rs` carries a paint callback's wgpu handles past `CallbackTrait`'s `Send + Sync`
bound. `edge_layer.rs` (with `edges.wgsl`) draws a network's
edges under the nodes. It reads both ends from the agent layer's position buffers, bound as
storage, and uploads the edge list only when its version or length changes. That binding
needs `DownlevelFlags::VERTEX_STORAGE`, and WebGL2 lacks it. Without that flag there is no edge
layer, and a network model runs with its edges undrawn. `world.wgsl` holds the helpers both
shaders `#import`, including `is_placed`, the finiteness test that hides a retired node.
`state.rs` (`AppState`) holds the set as `models` and keys the selection by id: `selected_model`
is an `Option<String>`, resolved at use through `selected_entry` and `lookup`. The live
simulation's entry is pinned: the private `loaded` holds the `ModelEntry` of the `RunSetup` it was
built from, `loaded_entry()` returns it and `loaded_id()` its id. The panels, pacing, the run
details, a fault's model name and live edits then describe the running code after the set's entry
under its id is replaced. `selection_is_loaded` compares the id and the code through `same_code`
(the `ModelSource` and the `schema_hash`), and `code_pending` holds for the same id under other
code. `offered_models` hides a GPU model
where the adapter has no compute, through `ModelSet::runnable`, and the app opens on the first
offered model, or with none selected and `NO_MODEL_RUNS` in the Model panel. The Model panel's list
(`model.rs`) shows each offered model's name, and its id in parentheses after the name when another
offered model has the same name, letter case aside (`list_labels`, `label_among`), as an applied
sheet starter has beside the example model it ports
(`the_model_list_names_the_ids_of_models_that_share_a_name`). The Identity section's Exact replay
row reads `ModelMetadata::replays_exactly` (`replay_cells`,
`the_replay_row_marks_a_model_that_does_not_replay_exactly`). `model::model_label` returns that
label for any id and name, and the Sweep tab's header, its Plan's Model row, a session's
`model_name` (taken at its start) and the Results tab's summary line show it. A missing id reads as
`lookup_message` words it, "This build does not include model 'x'" or "Model 'x' needs a GPU with
compute support, unavailable on this device", with no full stop. `select_model` loads the defaults
and clears the schedule, or, for the loaded model, restores `loaded_values` (its built values with
every live edit, kept by `send_live_param`), its seed and its schedule, carried by id to the set's
entry (`remap_values`, `remap_schedule`), for the Model panel and the Sweep tab alike, and an id
the set lacks leaves no values. `AppState` also holds the values the next build reads:
`param_values` with its `pending_reload` marks (`set_param_values`), the Seed field's `seed` with
its raw `seed_text` (`set_seed`), and the scheduled actions in `schedule`.
`AppState::build_setup` checks all three through `RunSetup::from_parts`. `AppState::build_refusal`
returns Build's disabled reason, which Playback shows. It shows a `SetupError` through
`setup_message`, as it shows `INVALID_SEED` and a shortfall through `selection_too_large`. Every
shortfall goes through `AppState::entry_shortfalls`, which takes `SheetFile::shortfalls` when the
Editor's sheet has the entry's code (`editor_sheet_of`), so a sheet's work past a budget names its
lines and no pass (`work_past_the_budget_on_a_sheet_names_its_lines_and_no_pass`).
`reset_simulation` builds nothing while the check fails or the Seed field holds no seed, and
`open_run` refuses a replay
`RunSetup::from_replay` refuses. `reset_simulation` builds the checked setup through the private
`load`, which tears the old run down, pins the setup's entry, records `loaded_values`,
`loaded_seed` and `loaded_schedule`, starts the build and sends `SimCommand::SetSchedule` when the
schedule is not empty. The loaded fields are the setup's from the start of the build.
`load_preview` builds the Editor's preview through the same `load_setup` with a `PreviewRun`
(`AppState::preview`), which leaves the next build's fields alone and plays the run when its
`plays` flag asks (a graph's preview, never a sheet's), and `builds` counts the other builds
(`henad-app-editor.md`). `runs_editor_sheet` holds while the run is the Editor's sheet, its preview
(`previews_editor_sheet`, the loaded id the sheet's) or the source the Editor applied, and Playback
then reads a tick in the sheet's words. Build replays the preview while the selection is the one
`load_preview` recorded in `preview_selection` (`build_replays_preview`, `henad-app-sheet-open.md`).
`state.rs` holds `AppState`, its selection, the next build's fields, the loaded run and the
snapshot intake. Beside it `state/preview.rs` holds the Editor's previews, `state/open.rs` the
openings and the opened run, `state/fault.rs` the fault routes and `state/files.rs` the save and
open dialogs, the recording and the viewport capture.
`state/build.rs` holds the build and the teardown, and `state/pool.rs` the `ModelPool`, a shared
slot holding the pool the app's model code runs in (`None` for rayon's global pool) and its
generation, and the `UiPool`. On native `start_build` runs a `BuildJob` on a thread named
`henad-model-build`. The job first waits (`settle`) until each detached thread in `stopping` has
ended, or has run past `DEFAULT_STOP_TIMEOUT` since its stop and retired its pool through
`ModelPool::retire`, which puts a fresh pool at the global width in place, its threads named
`henad-pool-N`. The old run then frees its memory before the new model allocates. The job builds
inside the current pool: `entry.build`, then `SimThread::new_in` or `GpuSimThread::new_in` (`new`
in the global pool). A GPU build runs whole inside `catching_on` on the thread that builds it,
since error scopes are thread-local. The thread sends the runner or the fault back over a channel
with the pool's generation and wakes the UI, and `HenadApp::logic` calls `poll_build` each frame,
which installs the runner, applies the Pacing tab's GPU batch settings and sends it the
`PendingBuild`'s queue.
Every command goes through `AppState::send_command`, which queues it while a build runs: the
`SetSchedule` of `load`, the `RunTo` of `run_to`, `open_run` and `open_setup`, live edits, action
presses and pacing. `has_simulation` holds for a runner or a build, `is_building` for a build, and
`is_gpu` reads the loaded entry during a build. Playback shows `BUILDING_MODEL` with Cancel in
place of the Run to tick row and disables Build. Cancel is `offload_simulation`. A build started
while one runs abandons it. A browser builds inline through the same `finish_build`. Every
teardown (`load`, `offload_simulation`, a fault, and the app's drop as it quits) goes through
`stop_simulation`, which stops the runner with `STOP_WAIT` (250 ms) and abandons a build. A
build's `phase` moves from waiting to building to done by compare and swap, and an abandon swaps
in cancelled: a waiting build ends without building, a building one is detached and drops its
runner on its own thread, and a done one hands its runner over to be stopped. A runner still
running and an abandoned build are detached into `stopping` with their pool's generation, and
`poll_stopping` drops each once its thread ends and retires the pool of each past
`DEFAULT_STOP_TIMEOUT`. Playback reads `stopping_status`: `STILL_STOPPING` ("Previous run is still
stopping.") and, once one has run past `DEFAULT_STOP_TIMEOUT` since the stop, `DID_NOT_STOP`
("Previous run did not stop. It will keep running until the app quits."). The viewport's uploads
(the grid, the density map, the agent and edge layers) and the export's grid pixels run through
`AppState::ui_pool`, a `UiPool` that no model code runs in, built on first use with threads named
`henad-ui-N` (the calling thread in a browser). A run that holds every worker of its pool then
never stalls a frame, stopped or not. The Sweep tab's column builds run through
`ModelPool::install`, and `SweepSession::start` and `resume_folder` pass the current pool as the
sweep's `build_pool`, so a stuck run in a retired pool never stalls them. After `stop_simulation`,
the app's drop stops a Sweep tab session through `SweepSession::stop` with `DEFAULT_STOP_TIMEOUT`,
and a sweep thread held in a step is detached. A CPU or GPU runner reports a panic into
`AppState::sim_faults`, which each teardown replaces, so a detached runner's late panic reaches no
later run. `src/tests/runaway.rs` holds
`a_cpu_build_after_a_detach_runs_on_a_fresh_pool`,
`a_slow_runner_is_waited_for_and_keeps_the_pool`,
`a_viewport_frame_after_a_detach_runs_on_the_fresh_pool`,
`a_viewport_frame_beside_a_run_that_holds_its_pool_returns`,
`a_build_that_never_returns_leaves_the_ui_responsive`,
`commands_sent_during_a_build_reach_the_new_runner`,
`offload_and_quit_do_not_wait_for_a_runaway_runner`,
`a_gpu_build_on_the_build_thread_reports_a_validation_error_as_a_fault`,
`a_sweep_after_a_detach_probes_in_the_fresh_pool` and
`quitting_does_not_wait_for_a_sweep_held_in_its_model`, over the `held` entries
of `support.rs`, whose first build waits on a `Gate` and whose first run's steps a `StepHold`
holds, on the step's thread or on every worker of its pool. The two viewport tests draw their frame
under a `Watchdog` that opens the gate after `WATCHDOG`, so a frame stuck on the held pool fails
instead of hanging. Keep them. A test that builds calls `AppState::wait_for_build` before it reads
the runner, and `support::wait_until` polls any other state a thread reaches.
**The Parameters tab's preview mode**. `params_ui` draws `preview_params_ui` instead
of its rows while `AppState::params_drive_preview` holds: a preview runs or builds and the selection
is still the one `load_preview` recorded. The selected model's `param_values`, seed and schedule
stay as they are, and once another model is selected the tab draws that model for the next build.
The mode draws the loaded entry's parameters but the hidden ones and `preview::SIZE_PARAMS` (the
sizes in front of a graph's own), each slider at its tried value or `loaded_values`, through the
shared `value_widget`, then the entry's actions (`action_buttons`, enabled while a run exists,
pressed into the preview) and no Seed row, schedule or notice. While the Editor holds the
previewed sheet, each action whose id is a sheet button's takes that button's words
(`button_descriptions` over `sheet_pane::button_does`, "Run once, every cell") as its tooltip and
AccessKit description
(`a_sheet_button_in_the_world_is_described_by_what_it_does`). A move is a
`TrialChoice::Try` for `apply_trial`: `Preview::try_value` records a `TriedValue` (id, value and
the value the model keeps, `preview::model_value`: the sheet parameter's value or the graph's
default, typed as the descriptor) and drops it when the two are equal. A live parameter then goes
to the running preview through `AppState::send_preview_param` (`SetParam`, `loaded_values`
updated, no rebuild, the tick going on), and a reload-only one calls `Preview::rebuild_after`,
which rebuilds the preview at tick 0 after `DEBOUNCE` with its key kept, carrying every tried value
(`with_tried` in `build_preview`, which Build's replay of a sheet's preview takes too). A
reload-only slider's tooltip is `RESTART_NOTICE`. Under a tried value `trial_row` shows
`trying_value` ("Trying this value. The model keeps 0.3.") with Reset (named `reset_name`) and,
while the Editor holds the previewed document (`holds_previewed_document`), Keep (named
`keep_name`, `KEEP_TOOLTIP`). Reset ends the trial and sends or rebuilds at the kept value. Keep
makes the tried value the document's: `SheetEditor::keep_value` (one `SheetEdit::SetParameter`,
range widened, the preview's key kept) or one `EditCommand::SetParam` on a graph, which rebuilds
its preview at that value. `preview::poll_at` ends a trial whose kept value changed, as a sheet
edit of the value does, and the Editor's loads reset the `Preview` and its trials. The Sheet pane
shows each trial under its parameter (`henad-app-sheet.md`). `src/tests/preview_params.rs` holds
`a_params_tab_change_rebuilds_the_preview_with_the_value_and_keeps_it_across_an_edit`,
`a_live_parameter_reaches_the_running_preview_and_the_tick_keeps_counting`,
`the_preview_mode_leaves_the_selected_model_alone`, `reset_and_keep_end_a_trial`,
`keep_widens_the_range_to_hold_the_value` and
`a_tried_value_shows_its_note_reset_keep_and_the_sheets_line`.
`seed_pending` and `schedule_pending` compare each with its loaded copy, and the Reload needed
notice counts both. A `None` seed is the model's default, the constant `henad-cli` uses without
`--seed`. The engines take `seed.map_or(CONST, mix_seed)`, and the field keeps `None` as a state
of its own, shown as the "Default" hint and never as a number. `params/` draws the Seed row
above the sliders, its dice (`draw_seed`, the clock mixed through `mix_seed`), and the
scheduled-actions list under the action buttons. `parse_seed` reads an empty field as `None` and
a decimal `u64` as `Some`. Anything else shows `INVALID_SEED` and disables Build. Picking another
model clears the schedule. Its entries index the previous model's actions. `params_ui` skips a
hidden parameter (`ParamDescriptor::is_hidden`, a model's tick length), and shows "This model has
no parameters." when every parameter is hidden. The graph editor's world panel skips it too and
counts only the shown ones, as the Model tab's Parameters row (`model.rs`, `interface_rows`) does
(`the_model_tab_counts_the_parameters_the_parameters_tab_shows`). The Sweep tab keeps a row per
parameter, hidden ones included, so a
sweep varies a tick length (`a_hidden_tick_length_stays_a_sweep_factor`). The time axis:
`AppState::loaded_time` returns a `time_axis::RunTime`, the unit of the loaded entry's
`ModelMetadata::time` and the tick length among `loaded_values`, `None` for a model with no time
scale. `time_axis::counter_text` is Playback's tick counter beside Play and Step ("Tick 24", or
"Tick 24 (12 days)", singular at exactly one unit), `axis_label` the x label ("Tick", or the unit
capitalised, "Day"), and `plot_x` a sample's x, `TimeScale::time_at` of its tick, which equals
the `time` column of `--export-stats`. The Charts tab plots every stat at `plot_x`, includes 0 in
the y range, and draws a series of one sample as `Points` of radius 3, since a `Line` of one point
draws nothing. Its x range then spans the sample's tick to the next (`single_sample_x_range`).
Otherwise egui_plot widens the zero-width range to the height of the y range. `tests/time_axis.rs` holds the app-level tests over henad-models'
`sir_rates.toml` fixture (`support::sir_rates`). Above the plot, the Charts tab's History length
slider draws its value box apart, `DragValue` beside a `Slider` with no value, since egui's slider
names its box only through `labelled_by` and keeps the box's id inside. Each takes the name
`HISTORY_LENGTH` (`the_history_length_slider_and_its_box_have_names`). `playback.rs` draws the Run
to tick row, its box named `RUN_TO_TICK` as well as labelled by it
(`the_run_to_tick_box_has_a_name`).
`AppState::run_to` rebuilds first for a tick behind the current one through `press_build` (a
replay of the Editor's sheet preview while `build_replays_preview` holds), then sends
`SimCommand::RunTo` and sets `run_to_target`. Run (`run_refusal`) is disabled while another model
is selected than the one loaded, unless Build replays the sheet's preview, which Run then drives
(`run_to_tick_drives_the_sheet_preview`), while the sim plays, and for a tick behind the current one
while Build is disabled. Behind a graph's preview with the selection it loaded under
(`params_drive_preview`), its reason is `preview_hover`, which names Apply, since Build would build
that selection (`run_to_tick_behind_a_graph_preview_names_apply`).
`AppState::hold_for_modal`, which `HenadApp::logic` calls each frame with `ui::modal_shown`,
whether egui's `top_modal_layer` (the last frame's) is set
(`a_dialog_drawn_in_a_frame_holds_the_run_from_the_next`), pauses a run that plays or runs to a
tick while a modal dialog is open and records it in `held_by_modal` (`HeldRun`), then plays or
runs to the tick again once no dialog is open, asking for a repaint each frame while it holds a
run, since a key that closes a dialog asks for no later frame. Play, Step, Pause, Run (`run_to`)
and `clear_run` end the hold, so a sweep's start, which pauses the run, and a build leave the run
as they set it. Sweeps are never held (`a_modal_dialog_holds_the_run_until_it_closes`).
A playing loop runs ahead of the snapshot's tick, and a target between the two would pause past
it. The progress line reads the target, Cancel sends `Pause`, and `AppState::receive_snapshot`
(each frame's snapshot, through `poll_snapshot` from `HenadApp::logic`) clears the target once a
snapshot reaches it. A snapshot past the target makes it call `run_to` again, and the second call
rebuilds, for the selection or the replayed preview. A rebuild that Build refuses, as for a Seed
field that holds no seed, leaves the snapshot, and `run_to` then sends nothing
(`a_run_to_a_tick_behind_that_build_refuses_leaves_the_run_alone`). Where the frame drives the sim
(`!CAN_SPAWN_THREADS`), `logic` keeps repainting while a run-to is pending.
`AppState::open_run(Replay, OpenAt)` builds one sweep run from a
`henad_core::explore::replay::Replay`. It selects the model, sets the params, seed
and schedule, rebuilds, sends `RunTo` for `OpenAt::Tick`, and sets `focus_request` to the
Viewport. `HenadApp::ui` applies that request through `Workspace::focus` after
`DockArea::show_inside`. The panels draw while the dock is borrowed. `OpenedRun` names the run in
Playback. A live param edit, an action press, or a build from other values or of other code
(`same_code`) than the `OpenedRun::entry` it was opened with marks it modified
(`settle_opened_run`), and a build of another model or Offload drops it.
`AppState::open_setup(&RunSetup, OpenAt)` builds the set's entry under the setup's id from its
values, seed and schedule through the same fields, keeps a default seed as `None`, and records no
`OpenedRun`. `AppState::open` takes an `AppOpening`. A run or setup it cannot open (a GPU model
the compute filter hid, say) leaves nothing selected, keeps an `OpeningRefusal` (a lead line such
as "Run not opened" and the reason) in `opening_refusal`, which the Model panel shows with "Select
a model to continue." until a model is picked, or above "No model in this build runs on this
device." when none runs here, and brings the Model tab to the front.
`state/install.rs` is the install path. `AppState::install_models(&ModelSet)` puts each entry in
place of the set's entry with its id through `ModelSet::replace`, or appends it, so a runtime entry
can shadow a built-in id. A replacement of the same code (`same_code`: an equal `ModelSource` and
`schema_hash`) changes nothing else. For each other replaced id it carries a selected model whose
`schema_hash` changed to the replacement by id: `param_values` through `remap_values` (a value
at the previous default at the new default, as after an edit of a sheet's value, any other value
carried, a choice by its option's name, and a new parameter or a value the new descriptor rejects
at its default, which alone count in `defaulted`), the
`pending_reload` marks, and the schedule through `remap_schedule`, which drops an entry whose
action is gone. A selected model whose hash matches keeps everything. `SweepPanel::forget_model`
drops the id's `ColumnsBuild` and the last check, carries its draft by id when the hash changed
(`SweepDraft::carry_to`, `henad-app-sweep.md`), and
`ResultsPanel::model_replaced` has the store recompute `schema_matches`, `changed_builds`,
`replays_exactly` and `code_matches` against the replacement (`ResultsStore::model_changed`). A
store of files (`RecordedSpec`) plans its recorded spec again against a replacement whose
declarations differ from those of its plan, or without an entry, and takes the replacement's
parameters and action labels. An id that joins the set reaches the store as well. The store keeps
a `RecordedModel` for it: the recorded builds, schema hash, replay flag and manifest of a folder,
or the builds of the entry an in-app sweep ran, and a folder read on its thread compares again
with the current set when it lands. A loaded model replaced by another source
rebuilds at once while `run_at_start` holds (no snapshot past tick 0, not playing, no run to a
tick pending) and `AppState::rebuild_refusal` returns no reason. Otherwise it sets
`pending_rebuild` (`PendingRebuild`, with the run's values the new code does not take and the
run's actions it drops, counted against the loaded entry), which `ui/rebuild.rs` shows as the
Rebuild model modal, a dialog whose Rebuild is destructive and takes Enter. Rebuild calls
`AppState::rebuild_loaded`: Build for the selected model, or the running values carried by id
(`carried_setup`) for a model loaded while another one is selected, which keeps the selection.
`rebuild_refusal` is `build_refusal` for the selected model,
and the carried setup's `SetupError` or device shortfall otherwise. It disables Rebuild with its
reason, and `rebuild_loaded` checks it before any teardown. Cancel keeps the old run under its
pinned entry, and the Parameters banner (`notice_content`, a `ParamsNotice`) reads "Model code
changed. Press Build to apply.", and Run's disabled reason names the change too. `KeptDraft` keeps
the entry its session started with, a session's Save spec writes against it, and Edit sweep
restores the kept values only while the declarations match. The Editor's Apply is its caller in
the app.
`src/tests/install.rs` holds `the_panels_describe_the_running_entry`,
`an_opened_run_is_modified_after_its_source_changes`,
`install_keeps_values_when_the_schema_matches`,
`install_remaps_values_by_id_when_the_schema_changes`, `results_flag_runs_of_replaced_code`,
`install_works_for_every_model_type`, `a_loaded_run_past_tick_zero_asks_before_rebuilding`,
`cancel_keeps_the_old_run_and_flags_the_code_change`, `an_install_during_a_run_to_asks_first`,
`a_rebuild_the_new_code_refuses_leaves_the_old_run_alone` and
`equal_sources_with_other_declarations_are_other_code`,
`a_value_at_the_previous_default_takes_the_new_default` and
`a_sheet_value_changed_after_apply_runs_at_the_next_apply`, the Sweep tab's test
`install_forgets_the_sweep_tab_columns_of_the_model`, and the store's
`a_folder_store_compares_its_runs_with_a_replaced_entry`. Keep them.
`export/metadata.rs` writes `loaded_values` as `params`, the loaded `seed` (null for the default),
`scheduled_actions`, the `host` build from `AppState::product` and the `model_source` build, and
reads `engine_version` from `ENGINE_BUILD`. A sweep's manifest records the product's host build as
well. `about.rs` draws the product's name, its links and its build rows, Henad's logo and tagline
for the official app and the host's icon otherwise (decoded once into the `OnceCell`
`logo_texture`, a failed decode included), a Built on row from `ENGINE_BUILD` (with henad-core's
version when it differs) for any other product, and one Models row per distinct `ModelSource`
build, showing the hash of its sources when the build stamps no commit, and the code hash of a
runtime model under its id. Copy copies the name and every row. The menu entry reads "About" and
the product name.
The runs table's Copy command builds `ResultsStore::cli_command(program, run_id)` from the
product's `cli_command` and is hidden without one, and disabled for the runs of a model file
(`ResultsStore::command_refusal`, `MODEL_FILE_COMMAND`), which need `--model-file`, and the Sweep
tab's advice names it through `options::cli_phrase`, or "on the command line" without one.
