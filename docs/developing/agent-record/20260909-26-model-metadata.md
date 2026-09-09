---
date: 2026-09-09
title: "What a model says about itself"
description: The Model panel grows a metadata readout, derived from the trait consts each model already declares plus the device demand the capacity check already computes.
icon: material/information-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: "#16"
state: implemented and driven through the live app, `./check.sh` green with HENAD_REQUIRE_GPU=1
baseline_commit: dbb05e6
delta_state: uncommitted on `16-metadata-display`
---

# What a model says about itself

> The Model tab held a dropdown and a description, and nothing else in the app named a model's backend, its topology, or what it would cost to run.
> It now carries Identity, Structure and Interface under the description, plus a Footprint block for the models that have a device cost to report.
> Every value is derived, either from a trait const the model already declares or from the `Demand` the capacity check already computes, so nothing in the panel is written by hand and nothing can drift from the code.
> Three consts were added to make the derivation total: `AgentLanes::LANES`, emitted by `agent_lanes!`, and a `KIND` label on `NeighborIndex` and `FieldLayer`.

## State before

`model_ui` was twenty lines: a `ComboBox` over `registry`, a `load_default_params()` on change, and `ui.label(entry.description)`.

`ModelEntry` carried `name`, `id`, `description`, the two descriptor lists, `topology_hint`, the factory and an optional `capacity`.
Of those the panel showed two.
`topology_hint` was read by nothing but its own registry test, and `capacity` only ever produced `shortfalls`, a `Vec<String>` for the Parameters banner, throwing away the sizes it had just computed.

Everything else a model declares stayed inside its trait impl: `NEIGHBORHOOD` on `GridModel`; `CHUNK`, `Lanes`, `Index` and `Field` on `AgentModel`; `BUFFERS` and `WORKGROUP_SIZE` on `GpuGridModel`; `BUFFERS`, `STEP_PASSES`, `INDEX`, `DISPLAY` and `COUNTERS` on `GpuAgentModel`.
None of it reached the UI.

## What was done

New files marked **+**, modified marked **~**.

```
CHANGELOG.md                                 ~ Unreleased/Added

crates/henad-core/
└── src/
    ├── lib.rs                               ~ pub mod metadata
    ├── metadata.rs                          + Backend, LaneSpec, Structure, ModelMetadata
    └── authoring/model/
        ├── agent_model.rs                   ~ AgentLanes::LANES, NeighborIndex::KIND
        └── field.rs                         ~ FieldLayer::KIND

crates/henad-compute/
└── src/cpu/
    ├── primitives/lanes_macro.rs            ~ the macro emits LANES from the declaration
    └── field/
        ├── ca.rs                            ~ KIND = "Cellular automaton"
        └── scalar.rs                        ~ KIND = "Scalar"

crates/henad-models/
└── src/registry.rs                          ~ ModelEntry::metadata, filled by each register_*;
                                               ModelEntry::demand(); two tests

crates/henad-app/
└── src/ui/
    ├── model.rs                             ~ the four blocks, swatches, four unit tests
    └── dock.rs                              ~ left column re-split, Model 20% -> 38%

docs/
├── assets/app/model-metadata.png            + the panel for Ant Foraging on the GPU
├── guide/app.md                             ~ a Metadata subsection under the Model tab
└── developing/agent-record/
    └── 20260909-26-model-metadata.md        + this record

zensical.toml                                ~ nav entry for the record
```

### Where the facts come from

`ModelMetadata` sits in henad-core beside `TopologyHint`, and holds a `Backend`, an optional palette, and a `Structure`.
`Structure` has one variant per authoring trait, each holding that trait's own consts by reference, so the whole thing allocates nothing.
Each `register_*` fills it from the type parameter it already has.

Three gaps had to be closed before the derivation was total.
`AgentLanes` gained `const LANES: &'static [LaneSpec]`, which `agent_lanes!` emits with `stringify!` next to the fields it is already writing.
A lane list built by hand would have been a second declaration to keep in step, which is the thing the macro exists to prevent.
`NeighborIndex` and `FieldLayer` gained a `KIND` label each, as `&'static str` rather than an enum: `CaField` and `ScalarField` live in henad-compute, and an enum in henad-core would have had to name them.

The palette is an `Option`, since `GpuAgentModel` is the one trait with no `PALETTE`.
Its shaders write RGBA directly, and the panel says `In shader` rather than inventing a number.

### The footprint

`ModelEntry::demand` returns the `Demand` that `shortfalls` was already building and discarding.
That gives the GPU rows for free: total device bytes, the largest single buffer, the display texture, and the widest pass's storage bindings against the device's limit.
The last one is the trap from record #05 made visible, and reads `8 of 8` for `gpu_ants`.

A CPU model has no `capacity` and gets no Footprint block at all.
Its host allocation is already the Performance tab's Sim memory, and a second copy under a different name would only invite the two to disagree.

Cell counts and world size were left out.
They are the grid and extent sliders restated, one panel over.

### Layout

The default left column gave Model 20% of its height, which was sized for a dropdown and a sentence.
Model now takes 38%, out of Playback and Pacing, which are a handful of widgets each.
The panel scrolls regardless, since the models differ by seven rows between the shortest and the tallest, and the offset resets on a model switch so the next model does not open halfway down.

Long lists go in tooltips rather than rows: hovering `Lanes  5 (4 double-buffered)` names each lane and its type, and the buffer and pass counts do the same.
Both cells of such a row carry the tooltip, since a value as short as `7` is barely a pointer wide.
Buffer labels are prefixed with the model id, which the panel has already said two rows up, so the footprint strips it and shows `accum_a, 315.6 KB`.

## State after

`./check.sh` is green with `HENAD_REQUIRE_GPU=1`, the web build included.

Six tests, in the two places the panel can rot from.
In the registry, `declared_metadata_matches_the_entry_it_describes` pins the declared backend against the arm the factory returns and against whether a capacity is present, and the structure variant against the topology hint; `a_declared_palette_has_colours_in_it` catches an empty one.
In the panel, four cover the pure formatting: every topology combination, a count that drops its qualifier at zero, and the id-stripping on a label that does not carry the prefix.

Driven through the live app over the egui inspection port, one model per authoring trait:

- SIR reads `Moore`,
- Game of Life on the GPU reads `Buffers 1`, `Workgroup 16 x 16`,
- boids reads `Lanes 5 (4 double-buffered)`, `Chunk size 64 agents`, `Spatial hash`, `Field layer None`, and the lane tooltip lists `pos_x: f32 (dual)` through `color: u8`,
- ants reads `Chunk size 4096 agents` and `Field layer Scalar`, and shows no Footprint block,
- boids on the GPU reads `3 (3 double-buffered)`, one step pass, no display pass, `13.8 MB`, `7 of 8`,
- ants on the GPU reads seven buffers, two step passes, no index, a display pass, one counter, `986.0 KB`, a `201 x 201` texture and `8 of 8`.

## Issues found & future directions

1. **A CPU model cannot report its footprint before it is built.**
   `capacity.rs` is GPU-only, and a grid model's cost is two bytes a cell while an agent model's is the lanes plus the index.
   Both are computable from the params without allocating, which is what a Footprint block for a CPU model would need, and would also let the Parameters banner warn about a run that will not fit in RAM.
2. **The Parameters panel has no `ScrollArea`.**
   It was already possible to clip its sliders by shrinking the panel; taking height for Model brings that closer.
   The panel this session touched scrolls, that one still does not.
3. **`ui.horizontal(..).response.on_hover_text(..)` shows no tooltip.**
   It was the first attempt at a full-width hover target, and it silently did nothing while the same call on a `Label` worked.
   Worth knowing before reaching for it again.
4. **The tooltips are the only way to read a name.**
   Lane, buffer and pass names are one hover away with nothing marking the row as hoverable.
   A collapsing section per list would be discoverable, at the cost of the height the panel does not have.
5. **The registry test cannot check a lane list against the lanes.**
   `AgentLanes` exposes `len`, `heap_bytes` and `positions`, none of which count lanes, so `LANES` is only as right as the macro.
   The macro writes both from one declaration, which is most of the protection, but a lane added by hand to the struct would go unnoticed.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

- Determined scope and the metadata to display
- Edited display strings for UX, and refined scope decisions
- Edited panel styling and relevant layout code
- Edited comments for clarity
- Rewritten relavent documentation
- Cut down redundant code
- As always, reviewed everything
