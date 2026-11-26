# henad-graph: the lowering of agent sheets and the ground

`agents/henad-graph-sheet-lower.md` holds the `Builder`, the lowering of grid sheets that a sheet
of agents builds on, the folds over the cells around, node ids, keys, the line table and the fire
lines. This file holds what a sheet of agents adds: its refusals and lanes, starts, looks at agents,
motion, Repeat and the ground under the agents.

**The lowering of agent sheets**. A sheet of agents first refuses `edges = "walls"` when a
card moves agents by an arrow (`NOT_YET_AGENT_WALLS` at `[world]`), and the first Move by in
display order when the ground blocks some states (`NOT_YET_BLOCKED_MOVE_BY` at its line), through
`sheet::refusals` (`first_move_by`, `walls`, `blocked_state`), which an editor asks too. Agents
that move to places walk the cells and stop at walls and blocked cells on their own. `world.rs`
writes `[agents]` from the kind (`count`, `max_count`, the world's width and height in `f32`),
Position as the `vec2f` lane with `role = "position"`, an Arrow and a Heading as `vec2f` (a Heading
`vec2i` when a card moves agents to places), and the Heading dropped (no lane, in `dropped`) when
nothing reads it: no `my`, `my_new` or `their` chip of it in any line, phrase or formula
(`formula::references`), no measure of it, no colour rule by heading or of it, and no look at the
cell ahead. A read of a dropped property is an error. A Heading holds the last move that was
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
(`moved_to_already`), through `refusals::Way` and `refusals::changes_outside_repeat`. That error
takes the place of the line's own interim refusal, so a refusal gives one located error. The cell
ahead lowers only while `refusals::cell_ahead` lets it. The checker refuses a Count, Look around or
Pick a
cell inside a Repeat. A Count over agents in a slot is an error ("must be on a line of its own").

Looks (`agents.rs`): at the card's first look at agents, `open_looks` takes every look that runs,
in line id order, and groups them into folds: by `include_me` and by depth, then a new fold past
`MAX_ACCUMULATORS` (16). A look's depth (`look_depths`) is the most looks at agents along a chain of
results it reads (`lines_read`: the lines its result chips and formulas name at any depth, and for a
`my_new` chip every line above it that changes the property), through lines of any kind and, for a
change or a block, the When or Repeat that holds it, so a look that reads another takes a later fold
and a fold reads only folds of a lower depth, whichever line id is lower. A line that works out a
result, a look included, is worked out everywhere, so its own block adds nothing. A card whose looks
read no look keeps one depth and the folds it had. A distance is a
number or a parameter (`check::fixed_number`), which the checker refuses otherwise first. The sheet
owns,
in group `GROUP_LOOK` at path `card.<card>`: the largest distance, a `math.max` chain over the
distinct distances sorted by key (a parameter's id, a number's bits), `cell_size = math.div(largest,
3)`, one node every fold reads (V14), and each `agent.radius_fold` (index 1 up) at its own looks'
largest distance, `exclude_self = !include_me`, `center` the shared read of Position, `when = true`.
Its `acc` is written by `finish_looks` after the card, sorted by line number and place, so a reorder
of the looks keeps the code. Each look holds `<line>_count` (`u32` `count`) and per summary
`<line>_summary_<id>` (`vec2f` `sum` for a total or average of arrows, `f32` `sum`, `max`, `min`,
`bool` `or`, `and`). A fold of looks of more than one distance gives each look a `dist_sq` filter:
`math.mul(d, d)` at the line's `distance` path outside the region, and `cmp.le(dist_sq, d^2)`
inside, joined with `only` (and a Count's `whose`, always yes when blank, so a Count with no test
counts everyone within its distance) into each `region.put`. `their` is `lane.get_at`
at the fold's `other` (`FoldKind::Agents`), read once per look (`Fold::reads`), the relation chip
`offset_to_them` the fold's `delta`, `offset_away_from_them` its `math.neg`, `distance_to_them` the
`math.sqrt` of its `dist_sq`. The
fold's `delta` wraps, so in a world with walls (`walled_offset`, kept in `Fold::walled`) each look
reads their Position, takes `math.sub` of mine and its `vec.dot` with itself inside the fold, uses
those for the three relations, and filters every put by `cmp.le` of that square against its own
distance, filtered or not. The torus distance is never longer, so the fold visits every agent the
look keeps, and no look sees across an edge (`a_look_at_agents_sees_no_one_across_a_wall`: two
people beside opposite sides of a world 20 wide count each other with wrap and not with walls). An
average of arrows is `vec.scale(total, math.recip(count))`, of numbers `math.div`, and an average,
largest or smallest of no one is the zero of its type through `guard_by_count`. Each such part's
count sits in `CardLowering::empty`. Each fold's `FaultNames` words come from `sentence::look_name`
over its looks' display numbers and names ("lines 1 and 2, Flockmates and Too close").

Draws: a chance once for each of a count over agents is never `math.powi` (`count_bound` gives
no bound for agents within a distance), always the guarded `math.pow`.

Motion (`motion.rs`): a Steer's term is `value * strength` (`vec.scale` for an arrow, `math.mul` for
a number), towards `(value - my X) * strength`, behind `count > 0` for each look whose empty part
the value reads, directly or through the Calculate lines whose results it reads at any depth
(`empty_counts`, each look's count once, so a towards-value of no one does nothing). A Choose or a
Depending on stops the walk: it gives one of its values, and its rows can test the look's count, so
a Choose whose row tests the count steers towards its "otherwise" value
(`a_steer_towards_a_value_worked_out_from_an_empty_look_adds_nothing`). The term also sits behind
its condition (blank is always, the When lines around it joined) as a select against 0. Keep between
is a change of `low` and `high` under its condition. `steered` applies a property's Steers
(`math.add`) and Keep betweens in row order after its Set chain, the Steers between two Keep
betweens in line id order (a reorder of them keeps the lowered graph), and a Keep between above a
Set still holds the Set's value
(`a_keep_between_holds_the_value_after_every_set` in `sheet_agents.rs`): a number through
`math.clamp` in the lane's type, an arrow through `vec.length`, `math.clamp`, a divide by
`max(length, 1e-6)` and `vec.scale`, with an arrow of length 0 becoming `(low, 0)`. Each node of a
line's share is owned by the line at path `change`. A Move by
is a change of Position holding its arrow (0 where its condition fails), and `write_position` adds
the arrows in row order, `math.add` onto Position, `space.wrap` with `ctx.world`, the write, then
the Heading: `logic.select(vec.length(sum) > 0, sum, heading)`. `my_new X` is the value before its
`lane.set`. A Random direction, as a line or phrase, is a `rand.float` to 2 pi keyed by its site and
`vec.from_angle`.

Repeat (`repeat.rs`, Move by, Steer, Become and Set lines inside, `sum`, `or` and `min`
accumulators): one `region.repeat` owned by the Repeat line, `count` a whole number literal or
a Whole number parameter from 0 (`u32`), `max` that number or the parameter's maximum (an error past
`MAX_REPEAT_ITERATIONS` naming the parameter), a `sum` accumulator `change_<property>` for each
property a Move by or Steer inside it changes, and one accumulator per Become or Set inside, in
line id order (found by a scan of the lines inside that run). Each total is
one change of its property owned by the Repeat line, added when the Repeat lowers, and each line
inside writes its term as a `region.put` under its condition (`put_in_repeat`). A lone Become or
Set writes its condition, lowered inside the region, as a `region.put` of a `bool` `or` accumulator
`<line>_applies` and reads it (`any_time`). Two Becomes or two Sets of one property in one Repeat
that `can_both_apply` compete (`contests`): each puts `k` into a `u32` `min` `<line>_first` under
its condition, and its change holds where `cmp.lt(<line>_first, u32::MAX)` and, per lower
competitor, `cmp.ge(<lower>_first, <line>_first)` (path `change`), so the rows' display order picks
the line that first held at the earliest time round, the higher on a tie. A Become from a state
below one from any state puts only where my state, `cmp.eq` outside the region, is that state. A
change outside meets these by display order. Tests in `src/tests/sheet_repeat.rs`, among them
`becomes_that_test_a_number_drawn_inside_a_repeat_keep_display_order` on `repeat_roll.toml` in both
line id orders. A Set's value
is lowered outside the region, once per tick, and a value that reads a line inside the Repeat is
refused (`SET_IN_REPEAT`, `reads_inside` over chips, phrases and formulas). Lines inside a Repeat
take its region through `line_site`. A Keep between, When, Repeat, Leave, Record or Move to inside a
Repeat is refused (`NOT_YET_REPEAT_CHANGE`).

Agent lowering tests sit in `src/tests/sheet_agents.rs`, on the host:
`every_agent_sheet_lowers_checks_and_compiles_on_the_host` (the four agent starters and
`mobile_sir.toml`: no warning, no graph diagnostic, an entry, canonical, deterministic, ids in their
line blocks, the map covers every node), the worlds and the dropped Heading, one shared fold for the
boids' two looks with its accumulators, its name and two `cmp.le` filters, a look reorder keeping
the code, `the_radius_index_follows_the_current_largest_distance` (one `cell_size` source, and
`AgentHooks::index_cell_size` at 50 and 8, then 9 and 30), the power rule on the host (0.488 within
five standard errors over 4 people and 5000 ticks, 0 for a chance of 0 for each of 1000),
`for_each_of_zero_with_chance_1_is_0_on_host_and_device` (the device through `support::device_for`
and `support::context`), `each_agent_refusal_gives_one_located_error` (walls, Move to below Move
by, Set below Steer, a Record in a Repeat, a look in a Repeat, a second kind),
`the_shortfalls_of_a_sheet_name_its_looks_in_the_model_words` (boids at
1,000,000 birds and a Visual range of 200),
`a_count_of_agents_with_no_test_lowers_and_compiles`,
`a_look_at_agents_that_reads_an_earlier_look_opens_its_own_fold` (a Count over the average of line
1, a lower line id read as line 1's tolerance, and a Count over my new Flag, set inside a When on
line 1's count, two folds each, checked and run on the host),
`a_value_of_the_wrong_type_in_a_steer_or_a_summary_is_refused_at_its_line` (a Steer of an Arrow by a
number or towards a count, and a largest of their Velocity, each one checker error at its line),
`a_pass_cost_past_the_limit_names_its_line_in_the_model_words` (the random walk with a Steps per
tick maximum of 65536: one V22 error, placed at the Repeat line in `line_costs_too_much`'s words
with no note and no forbidden word, and another finding's words kept), and the fold as a node the
sheet owns. henad-models' device gates are in `agents/models.md`.

**The ground** (`ground.rs`; its starts in `paint.rs`, its colours in `shades.rs`, places and the
lines that read and mark it in `place.rs`). `declare` writes a `[[counter]]` per event and the
fields: a States property is a static field of its category, layer its id; a Number property that
takes marks, fades or is set by a button joins the field of its marks rule and share kept per tick
(the first such property's id, each property a layer, `max` for keep the largest, `Sum` at
`sheet::limits::ADD_UP_RESOLUTION_SHIFT`, 2^-16, for add up), with `finish = "fade_<field>"` when
it fades; any other Number property is a static field of its own; another type is an error. `fades`
holds each fade function, built in `finish` from the sheet's ids (`fn.input`, `math.mul` by the
number or Number parameter, `fn.output`, path `ground.<property>.kept_per_tick`), so the line map
covers function nodes too. The ground's passes run over the cells: `start_ground` (a value, or
`painted`: the base state then a `logic.select` per shape in order, each shape one `math.expr` over
the cell's column and row and `ctx.grid`, a cell shape the cell at `floor(center * size)`, an
ellipse or rectangle in a frame turned by `angle` degrees from the x axis towards smaller rows with
half-sides `size * grid / 2`), `colours_ground` (`by_state` through the category's palette, or with
`shade_state` and `shades` a palette `shades_<property>` of the states then `SHADE_BANDS` (6) blends
per shade, the largest shade's band from `log10(max(v, 1e-30)) / decades + 1` or `v` on a linear
scale, through a `cat.switch`; `shaded` a Number property linearly), `measures_ground` and Set
everywhere. `on_ground` places an agent at `floor(center * size) + 0.5` of a cell shape painting its
state, drawn by `rand.float` keyed `starts_as.cell` among several (`StartsAs::single_cells` counts
them, and the checker and the editor count such a start as drawing); a state the base or a larger
shape paints is `ON_GROUND_CELLS`. `StartsAs::paints_single_cells` decides it, and the sheet pane
offers the same states.

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
deposit, one counter, the tick pass's node count, `ANTS_TICK_NODES`, 116),
`seventeen_accumulators_split_into_two_folds`,
`a_block_and_the_8_cells_around_share_a_fold_with_a_guard` (and the values sheet's two folds),
`a_ground_property_that_adds_up_lowers_to_a_sum_and_runs` (100 ticks on the device),
`a_fixed_mark_that_can_pass_what_a_cell_adds_up_warns_at_its_line`,
`a_become_inside_a_repeat_applies_when_any_time_holds` (and `SET_IN_REPEAT`),
`each_ground_refusal_gives_one_located_error`,
`a_count_of_the_ground_around_joins_the_fold_of_the_picks`,
`a_look_that_reads_an_earlier_look_opens_its_own_fold` (two folds, checks and runs on the host),
`a_pick_that_reads_an_earlier_pick_opens_its_own_fold`,
`a_button_that_starts_ants_and_ground_again_reruns_two_passes` and
`a_move_to_a_blocked_cell_leaves_the_agent_where_it_is` (20 host ticks, and
`NOT_YET_BLOCKED_MOVE_BY`) and `the_first_move_to_whose_condition_holds_decides` (the test sheet
`src/tests/sheets/move_to_first.toml`, the cell ahead not open at the first tick, the lower line
reading "otherwise" and two Move by lines not).
