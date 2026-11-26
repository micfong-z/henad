# henad-app: the pickers of the Card pane

`ui/editor/sheet/picker.rs` builds and draws the picker of each blank and chip of a card's line,
and the selects of its fields. The card that opens them is in `agents/henad-app-card.md`, and the
checker's trial of a value in its place in `agents/henad-graph-sheet-check.md`.

**Pickers** (`picker.rs`). `control_picker` opens a control's picker or select when it is clicked or
due to open (`CardState::open_next`), and takes the focus for `CardState::focus_next`. A picker's
open state lives in egui memory under the control's id with "picker". `CardPicker::show` opens on a
click of its anchor and stays open on a second one, closes on a choice, on Escape (the focus back on
the anchor) or on a click outside both the picker and the anchor. `picker_overlay` places it with
`widgets::placed` and scrolls a picker past `LONG_PICKER` items at `PICKER_HEIGHT` at most, or one
that filled its room in the frame before (a short picker takes its own height, which a scroll area
lags behind). Each item is a
frameless button named by its label alone, or by `name` when that differs (the "otherwise" item),
with grey `detail` under it. `picker_for(context, want, property, ground, filled, typed, draw)`
builds the picker of a place from its `Blank`: states (`shared_state_name` when two properties share
a name, with the ground's states on a card over agents only where `ground` holds, and with no state
`NO_STATES` naming Cells, `NO_AGENT_STATES` Agents, or `NO_AGENT_OR_GROUND_STATES` both where
`ground` holds), "any state" first for an old state, the States or Number properties, or a number's
groups (`number_picker`: "From lines above" with "line N: [sentence]" under each result, "This
cell's properties" as "my Food" with "this cell's Food", "Parameters" with value and unit, "New
parameter…", and the Number box first, or last for the value left of a comparison and a "for
each" count). The Number box takes the focus as its picker opens, first or last, as the Sheet
pane's box does. A last box and the Remove item are the picker's footer (`CardPicker::footer`),
which `picker_overlay` draws under the scroll area of a picker that scrolls, its height of the frame
before kept under `FOOTER_HEIGHT_ID`, so the focused box stays in view
(`a_picker_with_its_number_box_last_focuses_the_box`, ants' line 11). A chance
reads in `NumberUse::Chance` and a rate in `NumberUse::Rate`, which refuse a chance outside 0 to 1
and a negative rate in the box. A typed list in a box of one number adds `LIST_IN_ONE_NUMBER`. A
list blank offers the list box (`LIST_GROUP`, `LIST_HINT`, "2, 3" in one go, `read_numbers`), a
filled item `REMOVE_FROM_LIST`, a "for each" `for_each_lead` and `REMOVE_FOR_EACH`. The Number box
opens on a typed number as typed (`typed_text`), and a refused text keeps the box focused. The
boxes start afresh each time the picker opens.
`LineContext::results_above` offers the results of lines above, a result inside a When only to the
lines after it inside that When, for "Choose when". Every picker of a value lists `values_above`
under "From lines above": every result above whatever its kind, then the parts above.
`mark_refusals(picker, context, target)` drops the Number box and "New parameter…" where the place
refuses a typed 1, which the range of every chance, rate and count takes, as the value of a Set of a
Vector and the left of a comparison with a state do, and gives each value item the place cannot take
a `refusal`, `place_refusal(...).reason(label)` from henad-graph's checker
(`agents/henad-graph-sheet-check.md`), so a picker and the checker never disagree, and `groups_ui`
draws it disabled with the reason as its tooltip. A value of a Calculate line of one operation (a
field) refuses a phrase (`CALCULATE_TAKES_PLAIN_VALUES`) and a cell (`kind_needed`). A picker whose
items are all refused still shows its empty words under them, such as `NO_PLACE` under the greyed
values of a Move to. `control_picker` keeps the picker it built under the popup's id with "built",
beside `CardState::fingerprint` (`fnv1a64` of the sheet's text, set as `card_pane` starts), and
builds it again only once the sheet changes, since each reason reads the checker.
`every_offered_value_the_checker_takes_and_every_greyed_one_it_refuses` puts each value of each slot
picker of four starters in its slot through `apply_choice`: an offered value adds no error and
lowers, a greyed one adds an error. `choose_when(context, otherwise)` lists the Boolean results as
"when [name]", then "Conditions" or "Or a new condition" with `ConditionItem`s and their grey lines,
"otherwise" in place of "always" when `otherwise_if_always` finds a higher line (the line's
condition set to always on a copy of the sheet), and "at rate" only on a card that runs each tick
(`offers_condition`), since a button's card has no duration and takes chances only. On a When line
the items read "When …" (`ConditionItem::label(true)`, and a result item
`result_condition_item(name, true)`), and "always" is left out. A line inside a When takes the lead
`when_lead` with the When's number. Selects list `comparison_options` (the list words of a States
property), `around_options`, `summary_options`, `operation_options` and `unit_options` ("second" to
"year", `SelectOption::Unit` writing `LineField::RateUnit`).

Pickers on agents (`LineContext`): `every`, `agents`, `properties` (the card's kind),
`ground_properties`, `my_heading` (`this_things_properties`, "This bird's properties"), and
`lines_above` (the lines a line can read, a block's lines only to the lines after them inside it).
`results_above_of` takes several `ResultKind`s (Number, YesOrNo, Arrow, Place; a Choose or a
Depending on fits a number, an arrow or a place, and a look at agents none, `result_kinds`).
`parts_above` offers each summary of a look above ("Flockmates' centre"), a pick's `amount` and each
ground property of a cell picked or looked at above, named as `Sentences::value` writes the chip.
`takes_ground_states(line, target)` keeps the ground's states out of a Become's new and old state
and out of the `whose.right` and `only.right` of a count or look at agents (`around` is
`AgentsWithin` on the phrase that owns the compare), which take the agent's own states. It offers
them in a compare such as "Next cell's Ground is Food" and in a count or look over the cells around.
`my_properties` adds "my new X" (`my_new_property_item`, detail `my_new_detail` with the number of
the last change) for each property `check::changes_property` finds changed for the last time above
the line (`last_change_above`), the same rule as the checker's skip. `lengths` offers
`length_of_item` as the formula `length(my.<id>)`. `value_picker` puts `new_values` (a random
number, and for a value a random direction, phrases with blank values) under `OR_A_NEW_VALUE`
before the parameters, since a long picker scrolls. `fits_property(kind, agents, property)` is the
property table: Steer a Number or an Arrow, Keep between a number or an Arrow, Set on agents
anything but States, Position and Heading, Set on cells a number, any other a States property.
`Blank::GroundProperty`
lists Number properties of the ground (`ground_number_items`), `Blank::Event` the events
(`event_items`, `Choice::Event`), `Blank::Times` integer parameters alone, with no results and no
properties since `repeat_count` lowers nothing else (`NumberUse::Times`, an integer from 0,
`TYPE_TIMES`; its New parameter dialog makes a `WholeNumber` and refuses other values with
`TIMES_LIMITS_NOT_INTEGERS`). A summary's value opens `looked_at_picker`: `their_property_item` for
each property but States, then the three relations, under `each_looked_at`. A Move to's value offers
the places above, every other value above greyed ("… is a vector, and Move to needs a cell."), and
`NO_PLACE` under them while nothing fits.

**Type symbols** (`widgets::TypeSymbol`). A weak glyph before each property and parameter of the
Sheet pane and each item of a picker names the type of its value: Number `MDI_CIRCLE_SMALL` and
Vector `MDI_ARROW_TOP_RIGHT_THIN` as the Statistics tab draws a scalar and a vector, Integer
`MDI_POUND`, Boolean `MDI_TOGGLE_SWITCH_OUTLINE`, Position `MDI_MAP_MARKER_OUTLINE`, Heading
`MDI_COMPASS_OUTLINE`, States `MDI_LABEL_OUTLINE` and Cell `MDI_SQUARE_OUTLINE`. `of_property`,
`of_parameter` and `of_kind` (henad-graph's `ValueKind`, through `value_kind` of the value on the
line, as `LineContext::symbol_of` reads it for a result or a part above) map to it, and
`TypeSymbol::name` is the type's word (`ARROW_KIND`, `YES_NO_KIND`, `CELL_KIND` and the rest), the
tooltip. `PickerItem::with_symbol` sets an item's symbol, which `groups_ui` passes as
`OverlayItem::symbol` while `type_symbols_shown` holds, and `overlay_item` paints it in its own
column before the swatch, with the name as a tooltip over that column alone. `symbol_space` keeps
the column empty for an item with no symbol, so it lines up with the symbols of its group, and the
Colours select (`sheet_pane/colors.rs`) gives each property its symbol and None the empty column
(`the_colours_select_shows_the_symbol_of_each_propertys_type`). In the Sheet pane `type_symbol_ui`
draws it as an image named `type_symbol_name` ("Type: Vector"), so it never shares a name with a
control, and a table leaves its symbols out before any of its columns gives up width
(`agents/henad-app-sheet-pane.md`). `type_symbols_shown` reads a persisted egui memory flag
(`SHOW_TYPE_SYMBOLS_ID`, on until turned off) that the View menu's checkbox
(`menu_bar::SHOW_TYPE_SYMBOLS`) sets through
`show_type_symbols` (`the_view_menu_hides_the_type_symbols_and_the_app_keeps_the_setting`,
`the_type_symbols_show_before_properties_parameters_and_values_until_hidden`).
