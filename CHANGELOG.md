# Changelog

This document follows [Keep a Changelog v1.1](https://keepachangelog.com/en/1.1.0/).
Henad uses [semantic versioning](https://semver.org/spec/v2.0.0.html) for its releases.

## [Unreleased]

## [0.4.0] - 2026-11-26

The app has a new Editor tab for writing models as sheets, node graphs or WGSL grid rules, and running and sweeping them on the GPU without Rust.
henad-cli runs the same model files.

### Added

- Editor tab in the app, for sheets, node graphs and WGSL grid rules. Apply installs the model, and a preview runs while you edit.
- Sheets, a new kind of model file (`kind = "sheet"`, `henad::graph::sheet`). A sheet describes a model in numbered sentences on cards, over cells or agents. Eight starters are included.
- Converting a sheet to a node graph copy.
- Pickers in the card editor grey out values that a line cannot take, and show the reason on hover.
- Card lines can be dragged to a new place, duplicated, and given a note.
- The editor asks before an edit that would also remove lines, measures or other parts of the model.
- henad-graph, a new crate (`henad::graph`) for graph models, with a checker, a host evaluator, a WGSL compiler, edit commands and starters.
- Grid rules (`henad::rules`): a WGSL `step_cell` function, checked and composed at run time.
- `ModelFile` and `henad-cli --model-file`, for running grid rules, graphs and sheets from a file. Errors point at `path:line:column`.
- `GpuGridSpec` and `GpuAgentSpec` for GPU models built at run time, with `ModelEntry::from_gpu_grid_spec`, `from_gpu_agent_spec` and `ModelSet::replace`.
- `ModelCode` and `code_hash`. Manifests record the code of a model built at run time.
- Time scales. A model can declare how long one tick is, and exported stats and `series.csv` get a `time` column.
- Whole-number `f32` parameters (`ParamDescriptor::integer`).
- Percent values such as `2.5%` in `--set`, `--vary`, spec files and the Sweep tab.
- Live parameters and fire counts (`FireCounts`) on the GPU engines.
- A work budget in `Demand`. A configuration whose GPU submission or host work is over the budget is rejected, naming the pass.
- The app builds a model on its own thread, with a Cancel button, and detaches a run that does not stop within 2 s.
- New model dialog, unsaved changes prompt, and Cmd+S (Ctrl+S) to save.
- Exact replay row in the Model tab.
- `CodeDiagnostic`, `pcg4d` in Rust and WGSL, the WGSL module `henad::hash` and `action::parse_entry`.
- Editor guide and model files reference in the docs.
- `SECURITY.md`.

### Changed

- Breaking: descriptor fields are private and read through accessors such as `id()`, `label()` and `kind()`. `ActionDescriptor` is no longer `Copy`, choice options are `ChoiceOptions`, and the `known` lists in errors are `Vec<String>`.
- Breaking: `GridView`, `PointView` and `EdgeView` borrow their palettes.
- Breaking: `ModelMetadata` and `Structure` are `#[non_exhaustive]`, and `ModelMetadata` is built with `new`.
- Breaking: GPU agent builds panic on a position or colour buffer that is not `drawable`, a double-buffered buffer that no step pass writes, or a buffer side bound as both read-only and writable.
- Breaking: `PassBindings` and `Demand` have new fields, and `Demand::push_pass` takes the pass kind and work.
- Breaking: resume and merge reject runtime code that has changed, and a different time scale (`ResumeError::CodeChanged`, `TimeScaleChanged`, `MergeError::CodeChanged`).
- Breaking: `RecordedBuild` is `#[non_exhaustive]`, and `ManifestModel` has new `code` and `author` fields.
- Breaking: the output writers and `ResumeScan::read` take the time scale, the position of the tick length or the `ModelSource`.
- Breaking: `ValueError`, `LevelSpec`, `LevelSpecError` and `FactorDomain` have new variants.
- Breaking: `choice_param` panics on an option that contains `,` or `:` or is named `all`. Debug builds panic on a stat named `run_id` or `tick`, or `time` next to a `time` column.
- Breaking: `SweepOptions` is no longer `UnwindSafe`.
- Breaking: `GpuSimThread::new` takes a `FaultSink`. Dropping a runner waits at most 2 s and then detaches it, and `stop(timeout)` and `runner::Driver::shutdown(timeout)` return `RunnerShutdown`.
- Breaking: `Snapshot` has new `fire_counts` and `run_to_samples` fields.
- The GPU engines are no longer generic. `GpuGridState<M>` and `GpuAgentState<M>` work as before.
- The GPU runner follows Target TPS, and publishes a snapshot once its stats have been read back.
- A number typed into a slider or into the Sweep tab's range fields applies on Enter or when the field loses focus, not on every key.
- Reworded error messages and CLI help.
- Requires egui 0.36.2 and wgpu 30.0.2 or later. On macOS the window is now colour-managed as sRGB.
- New Henad icon for the app, the web app and the docs site.
- The template's own files are dedicated to the public domain under CC0 1.0.
- Amended deprecation policy (Stability in the releasing guide).
- Benchmarks re-run on a Ryzen 9 9950X3D with an RTX 4090.

### Removed

- Breaking: `GpuCommand`. Use `GpuSimThread::set_batch_size`, `set_adaptive` or `set_target_ms` instead.

### Fixed

- Panic locations lost when several rayon workers panicked at once.
- Typing 10000 into a slider with a step of 50 giving 10001, and "NaN" setting a slider to its minimum or maximum.
- Whole-number sliders wrapping to negative values above 2147483647.
- Charts legend sorted alphabetically instead of in the order of the stats.
- Viewport Density mode panicking when there are no agents.
- Tab stopping on resizable panel dividers.
- `write_grid` panicking on a width of 0.
- Save recording in the Export tab naming the loaded model and the current tick instead of the recorded ones.
- GPU searches building candidates that the device cannot hold.
- The GPU runner publishing the previous stats after a pause, step or action.
- Template checkouts on Windows using CRLF line endings, and its fetch commands creating a branch that its CI does not run on.

### Security

- Updated `url` to 2.5.9 (RUSTSEC-2026-0592).

### Migrating to 0.4

- Update `henad` and `henad-build` to `"0.4"`.
- Read descriptor fields through their accessors. Build descriptors with the helpers, or with `ParamDescriptor::new`, `StatDescriptor::owned` and `ActionDescriptor::owned`. Clone an `ActionDescriptor` where it was copied, and pass `&[StatDescriptor]` to `stat_entries`.
- Build metadata with `ModelMetadata::new(backend, structure).palette(..).replays_exactly(..)`, add a wildcard arm to matches on `Structure`, and read its lists with `.iter()`.
- In GPU agent models, mark position and colour buffers `drawable`, write the next side of each double-buffered buffer in a step pass, and bind only one side of a buffer in each pass.
- Pass the kind and work to `Demand::push_pass`. Add the new fields to `PassBindings` and `Demand` literals, or start from `Demand::default()`.
- Add the new error and value variants to exhaustive matches. Build a `RecordedBuild` with `From`, pass `entry.source()` to `ResumeScan::read` and the time scale to `OutputDir::open_writer` and `append_writer`, and add `code: None` and `author: None` to `ManifestModel` literals.
- Wrap a closure that captures a `SweepOptions` in `AssertUnwindSafe` before passing it to `catch_unwind`.
- Pass `ctx.faults.clone()` as the last argument of `GpuSimThread::new`. Call `stop(timeout)` where the thread has to be gone, and add `fire_counts` and `run_to_samples` to `Snapshot` literals.
- Rename choice options that contain `,` or `:` or are named `all`, and stats named `run_id`, `tick` or `time`.

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

[Unreleased]: https://github.com/micfong-z/henad/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/micfong-z/henad/releases/tag/v0.4.0
[0.3.0]: https://github.com/micfong-z/henad/releases/tag/v0.3.0
[0.2.0]: https://github.com/micfong-z/henad/releases/tag/v0.2.0
[0.1.0]: https://github.com/micfong-z/henad/releases/tag/v0.1.0
