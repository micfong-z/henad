# henad-app: the sheet editor

`ui/editor/sheet/` shows a sheet, a model file of `kind = "sheet"` (`agents/henad-graph-sheet.md`),
in the Editor tab as its Sheet and Card panes, and holds the New model dialog. The canvas and the
code view are in `agents/henad-app-editor.md`, and the expert mode stays there beside the canvas it
extends. The panes are read-only labels.

`mod.rs` holds `SheetEditor`: the `SheetDocument`, its canonical text (`text`, which Save model
writes and Apply reads through `EditorModel::file_text`), the `SheetReport` of `ParsedSheet::check`
on that text (so every problem is placed in what Save writes), the `SheetFile` read, checked and
lowered from it or the problems that stop it lowering (`lowering_problems`, `has_errors`), the load
number from the graph editor's `LOADS` (shared, so a graph and a sheet never take one preview key)
and the compiled program. `SheetEditor::new(document)` writes the canonical text, parses it again
for the places, and parses it as a `SheetFile`. A document whose text does not write keeps an empty
text and the writer's error. `preview_key` is `[load, 0, 0]`. `program` compiles
`SheetFile::lowered` through `GraphProgram::compile` on first use, and passes each compiler problem
through `SheetFile::place_graph_diagnostic`. `place` does the same for a problem of the run, which
`EditorModel::place` calls for a sheet and passes through for any other model:
`EditorPanel::show_fault` (a fault of the applied code before tick 0) and `show_preview_fault`
(`Preview::show_problem`) both place their fault. A fault carries no node or pass, so it lands at
`[sheet]`. `sheet_ui` draws the Sheet pane as a resizable `Panel::left` (`henad_sheet_pane`, 40% of
the tab's width by default, each pane at least 160 points) and the Card pane in the `CentralPanel`,
each under a heading ("Sheet", "Card", `SHEET_PANE` and `CARD_PANE`) in a `ScrollArea` of its own
(`henad_sheet_pane_scroll`, `henad_card_pane_scroll`). The world stays in the dock's Viewport,
Playback, Pacing, Parameters and Charts tabs (D56).

`sheet_pane.rs` draws `SheetSection::ALL` in order, each heading from `SheetSection::heading`:
the purpose, the world (`CELLS_ON_A_GRID`, `SIZE_LABEL` W `SIZE_BY` H, `EDGES_LABEL` with
`WRAP_AROUND` or `WALLS` and its sentence, then `tick_text`, "One tick is one step (no unit)" or
"One tick is 1 day"), the Cells table (`CELLS_COLUMNS`: name, kind with the states, the start from
`starts_text`, and `changed_by` over the Become and Set lines of the cards that run each tick), the
parameters in a `kv_grid` (value and unit), the measures, `COLOR_CELLS_BY` X, and each button with
what it does (`START_AGAIN`, `run_once("cell")`). `starts_text` reads a `ByChance` start as
`BY_CHANCE`, and a `Value` start by its `value` slot: a state chip as `starts_all` of the state's
name ("All Tree"), a `RandomNumber` phrase as `A_RANDOM_NUMBER`, and anything else as `A_VALUE`.
`starts.state` belongs to `OnGround` alone. A painted, anywhere or on-ground start, which no grid
sheet holds, reads as an empty cell, and a `set_everywhere` step, which no grid control writes
(D98), as the pane's own "Set everywhere". `card_pane.rs` draws every card in file order:
`Sentences::card_heading`, then `card_lines` (display order from `CardRelations`, the number, the
depth of blocks around the line for an indent of 20 points, and `Sentences::line` with
`otherwise_numbers`), each line a weak number beside a wrapping label. Every word comes from
`henad_graph::sheet::sentence`.

The Editor opens a file by the `kind` in its header (`open_text`, through `model_text::read_header`,
D55): `sheet` through `ParsedSheet::parse` alone into `EditorPanel::load_sheet`, so a sheet with
checker errors opens and shows them, `graph` through `GraphFile::parse` onto the canvas, and any
other text in the code view. A sheet or a graph that does not read opens in the code view. For a
sheet the reader's problem is set as `ApplyOutcome::Unread` ("Model file has problems. Fix them,
then press Apply."), so the list under the code names it at once without reading as a refused
Apply. `load_sheet` clears the outcome, the applied source and the preview, as `load_graph` does.
The list under the panes (`sheet_outcome_ui`) shows the preview's state (`preview_ui` with `plays`
false, so a running preview reads `preview_running_text`'s "Preview at 256 × 256 cells starts at
tick 0. Press Play to run it, or Apply to run the model at its own size."), the last Apply's outcome
with any problem the check does not list, then every diagnostic of the report and every lowering
problem the report lacks, errors in the theme's error colour and warnings (skip warnings included)
in its warn colour, as `<line>:<column> <message>`. `save_name` suggests `<sheet id>.toml` for a
sheet with no file name, errors or not. Apply is unchanged: `ModelFile::parse(...).and_then(|file|
file.entry())` installs the lowered graph's entry, and `edit_model` reopens an applied sheet's
entry as its lowered graph unless the Editor applied it.

**The preview** (D52, D73). A sheet's preview takes the graph editor's path (`preview.rs`,
`henad-app-editor.md`) with one difference: `EditorModel::preview_plays` is false, so
`load_preview` builds it paused at tick 0 with its start drawn, and Playback's Play runs it.
Playback's `preview_line` adds `PREVIEW_WAITS_FOR_PLAY` ("Press Play to run the preview.") while a
sheet's preview waits at tick 0. `AppState::previews_editor_sheet` holds while the preview runs and
the loaded id is the sheet's. `load_preview` records the selection in `preview_selection`, and
`AppState::build_replays_preview` adds that the selection is still that one. Only then does Build's
`press_build` run `preview::rebuild_preview` instead of `reset_simulation`: the same setup at the
same seed (the preview's seed is `None`, the model's default), so the world starts again as it
started before, paused. Build is enabled then whatever the selected model's `build_refusal`, with
the tooltip `BUILD_SHEET_TOOLTIP`. Once the Model tab selects another model, Build builds that
model with its usual tooltip and refusal, as behind a graph's preview. After Apply the run is the
selected model's, and Build builds it as for any model, at the Seed field's seed.
`AppState::runs_editor_sheet` holds for the sheet's preview or the source the Editor applied, and
Playback's `tick_counter` then reads `henad_graph::sheet::sentence::tick_counter` ("Tick 4 (a tick
is one step of time)", or "Tick 4 (4 days)" through `time_axis::elapsed_text` once the entry has a
time scale). No autosave and no Facilitator panel (D61).

**The New model dialog** (`new_model.rs`, D57, D71). `NewModelDialog` sits in
`EditorPanel::new_model` while open: the `StarterChoice`s (`starter_choices`: each sheet starter of
a world made of cells, with its file name, the model name read from it, and its description from
`STARTER_CHOICES` by that name, the agent starters held out since the sheet pane has no agent
section),
the chosen index, the Name text and `name_typed`. `choose` sets the index and, until the author
types in Name, the name to the starter's. "Untitled model" is the default only for Blank, which is
not a starter yet. `open_new_model` opens it from the Editor header's New
model, the File menu's first entry and the Model tab's New model. `lib.rs` calls
`new_model_modal` every frame beside the other modals: an `egui::Modal`
(`henad_new_model_modal`) whose content `Ui` takes `Role::Dialog` and the label "New model"
through `accesskit_node_builder`, the heading `NEW_MODEL`, `START_FROM` with one `radio_value` per
starter and its description indented under it, a Name `TextEdit` labelled by its label, Cancel and
Create (`filled_button`, blue 700). Enter in Name creates, as Create does, and Cancel, Escape or a
click on the backdrop closes it. `create_model(app, file_name, name)` reads the starter, runs
`SheetEdit::SetSheetName` when the name differs (an empty name is `UNTITLED_MODEL`, and a refused
one keeps the starter's name with the reason as the file status), loads the sheet with no file name
and brings the Editor to the front. The world question of the final dialog shows only under Blank,
and Blank is not offered.

`src/tests/sheet_editor.rs` holds
`every_sheet_starter_opens_as_a_sheet_previews_at_tick_zero_plays_and_applies` (each starter waits
at tick 0 in the preview run of its world, `PreviewRun::Grid` or `PreviewRun::Agents`, plays past
it, applies and builds), `the_new_model_dialog_offers_every_grid_starter_and_no_agent_starter`,
`the_card_pane_reads_the_life_lines_as_their_sentences` (and the vote's "otherwise"),
`apply_installs_and_builds_the_sheet`,
`save_writes_the_canonical_text_and_the_saved_file_reopens_to_the_same_document` (a hand-edited
sheet saves canonical, and `save_name` without a file),
`a_sheet_with_a_skipped_line_opens_and_previews` (Game of Life with line 2's old state blank),
`a_sheet_with_a_checker_error_opens_in_the_sheet_editor` (a default outside its range: no preview,
Apply refused), `a_sheet_whose_text_does_not_read_opens_in_the_code_view_with_its_problem`,
`a_sheet_preview_waits_at_tick_zero_until_play` (the counter's words, Play advances it, a graph's
preview still plays), `a_rebuild_replays_the_same_start` (Build on the preview, then after Apply at
seeds 7, 7 and 8, compared through the Alive measure at tick 0),
`build_builds_another_selected_model_instead_of_the_sheet_preview`,
`the_cells_table_reads_each_start_in_the_wording_table_words`,
`the_sheet_preview_line_names_play_and_the_graph_line_is_unchanged`, and over a `Harness` that
draws the File menu, the Editor tab and the dialog with AccessKit on:
`new_model_opens_a_dialog_and_create_opens_the_chosen_sheet` (the dialog role, the three radios in
order, Name following the starter, a typed name kept, Create, the three lines, Cancel and Escape),
`enter_in_the_name_field_creates_the_model` and
`new_node_graph_lists_the_six_graph_starters_from_the_header_and_the_file_menu` (a submenu's label
ends with " ⏵", which the harness strips). `the_empty_editor_names_every_way_in` holds
`EMPTY_EDITOR`. Keep them.
