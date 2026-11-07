# henad-graph: the sheet lowering

`agents/henad-graph-sheet.md` holds the rest of the sheet module: the document, reader, writer,
wording table, relations, checker and `SheetFile`. `sheet/lower/` is private, and `sheet` re-exports
`LineMap`, `NodeOrigin` and `NodeOwner` from `lower/table.rs`. `Lowered` holds the document, the
`LineMap` and the `FaultNames` of the looks at agents.

The `Builder` (`lower/mod.rs`) lowers over one kind of thing, `check::Thing`: the cells, or the one
kind of agent (D43, the checker's `ONE_AGENT_KIND` refuses a second). `properties()`,
`property(id)`, `thing_id()` (`cells` or the kind's id, as a card's `every` names it),
`property_path()` (`cells` or `agents`, the first segment of a sheet-owned node's slot path),
`thing_words()`, `domain()` and `property_place(index)` read it, so the starts, the cards, the
colours, the measures and the buttons lower over either. Pass ids follow it: `start_<thing>`,
`tick_<card>` (`tick_<thing>` with no card), `colours`, `measures`, `button_<button>`,
`restart_<button>`. `node_mut` rewrites a written node's configuration, for a fold's accumulators.
`dropped` holds the properties with no lane.

**The lowering of grid sheets** (S7, S10). `lower::lower(&document, &report)` builds a
`GraphDocument` through its constructors in a fixed order (`world.rs`: the grid, a category per
States property with state ids as variant names and state names as labels, a lane per property, `u8`
for states, `f32` for Number, `i32` for Whole number, `bool` for Yes/No, the first state as a
category lane's `initial`, then `[[param]]` per parameter, `u32` for a Whole number from 0, `f32`
otherwise, step 1 for a Whole number with none), then the passes in stack order: `start_cells`
(`start.rs`), `tick_<card>` or `tick_cells` with no card (`card.rs`, a grid runs exactly one tick
pass, so a second card that runs each tick over the cells is an error), `colours` (`colours.rs`, no
pass when the rule is `by_state` on the first States property, `view.color` through the category for
another, `view.ramp` over 0 to 1 into the palette `shaded_<property>` for `shaded`, and an error for
`scale = "log"` or `decades`, since only a linear shading lowers), `measures` (`measures.rs`: one
`stat.count_by` per property shared by its counts, `stat.mean`, `stat.sum`, a spread as two means
and `f64` arithmetic, each `[[stat]]` labelled with the measure's name, D13), and per button an
`[[action]]` (`buttons.rs`). A grid sheet with no cells property lowers to the world alone, with the
checker's `SHEET_EMPTY` warning, and its graph's check reports the missing lane. The `[time]` table
is not lowered until rates lower (S21).

Nodes are written with temporary ids into `Draft`s at a `Site` (pass, owner, slot path, region), and
`Builder::finish` numbers them (`ids.rs`, R4 and D44): line `l<k>` owns `n(1000k + j)`, `j` in the
line's own emission order, and the sheet owns `n1` up in a walk of the passes, each pass's sheet
nodes ordered by `SheetKey` (`GROUP_READ` the shared `lane.get` per property, `GROUP_PART` starts,
colours and steps, `GROUP_CHANGE` a property's transition or chain write, `GROUP_MEASURE`), then
emission order. No id depends on another line's display place. Past `MAX_NODES_PER_LINE` a line is
refused at its table, past `MAX_SHEET_NODES` the sheet. `LineMap` (`table.rs`) maps every written
node id to its `NodeOrigin` (owner `Line(id)` or `Sheet`, and a path such as `condition.right.2`,
`cells.status.starts_as`, `measure.alive` or `card.cell_tick.status`), never by division, since
normalisation copies take ids past the file's last. `LineMap::result_node` names the node whose
output is a result line's value (a Look around's fold). A line's paths follow the slot path grammar
of `sheet::edit` (`parse_path` in `edit/lines.rs`), so one path names one slot for the lowering, the
diagnostics and the editor: indices count from 0 (`items.0`, `row.0.condition`, `summaries.0.value`,
`button.<id>.does.0` for a sheet node too), a summary is named by its index and never by its id, and
past the slot a path may name an item of a listed right side (`right.1`). The nodes of a line that
sit in no slot take `""` (the line itself), `when` (the join with a When around it), `change` (its
share of a change chain), `formula` (a Calculate's expression) and `row.<index>` (a Choose row's
select). A test holds the two together over the grid sheets and `values.toml`. These paths feed draw
keys, so a change to the grammar changes code identity.

Keys (`keys.rs`, R3, D12, D45): an owner's key is the stored one or the low 32 bits of
`fnv1a64("<sheet id>/<owner id>")` (the checker warns), and each draw takes `compose_key(owner key,
low 32 bits of fnv1a64(slot path))`, written straight on the draw, since the lowering writes no
function. A start's paths are `starts_as.row` and `starts_as.value`, a restart pass's the same with
`.<button>` added, since no two draws of a file share a key. A line's draw takes the key of the line
that owns its site (a top-level or nested line, never the phrase's own `key`) and the site's path,
`""` for a Chance or Random number line itself and `condition` for a chance in a change's condition.
A When's condition lowers once and its operand joins every change inside, so its draw is written and
keyed once. `keys::clashes` is V18 after lowering: a key two owners take is an error naming both, at
the second.

Starts: All [state], a number, yes or no or a parameter write the value (a fraction truncated into a
Whole number), a `random_number` phrase is `rand.float(high - low) + low` on a Number property and
an error on any other (a truncated draw never reaches the high end), and by chance draws once per
cell: `rand.chance` for one row, `rand.float(1)` against running sums of the chances for more, rows
chosen through a `logic.select` chain, the "otherwise" state last. Painted, anywhere and on-ground
starts are errors on a grid. Buttons: Start again runs `start_cells` when it names every property
with a start, otherwise a disabled init pass `restart_<button>` of those properties; Set everywhere
and Run once write the one action pass `button_<button>` (a property written twice in it is an
error, a card run by two buttons is an error), and a button holding both forms is an error. Draws in
a rerun key on the press count, so each press draws a new layout, and a rebuild at one seed replays
the start (D73).

Cards (`card.rs`): lines lower in display order, a skipped line (`SheetReport::is_skipped`, which
holds its dependents) left out (D39, D84). Looks (`look.rs`): a Count over cells is `grid.count`
(one per counted state, added in `u32`) when its test is `their` States property `is`, `is_not`,
`one_of` or `not_one_of` states (`is_not` counts the other states, exact at walls) and it has no
`only`, and otherwise a `grid.fold` (`moore_row_major` or `von_neumann`, `include_center` for the 3
by 3 block, `boundary = "domain"`) with a `count` accumulator, `their` as `lane.get_at` of its
`cell`, and a `region.put` whose `when` is the test, `only` joined in by `logic.and`. A Look around
over cells is one such fold with `count` and an accumulator `summary_<id>` per summary (`f32` `sum`
for total and average, `max`, `min`, `bool` `or` for any and `and` for all; a Count summary reads
`count`), each `region.put` under the `only` test, and its parts kept by line id and part for `{
result, part }` chips (`count` and each summary id). An average divides by `count` through
`conv.to_f32`, and an average, largest or smallest of no neighbour is 0 through a `logic.select` on
`count > 0`. A Look around in a slot is an error, since it gives several values. Draws (`draw.rs`):
a chance is one `rand.chance`, and with `tries` it holds with `1 - (1 - p)^n` (P), through
`math.powi` with `max_exponent` 8, 4 or 9 when `p` is a number, percent or parameter and `n` is a
grid Count or a grid Look around's `count` part or Count summary, and `math.pow` with `n` in `f32`
otherwise behind a `logic.select` that takes 1 (missed every try) for `n <= 0`, since `pow(0, 0)` is
not 1 on every device. A Random number is `rand.float(high - low) + low` in `f32`. Calculate
(`calculate.rs`): one `math.expr`, `out = <formula>` fully parenthesised, each distinct reference an
input `v1`, `v2` in first-use order (`vec2f` for an arrow, `f32` otherwise) resolved through
`formula_chip` and the `Scope` trait's `chip` (`CardLowering`, or `PassScope` for a measure's
formula over my properties and the parameters), the formula's type an arrow when an arrow meets it,
numbers written by `f32`'s `Debug` (always a point or an exponent, so the expression reads `f32`,
and a number past `f32` an error), `^` as `pow`, `smaller` and `larger` as `min` and `max`, `length`
as `length`, `direction` as `(v / max(length(v), 0.000001))`, and either an error on a number.
Choose: a `logic.select` chain from the last row up over the `otherwise` value, a row whose
condition is the constant yes ending the chain and a constant no left out, its paths
`row.<index>.value`, `row.<index>.condition`, `row.<index>` for the row's select, and `otherwise`.
Depending on: one `cat.switch` over the read of my property, `case.<state>` per state at
`branches.<state>`. Their values share a type through `card::choice_type` over `value::shared_type`
(one type, numbers of several types `f32`, a typed number the known type cannot hold `f32`, numbers
with no type `f32`). A state chip there is an error: the checker passes no governing property for
these slots, so a checked sheet holds none. Compare tests: `cmp.*`, `one_of` an or of `cmp.eq`,
`not_one_of` an and of `cmp.ne`, `within` as `abs(a - b) <= tolerance` in `f32`, `outside` an or of
`cmp.lt` low and `cmp.gt` high (D82). All of, any of and none of fold with `logic.and`, `logic.or`
and `logic.not`, a constant side folded away (`Builder::join`); `always` is the literal `true`. A
When's condition, joined with the When around it, is joined by `logic.and` into each change inside.
Becomes of one property are one `cat.transition` (rule id the line id, or `<line>_<state>` per state
for `any_state`, the `from == to` row kept so a higher any-state line wins, at most
`MAX_TRANSITION_RULES` rows), Sets a `logic.select` chain from the bottom row up over `my` X, each
select owned by its line, and either written by one `lane.set` the sheet owns. Rows come in a stable
topological sort: display order where `CardRelations::can_both_apply`, line id order otherwise, so a
reorder that cannot change the result leaves the text. `my_new X` writes X's changes when first
read, and the checker places it below every change of X. Values (`value.rs`, `Operand`): a count is
`u32`, a typed number takes the type it meets when that type holds it exactly (`common_type`), else
both sides go to `f32` through `conv.*`; a percent is its number divided by 100 (D110); a state chip
takes the category of the value it meets. Interim refusals (D38), one located error per line at its
line table in the author's words, whatever the line's skip: on a cells card `NOT_YET_RATE` and
`NOT_YET_REPEAT`, for the line's kind or a phrase in any slot. The refusals of chances, looks,
Calculate, Choose, Depending on, Random number and the world of agents were lifted with their
lowering, and their words removed from `sheet::sentence`. Record on a cells card is an error (D66),
and so is an agents-only line kind or a look at agents within a distance.

Grid lowering tests sit in `src/tests/sheet_lower.rs`, on the host evaluator:
`every_grid_sheet_lowers_checks_and_compiles` (the three starters and `sir_chance.toml`: no warning,
no graph diagnostic, `GridProgram`, an entry, canonical text), the vote's block count and rows,
`lowering_is_deterministic`, `lowered_ids_sit_in_their_line_blocks`,
`the_line_map_covers_every_written_node`,
`every_line_map_path_of_a_line_reads_as_an_edit_command_slot_path` (every starter, agents included),
`two_draws_with_one_key_are_an_error_naming_both` (two starts), the derived key, the percent, the
measure's label, one test per identity row of the author format for grids (a result rename, a
reorder of independent lines, a note, `purpose` or comment keep the code; a parameter, state or
model rename, a default, a colour, new numbers and a reorder of changes that can both apply change
it), `the_life_sheet_blinker_has_period_2` and
`the_life_sheet_glider_moves_one_cell_diagonally_every_4_ticks` (32 by 32, lanes planted, D74),
`a_rebuild_at_one_seed_replays_the_same_start`,
`a_start_again_button_draws_a_new_layout_each_press`,
`a_skipped_line_and_its_dependents_are_left_out`, `each_interim_refusal_gives_one_located_error`
(rate and Repeat), `each_lifted_refusal_now_lowers_checks_and_compiles` (S7's seven refused lines),
the second tick card, the empty grid, `the_largest_grid_sheet_lowers_under_half_the_node_limit` (a
card of `MAX_LINES_PER_CARD` count and Become lines over four properties),
`the_kinds_sheet_runs_its_lines_on_the_host` (the test sheet `src/tests/sheets/kinds.toml`,
canonical, the counts, yes or no lines and changes against a Rust reference over one tick), the
parts on Game of Life with a random Number `level` and a States `mood` added (`life_with`): Set
everywhere and a property set twice, Start again on some properties (`restart_randomise`, `level`
left alone, presses replay), the random start's range and its refusal on a Whole number, average,
total and spread against a Rust reference, `view.color` and `view.ramp`, the log scale refused, both
node limits named, `the_sheet_lowering_never_panics_on_the_mutation_corpus` (one in five edits of
the sheets that lower still lowers), `the_chance_sir_sheet_conserves_its_cells_every_tick` (64 by
64, a New outbreak press through its action pass, 200 ticks), `once_for_each_follows_the_power_rule`
(a 300 by 300 sheet whose Set draws once for each flagged neighbour, every cell flagged against `1 -
0.8^k` for k = 8, 4 and 9 with `max_exponent` checked, none flagged for 0, every third row for
0.488, with `p` typed for `math.powi` and a Calculate for `math.pow`), the forest fire's spread row
before its lightning row, new numbers on a chance and a renamed chance result against the code, a
chance inside a When keyed by its own line, `the_values_sheet_runs_its_lines_on_the_host` (the test
sheet `src/tests/sheets/values.toml`, canonical: three Look arounds covering every summary, two with
`only` and one of them able to leave out every neighbour, a Count with `only`, a Calculate over
their parts, a Choose, a Depending on, Sets and a Become, against a Rust reference over one tick),
`the_values_sheet_honors_each_only_and_gives_0_for_a_summary_of_no_cell` (the Count's `only` and the
average, largest and smallest of no cell, each met at least once), a formula of arrows or past the
depth limit refused at its line, and a Random number line's range.
`a_graph_diagnostic_is_placed_at_its_author_line` plants an unknown op on a node of line `l2` and
places the checker's error at `[card.line.l2]` with no pass or node, then a node the sheet owns at
`[[cells.property]]` and a pass alone at its card and button.
`an_unknown_node_is_placed_on_the_sheet` plants node `n900000` and places it, a pass of no card and
a bare warning at `[sheet]`.

**The lowering of agent sheets** (S17). A sheet of agents first refuses, each at its table, `edges =
"walls"` (`NOT_YET_AGENT_WALLS` at `[world]`, row H), a ground with properties or blocked states
(`NOT_YET_GROUND`) and a count of events (`NOT_YET_RECORD` at the measure). `world.rs` writes
`[agents]` from the kind (`count`, `max_count`, the world's width and height in `f32`), Position as
the `vec2f` lane with `role = "position"`, an Arrow and a Heading as `vec2f`, and the Heading
dropped (no lane, in `dropped`) when nothing reads it: no `my`, `my_new` or `their` chip of it in
any line, phrase or formula (`formula::references`), no measure of it, and no colour rule by heading
or of it. A read of a dropped property is an error. A Heading holds the last move that was not 0,
the round 1 hand-lowered files' form, and starts at none, (0, 0).

Starts (`start.rs`): Position `anywhere` is `ctx.world`, `vec.split` and two `rand.float` keyed
`starts_as.x` and `starts_as.y`, then `vec.make`. An Arrow starts as a random direction, its angle a
`rand.float` to 2 pi keyed `starts_as.value`, its length a number, a parameter or a random number
keyed `starts_as.value.length`. `on_ground` and `painted` read `NOT_YET_GROUND`.

Cards (`card.rs`, the card whose `every` is the kind): on a card of agents the interim refusals are
`NOT_YET_RATE`, `NOT_YET_RECORD`, `NOT_YET_LEAVE`, `NOT_YET_PICK_CELL`, `NOT_YET_LOOK_AT_CELL`,
`NOT_YET_MOVE_TO` (a place comes only from a Pick a cell or a look at a cell, both refused),
`NOT_YET_GROUND` for a Count or Look around
over the cells around and a chip naming a place's property, and `NOT_YET_REPEAT_CHANGE` for a
Become, Set, Keep between, When or Repeat inside a Repeat. A property changed two ways is refused at
the second line, naming the first in display order: Steer below a Set (`set_already`), Set below a
Steer (`steered_already`), Move to below Move by (`moved_by_already`) and Move by below Move to
(`moved_to_already`), round 1 section 6. That error takes the place of the line's own interim
refusal, so a refusal gives one located error. The checker refuses a Count, Look around or Pick a
cell inside a Repeat. A Count over agents in a slot is an error ("stands on a line of its own").

Looks (`agents.rs`, A): at the card's first look at agents, `open_looks` takes every look that runs,
in line id order, and groups them into folds: by `include_me`, then a new fold past
`MAX_ACCUMULATORS` (16). A distance is a number or a parameter, refused otherwise. The sheet owns,
in group `GROUP_LOOK` at path `card.<card>`: the largest distance, a `math.max` chain over the
distinct distances sorted by key (a parameter's id, a number's bits), `cell_size = math.div(largest,
3)`, one node every fold reads (V14), and each `agent.radius_fold` (index 1 up) at its own looks'
largest distance, `exclude_self = !include_me`, `center` the shared read of Position, `when = true`.
Its `acc` is written by `finish_looks` after the card, sorted by line number and place, so a reorder
of the looks keeps the code. Each look holds `<line>_count` (`u32` `count`) and per summary
`<line>_summary_<id>` (`vec2f` `sum` for a total or average of arrows, `f32` `sum`, `max`, `min`,
`bool` `or`, `and`). A fold of looks of more than one distance gives each look a `dist_sq` filter:
`math.mul(d, d)` at the line's `distance` path outside the region, and `cmp.le(dist_sq, d^2)`
inside, joined with `only` (and a Count's `whose`) into each `region.put`. `their` is `lane.get_at`
at the fold's `other`, read once per look (`Fold::agents`), `offset_to_them` the fold's `delta`,
`offset_away_from_them` its `math.neg`, `distance_to_them` the `math.sqrt` of its `dist_sq`. An
average of arrows is `vec.scale(total, math.recip(count))`, of numbers `math.div`, and an average,
largest or smallest of no one is the zero of its type through `guard_by_count`. Each such part's
count sits in `CardLowering::empty`. Each fold's `FaultNames` words come from `sentence::look_name`
over its looks' display numbers and names ("lines 1 and 2, Flockmates and Too close").

Draws (P): a chance once for each of a count over agents is never `math.powi` (`count_bound` gives
no bound for agents within a distance), always the guarded `math.pow`.

Motion (`motion.rs`): a Steer's term is `value * strength` (`vec.scale` for an arrow, `math.mul` for
a number), towards `(value - my X) * strength`, behind `count > 0` when the value is an empty part
(round 1: a towards-value of no one does nothing), and behind its condition (blank is always, the
When lines around it joined) as a select against 0. Keep between is a change of `low` and `high`
under its condition. `steered` applies a property's Steers (`math.add`) and Keep betweens in row
order after its Set chain: a number through `math.clamp` in the lane's type, an arrow through
`vec.length`, `math.clamp`, a divide by `max(length, 1e-6)` and `vec.scale`, with an arrow of length
0 becoming `(low, 0)`. Each node of a line's share is owned by the line at path `change`. A Move by
is a change of Position holding its arrow (0 where its condition fails), and `write_position` adds
the arrows in row order, `math.add` onto Position, `space.wrap` with `ctx.world`, the write, then
the Heading: `logic.select(vec.length(sum) > 0, sum, heading)`. `my_new X` is the value before its
`lane.set`. A Random direction, as a line or phrase, is a `rand.float` to 2 pi keyed by its site and
`vec.from_angle`.

Repeat (`repeat.rs`, Move by and Steer lines inside): one `region.repeat` owned by the
Repeat line, `count` a whole number literal or a Whole number parameter from 0 (`u32`), `max` that
number or the parameter's maximum (an error past `MAX_REPEAT_ITERATIONS` naming the parameter), and
a `sum` accumulator `change_<property>` for each property a Move by or Steer inside it changes
(found by a scan of the lines inside that run). Each total is one change of its property owned by
the Repeat line, added when the Repeat lowers, and each line inside writes its term as a
`region.put` under its condition (`put_in_repeat`). Lines inside a Repeat take its region through
`line_site`. A Become, Set, Keep between, When or Repeat inside a Repeat is refused
(`NOT_YET_REPEAT_CHANGE`).

Agent lowering tests sit in `src/tests/sheet_agents.rs`, on the host:
`every_agent_sheet_lowers_checks_and_compiles_on_the_host` (the three agent starters and
`mobile_sir.toml`: no warning, no graph diagnostic, an entry, canonical, deterministic, ids in their
line blocks, the map covers every node), the worlds and the dropped Heading, one shared fold for the
boids' two looks with its accumulators, its name and two `cmp.le` filters, a look reorder keeping
the code, `the_radius_index_follows_the_current_largest_distance` (one `cell_size` source, and
`AgentHooks::index_cell_size` at 50 and 8, then 9 and 30), the power rule on the host (0.488 within
five standard errors over 4 people and 5000 ticks, 0 for a chance of 0 for each of 1000),
`for_each_of_zero_with_chance_1_is_0_on_host_and_device` (the device through `GpuContext::new` over
`support::test_device`), `each_agent_refusal_gives_one_located_error` (walls, Pick a cell, Record,
Move to below Move by, Set below Steer, a Become in a Repeat, a look in a Repeat, a second kind), a
look at the cells around, `the_shortfalls_of_a_sheet_name_its_looks_in_the_model_words` (boids at
1,000,000 birds and a Visual range of 200),
`a_pass_cost_past_the_limit_names_its_line_in_the_model_words` (the random walk with a Steps per
tick maximum of 65536: one V22 error, placed at the Repeat line in `line_costs_too_much`'s words
with no note and no forbidden word, and another finding's words kept), and the fold as a node the
sheet owns. henad-models'
device gates are in `agents/models.md`.
