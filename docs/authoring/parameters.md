---
title: Parameters
description: The parameter declarations a model writes, and the difference between a live edit and one needing a reload.
icon: material/tune
---

# Parameters

Every slider in the app and every `--set` override in the CLI reads from a single declaration list on the model.
You write the id, label, range and default together in one place, and both front ends work from those declarations, leaving no second list to keep in step.
This page covers the declaration API, when an edit lands live, when it needs a rebuild, and the way the indices compose.

```rust
--8<-- "crates/henad-models/src/boids/mod.rs:params"
```

The `params!` macro expands to one `const` per entry, holding an index derived from the entry's position in the declaration, together with a `descriptors()` function that returns the whole list.
Your impl forwards `param_descriptors` to `descriptors`, and reads values back through the generated index constants.

```rust
fn param_descriptors() -> Vec<ParamDescriptor> {
    descriptors()
}
```

## Kinds

```rust
pub enum ParamKind {
    F32 { min: f32, max: f32, default: f32, step: Option<f32> },
    U32 { min: u32, max: u32, default: u32 },
    Bool { default: bool },
    Choice { options: &'static [&'static str], default: usize },
}
```

For the common cases there are the `f32_param` and `u32_param` helpers.
Both take an id, a label, a default, a minimum and a maximum, and `f32_param` additionally takes a slider step.

The id is the machine-facing name, matched by `henad-cli --set` and recorded in the benchmark CSV output.
The label is the human-facing name the app shows.

## Live against reload

```rust
pub enum ParamApply {
    Live,
    OnReload,
}
```

A live parameter takes effect on the next tick.
Anything marked reload is read once while the state is being built, and changing it means rebuilding the state.

Live is the default, and calling `.on_reload()` on a declaration flips it.

```rust
const DENSITY = f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01)).on_reload();
```

Both front ends read this behaviour from the descriptor, so you declare it in exactly one place.
`ParamStore::set` rejects an edit to anything declared `OnReload` and returns whether the edit landed.
The app reads the same flag and can say so before anything is sent.

Declare `.on_reload()` for any parameter that only `init` reads.
Otherwise a live edit to it changes nothing, with no sign that it failed.

A registry test builds every model, edits every parameter, and asserts that the state accepts exactly the edits the descriptor says it will.

## Hot parameters

Your kernel receives a `&Self::Params` rather than the raw `&[ParamValue]` slice.
Matching an enum per cell or per agent would put the match inside the inner loop, so `from_params` runs once at the start of each tick instead, and every invocation within that tick shares its result.

```rust
--8<-- "crates/henad-models/src/sir.rs:from_params"
```

This is also the place for anything the kernel would otherwise recompute on every invocation.
Boids precomputes squared ranges and half extents here, which leaves the neighbour loop with no per-neighbour setup.

A model with nothing to extract uses `type Params = ()` and an empty `from_params`.

## Indices

The engine prepends the parameters that every model of a given topology needs.

=== "Grid models"

    ```text
    0  grid_width      engine, reload
    1  grid_height     engine, reload
    2  the model's own
    ```

=== "Agent models"

    ```text
    0  num_agents      engine, reload
    1  world_width     engine, reload
    2  world_height    engine, reload
    3  the model's own
    ...   the field's own
    ```

Your `from_params` receives its own 0-based slice, never the composed list.
The split between the slices comes from the descriptor lengths rather than a hard-coded number, which keeps a model or a field layer from shifting the other's indices when it gains a parameter.

Both GPU traits drop the prefix entirely.
Nothing is prepended and the model spells its whole list out, letting a GPU port mirror the exact parameter order of the CPU model it is compared against.
In practice both GPU ports reuse their counterpart's composed list verbatim.

## Reading them

```bash
cargo run -p henad-cli -- boids --params
```

`--params` prints every id, kind, default and range for a model.
`--set id=value` overrides one value, and the flag can be repeated.
See [the command line](../reference/cli.md) for the full CLI, and [the models](../reference/models.md) for what every shipped model declares.

## Next

- [Statistics](statistics.md) covers the stat series, which a model declares in much the same way.
- [Writing fast models](performance.md) explains the reasoning behind the hot-parameter split.
