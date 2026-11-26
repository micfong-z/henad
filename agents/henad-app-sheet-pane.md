# henad-app: the Sheet pane's layout and a world of agents

`ui/editor/sheet/sheet_pane/layout.rs` lays the Sheet pane out, and `sheet_pane/agents.rs` draws the
sections of a world of agents. The rest of the Sheet pane, what each section edits and how, is in
`agents/henad-app-sheet.md`, and the look of each component in `agents/ui.md`.

**Sections.** `section(ui, title, help, add)` draws on the pane's own ground with no frame, from
the pane's left edge to `SECTION_END` 10 short of its right (the help clear of the divider's grab),
`region_title` (the heading as a
`title_text`, a hairline to the help's mark at the right edge), then `add`, its rows 4
apart. `sheet_pane` puts `REGION_GAP` 12 between sections. A section's "+ Add ..." is its last row,
the `widgets::add_row_button` with no kind, except "+ Add property" while the sheet holds no
property, which stays the primary `filled_button`. A form inside a section (property, measure,
button, event) is `form_heading` in the semibold face, `Form` rows and `form_buttons` (Done, Cancel,
then the property form's Delete, returning the presses), with no box of its own. The property,
button and event forms draw their Name field with `form_name_field` (at most `NAME_FIELD_WIDTH` 200,
focused once on opening, Enter as Done for a button or an event), and every name check of the pane
and the New parameter dialog goes through `widgets::name_taken`, case aside.

**Form rows.** `Form::new(ui, labels)` makes a label column as wide as the widest label, at most 45%
of the section, and `row` puts a label in it and the fields in the next column on wrapping lines of
`LINE` 22, `row_with` takes a label drawn by a closure (a swatch and a name, a label with a
tooltip), and `under` draws in the field column with nothing in the label column: a hint, or the
red message of a refused field under the field. `wider` widens the column for labels with a
swatch. Number fields are `commit_number`, right-aligned. `line` and `wrapped_line` draw a line of
`LINE` with its items centred, and `text_cell` a label centred on the first line.

**Tables.** `Table::new(id, columns, grow, give_up)` takes `Column`s (a header, the natural width,
the least width; `Column::fixed` for a delete column), the column that takes the room left over,
and the give-up steps `(column, floor)` run in order while the columns do not fit, none taking a
column below its least width. A table still too wide after every step scrolls sideways in a
`ScrollArea` keyed by `id`, its bar always shown. `show` draws the header row (a `caption` per
header, running over the columns after it that have none, then a hairline) and the rows through
`TableRows::row(ui, key, add)`: each cell is a child `Ui` keyed by the row's key and the column
(`TableRow::cell`, `cells`), top-aligned, and `TableRow::below` adds a line under the row from a
column to the right edge (the trying line, a refused field's message). A row ends in a gray 800
hairline and takes the theme's hover fill under the pointer. `TableRow::message` reads a field's
refusal. A delete button is `delete_button`, a "×" `text_control`, right-aligned in the last
column by `TableRow::delete_cell`, which returns the click. The × of a parameter or a property and
the property form's Delete go through `delete_or_ask`: a delete that `DeleteQuestion::before` finds
emptying a formula the editor cannot write again (henad-graph's `lines_losing_formulas` and
`measures_losing_formulas`) waits in `PaneState::delete_question` (a `PendingDelete` with its form),
and `delete_question_ui` draws the dialog from the next frame on, past the click
(`delete_question.rs`: `DELETE_PARAMETER_HEADING` or `DELETE_PROPERTY_HEADING`, `sentence::used_by`
naming each line by its number, or "Clear line 1" on a button's card, and each measure by its name,
then Cancel and Delete, destructive, which Enter presses). Delete pushes the edit with its form,
Cancel, Escape and a click outside drop it, and an undo closes it. Any other delete runs at once.
The deletes of an event, a measure or a button name nothing a formula reads
(`a_delete_of_an_item_that_formulas_use_asks_first`, which also holds the ants' To food, read by
the measure Total trail, and Undo after each delete). The tables and their orders:

- Properties (`property_table`, of any thing): Starts as takes the room left over. Short of room it
  gives up width down to 150, then Changed by, Kind and Name down to their longest words (Changed by
  to the word "Changed" of its header), wrapping at them, then Starts as down to
  `text_control_width` of "Use a parameter", its selects and chips ending in an ellipsis, then Kind
  down to the longest word of a state's name without its swatch (`kind_under`, the name wrapping
  under its swatch in a `horizontal_wrapped`), then Kind and Name down to 56 and 40, a word too long
  ending in an ellipsis with the tooltip. Each name follows the symbol of its type
  (`type_symbol_ui`, `agents/henad-app-picker.md`) while the symbols show. The symbols and the row
  actions (`RowActions`) never take room a word needs: `property_columns` builds the columns for a
  symbol width and a place of the actions, and the table takes the first of the Edit and × columns
  with symbols, the menu's column with symbols and the menu's column alone that fits with no step
  run and Starts as at `starts_line_width`, the width that holds each line of controls whole (a
  chance row with its ×, the otherwise row, a value's lead and numbers, `chip_width` for a
  parameter's chip; a Vector's or a painted start's sentence wraps as prose and counts nothing).
  Otherwise the menu goes into the Changed by column (`MenuUnder`), right-aligned beside its
  words where they fit with it and on a line under them otherwise, and the table lays its words out
  as it did before it had symbols or actions
  (`a_property_table_leaves_out_its_symbols_and_buttons_before_a_word_wraps`, over the SIR sheet
  at nine widths, `a_property_table_short_of_room_leaves_out_its_symbols_after_its_buttons`).
- Parameters: the symbol of its type before each name (Number or Integer), its width in the Name
  column. One line per parameter (name, value as slider and field, Min, Max, Unit, ×) when the
  section holds every column at its comfortable width without the symbols, Value taking the room
  left over and Unit, Value, Name, Min and Max giving up width in that order. Otherwise name, value
  and × on the first line, Value giving up width before Name, and Min, Max and Unit on a second
  line after their `caption`s. The symbols show only while every column of the layout fits at its
  natural width with them
  (`the_parameters_lay_out_their_rows_alike_with_and_without_symbols`). A refused field's message
  sits under its column.
- Plotted measures (headed `PLOTTED`), buttons (`NAME_LABEL`, `BUTTON_DOES`) and events (no header):
  the words or the field take the room left over and give up width first.

Every select of the pane is `select` (`select_within` at most as wide as its line, keyed by a salt,
over `select_list`), its options `OverlayItem`s with the current one
marked and a state's swatch. The kind of a States property shows "States" over each state's swatch
and name, the name wrapping under the swatch where the column has no room for both.

**A world of agents**. The pane works on a *thing*, `CELLS_ID`, `GROUND_ID` or the agent kind's id:
`properties_of`, `find_property`, `all_properties` and `over_agents` in `sheet/mod.rs`, which the
Card pane and its pickers share (`all_properties` also behind `starts_of`, so a start of any thing
waits for Start again), and
`measured_thing` (the cells, or the first kind of agent, which Measures and Buttons act on).
`property_table` draws any thing's table and `property_form_or_add` its form or "+ Add property",
named `add_property_name(kind)` under the agents and `ADD_GROUND_PROPERTY_NAME` under the ground.
`PropertyForm::thing` keeps the form under its own table. `form_kinds` offers States and Number,
Integer for cells and agents (the ground lowers only numbers and states), and for agents Boolean
(`YES_NO_KIND`) and Vector (`ARROW_KIND`, `ARROW_KIND_DESCRIPTION`), then Position or Heading while
the agents hold none, and always the property's own kind. A new property starts in its first state,
at 0, at no (`Bool(false)`), anywhere at random (a Position), in a random direction of length 1 (a
Vector, a start no control changes) or with no start (a Heading). `kind_words` and
`starts_text(document, property)` read every kind and start ("Anywhere at random",
`on_ground_start`, `PAINTED`, "All Yes", `HEADING_START`).
`start_select` offers by kind: a state or By chance (with `PAINTED` as a selected entry that
changes nothing while the start is painted), All Yes or All No, Anywhere at random or each state of
a ground States property that `StartsAs::paints_single_cells` accepts, the predicate the lowering's
`on_ground` uses, A value or, for a Number, A random number. No control paints, so a ground made
on the pane offers no on-ground start, and choosing a state or By chance replaces a painted start
until Undo. A Heading shows `HEADING_START`, an Arrow its start through `Sentences::slot`, and a
painted start `painted_shapes` with no control.
The Number start's lead is `every_starts_at(thing_word)`. `NumberTarget` (`Start` or `Kept`) is what
`number_field` (the field of a Starts as number or a share kept, taking a `NumberField` with an
optional `limit` that also refuses an empty field), `pick_number` and the New parameter dialog
fill. `changing_lines` adds Steer, Keep between, Leave on a ground property and a Move for the
card's Position and Heading.
`sheet_pane/agents.rs` holds the three sections. Agents: the name (`AGENT_NAME_FIELD`,
`rename_kind` renames and gives the kind the id its new name mints while its id is the one its old
name mints, a refused `ChangeId` leaving the old id), the number at the start (`AGENT_COUNT_NAME`)
and at most (`AGENT_MAX_NAME`, `AGENT_MAX_TOOLTIP`), each an integer with `integer_up_to`
(1 to the most, which is 1 to `MAX_AGENTS`, a smaller most lowering the count with it), then the
table and the refusal. Ground: `GROUND_EMPTY` or the table, then per Number property `marks_row`
(`MARKS_LABEL`, with `MARKS_TOOLTIP` on the label and the select, stating the bound of Add up
(`the_marks_rule_shows_the_bound_of_add_up_on_hover`), a select named `marks_name` of
`MARKS_NONE`, `MARKS_LARGEST`, `MARKS_ADD_UP`, one
`SetMarks`, a share of 1 put in when the share is blank, and while marked `KEPT_LEAD` a
`number_field` named from `kept_place` in `NumberUse::Share`, from 0 to 1 with `KEPT_OUT_OF_RANGE`
whether typed in the field or in its parameter chip's Number box, and a new parameter made there
in 0 to 1 (the checker warns at any other share or parameter range), `KEPT_TAIL`,
`a_share_kept_through_its_parameter_chip_stays_from_0_to_1`,
`a_share_kept_parameter_widened_past_1_warns`) and per States property `blocked_row`
(`blocked_lead`, a box per state named `blocked_state_name`, `SetBlocked`). Events:
`EVENTS_EMPTY`, each name a `commit_field` named `event_name_name` with the two checks, × named
`delete_event_name`, and `EventForm` (`NEW_EVENT`, `EVENT_NAME_FIELD`, Done or
Enter). Measures count agents (`COUNT_AGENTS_MEASURE`, `COUNT_AGENTS_LEAD`,
`count_agents_measure`), average a Number, Integer or Position, and count the times an event
happened (`an_event_count_is_plotted_from_the_measure_form`). Colours draw `colors_of` for
the cells (`COLOR_CELLS_BY`), or for the agents (`COLOR_AGENTS_BY`) and the ground
(`COLOR_GROUND_BY`, once it has a property), offering States, Number, Integer and Heading
(`ByHeading` with `BY_HEADING_NOTE`). The select shows what the world draws (`drawn_by`): the rule's
property, or with no rule the first States property of the cells or the agents, and otherwise
`COLOR_BY_NOTHING` ("None"). It offers None first, which takes the rule out, on the ground and on
cells or agents with no States property, where no rule draws them black, white or not at all. The
current item is the one shown, and choosing it changes nothing
(`the_colours_selects_show_and_offer_none_where_no_property_colours`,
`the_colour_of_cells_with_no_rule_shows_what_the_world_draws`). Buttons act on
`measured_thing`: Run once reads `run_once(thing_word)`, Start again offers the thing's properties
with a start and the ground's, `button_edit` keeps a button's set everywhere steps and puts its
start-again step in the old one's place, and `button_does` joins steps that set to one value
(`set_everywhere`, "Set To food and To home to 0 everywhere").

