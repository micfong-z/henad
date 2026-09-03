---
date: 2026-08-15
title: "Comment style sweep — applying the new AGENTS.md writing rules to every existing comment"
description: Applying the new comment rules in AGENTS.md across every comment in the workspace.
icon: material/note-text-outline
status: ai-generated
model: Claude Opus 5 (claude-opus-5), via Claude Code
issue: none — follow-up to the manual note on 20260814-05
state: complete — the four mechanical rules are at zero across the workspace, with two judgement calls named under "Issues found"
baseline_commit: 4315fe6
delta_state: uncommitted working tree on `master`
---

# Comment style sweep

> The manual note closing the previous session said the comments were the problem: *"Claude seems to disregard my style on comments unless instructed, and yapped quite a bit"*, and *"Removed unnecessary `#[must_use]` attributes since it is slowly becoming an overused style."*
> Both observations were then written up as rules in `AGENTS.md`, under a new **Writing style** section.
> This session applied those rules backwards over everything already in the tree.
>
> Nothing here changes behaviour.
> 55 files, +348/-511, comments and attributes only, and a net 144 comment lines gone.
>
> The four mechanically checkable rules are now at zero.
> Em dashes in comment prose went 99 to 0, prose semicolons and colons to 0, headless `What`/`Why`/`Whether`/`How`/`Which` docs 29 to 0, and `#[must_use]` 17 to 0.
> The rest was judgement: docs that restated the signature below them were deleted outright, design narration was cut where `docs/agent-record` already holds it, and two benchmark percentages were dropped from source.
>
> The largest single reductions were the two GPU runner files.
> `gpu/sim_thread.rs`'s module doc went from 50 lines to 23, and `gpu/timing.rs`'s `MAX_BATCH_SIZE` doc from 12 to 4.

---

## State before

At `4315fe6` on `master`, with one uncommitted change: the `AGENTS.md` **Writing style** and **Working agreements** sections, added but not yet applied to any source.

The workspace held 2 376 comment lines across 90 Rust files.
Sampling them showed the tree had two distinct comment registers, split by age rather than by crate.

The newer files — `cpu/primitives/scatter.rs`, `cpu/field/scalar.rs`, `authoring/agent_model.rs`, the `boids`/`ants` model files — were already written the way the new rules describe.
Terse, one line where one line does, non-obvious *why* only.
These needed almost nothing.

The older files were documentation prose.
`henad-core/src/grid.rs` and `params.rs` carried a doc on nearly every getter that restated its signature (`/// Returns a mutable slice of the next buffer.`), and `authoring/grid_model.rs` opened with a colon-led list of what the engine handles.
The two GPU runner files had gone furthest the other way, with `gpu/sim_thread.rs` holding a 50-line module essay and a 20-line doc on `step_batch`.

Counted before touching anything:

| rule | violations |
| --- | --- |
| em dashes in comment prose | 99 |
| semicolons or colons in comment prose | ~15 real (excluding WGSL in doc fences) |
| headless `What`/`Why`/`Whether`/`How`/`Which` docs | 29 |
| `#[must_use]` | 17 |

`AGENTS.md` says to leave pre-existing comments alone unless asked.
This session was the asking.

## What was done

### The mechanical rules

Em dashes became commas or full stops, and prose semicolons and colons were split into two sentences or rejoined with a conjunction.
Colons introducing a markdown list or a code fence were left, since those are structure rather than prose.
Headless clauses were given a subject, following the `AGENTS.md` examples: `/// What an agent kernel sees of the field.` became `/// The field as an agent kernel sees it.`, and `/// Which arm a ScatterGrid resolved to.` became `/// The arm a ScatterGrid resolved to.`

Two bool fields took the existing house form rather than a forced noun phrase, matching `Geometry::index`'s `/// Set when the model declares …`: `timestamp_query` is now `/// Set when the device granted TIMESTAMP_QUERY.`

`/// A real GPU :)` and `/// Not a GPU :(` in `runtime_info.rs` were left exactly as they are.
`AGENTS.md` cites them as the target register.

### Docs that restated their own signature

Deleted rather than reworded, since the rule is that the signature already says it.
`grid.rs` and `params.rs` took the bulk of this.
Also gone: `/// The value of a statistic entry.` on `StatValue`, `/// Kind of neighborhood for grid-based models.` on `NeighborhoodKind`, `/// Describes a simulation model …` on `Model`, `/// Send a command to the sim thread.` on `send`, and the five step-marker comments inside the native sim loop (`// Drain pending commands` above a drain loop).

Where a doc mixed restatement with one real fact, only the fact survived.
`StatsHistory::get` lost its "Returns `None` if out of bounds" half and kept the non-obvious half, that index 0 is the oldest *visible* entry.

### Design narration

Cut where `docs/agent-record` or `AGENTS.md` already carries it, per the rule that rationale belongs there and not in several files at once.

The largest was the `GpuSnapshot` doc in `snapshot.rs`, eight lines explaining why the GPU display path goes through `Snapshot` rather than `SimState::grid_view()` — the `henad-core` cannot see wgpu argument, which is settled and recorded.
The `Arc` teardown note next to it stayed, because that one is a live hazard rather than a decision.
Same treatment for `view/display.rs`'s note about the GoL spike's `render.wgsl` being already generic, and `sim_runner.rs`'s paragraph on why the two thread types were not unified.

`game_of_life.rs`'s palette doc lost "unifying the CPU palette table with the shader palette is deliberately out of scope for now", under the no-future-plans rule.

### Benchmark numbers

`cpu/primitives/chunked.rs` carried two: the closure-layer inlining regression "measured 30% on SIR", and "measured 14% on SIR" for folding the tick into `advance_tick_seed`.
Both figures are gone, both warnings stayed.
**The first one disagreed with `AGENTS.md`, which records the same regression as 48%.**
Removing the source copy leaves one number in the repo rather than two, but does not establish which was right — see below.

The `TimestampQuery::resolve_after` doc lost "failed 197/200 times", keeping the mechanism, the Metal-specific cause and the pointer to the test that covers it.

### `#[must_use]`

All 17 removed, not the 10 first proposed.
The initial pass named only the pure getters; re-checking the other seven against the rule's actual bar — *reserve it for cases where discarding the result is a plausible bug* — none of them clear it either.
`mix_seed`, `Domain::invocations`, `linear_dispatch`, `reduce_leaf` and `HashGrid::new` are pure computations with obvious names, which the rule calls out by name.

`headless_context` and `read_buffer` were the two worth arguing over, since discarding either wastes real work (a created device, a blocking GPU readback).
They went too: wasteful is not the same as buggy, and leaving seven behind would have left the tree inconsistent for the next reader.

Removal cannot break a build — `#[must_use]` only ever adds warnings at call sites — and the workspace enables neither `pedantic` nor `must_use_candidate`.

### What was deliberately not cut

The module docs on `authoring/gpu_grid_model.rs` (75 lines) and `authoring/gpu_agent_model.rs` (50 lines) both blow past "one line is the target" and were kept at length anyway.
They are the binding-convention *contract* a model author has to satisfy — which `@binding` index holds what, and the list of things nothing checks at compile time — and that is specification, not narration.
It is also recorded nowhere else.
Punctuation was fixed inside them and the structure left alone.

### Edited codebase structure

```
crates/
├── henad-core/src/
│   ├── authoring/
│   │   ├── agent_model.rs       ~ StepCtx doc
│   │   ├── field.rs             ~ module doc colon, HAS_GRID, Read
│   │   ├── gpu_agent_model.rs   ~ punctuation only, contract sections kept
│   │   ├── gpu_grid_model.rs    ~ punctuation only, contract sections kept
│   │   └── grid_model.rs        ~ trait doc rewritten, per-method docs cut (+9/-24)
│   ├── grid.rs                  ~ signature-restating getter docs deleted
│   ├── helpers.rs               ~ mix_seed doc condensed, must_use
│   ├── lib.rs                   ~ Extent re-export doc
│   ├── model.rs                 ~ Model/SimState docs
│   ├── params.rs                ~ restating docs deleted, ParamStore::set
│   ├── spatial_hash.rs          ~ SpatialHash doc, cell_index, 5x must_use
│   ├── topology.rs              ~ TopologyHint, NeighborhoodKind
│   └── view.rs                  ~ 8 restating docs deleted (+7/-18)
├── henad-compute/
│   ├── benches/scatter.rs       ~ module doc
│   └── src/
│       ├── display_scale.rs     ~ prose colon, semicolon
│       ├── runtime_info.rs      ~ timestamp_query, GpuVerdict
│       ├── snapshot.rs          ~ GpuSnapshot narration cut, em dashes
│       ├── cpu/
│       │   ├── agent_engine.rs  ~ from_seed_lanes doc
│       │   ├── grid_engine.rs   ~ from_cells, param descriptors
│       │   ├── sim_thread.rs    ~ 5 step-marker comments deleted (+9/-15)
│       │   ├── field/ca.rs      ~ from_cells doc
│       │   ├── field/scalar.rs  ~ headless clauses, duplicated COMBINE doc
│       │   └── primitives/
│       │       ├── chunked.rs   ~ two benchmark percentages removed
│       │       └── scatter.rs   ~ headless clauses
│       └── gpu/
│           ├── agent_engine.rs  ~ em dashes, semicolon, 2x must_use
│           ├── grid_engine.rs   ~ encode_steps doc, semicolons
│           ├── limits.rs        ~ module doc semicolon
│           ├── sim_thread.rs    ~ module doc 50→23, step_batch 20→13 (+50/-90)
│           ├── test_support.rs  ~ must_use
│           ├── timing.rs        ~ all four const docs condensed (+31/-54)
│           ├── primitives/      ~ dispatch, readback, reduce, spatial_hash, wgsl
│           └── view/            ~ display.rs narration cut, mod.rs headless
├── henad-models/src/
│   ├── ants/field.rs            ~ layer index doc
│   ├── game_of_life.rs          ~ palette doc, future-plans sentence removed
│   ├── registry.rs              ~ ModelFactory and model_registry docs
│   ├── sir.rs                   ~ count_sir doc
│   ├── gpu_sir/mod.rs           ~ seed_cells doc
│   └── gpu_game_of_life/
│       ├── mod.rs               ~ module doc condensed (+27/-33)
│       ├── reduce.wgsl          ~ em dashes
│       └── step.wgsl            ~ em dashes, SWAR explanation kept
├── henad-cli/src/
│   ├── main.rs                  ~ module doc, 11 em-dash sites (+39/-43)
│   └── stats_export.rs          ~ module doc condensed (+19/-21)
└── henad-app/src/
    ├── lib.rs                   ~ crate doc
    ├── main.rs                  ~ device_descriptor comment
    ├── sim_runner.rs            ~ module doc narration cut
    ├── state.rs                 ~ PointRenderMode, two semicolons
    └── ui/                      ~ agent_layer, charts, pacing, viewport
```

## State after

`./check.sh` exits 0.
`cargo fmt --check`, clippy at `-D warnings`, the wasm typecheck and `trunk build` all pass, and 14 test binaries report ok.
The suite was also run with `HENAD_REQUIRE_GPU=1` per `AGENTS.md`, with **zero skips** — the GPU tests genuinely ran rather than silently passing on a missing adapter.

| rule | before | after |
| --- | --- | --- |
| em dashes in comment prose | 99 | 0 |
| prose semicolons and colons | ~15 | 0 |
| headless `What`/`Why`/`Whether`/`How`/`Which` | 29 | 0 |
| `#[must_use]` | 17 | 0 |
| total comment lines | 2 376 | 2 232 |

The remaining grep hits for semicolons are WGSL declarations inside doc code fences in the two GPU authoring files, which are code and not prose.

No source file outside `crates/` was touched, and no behaviour changed anywhere.
The `AGENTS.md` edit that prompted the session is still uncommitted alongside this work.

## Issues found & future directions

**1. `AGENTS.md` and the source disagreed on the inlining regression, and only one survived.**
`AGENTS.md` records the `for_each_chunk_mut!`-as-a-function regression as costing 48% on SIR.
The comment in `cpu/primitives/chunked.rs` said 30% for what reads as the same measurement.
The no-benchmark-stats rule meant the comment's figure went, which leaves `AGENTS.md` as the single source — but that is a tidier repo, not a resolved question.
One of the two was wrong and nothing here establishes which.
Worth re-measuring once, since this is the number the rule against reintroducing a generic driver rests on.

**2. The GPU batching rationale still has no home in `docs/agent-record`.**
Checked before trimming: the existing records mention adaptive batching in passing (`20260812-01`, `20260813-03`, `20260813-04`) but never record *why* it is built the way it is.
Four things live only as source comments, now in condensed form:

- The controller measures **wall-clock encode-plus-submit time, not a GPU timestamp**, on the assumption that a continuously busy queue backpressures how fast `submit()` can be issued. That assumption is explicitly **not verified**. If `submit()` returns immediately regardless of queue depth, the controller regulates CPU dispatch-recording cost instead of GPU load, which is the main open risk in the design and the one thing in this file that most deserves a real write-up.
- `DEFAULT_TARGET_MS = 8.0` is half a 60fps frame.
- `ADAPTIVE_EMA_ALPHA = 0.25` reacts within a few batches while still absorbing sim-thread scheduling jitter; a single-sample estimate made the output oscillate.
- `MAX_BATCH_SIZE = 4096` exists because a cheap grid drives `target_ms / time_per_step` into tens of thousands of steps, and an oversized batch is already committed by the time a slowdown needs reacting to.

A dedicated record for the GPU runner would let all four shrink to a pointer, which is what the style rules want.

**3. The `resolve_after` Metal trap is not in the `AGENTS.md` GPU traps list.**
That list already carries the empty-pass timestamp and the oversized-submission watchdog.
It does not carry the third one: resolving a query set in the *same* command buffer as the timestamp writes is accepted by wgpu but reads a stale value on Metal, because the driver's counter sample buffer is only guaranteed populated after the writing buffer's completion handler runs.
It cost real debugging once and is a one-line addition to a section built for exactly this.

**4. Two long module docs are a standing exception.**
`gpu_grid_model.rs` and `gpu_agent_model.rs` were kept at 75 and 50 lines on the argument that a binding contract is specification.
That reading is defensible but was made here, not stated in `AGENTS.md`.
If the rule is meant to apply to them too, they are the obvious next target; if not, the exception is worth writing down so the next sweep does not relitigate it.

**5. Section-banner comments were left alone.**
`cpu/sim_thread.rs` still has `// ===== Native: threaded implementation =====` style dividers, and `helpers.rs` has `// --- Parameter descriptor builders ---`.
No rule covers them either way.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     If you update this document, stop at the line above.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
