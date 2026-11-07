# henad-graph: sheets

**`henad_graph::sheet`**: the author document, a model file of `kind = "sheet"` that the researcher
builds on the sheet and its cards. It sits inside henad-graph, so it takes no new dependency edge,
and it reads no file at run time, so it compiles on wasm32. `agents/henad-graph.md` holds the rest
of the crate.

Modules. `sheet/mod.rs` holds the module doc (sheet, card, line, slot, chip, phrase, formula),
`KIND_SHEET`, `SHEET_VERSION` (1) and `SheetFileError` (`Toml` and `Format`, each a
`CodeDiagnostic`, as `GraphFileError`). `sheet/document.rs` holds `SheetDocument` and its tables as
plain public data, every type `#[non_exhaustive]` with a constructor (`new`, or `Default` for
`SheetGround`, `StartsAsRow` and `ChooseRow`), every enum with `ALL`, `as_str` and `parse` from the
private `stored_names!` macro, which also implements the crate-private `StoredName` trait the reader
reads names through. `SheetDocument` writes its own `Debug` ending in `finish_non_exhaustive`.
`sheet/reader.rs` is private, and `sheet` re-exports `ParsedSheet`, `SheetPlaces` and `TablePlaces`.
`sheet/starters.rs` is private, and `sheet` re-exports `starters`. `sheet/sentence.rs` is public
(`sheet::sentence`), the wording table and `Sentences`. `sheet/writer.rs` is private, and `sheet`
re-exports `format_number`. `sheet/relations.rs` is public (`sheet::relations`), `CardRelations`.
`sheet/check/` is public (`sheet::check`): `mod.rs` holds `check`, `SheetReport`, `SheetDiagnostic`,
`ParsedSheet::check`, the private `Checker`, and `Place`, `Thing` and `span_of`, visible to `sheet`
for the lowering's errors, with `formula::references` visible to `sheet` for the lowering's read of
an unread Heading; `declarations.rs` the tables outside the cards; `lines.rs` the cards,
lines and values; `formula.rs` the Calculate formula parser, visible to `sheet` with `formula_chip`
for the lowering (`references` for the checker, `parse` returning a `FormulaTree`, built only by
`parse`, so a long sum read for its references takes no stack to drop). `sheet/file.rs` is private,
and `sheet` re-exports `SheetFile`. `sheet/lower/` is private, and `sheet` re-exports `LineMap`,
`NodeOrigin` and `NodeOwner` from `lower/table.rs`. `SheetLine::slot_values` (crate-private,
`document.rs`) returns every value in a line's slots, for the checker and the lowering.
`sheet/edit/` is public (`sheet::edit`), the edit commands that `agents/henad-graph-sheet-edit.md`
describes: `mod.rs` holds `SheetBuilder`, `SheetEdit`, `LineField`, `SheetItem`, `MoveDirection`,
`SheetEditError`, `IdSpace`, `mint_id`, `STATE_COLORS`, `MEASURE_COLORS` and
`suggested_state_color`; `names.rs` `automatic_name` and the rule that keeps names up to date;
`references.rs` every place an id is named; `lines.rs` slot paths, phrase depth and the outline
moves.
`sheet/convert.rs` and `sheet/changes.rs` are private, the conversion to a node graph copy that
`agents/henad-graph-sheet-convert.md` describes.
`sheet/limits.rs` holds the sheet's limits apart from `crate::limits`, so `library.lock` stays as it
is: `MAX_SHEET_BYTES` (1 MiB), `MAX_NESTING` (24), `MAX_LINES_PER_CARD` (256), `MAX_NODES_PER_LINE`
and `MAX_SHEET_NODES` (999 each, for the lowering's `n(1000k + j)` ids), `MAX_SHEET_ID_BYTES` (48,
so `tick_<card>` and `button_<button>` stay within the IR's 64-byte ids) and `MAX_FORMULA_NESTING`
(32 groups of parentheses, calls and `^` right sides, checked as the formula parser descends, so a
deep formula is an error instead of a stack overflow) and `MAX_FORMULA_DEPTH` (16 operations nested
in a formula the lowering writes, each operator, negation and call one level, since `math.expr`
reads 64 levels and its text spends up to three per operation). The reader enforces the first three.

**The document.** Tables follow the sheet's order: `sheet` (`SheetHeader`: version, id, name,
purpose, next_line), `world` (`MadeOf`, width, height, `Edges`), `time` (`SheetTime` with
henad-core's `TimeUnit`), `cells` (`Option<Vec<SheetProperty>>`, `None` with no `[cells]`), `agents`
(`AgentKind`, more than one of which reads), `ground` (`SheetGround`: `blocked`, properties),
`events`, `parameters` (`SheetParameter` with a free-text `unit`), `measures`, `colors` (a map by
thing id), `buttons` and `cards`. A property holds its `PropertyType`, states (each colour a `[u8;
4]`), `marks`, `kept_per_tick` and `StartsAs`. A card holds `lines`, the top-level ids in display
order, and `line_tables`, every line by id. A line is one `SheetLine`: a `LineKind` and every field
and slot any kind holds. The reader takes any line key on any kind. A field the file leaves out is
`None` or empty, and a slot is `Slot::Blank`, which is how a half-built line is stored. A filled
slot holds a `SheetValue`: a `SheetLiteral` (number, percent or bool), a `SheetChip` (`Param`,
`Result` with `part` and `property`, `My`, `MyNew`, `Their`, `Relation`, `State`), a phrase
(`Box<SheetLine>`), or a `List` of literals and chips. A formula is the `formula` field of a
Calculate phrase. The stored forms of the final editor: `{ kind = "always" }` (`LineKind::Always`)
for a condition that always holds, and `start_state = "any_state"` for a Become from any state, an
absent key in either being a blank; `test = "outside"` with `low` and `high`, a condition slot's
alone; `rate_unit_from_tick = true` beside `rate_unit` while the unit came from the tick, read as
`false` when absent; `{ percent = 30 }` for a number typed as a percent (D110), a literal that
stands wherever a number can, in a list included. `SheetLiteral::Percent` keeps the number as typed
(30), and `SheetLiteral::number` returns the number a literal stands for, a percent's divided by
100, so S5's ranges and S7's lowering read 30% and 0.3 alike. A parameter's default, min and max
stay plain numbers. `branches` is a map by state id, so the writer orders it by the property's
states.

**The reader.** `ParsedSheet::parse` checks `MAX_SHEET_BYTES` before parsing, then the header
through `model_text::ModelHeader` with the kinds `["sheet"]`, then `[sheet] version`, read with the
header so a newer file is refused on its key before an unknown key is ("the file is sheet version 2,
and this build reads sheet versions up to 1", and below 1 "sheet versions count from 1"). The reader
has no migration hook. It then parses the text once into a span tree, refuses tables and lists
nested past `MAX_NESTING` at the deepest one, reads the typed tables, and builds the places from the
span tree. The depth check comes first because each nested phrase is a large frame of the typed
read: a test thread's 2 MiB stack overflowed between 30 and 40 levels in a debug build, and TOML's
own limit sits near 80 (`nesting_past_the_limit_is_refused_without_a_panic` holds the limit and one
past it). Every table is `deny_unknown_fields`, and an unknown key's message drops serde's list of
expected keys ("unknown field `mystery`"). Enum values, time units, colours (`#RRGGBB` or
`#RRGGBBAA`, either case) and finite numbers are checked on their bytes as `Format` errors ("the
pick rule 'best' is not one of most, least or random"). A table in a slot is a percent when it holds
`percent`, a phrase when it holds `kind`, and a chip otherwise. A percent holds `percent` alone and
a finite number (`PercentNumber`, an integer or a float), and is refused on the other key or on the
number. A phrase refuses a chip key and `percent`, a chip refuses a line key, and a chip names
exactly one thing, `part` and `property` only beside `result`. A list holds literals and chips only,
and a line table without `kind` is refused at its header. A datetime anywhere reads as "a sheet
holds no dates or times", since the TOML crate passes it as a table under its own private key.

**The writer.** `SheetDocument::to_text` returns the canonical text, `Result<String,
SheetWriteError>` (the key and value of the first NaN or infinity). It builds a `Record` tree of
`Entry` values (`Inline`, `Rows` for an inline array with one element per line, `Table`, `Tables`,
`Map`) in the author format's key order, then writes it as the reference writer does
(`src/tests/golden/README.md`): inline keys before sub-tables, one blank line before each header, no
header for a table with no inline key that holds tables, `[card.line.<id>]` in line id order by
number, `[color.<thing>]` in thing order (cells, agent kinds, ground, then lexical), `branches` in
the property's state order. An absent field and a blank slot write nothing, `rate_unit_from_tick`
only when true, a When's `lines` only when non-empty, `Some(empty)` cells or ground as a bare
`[cells]` or `[ground]`. A phrase in a slot is a standard sub-table, in `items` an array of tables
of chips and phrases (an inline array, phrases as inline tables, when an item is a literal), in
`branches` an inline table. A key TOML cannot read bare is quoted, and a control character in a
comment becomes a space. Comments: `Sentences::card_heading` above each `[[card]]` and
`Sentences::line(id, &relations.otherwise_numbers(id))` above each line table. `format_number` is
the number rule: integer within 2^53 (`-0` as `0`), otherwise the shortest round-trip digits from
`{:e}` laid out by hand, plain for 10^-4 <= |x| < 10^16, else `1e-5` style; `None` for NaN and the
infinities. Every committed sheet is the writer's output.

**The wording table.** `sheet::sentence` holds every word the sheet editor shows, from the final
editor's wording table: constants for fixed text and functions for templates, each named for what it
words (`card_header`, `otherwise_words`, `not_run_choose`, `gap_note`, `for_each_words`), with the
enums `AddButton` and `AddItem` (labels, tooltips, headings, patterns; `+ Chance` has the tooltip
"Change a state by chance"), `ConditionItem` ("Choose when"), `Blank` (visible word, `want`,
accessible name, picker title), `LineSelect`, `DrawKind` and `ConditionKind`. A template takes the
line number, the line word ("line 2", "Clear line 1") or the line prefix ("Line 2", "Clear line 1")
it needs. Beyond the card it holds the sheet (`SheetSection` with each heading, help text and
example, the World and Time words, the property form and its checks, Starts as with `StartsField`
and the `starts_*_place` functions, the Parameters, Measures, Colours and Buttons words,
`SHEET_EMPTY`), the New model and New parameter dialogs, the top bar, the world tabs
(`tick_counter`, the tried-value words, `PLOT_EMPTY`, `WORLD_EMPTY`), the Diagram and the
conversion. Rows the wording table marks "Not built" are left out. The rate notes are templates
taking numbers already formatted (`rate_note`, `rate_for_each_note`, `worked_out_rate_note`,
`growing_rate_note`, `chance_for_each_note`, `growing_chance_note`, `competing_note`,
`mean_stay_note`). The words the plan adds for its own decisions sit in one group
(`RATE_IN_BUTTON_CARD`, `ONE_AGENT_KIND`, competing rates, the long tick, `chances_per_tick_note`),
with the interim refusals (`NOT_YET_RATE`, `NOT_YET_REPEAT`, `NOT_YET_REPEAT_CHANGE`,
`NOT_YET_GROUND`, `NOT_YET_PICK_CELL`, `NOT_YET_LOOK_AT_CELL`, `NOT_YET_MOVE_TO`, `NOT_YET_LEAVE`,
`NOT_YET_RECORD`, `NOT_YET_AGENT_WALLS`), a property changed two ways (`set_already`,
`steered_already`, `moved_by_already`, `moved_to_already`) and the words of a look at agents
(`look_name`, "line 3, Flockmates") and of its cost (`look_costs_too_much`, `card_costs_too_much`,
`start_costs_too_much`, counts written with thousands commas, and for a pass past its cost per
element `line_costs_too_much`, `smaller_maximum`, `FEWER_TIMES` and `FEWER_LINES`). `plural` is
crate-visible for `SheetFile::shortfalls`. Every editor milestone and the writer's comments draw
their words from here and from no literal of their own. `FORBIDDEN_WORDS` lists the words sheet mode never shows. The
conversion words sit under the node-graph exemption and are outside that test.

`Sentences::new(&document)` holds the sheet's names by id and writes `card_heading` ("Each tick,
every cell", or "Lines of Clear: run once, every cell, when pressed" for a card that runs once,
named by the button that runs it) and `line(line_id, otherwise)`, where `otherwise` holds the
numbers of the higher lines that override the line ("otherwise (when line 2 does not apply) become
..."). The sentence decides no "otherwise" of its own. The writer passes
`CardRelations::otherwise_numbers`. A blank reads `...`, an unknown reference reads as its id. Words
follow the final editor: "Become X from Y" ("from any state"), ", always", "when my Cargo is
Searching" (no "I am"), "is none of the numbers 2, 3" and "is one of the states" when the left reads
a States property, "the 3 by 3 block including me", "for each infected neighbour" through
`for_each_words` (a name whose last word is not a plural reads "for each of B in my block"), a
percent as typed ("30%"), a Compare line's name hidden while it equals its sentence, a Calculate
formula's operators as "plus", "minus", "times", "divided by" and "to the power of" with a call's
parentheses left out and a Calculate inside a slot in parentheses. Agent lines keep round 1's words
in those conventions, "including me" for `include_me`, an all of inside a slot joined by "and" with
its draws after commas. A state chip that another States property also names reads "Status Alive".
`line_parts` returns the same sentence as `SentencePart`s, which `line` joins through
`sentence_text`: words, a bold `ConditionWord`, `From`, a `Chip` (`ChipKind` and a grey `lead` such
as "my"), a `Blank`, a `Select`, `Always`, `Otherwise` and the empty `ForEachBlank`, which reads as
nothing. One builder (`Parts`) writes both forms, so the card and the writer never differ.

**Places.** `SheetPlaces::root` is a `TablePlaces` tree that follows the TOML structure: `table`
(the header, an inline table's braces, or the first key naming an implicit table such as
`card.line`), `keys` (each value's bytes) and `tables` (one entry under a key holding a table, one
per element under a key holding a list with a table in it). `SheetPlaces::line(card_index, line_id)`
reaches a line table. Places are offsets into the text with line ends made `\n`, and
`ParsedSheet::place` maps a diagnostic back to the text as passed.

**Starters.** `sheet::starters()` returns the sheet starters as `crate::starters::Starter` values
(file name and text), embedded with `include_str!` from the crate's `sheets/` directory, since the
packaging check refuses a path out of the crate: the grid sheets `game_of_life.toml`, `vote.toml`
("Majority vote") and `forest_fire.toml`, which entered with the chances' lowering, then the agent
sheets `random_walk.toml`, `boids.toml` and `bounded_confidence.toml`, which entered with the agent
lowering. A sheet enters `sheets/` and `starters()` only in the milestone whose lowering runs it,
and a test looping over `starters()` passes at every merge. The app's New model dialog lists only
the starters whose world it offers (`starter_choices`), the grid sheets today. Until then a sheet is
test data in `src/tests/sheets/`: the ants until the ground lowers, SIR with rates once rates lower.
`sir_chance.toml` and `mobile_sir.toml` (chances once for each infected person nearby) stay test
data. `sir_chance.toml` has a copy in henad-models' `tests/fixtures/sheets/` for the device gates.
No test holds the two equal, and an edit to this one is copied there. The graph starters stay graph
files in `starters/`.

**Relations.** `CardRelations::new(&document, &card)` holds a card's display order (a block's lines
after the block line, an id listed twice or inside its own block taken once at its first place, an
id with no table left out), each line's block (`parent`, `is_within`) and number (`number`, from 1).
Two changes collide when both are Become or both Set, on one property, and for Become both name a
start state (`any_state` counts, a blank does not). `can_both_apply(a, b)` is false when a test of
`a` and a test of `b` read one value and no value passes both. A line's tests: its own condition,
the condition of each When around it, and a Become's named start state as "my X is S" (a state reads
as its index). An all of gives each item, nested all of included, and any of, none of, a result
chip, a draw and a `within` give nothing (gate decision D80). A test reads `my` or a result chip
(with its part or property) against fixed numbers, percents or states, `outside` included. Two tests
exclude each other when no tried value passes both: each listed number, its neighbours (+-1 for
whole values, +-0.5 otherwise), the midpoints, and the ends +-2, with a 1e-9 allowance. Whole means
a States property, a Count result, or a Look around's `count` part or Count summary.
`otherwise_of(line)` returns the higher colliding lines in display order, `otherwise_numbers` their
numbers. `competing_groups()` returns two or more Become lines of one property from one named start
state (not `any_state`), each condition holding exactly one rate and no chance, an all of, any of or
none of searched, and `otherwise_of` leaves a partner in the line's own group out (decision D1's
"or").

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
card, one list, reachable, listed ids held), lines on a non-block kind, the world's kinds,
`ONE_AGENT_KIND` on every agent kind after the first, property types, marks, starts (states, keys, a
rate in a start), parameter defaults, measures, colour rules, buttons (a run-once step names a card
that runs once), a card's kind of thing (`cells` in any grid world), a name on a change, `always` as
a line or outside a condition, a When that is always, `outside` outside a condition, a phrase of a
change kind, unknown parameters, states of another property, `their` and relations outside a look,
results from another card or of a change line, parts, places, value types (a slot's expected type
against a literal, chip or phrase), formulas, keys held twice, Count, Look around and Pick a cell
inside a Repeat or a run-once card, `RATE_IN_BUTTON_CARD`, a rate inside a Repeat, and D75's ranges
from the wording table (`CHANCE_OUT_OF_RANGE` and `RATE_BELOW_ZERO` on literals, a percent divided
by 100, `CHANCE_LIMITS_OUTSIDE` and `RATE_BELOW_ZERO` on parameters). Agent-only fields with no
editor words yet (a Steer or Move mode, a pick rule, `which`, an event, `agent_kind`, `include_me`)
are errors when absent. Warnings that keep the line running: a draw with no key, two result lines
with one name, and `SHEET_EMPTY` at `[world]` for a grid sheet with no cells property. Skip warnings
(D39), one per line, the first problem in the line's sentence order: a blank slot or select reads
`not_run_choose` with `Blank::want` or `LineSelect::words` ("Not run yet: choose the old state"),
Become's property counting as its new state; a result of a line no card holds reads the same; a
result at or below its user reads `uses_result_below`; a result inside a When or Repeat that does
not hold the user reads `uses_result_inside` (D84); `my_new` at or above the last change reads
`uses_result_below("my new X")`, and with no change at all as a blank. After the slots, in order: a
result of a skipped line above reads `not_run_result_unfinished`, a rate with no `[time]` reads
`NOT_RUN_TICK`, and a change, When or Repeat inside a skipped block, or any line with a skipped
Repeat among its blocks at any depth, reads `not_run_when_unfinished` with the direct parent's
number, or else the nearest skipped Repeat's. A result without a name reads as its sentence in a
note. Outside the cards a blank or unusable value is an error. A value's type walk through results
stops past `MAX_NESTING`.

**Sheet files.** `SheetFile::parse(text)` reads (the reader's errors), checks (the checker's errors
as `SheetFileError::Invalid`, every one placed), lowers (the lowering's problems, also `Invalid`,
placed at their tables through `check::span_of`) and reads the lowered document back through
`GraphFile::from_document`. It keeps the `ParsedSheet` (text, places, line ends), the canonical text
(`to_toml`), the checker's warnings placed (`warnings`, skip warnings included), the lowered
`GraphFile` (`lowered`), the `LineMap` (`line_map`) and the `FaultNames` of its looks at agents
(`fault_names`, D47). `code()` is the lowered graph's `code()` (D8): the IR's `name` is `[sheet]
name`, both descriptions stay empty, and nothing else of the sheet text enters it. `Invalid` holds a
`SheetErrors`, never empty, since `SheetErrors::new` takes a first error and the rest.
`SheetFileError::diagnostics` returns every diagnostic, `diagnostic` the first, and `Display` writes
each on its line. `SheetFile` and `LineMap` write their own `Debug`.
`SheetFile::place_graph_diagnostic` maps a diagnostic of the lowered graph (its checker, host
evaluator or compiler) back into the sheet (R2, D46): a node through `LineMap` to its line's table,
found by the card whose `line_tables` hold the line, a node the sheet owns by the first two segments
of its slot path (`cells.<property>`, `agents.<property>`, `measure.<id>`, `button.<id>`,
`card.<id>`, `color.<thing>`), a node the map does not know (a normalisation copy, a planted node)
to `[sheet]`, and a diagnostic with a pass and no node by the pass id (`tick_<card>`, `button_<id>`,
`restart_<id>`) or to `[sheet]`. Its old span, line and column are replaced and its `pass` and
`node` cleared, so henad-cli's `located` prints no node id, and `--print-lowered` keeps the graph
within reach. henad-explore's `ModelFile` calls it for a sheet's entry errors, and leaves the
lowered graph's warnings out of a sheet's warnings. For a sheet of agents it compiles the lowered
graph through `AgentProgram::compile_with_fault_names` with `fault_names`, so a capped fold's fault
names its author lines. `SheetFile::shortfalls(entry, params, limits)` is X4's one cost mapping: the
entry's shortfalls, with the work lines (any line holding "units of work") replaced when the
heaviest submission or a host run passes its budget. A submission past `SUBMISSION_WORK_BUDGET`
names each fold of the heaviest step's pass (label `<sheet id>_<pass id>`) by its `FaultNames` words
through `look_costs_too_much` ("Line 1, Flockmates: looks at too many birds at 1,000,000 birds. A
shorter distance or fewer birds would fit."), or the card through `card_costs_too_much` when the
pass holds no fold. A host run past `HOST_WORK_BUDGET` reads `start_costs_too_much`. The population
is `num_agents`, or `grid_width` times `grid_height`, read from the entry's parameters. The
checker's per-element cost (V22) is placed at the heaviest node's line by `place_graph_diagnostic`,
which recognises it by `check::is_pass_cost` (the message's start and its `MAX_PASS_COST` end, built
in `check/cost.rs` beside the message), rewords it through `SheetFile::cost_words` and drops its
note: `line_costs_too_much` names the line by `look_name` ("Line 1: too much work for one walker. A
smaller maximum for Steps per tick would fit."), with `smaller_maximum` for a Repeat over a
parameter, `FEWER_TIMES` for a Repeat of a number and `FEWER_LINES` for any other line, or names the
card by its heading or a button by its name. `SheetFile::parse_planted` (`#[cfg(test)]`,
crate-private) lowers with a closure run on the lowered document before it is read back, which the
mapping tests use to plant a node the checker refuses.

**The lowering** (`sheet/lower/`) has its own file, `agents/henad-graph-sheet-lower.md`: the grid
and agent lowering, node ids, keys, the line table, the interim refusals and their tests.

**Tests** of the wording table and the sentences sit in `src/tests/sheet_sentence.rs`:
`every_wording_table_entry_equals_the_final_words` holds the final wording table's rows as literals
copied in and compares each entry, a template called with the table's example; the starters' and
test sheets' sentences, Game of Life's three exactly; the parts of its lines, blanks, "otherwise"
and the empty "for each"; `for_each_words`; and no fixed entry holding a forbidden word. Writer
tests sit in `src/tests/sheet_writer.rs`: `every_sheet_is_a_fixed_point`,
`the_rust_writer_agrees_with_check_py_apart_from_comments` against `src/tests/golden/` (the
reference writer's text for the four author-format examples with comment lines removed;
`golden/README.md` holds the procedure, the script's SHA-256 and the rules in full),
`a_hand_edited_sheet_reads_back_to_its_canonical_text`,
`unusual_values_read_back_to_the_same_document`, `a_number_that_is_not_finite_is_refused`,
`numbers_follow_the_sheet_number_rule`, `every_line_sentence_equals_its_comment`, the three diffs of
the author format (a new waning line in four hunks, a Steer reorder in one, a parameter rename in
two, through a line diff in the test) and `every_readable_mutation_writes_text_that_reads_back` over
the reader's mutation corpus. Checker tests sit in `src/tests/sheet_mutation.rs` (the 25 cases of
the author format's mutation run, one test each on a committed sheet edited in the test, each
asserting error, warning or skip and its table; the rate in a run-once card; D75's ranges;
`the_sheet_checker_never_panics_on_the_mutation_corpus`) and `src/tests/sheet_check.rs`
(`every_sheet_checks_clean`, file order, the otherwise rule with exclusive and overlapping tests,
the starters, the forest fire, the ants' Set lines, competing groups, skipped lines and blocks, the
When scope, `every_skip_note_equals_its_wording_table_entry`, reserved ids, the second agent kind,
the empty grid sheet). Reader tests sit in `src/tests/sheet_document.rs`, over ten sheets: the six
starters, and four test sheets in `src/tests/sheets/`. `sir.toml`, `boids.toml` and `ants.toml` are
the author format's examples, and `random_walk.toml`, `bounded_confidence.toml` and
`sir_chance.toml` (round 1's SIR with chances per tick, sheet id `sir_chance`) were written from
round 1's models, as `vote.toml` and `forest_fire.toml` were. `mobile_sir.toml` holds a chance once
for each infected person within reach, in a world 1 cell wide so every person reaches every other.
Every sheet holds `always` and `any_state` written in, Game of Life's Clear is a Run once card
holding "Become Dead from any state, always", and both SIR sheets name their button `new_outbreak`,
"New outbreak". The written sheets' keys were minted at random. Their text is the writer's canonical
text, comments included. The bounded confidence colour rule is `shaded` over `opinion` with no
colours, since the schema holds no ramp. The test helper `sheets()` lists the starters from
`starters()`, then the test sheets, so every starter is among the sheets the tests read.
`starters_returns_the_grid_sheets_then_the_agent_sheets` holds the list and each starter's world,
and `the_sheets_written_from_round_1_read_their_values` a value of each written sheet.
`every_sheet_reads` (places on every line table, the ants' chip in
`[[card.line.l7.row.condition.items]]`, and a depth at most half the limit),
`every_table_reads_its_values`, `a_sheet_with_an_unknown_key_is_refused_on_the_key` (also with
`\r\n` line ends), `a_newer_sheet_version_is_refused_on_its_key`,
`the_header_check_names_three_kinds_with_or`, `an_unknown_time_unit_is_refused_on_its_key`,
`a_blank_slot_reads_as_blank` (each slot kind of the Game of Life and SIR lines, a key deleted at a
time), `the_stored_forms_of_the_editor_read` (the outside test, the tick's unit mark, a list of
state chips, a half-built line, two agent kinds),
`a_percent_reads_as_typed_wherever_a_number_can_stand` (a start's row, inline and standard tables in
phrases, a compare slot, a list, and the places), `malformed_values_are_refused_on_their_bytes`,
`a_malformed_percent_is_refused_on_its_key`, `colours_and_sizes_follow_their_rules` and
`the_sheet_reader_never_panics_on_the_mutation_corpus` (its hostile values hold percents). Keep
them. The sheet files are copies, since the design folder they came from is not tracked.