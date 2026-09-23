---
title: GPU grid models
description: Writing a grid whose state lives in GPU buffers with the GpuGridModel trait.
icon: material/expansion-card
---

# GPU grid models

_See [Writing a GPU grid model](../guide/first-model/gpu-game-of-life.md) for a tutorial._

`GpuGridModel` is a grid whose state lives in GPU storage buffers and never round-trips to the CPU.
You declare three shaders, the buffer lengths, the seed data and a uniform block, and the engine derives every wgpu object and the whole runner interface from those declarations.
A model can also declare `ACTIONS`, a list of one-off `GpuGridAction` passes with a button each, and [actions on the GPU](parameters.md#actions-on-the-gpu) covers them.

`gpu_game_of_life/` and `gpu_sir/` are the two grid models shipping with the engine.
Each is a single `mod.rs` of declarations sitting next to its `.wgsl` files, so a complete GPU grid model amounts to one Rust file and its shaders.

## The three passes

| Pass | Dispatched over | Work |
|---|---|---|
| `STEP_SHADER` | `step_dims`, one invocation per cell by default | Reads every buffer's current side, writes every buffer's next side |
| `DISPLAY_SHADER` | One invocation per display texel | Writes RGBA into the display texture |
| `REDUCE_SHADER` | One invocation per cell | Accumulates the stat counters |

The shipped display and reduce shaders read only buffer 0, the primary state buffer.
An auxiliary buffer, a per-cell RNG for example, is left to the step shader.

## Buffers

`BUFFERS` declares one label per ping-ponged buffer.
Game of Life needs only one, `state`, while SIR declares two, `state` for its cells and `rng` for its per-cell RNG.

All `K` buffers ping-pong together in lockstep.
A step reads every buffer's current side and writes every buffer's next side, and a model keeping per-cell RNG state therefore advances it in the same pass that advances the cell.

The engine resolves each binding by the name the shader gives it, as for a [GPU agent model](gpu-agent-models.md#bindings).
A buffer is bound by its label, with an optional `_in` or `_out` suffix, and the access mode decides which side the name resolves to.
Four reserved names cover the resources the engine owns for a grid model: `params`, `dims`, `output` and `counters`.
Each shipped step shader binds a read/write pair per buffer and then the uniform, and SIR's has two pairs.

``` wgsl title="crates/henad-models/src/gpu_sir/step.wgsl"
--8<-- "crates/henad-models/src/gpu_sir/step.wgsl:bindings"
```

## Buffer length and dispatch domain

The engine does not prescribe how you map cells onto `u32`s.
You supply two numbers instead: `buffer_lens` for the length of each buffer, and `step_dims` for the invocation count the step needs.
Both default to one `u32` per cell and one invocation per cell, the unpacked arrangement most models want.

A bit-packed model overrides both and works in words.
GPU Game of Life packs 32 cells into each `u32` and pads rows to whole words, which gives one invocation a whole word to own and stops any two invocations writing the same one.
Its step then evaluates the rule SWAR-style: the neighbour count stays bit-sliced and is summed by a carry-save adder built from plain XOR and AND, which resolves all 32 cells at once without a loop.

Reduce always dispatches one invocation per *cell*, so a packed model's reduce shader reads the containing word and extracts its own bit.

## Display is a sampled view

The display texture is capped at 4096 texels a side, well under the largest grids.
A texture with one texel per cell would bound the grid at the device's maximum texture dimension and cost four bytes per cell, which at 16384^2^ comes to over a gigabyte of RGBA for something drawn into a panel roughly a thousand pixels wide.

The display pass instead dispatches one invocation per *texel* and reads the cell at `texel * grid / tex`.
Both pairs of dimensions arrive in a shared `Dims` uniform, and they stay equal until the grid outgrows the cap.

``` wgsl title="crates/henad-models/src/gpu_sir/display.wgsl"
--8<-- "crates/henad-models/src/gpu_sir/display.wgsl:bindings"
```

``` wgsl title="crates/henad-models/src/gpu_sir/reduce.wgsl"
--8<-- "crates/henad-models/src/gpu_sir/reduce.wgsl:bindings"
```

`Dims` comes from `shared::dims` in `henad-compute/src/gpu/shared/dims.wgsl`, and holds `grid` and `tex`, each a `vec2<u32>`.

The display shader writes RGBA directly, and it therefore carries its own copy of the palette colours in WGSL.
Only the stats UI reads `PALETTE`, so keeping the two in agreement is your responsibility as the model author.

## Parameters

Unlike a CPU `GridModel`, nothing is prepended to the parameter list here.
A GPU model spells out its whole descriptor list itself, which lets it mirror the parameter order of the CPU model it is compared against.
Both shipped ports do this and reuse the CPU model's list verbatim.

The engine reads width and height back out of that list for `dims`, clamping both to at least 1.

## Seeding

```rust
fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>>;
```

Buffer contents are built on the CPU and uploaded once at construction.
Both shipped ports call their CPU counterpart's `init` here and nowhere else, which starts both backends from the same data and makes tick 0 come out bit-identical between them.
See [porting a model to the GPU](porting.md) for the rest of that workflow.

## Contracts nothing checks

Shaders are opaque strings as far as Rust is concerned, and none of the contracts below is enforced at compile time.
Getting one wrong surfaces as a wgpu validation error when the model is first constructed, and knowing the list in advance makes that error much quicker to place.

- `WORKGROUP_SIZE` must equal the `@workgroup_size(N, N)` that all three shaders declare.
- `STATS.len()` must equal both the number of entries `stats` returns and the number of `atomic<u32>` in the reduce shader's `counters` binding.
  GPU Game of Life binds one bare `atomic<u32>` there, and GPU SIR an array of three.
- `buffer_lens` must return exactly `BUFFERS.len()` lengths, and `seed_buffers` must return exactly that many vectors, each of exactly the declared length.

Sizes and per-pass binding counts are checked before anything is allocated, and a model over the device's limit is refused with a readable message rather than a panic.
Every other construction error reaches the UI as a modal.

## Next

- [Shaders and bindings](shaders.md) covers the WGSL side, from imports and generated uniform structs to reading back what was actually compiled.
- [Porting a model to the GPU](porting.md) explains how to get from a working CPU model to this trait.
- [GPU agent models](gpu-agent-models.md) covers the counterpart trait for agent populations.
