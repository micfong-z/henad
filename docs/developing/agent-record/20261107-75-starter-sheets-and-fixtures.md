---
date: 2026-11-07
title: "Starter sheets and fixtures"
description: "S1b of the #37 redesign. henad-graph gains its first sheet starters, Game of Life and Majority vote, in a sheets directory returned by sheet::starters(), and four more sheets written from round 1's models wait as test data. henad-models gains eleven hand-lowered graph files, round 1's eight models and the rates design's three SIR models, with the procedure that wrote and checked them."
icon: material/file-star-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s1b-starter-sheets, two unsigned [draft] commits and this record after d1838bb; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: f4088d7
delta_state: "committed on `37-node-editor` by fast-forward: `feat: starter sheets and hand-lowered fixtures [draft]`, `fix: s1b review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Starter sheets and fixtures

> S1b adds the data the sheet milestones after it test against.
> henad-graph gains a `sheets/` directory and `henad_graph::sheet::starters()`, which returns its two sheets: Game of Life, moved there from the test sheets, and Majority vote, written from round 1's model.
> Four more sheets written from round 1, the forest fire, the random walk, bounded confidence and SIR with chances per tick, join the test sheets until their lowering exists.
> Every written sheet holds the gate's stored forms: an explicit `always`, `any_state` for a Become from any state, and the SIR button named New outbreak.
> henad-models gains `tests/fixtures/lowered/`, eleven graph files written by hand as the sheet lowering would write them: round 1's eight models and the rates design's SIR with rates and its two SIRD variants.
> Each of the eleven ran 20 steps with `henad-cli --model-file`, every button pressed, before the commit, and `tests/fixtures/docs/lowered_fixture.md` holds that procedure and the mapping the files were written from.
> No engine or library code changes, apart from the starters function and two doc lines on `Starter`.

## State before

- `37-node-editor` at `15d8717`, with S1a, S3 and S6 merged.
  S1a's sheet reader read four test sheets in `crates/henad-graph/src/tests/sheets/`: Game of Life, SIR with rates, boids and ants, copied from the author format's examples with the gate's stored forms written in.
- No sheet starter existed, and nothing in the repository held round 1's other four models as sheets.
- The hand-lowered graph files of round 1 and of the rates design existed only in the design folders, which git ignores.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Sheet starters

`crates/henad-graph/sheets/` holds the sheet starters, embedded with `include_str!` inside the crate, since the packaging check refuses a path that leaves it.

- `game_of_life.toml`, moved from `src/tests/sheets/` unchanged.
- `vote.toml`, "Majority vote", the name the gate gives it, written from round 1's Vote: Opinion with states A and B, B with chance Initial share of B, the measure Holding B, and the card "B in my block = count of the 3 by 3 block, me included, whose Opinion is B", "Become B from any state, when B in my block is at least 5", "otherwise become A from any state, always".

`sheet/starters.rs` holds `starters()`, re-exported as `henad_graph::sheet::starters`.
It returns `crate::starters::Starter` values, the file name and text the graph starters already use, so the New model dialog of S13a can list both kinds through one type.
The doc of `Starter` now names a sheet as well as a graph file.

### Test sheets written from round 1

Four sheets join `crates/henad-graph/src/tests/sheets/`, each written from round 1's model data.

| Sheet | Model | What it adds to the test sheets |
| --- | --- | --- |
| `forest_fire.toml` | Drossel and Schwabl's forest fire, Cover with Empty, Tree and Burning, the button Replant | a count of the 4 cells around, two independent chances each holding a key, a Become that always holds, a parameter default of `1e-5` |
| `random_walk.toml` | walkers on a wrapping plane, the colour by heading | a Repeat holding a Move by, a whole-number parameter |
| `bounded_confidence.toml` | Hegselmann and Krause among people who wander, the button Mix opinions | a Look around over agents with me included, an "only" filter comparing their Opinion with mine within Confidence, a Steer towards, a spread measure, a shaded colour |
| `sir_chance.toml` | round 1's SIR with chances per tick, sheet id `sir_chance` | a chance "for each of" a count, a sheet with no `[time]`, the button New outbreak and its Run once card |

The keys of every drawing line and Starts as were minted at random.
Each sheet was made canonical with the author format's checker script on a scratch copy outside the repository.
The script left every table as written and added the comments, and every reference resolved.
The script predates D81, so it refuses `any_state` as a state and writes " if any_state" and ", when always" in comments, and those two forms were rendered "from any state" and ", always", as S1a wrote Game of Life's Clear.

### Hand-lowered fixtures

`crates/henad-models/tests/fixtures/lowered/` holds:

- `round1/`: the eight hand-lowered graph files of round 1 (`game_of_life`, `vote`, `sir`, `forest_fire`, `random_walk`, `bounded_confidence`, `boids`, `ants`), model ids `r1_<model>`.
- `rates/`: `sir_rates.toml`, `sird_competing.toml` and `sird_higher_wins.toml`, model ids `r2_<model>`, with the tick length a `tick_length` parameter in days.
  `sir_rates.toml` is unchanged, as the plan asks until S2 adds its keys.

The files are copies of the design's, apart from one comment in `sird_higher_wins.toml`, which named a design file by its name and now reads "This is the biased lowering the rates design rejects."
The directory sits apart from `tests/fixtures/graphs/`, whose corpus loaders panic on a file that does not equal its stock model.

`tests/fixtures/docs/lowered_fixture.md` is the procedure, written by hand from round 1's lowering map and the rates design's sections on the lowering and the SIR starter:

- the cards the files hold, in the card's words,
- the mapping from each author form to the graph, and the conventions (ids from the sheet, `r1_` and `r2_` model ids, hand-numbered node ids and keys),
- the rate conversion 1 - exp(-r × Δt × n), the competing group's thresholds and the contrast file's bias,
- the check: a 20-step `henad-cli --model-file` run with one `--act <id>@10` per action, and what to read in the exported statistics.

Before the commit each of the eleven files ran that check in this worktree, a debug build at `15d8717` with the fixtures added.
All eleven read, checked, compiled, pressed every action and wrote 21 rows.
The statistics behaved as the procedure lists: Game of Life's Alive count went to 0 after Clear, the SIR counts summed to the cells at every tick with Recovered never falling, the forest fire began with no cell burning and burned within a few ticks, Replant drew the cover again, the random walk's average position stayed near the centre, the spread of opinion fell except at Mix opinions, and Reset colony cleared the ants' trails.

### Tests

| Test | What it holds |
| --- | --- |
| `every_sheet_reads` (S1a's, extended) | the nine sheets, the two starters taken from `starters()` and seven test sheets, each with a place on every line table, and none nesting deeper than half the limit |
| `starters_returns_game_of_life_and_vote` | `starters()` lists exactly Game of Life and Vote, the forest fire is not among them, each starter is a grid sheet, and the vote is named "Majority vote" |
| `the_sheets_written_from_round_1_read_their_values` | one value of each written sheet that its model depends on: the vote's 3 by 3 block, `any_state` and `always`, the forest fire's 4 neighbours, chances, keys and `1e-5`, the chance SIR's tries and New outbreak card, the walk's Repeat, and bounded confidence's "within" filter and spread measure |
| `the_sheet_reader_never_panics_on_the_mutation_corpus` (S1a's) | now runs over the nine sheets |
| `every_lowered_fixture_reads` (henad-models, new) | the two directories hold exactly the eleven files, each with `kind = "graph"` in its header by `read_header`, read by `GraphFile::parse`, and checked by `GraphFile::check` with no error |

### Docs and topic files

- `agents/henad-graph-sheet.md`: `sheet/starters.rs`, a Starters paragraph (the two starters, `Starter`, the rule that a sheet enters `sheets/` with its lowering, where each test sheet waits), and the Tests paragraph over nine sheets with how the written sheets were made.
- `agents/models.md`: the lowered fixtures, their procedure and `every_lowered_fixture_reads`.
- One CHANGELOG line under Unreleased.

Sizes after the change: `agents/henad-graph-sheet.md` 9,334 characters, `agents/models.md` 17,551.

### Edited tree

```
CHANGELOG.md                                     sheet starters
agents/
├── henad-graph-sheet.md                         starters, the nine test sheets
└── models.md                                    the lowered fixtures and their procedure
crates/
├── henad-graph/
│   ├── sheets/                                  new
│   │   ├── game_of_life.toml                    moved from src/tests/sheets/
│   │   └── vote.toml                            new: Majority vote
│   └── src/
│       ├── sheet/
│       │   ├── mod.rs                           mod starters, pub use starters::starters
│       │   └── starters.rs                      new: starters()
│       ├── starters.rs                          Starter's doc names sheets too
│       └── tests/
│           ├── sheet_document.rs                nine sheets, two new tests
│           └── sheets/
│               ├── bounded_confidence.toml      new
│               ├── forest_fire.toml             new
│               ├── random_walk.toml             new
│               └── sir_chance.toml              new
└── henad-models/
    ├── src/tests/
    │   ├── mod.rs                               lowered_fixtures
    │   └── lowered_fixtures.rs                  new: every_lowered_fixture_reads
    └── tests/fixtures/
        ├── docs/lowered_fixture.md              new: the procedure
        └── lowered/                             new
            ├── rates/                           sir_rates, sird_competing, sird_higher_wins
            └── round1/                          ants, boids, bounded_confidence, forest_fire,
                                                 game_of_life, random_walk, sir, vote
```

### Deviations from the plan

- **The record is not committed here.** The orchestration asks for the record in the design folder, numbered and committed at the merge step, so the plan's `docs: record starter sheets and fixtures [draft]` commit is left to that step.
- **The fixtures ran in the milestone's worktree, not on the main checkout.** The worktree held the main checkout's tip, `15d8717`, and the rule of the run keeps agents out of the main checkout. The run used a debug build and pressed every action at tick 10, which the plan did not ask for.
- **`Starter` is reused.** The plan names `starters()` without a type. A second type with the same two fields would have one caller, and S13a lists both kinds of starter, so the graph starters' `Starter` holds the sheets too, with its doc widened.
- **The chance SIR's sheet id is `sir_chance`**, so it never shares an id with the rates SIR, while its name stays round 1's "SIR epidemic".
- **Bounded confidence's colour rule holds no colours.** Round 1 says "Shaded by Opinion, 0 blue to 1 orange", and the author format's `shaded` rule has a `property` and a `scale` but no colour at either end. The sheet writes `rule = "shaded"` and `property = "opinion"`, and the ramp is left to the agent lowering. The lowered fixture holds the eight-step ramp round 1 used.
- **Comments.** The checker script's wording stays, apart from D81's two forms. Its plural "persons" in bounded confidence's line 1 stays too, since S4 rewrites every comment.
- **Parameters keep round 1's text.** Initial share of B and the chance SIR's Initial infected have no "means", as round 1 gives none.
- **One comment changed in a copied fixture**, the `sird_higher_wins.toml` line that named a design file. The round 1 `sir.toml` fixture keeps "Seed outbreak" as its action label: D97 renames the button in the two SIR sheets, and a fixture never reaches a researcher.
- **One test more than the plan's**, `the_sheets_written_from_round_1_read_their_values`, since `every_sheet_reads` reads a sheet without checking what it says.

## Review

The review found one should-fix and four nits, and all five were fixed in `fix: s1b review findings [draft]`.
The description counted five waiting test sheets where there are four.
`starters_returns_game_of_life_and_vote` compared each starter with itself, since the test helper `sheets()` and `starters()` both embedded the same file.
The helper now builds its starter entries from `starters()`, so the two lists cannot drift, and the test drops the comparison.
The henad-graph sheet topic file no longer names the author format's checker script, which lives outside the repository, and states only what the test sheets hold.
A trailing "which" clause in the lowered fixture procedure became two sentences.
This record no longer cites section numbers of the gate and of round 1 in its prose, since neither document is published.

## Decisions adopted for review

The rows of the plan's section 6 and the gate's section 7 that S1b builds, as written there.

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D64 | Which round 1 models ship as starters | round 1 section 14, answer 13a | All eight, forest fire and bounded confidence included, and the ants keep their slower trail with a note | S1b, S10, S17, S20 |
| D81 | "always" and "any state" in the sheet | final specification 8.3 and 8.4 (an empty "when" and an empty old state stop the line, while "always" and "any state" are choices), against author-file 2.4 (`condition` and `start_state` optional, absent meaning always and any state) and D39 (an absent slot key is a blank) | **Conflict.** The final specification's, which D39 already implies. Every condition slot holds `{ kind = "always" }` for always, and `start_state` holds the reserved id `any_state` for any state. An absent `condition` or `start_state` is a blank (D39). `any_state` joins the ids that minting refuses (S12) and that the checker refuses as a state id (S5). A When whose condition is always is a checker error, since a When cannot be always. The four author-file examples are copied with these values written in wherever a line has no condition or a Become has no `start_state` (S1a), and the sheets written from round 1's answers take them too (S1b) | S1a, S1b, S4, S5, S7, S12 |
| D97 | "Seed outbreak" | final specification 4 and 17 ("Builder") | Renamed "New outbreak", id `new_outbreak`, on the button and its Run once card in `sir.toml` and `sir_chance.toml`, since "seed" is a forbidden word | S1a, S1b |

What S1b built of each: D64's first two starters, with the forest fire, random walk and bounded confidence written and waiting as test data for S10 and S17. D81's values in all five written sheets: the vote's two Becomes from `any_state`, its line 3 and the forest fire's line 4 `always`. D97 in `sir_chance.toml`, on the button and its card.

## Benchmark commands left for the maintainer

None.
S1b adds data and one function returning static text, outside every tick.

## State after

- `redesign/s1b-starter-sheets` held two unsigned `[draft]` commits after `15d8717`, the feature and the review fix, and after the rebase it holds them after `d1838bb`.
- In the worktree, with `HENAD_REQUIRE_GPU=1`:
  - `cargo fmt --all -- --check` passes.
  - `cargo clippy -p henad-graph -p henad-models --all-targets --all-features -- -D warnings -W clippy::all` passes.
  - `cargo test -p henad-graph -p henad-models --all-targets --all-features --no-fail-fast`: henad-graph 195 passed and 1 ignored, henad-models' library 160 passed, 1 ignored and 1 failed, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`, and every integration test of henad-models passes.
  - `cargo test -p henad-graph -p henad-models --doc`, `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features` and `./scripts/check_packaging.sh` pass.
  - `cargo check --lib --target wasm32-unknown-unknown --all-features` passes for henad-graph, henad-models and the facade.
- The merge check rebased the branch onto `37-node-editor` at `d1838bb`, with S8 (record #74) merged since the branch began.
  The rebase met one conflict, in `CHANGELOG.md`'s Added list, resolved by keeping both lines, and auto-merged the module list of henad-models' `src/tests/mod.rs`.
- The full check of plan section 2.3 ran on the rebased branch at `57e6517`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models with the baseline's message (`boids.toml: 14816908000000 units in one submission`), and its log shows no other test binary that failed or aborted.
  No baseline test now passes.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.

## Issues found & future directions

- **The fixtures and the sheets draw different numbers.** The hand-lowered files keep the keys their author chose (`3100000101` and so on), and the sheets' keys were minted at random, so a lowered sheet and its fixture never draw alike. The comparisons of S14, S17 and S20 are bands over statistics, which this does not affect.
- **The fixtures' node ids do not follow the lowering's numbering.** Round 1's files number their nodes by hand, where the lowering will write `n(1000k + j)` per line. No comparison reads node ids.
- **`crate::starters::starter(file_name)` finds graph starters only.** The sheet starters reuse two of its file names, `game_of_life.toml` and `vote.toml`, so S13a keeps the two lists apart rather than looking a sheet up by file name there.
- **The shaded colour has no ramp in the sheet.** S17 decides whether `shaded` takes end colours, as round 1's "0 blue to 1 orange" reads, or keeps a fixed ramp.
- **The mutation corpus grew.** It now edits nine sheets, 3600 random edits and every key line, and the sheet tests finished in 1.9 seconds in a debug build.
- **The checker script refuses `any_state`.** It predates D81, so the vote fails its check on that alone. The Rust checker of S5 replaces it as the reference.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
