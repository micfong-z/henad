---
date: 2026-11-10
title: "Open decisions settled"
description: "Round 5 of the #37 redesign settles the nine decisions rounds 1 to 4 left open. Option A was the tiebreaker, and the session's own call went against it three times: no push, the first-time rule inside a Repeat, and benchmarks A and C together. Sixteen commits in five packages and the CHANGELOG commit `bd62a9c` re-key the grid rule streams on pcg4d, replace the failing budget test, state the float results contract, fix the Apply line and lower the first-time rule. The full check passes every step, the device tests pass on lavapipe, and the benchmarks hold the gates of rounds 1 to 3 at the tip."
icon: material/scale-balance
status: ai-generated
model: "claude-fable-5-2 (Claude Code): decisions, analysis and reviews. Implementation by Opus and Sonnet subagents"
issue: "#37"
state: "37-node-editor at bd62a9c plus two docs follow-ups and this record: seventeen implementation commits after b78947e, 20 commits in all, all unsigned [draft] commits, nothing pushed. The full check with HENAD_REQUIRE_GPU=1 at bd62a9c exits 0 on every step"
baseline_commit: b78947e
delta_state: "committed on `37-node-editor`: seventeen implementation commits by fast-forward from five branches, two docs follow-ups (`bd62a9c` and a second one after this record) and this record, unsigned, for the maintainer to reword and sign"
---

# Open decisions settled

> Rounds 1 to 4 of the #37 redesign left nine decisions with the maintainer: six standing ones from rounds 1 to 3 and three questions from the audit of record #105.
> The maintainer asked for option A on each, provisionally, with every option checked and the session's own call taking precedence.
> The session's main model made every call and analysis, and Opus and Sonnet subagents wrote the code in five packages, each reviewed before its merge.
> The call went against A three times: no push (the tests ran on lavapipe in a local container), benchmarks A and C together, and round 1's first-time rule inside a Repeat.
> Decision 1 went beyond its options: the grid rule template, whose stream repeated whole random fields, now draws from the same `pcg4d2` core as the graph draws.
> The full check passes every step for the first time since the redesign began, with 2,219 tests passing and the two float probes ignored, and `cargo deny` is clean.
> The benchmarks ran on the final tree, on a machine under load, and every median paired ratio of the stock models and the graph files lies between 0.976 and 1.016 against its earlier build.

## State before

`37-node-editor` at `b78947e` held S0 to S32, records #71 to #105, as 233 unsigned `[draft]` commits that the remote lacked.
The full check failed on two steps, as at every merge point of the redesign: `cargo deny` on RUSTSEC-2026-0370, 0371 and 0372, and the workspace test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` on `boids.toml`.
No benchmark had run since round 3, and no device test had run on a second adapter.
The artifact of round 4 listed nine decisions for the maintainer in its chapter 19, each with its options.

## What was done

### The instruction

The maintainer asked for option A provisionally on every decision with options, and for each option to be checked.
The session's own call takes precedence, and the maintainer's A is the tiebreaker where the evidence leaves two options level.
Every decision and analysis was made by the session's main model, and the implementation went to Opus and Sonnet subagents.

### The nine decisions

| # | Decision | Options | Call | Against the tiebreak | Evidence |
| --- | --- | --- | --- | --- | --- |
| 1 | Hash core of the draws | A `pcg4d2`. B `chainm`, about 5 multiplies a draw. C another core | A. Beyond the options, the grid rule template's stream is re-keyed on the same core | No, extended | Record #61: `pcg4d2` is the only core passing 14 of 14 PractRand walks to the cap, and `chainm` passed 6. The template's chain repeats whole random fields at four tick pairs (below). No shipped rule draws, and `henad-grid-rule/1` is unreleased |
| 2 | Metal fast math and the float Repeat gate | A integer bodies bit for bit plus replay on one device. B safe math through a `[patch]` of wgpu-hal. C break contraction in the emitter | A | No | The maintainer's contract of 2026-11-06: a node model reproduces itself on one device. Both float probes fail on lavapipe too. A published crate cannot carry B's `[patch]`. C costs every float node and still leaves reassociation |
| 3 | Boids walk budget and the failing test | A the refusal stands, and the test holds each file at the largest population its world admits. B lower `max_count`. C a separate rate for walks | A | No | The probes below. Stock `gpu_boids` at 1,000,000 in the default world runs 3.2 s a step, so the refusal bounds a real hang |
| 4 | Three cargo-deny advisories | A ignore entries. B leave them and run the later lines by hand | A | No | `zvariant`, `zbus_names` and `zvariant_derive` reach Henad only through `accesskit_unix` under eframe. cargo-deny reports no safe upgrade, and `deny.toml` already ignores two unmaintained advisories the same way |
| 5 | Second GPU | A push and let CI's Linux job run on lavapipe. B run by hand on a Vulkan or DX12 machine | Neither as written: no push, and the tests ran on lavapipe in a local Linux container | Yes | The 233 commits are unsigned drafts the maintainer will reword and sign, and a push publishes them. The container runs what CI's job would run |
| 6 | Benchmarks | A repeat the three gates of rounds 1 to 3. B also what never ran. C record #105's nine sets | A and C together, on the final tree, run alone | Yes | A alone leaves D48, which the plan put before counts went on, without its measurement |
| 7 | Which change wins inside a Repeat | A the any-time rule as built. B round 1's first-time rule | B | Yes | Round 1 section 6 says a change inside a Repeat "takes the first time it applies", and the card says the lines "run several times each tick". The shares below |
| 8 | A press's draws and its button's place | A keep the phase keyed on the button's place. B key it on the action id | A | No | Adding or deleting a button changes the model's code, so a resume is refused and no run compares across the edit either way. B changes the action draws of every graph model |
| 9 | A new model while another runs | A keep the split of records #55 and #65. B revisit it | A, plus a fix: the Apply line must not name Build once Build builds something else | No | Record #55 listed this as an open question and never answered it. Build and Offload already end a run with no dialog, and a dialog on Apply would make Apply stricter than Build |

### How each call was checked

**The grid rule streams (decision 1).**
The template seeded each cell with `pcg_hash(seed ^ pcg_hash(index ^ pcg_hash(tick)))`.
Where `pcg_hash(t1) ^ pcg_hash(t2) = d` falls below the cell count, cell `e` at tick `t2` starts from the state of cell `e ^ d` at tick `t1`, and the whole field of draws repeats, shifted.
A Python check for one seed found the first such pair at ticks (3, 326) at 256 by 256, (42, 119) at 1024 by 1024, (1, 24) at 4096 by 4096 and (2, 3) at 16384 by 16384, and the fields at each pair are equal.
The streams reviewer reproduced the four pairs independently.
At 256 by 256 and seed 5 the old chain shares all 65,536 start states between ticks 3 and 326, while the new seeding shares 1.007 states a tick pair on average over 300 pairs, against 1.0 by chance, and 5 at worst.

**The boids budget (decision 3).**
Probes at `b78947e` admit the boids graph at 200,000 agents and refuse it at 210,000 in the default world.
At 1,000,000 it runs in a 10000 by 10000 world at 18.5 steps a second and is refused at the default density, at 7.5e11 units.
Stock `gpu_boids` at 1,000,000 in the default world runs, at 0.316 steps a second, 3.2 s a step.
The boids and bounded confidence sheets reach their largest population in the largest world too, and their `max_count` stays, which answers record #87's question.

**The Repeat shares (decision 7).**
Under the any-time rule, "Roll is less than 0.5 gives A, otherwise B" over 4 times splits 94 to 6, where each time round splits it evenly.
"Chance 0.1, otherwise chance 0.5" gives A 0.344 under the any-time rule and 0.174 under the first-time rule.
The implementer ran the new tests against the old lowering and measured 0.937 and 0.063 for the roll, and 0.343 and 0.617 for the chances on the host, 0.342 and 0.617 on the device.
The first-time rule gives 0.17436 and 0.78463 on the host.
No shipped sheet holds two changes of one property in a Repeat, a lone change keeps its lowering, and the catalogue already has the `min` accumulator the rule needs.
The prototype never offered Repeat, so the simulated-researcher study met neither rule.

**The Apply line (decision 9).**
Record #55 listed a new model beside another model's run as an open question, and no later record answered it, so it counted as undecided.
After Apply and then Offload, the sheet's preview loads and Build builds that preview again, while the Editor still read "Model applied. Press Build to run it."
Package "apply" fixed that line.

**The second adapter (decisions 2 and 5).**
Docker on the maintainer's machine gives `llvmpipe (LLVM 19.1.7)` on Vulkan, Mesa 25.0.7 on aarch64, with Rust 1.97.1.
At `b78947e` henad-graph passed 491 tests and failed none.
henad-models passed 193 and failed 1, the budget test of decision 3, with `graph_ants_parity`, the sheet tests and the grid lockstep passing.
Both ignored float tests, henad-graph's `a_float_repeat_body_equals_its_hand_chained_copies_on_the_device` and henad-models' `a_float_repeat_over_agents_equals_its_hand_chained_copies_bit_for_bit`, fail there as on Metal, at "the device and the host": LLVM contracts the multiply and the add.
Contraction is therefore no Metal peculiarity, and decision 2's A reads the float tests as probes of a device.
The run at the final tree is under [The second adapter](#the-second-adapter).

### The packages

Each package ran in its own worktree from `b78947e`, and a reviewer with a fresh context read it before its merge.
The session reviewed package "contract" itself, and reviewers on the session's model read the other four.
A Sonnet subagent rebased the five branches onto `37-node-editor` in the order below with no conflicts, fast-forwarded each, wrote the CHANGELOG commit `bd62a9c` and ran the full check.

#### contract (Sonnet): decisions 2 and 4

Commits `bd62a9c`, `bd62a9c` and `bd62a9c`.

- `deny.toml` ignores RUSTSEC-2026-0370, 0371 and 0372, each with its crate, "unmaintained, merged into zbus 6" and the path in through `accesskit_unix` under eframe's `accesskit` feature. The implementer confirmed the path with `cargo tree -i zvariant`.
- `docs/reference/model-files.md` gains a section "Results across devices", between "Fields of a graph model" and "Sheets", where it reaches graph models and sheets alike.
- The two float probes take new ignore reasons and doc comments: a device may fuse a multiply with an add or reorder float operations. The doc comments name Metal and lavapipe, the ignore strings stay under 120 columns without them, and the Mesa and LLVM versions sit in `agents/gpu-traps.md` alone.
- `agents/henad-graph.md` and `agents/models.md` had told a reader to run the float test with `--ignored` on another backend before calling the gate met, against decision 2. Both now call the tests probes.

The session's review found one fault: the section promised that every whole-number result is equal on every device and equal to the host.
A whole number worked out from floats, such as a count of agents within a distance, can differ where the floats do.
`bd62a9c` rewords the promise to computations over whole numbers and adds that sentence.

#### budget (Opus): decision 3

Commits `bd62a9c` and `bd62a9c`.
Two tests in `crates/henad-models/src/tests/graph_agent_runs.rs` replace the failing one, over the agents corpus and the agent sheet starters.

- `every_agent_file_at_its_largest_population_in_its_largest_world_fits_the_work_budgets` sets `num_agents`, `world_width` and `world_height` to their maxima and holds every file to the submission budget.
- `every_agent_file_in_its_declared_world_admits_its_pinned_largest_population` finds the largest admitted population by bisection over `ModelEntry::shortfalls`, with no device, and checks it against a pin: 201,012 for both boids starters, 195,355 for `gpu_boids.toml`, 552,857 for the bounded confidence sheet and `max_count` for every other file. One agent past a pin must be refused by exactly the submission-budget sentence.

The implementer named files by directory in these tests alone, since boids, ants and random walk each exist as a graph and a sheet, and read the populations from the `num_agents` descriptor by id.
One sentence in `model-files.md` says a larger world admits a larger population, and `agents/models.md` and `agents/henad-compute.md` name the tests.

The reviewer found the pins equal to a hand computation and an independent bisection, admission monotone in population, and the pin test failing when `VISIT_COST` goes from 8 to 9.
Finding B1 (should fix): the first test cannot hold the host budget for the three ants files.
Each lays a ground of one cell per world unit, and in a 10000 by 10000 world the ground's init on the host costs 7.7e9, 8.3e9 and 15.3e9 units against the host budget of 4e9, at every population.
Their world sliders are refused past about 7200, 6900 and 5100 a side, while all three reach their `max_count` of 5,000,000 in their declared world.
The session accepted B1 as the budget's answer and recorded it in D117.
The test lists the three files in `GROUND_INIT_PAST_THE_HOST_BUDGET` and requires exactly that one host-init refusal for them, and it fails once the refusal goes away.
The fix round changed nothing.

#### streams (Opus): decision 1

Commits `bd62a9c`, `bd62a9c`, `bd62a9c` and `bd62a9c`.

- `the_draw_core_keeps_its_words` pins the words of henad-graph's draw core. It went in first and passed against the code before the move.
- `pcg4d` moves into henad-core as `rng::pcg4d` and its WGSL twin `henad::rng::pcg4d`, with a doc example, cases in the device parity test and a line in `docs/reference/primitives.md`. henad-graph's `draw.rs` re-exports it and `draw.wgsl` imports it, so one definition remains.
- The grid rule template seeds `rng` with `henad_pcg4d(henad_pcg4d(vec4<u32>(params.seed, henad_index, henad_tick, 0u))).x`, which equals `draw_core(seed, index, tick, 0, 0)`. `CODE_FORMAT` stays `henad-grid-rule/1`.
- `a_random_rule_draws_a_fresh_stream_each_tick` recomputes the stream from a Rust copy of the seeding. `a_rule_draws_the_words_of_the_rust_twin` compares every cell on the device over 4 ticks. `one_tick_draws_no_shifted_copy_of_another_ticks_field` checks the four tick pairs with 3 seeds and 1024 sample cells each, and asserts that the old chain repeats there and the new seeding does not. With the template put back to the old chain, both device stream tests failed.
- "provisional" leaves every tracked text, and `agents/henad-graph.md`, `agents/henad-core.md` and `agents/henad-compute.md` describe the core and the seeding.

The implementer put `pcg4d` in henad-core's public `rng` module in place of a private template helper, since two crates now use it, and left it out of the facade's prelude, which holds draws.
The shared WGSL hash `SHARED_WGSL_FNV1A64` changes with it.
The reviewer reproduced every pinned word with an independent Python `pcg4d`, and ran a rule that imports `pcg4d` and one that declares its own through henad-cli.
Finding S1 (nit): the proposed CHANGELOG wording describes the defect of a chain that no release carried.
It was noted, and the CHANGELOG's Grid rules line keeps the description.

#### apply (Opus): decision 9

Commits `bd62a9c`, `bd62a9c` and `bd62a9c`.
`EditorPanel::settle_outcome` in `crates/henad-app/src/ui/editor/mod.rs` takes the source of the model Build would build now: the selected model's source, or none while `AppState::build_replays_preview()` holds.
An `ApplyOutcome::Installed` whose source no longer matches is cleared, and the Editor then shows the preview line and "Press Apply to check and install the model.".
Apply, Build and Offload behave as before, and no dialog was added.
Four tests in `src/tests/editor.rs` cover a sheet, which failed before the fix, a graph and a grid rule, which keep the line because Build still builds the applied model, and a selection moved to another model.
`docs/guide/editor.md` describes the case with the lines the app shows.

The implementer cleared the line only where it is false, and to no outcome in place of a new sentence, since the Editor already shows the right lines for none.
The fix also covers Apply of a sheet at tick 0 followed by an edit whose preview replaces the run.
The implementer checked the fix in the running app on the built-in display, since the secondary display was not connected: Game of Life applied and paused at tick 169, then SIR applied beside it, Offload, the Build line gone, Build replaying the preview at 256 by 256 and Apply building the sheet at 1024 by 1024.

The reviewer reverted the fix in a scratch copy: the sheet and selection tests fail there, and the graph and grid rule tests pass both ways, as guards against clearing the line on every Offload.
Four findings, all fixed in `bd62a9c`: apply-1 (should fix) added the four tests to the list in `agents/henad-app-editor.md`, apply-2 renamed `built` to `next_build`, apply-3 qualified the guide's quote with "or the number of agents", and apply-4 corrected the doc of `editor_texts`.

#### repeat (Opus): decision 7

Commits `bd62a9c`, `bd62a9c`, `bd62a9c` and `bd62a9c`.

- `crates/henad-graph/src/sheet/lower/repeat.rs` gives each competing line a `u32` `min` accumulator `<line>_first`, to which each time round puts its number when the line's condition holds. A line's row holds where it held and no lower competing line first held earlier.
- Two lines compete when both are a Become or both a Set, change one property and can both apply. A Become from a state below a Become from any state records its first round only for an agent in its state, through a `cmp.eq` outside the region.
- A lone change lowers node for node as before, and `--print-lowered` of every sheet and starter matches the text from before byte for byte.
- `sheet_repeat.rs` holds the new tests over two test sheets copied from the audit's probes, on the host with 20,000 agents and on the device with 200,000, at bands of five standard errors.
- The Lines help of agent cards reads "Inside a Repeat, the first time a change applies decides, so a lower line that applies earlier wins.", pinned in the wording table.
- A Become inside a Repeat does count the agents it changed, through its transition row outside the region, which the docs had denied. A Set, Steer or Move by inside a Repeat has no count.

The implementer checked competition pair by pair with one `cmp.ge` per lower line, in place of one shared accumulator that would need a remainder node.

The reviewer probed 19 more sheets, host and device equal to five digits, and found five faults.
R1 (should fix): each competing line kept its `<line>_applies` accumulator beside `<line>_first`, so a Repeat of 9 to 16 competing changes passed the 16-accumulator limit, and the cost of a time round rose from 13 units to 15.
Fixed: a competing line keeps `<line>_first` alone and holds where it is below the largest `u32`, and `sixteen_competing_becomes_take_one_accumulator_each` fills the 16.
R2 (should fix): the public `FireLines` doc said every change inside a Repeat has no count. Fixed.
R3 (nit): the "otherwise" tooltip and notice inside a Repeat named a winner without the time round. Fixed: `sentence::otherwise_tooltip` and `otherwise_notice` take `in_repeat` and read "When both apply at the same time, line N wins.", a breaking change in henad-graph, with `CardRelations::repeat_of` made public for the app.
R4 (nit): every test held two lines. Fixed with three competing Becomes in three line id orders, against `p_i (1 - 0.28^4) / 0.72`, and a Become from a state between two from any state.
R5 (nit): `agents/henad-graph-sheet-lower.md` neared 40,000 bytes. Noted, with the Repeat paragraph trimmed to leave the file at 39,194.

### The full check

The full check of `AGENTS.md`'s commands ran at `bd62a9c` with `HENAD_REQUIRE_GPU=1` on the Apple M4 Pro, each line as its own step.
Every step exits 0: the native and wasm32 checks, the facade's two wasm32 checks, fmt, packaging, `cargo deny`, clippy, the workspace tests, the doc tests, the README program, `cargo doc`, the scripts' tests and the web build.
The workspace tests pass 2,219 and ignore 2, the float probes, and none fail.
`cargo deny` reports no advisory.
This is the first full check of the redesign with no failing step.

### The second adapter

At `bd62a9c` in the same container on lavapipe, every suite passes.
Both runs set `HENAD_REQUIRE_GPU=1`.

| Crate | Passed | Failed |
| --- | --- | --- |
| henad-graph | 518 | 0 |
| henad-models | 236 | 0 |
| henad-compute | 249 | 0 |
| henad-explore | 310 | 0 |
| henad-cli | 53 | 0 |

The two float probes fail there as on Metal, as decision 2 expects.
The container reports `llvmpipe (LLVM 19.1.7, 128 bits)`, Mesa 25.0.7, Vulkan, aarch64 and Rust 1.97.1, with 8 storage buffers per stage and a 128 MiB storage binding.
Nothing was pushed.

### Benchmarks

The benchmarks ran on the maintainer's Apple M4 Pro (14 cores, 24 GB) on Metal, release builds, from 01:38 to 02:07 on 2026-11-10, with nothing else of the session running.
The machine was not cooled or idle: the load average went from 1.0 to 4.6 while the maintainer's own apps ran.
GPU utilisation went from 81 to 99 percent, with a median of 99, over 79 samples logged every 10 s from 01:53.
The binaries were `bad936b` (before #37), `79e6d1c` (the end of round 3), the tip `bd62a9c`, and the tip with `sheet_probes` returning empty probes ("silent", record #104's patch, never committed).
Each round ran every contender of a set once in rotated order, 255 runs in all, a run being `--steps S --reps 3 --json` and its time the median rep.
A ratio below is a contender's time over the reference's time in the same round, and the table gives the median over the rounds with the lowest and highest beside it, so a ratio below 1 is faster.
Read them as the gates of rounds 1 to 3 were read: a median paired ratio with its range, not a gate result from a cooled machine.

#### Stock models, tip against `bad936b`

Five rounds per set.
Step counts came from a probe taken under load, which under-read the fast configurations.
The sets marked short ran about 0.5 s a rep, and their ranges are wide for that reason.

| Model and size | Steps | `bad936b` steps/s | Tip steps/s | Ratio | Range | Load |
| --- | --- | --- | --- | --- | --- | --- |
| `gpu_boids` 4,096 (short) | 3,072 | 5,787 | 5,783 | 0.997 | 0.983 to 1.036 | 1.0 to 4.6 |
| `gpu_boids` 100,000 | 192 | 72.1 | 72.1 | 1.001 | 0.999 to 1.011 | 1.1 to 4.6 |
| `gpu_boids` 1,000,000 in 10000 by 10000 | 128 | 26.8 | 26.8 | 0.985 | 0.882 to 1.021 | 1.0 to 4.4 |
| `gpu_ants` 4,096 (short) | 12,800 | 20,760 | 20,452 | 0.990 | 0.986 to 1.080 | 1.1 to 3.2 |
| `gpu_ants` 100,000 (short) | 8,064 | 15,763 | 15,743 | 0.999 | 0.989 to 1.016 | 1.1 to 3.4 |
| `gpu_ants` 1,000,000 in 1000 by 1000 | 3,072 | 855.0 | 845.6 | 0.999 | 0.999 to 1.011 | 1.1 to 3.5 |
| `gpu_game_of_life` 256 by 256 (short) | 24,064 | 40,763 | 40,152 | 1.008 | 0.962 to 1.024 | 1.1 to 2.7 |
| `gpu_game_of_life` 1024 by 1024 (short) | 20,992 | 42,030 | 40,391 | 1.000 | 0.945 to 1.056 | 1.1 to 2.7 |
| `gpu_game_of_life` 4096 by 4096 (short) | 12,032 | 25,936 | 26,197 | 1.002 | 0.957 to 1.025 | 1.3 to 2.9 |
| `gpu_sir` 256 by 256 (short) | 20,992 | 40,580 | 39,373 | 1.016 | 0.983 to 1.042 | 1.7 to 2.9 |
| `gpu_sir` 1024 by 1024 (short) | 4,800 | 10,135 | 10,222 | 1.000 | 0.991 to 1.031 | 1.6 to 3.1 |
| `gpu_sir` 4096 by 4096 | 1,216 | 727.1 | 720.6 | 1.009 | 0.999 to 1.016 | 1.6 to 3.1 |

Every median lies within 0.985 to 1.016 over the twelve configurations, so the gates of rounds 1 to 3 (A1, G2 and A2) hold at the tip in one comparison.

#### Graph files, tip against `79e6d1c`

Five rounds per set, measuring change J (the workgroup counters) and the other engine changes since round 3.

| File and size | Steps | `79e6d1c` steps/s | Tip steps/s | Ratio | Range | Load |
| --- | --- | --- | --- | --- | --- | --- |
| `gpu_boids.toml` 100,000 | 192 | 71.8 | 71.8 | 0.998 | 0.996 to 1.001 | 1.6 to 3.3 |
| Boids starter 100,000 | 192 | 71.7 | 72.0 | 1.000 | 0.997 to 1.002 | 1.2 to 2.9 |
| `gpu_ants.toml` 1,000,000 in 1000 by 1000 | 1,920 | 955.1 | 984.6 | 0.976 | 0.966 to 0.980 | 1.2 to 3.0 |
| Ants starter 1,000,000 in 1000 by 1000 | 1,920 | 965.8 | 984.7 | 0.983 | 0.981 to 0.984 | 1.3 to 3.2 |
| Game of Life starter 4096 by 4096 | 1,024 | 554.1 | 552.6 | 1.005 | 0.968 to 1.008 | 1.5 to 2.9 |
| SIR starter 4096 by 4096 | 1,024 | 501.1 | 501.9 | 1.002 | 0.998 to 1.006 | 1.5 to 3.2 |

#### Sheets against stock models and graph starters, at the tip

Three rounds per set.
Each row gives the median paired ratio to the stock model and to the row named in the last column.

| Set | Contender | Steps/s | Against stock | Against | Ratio there |
| --- | --- | --- | --- | --- | --- |
| Game of Life 4096 by 4096, 2,048 steps | stock `gpu_game_of_life` | 25,559 | reference | | |
| | graph starter | 571.7 | 44.703 (42.158 to 46.699) | | |
| | sheet | 541.1 | 47.516 (42.829 to 47.678) | graph starter | 1.017 (1.016 to 1.067) |
| Boids 200,000, 64 steps | stock `gpu_boids` | 18.3 | reference | | |
| | graph starter | 18.2 | 1.005 (0.976 to 1.008) | | |
| | sheet | 18.3 | 0.998 (0.983 to 1.008) | graph starter | 1.000 (0.993 to 1.007) |
| Ants 1,000,000 in 1000 by 1000, 1,920 steps | stock `gpu_ants` | 869.4 | reference | | |
| | graph starter | 1,001.6 | 0.860 (0.859 to 0.868) | | |
| | sheet | 898.1 | 0.963 (0.959 to 0.968) | graph starter | 1.115 (1.114 to 1.121) |
| | sheet with Energy, packed | 895.0 | 0.972 (0.971 to 0.972) | sheet | 1.010 (1.003 to 1.014) |
| SIR 4096 by 4096, 1,216 steps | stock `gpu_sir` | 677.4 | reference | | |
| | graph starter | 502.8 | 1.344 (1.310 to 1.459) | | |
| | sheet with rates | 1,540.2 | 0.447 (0.424 to 0.473) | graph starter | 0.324 (0.323 to 0.332) |
| | sheet with chances | 1,494.1 | 0.476 (0.423 to 0.487) | sheet with rates | 1.031 (0.998 to 1.065) |
| | SIRD sheet | 1,521.8 | 0.456 (0.425 to 0.478) | sheet with rates | 1.012 (1.003 to 1.020) |

The stock Game of Life rep took 0.08 s at 2,048 steps, much shorter than the others of its set.
The sheet and the graph starter run 45 to 47 times slower than the bit-packed stock model.
D120's figures for the sheets against their graph starters (Game of Life 1.06, boids 0.99, ants 1.12) are ratios of median times, and the paired medians above are 1.017, 1.000 and 1.115.
The ants graph starter runs faster than stock `gpu_ants`, and the ants sheet 4 percent faster than stock.
The packed Energy lane costs about 1 percent over three rounds, which D120 reads as nothing measurable on a machine under load.

The SIR figures are confounded by the trajectory and say nothing yet about cost per cell.
At the defaults the epidemic ends by about tick 130 of the 1,216.
A sheet steps a settled grid at about twice the rate of a live one, 901 steps a second over the first 64 ticks against 1,467 over all 1,216, while the stock model and the graph starter cost the same on any grid.

#### The cost of counting, silent build against counting build

Five rounds per set, in 64-step submissions.

| Sheet and size | Steps | Silent steps/s | Counting steps/s | Ratio | Range | Load |
| --- | --- | --- | --- | --- | --- | --- |
| Game of Life 4096 by 4096 | 2,048 | 557.2 | 543.4 | 1.030 | 1.010 to 1.048 | 1.5 to 3.0 |
| SIR 4096 by 4096 | 2,048 | 1,557.8 | 1,530.8 | 1.024 | 1.015 to 1.032 | 1.3 to 2.8 |
| Boids 200,000 | 128 | 17.9 | 17.9 | 1.000 | 0.981 to 1.002 | 1.4 to 2.5 |

The SIR rows step mostly a settled grid, as above.
Counting costs 3.0 and 2.4 percent on the grid sheets and nothing measurable on the boids sheet.
D124 keeps counts on for every sheet, as answer 9a chose, and decides D48 from this measurement.
A build switch for sweeps would recover the cost, and would make a sweep's shader differ from the app's.

### Edited tree

```
CHANGELOG.md                                   the round's lines under Added and Changed
deny.toml                                      RUSTSEC-2026-0370, 0371 and 0372 ignored with reasons
agents/
├── gpu-traps.md                               float probes, the results contract, lavapipe
├── henad-app-card.md                          "otherwise" inside a Repeat
├── henad-app-editor.md                        the Apply line, four new tests listed
├── henad-app-sheet.md                         Apply of a sheet beside another run
├── henad-compute.md                           grid rule seeding, agent budget tests
├── henad-core.md                              rng::pcg4d
├── henad-graph-sheet-lower.md                 the first-time rule, fire counts inside a Repeat
├── henad-graph-sheet.md                       otherwise words with in_repeat
├── henad-graph.md                             pcg4d2 settled, float probes
└── models.md                                  budget tests and pins, float probes
crates/
├── henad-app/src/
│   ├── tests/
│   │   ├── editor.rs                          four Apply line tests, next_build
│   │   └── sheet_editor.rs                    otherwise inside a Repeat
│   └── ui/editor/
│       ├── mod.rs                             settle_outcome takes next_build
│       └── sheet/card_pane.rs                 in_repeat for the otherwise words
├── henad-compute/src/gpu/
│   ├── runtime_grid/mod.rs, template.rs       rng from two pcg4d rounds
│   └── tests/
│       ├── parity.rs, parity.wgsl             pcg4d cases
│       └── runtime_grid.rs                    stream tests, no shifted field
├── henad-core/src/authoring/primitives/
│   ├── rng.rs                                 rng::pcg4d
│   └── wgsl/rng.wgsl                          henad::rng::pcg4d
├── henad-graph/src/
│   ├── draw.rs, draw.wgsl                     pcg4d from henad-core, core words pinned
│   ├── sheet/
│   │   ├── lower/card.rs                      repeat_of from the relations
│   │   ├── lower/repeat.rs                    <line>_first accumulators
│   │   ├── lower/table.rs                     FireLines doc
│   │   ├── relations.rs                       CardRelations::repeat_of
│   │   └── sentence.rs                        Lines help sentence, in_repeat
│   └── tests/
│       ├── gpu_repeat.rs                      ignore reason of the float probe
│       ├── mod.rs                             sheet_repeat
│       ├── sheet_fires.rs                     a Become inside a Repeat counts
│       ├── sheet_repeat.rs                    new: the first-time rule
│       ├── sheet_sentence.rs                  new wordings pinned
│       └── sheets/
│           ├── repeat_chances.toml            new
│           └── repeat_roll.toml               new
└── henad-models/src/tests/
    ├── graph_agent_repeat.rs                  ignore reason of the float probe
    ├── graph_agent_runs.rs                    two budget tests in place of one
    └── sheet_agents.rs                        agent_sheets() pub(super)
docs/
├── developing/agent-record/
│   └── 20261110-106-open-decisions-settled.md   this record
├── guide/editor.md                            Apply beside another run, the first-time rule, counts
└── reference/
    ├── model-files.md                         results across devices, budgets, the first-time rule
    └── primitives.md                          pcg4d
zensical.toml                                  nav entry for this record
```

## State after

- `37-node-editor` holds the seventeen implementation commits `bd62a9c` to `bd62a9c`, two docs follow-ups (`bd62a9c` and a second one after this record) and this record after `b78947e`, 20 commits in all. All are unsigned `[draft]` commits, and nothing was pushed, tagged or opened as a PR.
- The full check at `bd62a9c` with `HENAD_REQUIRE_GPU=1` exits 0 on every step, with 2,219 tests passing, 2 ignored and none failing.
- The device tests at `bd62a9c` pass on lavapipe, and the two float probes fail there as on Metal.
- The two docs follow-ups and this record change only docs and the nav, so the full check was not run again. `uv run zensical build --clean` builds the site with them.
- Decision rows D114 to D124 hold the round's calls, below.

## Issues found & future directions

- **The push and the release work** stay with the maintainer, who rewords and signs the drafts.
- **Game of Life as a node model.** As a graph or a sheet it runs 45 to 47 times slower than the bit-packed stock model at 4096 by 4096, 541 against 25,559 steps a second. The gap predates the redesign and is the largest known gap of a node model.
- **The SIR comparison** needs parameters that keep the epidemic alive before it says anything about cost per cell.
- **Walks stay under-charged.** A 64-step submission of the boids graph at 200,000 took 3.5 s against the budget's 1.7 s. Capping the steps of one submission by the budget, in place of refusing, would admit 1,000,000 boids at the declared density at the price of single steps of seconds.
- **The ants files' world slider** is refused past about 7200, 6900 and 5100 a side by the host budget for the ground's init (B1, D117).
- **The stock models' own streams**, a self-advancing state per cell seeded by `pcg_hash(seed ^ index)` and the two-level hash of their actions, were never put through PractRand.
- **The float probes** fail on both adapters tried, Metal and lavapipe.
- **The benchmarks** did not run on a cooled idle machine, and the fast configurations ran about 0.5 s a rep.
- **The app check** of package "apply" ran on the built-in display, since the secondary display was not connected.
- **The whole-number promise** and the CHANGELOG's Grid rules line: a later docs commit of this round rewords the first in `agents/gpu-traps.md` and the CHANGELOG's Docs line, and trims the second to what a release reader needs.
- **The "otherwise" tooltip** of a line outside a Repeat whose higher lines are two competing lines inside one Repeat reads "When more than one applies, the highest line wins.", which does not hold between the two inside lines.
- **Three checks by eye** left in record #105 still wait: the legend order, the Charts after Run to tick, and the Measures rows.
- **The smaller calls** of records #61 to #105 that have no options stay open.
- **Records #92 and #105** describe the any-time rule in record #92 item 7 and in record #105's findings and choices. This round replaced that rule, and neither record was changed.

## Decisions adopted for review

This round adds these rows.

| Id | Topic | Decision | Source |
| --- | --- | --- | --- |
| D114 | Hash core | `pcg4d2` is library 1's hash core. The word "provisional" leaves the tracked text | Decision 1, record #61 |
| D115 | Grid rule streams | A grid rule's `rng` starts from two `pcg4d` rounds over the seed, the cell index and the tick, in place of the three-step `pcg_hash` chain. `henad-grid-rule/1` keeps its number, since it has not shipped | Decision 1, this session's check |
| D116 | Float results | A graph or sheet replays exactly with one seed on one device. Whole-number results are equal on every device and equal the host. Float results may differ in their last places between devices and between two shapes of one computation. The two float Repeat tests stay ignored as probes | Decision 2, lavapipe run |
| D117 | Agent budgets | The submission budget refuses a population past what its world's density admits. A file's largest population must be admitted in its declared world or in the largest world its parameters allow, and the test pins the largest population each declared world admits (boids starters 201,012, `gpu_boids.toml` 195,355, the bounded confidence sheet 552,857, every other file its `max_count`). The three ants files reach their `max_count` in the declared world. In a 10000 by 10000 world their ground's init on the host passes the host budget at every population (7.7e9 to 15.3e9 units against 4e9), accepted as the budget's answer and pinned in the test. The sheets' `max_count` stays | Decision 3, probes, review finding B1 |
| D118 | cargo-deny | RUSTSEC-2026-0370, 0371 and 0372 are ignored with reasons | Decision 4 |
| D119 | Second adapter | The device tests ran on lavapipe in a local container. Nothing was pushed | Decision 5 |
| D120 | Benchmarks | Run in this session on the final tree `bd62a9c`, alone, 255 interleaved runs, load 1.0 to 4.6, GPU at a median of 99 percent. Stock models, tip against `bad936b`: every median paired ratio from 0.985 to 1.016 over twelve configurations, so the gates of rounds 1 to 3 hold at the tip. Graph files, tip against `79e6d1c`: 0.976 to 1.005 over six. Sheets against their graph starters, as ratios of median times: Game of Life 1.06, boids 0.99, ants 1.12 (the starter is faster than stock `gpu_ants`, the sheet 4 percent faster than stock). Lane packing costs nothing measurable. Game of Life as a graph or a sheet runs 45 to 47 times slower than the bit-packed stock model at 4096 squared. The SIR sheet figures are confounded by the trajectory | Decision 6 |
| D121 | Order inside a Repeat | Among the changes of one property inside one Repeat, the change that first held in the earliest time round wins, and the higher line wins a tie. Between a change inside a Repeat and one outside it, or in another Repeat, the higher line wins as before. Replaces the any-time rule of S20 | Decision 7, round 1 section 6 |
| D122 | A press's draws | Keyed on the button's place among the buttons | Decision 8 |
| D123 | Apply beside another model's run | Installs, selects and leaves the run. Build replaces it. Answers record #55's open question | Decision 9 |
| D124 | Fire counts | Counts stay on for every sheet, as answer 9a chose. Counting costs 3.0 percent on the Game of Life sheet and 2.4 percent on the SIR sheet at 4096 squared in 64-step submissions (five rounds, ranges 1.010 to 1.048 and 1.015 to 1.032), and nothing measurable on the boids sheet at 200,000. A build switch for sweeps would recover it and would make a sweep's shader differ from the app's, so it is left out | D48, decision 6 |

D116's sentence on whole numbers holds for computations over whole numbers alone, as `bd62a9c` words it in the reference.
D1 to D75 and D80 to D113 stand as built, read against their alternatives.
Record #105's eleven choices stand, except the order inside a Repeat (F2), which D121 replaces.
D48 stands, with its measurement in D124.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
