---
title: Registering a model
description: Adding a model to the registry so it appears in the app and the CLI.
icon: material/format-list-bulleted
---

# Registering a model

To register a model, add it to `model_registry()` in `crates/henad-models/src/registry.rs` through the `register_*` function for its trait.
The call type-erases the model into a `ModelEntry`, and from there it appears in the app and in `henad-cli --list`.

```rust
--8<-- "crates/henad-models/src/registry.rs:cpu_entries"
```

| Trait | Function |
|---|---|
| [`GridModel`](grid-models.md) | `register_grid_model::<M>()` |
| [`AgentModel`](agent-models.md) | `register_agent_model::<M>()` |
| [`GpuGridModel`](gpu-grid-models.md) | `register_gpu_grid_model::<M>(&ctx)` |
| [`GpuAgentModel`](gpu-agent-models.md) | `register_gpu_agent_model::<M>(&ctx)` |

You never write any part of an entry by hand, because the name, parameters, statistics and topology all derive from the trait impl.

## GPU entries

The two GPU functions take a `GpuContext`, and the whole GPU half of the registry is built only when one is supplied.
On a machine without a device those models are **omitted entirely**, instead of being listed and left to fail on selection, which keeps everything in the dropdown runnable.

A GPU entry also carries a capacity closure.
The app asks it whether the current parameters fit the device, and if they do not it disables Build with a reason instead of letting wgpu take the process down.
See [porting a model to the GPU](porting.md#ask-before-you-allocate).

Adding a GPU model can raise how many storage buffers per shader stage the engine has to request.
`gpu_storage_bindings_needed()` works that number out by walking every model's declared passes, and a test enforces that its list stays in step with the registry's.

## Tests that come with it

The [registry tests](determinism.md#tests-the-registry-brings) confirm that a model's declared parameters, topology and stat series match what its state actually does, and they cover GPU entries too when a device is available.

To check the entry landed:

```bash
cargo run -p henad-cli -- --list
cargo run -p henad-cli -- <your-id> --params
```

## Next

- [Determinism and testing](determinism.md) covers the tests you add on top of the registry's own.
- [The models](../reference/models.md) shows what a registered entry looks like from the outside.
