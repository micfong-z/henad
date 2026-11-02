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
(`menu_bar.rs`, `model.rs`, `params.rs`, `playback.rs`, `pacing.rs`, `viewport.rs`, `stats.rs`,
`charts.rs`, `performance.rs`, `system.rs`, `fault.rs`, `rebuild.rs`, `about.rs`). The Export tab
and the Editor live in directories, `export/` and `editor/`. The Export tab's `mod.rs` draws the
tab and writes the stat series and final state, `image.rs` captures the viewport and
`metadata.rs` builds the run details. `files/` holds the file dialogs: `save.rs` hands bytes to a
save dialog (several files go into one picked folder on native and download one by one in a
browser), and `open.rs` reads picked files, or takes a picked folder on native. Each outcome
carries a `SaveTarget` or `OpenTarget`, and `AppState::poll_saves` and `poll_opens` route it to
the panel that asked. `dock.rs` holds the tab definitions, the default layout and the dispatch to
each panel. Sweep and Results stack behind Viewport. `Tab::Editor` is the last variant, since
egui keys a tab's stored state on the discriminant, and sits after Parameters in `Tab::ALL`, the
View menu's order. The default layout leaves it closed, and `focus_tab` and `toggle_tab` open it
through `open_tab`, in a leaf of its own split to the right of the Viewport's leaf, with the
Viewport brought to the front of its stack. The menu bar's File menu holds New grid rule, Open
model and Save model.
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
offered model, or with none selected and `NO_MODEL_RUNS` in the Model panel. A missing id reads as
`lookup_message` words it, "This build does not include model 'x'" or "Model 'x' needs a GPU with
compute support, and this device has none", with no full stop. `select_model` loads the defaults
and clears the schedule, or, for the loaded model, restores `loaded_values` (its built values with
every live edit, kept by `send_live_param`), its seed and its schedule, carried by id to the set's
entry (`remap_values`, `remap_schedule`), for the Model panel and the Sweep tab alike, and an id
the set lacks leaves no values. `AppState` also holds the values the next build reads:
`param_values`, the Seed field's `seed` with its raw `seed_text`, and the scheduled actions in
`schedule`.
`AppState::build_setup` checks all three through `RunSetup::from_parts`. `AppState::build_refusal`
returns Build's disabled reason, which Playback shows. It shows a `SetupError` through
`setup_message`, as it shows `INVALID_SEED` and a shortfall through `too_large_message`.
`reset_simulation` builds nothing while the check fails, and `open_run` refuses a replay
`RunSetup::from_replay` refuses. `reset_simulation` builds the checked setup through the private
`load`, which tears the old run down, pins the setup's entry, records `loaded_values`,
`loaded_seed` and `loaded_schedule`, starts the build and sends `SimCommand::SetSchedule` when the
schedule is not empty. The loaded fields are the setup's from the start of the build.
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
holds, on the step's thread or on every worker of its pool. Keep them. A test that builds calls
`AppState::wait_for_build` before it reads the runner.
`seed_pending` and `schedule_pending` compare each with its loaded copy, and the Reload needed
notice counts both. A `None` seed is the model's default, the constant `henad-cli` uses without
`--seed`. The engines take `seed.map_or(CONST, mix_seed)`, and the field keeps `None` as a state
of its own, shown as the "Default" hint and never as a number. `params.rs` draws the Seed row
above the sliders, its dice (`draw_seed`, the clock mixed through `mix_seed`), and the
scheduled-actions list under the action buttons. `parse_seed` reads an empty field as `None` and
a decimal `u64` as `Some`. Anything else shows `INVALID_SEED` and disables Build. Picking another
model clears the schedule. Its entries index the previous model's actions. `playback.rs` draws
the Run to tick row. `AppState::run_to` rebuilds first for a tick behind the current one, then
sends `SimCommand::RunTo` and sets `run_to_target`. Run is disabled while another model is
selected, while the sim plays, and for a tick behind the current one while Build is disabled.
A playing loop runs ahead of the snapshot's tick, and a target between the two would pause past
it. The progress line reads the target, Cancel sends `Pause`, and `HenadApp::logic` clears the
target once a snapshot reaches it. A snapshot past the target makes `logic` call `run_to` again,
and the second call rebuilds. Where the frame drives the sim (`!CAN_SPAWN_THREADS`), `logic`
keeps repainting while a run-to is pending. `AppState::open_run(Replay, OpenAt)` builds one sweep
run from a `henad_core::explore::replay::Replay`. It selects the model, sets the params, seed
and schedule, rebuilds, sends `RunTo` for `OpenAt::Tick`, and sets `focus_request` to the
Viewport. `HenadApp::ui` applies that request through `dock::focus_tab` after
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
`schema_hash` changed to the replacement by id: `param_values` through `remap_values` (a choice
by its option's name, and a new parameter or a value the new descriptor rejects at its default), the
`pending_reload` marks, and the schedule through `remap_schedule`, which drops an entry whose
action is gone. A selected model whose hash matches keeps everything. `SweepPanel::forget_model`
drops the id's `ColumnsBuild`, its draft when the hash changed, and the last check, and
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
Rebuild model modal. Rebuild calls `AppState::rebuild_loaded`: Build for the selected model, or
the running values carried by id (`carried_setup`) for a model loaded while another one is
selected, which keeps the selection. `rebuild_refusal` is `build_refusal` for the selected model,
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
`equal_sources_with_other_declarations_are_other_code`, the Sweep tab's test
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
