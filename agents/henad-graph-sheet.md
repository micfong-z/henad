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
`CELLS_ID` and `GROUND_ID` are the ids of the two fixed kinds of thing, `cells` and `ground`.
`sheet/reader.rs` is private, and `sheet` re-exports `ParsedSheet`, `SheetPlaces` and `TablePlaces`.
`sheet/starters.rs` is private, and `sheet` re-exports `starters`. `sheet/sentence/` is public
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
describes: `mod.rs` holds `SheetBuilder` and re-exports `SheetEdit`, `LineField`, `SheetItem`,
`MoveDirection` and `SheetEditError` from `command.rs`, `IdSpace` and `mint_id` from `ids.rs`, and
`STATE_COLORS`, `MEASURE_COLORS` and `suggested_state_color` from `colors.rs`; `names.rs`
`automatic_name` and the rule that keeps names up to date; `references.rs` every place an id is
named; `lines.rs` slot paths, phrase depth and the outline moves.
`sheet/convert.rs` and `sheet/changes.rs` are private, the conversion to a node graph copy that
`agents/henad-graph-sheet-convert.md` describes.
`sheet/rates.rs` is public (`sheet::rates`): `chance_from_rate`, `rate_from_chance`, `tick_in` (one
tick in a unit, through `TimeUnit::days`), `chance_per_tick`, `mean_stay`, `parameter_defaults`,
`chance_words` (the number rule of the final wording table: three significant figures, more until
the chance prints differently from `r dt n`), `LONG_TICK_CHANCE` (0.5), `largest_count_chance`
(`LargestCount` with `is_long` and `words`, "up to 0.909 with 8" or "almost 1 with 8"; a fixed rate
with a "for each" of a Count over the cells around, or a Calculate of a fixed number times such a
Count), and
`rate_note(document, line_id, values)`, a `RateNote` (`text`, `mean_stay`, `long_tick`) filled from
the wording table's templates with the values passed (the defaults, or a run's). The rate is
the line, its condition or an item of an all of in it. A fixed rate gives `rate_note` or
`rate_for_each_note`, a Calculate of a fixed number times a Count with no "for each" gives
`worked_out_rate_note` (the formula read through `check::formula::parse`), any other
worked-out rate `growing_rate_note` with the Calculate's words. Δt prints in the rate's unit with
three figures ("0.25 days"). The highest line of a competing group takes `competing_note` and the
others none, and a group with a "for each", a worked-out rate, two units, or a line whose rate is
not its whole condition or that a block holds has no note. A line with no `[time]` has none.
`chance_note` returns the note of a chance with "for each" on the line or in its condition:
`chance_for_each_note` for a fixed chance, its chance at 1 the chance itself and at 2 by the number
rule against `2p`, or `growing_chance_note` with words first for a chance that is a result.
`count_as_draw` returns the name of the Count a chance or a rate holds as its value, with the
`DrawKind`, for the card's note and its button. `conversion` returns the `DrawKind` a typed chance
or rate can change into (none with no `[time]`, and no rate on a card that runs once),
`chance_line_numbers` the lines drawing a chance on cards that run each tick (the Time notice),
`grow_count` the nearest open Count above a Become at a fixed rate with no "for each" (or a Rate
line a Become takes whole) that counts its new state (`GrowCount`), and `parameter_rate_unit` the
unit a parameter's Unit names. The
mean stay is a Become from a named state at a fixed rate with no "for each", its rate the whole
condition and no block around it (`rate_alone`). A worked-out note names an unnamed Count by
`edit::automatic_name`. `pub(super) shortest_f32` is the conversion commands' number.

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
Calculate phrase. Two stored forms: `{ kind = "always" }` (`LineKind::Always`)
for a condition that always holds, and `start_state = "any_state"` for a Become from any state, an
absent key in either being a blank; `test = "outside"` with `low` and `high`, a condition slot's
alone; `rate_unit_from_tick = true` beside `rate_unit` while the unit came from the tick, read as
`false` when absent; `{ percent = 30 }` for a number typed as a percent, a literal that
stands wherever a number can, in a list included. `SheetLiteral::Percent` keeps the number as typed
(30), and `SheetLiteral::number` returns the number a literal stands for, a percent's divided by
100, so the checker's ranges and the lowering read 30% and 0.3 alike. A parameter's default, min and
max stay plain numbers. `branches` is a map by state id, so the writer orders it by the property's
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
infinities. Every committed sheet is the writer's output. A colour rule's `hue` (`ShadeHue`, the
seven MCS hues stored `gray` to `purple`, `ends` their steps 900 and 200), `darkest` and `lightest`
(finite numbers) shade a Number property and are written after `decades`.

**The wording table** and the sentences, their parts and places have their own file,
`agents/henad-graph-sheet-wording.md`.

**Places.** `SheetPlaces::root` is a `TablePlaces` tree that follows the TOML structure: `table`
(the header, an inline table's braces, or the first key naming an implicit table such as
`card.line`), `keys` (each value's bytes) and `tables` (one entry under a key holding a table, one
per element under a key holding a list with a table in it). `SheetPlaces::line(card_index, line_id)`
reaches a line table. Places are offsets into the text with line ends made `\n`, and
`ParsedSheet::place` maps a diagnostic back to the text as passed.

**Starters.** `sheet::starters()` returns the sheet starters as `crate::starters::Starter` values
(file name and text), embedded with `include_str!` from the crate's `sheets/` directory, since the
packaging check refuses a path out of the crate: the grid sheets `game_of_life.toml`, `sir.toml`
("SIR epidemic", rates per day and one tick of 1 day, which entered with the rates' lowering),
`vote.toml` ("Majority vote") and `forest_fire.toml`, which entered with the chances' lowering, then
the agent sheets `random_walk.toml`, `boids.toml` and `bounded_confidence.toml`, which entered with
the agent lowering, and `ants.toml`, which entered with the ground and holds a note under its last
line on its slower trail formation (answer 13a). A starter of a model henad-models also holds takes
the id `sheet_<model>` (`sheet_game_of_life`, `sheet_sir`, `sheet_boids`, `sheet_ants`), as the
graph starters take `node_<model>`, so Apply or `--model-file` adds it beside the example instead of
replacing it. The vote, forest fire, random walk and bounded confidence keep their file names
as ids. henad-models' `no_starter_takes_the_id_of_an_example_model` holds every sheet and graph
starter's id apart from `example_models()`. A sheet enters `sheets/` and `starters()` only in
the milestone whose lowering runs it, and a test looping over `starters()` passes at every merge.
The app's New model dialog lists only the starters whose world it offers (`starter_choices`), the
grid sheets today. Until then a sheet is test data in `src/tests/sheets/`. `sir_chance.toml`,
`mobile_sir.toml` (chances once for each infected person nearby) and `sird.toml` (SIR with a Dead
state, two competing rates of 0.05 per day out of Infected, its lines numbered in creation order)
stay test data. `sir_chance.toml` and `sird.toml` have copies in henad-models'
`tests/fixtures/sheets/` (`agents/models.md`) and henad-app's `src/tests/sheets/`
(`agents/henad-app-card.md`), and `the_copies_of_the_test_sheets_equal_their_source` fails when one
differs. The graph starters stay graph files in `starters/`.

**Relations.** `CardRelations::new(&document, &card)` holds a card's display order (a block's lines
after the block line, an id listed twice or inside its own block taken once at its first place, an
id with no table left out), each line's block (`parent`, `is_within`) and number (`number`, from 1).
Two changes collide when both are Become or both Set, on one property, and for Become both name a
start state (`any_state` counts, a blank does not). `can_both_apply(a, b)` is false when a test of
`a` and a test of `b` read one value and no value passes both. A line's tests: its own condition,
the condition of each When around it, and a Become's named start state as "my X is S" (a state reads
as its index). An all of gives each item, nested all of included, and any of, none of, a result
chip, a draw and a `within` give nothing. A test reads `my` or a result chip
(with its part or property) against fixed numbers, percents or states, `outside` included, and a
result of a line inside a Repeat gives none (`repeat_of`), as each time round works it out
again. Two tests exclude each other when no tried value passes both: each listed number, its
neighbours (+-1 for whole values, +-0.5 otherwise), the midpoints, and the ends +-2, with a 1e-9
allowance. Whole means a States property, a Count result, or a Look around's `count` part or Count
summary.
`otherwise_of(line)` returns the higher colliding lines in display order, `otherwise_numbers` their
numbers. `competing_groups()` returns two or more Become lines of one property from one named start
state (not `any_state`), each condition holding exactly one rate and no chance. The rate stands as
the condition or as an item of an all of, nested all of included. A rate inside an any of or a none
of keeps the line out of every group, since no share of a hazard is defined there, and a chance
anywhere does too, as does a Repeat around the line (`group_key`). `otherwise_of` leaves a partner
in the line's own group out, since the group's lines read "or".
`CardRelations::notes(sentences, runs)` returns the card's `RelationNote`s (line, words,
`RelationNoteKind`), a line's notes in the wording table's order, `runs` saying whether a line runs:
`Gap` under the last of two or more top-level Become lines of one property from one start state
(`any_state` included), each running and a compare or outside test of one Count result, with two or
more new states, naming the counts from 0 to 8, 4 or 9 (`most_counted`) none passes; `Coverage`
under the last Become line of a States property whose Become lines all sit at the top level, run,
name a start state, and test one Count or hold always, when for every count each state of the
property ends the tick in one state (the first passing line from the top, else its own) and two or
more end states appear; `Order` under the last Become line of a property whose Become lines,
at any depth, all run and start from distinct named states; `WhenRepeats` under a When of "my X is
S" whose lines are all Become lines of X from S; `OnlyOneInside` under a When around one change that
holds always (with its id and number, for `MoveConditionInto`), neither for a When holding a When;
`Inside` under a change or When inside a When (`only_when_note` with the When's condition words, or
none while it is blank) and under a result line inside one (`worked_out_everywhere_note`);
`NothingInside` under an empty When. `check::holds_draw` is public for the card's Draw new numbers.

**The checker** and the sheet's limits have their own file, `agents/henad-graph-sheet-check.md`.

**Sheet files.** `SheetFile::parse(text)` reads (the reader's errors), checks (the checker's errors
as `SheetFileError::Invalid`, every one placed), lowers (the lowering's problems, also `Invalid`,
placed at their tables through `check::span_of`) and reads the lowered document back through
`GraphFile::from_document`. It keeps the `ParsedSheet` (text, places, line ends), the canonical text
(`to_toml`), the checker's warnings placed (`warnings`, skip warnings included), the lowered
`GraphFile` (`lowered`), the `LineMap` (`line_map`) and the `FaultNames` of its looks at agents
(`fault_names`), and the `FireLines` of its change lines (`fire_lines`). `code()` is the lowered
graph's `code()`: the IR's `name` is `[sheet] name`, both descriptions stay empty, and nothing else
of the sheet text enters it. The lowered `GraphFile` records the canonical text as its author text
(`GraphFile::author`, crate-private `with_author`) in `SHEET_CODE_FORMAT` (`henad-sheet/1`), and
`GraphFile::source` adds it to the `ModelSource` every entry of the graph records.
`GraphFile::from_code` and a graph read as a graph carry none
(`a_lowered_sheet_records_its_canonical_text_outside_the_code`). `Invalid` holds a `SheetErrors`,
never empty, since `SheetErrors::new` takes a first error and the rest.
`SheetFileError::diagnostics` returns every diagnostic, `diagnostic` the first, and `Display` writes
each on its line. `SheetFile` and `LineMap` write their own `Debug`.
`SheetFile::place_graph_diagnostic` maps a diagnostic of the lowered graph (its checker, host
evaluator or compiler) back into the sheet: a node through `LineMap` to its line's table, found by
the card whose `line_tables` hold the line, a node the sheet owns by the first two segments of its
slot path (`cells.<property>`, `agents.<property>`, `measure.<id>`, `button.<id>`, `card.<id>`,
`color.<thing>`), a node the map does not know (a normalisation copy, a planted node) to `[sheet]`,
and a diagnostic with a pass and no node by the pass id (`tick_<card>`, `button_<id>`,
`restart_<id>`) or to `[sheet]`. Its old span, line and column are replaced and its `pass` and
`node` cleared, so henad-cli's `located` prints no node id, and `--print-lowered` keeps the graph
within reach. henad-explore's `ModelFile` calls it for a sheet's entry errors, and leaves the
lowered graph's warnings out of a sheet's warnings. For a sheet of agents it compiles the lowered
graph through `AgentProgram::compile_with_fault_names` with `fault_names`, so a capped fold's fault
names its author lines. `SheetFile::shortfalls(entry, params, limits)` is the one cost mapping: the
entry's shortfalls, with the work lines (any line holding "units of work") replaced when the
heaviest submission or a host run passes its budget. A submission past `SUBMISSION_WORK_BUDGET`
names each fold of the heaviest step's pass (label `<sheet id>_<pass id>`) by its `FaultNames` words
through `look_costs_too_much` ("Line 1, Flockmates: looks at too many birds at 1,000,000 birds. Use
a shorter distance or fewer birds."), or the card through `card_costs_too_much` when the pass holds
no fold. A host run past `HOST_WORK_BUDGET` reads `start_costs_too_much`. The population is
`num_agents`, or `grid_width` times `grid_height`, read from the entry's parameters. A buffer a line
names takes its properties' names (`with_property_names`). The checker's per-element cost (V22) is
placed at the heaviest node's line by `place_graph_diagnostic`, which recognises it by
`check::is_pass_cost` (the message's start and its `MAX_PASS_COST` end, built in `check/cost.rs`
beside the message), rewords it through `SheetFile::cost_words` and drops its note:
`line_costs_too_much` names the line by `look_name` ("Line 1: too much work for one walker. Set a
smaller maximum for Steps per tick."), with `smaller_maximum` for a Repeat over a parameter,
`FEWER_TIMES` for a Repeat of a number and `FEWER_LINES` for any other line, or names the card by
its heading or a button by its name. `SheetFile::lowering_problems(parsed, report)` returns the
lowering's problems of a sheet that checks, each a `SheetDiagnostic` with its card and line, which
the card draws under the line. `SheetFile::parse_planted` (`#[cfg(test)]`, crate-private) lowers
with a closure run on the lowered document before it is read back, which the mapping tests use to
plant a node the checker refuses.

**The lowering** (`sheet/lower/`) has its own files, `agents/henad-graph-sheet-lower.md` (the grid
lowering, node ids, keys, the line table, the interim refusals and their tests) and
`agents/henad-graph-sheet-lower-agents.md` (what a sheet of agents adds, and the ground).

**Tests** of the wording table and the sentences are in `agents/henad-graph-sheet-wording.md`.
Writer tests sit in `src/tests/sheet_writer.rs`: `every_sheet_is_a_fixed_point`,
`the_rust_writer_agrees_with_check_py_apart_from_comments` against `src/tests/golden/` (the
reference writer's text for the four author-format examples with comment lines removed, each line's
note dropped first since the ants starter holds one the example lacks, and `sheet_` taken off each
starter's id, since the examples hold the ids without it; `golden/README.md` holds the
procedure, the script's SHA-256 and the rules in full),
`a_hand_edited_sheet_reads_back_to_its_canonical_text`,
`unusual_values_read_back_to_the_same_document`, `a_number_that_is_not_finite_is_refused`,
`numbers_follow_the_sheet_number_rule`, `every_line_sentence_equals_its_comment`, the three diffs of
the author format (a new waning line in four hunks, a Steer reorder in one, a parameter rename in
two, through a line diff in the test) and `every_readable_mutation_writes_text_that_reads_back` over
the reader's mutation corpus. Checker tests are in `agents/henad-graph-sheet-check.md`. Rate tests
sit in `src/tests/sheet_rates.rs`: the known chances (0.2592, 0.9093 with 8, a rate per week in
days, the competing 0.5034 and its half, the number rule), the uniform block of the SIR tick shader
(no `exp`, 1 - e^(-0.1) with e^(-0.1) one f32 step from the true value, the `math.powi` table within
3 steps, since its powers are f32 `powi` of the base), the competing thresholds at a tick of 7 days,
a group with a worked-out rate splitting on the host, the rates SIR conserving its cells at half a
day, `a_tick_length_override_keeps_the_code_and_the_keys`, the factor of a rate per week, every
other form of rate, a rate in an any of drawing alone, the notes of the wording table
(`rate_notes_match_the_spec_examples`, `worked_out_rate_notes_match_the_spec_examples`), the two
conversion commands and their refusals (a chance on a card that runs once and one inside a Repeat
among them, where the card offers no rate), the unit mismatch warning, and
`a_chance_for_each_has_its_note_and_a_count_as_a_chance_is_named`. Reader tests sit in
`src/tests/sheet_document.rs`, over eleven sheets: the eight starters, and three test sheets in
`src/tests/sheets/`. `sir.toml`, `boids.toml` and `ants.toml` are the author format's examples, and
`random_walk.toml`, `bounded_confidence.toml` and `sir_chance.toml` (SIR with chances per tick,
sheet id `sir_chance`) were written from the models of henad-models'
`tests/fixtures/lowered/round1/`, as `vote.toml` and `forest_fire.toml` were. The random walk's
measure is "Average of Position" (`average_of_position`), the name the sheet editor's average gives
it. `mobile_sir.toml` holds a chance once for each infected person within reach, in a world 1 cell
wide so every person reaches every other. Every sheet holds `always` and `any_state` written in,
Game of Life's Clear is a Run once card holding "Become Dead from any state, always", and both SIR
sheets name their button `new_outbreak`, "New outbreak". The written sheets' keys were minted at
random. Their text is the writer's canonical text, comments included. The bounded confidence colour
rule is `shaded` over `opinion` with no colours, since the schema holds no ramp. The test helper
`sheets()` lists the starters from `starters()`, then the test sheets, so every starter is among the
sheets the tests read. `starters_returns_the_grid_sheets_then_the_agent_sheets` holds the list and
each starter's world, and `the_sheets_written_from_round_1_read_their_values` a value of each
written sheet. `every_sheet_reads` (places on every line table, the ants' chip in
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
them.
