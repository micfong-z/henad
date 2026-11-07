---
date: 2026-11-07
title: "Agent sheets"
description: "S17 of the #37 redesign. Sheets of one kind of agent in open space lower: the agents world, Position, Arrow and Heading lanes, starts anywhere and as random directions, looks at agents within a distance sharing one radius fold at the largest distance, the power rule over agents, Steer, Keep between, Move by and a Repeat of moves, and fault messages that name the author's lines. The random walk, boids and bounded confidence become sheet starters, run 100 ticks on a baseline device, and hold bands against round 1's hand-lowered files."
icon: material/bird
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s17-agent-sheets, whose review step had rebased it onto fc8aa77 and whose merge rebase found nothing to move: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: fbd9aa9
delta_state: "committed on `37-node-editor` by fast-forward: `feat: fault messages name author lines [draft]`, `feat: lower agent sheets with radius folds and pow [draft]`, `fix: s17 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Agent sheets

> S17 lowers a sheet of one kind of agent in open space.
> Position is the `vec2f` lane the spatial index reads, an Arrow and a Heading are `vec2f` lanes, and the Heading is dropped when nothing reads it.
> The looks at agents of a card share one `agent.radius_fold` at the largest of their distances, a smaller distance kept as a `dist_sq` filter, and the index's cell size is the largest current distance divided by 3 from one node, so V14 holds and the engine sizes the index from the parameters as they stand (row A).
> A chance once for each of a count of agents is a `math.pow` behind a guard for no try, never a `math.powi` (row P).
> Steer, Keep between, Move by with `space.wrap`, Random direction and a Repeat of moves and steers lower, and the lines that wait for the ground, places and events are refused at their table in the author's words (D38).
> A capped fold's fault names the author's lines through a `FaultNames` table beside the graph (D47), and `SheetFile::shortfalls` names a look that costs too much in the model's words (X4).
> The random walk, boids and bounded confidence move into `crates/henad-graph/sheets/` and `starters()`, run 100 ticks on a baseline device, and hold their bands against round 1's hand-lowered files on seeds 1 to 3, each band missed by a planted rule error.

## State before

- `37-node-editor` at `fbd9aa9`, S10 and S11 merged, with S13a and S14 open beside S17 on no shared file.
  The review step rebased the branch onto `fc8aa77`, after S13a and S14 had merged.
- The lowering held every grid line kind but rates and Repeat, and refused a world of agents at `[world]` with "Agents in open space are not supported yet." (D38).
- The random walk, boids and bounded confidence sheets waited as test data in `crates/henad-graph/src/tests/sheets/`.
- A radius fold's fault named the pass, the node and the line of the lowered graph's text, which a sheet's reader never sees.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### One kind of thing

The lowering's `Builder` now lowers over one kind of thing, the cells or the one kind of agent (D43, which the checker's `ONE_AGENT_KIND` already enforces).
Its properties, its id as a card's `every` names it, the first segment of a sheet-owned node's slot path (`cells` or `agents`), its domain and its tables come from that kind, so the starts, the card, the colours, the measures and the buttons lower over either.
Pass ids follow the kind: `start_<thing>`, `tick_<card>`, `colours`, `measures`, `button_<button>` and `restart_<button>`.
A sheet of agents first refuses walls (row H), a ground with properties or blocked states, and a count of events, each at its table.

### World and starts

`[agents]` takes the kind's `count` and `max_count`, and the world's width and height.
Position is the `vec2f` lane with `role = "position"`, and an Arrow and a Heading are `vec2f` lanes.
The Heading has no lane when no line, phrase, formula, measure or colour rule reads it, so bounded confidence carries none.
A Heading holds the last move that was not 0, the form of round 1's hand-lowered files.
Position starts anywhere as two `rand.float` over `ctx.world`, keyed `starts_as.x` and `starts_as.y`, and an Arrow as a random direction whose length is a number, a parameter or a random number.

### Looks at agents (row A)

`sheet/lower/agents.rs` is new.
At the card's first look at agents, every look that runs is grouped into folds in line id order: by whether it includes me, then a new fold past 16 accumulators.
The sheet owns, in a new group `GROUP_LOOK` at path `card.<card>`, the largest distance (a `math.max` chain over the distinct distances in a fixed order), the cell size `math.div(largest, 3)`, and each fold.
Each look holds a count accumulator and one per summary, named by its line id and written in line id order after the card, so reordering two looks keeps the code.
A look whose fold walks a larger distance than its own keeps the agents within its own through `cmp.le(dist_sq, d * d)`.
`their` reads the other agent's property at the fold's `other`, and the offset to them, away from them and the distance to them come from the fold's `delta` and `dist_sq`.
An average, largest or smallest of no one is the zero of its type, and a Steer towards one adds nothing, as round 1's vocabulary says.
The boids' Flockmates and Too close share one fold with five accumulators, the hand-lowered file's shape.

### The power rule over agents (row P)

A chance once for each of a count of agents takes `math.pow`, since the count has no bound by the cells around, and every `math.pow` now sits behind a `logic.select` that takes 1, a miss of every try, for a count of 0 or less.
`pow(0, 0)` is not 1 on every device, which made a chance of 1 for each of no one a coin toss there.

### Motion and Repeat

`sheet/lower/motion.rs` and `sheet/lower/repeat.rs` are new.
A Steer adds `value * strength`, or `(value - my X) * strength` for towards, under its condition.
Keep between holds a number through `math.clamp`, and an arrow through the clamp of its length, an arrow of length 0 becoming one of the lower length along the x axis.
Steers and Keep betweens apply in row order after a property's Set chain, and `my new X` is the value before the write.
The Move by lines add up, Position moves by their sum and wraps at the edges, and the Heading takes the sum when it is not 0.
A Repeat is one `region.repeat` whose count is a whole number or a Whole number parameter from 0, its `max` that number or the parameter's maximum, with a `sum` accumulator for each property a Move by or Steer inside it changes.
The Calculate lowering reads arrows as `vec2f` inputs, with `length` as `length` and `direction` as the arrow divided by its length, and a measure's formula lowers through the same code with a scope of my properties and the parameters.

### Refusals

On a card of agents the refusals in the author's words are rates, Record, Leave, Pick a cell, the cell here or ahead, Move to, a look at the cells around (the ground), and a Become, Set, Keep between, When or Repeat inside a Repeat.
A property changed two ways is refused at the second line, naming the first: "Line 3 steers Velocity. A card that steers a property cannot also set it.", and likewise for Set before Steer and for Move by and Move to.
That error takes the place of the line's own refusal, so each refusal gives one located error.
The words of the S7 refusal of a world of agents are removed from `sheet::sentence`.

### Fault names and costs (D47, X4)

`henad_graph::wgsl::FaultNames` maps a node id to words such as "line 1, Like-minded", and `AgentProgram::compile_with_fault_names` takes it beside the graph.
A named fold's fault reads "lines 1 and 2, Flockmates and Too close: looked at more than 65536 agents around one agent, and the run stops", with no pass, node or line of the graph, and a graph file passes no table and keeps its message.
`SheetFile::fault_names` holds the lowering's table, and henad-explore's `ModelFile::entry` passes it for a sheet.
`SheetFile::shortfalls` maps the budgets: the entry's shortfalls, with a submission past its budget named by the looks of the heaviest card ("Line 1, Flockmates: looks at too many birds at 1,000,000 birds. A shorter distance or fewer birds would fit."), or by the card when it looks at no agents, and starts past the host's budget by the number started.
`SheetFile::place_graph_diagnostic` maps the checker's cost per element (rule V22) on a lowered pass to the line of its heaviest node, as "Line 1: too much work for one walker. A smaller maximum for Steps per tick would fit.", and drops the note naming a node.
A Repeat of a fixed number of times suggests fewer times, and any other line or a card fewer lines.

### Tests

- `crates/henad-graph/src/tests/sheet_agents.rs`, new: the three agent starters and the test sheet `mobile_sir.toml` lower with no warning, check, compile, are their own canonical text, lower twice to one text, and keep their nodes in their line blocks; the worlds and the dropped Heading; one shared fold for the boids with its accumulators, name and filters, and a look reorder keeping the code; `the_radius_index_follows_the_current_largest_distance`; the power rule on the host (0.488 for p = 0.2 and n = 3 within five standard errors over 20,000 draws, 0 for p = 0 and n = 1000); `for_each_of_zero_with_chance_1_is_0_on_host_and_device`; `each_agent_refusal_gives_one_located_error`; a look at the cells around; `the_shortfalls_of_a_sheet_name_its_looks_in_the_model_words`; `a_pass_cost_past_the_limit_names_its_line_in_the_model_words`; and the fold as a node the sheet owns.
- `crates/henad-models/src/tests/sheet_agents.rs`, new, on a `TestDeviceRequest::baseline()` device: `every_agent_sheet_lowers_checks_and_compiles` (100 ticks and a stats read), `the_lowered_agent_sheets_bind_at_most_8_storage_buffers`, `every_agent_sheet_entry_conforms` (the testing kit, then each sheet at its `max_count`), `a_capped_fold_fault_names_the_author_line`, and three bands with their procedures in each test's doc.
  The random walk's mean squared step over a tick lies within five standard errors of 16 for the sheet and the hand-lowered file on seeds 1 to 3, and a planted move outside the Repeat misses it.
  The sheet's boids keep every speed within 3 and 15 after each of 300 ticks with 20,000 birds on every seed, and a planted card without its Keep between line leaves those limits.
  Their polarisation after the last tick lies within 0.15 and 0.95 for both files on every seed, while a planted Steer by the average velocity reaches 0.999.
  Bounded confidence forms one opinion group after 100 ticks for both files on every seed, and a planted Steer by the average leaves none.
- `sheet_lower.rs`: the slot path test covers every starter, the agent refusal leaves the interim refusal test, and `grid_sheets` keeps the grid starters.
- `sheet_document.rs` and `sheet_sentence.rs`: six starters, the test sheet list with `mobile_sir.toml`, and the new words in the wording table test.
- henad-app's `src/tests/sheet_editor.rs`: the starter loop expects the preview run of each starter's world, and `the_new_model_dialog_offers_every_grid_starter_and_no_agent_starter` holds the dialog's filter.

### Docs and topic files

- `agents/henad-graph-sheet.md` would pass 40k, so its lowering section moved to the new `agents/henad-graph-sheet-lower.md` (22,943 characters) with a row in `AGENTS.md`'s topic table, and gained the agent lowering, A, P, the refusals, `Repeat` and the tests. The sheet file keeps `SheetFile`, `fault_names` and `shortfalls` (28,477 characters).
- `agents/henad-graph-wgsl.md`: `FaultNames` (14,985 characters).
- `agents/models.md`: the agent sheet gates and bands (21,640 characters after the rebase).
- `agents/henad-explore.md`: `graph_entry` passing a sheet's fault names (32,060 characters).
- `docs/reference/model-files.md`: a section on a sheet of agents with the random walk included.
- `agents/henad-app-sheet.md`: the New model dialog's filter and the two app tests.
- `CHANGELOG.md`: one line under Unreleased.

### Edited tree

```
AGENTS.md                                        topic row for the sheet lowering
CHANGELOG.md                                     agent sheets, fault names, shortfalls
agents/
├── henad-app-sheet.md                           the dialog's grid starters, the starter loop
├── henad-explore.md                             graph_entry takes fault names
├── henad-graph-sheet.md                         lowering moved out, SheetFile additions, starters
├── henad-graph-sheet-lower.md                   new: the grid and agent lowering
├── henad-graph-wgsl.md                          FaultNames
└── models.md                                    agent sheet gates and bands
crates/henad-app/src/
├── tests/sheet_editor.rs                        each starter's preview run, the dialog's filter
└── ui/editor/sheet/new_model.rs                 the grid starters alone
crates/henad-explore/src/
└── model_file.rs                                a sheet's fault names passed to the compiler
crates/henad-graph/
├── sheets/
│   ├── random_walk.toml                         moved from src/tests/sheets/
│   ├── boids.toml                               moved from src/tests/sheets/
│   └── bounded_confidence.toml                  moved from src/tests/sheets/
└── src/
    ├── check/
    │   ├── cost.rs                              is_pass_cost
    │   └── mod.rs                               is_pass_cost
    ├── sheet/
    │   ├── file.rs                              fault_names, shortfalls, V22 words, agents.<property> places
    │   ├── sentence.rs                          refusals, two-way changes, look names, cost words
    │   ├── starters.rs                          the three agent sheets
    │   ├── check/formula.rs                     references visible to the sheet module
    │   └── lower/
    │       ├── mod.rs                           one kind of thing, agent tables refused, fault names
    │       ├── agents.rs                        new: looks at agents, folds, relations
    │       ├── motion.rs                        new: Steer, Keep between, Move by, Random direction
    │       ├── repeat.rs                        new: Repeat of moves and steers
    │       ├── world.rs                         agents world, vec2f lanes, the dropped Heading
    │       ├── start.rs                         anywhere, random directions, start_<thing>
    │       ├── card.rs                          agent refusals, two-way changes, new changes, transition
    │       ├── look.rs                          Fold over agents, guard of any type
    │       ├── draw.rs                          the guarded math.pow
    │       ├── calculate.rs                     arrows, length, direction, the Scope trait
    │       ├── measures.rs                      over agents, vec2f means, formulas
    │       ├── colours.rs                       by heading, over agents
    │       ├── buttons.rs                       over agents
    │       └── ids.rs                           GROUP_LOOK
    ├── wgsl/
    │   ├── mod.rs                               fault_names module
    │   ├── fault_names.rs                       new: FaultNames
    │   └── agents.rs                            compile_with_fault_names
    └── tests/
        ├── mod.rs                               sheet_agents
        ├── sheet_agents.rs                      new: the tests above
        ├── sheet_document.rs                    six starters, mobile_sir.toml
        ├── sheet_lower.rs                       every starter's paths, grid starters
        ├── sheet_sentence.rs                    new words
        └── sheets/
            └── mobile_sir.toml                  new: a chance once for each infected person nearby
crates/henad-models/src/tests/
├── mod.rs                                       sheet_agents
└── sheet_agents.rs                              new: device gates and bands
docs/reference/
└── model-files.md                               a sheet of agents
```

### Deviations from the plan

- **A Repeat of moves and steers lowers in S17.** The plan leaves Repeat to S20, yet S17's acceptance runs the random walk, whose card is a Repeat of one Move by.
  `sheet/lower/repeat.rs` lowers a Repeat whose changes are Move by and Steer, with `sum` accumulators, which round 1 section 6 and answer 11a both allow.
  A Become, Set, Keep between, When or Repeat inside a Repeat is refused with "Become, Set, Keep between, When and Repeat inside a Repeat are not supported yet." until S20.
- **Move to is refused, not lowered.** A Move to needs a place, and only Pick a cell and the cell here or ahead give one, both S20's.
  Move to is refused with "Move to is not supported yet.", and the refusal of Move by with Move to is tested with a refused Pick a cell feeding the Move to, which gives one error per line.
- **The refusals of a second kind and of a look inside a Repeat are the checker's**, at their tables, since S5 built them.
  S17's test holds that each gives one located error.
- **A fused fold per card in S17.** The plan's "one `agent.radius_fold` per card" fuses the looks at agents of a card now, where F1 (S20) fuses the folds over cells.
  The fold belongs to the sheet, not to a line, so its id depends on no line's place, and it splits by `include_me` and past 16 accumulators.
- **A distance is a number or a parameter.** A fold's radius is the same for every agent, so a distance from a property or a result is refused at the line.
- **A Count over agents stands on its own line.** In a slot it is refused, so the mobile test sheet counts on a line and reads the count through a result chip.
- **The `math.pow` guard covers grid sheets too.** Every `math.pow` of the power rule gains the guard, which changes the code of a grid sheet whose chance takes `math.pow`, none of the starters among them.
- **Heading holds the last move**, not its unit direction, as round 1's hand-lowered files do.
- **The bands are absolute.** The boids' polarisation varies between 0.3 and 0.9 across seeds for both files, so the band is an interval each run must fall in, not a distance between the two files' means.
  The bounded confidence band counts opinion groups after 100 ticks, where both files converge to one group at the defaults.
- **The first commit is the fault names**, so each commit builds: the lowering's commit uses the table.
- **The mobile test sheet** is named "Mobile contacts" and has no Become: it holds a Count and a Set of a yes or no property to a chance once for each, which is what the power rule's tests read.

### Review

A reviewer with a fresh context listed one must-fix, one should-fix and six nits, and `fix: s17 review findings [draft]` holds every one of them.

- **Rebased before the merge step.** The branch started at `fbd9aa9`, and S13a and S14 merged after that.
  The review step rebased it onto `fc8aa77`, since henad-app's fix only exists on the rebased tree.
  `CHANGELOG.md` and henad-models' `src/tests/mod.rs` conflicted in lists, and both sides were kept.
  `agents/henad-graph-sheet.md`'s starters paragraph now holds S14's sentence about the henad-models copy of `sir_chance.toml`.
  `agents/models.md` takes the tip's text with S17's paragraph added after S14's, without the reflow.
- **The agent starters in henad-app.** S13a's loop over `starters()` asserted a grid preview, and would have failed on the random walk.
  It now expects `PreviewRun::Agents` for an agent sheet, and all three agent starters open, preview at tick 0, play, apply and build in the app's test.
  The New model dialog keeps its three grid starters: `starter_choices` holds out a starter whose world is not made of cells, since the sheet pane has no agent section and gate 10.13 leaves the agent starters' descriptions to S26.
  S26 lists them, and `the_new_model_dialog_offers_every_grid_starter_and_no_agent_starter` holds the filter until then.
  No egui MCP check ran: the dialog renders the same three radios as at the tip, which the `Harness` test confirms by accessible names.
- **V22 in the model's words.** A pass past `MAX_PASS_COST` on a lowered sheet read "the pass 'tick_walker_tick' costs 983056 units per element ... / note: Repeat n1001 costs 983041".
  `check::is_pass_cost` recognises the message from the same text the checker writes, and `place_graph_diagnostic` rewords it through `SheetFile::cost_words` for every caller (henad-explore's `ModelFile::entry` and the app's sheet editor).
  `henad-cli --model-file` on the random walk with `max = 65536` now prints "error: Line 1: too much work for one walker. A smaller maximum for Steps per tick would fit." at the Repeat line.
  The plan's Risks entry asks for one mapping function, but the two mappings stay apart: `shortfalls` works on an entry's budgets at given parameters, and `cost_words` on a checker diagnostic before any entry exists.
  Both draw their words from the same group in the wording module, and S20 extends `cost_words`.
- **Nits.** `NOT_YET_REPEAT_CHANGE` names Repeat.
  Five comment paragraphs were re-broken within 120 columns: the four the reviewer named and `SheetFile::fault_names`.
  The trailing "which" clauses the reviewer named were split into two sentences.
  `agents/henad-explore.md` is back to its original wrapping with only the added sentence.
  The future S20 sentence left `agents/henad-graph-sheet-lower.md`.
  The boids band now reads every speed after each of the 300 ticks on all three seeds, with a planted card without its Keep between line that leaves the limits.
- **Checks after the fixes**, in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph -p henad-models -p henad-explore -p henad-app --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-models -p henad-explore -p henad-app --all-targets --all-features --no-fail-fast` (only the baseline budget test fails); `henad-cli --test model_file`; the facade's `facade_paths`; the doc tests and `cargo doc` of henad-graph and henad-app; the wasm32 check of henad-graph and henad-app.
  The full check still runs at the merge point.

## State after

- Sheets of one kind of agent in a wrapping world lower, check and compile, and the random walk, boids and bounded confidence are sheet starters after the three grid sheets.
- Ground, Pick a cell, the cell here or ahead, Move to, Leave, Record and the changes other than moves and steers inside a Repeat wait for S20, and rates for S21, each refused at its table.
- A capped fold's fault names the author's lines, and `SheetFile::shortfalls` words a population past the work budget.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-graph -p henad-models -p henad-explore --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-models -p henad-explore --all-targets --all-features --no-fail-fast` (only the baseline budget test fails); `HENAD_REQUIRE_GPU=1 cargo test -p henad --all-features --test facade_paths`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-cli --test model_file`; `cargo test -p henad-graph -p henad-explore --doc`; `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph -p henad-explore --no-deps --all-features`; `cargo check -p henad-graph -p henad-models -p henad-explore --all-features --lib --target wasm32-unknown-unknown`.
  The full check runs at the merge point.

### Merge check

- The merge rebase onto `37-node-editor` found the branch up to date: its review step had already rebased it onto `fc8aa77`, and nothing merged since.
  No conflict came up, and no henad-app change arrived, so no egui MCP check was repeated.
- The full check ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

1. At their `max_count` of 1,000,000 and their defaults, boids and bounded confidence go past the submission budget, and `every_agent_sheet_entry_conforms` accepts each as refused in the model's words ("Lines 1 and 2, Flockmates and Too close: looks at too many birds at 1,000,000 birds. ..."); the random walk fits.
   The app's build does not call `SheetFile::shortfalls` yet, so the app shows henad-compute's own words until a later app milestone wires it in.
   The sheets' `max_count` could come down to what the budget admits, a choice for the maintainer.
2. The plural of a kind's name adds an "s", so bounded confidence's people read "persons", as the sheet's sentences already do.
3. The checker accepts `my Velocity is more than 1`, an arrow compared with a number, and the lowering refuses it for a look at the cells around before the comparison is reached.
   A comparison of an arrow with a number deserves a checker error in the author's words.
4. A Look around summary no line reads keeps its accumulator, and a Heading read only by a measure keeps its lane and its write in the tick pass.
   Lane packing (K, S29) and an unused-accumulator pass would trim both.
5. The bands run on the device with seeds 1 to 3, and the boids' polarisation is a wide band: it catches a rule that turns flocking into one heading, and would miss a subtler error in alignment.
   A band on the mean speed or the nearest-neighbour distance would tighten it.
6. Set always lowers as a `logic.select` with a constant `true` condition, as S7's grid lowering does, and the lowered text keeps the select.
7. The sentence of a Set to a chance reads "Set Hit to with chance Infection chance for each of Infected nearby, always", S4's wording, which reads poorly for an author.
8. `space.wrap` keeps every agent on the torus, and a world with walls is refused until row H.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D38 | Line kinds whose lowering lands later | the plan | A located error in the author's words until the lowering exists, each lifted with a test that the refusal is gone | S7, S10, S17, S21 |
| D43 | More than one agent kind | recon of henad-graph | Refused, since `AgentsWorld` holds one population | S5, S17 |
| D47 | Fault messages from GPU code | recon of henad-graph | A `FaultNames` table beside the document, so a fault names the author line | S17 |
| D64 | Which round 1 models ship as starters | round 1, section 14, answer 13a | All eight, forest fire and bounded confidence included, and the ants keep their slower trail with a note | S1b, S10, S17, S20 |

The gate's section 10.21 lists "One kind of agent per model in this version." (D43), which S5 built and S17's test reads.
The words S17 adds for its own refusals, the two-way changes, the look names and the cost messages sit in the wording module's group of words the plan adds, and no researcher met them.

## Benchmark commands left for the maintainer

None are required: S17 changes no engine and no hot loop.
To compare the lowered boids with the graph starter, interleaved on a cooled machine as `agents/performance.md` asks:

```sh
for i in 1 2 3; do
  cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/boids.toml --set num_agents=200000 --steps 500 --reps 3
  cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/boids.toml --set num_agents=200000 --steps 500 --reps 3
done
```

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
