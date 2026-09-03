---
date: 2026-08-14
title: "GPU grid size limits — raising the device limits and decoupling display from grid"
description: Raising the device limits past the WebGPU baseline, and decoupling the display texture from the grid.
icon: material/note-text-outline
status: ai-generated
model: Claude Opus 5 (claude-opus-5), via Claude Code
issue: 9 — GPU grid size limit fixes, and 30 — auto-detect and request higher wgpu Device limits
state: complete — both issues are addressed, with one caveat named under "Issues found"
baseline_commit: 3489999
delta_state: uncommitted working tree on `master`
---

# GPU grid size limits

> Two walls stopped a GPU model growing, and neither was the hardware.
> `Limits::default()` caps a storage binding at 128 MiB and a texture side at 8192 where the M4 Pro offers 4 GiB and 16384, and `limits::raise` was only asking for more *storage buffers*, never more *bytes*.
> The display texture was one texel per cell, which is both an absolute cap at the texture limit and 4 bytes of VRAM per cell — 1.07 GB at 16384² for something drawn into a ~1000 px panel.
>
> Both are gone.
> The three size limits now go to whatever the adapter reports, and the display texture is a **sampled view** of the grid rather than a mirror of it: capped at 4096 a side, with the display pass dispatching per texel and reading the cell at `texel * grid / tex`.
> `gpu_game_of_life` now runs at 16384² (268 M cells) where it used to panic at 9000, and `gpu_sir` at 16384² where it used to panic at 6000.
>
> Issue 30 closed with it.
> `gpu/capacity.rs` derives a model's buffer sizes, texture dimensions and per-pass storage-binding counts from what it already declares, so the app can grey out Build with a sentence naming the buffer or the pass, instead of handing wgpu something it rejects — which arrives as a panic on the UI thread, not an error return.
> The binding count is the limit that filed issue 30, and it was the last thing still failing the fatal way.
>
> **Nothing that already fitted changed.** Below the cap the display maps `texel * w / w = texel`, so every existing model's picture and every stat is identical, and the step path was not touched at all.

---

## State before

At `3489999`, on `master`, clean.

Two failures, both reproduced with the release CLI before any change:

```
$ henad-cli gpu_game_of_life --steps 5 --set grid_width=9000 --set grid_height=9000
wgpu error: Validation Error
  In Device::create_texture, label = 'henad_gpu_display_texture'
    Dimension X value 9000 exceeds the limit of 8192

$ henad-cli gpu_sir --steps 3 --set grid_width=6000 --set grid_height=6000
wgpu error: Validation Error
  In Device::create_bind_group, label = 'gpu_sir_bind_a2b'
    Buffer binding 0 range 144000000 exceeds `max_*_buffer_binding_size` limit 134217728
```

Both `GpuGridModel` implementations declare grid width and height with a maximum of 16 384, so the UI already let a user ask for something that panicked the process at Build.

The adapter was never the constraint.
Probed on this machine:

| limit | `Limits::default()` | M4 Pro reports | `limits::raise` asked for |
| --- | --- | --- | --- |
| `max_texture_dimension_2d` | 8 192 | 16 384 | untouched |
| `max_storage_buffer_binding_size` | 128 MiB | 4 GiB | untouched |
| `max_buffer_size` | 256 MiB | 14.3 GB | untouched |
| `max_storage_buffers_per_shader_stage` | 8 | 31 | 16 (unused headroom, see below) |

The third fact that shaped the fix: **the display texture was the dominant memory cost, not the state.**
At 16384² a bit-packed `gpu_game_of_life` holds 33.5 MB per side; its RGBA display texture would have been 1.07 GB.
Since the viewport has no zoom or pan and fits the whole grid into a panel, every one of those texels past the panel's width was thrown away by the sampler anyway.

The same wall existed on the CPU display path, reached through egui's `ColorImage` upload rather than a compute pass — CPU grid models declare a maximum of 10 000 a side, also past 8192.

## What was done

Five pieces, in dependency order. The fifth is a UI bug the user reported partway through, which the fourth had made worse.

### 1. `limits::raise` raises sizes, and now *only* sizes

`max_texture_dimension_2d`, `max_storage_buffer_binding_size` and `max_buffer_size` go to whatever the adapter reports.
How big a run can be is a property of the hardware however we ask, so there is nothing to gain by pinning it to a constant.
That single change is what moves `gpu_sir` from 33.5 M cells per binding to 1.07 G.

`STORAGE_BUFFERS_PER_STAGE = 16` is **deleted**. The count is now derived, not guessed.

It was aspirational when issue 8 added it and never became anything else.
`every_gpu_model_builds_on_a_baseline_device` builds every GPU entry on a `Limits::default()` device, so no model may exceed 8 storage buffers per pass whatever we grant — 16 was dead headroom guarding a door a test already welds shut.

In its place, `raise` takes the count as an argument and the host supplies `registry::gpu_storage_bindings_needed()`, which is the max over every model's declared passes.
Today that is 8: `gpu_ants`'s step pass binds exactly 8, `gpu_boids` 7, `gpu_sir` 4.
The layering forces the argument — henad-compute is below henad-models and cannot enumerate them, and a host needs the number *before* it has a device to build a registry with. Since every pass's binding list is a const, no device is needed to compute it.

Each engine grew `declared_passes()`, which both `demand` and `max_storage_bindings` read, so the width a host asks for and the shortfall the UI reports cannot disagree.
The hand-written model list in `gpu_storage_bindings_needed` is the one thing that can drift from `model_registry`, and `the_declared_binding_need_matches_the_registry` makes that a test failure — verified by mutation: dropping `gpu_ants` from the list fails it.

wgpu's own documentation argues the same way, and more generally than I had assumed:

> Requesting limits that are "better" than you need may cause performance to decrease because the implementation needs to support more than is needed. You should ideally only request exactly what you need.

That reads as an argument against maxing the *count*, where the backend may pick a different descriptor-table or argument-buffer layout.
I take it as much weaker for the *sizes*, which are validation thresholds rather than a switch between binding models — but that is reasoning, not measurement, and the benchmarks in this session were not designed to detect it.

The practical gain is that the app's device and the portability gate now agree on one number.
Before this, the app ran on 16 and every model was tested against 8; the difference was invisible only because nothing ever used it.

### 2. The display texture is a sampled view

New `henad-compute/src/display_scale.rs`, above both backends since both draw through it:

```rust
pub const MAX_DISPLAY_DIM: u32 = 4096;
pub fn display_dims(width: u32, height: u32, device_max: u32) -> (u32, u32);
```

Each axis is capped on its own, because the viewport fits the rect to the **grid** aspect and samples the texture across it — so a non-square texel draws correctly, and a 12000 × 3000 grid keeps full detail on its short axis (4096 × 3000) rather than being squared down.

The WGSL contract for `GpuGridModel` changed accordingly.
The shared display/reduce uniform went from `vec2<u32> dims` to

```wgsl
struct Dims {
    grid: vec2<u32>,
    tex: vec2<u32>,
}
```

and a display shader now bounds itself by `dims.tex`, reading the cell at `global_id * dims.grid / dims.tex`.
Reduce still dispatches one invocation per cell and reads `dims.grid`.
For `GpuAgentModel` the same information rides in `Geometry::display`, which `gpu_ants` copies into its own `DisplayParams`.

Below the cap this is arithmetically the identity (`texel * w / w == texel`), which is why no existing model's output moved.

`viewport.rs` does the same thing on the CPU path, in `sample_grid`. The 1:1 case keeps the old flat `par_iter` expansion untouched, so the common path did not change shape.

### 3. Capacity, asked before anything is allocated

New `henad-compute/src/gpu/capacity.rs`.
`Demand` is a list of labelled allocations, the (already capped) display texture, and a per-pass storage-binding count; `Demand::shortfalls(&Limits)` returns a sentence per distinct over-budget size and one per over-bound pass.
Nothing new is declared by a model to make this work — the demand is derived from `buffer_lens`, `dims`/`Geometry`, `BUFFERS` and the `&[Binding]` slices, which every model already spells out.

The binding count is the limit that filed issue 30 in the first place, and it is a different kind of failure from the sizes: no amount of shrinking the grid fixes a pass that binds too many buffers, so it says so separately.
It also subsumes `PassBuilder::check_budget`, which only ran during construction and only for agent models — `grid_engine` had no such check at all, so a `GpuGridModel` with a high `BUFFER_COUNT` would have hit raw wgpu validation.
`check_budget` is deleted; both engines now go through `shortfalls`.

Both engines gained `demand(params, limits)` and assert on it before their first `create_buffer`, so the backstop message names the model's own buffer.
`ModelEntry` gained `capacity: Option<CapacityFn>` (`None` for a CPU model, which has no device limit to miss) and `ModelEntry::shortfalls`, which is what the app and the CLI call.

Sides of a ping-ponged buffer are always the same size, so a line per buffer said the same sentence four times for `gpu_sir`. They are grouped by size, and the message reads:

> 'gpu_sir_buffer0_a' and 3 more each need 137.3 MB, past the 128.0 MB this device allows for one storage binding

### 4. The UI says so

- **Parameters panel** shows an error banner with the shortfalls, ahead of the existing reload notices since it is the one that stops Build working at all. It is drawn as a **footer**, under the widgets — see the note below.
- **Build button** is disabled while any shortfall stands, with the reason on hover.
- **System tab** gained a *Device limits* section: granted against available for the six limits that bound model size, plus the display texture cap. A gap between the two columns is exactly where Henad asks for less than the hardware would give — which is now only the storage-buffer count (16 of 31) and invocations per workgroup (256 of 1024).
- **CLI** refuses with an error before calling the factory, and `--info` prints the same limits.

`fmt_bytes` moved from `henad-app/src/ui/mod.rs` to `henad-core::helpers` so `capacity.rs` could use it.

### 5. The parameter notices moved to the bottom of the panel

Reported by the user, and a pre-existing bug that the shortfall banner made considerably worse.

`params_ui` drew its notice banner *above* the sliders, and every one of those banners appears or disappears in response to an edit to a slider below it.
Change a reload-only parameter and the "Reload needed" banner arrives the next frame, pushing every widget down by its height — out from under a pointer that may still be dragging one.
The new shortfall banner is the worst case of the same shape, because it can appear *mid-drag*, the moment Grid Width crosses the device limit.

`notice` now runs after the widget loop and after the edits are applied, so it recomputes `pending_count` rather than trailing it by a frame.
Measured through the accessibility tree: the five SIR sliders sit at y = 238/260/282/304/326 with the banner absent, present as one line, and present as three, and a full drag of a reload-only slider from 0.01 to 0.49 lands cleanly.

One wrinkle the move introduced and this fixes too: a slider with a long label (`Random Action Probability`) overflows the panel and widens the region behind it, so a banner drawn afterwards wrapped its text against that wider region and was then clipped mid-word by the panel.
`params_ui` captures `ui.available_width()` before any slider draws and the footer is scoped to it.

### Edited codebase structure

```
crates/
├── henad-core/src/
│   ├── authoring/
│   │   ├── gpu_agent_model.rs   ~ Geometry::display, DisplaySpec doc
│   │   └── gpu_grid_model.rs    ~ Dims uniform, "display is a sampled view" section
│   └── helpers.rs               ~ fmt_bytes (moved in from henad-app)
├── henad-compute/src/
│   ├── display_scale.rs         + NEW: MAX_DISPLAY_DIM, display_dims, source_row
│   ├── lib.rs                   ~ module registration
│   ├── runtime_info.rs          ~ granted/available Limits, display_cap()
│   └── gpu/
│       ├── capacity.rs          + NEW: Alloc, Demand, shortfalls
│       ├── agent_engine.rs      ~ geometry_for, demand, per-texel display dispatch
│       ├── grid_engine.rs       ~ demand, 4-word dims uniform, texel_workgroups
│       ├── limits.rs            ~ raises the three size limits
│       ├── mod.rs               ~ module registration
│       └── view/display.rs      ~ build_display_target caps the texture, returns dims
├── henad-models/src/
│   ├── registry.rs              ~ ModelEntry::capacity + shortfalls, 2 new tests
│   ├── gpu_game_of_life/
│   │   ├── mod.rs               ~ 1 new test
│   │   ├── display.wgsl         ~ per-texel
│   │   └── reduce.wgsl          ~ Dims
│   ├── gpu_sir/
│   │   ├── display.wgsl         ~ per-texel
│   │   └── reduce.wgsl          ~ Dims
│   └── gpu_ants/
│       ├── mod.rs               ~ DisplayParams.tex
│       └── display.wgsl         ~ per-texel
├── henad-app/src/
│   ├── state.rs                 ~ selection_shortfalls
│   └── ui/
│       ├── mod.rs               ~ fmt_bytes moved out
│       ├── params.rs            ~ notices moved to a footer, + "Too large for this device"
│       ├── playback.rs          ~ Build disabled on a shortfall
│       ├── performance.rs       ~ fmt_bytes import
│       ├── system.rs            ~ Device limits section
│       └── viewport.rs          ~ sample_grid for the CPU upload
└── henad-cli/src/main.rs        ~ refuses before building, --info prints limits
```

## State after

Both reproductions run. Release CLI, flat-out `step()` loop, M4 Pro:

| model | grid / world | before | after |
| --- | --- | --- | --- |
| `gpu_game_of_life` | 8192² (67 M) | 622 steps/s | 651 and 729 steps/s |
| `gpu_game_of_life` | 9000² (81 M) | **`create_texture` panic** | 578 steps/s |
| `gpu_game_of_life` | 16384² (268 M) | **`create_texture` panic** | 298 steps/s |
| `gpu_sir` | 6000² (36 M) | **`create_bind_group` panic** | 58 steps/s |
| `gpu_sir` | 16384² (268 M) | **`create_bind_group` panic** | 1.5 steps/s |
| `gpu_ants` | 200 k agents / 9000×2000 | **`create_texture` panic** | 285 steps/s |

`gpu_sir` at 16384² is 4.3 GB of buffers (state and RNG, two sides each) and its throughput per cell falls about 5× against the 6000² run — it is squarely bandwidth-bound there. It runs, which it could not before; nothing here claims it runs *well*.

**No performance claim is made about the step path, because it was not touched.**
The only changed dispatch is display, which runs at snapshot cadence and is not in the CLI's timed loop at all.
The 8192² row above is the reason to say so plainly: 43.7 and 48.9 G cell-updates/s after, against 41.8 before, on 5-step runs whose spread is wider than the gap.
The honest reading is "unchanged", not "faster", and no interleaved old-vs-new run was done because there is no changed code in that loop to attribute a difference to.

Verified in the live app through the egui MCP:

- **`gpu_game_of_life` 12000 × 3000** — 36 M cells, tick 4677+, alive count decaying from a random soup as it should, 4:1 viewport aspect correct, sim memory 55.5 MB — of which 8.6 MiB is the packed state and the rest a 4096 × 3000 display texture.
- **`gpu_ants` 200 k agents / 9001 × 2001** — obstacles and agent layer both drawing over a sampled field, 320 TPS.
- **`gpu_ants` at 401²** — unchanged picture, trails and obstacles as before.
- **CPU `game_of_life` 10000 × 2000** — 20 M cells, renders and steps. This is past 8192 and would previously have failed inside egui's texture upload.
- **`gpu_sir` at 6000²** against baseline limits — Build disabled, banner reading *"'gpu_sir_buffer0_a' and 3 more each need 137.3 MB, past the 128.0 MB this device allows for one storage binding. Reduce the size parameters to build."*
- **System tab** — all six limits and the display cap, with `of 31` and `of 1024` shown dim beside the two Henad does not max out.

Tests: **178 passing** with `HENAD_REQUIRE_GPU=1`, up from 165. New ones:

- `display_scale`: identity below the cap, both axes capped independently, a weaker device lowers the cap further, sampled rows stay in bounds.
- `capacity`: a fitting model reports nothing, the issue-9 `gpu_sir` case is named, different sizes get their own line, the whole-buffer cap says so, an over-bound pass is named, size and count failures do not mask each other, display bytes stop growing with the grid.
- `limits`: the three size limits reach what the adapter reports.
- `gpu_game_of_life::a_grid_past_the_texture_limit_still_builds_and_steps` — 8200 × 64 on the **baseline** test device, checked against the CPU oracle for 10 ticks. Only the width is over, so it also pins the independent per-axis capping: display runs 4096 × 64 while step and reduce still cover all 8200 columns.
- `registry`: every GPU entry reports a capacity and every CPU entry reports none; a too-large model is reported rather than built.
- `every_gpu_model_builds_on_a_baseline_device` now asserts the build **and** that the declared demand agrees it fits, in the same loop. The two pin each other: under-report a pass count and the build fails, over-report one and the new assert does. Checked by mutation — declaring six extra bindings on the grid step pass makes the test fail with `pass 'gpu_sir_step' binds 10 storage buffers, past the 8 this device allows per shader stage`.

  The pin is one-sided for a model that comfortably fits, though: if a pass really binds 6 and the metadata claims 4, both numbers are under the limit and nothing notices. `gpu_ants`'s step pass sits at exactly 8 against a baseline limit of 8, so that one is pinned tightly from both directions.

`./check.sh` green, including the wasm typecheck and `trunk build`.

## Issues found & future directions

### 1. The remaining wall is `gpu_sir`'s per-cell RNG, and it is a model problem now, not an engine one

At 4 GiB per binding, `gpu_sir` caps near 1.07 G cells of state — but it carries a second buffer of the same size for per-cell RNG, so the real ceiling is memory rather than a limit.
This is exactly the coupling the phase-3 notes flagged: packing SIR's state to 2 bits per cell buys density the RNG gives straight back, so the RNG question has to be answered first.
Nothing in this session changes that trade-off; it only means the engine is no longer the thing standing in the way.

### 2. `MAX_DISPLAY_DIM` is a constant, and the honest version is viewport-driven

4096 is generous for a panel-sized viewport and costs a fixed 64 MB.
But it is a guess, and the moment the viewport grows zoom and pan it becomes the wrong abstraction: what you actually want then is to render the *visible region* at native resolution, which scales indefinitely and looks better at every size.
That needs camera state to cross the sim-thread/UI boundary, which is a real design question and was deliberately not opened here.

### 3. Host-side seeding is the next thing to hit

`seed_buffers` builds a `Vec<u32>` (grid) or `Vec<u8>` (agent) on the CPU for every buffer, single-threaded, before uploading.
`gpu_sir` at 16384² spends most of its construction time there and allocates 2.1 GB of host memory to do it.
A packed model dodges this (`gpu_game_of_life` at 16384² seeds 8.4 M words), but an unpacked one does not.
Options are a GPU seeding pass, or chunked uploads — neither was needed to close this issue.

### 4. The CPU grid still round-trips 100 M cells per snapshot

`sample_grid` fixed the *upload*, not the *snapshot*: `build_snapshot` still clones the whole `Vec<u8>` of cells to the UI thread, so a 10000 × 2000 CPU model copies 20 MB per publish and a 10000² one would copy 100 MB.
The named end state in the render-layer notes — upload the raw `u8` as `R8Uint` and do the palette lookup in the fragment shader — would let the sampling happen on the GPU instead, and would delete `expand_grid` and `sample_grid` together.
That is the same migration already on the list, now with one more reason.

### 5. One binding is still one binding

Issue 9 names the storage-binding cap, and that is answered by asking the adapter for its real value rather than by a structure that spans bindings.
32x on this machine, and enough that every current model clears the 100 M target inside a single binding: packed GoL reaches 34.4 G cells, unpacked SIR 1.07 G, and the boids/ants position lane 536 M agents.
A device with a genuinely small binding cap relative to its memory would still need a logical buffer chunked across several bindings, which would mean a WGSL accessor branching over chunk arrays in the hot loop.
Not worth paying for until a device demands it — and notably, what a **browser** reports for `max_storage_buffer_binding_size` is untested here. `trunk build` typechecks the wasm target but this was never run against a real WebGPU device.

### 6. `push_error_scope` around model construction is still missing

Sizes are checked before allocation now, so the common failure is readable.
Everything else about a model's contract with wgpu — workgroup size, uniform layout, WGSL/Rust type correspondence — still surfaces as a panic from wgpu's default error handler on whichever thread built the model.
Wrapping construction in `push_error_scope` would turn all of them into a returned error, and is the natural companion to making `create_state` fallible.

### 7. The parameter panel does not scroll

Moving the notices to the bottom traded one problem for a smaller one: `params_ui` draws straight into the dock tab with no `ScrollArea`, so a model with enough parameters pushes the footer past the panel edge where it is simply clipped.
None of the four models comes close today, and the Build button carries the same reason on hover, so nothing is unreachable.
Wrapping the panel in a `ScrollArea` is the fix whenever a model does get that long.

The same panel already clips long slider labels horizontally (`Random Action Probabil…`), which is the pre-existing half of this and was not touched.

### 8. Grid maxima were left where they were

Both GPU grid models still declare 16 384, which is now inside what the hardware allows rather than past it.
Raising them further is possible on paper (a packed `gpu_game_of_life` fits ~34 G cells in one 4 GiB binding) but pointless until item 3 is solved, since the CPU seeding loop would be the wall long before the GPU was.

---

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     If you update this document, stop at the line above.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

Still, Claude seems to disregard my style on comments unless instructed, and yapped quite a bit.
I've also removed unnecessary labels in the UI.
The newly added string about `Device` limits is a bit long and technical for a user-facing banner, so I have also trimmed it.

Switched `MAX_STORAGE_BUFFERS_PER_STAGE` to dynamic.

Removed unnecessary `#[must_use]` attributes since it is slowly becoming an overused style. 

