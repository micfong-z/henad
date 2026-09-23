---
title: Shaders and bindings
description: The WGSL side of a GPU model, from generated bindings to the composed source the engine compiles.
icon: material/code-braces
---

# Shaders and bindings

Both GPU traits hand the engine WGSL as a `&'static str`, because `henad-core` depends on nothing, not even wgpu, and therefore cannot name wgpu types.
Most of what surrounds that string is generated at build time, and this page covers the generated part.

## Generated from the WGSL

A `build.rs` in `henad-compute`, `henad-models` and `henad-app` runs `wgsl_bindgen` over the shaders it lists as entry points, and the output lands behind a `shader_bindings` module.

Uniform structs, workgroup sizes and bind group layouts therefore come from the WGSL instead of being retyped in Rust.
Your model fills in the generated struct and hands back its bytes, and declares no uniform struct of its own.
A uniform declared as a bare vector, as GPU Game of Life's step uniform is, has no struct, and the model hands back the values themselves.
The generated struct always has the shader's layout.
An initialiser that names every field also stops compiling when the WGSL `struct Params` gains one, while one ending in `..bytemuck::Zeroable::zeroed()` leaves the new field at zero.

```rust
use crate::shader_bindings::gpu_boids::step::Params as StepParams;
```

The shader source a model declares comes from the same place, as `SHADER_STRING`.

Generation cannot reach two things: a type no shader in the crate uses, because naga keeps only what an entry point references, and a constant that arrives through an `#import`.

## Shared WGSL

Shared code lives in `henad-compute/src/gpu/shared/` and is reached with `#import`, which is resolved at build time.

```wgsl
#import shared::prelude::linear_index
#import shared::space::{TORUS, axis_delta, heading_octant, wrap_index}
```

| Module | Contents |
|---|---|
| `shared::prelude` | `WORKGROUP`, and `linear_index` for folding a linear domain onto the workgroup grid |
| `shared::space` | The WGSL twins of the [space primitives](../reference/primitives.md#space) |
| `shared::rng` | The WGSL twins of the [random primitives](../reference/primitives.md#random) |
| `shared::dims` | The `Dims` struct a grid model's display and reduce shaders read |
| `shared::reduce_tree` | `block_sum`, the workgroup fold a reduce leaf repeats |

Most primitives here pair with a Rust function under `henad_core::authoring::primitives`, and a parity test pins each pair of pure functions together.
[Authoring primitives](../reference/primitives.md) is the index.
It names the WGSL-only primitives and records what is deliberately absent.

## Bindings

The `build.rs` in `henad-models` reads the `@group(0)` lines of every shader in its `ENTRY_POINTS` list into a `binding_decls` module, in `@binding` order.
Every pass of either GPU trait points at one of its constants, named after the shader's path, as `crate::binding_decls::bindings::GPU_SIR_STEP` is for `gpu_sir/step.wgsl`.
A new shader has to be added to `ENTRY_POINTS` before `shader_bindings` or `binding_decls` knows about it.
The engine resolves each name itself.
Otherwise a slot index could disagree with the shader that owns it.

Seven names are reserved for resources the engine owns, and anything else names one of the model's own buffers by its label.
The full list is in [GPU agent models](gpu-agent-models.md#bindings).
A [GPU grid model](gpu-grid-models.md) has a resource for four of them, `params`, `dims`, `output` and `counters`.

The shipped grid models name each buffer twice in the step shader, `<label>_in` for reading and `<label>_out` for writing, and bind `params` after the last pair.
That suffix is a naming convention.
The access mode decides which side a name resolves to.

!!! note "Names are read, not typechecked"

    Resolution goes by name, and every storage slot looks alike to wgpu.
    A binding whose declared WGSL type does not match what the buffer holds still produces a valid layout, which then reads the wrong bytes.

## Dispatch

An agent pass folds its linear invocation domain onto a 2D workgroup grid, because a hundred million agents overflow one row of workgroups.

```wgsl
@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>,
        @builtin(workgroup_id) wid: vec3<u32>) {
    let i = linear_index(lid, wid, params.groups_x);
    if i >= params.num_agents { return; }
    // ...
}
```

The fold width the engine picked arrives as `groups_x` in the uniform block, through `PassCtx::groups_x`, so a shader that folds must carry that field.

A grid model's shaders dispatch 2D directly and declare a `@workgroup_size(N, N)` matching `WORKGROUP_SIZE`.

## Reading the composed source

A shader is composed from its imports and re-emitted by naga, so the text the engine compiles is not the file as you wrote it, and a WGSL error names the composed text rather than your source.

```bash
HENAD_DUMP_WGSL=/tmp/wgsl cargo run --release -p henad-app
```

With that variable set, every shader the engine compiles lands in `<dir>/<label>.wgsl`, which lets you read a validation error against the composed source as ordinary text.
See [environment variables](../reference/environment.md).

## Next

- [GPU grid models](gpu-grid-models.md) and [GPU agent models](gpu-agent-models.md) are the two traits this machinery sits under.
- [Authoring primitives](../reference/primitives.md) is the index of what a kernel can call.
