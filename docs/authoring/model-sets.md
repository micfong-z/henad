---
title: Model sets
description: Registering a model, and the set of models a host offers in the app and the command line.
icon: material/format-list-bulleted
---

# Model sets

A model joins the app and the command line through a **model set**, the `ModelSet` a host is handed.
The template's `models()` in `src/lib.rs` builds the set of a project, and the app, the command line and the test all read their models from it.

!!! info "Henad 0.3"

    This page describes Henad 0.3.

``` rust title="src/lib.rs"
--8<-- "templates/model-project/src/lib.rs"
```

## Registering a model

The `register_*` function for a model's trait type-erases the model into a `ModelEntry`, and `ModelSet::insert` adds the entry to the set.

| Trait | Function |
|---|---|
| [`GridModel`](grid-models.md) | `register_grid_model::<M>()` |
| [`AgentModel`](agent-models.md) | `register_agent_model::<M>()` |
| [`NetworkModel`](network-models.md) | `register_network_model::<M>()` |
| [`GpuGridModel`](gpu-grid-models.md) | `register_gpu_grid_model::<M>()` |
| [`GpuAgentModel`](gpu-agent-models.md) | `register_gpu_agent_model::<M>()` |

All five sit at `henad::authoring`.
Adding a model to a project is a `mod` line and an `insert` line in `src/lib.rs`, and an import of its `register_*` function when the file lacks it.

You never write any part of an entry by hand.
The name, parameters, statistics, actions and topology all derive from the trait impl.
The engines and the kernels compile in the crate that calls `register_*`, at that crate's opt-level, and the template sets its [build profiles](../guide/your-project.md#build-profiles) for them.

To check the entry landed:

```bash
cargo run --bin my-model-cli -- --list
cargo run --bin my-model-cli -- <your-id> --params
```

## Ids

A set holds each id once.
`insert` refuses an id the set already holds, and an id outside the grammar: a lowercase ASCII letter, then lowercase letters, digits and underscores.
An id names the model on the command line, in a spec file and in every results folder, so keep it once a sweep has used it.

## Sources

Every entry records its **source**: the type path of the model, and the build of the crate that registered it.
`ModelSet::new` takes that build, and `henad::build_info!()` returns the build of the crate it expands in.
A set records its build on every entry inserted without one, and an entry that already carries a source keeps it.

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
```

A crate that holds models builds its own set this way, and calls `henad_build::stamp_commit()` from its `build.rs`.
The build then carries the commit, whether the sources differed from it, and a hash of the sources.
A results folder records each model's source, and a resume, a merge or a replay warns when it differs.
[Your own project](../guide/your-project.md#the-build-script) covers the stamp.

A model library exports a `models()` built from its own `build_info!()`, and its entries keep the library's name and version in any set they join.
`ModelSet::extend` adds a whole set, and each entry keeps its source:

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
models.extend(my_model::models()?)?;
```

`extend` checks every id before it adds any, and leaves the set unchanged when two ids clash.

## The example models

With the `example-models` feature on, `henad::models::example_models()` returns the ten [example models](../reference/models.md), with henad-models as their source.
It registers them in two groups:

``` rust title="crates/henad-models/src/lib.rs"
--8<-- "crates/henad-models/src/lib.rs:cpu_entries"
--8<-- "crates/henad-models/src/lib.rs:gpu_entries"
```

A project that wants every example model beside its own extends its set with them:

```rust
models.extend(henad::models::example_models())?;
```

A project that wants a few takes clones of their entries:

```rust
let examples = henad::models::example_models();
for id in ["sir", "boids"] {
    models.insert(examples.get(id).ok_or("the example set holds the model")?.clone())?;
}
```

Each clone keeps henad-models as its source.

## Network entries

`register_network_model` gives the entry the `TopologyHint::NETWORK` hint, for nodes drawn as agents with edges between them and no grid.
The Model tab shows that topology as Network.
The entry's metadata is a `Structure::Network` carrying the chunk size, the node lanes and the edge palette, and the Model tab lists all three.

## GPU entries

A GPU entry holds no device, and builds on the one the host passes to `ModelEntry::build`.
Built with no device, it returns a fault instead of a state.
On a machine without a device the app and the command line leave the GPU models **out of the list entirely**, instead of listing them and letting them fail on selection.

A GPU entry also carries a capacity check.
The app asks it whether the current parameters fit the device, and if they do not it disables Build with a reason instead of letting wgpu take the process down.
See [porting a model to the GPU](porting.md#ask-before-you-allocate).

### Device needs

A WebGPU device offers eight storage buffers per shader stage by default, and a pass can bind more only on a device asked for more.
Each GPU entry declares what its widest pass binds as its `GpuNeeds`, read from its declared passes, action passes included.
`ModelSet::gpu_needs` merges the needs of every GPU entry, and a host requests its device for them before any model builds.
The app, the command line and `henad::gpu::acquire_headless` all do.
A browser at the default limits refuses a model whose defaults need more, and such a model fails the kit's [`DefaultsFit` check](testing.md#the-checks) unless its test exempts it.

### Exact replay

A GPU model whose passes leave the order of their writes to the GPU, as atomic additions to one cell do, can step two runs of one seed to different states.
Such a model declares `REPLAYS_EXACTLY = false`, as the example `gpu_boids` does.
The entry's metadata carries the flag as `replays_exactly`, and a results folder records it.
The testing kit skips the checks that compare two runs on one seed, and the app notes beside a run's Open button that its replay can drift.
Every other model replays a recorded run exactly.

## Next

- [Testing your model](testing.md) covers the checks the template's test runs over the set.
- [The example models](../reference/models.md) shows what a registered entry looks like from the outside.
