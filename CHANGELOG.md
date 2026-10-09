# Changelog

This document follows [Keep a Changelog v1.1](https://keepachangelog.com/en/1.1.0/).
Henad uses [semantic versioning](https://semver.org/spec/v2.0.0.html) for its releases.

## [Unreleased]

## [0.3.0] - 2026-10-10

Henad as libraries on crates.io: your own program can build, run, sweep, test and show your own models.

### Added

- `henad` facade crate: one module tree over engine, sweeps and model authoring, with `example-models`, `app`, `cli` and `testing` features.
- All crates published on crates.io: henad-core, henad-build, henad-compute, henad-models, henad-explore, henad-cli, henad-app, henad.
- `ModelSet` for the models a host offers, and `henad_models::example_models()` with the ten example models.
- `henad_compute::simulation`: build, step, sample, edit and export a model from Rust (`RunSetup`, `Simulation`).
- `run_spec` and `plan_spec` to run or plan a sweep or search from a spec, and `run_benchmark`.
- henad-build: shader bindings (`ShaderBuild`) and build stamps (`stamp_commit`) from a build script, brought in by `include_shaders!`.
- Build provenance (`build_info!`, `BuildInfo`, `ModelSource`): manifests record Henad, host and model builds per session, and resume, merge and the app warn when they differ.
- henad-cli and henad-app as libraries (`henad_cli::run` with `CliOptions`, `henad_app::run_native` or `start_web` with `AppOptions`), the app openable on a results folder, a recorded run or a setup.
- Testing kit (`henad_explore::testing`): `assert_set_conforms` checks declarations, determinism across thread counts and seeds, and GPU submissions.
- `REPLAYS_EXACTLY` on GPU model traits, `GpuNeeds` for device features a model or set needs, `pcg_hash` in authoring primitives.
- Lock on a results folder while a sweep, search or merge writes it.
- Project template (`templates/model-project`): CPU and GPU model, own app, CLI, tests, CI and web build.
- `examples/tutorial` with the first-model guide's finished code, and `crates/henad/examples/complete.rs`, a whole program on the library.
- Guide pages: your own project, Henad from Rust, model sets, testing models, releasing.
- CI jobs for MSRV, docs.rs builds, packaging, and the template built against the packaged crates.

### Changed

- Breaking: `ModelEntry`, `ModelState` and `register_*` moved from `henad_models::registry` to `henad_compute::entry`; an entry builds on the device passed to `ModelEntry::build`.
- Breaking: shared WGSL modules moved to henad-core, imported as `henad::space`, `henad::rng`, `henad::dims`, `henad::reduce_tree` and `henad::dispatch` (was `shared::prelude`) in place of `shared::`.
- Breaking: `buffers!` emits `BUFFER_SPECS` (was `SPECS`), and `agent_lanes!` implements `Debug` (drop any `#[derive(Debug)]` on lanes).
- Breaking: `SweepOptions`, `SweepRunOptions` and `Provenance` built through `new`; `gpu_memory` renamed `gpu_memory_budget`; `SweepWarning::CommitChanged` renamed `BuildChanged`.
- Breaking: `FaultKind`, `ExploreError`, `SweepOutput` and `ShaderBuildError` are `#[non_exhaustive]` (match needs a wildcard arm), and several other error enums have new variants.
- Breaking: `acquire_headless` and `limits::raise` take `GpuNeeds`, `ResultSet::replay` takes a `ModelSchema` (`entry.schema()`), `stepping::sample_stats` returns `Result`, `ProgressUpdate` times are `Duration`s.
- Breaking: sweeps no longer install the panic hook; call `henad_compute::fault::install_panic_hook()` once.
- Breaking: `henad-cli` exits 2 (was 1) on a malformed `--set`, `--act`, `--vary`, `--stop` or `--reduce` value, and on `--spec` without `--out`, `--dry-run` or `--params`.
- Official app renamed Henad (was Henad Engine); native settings start afresh in a folder of the new name.
- App text fonts rebuilt from IBM Plex as Henad Sans and Henad Mono; henad-app's licence names the font licences.
- About window shows the Henad and model builds.
- Web build on one pinned nightly toolchain and Trunk release.
- Install line: `cargo install --locked --config profile.release.opt-level=2 henad-app henad-cli`.

### Removed

- Breaking: `henad_models::registry` and the `Model` trait; use `example_models()`, `ModelSet`, and `register_gpu_grid_model` or `register_gpu_agent_model`.
- Breaking: `run_sweep`, `run_search` and `model_schema`; use `run_spec` and `ModelEntry::schema()`.
- Breaking: henad-app's public `HenadApp`, `state` and `ui`; use `run_native` or `start_web`.

### Fixed

- `gpu_ants` ignoring its Evaporation parameter.
- `gpu_boids` overstating its neighbour index's memory and being refused on large worlds.
- GPU stat reduction writing past its buffer above about 16.7 million agents or cells.
- Lost GPU device unreported by `--export-stats` and the GPU benchmark.
- Paused one-lane sweep stalling other simulations in the same process.
- PSE search failing to resume when its axis came from Use range from results.
- Huge specs and search batches running out of memory in place of a planning error.
- Resume and merge miscounting a killed session's runs and naming the wrong build.
- `wrap_coord` able to return the world size itself.
- Browser Save reporting files saved that never downloaded, and browser GPU Run to recording a zero row at tick 0.
- App crash or freeze on a grid cell past the palette, a results folder with an invalid search, or a huge design.

## [0.2.0] - 2026-10-01

### Added

- Model metadata in the Model tab in GUI.
- Export tab in the GUI app.
- Unlimited history setting in the Charts tab.
- Model actions for GPU and CPU models (see `henad_core::actions!` and `henad_core::authoring::model::gpu_agent_model::GpuAgentAction`).
- `NetworkModel` trait and engine for models on a graph that can change every tick, with edges drawn in the GUI app.
- Default network models on CPU: Virus on a Network and Team Assembly.
- `henad-cli --act ID@TICK` to replay a model's actions.
- Parameter sweeps in `henad-cli` (`--vary`, or a TOML spec file through `--spec`) over factorial, zip, Latin hypercube, random and CSV designs.
- Sharding, merging and resuming of sweeps (`--shard`, `--merge`, `--resume`), and several CPU or GPU runs at once.
- In-engine search: random search, hill climbing, a genetic algorithm and Pattern Space Exploration.
- Sweep and Results tabs in the GUI app, which can open any run of a sweep in the viewport.
- Seed field, scheduled actions and Run to tick in the GUI app.
- `henad-explore` crate and `henad_core::explore` module.

### Changed

- `henad-cli --export-stats` prepares each sample as the GUI does, so derived statistics match between the two.
- `FaultKind` and `SimCommand` have new variants, and an exhaustive `match` on either needs new arms.
- The GPU stats readback polls return a `StatsPoll` (`GpuSimState::poll_stats_readback` and the `poll` and `poll_blocking` of the readback primitives), `ModelFactory` is thread-safe, and `GpuContext` is built through `GpuContext::new` only.

## [0.1.0] - 2026-09-08

This is the initial release.

### Added

- `GridModel`, `AgentModel`, `GpuGridModel`, `GpuAgentModel` traits and engines for running them.
- Default models on CPU and GPU: Game of Life, SIR epidemic, boids flocking, ant foraging.
- GUI application and CLI application.
- Documentation.
- Benchmarking harness and cross-ABM implementations for comparison.
- Model authoring tools, primitives, and consistency tests.

[Unreleased]: https://github.com/micfong-z/henad/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/micfong-z/henad/releases/tag/v0.3.0
[0.2.0]: https://github.com/micfong-z/henad/releases/tag/v0.2.0
[0.1.0]: https://github.com/micfong-z/henad/releases/tag/v0.1.0
