# henad-app: the Card pane

`ui/editor/sheet/card_pane.rs` and `picker.rs` draw the cards of a sheet and write their lines
(S19), with When blocks, moves and the notes of the relations between lines (S22). The rest of the
sheet editor, the Sheet pane, the toolbar and the New model dialog are in
`agents/henad-app-sheet.md`. The card writes the lines of a card over `cells`, rates included
(S25). An agent card stays read-only until S28, with no add button, no line menu and no picker.

**Drawing.** `card_pane(ui, sheet, past_start, trials)` draws every card in file order:
`Sentences::card_heading`, the line count (`line_count`) and, on a card that runs every tick, the
Lines help and the law (`LAW`) in a `GRAY_900` box, then the lines in a group of role `List` named
`line_list_name` ("Lines", "Lines of Clear"), then, on a card holding a When, `ADD_AT_END_NOTE`,
then the add buttons. A world of cells with no cells card that runs each tick shows
`card_header("cell")`, `BLANK_MODEL_CARD` and the add buttons first, and its first line adds the
card (`AddCard`, then `AddLine`, one edit). `card_ui` computes the card's `CardRelations::notes`
once, `runs` read from the report's skip notes, and passes them to each line. After each line,
`closing_blocks` gives the When blocks that close there (after the last line inside them, or after
an empty When), inner blocks first, and `add_inside_ui` draws each one's row, `add_inside_label`
named by its label, indented to the block's inner level. Its `Popup::menu` is a group of role
`Dialog` named `add_inside_name`, with that strong title and, for each group of `add_inside_groups`
(`AddButton::ADD_INSIDE_ORDER`, or the Change group alone on a card that runs once, which only a
written file can give a When), the weak `AddButton::group` heading and `add_items_ui`'s items, and
an item runs `AddLine` with `inside` set to the When.
`card_lines(document, sentences, report, card_index)` returns a `CardLine` per line in display order
(`CardRelations`): id, number, depth (indent 20 points), `prefix` (`line_prefix`, "Line 2" or "Clear
line 1"), `word` (`line_word`, for tool names), the plain `sentence`, its `parts` and their `places`
from `Sentences::placed_parts` with `otherwise_numbers`, the ids and tooltip of its "otherwise",
`red_chips` (a result chip whose line sits at or below this one, or whose direct When does not hold
this line), `swatches`, `otherwise_notice` (`otherwise_notice`, beside the tooltip from
`otherwise_words_of`), `notes` (the report's diagnostics at that card and line, the skip notes
first, red for an error or a skip note that names a red chip, amber otherwise) and `skips`, the
number of skip notes. `sheet_outcome_ui` in `editor/mod.rs` lists every problem the card does not
draw (`draws_diagnostic`). `line_ui` draws a row in a `horizontal_wrapped`, a group of role `Group`
named `CardLine::label` with bounds from `accessible_group`, then the line's menu, the rename field
and the notes. `card_notes` adds to the report's notes, in the order of the wording table: the
one-side offer after the skip notes, then `count_used_as_draw` with its button
(`rates::count_as_draw`, `UseCountForEach`, after which the emptied chance or rate opens its
picker), then the grey rate or chance note, then the grow offer (both in **Rates** below), then the
line's relation notes (`relation_line_note`):
grey, or amber with a button for the fold offers, `REMOVE_THIS_WHEN` named `remove_when_name`
running `DeleteLine` on the When, and `move_into_label` named `move_into_name` running
`MoveConditionInto`. Both names are the When's line prefix and the button's words ("Line 2: remove
this When and keep the lines inside", "Line 4: move it into line 5"), since every accessible name
holds its visible words (D112). A note's button runs one edit.

**The "otherwise" notice.** `cards_ui` gathers each line that reads "otherwise" on every card, and
`explain_otherwise` keeps `CardState::explained`, the ids explained this session: on the pane's
first frame every such line counts as explained (a new or opened model), and afterwards a line not
yet in the set joins it and sets `CardState::otherwise_notice` to its notice, drawn above the cards
after an information icon. The notice opens with the line prefix ("Line 6", "Clear line 2"). It is
set after the cards are drawn, so `explain_otherwise` requests another frame. The next card edit and
`CardState::close` clear the notice, and the set lives as long as the pane state, one per sheet
opened.

**Controls.** `part_ui` draws each part. Words are labels, a `ConditionWord` bold with no click, and
after the first condition word of a Become, Set or When (`takes_condition`) the condition arrow
(`condition_arrow_ui`, a chevron named `condition_arrow_name`, tooltip `CONDITION_ARROW_TOOLTIP`,
D104) that opens "Choose when". `From` carries `FROM_TOOLTIP`. A chip (`chip_with_id`), a blank
(`blank_ui`), a select (`select_ui`, a chevron box named `LineSelect::name`, its words as value),
`Always` (a grey button named `always_name`), `ForEach` (the words "for each", named
`for_each_change_name`) and `ForEachBlank` (named `for_each_optional_name`, painted only on a
hovered or focused row, D99) each take the widget id `CardState::control_id(line, target, tag)`, so
another frame can open or focus them. After the last item of a list, `add_value_ui` draws "+" named
`add_value_name`, which opens the list's picker for one item past the end. `CardLine::control`
returns each control's visible words and name, which the label-in-name tests read. Hovering or
focusing a result chip or "otherwise" highlights the lines it names on the next frame.

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
condition set to always on a copy of the sheet), and "at rate" only on a card that runs each tick
(`offers_condition`), since a button's card has no duration and takes chances only. On a When line
the items read "When ..." (`ConditionItem::label(true)`, and a result item
`result_condition_item(name, true)`), and "always" is left out. A line inside a When takes the lead
`when_lead` with the When's number.
Selects list `comparison_options` (the list words of a States property), `around_options`,
`summary_options`, `operation_options` and `unit_options` ("second" to "year", `SelectOption::Unit`
writing `LineField::RateUnit`).

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
are `AddButton::ALL`, "+ When, for several lines" last, or "+ Change" alone on a card that runs
once, each a `Popup::menu` with the heading and items of `AddButton`. The
pattern under an item is painted into `allocate_space`, so it stays out of the accessibility tree.
`new_line` gives each item's line with its blanks empty: a Count's `whose` a comparison "is", a
Total or average one summary "total", a change from "+ Change" an empty condition, from "+ Chance" a
chance or a rate phrase. The line menu (`line_menu_ui`, "Line 2 actions", always drawn, D58) holds
`rename_line` on a result line, `move_line_up` and `move_line_down` (`MoveLine`, disabled where
`can_move` finds the outline's edge: only the card's first line stays at the top, and only a
top-level line with no top-level line after it at the bottom), `DRAW_NEW_NUMBERS` on a line
`check::holds_draw` finds drawing (`DrawNewNumbers`), `CHANGE_TO_RATE` or `CHANGE_TO_CHANCE` where
`rates::conversion` finds one (`ChangeToRate`, `ChangeToChance`), and `delete_line`, with
`DELETE_WHEN_TOOLTIP`
on a When. Rename opens a field named `rename_field_name` under the line: Enter or leaving it keeps
(`RenameResult`), Escape cancels, and a taken name shows `result_name_taken` with the field focused
again.

**Rates** (S25). The notes read `CardDrawing::values`: `rates::parameter_defaults` with each
`SheetTrial` of the Params tab in its place while a run of the sheet is loaded (D62). After the
count-as-draw offer, `card_notes` adds `rates::rate_note` in grey with its `mean_stay` as the
label's hover text and AccessKit description, and its `long_tick` as a second grey note
(`long_tick_note`, "Up to 0.909 with 8. This tick is long ..."), or `rates::chance_note`, then the
amber `rate_grow_prompt` when `rates::grow_count` finds a count, with `add_for_each_label` named
`add_for_each_name` running `SetSlot` of the count's result in `tries_path` (`tries` or
`condition.tries`). The unit mismatch is the checker's warning among the report's notes. A picker of
a rate leaves out a parameter whose Unit is set and names no unit of time, and a picker of a chance
a rate parameter (`LineContext::parameters` with `rates::parameter_rate_unit`). `run_card_edit` sets
`CardState::tick_notice` to `tick_set_notice` when the edit gave a sheet with no `[time]` one (a
unit chosen or a rate parameter put in), drawn above the cards with the "otherwise" notice and
cleared by the next card edit or by `CardState::drop_tick_notice`, which the Sheet pane calls when
its edit changes the length or unit of one tick. `card_lines` gives each lower line of a
`competing_groups` group with no "otherwise" the parts of `competing_parts` ("or",
`SentencePart::Competing`, drawn by `lead_word_ui` as "otherwise" is), `CardLine::competing` (the
group's other ids, highlighted on hover or focus) and `competing_tooltip` with the group's old
state. `card_ui` collects each row's rect from `line_ui` and `bracket_ui` paints a gray 500 bracket
for each group in the pane's margin, `BRACKET_OFFSET` left of the rows, where no wrapped text
reaches (`brackets`, `competing_brackets` in tests). The group note sits under the highest line,
where `rate_note` puts it. The Sheet pane's Time notice is in `agents/henad-app-sheet.md`.

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

**The Diagram** (`ui/editor/sheet/diagram.rs`, D59, D95). The toggle `DIAGRAM` beside the Card
heading (tooltip `DIAGRAM_TOOLTIP`) sets `SheetEditor::diagram` and closes the card's pickers, and
the pane then draws `diagram_ui` in place of `card_pane`: `DIAGRAM_NOTE` and `EDIT_AS_NODE_GRAPH`
(disabled with `ConvertRefusal::tooltip`, `agents/henad-app-editor.md`), then each card's heading
and line count and its drawing in a horizontal scroll area. `diagram_boxes` returns a `DiagramBox`
per line from `card_lines` and `CardRelations`: prefix, number, sentence, the result it works out,
the results of lines on the card it reads (from its `ChipKind::Result` chips, once each), its When
and whether it is one. A box's column is one past every box it reads and its When (`columns`), and
each box takes a row of its own in display order, so every arrow runs right and down. A frame
"Inside line N" (`inside_line_frame`) surrounds the boxes inside each When, nested Whens included,
built innermost first as the union of its boxes and inner frames, with a connector from the When's
box. The drawing is painted with the painter and holds no widget. It is one accessible node of role
`Image` named `diagram_name` ("Diagram of the card's 3 lines, read-only"). Colours: boxes gray 900
with a gray 600 stroke and a gray 800 header, results and arrows blue, frames dashed gray 500.

`src/tests/sheet_card.rs` holds
`a_blank_model_builds_game_of_life_with_its_lines_and_equals_the_starter` and
`a_blank_model_builds_the_chance_sir_with_its_lines_and_equals_the_sheet` (from Blank by accessible
names, then `align_with_starter` and equal canonical text and lowered code, the words rule over
every menu, picker, select and help opened, every control's name holding its visible words through
`assert_controls_hold_their_words`, which also reads the note buttons and add rows of
`card_buttons`, and Undo back to Blank and Redo to the end for Game of Life),
`a_list_blank_takes_two_values` (with the skip note of a cleared blank),
`a_states_blank_starts_a_list_that_takes_another_state`,
`a_single_blank_given_a_list_says_choose_is_one_of`, `a_chance_of_1_5_is_refused_in_the_field`,
`chips_and_pickers_name_the_property_when_two_share_a_state_name`,
`the_line_menu_renames_a_result_and_deletes_a_line`,
`focus_moves_to_the_next_blank_and_only_state_blanks_open_by_themselves` (through
`Harness::focused`, the focused node's name),
`a_chance_result_is_named_happens_until_its_chance_names_it`,
`a_negative_rate_is_refused_in_the_field`,
`the_one_side_offer_waits_while_the_range_is_being_filled`,
`a_count_chosen_as_a_chance_offers_to_count_it_for_each`,
`changed_by_and_the_controls_of_the_rebuilt_lines_follow_the_wording_table`,
`a_blank_model_builds_the_forest_fire_with_otherwise_and_equals_the_starter` (the sheet through
`build_forest_fire_sheet`, line 3's "otherwise" notice shown once, Draw new numbers, alignment and
the words rule), `the_run_once_condition_picker_offers_no_rate`,
`a_nested_when_stores_its_parent_and_deleting_the_outer_when_keeps_its_lines` (two Whens through "+
When, for several lines" and "+ Add inside line 4", a Become inside starting always from Dead, the
inside notes and the lead, depths 0, 1 and 2, Delete and Undo),
`a_when_that_repeats_its_lines_offers_to_remove_itself` and
`moving_a_line_above_its_result_skips_it_and_moving_it_back_runs_it`. The chance SIR is read from
`src/tests/sheets/sir_chance.toml`, a copy of henad-graph's test sheet, since `include_str!` stays
inside its crate.

`src/tests/sheet_rates.rs` (S25, helpers shared from `sheet_card.rs` as `pub(super)`) holds
`a_blank_model_builds_the_rates_sir_with_its_lines_and_equals_the_starter` (the time set on the
sheet, both rates through "New parameter..." in "per day", line 2's "for each" from the grow offer,
New outbreak's "Choose when" with no rate, then alignment, label-in-name and the words rule),
`the_run_once_card_offers_no_rate_after_rates_arrive` (the SIR notes, the mean stay, "Change to a
chance" on a typed rate, none on New outbreak's typed chance),
`the_sird_lines_3_and_4_read_or_with_one_bracket_and_one_group_note` (from
`src/tests/sheets/sird.toml`, a copy of henad-graph's),
`change_to_a_rate_and_back_keeps_the_chance_per_tick` (with the Time notice),
`the_long_tick_note_shows_at_one_day_and_leaves_at_a_quarter_day` and
`removing_the_time_unit_marks_the_rate_lines_and_a_rate_unit_sets_it_again` (the tick notice and the
mismatch warning). A label's AccessKit node holds a `TextRun` child with its text, so the tests
count nodes other than text runs.
