# henad-app: the Editor tab

`ui/editor/` is the Editor tab, the in-app editor of one model file: a grid rule as text in the
code view, or a graph model on the canvas of `ui/editor/graph/` (below). `mod.rs` holds
`EditorPanel` (`AppState::editor`: a grid rule's text as typed, the graph under edit as
`graph: Option<Box<GraphEditor>>`, the file name, the `ApplyOutcome` of the last Apply, the open
and save status, and the `ModelSource` applied last) and `editor_ui`, which dispatches on `graph`:
a header of New grid rule, New graph model (a menu of henad-graph's `starters()`, labelled by
each starter's model name), Open model, Save model and Apply, then the code view or the graph
editor. For a grid rule the list under the code holds the outcome and each `CodeDiagnostic` as a
selectable `Label` reading `<line>:<column> <message>`, whose press moves the cursor there
(`move_cursor_to`).
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

## The graph editor

`ui/editor/graph/` edits a graph model through henad-graph's `GraphBuilder` and `EditCommand`s,
never by writing the document. `EditorPanel::file_text` is the graph's canonical text, which Apply,
Save model and `save_name` read, so Apply runs the same path as a grid rule (`ModelFile::parse`,
`entry`, `install_models`, the Rebuild model modal past tick 0, fault claiming). A grid graph
installs as a GPU grid entry at its own size, and a shortfall of `Demand::shortfalls` (the
submission budget included) is `ApplyOutcome::Faulted`, as for a grid rule. Apply also replaces a
running preview at any tick: `runs_source` never holds for a preview, and `install`'s
`settle_loaded` leaves a preview alone. `receive_open` opens a file that `GraphFile::parse` reads
on the canvas, and any other text in the code view, a graph that does not read included, where Apply
places its problems. `new_graph_model(app, file_name)` opens a starter, and Edit model opens a
runtime entry whose code is `henad-graph/1` (`editable_graph`, `can_edit`). `GraphEditor::new`
places every body without placement through `arrange` (no undo step) and opens the first tick pass.

`mod.rs` holds `GraphEditor`: the builder, `History`, the open `Scope`, one `BodyCanvas` per body
opened (keyed by scope), the `CheckReport` and a per-pass map of `NodeIssues` from diagnostics that
carry a pass and a node. Each canvas takes an egui id of its own from the load counter and the
editor's count of canvases made (`canvases_made`), never from the scope's name. egui-snarl keeps
the selection, the transform and the node sizes under that id, and a body reopened after its canvas
was dropped, or a new pass that takes a renamed pass's id, would otherwise read a stale selection
by pin index. Every gesture is a `GraphRequest`
(`Edit(GraphEdit)` of commands with a coalesce key, `AddNode` with an optional `DroppedWire`,
`Undo`, `Redo`, `Copy`, `Cut`, `Paste`, `Delete`, `Arrange`, `Open`, `Frame`, `FrameAll`).
`graph_ui` draws the side panel (`passes.rs`, `world.rs`), the toolbar and the canvas, then
`settle` runs the requests: while the primary button is down they wait in `pending`, and once it
is up a drag's `Move`s and any wire egui-snarl dropped without a callback (`lost_wires`) commit
first, then the requests, then the open canvas syncs. This order is the guard of trap 8 of record
#62: a removal while a wire is in flight panics egui-snarl. `edit` runs commands on a clone, so a
refused command changes nothing, and its message (capitalised, with a full stop) is `status`, a
`GraphStatus` whose `warning` draws it in the theme's warn colour. A copy's "Copied N nodes." is a
note (`GraphStatus::note`) in the text colour.
`refresh` checks again, rebuilds `issues` and reopens the first pass when the open one is gone.
Undo keeps the canonical text before each edit, `[editor]` included (`history.rs`, 200 snapshots),
and consecutive edits with one coalesce key make one step (a literal dragged, a parameter's range,
the world table) until the gesture ends. `settle` ends it when the primary button goes up, before
and after the batch the release runs, and when keyboard focus moves (`pressed`, `focused`), so two
drags of one field are two steps. The world table and a parameter's range and default show
`shown_world` and `shown_param`, the last waiting `SetWorld` or `SetParam`, so a dragged field
shows its value while the requests wait for the release. Keys over the canvas while no field has
focus: command Z, command shift Z or command Y, Delete or Backspace on the selection, and egui's
`Copy`, `Cut` and `Paste` events. Copy puts
`GraphBuilder::copy` text on the clipboard, and Paste places the nodes' top left at the pointer.

`canvas.rs` holds `BodyCanvas` (the `Snarl<NodeView>`, the `NodeId` to egui-snarl node map, the
drawn rectangles, the transform, `framing`), the viewer and `show_canvas`. `sync` brings the
`Snarl` to the document by difference (remove, insert, update in place, then wires sorted by
`wire_order` so a run is reproducible), never by rebuilding, which would move the selection. A
`NodeView` holds the pins in the order of the kind's resolved ports, outer outputs before inner
outputs, so a pin index maps back to a port name, and types and rates come from the report's
`PassReport`. The colour of a pin is its type and its shape its rate (`sockets.rs`, a data palette
outside MCS). A constant input edits in place (`DragValue`, a checkbox, a variant menu), a bound
parameter shows as a chip, and the node menu holds Frame, Copy, Inputs (bind to a parameter, set a
constant, clear), Settings (configuration keys with a short list of values), Add statistic (a
tick-rate output in a stats pass) and Delete. A node with an error takes a red 500 stroke and a
red 900 badge, a warning alone orange, and the badge's press frames the node, as does a press on a
problem in the list under the canvas. A framing holds over frames until the target's rectangle
stops changing. A plain click selects through the modifier write of record #62 around `show`.
Quick-add (`quick_add.rs`) opens on the background and on a dropped wire, and adds the node where
the pointer was when the menu opened (`menu_origin`), since egui-snarl passes the menu's top left: `offered_kinds` keeps the
kinds the pass's stage, backend and domain take (no region-only kind), that have a socket for the
wire's type, and that match every word of the query or a synonym. Arrange (`arrange.rs`) places
the body in columns by longest path, each as wide as its widest drawn node.

`GraphEditor::program` compiles the document through `GridProgram::compile` once per revision
(`CompiledProgram`), for the generated WGSL view and the preview. `wgsl_view.rs` is a right panel,
toggled by the toolbar's WGSL switch (`GraphRequest::ShowWgsl`), that shows `GeneratedShader::text`
of each shader whose `passes` hold the open pass as a selectable non-wrapping monospace label, and
`HOST_PASS`, `DISABLED_PASS` or `NOT_COMPILED` otherwise. View in viewport (`view_pass.rs`) is a
node-menu submenu over the outputs `can_view` accepts (element rate, `f32`, `u32`, `i32`, `bool` or
a category) of a node outside every region. `GraphRequest::View` sets `viewing` (a `ViewTarget`)
when `view_file` builds and checks: on a copy of the builder it switches every view pass off, adds
the palette `view_ramp` (`RAMP`, nine viridis stops, a data palette) and the view pass
`view_in_viewport`, pastes `upstream` (the node and everything it reads, regions whole) into it,
and colours the value (a category by its own palette, a `u32` as an index, an `i32` and an `f32`
through `view.ramp`, a flag as 0 or 8). The document never changes, and a refusal (a draw, which a
view pass cannot make) is `status`. `preview_program` compiles the view file while `viewing` holds,
and drops a view the document no longer allows with "Stopped viewing ...". `preview_key` (load,
revision, view changes) is what the preview compares.

## The live preview

`ui/editor/preview.rs`: `EditorPanel::preview` (`Preview`), and `poll`/`poll_at`, which
`HenadApp::logic` calls every frame after `report_fault`. A new `preview_key` with no errors sets
`due` to now plus `DEBOUNCE` (250 ms) and records `AppState::builds`; one with errors drops `due`
and leaves the running preview alone. Once due, a build counted since the edit (Build, Apply)
drops the preview to `Idle`, `preview_can_load` false leaves it `Waiting` (`PREVIEW_WAITS`, "Preview
waits for the running model."), and otherwise `build_preview` compiles `preview_program`, takes
`preview_values` (defaults, `grid_width` and `grid_height` at most `PREVIEW_SIDE`, 256), refuses a
shortfall or a `SetupError` as `Refused`, and calls `AppState::load_preview`, which plays the run.
`AppState::preview` (`PreviewRun`, the size) marks a loaded preview, and `load` clears it.
`load_preview` plays the run only when the preview is still loaded after `load_setup`: a build that
fails inline (a browser, or a test with `AppState::inline_builds`) or whose thread does not start
has already offloaded it. `EditorPanel::load` and `load_graph` reset `preview` to its default, so a
state of the last document never shows over the next.
`preview_can_load` holds with nothing loaded and no build, with a preview loaded, or with a built
run at tick 0 and no Rebuild model modal. A preview is never the selection: `selection_is_loaded`
and `code_pending` are false, `select_model` does not restore its values, the next build's fields
stay, and `settle_outcome` sees no loaded source, so "Model applied." becomes "Press Build to run
it." behind a preview. A code fault of a preview (any tick) goes to `Preview::show_fault`
(`Faulted`) through `show_fault` while the Editor holds a graph, and the modal keeps the rest,
the fault of a preview that outlived its graph included. Playback's `preview_line` names the
preview and its size, with the hover `preview_hover`: `PREVIEW_APPLY`, or `PREVIEW_ORPHANED` once
the Editor holds no graph.

`src/tests/graph_editor.rs` holds the state tests (starters, open by kind, Apply installs at full
size, the Rebuild model modal past tick 0, refused edits, undo, coalescing, dropped wires, pass
renames, issue counts, parameters, Arrange, opening without placement, the colour of a copy's
status),
`src/tests/graph_preview.rs` the preview, the WGSL view, View in viewport and the budget
(`a_new_graph_model_previews_at_the_preview_size`, `binding_a_parameter_restarts_the_preview`,
`an_invalid_edit_leaves_the_preview_running`,
`an_edit_during_a_full_size_run_past_tick_zero_leaves_that_run_going`,
`a_build_after_the_edit_replaces_the_waiting_preview`,
`a_run_at_tick_zero_gives_way_to_the_preview`,
`apply_replaces_a_running_preview_with_the_full_size_run`,
`a_preview_keeps_a_side_below_the_limit`, `the_wgsl_view_reads_the_shaders_of_the_open_pass`,
`a_preview_whose_build_fails_at_once_stays_paused_with_nothing_loaded`,
`another_document_clears_the_state_of_the_preview`,
`a_preview_that_outlived_its_graph_reports_to_the_fault_dialog`,
`view_in_viewport_previews_a_view_pass_of_the_output`,
`the_submission_budget_refuses_apply_at_full_size_and_lets_the_preview_run`,
`every_starter_previews_within_the_device_limits_and_the_work_budget`), and
`src/tests/graph_canvas.rs` the canvas over a headless `egui::Context` (a fresh egui state per
canvas, two drags as two undo steps, a dragged world field and the accessible bounds of a node
among them), which arranges the starter first, since the starters' placement puts pins of
neighbouring nodes on top of each other. In a test build each pin records its rectangle under
`canvas::pin_key`. egui writes AccessKit bounds in a layer's own coordinates and ignores its
transform, so `show_canvas` ends with `place_accessible_nodes`, which moves the bounds of every
widget on egui-snarl's layer to the screen. Driving the canvas through the egui MCP server: a
locator finds a widget on a node, and a drag between pins still takes raw positions.
`click` and `drag` modifiers never reach `InputState::modifiers`, which egui-snarl reads, but a
plain click selects anyway. Command with `scroll` zooms.
