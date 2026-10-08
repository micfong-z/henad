---
title: API reference
description: The crates Henad publishes, which one a program depends on, and the module tree of the henad crate.
icon: material/api
---

# API reference

Henad publishes eight crates to crates.io, all at one version.
Each crate's API documentation is on docs.rs.

## Which crate to depend on

A program or a model crate depends on **`henad`**, and on **`henad-build`** as a build dependency when it registers its own models.
Every item a model, a host or a test needs has a path in `henad`, and the guides refer to items by those paths alone.
The code they include from the example models is the exception, since henad-models sits below the facade and refers to `henad_core` and `henad_compute`.
The [template](../guide/your-project.md) depends on these two Henad crates and no other.

The other six are the crates `henad` re-exports.
A program never needs to name them, and their paths can change between releases, while the facade paths stay the same.

| Crate | Documentation | Holds |
|---|---|---|
| `henad` | [docs.rs/henad](https://docs.rs/henad) | The facade: one module tree over the others, with the example models, the app, the command line and the testing kit behind features |
| `henad-build` | [docs.rs/henad-build](https://docs.rs/henad-build) | The build-script side: shader bindings and the commit stamp |
| `henad-core` | [docs.rs/henad-core](https://docs.rs/henad-core) | The model traits, the primitives, the shared WGSL, and the parts of a sweep that need no engine |
| `henad-compute` | [docs.rs/henad-compute](https://docs.rs/henad-compute) | The engines, the runners, model entries and sets, and `Simulation` |
| `henad-explore` | [docs.rs/henad-explore](https://docs.rs/henad-explore) | Sweeps, searches, results folders and the testing kit |
| `henad-models` | [docs.rs/henad-models](https://docs.rs/henad-models) | The ten example models |
| `henad-cli` | [docs.rs/henad-cli](https://docs.rs/henad-cli) | The command line as a library, and the `henad-cli` binary |
| `henad-app` | [docs.rs/henad-app](https://docs.rs/henad-app) | The app as a library, and the `henad-app` binary |

## Features of `henad`

| Feature | Adds |
|---|---|
| `example-models` | `henad::models`, the ten example models and `example_models()` |
| `app` | `henad::app`, the app over a host's own model set |
| `cli` | `henad::cli`, the command line over a host's own model set, on native targets |
| `testing` | `henad::testing`, the [testing kit](../authoring/testing.md) |

No feature is on by default, and no feature changes a result.

## The module tree

| Path | Holds |
|---|---|
| `henad` | `ModelEntry`, `ModelSet`, `RunSetup`, `Simulation`, `StatSample`, `Fault`, `install_panic_hook`, `BuildInfo`, `ModelSource`, `ModelMetadata`, and the macros `params!`, `actions!`, `agent_lanes!`, `for_each_chunk_mut!`, `buffers!`, `build_info!` and `include_shaders!` |
| `henad::prelude` | The names a program imports whole to build, run and sweep models |
| `henad::authoring` | Everything a model is written against: the five model traits and their types, the `register_*` functions, the helpers, `Grid2D`, `Network`, `SpatialHash` and the engine constants |
| `henad::authoring::prelude` | The names a model's source file imports whole |
| `henad::authoring::primitives` | `rng`, `space` and `wgsl`, each named after its WGSL twin's import path |
| `henad::params` | Parameter descriptors and values, and the text form that `--set` accepts |
| `henad::stats` | Statistic descriptors and values, and the CSV writer for stat series |
| `henad::views` | The grid, point and edge views that a CPU model passes to a host |
| `henad::action` | Action descriptors and the schedule of actions that a run fires |
| `henad::gpu` | `GpuContext`, `GpuNeeds`, `acquire_headless`, `raise_limits` for a host that requests its own device, and `wgpu` itself |
| `henad::runner` | The paced runners `SimThread` and `GpuSimThread`, and the snapshots they publish |
| `henad::engine` | The engine states that a test builds directly, without an entry |
| `henad::explore` | Sweeps, searches, results folders and replay: `run_spec`, `plan_spec`, `SweepRun`, `ResultSet`, `Replay`, the manifest, and the planning modules `spec`, `plan`, `design`, `search` and the rest |
| `henad::benchmark` | `run_benchmark`, the benchmark `henad-cli` runs, on native targets |
| `henad::models` | The example models, with `example-models` |
| `henad::app` | `AppOptions`, `AppOpening`, `OpenAt`, `run_native` and `start_web`, with `app` |
| `henad::cli` | `CliOptions` and `run`, with `cli` |
| `henad::testing` | `assert_set_conforms`, `check_model_set`, `CheckSettings`, `headless_test_device`, with `testing` |

[Using Henad from code](../guide/library.md) walks through a program over these paths, and [Releasing](../developing/releasing.md#stability) says what a release may change in them.
