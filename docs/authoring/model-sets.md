---
title: Model sets
description: Registering a model, and the set of models a host offers in the app and the command line.
icon: material/format-list-bulleted
---

# Model sets

A model joins the app and the command line through a **model set**, the `ModelSet` that a host receives.
The template's `models()` in `src/lib.rs` builds a project's set, and the app, the command line and the test all read their models from it.

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
Adding a model to a project is a `mod` line and an `insert` line in `src/lib.rs`, and an import of its `register_*` function when `src/lib.rs` does not import it yet.

You never write any part of an entry by hand.
The name, parameters, statistics, actions and topology all derive from the trait impl.
The engines and the kernels compile in the crate that calls `register_*`, at that crate's opt-level, and the template sets its [build profiles](../guide/your-project.md#build-profiles) for them.

To check that the entry was added:

```bash
cargo run --bin my-model-cli -- --list
cargo run --bin my-model-cli -- <your-id> --params
```

## Ids

A set holds each id once.
`insert` rejects an id that the set already holds, and an id outside the grammar: a lowercase ASCII letter, then lowercase letters, digits and underscores.
An id identifies the model on the command line, in a spec file and in every results folder, so keep it once a sweep has used it.

`ModelSet::replace` puts an entry in place of the entry with its id, at the same position in the list, and returns the entry it replaced.
When the set holds no entry with that id, `replace` adds the entry at the end and returns `None`.
It rejects an id outside the grammar, as `insert` does.

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
models.extend(henad::models::example_models())?;
let example_sir = models.replace(register_grid_model::<MySir>())?;
```

Here `MySir` declares the id `sir`, takes the example model's place in the list, and records the project's build.
A host also replaces an entry when the code behind an id changes while it runs.
A sweep or a simulation holds its own clone of the entry, and keeps running the code it started with.

## Sources

Every entry records its **source**: the type path of the model, and the build of the crate that registered it.
`ModelSet::new` takes that build, and `henad::build_info!()` returns the build of the crate it expands in.
A set records its build on every entry inserted without a build, and an entry that already carries a source keeps it.

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
```

A crate that holds models builds its own set this way, and calls `henad_build::stamp_commit()` from its `build.rs`.
The build then carries the commit, whether the sources differed from it, and a hash of the sources.
A results folder records each model's source, and a resume or a merge warns when it differs, as the app does before it opens a run.
[Your own project](../guide/your-project.md#the-build-script) covers the stamp.

A model library exports a `models()` built from its own `build_info!()`, and its entries keep the library's name and version in any set they join.
`ModelSet::extend` adds a whole set, and each entry keeps its source:

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
models.extend(my_model::models()?)?;
```

`extend` checks every id before it adds any entry, and leaves the set unchanged when two ids clash.

### Origins

A source also records how the model's code reached the process, as a `CodeOrigin`:

| Origin | Code | Identified by |
|---|---|---|
| `Compiled` | Compiled into the host from a crate it links, as every `register_*` entry is | The crate's source hash |
| `Runtime` | Composed at run time from text, such as a grid rule from a model file | The hash of its text |
| `Plugin` | Compiled from a crate into a library that a dev host loaded while it ran | The crate's source hash |

`ModelSource::origin` returns the origin, and `ModelSource::code_hash` the hash that identifies the code.
A runtime source holds its text as a `ModelCode`, the canonical text and the name and version of its format, and `ModelSource::code` returns it.
`ModelSource::runtime` builds such a source, and its type path reads `runtime:` and the label it was given.
A set never records its build on a runtime source.
The text identifies the code, and the host's build says nothing about it.
A plugin's source keeps the build of the plugin's crate, and a set records none on it either.

A runtime or plugin model can take the id of a compiled model, and the results folder records that its code differs.
A results folder records each model's origin and code hash, and a runtime model's text in the manifest.
A changed compiled build raises a warning on a resume or a merge.
Changed runtime or plugin code makes a resume or a merge reject the folder, as [Sweeps](../guide/sweeps.md#resuming-a-sweep) describes.
A plugin built from unchanged sources is the same code as the compiled crate, and resumes its folder.

## The example models

With the `example-models` feature on, `henad::models::example_models()` returns the ten [example models](../reference/models.md), with henad-models as their source.
It registers the six CPU models, then the four GPU models, and inserts them all into one set:

``` rust title="crates/henad-models/src/lib.rs"
--8<-- "crates/henad-models/src/lib.rs:example_models"
```

A project that wants every example model beside its own models extends its set with them:

```rust
models.extend(henad::models::example_models())?;
```

A project that wants only some example models inserts clones of their entries:

```rust
let examples = henad::models::example_models();
for id in ["sir", "boids"] {
    models.insert(examples.get(id).ok_or("the example set lacks this model")?.clone())?;
}
```

Each clone keeps henad-models as its source.

## Network entries

`register_network_model` assigns the entry the `TopologyHint::NETWORK` hint, for nodes drawn as agents with edges between them and no grid.
The Model tab shows that topology as Network.
The entry's metadata is a `Structure::Network` carrying the chunk size, the node lanes and the edge palette, and the Model tab lists all three.
`ModelMetadata` and `Structure` are `#[non_exhaustive]`, and a host that matches on `entry.metadata().structure` takes a wildcard arm.

## GPU entries

A GPU entry holds no device, and builds on the device that the host passes to `ModelEntry::build`.
Built with no device, it returns a fault instead of a state.
On a machine without a device the app and the command line leave the GPU models **out of the list entirely**, instead of listing them and letting them fail on selection.

A GPU entry also carries a capacity check.
The app asks it whether the current parameters fit the device, and if they do not it disables Build with a reason instead of letting wgpu take the process down.
See [porting a model to the GPU](porting.md#ask-before-you-allocate).

### Device needs

A WebGPU device offers eight storage buffers per shader stage by default, and a pass can bind more only on a device requested with a higher limit.
Each GPU entry declares what its widest pass binds as its `GpuNeeds`, read from its declared passes, action passes included.
`ModelSet::gpu_needs` merges the needs of every GPU entry, and a host requests a device with those needs before any model builds.
The app, the command line and `henad::gpu::acquire_headless` all request a device this way.
A browser at the default limits rejects a model whose defaults exceed those limits, and such a model fails the kit's [`DefaultsFit` check](testing.md#the-checks) unless its test exempts it.

### Exact replay

A GPU model whose passes leave the order of their writes to the GPU, as atomic additions to one cell do, can step two runs of one seed to different states.
Such a model declares `REPLAYS_EXACTLY = false`, as the example `gpu_boids` does.
The entry's metadata carries the flag as `replays_exactly`, and a results folder records it.
The testing kit skips the checks that compare two runs on one seed, and the app notes beside a run's Open button that its replay can drift.
Every other model replays a recorded run exactly.

## Next

- [Testing your model](testing.md) covers the checks the template's test runs over the set.
- [The example models](../reference/models.md) shows what a registered entry looks like from the outside.
