# henad-app: the Card pane

`ui/editor/sheet/card_pane.rs` and `picker.rs` draw the cards of a sheet and write their lines
(S19). The rest of the sheet editor, the Sheet pane, the toolbar and the New model dialog are in
`agents/henad-app-sheet.md`. The card writes the lines of a card over `cells`. An agent card stays
read-only until S28, with no add button, no line menu and no picker.

**Drawing.** `card_pane(ui, sheet, past_start)` draws every card in file order:
`Sentences::card_heading`, the line count (`line_count`) and, on a card that runs every tick, the
Lines help and the law (`LAW`) in a `GRAY_900` box, then the lines in a group of role `List` named
`line_list_name` ("Lines", "Lines of Clear"), then the add buttons. A world of cells with no cells
card that runs each tick shows `card_header("cell")`, `BLANK_MODEL_CARD` and the add buttons first,
and its first line adds the card (`AddCard`, then `AddLine`, one edit). `card_lines(document,
sentences, report, card_index)` returns a `CardLine` per line in display order (`CardRelations`):
id, number, depth (indent 20 points), `prefix` (`line_prefix`, "Line 2" or "Clear line 1"), `word`
(`line_word`, for tool names), the plain `sentence`, its `parts` and their `places` from
`Sentences::placed_parts` with `otherwise_numbers`, the ids and tooltip of its "otherwise",
`red_chips` (a result chip whose line sits at or below this one, or whose direct When does not hold
this line), `swatches`, `notes` (the report's diagnostics at that card and line, the skip notes
first, red for an error or a skip note that names a red chip, amber otherwise) and `skips`, the
number of skip notes. `sheet_outcome_ui` in `editor/mod.rs` lists every problem the card does not
draw (`draws_diagnostic`). `line_ui` draws a row in a `horizontal_wrapped`, a group of role `Group`
named `CardLine::label` with bounds from `accessible_group`, then the line's menu, the rename field
and the notes. `card_notes` adds to the report's notes, in the order of the wording table: the
one-side offer after the skip notes, then `count_used_as_draw` with its button
(`rates::count_as_draw`, `UseCountForEach`, after which the emptied chance or rate opens its
picker), then the grey `rates::chance_note`. A note's button runs one edit.

**Controls.** `part_ui` draws each part. Words are labels, a `ConditionWord` bold with no click, and
after the first condition word of a Become or Set the condition arrow (`condition_arrow_ui`, a
chevron named `condition_arrow_name`, tooltip `CONDITION_ARROW_TOOLTIP`, D104) that opens "Choose
when". `From` carries `FROM_TOOLTIP`. A chip (`chip_with_id`), a blank (`blank_ui`), a select
(`select_ui`, a chevron box named `LineSelect::name`, its words as value), `Always` (a grey button
named `always_name`), `ForEach` (the words "for each", named `for_each_change_name`) and
`ForEachBlank` (named `for_each_optional_name`, painted only on a hovered or focused row, D99) each
take the widget id `CardState::control_id(line, target, tag)`, so another frame can open or focus
them. After the last item of a list, `add_value_ui` draws "+" named `add_value_name`, which opens
the list's picker for one item past the end. `CardLine::control` returns each control's visible
words and name, which the label-in-name tests read. Hovering or focusing a result chip or
"otherwise" highlights the lines it names on the next frame.

**Pickers** (`picker.rs`). `control_picker` opens a control's picker or select when it is clicked or
due to open (`CardState::open_next`), and takes the focus for `CardState::focus_next`. A picker's
open state lives in egui memory under the control's id with "picker". `CardPicker::show` opens on a
click of its anchor and stays open on a second one, closes on a choice, on Escape (the focus back on
the anchor) or on a click outside both the picker and the anchor, and scrolls past `LONG_PICKER`
items (a short picker takes its own height, which a scroll area lags behind). Each item is a
frameless button named by its label alone, or by `name` when that differs (the "otherwise" item),
with grey `detail` under it. `picker_for(context, want, property, filled, typed, draw)` builds the
picker of a place from its `Blank`: states (`shared_state_name` when two properties share a name),
"any state" first for an old state, the States or Number properties, or a number's groups
(`number_picker`: "From lines above" with "line N: [sentence]" under each result, "This cell's
properties" as "my Food" with "this cell's Food", "Parameters" with value and unit, "New
parameter...", and the Number box first and focused, or last for the value left of a comparison and
a "for each" count). A chance reads in `NumberUse::Chance` and a rate in `NumberUse::Rate`, which
refuse a chance outside 0 to 1 and a negative rate in the box (D75). A typed list in a box of one
number adds `LIST_IN_ONE_NUMBER`. A list blank offers the list box (`LIST_GROUP`, `LIST_HINT`, "2,
3" in one go, `read_numbers`), a filled item `REMOVE_FROM_LIST`, a "for each" `for_each_lead` and
`REMOVE_FOR_EACH`. The Number box opens on a typed number as typed (`typed_text`, D110), and a
refused text keeps the box focused. The boxes start afresh each time the picker opens.
`LineContext::results_above` offers the results of lines above, a result inside a When only to the
lines after it inside that When (D84). `choose_when(context, otherwise)` lists the yes or no results
as "when [name]", then "Conditions" or "Or a new condition" with `ConditionItem`s and their grey
lines, "otherwise" in place of "always" when `otherwise_if_always` finds a higher line (the line's
condition set to always on a copy of the sheet), and no "at rate", since the card writes no rate
line. Selects list `comparison_options` (the list words of a States property), `around_options`,
`summary_options` and `operation_options`.

**Edits.** A choice becomes a `CardEdit`, a boxed closure of `SheetEdit`s run after the card is
drawn through `SheetEditor::edit`, one undo step, with `After` for the focus. `apply_choice`
(`picker.rs`) maps a `Choice` at a place to commands: `SetSlot` for a slot (a Count's state also
sets `whose.left`, a new property clears a state of another one, and a state in a slot that wants
`Blank::StateList` starts a list), the whole list for a list item (an emptied list turns blank),
`ChooseCondition` for a form, and `SetField` for a field (an old state of another property also sets
the line's property). A Calculate line of one operation keeps the values chosen while its formula is
incomplete in `CardState::drafts` (`CalcDraft`, `+` until another operation is chosen), shown as
chips, and writes the formula with `Formula` once both values are chosen (`calculate_edit`,
`value_token`). A percent operand is written as its decimal and reads as one. Only the line's own
values and operation take a choice: a formula inside a slot has no places, and the guards in
`field_choice`, `select_edit` and `draft_value` hold an empty phrase path. "New parameter..." opens
the New parameter dialog for the place (a chance's or a rate's values, the rate's "per [unit]" as
its Unit), drawn from the next frame, since the click that chose it would land on its backdrop.
Create runs `AddParameter` and the place's commands as one edit. The add buttons (`add_buttons_ui`)
are "+ Look around", "+ Chance", "+ Yes or no", "+ Calculate" and "+ Change", or "+ Change" alone on
a card that runs once, each a `Popup::menu` with the heading and items of `AddButton`, the rate
items left out. The pattern under an item is painted into `allocate_space`, so it stays out of the
accessibility tree. `new_line` gives each item's line with its blanks empty: a Count's `whose` a
comparison "is", a Total or average one summary "total", a change from "+ Change" an empty
condition, from "+ Chance" a chance phrase. The line menu (`line_menu_ui`, "Line 2 actions", always
drawn, D58) holds `rename_line` on a result line and `delete_line`. Rename opens a field named
`rename_field_name` under the line: Enter or leaving it keeps (`RenameResult`), Escape cancels, and
a taken name shows `result_name_taken` with the field focused again.

**Focus** (F5, F7, C13, D103). After `AddLine`, the new line's first blank opens. After a choice,
`next_control` finds the control after the edited place (or, for a place whose form changed, the
first control under its slot path): an empty state blank or old-state blank opens, the empty upper
number opens after the lower number, and any other control takes the focus without opening. The
`ForEachBlank` and the words "for each" are skipped. The one-side offer (`ONE_SIDED_RANGE` with
`use_instead_label`, `UseOneSide`) shows on a skipped line whose own condition is an outside test
with one side filled, unless `CardState::filling` holds the line: an edit of one side that leaves
the other empty marks it, and the mark ends when a picker of the line closes with no value, a click
or the focus lands outside the line and its pickers, both sides are filled, or Undo or Redo runs
(`CardState::close`). The mark lives in the pane, so a sheet opened with one side empty shows the
offer at once.

`src/tests/sheet_card.rs` holds
`a_blank_model_builds_game_of_life_with_its_lines_and_equals_the_starter` and
`a_blank_model_builds_the_chance_sir_with_its_lines_and_equals_the_sheet` (from Blank by accessible
names, then `align_with_starter` and equal canonical text and lowered code, the words rule over
every menu, picker, select and help opened, every control's name holding its visible words
through `assert_controls_hold_their_words`, and Undo back to Blank and Redo to the end for Game of
Life), `a_list_blank_takes_two_values` (with the skip note of a cleared blank),
`a_states_blank_starts_a_list_that_takes_another_state`,
`a_single_blank_given_a_list_says_choose_is_one_of`, `a_chance_of_1_5_is_refused_in_the_field`,
`chips_and_pickers_name_the_property_when_two_share_a_state_name`,
`the_line_menu_renames_a_result_and_deletes_a_line`,
`focus_moves_to_the_next_blank_and_only_state_blanks_open_by_themselves` (through
`Harness::focused`, the focused node's name),
`a_chance_result_is_named_happens_until_its_chance_names_it`,
`a_negative_rate_is_refused_in_the_field`,
`the_one_side_offer_waits_while_the_range_is_being_filled`,
`a_count_chosen_as_a_chance_offers_to_count_it_for_each` and
`changed_by_and_the_controls_of_the_rebuilt_lines_follow_the_wording_table`. The chance SIR is read
from `src/tests/sheets/sir_chance.toml`, a copy of henad-graph's test sheet, since `include_str!`
stays inside its crate.
