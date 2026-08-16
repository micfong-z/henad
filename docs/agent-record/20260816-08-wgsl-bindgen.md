---
date: 2026-08-16
title: "Incorporate wgsl_bindgen: generated uniforms, shared WGSL modules, and binding resolution by name"
model: claude-opus-5 (Claude Code)
issue: "#33, under #10"
status: complete — stages 1–7 landed, one stage measured and reverted, name resolution added on top
baseline_commit: 299702c
delta_state: six commits (49fd0f3 … f8f86f1) plus an uncommitted tail
---

# Incorporating `wgsl_bindgen`

> The WGSL/Rust seam is now generated rather than hand-mirrored.
> Uniform structs, shared WGSL modules and every binding's `@group(0)` declaration come from the shaders themselves.
> No shader is assembled at runtime and no Rust file contains WGSL as a string literal.
>
> One stage went the wrong way and was reverted after measurement.
> Routing bind groups through `wgsl_bindgen`'s generated types cost +248 lines _inside the models_ and did not touch the indices it was supposed to fix.
> The replacement resolves each binding by the name its shader gives it, which deletes the declaration instead of checking it.

---

## State before

At `299702c` on `master`.
wgpu 29, egui 0.35.
`AGENTS.md` named `wgsl_bindgen` as the known fix for the WGSL/Rust seam and recorded it as not yet taken.

Every GPU model kept a hand-written Rust copy of facts only the WGSL knew.
`gpu_boids::StepParams` was 21 fields transcribed three times.
`gpu_ants::DisplayParams` spelled `_pad`/`_pad2` in both languages.
`Dims` existed verbatim in four shader files with no Rust type at all, packed as an anonymous `[u32; 4]`.
The `&[Binding]` slice per pass was positional, so entry `i` had to be `@binding(i)` by hand.
`ants/field.rs` carried `const EVAPORATION: usize = 7`, a number owned jointly by the engine, the ants model and the field layer.

`cargo fmt --check` was already failing on `master` from `299702c`, so CI was red before any of this started.

## What was done

Seven planned stages, executed one commit at a time with a hand-back after each.

### 1 — wgpu 30 / egui 0.36 (`49fd0f3`)

`wgsl_bindgen` 0.23 pins `naga ^30`, so the toolchain moved first.
`eframe`/`egui` 0.36.1, `egui_dock` 0.21.1, `egui_plot` 0.37.0, wgpu 30.0.

Three API changes: `get_mapped_range()` returns a `Result` (the two non-test call sites unmap on the error path, or the next `map_async` finds the buffer still mapped), `VertexState::buffers` takes `&[Option<VertexBufferLayout>]`, and `RequestAdapterOptions` gained `apply_limit_buckets`.
`Visuals::clip_rect_margin` is deprecated and was dropped.

The pre-existing `cargo fmt` failure was fixed here so `./check.sh` could serve as a gate for the rest.

### 2 — spike (not committed)

Gated on four questions before committing to the rest.
Generated bind groups contain `pub unsafe fn from_raw` and two `unsafe impl bytemuck` lines, which trip the workspace's `unsafe_code = "deny"`.
The user accepted a scoped exception on the grounds that this is FFI.
Lints needed `#[allow(unsafe_code, dead_code, elided_lifetimes_in_paths, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery)]` on the wrapping module — group allows alone are not enough, since most of what generated code trips is pedantic or restriction.
`RustWgslTypeMap` plus `default-features = false` keeps `glam` out.
The build-dep tree cost nothing net: the 35 packages `wgsl_bindgen` adds are exactly offset by the 35 the wgpu 30 upgrade drops.

### 3 — shared WGSL modules (`bccf9fd`)

`crates/henad-compute/src/gpu/shared/` holds `prelude.wgsl`, `dims.wgsl` and `rng.wgsl`, reached with `#import` and resolved at build time.
`henad-models`' build script reaches across with `additional_scan_dir`, so no staging and no relative `include_str!`.

Removed: five copies of `const WORKGROUP: u32 = 256u` across henad-compute's primitives, four of `Dims`, two of `pcg_hash`, and the runtime `format!("{PRELUDE}\n{shader}")` prefix.
`dispatch::WORKGROUP` now reads the generated const, so the Rust and WGSL values cannot drift.

### 4 — generated uniform structs (`9c30f38`)

Eight hand-written structs replaced, plus the two anonymous arrays.
Each swap was proven byte-identical first, by compiling size and per-field offset assertions against the old struct, then deleting the old.
Generated structs are `align(16)`/`align(8)` where the hand-written ones were `align(4)`; same size and offsets, so identical bytes, and the generated alignment is the correct one.

`Dims` could not be generated in henad-compute — nothing in that crate _uses_ the type and naga drops what nothing references, which was tested directly.
It is hand-written in `grid_engine.rs`, with henad-models (the only crate seeing both sides) asserting the two agree.

The `texel * grid / tex` arithmetic, duplicated in both grid display shaders, became `shared::dims::cell_at`.

### 5 — reduce leaf becomes a real shader (`b571d0d`)

`REDUCE_HEADER`/`REDUCE_VALUE`/`REDUCE_LANES`/`REDUCE_DOMAIN`/`REDUCE_BINDINGS` collapsed into one `ReduceSpec`.
`wgsl::reduce_leaf` and `primitives/wgsl.rs` are deleted; nothing is assembled at runtime any more.
The workgroup fold lives in `shared/reduce_tree.wgsl` as `block_sum`.
Ants' inline header carried a fourth copy of `HAS_FOOD_BIT`, so the three state-packing constants moved to `gpu_ants/state.wgsl`.

### 6 — bind groups: attempted, measured, reverted (`0ba9601`)

The plan called for routing bind groups through the generated `WgpuBindGroup0` types, retiring the `Binding` enum and taking `wgpu` into `henad-core`.
It was implemented in full, passed every test, and was then measured at the user's request:

| area               | net      |
| ------------------ | -------- |
| models (authoring) | **+114** |
| henad-core         | +134     |
| henad-compute      | −152     |
| **total**          | **+96**  |

The planning estimate had been wrong.
It weighed the engine deletion against a checker that was never written — a counterfactual — while not counting the +248 lines of `PassResources`/`GridResources` and per-model `bind_entries` that replaced the enum.
It also did not touch the buffer indices it was framed as addressing: `res.read(POS)` and `Binding::Read(POS)` use the identical const.

Half B was reverted.
What was kept is the half that was pure deletion: `prefix_scan`, `spatial_hash` and `reduce` now build their layouts from their own generated `LAYOUT_DESCRIPTOR`, which is **−29 lines** and turns `min_binding_size` on.
`henad-core` stayed dependency-free and the `Binding` enum came back.

### 7 — param indices (`f8f86f1`)

Models no longer see the composed param list.
The engines slice it and hand each impl its own, with the boundary computed from the descriptor lists rather than hard-coded.
`AgentModel::from_params` takes `extent` as an argument instead of digging it out at index 1 and 2.
`EVAPORATION: usize = 7` became `0`.

A `params!` macro pairs each declaration with its index, so the index is the position and there is nothing left to assert.
This replaced five per-model guard tests written earlier in the stage: a test the model author has to write and keep in step is authoring burden, which is the opposite of what #10 is for.

`initial_flock_matches_the_cpu_model` caught a real bug mid-stage — GPU boids and ants seed through the CPU `init`, and only the `from_params` call sites had been sliced.

### Uncommitted tail

- `params!` extended to all four GPU models; the engine's prepended param indices named once in `cpu/agent_engine.rs` and `cpu/grid_engine.rs` instead of copied into each GPU agent model.
- A `buffers!` macro in the same shape, with named flags (`const POS = "pos" double_buffered drawable;`) following `agent_lanes!`'s keyword style rather than positional booleans.
- Both macros brought in line with the [Rust API Guidelines macros page](https://rust-lang.github.io/api-guidelines/macros.html): `const` syntax with semicolons (C-EVOCATIVE), per-entry attributes (C-MACRO-ATTR), per-entry visibility (C-MACRO-VIS), and function-scope expansion covered by a test in henad-core (C-ANYWHERE). `agent_lanes!` already complied on every point.
- `gpu_sir`'s `CELL_S`/`CELL_I`/`CELL_R` now come from the shader's own `S`/`I`/`R`.
- **Binding resolution by name.** The `Binding` enum is gone from the codebase.

### Binding resolution

`build.rs` reads each shader's `@group(0)` lines and emits them as data — name, index, address space, access — into `binding_decls.rs`.
A pass declares `bindings: crate::binding_decls::bindings::GPU_ANTS_STEP` instead of a hand-written slice.

The engine walks them in `@binding` order and resolves each name.
Seven names are reserved for engine-owned resources (`params`, `dims`, `output`, `cell_start`, `sorted`, `counters`, `partials`); everything else names a buffer by its label.
**Which side it binds comes from the access mode, not from the name**, so ants' `field` being read in `step.wgsl` and read-write in `merge.wgsl` needs no suffix convention.

The build script does not parse WGSL with naga: the shaders reference imported types, so a raw parse fails, and composing them again would duplicate what `wgsl_bindgen` already does.
Everything needed is on the declaration line itself — only the _type_ needs imports resolved.
A `@binding` line that does not match the expected shape fails the build, and indices must be `0..n` with no gaps.

Grid models gained more than agent models.
They had no binding declarations before, but only because the engine hardcoded `2K + 1` interleaved pairs.
`BUFFER_COUNT: usize = 2` became `BUFFERS: &["state", "rng"]`, the arithmetic is gone and binding order is free.
Five shader identifiers were renamed to get there, all of them pre-existing inconsistencies: `gpu_game_of_life/step.wgsl` called its buffer `current`/`next` while its own reduce called it `state`, gol said `total` where sir said `totals`, and gol's step-params uniform was named `dims`, colliding with the engine's actual dims buffer.

Prior art: [blade](https://github.com/kvark/blade) resolves bindings by name against naga reflection and asserts the kind, panicking with a readable message; [Bevy's `AsBindGroup`](https://docs.rs/bevy_render/latest/bevy_render/render_resource/derive.AsBindGroup.html) takes the opposite approach with hand-written `#[uniform(0)]` indices.
blade compiles to its own backends so it can _assign_ indices; we hand WGSL to wgpu, so `@binding(n)` is fixed and only the resource filling slot _n_ is resolved.

### Edited codebase structure

```
crates/
├── henad-core/
│   ├── Cargo.toml                       ~ unchanged: still no dependencies
│   └── src/
│       ├── params.rs                    ~ params! macro, __indices!, C-ANYWHERE test
│       └── authoring/
│           ├── binding.rs               + NEW: BindingDecl, BindingKind, buffer_target, RESERVED
│           ├── mod.rs                   ~ module registration
│           ├── agent_model.rs           ~ from_params takes its own slice + extent
│           ├── field.rs                 ~ from_params takes its own slice
│           ├── grid_model.rs            ~ from_params takes its own slice
│           ├── gpu_agent_model.rs       ~ buffers! macro, ReduceSpec, specs take BindingDecl,
│           │                              Binding enum deleted
│           └── gpu_grid_model.rs        ~ BUFFERS labels replace BUFFER_COUNT, three *_BINDINGS
├── henad-compute/
│   ├── build.rs                         + NEW: wgsl_bindgen over primitives/ and shared/
│   └── src/
│       ├── lib.rs                       ~ shader_bindings module
│       ├── cpu/
│       │   ├── agent_engine.rs          ~ NUM_AGENTS/WORLD_*, AGENT_PARAM_BASE, split_params
│       │   └── grid_engine.rs           ~ GRID_WIDTH/HEIGHT, GRID_PARAM_BASE, own_params
│       └── gpu/
│           ├── capacity.rs              ~ storage_bindings + layout_entry, shared by both engines
│           ├── agent_engine.rs          ~ resolves bindings by name; PassBuilder resolvers gone
│           ├── grid_engine.rs           ~ resolves by name; Dims struct; 2K+1 arithmetic gone
│           ├── primitives/
│           │   ├── wgsl.rs              − DELETED: nothing is assembled at runtime
│           │   ├── dispatch.rs          ~ WORKGROUP reads the generated const
│           │   ├── prefix_scan.rs       ~ generated layouts
│           │   ├── reduce.rs            ~ generated layouts, generated ReduceParams
│           │   └── spatial_hash.rs      ~ generated layouts, generated HashParams
│           └── shared/
│               ├── prelude.wgsl         + NEW: WORKGROUP, linear_index
│               ├── dims.wgsl            + NEW: Dims, cell_at
│               ├── rng.wgsl             + NEW: pcg_hash
│               └── reduce_tree.wgsl     + NEW: block_sum
├── henad-models/
│   ├── build.rs                         + NEW: wgsl_bindgen + emit_binding_decls
│   └── src/
│       ├── lib.rs                       ~ shader_bindings + binding_decls modules
│       ├── sir.rs, game_of_life.rs      ~ params!, local indices
│       ├── boids/mod.rs                 ~ params!, extent argument, local indices
│       ├── ants/mod.rs, ants/field.rs   ~ params!, EVAPORATION 7 → 0
│       ├── gpu_boids/
│       │   ├── mod.rs                   ~ buffers!, generated params, decls
│       │   └── reduce.wgsl              + NEW: real shader, was an inline string
│       ├── gpu_ants/
│       │   ├── mod.rs                   ~ buffers!, generated params, decls
│       │   ├── reduce.wgsl              + NEW: real shader, was an inline string
│       │   ├── state.wgsl               + NEW: the three state-packing constants
│       │   └── step.wgsl                ~ imports prelude, rng and state
│       ├── gpu_sir/mod.rs               ~ params!, BUFFERS labels, decls, S/I/R from the shader
│       └── gpu_game_of_life/            ~ params!, BUFFERS labels, decls; step.wgsl renamed
│                                          current/next/dims → state_in/state_out/params
└── henad-app/
    ├── build.rs                         + NEW: wgsl_bindgen over ui/agents.wgsl
    └── src/
        ├── lib.rs                       ~ shader_bindings module
        ├── init.rs                      ~ deprecated clip_rect_margin dropped
        └── ui/agent_layer.rs            ~ Option<VertexBufferLayout>, generated Uniforms
```

## State after

`./check.sh` exits 0.
184 tests pass with `HENAD_REQUIRE_GPU=1`.
All four GPU models were driven in the live app after the stages that touched display paths, including Game of Life at 8192² where the 4096 texture cap makes `cell_at` non-identity.

Whole session against `299702c`: **+219 lines of code**, of which **model authoring is −118**.
Three build scripts and the `binding` module account for most of the increase.
`Cargo.lock` went 472 → 470 entries, with 35 packages added and 35 removed.

What is now generated: uniform struct layouts with offset assertions, workgroup sizes, bind group layout descriptors, composed shader sources, and every binding's name and kind.
What a model still writes by hand: buffer labels, `POS_BUFFER`/`COLOR_BUFFER`, `buffer_lens` ordering, and the WGSL itself.

Two hand-written `bytemuck::Pod` structs remain, both understood: `Dims` in `grid_engine.rs` and a test-local struct in `reduce.rs`.

## Issues found & future directions

**1. Generation only reaches what an entry point references.**
naga keeps nothing else, which is why `Dims` cannot be generated in henad-compute and why constants arriving through an `#import` do not surface as Rust consts.
Constants declared _in_ an entry point do — that is how `gpu_sir` gets `S`/`I`/`R`.
Ants' Rust-side `HAS_FOOD_BIT` still cannot derive from the bindings for this reason.

**2. Shader binding names are now load-bearing.**
Renaming one without renaming the buffer label is a construction-time panic (`no buffer labelled 'poss', wanted by 'poss_in'`), not a compile error.
That is the price of deleting the declaration rather than checking it.

**3. `HENAD_DUMP_WGSL` output is no longer the authored text.**
`EmbedSource` embeds naga's re-emission, so `gpu_boids/step.wgsl` dumps as 393 lines against 177 written, with imported symbols mangled (`linear_indexX_naga_oil_mod_X…`).
Mangling is bounded — two distinct names in that shader — but the round-trip is not.
The alternative (concatenating authored text in `build.rs`) was offered and declined in favour of a real module system.

**4. The registry's binding-need test still skips without an adapter.**
`gpu_storage_bindings_needed()` was already device-free; what needs a GPU is `all_entries()`, since the registry only lists GPU models when it has a context.
Its "count is wrong" failure mode is now impossible, leaving only "a model is missing from the list".

**5. Algorithms still written twice.**
`pcg_hash` was deduplicated, but `heading_octant`, the SWAR adder, `words_per_row` and the ants quantisation ramp are still one Rust copy and one WGSL copy tied by a doc comment.
Generation has nothing to say about these; worth its own sub-issue under #10.

**6. Palettes.**
Game of Life and SIR still bake hex colours into WGSL as hand-transcoded decimal literals. Boids and ants route theirs through the uniform.

**7. No performance measurement was taken.**
The machine was in use for unrelated GPU work through most of the session, so no number would have meant anything.
Composition moved from runtime to build time and binding resolution happens once at construction, so runtime should be unchanged — but that is reasoning, not a measurement, and an interleaved before/after on a cooled machine is still owed.

**8. A note on estimating.**
The stage 6 error is worth keeping in mind: an estimate that compares a measured cost against a benefit that was never built will favour the bigger change every time.
Counting the replacement before proposing it, not after, would have caught it.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

I suppose it is worth recording human effort from now on, for archival purposes.
The agent is good at executing a plan, but anything structural, creative or evaluative is still nearly entirely human work.

**General**

- Came up and decided on the scope of this session.
- Introduced stage 1 manually.
- The agent seems to be hesitant to revert previous changes (also including previous sessions), even if it is clearly beneficial of doing so.
  Human intervention reverted those changes.
- Reviewing each stage and the changes made line-by-line.

**Corrections**

- Stage 6 was reverted despite the agent felt good about it, as it does not understand the rationale of the change.
- Removed per-model guard tests in favour of a macro, which is shorter and safer, and reduces human authoring burden.
- Loosened the agent self-imposed constraint of henad-core being dependency-free.
- Noticing that the agent output does not match [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/about.html) for macros, and fixing it.
- Manually auditing the changes of the model authoring burden and identifying duplicate code or manual maintenance burdens.
  Suggested the `params!` and `buffers!` macros.

**Trivial**

- Approved `unsafe_code` exception for generated bind groups, as it is FFI and unavoidable.
- Suggested Clippy group-allow list to clear errors, which the agent clearly struggled.
