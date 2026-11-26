# Changelog

This document follows [Keep a Changelog v1.1](https://keepachangelog.com/en/1.1.0/).
Henad uses [semantic versioning](https://semver.org/spec/v2.0.0.html) for its releases.

## [Unreleased]

## [0.4.0] - 2026-11-26

Models without Rust: write a sheet, a node graph or a WGSL grid rule in the app's new Editor tab, then run and sweep it on the GPU, in the app or with henad-cli.

### Added

- Editor tab in the app for sheets, node graphs and WGSL grid rules: Apply, live preview, undo, Open and Save model (Cmd+S), an unsaved-changes question, and an Editor layout in the View menu.
- Sheets (`kind = "sheet"` model files, `henad::graph::sheet`): models written as numbered sentences on cards, over cells or agents, with rates per unit of time, ground, events, buttons, measures and colours; eight starters; conversion to a node graph copy.
- Card editor: pickers that grey a value its place cannot take, with the reason; drag and drop of lines; a line menu to add, duplicate, move and annotate lines; a question before an edit that loses work; fire counts beside change lines; a Diagram view.
- henad-graph crate (`henad::graph`): graph models as model files, with a checker, a host evaluator, a WGSL compiler for grid and agent graphs, edit commands and starters.
- Grid rules (`henad::rules`): a WGSL `step_cell` composed and checked at run time (`compose_wgsl`).
- Model files (`ModelFile`, facade feature `model-files`) and `henad-cli --model-file`, with `--print-lowered`; problems reported at `path:line:column`.
- GPU models built at run time from a spec (`GpuGridSpec`, `GpuAgentSpec`, `ModelEntry::from_gpu_grid_spec`, `from_gpu_agent_spec`), and `ModelSet::replace`.
- Code identity (`CodeOrigin`, `ModelCode`, `code_hash`): manifests record a runtime model's code, and the app carries values and scheduled actions to new code with a Rebuild model dialog.
- Time scales: hidden parameters, a `time` column in exported stats and `series.csv`, and time in natural units in the app.
- Whole-number `f32` parameters (`ParamDescriptor::integer`) and percent values (`2.5%`) in `--set`, `--vary`, specs and the Sweep tab.
- Live parameters and fire counts (`FireCounts`) on the GPU engines.
- Work budget: `Demand` rejects a configuration whose submission or host work is too heavy, naming the pass.
- App builds a model on its own thread with Cancel, and detaches a run that does not stop within 2 s.
- App: New model dialog with grid rule and node graph starters under Advanced; Exact replay row in the Model tab; a run pauses while a modal dialog is open.
- `CodeDiagnostic`, `pcg4d` in Rust and WGSL, the shared WGSL module `henad::hash`, `action::parse_entry`.
- Docs: Editor guide and model files reference.
- `SECURITY.md`; the template's own files dedicated to the public domain under CC0 1.0.

### Changed

- Breaking: descriptor fields private behind accessors (`id()`, `label()`, `kind()`, ...); `ActionDescriptor` no longer `Copy`; choice options are `ChoiceOptions`, and `known` lists are `Vec<String>`.
- Breaking: `GridView`, `PointView` and `EdgeView` borrow their palettes; `ModelMetadata` (built through `new`) and `Structure` are `#[non_exhaustive]`, with `Cow` buffer and pass lists.
- Breaking: GPU agent builds panic on a position or colour buffer not `drawable`, a double-buffered buffer no step pass writes, or one buffer side bound both read-only and writable.
- Breaking: `PassBindings` and `Demand` gain kind, work, host and binding-size fields; `Demand::push_pass` takes the kind and work.
- Breaking: resume and merge reject changed runtime code and another time scale (`ResumeError::CodeChanged`, `TimeScaleChanged`, `MergeError::CodeChanged`); `RecordedBuild` is `#[non_exhaustive]`; `ManifestModel` gains `code` and `author`.
- Breaking: the output writers and `ResumeScan::read` take the time scale, tick length or `ModelSource`.
- Breaking: `ValueError`, `LevelSpec`, `LevelSpecError` and `FactorDomain` gain variants.
- Breaking: `choice_param` panics on an option holding `,` or `:` or named `all`; debug builds panic on a stat named `run_id` or `tick`, or `time` beside a `time` column.
- Breaking: `SweepOptions` no longer `UnwindSafe`.
- Breaking: `GpuSimThread::new` takes a `FaultSink`; dropping a runner waits at most 2 s, then detaches it; `stop(timeout)` and `runner::Driver::shutdown(timeout)` return `RunnerShutdown`.
- Breaking: `Snapshot` gains `fire_counts` and `run_to_samples`.
- GPU engines non-generic, built from one spec per entry; `GpuGridState<M>` and `GpuAgentState<M>` unchanged.
- GPU runner keeps to Target TPS, and publishes a snapshot once its stats readback lands.
- Typed slider and Sweep range values apply on Enter or when the box loses focus (was: on every key).
- Messages reworded: "rejects", "failed", "expected … got …", "is outside the range 0 to 1".
- Minimum egui 0.36.2 and wgpu 30.0.2; on macOS the window is colour-managed as sRGB.
- New Henad icon for the app, the web app and the docs site.
- Deprecation policy amended (Stability in the releasing guide).
- Benchmarks measured again on a Ryzen 9 9950X3D with an RTX 4090.

### Removed

- Breaking: `GpuCommand`; use `GpuSimThread::set_batch_size`, `set_adaptive` or `set_target_ms`.

### Fixed

- Panic sites lost when several rayon workers panicked at once.
- App: Charts legend order, a slider snapping past its maximum or on "NaN", a whole-number slider wrapping above 2147483647, a Viewport Density panic with no agents, Tab stopping on panel dividers.
- `write_grid` panicking on a width of 0.
- Export tab's Save recording naming the loaded model and tick.
- GPU search building a candidate the device cannot hold.
- GPU runner publishing the previous readback's stats after a pause, step or action.
- Template: LF line ends on Windows, and `git init -b main` so pushes run its CI.

### Security

- `url` 2.5.9 for RUSTSEC-2026-0592.

### Migrating to 0.4

- Requirements: `henad` and `henad-build` to `"0.4"`.
- Descriptors: read fields through accessors, build them through the helpers or `ParamDescriptor::new`, `StatDescriptor::owned` and `ActionDescriptor::owned`, clone an `ActionDescriptor` where it was copied, and pass `&[StatDescriptor]` to `stat_entries`.
- Metadata: `ModelMetadata::new(backend, structure).palette(..).replays_exactly(..)`; a wildcard arm on `Structure`; its lists through `.iter()`.
- GPU agent models: mark position and colour buffers `drawable`, write each double-buffered buffer's next side in a step pass, and bind one side of a buffer per pass.
- Capacity: pass the kind and work to `Demand::push_pass`; add the new fields to `PassBindings` and `Demand` literals, or start from `Demand::default()`.
- Results: add the new error and value variants to exhaustive matches; build a `RecordedBuild` through `From`; pass `entry.source()` to `ResumeScan::read` and the time scale to `OutputDir::open_writer` and `append_writer`; a `ManifestModel` literal adds `code: None` and `author: None`.
- Sweeps: wrap a closure that captures a `SweepOptions` in `AssertUnwindSafe` before `catch_unwind`.
- Runners: pass `ctx.faults.clone()` as `GpuSimThread::new`'s last argument; call `stop(timeout)` where the thread has to be gone; a `Snapshot` literal adds `fire_counts` and `run_to_samples`.
- Models: rename a choice option holding `,` or `:` or named `all`, and a stat named `run_id`, `tick` or `time`.

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
