# henad-graph: the sheet checker and limits

`sheet/check/` (`sheet::check`) checks a sheet document, and `sheet/limits.rs` holds the limits the
reader, the checker and the lowering hold a sheet to. `agents/henad-graph-sheet.md` holds the rest
of the sheet module, the layout of `sheet/check/` included, and
`agents/henad-graph-sheet-wording.md` the words of the checker's messages.

**Limits.** `sheet/limits.rs` holds the sheet's limits apart from `crate::limits`, so `library.lock`
stays as it is: `MAX_SHEET_BYTES` (1 MiB), `MAX_NESTING` (24), `MAX_LINES_PER_CARD` (256),
`MAX_NODES_PER_LINE` and `MAX_SHEET_NODES` (999 each, for the lowering's `n(1000k + j)` ids),
`MAX_SHEET_ID_BYTES` (48, so `tick_<card>` and `button_<button>` stay within the IR's 64-byte ids)
and `MAX_FORMULA_NESTING` (32 groups of parentheses, calls and `^` right sides, checked as the
formula parser descends, so a deep formula is an error instead of a stack overflow) and
`MAX_FORMULA_DEPTH` (16 operations nested in a formula the lowering writes, each operator, negation
and call one level, since `math.expr` reads 64 levels and its text spends up to three per
operation, so a flat sum of 17 terms is past it; the checker holds a Calculate to it through
`formula::checked_references`, and the message asks for parentheses, which make a sum shallower).
The reader enforces the first three. `ADD_UP_RESOLUTION_SHIFT` (16) is the resolution 2^-16 of a
ground property that adds up its marks, which the lowering and the checker's warning share, and
`MARKS_TOOLTIP` states its bound (`the_marks_tooltip_states_the_bound_of_add_up`).

**The checker.** `check::check(&document)` returns diagnostics with no span in a stable order (the
tables outside the cards, then every card's line ids, then each card's lists and its lines in
display order), and `ParsedSheet::check` places each at its table (`[card.line.l2]` for a line, the
`[[parameter]]` header for a parameter) through `SheetPlaces` and sorts them by place.
`SheetDiagnostic` holds the `CodeDiagnostic`, the card index, the line id and `skips_line`.
`SheetReport` has `has_errors`, `errors`, `warnings`, `is_skipped`, `skip_note` and `skipped`.
Errors (file-level, technical messages in the graph checker's style): version, the id grammar and
`MAX_SHEET_ID_BYTES` for every id, the reserved ids (`my`, `my_new`, the formula and part words,
`l<digits>`, and in the item namespace `grid_width`, `grid_height`, `num_agents`, `world_width`,
`world_height`, `tick_length` and `henad_*`, for a kind of thing `cells` and `ground`, for a state
`any_state`), a Look around's summary ids (grammar, length, the reserved words and `l<digits>`, one
id per line), the namespaces of author-format section 2.2, line ids (form, below `next_line`, one
card, one list, reachable, listed ids held), lines on a non-block kind, the world's kinds, the
world's sides and a kind's `max_count` against `crate::limits` (`MAX_GRID_SIDE` for cells,
`MAX_WORLD_SIDE` for agents, `MAX_AGENTS`) at `[world]` and `[[agents]]`, so the graph checker's
own limits never place an error at `[sheet]`,
`ONE_AGENT_KIND` on every agent kind after the first, property types, marks, starts (states, keys, a
rate in a start), parameters (`parameter_id_reserved` at a parameter whose id is one of the graph
checker's `RUNS_CSV_COLUMNS`, and the default inside the range), measures
(`measure_name_reserved` at a measure whose name,
its stat's label and column, is one of the graph checker's `RESERVED_STAT_LABELS`, `run_id`,
`tick` or `time` compared exactly), colour rules, buttons (a run-once step names a card
that runs once), a card's kind of thing (`cells` in any grid world), a name on a change, `always` as
a line or outside a condition, a When that is always, `outside` outside a condition, a phrase of a
change kind, unknown parameters, states of another property, `their` and relations outside a look,
results from another card or of a change line, parts, places, value types (a slot's expected type
against a literal, chip or phrase, a Steer's value against the type of the property it steers, a
largest or smallest summary's value against a number, an any or all summary's value against a yes
or no at the summary's own line, and `ARROW_COMPARED_WITH_NUMBER`; a Set's value against its
property, `set_takes`, a Number taking a yes or no worked out or read but never
`true` or `false`, and an Arrow a place; Keep between on a States or yes or no property; and the two
sides of a comparison, `compared`, each item of a list on its own: equal types for is and one of, no
order on states or yes or no, and "is within" over numbers and worked-out yes or no; each only where
both types are known, and only what the lowering refuses too), formulas, keys held twice,
Count, Look around and Pick a cell inside a Repeat (`LOOK_IN_REPEAT`) or a run-once card,
`RATE_IN_BUTTON_CARD`, a rate inside a Repeat (`RATE_IN_REPEAT`), and the ranges from the wording
table (`CHANCE_OUT_OF_RANGE` and `RATE_BELOW_ZERO` on literals, a percent divided by 100,
`CHANCE_LIMITS_OUTSIDE` and `RATE_BELOW_ZERO` on parameters). Agent-only fields with no editor words
yet (a Steer or Move mode, a pick rule, `which`, an event, `agent_kind`, `include_me`) are errors
when absent. Warnings that keep the line running: a draw with no key, two result lines with one
name, `SHEET_EMPTY` at `[world]` for a grid sheet with no cells property, a ground property's share
kept each tick outside 0 to 1 (`Checker::kept_range`: `kept_outside_note` on a literal,
`kept_limits_outside_note` on a parameter whose Max is above 1 or Min below 0; the lowering
multiplies by the share unclamped, and each note says what that does), and `add_up_limit_note` at
a Leave of a number or a parameter's maximum on a ground property that adds up its marks, when the
kind's `max_count` times the amount reaches 2^(32 - `ADD_UP_RESOLUTION_SHIFT`), 65536, the most one
cell adds up in a tick (`add_up_limit`), or `add_up_worked_out_note` at a Leave of any other amount,
worked out as the model runs and unbounded, when `max_count` alone reaches 65536. Skip warnings,
one per line, the first problem in the line's sentence order: a blank slot or select reads
`not_run_choose` with `Blank::want` or `LineSelect::want` ("Not run yet: choose the old state"), an
empty list reading as its slot's blank (the sentence draws it as one too), Become's property
counting as its new state; a result of a line no card holds reads the same; a result below its user
reads `uses_result_below` and the user's own result `USES_OWN_RESULT`; a result inside a When or
Repeat that does not hold the user reads
`uses_result_inside`; `my_new` at or above the last change reads `uses_result_below("my new X")`,
and with no change at all as a blank. After the slots, in order: a result of a skipped line above
reads `not_run_result_unfinished`, a rate with no `[time]` reads `NOT_RUN_TICK`, and a change, When
or Repeat inside a skipped block, or any line with a skipped Repeat among its blocks at any depth,
reads `not_run_when_unfinished` with the direct parent's number, or else the nearest skipped
Repeat's. A result without a name reads in a note as its chip does (`result_name`): an unnamed Count
by its automatic name, another by its line word, and the note of such a result on a skipped line is
`not_run_when_unfinished`. Outside the cards a blank or unusable value is an error. A value's type
walk through results stops past `MAX_NESTING`. A rate whose `rate` is a parameter whose Unit names
another unit (`edit::unit_in_text`) than the line's `rate_unit` warns `unit_mismatch` at the line
and keeps running in the line's unit. A Record with no event, a Leave or a pick with no ground
property and a Repeat with no times skip their line in the blank's words, and so does a Depending on
with no value for a state of its property (a new state included), each missing value a
`Blank::Value` in state order. A value keyed by no state of the property is an error, and a property
of another type is one error for the whole line. `changes_property` is the rule of `my_new`, public
for the card's picker.

**Tests.** Checker tests sit in `src/tests/sheet_mutation.rs` (the 25 cases of the author format's
mutation run, one test each on a committed sheet edited in the test, each asserting error, warning
or skip and its table; the rate in a run-once card; the chance and rate ranges;
`the_sheet_checker_never_panics_on_the_mutation_corpus`, whose edits each sheet draws from its own
seed, so a sheet added to the corpus leaves the others' edits alone) and `src/tests/sheet_check.rs`
(`every_sheet_checks_clean`, file order, the otherwise rule with exclusive and overlapping tests,
the starters, the forest fire, the ants' Set lines, competing groups, skipped lines and blocks, the
When scope, `every_skip_note_equals_its_wording_table_entry`, reserved ids, the second agent kind,
the empty grid sheet, and the relation notes:
`the_order_note_shows_on_game_of_life_and_sir_and_not_on_the_vote_or_the_forest_fire`,
`the_vote_is_covered_and_a_gap_between_two_thresholds_is_named` (with skipped lines that take the
gap note away) and `a_when_offers_to_fold_into_its_lines_and_notes_what_holds_inside_it`).
