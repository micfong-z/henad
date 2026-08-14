---
date: 2026-08-13
title: "GPU agent models, part 3 — the GpuAgentModel extraction"
model: Claude Opus 5 (claude-opus-5), via Claude Code
issue: 8 — GPU implementation of agent models
status: complete — issue 8 is done
baseline_commit: 52384d4
delta_state: uncommitted working tree on `8-gpu-agent-models`
---

# GPU agent models, part 3 — the `GpuAgentModel` extraction

> The third and last part of issue 8.
> `gpu_boids` and `gpu_ants` were two hand-written `GpuSimState` impls; they are now two `GpuAgentModel` impls over a shared `gpu/agent_engine.rs`, and they register through one generic function instead of two bespoke ones.
>
> The concrete-before-abstract sequencing paid off in a specific way: the trait's `Domain` enum has exactly three variants because those are the three the two models actually use, and the pass list is a list precisely because ants needs two passes and boids needs one plus an index.
> A trait designed against boids alone would have baked in ping-pong and a neighbour index, both of which ants has neither of.
>
> **Behaviour did not move.** `gpu_ants` replays bit-identically, and its buffer hashes at ticks 0, 100 and 300 are unchanged from the pre-refactor build. Throughput is unchanged on both models, measured interleaved against a binary built from `52384d4`.
>
> Two contracts that were previously unverified for **every** GPU model are now tested, and "runs on a stock WebGPU device" is now an assertion rather than an argument.

---

## State before

At `52384d4`, on branch `8-gpu-agent-models`.

Both GPU agent models existed, were registered, and were green.
Both were hand-written: `gpu_boids/mod.rs` was 799 lines (545 before its tests) and `gpu_ants/mod.rs` was 880 (597), each constructing every wgpu object inline and implementing all five `GpuSimState` methods plus six `SimState` ones.

Three things followed from that, and they are what this session was for.

**They were the only non-generic registrations.**
CPU grid, CPU agent and GPU grid all went through a `register_*<M>()` generic; `register_gpu_boids` and `register_gpu_ants` were hardcoded closures with a hand-written `TopologyHint` literal, fed by free `NAME`/`ID`/`DESCRIPTION`/`param_descriptors()`/`stat_descriptors()` items that existed only to serve them.

**The registry's invariant tests skipped every GPU entry.**
All three ran `model_registry(None)` and opened with `let ModelState::Cpu(..) else { continue }`, so the declared-apply-mode, declared-topology and stat-arity contracts were unchecked for `gpu_boids`, `gpu_ants`, `gpu_game_of_life` and `gpu_sir` alike.

**The binding budget was uncountable.**
A model over `max_storage_buffers_per_shader_stage` failed as a fatal wgpu validation panic on the UI thread at Build time. Nothing could state, let alone test, that a model fit the WebGPU baseline of 8.

The precedent was `GpuGridModel` (`henad-core/src/authoring/gpu_grid_model.rs`, 139 lines) with `gpu/grid_engine.rs` (525) — extracted only after Game of Life and SIR both existed, for the same reason.

---

## What was done

### Decisions taken first

Three were put to the user before any code, because each changes the shape of the trait.

| Decision | Choice | Why |
| --- | --- | --- |
| CPU coupling | **None.** The trait never names `AgentModel` | The human note on the ants hand-off flagged unease about `gpu_ants` depending on the CPU model. It still does, but now only inside `seed_buffers`, so dropping it later is one function per model rather than a rewrite |
| Pass encoding | **Declarative list.** The model declares buffers, passes, bindings and domains as plain data | Lets the engine own the index rebuild, the ping-pong and the timestamps, and makes the binding count countable |
| Shared WGSL | **Splice.** The reduce leaf is generated; step shaders get a prepended prelude | Removes ~130 duplicated WGSL lines. The debuggability cost is paid for by `HENAD_DUMP_WGSL` |

### Codebase structure after this session

New files marked **+**, modified **~**, deleted **−**.

```
crates/
├── henad-core/src/
│   ├── spatial_hash.rs                  ~ HashGrid moved here from henad-compute, next to its
│   │                                      CPU twin — the trait's Geometry carries it, and core
│   │                                      has no wgpu to reach the old home
│   └── authoring/
│       ├── mod.rs                       ~ + pub mod gpu_agent_model
│       └── gpu_agent_model.rs           + the trait (219 lines): BufferSpec, Binding, Domain,
│                                          PassSpec, DisplaySpec, PassId, Geometry, PassCtx
│
├── henad-compute/src/gpu/
│   ├── mod.rs                           ~ module wiring + re-exports
│   ├── agent_engine.rs                  + GpuAgentState<M> + GpuAgentModelDescriptor<M> (724)
│   └── primitives/
│       ├── mod.rs                       ~ + pub mod wgsl
│       ├── spatial_hash.rs              ~ re-exports HashGrid rather than defining it
│       ├── pipeline.rs                  ~ compute_pipeline dumps to $HENAD_DUMP_WGSL
│       └── wgsl.rs                      + PRELUDE + reduce_leaf(header, value) (62)
│
└── henad-models/src/
    ├── registry.rs                      ~ register_gpu_agent_model::<M>() replaces the two
    │                                      bespoke functions; three tests now cover GPU entries,
    │                                      plus two new ones
    ├── gpu_boids/
    │   ├── mod.rs                        ~ 799 → 464 lines (545 → 267 before tests)
    │   ├── step.wgsl                     ~ hand-rolled fold → linear_index
    │   └── reduce.wgsl                   − generated from REDUCE_HEADER + REDUCE_VALUE
    └── gpu_ants/
        ├── mod.rs                        ~ 880 → 612 lines (597 → 385 before tests)
        ├── step.wgsl                     ~ hand-rolled fold → linear_index
        ├── merge.wgsl                    ~ same
        └── reduce.wgsl                   − same as boids
```

### The trait's shape came from the disagreements, not the agreements

The two models share their boilerplate and disagree about their structure, so the trait is built around the disagreements.

`BUFFERS` is a list of `{label, double_buffered, drawable}` because boids ping-pongs all three of its lanes and ants ping-pongs none of its seven.
The engine builds a `b` side, a second bind group per pass and a second `GpuAgents` handle **only when some buffer asks for it**, and does not flip `current_is_a` at all when nothing does — so ants pays nothing for boids' double buffering.

`STEP_PASSES` is a list because ants runs two passes and boids runs one, and `INDEX` is a bool because boids has a neighbour index and ants has none.

`Domain` has exactly three variants — `Agents`, `Cells(n)`, `AgentsOrCells` — because those are the three the two models use: boids' step and reduce are per agent, ants' merge is two per cell, ants' reduce spans both and covers the longer.
Nothing speculative was added.

Bindings are positional: a pass's `&[Binding]` slice index *is* its `@group(0) @binding(i)`. That replaces a naming convention with an ordering, and it is what lets the engine count a pass's storage buffers.

### What the engine now owns, that neither model did correctly

- **Timestamp placement.** One implementation instead of two subtly different hand-rolled ones. The opening stamp lands on the index rebuild's counting pass when there is an index, and on the first declared pass when there is not — a stamp on an empty pass is silently never written, which cost real debugging in part 1.
- **The 64-step submission batching**, as `run_batched`. One oversized submission trips the OS GPU watchdog and every later readback returns zeros with no error; that rule was previously re-derived in each model's test module.
- **`heap_bytes` over everything.** Buffers, index, reduce, display texture and counters. `gpu_ants` previously omitted its display texture and its delivery counter, so its reported Sim memory has gone up by about 162 KB at the default 201² — the old number was wrong, not the new one.
- **The binding budget**, asserted per pass against `device.limits().max_storage_buffers_per_shader_stage` with a message naming the model and the pass.

### Splicing WGSL, and paying for it

`gpu_boids/reduce.wgsl` and `gpu_ants/reduce.wgsl` were ~45 lines of identical workgroup-reduction skeleton each, differing only in how `value` is computed.
Both are gone; `wgsl::reduce_leaf(header, value)` generates them from the model's own binding declarations and its value expression.
The linear-dispatch fold, byte-identical across three shaders, is now `linear_index` in a prelude that is **prepended, never interleaved**, so a compile error's line number is off by a constant rather than scrambled.

The cost is real and was mitigated rather than waved away: `compute_pipeline` writes every shader it compiles — generated or not — to `$HENAD_DUMP_WGSL/<model_id>_<pass>.wgsl` when that variable is set.
This was not theoretical. The first build of the spliced leaf failed with `redefinition of WORKGROUP` (the leaf generator included the prelude that the engine also prepends), and the error pointed at generated lines 2 and 30, which was enough to fix it immediately.

### Registry contracts that GPU models now have to satisfy

`declared_apply_mode_matches_what_the_state_accepts` and `every_declared_stat_series_gets_a_value` now iterate GPU entries too, via a `sim_state(&mut ModelState) -> &mut dyn SimState` helper that upcasts both arms.

`declared_topology_matches_the_views_the_state_returns` genuinely cannot cover them — a GPU state publishes through `GpuSnapshot`, not `grid_view`/`point_view`, and returns `None` from both by design.
It gained a sibling, `declared_topology_matches_the_layers_a_gpu_state_publishes`, which asserts the hint against what `view()` actually carries. That is the invariant that matters, and `topology_hint` is now *derived* from `M::DISPLAY.is_some()` rather than written by hand, so the two can only disagree if the display pass and the snapshot do.

`every_gpu_model_builds_on_a_baseline_device` builds all four GPU entries on a `Limits::default()` device.
Since the engine asserts each pass against that device's own limit of 8, this is a direct test of "every model runs on a stock WebGPU device" — the invariant part 2 suggested making explicit.

### Verification

- **`gpu_ants` reproduces bit-identically.** Buffer hashes (FNV-1a over `pos`, `state` and the whole `field`) recorded on `52384d4` at ticks 0, 100 and 300, then compared after the rewrite:

  | tick | `pos` | `state` | `field` |
  | --- | --- | --- | --- |
  | 0 | `bd4f8cbe281ff425` | `69c2b4238ac44405` | `9b85a68c78294d25` |
  | 100 | `43a1f87b2ca0f425` | `9da6378ff4f5c58a` | `f1b7f54075bbfc7f` |
  | 300 | `ef05ac6a0d70f425` | `c0cfe0f5bdc19979` | `b7e2356e70129a98` |

  All nine match exactly, as does total pheromone (338.1441955566406).
  This is the check the existing `a_run_replays_bit_identically` cannot make: that test compares two runs of the *same* build, so a consistent behaviour change would pass it.

- **`gpu_boids` tick 0 is exact** (lane sums unchanged to six decimals). Later ticks are not comparable — boids does not replay — so its own run-to-run spread was measured instead: mean speed at tick 300 lands in 3.82–3.89 across three runs, and mean velocity x varies fourfold. The pre-vs-post gap sits inside that.
- **58 tests in `henad-models`, 52 in `henad-compute`**, all passing with `HENAD_REQUIRE_GPU=1`. Every assertion from both models' suites survived, including the deliberately-chosen 800-tick flocking and 1500-tick trail horizons.
- **`./check.sh` green**, including the wasm typecheck and `trunk build`.
- **Driven in the live app** via the egui MCP. `gpu_boids` at 50k: tick 1609, TPS 352, agent layer only, mean velocity 5.4 against mean speed 6. `gpu_ants` at tick 18075: both layers rendering, a consolidated two-lane highway around both obstacle blobs, 64 834 deliveries, all three stats reporting.

### Measured

Release `henad-cli`, flat-out `step()` loop. Old and new binaries built from `52384d4` and the working tree and run **interleaved**, twice each, since this machine drifts under sustained load.

| | old | new | old | new |
| --- | --- | --- | --- | --- |
| `gpu_boids`, 50k / 1000² | 301.3 | 305.4 | 303.8 | 299.9 |
| `gpu_ants`, 50k / 500² | 8 991 | 9 500 | 9 368 | 9 418 |

Unchanged on both, within a spread of about 1.5%.
Expected: the encoded pass sequence is identical, and the only per-step difference is iterating a `Vec<EncodedPass>` rather than straight-line code.

### What the extraction actually bought

Worth stating plainly, because the line count does not tell the story.

The two models lost 603 lines between them (490 before tests), and 130 lines of WGSL disappeared.
The three new shared files add 1 005, so **total workspace lines went up by about 330.**

The payoff is elsewhere: a third GPU agent model now costs a buffer list, a shader, a uniform and a stat mapping instead of ~550 lines of wgpu construction; registration and `topology_hint` are derived rather than written; and three contracts that were silently unchecked for every GPU model are now tested.
That is the same argument that justified `GpuGridModel`, and it should be judged on that rather than on a diff stat.

---

## State after

**Issue 8 is complete.** Both GPU agent models exist, and the trait they were meant to produce exists and is what they are built on.

The four registration paths are now uniform: `register_grid_model<M>`, `register_agent_model<A>`, `register_gpu_grid_model<M>` and `register_gpu_agent_model<M>`, each generic over its authoring trait, each deriving its metadata and topology from it.

Every divergence the two models declared in parts 1 and 2 survived deliberately:

- boids still does not replay run to run, for the same reason (the counting sort's within-cell order is arbitrary and `f32` addition is not associative),
- ants still uses a per-agent self-advancing `pcg_hash` rather than the CPU's per-chunk `xorshift64`, still carries `reward` as one bit, and still quantises its display with `log2 * 0.30103`,
- the krABMaga tie-break defect in `ants/step.rs` is untouched, including its two explanatory comments.

Both models still seed through their CPU counterpart's `init`, which is what keeps tick 0 bit-identical — but that call now lives in exactly one function per model (`seed_buffers`), which is the part of the human note from part 2 that this session could act on.

---

## Issues found & future directions

### 1. `limits::raise(16)` is now demonstrably unnecessary, and there is a test to prove it

:human: This is tracked in https://github.com/micfong-z/henad/issues/30.

Part 1 raised `max_storage_buffers_per_shader_stage` to 16 as an estimate for ants; part 2 found ants needs exactly 8.
`every_gpu_model_builds_on_a_baseline_device` now asserts that every registered GPU model builds on a `Limits::default()` device, so the raise is unused headroom.

Not removed here, because it is a decision about future models rather than a fact about current ones.
Removing it would make "fits the WebGPU baseline" a hard invariant; keeping it means a model can exceed the baseline and only fail on devices that cannot raise. Either is defensible; the test makes the choice explicit rather than accidental.

### 2. The WGSL/Rust binding correspondence is still hand-maintained, and the trait moved the seam

:human: See https://github.com/micfong-z/henad/issues/10.

The positional `&[Binding]` slice is a genuine improvement over the old scattered `wgpu::BindGroupEntry { binding: 7, .. }` literals — bindings are now declared in one list per pass, next to the shader they belong to.
But the *types* are still unchecked: nothing verifies that `Binding::Read(VEL)` corresponds to `array<vec2<f32>>` in the shader, nor that `StepParams`'s 21 fields match the WGSL `Params` struct's layout.
`wgsl_bindgen` remains the real fix, and the trait would now be a natural place to hang it, since `BufferSpec` could carry the WGSL element type.

### 3. `HENAD_DUMP_WGSL` should probably be documented for humans

:human: Will do in https://github.com/micfong-z/henad/issues/13.

It is currently only mentioned in the trait's module docs. It is the thing that makes a generated-shader validation error tractable, so it belongs in `AGENTS.md`'s command list too.

### 4. Follow-ups not taken, carried forward from parts 1 and 2

- **The first TPS sample after Play is garbage for every GPU model** (`gpu/sim_thread.rs` does not reset `last_stats_publish` on `Play`). Still unfixed — shared runner code, not this change's bug.
- **`SpatialHash::new` still duplicates `HashGrid::new`'s fitting math.** The type moved into `henad-core` next to it this session, so rewiring one to use the other is now a local change. Deliberately not bundled, since it touches the CPU hot path.
- **Three copies of headless device acquisition** still exist, and `henad-models`'s copy still omits the `limits::raise` that `henad-compute`'s applies. That difference is now load-bearing in a good way — it is what makes the baseline test a baseline test — so it should be documented rather than consolidated away.
- **`world_width` still defaults to 201 in the GUI and 200 from the CLI**, so a GUI-vs-CLI comparison is not over the same field size.

### 5. A shape question the next model will answer

The engine binds `Binding::Read(k)` and `Binding::Write(k)` to the same buffer when `k` is not double buffered, which means a model cannot declare both for one in-place buffer — wgpu would see the same buffer bound read-only and read-write in one bind group.
Neither current model wants to, and the trait documents "a buffer written in place declares `Write` alone".
If a third model does want it, the answer is probably to make that combination an explicit error at construction rather than a wgpu validation surprise.

---

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     If you update this document, stop at the line above.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

Looks fair enough, though some naming are questionable and comments are still terse.

Most code that are duplicated are between WGSL and Rust, which we really need to fix with `wgsl_bindgen` or similar.
