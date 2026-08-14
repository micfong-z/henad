---
date: 2026-08-12
title: "Module restructure — cpu/ and gpu/ as siblings"
model: Claude Opus 5 (claude-opus-5), via Claude Code
issue: none (housekeeping, ahead of gpu_ants)
status: complete
baseline_commit: ca058d7
delta_state: staged, uncommitted
---

# Module restructure — `cpu/` and `gpu/` as siblings

> `henad-compute` had grown to a flat 20-file `gpu/` module beside a flat crate root, and the naming implied a hierarchy that does not exist: `grid_engine.rs` at the root with `gpu/gpu_grid_engine.rs` beneath it reads as though the GPU engine is a child of the CPU one, when they are siblings.
> This moves both backends into `cpu/` and `gpu/`, each with its own runner, engines and `primitives/`, and groups `henad-core`'s four authoring traits under `authoring/` to separate them from `model.rs`, which is the runner interface.
> Pure move plus path updates.
> No behaviour change, no new broken doc links, all 151 tests pass with `HENAD_REQUIRE_GPU=1`.

---

## State before

At `ca058d7`, immediately after the GPU boids work.

`henad-compute` had 9 files at its crate root and 20 flat files under `gpu/`.
The root mixed CPU engines, the CPU runner, CPU primitives and two shared modules; `gpu/` mixed its runner, its engine, its primitives, its view seam and four small helpers with no grouping at all.

Two things about this were actively misleading rather than merely untidy:

- **The naming implied a hierarchy.** `grid_engine.rs` at the crate root and `gpu/gpu_grid_engine.rs` nested inside `gpu/` reads as the GPU engine being a specialisation of the CPU one. They implement different traits over different state and share nothing.
- **`henad-core` had `model.rs` next to `grid_model.rs`, `agent_model.rs` and `gpu_grid_model.rs`.** `model.rs` is the *runner* interface (`Model`/`SimState`); the other three are what a model author implements. AGENTS.md already had to spell out that distinction in prose, which is a sign the layout was fighting it.

Decisions taken before any move, put to the user:

| Question | Answer |
|---|---|
| Split the GPU half into its own `henad-gpu` crate? | No, keep it in `henad-compute` |
| Give the CPU side a matching `cpu/`? | Yes, so the two read as siblings |
| Subdirectories, or flat with better names? | Subdirectories |
| Short names inside a scoping module (`authoring/grid.rs`), or the original `grid_model.rs`? | Keep `*_model.rs`, so no two unrelated files share a basename |

---

## What was done

### `henad-compute`

```
crates/henad-compute/src/
├── lib.rs
├── snapshot.rs                     shared, both backends publish through it
├── runtime_info.rs                 shared
│
├── cpu/
│   ├── mod.rs
│   ├── sim_thread.rs               ← src/sim_thread.rs
│   ├── grid_engine.rs              ← src/grid_engine.rs
│   ├── agent_engine.rs             ← src/agent_engine.rs
│   ├── field/                      ← src/field/  (ca.rs, scalar.rs)
│   └── primitives/
│       ├── chunked.rs              ← src/chunked.rs
│       ├── scatter.rs              ← src/scatter.rs
│       └── lanes_macro.rs          ← src/lanes_macro.rs
│
└── gpu/
    ├── mod.rs                      GpuContext
    ├── sim_thread.rs               unchanged path
    ├── timing.rs                   unchanged path
    ├── limits.rs                   unchanged path
    ├── test_support.rs             unchanged path
    ├── grid_engine.rs              ← gpu/gpu_grid_engine.rs
    ├── view/
    │   ├── display.rs + display.wgsl   ← gpu/display.*
    │   └── agents.rs               ← gpu/agent_display.rs
    └── primitives/
        ├── spatial_hash.rs + hash_count.wgsl + hash_scatter.wgsl
        ├── prefix_scan.rs + scan.wgsl + scan_add.wgsl
        ├── reduce.rs + reduce.wgsl
        ├── readback.rs
        ├── dispatch.rs
        └── pipeline.rs
```

The two backends now mirror each other: a runner and the engines at the module root, `primitives/` for the shared building blocks.
`cpu/field/` and `gpu/view/` have no counterpart on the other side, which is honest — a CPU model's view is plain data in the shared `snapshot.rs`, and a GPU field layer does not exist yet.

An `engine/` subdirectory was tried and dropped.
It held one or two files per side, and `cpu::engine::grid_engine` stutters while `cpu::grid_engine` does not.
The rename also drops a stutter that was already there: `gpu::gpu_grid_engine::GpuGridState`.

### `henad-core`

```
crates/henad-core/src/
├── lib.rs                          re-exports Extent
├── authoring/
│   ├── grid_model.rs               GridModel
│   ├── agent_model.rs              AgentModel, AgentLanes, NeighborIndex
│   ├── gpu_grid_model.rs           GpuGridModel
│   └── field.rs                    FieldLayer, Extent, NoField
├── model.rs                        Model / SimState, the runner interface
├── grid.rs, spatial_hash.rs        data structures
├── params.rs, view.rs, topology.rs descriptors the UI reads
└── helpers.rs
```

`authoring/` is the project's own word for these — AGENTS.md already called them "three authoring traits ... plus `FieldLayer`".
Grouping them puts the distinction from `model.rs` in the directory tree instead of in a paragraph explaining it.

The files keep their original `*_model.rs` names rather than shortening to `grid.rs` inside the scoping module.
Shortening was tried first and reverted: it put four files named `grid.rs` in the workspace covering three unrelated concepts (`Grid2D` the data structure, `GridModel` the trait, `GridModelState` the engine), which is worse in a fuzzy-finder than a slightly redundant path.

`Extent` is re-exported at the crate root.
It is a plain world-size type used in nearly every file, and `henad_core::authoring::field::Extent` is too much path for what it is.

### Path updates

Every call site was repointed by regex over the five crates, then verified by compilation:

- `henad_compute::grid_engine` → `henad_compute::cpu::grid_engine`, and the same shape for `agent_engine`, `sim_thread`, `chunked`, `scatter`, `field`.
- `henad_compute::gpu::gpu_grid_engine` → `gpu::grid_engine`, `gpu::display` → `gpu::view::display`, `gpu::agent_display` → `gpu::view::agents`, and the four primitives into `gpu::primitives::`.
- `henad_core::grid_model` → `henad_core::authoring::grid_model`, and the same for the other three.

`cpu/mod.rs` re-exports `GridModelState`, `AgentModelState`, their param-descriptor helpers and `GRID_INIT_SEED`, so the common types stay one path segment away.
`gpu/mod.rs` keeps the re-exports it already had.

**The two exported macros needed their internals repointed.** `agent_lanes!` and `for_each_chunk_mut!` are `#[macro_export]`, so their own paths are unchanged — they live at the crate root regardless of which file defines them — but their bodies expand to `$crate::chunked::chunk_seed`, `$crate::chunked::__rayon` and `$crate::__lanes`.
Those became `$crate::cpu::primitives::chunked::...`, and `lib.rs` still re-exports `__lanes` from its new home.
A missed one here would have failed only at the macro's *use* site in `henad-models`, not where it is defined.

---

## State after

30 `.rs` files in `henad-compute/src` (26 before, the 4 new ones all `mod.rs`), 13 in `henad-core/src` (12 before).
Git recorded the moves as renames, so `git log --follow` still works on every moved file.

No two unrelated files share a basename any more.
Every remaining duplicate is a counterpart pair — `cpu/sim_thread.rs` and `gpu/sim_thread.rs`, `ants/step.rs` and `boids/step.rs`, `henad-core/src/spatial_hash.rs` and `gpu/primitives/spatial_hash.rs` — where the shared name is the point.
That rule is now written into AGENTS.md.

Verification:

- `./check.sh` green, including the wasm typecheck and `trunk build`.
- All 151 tests pass with `HENAD_REQUIRE_GPU=1`, so the GPU paths were genuinely exercised rather than skipped.
- `cargo doc` reports **9 broken intra-doc links, exactly the 9 that existed at `ca058d7`** — checked by running the doc build against a stash of the restructure and diffing the two lists. No new breakage.

`AGENTS.md` was rewritten to describe the new layout, including an explicit statement that `cpu/` and `gpu/` are siblings rather than a base and a specialisation, since that is the misreading the old naming invited.

---

## Issues found & future directions

### 1. A regex rewrite hit the wrong crate

Rewriting `crate::field` → `crate::cpu::field` was correct inside `henad-compute` and wrong inside `henad-core`, which has its own `field` module.
It produced `use crate::cpu::field::{Extent, FieldLayer};` in two henad-core files.
Caught immediately by `cargo check`, reverted with `git checkout crates/henad-core/`, and the rewrite was rerun scoped per crate.

Worth remembering for the next bulk rename: `crate::`-relative paths mean different things in different crates, so a workspace-wide regex on them is unsafe by construction.
Only `henad_compute::`-style absolute paths are safe to rewrite globally.

### 2. Pre-existing broken doc links

Nine, all predating this work, mostly `[`Self::FOO`]` used inside a `//!` module doc where `Self` does not resolve — `authoring/gpu_grid.rs` accounts for six.
Also `SNAPSHOT_INTERVAL`, `GpuSimLoop::step_batch` and `SimState::stats`.
Not fixed here, since that is a content change rather than a move, and mixing the two is what makes a restructure hard to review.
Worth a follow-up: `cargo doc` is not part of `check.sh`, so nothing currently catches these.

### 3. `henad-models` has the same asymmetry, untouched

:human: This is intentional.

`boids/`, `ants/`, `sir.rs` and `game_of_life.rs` sit beside `gpu_boids/`, `gpu_sir/` and `gpu_game_of_life/`.
The same `cpu/` and `gpu/` split would apply, and the `gpu_` prefixes would stop stuttering.
Not done, as it was outside what was asked, and `gpu_ants` will land there shortly — doing it after that model exists means moving it once rather than twice.

### 4. `AGENTS.md` is still hard-wrapped

:human: Don't do this. No human is likely going to read that any way.

The repo now uses semantic line breaks for markdown, but `AGENTS.md` predates that and is wrapped to a column.
The section rewritten here matches its surrounding style rather than mixing the two conventions in one file.
A whole-file reflow is a separate change.

---
<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     If you update this document, stop at the line above.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

Dumb little AI named unrelated files the same, like `grid.rs` for both the files holding `GridModel` and `Grid2D` respectively.
I asked it to rename them to `grid_model.rs` and `grid_engine.rs`, and changed counterparts to match.

