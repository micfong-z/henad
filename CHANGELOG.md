# Changelog

This document follows [Keep a Changelog v1.1](https://keepachangelog.com/en/1.1.0/).
Henad uses [semantic versioning](https://semver.org/spec/v2.0.0.html) for its releases.

## [Unreleased]

### Added

- Model metadata in the Model tab in GUI
- Export tab in the GUI app
- Unlimited history setting in the Charts tab
- Model actions for GPU and CPU models (see `henad_core::actions!` and `henad_core::authoring::model::gpu_agent_model::GpuAgentAction`)
- `NetworkModel` trait and engine for models over a graph that can change every tick, with a spring layout and edge drawing in the GUI
- Default network models on CPU: Virus on a Network and Team Assembly
- `henad-cli --act ID@TICK` to replay a model's actions, and a `# edges` section for network models in `henad-cli --export` and the Export tab's Save state
- `next_index` random primitive, a uniform index below a bound
- `bool_param` and `choice_param` builders, and fractional parameters displayed as percentages (`ParamDescriptor::percent`)
- Consistency fixture procedures for the two network models, compared by `scripts/compare_network.py`
- Prepare view timing in the Performance tab, including a network model's layout
- Network edges row in the System tab, showing whether the GPU can draw a network model's edges
- Note from `henad-cli` when the population drifts during the timed steps

### Changed

- `henad-cli --export-stats` prepares each sample the way the GUI does before it draws, so a model's derived statistics match between the two
- The Statistics tab shows fractional values with up to three decimals
- `scripts/bench_matrix.py` skips `team_assembly` unless named with `--models`

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
