# Changelog

This document follows [Keep a Changelog v1.1](https://keepachangelog.com/en/1.1.0/).
Henad uses [semantic versioning](https://semver.org/spec/v2.0.0.html) for its releases.

## [Unreleased]

### Added

- Descriptors that own their strings, for models read at run time: `ParamDescriptor::new`, `StatDescriptor::owned`, `ActionDescriptor::owned`, `StatEntry::new`, `ChoiceOptions` (`from_static`, `new`; `henad::params::ChoiceOptions`); `StatDescriptor::entry`.
- `ModelSet::replace`: an entry in place of the one with its id, at its position, or appended.
- Code identity: `CodeOrigin` (`Compiled`, `Runtime`), `ModelCode` (FNV-1a over format and canonical text), `ModelSource::runtime`, `origin`, `code`, `code_hash`; manifests record a model's `origin` and `code_hash`, and a runtime model's text as `model.code` (format version stays 1); `ResultSet::code_matches`.
- Grid rules (henad-compute `runtime-wgsl`, henad-explore and facade `model-files`, at `henad::rules`): `GridRule`, `RuleState`, `RuleDeclaration`, `GridRule::check_declarations`, `CompiledGridRule::entry`, `GridRule::life()`; a WGSL `step_cell` composed at run time after an engine template, refused at its line for loops, bindings, entry points, atomics, barriers, shared writes, engine buffers but `params`, built-in names at module scope, template names, control characters in the name, description, labels and options, pointers into memory the cell does not own, line breaks WGSL reads and the composer does not (VT, FF, U+0085, U+2028, U+2029), variables past WGSL's 8192-byte limits, and a call past 65536 expressions and statements.
- `compose_wgsl`: run-time WGSL composition over naga_oil 0.23, byte-equal to henad-build's for every stock shader; a directive check refuses what naga_oil would rewrite or panic on.
- Model files (`ModelFile`, `ModelFileError`): a grid rule as one TOML file (`format = "henad-model"`, version 1, `kind = "gpu_grid"`), canonical text as its code identity, declaration and step problems at their file line; `specs/models/life_rule.toml`. `ModelFile` an enum over the kinds (`GpuGrid(Box<GridRuleFile>)`, `Graph(Box<GraphFile>)`, `#[non_exhaustive]`), dispatching on `kind`; `id`, `rule` and `graph` (both `Option`), `warnings`; `ModelFileError::Graph` (checker and host errors with pass and node), `ModelFileError::Unsupported` (unused since graphs of agents compile); graph files past 1 MiB, or whose canonical text passes it, refused.
- henad-graph (ninth published crate; henad-explore and facade `model-files`, at `henad::graph`): graph models as `kind = "graph"` model files, library 1. Reader with places, canonical writer (code identity `henad-graph/1`, editor region outside it; count lists sorted, each count once; a link to a node's only output bare), checker of every rule a grid can meet (`CheckReport`, warnings, pass and node ids), catalogue of 64 kinds with known-answer cases (`conv.to_u32` and `conv.to_i32` from `f32` clamp as WGSL does: at most 4294967040 and 2147483520), `library.lock`, counter-based draws (`draw`, provisional hash core `pcg4d2`: `draw_core`, `pcg4d`), `region.repeat`, host evaluator (`HostEvaluator`: init passes and action reruns on the CPU, lanes at most 4 GiB), edit commands (`GraphBuilder`, `EditCommand`), starters (`starters()`: Game of Life, Vote, SIR).
- `DiagnosticSeverity::Warning`, `CodeDiagnostic::warning`; `CodeDiagnostic` `pass` and `node` (`with_pass`, `with_node`).
- henad-cli `--model-file PATH` (repeatable): a model file's model added to the set, replacing a model of its id with a note on stderr (text mode); with `--spec`, `--resume`, `--list`, `--params` and `--dry-run`, not `--merge`; problems at `path:line:column`, exit 1; `MODEL` optional with one file, required with two (exit 2). Model files reference page.
- henad-cli `--model-file` reads graph files: warnings on stderr, errors at `path:line:column` with a pass and node note; graphs on a grid and of agents run.
- henad-graph WGSL compiler for grid graphs (`henad_graph::wgsl`): `GridProgram::compile`, `shaders`, `spec`, `entry`; `GeneratedShader` (`text`, `composed`, `ranges`, `node_at`, `role`, `passes`, `cost`), `NodeRange`, `ShaderRole`. Values named `n<id>_<port>`, a dotted port's `.` as `__`; host-evaluated prelude in the uniform block, tick-reading values in the shader; Repeat a counted loop, grid count unrolled. `ModelFile::entry` builds grid graphs; `draw::DRAW_WGSL` (`henad_graph::draw`, the `pcg4d2` twin). A pure Repeat body value reading no iteration and no element runs in the prelude (lane seed, press word, powi base in a body compile). A shader past `compose::MAX_FILE_BYTES` (now public) refused on the pass that wrote most of it. Reduce shader's tallies and each pass's uniform block counted in the spec's demand.
- henad-graph agent kinds (library 1, host and WGSL forms; every kind with a WGSL form outside init runs a case on the device, `vec.from_angle` included, compared within 1e-5 by `CaseValue::NearVec2F`): `ctx.world`, `ctx.grid`, `ctx.cell_xy`, `lane.get_at`, `field.get`/`set`/`deposit`, `counter.add`, `stat.sum`/`mean`/`counter`, `math.neg`/`clamp`/`sqrt`/`recip`/`log2`/`log10`/`floor`/`midpoint`, `math.expr` (expanded before checking; depth 64 counts chain operands), `conv.to_f64`/`to_vec2f`/`to_vec2i`/`to_vec2u`, `vec.*`, `space.*`, `rand.bits`/`choice3`, `rng.reservoir`/`choice3`, `stream.float`, `grid.fold`, `agent.radius_fold` (65536-candidate fault; walks past 65536 index cells fault before starting), `fn.call`/`input`/`output`. Normalisation (`henad_graph::normal`): expressions expanded, calls inlined, keys composed per call; checker rules V12, V14, V25, region nesting, V18 after inlining, call contexts, no `field.get` of a field in its `finish`, `stat.counter` read after the elements only, `MAX_NODES_INLINED` counting expansions, checked before inlining; host evaluator over agents worlds (`HostWorld::Agents` with extent, field layers, deposits, counters). Starters `boids`, `ants`, `random_walk` (`agent_starters()`); fixtures `gpu_boids.toml`, `gpu_ants.toml`; tick-0 lanes equal the stock inits.
- henad-graph WGSL compiler for agents graphs: `AgentProgram::compile`, `shaders`, `spec`, `entry`, `replays_exactly` (false for a radius fold with a `carry`, a float `sum`, or a read of `k` or `prev.<acc>`); `ShaderRole::Color`, `ShaderRole::Merge`. Linear passes (256), one buffer per lane, `vec2` lanes interleaved, dual lanes swapped per step; fields one buffer of layers, deposits `atomicMax` of the bits into `accum_<field>_cells`, field update step per field (`max`, then `finish`), one invocation per layer and cell; tick, fault word and counters in the engine's `counters`; snapshot colour pass to `agent_colors`; display pass of cell views; one reduce leaf over the longest stats domain (tallies cleared per leaf, float sums as `block_sum` lanes); radius fold as the `henad::hash` walk, past 65536 candidates or 65536 cells a fault naming pass, node and line at the next stats read or the end of the steps, run ended; walk cells and uniform-spread visits charged per step in `demand`. Normalised document checked without the 1 MiB file limit. Budgets, uniform and workgroup limits as for grids. `ModelFile::entry` builds graphs of agents.
- Shared WGSL module `henad::hash` (`walk_bounds`, `walk_cell`, `walk_cells`, `MAX_VISITS`, `MAX_WALK_CELLS`).
- `GpuAgentSpec::color`, `cleared_counters`, `init_cost`; per-pass `cost`, `uniform_bytes`, `workgroup_bytes`, `AgentActionSpec::rerun_cost`, counted in `demand`. `AgentHooks` provided methods `run_params_bytes` (a `Result`: build fails, press refused), `run_stats`, `rerun`, `fault`, `reports_faults`, `step_cost`; `AgentPassId`, `AgentRun`, `AgentUploads`; engine reruns on press, fault from counters ends the run. `GpuSimState::read_fault` (provided, default nothing), `stepping::wait_for`: `run_steps`, `run_steps_acting` and benchmarks read a recorded fault without a stats read. `CounterReadback::encode_clear_range`.
- Agent graph oracles (henad-models tests): `gpu_boids.toml` equal to `gpu_boids` in a sweep's engine-owned fields, `gpu_boids.toml` and `boids.toml` within 0.002 of its stats over 8 steps, under its invariants and the `consistency_boids.rs` gate; boids fold's cap corner modelled on the CPU at tick 0 (1M boids, `visual_range` 200, world under ~1820), every such configuration refused by the submission budget before a build (budget admits ~195000 / ~201000 boids in the default world); float Repeat body over agents replays on the device; `gpu_ants.toml` equal to `gpu_ants` every tick and press (lanes, layers, colours, stats), after a contraction probe; `ants.toml` equal at tick 0, to CPU `ants` under the cross-engine gate, in exact decay and the `consistency_ants.rs` checks; Repeat over agents (known answers, hand-chained copies, draws, `grid.fold` nesting both ways, random walk, budget refusal); schemas hash as the stock models' under their ids; testing kit passes every agents graph entry.
- Grid graph oracles (henad-models tests): `gpu_sir_exact.toml` equal to `gpu_sir` every tick and press; starter `sir.toml` equal at tick 0 and under rate gates, within the SIR bands past them; fixture `repeat_trials.toml` (`region.repeat`, `max = 4`) at the closed-form rate `1 - (1 - p)^trials`, its device ticks equal to the host evaluator's; testing kit passes every grid graph entry; `stat.sum` and `stat.mean` of a grid graph reduced on the device to closed forms.
- Grid float sums: `GpuGridSpec::sums`, `GridHooks::stats_with_sums` (provided, default `stats`), `GpuLaneReduce::over_blocks`; a grid graph's `stat.sum` and `stat.mean` compile (reduce shader binds `partials`, `block_sum` per lane per cell workgroup) where they were refused.
- `CodeDiagnostic` and `DiagnosticSeverity` (henad-core, facade root): span, 1-based line and character column, notes; `Display` escapes control characters.
- `helpers::check_choice_options` (`henad::authoring`): the option-name check of `choice_param`, without the panic.
- App install path for replaced model code: the running model keeps its own entry, which the panels describe; values and scheduled actions carried to the new code by id; Rebuild model dialog for a run past tick 0, a run at tick 0 rebuilt at once; Rebuild disabled, old run kept, when the new code cannot build; Sweep tab drops the model's stale columns and draft; Results warn on runs of other code, Resume disabled for a folder of other code where either code is runtime code (`Manifest::code_matches`).
- `SimThread::new_in`, `GpuSimThread::new_in`, `runner::Driver::spawn_in` (native): a runner whose start, pumps and commands (and, on the CPU, first snapshot) run in a given rayon pool.
- App builds a model on its own thread: Building model… with Cancel in Playback, commands sent meanwhile queued for the new run; a run or build that does not stop within 250 ms is detached and reported (Previous run is still stopping; did not stop after 2 s); the next build waits up to 2 s for it; past 2 s the app's CPU work (builds, runners, viewport uploads, Sweep tab column builds, sweep probes and GPU run builds) moves to a fresh pool; quitting waits at most 2 s for a sweep.
- App Editor tab for grid rules (closed by default, opens beside the Viewport): one model file as text with a line-number gutter; Apply checks, installs and selects the rule, rebuilt at tick 0 or after the Rebuild model dialog, a refused text leaving the run alone; problems listed at line and column, pressed to move the cursor; New grid rule (the Life starter), Edit model, Open model and Save model, also in a new File menu (a download in a browser); a device fault of the applied rule before its run passes tick 0 shown in the Editor.
- App graph editor in the Editor tab (egui-snarl 0.12): New graph model from a starter (also in the File menu), passes and world panel, canvas with quick-add, node menus, parameter chips, error badges that frame their node, snapshot undo, copy and paste as model-file text, Arrange; Apply installs a grid graph at its own size (submission budget refusal shown in the Editor); live preview at most 256 × 256 cells, rebuilt from tick 0 250 ms after each valid edit, only over no run, a run at tick 0 or a preview, otherwise "Preview waits for the running model."; read-only generated WGSL per pass; View in viewport on an element-rate output through a temporary view pass; AccessKit bounds of canvas widgets follow pan and zoom.
- Graph editor regions: a region drawn as an input half and an output half (`[editor] node.<id>.output_pos`) with a frame behind its body, frames nested; Repeat button wraps the selection (`EditCommand::WrapInRepeat`: count 1, max 1, each value read outside a `carry` accumulator; refused past library 1's nesting and depth 3); a node dropped in a frame joins the region, node menu Move into region and Move out of region (a node left inside the frame placed right of it); Repeat controls (Max, accumulator name, type, combine, Add and Remove accumulator), a constant count sets Max; quick-add in a frame adds to the region. henad-graph `AddAccumulator`, `SetAccumulator`, `RenameAccumulator`, `RemoveAccumulator`, `MoveRegionOutput`, `Accumulator::new`, `to_config`, `Combine::ALL`, `applies_to`, `NodeBody::region_contents`; `SetConfig`, `AddAccumulator` and `SetRegion` name a bare link's port before it could read another output; wraps in a function body too; menus and frames offer only the nesting library 1 allows (radius fold in no region, depth 3).
- Graph editor functions: functions panel (rename, ports, remove, add), a call's Open function under a breadcrumb (`flock › clamp_speed`), a canvas per function body typed by the checker, problems on a function's nodes; Function button makes a function of the selection, a call's Ungroup inlines it; Import function from a picked model file (`OpenTarget::Function`); Expression node editor (text, typed inputs, outputs). henad-graph `AddFunction`, `RenameFunction`, `RemoveFunction`, `AddFunctionPort` (second output names bare links to every call first), `RenameFunctionPort`, `SetFunctionPortType`, `RemoveFunctionPort`, `MakeFunction`, `Ungroup` (draws keep their numbers), `ImportFunction` (`[editor] function.<id>.origin = { name, hash }`, `FunctionOrigin`, `EditError::Import`), `PortSide`; `CheckReport::functions`.
- Graph editor runs agents graphs: Boids, Ants and Random walk starters, preview at up to 16384 agents (`PREVIEW_AGENTS`, `PreviewRun::Agents`), WGSL view over `AgentProgram`.
- `SweepRunOptions::build_pool` (native): rayon pool for a sweep's probes and GPU run builds. `SweepRun::stop(timeout)` (native): abort, join or detach after `timeout`.
- GPU model specs (henad-compute `runtime-wgsl`, at `henad_compute::gpu`): `GpuAgentSpec`, `AgentHooks`, `AgentBufferSpec`, `AgentPassSpec`, `AgentDisplaySpec`, `AgentReduceSpec`, `AgentActionSpec`, per-pass `SideTable` (`swapping`, `in_place`, `new`) of `SideRow` and `BufferSide`; `GpuGridSpec`, `GridHooks`, `GridActionSpec`; `ShaderSpec`, `ShaderBinding`. `henad_compute::entry::EntryDeclarations`, `ModelEntry::from_gpu_grid_spec`, `from_gpu_agent_spec`.
- `GpuSimState::read_buffer` (provided, default `None`), implemented by both GPU engines.
- Sheets (henad-graph `sheet`, facade `henad::graph::sheet`): model files of `kind = "sheet"`, sheet version 1. `ParsedSheet::parse` (header and `[sheet] version` first, every table refusing an unknown key, places per table and key in `SheetPlaces`), `SheetDocument` (world, `[time]`, properties, agent kinds, ground, events, parameters with a `unit`, measures, colour rules, buttons, cards, lines as `SheetLine` with `LineKind`, slots blank when absent, values as literal, chip, phrase or list, a percent `{ percent = 30 }` a literal read by `SheetLiteral::number` as 0.3), `SheetFileError`, limits in `sheet::limits`. `TimeUnit` (henad-core `metadata`, facade root): `as_str`, `parse`, `plural`, `days`. `model_text::ModelHeader`, `HeaderProblem`, `read_header`: one header check for every kind; a refused kind lists the kinds read with "or".
- Fire-count channel: `GpuGridSpec::fire_counters`, `GpuAgentSpec::fire_counters` (empty from `new`); the last step of each `encode_steps` call clears the range and counts through a second uniform block per step pass (`GridPassCtx::counting`, `AgentRun::counting`, `AgentRun::with_counting`), never a storage binding; grid stats clear leaves the range; `GpuSimState::fire_counts` (provided, default empty), `FireCounts` (`henad::runner`). henad-graph probes: `Probes` (`henad_graph::wgsl`), `GridProgram::compile_with_probes`, `AgentProgram::compile_with_probes` (a `bool` output of a tick pass per probe, outside the file's text and code identity; a probed grid step binds `counters`), `HostEvaluator::run_pass_counting`.
- `GpuGridSpec::counters` (reduce counters, one per stat from `new`), `GridPassCtx::tick`.
- `henad::gpu` re-exports `PassKind`, `HostWork`, `SUBMISSION_WORK_BUDGET`, `HOST_WORK_BUDGET`.
- `GpuBufferInfo`, `GpuPassInfo` (`henad_core::metadata`, facade root). `binding::named_buffer_target`.
- Generalised `GpuGridSpec` (`runtime-wgsl`): `GridBufferSpec` (dual or plain set), several step passes (`GridPassSpec` with a `SideTable` and a cost), display and reduce as passes, multi-pass actions (`GridActionSpec::with_passes`) after a host rerun (`GridHooks::rerun`, `RerunUploads`, uploaded to the current side; a fault to the fault sink), `GridPassId`, `GridPassCtx`, `GridPress`, `GpuGridSpec::single_step`, `init_cost`, `rerun_cost`. `GridHooks` writes each pass's uniform block (`pass_params_bytes(pass, ctx, params)`) and reads the tick of a reduce (`stats(counts, ctx, params)`). `GridPassSpec::uniform_bytes` and `workgroup_bytes` (zero when unknown). `SideTable`, `SideRow`, `BufferSide` now in `gpu::sides`, shared by both specs (same paths).

### Changed

- Breaking: `ParamDescriptor`, `StatDescriptor`, `StatEntry` and `ActionDescriptor` fields private behind accessors (`id()`, `label()`, `kind()`, `apply()`, `format()`, `value()`, `color()`); `ActionDescriptor` no longer `Copy`.
- Breaking: `ParamKind::Choice` holds `ChoiceOptions` (was `&'static [&'static str]`); `stat_entries` takes `&[StatDescriptor]`; `known` lists and `ValueError::UnknownOption.options` are `Vec<String>`.
- Breaking: `GridView`, `PointView` and `EdgeView` borrow their palettes for `'a`; snapshots own theirs as `Vec<[u8; 4]>`.
- Breaking: `ModelMetadata` and `Structure` are `#[non_exhaustive]`; `ModelMetadata` built through `new`, `palette` and `replays_exactly`, its palette a `Cow`.
- GPU grid engine non-generic, built from a private spec of the model's declarations; `GpuGridState<M>` an adapter over it, its API unchanged. Workgroup counts and buffer sizes cached at construction.
- GPU agent engine non-generic, built from a `GpuAgentSpec`; each pass binds through a side table; `GpuAgentState<M>` an adapter in today's single orientation, its signatures unchanged. `register_gpu_agent_model` builds one spec per entry, shared by its builds and its demand.
- Breaking: GPU agent builds panic on a position or colour buffer not `drawable` (a headless-only model built before), on a double buffered buffer no step pass writes to its next side (alternated seed and zeros before), and on one side of a buffer bound both read-only and writable, such as an action's `x_in` with `x_out` (failed only at the press before). Applies to `GpuAgentState<M>` and `GpuAgentSpec`.
- Breaking: `PassBindings` gains `kind` (`PassKind`: `Step`, `Display`, `Reduce`, `Action(index)`) and `work`, `Demand` gains `host` (`HostWork`); `Demand::push_pass` takes the kind and the work. `Demand::shortfalls` refuses one submission (64 steps with display and reduce, or one press) past `SUBMISSION_WORK_BUDGET` (600e9 units), naming the heaviest pass, and a host run past `HOST_WORK_BUDGET` (4e9 units); `Demand::submission_work`, `push_host`. Hand-written shaders count no work. `PassBindings` gains `uniform_bytes` and `workgroup_bytes`, `push_pass` returns the record; `shortfalls` refuses a pass past `max_uniform_buffer_binding_size` or `max_compute_workgroup_storage_size` (16 KiB on the baseline), naming it.
- Breaking: `Structure::GpuGrid.buffers` is `Cow<'static, [Cow<'static, str>]>` (was `&'static [&'static str]`); `Structure::GpuAgents.buffers` and `passes` are `Cow<'static, [GpuBufferInfo]>` and `Cow<'static, [GpuPassInfo]>` (were `&'static [BufferSpec]` and `&'static [PassSpec]`). `BufferSpec` and `PassSpec` unchanged.
- `register_gpu_grid_model` builds one spec per entry, shared by its builds and its demand.
- henad-app and henad-cli turn on henad-explore's `model-files`.
- Engine stamp (`ENGINE_BUILD`, dirty flag and source hash) covers henad-graph.
- henad-cli reads `--model-file` and `--spec` before it acquires the GPU device, which is sized to the files' models too.
- henad-graph agents graphs: `counter.add` tallied in workgroup memory, one global `atomicAdd` per counter per workgroup, zero amounts skipped; a step's tallies counted in its `workgroup_bytes`; `field.deposit` of 0, a negative or a NaN issues no atomic; no step returns early (grid steps and field updates guard their bounds in a block). Results unchanged.
- henad-graph: a port segment holds no `__`; known-answer cases over `u64` or `f64` are `CaseScope::Host` (five lock lines).
- Breaking: sweep resume, search resume and merge refuse model code that changed when either side is runtime code (new variants `ResumeError::CodeChanged`, `MergeError::CodeChanged`; add match arms); compiled against compiled still warns. `ResumeScan::read` takes the model's `ModelSource` (pass `entry.source()`).
- Breaking: `RecordedBuild` is `#[non_exhaustive]`, with `origin` and `code_hash` (build through `From<&BuildInfo>`, `From<&ModelSource>` or `engine()`); `ManifestModel` gains `code` (`None` for compiled models); `ModelSource` compares its origin and code; `same_build` with a runtime build on either side holds only for runtime code of one hash.
- Breaking: `choice_param` panics on, and `check_choice_options` refuses, an option name holding `,` or `:` or named `all`, which `--vary` and Sweep tab level text cannot list.
- Breaking: `GpuSimThread::new` and `new_in` take a `FaultSink` for a panic out of the loop, as `SimThread::new` does (was: `ctx.faults`); device errors still go to `ctx.faults`. The app's GPU runner reports into `sim_faults`, replaced at each teardown.
- Breaking: dropping a `SimThread` or `GpuSimThread` waits at most `DEFAULT_STOP_TIMEOUT` (2 s) for its thread, then detaches it and logs (was: joined); `stop(timeout)` returns `RunnerShutdown` (`Joined`, `Detached(DetachedRunner)`); `runner::Driver::shutdown` takes a timeout and returns it.
- Graph editor: a move of a node, or Arrange, leaves the preview running (preview key follows the code); Arrange puts a region's input half in the column before its body.
- Breaking: `Snapshot` gains `fire_counts` (`FireCounts`, empty on the CPU path and for a model with no probes).

### Fixed

- Panic locations: a burst of one panic across rayon workers no longer drops the site of another panic being caught (a site recorded once, 64 kept, a catch leaves it listed).

### Migrating to 0.4

- Requirements: `henad` and `henad-build` to `"0.4"` (the next build resolves them).
- Models: model code built through the authoring traits and helpers compiles unchanged; tests that read descriptor or stat fields switch to the accessors, as the tutorial's `gpu_life` and `gpu_foraging` tests did (`d.kind().default_value()`, `s.value().clone()`); rename a choice option holding `,` or `:` or named `all`.
- Descriptors: read fields through accessors (`descriptor.id()`, `.label()`, `.kind()`, ...); build them through the helpers, `ParamDescriptor::new`, `StatDescriptor::owned` or `ActionDescriptor::owned` (struct literals no longer compile); clone an `ActionDescriptor` where it was copied; a choice's options are `ChoiceOptions` (`get`, `iter`, `position`); pass `&[StatDescriptor]` to `stat_entries`; `known` lists and `ValueError::UnknownOption.options` are `Vec<String>`.
- Views: `GridView`, `PointView` and `EdgeView` borrow their palettes for `'a`; a snapshot's palette is a `Vec<[u8; 4]>`.
- Metadata: build `ModelMetadata` with `ModelMetadata::new(backend, structure).palette(..).replays_exactly(..)`; add a wildcard arm to matches on `Structure`.
- Results: add arms for `ResumeError::CodeChanged` and `MergeError::CodeChanged`; build a `RecordedBuild` through `From`; pass `entry.source()` to `ResumeScan::read`.
- Runners: pass `ctx.faults.clone()` as `GpuSimThread::new`'s new last argument to keep its panics in the device's sink; a drop no longer waits for a stuck thread; call `stop(timeout)` and match `RunnerShutdown` where the thread has to be gone; `runner::Driver::shutdown` takes a timeout and returns `RunnerShutdown`; a `Snapshot` literal adds `fire_counts: FireCounts::default()`.

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
