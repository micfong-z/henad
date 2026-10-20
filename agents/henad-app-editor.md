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
fault modal. A lost device, a failed poll and a device out of memory stay with the modal, since
no edit of the code fixes them. `PendingBuild` keeps the entry it builds, and `finish_build`
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
page.
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
