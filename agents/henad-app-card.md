# henad-app: the Card pane

`ui/editor/sheet/card_pane.rs` and `picker.rs` draw the cards of a sheet and write their lines,
with When blocks, moves and the notes of the relations between lines. The Sheet pane and the
toolbar are in `agents/henad-app-sheet.md`, and the New model dialog in
`agents/henad-app-sheet-open.md`. The card writes the lines of a card over `cells`, rates included,
and of a card over agents (**Agent lines** below).

**Drawing.** `card_pane(ui, sheet, past_start, trials, fires)` collects every `CardEdit` of a frame
in draw order and runs each through `run_card_edit`, as the Sheet pane does, so a rename field that
loses the focus to a note's button keeps its name. It draws the notices (`notice_ui`) and every card
in file order with the body text at `SENTENCE_SIZE` 13, `REGION_GAP` 12 apart, each in
`widgets::region` (gray 900 inside a gray 800 hairline): `title_bar_ui` (`Sentences::card_heading`
in the semibold face in gray 100, the Lines help on a card that runs every tick, `line_count` at the
right as a `caption`, a hairline), on a card that runs every tick `law_ui` (`LAW` at 12 in gray 400
under the title, then a hairline), the empty slot `empty_card_ui` of an empty card (weak words in a
`slot_outline`), the lines in a group of role `List` named `line_list_name` ("Lines", "Lines of
Clear"), then `footer_ui` (a hairline, `ADD_AT_END_NOTE` on a card holding a block, the add
buttons). A world of cells with no cells card that runs each tick shows `card_header("cell")`,
`BLANK_MODEL_CARD` and the add buttons in a frame first, and its first line adds the card
(`AddCard`, then `AddLine`, one edit). `card_ui` computes the card's `CardRelations::notes` once,
`runs` read from the report's skip notes, and passes them to each line. After each line,
`closing_blocks` gives the blocks that close there (after the last line inside them, or after an
empty When or Repeat), inner blocks first, and `add_inside_ui` draws each one's row: an
`add_row_button` `add_inside_label` at the block's inner indent, a bar in the block line's hue. Its
`Popup::menu` is a `menu_dialog` (an `overlay`, a group of role `Dialog` named by its title, the
title a heading) named `add_inside_name`, with, for each group of `add_inside_groups`
(`AddButton::ADD_INSIDE_ORDER`, or the Change group alone on a card that runs once, which only a
written file can give a When), the `AddButton::group` label and `add_items_ui`'s items, and an item
runs `AddLine` with `inside` set to the When. `add_items_ui` puts the items of one add button in a
`menu_group` labelled by the node it is passed (the group label here, the title in an add button's
menu), and each later group that `opens_group` heads ("Or a Boolean that several lines share", "Or
a random value") in a `menu_group` of its own. The groups, and the items of each add button's menu,
sit in `widgets::overlay_items`, a scroll area as tall as the window leaves under the title, so the
24 items of a card over agents stay in reach of the wheel in a short window.

**Rows.** `line_ui` lays a row out as columns, each a child `Ui`: the kind bar (`KIND_BAR_WIDTH` 3,
`kind_bar_ui` over `widgets::kind_bar`, the bar and the gutter the handle that drags the line,
`drag_line_tooltip` of the group's word as their hover text, `agents/henad-app-card-drag.md`), the
gutter (`GUTTER_WIDTH` 30, `number_ui` right-aligned as a `small_label`,
`NUMBER_GAP` 9 before the body), the body (from the gutter plus `INDENT` 20 per block, to the tail),
and the tail (`fire_count_ui` in `COUNT_WIDTH` on a line of `CardDrawing::counted` on a card that
runs each tick, then the line menu in `MENU_WIDTH` 28, both at the row's top). A line without a
count gives the count column to its body. `sentence_ui` draws the sentence in rows of
`SENTENCE_ROW` 24, each `SentencePart::Alternative` starting a row of its own one indent in, with a
gray 500 dash (`ALTERNATIVE_DASH`) in the indent of its first row. The rename field and the notes go
in a child under the sentence that runs to the row's right edge. A row has `ROW_PADDING` 4 above and
below, a gray 800 hairline above it but on a card's first row, and its rect covers its notes.
`paint_row` fills the blue 950 tint of a highlighted line into a `Shape::Noop` reserved before the
row and paints a 1 point gray 800 guide per level at `GUIDE_OFFSET` 5 into each indent, so
consecutive rows join into one guide. A row whose control holds the keyboard focus (a part, the
menu, a note's button, the rename field, gathered in `LineDrawing::focused`) gets `focus_mark`
under its number. The menu (`line_menu_ui` over `widgets::menu_button`, the tail's whole rect) is
allocated after the sentence and before the notes, so the keyboard path is unchanged. `bracket_ui`
paints a competing group's bracket in the gutter between the numbers and the bodies.

`card_lines.rs` holds the line data the pane, the Diagram and the Sheet pane read, with no `Ui`
(`card_pane` re-exports what other modules import from it):
`card_lines(document, sentences, report, card_index)` returns a `CardLine` per line in display order
(`CardRelations`): id, number, depth (indent 20 points), `prefix` (`line_prefix`, "Line 2" or "Clear
line 1"), `word` (`line_word`, for tool names), the plain `sentence`, its `parts` and their `places`
from `Sentences::placed_parts` with `otherwise_numbers`, the ids and tooltip of its "otherwise",
`red_chips` (a result chip of the line itself, with `result_own_tooltip` and `USES_OWN_RESULT`, of
a line below it, or of a line whose direct When does not hold this line), `swatches`,
`otherwise_notice` (`otherwise_notice`, beside the tooltip from `otherwise_words_of`, both saying
"at the same time" when a higher line sits in the line's own
Repeat, as `CardRelations::repeat_of` finds it, and that the first time round decides when two
higher lines share another Repeat, through `sentence::OtherwiseRepeat`, and below a Move to
naming the kind's Position and saying the agent stays where it is when the cell of the line that
wins is not open, through `move_to_otherwise_tooltip` and `move_to_otherwise_notice`, tested by
`a_move_to_below_another_says_the_higher_line_wins`), `notes` (the report's
diagnostics at that card and
line, the skip notes first, red for an error or a skip note that names a red chip, amber otherwise)
and `skips`, the number of skip notes. `sheet_outcome_ui` in `editor/mod.rs` lists every problem
the card does not draw (`draws_diagnostic`). `line_ui` names the row a group of role `Group`
named `CardLine::label` with bounds from `accessible_group`, the whole row. `card_notes` adds to the
report's notes, in the order of the wording table: the one-side offer after the skip notes, then
`count_used_as_draw` with its button (`rates::count_as_draw`, `UseCountForEach`, after which the
emptied chance or rate opens its picker), then the grey rate or chance note, then the grow offer
(both in **Rates** below), then the line's relation notes (`relation_line_note`):
grey, or amber with a button for the fold offers, `REMOVE_THIS_WHEN` named `remove_when_name`
running `DeleteLine` on the When, and `move_into_label` named `move_into_name` running
`MoveConditionInto`. Both names are the When's line prefix and the button's words ("Line 2: remove
this When and keep the lines inside", "Line 4: move it into line 5"), since every accessible name
holds its visible words. A note's button runs one edit.

**The "otherwise" notice.** `cards_ui` gathers each line that reads "otherwise" on every card, and
`explain_otherwise` keeps `CardState::explained`, the ids explained this session: on the pane's
first frame every such line counts as explained (a new or opened model), and afterwards a line not
yet in the set joins it and sets `CardState::otherwise_notice` to its notice, drawn above the cards
after an information icon. The notice opens with the line prefix ("Line 6", "Clear line 2"). It is
set after the cards are drawn, so `explain_otherwise` requests another frame. The next card edit and
`CardState::close` clear the notice, and the set lives as long as the pane state, one per sheet
opened.

**Controls.** `part_ui` draws each part, the look of each control in `widgets.rs` (`agents/ui.md`
lists the colours). Plain words go through `text_ui`: closing punctuation at their start is placed
at the cursor with no wrap check, so a row never starts with ")" or ";", a space that ends a full
row takes no row, and one label keeps the words. `words_ui` (`Words`) lays a label out in rows of
`SENTENCE_ROW` and paints it level with the chips, with a painted dotted underline where
`Words::dotted` holds: plain words in the text colour, a `Keyword` and a `ConditionWord` in the
semibold face (`semibold_font` in `init.rs`, the regular face where the context has no semibold
family, as in a test) in gray 100, `From` dotted with `from_tooltip` of the card's thing word, and
"otherwise" and "or" (`lead_word_ui`) weak, italic and dotted, focusable with their tooltip. A
`ResultName` is the line's result chip (`result_name_ui`), a button named `rename_result_name`
("Line 1: rename Alive neighbours") whose press opens the rename field as the menu's Rename does.
After the first condition word of a Become, Set, Leave, Record or When (`takes_condition`) the
condition arrow follows on the word's row (`arrow_control`, `ARROW_WIDTH` with its hit area the
row's height, named `condition_arrow_name`, tooltip `CONDITION_ARROW_TOOLTIP`) and opens "Choose
when". A chip (`chip_with_id`, the open picker's look read from the popup id), a blank (`blank_ui`),
a select (`select_control`, its words and a chevron, named `LineSelect::name`, its words as value, a
`slot_outline` while empty), `Always` (a `text_control` named `always_name`), `ForEach` (the words
"for each", named `for_each_change_name`) and `ForEachBlank` (named `for_each_optional_name`, its
weak words alone until its row is hovered or focused, then a blank) each take the widget id
`CardState::control_id(line, target, tag)`, so another frame can open or focus them. The first words
of a random number, a random direction or a length a picker put in a slot
(`SentencePart::PhraseLead`) are a `text_control` named `phrase_lead_name` ("Line 6: replace a
random number between", tooltip `PHRASE_LEAD_TOOLTIP`) at the slot's place, so its picker and its
Number box put a number back in its place
(`a_random_value_returns_to_a_typed_number_from_its_first_words`). After the last item of a list,
`add_value_ui` draws "+" named `add_value_name`, which opens the list's picker for one item past the
end. `CardLine::control` returns each control's visible words and name, which the
label-in-name tests read. Hovering or focusing a result chip or "otherwise" highlights the lines it
names on the next frame. Notes are `NOTE_SIZE` 11.5 in their three colours, a note's button a
`note_button`. The add buttons (`add_buttons_ui`) are `add_row_button`s, each with a square of its
group's `kind_hue`, so they read as the legend of the bars, and each menu is a `menu_dialog` named
`AddButton::heading_for`. Every popup of the pane takes `Frame::NONE` and draws its contents in an
`overlay`: titles are headings (`overlay_title`), group labels `caption`s, a picker's or a menu's
items `menu_item`s in `menu_group`s and a select's options `overlay_item`s (a picker's current value
marked by `CardPicker::current`, the chip's name, and a state's swatch from `PickerItem::swatch`, a
select's current option by its words, its list drawn by `widgets::select_list`, which the Sheet
pane's selects share). The line menu's tools are one `menu_group` labelled by the menu's button. Its
Move up and Move down stay listed where `can_move` refuses, disabled with `MOVE_UP_FIRST` or
`MOVE_DOWN_LAST` as their tooltip. The pane's header draws Lines and Diagram as the two segments of
a toggle (`toggle_label`), each egui's selectable button on the keyboard path that Space or Enter
presses (`both_segments_of_the_card_toggle_take_the_focus_and_a_key`).

**Pickers** (`picker.rs`) are in `agents/henad-app-picker.md`: what each blank offers, the values
a place refuses, shown greyed with the checker's reason, and the symbols of types.

**Edits.** A choice becomes a `CardEdit`, a boxed closure of `SheetEdit`s run after the card is
drawn through `SheetEditor::edit`, one undo step, with `After` for the focus. The closure returns
the ids made and the draft it leaves, which `CardState::keep_draft` stores once the edit is kept.
`apply_choice` (`picker.rs`) maps a `Choice` at a place to commands: `SetSlot` for a slot (a
Count's state also sets `whose.left`, a new property clears a state of another one, the property
looked up among every kind of thing's (`find_property`), and a state in a
slot that wants `Blank::StateList` starts a list), the whole list for a list item (an emptied list
turns blank), `ChooseCondition` for a form, and `SetField` for a field (an old state of another
property also sets the line's property). A Calculate line of one operation keeps the values chosen
while its formula is incomplete in `CardState::drafts` (`CalcDraft`, `+` until another operation is
chosen), shown as chips, and writes the formula with `Formula` once both values are chosen
(`calculate_edit`, `value_token`: a part of a result as `l1.centre`, a property of a cell picked
above as `l3.food`, "my new" as `my_new.velocity`, held by
`a_calculate_line_takes_the_parts_of_the_lines_above`). A percent operand is written as its decimal
and reads as one. Only the line's own values and operation take a choice: a formula inside a slot
has no places, and
the guards in `field_choice`, `select_edit` and `draft_value` hold an empty phrase path. A chip or
blank with no place, a value of a formula of more than one operation or of one inside a slot, takes
no widget id: `widgets::chip_within` and `blank_ui` allocate it with `Sense::hover()` alone, keep
its idle look under the pointer, name it as a `Label` and take no keyboard focus, and `part_ui`
gives it `FORMULA_PART_TOOLTIP` under the pointer, after a red chip's own tooltip. The operation of
a formula inside a slot is a select with no place and reads as plain words, as the operations of a
longer formula do. `CardLine::controls` leaves such parts out
(`a_value_inside_a_formula_opens_nothing_and_says_how_to_change_it`, the ants' Trail falloff). "New
parameter…" opens the New parameter dialog for the place (a chance's or a rate's values, the
rate's "per [unit]" as its Unit), drawn from the next frame, since the click that chose it would
land on its backdrop. Create runs `NewParameter::add_edit` and the place's commands as one edit.
The add buttons (`add_buttons_ui`) are `AddButton::ALL`, "+ When, for several lines" last, or "+
Change" alone on a card that runs once, each a `Popup::menu` with the heading and items of
`AddButton`. The pattern under an item is painted into `allocate_space`, so it stays out of the
accessibility tree.
`new_line` gives each item's line with its blanks empty: a Count's `whose` a comparison "is", a
Total or average one summary "total", a change from "+ Change" an empty condition, from "+ Chance" a
chance or a rate phrase. The line menu (`line_menu_ui`, "Line 2 actions", always drawn) holds
`rename_line` on a result line, `add_note_tool` or `edit_note_tool` (`CardState::noting`: a
multiline field named `note_field_name` in the note's place under the line, hint `NOTE_FIELD_HINT`,
Enter or leaving it keeps, Escape cancels, empty removes the note, and `keep_note` runs `SetNote`
after the card's edits as a `starts_only` edit, so a played preview runs on), `add_line_above` and
`add_line_below` (`CardState::inserting`: `insert_menu_ui` under the menu's button, a `menu_dialog`
titled `add_above_name` or `add_below_name` of `add_inside_groups` through `add_items_ui`, greying
each item `add_refusal` refuses, inside a Repeat around the line too, whose item runs `InsertLine`
and opens the new line's first blank), `duplicate_line` (`DuplicateLine`, `DUPLICATE_BLOCK_TOOLTIP` on a When or a Repeat),
`move_line_up` and `move_line_down` (`MoveLine`, disabled with
`MOVE_UP_FIRST` or `MOVE_DOWN_LAST` as tooltip where `can_move` finds the outline's edge: only the
card's first line stays at the top, and only a top-level line with no top-level line after it at
the bottom), `DRAW_NEW_NUMBERS` on a line
`check::holds_draw` finds drawing (`DrawNewNumbers`), `CHANGE_TO_RATE` or `CHANGE_TO_CHANCE` where
`rates::conversion` finds one (`ChangeToRate`, `ChangeToChance`), and `delete_line`, with
`DELETE_WHEN_TOOLTIP` on a When (a delete that takes away parts of the sheet besides the line, as
a line that reads its result, waits in `CardState::delete_question` on the dialog titled
`DELETE_LINE_HEADING`, drawn by `card_pane` from the next frame on, as the Sheet pane's in
`henad-app-sheet-pane.md`, and names the line as a chip of its result reads: its name, an unnamed
Count's automatic name, else its line word, "line 2"),
and on a look at agents `include_me_tool` or `leave_me_out_tool`
(`SetField` `IncludeMe`). Rename shows on a line of a result (`LineKind::works_out_result`). Rename
opens a field named `rename_field_name` under the line: Enter or leaving it keeps (`RenameResult`),
Escape cancels, and a taken name shows `result_name_taken` with the field focused again.

**Rates**. The notes read `CardDrawing::values`: `rates::parameter_defaults` with each
`SheetTrial` of the Parameters tab in its place while a run of the sheet is loaded. After the
count-as-draw offer, `card_notes` adds `rates::rate_note` in grey with its `mean_stay` as the
label's hover text and AccessKit description, and its `long_tick` as a second grey note
(`long_tick_note`, "Up to 0.909 with 8. This tick is long ..."), or `rates::chance_note`, then the
amber `rate_grow_prompt` when `rates::grow_count` finds a count, with `add_for_each_label` named
`add_for_each_name` running `SetSlot` of the count's result in `tries_path` (`tries` or
`condition.tries`). The unit mismatch is the checker's warning among the report's notes. A picker of
a rate leaves out a parameter whose Unit is set and names no unit of time or whose Min is below 0, a
picker of a chance a rate parameter or one whose range leaves 0 to 1, and a picker of a share one
whose range leaves 0 to 1 (`parameter_items`, `rates::parameter_rate_unit`). `run_card_edit` sets
`CardState::tick_notice` to `tick_set_notice` when the edit gave a sheet with no `[time]` one (a
unit chosen or a rate parameter put in), drawn above the cards with the "otherwise" notice and
cleared by the next card edit or by `CardState::drop_tick_notice`, which the Sheet pane calls when
its edit changes the length or unit of one tick. `card_lines` gives each lower line of a
`competing_groups` group with no "otherwise" the parts of `competing_parts` ("or",
`SentencePart::Competing`, drawn by `lead_word_ui` as "otherwise" is), `CardLine::competing` (the
group's other ids, highlighted on hover or focus) and `competing_tooltip` with the group's old
state. `card_ui` collects each row's rect from `line_ui` and `bracket_ui` paints a gray 500 bracket
for each group in the gutter (`brackets`, `competing_brackets` in tests). The group note sits under
the highest line, where `rate_note` puts it. The Sheet pane's Time notice is in
`agents/henad-app-sheet.md`.

**Focus**. After `AddLine`, the new line's first blank opens. After a choice,
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

**Fire counts**. `SheetEditor::program` compiles the preview with henad-explore's
`sheet_probes(file)` through `GraphProgram::compile_with_probes`, the probes Apply builds with.
`SheetEditor::refresh` keeps the file's code identity in `SheetEditor::code` and the ids of
`fire_lines().lines()` in `SheetEditor::counted`, the latter kept over an edit that stops the sheet
lowering. `editor_ui` computes `sheet::line_fires(sheet, loaded, snapshot)` once a frame
(`sheet_fires`): a `LineFires` (the counts' tick and `FireLines::counts_by_line`) while the
snapshot's `fire_counts` hold a tick past 0 and one count per probe of the sheet, and the loaded
entry's code equals the cached code, else `None`. The checks run in that order, and nothing writes
the graph's text on a frame. An edit takes the counts away until the preview built from it counts,
and a paused run keeps the snapshot and its counts. `SheetShown::fires` passes it to `card_pane`,
and `CardDrawing::fires` holds it on a card that runs each tick. `CardDrawing::counted` holds on a
card that runs each tick with a line in `SheetEditor::counted`, and there `line_ui` narrows each row
by `COUNT_WIDTH` (72 points) whether or not counts have arrived, so the rows do not reflow on Play
or after an edit. `fire_count_ui` puts the line's count (`sentence::fire_count`, "30,877") in grey,
right-aligned in that column level with the row's top, a label with the hover text and AccessKit
description `fire_count_description` ("Line 2 applied to 2 cells in tick 4."). A line with no count
draws nothing there. `src/tests/sheet_fires.rs` holds
`the_change_lines_show_their_counts_while_playing_and_keep_them_when_paused` (Game of Life and SIR
from Play, the snapshot's counts per line, line 3 the same size before and after the first count,
then Pause), `a_sheet_shows_no_counts_of_a_run_of_other_code` and
`an_edit_hides_the_counts_and_a_blank_keeps_the_column` (an edit in place hides the counts, and a
blank that skips line 2 leaves line 3's sentence its rows, held by the place of its last control,
since the row now covers its notes). `card_pane`'s own `each_add_button_adds_lines_in_its_own_hue`
holds that every add item's line takes its button's hue.

**The Diagram** (`ui/editor/sheet/diagram.rs`). The toggle `DIAGRAM` beside `LINES_VIEW` after the
Card heading (tooltip `DIAGRAM_TOOLTIP`) sets `SheetEditor::diagram` and closes the card's pickers,
a click on Lines clears it, and the pane then draws `diagram_ui` in place of `card_pane`:
`DIAGRAM_NOTE` and `EDIT_AS_NODE_GRAPH` (disabled with `ConvertRefusal::tooltip`,
`agents/henad-app-editor.md`), then each card's heading and line count and its drawing in a
horizontal scroll area. `diagram_boxes` returns a `DiagramBox` per line from `card_lines` and
`CardRelations`: prefix, number, sentence, the result it works out, the results of lines on the card
it reads (from its `ChipKind::Result` chips, once each), its When and whether it is one. A box's
column is one past every box it reads and its When (`columns`), and each box takes a row of its own
in display order, so every arrow runs right and down. A frame "Inside line N" (`inside_line_frame`)
surrounds the boxes inside each When, nested Whens included, built innermost first as the union of
its boxes and inner frames, with a connector from the When's box. The drawing is painted with the
painter and holds no widget. It is one accessible node of role `Image` named `diagram_name`
("Diagram of the card's 3 lines, read-only"). Colours: boxes gray 900 with a gray 600 stroke and a
gray 800 header, a bar at a box's left in its line's `kind_hue` (`DiagramBox::group`), results and
their arrows blue, frames a gray 700 outline. A card's heading is in the semibold face with its
count as a `caption`, as on the card. The toggle is the Diagram segment beside Lines, egui's
selectable button.

`src/tests/sheet_card.rs` holds
`a_blank_model_builds_game_of_life_with_its_lines_and_equals_the_starter` and
`a_blank_model_builds_the_chance_sir_with_its_lines_and_equals_the_sheet` (from Blank by accessible
names, then `align_with_starter` and equal canonical text and lowered code, the words rule over
every menu, picker and select opened, every control's name holding its visible words through
`assert_controls_hold_their_words`, which also reads the note buttons and add rows of
`card_buttons`, and Undo back to Blank and Redo to the end for Game of Life),
`a_list_blank_takes_two_values` (with the skip note of a cleared blank),
`a_states_blank_starts_a_list_that_takes_another_state`,
`a_single_blank_given_a_list_says_choose_is_one_of`, `a_chance_of_1_5_is_refused_in_the_field`,
`chips_and_pickers_name_the_property_when_two_share_a_state_name`,
`the_line_menu_renames_a_result_and_deletes_a_line`,
`the_line_menu_adds_a_line_beside_another_and_duplicates_a_line`,
`the_line_menu_writes_a_note_that_leaves_a_played_preview_running`,
`the_menu_that_adds_a_line_offers_what_its_place_takes`,
`add_menus_line_menus_and_pickers_hold_menu_items_in_menus` (each item a `MenuItem` in its named
menu, a titled overlay's heading, through `Node::menu`),
`arrow_keys_move_round_an_open_menu_and_escape_returns_to_its_button` (an add menu, the line menu
and "Choose when", each settled once open, as the rows come from the frame before),
`a_focused_item_scrolls_into_its_picker_and_the_card_stays_put` (↑ to the last item of "Choose
when" over the list under the panes and in a scrolled picker, through `last_item_by_key`),
`escape_returns_the_focus_from_a_select`,
`the_number_box_keeps_the_arrow_keys_and_its_use_button_passes_them_to_the_items` (through
`focus_is_an_item_of`), `an_empty_number_box_passes_the_arrow_keys_to_the_items` (a box last, ↓ to
the first item and ↑ to the last, and a typed number keeps them in the box),
`a_tooltip_shows_on_keyboard_focus_but_not_while_a_menu_is_open` (an add button focused by Escape,
then by a click), `the_new_model_button_shows_its_tooltip_on_keyboard_focus` (reached by Tab),
`a_click_on_a_control_under_a_focus_tooltip_reaches_the_control` (a window 1000 points wide, where
the tooltip of "+ Chance" covers part of "+ When, for several lines"),
`a_picker_opens_below_or_above_its_anchor_and_scrolls_where_neither_fits` ("Choose when" in three
window heights through `choose_when_in_window`, and the "+ Chance" menu above its button),
`no_name_in_choose_when_or_a_line_menu_holds_another` (no menu item's name holds
another's, case aside, in "Choose when" of a change and of a new When and in the line menu, through
`names_holding_others`. The add menus hold "Become, with chance" beside "With chance", the colour
picker Grey beside Dark grey, and a result picker can hold user names such as "Alive neighbours 2"
beside "Alive neighbours"),
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
inside its crate. henad-graph's `the_copies_of_the_test_sheets_equal_their_source` holds it and
`sird.toml` equal to the source.

`src/tests/sheet_rates.rs` holds
`a_blank_model_builds_the_rates_sir_with_its_lines_and_equals_the_starter` (the time set on the
sheet, both rates through "New parameter…" in "per day", line 2's "for each" from the grow offer,
New outbreak's "Choose when" with no rate, then alignment, label-in-name and the words rule),
`the_run_once_card_offers_no_rate_after_rates_arrive` (the SIR notes, the mean stay, "Convert to a
chance" on a typed rate, none on New outbreak's typed chance),
`the_sird_lines_3_and_4_read_or_with_one_bracket_and_one_group_note` (from
`src/tests/sheets/sird.toml`, a copy of henad-graph's),
`change_to_a_rate_and_back_keeps_the_chance_per_tick` (with the Time notice),
`the_long_tick_note_shows_at_one_day_and_leaves_at_a_quarter_day` and
`removing_the_time_unit_marks_the_rate_lines_and_a_rate_unit_sets_it_again` (the tick notice and the
mismatch warning) and `a_parameter_put_into_a_rate_takes_its_unit_and_one_undo_restores_both`. A
label's AccessKit node holds a `TextRun` child with its text, so the tests count nodes other than
text runs.

**Agent lines**. Every card is editable. A card is over agents when its `every` is not
`cells`, and the pane passes that as `agents` to `AddButton::items_for`, `tooltip_for` and
`heading_for`, which return the agent menus (Look around: `CountAgents`, `LookAroundAgents`,
`PickCell`, `CellHereOrAhead`; Chance: `BecomeWithChance`, `WithChance`, then under
`RANDOM_VALUE_GROUP` `RandomNumber`, `RandomDirection`, and no rates; Calculate: `Calculate`,
`Choose`, `DependingOn`; Change: `Become`, `Set`, `Steer`, `KeepBetween`, `MoveBy`, `MoveTo`,
`Leave`, `Record`). The buttons are `AddButton::AGENTS`, "+ Repeat…" last, and a block's add row
takes `AGENTS_INSIDE_ORDER`. A Repeat is a block like a When (`LineKind::is_block`), with its add
row and the add-at-end note. `cards_ui` draws a world of agents with no tick card as `card_header`
of the kind and adds `AddCard { every: kind }` with the first line. `new_line(item, every)` fills
what the checker asks of a template: a look at agents the card's kind, `include_me` false and one
`LookSummary::new("", "")` average (named by henad-graph), a Pick a cell `Cells8`, `Most` and open
cells, a Look at a cell `Here`, Steer and Move a mode, a Choose one empty row. Selects:
`mode_options` (by and towards for Steer, by and to for Move), `pick_options`, `which_options`, and
`look_summary_options` per summary (`LOOK_SUMMARIES`, "Remove this summary" running `DeleteSummary`
while two or more are left, and a count blanks its value). A removal that blanks a line that reads
the summary waits on the dialog titled `REMOVE_SUMMARY_HEADING`, with `lost_question` and Remove
(`removing_a_summary_that_lines_read_asks_first`, in `src/tests/sheet_agent_lines.rs`). `add_summary_ui` draws "+" named
`add_summary_name` after a look's summaries, opening `summary_picker` (`CHOOSE_A_SUMMARY`), whose
choice runs `AddSummary` and opens the new summary's value. `add_row_ui` draws "+" named
`add_choose_row_name` after a Choose line, a value picker at `row.N.value` that `SetSlot` appends,
and `remove_row_ui` a "×" named `remove_choose_row_name` after each row while two or more are left,
`DeleteRow` with `REMOVE_CHOOSE_ROW` as its tooltip.
A Choose row's condition takes `ChooseRowCondition` (`row_of_condition` in `slot_choice`). Leave and
Record take the condition arrow too (`takes_condition`).

Refusals: `SheetEditor::refresh` adds `SheetFile::lowering_problems` to the report when the sheet
checks and does not lower, so a lowering refusal about a line (a Set and a Steer of one property,
Move by and Move to, a change a Repeat cannot hold) draws in red under its line, and the list under
the panes keeps those about no line. A Count, Look around or Pick a cell inside a Repeat is a check
error in the author's words (`LOOK_IN_REPEAT`) and draws there too. An add menu shows weak, with
the refusal as its tooltip, each item whose `new_line` `add_refusal` refuses, through henad-graph's
`sheet::refusals`, the tests the check and the lowering refuse by: `refusals::change` beside the
card's other lines (a Move to beside a Move by and the other way round, a Move by behind walls or
over a ground that blocks a state), then inside a Repeat, or a block inside one
(`ClosingBlock::in_repeat`), `refusals::in_repeat` (the looks with `LOOK_IN_REPEAT`, and Keep
between, When, Repeat, Leave, Record and Move to with `NOT_YET_REPEAT_CHANGE`). A card not made yet
counts as an empty one. A select's option takes the same tests (`option_refusal`): a mode a Move
line cannot take, through `refusals::change` in the line's own place, and "the cell ahead of me"
while `refusals::cell_ahead` refuses it. The current option is never greyed, so a refused line can
always go back. `the_add_menus_and_selects_grey_each_choice_the_build_refuses_with_its_reason` (in
`src/tests/sheet_agent_lines.rs`) holds each greyed item and its tooltip, and that a weak item adds
nothing. `add_summary_ui` opens the new summary's value, except for a count, which has none. The author's own `note` under a line draws
last, in grey (`author_note`).

`src/tests/sheet_agent_lines.rs` holds
`a_blank_agent_model_builds_the_random_walk_with_its_lines_and_equals_the_starter` and
`a_blank_agent_model_builds_boids_with_its_lines_and_equals_the_starter` (by accessible names,
`align_with_starter`, equal text and lowered code, label-in-name and the words rule; boids against
`boids_as_the_editor_builds_it`, the starter with Velocity starting as a new Arrow does and no
measures), `my_new_velocity_is_offered_below_the_last_change_to_velocity_and_nowhere_above`,
`a_property_changed_two_ways_and_a_look_inside_a_repeat_show_their_refusals_on_the_line`,
`a_choose_row_is_removed_by_its_cross_while_two_rows_are_left`,
`opening_boids_leaves_a_default_off_its_slider_steps_as_it_is`,
`each_agent_slot_offers_what_fits_it` (the picker table, a test per slot kind),
`a_state_picker_offers_the_ground_only_where_a_cell_is_compared`,
`a_count_of_the_cells_around_an_agent_offers_the_ground_states`,
`choosing_the_same_property_of_a_count_of_agents_keeps_its_state`,
`a_slider_that_takes_the_focus_keeps_a_default_off_its_steps`,
`the_agent_menus_and_pickers_show_no_forbidden_word_and_name_each_control_by_its_words` and
`the_ants_note_on_slower_trail_formation_stays_under_its_line_after_an_edit`.
