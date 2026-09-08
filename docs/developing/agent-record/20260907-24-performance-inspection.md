---
date: 2026-09-07
title: "Closing the CPU gaps the cross-engine sweep found"
description: The sweep's four losses traced to three mechanisms, a dense field rebuild, a rayon wake-up per pass, and a neighbour walk that measured every distance twice. All three are fixed, and every model is faster on one thread and on all cores.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code), finders and verifiers on claude-opus-5, one dedup pass on claude-sonnet-5
issue: 25 — Benchmarks against other ABMs
state: implemented and measured, `./check.sh` green, published sweep not re-run
baseline_commit: 1df9464
delta_state: uncommitted on `master`
---

# Closing the CPU gaps the cross-engine sweep found

> The published sweep had four red cells: ants on all cores slower than on one thread, ants and boids on one thread behind Agents.jl, krABMaga and MASON, and the small grid rungs slower on all cores than on one.
> Six finders proposed sixty-one changes and one verifier tried to refute each; twenty-nine survived.
> Three mechanisms turned out to account for nearly all of it, and this session fixes them.
> Every model is now faster on one thread and on all cores, ants by 1.6x to 4.7x and boids by 1.6x to 7.2x, and the grid models lose nothing at the top of the ladder while gaining 14x at the bottom.

## State before

`results/compare/hardywen_20260907.csv` (24-core Threadripper 3960X, RTX 4090, commit da07055) is the reference.
The reference engines all run single-threaded, so the fair CPU comparison is Henad's `1t` column.

| Model | Loss | Size |
|---|---|---|
| ants, 1 thread | Agents.jl at 2k, 6k, 20k | 3.1x, 1.5x, 1.3x |
| ants, 1 thread | MASON and krABMaga at 2k | 1.6x, 1.4x |
| ants, all cores | Henad's own 1-thread column at 2k to 60k | 2.4x to 3.7x slower |
| boids, 1 thread | krABMaga at every rung | 1.5x at 1k, 1.16x at 100k |
| boids, 1 thread | MASON up to 30k | 1.4x at 1k |
| game of life, all cores | 1-thread column below 512² | 2.6x slower at 64² |
| sir, all cores | 1-thread column below 256² | 1.9x slower at 64² |

Everything else was a win, most of it by one to three orders of magnitude, and the GPU column won everywhere.

## What was done

Everything below was measured on an Apple M4 Pro (10 performance and 4 efficiency cores, 14 rayon threads), medians of three reps, interleaved against a binary built from the unmodified tree in the same script.
Where a change claims to be bit-identical, the final state was exported with `--export` and compared byte for byte, at one thread and at fourteen.

### The pheromone field stops being rebuilt densely

`ScalarField::update` ran, per layer and per tick, a scatter and then a decay pass.
The scatter's shadow arm clears one full-field shadow per worker, deposits into it, and reduces every shadow into the next buffer, so its cost is workers times cells however few deposits arrive.
The ladder puts twenty cells behind every ant, so at 48 workers the engine moved about 8 kB of grid per deposit.
The sorted arm was picked only when the shadows would exceed 256 MiB, which on the sweep's machine happens at 200k agents and nowhere else, and that is exactly where the all-cores regression ended.

`ScatterGrid` gains a third arm.
`Banded` splits the grid into one contiguous band per worker, buckets the deposit lanes by band with a counting sort over the band index, and has each band copy its base, merge its own deposits and apply the finish.
No shadows, no atomics, and the result does not depend on the band count.
One band skips the bucketing entirely, which is what a single worker gets.
A deposit of the identity cannot raise a non-negative cell, so those are dropped while bucketing; ants leaves half of every lane at the identity, since an ant writes one of two layers.

`scatter_then` carries a closure applied to every combined cell, and the field hands it its decay instead of walking the grid again.
Decay is monotone on non-negative values, so decaying a merged cell and merging decayed ones agree bit for bit.

The arm rule was measured, not reasoned.
`benches/scatter.rs` swept 1 to 100 agents per cell and never the ladder's 0.05, so it gains a sparse group at the density a field layer actually runs, across the agent counts of the agent ladder and three pool widths, and a banded candidate in the existing dense thread sweep.

| Scatter arm, µs | sort | shadow | banded |
|---|---|---|---|
| 2k agents, 1 thread | 96.2 | 34.2 | 9.8 |
| 2k agents, 14 threads | 315.1 | 200.0 | 46.2 |
| 20k agents, 14 threads | 1078.0 | 610.1 | 103.0 |
| 200k agents, 14 threads | 8157.1 | 5690.9 | 462.7 |

Banded led every sparse rung by 2.3x to 12.3x.
The dense sweep refuted the half of the rule I had written from the arithmetic: at 10M deposits into 10M cells on one worker, shadow won at 42.2 ms against banded's 68.9.
So the rule is one clause, `n_deposits < n_cells`, and the crossover does not move with the pool width.

### Every parallel pass stops waking a parked caller

Both hosts stepped from a thread that is not a rayon worker, so each parallel pass inside a kernel was injected from outside the pool and parked the caller until it finished.
`henad-cli` now steps inside `rayon::scope`, and `runner/thread.rs` pumps inside one.
Only the pump moves: the waits either side stay outside, since a worker blocked on a command channel is a worker the pool cannot use, and on a one-worker pool it would never come back.

The larger half of the cost is per worker rather than per pass.
A worker that finds no work sleeps, and each pass then wakes the sleepers one at a time.
An ants tick at 2k agents on 14 threads issued about 66 such wake-ups against a 185 µs single-thread tick.
Fewer, bigger jobs is the answer, and for the grid models that is a floor under how many rows a rayon leaf takes.
`for_each_chunk_mut!` gains a `min_leaf` arm, and `rows_per_leaf` derives the floor from the grid width alone.
Each row keeps its own index and its own `chunk_seed`, so the grid a tick produces is unchanged.

The floor is only a floor.
A first version also aimed at a leaf count per worker and left a 4096 wide SIR grid slower than no floor at all, because a row's cost varies with what its cells hold and coarser leaves stop rayon balancing that by stealing.
Measured across both models from 64² to 4096², 8192 cells is the largest floor that costs nothing at the top of the ladder.

### The boids kernel stops measuring every distance twice

`query_radius` computed the toroidal squared distance for every candidate to reject the ones out of range, pushed the survivors into a thread-local `Vec`, and the kernel then recomputed both axis deltas and the distance for each one.
`SpatialHash::for_each_within` walks the same cells in the same order and hands the kernel the deltas and the squared length it already has, and `query_radius` is now that walk collecting indices.
The summation order is unchanged, so the trajectory is unchanged.

The hash cell was the visual range, so the 3x3 block covered nine r² for a disc of about 3.14, and roughly two candidates in three were fetched only to be discarded.
It is now a third of the visual range.
That changes the float visit order, so the boids trajectory differs in its last bits.
It is the one behaviour change in this session.
Both boids consistency fixtures pass (they are 1e-5 absolute after one tick), `results_do_not_depend_on_the_thread_count` passes, and `gpu_boids` reads its cell size from the CPU model so the two backends still walk one grid.

Nothing had bounded the cell count, which grows with the square of that divisor, and the app admits a 10,000 wide world with a visual range of 1.
`HashGrid::new` is now the one place the geometry is decided, for the CPU sort and the GPU one alike, and it caps the grid at `MAX_INDEX_CELLS`.
A coarser cell only makes a query scan candidates it then rejects, where an unbounded one asked for gigabytes.

`BoidsModel::CHUNK` drops from the default 512 to 64.
A thousand agents were two chunks, so the all-cores column at the small rungs was a two-thread run.
Boids draws no random numbers in its kernel, so the seeding granularity costs it nothing, and the value is bit-identical.
Measured across the ladder, 64 is 3.3x at a thousand agents and costs 4% at a hundred thousand.

### What was left alone

Ants' `CHUNK` has the same cap but is also its RNG seeding unit, so changing it changes its results for a 2 to 6% gain on a column that has no cross-engine counterpart.

An interior peel of the two 3x3 walks in the ants kernels measured 8% on its own and 16% on top of the field change, bit-identical.
It is not here, because the reference ports all loop plainly over the Moore offsets and hand-unrolling only Henad's would be the model-side tuning that makes a cross-engine comparison unfair.
The fair version puts the peel in the neighbourhood primitive, where the offsets are a runtime slice and the peel needs a bound the primitive does not have.
That is a design question, not a mechanical change.

A per-tick table of `(1 - p)^k` for SIR measured 0.5 to 3.5%.
Disassembly showed the SIR row loop carries no vector instruction at all, where Game of Life runs 16 cells per iteration: the per-row RNG state that `step_cell` advances on two of its three arms is a loop-carried dependency.
Only a draw independent of the row's earlier cells would let the row vectorise, and that changes the trajectory.
SIR already beats every reference engine on one thread.

`opt-level = 3` measured 0 to 8%, and fat LTO measured 12 to 14% on ants against 4 to 5% lost on both grid models.
The profile comment is explicit that level 2 is for wasm size, so a native benchmark profile is worth adding, but it is a build decision rather than an engine one and nothing here turns on it.

### Measured

Medians of three reps, one interleaved run on a cooled machine, base against the tree at 1df9464.

| One thread | base | now | |
|---|---|---|---|
| game of life 64², 100 steps | 682 µs | 340 µs | 2.0x |
| game of life 256² | 2.36 ms | 1.45 ms | 1.6x |
| game of life 1024² | 13.0 ms | 12.1 ms | 1.1x |
| game of life 4096², 40 steps | 60.5 ms | 60.7 ms | 1.0x |
| sir 64², 100 steps | 920 µs | 394 µs | 2.3x |
| sir 1024² | 110 ms | 114 ms | 1.0x |
| sir 4096², 40 steps | 1.33 s | 1.34 s | 1.0x |
| boids 1k, 100 steps | 585 ms | 256 ms | 2.3x |
| boids 10k | 5.16 s | 3.00 s | 1.7x |
| boids 100k, 20 steps | 10.42 s | 6.63 s | 1.6x |
| ants 2k, 200 steps | 37.5 ms | 20.5 ms | 1.8x |
| ants 20k | 336 ms | 198 ms | 1.7x |
| ants 200k, 100 steps | 1.59 s | 973 ms | 1.6x |

| All cores, 14 threads | base | now | |
|---|---|---|---|
| game of life 64², 100 steps | 4.23 ms | 295 µs | 14.3x |
| game of life 256² | 5.85 ms | 2.29 ms | 2.6x |
| game of life 1024² | 9.86 ms | 7.19 ms | 1.4x |
| game of life 4096², 40 steps | 11.5 ms | 10.1 ms | 1.1x |
| sir 64², 100 steps | 4.19 ms | 432 µs | 9.7x |
| sir 1024² | 20.8 ms | 18.6 ms | 1.1x |
| sir 4096², 40 steps | 133 ms | 136 ms | 1.0x |
| boids 1k, 100 steps | 327 ms | 45.3 ms | 7.2x |
| boids 10k | 765 ms | 654 ms | 1.2x |
| boids 100k, 20 steps | 995 ms | 658 ms | 1.5x |
| ants 2k, 200 steps | 147 ms | 58.9 ms | 2.5x |
| ants 20k | 358 ms | 125 ms | 2.9x |
| ants 200k, 100 steps | 1.33 s | 281 ms | 4.7x |

Three of the four red cells are gone on this machine.
Game of Life on all cores now beats its own one-thread column at 64², where it was 6x behind.
Ants on all cores beats one thread from 20k up, where it used to lose everywhere below 200k.
The single-thread gains are what the cross-engine table cares about, and on the sweep's ratios they put boids ahead of krABMaga at every rung and ants ahead of Agents.jl from 6k up.

The fourth is smaller but not gone.
At the smallest rungs all cores is still behind one thread, by 1.1x for sir 64² and 2.9x for ants 2k, and what remains there is workers being woken one at a time for passes that are over before the last of them arrives.

```text
AGENTS.md                                            chunk sizes, three arms, the wake-up note
crates/
    henad-cli/src/main.rs                            steps inside a rayon scope
    henad-compute/
        benches/scatter.rs                           a banded candidate, and the sparse regime
        src/
            cpu/field/ca.rs                          a floor under the rows in a rayon leaf
            cpu/field/scalar.rs                      decay folded into the merge
            cpu/primitives/chunked.rs                a min_leaf arm on the macro
            cpu/primitives/scatter.rs                the banded arm, scatter_then, the arm rule
            runner/thread.rs                         pumps inside a rayon scope
    henad-core/src/spatial_hash.rs                   for_each_within, one capped geometry
    henad-models/src/boids/
        mod.rs                                       CHUNK 64, cell size a third of the range
        step.rs                                      one pass over the neighbours
docs/
    authoring/agent-models.md                        the chunk sizes
    authoring/fields.md                              three arms, decay in the merge
    authoring/performance.md                         chunk sizes, arms, for_each_within
    developing/cpu-backend.md                        arms, leaf floor, the scope in the runner
    developing/agent-record/20260907-24-performance-inspection.md   this file
zensical.toml                                        nav entry #24
```

## State after

`./check.sh` is green, and `HENAD_REQUIRE_GPU=1 cargo test --workspace --all-targets --all-features` passes.
The app was run and driven through the inspection port: SIR at 1024² builds, plays, holds the 30 TPS its slider asks for and reports 1.0 ms per engine tick, so the scope in the runner steps and does not deadlock.

`ScatterGrid` gained three tests, covering all three arms in the dense regime and the sparse one, with and without a finish applied, plus the arm rule itself.
`SpatialHash` gained two, one pinning the cell cap and one asserting that the sort and the shared geometry agree.

Every model except boids is bit-identical to 1df9464, checked by export at one thread and at fourteen.
Boids differs in its last bits, from the hash cell size, and passes both consistency fixtures.

`results/compare` is untouched and now describes the old code.
The published tables need a fresh sweep on the reference machine before they mean anything again.

## Issues found & future directions

1. **Re-run the sweep.**
   Every published number predates this session, and the ratio tables under `docs/assets/benchmarks/tables/` are stale in Henad's favour by the factors above.
   Nothing else here should be believed about the Threadripper until that runs, since these gains were measured on a machine with a third of its threads and a different memory system.
2. **Worker wake-ups.**
   What is left of the small-rung all-cores gap is workers sleeping between passes and being woken one at a time.
   The band arm already cut the ants tick from eight parallel dispatches to four.
   Fewer passes per tick and jobs sized so a small pass touches few workers are the two levers that need no new machinery; keeping workers spinning between passes would need a persistent-worker design rayon does not offer.
3. **The ants agent passes.**
   Now that the field update is one pass, the two 3x3 walks are most of an ants tick.
   The peel is worth 16% and wants a home in the neighbourhood primitive rather than in the model.
   A static open-neighbour mask and a reservoir threshold table are each worth under 4% behind it.
4. **A native benchmark profile.**
   Level 3 is worth up to 8% on sir and free; LTO is a net loss on the grid models and should not be switched on by default.
   If it lands, `benchmarks/protocol.md` should record it, since the krABMaga port deliberately pins cargo's defaults and the two AOT builds should be told apart on purpose rather than by accident.
5. **Report the effective parallel width.**
   Three of the four losses were partly chunk- or job-limited runs reported as 48 threads.
   The CSV should carry the number of jobs a tick actually produced beside the thread count.
6. **Two cold-start cells.**
   `rep_times_s` in the committed CSV has the median 54% above the minimum at Game of Life 128² and 44% at sir 64².
   More reps at cheap points, applied to every engine alike, is the fix; a Henad-only hardware ramp is not.
7. **Boids beyond the kernel.**
   A cell-ordered copy of the positions inside the hash was estimated at 10 to 15% at 100k and above, and the counting-sort rebuild is still sequential.
   Neither was measured here.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

This fixes performance issues mentioned in [#23](20260905-23-audit-repairs.md).
Most of the fixes are reasonable and should be correct given the tests, though I refused a few places that would make us dig too deep into the model code rather than the engine itself.