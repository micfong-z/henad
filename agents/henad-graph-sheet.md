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
`sheet/limits.rs` holds the sheet's limits apart from `crate::limits`, so `library.lock` stays as it
is: `MAX_SHEET_BYTES` (1 MiB), `MAX_NESTING` (24), `MAX_LINES_PER_CARD` (256), `MAX_NODES_PER_LINE`
and `MAX_SHEET_NODES` (999 each, for the lowering's `n(1000k + j)` ids) and `MAX_SHEET_ID_BYTES`
(48, so `tick_<card>` and `button_<button>` stay within the IR's 64-byte ids). The reader enforces
the first three.

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

**Places.** `SheetPlaces::root` is a `TablePlaces` tree that follows the TOML structure: `table`
(the header, an inline table's braces, or the first key naming an implicit table such as
`card.line`), `keys` (each value's bytes) and `tables` (one entry under a key holding a table, one
per element under a key holding a list with a table in it). `SheetPlaces::line(card_index, line_id)`
reaches a line table. Places are offsets into the text with line ends made `\n`, and
`ParsedSheet::place` maps a diagnostic back to the text as passed.

**Tests** sit in `src/tests/sheet_document.rs`, over the four sheets in `src/tests/sheets/`: round
1's Game of Life, SIR with rates, boids and ants in the author format, with `always` and `any_state`
written in, Game of Life's Clear a Run once card holding "Become Dead from any state, always", and
SIR's button `new_outbreak`, "New outbreak". Their generated comments keep the wording before the
final editor, apart from the lines added. `every_sheet_reads` (places on every line table, the ants'
chip in `[[card.line.l7.row.condition.items]]`, and a depth at most half the limit),
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
