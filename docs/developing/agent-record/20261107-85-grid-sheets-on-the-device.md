---
date: 2026-11-07
title: "Grid sheets on the device"
description: "S14 of the #37 redesign. The grid sheets run on the GPU engine under test: the Game of Life sheet's blinker and glider from planted lanes, Start again and a rebuild replaying on the device, every grid sheet stepping to the host evaluator's lanes, the chance SIR keeping its cell count, Game of Life and Majority vote equal to round 1's hand-lowered files every tick, and the forest fire and the chance SIR within bands of theirs with a planted rule error outside."
icon: material/chip
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s14-grid-sheets-device, rebased onto S10's record dc79b1b with no conflict: two unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: dc79b1b
delta_state: "committed on `37-node-editor` by fast-forward: `feat: grid sheet gates on the device [draft]`, `fix: s14 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Grid sheets on the device

> S14 holds the lowered grid sheets to the GPU engine, where S7 and S10 held them to the host evaluator alone.
> A test plants tick-0 lanes through the entry's hooks, since no sheet start and no app tool paints cells (D74): on the device the Game of Life sheet's blinker has period 2 and its glider moves by one cell on each axis every 4 ticks.
> Randomise draws a new layout each press on the device, the layout the host's rerun draws, and a rebuild at one seed replays the start.
> Game of Life, Majority vote, the forest fire and the chance SIR step to the host evaluator's lanes every tick for 50 ticks at 64 by 64, and the chance SIR keeps its cell count in lanes and measures for 200 ticks.
> Game of Life and Majority vote equal round 1's hand-lowered files in every lane every tick for 200 ticks, on the host and on the device, from shared tick-0 lanes at seeds 1 to 4.
> The forest fire and the chance SIR, whose draws use other keys, sit within bands of their hand-lowered files over seeds 1 to 4, and a sheet with a "from" state dropped from one Become falls outside them.

## State before

- `37-node-editor` at `dc79b1b` (S10 merged after S11), with S13a and S17 planned beside S14.
- The grid lowering held every line kind of a cells card but rates and Repeat, and `henad_graph::sheet::starters()` returned Game of Life, Majority vote and the forest fire.
  `sir_chance.toml` (round 1's SIR with chances per tick) was henad-graph test data.
- S11's `ModelFile` built a sheet's entry through its lowered graph, and henad-cli ran sheets.
- Every sheet test ran on the host evaluator.
  No test had run a lowered sheet on a device, and none had compared a sheet with the hand-lowered files of `crates/henad-models/tests/fixtures/lowered/round1/`, written in S1b for these gates.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### The sheets and their entries

`crates/henad-models/src/tests/sheet_grid.rs` is new, listed in `src/tests/mod.rs`.
Its sheets are the grid sheets of `henad_graph::sheet::starters()`, filtered by the lowered graph's world so an agents sheet joining the starters (S17) stays out, and `tests/fixtures/sheets/sir_chance.toml`, a byte copy of `crates/henad-graph/src/tests/sheets/sir_chance.toml`.
Entries come from `ModelFile::parse(text).entry()`, the path henad-cli and the app take.
Every lane of a lowered graph is read from its device buffer by the label `lane_<id>_cells`, found in `GridProgram::spec`, and from the host evaluator through `HostLanes::column`.

### Planted lanes

`planted_entry(file, planted)` compiles a graph on a grid with `GridProgram`, and its spec's hooks are a `PlantedHooks` that wraps the graph's own `GridHooks` and replaces the seeded words of the lanes `planted` names.
Every other hook forwards, so steps, presses and stats run as the plain entry's do.
The entry is rebuilt from the plain entry's declarations through `ModelEntry::from_gpu_grid_spec`, as `graph_ants_parity.rs`'s `planted_entry` does for agents.

### Tests

- `the_life_sheet_blinker_has_period_2_on_the_device` and `the_life_sheet_glider_moves_by_one_cell_on_each_axis_every_4_ticks_on_the_device`: S7's host known answers on a 32 by 32 wrapping world, the tick-0 Status planted.
- `a_start_again_button_draws_a_new_layout_each_press`: two presses of Randomise on the device at seed 7 each change the Status lane, a second build replays the start and both presses, and the host's `rerun` of presses 1 and 2 from `action_seed(Some(7))` writes the same words.
- `a_rebuild_replays_the_same_start`: two builds at seed 7 start alike, one at seed 8 differs, and the start holds between 1,000 and 1,500 alive cells of 4,096.
- `every_grid_sheet_steps_alike_on_the_host_and_the_device`: the four grid sheets, at seeds 3 and 11, from the start each side draws, every lane equal at tick 0 and after each of 50 ticks at 64 by 64.
  The host runs every tick pass in document order with `RunContext::tick`.
  The forest fire runs at a lightning chance of 0.001 and a growth chance of 0.05, so its lightning draw decides cells within 50 ticks of a 64 by 64 world.
  Each run must change a cell.
- `the_chance_sir_sheet_conserves_its_cells_every_tick_on_the_device`: 200 ticks at 64 by 64 and seed 5, a New outbreak press after the first tick that infects more cells, and Susceptible, Infected and Recovered summing to 4,096 every tick in the lanes and equal to the three measures.
- `the_drawless_sheets_equal_their_hand_lowered_files_every_tick_on_the_host` and `..._on_the_device`: Game of Life and Majority vote, whose cards draw nothing, from the tick-0 lanes each sheet draws on the host at seeds 1 to 4, planted in both graphs, equal to `lowered/round1/game_of_life.toml` and `vote.toml` in every lane after each of 200 ticks at 64 by 64.
- `the_forest_fire_and_the_chance_sir_sheets_sit_within_their_hand_lowered_bands`, below.
- `every_grid_sheet_entry_conforms`: `assert_set_conforms` over the four grid sheet entries, GPU checks on a baseline device, as `every_grid_corpus_entry_conforms` does for the graph corpus.

### Bands

The test's doc holds the procedure.
Each side, the hand-lowered file and the sheet, runs on the device from its own start at seeds 1 to 4, and each run gives five measures: the peak share of the active state (Burning or Infected), the share of each of the three states at the last tick, and the share of cells ever in the active state, the forest fire's burnt share.
The band of a measure is the hand-lowered file's mean over the four seeds, give or take five standard errors of the difference of the two means.
The variance of one run is the larger of the variance about each side's mean pooled over the eight runs (6 degrees of freedom) and the binomial `p (1 - p) / cells`, with `p` the centre kept at least one cell from 0 and 1.
The sheet's mean of every measure sits within its band, and the planted sheet moves at least one mean outside.

| Case | World | Ticks | Parameters | Planted rule error |
| --- | --- | --- | --- | --- |
| Forest fire | 512 by 512 | 150 | lightning chance 5e-5, others at defaults | line 3, "otherwise become Burning from Tree, with chance Lightning chance", strikes any state |
| Chance SIR | 512 by 512 | 60 | infection and recovery chances 0.1, initial infected at its default | line 3, "Become Recovered from Infected, with chance Recovery chance", recovers any state |

Margins on this machine (Apple M4 Pro), as the distance of the sheet's mean from the centre over the half-width:

| Measure | Forest fire | Planted forest fire | Chance SIR | Planted chance SIR |
| --- | --- | --- | --- | --- |
| Peak share | 0.33 | 0.36 | 0.15 | 40.2 |
| First state at the end (Empty, Susceptible) | 0.31 | 5.87 | 0.08 | 4.24 |
| Second state at the end (Tree, Infected) | 0.31 | 6.05 | 0.18 | 12.3 |
| Third state at the end (Burning, Recovered) | 0.02 | 3.53 | 0.16 | 12.3 |
| Ever in the active state | 0.35 | 3.74 | 0.08 | 644 |

### Docs and topic files

- `agents/models.md`: a paragraph on `sheet_grid.rs`, the fixture copy, the planted entry, each test, the band measures, sizes and parameters, the procedure and the planted rule errors.
  20,242 characters.
- `agents/henad-graph-sheet.md`: the line saying `sir_chance.toml` stays test data names the copy in henad-models and asks for an edit to be copied there.
  39,120 characters, near the 40k split point.
- `CHANGELOG.md`: one line under Unreleased for the grid sheet oracles.

### Edited tree

```
CHANGELOG.md                                     grid sheet oracles
agents/
├── henad-graph-sheet.md                         the copy of sir_chance.toml in henad-models
└── models.md                                    the grid sheet gates, the fixture copy, the bands
crates/henad-models/
├── src/tests/
│   ├── mod.rs                                   sheet_grid
│   └── sheet_grid.rs                            new: the device gates, planted lanes, bands
└── tests/fixtures/sheets/
    └── sir_chance.toml                          new: a copy of henad-graph's test sheet
```

### Deviations from the plan

- **The band procedure.** The plan asks for the band procedure of `graph_sir_parity.rs`, five binomial standard deviations about an expected share.
  The binomial deviation alone is far too narrow for a peak or a final share of an epidemic or of fires, whose cells decide together.
  A first version took the band about the hand-lowered file's mean with the larger of the binomial deviation and the four reference runs' sample deviation, and held each sheet run to it.
  It failed the sheet's empty share of the forest fire at seed 1 (0.4804 against an upper bound of 0.4782): seeds 1 to 3 of the hand-lowered file landed within 0.0006 of each other in the empty and tree shares, and the four runs' deviation of the tree share came out at 0.0030, against 0.0076 over 16 seeds.
  Over 16 seeds each, the sheet's and the hand-lowered file's means agreed on every measure of both cases within 1.6 standard errors of their difference.
  The test now compares the two means over seeds 1 to 4 with the pooled variance of all eight runs, a two-sample test at five standard errors that keeps the plan's seeds and the binomial floor of `graph_sir_parity.rs`.
- **Band sizes and parameters.** The plan names neither.
  At the defaults the chance SIR infects every cell of a 256 by 256 world within 30 ticks, so its final shares and its share ever infected hold no information, and the forest fire at a lightning chance of 10^-5 has a few strikes per run, with the spread of self-organised criticality.
  The cases above make every measure informative.
  At a lightning chance of 0.001 and 300 ticks every cell of the forest fire burned at least once in every run.
- **The planted rule errors.** The plan asks for "the 'from' state dropped from one Become".
  The forest fire's line 2 so planted keeps every burning cell with a burning neighbour alight and burns every cell, which shows nothing about the margin.
  Line 3, lightning striking empty cells too, is the smallest change of the five, caught by 3.5 to 6 half-widths.
  By the row order, line 5 dropping its "from" would only add a row after line 4 for a burning cell and a Tree to Tree row for a tree, so it was not used.
  This was read from the lowering, not run.
- **Test names.** The glider test reads "moves by one cell on each axis" where the plan has "the same shape moved by (1, 1)", and the device known answers carry `_on_the_device`, since `sheet_lower.rs` holds the host names.
- **The New outbreak press** in the conservation test comes after the first tick.
  At tick 20, the first choice, a 64 by 64 world had no susceptible cell left, and a press could not show it infects more.
- **The host and device comparison** uses seeds 3 and 11 and raises the forest fire's lightning and growth chances, which the plan leaves open.

### Review

The review raised nine findings, and all nine were fixed in `fix: s14 review findings [draft]`.
The closure bindings `centre` in `bands` and `outside` became `center`, as code identifiers take American spelling, and the binding `out` became `outliers`.
`agents/models.md` lost the sentence on why the plain binomial band and a four-run deviation were not used, with its measured ratio.
That reasoning stays in the deviations above.
Several test comments now follow the wording rules: "instead of" for "in place of", the trailing "which" on the planted hooks split into two sentences, the `lane_buffers` doc opening with "Returns", and the `device` doc saying it returns `None` with a warning.
The band test's doc now opens by naming the two-sample comparison with the binomial variance as a floor, and its step 3 states the variance rule in a sentence of its own.
`device_params` panics when an override id names no parameter of the entry, and `a_mistyped_override_id_panics` holds that.
The drawless host test and `shared_start` pass each document's default parameters through `default_host_params` instead of an empty slice that worked only through the host evaluator's fallback to defaults.
`agents/henad-graph-sheet.md` now names the copy of `sir_chance.toml` in henad-models.
A deviation bullet about the record's commit, written for the merge step alone, was dropped, and the size of `agents/models.md` corrected.
The eleven `sheet_grid` tests pass with `HENAD_REQUIRE_GPU=1`, and clippy and rustfmt are clean over henad-models.

## State after

- Every grid sheet runs on the device under test, equal to the host evaluator, and Game of Life and Majority vote equal their hand-lowered references on both.
- The forest fire and the chance SIR are held to their hand-lowered references by bands with a planted rule error behind each.
- The testing kit passes every grid sheet entry.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-models --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-models --all-targets --all-features --no-fail-fast` (library 175 passed, 1 failed, 1 ignored, the failure `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` of the baseline; the six integration test binaries pass, 41 tests); `cargo check -p henad-models --all-features --lib --target wasm32-unknown-unknown`.
  The eleven new tests take about 35 seconds together in a debug build.
  The full check runs at the merge point.

### Merge check

- The branch already started at `dc79b1b`, the tip of `37-node-editor`, so the rebase changed nothing and brought in no conflict.
- The full check ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

1. The device runs of the bands and of the host comparison hold on one adapter.
   A sheet's chance once for each of a count goes through `math.powi` in `f32`, and an adapter that rounds it differently by one unit would move a cell, after which a trajectory drifts.
   The host comparison would then fail where the lowering is right, and the bands would see another sample of the same law.
   CI's lavapipe runner is the second adapter these tests meet.
2. Four seeds give 6 degrees of freedom to the pooled variance, so a five standard error band misses about one fair comparison in 400 per measure, ten measures in all.
   The outcome is fixed on one adapter, since every run is seeded.
3. `tests/fixtures/sheets/sir_chance.toml` can drift from henad-graph's copy.
   No test holds the two equal, since a henad-models test would have to read a path outside its crate.
   `agents/models.md` and `agents/henad-graph-sheet.md` both ask for the copy to be refreshed with the source.
4. `PlantedHooks` exists twice now, once per engine trait (`AgentHooks` in `graph_ants_parity.rs`, `GridHooks` here).
   Both are test code, and the traits share no method set to abstract over.
5. The bands run the hand-lowered files on the device only.
   The host evaluator would take minutes for 512 by 512 worlds in a debug build.
6. S20's fold fusion changes the lowered grid sheets' code, and its plan asks these gates to pass after fusion.
   S21 adds `sird.toml` beside `sir_chance.toml` in `tests/fixtures/sheets/`.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D73 | The World pane's controls kept as the dock has them | the final SPEC, "World"; round A changes F15, F20, F22; the prototype notes | **Plan's choice** within D56. The plot's whole run (F15) is the Charts tab's Unlimited history. The sticky run bar (F20) is the Playback tab, which never scrolls away. The legend stays on the Charts tab and current values on the Stats tab. The world's Start again is Playback's Build, which replays at the same seed, and a sheet button's Start again draws a new layout each press, as the prototype notes say. The 0 label on the left axis (F22) is built | S7, S9, S13a, S14 |
| D74 | Hand-set cells | round A changes F1 and the prototype notes name "a hand-set cell" | **Plan's choice.** Not built. The prototype's hand-set cell is a test hook (`setCell` in its test API), no control in the final SPEC paints cells, and henad-app has no cell brush. Blinker and glider checks stay headless, with lanes planted by the test, and the app checks use a random start | none |

S14 builds D73's two meanings of Start again on the device (`a_rebuild_replays_the_same_start`, `a_start_again_button_draws_a_new_layout_each_press`) and D74's headless blinker and glider with lanes planted through the entry's hooks.
No row of the gate names S14.

## Benchmark commands left for the maintainer

None.
S14 adds tests only and changes no engine, lowering or hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
