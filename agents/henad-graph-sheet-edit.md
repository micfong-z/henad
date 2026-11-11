# henad-graph: sheet edit commands

**`henad_graph::sheet::edit`**: the commands every edit of the sheet editor runs on a
`SheetDocument`. `agents/henad-graph-sheet.md` holds the document, its modules and the checker the
commands leave meaning to, and `agents/henad-graph-edit.md` holds `KeyStream`.

**Edit commands.** `SheetBuilder` holds a `SheetDocument` and a `KeyStream` (`new` seeds it from
entropy, `with_key_seed` from a seed), and `apply(SheetEdit)` runs one command on a copy (`Draft`),
so a refused command changes nothing, then returns the ids it made. A command checks structure (ids,
names, the line a path reaches, a block to add inside) and leaves meaning to the checker. A command
that leaves a number that is not finite is refused in `sentence::number_not_kept`'s words, found
by `to_text` failing after it and not before, so the history's texts always write.
`SheetEditError` is `Missing`, `Invalid`, `Taken` (a name of the same kind, case aside, or an id)
and `Reserved` (the id). Commands: the sheet's name and purpose, `SetWorld`, `SetTime`;
`AddProperty` (the first States property of a kind of thing with no colour rule colours it by
state), `SetPropertyType` (clears every reference to the property and its states as a delete does,
drops a colour rule over it, empties its states, start and marks, keeps its place and id, and
colours its thing by state again when it becomes a States property of a thing with no colour rule;
the same type changes nothing), `AddState` (the next of `STATE_COLORS` after the property's states,
dark grey, blue, orange, purple and on, and `PALETTE_COLORS` holds the same
twelve in the order of `sentence::COLOR_NAMES`), `AddParameter`, `SetParameter` (min, max, default,
step, unit and means as one edit), `AddMeasure` (a count in a state takes the state's colour, else
the next of `MEASURE_COLORS`), `AddEvent`, `AddButton` (start again over a kind of thing, or run
once with a card of the button's id), `SetButtonSteps`, `AddCard` (`cell_tick` or
`<agent kind>_tick`, before the cards that run once); `Rename` (an average of a property still
named `sentence::average_measure` of its old name follows it, unless another measure holds the new
name), `ChangeId`, `Recolor` (a measure drawn in its state's colour follows the state) and `Delete`
over a `SheetItem` (property, state,
parameter, measure, event, button, card, agent kind); `SetStartsAs` (one edit for a Starts as
commit, the old key kept), `SetColorRule`; `SetAgentKind` (the number of agents at the start, from 1
to the largest number, and that largest number), `SetMarks` (a Number property of the ground: how
marks combine, and the share kept per tick, which no marks rule clears), `SetBlocked` (a state of a
States property of the ground kept from agents or let in, held once); and the line commands
`AddLine` (a `Box<SheetLine>` template at the end of a card or a When or Repeat), `SetSlot` and
`SetField` (a `LineField`) at a slot path, `ChooseCondition` (a `ConditionItem`),
`ChooseRowCondition` (a Choose row's condition, both through `condition_phrase`), `UseOneSide`,
`UseCountForEach`, `MoveConditionInto`, `RenameResult`, `SetNote`, `MoveLine`, `DeleteLine`,
`DrawNewNumbers`, `ChangeToRate` and `ChangeToChance`, and for a look at agents `AddSummary` (a
summary of a kind with its value blank, refused on a line that looks at no agents) and
`DeleteSummary` (refused on the last summary, its users' slots cleared through `Target::Summary`).
"Remove this When" is `DeleteLine` on the When, and "Add for each" is `SetSlot` of `tries`.

Ids: `mint_id(name, space, is_taken)` lower-cases the name, turns each run of other characters than
`a-z0-9` into one `_`, puts `x` before a leading digit (and mints `x` for a name with no letter or
digit), cuts to `MAX_SHEET_ID_BYTES`, refuses an id `IdSpace::is_reserved` holds (`Reserved`), and
takes `_2`, `_3` against a clash. `IdSpace` is `Item` (properties, parameters, measures, events: the
checker's reserved words, `l<digits>`, the IR's parameter ids, `tick_length`, `henad_*`), `Thing`
(also `cells`, `ground`), `Button`, `Card` and `State` (`any_state` alone). A button that runs once
mints against buttons and cards together. `ChangeId` checks the grammar, the length, the reservation
and the namespace, then rewrites every reference; a card of the button's id that the button runs and
an agent kind's tick card follow. A suffixed id that is reserved (`henad_2`) is refused like a
reserved base. `is_reserved_name(name, space)` lets a form refuse such a name first, in the words of
`sentence::name_reserved` ("Henad uses the name Count itself. Choose another name."). Lines take `l<next_line>`, and `next_line` grows past it, so a deleted number
never comes back. After a command, every line it touched that draws gets a key no other draw holds
(a template's own key is kept when free), and every start it set that draws (by chance, anywhere, a
value that draws, on the ground when its state is painted on two or more cells, `starts_draws`) or
shares its key gets one; a start on the ground counts as set when the ground's start is. A line
keeps its key while an edit takes its draw away (`a_start_on_two_painted_cells_takes_a_key`).

References (`references.rs`): `change_references(document, target, Rename(id) | Clear)` walks every
slot, list, item, branch, summary, row, line field, formula (`rewrite_formula`, the formula parser's
lexing, so numbers and function names stay), start, blocked state, measure, colour rule, button step
and card. A state chip belongs to the property its slot is about (a Become's property, the left of a
comparison through `compared_property`, a start's own property, a Set's property), else to the one
property that holds the state id. A clear blanks a slot, drops a list item, empties a field, empties
a formula that names the target, and drops a table about the target (a measure, a start row or
shape, a blocked state, a colour rule over a deleted property, a button step naming a deleted
property or card); a start's `otherwise`, `base` or state chip `value` falls back to the first
remaining state of the property, and an `on_ground` start's `state` to the first remaining state of
its ground property. A deleted property clears its states' references first. A deleted line's
result chips turn blank: the user is skipped, never refused. Most blanks in a card line are
skips, but a blank outside the card lines (a start's chance or value, `kept_per_tick`, an
`on_ground` start's property) and a few agent line fields (a Record's event, some agent slots) are
checker errors after a delete. A deleted When or Repeat leaves its lines one level up in its place,
a deleted card takes its lines, and a deleted button takes the cards that run once that no other
button runs. An agent kind is not deleted. A `[ground]` table comes with the first ground property
`AddProperty` adds, and a delete that leaves it with no property and no blocked state removes it.

Lines (`lines.rs`): a slot path is the author format's keys joined by dots (`condition.rate`,
`whose.right`, `items.2`, `row.0.condition`, `branches.alive`, `summaries.0.value`), each step but
the last into a phrase. The lowering's `LineMap` paths use the same grammar, and `lines` with
`parse_path`, `PathStep`, `RowSlot`, `phrase_mut` and `value_mut` is crate-visible for the test that
holds them together. `SetSlot` with a filled value at one past the last item or row appends, and
a blank removes an item or a branch. A line whose tables would nest past `MAX_NESTING` (counted
generously from the line table's depth of 4) is refused, and so is a card past `MAX_LINES_PER_CARD`.
Moves follow the outline: the first line of a block moves up out
of it and the last moves down out of it, a line that is not a block beside a block moves into it (at
its end from below, at its start from above), and a block swaps with the line or block beside it; a
move past the card's top or bottom is `Invalid`. `AddLine` inside a When gives a Become or Set with
a blank condition `always`, and a Become with no old state the state of the nearest When that tests
`my X is S`. `ChooseCondition` keeps the condition's values when the form stays (Is, AtLeast and
AtMost share the compare form and set its test, a list right side keeping its first item), makes a
blank phrase otherwise, and refuses always on a When. `UseOneSide` turns an outside test with one
side filled into `less_than` the low or `more_than` the high. `UseCountForEach` moves a Count result
in the chance or rate (of a Chance or Rate line, or a change's condition) into `tries`.
`MoveConditionInto` copies a When's condition into its one change and removes the When, refused when
the change's condition is neither blank nor always.
`LineField::NewState` sets the property with the state and clears an old state of another property,
`Property` clears states of another property, `Test` between a single value and a list keeps the
first value or makes a list of one.

Rates: a rate phrase put in by `AddLine`, `SetSlot` or `ChooseCondition` with no unit takes
the tick's unit and `rate_unit_from_tick = true` while `[time]` exists. A parameter put into `rate`
whose rate has no unit or a marked one gives the rate the first "per <unit>" its Unit text names and
clears the mark, and with no `[time]` sets one tick to 1 of that unit. A parameter with no Unit
put into `rate` takes `unit_suggestion` of the rate's unit in the same command
(`parameter_takes_unit`), the rate's mark left alone. `unit_in_text` reads the Unit
case aside, spaces after "per", a plural "s", a word
boundary at both ends. `LineField::RateUnit` by hand clears the mark, sets one tick when `[time]` is
absent, and gives a parameter with no unit in the rate's `rate` slot `unit_suggestion` of the unit.
`ChangeToRate` and `ChangeToChance` turn the chance of a Chance line or of a change's condition
into a rate and back, keeping the chance per tick through `sheet::rates` (`rate_from_chance` with
the tick's length, `chance_from_rate` with `tick_in` of the rate's unit). Each refuses a sheet with
no `[time]`, a value that is no typed number (a parameter or a result), a chance outside 0 to below
1 and a rate with no unit. The rate takes the tick's unit, marked, the chance clears the unit and
the mark, both keep `tries` and the line's key, and the number is written as the shortest decimal
that reads back as the same `f32` (`rates::shortest_f32`), so 1 - e^(-0.1) turns into 0.1 per day
and back into the same chance.

Names (`names.rs`): `automatic_name(document, line)` follows the wording table's result names
(Count, Total or average as a cells Look around with one total or average summary, chance and rate
with `happens_base` of a parameter, of the first parameter in a named Calculate's formula, of a
typed number or percent, or of another value's words, Compare and Calculate through
`Sentences::result_words`, all of, any of), `None` for other kinds. After every command
`update_names(before, after)` renames each line that follows: a name the edit left alone follows
when it had no name or equalled its old automatic name (a " N" suffix included), and a new line or a
name the edit set follows when it is empty or its new automatic name. Names typed by hand are taken
first, then the followers in display order take the base, " 2", " 3", unique case aside.
`RenameResult` refuses a name another result holds.
Summary names: `automatic_summary_name` names a summary of a look at agents "count", "[kind]
[property]" for their property ("average velocity", the name's ordinary capital dropped), "centre"
for the average offset to them, "push" for the total offset away from them (`CENTER_SUMMARY`,
`PUSH_SUMMARY`), "[kind] [relation words]" otherwise, and its kind alone while its value is empty.
`update_summary_names(before, after)` runs before `update_names`: a summary follows when it is new
with an empty or automatic name, or its name before the edit was its automatic name then (" 2"
included), and a following summary takes its automatic name, unique within the line. Steady
summaries, held by id before the edit with the same automatic name, take theirs first, and an edited
summary that reaches another's name takes " 2". A summary whose name changes or that has no id takes
the id `mint_id` gives its name, unique within the line, and `count_summary` for a reserved id such
as `count`. `change_references` with `Target::Summary` rewrites every `part` and formula reference
(`l1.centre`) in two passes, each old id first to a free `henad_summary_N`, so a new id equal to
another summary's old id renames no reference twice. Every committed look at agents names its
summaries this way, so a rebuild mints the starters' ids.

Tests sit in `src/tests/sheet_edit.rs`: the author format's three diffs built by commands from the
sheets (the waning line through `AddParameter`, `SetParameter`, `AddLine` with the diff's key,
`SetField` and `SetSlot`; two `MoveLine`; a `Rename`), `every_outline_move_matches_the_prototype`,
`a_rename_changes_one_name_and_the_comments_that_show_it`,
`change_id_rewrites_every_reference_and_nothing_else` (a parameter, a property, a button and its
card, a state and a parameter in formulas, each the old text with the quoted id replaced),
`a_deleted_line_number_is_never_minted_again`,
`a_new_property_type_clears_every_reference_and_keeps_the_place`,
`id_minting_follows_its_known_answers`, `every_starter_id_follows_the_minting_rule` (every committed
sheet, ids minted in file order, cards from their kind or button, lines `l1` up in display order
across cards and `next_line` one past), `a_seeded_key_stream_gives_its_known_first_keys`, the
automatic names, the condition offers, the for-each and fold offers, deletes that leave skipped
lines and no error, colours, rate units and marks, Starts as commits, new buttons and cards, and
`every_edit_leaves_a_document_that_reads_back` (random command sequences on every sheet: the text
reads back to the same document, the checker never panics, no two lines share a key),
`the_agents_take_a_count_up_to_their_largest_number`,
`a_summary_follows_its_automatic_name_and_its_new_id_reaches_every_reference`,
`a_summary_named_by_hand_keeps_its_name_and_a_removed_summary_blanks_its_users`,
`the_automatic_summary_names_follow_round_1`, `a_choose_row_takes_its_condition_from_choose_when`
and `a_ground_property_takes_its_marks_and_fading_and_comes_and_goes_with_the_ground` (an add-up
mark writes `marks = "add_up"` and lowers to `combine = "sum"`).
