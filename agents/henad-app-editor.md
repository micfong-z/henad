# henad-app: the Editor tab

`ui/editor/` is the Editor tab, the in-app editor of one model file: a grid rule as text in the code
view, a graph model on the canvas of `ui/editor/graph/` (below), or a sheet in the panes of
`ui/editor/sheet/` (`agents/henad-app-sheet.md`). `mod.rs` holds `EditorPanel`
(`AppState::editor`: the model under edit as `model: EditorModel`, the file name, the `ApplyOutcome`
of the last Apply, the open and save status, and the `ModelSource` applied last) and `editor_ui`,
which matches on the model: a header of New model (`NEW_MODEL_BUTTON`, which opens the New model
dialog of `henad-app-sheet-open.md`), New grid rule, New node graph (`NEW_NODE_GRAPH`, its widget
name too, a menu of henad-graph's `starters()` then `agent_starters()`, labelled by each starter's
model name), a vertical hairline, Open model and Save model, wrapping in a narrow tab, then Apply at
the right end as the one primary button (`filled_button`, blue 700), then the code view, the graph
editor or the sheet editor. Apply is drawn last, so the keyboard path keeps its order. Each of
these buttons shows its tooltip under the pointer and on keyboard focus
(`FocusTooltip::on_hover_or_focus_text`, `agents/ui.md`). The header's second row (`name_row`)
holds the model's name (`model_name`) in the body colour, the dot of `widgets::unsaved_mark` while
`is_unsaved` holds, and the file's name in the weak colour when it is not the name. A sheet's row
is `sheet::name_row`, with the rename button, Undo, Redo and the menu
of the model's actions (`henad-app-sheet.md`). A graph keeps Undo and Redo in its canvas toolbar.
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
dispatches on the header's `kind` (`henad-app-sheet-open.md`). The `Empty` arm draws the code view
over a fresh `String` each frame and becomes a `Rule` once typing leaves it non-empty. For a grid
rule the list under the code holds the outcome and each `CodeDiagnostic` as a selectable notice in
the mono face reading `<line>:<column> <message>`, its notes weak under it, whose press moves the
cursor there (`move_cursor_to`). Every line of the status area under the panes is a
`widgets::Notice` (`NoticeKind::Info`, `Warning` or `Error`, `NoticeKind::of` a problem's
severity): the last open or save (`file_status`, a `FileStatus` that is a note or a failure,
`file_status_ui`), `NO_COMPUTE` as a warning, the preview's line, the outcome (`outcome_line`, an
error for a refusal or a fault), "Press Apply to check and install the model." (`PRESS_APPLY`)
and each problem.
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
after another selection, or once a sheet's preview loads after Offload. An `Applied`,
`AwaitingRebuild`, `Installed` or `Unchanged` outcome is dropped once `EditorModel::edit_mark` (a
hash
of a rule's or a sheet's text, a graph's `revision`) differs from `applied_mark`, the mark Apply
took. `load` commits the loaded entry when the build starts, and the outcome reads `Applied` from
then on. New grid rule loads
`life_starter()`, the canonical text of `GridRule::life()`, and Edit model, shown for a runtime
entry whose code is in `GridRule::CODE_FORMAT`, its canonical text. Edit model and the Results tab's
Open in Editor read through `editable_model(author, code)`, which returns an `EditableModel` (the
sheet of a `henad-sheet/1` author text, else the graph or the grid rule of the code), and
`EditorPanel::load_model` loads it from no file. Each action that replaces the model asks first
while it holds unsaved edits (`henad-app.md`, unsaved edits). `can_edit` is
`editable_model(...).is_some()`. Open model and Save model go through `OpenTarget::Model` and
`SaveTarget::Model(n)`, a `.toml` file on
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
`NO_COMPUTE`. Apply builds no model that Build refuses (`build_refusal`: the sizes the device
lacks at the next build's values, a Seed field that does not parse, values the setup rejects), and
reports the reason as `ApplyOutcome::BuildRefused`, which reads as a fault does ("Model build
failed. Fix the problem below."). An outcome that would ask for a Rebuild or a Build that is
disabled (`rebuild_refusal`, `build_refusal`) reads `BuildRefused` with the reason as well, the
Rebuild model modal still open (`an_apply_that_build_refuses_names_the_reason`).
`settle_outcome` takes the reason the button gives now (`rebuild_refusal` while the modal is open,
else `build_refusal`, read only for a `BuildRefused` outcome): it names a new reason, turns
`AwaitingRebuild` or `Installed` once the reason is gone, is dropped once Build builds another
model, and turns `Applied` once the loaded model runs the applied source, as after Build with the
seed fixed (`a_refused_build_says_what_the_button_does_once_the_reason_is_gone`). While Build
replays the sheet's preview, which Playback enables whatever the refusal, Apply builds the applied
model, so the line keeps the reason Apply meets and goes once it is gone, as `Installed` does then
(`a_refused_build_line_keeps_its_reason_while_build_replays_the_preview`). `Faulted` is a
fault of a build of the applied code, which leaves nothing loaded, and turns `Applied` the same way.
`reset_simulation`, `open_run` and
`open_setup` refuse the sizes too, since the engine's own assert would end a browser's page, and
`reset_simulation` refuses a Seed field that holds no seed, which would build at the last valid
seed (`an_apply_while_the_seed_is_no_integer_builds_nothing`). `src/tests/editor.rs` holds
`new_grid_rule_starts_from_the_life_starter`,
`applying_an_edit_installs_and_rebuilds_from_tick_zero`,
`an_edit_after_apply_drops_the_applied_line`,
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
`a_diagnostic_moves_the_cursor_to_its_line`, `edit_model_shows_the_code_the_rule_runs`,
`a_refused_build_says_what_the_button_does_once_the_reason_is_gone` and
`a_refused_build_line_keeps_its_reason_while_build_replays_the_preview`. Keep them.

## The graph editor

`ui/editor/graph/` edits a graph model through henad-graph's `GraphBuilder` and `EditCommand`s,
never by writing the document. `EditorModel::file_text` is the graph's canonical text, which Apply,
Save model and `save_name` read, so Apply runs the same path as a grid rule (`ModelFile::parse`,
`entry`, `install_models`, the Rebuild model modal past tick 0, fault claiming). A grid graph
installs as a GPU grid entry, and a graph of agents as a GPU agents entry, at its own size, and a
shortfall of `Demand::shortfalls` (the submission budget included) is `ApplyOutcome::BuildRefused`,
as for a grid rule. Apply also replaces a running preview at any tick: `runs_source` never holds for
a preview, and `settle_loaded` leaves a preview alone. `open_text` opens a file whose header reads
`kind = "graph"` and that `GraphFile::parse` reads on the canvas, and a graph that does not read in
the code view, where Apply places its problems. `new_graph_model(app, file_name)` opens a starter,
and Edit model opens a runtime entry whose code is `henad-graph/1` and records no sheet.
`new_model_id(models, id, prefix)` gives a new model its id: `id` while no compiled entry of the set
holds it, else `prefix` and `id` (`sheet_` for New model from Blank or a starter, `node_` for
`new_graph_model`, none for New grid rule), numbered by `mint_id` while compiled code holds that
too. A runtime entry holds no id, and a new model replaces it on Apply. `apply` asks first through
`replace_question` when the runtime entry was applied from a sheet (author text in
`SHEET_CODE_FORMAT`) whose name differs from the new entry's and whose source is not the one the
Editor applied last (`is_applied`, so a rename of that sheet does not ask):
`EditorPanel::replace_question` holds a `ReplaceQuestion` (name and id), and
`replace_question_modal`, drawn from `lib.rs`, is a dialog titled `REPLACE_APPLIED_HEADING` with
`sentence::replace_applied`, Cancel and Replace (destructive, which Enter presses). Replace runs
`apply_replacing`, which reads the text again and installs it through `install_applied`, or
applies it as `apply` does when its id is no longer the question's. Nothing changes before the
answer, and a load or a swap drops the question (`clear_outcome`). The test `Harness` draws the
dialog too. Note that a swap clears `applied`, so a rename of the sheet after a swap and back asks.
`src/tests/model_ids.rs` holds it, with compiled models of the user's own beside the examples
(`apply_asks_before_it_replaces_a_model_applied_from_another_sheet`,
`the_replace_question_closes_on_cancel_a_click_outside_and_a_swap`).
`GraphEditor::new` places every body without placement through `arrange` (no undo step) and opens
the first tick pass.

The canvas, its panels, regions, functions, the expression editor and the WGSL view are in
`agents/henad-app-graph-editor.md`.

## The expert mode

`ui/editor/expert.rs`. Convert to node graph in the sheet toolbar's
Model actions menu and Edit as node graph at the top of the Diagram send `EditorRequest::Convert`,
and `open_convert` opens the dialog (`EditorPanel::convert` holds the sheet's name) unless
`convert_refusal` (`ConvertRefusal::of` the report, or `Errors` for a sheet that does not lower)
holds. Both controls are disabled then, with `ConvertRefusal::tooltip`. `convert_modal` is a dialog
named `CONVERT_HEADING`, the two paragraphs of `convert_text` one label per line, Cancel and Convert
(the primary, which Enter presses), and Convert sends `ConfirmConvert`, which goes through
`replace_model(app, Replacement::Convert)`: a copy beside the sheet with unsaved edits, which the
new copy replaces, asks first (`henad-app.md`, unsaved edits). `convert_sheet` takes
`ConvertOptions::new` with `today()` (UTC, Hinnant's `civil_date`) and a `taken` closure over the
app's model ids and names and the files beside the sheet, and the sheet's own file name for
`origin.file`. On native, a sheet with `EditorPanel::file_path` (set by `receive_open` from
`DialogFile::path`, cleared by every load and by a save of the model) has its copy written beside it
(`save_beside`, status "Saved <file>"). The copy opens through `load_graph`, so `GraphEditor::new`
places it: `place_unplaced` and Arrange pass `arrange::frame_groups` (one group per frame with
`lines`, a frame drawn by hand makes none) and `arrange` gives each group a band of rows of its own
across the columns its nodes and estimated title cover (`FRAME_SPACE`, `TITLE_CHAR_WIDTH`), a region
joining the group of its contents (`group_of_nodes`). The sheet moves to `EditorPanel::counterpart`
(`Counterpart::take_from` the panel: model, file name, path, saved and saving marks), which keeps
its undo history, and `EditorModel::is_edited_since` answers whether either holds unsaved edits.
Every `load*` goes through `EditorPanel::replace`, which clears the counterpart, the path, `beside`,
the banner and the dialog (`clear_expert`) and the outcome, the line to scroll to and the preview
(`clear_outcome`), and `swap_counterpart` exchanges the model with the counterpart and clears what
`settle_swap` names, `clear_outcome` among them.

The *original* of a copy is the counterpart when it is a sheet with `origin.model`'s id, else
`EditorPanel::beside`, the `expert::Beside` that `find_original` read from `origin.file` beside the
copy's path (`ParsedSheet` for the document and the id, and `SheetFile::parse` for the lowered
file, `None` while errors stop it lowering).
`copy_banner` returns a `CopyBanner` (cached by load, code revision and a hash of the original's
text) in four states (`CopyState`): `Unedited`, `Edited(node_edits)` when `GraphEditor::code_hash`
differs from `origin.copy_code` (counts from `changes_since_conversion`), `OriginalChanged` (the
code's hash differs from `origin.code`, or the counterpart or the sheet beside no longer lowers) and
`NotFound`.
`header_ui` draws it in a
`Panel::top` under the header as an information notice with small buttons after its text, Open
<name> (`EditorRequest::OpenOriginal`, `open_original_of`: a swap, or the sheet read from beside,
errors included, which takes the copy as its counterpart) in every state but `NotFound`. The sheet's
link line (`counterpart_copy`) sends `OpenCopy`. The graph side panel ends with `origin_ui` under
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
are found, so two runs at one seed differ). Each run sits in a `Side` whose drop stops it within
`STOP_WAIT`, so closing, replacing or loading never joins a run's thread without a bound
(`agents/sim-runner.md`). `EditorPanel::comparison` survives a swap to the sheet and is cleared by
every load. `Comparison::key` holds the copy's load and code revision and the hash of the original's
text when the runs started (`BannerKey`, `expert::current_copy_key`), and `header_ui` drops the
comparison once the Editor shows that copy with another key. A placement keeps the code revision and
the comparison.

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
`a_converted_copy_opens_at_a_readable_zoom_and_frame_all_fits_every_node` (SIR's copy at 0.5 or
more in a window of 1000 by 700, then Frame all below 0.5 with every node in view),
`editing_a_node_of_the_copy_changes_the_banner_and_a_new_node_joins_no_frame`,
`open_original_returns_to_the_sheet_and_its_line_opens_the_copy` (and `OriginalChanged` after a
sheet edit), `a_sheet_with_a_file_has_its_copy_written_beside_it_and_found_again` (a taken file
numbers the copy, `NotFound` once the sheet is gone),
`a_copy_finds_a_sheet_beside_it_that_no_longer_lowers_and_opens_it`,
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
114 kinds, `display_name(kind)`, and `kind_name(kind)`, the display name or else the kind, which the
header, Move into region and Show changes share. A node's header (`node_titles`) shows its label,
or else the display name, with the raw kind in grey under it, after the display name when the node
has a label ("Count around me · grid.count"), then the id. The hover shows the description
before the catalogue's semantics. Quick-add lists display names, its hover the kind and the
description, and
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
`due` to now plus `DEBOUNCE` (250 ms) and records `AppState::builds`. One with errors drops `due`
and leaves the running preview alone, and the status area then reads `STALE_PREVIEW`, or a sheet's
`STALE_WORLD_NOTICE`, as a warning in place of the running line. Once due, a build counted since the
edit (Build, Apply) drops the preview to `Idle`, `preview_can_load` false leaves it `Waiting`
(`PREVIEW_WAITS`, "Preview waits for the running model."), and otherwise `build_preview` compiles
`preview_program`, takes `preview_values` (defaults, `grid_width` and `grid_height` at most
`PREVIEW_SIDE`, 256, `num_agents` at most `PREVIEW_AGENTS`, 16384, `PreviewRun::Grid` or `Agents`)
with the Parameters tab's tried values in place (`with_tried`, `agents/henad-app.md`), refuses a
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
stay, and `settle_outcome` sees no loaded source, so "Model applied." becomes "Model applied. Press
Build to run it." behind a graph's preview, and is dropped behind a sheet's preview that Build
replays. A code fault of a preview (any tick) goes to `Preview::show_problem` (`Faulted`, placed by
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
canvas, two drags as two undo steps, a dragged world field, a world value outside its range kept
(`the_world_panel_keeps_a_value_outside_its_range`) and the accessible bounds of a node among them).
The `CanvasHarness` (a `GraphEditor` alone on a context, `over` an editor before any frame or `new`
over an arranged starter, with `accessible_nodes`, `accessible` and `accessible_with_role`), `n`,
`link`, `graph_editor_of` (an editor of a graph starter) and `bind_spread_rate` (SIR's new parameter
bound to node 12) that the graph test files share sit in `src/tests/support.rs`.
`src/tests/graph_regions.rs` holds the region tests (two halves in a frame, the Repeat button,
nested frames, a drop into a frame with its blue stroke and Move out of region, the output half's
drag, the bound from a constant count, an out-of-range bound left as it is, the Repeat controls,
quick-add in a frame and in a radius fold), and `src/tests/graph_functions.rs` the function and
expression tests (a call opens its function under a breadcrumb on a canvas of its own, a problem in
a function, make and ungroup, an added output with its node, one undo per added port and per
unplaced import, an import with its origin, a wrap in a function, `can_join`'s and `can_add`'s
rules, the node menu of a radius fold, and the expression editor's rename and refused duplicate
typed through egui). The harness arranges the starter first, since the starters' placement puts pins
of neighbouring nodes on top of each other. In a test build each pin records its rectangle under
`canvas::pin_key`. egui writes AccessKit bounds in a layer's own coordinates and ignores its
transform, so `show_canvas` ends with `place_accessible_nodes`, which moves the bounds of every
widget on egui-snarl's layer to the screen. Driving the canvas through the egui MCP server: a
locator finds a widget on a node, and a drag between pins still takes raw positions. `click` and
`drag` modifiers never reach `InputState::modifiers`, which egui-snarl reads, but a plain click
selects anyway. Command with `scroll` zooms.
