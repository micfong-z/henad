# henad-app: the Editor tab

`ui/editor/` is the Editor tab, the in-app editor of one model file: a grid rule as text in the code
view, a graph model on the canvas of `ui/editor/graph/` (below), or a sheet in the panes of
`ui/editor/sheet/` (`agents/henad-app-sheet.md`). `mod.rs` holds `EditorPanel`
(`AppState::editor`: the model under edit as `model: EditorModel`, the file name, the `ApplyOutcome`
of the last Apply, the open and save status, and the `ModelSource` applied last) and `editor_ui`,
which matches on the model: a header of New model (`NEW_MODEL_BUTTON`, which opens the New model
dialog of `henad-app-sheet.md`), New grid rule, New node graph (`NEW_NODE_GRAPH`, its widget name
too, a menu of henad-graph's `starters()` then `agent_starters()`, labelled by each starter's model
name), Open model, Save model and Apply, then the code view, the graph editor or the sheet editor.
The File menu holds New model, New grid rule, New node graph, Open model and Save model, and the
Model tab New model beside New grid rule and Edit model. Every site that loads a model into the
Editor sets `focus_request` to `Tab::Editor`, which switches to the Editor layout while the View
menu's checkbox holds (`henad-app.md`). `EMPTY_EDITOR` names New model, New
node graph, New grid rule and Open model. `EditorModel` is `Empty` (the
default), `Rule(String)` (a grid rule's text as typed, which may be empty or not read),
`Graph(Box<GraphEditor>)` or `Sheet(Box<SheetEditor>)`, so two models are never held at once.
Every site that differs by kind calls a method of the enum: `is_empty` (`Empty`, or a `Rule` whose
text is empty, on which Save model and Apply are disabled), `file_text` (what Apply, Save model and
`save_name` read), `holds_preview_model` (`Graph` and `Sheet`, read by `AppState::show_fault` and
`preview_hover`), `preview_source` (a `PreviewSource` of the preview key and the error flag, `None`
with no preview), `preview_program` and `place` (a sheet places a fault's problem in the sheet),
plus `rule_text`, `graph`, `graph_mut` and `sheet` for the sites of one kind. `load` sets `Rule`,
`load_graph` sets `Graph` and `load_sheet` sets `Sheet`. Open model goes through `open_text`, which
dispatches on the header's `kind` (`henad-app-sheet.md`). The `Empty` arm draws the code view over
a fresh `String` each frame and becomes a `Rule` once typing leaves it non-empty. For a grid rule
the list under the code holds the outcome and each `CodeDiagnostic` as a selectable `Label`
reading `<line>:<column> <message>`, whose press moves the cursor there (`move_cursor_to`).
`code_view.rs` draws one `TextEdit::multiline(..).code_editor()` with a layouter
that never wraps, in one `ScrollArea::both`, beside a gutter outside it. The gutter's numbers come
from the galley's rows (`gutter_numbers`) and follow them up and down, and stay in place while a
long line scrolls sideways. A text replaced from outside the field clears its undo history on the
next frame (`ViewReset`, `clear_history`), since egui keeps one history under the field's id, and
another file's text also returns the view to the top left. No syntax highlighting, and the check
runs on Apply alone. `editor::apply` reads the text through `ModelFile::parse` and `entry`, which
leave the set and the run alone on a refusal, then installs a one-entry set through `install_models`
and selects the model. With no run, or a run at tick 0 (`run_at_start`, also true during a build
unless a run to a tick waits for it), it starts a build as Build does, on the build thread on
native, and a build of another model is abandoned for it. A run of the same id past tick 0 waits on
the Rebuild model modal (`ApplyOutcome::AwaitingRebuild`, "Model applied. Press Rebuild to run it
from tick 0."), and Apply calls `settle_loaded` itself when `install_models` passes over an entry
the set already holds while the run differs, so the modal opens again after Cancel. A run of another
model past tick 0 keeps going, as does the run after Cancel (`ApplyOutcome::Installed`, "Model
applied. Press Build to run it."). `EditorPanel::settle_outcome` moves between the two, to `Applied`
once the loaded model runs the applied source, and from `Applied` to `Installed` once nothing is
loaded, as after Cancel of a build or Offload. `editor_ui` also passes it the source Build builds
now, the selected entry's, or `None` while `build_replays_preview` holds, and an `Installed` outcome
that source no longer matches is dropped to `None` ("Press Apply to check and install the model."):
after another selection, or once a sheet's preview loads after Offload. `load` commits the loaded
entry when the build starts, and the outcome reads `Applied` from then on. New grid rule loads
`life_starter()`, the canonical text of `GridRule::life()`, and Edit model, shown for a runtime
entry whose code is in `GridRule::CODE_FORMAT`, its canonical text. Edit model and the Results tab's
Open in Editor read through `editable_model(author, code)`, which returns an `EditableModel` (the
sheet of a `henad-sheet/1` author text, else the graph or the grid rule of the code), and
`EditorPanel::load_model` loads it from no file. `can_edit` is `editable_model(...).is_some()`. Open
model and Save model go through `OpenTarget::Model` and `SaveTarget::Model`, a `.toml` file on
native and a picked file or a download in a browser. Opening loads the text and applies nothing.
`AppState::show_fault` gives a fault to the Editor (`EditorPanel::claims`) when it belongs to a run
built from the applied source that has not passed tick 0, in place of the fault modal. A lost
device, a failed poll and a device out of memory stay with the modal, since no edit of the code
fixes them. `PendingBuild` keeps the entry it builds, and `finish_build` passes it to `show_fault`,
so a fault the build thread returns reaches the Editor on the frame `poll_build` takes it.
`show_fault_in_modal` passes the Editor over: a build thread that did not start or ended without a
result, and on native a fault `report_fault` takes from a sink while a build runs. That fault comes
from a detached run or from rendering. Apply returns before `poll_build`'s frame, and ends early
only when the Editor claims a fault during the call, as it can for a browser's inline build. In a
browser `catching_on` pushes no scopes, and the device reports a refused rule through the
`FaultSink` a frame or more after the build. Without compute, Apply is disabled and the tab reads
`NO_COMPUTE`. Apply builds no model the device cannot hold at the next build's values, and reports
the sizes as `ApplyOutcome::Faulted`. `reset_simulation`, `open_run` and `open_setup` refuse it too,
since the engine's own assert would end a browser's page. `src/tests/editor.rs` holds
`new_grid_rule_starts_from_the_life_starter`,
`applying_an_edit_installs_and_rebuilds_from_tick_zero`,
`applying_beside_another_run_past_tick_zero_waits_for_build`,
`a_sheet_applied_beside_another_run_drops_the_build_line_once_its_preview_loads`,
`a_graph_applied_beside_another_run_keeps_the_build_line_over_its_preview`,
`a_grid_rule_applied_beside_another_run_keeps_the_build_line_after_offload`,
`selecting_another_model_drops_the_build_line`,
`applying_again_after_cancel_asks_to_rebuild_again`,
`applying_during_another_build_replaces_it_and_cancel_waits_for_build`,
`a_code_edit_that_fails_validation_keeps_the_running_model`,
`a_fault_in_the_apply_window_is_shown_in_the_editor`,
`a_build_fault_of_the_applied_code_reaches_the_editor_from_the_build_thread`,
`a_diagnostic_moves_the_cursor_to_its_line` and `edit_model_shows_the_code_the_rule_runs`. Keep
them.

## The graph editor

`ui/editor/graph/` edits a graph model through henad-graph's `GraphBuilder` and `EditCommand`s,
never by writing the document. `EditorModel::file_text` is the graph's canonical text, which Apply,
Save model and `save_name` read, so Apply runs the same path as a grid rule (`ModelFile::parse`,
`entry`, `install_models`, the Rebuild model modal past tick 0, fault claiming). A grid graph
installs as a GPU grid entry, and a graph of agents as a GPU agents entry, at its own size, and a
shortfall of `Demand::shortfalls` (the submission budget included) is `ApplyOutcome::Faulted`, as
for a grid rule. Apply also replaces a running preview at any tick: `runs_source` never holds for a
preview, and `install`'s `settle_loaded` leaves a preview alone. `open_text` opens a file whose header reads
`kind = "graph"` and that `GraphFile::parse` reads on the canvas, and a graph that does not read in
the code view, where Apply places its problems. `new_graph_model(app, file_name)` opens a starter,
and Edit model opens a runtime entry whose code is `henad-graph/1` and records no sheet.
`GraphEditor::new` places every body without placement through `arrange` (no undo step) and opens
the first tick pass.

`mod.rs` holds `GraphEditor`: the builder, `History`, the open `Scope`, the breadcrumb `trail` (the
bodies a function was entered from through calls), one `BodyCanvas` per body opened (keyed by
scope), the `CheckReport` and a per-scope map of `NodeIssues`. `diagnostic_scope` puts a diagnostic
in its pass, or in the function that holds its node (a function's diagnostics carry a node and no
pass), and `issue_counts(scope)` counts them. Each canvas takes an egui id of its own from the load
counter and the editor's count of canvases made (`canvases_made`), never from the scope's name.
egui-snarl keeps the selection, the transform and the node sizes under that id, and a body reopened
after its canvas was dropped, or a new pass that takes a renamed pass's id, would otherwise read a
stale selection by pin index. Every gesture is a `GraphRequest` (`Edit(GraphEdit)` of commands with
a coalesce key, `AddNode` with an optional `DroppedWire`, `Undo`, `Redo`, `Copy`, `Cut`, `Paste`,
`Delete`, `Arrange`, `Open` (clears the trail), `Frame`, `FrameAll`, `OpenCall`, `OpenParent`,
`MakeFunction`, `Ungroup`, `AddFunctionPort`, `PickImport`, `Import`, `CloseImport`). `graph_ui`
draws the side panel (`passes.rs`, `world.rs`), the toolbar and the canvas, then `settle` runs the
requests: while the primary button is down they wait in `pending`, and once it is up a drag's
`Move`s and any wire egui-snarl dropped without a callback (`lost_wires`) commit first, then the
requests, then the open canvas syncs. This order is the guard of trap 8 of record
#62: a removal while a wire is in flight panics egui-snarl. `edit` runs commands on a clone, so a
refused command changes nothing, and its message (capitalised, with a full stop) is `status`, a
`GraphStatus` whose `warning` draws it in the theme's warn colour. A copy's "Copied N nodes." is a
note (`GraphStatus::note`) in the text colour. `world.rs` leaves a hidden parameter out.
`refresh` checks again, rebuilds `issues` and reopens the first pass when the open one is gone.
The *code revision* counts changes of `GraphFile::code`'s hash, which `refresh` compares, so a
`Move` or Arrange changes the revision and not the code revision. `preview_key` and the `program`
cache follow the code revision, and a move leaves the preview running
(`moving_a_node_leaves_the_preview_running`).
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
drawn rectangles, the transform, `framing`), the viewer and `show_canvas`. `sync` brings the `Snarl`
to the document by difference (remove, insert, update in place, then wires sorted by `wire_order` so
a run is reproducible), never by rebuilding, which would move the selection. A `NodeView` holds the
pins in the order of the kind's resolved ports, outer outputs before inner outputs, so a pin index
maps back to a port name, and types and rates come from the report's `PassReport`. The colour of a
pin is its type and its shape its rate (`sockets.rs`, a data palette outside MCS). A constant input
edits in place (`DragValue`, a checkbox, a variant menu), a bound parameter shows as a chip, and the
node menu holds Frame, Copy, Inputs (bind to a parameter, set a constant, clear), Settings
(configuration keys with a short list of values), Add statistic (a tick-rate output in a stats pass)
and Delete. A node with an error takes a red 500 stroke and a red 900 badge, a warning alone orange,
and the badge's press frames the node, as does a press on a problem in the list under the canvas. A
framing holds over frames until the target's rectangle stops changing. A plain click selects through
the modifier write of record #62 around `show`. Quick-add (`quick_add.rs`) opens on the background
and on a dropped wire, and adds the node where the pointer was when the menu opened (`menu_origin`),
since egui-snarl passes the menu's top left: `offered_kinds(document, Option<&Pass>, ..)` keeps the
kinds the pass's stage, backend and domain take (`fn.input` and `fn.output` never), or in a function
(`None`) the kinds with no sink effect (`SINKS`, as henad-graph's checker), the world's domain and
the function's own input and output (`function_takes`), that have a socket for the wire's type, and
that match every word of the query or a synonym. Opened inside a region's frame (`region_at`, the
deepest frame painted this frame), it offers the kinds a region body takes (`ContextSet::region`,
`region.put` included, `lane.set` not) and the node joins that region
(`GraphRequest::AddNode::region`), and outside every frame no region-only kind. Inside a frame,
`can_add` (the nesting rule of `can_join` for a new node) leaves out a region kind that cannot nest
there, and `GraphEditor::add_node` refuses the same cases. A Repeat added from
quick-add starts with `count` 1, `max` 1 and `config.acc = []`. Arrange (`arrange.rs`) places canvas
nodes (`CanvasKey`: a node, or a region's output half) in columns by longest path, each as wide as
its widest drawn node, a region's input half then moved right to the column before its first reader
(`the_input_half_of_a_region_sits_in_the_column_before_its_body`): a link to an inner output leaves
the input half, a link to an accumulator the output half, and every node inside a region reads the
input half and feeds the output half, so a body sits between its halves. `place_unplaced` places an
output half only in a body placed from scratch, and counts a node as placed by
`EditorRegion::node_pos`, so a converted copy's labelled nodes with no `pos` are arranged too. It
runs at load, on every edit's draft before the snapshot (`edit`, `add_node`, `wrap`,
`add_function_port`) and after `restore`. Otherwise a node an
edit leaves unplaced, such as a new function port's node or an import from a file with no node
placement, reaches `BodyCanvas::moved` as a move, an edit of its own that clears redo after each
undo.

**Regions** (E2 step 1). A region draws as two canvas nodes (`NodePart`): the input half
(`BodyCanvas::nodes`, the region's inputs and inner outputs) and the output half
(`BodyCanvas::output_halves`, its accumulator outputs, titled "<title> output", no badge). Both map
to the region's id, so selecting or deleting either half acts on the region. The output half sits at
`[editor] node.<id>.output_pos`, or one column right of the region's contents, and a drag of it is
`MoveRegionOutput` (compared with `placed`, the position the last sync gave each canvas node).
`document_wires` and `output_pin` put a link to an inner output on the input half and any other
link, a bare one included, on the output half. Every region has a `CanvasFrame` (`region` set, no
title, `region_colors` by depth: gray 900, 800, 700 fills) around both halves and
`NodeBody::region_contents`, painted outermost first from the rectangles cached in the last frame,
which nests frames. `regions_entered` gives the region a node moved since the last sync joins on
release: the deepest region frame, drawn without the moved canvas nodes, that holds the node's
centre, never one around the node (the node menu's Move out of region reaches those) and nothing for
a node whose region moves with it. `settle` runs those `SetRegion`s in the drag's edit, after its
`Move`s, and while the button is down `show_canvas` strokes the target frames blue 500
(`drop_targets`). The node menu holds Move into region (every region outside the node's own subtree
but its current one) and Move out of region (to the region around its region). Move out of region
also moves the node, with every node inside it, right of the frame it leaves by `COLUMN_GAP`
(`moves_out_of_frame`, in the same edit) when any of them lies within `FRAME_MARGIN` of the frame
drawn without them. Otherwise a non-member stays drawn inside the frame and reads as a member. `can_join` filters
drop targets, both menus' targets and Move out of region by library 1's nesting: a plain node joins
any region, a Repeat holds a Repeat or a `grid.fold`, a `grid.fold` a Repeat, a radius fold holds no
region and sits in none, and the chain of regions stays within `MAX_REGION_DEPTH` (3). The toolbar's
Repeat (`MDI_REPEAT`) sends `GraphRequest::WrapInRepeat` of the selection, and `GraphEditor::wrap`
runs `EditCommand::WrapInRepeat` with the selection's drawn bounds, then `fit_wrap` moves the halves
clear of the wrapped nodes and the puts once the canvas has drawn them at one size for two frames,
inside the wrap's undo step. The input half of a region holds the region controls
(`region_controls`): a Repeat's Max (`SetConfig`, coalesced, `clamp_existing_to_range(false)` so
an out-of-range `max` is drawn as it is and left to the checker), one row per accumulator of any region
with `acc` (a name field that renames on losing focus, the type menu `accumulator_types`, the
combine menu filtered by `Combine::applies_to`, a type change keeping the combine when it applies
and taking `carry` otherwise, and Remove accumulator), and Add accumulator (`acc`, `f32`, `sum`).
`GraphEditor::edit` adds `max` = count clamped to 1 to 65536 after every constant `count` of a
Repeat (`fill_repeat_max`). `settings_lines` leaves out the keys the controls show.

**Functions** (E2 step 2). `functions.rs` is the functions panel under the pass stack: a row per
function (open, issue badges, `from <file>` with the origin's hash as hover), the open function's
forms (Name renames, each input and output with a name field, a type menu of `canvas::value_types`
and Remove, Add input and Add output, which `GraphEditor::add_function_port` runs as the port and a
`fn.input`/`fn.output` node naming it, Remove function), an add form, and Import function.
`PickImport` sets `pick_import`, which `editor_ui` turns into `OpenTarget::Function`, and
`receive_import` passes the file to `set_import_source`, which lists its functions (a file that is
no graph or holds none is a warning). Each listed function imports under the name in its field
(`GraphRequest::Import`). A call's node menu holds Open function (`OpenCall`, the open body pushed
on the trail) and Ungroup. The toolbar shows the breadcrumb (`breadcrumb_ui`: a link per trail
entry, `OpenParent(index)`, then the open body), Repeat in any body, and Function
(`MDI_FUNCTION_VARIANT`, `make_function`, named `function`, `function_2`, ...). A function's canvas
syncs with `CheckReport::functions`' report of its body. Settings lists functions for a call's
`function` and the open function's ports for `fn.input`/`fn.output`. An Expression node
(`math.expr`) draws `expression_editor` in its body: the text (multiline, commits on losing focus),
each input (name field that moves the input's value to the new port, `moved_input`, and refuses
another input's name through `GraphRequest::Refuse`, which sets the status line, a type menu and
Remove), Add input (`x`, `f32`), and the outputs as one comma-separated field. Its `text`, `inputs`
and `outputs` leave `settings_lines`.

`GraphEditor::program` compiles the document through `GraphProgram::compile`, `GridProgram` or
`AgentProgram` by the world, once per code revision (`CompiledProgram`), for the generated WGSL view
and the preview. `wgsl_view.rs` is a right panel, toggled by the toolbar's WGSL switch
(`GraphRequest::ShowWgsl`), that shows `GeneratedShader::text` of each shader whose `passes` hold
the open pass as a selectable non-wrapping monospace label, and `HOST_PASS`, `DISABLED_PASS` or
`NOT_COMPILED` otherwise. View in viewport (`view_pass.rs`) is a node-menu submenu over the outputs
`can_view` accepts (element rate, `f32`, `u32`, `i32`, `bool` or a category) of a node outside every
region. `GraphRequest::View` sets `viewing` (a `ViewTarget`) when `view_file` builds and checks: on
a copy of the builder it switches every view pass off, adds the palette `view_ramp` (`RAMP`, nine
viridis stops, a data palette) and the view pass `view_in_viewport`, pastes `upstream` (the node and
everything it reads, regions whole) into it, and colours the value (a category by its own palette, a
`u32` as an index, an `i32` and an `f32` through `view.ramp`, a flag as 0 or 8). The document never
changes, and a refusal (a draw, which a view pass cannot make) is `status`. `preview_program`
compiles the view file while `viewing` holds, and drops a view the document no longer allows with
"Stopped viewing ...". `preview_key` (load, revision, view changes) is what the preview compares.

## The expert mode

`ui/editor/expert.rs`. Convert to node graph in the sheet toolbar's
Model actions menu and Edit as node graph beside the Diagram send `EditorRequest::Convert`, and
`open_convert` opens the dialog (`EditorPanel::convert` holds the sheet's name) unless
`convert_refusal` (`ConvertRefusal::of` the report, or `Errors` for a sheet that does not lower)
holds. Both controls are disabled then, with `ConvertRefusal::tooltip`. `convert_modal` is a `Modal`
of role `Dialog` named `CONVERT_HEADING`, the two paragraphs of `convert_text` one label per line,
Cancel and Convert (`filled_button`), and Convert sends `ConfirmConvert`. `convert_sheet` takes
`ConvertOptions::new` with `today()` (UTC, Hinnant's `civil_date`) and a `taken` closure over the
app's model ids and names and the files beside the sheet, and the sheet's own file name for
`origin.file`. On native, a sheet with `EditorPanel::file_path` (set by `receive_open` from
`DialogFile::path`, cleared by every load and by a save of the model) has its copy written beside it
(`save_beside`, status "Saved <file>"). The copy opens through `load_graph`, so `GraphEditor::new`
places it: `place_unplaced` and Arrange pass `arrange::frame_groups` (one group per frame with
`lines`, a frame drawn by hand makes none) and `arrange` gives each group a band of rows of its own
across the columns its nodes and estimated title cover (`FRAME_SPACE`, `TITLE_CHAR_WIDTH`), a region
joining the group of its contents (`group_of_nodes`). The sheet moves to `EditorPanel::counterpart`
(`Counterpart`: model, file name, path), which keeps its undo history. Every `load*` clears the
counterpart, the path, `beside`, the banner and the dialog (`clear_expert`), and `swap_counterpart`
exchanges the model with the counterpart and clears what `settle_swap` names.

The *original* of a copy is the counterpart when it is a sheet with `origin.model`'s id, else
`EditorPanel::beside`, the sheet `find_original` read from `origin.file` beside the copy's path.
`copy_banner` returns a `CopyBanner` (cached by load, code revision and a hash of the original's
text) in four states (`CopyState`): `Unedited`, `Edited(node_edits)` when `GraphEditor::code_hash`
differs from `origin.copy_code` (counts from `changes_since_conversion`), `OriginalChanged` (the
hash differs, or the counterpart no longer lowers) and `NotFound`. `header_ui` draws it in a
`Panel::top` under the header, blue 950 fill and blue 200 text, with Open <name>
(`EditorRequest::OpenOriginal`, `open_original_of`: a swap, or the sheet read from beside, which
takes the copy as its counterpart) in every state but `NotFound`. The sheet's link line
(`counterpart_copy`) sends `OpenCopy`. The graph side panel ends with `origin_ui` under
`FROM_AUTHOR_MODEL` (model, file, converted date, then each `[[editor.note]]` title and text, the
sheet's purpose and each parameter's meaning) when the file records a `ModelOrigin`. Nodes
added after conversion join no frame. The app draws and renames no frame, so a frame's title
stays as the conversion wrote it.

**Show changes**. `CopyBanner::changes` holds a `CopyChanges` in the `Edited` state,
empty otherwise: the `ChangeGroup`s of `changes_since_conversion` and the name of each removed node,
the display name of its kind in the original's lowered graph (`CopyChanges::new`). The banner
offers the `SHOW_CHANGES` toggle only for an edited copy with a change to list (`shows_changes`).
An edit that changes no node, such as a new category, leaves the copy `Edited` with no toggle.
The toggle runs `EditorRequest::ShowChanges` on `EditorPanel::show_changes`, cleared by every load
and swap. `editor_ui` sets `GraphEditor::shown_changes` from `expert::shown_changes` each frame
before `graph_ui`. `graph/changes.rs` draws `CHANGES_HEADING` at the top of the side panel: each
group under its frame title or `IN_NO_FRAME`, its pass, then a link per added or changed node
("Changed: Either (n2001)", the label or display name, `GraphRequest::Frame` on its canvas,
`canvas_scope`) and a line per removed node ("Removed: Is (n2001)", the id alone when the original
holds no such node). `marks_of` gives the open canvas a `ChangeMark` per
added or changed node (`CanvasContext::marks`): a 2-point stroke and a header badge, green for
`NODE_ADDED` and yellow for `NODE_CHANGED`, an issue's colour taking the stroke first.

**Compare runs**. The banner offers `COMPARE_RUNS` in the `Unedited` and `Edited` states
(`compares_runs`), disabled in a browser with `COMPARE_NEEDS_NATIVE`, since a GPU model runs in no
sweep there. `compare::start_comparison` reads the original's canonical text (`original_text`) and
the copy's, builds both entries through `ModelFile`, and `Comparison::start` runs each as a sweep of
one run in memory (`SweepRun`, `SweepOutput::Memory`, the Sweep tab's `provenance` and build pool, a
repaint waker) for `COMPARE_TICKS` (200) ticks at the Parameters tab's seed or
`DEFAULT_COMPARE_SEED`, with the sizes `preview_values` caps (`grid_width`, `grid_height`,
`num_agents`) and no reducers. `header_ui` polls it each frame (`Comparison::poll`, `Planned` gives
the stat columns, `RunFinished` the series) and draws `comparison_ui` under the banner while the
Editor shows a graph: its line (`compare_running`, `runs_match`, `runs_differ` with both values in
`format_precise`, or in every digit when that prints both the same, `runs_share_no_measure`,
`runs_do_not_replay` for `CompareOutcome::Inexact`, `compare_failed`), Close comparison
(`EditorRequest::CloseComparison`) and one `show_plot` of every measure, the original solid and the
copy dashed in the same data colour, with a vertical line at the first difference.
`first_difference` compares the measures both runs hold by name, tick by tick, two non-numbers
counting as equal, and `Comparison::outcome` turns a difference into `Inexact` when either entry's
`metadata().replays_exactly` is false (bounded confidence and boids add floats in the order agents
are found, so two runs at one seed differ). Each run sits in a `Side` whose drop stops it within `STOP_WAIT`, so closing,
replacing or loading never joins a run's thread without a bound (`agents/sim-runner.md`).
`EditorPanel::comparison` survives a swap to the sheet and is cleared by every load.
`Comparison::key` holds the copy's load and code revision and the hash of the original's text when
the runs started (`BannerKey`, `expert::current_copy_key`), and `header_ui` drops the comparison
once the Editor shows that copy with another key. A placement keeps the code revision and the
comparison.

**Categories**. `world.rs`'s Categories section draws each variant's colour as the sheet's
`color_button` (the twelve palette colours, named "Colour of waned: Purple" by `variant_color_name`
through `sentence::color_button_name`), which runs `SetVariantColor`, Add variant gives the new
variant `next_variant_color` (the first of `STATE_COLORS` its category lacks, gray once all are
taken), and `add_category_ui` adds a category with its id and first variant (`AddCategory`, the
first variant in `STATE_COLORS[0]`), since the checker refuses a category with none. The three
fields carry their accessible names through `set_name`.

`src/tests/expert.rs` holds the Diagram tests, `the_conversion_modal_names_no_preset`,
`convert_is_disabled_with_its_tooltip` (an error and a skipped line, both controls disabled and
their tooltips shown on hover),
`the_words_rule_exempts_whole_controls_and_still_reports_a_bare_node`,
`converting_game_of_life_opens_the_copy_with_frames_titled_by_line_and_the_banner` (frames apart
from one another, at their drawn sizes),
`editing_a_node_of_the_copy_changes_the_banner_and_a_new_node_joins_no_frame`,
`open_original_returns_to_the_sheet_and_its_line_opens_the_copy` (and `OriginalChanged` after a
sheet edit), `a_sheet_with_a_file_has_its_copy_written_beside_it_and_found_again` (a taken file
numbers the copy, `NotFound` once the sheet is gone),
`show_changes_lists_a_highlife_edit_under_the_frame_of_line_2_and_marks_it_on_the_canvas` (the
HighLife edit of henad-graph's `a_highlife_edit_is_found_by_changes_since_conversion`, rebuilt in
the app test, one changed node under line 2's frame, two added in no frame, badges on the canvas, a
link that opens its node), `an_unedited_copy_offers_no_show_changes`,
`a_removed_node_is_listed_by_the_name_of_its_kind`,
`the_notes_of_the_author_model_show_under_its_section`,
`add_category_on_a_converted_sir_adds_a_state_with_its_colour_and_the_copy_still_checks` (and no
Show changes after it), `an_unedited_copy_matches_its_sheet_at_every_tick_of_compare_runs`,
`compare_runs_names_the_first_tick_at_which_a_highlife_edit_differs` (hidden in sheet mode, shown
again on the copy), `compare_runs_of_a_model_that_does_not_replay_exactly_names_no_first_difference`
(bounded confidence) and `an_edit_of_the_copy_drops_the_comparison_of_the_copy_before_it` (a move
keeps it). `compare.rs` holds the unit tests of `first_difference` and `difference_texts`.

**Display names, origin ports and frame titles**. `graph/display.rs` holds
`DISPLAY_NAMES`, a `DisplayName` (kind, name, category, description) for each of the catalogue's
114 kinds, and `display_name(kind)`. A node's header (`node_titles`) shows its label, or else the
display name, with the raw kind in grey under it, after the display name when the node has a label
("Count around me · grid.count"), then the id. The hover shows the description before the
catalogue's semantics. Quick-add lists display names, its hover the kind and the description, and
`matches_words` searches the id, the catalogue label and group, the display name and the category,
so a catalogue label still finds its kind. Move into region names nodes by display name. A pin
shows `InputView::label` or `OutputView::label`, the port name except on a Become, where
`label_by_origin` names `rule.<id>.when` and `fired.<id>` by the line prefix that a frame recording
that line alone gives before ": " (`line_prefixes`): "Line 2 when", "Line 2 fired", and "Line 2 when
(Alive)" for a rule `l2_alive` of a change from any state. The hover keeps the port name. A titled
frame is at least as wide as its title laid out (`fit_title`, `FRAME_TITLE_PADDING`), and the title
is painted whole.

## The live preview

`ui/editor/preview.rs`: `EditorPanel::preview` (`Preview`), and `poll`/`poll_at`, which
`HenadApp::logic` calls every frame after `report_fault`. They read the model only through
`EditorModel::preview_source` and `preview_program`. A new preview key with no errors sets
`due` to now plus `DEBOUNCE` (250 ms) and records `AppState::builds`; one with errors drops `due`
and leaves the running preview alone. Once due, a build counted since the edit (Build, Apply)
drops the preview to `Idle`, `preview_can_load` false leaves it `Waiting` (`PREVIEW_WAITS`, "Preview
waits for the running model."), and otherwise `build_preview` compiles `preview_program`, takes
`preview_values` (defaults, `grid_width` and `grid_height` at most `PREVIEW_SIDE`, 256,
`num_agents` at most `PREVIEW_AGENTS`, 16384, `PreviewRun::Grid` or `Agents`) with the Parameters
tab's tried values in place (`with_tried`, `agents/henad-app.md`), refuses a
shortfall (`AppState::entry_shortfalls`) or a `SetupError` as `Refused`, and calls
`AppState::load_preview` with `EditorModel::preview_plays`: a graph's preview plays, and a sheet's
waits at tick 0 for Play.
`AppState::preview` (`PreviewRun`, the size) marks a loaded preview, and `load` clears it.
`load_preview` plays the run only when asked and the preview is still loaded after `load_setup`: a
build that fails inline (a browser, or a test with `AppState::inline_builds`) or whose thread does
not start has already offloaded it. `EditorPanel::load`, `load_graph` and `load_sheet` reset
`preview` to its default, so a state of the last document never shows over the next.
`preview_can_load` holds with nothing loaded and no build, with a preview loaded, or with a built
run at tick 0 and no Rebuild model modal. A preview is never the selection: `selection_is_loaded`
and `code_pending` are false, `select_model` does not restore its values, the next build's fields
stay, and `settle_outcome` sees no loaded source, so "Model applied." becomes "Press Build to run
it." behind a graph's preview, and is dropped behind a sheet's preview that Build replays. A code
fault of a preview (any tick) goes to `Preview::show_problem` (`Faulted`, placed by
`EditorModel::place`) through `show_fault` while `holds_preview_model` holds, and the modal keeps
the rest, the fault of a preview that outlived its graph included. Playback's `preview_line` names
the preview and its size, with the hover `preview_hover`: `PREVIEW_APPLY`, or `PREVIEW_ORPHANED`
once the Editor holds no graph or sheet.

`src/tests/graph_editor.rs` holds the state tests (starters, open by kind, Apply installs at full
size, the Rebuild model modal past tick 0, refused edits, undo, coalescing, dropped wires, pass
renames, issue counts, parameters, Arrange, opening without placement, the colour of a copy's
status), `src/tests/graph_preview.rs` the preview, the WGSL view, View in viewport and the budget
(`a_new_graph_model_previews_at_the_preview_size`, `binding_a_parameter_restarts_the_preview`,
`an_invalid_edit_leaves_the_preview_running`,
`an_edit_during_a_full_size_run_past_tick_zero_leaves_that_run_going`,
`a_build_after_the_edit_replaces_the_waiting_preview`,
`a_run_at_tick_zero_gives_way_to_the_preview`,
`apply_replaces_a_running_preview_with_the_full_size_run`, `a_preview_keeps_a_side_below_the_limit`,
`the_wgsl_view_reads_the_shaders_of_the_open_pass`,
`a_preview_whose_build_fails_at_once_stays_paused_with_nothing_loaded`,
`another_document_clears_the_state_of_the_preview`,
`a_preview_that_outlived_its_graph_reports_to_the_fault_dialog`,
`view_in_viewport_previews_a_view_pass_of_the_output`,
`the_submission_budget_refuses_apply_at_full_size_and_lets_the_preview_run`,
`every_starter_previews_within_the_device_limits_and_the_work_budget`), and
`src/tests/graph_canvas.rs` the canvas over a headless `egui::Context` (a fresh egui state per
canvas, two drags as two undo steps, a dragged world field and the accessible bounds of a node among
them, and the `Harness`, `accessible` and `accessible_with_role` that `src/tests/graph_regions.rs`
and `src/tests/graph_functions.rs` share. `graph_regions.rs` holds the region tests: two halves in a
frame, the Repeat button, nested frames, a drop into a frame with its blue stroke and Move out of
region, the output half's drag, the bound from a constant count, an out-of-range bound left as it
is, the Repeat controls, quick-add in a frame and in a radius fold), and `graph_functions.rs` the function and expression tests (a call opens its function
under a breadcrumb on a canvas of its own, a problem in a function, make and ungroup, an added
output with its node, one undo per added port and per unplaced import, an import with its origin, a
wrap in a function, `can_join`'s and `can_add`'s rules, the node menu of a radius fold, and the
expression editor's rename and refused duplicate typed through egui). The harness arranges
the starter first, since the starters' placement puts pins of neighbouring nodes on top of each
other. In a test build each pin records its rectangle under `canvas::pin_key`. egui writes AccessKit
bounds in a layer's own coordinates and ignores its transform, so `show_canvas` ends with
`place_accessible_nodes`, which moves the bounds of every widget on egui-snarl's layer to the
screen. Driving the canvas through the egui MCP server: a locator finds a widget on a node, and a
drag between pins still takes raw positions. `click` and `drag` modifiers never reach
`InputState::modifiers`, which egui-snarl reads, but a plain click selects anyway. Command with
`scroll` zooms.
