---
title: Registering a model
description: Adding a model to the example set so it appears in the app and the CLI.
icon: material/format-list-bulleted
---

# Registering a model

To register a model, add it to `example_models()` in `crates/henad-models/src/lib.rs` through the `register_*` function for its trait.
The call type-erases the model into a `ModelEntry`, and `example_models()` collects the entries into a `ModelSet`.
From there the model appears in the app and in `henad-cli --list`.

```rust
--8<-- "crates/henad-models/src/lib.rs:cpu_entries"
```

| Trait | Function |
|---|---|
| [`GridModel`](grid-models.md) | `register_grid_model::<M>()` |
| [`AgentModel`](agent-models.md) | `register_agent_model::<M>()` |
| [`NetworkModel`](network-models.md) | `register_network_model::<M>()` |
| [`GpuGridModel`](gpu-grid-models.md) | `register_gpu_grid_model::<M>()` |
| [`GpuAgentModel`](gpu-agent-models.md) | `register_gpu_agent_model::<M>()` |

You never write any part of an entry by hand, because the name, parameters, statistics, actions and topology all derive from the trait impl.
The entry also records the model's type path as its source, and the set records the build of the crate that inserted it.

A set holds each id once.
`ModelSet::insert` refuses an id the set already holds, and an id outside the grammar: a lowercase ASCII letter, then lowercase letters, digits and underscores.

## Network entries

`register_network_model` gives the entry the `TopologyHint::NETWORK` hint, for nodes drawn as agents with edges between them and no grid.
The Model tab shows that topology as Network.
The entry's metadata is a `Structure::Network` carrying the chunk size, the node lanes and the edge palette, and the Model tab lists all three.

## GPU entries

A GPU entry holds no device, and builds on the one the host passes to `ModelEntry::build`.
Built with no device, it returns a fault instead of a state.
On a machine without a device the app and the CLI leave the GPU models **out of the list entirely**, instead of listing them and letting them fail on selection, which keeps everything in the dropdown runnable.

A GPU entry also carries a capacity closure.
The app asks it whether the current parameters fit the device, and if they do not it disables Build with a reason instead of letting wgpu take the process down.
See [porting a model to the GPU](porting.md#ask-before-you-allocate).

Adding a GPU model can raise how many storage buffers per shader stage the engine has to request.
Each GPU entry declares that number as its `GpuNeeds`, read from its declared passes, action passes included, and a host requests the merged needs of its set through `ModelSet::gpu_needs` before it has a device.

A GPU model whose passes leave the order of their writes to the GPU declares `REPLAYS_EXACTLY = false`, as `gpu_boids` does.
The entry's metadata carries the flag as `replays_exactly`, and the tests that compare two runs on one seed skip such a model.

## Tests that come with it

The [testing kit](determinism.md#the-testing-kit) checks that a model's declared parameters, topology and stat series match what its state actually does, and it covers GPU entries too when a device is available.

Every CPU entry's state has to return a grid, point or edge view exactly when the entry's hint says the model draws one.
A `Structure::Network` has to come with a hint that has both agents and edges.
Every entry's declared actions are pressed as well.
The state has to accept each one and refuse an index past the last, and no two actions of one model can share an id.

To check the entry landed:

```bash
cargo run -p henad-cli -- --list
cargo run -p henad-cli -- <your-id> --params
```

## Next

- [Determinism and testing](determinism.md) covers the testing kit and the tests you add on top of it.
- [The models](../reference/models.md) shows what a registered entry looks like from the outside.
