# henad-app: opening a sheet, its preview and the New model dialog

How the Editor opens a sheet and lists its problems under the panes, how Apply installs it, the
preview of a sheet and the New model dialog. The rest of the sheet editor, the `SheetEditor`, the
Sheet pane and the toolbar, is in `agents/henad-app-sheet.md`, and the tests of both in its last
paragraphs.

The Editor opens a file by the `kind` in its header (`open_text`, through
`model_text::read_header`): `sheet` through `ParsedSheet::parse` alone into
`EditorPanel::load_sheet`, so a sheet with checker errors opens and shows them, `graph` through
`GraphFile::parse` onto the canvas, and any other text in the code view. A sheet or a graph that
does not read opens in the code view. For a sheet the reader's problem is set as
`ApplyOutcome::Unread` ("Model file has problems. Fix them, then press Apply."), so the list under
the code names it at once without reading as a refused Apply. `load_sheet` clears the outcome, the
applied source and the preview, as `load_graph` does. The list under the panes (`sheet_outcome_ui`)
shows the preview's state (`preview_ui` over a `PreviewShown`: `preview_running_text` names Play
until `past_start`, and a preview running while the sheet has errors reads `STALE_WORLD_NOTICE`
instead), the last Apply's outcome with any problem the check does
not list, then every diagnostic of the report the card does not draw and every lowering problem the
report lacks, each an error or a warning notice (`NoticeKind::of`) of the message alone. Its line
and column point into the sheet's text, which sheet mode never shows. A waiting preview reads the
sheet's `PREVIEW_WAITS`, which names Offload. A problem of a line the card shows sits under it on
the card. `save_name` suggests `<sheet id>.toml` for a sheet with no file name,
errors or not. Apply's `ModelFile::parse(...).and_then(|file| file.entry())` installs
the lowered graph's entry, whose source records the sheet's canonical text as its author text, and
`edit_model` reopens an applied sheet's entry as that sheet unless the Editor applied it. A note
edit leaves the source equal: Apply reads `Unchanged`, rebuilds nothing, and replaces the set's
entry through `install_models` when `same_author` fails, so a sweep started next records the new
text (`a_note_edit_does_not_reinstall_the_model`).

**The preview**. A sheet's preview takes the graph editor's path (`preview.rs`,
`henad-app-editor.md`) with one difference: `EditorModel::preview_plays` is false, so
`load_preview` builds it paused at tick 0 with its start drawn, and Playback's Play runs it.
Playback's `preview_line` adds `PREVIEW_WAITS_FOR_PLAY` ("Press Play to run the preview.") while a
sheet's preview waits at tick 0. `AppState::previews_editor_sheet` holds while the preview runs and
the loaded id is the sheet's. `load_preview` records the selection in `preview_selection`, and
`AppState::build_replays_preview` adds that the selection is still that one. Only then does Build's
`press_build` run `preview::rebuild_preview` instead of `reset_simulation`: the same setup at the
same seed (the preview's seed is `None`, the model's default), so the world starts again as it
started before, paused. Build is enabled then whatever the selected model's `build_refusal`, with
the tooltip `BUILD_SHEET_TOOLTIP`, and Run to tick runs the preview, replaying its start for a tick
behind (`henad-app.md`). Once the Model tab selects another model, Build builds that
model with its usual tooltip and refusal, as behind a graph's preview. After Apply the run is the
selected model's, and Build builds it as for any model, at the Seed field's seed. While Build
replays the preview, `settle_outcome` drops an `Installed` outcome ("Model applied. Press Build to
run it."), as after Apply beside another model's run past tick 0 and then Offload, which loads the
waiting preview. The list then reads the preview's line and "Press Apply to check and install the
model.", and Apply builds the sheet at its own size
(`a_sheet_applied_beside_another_run_drops_the_build_line_once_its_preview_loads`).
`AppState::runs_editor_sheet` holds for the sheet's preview or the source the Editor applied.
Playback's tick counter reads as for any model, "Tick 4" with no unit and "Tick 60 (2.5 days)" once
One tick is has one, since the lowering gives every sheet with a `[time]` table a time scale, rate
lines or none (`a_sheet_s_tick_length_reaches_the_counter_in_the_unit_that_fits`). Nothing saves a
sheet on its own.

**The New model dialog** (`new_model.rs`). `NewModelDialog` sits in
`EditorPanel::new_model` while open: the `StarterChoice`s (`starter_choices`: every sheet starter,
grid and agent alike, with its file name, the model name read from it, and its
description from `STARTER_CHOICES` by that name), the chosen index, the Name text, `name_typed` and
`made_of`. `choose` sets the index and, until the
author types in Name, the name to the starter's. "Untitled model" is the default name of Blank, the
first choice. `open_new_model` opens it from the Editor header's New model, the File menu's first
entry and the Model tab's New model. `lib.rs` calls `new_model_modal` every frame beside the other
modals: an `egui::Modal` (`henad_new_model_modal`, a dialog of `widgets.rs` named "New model"),
the title `NEW_MODEL`, `START_FROM` as a caption with one `widgets::choice_row` per starter (a
radio button named by the starter, its description weak under its name, the selection fill and
a check while chosen) in `widgets::dialog_list`, which scrolls in a short window, the world
question indented under Blank, a Name `TextEdit` labelled by its label, Cancel and Create (blue
700). Enter creates, as Create does, and Cancel, Escape or a click on
the backdrop closes it. `create_model(app, file_name, name)` reads the
starter, runs `SheetEdit::SetSheetName` when the name differs (an empty name is `UNTITLED_MODEL`,
and a refused one keeps the starter's name with the reason as the file status), loads the sheet
under the id `new_model_id` gives (`agents/henad-app-editor.md`) with no file name and brings the
Editor to the front. The world question of the dialog shows only
under Blank, and Blank is offered first. `starter_choices` puts Blank first (`file_name`
`None`, "Blank" as its name), and the world question (`WORLD_QUESTION`, `CELLS_ON_A_GRID`
or `AGENTS_IN_OPEN_SPACE` into `made_of`) shows under Blank while it is chosen. `create_blank_model`
loads `blank_model(name)`: 100 by 100 cells that wrap, the id minted from the name (or
`untitled_model`, and `open_blank` passes it through `new_model_id`), no property, parameter or
card. `create_blank_agent_model` loads
`blank_agent_model(name)`: the same world made of agents, one kind `agent` named `BLANK_AGENT_NAME`
with 1000 agents and room for a million, a Position that starts anywhere at random (through
`SheetBuilder`, so its start takes a key) and a Heading, and a colour rule by heading, so the
preview draws the agents by heading with no pick under Colours
(`a_blank_agent_model_is_coloured_by_heading_without_a_pick`).
