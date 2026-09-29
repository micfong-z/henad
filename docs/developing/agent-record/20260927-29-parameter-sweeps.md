---
date: 2026-09-27
title: "Parameter sweeps and model exploration"
description: A survey of how twelve engines and tools explore a model's parameter space, a plan chosen feature by feature, and its seven milestones, from headless sweeps with resume and sharding to the app's Sweep and Results tabs, several GPU runs on one device, and four search methods, followed by a researched redesign of the Sweep tab.
icon: material/tune-vertical
status: ai-generated
model: claude-opus-5-5 (Claude Code), with research, planning, implementation and review subagents on claude-opus-5-5 and one rename pass on claude-sonnet-5
issue: "#14"
state: researched, planned, all seven milestones implemented and reviewed, `./check.sh` green, existing CLI outputs identical to the baseline
baseline_commit: 5de4474
delta_state: uncommitted on `14-parameter-sweep`
---

# Parameter sweeps and model exploration

> Issue #14 asked for parameter sweeps and model exploration "like krABMaga".
> Before planning, the session surveyed krABMaga and eleven other engines and tools, plus the methodology literature on exploring stochastic models.
> The result was a catalogue of 55 features, each with a recommendation, from which the maintainer chose the scope.
> The plan adds a sixth crate, `henad-explore`, and splits the work into seven milestones: M0 moves shared code into place, M1 and M2 build headless sweeps, M3 and M4 bring them into the app, M5 runs several GPU simulations at once, and M6 adds four search methods.
> All seven milestones are done.
> M0 moved value parsing, the action schedule, CSV helpers and the batched GPU step helpers to where both the command line and a sweep runner can reach them, and every CLI output stayed byte-identical.
> M1 added the sweep itself: `henad-cli --vary ... --out DIR` or `--spec FILE` runs a factorial or zipped design with replicates on CPU lanes or on the GPU, and writes runs, series, a replicate summary and a manifest.
> M2 completed the headless side: Latin hypercube, random and table designs, stop conditions, actions inside a sweep, more reducers, a run timeout, sharding with merge, and resume.
> The files come out byte for byte the same at any lane count, for merged shards and for a resumed sweep.
> M3 gave the app a Seed field, scheduled actions and Run to tick, so a run from the command line or a sweep can be rebuilt exactly in the app.
> M4 added the Sweep and Results tabs: a sweep built, run and paused in the app, on a thread on the desktop and inside the frame in a browser, its results plotted, and any run opened live.
> M5 lets several GPU runs share one device. On the two models measured it saves only 4 to 10 percent, since a single run already steps at the device's dispatch rate.
> M6 added four search methods behind one ask/tell trait: random search, hill climbing, a genetic algorithm that re-evaluates to handle noise, and Pattern Space Exploration, which maps the regimes a model can produce.
> A last pass redesigned the Sweep tab after an audit of the first version: pinned header and footer panels, a Plan panel, and collapsible sections in task order.
> Every user-facing string written for #14 was then rewritten to the maintainer's style, and the app tour gained screenshots of the Sweep and Results tabs.

## State before

`henad-cli` could override parameters with `--set`, repeat a run with `--reps` (seeded `base + i`), replay actions with `--act` and stream a stat series with `--export-stats`.
Everything needed to run a model more than once lived inside the CLI binary: parsing a parameter value, the action schedule, and the GPU helpers that batch at most 64 steps into one command buffer.
The app had no seed control at all and always built with the model's default seed.
`scripts/bench_matrix.py` swept scaling axes from outside, one process per point, for performance only.

`henad-core` has no dependencies, and `ModelFactory` was a plain boxed closure, so a registry entry could not be shared across threads.

## What was done

### Research

Three research agents read primary documentation and, where it was thin, source code.
Their reports cover krABMaga, Mesa (3.5 and the version 4 scenarios), Agents.jl, FLAME GPU 2, NetLogo's BehaviorSpace, BehaviorSearch and nlrx, GAMA, Repast Simphony, MASON, AnyLogic, OpenMOLE, EMA Workbench and SALib, along with Optuna, pyABC and history matching.
They also covered the methodology papers of Thiele et al. (2014), Lee et al. (2015), ten Broeke et al. (2016), Lorscheid et al. (2012) and Chérel et al. (2015), and GPU batching in FLAME GPU, Madrona and ABMax.

The findings that shaped the plan:

- **krABMaga's sweep is smaller than its page suggests.**
  - `explore!` takes a cartesian product (`Exaustive`, sic) or zipped vectors (`Matched`) and writes one row per run, with the end-of-run fields plus timing.
  - There is no seed handling, no per-tick series and no sensitivity analysis.
  - The cloud mode for sweeps prints "not yet implemented", and the `bayesian_opt!` macro on the page does not exist (the real one is `bayesian_search!`).
  - Its genetic algorithm and random search are real.
- **NetLogo's two most common complaints are both avoidable.** Seeds are not recorded in the output (issue #488), and "spreadsheet" output is held in memory until the end, so an interrupt loses it. The "table" output streams.
- **FLAME GPU 2 runs several independent simulations per GPU.** Each pulls its next run from an atomic counter. The paper lists launching every simulation in one kernel as future work. Madrona and ABMax do that outside agent-based modelling, with a world id column and per-world parameters.
- **The external tools share one interface.** SALib, EMA Workbench, OpenMOLE and Optuna all need a machine-readable parameter schema, a way to run a design matrix, a documented seed input and scalar outcomes.
- **The literature gives four working rules.** Vary one factor at a time before trying global methods, use common random numbers across configurations, choose the replicate count from the stability of the coefficient of variation, and use Pattern Space Exploration to find what regimes a model can produce.

### Plan

The maintainer chose the scope from the catalogue, and the full plan is kept alongside the session.
The decisions that fix the shape of the later milestones:

- **Crates.** Pure, dependency-free pieces go in `henad-core/src/explore/`: values, factors, designs, seeds, reducers, summaries and search algorithms. Execution, TOML spec files and output go in a new crate, `henad-explore`, which sits between `henad-models` and the two front ends.
- **Runs are cursors.** A `RunCursor` advances one run by a slice of ticks.
  - The CPU executor drives cursors on lane threads. Each lane owns a rayon pool sized to its share of the cores.
  - The GPU executor takes turns across several live simulations, at most 64 steps per command buffer.
  - The web build pumps one cursor per frame.
  - All three drive the same cursor, so they cannot drift apart.
- **Seeds.** A run's seed is `mix_seed(mix_seed(root ^ salt) + rep)`, the same for replicate `r` in every configuration, and every output row records it. The benchmark path keeps `base + i`, which `benchmarks/protocol.md` fixes.
- **Output.** A directory holds `runs.csv`, `series.csv`, `summary.csv` and `manifest.json`. Rows are written in plan order at any concurrency, so two runs of one sweep compare byte for byte apart from their timing columns.
- **Chosen features.**
  - Designs: factorial, zip, CSV, Latin hypercube and uniform random, plus a "One at a time" preset in the app.
  - Run control: stop conditions, actions inside a sweep (with an action's tick as a factor), reducers, a run timeout.
  - Operations: sharding with merge, resume (which also adds replicates to an existing sweep), concurrent GPU simulations.
  - App: Sweep and Results tabs, and opening any run in the live app.
  - Search: random search, hill climbing, a genetic algorithm and Pattern Space Exploration.
- **Left out.** Sobol and Morris designs are left to SALib through the CSV design. Adaptive replicates and an ask/tell server wait for a later issue. Batching many configurations into one GPU dispatch becomes its own research issue.

### M0: moves with no change in behaviour

New files marked **+**, modified marked **~**.

```
AGENTS.md                                   ~ naming rule, six crates, henad-explore, where the moved code lives
Cargo.toml, Cargo.lock                      ~ henad-explore workspace member and dependency
check.sh, .github/workflows/ci.yml          ~ wasm typecheck includes henad-explore
docs/license.html                           ~ regenerated, lists henad-explore
docs/developing/architecture.md             ~ six crates, henad-explore in the diagram
docs/index.md                               ~ six crates
zensical.toml                               ~ nav: this record

crates/henad-core/src/
├── explore/
│   ├── mod.rs                              + module doc
│   └── value.rs                            + ValueError, parse_value, resolve_params, parse_overrides
│                                             (moved from henad-cli), format_value, check_value
├── action.rs                               ~ Scheduled, Fire, Schedule moved from henad-cli, ScheduleError,
│                                             RefusedActions, Schedule::from_entries
├── export/csv.rs                           + fmt_f64 and escape_field (from stats_csv.rs), parse_records
├── export/stats_csv.rs                     ~ StatColumns extracted, StatsWriter unchanged
├── export/mod.rs, lib.rs                   ~ modules
crates/henad-compute/src/
├── gpu/stepping.rs                         + submit_steps, wait, run_steps, run_steps_acting, run_due,
│                                             sample_stats (moved from henad-cli), native only
├── gpu/mod.rs                              ~ pub mod stepping
└── fault.rs                                ~ FaultKind::Poll, Fault::is_out_of_memory
crates/henad-models/src/registry.rs         ~ Factory and Capacity helper traits (WasmNotSend + WasmNotSync),
                                              pub register_* functions, a Send and Sync assertion
crates/henad-explore/                       + Cargo.toml, src/lib.rs, src/device.rs (acquire_headless)
crates/henad-cli/src/
├── main.rs                                 ~ calls the moved code, set_error keeps the "--set ID" context
├── actions.rs                              ~ keeps BENCH_FIRE, note_refused and one test
└── json_report.rs                          ~ Schedule from henad-core
crates/henad-app/src/ui/fault.rs            ~ FaultKind::Poll arm
```

Some changes are not straight moves:

- **`Schedule::parse` takes a model id and its action descriptors** instead of a `ModelEntry`, since `henad-core` cannot see the registry.
- **`run_due` returns the refused actions instead of printing them,** and carries a `#[must_use]` with a reason. Before the move it printed each refusal itself, so a call site that drops the result would lose the note silently.
- **`ModelFactory` and `CapacityFn` are boxed helper traits** (`Factory`, `Capacity`) bounded by `WasmNotSend + WasmNotSync`. A trait object can take auto traits only after its first trait, so the bounds cannot go on the closure type directly. Lane threads in M1 share `&ModelEntry` through them. The wasm32 typecheck still passes with and without atomics.
- **`StatColumns` carries the unescaped column name and label** beside the escaped header, so a stop condition or reducer can name a column. A bare vector or histogram label resolves to its magnitude or total column, which is what `StatValue::scalar()` returns.

Checking:

- **Baseline.** Before any edit, a script captured the debug CLI's output for `--list`, `--params` of every model, CPU and GPU `--export-stats` runs with actions, a Team Assembly export, a Game of Life `--export` and a boids `--json` benchmark. After the move every output is byte-identical, with the boids timing fields stripped.
- **Tests.** The moved tests keep their names. New tests cover the value round trip, the CSV reader, `StatColumns` and the schedule's error messages.
- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`.
- **Review.** Three rounds of review with adversarial verification confirmed five findings out of 43, and all five are fixed:
  - a fault raised by a GPU sample was reported one row earlier than before;
  - abbreviated bindings (`err`, `c`) were renamed;
  - three doc comments were reworded.

### M1: the headless sweep

New files marked **+**, modified marked **~**, relative to the end of M0.

```
CHANGELOG.md                                ~ Unreleased: parameter sweeps, and the public API of M0 and M1
AGENTS.md                                   ~ henad-explore and explore/ described, a sweep command
docs/guide/sweeps.md                        + Parameter sweeps guide, spec regions included from specs/
docs/guide/running.md                       ~ a link to the sweeps guide
docs/reference/cli.md                       ~ explore mode flags, exit codes, output files, --params --json
docs/developing/architecture.md             ~ what henad-explore holds
zensical.toml                               ~ nav: sweeps guide

crates/henad-core/src/explore/
├── factor.rs                               + FactorSpec, LevelSpec, Factor, FactorLevel, inclusive_steps
├── design.rs                               + DesignKind {Factorial, Zip}, Block, generate
├── seed.rs                                 + SeedScheme, run_seed, independent_run_seed
├── reducer.rs                              + ReducerKind {Final, Min, Max, Mean}, ReducerPlan, ReducerState
├── measure.rs                              + MeasurePlan, Sampler, SeriesBuffer
├── outcome.rs                              + PlannedRun, RunStatus, StopReason, RunOutcome
├── summary.rs                              + RunningMoments, student_t_975, SummaryAccumulator
├── spec.rs                                 + SweepSpec, BlockSpec, RunSettings, MeasureSettings, SeedSettings
├── plan.rs                                 + ModelSchema, Config, Plan, PlanError
└── fingerprint.rs                          + Fnv1a64, schema_hash, results_fingerprint, run_key, plan_hash
crates/henad-compute/src/
├── runner/mod.rs                           ~ CAN_SPAWN_THREADS
└── gpu/capacity.rs                         ~ Debug on Demand and its parts
crates/henad-explore/
├── specs/sir_sweep.toml                    + an SIR sweep with a factorial and a zipped block
└── src/
    ├── spec_file.rs                        + TOML tables, refusing unknown keys
    ├── schema.rs                           + parameters, stats and actions as JSON
    ├── probe.rs                            + ProbeReport, rebuilt at lane width when lanes are narrower
    ├── cursor.rs                           + RunCursor: one run, advanced a slice at a time
    ├── exec/mod.rs                         + ExecutionLayout, ExecutionBudget, SweepControl, Executor, reorder buffer
    ├── exec/cpu.rs                         + lanes, each with its own rayon pool
    ├── output/                             + OutputDir, OutputWriter, runs, series and summary writers, manifest
    ├── sweep.rs                            + run_sweep, and the dry run
    ├── progress.rs                         + progress events, the library never prints
    └── tests/                              + determinism, failure, gpu, support, broken test models
crates/henad-cli/
├── build.rs                                + commit stamp, as henad-app's
└── src/
    ├── explore.rs                          + flags to a spec, --spec, progress lines, JSON lines
    ├── main.rs                             ~ explore mode, --params --json, exit codes 0, 3 and 1
    └── json_report.rs                      ~ the params line
```

The run loop:

- **Slices.** A cursor advances its run a slice at a time, up to the next sampled tick. A slice starts at one step and adapts toward about 20 ms of stepping. `SweepControl` is checked between slices, so a pause or an abort lands within a slice. Each run starts its slices afresh. A slice size carried over from a light run once kept an abort waiting 106 s behind a heavy one.
- **Commit order.** Outcomes are committed in plan order through a reorder buffer, so every file is independent of which lane finished first.
- **Lanes.** One lane uses the global pool inside `rayon::scope`, as the benchmark does. Several lanes each own a pool of their share of the cores, built once per sweep. Each run is built and stepped inside that pool, so a model sizes its scratch to the lane.
- **Probe.** The layout comes from a probe build of the first config that builds cleanly. Memory for several lanes is measured on a probe rebuilt at lane width, since a scatter grid's buffers scale with the pool.
- **Failures.** A panic in a lane or in the committing thread aborts the other lanes at once. A GPU run that faults records the tick of its last sample, which does not depend on the slice size.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`. The existing CLI outputs still match the baseline.
- **Determinism.**
  - `a_sweep_writes_the_same_files_at_any_concurrency` compares all three CSVs at 1, 3 and 4 lanes.
  - `ants_results_do_not_depend_on_lane_width` compares one lane of four threads against four lanes of one.
  - `sampling_cadence_does_not_change_the_trajectory` steps every CPU model, and every GPU model that replays exactly, at a stats cadence of 1 and 10. It compares both the final row and the final state.
- **Parity with `--export-stats`.** A single-point sweep's `series.csv` equals the `--export-stats` output on CPU and on GPU.
- **Review.** Two review rounds confirmed 30 findings, and all are fixed. They include the two determinism bugs above (slice carry-over, GPU fault ticks), lanes left running after a panic, and memory projected at the wrong pool width.
- **Naming.** A further rename pass applied the naming rules the writing verifier had let through:
  - `…File` TOML mirror types became `…Table`;
  - `eta_s` became `remaining_s`;
  - `out` became `output_dir`;
  - `Fnv64` became `Fnv1a64`;
  - `sd` became `standard_deviation`;
  - "could not" became "cannot" in error strings.

  That pass also changed two older `--export` error strings in `main.rs` to "cannot create".

### M2: the rest of the headless sweep

New files marked **+**, modified marked **~**, relative to the end of M1.

```
docs/guide/sweeps.md                        ~ sampled designs, tables and SALib, stops, timeouts, actions,
                                              resume, adding replicates, shards and a Slurm array job
docs/reference/cli.md, AGENTS.md            ~ the new flags, statuses and files
CHANGELOG.md                                ~ the M2 features

crates/henad-core/src/explore/
├── design_rng.rs                           + DesignRng: unit draws, bounded indexes, permutations
├── design_csv.rs                           + design tables: columns are param ids or action.<name>
├── stop.rs                                 + Comparator, Comparison, StopSpec ("Infected <= 0"), StopCondition
├── design.rs                               ~ Random, LatinHypercube and Table designs, design seeds
├── factor.rs                               ~ action ticks as factors, FactorDomain, LevelSpec::parse (from henad-cli)
├── reducer.rs                              ~ argmax, argmin, first crossing, window mean
├── measure.rs, outcome.rs                  ~ the stop, timed_out, stop reasons condition and timeout
├── spec.rs, plan.rs, seed.rs               ~ actions, per-config schedules, shards, design_seed, plan warnings
└── fingerprint.rs                          ~ hashes cover actions, stops and design seeds, not the timeout
crates/henad-explore/
├── specs/sir_sweep.toml                    ~ the plan's full example: tick factor, Latin hypercube, stop
├── specs/sir_table.toml, sir_design.csv    + a table design
└── src/
    ├── merge.rs                            + --merge: checks the plan, interleaves by run id, rebuilds the summary
    ├── output/read.rs, output/resume.rs    + reading tables back, ResumeScan, staged table rewrites
    ├── cursor.rs                           ~ actions fired, stops, timeouts
    ├── spec_file.rs, sweep.rs, probe.rs    ~ the new keys, shards, resume, the last config probed for memory
    └── tests/                              ~ stop, actions, merge, resume, replicates, timeout, capacity
crates/henad-cli/src/explore.rs             ~ --sample, --design, --stop, --timeout, --act and action ticks,
                                              --shard, --merge, --resume, --retry-failed
```

How the new parts behave:

- **Actions.** A sweep fires each action once per run, tick 0's before the first step and later ones after the step that reaches them. That is the rule `--export-stats` follows, so a single-point sweep still matches it. A refused action is noted in the `note` column and leaves the run `ok`.
- **Stops.** A stop condition is checked at sampled ticks only. Its run ends at that sample with `stop_reason` `condition`.
- **Timeouts.** A timeout is checked between slices by wall clock and gives `timed_out`. It is the one outcome that depends on the machine. It is left out of the plan hash, and resume always reruns it.
- **Resume.** A resume matches each row of `runs.csv` to its run by config and replicate, and checks the run key against the plan. It truncates a partial last line and drops series rows past the last committed run. It rewrites both files through staged copies and a marker, so an interrupted rewrite is detected on the next start. The plan hash leaves out the replicate count, so resuming with a higher `--reps` runs only the new replicates. Lowering the count is refused.
- **Merge.** A merge checks that its inputs are distinct shards of one plan, interleaves them by run id and rebuilds the summary. It writes through the same staged path, and marks the manifest `failed` if a write fails partway.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`. The existing CLI outputs still match the baseline.
- **Byte-identical outputs.** Tests check that merged shards equal an unsharded sweep, that a resumed sweep equals a fresh one, and that raising replicates on resume equals a fresh sweep at the higher count. A Latin hypercube sweep with an action tick factor and a stop writes the same files at 1 and 3 lanes.
- **By hand.** A 12-run SIR Latin hypercube with a stop and a varied outbreak tick was sharded in two and merged back. `runs.csv`, `series.csv` and `summary.csv` equal the unsharded sweep apart from timing.
- **Review.** Two review rounds confirmed 40 findings, and all are fixed. Among them:
  - a merge interrupted partway;
  - run ids a stale shard could mislabel;
  - a resume that accepted a lower replicate count;
  - `--merge` accepting `--retry-failed`;
  - several names the naming rule rejects.

Common random numbers show up plainly in that hand run.
Replicate 0 of every config went extinct at tick 150, and replicate 1 at tick 190, across infection rates from 0.20 to 0.50.
Each config's early curve differs, but its last infected cells recover on the same draws, since every config uses the same seed for a replicate.

### M3: seeds and replay in the app

New files marked **+**, modified marked **~**, relative to the end of M2.

```
docs/guide/app.md                           ~ the Seed field, scheduled actions, Run to tick
docs/assets/app/params.png, params-reload.png, playback.png  ~ retaken
AGENTS.md, CHANGELOG.md                     ~ the app's seed, schedule and run-to

crates/henad-core/src/
├── action.rs                               ~ Schedule: Default, is_empty, next_due_after
└── explore/replay.rs, plan.rs              + Replay, ~ Plan::replay and Plan::model
crates/henad-compute/src/
├── cpu/sim_thread.rs                       ~ SetSchedule and RunTo, the schedule fired after each step
├── gpu/sim_thread.rs                       ~ the same, batches split at due ticks and at the target
└── runner/mod.rs                           ~ RUN_TO_PUBLISH_INTERVAL
crates/henad-app/src/
├── state.rs                                ~ seed, loaded seed, schedule, run-to target, open_run, focus_tab
├── lib.rs                                  ~ focus_tab applied after the dock, a Run that overshot rebuilds
└── ui/
    ├── params.rs                           ~ the Seed row and the scheduled-actions list
    ├── playback.rs                         ~ Run to tick, its progress and Cancel, the opened-run line
    ├── dock.rs                             ~ Pacing split at 0.5 so Build stays in view
    └── export/metadata.rs                  ~ seed and scheduled_actions
crates/henad-explore/src/tests/replay.rs    + a replayed sweep run, and a GPU schedule, against their CLI output
```

How it behaves:

- **Seed field.** An empty field means **Default**, the model's own seed, and a number means that seed. The two differ, since an engine uses its constant for `None` and `mix_seed(n)` for a number. A pending seed counts in the Reload needed notice, and the dice fills in a fresh one.
- **Schedule.** A build sends the scheduled actions with `SetSchedule`. The sim thread fires each after the step that reaches its tick, the rule `--export-stats` and a sweep follow.
- **Run to tick** steps uncapped, never past the target, then pauses. On the GPU the batch is split at the target and at every due action, and the stats at the target come from a blocking snapshot. Play, Pause and Step cancel it.
- **Replay.** `Plan::replay` gives a sweep run's model, parameters, seed and schedule. `AppState::open_run` rebuilds it, and M4 opens it from the Results tab.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`.
- **Tests.** The new tests cover run-to, the schedule rule on both sim threads, and a sweep run replayed through `SimThread` against its row. Each test was broken on purpose to confirm it can fail.
- **Live app.** An agent drove the release app through the egui MCP and compared the Statistics panel with `henad-cli --export-stats` at several ticks. The cases were SIR at seed 42, SIR at **Default**, SIR with a scheduled outbreak at tick 3, and the GPU SIR at seed 42 with and without it. Every tick compared was equal.
- **Review.** Two review rounds confirmed 20 findings, and all are fixed. Among them:
  - Build clipped off the Playback panel;
  - Run stepping the loaded model while another was selected;
  - a network layout that stopped relaxing after a run-to;
  - a Run pressed while playing that could stop past its target.

### M4: the Sweep and Results tabs

New files marked **+**, modified marked **~**, removed marked **-**, relative to the end of M3.

```
.gitignore                                  ~ the benchmark rule no longer hides ui/results/
Cargo.toml, docs/license.html               ~ egui_extras for the runs table
docs/guide/app.md, sweeps.md                ~ the Sweep and Results tabs, the browser's limits, --open
AGENTS.md, CHANGELOG.md                     ~ the new tabs and the explore handle

crates/henad-explore/src/
├── handle.rs                               + SweepRun, SweepEvent, SweepProgress: one API on both targets
├── pumped.rs                               + PumpedSweep, a runner::SimLoop that steps one CPU run per frame
├── result_set.rs                           + ResultSet: a henad-cli --out folder read back, with a series budget
└── output/memory.rs                        + SweepOutput::Memory, the same writers over bytes
crates/henad-app/src/
├── main.rs                                 ~ --open DIR
├── state.rs, lib.rs                        ~ the sweep session, the results panel, focus_tab, repaint while a sweep runs
└── ui/
    ├── files/                              + save (moved from export/), open: a folder on native, picked files on the web
    ├── sweep/                              + draft, builder, session, progress
    ├── results/                            + store, plot, series, response, heatmap, table
    ├── dock.rs                             ~ Sweep and Results stacked behind Viewport
    ├── params.rs                           ~ an f32 slider that no longer rewrites its value
    ├── export/save.rs                      - moved to files/
    └── playback.rs, model.rs, fault.rs     ~ small follow-ups
crates/henad-compute/src/*/sim_thread.rs    ~ test harness types named as nouns (M3 cleanup)
```

How it behaves:

- **Sweep tab.** The builder takes the model selected in the Parameters tab. Each parameter's levels are written in the same syntax as `--vary`, parsed by `LevelSpec::parse`.
  - Designs: every combination, zip, One at a time, Latin hypercube, uniform random, or a design CSV. One at a time writes one block per varied parameter, with the rest at their Parameters values.
  - Other settings: replicates, root seed, common random numbers, steps, warm-up, sampling, a stop, extra outputs, concurrency and an output folder.
  - Starting a sweep pauses the live simulation. The progress view offers Pause, Resume and Abort, and Save spec and Load spec write and read the TOML.
- **Where a sweep runs.** On the desktop, `SweepRun` runs the existing sweep on a thread, under the same `SweepControl` the CLI's lanes check. In a browser, `PumpedSweep` steps one CPU run at a time inside the frame budget and keeps its results in memory. A GPU model is refused there.
- **Results tab.** Four views:
  - Series: replicate bands per config;
  - Response: an output against one varied parameter, with 95% confidence whiskers;
  - Heatmap: two parameters;
  - Runs: a sortable table.

  A run's Open rebuilds it at tick 0, and Open at end runs it to the tick it recorded, through `AppState::open_run`. Copy command gives the equivalent `henad-cli` line. Open results reads a folder written by `henad-cli --out`, and `henad-app --open DIR` does the same at start.
- **The slider fix.** The live check found an older app bug. egui's slider clamps and snaps its value on every draw, and a step of `0.01_f32` rewrote 0.05 as 0.049999997. Opening any run therefore marked it modified at once and sent a parameter one ULP off the recorded one in the middle of the replay. The f32 slider now reads and writes through `Slider::from_get_set` with clamping on edits only, and a step read as its decimal value. Drawing no longer changes a value, and stepping lands on 0.05 exactly.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`. The CLI outputs still match the baseline.
- **Live app on the desktop.** An agent drove the release app through the egui MCP:
  - it built and ran a 15-run SIR sweep into a folder;
  - it paused a long sweep, confirmed the count held still, resumed it and aborted it;
  - it sorted the Runs table and opened the top run at its end, and the Statistics panel equalled the row;
  - it opened a 25-run folder written by `henad-cli --out` with `--open`.

  The screen was locked for the whole session, so no plot was seen. The checks went through the accessibility tree and the CSV files.
- **Live app in the browser.** A small CPU sweep ran in the web build in the built-in browser, and the page stayed responsive.
- **Review.** Two review rounds sent 63 findings to the fixers. They include the slider bug, plurals, view caches rebuilt every frame while results stream in, a narrow Response panel overflowing, and more naming.

### M5: several GPU runs on one device

New files marked **+**, modified marked **~**, relative to the end of M4.

```
docs/guide/sweeps.md, docs/reference/cli.md ~ GPU tracks, --concurrent for a GPU model, --gpu-memory
AGENTS.md, CHANGELOG.md                     ~ the interleaver, a GPU trap entry, the API changes

crates/henad-compute/src/gpu/
├── primitives/readback.rs, reduce.rs       ~ StatsPoll { Pending, Landed, Failed } from every poll
├── grid_engine.rs, agent_engine.rs         ~ encode_stats_passes: the stats passes without the display pass
├── stepping.rs                             ~ submit_slice, await_submission
├── mod.rs                                  ~ GpuContext::is_lost, from a device-lost callback
└── sim_thread.rs                           ~ ignores the new poll result, as before
crates/henad-explore/src/
├── exec/gpu.rs                             + the interleaver: GpuTrack, admission, backoff, fault handling
└── cursor.rs, exec/mod.rs, sweep.rs        ~ GPU runs moved onto tracks, the layout's GPU rule
crates/henad-cli/src/main.rs, explore.rs    ~ --gpu-memory
```

How it behaves:

- **Tracks.** The interleaver keeps up to N GPU states alive and visits them in turn on one thread, since error scopes are per thread. Each command buffer holds at most 64 steps of one state, and never steps of two. An action goes in a submission of its own. A slice is cut at every action tick and sample tick.
- **Readback.** A sample submits the stats passes with its slice and starts the readback. The track keeps encoding its next interval while that readback is in flight, and never encodes a second sample until the first lands (a copy encoded over a pending map would be skipped silently). The waits go to the oldest submission in flight, never to the whole device.
- **Admission.** A run is built only when the summed demand of the live tracks and its own fits the budget, `--gpu-memory` or else the device's largest buffer. An out-of-memory build beside live tracks goes back to the head of the queue. The track cap drops by one, and nothing more is built until a track finishes.
- **Faults.** A fault inside a slice's error scopes fails that track only. A fault outside every scope fails every live track, since it cannot be traced to one. A lost device ends the GPU part, with the manifest `incomplete` for a later `--resume`.
- **Tracks by default.** Auto mode uses `min(4, budget / demand)` tracks, and one when the probed population reaches 2^20. Commits stay in plan order, so the files match at any track count.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`. The CLI outputs still match the baseline.
- **Tests.** Interleaved runs match sequential ones on `gpu_sir` and `gpu_game_of_life`, including a stop and an action tick factor. Other tests cover:
  - a pipelined sample against a blocking one;
  - every GPU model on 8 tracks with two slices in flight each;
  - admission within a budget;
  - a fault in one track leaving the others running;
  - a stop discarding the steps encoded past it;
  - an untraced fault failing every live run.
- **Review.** Two review rounds sent 44 findings to the fixers. The GPU ones: an out-of-memory build retried in the very next round, and an untraced fault that left runs already stopping reported as `ok`.

Measured on an Apple M4 Pro, release build, whole-sweep wall time including device creation, builds, sampling and file writes.
The two settings were interleaved, with 5 repetitions each.

| model | grid | runs x steps | 1 track (s) | 4 tracks (s) | ratio |
|---|---|---|---|---|---|
| `gpu_sir` | 64 x 64 | 32 x 1000 | 0.50 | 0.47 | 1.06 |
| `gpu_sir` | 512 x 512 | 32 x 1000 | 1.05 | 0.97 | 1.08 |
| `gpu_game_of_life` | 64 x 64 | 32 x 1000 | 0.49 | 0.46 | 1.07 |
| `gpu_game_of_life` | 512 x 512 | 32 x 1000 | 0.51 | 0.49 | 1.04 |

The gain is small and consistent.
One run at one track already steps at about the flat benchmark's rate: about 12 µs a step at both grid sizes, so the cost is dispatch, not the grid.
Interleaving separate states does not reduce the number of dispatches.
Batching many configurations into one dispatch (F5 in the catalogue) is the change that would.
The sweeps sampled sparsely (every 100 ticks, no series), so a sweep that samples often, where pipelined readback matters more, is not measured here.

### M6: search

New files marked **+**, modified marked **~**, relative to the end of M5.

```
docs/guide/search.md                        + Searching a model
docs/guide/app.md, sweeps.md, cli.md        ~ Search mode, the search views, the [search] table
AGENTS.md, CHANGELOG.md, zensical.toml      ~ the search modules, nav for the new page

crates/henad-core/src/explore/search/
├── mod.rs                                  + Searcher (ask, tell, is_done, report), Candidate, CandidateOrigin,
│                                             Evaluation, Objective, SearchSpec, the shared candidate bookkeeping
├── genome.rs                               + SearchSpace, Genome, decoding, triangular mutation, crossover,
│                                             ConfigKey for duplicates
├── evaluation_log.rs                       + every candidate's replicate values, aggregated
├── random.rs, hill_climb.rs                + RandomSearch, HillClimb
├── genetic.rs, pse.rs                      + GeneticAlgorithm, PatternSpaceExploration
└── tests/                                  + a driver over pure objectives, tests across all four
crates/henad-explore/
├── specs/sir_search_genetic.toml, sir_search_pse.toml  + example searches
└── src/search_run.rs                       + run_search: ask, run a batch, tell; tables; resume; app events
crates/henad-app/src/ui/
├── sweep/search.rs                         + Search mode in the Sweep tab
└── results/search.rs                       + best so far, generations, the Pattern Space Exploration grid
```

How it behaves:

- **The ask/tell loop.** A searcher is pure and deterministic given its seed and what it is told. The driver asks for a batch, runs each candidate's replicates on the same executor a sweep uses (CPU lanes or GPU tracks), and tells the evaluations back in candidate-id order, so a search follows the same trajectory at any concurrency. Every candidate counts against `max_evaluations`, re-evaluations included. A failed replicate counts as the worst value.
- **Genomes.** A genome is a point in the unit cube, one coordinate per factor. A float decodes linearly, and a whole number, an action tick or a choice by index. Mutation is a triangular step reflected at the bounds, and nothing calls libm, so a trajectory reproduces on another machine.
- **The four methods.**
  - Random search draws batches and keeps the best.
  - Hill climbing moves to a better neighbour and restarts after a patience of batches without one.
  - The genetic algorithm uses tournament selection, uniform crossover, per-gene mutation and elites. Each generation it re-asks a share of its members with fresh replicates, and ranks by the median over every replicate a member has.
  - Pattern Space Exploration keeps an archive over a grid of two outputs and breeds from the cells hit least.
- **Duplicates.** The genetic algorithm and hill climbing redraw a candidate whose configuration is already known, since under common seeds it would only rerun identical runs.
- **Output.** A search writes `evaluations.csv`, `batches.csv`, `best.csv` or `archive.csv` (and `generations.csv` for the genetic algorithm) beside the sweep files. `config_id` in `runs.csv` is the candidate id. A resumed search replays its recorded batches through the searcher and refuses a directory whose runs no longer match, before it writes anything.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`. The CLI outputs still match the baseline.
- **Tests.**
  - Unit tests: decoding within bounds, reproducible random search, hill climbing reaching a known peak, the genetic algorithm improving on a noisy objective, re-evaluations taking fresh replicate offsets, and Pattern Space Exploration filling more cells than random search.
  - End to end: identical tables at any concurrency, a resumed search following the same trajectory, and a GPU search.
  - The statistical tests were checked across 12 to 15 seeds, so they pass on more than the one committed.
- **Live app.** In the app, random search's best so far never got worse, the genetic algorithm's generation view filled in, and the Pattern Space Exploration grid filled and opened a cell's run. The screen was locked again, so these were read from the accessibility tree and the output files.
- **Review.** Two review rounds sent 70 findings to the fixers. The search ones:
  - duplicate candidates rerunning identical seeds;
  - a resume that could cut tables short before it found a changed trajectory;
  - several labels and defaults the live check showed were confusing.

The genetic example maximizes `Infected:argmax`, the tick of the epidemic's peak.
The design's objective, minimizing `Infected:max`, only drives the infection rate to its lower bound.
The peak tick is noisy enough that re-evaluation visibly corrects a lucky best.

### Wrap-up

A last pass fixed four problems the M6 live check found in the app:
- **Refused Start.** It showed its reason at the top of a scrolled builder. The refusal now sits under Start and scrolls into view. A folder that already holds results is flagged, naming whether it holds a sweep or a search, before Start is pressed.
- **Initial samples.** A Pattern Space Exploration whose initial samples cover every evaluation shows a warning above Start. It was already in place.
- **Precision.** The best objective shows six significant digits, so 3108.5 no longer reads as 3,109. This was already in place too.
- **Short durations.** These read in milliseconds instead of "0.0 s".

It also fixed two slips the audit found: a closure parameter named `op`, and "centre" in a test name and an assertion string.
`./check.sh` passes after it, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`. The CLI outputs still match the baseline, and the docs build without issues.

### The Sweep tab, redesigned

The first Sweep tab was one flat column of nine sections, left-aligned in a leaf about 845 points wide.
An audit on the running app found five high-severity problems:

- Start and the run count sat at the end of a long scroll, below the fold for most models.
- Nothing grouped essentials apart from advanced settings.
- Errors showed before anything was typed.
- Rows jumped as issue lines came and went.
- Controls were clipped at narrow widths.

The redesign drew on the experiment setup screens of NetLogo BehaviorSpace, AnyLogic, GAMA, Repast, MASON, OpenMOLE, Mesa, Weights & Biases, Optuna and Ray Tune, and on Apple, GNOME, Fluent and Nielsen Norman Group guidance for dense forms.
Most tools keep the start action in view and set the parameters being varied apart from the fixed ones, and the redesign follows both.
Two independent proposals were merged into one spec.

```
crates/henad-app/src/ui/
├── dock.rs                                 ~ the Sweep tab owns its scrolling, its title carries the sweep's state
└── sweep/
    ├── mod.rs                              ~ header, footer, Plan and central panels, width breakpoints
    ├── header.rs                           + the Sweep | Search switch, the model, Plan, the Spec menu
    ├── footer.rs                           + the run count, problems, destination and one trailing action
    ├── plan.rs                             + PlanSummary: configurations, runs, varied and held values, problems
    ├── layout.rs                           + FormLayout (one label column), section headers, reserved note lines
    ├── parameters.rs                       + the parameter rows, the values editor, the options menu
    ├── builder.rs, search.rs, progress.rs  ~ sections in task order, Search mode, the progress view
    └── draft.rs, session.rs                ~ issue kinds and a one-pass check, the draft kept for Edit sweep
docs/guide/app.md, AGENTS.md, CHANGELOG.md  ~ the rebuilt tab
```

How it behaves:

- **Panels.** The tab is split into panels:
  - a pinned header: the Sweep | Search switch, the model, the Plan toggle and a Spec menu;
  - a pinned footer: the run count, the problems and the destination;
  - a resizable Plan panel on the right;
  - the form in the middle.

  The footer's trailing button is always the next step: Start, then Pause or Resume, then Show results. Abort never takes that spot, so a late second click on Start lands on Pause. Cmd+Enter presses Start. Below 744 points the Plan panel folds into the form as its last section, and below 400 the form stacks each label over its field.
- **Sections.** Design or Search comes first, since the design decides what the level text means. Parameters, Actions, Replicates and seeds, Run length, Outputs and Execution follow. Each is collapsible, and its header carries a one-line summary and a problem count, so a collapsed section still shows its problems. Every label shares one measured column. The consequence of a choice shows beside it, such as "Infection Rate (5) × Recovery Rate (2) = 10 configurations".
- **Problems.** Input still to fill in (a pencil) is kept apart from input that is wrong (an alert). A typed error shows once the field loses focus or after 0.8 s idle, and every problem the draft can find is collected in one pass. Each row reserves its note line, so nothing below it moves. A problem in the footer or the Plan panel opens its section and focuses the field.
- **Results.** In memory or In a folder is a radio. A folder that already holds results is refused before Start is pressed.
- **Progress view.** It keeps the same frame and shows a Status table, the runs in progress with their parameter values, and the plan of the running sweep.

Checking:

- **CI.** `./check.sh` passes, and so do the GPU tests with `HENAD_REQUIRE_GPU=1`.
- **Tuning.** A tuning pass ran the release app through the egui MCP and took about 70 screenshots. It covered 1512, 1400, 1000, 800, 740 and 640 points wide, SIR, Ants and Virus on a Network, every design and search method, running, paused, ended and aborted sweeps, both modals and a GPU model. It fixed what the screenshots showed: descriptions cut off, sub-rows not indented when stacked, an output row pushing its Remove button out of the cell, and empty note lines under the seed fields.
- **Review.** A review of the rebuilt tab sent 16 findings to the fixers. One was a backwards Mean over window that validation missed.
- **Baseline.** The baseline comparison folder was lost from the session's temporary space during this pass. It was rebuilt from commit 5de4474, before any #14 work, and every CLI output of the baseline set still matches it byte for byte, the benchmark's timing fields aside.

### UI text and the app tour

Every user-facing string written for #14 went into one inventory: 1,116 entries covering app labels, tooltips and messages, CLI help and output, and the library errors a host shows.
The maintainer edited eleven by hand and named three faults running through the rest:
- tenses that avoided "will" where the future reads naturally;
- widgets used as the subject of a sentence ("Show results opens them");
- home-made phrasing where software has a conventional word ("whole number", "fill in", "stay at their values").

`AGENTS.md`'s UI rules were rewritten to match, with before-and-after examples, and 320 further strings were rewritten against them and the maintainer's edits.
A single pass then settled the wording:
- "configuration" in app prose;
- **select** in place of pick and choose;
- **Press** followed by the button's label;
- "unavailable in a browser";
- "integer";
- "will appear".

Vary each alone became **One at a time**, and the Plan panel's Held list became **Fixed**.
The docs that quote these strings were brought up to date, and the prose moved from "whole number" to "integer".

Eight screenshots were taken from the native app through the egui MCP, at twice the pixel density and cropped to the centre dock leaf, as the app tour's first screenshots were.
They show:
- the Sweep tab with a sweep set up (`sweep.png`);
- Search mode (`sweep-search.png`);
- a running sweep (`sweep-running.png`);
- the Results tab's Series, Response, Heatmap, Runs and Search views, from a 4 by 4 sweep of SIR and a 300-evaluation Pattern Space Exploration.

`docs/guide/app.md` places each in the section it shows.

### Square corners and the MCS palette

The maintainer asked for no rounded corners anywhere, chips and progress bars included.
`init.rs` already zeroed every widget, window and menu radius, and the rest went through three helpers in `ui/mod.rs`:
- `add_progress_bar` draws a progress bar with square ends and a fill colour;
- `show_plot` paints a square plot background, which `egui_plot` would otherwise round;
- `kv_grid` returns a `KvGrid` that paints square stripes.

Dock tabs have square corners too (`lib.rs`), and the tab bar no longer shows a scroll bar when it overflows.

The maintainer then supplied the Micfong Colour System (MCS), seven hues in eleven steps, which they have used across their work for about five years.
`ui/mcs.rs` holds all 77 entries as constants, and every colour chosen for a badge, a fill or a coloured text now comes from it.
The theme's neutral surfaces and strokes predate the palette and stay as they were.
The rule maps a state to a hue:
- blue for running and for the primary action;
- orange for paused, ended early and warnings;
- green for finished;
- red for failed and invalid;
- gray for neutral.

A badge takes the 900 step as its fill and the 200 step as its text, and a solid fill takes the 700 step with gray 50 text.
The theme's accents in `init.rs` already came from the palette and now name its constants.
Data colours (the series palette, the heatmap scale and the no-data gray) are chosen for data and stay outside it.
`AGENTS.md` records the rule, and fourteen app tour screenshots were retaken to show the new colours.

```
crates/henad-app/src/
├── init.rs                    ~ theme accents named by their MCS constants
├── lib.rs                     ~ square dock tabs, no overflow scroll bar
└── ui/
    ├── mcs.rs                 + the MCS constants
    ├── mod.rs                 ~ add_progress_bar, show_plot, KvGrid
    ├── sweep/, results/       ~ badges, buttons and bars in MCS colours
    ├── playback.rs            ~ the Run to tick progress bar
    └── charts.rs, stats.rs, … ~ square plots and striped grids through the helpers
docs/assets/app/*.png          ~ 14 screenshots retaken
.gitignore                     ~ /results anchored
AGENTS.md                      ~ square corners, the MCS rule
```

### PR #47 review

An agent audited PR #47 and reported 128 findings: 5 major, 81 minor and 42 nits.
The maintainer decided two questions it raised.
A GPU run's timeout counts the run's share of the device.
Before, it counted the whole time the run was on a track, the other tracks' turns included.
A spec file's memory budgets apply to a sweep started from the app, and the Plan shows them.

The fixes ran as 13 groups, each in a worktree of its own: the search methods, the rest of `henad-core`, `henad-compute` with the app's loose ends, `henad-explore`'s output, execution, results, pumped sweeps and tests, and the app's sweep draft, sweep form, sweep device, results store and result views.
Each group was verified once and repaired once, then merged.

Modified files marked **~**, relative to the end of the colour pass.

```
AGENTS.md, docs/guide/, docs/reference/     ~ the behaviour below
crates/henad-core/src/
├── explore/search/pse.rs                   ~ initial samples in batches of their own, batch_count, NaN outside
├── explore/search/hill_climb.rs            ~ starting points drawn through draw_config
├── explore/search/genetic.rs               ~ MAX_POPULATION, MAX_TOURNAMENT_SIZE
├── explore/search/genome.rs                ~ a repeated listed level kept once
├── explore/measure.rs, stop.rs, reducer.rs ~ a threshold that is not finite refused at plan time
├── explore/design.rs, factor.rs, value.rs  ~ DesignError::NoLevels, trimmed levels, an option's name before its index
└── helpers.rs                              ~ choice_param refuses an option named like an index or named twice
crates/henad-compute/src/
├── cpu/sim_thread.rs, gpu/sim_thread.rs    ~ fired_through: no tick fires twice
├── gpu/stepping.rs                         ~ sample_stats collects a readback still in flight
└── gpu/primitives/readback.rs              ~ a map given up unmaps its staging buffer
crates/henad-explore/src/
├── handle.rs                               ~ BoundModel: a GPU sweep on a device of its own, gpu_memory,
│                                             SearchBatchTold carries an Arc
├── device.rs                               ~ the WebGPU baseline, DeviceError::BelowBaseline
├── exec/gpu.rs                             ~ each round split evenly between the live tracks
├── probe.rs, pumped.rs                     ~ PlanProbe, one probe build per pump, a pause holding the runs alone
├── output/summary_csv.rs, read.rs          ~ the summary streamed from runs.csv, RecordScan
├── output/mod.rs, search_tables.rs         ~ staged files refused, search columns found by position
├── result_set.rs, spec_file.rs             ~ run keys in replays, series headers checked, TablePath
└── tests/support.rs                        ~ OutputTables (was Tables) compares bytes, CommitLimit, ticks_seen
crates/henad-app/src/ui/
├── sweep/mod.rs, session.rs                ~ ColumnsBuild, SessionExecution, no fault pause
├── sweep/draft.rs, builder.rs, plan.rs     ~ stat columns, TickSource, Save spec refusals, the budget rows
├── sweep/parameters.rs                     ~ lists without spaces, a values field per model and parameter
├── results/store.rs                        ~ batches extend the axes in place, SeriesBand (was Band), run keys
├── results/plot.rs, response.rs            ~ HeatmapTiles, Whiskers
└── results/mod.rs, search.rs, table.rs     ~ Reading results, the generation messages, Selected candidates
crates/henad-cli/src/main.rs, explore.rs    ~ --list and --info beside a sweep, the names of a repeated --act
```

The decisions, those two included:

- **GPU timeout.** Each interleaver round is split evenly between the live tracks, and a run's `--timeout`, `wall_ms` and `steps_per_s` read its share.
- **Loaded budgets.** A spec file's `[execution] memory` and `gpu_memory` are kept, written back by Save spec, and passed to `SweepRunOptions` through `SessionExecution`. The Plan lists them as Memory budget and GPU memory budget. The Execution section shows a read-only Memory budget row, noted "From loaded spec file", with a button that clears both.
- **Pattern Space Exploration.** The initial samples fill batches of their own. With `initial_samples = 0` the first candidate is alone in its batch, and after the random phase an ask on an empty archive draws a whole random batch.
- **Timeout field.** Its minimum of 1 s only limits editing. A loaded spec with a shorter timeout, 0 included, is kept and not refused, since `henad-cli` accepts it.
- **Stat columns.** The Sweep tab learns a model's stat columns from one build at its default values, on a thread of its own. A GPU model builds there on a headless device of its own, never on the app's device.
- **Replays.** `ResultSet::replay` and the app's store compare a run's `run_key` only while the model's schema matches the one the sweep ran with. After a model change a replay opens under the store's existing warning.
- **Headless devices.** `acquire_headless` starts from the WebGPU baseline, and an adapter below it gets `DeviceError::BelowBaseline`.
- **The results store (A-02).** A search batch extends the store's configs and axes in place. A continuous axis still costs time linear in its levels per batch, measured at about 0.5 ms per batch on average up to 100,000 unique values in a release build.
- **The Latin hypercube (C-08).** A stratum of a discrete factor takes the lowest level it covers. The lattice stays, since it balances the levels exactly.
- **This record and the CHANGELOG.** The human section of this record stays removed, and `CHANGELOG.md` keeps unreleased features at the level of detail the maintainer trimmed it to.

Among the other fixes:

- A GPU sweep started from the app acquires a device of its own, so a fault in the live model or the renderer no longer pauses it, and a fault in the sweep no longer offloads the live model. The Paused after GPU error state is gone.
- A second `SetSchedule` no longer fires the actions of the tick the loop sits at.
- An untraced GPU fault no longer rewrites a run that ended earlier in the same round as `gpu_error` (E-04).
- A stray quote in a table fails its record, where it used to hide every record after it.
- A directory holding a staged table or the `tables.staged` marker counts as holding results.
- A search table's trailing columns are found by position, and a parameter named like one of them no longer shadows it.
- A browser sweep makes one probe build per pump, and a sweep or search paused after its last run still ends.
- Save spec refuses what Load spec would refuse, and the Objective and axis lists offer a vector's parts.
- `--list` refuses the sweep flags, `--info` no longer drops a sweep, and a repeated `--act` takes the first free name.

A second review read the merged diff by area, with a verifier on each finding.
It confirmed 18 of 20 findings, 2 minor and 16 nits, and all 18 are fixed:

- Start stays disabled with **Reading model outputs** in the Outputs section until the model's stat-column build reports. After a failed build, a watched part column is accepted for the sweep's own probe to check.
- A timeout that `Duration` cannot hold, such as a loaded `timeout_s = inf`, is an issue on the timeout row.
- An action's tick row follows its tick source under a design table.
- A frame's search batches reach the results store in one pass.
- A CSV error from `runs.csv` or a search table gives its line in the file.
- The rest were wording and small checks: the `--act` help, the timeout docs, a table path that starts with `./`, a choice option with spaces at an end, and the genetic bounds the app now takes from the core.

Skipped findings:

- D-03 and X-09 asked for the human section of this record. The maintainer removed it on purpose.
- A-25 and A-26 needed no change.
- A-28 came from a misreading of egui's `Sides`.
- C-08 is kept by design, as above.

Checking:

- **Summary bytes.** The streamed rebuild of `summary.csv` was checked byte for byte against the original implementation with a temporary differential test. Tests of the new code against itself could not show that the bytes stayed the same.
- **Result views.** A live check ran the release app on the maintainer's second monitor through the egui MCP. The Heatmap's new tiles, the Response view's whiskers (hidden with their line from the legend), the Pattern Space grid's grey empty cells, the vector part columns of GPU boids and a GPU sweep on its own device all behaved as intended. The maintainer chose to keep the grey empty cells.
- **Screenshots.** `sweep.png` was retaken for the new Parameters hint, at a window 18 points taller so the Execution section still fits, and `results-search.png` for the grey empty cells. The tour's search gave the same 76 filled cells as before, since its 64 initial samples fill whole batches.

## State after

All seven milestones are complete and uncommitted.
`henad-cli <model> --vary ... --out DIR` and `--spec FILE` run a sweep.
`--dry-run` prints the plan, the layout and the projected memory without running anything.
`--sample`, `--design`, `--stop`, `--timeout`, `--act`, `--shard`, `--merge`, `--resume` and `--retry-failed` cover the rest of the headless plan.
In the app, the Seed field, the scheduled-actions list and Run to tick rebuild any run exactly.
The Sweep and Results tabs build, run and inspect a sweep, and open any of its runs.
A GPU sweep runs up to four runs on one device at a time, with outputs identical to one at a time.
`henad-cli --spec` with a `[search]` table, or the Sweep tab's Search mode, runs any of the four searchers.

## Issues found & future directions

- **Text changes on failure paths.**
  - A failed device poll now reports the `FaultKind::Poll` message instead of anyhow's "GPU failed to finish the submitted work".
  - A GPU action refused during `run_steps_acting` is noted only once the wait succeeds. When the wait fails, the fault is reported and the note is not.

  Neither path is covered by the baseline.
- **`gpu::stepping` is native only,** since every helper in it blocks. The web GPU sim thread in M3 fires scheduled actions through its own encoder path, not through `stepping`.
- **The CHANGELOG.** Its Unreleased section gives #14 six Added entries and two Changed entries, the breaking changes to `FaultKind`, `SimCommand`, `poll_stats_readback`, `ModelFactory` and `GpuContext` among them. It was trimmed to match the length of v0.1.0's entries, and the network-model entries from #28 with it.
- **Verification was oversized.** The M0 workflow ran 149 agents, most of them verifiers, for a change with little risk. Later milestones use one or two review lenses and a single verifier per finding, and keep three-vote panels for determinism, concurrency and GPU submission.
- **Open after M1.**
  - A run that samples a non-finite value makes the CLI exit with status 3, the same as a failed run.
  - The memory projection and `--memory` saw only the probed config. M2 probes the last config too and takes the larger footprint.
  - A sweep that failed partway left its manifest at `running`. M2 writes `failed` when it still can. A killed process still leaves `running`.
  - A model whose parameter id equals a `runs.csv` column name (`status`, `ticks` and so on) would give duplicate headers. No model does today.
  - `--vary` level parsing lived in `henad-cli`. M2 moved it into `henad-core` as `LevelSpec::parse`, for the app's builder in M4.
  - A sweep piped into a closed stdout under `--json` still ends in a broken-pipe panic, now promptly.
- **Open after M2.**
  - The Latin hypercube is written `lhs` in spec files, the manifest and `--sample lhs:N`, as the plan has it. The naming rule would spell it out.
  - Two `--resume` processes on one directory are not locked out from each other.
  - A merged or resumed manifest keeps one engine and runtime record. Its `sessions` list notes a changed commit, but not a changed GPU adapter.
  - From the command line, a varied action tick needs the action declared with `--act ID@TICK` as well, and a stop's `min_tick` can only be set in a spec file.
  - The EMA Workbench recipe the plan mentions is not written yet.
- **Open after M3.**
  - After Run to tick, the Charts panel holds only the snapshots the UI received, often just the build tick and the target, so a fast run draws as a straight line.
  - Sending `SetSchedule` again mid-run fires the current tick's actions a second time. The app only sends it on a build, and a schedule edit waits for the next Build. The PR #47 review fixed it: both loops record the ticks that have fired, and no tick fires twice.
  - The replay label is built in `henad-core`, although the app shows it.
- **Open after M4.**
  - The builder had one text field per parameter in the `--vary` syntax. The redesign adds a values editor with Range fields and quick fills beside it, and an options menu for checkbox and dropdown parameters.
  - The Estimate button (a probe build of the first config that builds and of the last config, with a time estimate) is not built. It needs a probe API on `SweepRun`.
  - Some UI strings said "panel" where the dock and the docs say "tab". M6 changed them to "tab".
  - Load spec only reads a spec the builder can edit: one block, or the blocks One at a time writes. Any other spec, the shipped `sir_sweep.toml` included, is refused and runs from the command line only, as the app guide says.
  - The Response and Heatmap views pool over the axes not pinned to a level.
  - The benchmark ignore rule `results` was unanchored and caught `ui/results/`. It now reads `/results`, the one folder the benchmark scripts write.
  - Neither the Sweep nor the Results section of the app guide had a screenshot. Eight were taken from the redesigned tab and added.
- **Open after M5.**
  - A GPU run's timeout counts wall time, so with N tracks a run reaches its timeout about N times sooner. The PR #47 review fixed it: a run's clock counts its share of each round.
  - In the app, the interleaver and the UI thread take faults from one `FaultSink`, so a fault outside every scope can be blamed on the wrong side. The PR #47 review fixed it: a GPU sweep started from the app runs on a device of its own.
  - A suspected device loss is confirmed with a blocking device-wide poll. That runs only on a failure path.
  - The app's builder has no GPU memory field. It uses the device's largest buffer as the budget. Since the PR #47 review, a loaded spec's `gpu_memory` applies, and the tab still has no field to set one.
  - `GpuSimState::poll_stats_readback` now returns `StatsPoll`, a breaking change for an implementation outside the workspace. The CHANGELOG lists it.
- **Open after M6.**
  - The genetic algorithm ranks by the objective's aggregate, so `aggregate = "mean"` gives mean fitness where the plan named the median.
  - Pattern Space Exploration leaves a failed replicate out of an axis rather than counting it as the worst value, since an axis has no direction.
  - Random search and Pattern Space Exploration do not skip a configuration already evaluated, so in a small discrete space they can rerun it.
  - Hill climbing resamples a categorical gene on every neighbour and has no mutation rate of its own.
  - The objective and axis pickers in the app list stat labels, not the vector and histogram columns behind them. The PR #47 review fixed it: the lists offer a vector's parts and a histogram's total, from a build of the model at its default values.
  - With `reevaluate` on, hill climbing's best-so-far line can move against the goal when a re-evaluation lowers the incumbent's aggregate.
- **Departures from the plan.** A final audit compared the plan with the tree. Every planned file, type, flag, spec key, CSV column and manifest field is present. These differ:
  - The plan asked for one record per milestone. All seven milestones ran in one session, so this one record covers them, a section each.
  - `RunCursor` drives CPU runs only, on the command line, on the desktop and in a browser. A GPU run steps in the interleaver's own loop over `GpuTrack`s, with its own `Sampler`, and shares only the outcome helpers with the cursor. Tests pin the two paths together: a single-point GPU sweep equals `--export-stats`, and interleaved GPU runs equal sequential ones.
  - Under `--json` the search batch line is `explore_search_batch`, not `search_batch`, and a merge adds an `explore_merge` line. `docs/reference/cli.md` lists both.
  - Three tests carry other names:
    - `a_gpu_sweep_matches_export_stats` is `a_gpu_single_point_sweep_matches_export_stats`;
    - `a_replayed_run_matches_its_record` became four tests in `henad-explore/src/tests/replay.rs`, for a planned run, a GPU schedule, and CPU and GPU runs read back from a results folder;
    - `sampling_cadence_does_not_change_the_trajectory` is a module with one test per model.
  - Save results in a browser, which downloads the files one after another, was not exercised in the web check.
- **Fixed after the redesign.** The screenshot pass noticed five problems, and all five are fixed:
  - In Search mode, ticking Stop when picked the first stat (Susceptible). It now picks the objective's stat.
  - The Runs table cut every seed to four digits. The Seed column now fits a full `u64`.
  - The Sweep tab's title kept "Sweep done" after Edit sweep until the form was drawn.
  - The Series legend listed configurations out of id order.
  - A Pattern Space Exploration's axes started at 0 to 0. An axis given no bounds now takes its range from the initial samples, widened by 5% on each side.
- **Open after the colour pass.**
  - Error text is red 500, which falls below the WCAG AA ratio of 4.5:1 for small text: 3.79:1 on a tab body, 2.59:1 in a section header, and 1.96:1 in a selected Runs row. Red 400 would reach 5.0:1 on a tab body. It is a theme change, left to the maintainer.
  - Some shapes are still round: the legend markers, the Response view's points, radio buttons and the web loading ring.
- **Open after the PR #47 review.**
  - The search batches of a frame cost the results store time linear in the levels of a continuous axis, about 0.5 ms per batch on average up to 100,000 unique values in a release build. A new level moves every level above it.
  - A `first` reducer whose threshold is not finite is refused through `ReducerError::Comparison`, whose message now names the threshold. It has no variant of its own, unlike a stop condition's `StopError::NonFiniteThreshold`.
  - A search directory written before the Pattern Space Exploration fix, whose initial samples ended inside a batch, is refused on `--resume` through the run-key check, since its trajectory differs.
  - A quote that opens a field and never closes still hides the rest of a table from its readers. `RecordScan`'s doc says so. Refusing that tail would also refuse a directory whose process died inside a multi-line note, which resume has to repair.
  - A stat-column build that fails is not tried again in the same session, and the model's draft keeps counting each stat as one column.
- **Next.** Review and commit, and open F5, batched multi-world GPU dispatch, as its own research issue. M5's measurement is its baseline.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

