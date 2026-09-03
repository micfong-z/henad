---
date: 2026-08-17
title: "Authoring primitives: a paired Rust/WGSL vocabulary, a GPU parity test, and the primitives dictionary"
description: Building the paired Rust and WGSL primitive vocabulary, with a GPU parity test behind it.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: "#11, with the first slice of #13"
state: complete — six stages landed, one migration measured and reverted then reinstated on the maintainer's call
baseline_commit: 6e78a77
delta_state: five commits (46a0712 … 52ca8cf), working tree clean
---

# Authoring primitives

> Wrapping, neighbourhoods, distances and random draws now exist once each, as a Rust function and a WGSL function with the same name, pinned to each other by a GPU test rather than by a comment.
> `henad-core`'s `authoring/` splits into `model/` (the four traits) and `primitives/` (the vocabulary their kernels call).
>
> The work turned up two real defects.
> SIR could fail to infect at a rate of 1, and a closed-range unit draw made the reservoir tie-break reject the first candidate of a run.
> It also turned up a 64 to 75% speedup on boids that had nothing to do with the primitives themselves.

## State before

At `6e78a77` on `master`, immediately after #33 closed.

The same arithmetic was written out by hand in several places, in two languages, with a `// Mirrors X` comment as the only thing keeping the copies in step.
The toroidal delta had three spellings, the wrapped index three, the unit float draw four.
`heading_octant` and `deposit_value` each existed twice, once per language.

Issue #10 (a Rust to WGSL generator) was already deferred to v0.3.0, and the WGSL half of #11 was assumed to be blocked behind it.
It was not.
Cross-crate `#import` already worked with no new machinery, since `shared::rng` was being imported by two model shaders without even being a `build.rs` entry point.

`docs/` was entirely gitignored apart from `agent-record/`, so there was nowhere for a user-facing document to live.

## What was done

Six stages, one commit each, handed back between them.

### 1 — Doc style, chosen before anything was written

Three styles were drafted for the same two primitives and the maintainer picked std structure plus a trailing "See also" line.
The deciding argument was that std's `# Examples` are assertions the build checks, where the NetLogo dictionary's caveats are prose a reader has to trust, and `cargo test --workspace --doc` already runs in `check.sh`.

`authoring/primitives/` is therefore a deliberate exception to the terse house comment style.
It is a public authoring API and carries contract-level documentation.

A naming constraint came out of the drafting: `from` and `target` are in naga's `RESERVED` list and are parse errors as WGSL identifiers, so a delta takes `a` and `b`.
`step` is only a `BUILTIN_IDENTIFIER`, which is why `gpu_ants/display.wgsl` can shadow it.

### 2 — The dictionary, written before the code

`docs/authoring/primitives.md` maps the NetLogo dictionary onto Henad with four statuses, and a reason on every row that is not shipped.
Written first so its coverage map fixed the scope of stages 4 and 5 rather than describing them afterwards.

`.gitignore` gained `!/docs/authoring/`, mirroring the existing `!/docs/agent-record/`.
The private notes under `docs/` stay ignored.

NetLogo is the coverage checklist, not the semantic model.
`ask`, agentsets and `with` are rejected with the reason recorded, since they are the direct cause of the scaling ceiling Henad exists to clear.

### 3 — `authoring/` split in two

```
crates/henad-core/src/authoring/
├── mod.rs
├── model/                  ← the six trait files, moved unchanged
│   ├── agent_model.rs
│   ├── binding.rs
│   ├── field.rs
│   ├── gpu_agent_model.rs
│   ├── gpu_grid_model.rs
│   └── grid_model.rs
└── primitives/             ← new
    ├── mod.rs
    ├── space.rs            ↔ henad-compute/src/gpu/shared/space.wgsl
    └── rng.rs              ↔ henad-compute/src/gpu/shared/rng.wgsl
```

All six moved as git renames.
Twenty call sites and the `BindingDecl` path inside `henad-models/build.rs`'s generated string were rewritten.
AGENTS.md was updated in the same commit.

The new directory was called `std/` first.
That was wrong: inside `authoring/mod.rs` a bare `std::` resolves to the child module, and the doc link needed `[`self::std`]` to disambiguate.
`primitives/` collides with `cpu/primitives/` and `gpu/primitives/`, which AGENTS.md's counterpart rule would normally forbid, so that rule gained a line resolving it — the two engine directories are counterparts of each other, while `authoring/primitives/` is the Rust half of a cross-language pair with `gpu/shared/`.

### 4 — `space`, and the parity harness

`Boundary`, `wrap_index`, `wrap_coord`, `cell_index`, `offset_cell`, `axis_delta`, `dist_sq`, three offset tables, `offsets`, `for_each_neighbor`, `heading_octant`.

WGSL has no `Option`, so `offset_cell` returns `vec3<i32>` with `.z` as a validity flag.
That is now the convention for any fallible primitive.

Two Moore tables ship, not one.
`MOORE_ROW_MAJOR` is the order `GridModel::step_cell` receives its slice in, which is published API.
`MOORE_COLUMN_MAJOR` is the `dx`-outer order ants needs, because its ties are broken by a reservoir draw and the visit order changes the result.

`gpu/shared/parity.wgsl` runs every primitive over caller-supplied cases in one dispatch, switching on an op code, and `gpu/tests/parity.rs` compares 4282 cases against the Rust twins.
The op codes and the boundary and table codes come from the generated bindings, so nothing in the test can drift from the WGSL.
The harness was verified by injecting faults rather than assumed to work.

Float parity is not uniform.
Integer results and `random_float` must match exactly, and the rest are compared within `1e-4`, because WGSL's float `%` is defined through a division where Rust's is an exact fmod.
A blanket tolerance was tried first and rejected, since it hid a 1e-5 drift in a primitive the docs claimed was bit-equal.

### 5 — `rng`

`next_bits`, `random_float`, `next_float`, `below`, `choice3`, `reservoir_accept`, plus `xorshift64` and `mix_seed` moved out of `helpers.rs`.

The first draft had `unit(word) = word / u32::MAX`, and it was wrong.
`u32::MAX as f32` rounds up to 2^32, so `unit(u32::MAX)` was exactly `1.0`, and `reservoir_accept(bits, 1)` computed `1.0 < 1.0` and rejected the first candidate of every tie run.
Adopting NetLogo's half-open `random-float` fixes it: the top 24 bits over a power of two is exact, genuinely half-open, and more precise than the 32-bit form, which was discarding those bits in rounding anyway.
Ants' original `(rng >> 40) as f32 / 16_777_216.0` had been this all along.

Splitting an advance from a pure draw is what makes the draws parity-testable, and it also makes it possible to feed one word to two draws.
That is silent when it happens, so the module doc says every draw needs its own `next_bits`.

### 6 — Models onto the primitives

`game_of_life.rs`, `sir.rs`, `boids/`, `ants/`, `spatial_hash.rs`, and the SIR, boids and ants shaders.
`gpu_game_of_life/step.wgsl` was left alone, since its SWAR kernel has no neighbour loop to extract.

Both grid models' `init` and all of ants came out bit-identical, which was checked by hand rather than hoped for.
`below(next_bits(rng), threshold)` is exactly the old expression, and `random_float(next_bits(rng), 1.0)` reproduces ants' old `>> 40 / 2^24` exactly.
SIR's step draw changed, since it used a different bit window.
Its consistency test is statistical and the GPU side runs a different generator by design.

Two probe models in `cpu/field/ca.rs` now assert that the engine's gather order really is `MOORE_ROW_MAJOR` and `VON_NEUMANN`.
That correspondence is published in two places and nothing checked it.

## State after

Five commits, `46a0712` through `52ca8cf`, working tree clean.
`./check.sh` passes, as does the full suite under `HENAD_REQUIRE_GPU=1`.
The dictionary has no `planned` rows left.

### Measured

All figures from order-alternated interleaved runs of two binaries differing in one thing.
Cross-session comparison was tried first and produced three false results in a row, including a 20% ants "regression" that was a thermally throttled reading, and a Game of Life "improvement" on a model this work never touched.

| Change                               | Effect on the model        |
| ------------------------------------ | -------------------------- |
| `query_radius` onto `dist_sq`        | boids 64 to 75% **faster** |
| `axis_delta` in the boids inner loop | boids about 7% slower      |
| `offset_cell` in the ants 3x3 walks  | ants about 1% slower       |

The first is the interesting one.
`query_radius` was calling `rem_euclid` twice per neighbour candidate, and `rem_euclid` on `f32` is a libm `fmodf`.
Replacing it with two compares is worth more than everything the primitives cost put together.
The size of that number warrants an independent check before it is quoted anywhere public.

`axis_delta` reached its final shape through two rejected ones.
The total fmod form cost boids 17%.
A branchy fast path recovered most of it but had its two guard conditions swapped, which the totality test caught.
The shipped form drops totality for a `debug_assert`ed precondition, positions within one world of each other, which every position the engine produces satisfies.

## Issues found & future directions

**Two defects, both pre-existing, both surfaced by the migration rather than by a test.**
SIR compared `draw > prob_safe`, so a zero draw escaped infection at a rate of 1.
Now `>=`, on both backends.
The unit-draw range bug is described under stage 5.

**The `+70%` boids number deserves independent confirmation.**
It is isolated to a one-line change and the reference fixtures still pass, but a favourable result of that size has been wrong here before.

**`SpatialHash::build` is still sequential** and now stands out more, since the query it feeds got much cheaper.

**Deferred primitives are listed in the dictionary with reasons**, chiefly true `distance`, headings in radians, `in-cone`, and the non-uniform random distributions.
Each waits for a second caller.

**#10 is better positioned than it was.**
The paired vocabulary is the mapping table a Rust to WGSL generator needs, and the parity harness is how such a generator's output would be checked.
Nothing about #10's design was decided here.

**`henad-models`'s `tests/support.rs` and `henad-compute`'s look like duplicates and must not be merged.**
The first uses baseline device limits so `every_gpu_model_builds_on_a_baseline_device` can catch a model outgrowing the WebGPU floor, and the second raises them.
This is now recorded in AGENTS.md.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

Human intervention in this session:

- Opened the session and guided the plan.
- Deciding on docstring formats.
- Rewrote parts of `primitives.md` generated.
- Proposed renaming `std/` to `primitives/`.
- Moved files and reorganised codebase.
- Renamed identifiers in `parity.rs` directly.
- Noticed logical convolution in the `rng` primitives and applied fixes, including strange choice of identifies and functions that have subtle rationales.
  Re-designed functions and primitive lists.
- Added further clarification in docs.
- Overrode the agent's performance-driven decision to keep a hand-written delta in the boids model, and instead used primitives for readability.
