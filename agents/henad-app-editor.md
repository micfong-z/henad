# henad-app: the Editor tab

`ui/editor/` is the Editor tab, the in-app editor of a grid rule's model file, a test harness
that #37's node editor replaces. `mod.rs` holds `EditorPanel` (`AppState::editor`: the text as
typed, the file name, the `ApplyOutcome` of the last Apply, the open and save status, and the
`ModelSource` applied last) and `editor_ui`: a header of New grid rule, Open model, Save model and
Apply, the code, and a list of the outcome and each `CodeDiagnostic` as a selectable `Label`
reading `<line>:<column> <message>`, whose press moves the cursor there (`move_cursor_to`).
`code_view.rs` draws one `TextEdit::multiline(..).code_editor()` with a layouter that never wraps,
in one `ScrollArea::both`, beside a gutter outside it. The gutter's numbers come from the galley's
rows (`gutter_numbers`) and follow them up and down, and stay in place while a long line scrolls
sideways. A text replaced from outside the field clears its undo history on the next frame
(`ViewReset`, `clear_history`), since egui keeps one history under the field's id, and another
file's text also returns the view to the top left. No syntax highlighting, and the check runs on
Apply alone. `editor::apply` reads the
text through `ModelFile::parse` and `entry`, which leave the set and the run alone on a refusal,
then installs a one-entry set through `install_models` and selects the model. With no run, or a
run at tick 0 (`run_at_start`, also true during a build unless a run to a tick waits for it), it
starts a build as Build does, on the build thread on native, and a build of another model is
abandoned for it. A run of the same id past tick 0 waits on the Rebuild model modal
(`ApplyOutcome::AwaitingRebuild`, "Model applied. Press Rebuild to run it from tick 0."), and
Apply calls `settle_loaded` itself when `install_models` passes over an entry the set already
holds while the run differs, so the modal opens again after Cancel. A run of another model past
tick 0 keeps going, as does the run after Cancel (`ApplyOutcome::Installed`, "Model applied.
Press Build to run it."). `EditorPanel::settle_outcome` moves between the two, to `Applied` once
the loaded model runs the applied source, and from `Applied` to `Installed` once nothing is
loaded, as after Cancel of a build or Offload. `load` commits the loaded entry when the build
starts, and the outcome reads `Applied` from then on. New grid rule
loads `life_starter()`, the canonical text of `GridRule::life()`, and Edit model, shown for a
runtime entry whose code is in `GridRule::CODE_FORMAT` (`editable_text`), its canonical text.
Open model and Save model go through `OpenTarget::Model` and `SaveTarget::Model`, a `.toml` file
on native and a picked file or a download in a browser. Opening loads the text and applies
nothing. `AppState::show_fault` gives a fault to the Editor (`EditorPanel::claims`) when it
belongs to a run built from the applied source that has not passed tick 0, in place of the
fault modal. A project file in the buffer gives way to the model file text kept from before it
opened (`project::show_model_file`), and without that text the fault goes to the modal. A lost
device, a failed poll and a device out of memory stay with the modal, since no edit of the code
fixes them. `PendingBuild` keeps the entry it builds, and `finish_build`
passes it to `show_fault`, so a fault the build thread returns reaches the Editor on the frame
`poll_build` takes it. `show_fault_in_modal` passes the Editor over: a build thread that did not
start or ended without a result, and on native a fault `report_fault` takes from a sink while a
build runs. That fault comes from a detached run or from rendering. Apply returns before
`poll_build`'s frame, and ends early only when the Editor claims a fault during the call, as it
can for a browser's inline build. In a browser `catching_on` pushes no scopes, and the device
reports a refused rule through the `FaultSink` a frame or more after the build. Without compute,
Apply is disabled and the tab reads `NO_COMPUTE`. Apply builds no model the device cannot hold at
the next build's values, and reports the sizes as `ApplyOutcome::Faulted`. `reset_simulation`,
`open_run` and `open_setup` refuse it too, since the engine's own assert would end a browser's
page. An Open model that fails beside a project file writes its status in the project's status.
`src/tests/editor.rs` holds
`new_grid_rule_starts_from_the_life_starter`,
`applying_an_edit_installs_and_rebuilds_from_tick_zero`,
`applying_beside_another_run_past_tick_zero_waits_for_build`,
`applying_again_after_cancel_asks_to_rebuild_again`,
`applying_during_another_build_replaces_it_and_cancel_waits_for_build`,
`a_code_edit_that_fails_validation_keeps_the_running_model`,
`a_fault_in_the_apply_window_is_shown_in_the_editor`,
`a_build_fault_of_the_applied_code_reaches_the_editor_from_the_build_thread`,
`a_diagnostic_moves_the_cursor_to_its_line` and `edit_model_shows_the_code_the_rule_runs`. Keep
them.
`ui/editor/project.rs`, behind `hot`, is the Editor's project mode in a dev host. A left panel
names the package, holds Reload models and the dev host's status line, and lists the `.rs` and
`.wgsl` files under the dev host's source directories (`list_files`: labels relative to the
directory that holds each source directory's parent, names starting with `.` left out, listed
again after each reload). With several source directories the list groups the files under each
(`file_groups`), each named from its directory, with its label as the tooltip. A press opens a
file into the same code buffer (`open_file`). An edited project file keeps its text in
`ProjectEditor::unsaved`, with the disk text it started from, while another file is open, a model
file
the buffer held becomes the list's first entry, and `EditorPanel::load` keeps an open file's edit
before New grid rule, Open model or Edit model replace the text. Save writes every edited file,
and Apply writes them, then calls `AppState::reload_models`. `problems` places a failed build's
errors: rustc's `compiler-message` errors by their primary span, a relative path matched to a
listed file under a directory that holds the manifest's and an absolute one by its path, and
without any the shader compiler's `┌─ path:line:column` lines on standard error, then any line
that starts with `error`. The list reads `<line>:<column> <message>` in the open file and puts the
file's label first for another, and a press opens the file at the place. `check_disk` reads the
open file again (`read_changed_file`), at most once a second, when its modification time changes,
and takes the new text into a buffer without edits. A Save or Apply that finds a file changed on
disk since its edit started, open or not, writes nothing, says so, and takes the disk text as the
edit's start, so the next press writes it. Opening a kept edit of a changed file says the same.
`src/tests/project.rs` holds
`a_plugin_set_replaces_every_id_it_holds`, `a_model_removed_from_the_crate_leaves_the_set`,
`a_rule_the_editor_applied_stays_when_the_crate_drops_its_id`, `apply_saves_before_it_builds`,
`a_save_that_fails_starts_no_build`, `a_file_changed_on_disk_is_read_again_unless_edited`,
`a_kept_edit_of_a_file_changed_on_disk_is_not_written_unasked`,
`a_warm_up_that_fails_to_compile_lists_its_problems`,
`a_library_compiled_with_no_model_changed_says_so`,
`a_build_overtaken_by_a_reload_passes_its_compile_on`,
`an_apply_during_a_build_queues_one_more_build`, `a_build_error_keeps_the_running_model`,
`a_crate_without_a_source_hash_is_refused_with_its_advice` and
`a_plugin_step_that_never_returns_leaves_the_app_responsive` (a reload past a held plugin step
returns within `STOP_WAIT`, and the rebuild steps on a fresh pool), over a scripted builder that
reports a library compiled again when its file's text changed, and
`project.rs` holds `the_file_list_holds_the_sources_by_label`,
`a_compiler_error_is_placed_in_its_project_file` and `a_shader_error_on_standard_error_is_placed`.
Keep them.
