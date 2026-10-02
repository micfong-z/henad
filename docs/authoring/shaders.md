---
title: Shaders and bindings
description: The WGSL side of a GPU model, from generated bindings to the composed source the engine compiles.
icon: material/code-braces
---

# Shaders and bindings

Both GPU traits hand the engine WGSL as a `&'static str`, because `henad-core` depends on nothing, not even wgpu, and therefore cannot name wgpu types.
Most of what surrounds that string is generated at build time, and this page covers the generated part.

!!! info "Henad 0.3"

    This page describes Henad 0.3.

## Generated from the WGSL

A crate's `build.rs` runs henad-build over its shaders, and `henad::include_shaders!()` at the crate root brings the output in as two modules, `shader_bindings` and `binding_decls`.
henad-build is a build dependency, of the same release as `henad`.
A project made from the [template](../guide/your-project.md) has both already:

``` rust title="build.rs"
--8<-- "templates/model-project/build.rs"
```

`stamp_commit` records the commit the crate was built from, whether its sources differed from that commit, and a hash of its sources, and a sweep's manifest records them beside each model the crate registers.
A crate without shaders keeps its `build.rs` for the stamp, and drops the `ShaderBuild` line and `henad::include_shaders!()`.
A file a model reads at compile time, through `include_bytes!` or `include_str!`, belongs under `src`, where the stamp sees it.
A model crate keeps its shaders under `src` and passes `"src"` to `discover`.

``` rust title="src/lib.rs"
henad::include_shaders!();
```

`ShaderBuild::discover` takes every `.wgsl` file under `src` without a `#define_import_path` line as an entry point, and a new shader joins the bindings the next time the crate builds.
A shader's path below `src` becomes its names.
`gpu_sir/step.wgsl` is the module `shader_bindings::gpu_sir::step` and the constant `binding_decls::bindings::GPU_SIR_STEP`, the path's components upper-cased and joined by `_`.
Each component of a `.wgsl` path is therefore a Rust identifier and no keyword, and a folder named `gpu-sir` fails the build.
So do two shaders whose names collide, as `gpu/sir_step.wgsl` and `gpu_sir/step.wgsl` both give `GPU_SIR_STEP`.

Uniform structs, workgroup sizes and bind group layouts come from the WGSL instead of being retyped in Rust.
Your model fills in the generated struct and hands back its bytes, and declares no uniform struct of its own.
A uniform declared as a bare vector, as GPU Game of Life's step uniform is, has no struct, and the model hands back the values themselves.
The generated struct always has the shader's layout.
An initialiser that names every field also stops compiling when the WGSL `struct Params` gains one, while one ending in `..bytemuck::Zeroable::zeroed()` leaves the new field at zero.

```rust
use crate::shader_bindings::gpu_boids::step::Params as StepParams;
```

The shader source a model declares comes from the same place, as `SHADER_STRING`.

An imported constant or type reaches the generated bindings exactly when an entry point references it, since naga keeps only what an entry point references.
A type or a constant no shader in the crate uses has no Rust twin.

The generated bindings define a module named `henad` of their own, for the shared modules below.
Code inside `include_shaders!` reaches Henad through `$crate`, and a hand-written module that includes the generated files itself names Henad as `::henad`.
A bare `use henad::...` there is ambiguous between the two, and fails with E0659.

!!! note "Deny unsafe code, never forbid it"

    The generator writes `unsafe impl bytemuck::Pod` and an `unsafe fn from_raw`.
    `include_shaders!` allows `unsafe_code` for the two generated modules alone, which works under `unsafe_code = "deny"` and fails under `#![forbid(unsafe_code)]`.

## Shared WGSL

The shared modules ship with henad-core, in `henad-core/src/authoring/primitives/wgsl/`, and a shader reaches them with `#import henad::<module>`, resolved at build time.

```wgsl
#import henad::dispatch::linear_index
#import henad::space::{TORUS, axis_delta, heading_octant, wrap_index}
```

| Module | Contents |
|---|---|
| `henad::dispatch` | `WORKGROUP`, and `linear_index` for folding a linear domain onto the workgroup grid |
| `henad::space` | The WGSL twins of the [space primitives](../reference/primitives.md#space) |
| `henad::rng` | The WGSL twins of the [random primitives](../reference/primitives.md#random) |
| `henad::dims` | The `Dims` struct a grid model's display and reduce shaders read |
| `henad::reduce_tree` | `block_sum`, the workgroup fold a reduce leaf repeats |

Most primitives here pair with a Rust function under `henad::authoring::primitives`, and a parity test pins each pair of pure functions together.
[Authoring primitives](../reference/primitives.md) is the index.
It names the WGSL-only primitives and records what is deliberately absent.

A module of your own is a file with a `#define_import_path` line.
An import resolves by file path alone, so the import path mirrors the file's path below `src`, or below the directory of the shader importing it.
`#define_import_path gpu_ants::state` sits in `gpu_ants/state.wgsl`, and the same line in `common/state.wgsl` is never found.
The root `henad` is reserved for the shared modules, in any case.
A file named `henad.wgsl`, or a folder named `henad` holding a `.wgsl` file, shadows one of them, and fails the build.
So does a path starting with a name the generated bindings use at their root: `wgpu`, `bytemuck`, `std`, `core`, `alloc`, `_root`, `ShaderEntry`, `layout_asserts` or `bytemuck_impls`.
A module declares no binding, since bindings belong in the entry shader.

## Bindings

henad-build reads the `@group(0)` lines of every entry point into `binding_decls`, in `@binding` order.
Every pass of either GPU trait points at one of its constants, as `crate::binding_decls::bindings::GPU_SIR_STEP` is for `gpu_sir/step.wgsl`.
Each binding sits on one line, as `@group(0) @binding(N) var<...> name: Type;`, and a line holding `@binding(` in any other form fails the build.
A compile-time assertion holds each list to the length of the layout naga derives from the composed shader.
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
HENAD_DUMP_WGSL=/tmp/wgsl cargo run --release
```

With that variable set, every shader the engine compiles lands in `<dir>/<label>.wgsl`, which lets you read a validation error against the composed source as ordinary text.
See [environment variables](../reference/environment.md).

## Next

- [GPU grid models](gpu-grid-models.md) and [GPU agent models](gpu-agent-models.md) are the two traits this machinery sits under.
- [Authoring primitives](../reference/primitives.md) is the index of what a kernel can call.
