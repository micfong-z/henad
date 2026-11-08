# henad-graph: the sheet lowering

`agents/henad-graph-sheet.md` holds the rest of the sheet module: the document, reader, writer,
wording table, relations, checker and `SheetFile`. `sheet/lower/` is private, and `sheet` re-exports
`LineMap`, `NodeOrigin` and `NodeOwner` from `lower/table.rs`. `Lowered` holds the document, the
`LineMap` and the `FaultNames` of the looks at agents.

The `Builder` (`lower/mod.rs`) lowers over one kind of thing, `check::Thing`: the cells, or the one
kind of agent (the checker's `ONE_AGENT_KIND` refuses a second), and for a sheet of agents on a
ground with properties also the ground (`on_ground` runs a lowering with `thing` set to
`Thing::Ground`). `properties()`, `property(id)`, `thing_id()` (`cells`, the kind's id as a card's
`every` names it, or `ground`), `property_path()` (`cells`, `agents` or `ground`, the first segment
of a sheet-owned node's slot path), `thing_words()`, `domain()` (cells for the ground) and
`property_place(index)` read it, so the starts, the cards, the colours, the measures and the buttons
lower over any of them. Over the ground `read` is a `field.get` and `write` a `field.set` through
`ground_access` (the field and layer of `ground_layers`). `lane_type` is the lane's type, the
Heading a `vec2i` when `cell_mover` holds (a card moves agents to places). Pass ids follow it:
`start_<thing>`, `tick_<card>` (`tick_<thing>` with no card), `colours` (`colours_ground`),
`measures` (`measures_ground`), `button_<button>`, `restart_<button>` (`restart_<button>_ground` for
the ground). The `[[stat]]` tables keep the measures' order across the two measure passes
(`stat_measures`). `node_mut` rewrites a written node's configuration, for a fold's accumulators.
`dropped` holds the properties with no lane.

**The lowering of grid sheets**. `lower::lower(&document, &report)` builds a
`GraphDocument` through its constructors in a fixed order (`world.rs`: the grid, a category per
States property with state ids as variant names and state names as labels, a lane per property, `u8`
for states, `f32` for Number, `i32` for Whole number, `bool` for Yes/No, the first state as a
category lane's `initial`, then `[[param]]` per parameter, `u32` for a Whole number from 0, `f32`
otherwise, step 1 for a Whole number with none), then the passes in stack order: `start_cells`
(`start.rs`), `tick_<card>` or `tick_cells` with no card (`card.rs`, a grid runs exactly one tick
pass, so a second card that runs each tick over the cells is an error), `colours` (`colors.rs`, no
pass when the rule is `by_state` on the first States property, `view.color` through the category for
another, `view.ramp` from `darkest` (0 when left out) to `lightest` (1) into the palette
`shaded_<property>` for `shaded`, eight colours from the dark end of `hue` to its light end
(`hue_colors` over `ShadeHue::ends`) or a blue to orange scale with no hue, the palette reversed and
the ends swapped when `darkest` is the higher value, because `view.ramp` takes band 0 whenever
`high <= low`, equal ends an error, and an error for `scale = "log"` or `decades`, since only a
linear shading lowers), `measures` (`measures.rs`: one `stat.count_by` per property shared by its
counts, `stat.mean`, `stat.sum`, a spread as two means and `f64` arithmetic, each `[[stat]]`
labelled with the measure's name), and per button an `[[action]]` (`buttons.rs`). A grid sheet
with no cells property lowers to the world alone, with the checker's `SHEET_EMPTY` warning, and its
graph's check reports the missing lane. A `[time]` table lowers in `world.rs` to the first
`[[param]]`, `tick_length` (`rates::TICK_LENGTH`, reserved in the item namespace), an `f32` labelled
"One tick is (days)" with the sheet's length as its default, a hundredth of it as `min` and `step`
and 30 times it as `max`, `hidden`, and `GraphDocument.time` naming it with the tick's unit,
whether or not a line reads it.

Rates (`rates.rs`): a rate phrase or Rate line is `1 - e^(-h)` drawn by one `rand.chance`,
`h = r * f * tick_length`, where `f` is the literal `tick unit days / rate unit days` when it is not
1 (`math.mul` before the tick length), so a rate of parameters is a prelude value the host computes
once per build. Such a rate with no "for each" (`exact_chance_of`) takes `h (1 - h (1/2 - h/6))`
below `SMALL_HAZARD` (2^-10) through a `logic.select`, since `1 - e^(-h)` in `f32` sits on the 2^-24
grid and is 0 below about 3e-8, and draws by `rng.threshold` and `rand.below` at 2^-32
(`a_small_rate_keeps_its_chance_in_the_threshold`, and henad-models'
`a_rate_below_the_resolution_of_a_float_draw_recovers_its_share` on the device). Every other rate and
every chance line keeps `rand.chance`, resolved to 2^-24. A "for each" over a grid count (`count_bound`) with a rate the same for every
element is `1 - powi(e^(-h), n)` with `max_exponent` 8, 4 or 9. Any other count, agents included, is
`1 - select(n <= 0, 1, exp(-h * n))` in `f32`, P's guard, since `exp(0)` is not exactly 1 on every
device. A competing group is `CardRelations::competing_groups` less its skipped lines, two or more
left (`CardLowering::groups`, set before the lines lower). Each Become of a group lowers at its turn
through `group_line`: its condition with the rate read as yes (`gate`, the rate found as the
condition or in an all of, and the When lines around it joined by `inherit`), and its hazard `a = r
* f`, times `n` for a "for each", through `select(gate, a, 0)` when the gate is not always yes. Its
change takes a placeholder condition, so it keeps its place in the row order. After the group's
last line, `finish_group` writes at the highest line's rate site the running sums of the hazards,
`t = 1 - select(S <= 0, 1, e^(-S * tick_length))` (the guard of a rate for each, so a cell with no
exposure never leaves), `max(S, 1e-30)` and one `rand.float` up to 1 keyed by the highest line, and
at each line's rate site its threshold, `t * sum_j / max(S, 1e-30)` and `t` for the last line,
`cmp.lt(u, threshold)` joined with its gate, which replaces the placeholder. The transition takes
the first row that holds, so each line takes its share whatever the row order, and a line between
the group's lines keeps its "otherwise" as the card shows it. The other lines' keys stay unused
while they are grouped. A line inside a Repeat joins no group (`CardRelations::group_key`) and
draws alone. A rate with no `[time]` never lowers: the checker skips it. The rate tests sit in
`src/tests/sheet_rates.rs`, which `agents/henad-graph-sheet.md` lists.

Nodes are written with temporary ids into `Draft`s at a `Site` (pass, owner, slot path, region), and
`Builder::finish` numbers them (`ids.rs`): line `l<k>` owns `n(1000k + j)`, `j` in the
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

Keys (`keys.rs`): an owner's key is the stored one or the low 32 bits of
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
the start. A press runs with `action_phase` of its button's place among the `[[action]]`s, so
a button added or deleted above another changes that button's press draws while its keys stay, as
`docs/reference/model-files.md` says.

Cards (`card.rs`): lines lower in display order, a skipped line (`SheetReport::is_skipped`, which
holds its dependents) left out. Looks (`look.rs`): a Count over cells is `grid.count`
(one per counted state, added in `u32`) when its test is `their` States property `is`, `is_not`,
`one_of` or `not_one_of` states (`is_not` counts the other states, exact at walls) and it has no
`only`, and otherwise a `count` accumulator `<prefix>_count` of a shared fold over the cells around
(`fuse.rs`, below), `their` as `lane.get_at` of its `cell`, and a `region.put` whose `when` is the
test, `only` and the fold's guard joined in by `logic.and`. A Look around over cells takes
`<prefix>_count` and an accumulator `<prefix>_summary_<id>` per summary (`f32` `sum` for total and
average, `max`, `min`, `bool` `or` for any and `and` for all; a Count summary reads the count), each
`region.put` under the `only` test, and its parts kept by line id and part for `{ result, part }`
chips (`count` and each summary id). An average divides by `count` through
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
`cmp.lt` low and `cmp.gt` high. All of, any of and none of fold with `logic.and`, `logic.or`
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
both sides go to `f32` through `conv.*`; a percent is its number divided by 100; a state chip
takes the category of the value it meets. Interim refusals, one located error per line at its
line table in the author's words, whatever the line's skip: on a cells card `NOT_YET_REPEAT`, for
the line's kind or a phrase in any slot. The refusals of chances, looks, Calculate, Choose,
Depending on, Random number, rates and the world of agents were lifted with their lowering, and
their words removed from `sheet::sentence`. Record on a cells card is an error,
and so is an agents-only line kind or a look at agents within a distance.

Grid lowering tests sit in `src/tests/sheet_lower.rs`, on the host evaluator:
`every_grid_sheet_lowers_checks_and_compiles` (the four grid starters and `sir_chance.toml`: no
warning, no graph diagnostic, `GridProgram`, an entry, canonical text), the vote's block count and
rows, `lowering_is_deterministic`, `lowered_ids_sit_in_their_line_blocks`,
`the_line_map_covers_every_written_node`,
`every_line_map_path_of_a_line_reads_as_an_edit_command_slot_path` (every starter, agents included),
`two_draws_with_one_key_are_an_error_naming_both` (two starts), the derived key, the percent, the
measure's label, one test per identity row of the author format, each asserting the code and the
schema hash through `changes` (a result rename, a reorder of independent lines or of Steers of one
property, a note, `purpose` or comment, a measure's id, a parameter's unit or `means` and the
`rate_unit_from_tick` mark keep both; a parameter, state or model rename, a colour, new numbers, a
step, a tick unit, a reorder of properties and a reorder of changes that can both apply change the
code alone; a default, minimum or maximum, a measure rename, a parameter id, a tick length and a
reorder of parameters or measures change both), `the_life_sheet_blinker_has_period_2` and
`the_life_sheet_glider_moves_one_cell_diagonally_every_4_ticks` (32 by 32, lanes planted),
`a_rebuild_at_one_seed_replays_the_same_start`,
`a_start_again_button_draws_a_new_layout_each_press`,
`a_skipped_line_and_its_dependents_are_left_out`, `each_interim_refusal_gives_one_located_error`
(Repeat), `each_lifted_refusal_now_lowers_checks_and_compiles` (the seven line kinds grid sheets
first refused), the second tick card, the empty grid,
`the_largest_grid_sheet_lowers_under_half_the_node_limit` (a card of `MAX_LINES_PER_CARD` count and
Become lines over four properties), `the_kinds_sheet_runs_its_lines_on_the_host` (the test sheet
`src/tests/sheets/kinds.toml`, canonical, the counts, yes or no lines and changes against a Rust
reference over one tick), the parts on Game of Life with a random Number `level` and a States `mood`
added (`life_with`): Set everywhere and a property set twice, Start again on some properties
(`restart_randomise`, `level` left alone, presses replay), the random start's range and its refusal
on a Whole number, average, total and spread against a Rust reference, `view.color` and `view.ramp`,
the log scale refused, both node limits named,
`the_sheet_lowering_never_panics_on_the_mutation_corpus` (one in five edits of the sheets that lower
still lowers), `the_chance_sir_sheet_conserves_its_cells_every_tick` (64 by 64, a New outbreak press
through its action pass, 200 ticks), `once_for_each_follows_the_power_rule` (a 300 by 300 sheet
whose Set draws once for each flagged neighbour, every cell flagged against `1 - 0.8^k` for k = 8, 4
and 9 with `max_exponent` checked, none flagged for 0, every third row for 0.488, with `p` typed for
`math.powi` and a Calculate for `math.pow`), the forest fire's spread row before its lightning row,
new numbers on a chance and a renamed chance result against the code, a chance inside a When keyed
by its own line, `the_values_sheet_runs_its_lines_on_the_host` (the test sheet
`src/tests/sheets/values.toml`, canonical: three Look arounds covering every summary, two with
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

**The lowering of agent sheets**. A sheet of agents first refuses `edges = "walls"` when a
card moves agents by an arrow (`NOT_YET_AGENT_WALLS` at `[world]`), and the first Move by in
display order when the ground blocks some states (`NOT_YET_BLOCKED_MOVE_BY` at its line). Agents
that move to places walk the cells and stop at walls and blocked cells on their own. `world.rs`
writes `[agents]` from the kind (`count`, `max_count`, the world's width and height in `f32`),
Position as the `vec2f` lane with `role = "position"`, an Arrow and a Heading as `vec2f`, and the
Heading dropped (no lane, in `dropped`) when nothing reads it: no `my`, `my_new` or `their` chip of
it in any line, phrase or formula (`formula::references`), no measure of it, and no colour rule by
heading or of it. A read of a dropped property is an error. A Heading holds the last move that was
not 0, the form of the hand-lowered files in henad-models' `tests/fixtures/lowered/round1/`, and
starts at none, (0, 0).

Starts (`start.rs`): Position `anywhere` is `ctx.world`, `vec.split` and two `rand.float` keyed
`starts_as.x` and `starts_as.y`, then `vec.make`. An Arrow starts as a random direction, its angle a
`rand.float` to 2 pi keyed `starts_as.value`, its length a number, a parameter or a random number
keyed `starts_as.value.length`. `on_ground` and `painted` lower as the ground section says. A start
ends in `Builder::write`, a `lane.set` or over the ground a `field.set`.

Cards (`card.rs`, the card whose `every` is the kind): on a card of agents the interim refusal is
`NOT_YET_REPEAT_CHANGE` for a Keep between, When, Repeat, Leave, Record or Move to inside a
Repeat. A property changed two ways is refused at
the second line, naming the first in display order: Steer below a Set (`set_already`), Set below a
Steer (`steered_already`), Move to below Move by (`moved_by_already`) and Move by below Move to
(`moved_to_already`). That error takes the place of the line's own interim
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
(a towards-value of no one does nothing), and behind its condition (blank is always, the
When lines around it joined) as a select against 0. Keep between is a change of `low` and `high`
under its condition. `steered` applies a property's Steers (`math.add`) and Keep betweens in row
order after its Set chain, the Steers between two Keep betweens in line id order (a reorder of them
keeps the lowered graph), and a Keep between above a Set still holds the Set's value
(`a_keep_between_holds_the_value_after_every_set` in `sheet_agents.rs`): a number through
`math.clamp` in the lane's type, an arrow through `vec.length`, `math.clamp`, a divide by
`max(length, 1e-6)` and `vec.scale`, with an arrow of length 0 becoming `(low, 0)`. Each node of a
line's share is owned by the line at path `change`. A Move by
is a change of Position holding its arrow (0 where its condition fails), and `write_position` adds
the arrows in row order, `math.add` onto Position, `space.wrap` with `ctx.world`, the write, then
the Heading: `logic.select(vec.length(sum) > 0, sum, heading)`. `my_new X` is the value before its
`lane.set`. A Random direction, as a line or phrase, is a `rand.float` to 2 pi keyed by its site and
`vec.from_angle`.

Repeat (`repeat.rs`, Move by, Steer, Become and Set lines inside, answer 11a: `sum` and `or`
accumulators only): one `region.repeat` owned by the Repeat line, `count` a whole number literal or
a Whole number parameter from 0 (`u32`), `max` that number or the parameter's maximum (an error past
`MAX_REPEAT_ITERATIONS` naming the parameter), a `sum` accumulator `change_<property>` for each
property a Move by or Steer inside it changes, and a `bool` `or` accumulator `<line>_applies` per
Become or Set inside, in line id order (found by a scan of the lines inside that run). Each total is
one change of its property owned by the Repeat line, added when the Repeat lowers, and each line
inside writes its term as a `region.put` under its condition (`put_in_repeat`). A Become or Set
writes its condition, lowered inside the region, as a `region.put` of its `or` accumulator
(`any_time`), and its change reads the total: it applies when it held at any time, and the higher of
two such changes wins whichever time round each held, where the first time could decide instead
(record #92 item 7, an open choice for the maintainer). A Set's value is
lowered outside the region, once per tick, and a value that reads a line inside the Repeat is
refused (`SET_IN_REPEAT`, `reads_inside` over chips, phrases and formulas). Lines inside a Repeat
take its region through `line_site`. A Keep between, When, Repeat, Leave, Record or Move to inside a
Repeat is refused (`NOT_YET_REPEAT_CHANGE`).

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
sheet owns. The refusal test holds walls, Move to below Move by, Set below Steer, a Record in a
Repeat, a look in a Repeat and a second kind. henad-models' device gates are in `agents/models.md`.

**Folds over the cells around** (`fuse.rs`). The Counts that need a fold, Look arounds and
picks of one card over one neighbourhood family share a `grid.fold` the sheet owns, in group
`GROUP_CELL_FOLD` at path `card.<card>`: the 8 cells around and the 3 by 3 block take
`moore_row_major`, the 4 cells around `von_neumann`. `enter_cells(site, line, needed)` joins the
newest fold of the family with room for `needed` accumulators (at most `MAX_ACCUMULATORS`, 16) that
is not open around the look and was opened below the last line the look reads (`last_line_read`: the
lines its result chips and formulas name at any depth, and for a `my_new` chip every line above it
naming that property; `CellFold::opened_at` is the opener's display index), or writes a new one. A
look that reads an earlier look over the same family therefore takes a second fold, and every fold
reads only folds opened before it. It then pushes a `Fold` for `their`, and returns a `CellLook`:
the fold, the accumulator prefix (the line id for a line, `<line>_p<k>` for the `k`th look in a
line's phrases) and `within`. A Moore fold includes the cell itself when any line or phrase of the
card looks at a 3 by 3 block through a fold (`folds_block`: a Look around, or a Count that
`counted_states` leaves to a fold), and a look at the 8 cells around then takes `within`, one shared
`cmp.ne(offset, (0, 0))` per fold. `finish_cell_folds` writes each fold's `acc` sorted by line
number, look and place, so a reorder of looks that share one fold keeps the code. Past 16
accumulators, or past a read of an earlier look, the lowering order decides which fold a look joins.
Draws in a body read the fold's `k`, so fusion changes the iteration word of a draw inside a look.
On a card of agents the fold has `at` = my cell's `xy` and `boundary = "bounded"` with walls
(`domain`, the torus, otherwise), its `Fold` reads the ground (`FoldKind`: `Cells`, `Ground`,
`Agents`), and its open test (`fold_open`) is written once per fold. No grid starter holds a fold,
so fusion left their codes unchanged.

**The ground** (`ground.rs`). `declare` writes a `[[counter]]` per event and the fields: a
States property is a static field of its category, layer its id; a Number property that takes marks,
fades or is set by a button joins the field of its marks rule and share kept per tick (the first
such property's id, each property a layer, `max` for keep the largest, `Sum` at 2^-16 for add up),
with `finish = "fade_<field>"` when it fades; any other Number property is a static field of
its own; another type is an error. `fades` holds each fade function, built in `finish` from the
sheet's ids (`fn.input`, `math.mul` by the number or Number parameter, `fn.output`, path
`ground.<property>.kept_per_tick`), so the line map covers function nodes too. The ground's passes
run over the cells: `start_ground` (a value, or `painted`: the base state then a `logic.select` per
shape in order, each shape one `math.expr` over the cell's column and row and `ctx.grid`, a cell
shape the cell at `floor(center * size)`, an ellipse or rectangle in a frame turned by `angle`
degrees from the x axis towards smaller rows with sides `size * grid / 2`), `colours_ground`
(`by_state` through the category's palette, or with `shade_state` and `shades` a palette
`shades_<property>` of the states then `SHADE_BANDS` (6) blends per shade, the largest shade's band
from `log10(max(v, 1e-30)) / decades + 1` or `v` on a linear scale, through a `cat.switch`; `shaded`
a Number property linearly), `measures_ground` and Set everywhere. `on_ground` places an agent at
`floor(center * size) + 0.5` of a cell shape painting its state, drawn by `rand.float` keyed
`starts_as.cell` among several (`StartsAs::single_cells` counts them, and the checker and the editor
count such a start as drawing); a state the base or a larger shape paints is `ON_GROUND_CELLS`.
`StartsAs::paints_single_cells` decides it, and the sheet pane offers the same states.

On a card of agents `Builder::my_cell` is one `space.cell_of` of my position per pass
(`GROUP_GROUND`, path `agents.<position>`). A place is `Operand::Place(PlaceValue)`: `xy` (`vec2i`),
the `amount` a pick ranked by, and `open` when known; it takes `vec2i` wherever a value is wanted.
Here is my cell, ahead `xy + heading` (wrapped through `space.wrap` of the `vec2f` in a world that
wraps, open only with a heading, `AHEAD_NEEDS_CELLS` without `cell_mover`), and `at` a place slot.
`place_open` is `space.cell_index`'s `valid` and no blocked state at its cell, `place_property` a
`field.get` there, and a result chip's `amount` or `open` part and `property` read a place line's
result. A Choose of places selects `xy` and, when every row has one, `amount` (`choose_place`); a
Depending on of places is a `cat.switch` per part. Pick a cell (`pick.rs`): ranked, three `carry`
accumulators `<prefix>_best`, `<prefix>_xy`, `<prefix>_ties` (inits 0, my `xy`, 0); a considered
cell (the guard, `open_only`'s open test, `only`) replaces the best when it is the first or ranks
higher, and a tie replaces it when `rng.reservoir` over a `rand.bits` of the new tie count holds;
the amount is the best behind `ties > 0`. Random: `<prefix>_xy` and `<prefix>_seen`, kept with a
chance of one over the count. Move to is a change of Position (`ChangeKind::MoveTo` holding the
`PlaceValue`), `write_moves` joining each row's condition with `logic.not` of the higher rows'
conditions (the first whose condition holds decides) and with `place_open` of its
place (the `[ground] blocked` open test of moves), so a blocked place keeps the agent where it is
even where a lower row holds, selecting `xy` from the bottom row up over my cell and the
position the cell's centre where some row applies, then the Heading the `vec2i` of `space.delta`
from my cell to the new one when it is not 0. Leave collects a `LeaveMark`; after the changes
`write_leaves` writes one `field.deposit` at my cell per field whose Leaves cannot both apply (layer
and amount through `logic.select` chains, owned by the sheet at `card.<card>.<field>`), else one per
Leave, its amount 0 where the condition fails. Record is a `counter.add` of `conv.to_u32` of its
condition, and an event count a `stat.counter` in the agents' measures. Buttons (`buttons.rs`): a
button starts again or changes each kind of thing, and changes one kind, so Start again of the ants
beside Set everywhere of the trails is one action with `runs = ["start_ant"]` and a
`button_<button>` pass over the cells. Start again of some ant and some ground properties reruns
`restart_<button>` and `restart_<button>_ground`; a States property of the ground is never set.
`convert.rs` and `SheetFile::place_graph_diagnostic` read `ground.<property>` paths.

Tests in `src/tests/sheet_ground.rs`: `the_ants_sheet_lowers_checks_and_runs_on_the_host` (starts at
the nest, the painted ground, 20 ticks on open cell centres),
`the_ants_card_lowers_to_one_fused_fold_per_neighbourhood` (one fold, eight accumulators, one merged
deposit, one counter, the tick pass's node count, `ANTS_TICK_NODES`, 110),
`seventeen_accumulators_split_into_two_folds`,
`a_block_and_the_8_cells_around_share_a_fold_with_a_guard` (and the values sheet's two folds),
`a_ground_property_that_adds_up_lowers_to_a_sum_and_runs` (100 ticks on the device),
`a_become_inside_a_repeat_applies_when_any_time_holds` (and `SET_IN_REPEAT`),
`each_ground_refusal_gives_one_located_error`,
`a_count_of_the_ground_around_joins_the_fold_of_the_picks`,
`a_look_that_reads_an_earlier_look_opens_its_own_fold` (two folds, checks and runs on the host),
`a_pick_that_reads_an_earlier_pick_opens_its_own_fold`,
`a_button_that_starts_ants_and_ground_again_reruns_two_passes` and
`a_move_to_a_blocked_cell_leaves_the_agent_where_it_is` (20 host ticks, and
`NOT_YET_BLOCKED_MOVE_BY`), `the_first_move_to_whose_condition_holds_decides` (the test sheet
`src/tests/sheets/move_to_first.toml`, the cell ahead not open at the first tick) and
`becomes_that_test_a_number_drawn_inside_a_repeat_keep_display_order` (both line id orders).

**Fire lines**. `Builder::record_fire(node, port, line)` records a `bool` output
that holds where a change line applies, and `Builder::fire_lines` keeps, at `finish`, those of a
node in a tick pass outside every region and not a `math.expr` or `fn.call`, renumbered, as
`Lowered::fires`. `transition` records `fired.<rule>` of each row of a property's `cat.transition`,
one row per state for a Become from any state. `CardLowering::fires_when(id, condition)` records a
condition that is a link (a constant `always` has no node): a Set's after `any_time`, unless
`overridden_set` finds a line of `CardRelations::otherwise_of` that the report does not skip (the
"otherwise" Set, which applies where its higher Sets do not and no output holds), and
`change_condition` of a Steer, Keep between, Move by, Leave and Record. A Move to records nothing,
since its open test decides. `fires_when` records nothing for a line with a `repeat_of`, wherever
its condition was lowered, and the region filter of `Builder::fire_lines` stays behind it.
`FireLines` (`table.rs`, re-exported from `sheet`) holds the `Probes` and the line id of each,
`counts_by_line` summing a line's rows into a `BTreeMap` and returning an empty one for counts of
another length. `SheetFile::fire_lines` returns it. The probes leave the lowered document and the
code alone. Tests in `src/tests/sheet_fires.rs`:
`every_sheet_compiles_with_the_probes_of_its_change_lines`, `the_starters_count_their_change_lines`
(Game of Life `l2`, `l3`; the vote's rows; the ants' otherwise Set left out),
`the_life_sheet_blinker_fires_lines_2_and_3_twice_each_tick` (host),
`the_sird_competing_lines_count_separately` (each count equal to the cells that changed by its line,
30 host ticks), `probes_leave_the_sheet_code_and_the_lowered_text_alone`,
`counts_by_line_adds_the_rows_of_a_line_and_drops_counts_of_another_length`,
`a_change_inside_a_repeat_has_no_count` (a Move by with a blank condition in a Repeat inside a When)
and `a_fire_count_reads_as_its_number_and_its_description`. henad-models' `sheet_grid.rs` holds the
device: `the_life_sheet_blinker_fires_lines_2_and_3_twice_each_tick_on_the_device` and
`the_forest_fire_counts_equal_the_host_every_tick_for_50_ticks` (the model file's entry).
