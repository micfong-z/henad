# henad-app: the sheet editor

`ui/editor/sheet/` shows a sheet, a model file of `kind = "sheet"` (`agents/henad-graph-sheet.md`),
in the Editor tab as its Sheet and Card panes, and holds the New model dialog. The canvas and the
code view are in `agents/henad-app-editor.md`, and the expert mode stays there beside the canvas it
extends. The Sheet pane edits every section (S16a, S16b), those of a world of agents included
(S26), and the Card pane writes the lines of a cells card (S19, S22, `agents/henad-app-card.md`).

`mod.rs` holds `SheetEditor`: a `SheetBuilder` over the `SheetDocument` (so new draws take fresh
keys), its canonical text (`text`, which Save model writes and Apply reads through
`EditorModel::file_text`), the `SheetReport` of `ParsedSheet::check` on that text (so every problem
is placed in what Save writes), the `SheetFile` read, checked and lowered from it or the problems
that stop it lowering (`lowering_problems`, `has_errors`), the load number from the graph editor's
`LOADS` (shared, so a graph and a sheet never take one preview key), the compiled program,
`built_starts` (each cells property's start when the preview was last built, set by `mark_built`
from `preview::build_preview`), the `notice` of the last edit and the pane's `PaneState`.
`SheetEditor::new(document)` and every edit call `refresh`, which writes the canonical text, parses
it again for the places, and parses it as a `SheetFile`. A document whose text does not write keeps
an empty text and the writer's error. `preview_key` is `[load, 0, 0]`. `program` compiles
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

**Edits** (D52). `SheetEditor::edit(past_start, starts_only, run)` runs `run`, a closure of
`SheetEdit` commands, on a clone of the builder and keeps it only when every command is accepted, so
one form or field commit is one edit and one undo step. An edit that leaves the document
equal changes nothing. Every edit takes a fresh `load`, the preview key, so the preview builds again
at tick 0, except a `starts_only` edit while `past_start`, which keeps the key: the world runs on
and the Cells table shows `START_AGAIN_TO_SEE` under that property's select while `start_changed`
holds (Build, which is Start again, clears it). A restart while `past_start` sets `notice` to
`RESTART_NOTICE`, which `sheet_outcome_ui` lists under the preview line with an information icon.
`AppState::sheet_preview_past_start` is `previews_editor_sheet()` and either `Preview::acted` (set
by `ui/params.rs` when a button of the model is pressed in the preview, cleared by `build_preview`)
or `!run_at_start()`. `editor_ui` reads it before borrowing the panel and passes it to `sheet_ui`.

**The empty world.** `SheetEditor::shows_nothing` holds for a world of cells with no property.
`preview_source` then reports `has_errors` and `empty`, `preview::poll_at` offloads a running Editor
preview (so the Viewport draws nothing), and `sheet_outcome_ui` shows `WORLD_EMPTY` in place of the
preview line and leaves the checker's `SHEET_EMPTY` warning out of the list. The Cells section shows
`SHEET_EMPTY` with "+ Add property" as a `filled_button`, and the Card pane, with no card, shows
`card_header("cell")` and `BLANK_MODEL_CARD` (also under a tick card with no lines).

`sheet_pane.rs` draws `SheetSection::shown(made_of)` in order (`ALL` for cells, `AGENT_WORLD` with
Agents, Ground and Events in place of Cells), each heading from `SheetSection::heading` with its
help button, under `ui.push_id(PaneState::widgets)`, an id per sheet opened (`PaneState::new` takes
the load number from `SheetEditor::new`, not the preview key, which changes on every edit). The
field ids (`widgets.with(...)`, `start_field_id(widgets, property, slot)`) and the picker's Number
box derive from it, so a refused draft never shows in another sheet, and removing a by-chance row
forgets the drafts of that row and every row after it. The pane collects at most one `Pending` edit
per frame (a boxed closure, `starts_only`, `from_form`), run through `SheetEditor::edit` after
drawing. A refusal shows as the form's message or as `PaneState::refusal` under the Cells or Agents
section, through `refusal_text`. The world: `CELLS_ON_A_GRID`, Size as two `commit_field`s named
`WIDTH_NAME` and `HEIGHT_NAME` (a whole number of cells from 1 to
`henad_graph::limits::MAX_GRID_SIDE`, D72, with `world_side_out_of_range` for any other number, a
fraction or a percent included, and `TYPE_ONE_NUMBER` for text that is no number), Edges as a
`ComboBox` named `EDGES_LABEL` with `WRAP_TOOLTIP` or `WALLS_SENTENCE` and the grey sentence (no
torus mark, D93), then the Time line: `ONE_TICK_IS`, the length field named `TICK_LENGTH_NAME` while
a unit is set (0 or less becomes 1), and the unit select named `TICK_UNIT_NAME` with
`tick_unit_choice` (None first, "one step (no unit)", which writes no `[time]`). An edit that gives
the tick a new length or unit sets `PaneState::time_changed`, and while it holds the Time line is
followed by `chances_per_tick_note` of `rates::chance_line_numbers`, read from the sheet each frame
so that a card edit renumbers or removes the lines it names, and left out while no line draws a
chance (T9). The next edit of the pane or `close_forms` clears the flag. The same edit drops the
Card pane's tick notice (`CardState::drop_tick_notice`). The Cells table
(`CELLS_COLUMNS`): the name as a frameless button named `edit_property_heading` that opens the
property form, the kind (`STATES_KIND` with a swatch per state, `NUMBER_KIND`, or `INTEGER_KIND`
with `INTEGER_KIND_TOOLTIP`, D23), Starts as (a select named `starts_place` showing `starts_text`:
`starts_all` per state and `BY_CHANCE`, or `A_VALUE` and `A_RANDOM_NUMBER`, the latter for Number
only, since the lowering refuses a random whole number), and `changed_by` over the Become and Set
lines of the cards that run each tick. By chance draws each row on a line of its own (state select,
`WITH_CHANCE`, the number, × named `remove_row_name`), "+ Add row" named `add_row_name` (the first
state no row draws), and `OTHERWISE` with its select. A new by-chance start draws the second state
with an empty chance, otherwise the first. A Number start shows `STARTS_AT` or
`RANDOM_NUMBER_BETWEEN` .. `RANDOM_NUMBER_AND` (0 and 1 to start). Each Starts as number
(`start_number`, D111) is a `commit_field` named `starts_field_name(place, ...)` showing
`literal_words` (a percent as typed), with "Use a parameter" beside it opening `CHOOSE_A_PARAMETER`.
A parameter chip replaces the field and reopens `Blank::picker_title` with the Number box. A row's
empty field empties the chance, text that does not read keeps the old value with `TYPE_A_CHANCE` or
`TYPE_ONE_NUMBER`, and a chance outside 0 to 1 `CHANCE_OUT_OF_RANGE` (D75). `start_edit` changes the
start as the builder holds it when the edit runs. "New parameter..." opens the New parameter dialog
for that `StartTarget`, and Create runs `AddParameter` and the start's change as one edit, marked
`starts_only` since only the start uses the parameter. Past the start it waits for Start again, as a
chosen parameter does.

The property form (`PropertyForm`, under the table, one at a time): `NEW_PROPERTY` or
`edit_property_heading`, Name (`PROPERTY_NAME_FIELD`, `PROPERTY_NAME_PLACEHOLDER`), Kind radios
(States, Number, Integer), for States the description, rows of a `color_button` named
`state_color_name`, a name field named `state_name_field` and × named `remove_state_name`, then "+
Add state". Two empty rows start a new property, in `STATE_COLORS` 0 and 1. Enter in a state name
moves to the next row or adds one. While a row's colour is not picked by hand, its name sets
`suggested_state_color` or the row's default (B16). An existing property's states count as picked.
Done checks in the page's order (`check_form`) and runs `form_edit`: a new property is
`AddProperty`, `AddState` per row and a start in the first state (or at 0 for a number). A changed
kind runs `SetPropertyType` and starts afresh the same way, and otherwise removed states are
`Delete`, kept ones `Rename` and `Recolor`, new rows `AddState`. "Delete property" (edit only) runs
`Delete`. The form closes once its edit is kept.

The other sections (S16b). Purpose is a `commit_area` named "Purpose" with `PURPOSE_PLACEHOLDER`,
committing `SetPurpose` on focus loss. Parameters: a header ("Name", "Value") and per parameter two
rows, the name as a `commit_field` named `sheet_parameter_name_field` (an empty or taken name puts
the old name back, with `PARAMETER_NAME_MISSING` or `parameter_name_taken` in
`PaneState::parameter_message`), `value_slider` (an egui `Slider` named `value_slider_name`, step
`slider_step`: about a 200th of the range rounded down to 1, 2 or 5 times a power of ten. A drag
holds its value in temp data and commits once when let go, a click or a key at once), the value
field named `value_field_name` (a typed value outside the range widens it) and × named
`delete_parameter_name`, then `range_row`: Min and Max fields (`with_range_end`: Min above Max
swaps, equal ends make Max one more, the value moves into the range) and Unit as a
`suggestion_field` named `parameter_unit_name` with `UNIT_FIELD_PLACEHOLDER` and `unit_suggestions`
(free text, D89). Each edit is one `SetParameter` keeping `step` and `means`. Every number field
reads a percent. The trying line of S24 sits under its parameter. "+ Add parameter" opens the New
parameter dialog with no `StartTarget` (1 in 0 to 10), and Create runs `AddParameter` alone.
Measures: per States property a "Plot" box beside each state named `plot_state` of `state_label`
("Status Alive" when another property shares the state name), adding a count named by the state
(`add_count`, the property's name before it when the name is taken) or deleting the counts of that
state, then `PLOTTED` with a `color_button` named `measure_color_name` (`Recolor`), `measure_words`
(`count_measure`, `average_measure`, or the name) and × named `remove_measure_name`, then the
measure form (`MeasureForm`: `NEW_MEASURE`, radios `COUNT_MEASURE` and `AVERAGE_MEASURE`, selects
named `MEASURE_PROPERTY_NAME` and `MEASURE_STATE_NAME`, Done checked by `measure_edit` with the
three messages, and an average reads `{ my = <property> }` and takes the next of `MEASURE_COLORS`).
Colours: "Colour cells by" a select of every property (the rule's, else the first States property,
else the first), `color_rule_edit` writing `by_state` or `shaded_rule` (blue from 0 to 1, or the
previous shading's hue and ends), swatches under a States property, and `shading_ui` for a Number
property: `SHADE` select of `SHADE_COLORS` over `ShadeHue::ALL`, a gradient of `ShadeHue::ends` in
16 bands, and `SHADE_FROM` [`DARKEST_NAME`] `SHADE_DARK_TO` [`LIGHTEST_NAME`] `SHADE_LIGHT`, the
ends left out read as 0 and 1, and a value equal to the other end refused with `SHADE_ENDS_EQUAL`
(a darkest value above the lightest one is kept, and the lowering reverses the palette). A first
property that is a Number, with no States property and no rule, gets a shaded rule in `form_edit`.
Buttons: each with its name, `button_does` (`start_again_for`, `run_once("cell")`), `EDIT` named
`edit_button_name` and × named `delete_button_name`, the button form (`ButtonForm`, under the button
it edits or at the end: `NEW_BUTTON` or `edit_button_name`, Name named `BUTTON_NAME_FIELD`,
`BUTTON_DOES` with radios `START_AGAIN` beside a select named `START_AGAIN_FOR` and `run_once`,
checked by `check_button_form`), and `run_once_card` read-only under a Run once button (the card's
heading, each line's number and sentence from `card_lines`, or `BUTTON_CARD_EMPTY`). `button_edit`
adds with `AddButton` and `SetButtonSteps` (one property as `properties`, every property as `thing =
"cells"`), renames and sets steps, and deletes and adds again a button whose kind changes. A refusal
shows as the form's message (`FormKind` on `Pending`).

**A world of agents** (S26). The pane works on a *thing*, `cells`, `ground` or the agent kind's id:
`properties_of`, `find_property`, `all_properties` (also behind `starts_of` and
`without_starts_or_words` in `mod.rs`, so a start of any thing waits for Start again) and
`measured_thing` (the cells, or the first kind of agent, which Measures and Buttons act on).
`property_table` draws any thing's table and `property_form_or_add` its form or "+ Add property",
named `add_property_name(kind)` under the agents and `ADD_GROUND_PROPERTY_NAME` under the ground.
`PropertyForm::thing` keeps the form under its own table. `form_kinds` offers States and Number,
Integer for cells and agents (the ground lowers only numbers and states), and for agents Yes or no,
then Position or Heading while the agents hold none, and always the property's own kind. A new
property starts in its first state, at 0, at no (`Bool(false)`), anywhere at random (a Position) or
with no start (a Heading). `kind_words` and `starts_text(document, property)` read every kind and
start ("Anywhere at random", `on_ground_start`, `PAINTED`, "All Yes", `HEADING_START`).
`start_select` offers by kind: a state or By chance (with `PAINTED` as a selected entry that
changes nothing while the start is painted), All Yes or All No, Anywhere at random or each state of
a ground States property that `StartsAs::paints_single_cells` accepts, the predicate the lowering's
`on_ground` uses, A value or A random number. No control paints, so a ground made on the pane offers
no on-ground start, and choosing a state or By chance replaces a painted start until Undo. A Heading
shows `HEADING_START`, an Arrow its start through `Sentences::slot`, and a painted start
`painted_shapes` with no control.
The Number start's lead is `every_starts_at(thing_word)`. `NumberTarget` (`Start` or `Kept`) is what
`number_field` (the old start field, taking a `NumberField` with an optional `limit` that also
refuses an empty field), `pick_number` and the New parameter dialog fill. `changing_lines` adds
Steer, Keep between, Leave on a ground property and a Move for the card's Position and Heading.
`sheet_pane/agents.rs` holds the three sections. Agents: the name (`AGENT_NAME_FIELD`,
`rename_kind` renames and gives the kind the id its new name mints while its id is the one its old
name mints, a refused `ChangeId` leaving the old id), the number at the start (`AGENT_COUNT_NAME`)
and at most (`AGENT_MAX_NAME`, `AGENT_MAX_TOOLTIP`), each an integer with `integer_up_to`
(1 to the most, which is 1 to `MAX_AGENTS`, a smaller most lowering the count with it), then the
table and the refusal. Ground: `GROUND_EMPTY` or the table, then per Number property `marks_row`
(`MARKS_LABEL`, a select named `marks_name` of `MARKS_NONE`, `MARKS_LARGEST`, `MARKS_ADD_UP`, one
`SetMarks`, a share of 1 put in when the share is blank, and while marked `KEPT_LEAD` a
`number_field` named from `kept_place` from 0 to 1 with `KEPT_OUT_OF_RANGE`, `KEPT_TAIL`) and per
States property `blocked_row` (`blocked_lead`, a box per state named `blocked_state_name`,
`SetBlocked`). Events: `EVENTS_EMPTY`, each name a `commit_field` named `event_name_name` with the
two checks, × named `delete_event_name`, and `EventForm` (`NEW_EVENT`, `EVENT_NAME_FIELD`, Done or
Enter). Measures count agents (`COUNT_AGENTS_MEASURE`, `COUNT_AGENTS_LEAD`,
`count_agents_measure`) and average a Number, Integer or Position. Colours draw `colours_of` for
the cells (`COLOR_CELLS_BY`), or for the agents (`COLOR_AGENTS_BY`) and the ground
(`COLOR_GROUND_BY`, once it has a property), offering States, Number, Integer and Heading
(`ByHeading` with `BY_HEADING_NOTE`). The select's choice compares with the rule's own property, so
a thing with no rule takes one from the property it shows. Buttons act on `measured_thing`: Run once
reads `run_once(thing_word)`, Start again offers the thing's properties with a start and the
ground's, `button_edit` keeps a button's other steps (set everywhere) and puts its start-again step
in the old one's place, and `button_does` joins steps that set to one value
(`set_everywhere`, "Set To food and To home to 0 everywhere").

The toolbar (`toolbar_ui`, a `Panel::top` above both panes): the model name as a frameless button
named `model_name_button` ("Model name: X. Rename", tooltip `RENAME_MODEL_TOOLTIP`) that opens
`SheetEditor::renaming`, a field named `MODEL_NAME_FIELD` that takes the focus, keeps on Enter or a
click elsewhere (`SetSheetName`, the id kept) and puts the old name back on Escape or an empty name,
then Undo and Redo with their icons and tooltips. Undo (`SheetEditor::history`, the graph editor's
`History` of canonical texts): `edit` and `keep_value` record the text before a kept edit, and
`undo` and `redo` read a text back through `restore` (a fresh `SheetBuilder`, the pane's forms
closed, the restart rule with `starts_only` when the two documents differ only in starts, the name
and the purpose, `without_starts_or_words`). `sheet_ui` takes Cmd or Ctrl+Z, Shift+Cmd+Z and Cmd+Y
(`consume_shortcut`) while `keys` (no New model dialog), no New parameter dialog (`dialog_open`), no
modal of the app (egui memory's `top_modal_layer`, from the previous frame) and no focused text
field (`text_edit_focused`), anywhere in the Editor tab. A focused field keeps its own undo.
`Pending::one` counts `SetPurpose` and `SetSheetName` as `starts_only`, so a name or a purpose
leaves the running world alone. The Charts tab shows `PLOT_EMPTY` while `runs_editor_sheet` and the
sheet holds no measure (D87, D107). The Card pane shows `BUTTON_CARD_EMPTY` under a card that runs
once with no line.

`widgets.rs` adds `commit_area` and `suggestion_field` beside `commit_field`, all three over
`commit_edit`. The suggestions are a `Popup` kept open in temp data from the field's focus until a
choice, Enter, Tab, Escape or a press outside the field and the popup (the popup's rect kept from
the last frame), since a click on a suggestion takes the focus from the field first.

`sheet_ui` and `sheet_pane` take the `SheetTrial`s (parameter id and tried value) that `editor_ui`
reads through `sheet_trials` (`params::sheet_number`, which Keep uses too) while the sheet's preview
runs, and a wrapped weak label under the tried parameter reads `sheet_trying_value` ("The world is
trying 0.5. The model keeps 0.3.", D88), wrapping at the clip rectangle's right edge, since the
pane's content can be wider than the pane. `SheetEditor::keep_value` runs one
`SheetEdit::SetParameter` for the Parameters tab's Keep (range widened), one step of Undo, and
refreshes the sheet with its load number kept, so the preview starts nothing again.
`starts_text` reads a `ByChance` start as `BY_CHANCE`, and a `Value` start by its `value` slot: a
state chip as `starts_all` of the state's name ("All Tree"), a `RandomNumber` phrase as
`A_RANDOM_NUMBER`, and anything else as `A_VALUE`. `starts.state` belongs to `OnGround` alone. A
painted, anywhere or on-ground start, which no grid sheet holds, reads as an empty cell, and a
`set_everywhere` step, which no grid control writes (D98), as the pane's own "Set everywhere".

`widgets.rs` holds what both panes share: `chip_ui`, `blank_ui` and `blank_shapes` (moved from the
card pane), `set_name` (the accessible name of a `TextEdit` or a `ComboBox`, set after the widget),
`color32`, `commit_field` (a text field that commits on Enter or focus loss through a closure that
returns the refusal message; a `FieldDraft` in egui temp data keeps typed or refused text while
memory focus holds, read through `Memory::has_focus` since `Response::has_focus` also needs the
window's system focus, which an app driven through its inspection port lacks, `field_message`
returns the message drawn by `message_ui` under the row, and `forget_field` drops a draft), the
`Picker` (a `Popup` toggled by its anchor, named as a `Dialog` by its title, groups with weak
headings and frameless item buttons named by their first line with grey detail beside them, empty
groups left out, "New parameter...", a Number box named by its hint, and `empty` when nothing is
offered), `NumberUse` (`Chance` or `Number`, `read` with the range messages), `color_button` (a
swatch opening `CHOOSE_A_COLOR` with the twelve `PALETTE_COLORS` named by `COLOR_NAMES`),
`color_name` (the nearest palette name, for a starter's own colours), and the New parameter dialog:
`NewParameterDialog::new(number_use, typed, unit)` starts from 0.1 in 0 to 1 for a chance, 1 in 0 to
10 otherwise, or from the typed number with Min at twice a negative one and Max at `default_max` (1,
2 or 5 times a power of ten about ten times it, at most 1 below 1 or for a chance). Max and a
negative Min follow Default until typed. `parameter(taken)` runs the checks of the wording table in
order, reading every field with a percent. The modal (`henad_new_parameter_modal`) is a `Dialog`
named `NEW_PARAMETER_HEADING`. A rate's starting values and its "per [unit]" Unit wait for the first
rate blank (S19).

`card_pane.rs` and `picker.rs` draw and edit the cards, which `agents/henad-app-card.md` describes.

`help.rs` holds `Help` (`Section(SheetSection)` or `Lines`) and `help_button`: a borderless
`MDI_HELP_CIRCLE_OUTLINE` button named `SheetSection::help_name` or `HELP_LINES_NAME` ("Help:
World", "Help: lines") toggling a `Popup` (320 points wide, closed by a click outside, Escape or
Close help) that holds the heading (`LINES_HELP_HEADING` for the lines), the text, "Example:" with
the example, and Close help. The popup is a `Dialog` named by `help_dialog_name` ("World help"),
apart from the button's name. Its id derives from the button's, so two "Help: lines" buttons open
two popovers. `sheet_ui` makes each pane a `Group` named "Sheet" or "Card" (`pane_group`), so
"region" never reaches the tree.

**Accessible names and the words rule** (D57, D60, D96, D112). Every chip is a button named by its
text, blanks and selects by what they want and where ("Line 2: choose the old state", "Line 2:
comparison"), every name holds its visible words, and a control with no words of its own (the help
button) is named by what it does. Sheet mode never shows a word of `FORBIDDEN_WORDS`, in the Editor
tab or in what a sheet puts in the world tabs (names, measures, buttons, the tick counter). The
controls that open or create a node graph (New node graph) and the world tabs' own controls (Seed,
Build) are exempt.

The Editor opens a file by the `kind` in its header (`open_text`, through `model_text::read_header`,
D55): `sheet` through `ParsedSheet::parse` alone into `EditorPanel::load_sheet`, so a sheet with
checker errors opens and shows them, `graph` through `GraphFile::parse` onto the canvas, and any
other text in the code view. A sheet or a graph that does not read opens in the code view. For a
sheet the reader's problem is set as `ApplyOutcome::Unread` ("Model file has problems. Fix them,
then press Apply."), so the list under the code names it at once without reading as a refused Apply.
`load_sheet` clears the outcome, the applied source and the preview, as `load_graph` does. The list
under the panes (`sheet_outcome_ui`) shows the preview's state (`preview_ui` with `plays` false, so
a running preview reads `preview_running_text`'s "Preview at 256 × 256 cells starts at tick 0. Press
Play to run it, or Apply to run the model at its own size."), the last Apply's outcome with any
problem the check does not list, then every diagnostic of the report the card does not draw and
every lowering problem the report lacks, errors in the theme's error colour and warnings in its warn
colour, as `<line>:<column> <message>`. A problem of a line the card shows sits under it on the
card. `save_name` suggests `<sheet id>.toml` for a sheet with no file name, errors or not. Apply is
unchanged: `ModelFile::parse(...).and_then(|file| file.entry())` installs the lowered graph's entry,
whose source records the sheet's canonical text as its author text, and `edit_model` reopens an
applied sheet's entry as that sheet unless the Editor applied it. A note edit leaves the source
equal (D51): Apply reads `Unchanged`, rebuilds nothing, and replaces the set's entry through
`install_models` when `same_author` fails, so a sweep started next records the new text
(`a_note_edit_does_not_reinstall_the_model`).

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
`EditorPanel::new_model` while open: the `StarterChoice`s (`starter_choices`: every sheet starter,
grid and agent alike (D64, D67), with its file name, the model name read from it, and its
description from `STARTER_CHOICES` by that name), the chosen index, the Name text, `name_typed` and
`made_of`. `choose` sets the index and, until the
author types in Name, the name to the starter's. "Untitled model" is the default name of Blank, the
first choice. `open_new_model` opens it from the Editor header's New model, the File menu's first
entry and the Model tab's New model. `lib.rs` calls `new_model_modal` every frame beside the other
modals: an `egui::Modal` (`henad_new_model_modal`) whose content `Ui` takes `Role::Dialog` and the
label "New model" through `accesskit_node_builder`, the heading `NEW_MODEL`, `START_FROM` with one
`radio_value` per starter and its description indented under it, a Name `TextEdit` labelled by its
label, Cancel and Create (`filled_button`, blue 700). Enter in Name creates, as Create does, and
Cancel, Escape or a click on the backdrop closes it. `create_model(app, file_name, name)` reads the
starter, runs `SheetEdit::SetSheetName` when the name differs (an empty name is `UNTITLED_MODEL`,
and a refused one keeps the starter's name with the reason as the file status), loads the sheet with
no file name and brings the Editor to the front. The world question of the final dialog shows only
under Blank, and Blank is offered first (S16a). `starter_choices` puts Blank first (`file_name`
`None`, "Untitled model" as its name), and the world question (`WORLD_QUESTION`, `CELLS_ON_A_GRID`
or `AGENTS_IN_OPEN_SPACE` into `made_of`) shows under Blank while it is chosen. `create_blank_model`
loads `blank_model(name)`: 100 by 100 cells that wrap, the id minted from the name (or
`untitled_model`), no property, parameter or card. `create_blank_agent_model` loads
`blank_agent_model(name)`: the same world made of agents, one kind `agent` named `BLANK_AGENT_NAME`
with 1000 agents and room for a million, a Position that starts anywhere at random (through
`SheetBuilder`, so its start takes a key) and a Heading.

`src/tests/sheet_editor.rs` holds
`every_sheet_starter_opens_as_a_sheet_previews_at_tick_zero_plays_and_applies` (each starter waits
at tick 0 in the preview run of its world, `PreviewRun::Grid` or `PreviewRun::Agents`, plays past
it, applies and builds), `the_new_model_dialog_offers_every_starter_with_its_description`,
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
`the_sheet_preview_line_names_play_and_the_graph_line_is_unchanged`,
`a_skipped_line_shows_its_note_under_it`, `a_checker_error_shows_at_its_line` (once, under its
line), `a_result_used_above_its_line_is_red_with_its_tooltip`,
`chips_name_the_property_when_two_share_a_state_name` ("Status Alive", swatch colours),
`otherwise_carries_its_partner_as_a_description` (forest fire line 3),
`every_line_of_the_life_starter_reads_its_sentence_by_accessible_name` (the four lines, the pane and
list groups, chips, selects and help buttons), `a_help_popover_opens_and_closes`,
`the_sheet_editor_never_shows_a_forbidden_word` (every starter, every help popover, a skipped and a
red line, the New model dialog, D96's control exempt),
`every_accessible_name_holds_its_visible_words` (D112, each control's name in the tree and holding
its words, selects aside), `the_world_tabs_show_no_forbidden_word_from_a_sheet`, and over a
`Harness` that draws the File menu, the Editor tab, the dialog and the About modal with AccessKit
on:
`new_model_opens_a_dialog_and_create_opens_the_chosen_sheet` (the dialog role, the three radios in
order, Name following the starter, a typed name kept, Create, the three lines, Cancel and Escape),
`enter_in_the_name_field_creates_the_model` and
`new_node_graph_lists_the_six_graph_starters_from_the_header_and_the_file_menu` (a submenu's label
ends with " ⏵", which the harness strips). `the_empty_editor_names_every_way_in` holds
`EMPTY_EDITOR`. The `Harness` (`Node`, `top`, `accessible_texts`, `open_sheet`, `window_focused`),
`forbidden_words`, `tick_zero_stat` and `wait_for_snapshot` sit in `src/tests/support.rs` for both
files.
`src/tests/sheet_pane.rs` holds `a_blank_model_builds_the_life_start_by_accessible_names` (New
model, Blank, Status with Dead and Alive, By chance, "Use a parameter", New parameter Initial
density at 0.3, then a count of Alive added by command reads a tick-0 share of 0.3 +/- 0.02 on 100
by 100), `a_blank_world_shows_nothing_until_a_property_is_added`,
`after_play_a_start_edit_waits_and_a_size_edit_restarts_with_the_notice` (a parameter made from a
start's field waits too), `the_world_takes_the_engine_grid_limit_and_refuses_past_it` (a fraction
and a percent refused, also Edges and the Time line),
`a_refused_text_stays_with_the_sheet_and_the_row_it_was_typed_in` (a refused width or chance does
not reach a new Blank, and a row that moves up shows its own chance),
`a_field_keeps_typed_text_while_the_window_lacks_the_system_focus` (`Harness::window_focused`
false), `a_starts_as_chance_takes_a_percent_and_refuses_what_is_no_chance` (and a Number start's
value and random number), `editing_a_property_renames_recolors_and_changes_its_kind`,
`the_property_form_checks_in_the_order_of_the_page`,
`the_new_parameter_dialog_starts_from_the_values_of_the_page` and
`the_edited_sections_show_no_forbidden_word_in_any_state` (at least 25 states, with the
label-in-name check of D112), and from S16b
`a_blank_model_builds_the_life_sheet_and_undo_walks_back_to_blank` (New model named Game of Life,
1024 by 1024, Status with Alive picked Green, Initial density, Plot Alive, colour by Status, the
starter's purpose, Randomise and Clear, then `align_with_starter` against `without_card_lines` of
the starter equal in canonical text, Undo through each step to Blank and Redo back, the same walk
with Cmd+Z and Shift+Cmd+Z, Cmd+Y, the About modal and a focused field keeping the keys),
`aligning_a_starter_with_itself_changes_nothing_and_restores_a_changed_key` (Game of Life's cells,
then the ants' agent and ground starts),
`parameters_edit_on_the_sheet_by_the_rules_of_the_page`,
`a_parameter_edit_after_play_restarts_at_tick_zero_with_the_notice` (a purpose, its Undo and its
Redo do not),
`measures_count_states_average_numbers_and_follow_their_state_colour` (the average published by the
preview), `a_number_property_shades_the_cells_in_a_hue_between_two_values` (equal ends refused in
the field, reversed ends kept),
`buttons_start_again_or_run_once_and_check_their_names`, `the_model_name_renames_from_the_toolbar`,
`the_charts_tab_says_how_to_plot_while_the_sheet_has_no_measure`,
`the_remaining_sections_show_no_forbidden_word_in_any_state` (at least 25 states) and
`keep_on_the_parameters_tab_is_one_step_of_undo`. `Harness::screen` (1600 by 1000, `TALL_SCREEN`
for a long sheet), `align_with_starter` (keys of drawing lines and starts, a parameter's `step` and
`means`, a state's or measure's colour the palette names alike, no id) and `without_card_lines`
(card lines, the cards that run each tick and `next_line`) sit in `src/tests/support.rs`. Keep
them. The alignment covers the properties of every thing and copies no parameter's type. The
random walk's rebuild, which has no card lines, sets Steps per tick to Number in the expected
document. `src/tests/sheet_agents.rs` (S26), on the helpers
`sheet_pane.rs` shares as `pub(super)`, holds
`a_blank_agent_model_builds_the_random_walk_sheet_by_accessible_names` (New model, Agents in open
space, 1000 by 1000, Walker, 10000, two parameters, Average of Position, Colour agents by Heading,
the purpose, then equal to `without_card_lines` of the starter after `align_with_starter`),
`the_agents_take_a_name_and_an_integer_up_to_the_most_the_world_holds`,
`agents_take_states_numbers_and_yes_or_no_and_no_start_on_an_unpainted_ground` (the kinds
offered, starts, no on-ground start on a ground made on the pane, a count of agents, the buttons),
`a_ground_property_adds_up_its_marks_fades_and_blocks_agents` (only States and Number for the
ground, `marks = "add_up"` lowering to `combine = "sum"`),
`events_are_added_renamed_and_deleted_on_the_sheet`,
`the_ant_starter_shows_its_ground_and_starts_ants_on_single_painted_cells` (the painted entry,
only Food and Nest offered, the Food start lowering) and
`the_agent_sections_show_no_forbidden_word_in_any_state` (at least 25 states, the agent starters,
and the label-in-name check). `the_sheet_editor_never_shows_a_forbidden_word` opens the help of
each section `SheetSection::shown` lists for the starter's world.
