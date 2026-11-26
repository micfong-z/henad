# henad-graph: the wording table and the sentences

`sheet/sentence/` is public (`sheet::sentence`): every word the sheet editor shows, and the
sentences of the cards' lines with their parts and places. `mod.rs` holds the module doc and
re-exports every item of its private files, so each stays at `sheet::sentence::X`. `card.rs` holds
the card's fixed words and templates (its frame, the add menus, the words of line sentences,
comparisons, conditions and blanks, the Lines help and `FORBIDDEN_WORDS`), `pickers.rs` the
pickers, their messages, typed numbers and result names, and the reasons of a greyed value
(`BOOLEAN_WORDS` to `LIST_WORDS`, `kind_needed_by`, `kind_needed`, `as_sentence`, `move_owner`,
`set_owner`, `steer_owner`, `CALCULATE_TAKES_PLAIN_VALUES`), `notes.rs` the notes under a line and
"otherwise", `rates.rs` the words and notes of rates and chances, competing rates, the long tick and
rate conversion, `errors.rs` the errors and refusals with the cost words and fire counts, `sheet.rs`
the sheet's sections and forms, `editor.rs` the New model, unsaved changes, replace, delete and New
parameter dialogs, the top bar, the world tabs and the Diagram, `convert.rs` the conversion words,
`grammar.rs` lists, articles, capitals, possessives, plurals and `thing_word`, `parts.rs` the part
types and `sentences.rs` the `Parts` builder, `Sentences` and `competing_parts`.
`agents/henad-graph-sheet.md` holds the rest of the sheet module, and `agents/henad-app-card.md` the
card that draws the parts. The drag of a line takes `drag_line_tooltip`, `more_lines`,
`DROP_ON_OTHER_CARD`, `DROP_INTO_BLOCK` and `DROP_MISSED` from `card.rs`.

**The wording table.** `sheet::sentence` holds every word the sheet editor shows: constants for
fixed text and functions for templates, each named for what it
words (`card_header`, `otherwise_words`, `not_run_choose`, `gap_note`, `for_each_words`), with the
enums `AddButton` and `AddItem` (labels, tooltips, headings, patterns; `+ Chance` has the tooltip
"Change a state by chance or at a rate"), `ConditionItem` ("Choose when"), `Blank` (visible word,
`want`,
accessible name, picker title), `LineSelect` (`words` in the accessible name, `want` with its
article in a skip note: "Not run yet: choose a unit of time"), `DrawKind` and `ConditionKind`. A
template takes the
line number, the line word ("line 2", "Clear line 1") or the line prefix ("Line 2", "Clear line 1")
it needs. Beyond the card it holds the sheet (`SheetSection` with each heading, help text and
example, `SheetSection::shown` for a world of cells or of agents, the World and Time words, the
property form and its checks, Starts as with `StartsField` and the `starts_*_place` functions, the
Parameters, Measures, Colours and Buttons words, `SHEET_EMPTY`, and the Agents, Ground and Events
words), the New model and New parameter dialogs (`STARTER_CHOICES` with every starter's
description), the Editor's unsaved changes dialog (`UNSAVED_HEADING`, `unsaved_changes`,
`SAVE_MODEL`, `DISCARD_CHANGES`) and the mark beside the name of a model with unsaved changes
(`UNSAVED_MARK`, `UNSAVED_MARK_TOOLTIP`), the top bar, the world tabs (the tried-value words,
`PLOT_EMPTY`, `WORLD_EMPTY`), the Diagram and the conversion. A value's type is named "Boolean" for
yes or no, capitalised as a proper adjective in running text ("a Boolean result", "+ Boolean"), and
"vector" for an arrow ("a vector", the kind `ARROW_KIND` "Vector"), while the values keep the words
yes and no and the file keeps its stored names `yes_no` and `arrow`. Shown text takes
the proper characters: "…" in a menu item, a pattern or a button ("+ Repeat…", "New parameter…",
"when … is …"), "→" between a state and its
value in a Depending on, and "×" and "−" in a note. A blank reads `BLANK_TEXT`, "…", in shown
text and in an automatic name. An automatic name that an older sheet stores with "..." for a blank
still counts as automatic (`names::read_blanks`), and the next edit writes it with "…". The rate
notes are templates taking numbers already formatted (`rate_note`,
`rate_for_each_note`, `worked_out_rate_note`, `growing_rate_note`, `chance_for_each_note`,
`growing_chance_note`, `competing_note`, `mean_stay_note`, and `otherwise_rate_note` after any of
them), which `sheet::rates` fills. The end of
`rates.rs` holds the words of competing rates, the long tick (`long_tick_note`) and rate conversion
(`chances_per_tick_note`), and `errors.rs` the remaining errors (`RATE_IN_BUTTON_CARD`,
`RATE_IN_REPEAT`, `ONE_AGENT_KIND`, `measure_name_reserved`), with the interim refusals
(`NOT_YET_REPEAT`, `NOT_YET_REPEAT_CHANGE`,
`NOT_YET_AGENT_WALLS`, `NOT_YET_BLOCKED_MOVE_BY`), the errors of the ground and the places
(`ON_GROUND_CELLS`, `AHEAD_NEEDS_CELLS`, `SET_IN_REPEAT`, `DISTANCE_NOT_FIXED`), a property changed
two ways (`set_already`, `steered_already`, `moved_by_already`, `moved_to_already`) and the words
of a look at agents (`look_name`, "line 3, Flockmates") and of its cost (`look_costs_too_much`,
`card_costs_too_much`, `start_costs_too_much`, counts written with thousands commas, and for a pass
past its cost per element `line_costs_too_much`, `smaller_maximum`, `FEWER_TIMES` and
`FEWER_LINES`). `plural` (the rules of English and `IRREGULAR_PLURALS`) is crate-visible for
`SheetFile::shortfalls`. Every editor and
the writer's comments draw their words from here and from no literal of their own. `FORBIDDEN_WORDS`
lists the words sheet mode never shows, and `is_forbidden_word` also matches their endings ("nodes",
"compiled"). A note or tooltip about what a card runs on takes `thing_word` ("cell", "bird"): the
relation notes, the rate subjects, `from_tooltip`, the "otherwise" and competing words, with
`LINES_HELP_AGENTS`, `WORLD_HELP_AGENTS` and the agent edges words for a world of agents. The
conversion words sit under the node-graph exemption and are outside that test.

`Sentences::new(&document)` holds the sheet's names by id and writes `card_heading` ("Each tick,
every cell", or "Lines of Clear: run once, every cell, when pressed" for a card that runs once,
named by the button that runs it) and `line(line_id, otherwise)`, where `otherwise` holds the
numbers of the higher lines that override the line ("otherwise (when line 2 does not apply) become
…"). The sentence decides no "otherwise" of its own. The writer passes
`CardRelations::otherwise_numbers`. A blank reads `…`, an unknown reference as its id, an
unnamed Count result as its automatic name, and another unnamed result as its line word ("line
4"), since its automatic name can read the same chip again. Words
follow the wording table: "Become X from Y" ("from any state"), ", always", "when my Cargo is
Searching" (no "I am"), "is none of the numbers 2, 3" and "is one of the states" when the left reads
a States property, "count of … whose X, only those whose Y", "Set Hit to yes with chance P,
otherwise no", "the 3 by 3 block including me", "for each infected neighbour" through
`for_each_words` (a name whose last word is not a plural reads "for each of B in my block"), a
percent as typed ("30%"), a Compare line's name hidden while it equals its sentence, a Calculate
formula's operators as "plus", "minus", "times", "divided by" and "to the power of" with a call's
parentheses left out and a Calculate inside a slot in parentheses. Agent lines keep those
conventions, "including me" for `include_me`, an all of inside a slot joined by "and" with its draws
after commas. A state chip that another States property also names reads "Status Alive".
`literal_words` is public for an editor field that shows a literal, and `read_typed_number` reads a
typed number back (a trailing "%" makes a percent, "inf" and "nan" are refused), with
`world_side_out_of_range` for a grid side that is no whole number from 1 to the limit. `line_parts`
returns the same sentence as `SentencePart`s, which `line` joins through `sentence_text`: words, a
bold `ConditionWord`, `From`, a `Chip` (`ChipKind` and a grey `lead` such as "my"), a `Blank`, a
`Select`, `Always`, `Otherwise`, the empty `ForEachBlank`, which reads as nothing, and `ForEach`,
the words "for each" once on (`FOR_EACH_WORDS`). A `Keyword` is a lead word the card sets in the
semibold face: the verb of a change ("Become", "Set", "Steer", "Keep", "Move", "Leave", "Record"),
"Repeat" before its times, "When" before a blank or "always" (a When with any other condition
opens with the `ConditionWord` "When", "When with chance" or "When at rate"), and the words that
name what a result line
works out ("count of", "look at", "all of", "any of", "none of", "choose, first that applies" with
its "otherwise", "depending on"). A phrase inside a slot writes the same words as plain `Text`
(`Parts::lead`). A `ResultName` is the name before " = " on a named result line, which the card
draws as the line's result chip. An `Alternative` reads as nothing and opens each row of a Choose
and each branch of a Depending on, which the card draws on a row of its own. `map_first_text` lowers
the first letter of a lead word as of words, so "otherwise ... become" keeps its case. `Competing`
is the "or" (`COMPETING_WORD`) that `competing_parts` puts before a lower line of a competing group,
in the card alone. One builder (`Parts`) writes both forms, so the card and the writer never differ.
`placed_parts` returns the same parts as `PlacedPart`s, each with the `PartPlace` an edit of it
sets: a `target` (`PlaceTarget::Slot` with its slot path, `ListItem` with the list's slot path and
the item's index, or `Field` with the phrase's slot path and a `FieldPlace`: `Property`, `NewState`,
`StartState`, `Test`, `Around`, `RateUnit`, `Summary`, `SummaryValue`, `Operation`, `FirstOperand`,
`SecondOperand`, `Mode`, `Pick`, `RankedBy`, `Which`, `GroundProperty`, `Event`,
`LookSummary(index)`) and the `want`, the `Blank` the place shows while empty (`None` for a select).
`Parts` places a value relative to its slot and `Parts::under(key, want)` puts it under the slot's
key, so a phrase's places compose (`condition.left`). Words, `From`, `Otherwise` and a hand-written
formula of more than one operation have no place. A Count's `whose` comparison wants a property on
its left and, while that is empty or a States property, a state on its right, and an empty Calculate
line shows a number blank, the operation select and a number blank ("Result = … … …" as text),
both values and the select placed. Agent lines place
their property (`property_part`), modes, picks and cells as selects, each summary of a look at
agents (`LineSelect::LookSummary`), a Leave's and a pick's ground property, a Record's event
(`ChipKind::Event`), every state of a Depending on, Keep between and Random number bounds as the
lower and upper number, and a Repeat's `Blank::Times`. A Set over agents wants a property and a
value. The agent menus are `AddButton::AGENTS` with `items_for`, `tooltip_for`, `heading_for`.
`AddButton::for_kind` returns the group of a `LineKind`, the button whose menus add such a line (a
Become belongs to Change wherever it was added), which the card's kind bar and the add buttons
share. The words "for each" and their count share the place `tries`. `binary_formula` splits a
formula of one operation between two values, and `Sentences::formula_operand` returns the chip of
one value as a formula writes it.

**Tests** of the wording table and the sentences sit in `src/tests/sheet_sentence.rs`:
`every_wording_table_entry_equals_the_final_words` holds every entry's words as literals and
compares each entry, a template called with an example; the starters' and
test sheets' sentences, Game of Life's three exactly; the parts of its lines, blanks, "otherwise"
and the empty "for each"; `for_each_words`; and no fixed entry holding a forbidden word, and
`lead_words_result_names_and_alternatives_leave_every_sentence_as_written`, which holds every line
of every starter and test sheet to the sentence its comment holds, both through `sentence_text` and
with its `Keyword`, `ResultName` and `Alternative` parts read as their words, each change line to a
lead word, each named result line to its `ResultName` and each Choose
and Depending on to its alternatives. The places of parts are held by
`every_control_of_a_life_line_is_placed_at_the_slot_or_field_it_shows`,
`a_new_count_wants_a_property_then_a_state_and_an_empty_calculate_its_two_values`,
`the_words_for_each_are_a_part_placed_at_its_count` and
`a_formula_inside_a_slot_places_no_value_and_no_operation` in `src/tests/sheet_sentence.rs`. A
formula inside a slot has no places, and the card edits the values and the operation of a Calculate
line only.
