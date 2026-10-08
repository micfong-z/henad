---
title: GPU agent models
description: Writing a population stepped by compute shaders with the GpuAgentModel trait.
icon: material/expansion-card-variant
---

# GPU agent models

_See [Writing a GPU agent model](../guide/first-model/gpu-ants.md) for a tutorial._

`GpuAgentModel` is a population whose state lives in GPU buffers.
You declare your buffers, your passes and your bindings as plain data, and the engine derives every wgpu object, the neighbour index, the ping-pong, the stat reduction and the whole runner interface from them.
A model can also declare `ACTIONS`, a list of one-off `GpuAgentAction` passes with a button each, and [actions on the GPU](parameters.md#actions-on-the-gpu) covers them.

Unlike a grid, a step here is a *list* of passes, because the two real models disagree about almost everything structural.
Boids rebuilds a neighbour index and runs one pass over three ping-ponged lanes.
Ants runs two passes over seven in-place buffers, together with a display pass and a persistent counter.

`gpu_boids/` and `gpu_ants/` are the two implementations to read.

## Buffers

```rust
--8<-- "crates/henad-models/src/gpu_boids/mod.rs:buffers"
```

`buffers!` gives each buffer a label and an index derived from its declaration position, in the same way `params!` does.
A project uses `henad::buffers!`.
The example models sit below the facade and use `henad_core::buffers!`.
Flags are named rather than positional, and every flag defaults to off.

`double_buffered`

:   Allocates a second side, so that a pass can read the buffer's previous values while it writes this tick's values.
    The engine builds that side only when some `BufferSpec` requests it, so a model that writes in place pays nothing for the feature.
    Ants double-buffers none of its buffers: its ants never read one another, and deposits land in a separate accumulator instead of in the field the step is reading.

`drawable`

:   Also binds the buffer as a vertex stream, letting the view draw it without a copy.
    `POS_BUFFER` and `COLOR_BUFFER` specify the two buffers that the renderer reads.

`buffer_lens` returns each buffer's length in `u32`-sized elements, worked out from the resolved geometry.

## Passes

```rust
--8<-- "crates/henad-models/src/gpu_boids/mod.rs:passes"
```

`STEP_PASSES` runs in declaration order, once per step.
Each pass specifies its shader, its generated binding declarations and its invocation domain.

```rust
pub enum Domain {
    Agents,
    Cells(u32),
    AgentsOrCells,
}
```

`Cells(n)` dispatches `n` invocations per cell, for a field with `n` layers.
`AgentsOrCells` takes the larger of the two counts, for a pass whose lanes span both agents and cells.
The enum stops at three variants, one per case the two shipped models actually use, and more will appear only when a real model needs them.

Ants declares two passes: `step` over agents, then `merge` over `Cells(2)` for its two pheromone layers.

### Display

```rust
const DISPLAY: Option<DisplaySpec> = Some(DisplaySpec { shader, bindings, workgroup: 16 });
```

Only a model that draws a grid layer declares this.
The display pass is dispatched one invocation per display *texel*, never per cell, and reads the cell at `texel * grid / tex`, exactly as a [GPU grid model](gpu-grid-models.md#display-is-a-sampled-view) does.
Boids leaves it `None` and draws its agent buffers in place.

### Reduce

```rust
const REDUCE: ReduceSpec = ReduceSpec { shader, bindings, lanes, domain };
```

The engine owns every level of the reduction tree above the leaf, and your shader only computes one per-lane value.
`lanes` says how many values the leaf sums, and boids uses three lanes, for speed and the two velocity components.
For the workgroup fold, the leaf's shader imports `henad::reduce_tree::block_sum`.

`COUNTERS` is a separate mechanism for persistent `u32` counters, which a kernel accumulates into and nothing ever clears.
Ants counts cumulative deliveries this way, whereas the reduction target is cleared before every reduction.

## Bindings

A pass never says which resource goes in which slot, because its `BindingDecl` slice is generated from the shader at build time.
The engine resolves each name itself, which stops a slot index disagreeing with the shader that owns it.

Seven names are reserved for resources the engine owns.

| Name | Resource |
|---|---|
| `params` | The pass's own uniform block |
| `dims` | Grid and display texture size, for a grid model only |
| `output` | The display texture |
| `cell_start`, `sorted` | The neighbour index |
| `counters` | The persistent counters |
| `partials` | The reduction's leaf output |

The agent engine has no `dims` resource, and a pass that binds it fails to build.
A display pass carries its texture size in its own uniform block, from `geom.display` in the `PassCtx` that `pass_params_bytes` receives.

Anything else refers to one of your own buffers by its label, optionally with an `_in` or `_out` suffix.
The access mode decides which side a name resolves to, and the suffix does not, so a buffer that one pass reads and another writes needs no special naming.

## The neighbour index

`const INDEX: bool` tells the engine to rebuild a spatial hash from the positions before every step.
Boids sets it.
Ants leaves it off, since ants read the field instead of each other.

With it set, `cell_start` and `sorted` become bindable, and the resolved `HashGrid` geometry arrives in `Geometry::index` for the uniform block to carry onward.
The engine fixes the hash grid from `index_cell_size` at construction.
A GPU model accepts no live edit, and every parameter applies when the model is rebuilt.

## Parameters and geometry

As with a GPU grid model, nothing is prepended to the parameter list, and you spell the whole list out yourself.
Both ports reuse their CPU counterpart's composed list verbatim, which lets both backends accept the same vector and be driven from the same UI state.

`Geometry` is resolved once at construction and carries the population, the extent, the cell grid, the display size and the index geometry.
Once per pass, identified by `PassId`, the engine then requests that pass's uniform block from `pass_params_bytes`.
You return the bytes of the `Params` struct generated from that pass's shader, as `bytemuck::bytes_of(&Params { .. })`.

## Seeding

`seed_buffers` returns raw bytes per buffer, because agent lanes hold mixed types.
An empty vector leaves that buffer cleared, which is exactly right for a scratch buffer that is read before its first write.
Only the current side is seeded, since a double-buffered lane has its other side fully written by the first step anyway.

## Contracts nothing checks

- A binding's declared WGSL type must match what the buffer actually holds, because the engine resolves bindings by name and every storage slot looks alike.
- A pass shader must fold with `linear_index`.
- `buffer_lens` and `seed_buffers` must each return one entry per `BUFFERS` entry, and a non-empty seed must be exactly `len * 4` bytes long.
- `STATS.len()` must equal the number of values `stats` returns.
  The engine pairs the two lists by position and drops the entries past the end of the shorter list.

The engine reads the `@workgroup_size` of each shader's `main` when it builds the model.
It rejects a pass shader that declares anything but `@workgroup_size(256)`, and a display shader whose `@workgroup_size(N, N)` differs from its `DisplaySpec::workgroup`.
It also rejects a buffer label that is reserved or ends in `_in` or `_out`, since a binding of that name resolves to something other than the buffer.

The testing kit's `StatCount` check, given a device, catches a `stats` that returns fewer values than `STATS.len()`.

## Next

- [Shaders and bindings](shaders.md) covers the WGSL side.
- [Porting a model to the GPU](porting.md) explains how to get from a working CPU model to this trait.
- [GPU grid models](gpu-grid-models.md) covers the counterpart trait for grids.
