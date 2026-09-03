---
title: Palettes and views
description: A model's palette, the layers it publishes, and the display texture behind them.
icon: material/palette-outline
---

# Palettes and views

Getting a model on screen takes very little code on your side.
Every model declares a `PALETTE`, and beyond that it publishes one or two view layers.
The engine turns the model's state into those layers, and the app composites them.

```rust
pub const PALETTE: [[u8; 4]; 2] = [
    [0x15, 0x15, 0x15, 0xFF], // Dead - dark gray
    [0x00, 0xE6, 0x76, 0xFF], // Alive - green
];
```

The palette is RGBA, with one entry per value a cell or an agent can carry.
Cells and agents index the same table, which makes a cell value a palette index by construction and leaves no separate colour map to keep in step.

The same literal feeds the [stat descriptors](statistics.md).
Take your stat colours from the palette and each chart line automatically matches the thing it counts.

## Two layers

A model publishes up to two layers, drawn with the field first and the agents over the top.

`GridView`

:   Carries `width`, `height`, `cells: &[u8]` and the palette.
    A `GridModel` publishes its grid here, while an `AgentModel` publishes whatever its [field](fields.md) publishes, which for `NoField` is nothing.

`PointView`

:   Holds `pos_x`, `pos_y`, the world extent, an optional per-agent colour lane and the palette.
    Only an `AgentModel` publishes one.

Both layers stretch to the same rect.
The extent is the engine's, and neither layer supplies its own, which rules out an agent layer and a field layer disagreeing about how big the world is.

Ants publishes both, since it is the composite model, with a pheromone grid underneath and a population of ants on top.

## Colouring agents

The `agent_lanes!` macro takes a `color = <lane>` line naming the lane the renderer reads for palette indices.

```rust
color = has_food;
```

Ants points it at a lane it already keeps, leaving no second lane to write.
Boids derives a dedicated `color` lane from `heading_octant`, which gives eight cyclic hues, and a turning flock therefore shifts hue instead of jumping between colours.
Colouring by speed was tried first, but it collapsed to a single colour once the flock settled at `min_speed`.

A model that declares no colour lane draws its whole population in `PALETTE[0]`.

!!! warning "Seed the colour lane in `init`"

    The initial snapshot is published before any tick runs.
    If only the step writes your colour lane, the whole population shows as `PALETTE[0]` until the first tick lands.

## `prepare_view`

```rust
fn prepare_view(&mut self);
```

This hook runs before a snapshot is built rather than on every tick.
Anything a view needs but the step does not belongs here.

Ants quantises its two `f32` pheromone layers into palette indices in this hook, because `GridView::cells` is `&[u8]` while a field holds `f32`, and the layer owns that quantisation.
The trails fall off geometrically, so the ramp is logarithmic over three decades rather than linear.
With a linear ramp the display would show nothing but a bright dot at the nest.

At a few snapshots a second this work is nearly free.
The same quantisation on every tick, over ten million cells, would not be.

## Display scaling

For a GPU model the cells never reach the CPU.
The display is a texture the sim thread has already written, and the app only samples it.

That texture is capped at 4096 a side, on each axis independently.
The rect is fitted to the grid's aspect ratio, and a short axis therefore keeps its detail.
A display pass dispatches one invocation per *texel* and reads the cell at `texel * grid / tex`, and below the cap that mapping is the identity.

The CPU path samples its grid the same way when uploading, so a large CPU grid and a large GPU grid show the same picture.

A GPU display shader writes RGBA directly and carries its own copy of the palette colours in WGSL.
`PALETTE` still has to be declared, because the stats UI reads it, and keeping the WGSL copy in agreement with `PALETTE` is your model's job.

## Snapshots

The UI never touches live state.
The sim thread builds a `Snapshot` on a fixed cadence and leaves it in a slot, and the UI picks up the newest one.
The buffers are handed back to be refilled, so a publish copies into existing allocations instead of making a fresh multi-megabyte allocation each time.

A GPU snapshot owns no pixels at all, only handles to what already sits on the GPU.
Those handles are held through `Arc`, and an in-flight paint callback therefore keeps the texture alive even if the model is torn down mid-frame.

## Next

- [Statistics](statistics.md) covers the stat series that share the palette colours.
- [The app tour](../guide/app.md#viewport-tab) describes the viewport controls from the user's side.
