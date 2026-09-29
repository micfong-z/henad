# Changelog

This document follows [Keep a Changelog v1.1](https://keepachangelog.com/en/1.1.0/).
Henad uses [semantic versioning](https://semver.org/spec/v2.0.0.html) for its releases.

## [Unreleased]

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

[Unreleased]: https://github.com/micfong-z/henad/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/micfong-z/henad/releases/tag/v0.1.0
