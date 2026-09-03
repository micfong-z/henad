---
title: Shaders and bindings
description: The WGSL side of a GPU model, from generated bindings to the composed source the engine compiles.
icon: material/code-braces
---

# Shaders and bindings

Both GPU traits hand the engine WGSL as a `&'static str`, because `henad-core` depends on nothing, not even wgpu, and therefore cannot name wgpu types.
Everything that surrounds that string is generated at build time, and this page covers the generated part along with the part that remains yours to maintain.

## Generated from the WGSL

A `build.rs` in `henad-compute`, `henad-models` and `henad-app` runs `wgsl_bindgen` over that crate's shaders, and the output lands behind a `shader_bindings` module.

Uniform structs, workgroup sizes and bind group layouts therefore come from the WGSL instead of being retyped in Rust.
Your model keeps its own `#[repr(C)]` struct and asserts it against the generated one.
Adding a field to a WGSL `struct Params` without touching its Rust twin then fails the build, and the model never reaches the point of misreading memory at runtime.

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

Every primitive here pairs with a Rust function under `henad_core::authoring::primitives`, and a parity test pins each pair to the other.
[Authoring primitives](../reference/primitives.md) is the index, and it also records what is deliberately absent.

## Bindings

For a [GPU grid model](gpu-grid-models.md) the binding layout is fixed by the trait itself: interleaved read/write pairs, then the uniform.

A [GPU agent model](gpu-agent-models.md) resolves its layout by name instead.
Each shader's `@group(0)` declarations are read off the source at build time in `@binding` order, and the engine matches each name to a resource on its own, which stops a slot index disagreeing with the shader that owns it.

Seven names are reserved for resources the engine owns, and anything else names one of the model's own buffers by its label.
The full list is in [GPU agent models](gpu-agent-models.md#bindings).

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

## The hand-written half

Only the `&[Binding]` correspondence itself is still written by hand.
Routing it through the generated bind groups was tried and reverted: the attempt added 248 lines across the models and `henad-core` to remove an error wgpu already reports loudly at model construction, and it left the buffer indices exactly as hand-written as before.

## Next

- [GPU grid models](gpu-grid-models.md) and [GPU agent models](gpu-agent-models.md) are the two traits this machinery sits under.
- [Authoring primitives](../reference/primitives.md) is the index of what a kernel can call.
