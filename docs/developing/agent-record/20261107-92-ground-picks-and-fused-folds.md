---
date: 2026-11-07
title: "Ground, picks and fused folds"
description: "S20 of the #37 redesign. The ground under agents lowers to fields, painted at the start and marked by Leave, keeping the largest mark or adding the marks up at 2^-16, and fading by its share kept per tick. Pick a cell, the cell here or ahead, a place's property, Move to, Leave and Record lower, a Become or Set inside a Repeat applies when it holds at any time, and the looks and picks of a card over one neighbourhood share one fold. The ants become a sheet starter, run 100 ticks on a baseline device with its tick pass at 8 storage buffers, and deliver food within the band of round 1's hand-lowered ants."
icon: material/map-marker-path
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s20-ground-picks-fused-folds after its merge rebase onto a3f0480 (S21 merged): two unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: f879ba2
delta_state: "committed on `37-node-editor` by fast-forward: `feat: ground, picks, repeat and fused folds in agent sheets [draft]`, `fix: s20 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Ground, picks and fused folds

> S20 finishes R1 for agent worlds: the ground, the places agents pick and move to, the marks they leave and the events they record.
> A ground property is a field, its States painted at the start, its Numbers marked by Leave with the largest mark kept or the marks added up at a resolution of 2^-16 (D49), and faded by a share kept per tick.
> Pick a cell walks the cells around with a carried best value, cell and tie count and breaks ties through `rng.reservoir`, so every tied cell is equally likely.
> Move to takes an agent to the centre of the place its first applying line names, and the Heading of such an agent is a whole step between cells, a `vec2i`.
> Exclusive Leaves on one field merge into one deposit, Record adds to an event's counter, and a Become or Set inside a Repeat applies when it held at any time round, through an `or` accumulator (answer 11a).
> The Counts, Look arounds and picks of a card over one neighbourhood share one `grid.fold`, split past 16 accumulators (F1), and the ants' card holds one fold over the 8 cells around.
> The ants move into `crates/henad-graph/sheets/` and `starters()` with a note on their slower trail (D64, answer 13a), build on a baseline device with their tick pass at exactly 8 storage buffers, and deliver food within the band of round 1's hand-lowered ants on seeds 1 to 3.

## State before

- `37-node-editor` at `f879ba2`, with S15 (the add-up merge) and S17 (agent sheets) merged, and S13b and S18 planned beside S20.
- A sheet of agents lowered one kind of agent in a wrapping world: looks at agents within a distance, Steer, Keep between, Move by and a Repeat of moves and steers.
- The lowering refused at their tables walls, the ground, Pick a cell, the cell here or ahead, Move to, Leave, Record, a look at the cells around an agent, a Become, Set, Keep between, When or Repeat inside a Repeat, and an event count (D38).
- The ants sheet waited as test data in `crates/henad-graph/src/tests/sheets/`.
- A grid card's Counts and Look arounds each wrote a `grid.fold` of their own.

## What was done

### The ground as fields

`sheet/lower/ground.rs` is new.
The `Builder` lowers over the ground as a third kind of thing (`Thing::Ground`, `on_ground`), so the starts, the colours, the measures and the buttons lower over its properties through the same code, with `field.get` and `field.set` in place of `lane.get` and `lane.set`.
A States property of the ground is a static field of its category.
A Number property that takes marks, fades or is set by a button joins the field of its marks rule and share kept per tick, one layer per property, so the ants' To food and To home share one field and one buffer of deposits.
"Keep the largest" merges with `max` and "add up" with `sum` at 2^-16 (D49, row D's sheet half), and a share kept per tick is the field's `finish`, a function `fade_<field>` that the lowering writes with ids from the sheet's numbering.
Each event is a `[[counter]]`, and an event count a `stat.counter` in the agents' measures.
The ground's passes run over the cells of the field grid: `start_ground`, `colours_ground`, `measures_ground` and a button's Set everywhere, and the `[[stat]]` tables keep the measures' order across the two measure passes.

A painted start gives each cell its base state, then the state of each shape covering the cell's centre in the shapes' order, each shape one `math.expr` over the cell's column and row and the grid's size.
A cell shape covers the cell at `floor(center * size)`, and an ellipse or rectangle is centred at a fraction of the world with sides a fraction of the world's, turned by its angle in degrees from the x axis towards smaller rows.
With that convention the ants' two rocks lie across the diagonal from the food to the nest, as the stock geometry has them.
A start on the ground places an agent at the centre of a cell shape painting its state, drawn among several.

The ground's colour rule draws its States property through the category's palette, and with `shade_state` and `shades` a palette of the states followed by six blends per shade from the shade state's colour.
The largest shade picks its band, on a log scale over `decades` decades below 1 or linearly, through a `cat.switch` over the state.

### Places, picks and moves

A place is a new `Operand::Place`: the cell's column and row as a `vec2i`, the amount a pick ranked it by, and whether it is open when a line knows.
My cell comes from one `space.cell_of` of my position per pass.
Here is my cell, ahead is my cell plus my Heading (wrapped in a world that wraps, open only with a heading), and a Look at a cell's `at` reads a place slot.
A place is open when it lies on the grid and no blocked state of the ground covers it, and its property is a `field.get` at its cell.
A Choose of places selects the cell and, when every row has one, the amount, and a Depending on of places is a `cat.switch` per part.

`sheet/lower/pick.rs` is new.
A pick by rank holds three `carry` accumulators: the best value, its cell and the number of cells tied for it.
A considered cell replaces the best when it is the first or ranks higher, and a tied cell replaces it when `rng.reservoir` over a `rand.bits` of the new tie count holds.
A random pick counts the cells it considers and keeps each with a chance of one over the count.
A pick of no cell gives my own cell with amount 0, round 1's "stays put".

Move to is a change of Position whose rows select the cell from the bottom up over my cell, the position becoming the cell's centre where a row applies.
Each row applies only where its place is open, so `[ground] blocked` is the open test of every move, and an agent whose place is blocked or off the grid stays where it is.
The Heading of agents that move to places is a `vec2i` lane, set to the step from my cell to the new one through `space.delta` when it is not 0, the shorter way round a world that wraps.
A world with walls is allowed when no card moves by an arrow, as row H's note on the ants says, and the fold over the cells around then stops at the walls (`boundary = "bounded"`).
A Move by over a ground that blocks some states is refused at its line with `NOT_YET_BLOCKED_MOVE_BY`.

### Leave, Record and buttons

Leave deposits its amount at my cell, 0 where its condition fails.
The Leaves on one field that cannot both apply, by `CardRelations::can_both_apply`, merge into one `field.deposit` whose layer and amount come from `logic.select` chains, so the ants' two Leaves are one deposit, as in round 1's hand-lowered file.
Record is a `counter.add` of its condition as a whole number.
A button now starts again or changes each kind of thing, and changes one kind, so the ants' Reset colony starts the ants again and clears both trails in one action: `runs = ["start_ant"]` and an action pass over the cells.
A button that starts some ant properties and some ground properties again reruns two passes, `restart_<button>` and `restart_<button>_ground`.

### Repeat with `or`

A Become or Set inside a Repeat writes its condition, lowered inside the region, into an `or` accumulator `<line>_applies`, and its change reads the total: it applies when it held at any time round.
A Set's value is worked out once, outside the region, and a value that reads a line inside the Repeat is refused.
Keep between, When, Repeat, Leave, Record and Move to inside a Repeat stay refused.

### Fused folds (F1)

`sheet/lower/fuse.rs` is new.
The Counts that need a fold, the Look arounds and the picks of a card share one fold per neighbourhood family, the 8 cells around with the 3 by 3 block and the 4 cells around apart, owned by the sheet in a new group `GROUP_CELL_FOLD`.
A look joins the newest fold of its family while the fold holds at most 16 accumulators, is not open around the look, and was opened below the last line the look reads.
Otherwise it opens a new fold, so a look that reads an earlier look's result over the same cells never shares that look's fold, and every fold reads only folds opened before it.
A Moore fold includes the cell itself when the card looks at a 3 by 3 block through a fold anywhere, and each look at the 8 cells around then skips offset (0, 0) through one guard per fold.
Accumulators are named from their line, `<line>_count` for a line and `<line>_p<k>_count` for the `k`th look in a phrase of the line, and listed by line id, so a reorder of looks that share one fold keeps the code.
The fold's open test is written once per fold, and the ants' tick pass holds one fold with eight accumulators (Food way, Home way and the random open cell of line 7), 110 nodes in all.
No grid starter holds a look that needs a fold, so the grid starters' lowered text and codes are unchanged, and the S14 gates pass as before.

### The ants starter

`ants.toml` moved from `src/tests/sheets/` to `crates/henad-graph/sheets/` and into `starters()`, with a note under its last line: "This model forms its trail later than the ant model among Henad's examples: an ant lays as much on a diagonal step, lays nothing on its own cell, and picks among equal cells at random. Once a trail forms, deliveries rise at about the same rate." (D64, answer 13a).
The New model dialog offers only grid starters, so the app shows no new choice.

### Wording

`sheet::sentence` drops `NOT_YET_GROUND`, `NOT_YET_PICK_CELL`, `NOT_YET_LOOK_AT_CELL`, `NOT_YET_MOVE_TO`, `NOT_YET_LEAVE` and `NOT_YET_RECORD`, narrows `NOT_YET_REPEAT_CHANGE` and `NOT_YET_AGENT_WALLS`, and adds `NOT_YET_BLOCKED_MOVE_BY`, `ON_GROUND_CELLS`, `AHEAD_NEEDS_CELLS` and `SET_IN_REPEAT` to the group of words the plan adds.

### Tests

- `crates/henad-graph/src/tests/sheet_ground.rs`, new: `the_ants_sheet_lowers_checks_and_runs_on_the_host` (every ant starts at the nest's centre, the painted food, nest and two rocks, and after 20 ticks every ant stands at the centre of an open cell), `the_ants_card_lowers_to_one_fused_fold_per_neighbourhood` (one fold, its eight accumulators, one merged deposit, one counter, 110 nodes), `seventeen_accumulators_split_into_two_folds` (16 accumulators one fold, 17 two), `a_block_and_the_8_cells_around_share_a_fold_with_a_guard`, `a_ground_property_that_adds_up_lowers_to_a_sum_and_runs` (`combine = "sum"` at 2^-16, 100 ticks on the device), `a_become_inside_a_repeat_applies_when_any_time_holds` (and the Set refusal), `each_ground_refusal_gives_one_located_error`, `a_count_of_the_ground_around_joins_the_fold_of_the_picks`, `a_look_that_reads_an_earlier_look_opens_its_own_fold` (two folds, checks and runs on the host), `a_pick_that_reads_an_earlier_pick_opens_its_own_fold`, `a_button_that_starts_ants_and_ground_again_reruns_two_passes` and `a_move_to_a_blocked_cell_leaves_the_agent_where_it_is` (no ant on a rock over 20 host ticks, and the Move by refusal).
- `crates/henad-models/src/tests/sheet_agents.rs`: the agent starters now include the ants, which lower with no warning and run 100 ticks on a baseline device, meet the testing kit at their `max_count`, and bind at most 8 storage buffers in every pass, exactly 8 in the tick pass.
  `the_ants_sheet_delivers_food_in_band` holds the sheet and round 1's hand-lowered ants to at most 1000 deliveries by tick 2000 and at least 500 by tick 3000 on seeds 1 to 3, and a planted rule error, the Food way picking the least To food, misses the band.
  The measured deliveries, by tick 2000 and by tick 3000, follow.

  | Seed | Sheet | Round 1's file | Planted error |
  | --- | --- | --- | --- |
  | 1 | 12, 2191 | 651, 3594 | 1, 8 |
  | 2 | 3, 1912 | 0, 923 | 1, 8 |
  | 3 | 11, 2052 | 129, 2758 | 1, 11 |

  The upper edge, 1000 by tick 2000, sits about 1.5 times above the most any in-band run delivers then (651, round 1's file at seed 1), and catches an error that delivers without a trail, such as a Become with no food test.
  The lower edge, 500 by tick 3000, sits about 1.8 times below the fewest any in-band run delivers then (923, round 1's file at seed 2), and about 45 times above the most the planted error delivers (11).
  The sheet forms its trail later than round 1's file, as D64 notes, so its deliveries by tick 2000 sit near the floor of the band, and only the tick-3000 edge separates it from the planted error.
- `crates/henad-graph/src/tests/sheet_agents.rs`: the lifted refusals leave the refusal test, which now holds Move to below Move by and a Record in a Repeat, and the test of a look at the cells around waiting for the ground is gone.
- `sheet_document.rs`, `sheet_sentence.rs`, `sheet_lower.rs`, `sheet_convert.rs` and `sheet_writer.rs`: seven starters, the new words, function nodes in the line map count, the `ground` root of a frame, and the golden comparison dropping every line's note, since the ants starter holds one the format's example lacks.

### Docs and topic files

- `agents/henad-graph-sheet-lower.md`: the Builder over the ground, fused folds, the ground, places, picks, moves, Leave, Record, buttons and the Repeat's `or` (31,711 characters).
- `agents/henad-graph-sheet.md`: the wording groups, the ants starter, the test sheets (28,812 characters).
- `agents/models.md`: the ants' bindings and band (22,118 characters).
- `docs/reference/model-files.md`: the ground in the section on a sheet of agents, and the refusals left.
- `crates/henad-graph/src/tests/golden/README.md`: why the comparison drops notes.
- `CHANGELOG.md`: one line under Unreleased.

### Edited tree

```
CHANGELOG.md                                     ground, picks and fused folds
agents/
├── henad-graph-sheet-lower.md                   fused folds, the ground, places, picks, Repeat or
├── henad-graph-sheet.md                         wording, the ants starter, test sheets
└── models.md                                    the ants' bindings and band
crates/henad-graph/
├── sheets/
│   └── ants.toml                                moved from src/tests/sheets, note under its card
└── src/
    ├── sheet/
    │   ├── convert.rs                           ground.<property> paths
    │   ├── file.rs                              ground.<property> placed at its property
    │   ├── sentence.rs                          refusals lifted and narrowed, four new errors
    │   ├── starters.rs                          seven starters
    │   └── lower/
    │       ├── agents.rs                        FoldKind
    │       ├── buttons.rs                       a button over two kinds of thing
    │       ├── card.rs                          places, picks, Move to, Leave, Record, refusals
    │       ├── colours.rs                       colours_ground, shared colour helpers
    │       ├── fuse.rs                          new: folds over the cells around (F1)
    │       ├── ground.rs                        new: fields, painted starts, places, moves, marks
    │       ├── ids.rs                           GROUP_CELL_FOLD, GROUP_GROUND
    │       ├── look.rs                          Counts and Look arounds in shared folds
    │       ├── measures.rs                      measures_ground, event counts, stat order
    │       ├── mod.rs                           Builder over the ground, fade functions
    │       ├── motion.rs                        change_condition shared
    │       ├── pick.rs                          new: Pick a cell
    │       ├── repeat.rs                        or accumulators, any_time, reads_inside
    │       ├── start.rs                         painted and on-ground starts, write
    │       ├── value.rs                         Operand::Place, vector conversions
    │       └── world.rs                         category(), vec2i Heading, ahead reads Heading
    └── tests/
        ├── golden/README.md                     notes dropped before the comparison
        ├── mod.rs                               sheet_ground
        ├── sheet_agents.rs                      refusals lifted
        ├── sheet_convert.rs                     ground root
        ├── sheet_document.rs                    seven starters
        ├── sheet_ground.rs                      new
        ├── sheet_lower.rs                       function nodes in the map count
        ├── sheet_sentence.rs                    new words
        └── sheet_writer.rs                      notes dropped for the golden comparison
crates/henad-models/src/tests/
└── sheet_agents.rs                              the ants: bindings, band
docs/reference/model-files.md                    the ground in a sheet of agents
```

### Review

A reviewer with a fresh context listed one must-fix, four should-fixes and six nits.
Every one held against the code but part of the line-width nit, and `fix: s20 review findings [draft]` holds the fixes.

- **A look that reads an earlier look's fold.** A Count over the 8 cells around whose test reads the average of a Look around over the same cells joined that look's fold, and the fold read its own result: "these nodes depend on each other".
  A pick whose `only` read an earlier pick's amount failed the same way.
  `enter_cells` now takes the look's line, and joins a fold only when the fold is not open around the look and every line the look reads (result chips and formula references at any depth, and the lines above that name a property it reads through `my_new`) sits above the line that opened the fold.
  `a_look_that_reads_an_earlier_look_opens_its_own_fold` lowers the reviewer's grid sheet to two folds, checks it and runs one host tick, and `a_pick_that_reads_an_earlier_pick_opens_its_own_fold` does the same on the ants.
  Both tests failed with the reviewer's cycle on the code before the fix.
  The ants' card keeps its one fold.
- **A button that starts some ant and some ground properties again.** Each kind of thing wrote a partial rerun with the id `restart_<button>`, and the sheet was refused with the graph's "declares the pass twice".
  The ground's rerun is now `restart_<button>_ground`, and `a_button_that_starts_ants_and_ground_again_reruns_two_passes` runs both reruns on the host.
- **`[ground] blocked` as the open test of moves.** `ChangeKind::MoveTo` keeps the place, and `write_moves` joins `place_open` into each row's condition, so a move to a blocked or off-grid cell leaves the agent where it is.
  `a_move_to_a_blocked_cell_leaves_the_agent_where_it_is` drops "Ahead is open" from the ants' Next cell and moves the nest beside a rock, and no ant stands on a rock over 20 host ticks.
  Without the join, ant 14 stands on a rock after tick 5.
  A Move by over a ground that blocks some states is refused at its line with the new interim error `NOT_YET_BLOCKED_MOVE_BY`, "Blocked ground under agents that move by an arrow is not supported yet.".
  The ants' tick pass grows from 105 to 110 nodes (item 12).
- **Item 9 on the trail reads** now says the ants read each trail once per visited cell, as round 1's file does, and keeps the remark for two looks of one property in one fold.
- **The band's numbers.** The band test was rerun with the deliveries printed, and the Tests section lists them per seed with the margins of both edges.
  The print was removed after the run.
- **Nits.** The comments and string literals of this milestone's lines are within 120 columns, and the band test's doc comment is two paragraphs.
  Lines past 120 columns in `value.rs`, `sheet_lower.rs`, `sheet_writer.rs` and henad-graph's `sheet_agents.rs` predate S20 and were left alone.
  The topic file paragraphs the reviewer named are re-broken to the files' width.
  The trailing "which" clauses in `mod.rs` and `world.rs` are split.
  `centres` and `centre` in `ground.rs` are `centers` and `center`, and the message "shape ... has no centre" reads "center".
  `ON_GROUND_CELLS` reads "Agents can start only on a state the ground paints on single cells.".
  `folds_block` counts a 3 by 3 look only when it takes a fold, so a Count that lowers to `grid.count` no longer makes a Moore fold walk the cell itself, and the reorder claim in `fuse.rs` holds for looks that share one fold.
- **Checks after the fixes**, in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph -p henad-models -p henad-explore -p henad-cli -p henad-app --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-models -p henad-explore -p henad-cli -p henad-app --all-targets --all-features --no-fail-fast` (henad-graph library 406 passed and 1 ignored, henad-models library 185 passed with the baseline budget test failing, every other suite passing); `cargo test -p henad-graph --doc`; `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features`; `cargo check -p henad-graph -p henad-models --all-features --lib --target wasm32-unknown-unknown`.
  The full check still runs at the merge point.

## State after

- A sheet of agents lowers every line kind: the ground, picks, places, moves, marks, records and Repeats of moves, steers, Becomes and Sets, and rates since S21 merged first.
- Eight sheet starters with S21's SIR, the ants the first on a ground, the New model dialog still offering the grid starters alone.
- The looks and picks of a card over one neighbourhood share one fold on grid and agent cards alike.
- `37-node-editor` holds the two unsigned commits, the feature and the review's fixes, fast-forwarded after the merge rebase onto `a3f0480`.

### Merge check

- The merge rebase onto `37-node-editor` at `a3f0480` brought in S13b (sheet and card panes) and S21 (rates).
  The first commit conflicted in eight files, the ones record #91 predicted.
  - `card.rs`: the agent arm of the refusal takes both removals, S20's of the ground refusals and S21's of `NOT_YET_RATE`, and returns no refusal of its own.
    The card's fields keep S21's `groups` beside S20's `cell_folds` and `leaves`.
    The Become branch keeps S21's `group_of` split, and its non-group arm takes S20's `any_time` after `inherit`.
    A grouped line never sits inside a Repeat (`CardRelations::group_key`), so the group arm needs no `any_time`.
  - `world.rs`: the module doc takes both sentences, the `vec2i` Heading and the hidden `tick_length`, and the imports take both sides.
  - `lower/mod.rs`: both modules, `pick` and `rates`.
  - `sheet_document.rs`: neither test sheet line stays, since the ants and SIR both moved to `sheets/`.
    The test sheets are `sir_chance.toml`, `mobile_sir.toml` and `sird.toml`.
  - `CHANGELOG.md`, `agents/models.md`, `agents/henad-graph-sheet.md` and `agents/henad-graph-sheet-lower.md`: both sides' lines kept, with the sentences that named a refusal gone on either side rewritten to name only the refusals left.
    The reader tests now read eleven sheets, eight starters and three test sheets.
- One conflict surfaced at compile time rather than in the merge: `starters.rs`'s array length, 7 on both sides, is now 8 with SIR second and the ants last.
- The second commit conflicted in `CHANGELOG.md` and `agents/henad-graph-sheet.md`, resolved the same way, with the review's `NOT_YET_BLOCKED_MOVE_BY` added to the interim refusals.
- The full check then ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
  No fix was needed.
- No baseline test passes now.
- S20 changes no henad-app code, so no egui MCP check was repeated.

## Issues found & future directions

1. The plan lists two feature commits, `feat: ground, picks and repeat in agent sheets [draft]` and `feat: fuse folds over one neighbourhood [draft]`, and the branch holds one, `feat: ground, picks, repeat and fused folds in agent sheets [draft]`.
   Pick a cell is written as a member of the shared fold, and the ground's looks take the fold's cell from the same code, so a first commit without `fuse.rs` would write folds with no cell.
2. The plan puts the tests in `sheet_agents.rs` of both crates, and henad-graph's sit in a new `src/tests/sheet_ground.rs` beside it, since `sheet_agents.rs` holds the S17 tests at 600 lines.
3. The band reads deliveries by tick 2000 and by tick 3000, where the plan names tick 2000 alone.
   Round 1's hand-lowered ants deliver nothing by tick 2000 at seed 2, so no lower edge at tick 2000 can catch a rule error that stops deliveries, and the planted error sits outside only through the lower edge at tick 3000.
   The measured deliveries and the margins of both edges are in the Tests section.
4. The plan's `the_lowered_ants_and_boids_bind_at_most_8_storage_buffers` is S17's `the_lowered_agent_sheets_bind_at_most_8_storage_buffers`, which now covers the ants and asserts their tick pass at exactly 8.
   One more ant property or a second dynamic field passes the WebGPU default, which lane packing (K, S29) addresses.
5. A start on the ground supports a state painted on single cells only.
   A nest painted as an ellipse, or the base state, is refused with `ON_GROUND_CELLS`, since an agent cannot draw a cell of a state the field holds without a search.
   A later shape painted over a nest cell would also leave agents starting on the later state.
6. The shape geometry (fractions of the world, the angle towards smaller rows) is this milestone's reading of the author format, which leaves shapes unspecified.
   With it the ants' rocks cross the food-to-nest diagonal as the stock geometry's do, but the sizes differ from the stock's.
7. A Become or Set inside a Repeat applies when its condition held at any time, and the highest such row wins, where round 1's semantics say "the first time it applies".
   The two differ whenever two changes of one property inside one Repeat apply at different times round, and two chance lines inside one Repeat already do, with no carried value.
   Answer 11a chooses between sums that read the start of the tick and carried values, and says nothing about which time round wins.
   This sentence was corrected in the redesign audit (S32), which leaves the choice of rule to the maintainer.
8. Fusing a look into a shared fold changes the iteration word of a draw inside its body, the fold's `k`, so a sheet whose looks draw inside their test would replay differently from S17's lowering.
   No starter draws inside a look.
9. The ants read each trail once per visited cell, as round 1's hand-lowered file does: the fold holds three `field.get` nodes, the open test's Ground, Food way's To food and Home way's To home.
   When two looks in one fold read the same property, each look reads it, since the reads are cached per look.
   Sharing them per fold, as the open test is shared, would save the second read, and no starter holds two such looks.
10. The ants' grid of cells is the field grid, one cell per unit of the world, so a world of fractional size truncates its ground.
11. A look joins a fold only when every line it reads sits above the line that opened the fold.
    The rule is conservative: a look that reads an earlier line unrelated to the fold's looks, but lowered after the fold opened, takes a fold of its own.
    A rule on the lines that hold the fold's looks alone would let a cycle through a second fold, where the open-line rule keeps every fold reading only older folds.
    No starter meets the difference, and the ants keep their one fold.
12. The open test of Move to adds five nodes to the ants' tick pass (110 from 105), one `space.cell_index` and one `field.get` of the Ground at the next cell among them.
    The ants' Next cell is open on every row already, so the read is redundant for them, and a place known open from its line (an `open_only` pick, or a Choose whose rows test it) could skip it.
    The benchmark commands below measure the ants with it.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D49 | The add-up merge | recon of the engine, answer 10a | A power-of-two resolution (2^-16 for a sheet's "add up"), rounding half to even on both sides, a NaN adding 0, deposits clamped at 0 | S15, S20 |
| D64 | Which round 1 models ship as starters | round 1, section 14, answer 13a | All eight, forest fire and bounded confidence included, and the ants keep their slower trail with a note | S1b, S10, S17, S20 |

S20 also builds the rest of row R1 for agent worlds and row F1 for grids and agents, and keeps D38's interim refusals for the changes left inside a Repeat.
The refusal of rates went with S21.
The words S20 adds for its own errors sit in the wording module's group of words the plan adds, and no researcher met them.

## Benchmark commands left for the maintainer

S20 changes no engine and no hot loop, and the ants' cost is the question F1 left open.
To compare the ants sheet with `gpu_ants` and the ants graph starter at 1M ants, interleaved on a cooled machine as `agents/performance.md` asks:

```sh
for i in 1 2 3; do
  cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/ants.toml --set num_agents=1000000 --steps 500 --reps 3
  cargo run --release -p henad-cli -- gpu_ants --set num_agents=1000000 --steps 500 --reps 3
  cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/ants.toml --set num_agents=1000000 --steps 500 --reps 3
done
```

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
