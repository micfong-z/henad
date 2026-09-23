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

## Three layers

There are three layers, drawn with the field first, the edges over it and the agents on top.
A model publishes up to two of them.

`GridView`

:   Carries `width`, `height`, `cells: &[u8]` and the palette.
    A `GridModel` publishes its grid here, while an `AgentModel` publishes whatever its [field](fields.md) publishes, which for `NoField` is nothing.

`PointView`

:   Holds `pos_x`, `pos_y`, the world extent, an optional per-agent colour lane and the palette.
    An `AgentModel` publishes its agents here, and a [`NetworkModel`](network-models.md) publishes its nodes.

`EdgeView`

:   Holds `src` and `dst`, an optional colour byte per edge, the edge palette, whether the edges are directed, and a `version`.
    Each endpoint is an index into the `PointView`'s positions.
    Only a `NetworkModel` publishes one.

The grid and the points stretch to the same rect.
The extent is the engine's, and neither layer supplies its own, which rules out an agent layer and a field layer disagreeing about how big the world is.
Edges carry no coordinates at all.
The renderer reads both ends of each edge from the agent layer's position buffers.
An edge follows its nodes wherever the layout moves them.

Ants publishes a grid and points, since it is the composite model, with a pheromone grid underneath and a population of ants on top.
A network model publishes points and edges, and each node is drawn over the edges that meet it.
The Viewport tab's Edges checkbox shows or hides the edges, and Arrows adds arrowheads to a directed graph.
Edges are drawn only in Sprites mode.
They also need vertex shaders that can read storage buffers.
Some GPUs lack that support, and no GPU has it under WebGL2.
Without it, the Network edges row of the [System tab](../guide/app.md#system-tab) reads Unavailable, and a network model runs with its edges undrawn.

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

## Colouring edges

A `NetworkModel` declares a second palette, `EDGE_PALETTE`, for its edges.
Every edge carries one colour byte, an index into that table.

```rust
--8<-- "crates/henad-models/src/virus_network/mod.rs:edge_palette"
```

`add_edge(a, b, color)` sets the byte when the edge is created, and `set_edge_color` changes it for one edge later.
Team Assembly colours each new edge by how many of its ends are incumbents, and turns an edge red with `set_edge_color` when the same pair joins a team together again.

To recolour edges in bulk, call `Network::update_colors` from `prepare_view`.
Virus on a Network greys every edge with a resistant end this way.

```rust
--8<-- "crates/henad-models/src/virus_network/mod.rs:prepare_view"
```

The closure gets the edge list and a mutable slice of the colours, and returns whether it changed any of them.
The graph's `version` moves only when it returns `true`.
A snapshot refills its edge list only when the version or the edge count has changed, and the renderer uploads edges to the GPU on the same test.
A closure that always returned `true` would copy every edge on every publish.
Virus on a Network's `recolor` checks the edges before writing any, and writes nothing when none is out of date.

Adding or removing an edge, `set_edge_color`, retiring a node and switching the direction also move the version.
Note that `spawn` leaves it alone, and a cache keyed on the version misses a new node until an edge reaches it.

## Retired nodes

A network model can retire nodes partway through a run.
Team Assembly retires every node that has gone more than `max_downtime` ticks without joining a team.

`Nodes::retire` removes the node's edges and sets its position to `NaN`.
The slot keeps its entry in every lane, and the `NaN` marks it as empty for drawing and export.
The renderer hides the node's sprite, the density heatmap leaves the node out of its count, and the layout neither moves it nor lets it push its neighbours.
The [state export](../guide/app.md#current-state) leaves it out of the point section and closes up the rows after it, and each edge's endpoints are remapped to the rows as written.

!!! warning "Place a node when you spawn it"

    A spawned node is drawn wherever its `pos_x` and `pos_y` lanes put it.
    In a reused slot they still hold `NaN`, and the node and any edge to it stay hidden until the model writes a position.
    The layout does not move such a node, and no other node feels a force from it.
    In a slot appended at the end they start at the lanes' initial values.
    Both shipped models declare those as `0.0`, and an unplaced node there starts in the corner of the world, at (0, 0).
    [Retired nodes](network-models.md#retired-nodes) covers what a spawn leaves in the other lanes.

## `prepare_view`

```rust
fn prepare_view(&mut self);
```

This hook runs before a snapshot is built rather than on every tick.
Anything a view needs but the step does not belongs here.

Ants quantises its two `f32` pheromone layers into palette indices in this hook, because `GridView::cells` is `&[u8]` while a field holds `f32`, and the layer owns that quantisation.
The trails fall off geometrically, so the ramp is logarithmic over three decades rather than linear.
With a linear ramp the display would show nothing but a bright dot at the nest.

Snapshots go out at most once every 16 ms, about 60 a second, however fast the model ticks.
Done in the step, the same quantisation would cost a pass over ten million cells on every tick.

A `NetworkModel` gets the hook as `prepare_view(nodes, tick)`, with mutable access to its lanes, its graph and its `Aux`.
`tick` is the number of ticks completed so far.
Team Assembly colours the members of the latest team here.
It also labels its connected components for `stats` to read, as [statistics](statistics.md#network-models) describes.

How often the hook runs depends on how often snapshots go out, and a tick must never read anything the hook writes.
Both network models carry a [test](determinism.md#network-models) that holds them to this.

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

Building a CPU snapshot starts with `prepare_view`.
For a network model with the layout on, the engine then runs the layout until the Layout budget set in the [Pacing tab](../guide/app.md#pacing-tab) is spent.
Only after both does the snapshot copy the layers and call `stats`, and the stats see whatever `prepare_view` just computed.
The Prepare view row of the Performance tab shows the time the two took together.

[Network models](network-models.md#layout) covers which publishes move the nodes and what Layout while paused changes.
`henad-cli` calls `prepare_view` before each stats sample and before an export, and never runs the layout.

A GPU snapshot owns no pixels at all, only handles to what already sits on the GPU.
Those handles are held through `Arc`, and an in-flight paint callback therefore keeps the texture alive even if the model is torn down mid-frame.

## Next

- [Statistics](statistics.md) covers the stat series that share the palette colours.
- [Network models](network-models.md) covers the graph behind the edge layer, and the layout that places its nodes.
- [The app tour](../guide/app.md#viewport-tab) describes the viewport controls from the user's side.
