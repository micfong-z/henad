---
title: Parameters
description: The parameter declarations a model writes, and the difference between a live edit and one needing a reload.
icon: material/tune
---

# Parameters

Every slider in the app and every `--set` override in the CLI reads from a single declaration list on the model.
You write the id, label, range and default together in one place, and both front ends work from those declarations, leaving no second list to keep in step.
This page covers the declaration API, when an edit lands live, when it needs a rebuild, the way the indices compose, and the actions a model offers beside its parameters.

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

`bool_param` and `choice_param` cover the other two kinds.
`bool_param` takes an id, a label and a default, and `choice_param` takes an id, a label, the option labels and the index of the default option.
Virus on a Network declares all four.

```rust
--8<-- "crates/henad-models/src/virus_network/mod.rs:params"
```

`NETWORKS` is `&["Random", "Geometric"]`, and `RANDOM` is 0.
The Parameters tab draws a number as a slider, a bool as a checkbox and a choice as a dropdown.
Each kind has a matching reader, `extract_f32`, `extract_u32`, `extract_bool` or `extract_choice`.
All four take the slice, an index and a fallback, and return the fallback when that index is missing or holds another kind.
`extract_choice` returns the index of the chosen option.

`.percent()` sets a parameter's `format` to `ParamFormat::Percent`, for a fraction shown as a percentage.
The stored value stays a fraction in `0..=1`, and only the Parameters tab scales it.
Virus Spread Chance holds `0.025` and reads 2.5%.
`--set` takes the fraction too, and `--params` marks the parameter `format=percent`.

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

=== "Network models"

    ```text
    0  num_agents      engine, reload
    1  world_width     engine, reload
    2  world_height    engine, reload
    3  the model's own
    ```

    The node count keeps the id `num_agents` for the benchmark scripts, and the app labels it Number of Nodes.

Your `from_params` receives its own 0-based slice, never the composed list.
The split between the slices comes from the descriptor lengths rather than a hard-coded number, which keeps a model or a field layer from shifting the other's indices when it gains a parameter.

Both GPU traits drop the prefix entirely.
Nothing is prepended and the model spells its whole list out, letting a GPU port mirror the exact parameter order of the CPU model it is compared against.
In practice the two GPU agent ports reuse their counterpart's composed list verbatim, through `agent_model_param_descriptors`.
The two GPU grid ports spell theirs out, with the same ids in the same order.

## Reading them

```bash
cargo run -p henad-cli -- boids --params
```

`--params` prints every id, kind, default and range for a model.
`--set id=value` overrides one value, and the flag can be repeated.
A bool takes `true` or `false`, and a choice takes an option label or its index.
`--set network=Geometric` and `--set network=1` pick the same option.
See [the command line](../reference/cli.md) for the full CLI, and [the models](../reference/models.md) for what every shipped model declares.

## Actions

An action is a one-off change to the state that the user asks for between ticks, such as Game of Life's Randomise and Clear.
You declare actions next to the parameters, through the `actions!` macro.

```rust
--8<-- "crates/henad-models/src/game_of_life.rs:actions"
```

An `ActionDescriptor` holds an id and a label.
The id is the name `henad-cli --act` matches, and the label is the text on the button.
Like `params!`, the macro expands to one `const` per entry holding its index, along with an `ACTION_SPECS` slice for your impl to forward to `ACTIONS`.

```rust
--8<-- "crates/henad-models/src/game_of_life.rs:action_specs"
```

A model with no actions leaves `ACTIONS` at its default, the empty slice.

When the user presses a button, the engine calls `act` with that entry's index, and you match on the generated constants.

```rust
--8<-- "crates/henad-models/src/game_of_life.rs:act"
```

`act` takes the index plus what `init` takes, and on an agent model the field as well.

=== "Grid models"

    ```rust
    fn act(action: usize, grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64);
    ```

=== "Agent models"

    ```rust
    fn act(
        action: usize,
        lanes: &mut Self::Lanes,
        field: &mut Self::Field,
        extent: Extent,
        params: &[ParamValue],
        rng: &mut u64,
    );
    ```

=== "Network models"

    ```rust
    fn act(action: usize, nodes: &mut Nodes<'_, Self>, extent: Extent, params: &[ParamValue], rng: &mut u64);
    ```

    `nodes` holds the lanes, the graph and the model's `Aux`, the same as in `init`.
    An action can change the graph through it.
    Virus on a Network's Rewire a link moves one random edge to a pair of nodes not yet joined.

`params` is the model's own slice of the values the state holds.
A live parameter reads its current value there, and a reload parameter reads the value the state was built with.
Game of Life's Initial Density is a reload parameter, and Randomise refills at the density the model was built with, even after the slider has moved.

`rng` is a stream of its own, apart from the one the ticks draw from.
The engine seeds it from the state's seed through `action_seed`, and each press carries on where the last one stopped.
Pressing twice draws twice, and a press leaves the next tick's draws where they were.

The engine runs an action between two ticks and publishes a snapshot straight after it.
The result shows even while the simulation is paused.
The Parameters tab draws one button per action under the parameter widgets, disabled until the selected model is built.
`henad-cli --act ID@TICK` runs one when the state reaches that tick, and [the command line](../reference/cli.md) covers the flag.

A registry test presses every declared action on a freshly built state, and asserts that the state accepts each one and refuses an index past the last.
A second one asserts that no two actions of a model share an id, since `--act` could not tell them apart.

## Actions on the GPU

On the GPU an action is a compute pass of its own, dispatched once per press.

=== "GPU grid models"

    A `GpuGridAction` holds the descriptor, a shader and that shader's bindings.
    The pass is dispatched over `step_dims` like a step, and its uniform block comes from `action_params_bytes`.
    By default that is the step's block, enough for an action that needs nothing but the dimensions.

    ```rust
    fn action_params_bytes(action: usize, width: u32, height: u32, params: &[ParamValue], seed: u32) -> Vec<u8>;
    ```

=== "GPU agent models"

    A `GpuAgentAction` holds the descriptor and a `PassSpec`, dispatched once over the pass's own domain.
    The engine asks `pass_params_bytes` for its uniform block with `PassId::Action(i)`, where `i` is the action's index.
    `PassCtx::seed` carries the seed.
    Every other pass sees a seed of zero.

The seed is fresh on every press.
A shader that needs random numbers derives them by hashing the seed.
GPU SIR's Seed outbreak draws from it instead of from the per-cell `rng` buffer, and the run's own stream stays where it was.

An action pass writes the model's buffers in place.
Nothing swaps after it, so its write bindings resolve to the side that holds the current state, the same side its read bindings see.
Its bindings count towards the model's storage-buffer demand like those of any other pass.

A GPU action need not reproduce its CPU counterpart.
GPU Game of Life's Randomise hashes one draw per cell where the CPU model walks a stream, and the two refill the grid differently.

## Next

- [Statistics](statistics.md) covers the stat series, which a model declares in much the same way.
- [Writing fast models](performance.md) explains the reasoning behind the hot-parameter split.
