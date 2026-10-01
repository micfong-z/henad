---
date: 2026-10-01
title: "Library M2: surface hygiene"
description: The second milestone of #48. CPU agent models stop building their parameter list every tick, Network hides its engine-only methods, the exported macros reach their support items through a hidden module, every public type of the four library crates implements Debug, and a test pins each example model's schema hash to the value 0.2.0 recorded. Two fixes from the review of M1 come first.
icon: material/broom
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M2 implemented, `./check.sh` with HENAD_REQUIRE_GPU=1 and the docs build green, the boids and ants gate passed
baseline_commit: 9a0c09f
delta_state: uncommitted on `48-library`
---

# Library M2: surface hygiene

> M2 is the second of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> It tidies the public surface without changing a result.
> `AgentModelState` now counts its model's own parameters once, at construction. Before, every tick built and dropped the whole descriptor list to read its length.
> `Network`'s six engine-only methods are hidden from the documentation, the exported macros of henad-core and henad-compute name their support items through one hidden module per crate, and `agent_lanes!` names the standard prelude by full path.
> Every public type of henad-core, henad-compute, henad-models and henad-explore now implements `Debug`.
> A new test, `schema_hashes_are_unchanged_since_0_2_0`, pins each example model's schema hash to the value Henad 0.2.0 wrote, recorded from the tag through a written procedure.
> The performance gate compared boids and ants against the M1 binary, and every median paired ratio came out between 0.989 and 1.012, inside the bound of 1.05.
> Two fixes from the review of M1 came first: henad-models' `exclude` had dropped `src/tests/` from its package, and an AGENTS.md comment read ambiguously.

## State before

`48-library` stood at 9a0c09f, M1, with a clean tree.
`AgentModelState::step` and `act` called `split_params::<A>`, which built `A::param_descriptors()` to read its length, one `Vec<ParamDescriptor>` allocated and dropped per tick.
`agent_lanes!` and `for_each_chunk_mut!` named `$crate::cpu::primitives::chunked::chunk_seed`, `chunked::__rayon` and `$crate::__lanes`, and `agent_lanes!` named `Vec`, `Option`, `Some`, `Default`, `Send`, `Sync` and `Fn` through the caller's prelude.
henad-core's `actions!`, `params!` and `buffers!` named their types by module path.
About 90 public types in the four library crates had no `Debug`, and about 25 more in henad-app.
Nothing pinned the schema hashes across releases.

## What was done

### Fixes from the review of M1

- **henad-models' package.** `exclude = ["tests/"]` matched like a `.gitignore` line, at any depth, so it also dropped the 11 files of `src/tests/`.
  It now reads `"/tests/"`. `cargo package -p henad-models --list --allow-dirty --no-verify` lists all 11 files under `src/tests/` and none under the top-level `tests/`.
  The package built before because `src/tests/` is compiled only under `cfg(test)`.
- **AGENTS.md.** The comment on `./check.sh` now reads "all CI checks except the slow ones".

### The agent engine's cached parameter count

`AgentModelState` gained `own_params`, set from `A::param_descriptors().len()` in both constructors.
`step` and `act` split the parameter values through a private `split_at_own(params, own)`.
The public `split_params::<A>` stays for the GPU ports, which call it inside `pass_params_bytes`, and now delegates to the same function.
No other engine builds a descriptor list per tick.

### `Network`'s engine-only methods

`spawn`, `retire`, `set_directed`, `should_repack`, `repack` and `rebuild` are `#[doc(hidden)]`.
They stay public, since henad-compute's engine and its tests call them, and a model goes through `Nodes`.

### Macro support modules

henad-core and henad-compute each gained a `#[doc(hidden)] pub mod __macro_support`.
henad-core's re-exports `ActionDescriptor`, `ParamDescriptor` and `BufferSpec`, and `actions!`, `params!` and `buffers!` name them through it.
henad-compute's re-exports `AgentLanes`, `ChunkTally`, `LaneSpec`, `chunk_seed` and `rayon`, and replaces the hidden `henad_compute::__lanes` module and `chunked::__rayon` re-export.
`agent_lanes!` and `for_each_chunk_mut!` reach all of them through it.
`agent_lanes!` now names `::std::vec::Vec`, `::std::vec!`, `::core::option::Option`, `Some`, `::core::default::Default`, `::core::marker::{Send, Sync, Copy}`, `::core::clone::Clone` and `::core::ops::Fn` by path.
The expanded code is otherwise unchanged, and so are the kernels it inlines.
`for_each_chunk_mut!`'s method calls still lean on rayon's prelude, which [6.5] leaves for a benchmark that shows the hot loop still inlines.

### `Debug`

rustc's allow-by-default `missing_debug_implementations` lint, run once through clippy, listed 90 public types in henad-core, henad-compute, henad-models and henad-explore.
Each gained `Debug` in its derive, apart from these, which hand-write one:

| Type | What it prints |
|---|---|
| `AgentModelState`, `GridModelState`, `NetworkModelState` | The model id, the tick, the extent where it has one, the param store, and the graph's summary for a network |
| `GpuAgentState`, `GpuGridState` | The model id, the tick, and the grid size |
| `StepCtx` | The extent. Its other fields are a model's associated types |
| `Network` | Node, slot and edge counts, direction and version, in place of every edge |
| `ModelEntry` | Its declarations, without the factory and capacity closures, as [4.4.1] specifies |
| `ModelState` | The backend |
| `SimThread`, `GpuSimThread` | Nothing but the name |
| `RunCursor` | The planned run and its run key |
| `SweepRunOptions` | Every field, with the wake callback as whether it is set |
| `runner::Driver`, native and wasm32 | Whether the thread is held, or whether the loop has finished |
| `PumpedSweep` (wasm32 and tests) | The model id and the plan's run count |

Each hand-written impl ends in `finish_non_exhaustive`.
Two private types joined so a derive could compile, the GPU primitives' two `Level` structs, as did team assembly's `LiveSet` and `RetirementRing`.
The lint, run for the native target and for `wasm32-unknown-unknown` without atomics, finds none left.
The first `./check.sh` caught the wasm32 half: `SweepRun`'s derive reached the frame driver, which had none.
henad-app's roughly 25 were left alone. Its `state` and `ui` modules are app internals, and M9 makes them private.
No lint was added to the workspace, and AGENTS.md states the rule and the command that checks it.

### `schema_hashes_are_unchanged_since_0_2_0`

The hashes were recorded from the `v0.2.0` tag, never from the changed tree, through the procedure of [4.9.3], now written in `crates/henad-models/tests/fixtures/docs/schema-hashes-0.2.0.md`.
A worktree at 773a7a5 built a release `henad-cli`, which ran `henad-cli <id> --steps 1 --out <dir>/<id>` for each of the ten models `--list` named, and each manifest's `model.schema_hash` was read.
Every manifest's engine block read version 0.2.0 and commit `773a7a5b`, on an Apple M4 Pro with its Metal adapter.

The test sits in henad-explore's `schema.rs`, beside `schema_json`, since computing a schema hash takes `model_schema` from henad-explore and the entries from henad-models.
It compares the ten hashes against the current registry, and leaves the GPU models out without a device unless `HENAD_REQUIRE_GPU` is set.
It passed on the changed tree with every GPU model included.
A model added after 0.2.0 has no recorded hash, and the test does not ask for one.

### Edited tree

```text
.
├── AGENTS.md                           ~ check.sh comment, Network's hidden methods, the cached count,
│                                         the schema-hash test, the Debug and macro rules
├── CHANGELOG.md                        ~ Unreleased
├── crates/
│   ├── henad-core/src/
│   │   ├── lib.rs                      + __macro_support
│   │   ├── action.rs, params.rs        ~ actions! and params! through __macro_support
│   │   ├── authoring/model/
│   │   │   ├── gpu_agent_model.rs      ~ buffers! through __macro_support, Debug
│   │   │   ├── agent_model.rs          ~ Debug, StepCtx's by hand
│   │   │   └── field.rs, gpu_grid_model.rs, network_model.rs   ~ Debug
│   │   ├── network.rs                  ~ six methods hidden, Debug by hand
│   │   └── export/stats_csv.rs, grid.rs, metadata.rs, spatial_hash.rs, view.rs   ~ Debug
│   ├── henad-compute/src/
│   │   ├── lib.rs                      + __macro_support, - the __lanes re-export
│   │   ├── cpu/agent_engine.rs         ~ own_params, split_at_own, Debug by hand
│   │   ├── cpu/primitives/
│   │   │   ├── lanes_macro.rs          ~ __macro_support and full prelude paths, - __lanes
│   │   │   ├── chunked.rs              ~ through __macro_support, - __rayon
│   │   │   └── scatter.rs              ~ Debug
│   │   ├── cpu/{grid,network}_engine.rs, gpu/{grid,agent}_engine.rs   ~ Debug by hand
│   │   ├── cpu/sim_thread.rs, gpu/sim_thread.rs                       ~ Debug by hand
│   │   ├── runner/frame.rs, runner/thread.rs                          ~ Debug by hand on Driver
│   │   └── cpu/field/*, cpu/layout.rs, gpu/**, runner/mod.rs, runtime_info.rs, snapshot.rs   ~ Debug
│   ├── henad-explore/src/
│   │   ├── schema.rs                   + schema_hashes_are_unchanged_since_0_2_0
│   │   ├── cursor.rs, handle.rs, pumped.rs   ~ Debug by hand
│   │   └── exec/mod.rs, output/*       ~ Debug
│   └── henad-models/
│       ├── Cargo.toml                  ~ exclude "/tests/"
│       ├── src/registry.rs             ~ Debug by hand for ModelEntry and ModelState
│       ├── src/**                      ~ Debug on the model and hot-param types
│       └── tests/fixtures/docs/schema-hashes-0.2.0.md   +
├── docs/developing/agent-record/20261001-31-library-surface.md   +
└── zensical.toml                       ~ nav entry #31
```

### Performance gate

The gate of [7.1] for M2 is boids and ants, the two `AgentModelState` models, on the CPU rungs of the gate set, against the M1 binary.
Both binaries were release builds on rustc 1.97.1: M1 from a `git archive` of 9a0c09f with a target directory of its own, and M2 from the final working tree.
Before any timing, `--export-stats --seed 1 --steps 300` wrote identical bytes from both for all six CPU models, and again for boids and ants from the final M2 build.

Each configuration ran `henad-cli --json --seed 1 --steps 1000 --warmup 200 --global-warmup 1000 --reps 3` in two ABBA blocks, pairing rep `i` of one binary with rep `i` of the other through their shared seed, 12 pairs each.
Boids and ants ran at the model's default density with the world scaled as `scripts/bench_matrix.py` scales it.
The ratio is M2's time over M1's, so a ratio above 1 means M2 is slower.

| Configuration | M1 median rep | Median ratio, M2 over M1 | Range of ratios | Wall time, both binaries |
|---|---|---|---|---|
| boids 1,000 | 0.43 s | 0.994 | 0.960 to 1.006 | 16 s |
| boids 50,000 | 21.4 s | 0.989 | 0.977 to 1.008 | 771 s |
| ants 1,000 | 0.14 s | 1.012 | 0.976 to 1.039 | 5 s |
| ants 1,000,000 | 14.3 s | 1.012 | 0.945 to 1.067 | 524 s |

Every configuration passes the bound of 1.05, and none reached the 1000-second cap.
Nothing else of this session ran during the timings.
The machine was not fully cooled: the run waited until the one-minute load average fell to 2.4, and an orphaned Python process from another project's session held one core at 100% throughout, as it had for two and a half days.
Since each ratio pairs runs made minutes apart, that steady load weighs on both binaries alike.
The change removes one small allocation per tick, which is far below what these rungs can resolve, and no gain was expected or found.

## State after

M2 is implemented and uncommitted on `48-library`, on top of 9a0c09f.
The workspace version stays 0.2.0.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 961 tests, none failed, one more than M1 for the new hash test, with the packaging, cargo-deny and docs steps and the web build.
  Its first run failed at the no-atomics wasm32 typecheck, on the frame driver's missing `Debug`, which the second pass fixed.
- `uv run --locked zensical build` passes.
- `cargo clippy -- -W missing_debug_implementations` over the four library crates finds no type without `Debug`, natively and for wasm32.
- `schema_hashes_are_unchanged_since_0_2_0` passes with all ten example models on the Metal adapter.
- `--export-stats` at seed 1 is byte-identical between M1 and M2 for every CPU model.
- The gate passed, as above.

## Issues found & future directions

- **henad-app's `Debug`.** About 25 public types in henad-app still lack it, left for M9, which makes their modules private.
- **The `Debug` rule has no lint.** `missing_debug_implementations` would hold it, and it was left out of the workspace lints as the milestone asked. AGENTS.md gives the command that lists offenders.
- **Bulk containers print in full.** `Grid2D`, `SpatialHash`, `ScatterGrid`, the snapshots and the views derive `Debug`, and print every cell, as a `Vec` does. `Network` prints a summary because its CSR rows had no `Debug` to derive.
- **Two warnings for wasm32 predate M2.** Clippy for `wasm32-unknown-unknown` warns about a `drop` of a non-`Drop` value in `gpu/sim_thread.rs:428` and `output/mod.rs:331`. `./check.sh` and CI only typecheck that target, and never lint it, until the nightly atomics clippy of [4.10] arrives.
- **An orphaned process loaded every benchmark.** A Python process from another project's session, started on 2026-09-29 from a heredoc in a background task whose shell had since died, held one core throughout. Killing it before the final comparison of M11 would give quieter numbers.
- **Next.** M3, the entry layer in henad-compute, which depends on M1 alone.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
