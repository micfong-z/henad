---
date: 2026-10-10
title: "Runaway rails"
description: "Milestone R of #49. A runner stops within a timeout and hands back a thread that does not stop, a runner can pump in a pool of the host's own, and the app builds a model on a thread of its own with Cancel, queues the commands sent meanwhile, waits for a slow run to end, and moves its CPU work to a fresh pool once a stuck run has held the old one past the timeout."
icon: material/car-brake-alert
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#49"
state: "bounded runner stop, SimThread::new_in and GpuSimThread::new_in, async build with Cancel, a build that waits for the stopped run, a pool retired at the timeout, and two reviews' fixes built; check.sh and check_hot.sh run with HENAD_REQUIRE_GPU=1, the app checked through the egui MCP server, benchmark gate written out and not run, uncommitted on top of bf72686"
baseline_commit: bf72686
delta_state: "uncommitted on `49/r-runaway-rails`, 4 new files and 27 changed files"
---

# Runaway rails

> Milestone R of #49 (dynamic model loading), decision 13 of its plan: a Rust `loop {}` must not force a relaunch.
> Before it, dropping a runner joined its thread, so a step that never returned froze the UI on the next Build, Offload or quit, and held a worker of the global rayon pool for good.
> henad-compute's runners now stop within a timeout: `SimThread::stop` and `GpuSimThread::stop` return `RunnerShutdown::Joined` or `RunnerShutdown::Detached(DetachedRunner)`, and their drops wait at most `DEFAULT_STOP_TIMEOUT` (2 s), then detach and log.
> `SimThread::new_in(pool, ..)` and `GpuSimThread::new_in(pool, ..)` run a runner's start, pumps and commands in a pool of the host's own, so a stuck kernel holds workers of that pool alone.
> The app builds a model on a `henad-model-build` thread, shows **Building model…** with Cancel in the Playback tab, and queues the commands sent meanwhile for the runner.
> Every teardown waits at most 250 ms, keeps a runner or build that does not stop, and reports it as "Previous run is still stopping." and, after 2 s, "Previous run did not stop. It will keep running until the app quits."
> The next build waits on its thread for the stopped run, so a slow run frees its memory first.
> A run still going after 2 s retires its pool, and the app's CPU work, the viewport's uploads included, moves to a fresh pool at the global width.

## State before

`49/r-runaway-rails` branched from `49-dynamic-loading` at `bf72686`, which holds rounds 1 to 3 as placeholder commits: D1, M1 (#50), M3 (#52), D2, M2 (#51), M4 (#53) and M5 (#54).

- `runner::Driver::shutdown` sent the stop command and called `JoinHandle::join`, and both runners' `Drop` called it.
  A loop handles commands only between two pumps, so a step that never returned held the joining thread for good (research-code-map.md, cpu-grid-and-rust-mechanisms §2).
- `AppState::load` dropped the runner on the UI thread (`self.sim_thread = None`), then built the model and the runner on the UI thread through `build_runner`.
  `SimThread::new` built the first snapshot on the calling thread as well.
- The native driver pumped inside `rayon::scope` from a thread of its own, so a stuck pump held a worker of the global pool, and every later CPU run shared the pool with it.
- M5 added `build_refusal` before any teardown, the pinned loaded entry and the Rebuild model modal. This milestone keeps all three.

## What was done

### henad-compute: the bounded stop

- `runner/mod.rs` gains `DEFAULT_STOP_TIMEOUT` (2 s), `RunnerShutdown { Joined, Detached(DetachedRunner) }` and `DetachedRunner`.
  A `DetachedRunner` holds the `JoinHandle` and offers `is_finished`, and dropping it leaves the thread running.
- `runner/thread.rs`: `Driver::shutdown(cmd, timeout)` takes the handle first, so a second call sends nothing and returns `Joined`.
  It sends the stop command and polls `JoinHandle::is_finished` with a wait that starts at 50 µs and doubles to 1 ms, then joins a finished thread or returns it unjoined at the timeout.
  `runner/frame.rs`'s `shutdown` takes the same arguments and returns `Joined`: the loop handles the command at once, and nothing is ever detached in a browser.
- `SimThread::stop(timeout)` and `GpuSimThread::stop(timeout)` call it.
  Each `Drop` stops with `DEFAULT_STOP_TIMEOUT` and logs a warning when the thread is detached.
  `SimCommand::Shutdown`'s doc names both.

### henad-compute: `SimThread::new_in` and `GpuSimThread::new_in`

- `SimThread::new_in(pool: Arc<rayon::ThreadPool>, state, target_tps, wake, faults)` (native) builds the first snapshot inside `pool.install` and starts the loop through `Driver::spawn_in`.
- `Driver::spawn_in` runs the loop's `start`, each pump and each command through `pool.install`.
  The first version ran only the pumps there, as `spawn` does with `rayon::scope`, and `a_runner_in_a_pool_steps_on_its_workers` failed: `StepOnce`, `Act` and a `SetSchedule` that fires an action run a step or an action in `handle_command`, on the runner thread, whose parallel passes then went to the global pool.
- `Driver::spawn` keeps its shape: `start` and the commands on the runner thread, and each pump in `rayon::scope`.
  Both constructors go through a private `spawn_with` that takes the three calls as closures, and the loop `run` is generic over them, so the default path monomorphises to the code it had.
- `Loop::new` (CPU) builds the loop and its slot, shared by both constructors.
- `GpuSimThread::new_in(pool, ctx, state, settings, wake)` (native) starts the GPU loop through `Driver::spawn_in`, beside `new` over `Driver::spawn`.
  Both share a private `parts`, which returns the loop, its slot, its `GpuStats` and the panic handler, typed `WasmNotSend` so the threaded web build, where wgpu's types are not `Send`, still compiles.
  A GPU pump uses no rayon, but `spawn` runs it inside `rayon::scope` on the global pool, and it waits there for a free worker.
- The facade re-exports `DEFAULT_STOP_TIMEOUT`, `DetachedRunner` and `RunnerShutdown` under `henad::runner`, and `tests/facade_paths.rs`'s `named_only` names all three through a `detached` helper.

### henad-app: the build thread

`state/build.rs` is a new child module of `state`, beside M5's `install.rs`.

- `load` tears down, then commits the loaded fields (stats history, `loaded`, `loaded_values`, `loaded_seed`, `loaded_schedule`) at the start of the build, then calls `start_build` and sends `SetSchedule` through `send_command`.
  The panels, the run details, live edits and the actions then describe the model being built.
- `start_build` takes a `BuildJob` from the app: the setup, the GPU context, the CPU runner's fault sink, the waker, the pacing and layout settings, the GPU batch settings and, on native, the `ModelPool`, the detached threads not yet ended (`predecessors`) and the build's `phase`.
  On native `spawn_build` runs it on a thread named `henad-model-build`.
  The thread first calls `settle`, which waits in 5 ms steps until each predecessor has ended, or has run past `DEFAULT_STOP_TIMEOUT` since its stop and retired its pool, then takes the current pool.
  It then builds inside `pool.install` when the pool is the app's own, and sends the runner or the fault back over an `mpsc` channel with the pool's generation (`Built`), then wakes the UI.
  In a browser the job runs inline through `run_here` and the same `finish_build`.
- `BuildJob::run_here` wraps a GPU build whole (`entry.build` and the runner) in `catching_on` on the thread that builds it, since wgpu's error scopes are thread-local.
  A CPU build calls `SimThread::new_in` with the app's pool, or `SimThread::new` in the global pool, inside `catching(BUILDING, ..)`, as `build_runner` did. `build_runner` is gone.
  A GPU build calls `GpuSimThread::new_in` or `new` the same way.
- A build's `phase` (waiting, building, done, cancelled) moves forward by compare and swap.
  An abandon swaps in cancelled: a waiting build ends without building, a building one is detached and drops the runner it builds on its own thread, and a done one hands its runner over, to be stopped like any other.
- `HenadApp::logic` calls `poll_build` first each frame.
  It installs the runner, sends a GPU runner the Pacing tab's batch settings, then the queue of the `PendingBuild` in order, or shows the build's fault with the model's name.
- `AppState::send_command` sends to the runner, or queues while a build runs.
  `run_to`, `pause_simulation`, `send_live_param`, the action buttons and the Pacing tab's CPU controls go through it, so the `SetSchedule` of `load`, the `RunTo` of `run_to`, `open_run` and `open_setup`, a live edit and an action press made during a build reach the new runner.
  `has_simulation` holds for a runner or a build. `open_values` and `run_to` read it in place of `sim_thread.is_some()`, and so does the Parameters banner's "No simulation loaded".
  `is_gpu` reads the loaded entry while a build runs, so the Pacing tab shows a GPU model's batching controls during its build, and `finish_build` applies them.
- A `load` while a build runs abandons that build, as Cancel does.
  Build is disabled during a build, so this happens through an open from the Results tab or an install of new code (M5's rebuild at tick 0).
  A fault during a build abandons it through `offload_simulation`.

### henad-app: the teardown and the pool

- `stop_simulation` replaces the `sim_thread = None` of `load` and `offload_simulation`, and so serves Build, Offload, Cancel and the fault path.
  It abandons a running build and stops the runner with `STOP_WAIT` (250 ms).
  A build that finished meanwhile hands its runner over, and the runner stops like any other.
  A runner still running and a build abandoned while it builds go into `stopping` as `Arc<Stopping>`, with the time of the stop and the generation of their pool.
  The next build holds clones of the records and waits for them.
- `state/pool.rs` holds `ModelPool`, a shared slot with the pool the app's CPU work runs in (`None` for rayon's global pool) and its generation.
  `install(work)` runs `work` on a worker of the current pool, or on the calling thread for the global pool.
  `retire(generation)` puts a fresh pool at the global width in place, its workers named `henad-pool-N`, unless that generation has been retired already, so the UI thread and a build thread can both call it.
  A detached thread keeps its pool alive through its own `Arc`, and a pool nobody holds ends its workers once they are idle.
- `poll_stopping` (each frame) drops a detached thread that has ended, retires the pool of each that has run past `DEFAULT_STOP_TIMEOUT`, and repaints every 250 ms while one is younger, every second after.
  `stopping_status` returns `STILL_STOPPING` or, once one has run past `DEFAULT_STOP_TIMEOUT` since its stop, `DID_NOT_STOP`.
  Playback shows the first as a weak line with an information icon and the second in the theme's warn colour with an alert icon.
  The pool is retired at the moment the line turns to `DID_NOT_STOP`, by whichever of the UI thread and a waiting build sees it first.
- The UI thread's own parallel work went through `ModelPool::install`: the viewport's `expand_grid` and `sample_grid`, the density map, the agent layer's colours and the edge layer's packing (`upload` and `upload_edges` take the pool), and the export's `grid_pixels`.
  The final review of #49 moved it to a `UiPool` of its own, which no model code runs in, since a run that holds every worker of the model pool stalled the frame before any stop retired that pool.
  The Sweep tab's `ColumnsBuild` builds on its `henad-stat-columns` thread inside the pool (`SweepPanel::model_pool`, set by `AppState::new`).
  A stuck run that holds every worker of the global pool then stalls none of them once its pool is retired.
- A CPU runner reports into `AppState::sim_faults` in place of `render_ctx.faults`, and each teardown replaces the sink, so a detached runner that panics later reaches no later run.
  `logic` reads both sinks.
- `impl Drop for AppState` runs `stop_simulation`, so quitting waits at most 250 ms where the runner's own drop would wait 2 s.
  A `Drop` on `AppState` covers eframe with or without its `glow` feature, whose `App::on_exit` changes signature with it.

### henad-app: Playback

- While a build runs, the Run to tick row becomes `building_row`: a spinner, **Building model…** (`BUILDING_MODEL`) and Cancel, whose tooltip reads "Stop waiting for build".
  Cancel calls `offload_simulation`.
  Build is disabled with "Building model. Press Cancel first." (`BUILD_RUNNING`), and Play, Step and Offload are disabled since no runner exists yet.
- The Run to tick progress row's Cancel calls `pause_simulation`, so it reaches a queued run to a tick as well.
- `stopping_line` follows the Build row.

### Tests

| Test | What it holds |
|---|---|
| `a_runner_that_never_returns_is_detached_within_the_timeout` (henad-compute, `cpu/sim_thread.rs`, `stop_tests`) | A step blocked on a channel: `stop(200 ms)` returns `Detached` after the timeout and within a second of it, the thread runs on, and once the channel is released the thread ends and drops its state |
| `dropping_a_runaway_runner_returns_within_the_timeout` | The same step: the drop returns after `DEFAULT_STOP_TIMEOUT` and within a second of it, and the released thread ends |
| `a_runner_that_stops_is_joined` | A runner whose step returns stops at once with `Joined` and has dropped its state |
| `a_runner_in_a_pool_steps_on_its_workers` | Under `new_in`, the first snapshot's `prepare_view` and a step run on workers of a pool one wider than the global pool |
| `a_gpu_runner_in_a_pool_pumps_on_its_workers` (`gpu/sim_thread.rs`) | Under `GpuSimThread::new_in`, every batch is encoded on a worker of a pool one wider than the global pool |
| `a_cpu_build_after_a_detach_runs_on_a_fresh_pool` (henad-app, `src/tests/runaway.rs`) | The first build runs on the `henad-model-build` thread in the global pool. Offload of a run held in its step reads `STILL_STOPPING` and keeps the global pool. The next build waits past the timeout, retires the global pool, and it and its step run on `henad-pool-N` workers of a pool at the global width while the first run is still held |
| `a_slow_runner_is_waited_for_and_keeps_the_pool` | Build during a held step returns within `STOP_WAIT` and a second, with the run detached. Released, the run ends before the second build starts (`drops_seen`), and the pool stays the global one |
| `a_viewport_frame_after_a_detach_runs_on_the_fresh_pool` | A held step occupies every worker of the global pool. After the detach and a rebuild past the timeout, a frame of `viewport_ui` uploads the new run's grid while the global pool is still held. A watchdog opens the gate after 20 s and fails the test, where a frame stuck on the global pool would otherwise wait for good |
| `a_build_that_never_returns_leaves_the_ui_responsive` | Build returns at once while the build is held, a frame of `playback_ui` shows `BUILDING_MODEL` and Cancel, Cancel returns at once with nothing loaded, `STILL_STOPPING` and later `DID_NOT_STOP`, a second build waits for the abandoned one past the timeout and completes on a fresh pool, and the released first build ends and drops its state |
| `commands_sent_during_a_build_reach_the_new_runner` | `open_setup` to tick 20 with an outbreak scheduled at tick 5, a live edit and an action press, all during a held build, reach the runner: the run stops at tick 20, the outbreak runs at tick 5, and the edit and the press arrive |
| `offload_and_quit_do_not_wait_for_a_runaway_runner` | Offload and the drop of an app, each holding a run in its step, return within `STOP_WAIT` and a second |
| `a_gpu_build_on_the_build_thread_reports_a_validation_error_as_a_fault` | GPU Game of Life whose factory creates a buffer past the device's limit outside the entry's own scopes: the build thread's scopes catch it, and the modal shows a device fault while building, with the model's name and nothing left in the device's sink |

The henad-app tests run over `held` entries in `src/tests/support.rs`: a CPU entry whose factory records the thread of each build and the states dropped when it started, and whose state records the thread of each step and each action and parameter edit in a `Probe`.
The first build waits on an optional build `Gate`, and the first run's steps are held by an optional `StepHold`, on the step's own thread (`Step`) or on every worker of the pool the step runs in (`Pool`), so a later build runs free.
Each test opens its gate through an `OpenOnDrop` guard as well, and a failed assertion never leaves a thread, or the global pool, held.
`accessible_texts` and `accessible_names` moved from `ui/params.rs`'s tests to `support.rs`, since the Playback test reads the same way.

Removing the `catching_on` around a GPU build on the build thread fails the GPU test ("the build failed"), and building a CPU runner with `SimThread::new` in place of `new_in` fails the fresh-pool test ("stepped on [None]").
Calling `expand_grid` outside `ModelPool::install` fails the viewport test after its 20 s watchdog, and a build that skips the wait for its predecessors fails the slow-runner test ("left: [0]").
Each was checked by hand and reverted.

The existing tests that build now call `AppState::wait_for_build` before they read the runner or count builds: twelve calls in `src/tests/install.rs` and `support.rs::step_past`, three in `state.rs` and two in `ui/playback.rs`.
None of their assertions changed.

### Docs

- `docs/guide/app.md`: Build links to a new Building a model section under the Playback tab (the thread, Building model… with Cancel, commands applied when the run appears, a build waiting for a run still stopping, a browser waiting), and a new A run that does not stop section after Model build and simulation errors (the quarter second, both status lines, a build waiting up to two seconds, the fresh worker threads, a restart to end the run, quitting, a browser freezing).
- `docs/developing/cpu-backend.md` (The runner) describes both `new_in` and the bounded stop, and `docs/developing/architecture.md` (Where a tick runs) the app's build thread, queue, teardown, wait and fresh pool.
- `AGENTS.md`: the henad-compute paragraph names the stop types, the henad-app paragraph describes `load`, `state/build.rs`, `state/pool.rs`, `settle`, the phases, `send_command`, Playback, the teardown, the pool, the UI-side `install` calls, `sim_faults` and the seven tests, the performance bullet names `pool.install`, and Sim runs off the UI thread describes the bounded stop, both `new_in`, the five runner tests and the browser's inline build.
- `CHANGELOG.md`: two Added lines and one breaking Changed line.

### Edited tree

```text
.
├── AGENTS.md, CHANGELOG.md, zensical.toml         ~ runner stop, app build thread; Added and Changed; nav #57
├── crates/henad-compute/src/
│   ├── runner/
│   │   ├── mod.rs                                 ~ DEFAULT_STOP_TIMEOUT, RunnerShutdown, DetachedRunner
│   │   ├── thread.rs                              ~ bounded shutdown, spawn_in, spawn_with, run over closures
│   │   └── frame.rs                               ~ shutdown takes a timeout, returns Joined
│   ├── cpu/sim_thread.rs                          ~ new_in, stop, bounded Drop, Loop::new, stop_tests
│   └── gpu/sim_thread.rs                          ~ new_in, parts, stop, bounded Drop, pool test
├── crates/henad/
│   ├── src/lib.rs                                 ~ runner re-exports
│   └── tests/facade_paths.rs                      ~ named_only::detached
├── crates/henad-app/src/
│   ├── lib.rs                                     ~ poll_build, poll_stopping, sim_faults read in logic
│   ├── sim_runner.rs                              ~ SimRunner::stop
│   ├── state.rs                                   ~ fields, load commits at start, send_command users, is_gpu,
│   │                                                build_runner gone
│   ├── state/
│   │   ├── build.rs                               + BuildJob, settle, phases, PendingBuild, Stopping, start_build,
│   │   │                                            poll_build, send_command, stop_simulation, Drop for AppState
│   │   └── pool.rs                                + ModelPool: current, install, retire
│   ├── tests/
│   │   ├── mod.rs                                 ~ mod runaway
│   │   ├── runaway.rs                             + seven tests
│   │   ├── support.rs                             ~ Gate, OpenOnDrop, StepHold, Probe, held, accessible_texts
│   │   └── install.rs                             ~ wait_for_build
│   └── ui/
│       ├── playback.rs                            ~ building_row, stopping_line, Cancel through pause_simulation
│       ├── pacing.rs                              ~ send_command
│       ├── params.rs                              ~ send_command, has_simulation, helpers moved out of the tests
│       ├── viewport.rs                            ~ grid and density through ModelPool::install
│       ├── agent_layer.rs, edge_layer.rs          ~ uploads take the pool
│       ├── export/image.rs                        ~ grid_pixels through the pool
│       └── sweep/mod.rs                           ~ SweepPanel::model_pool, column builds in the pool
└── docs/
    ├── guide/app.md                               ~ Building a model, A run that does not stop
    ├── developing/cpu-backend.md                  ~ new_in and the bounded stop
    ├── developing/architecture.md                 ~ the app's build thread and teardown
    └── developing/agent-record/
        └── 20261010-57-runaway-rails.md           +
```

## State after

Everything is uncommitted on `49/r-runaway-rails`, on top of `bf72686`, in the worktree `.claude/worktrees/49-r`.
Nothing was committed, pushed or tagged.

All checks below ran on the final tree, after the review fixes.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: the wasm32 typechecks, formatting of the workspace and the excluded packages, packaging, cargo-deny, clippy, 1325 Rust and doc tests with none failed and 6 ignored as before, the README program, the docs with `-D warnings`, the scripts' tests and the web build.
  `a_location_survives_a_panic_on_a_rayon_worker`, the known flaky test, passed.
  The one warning is the future-incompatibility note on naga, wgpu, wgpu-core and wgpu-hal, as on `master`.
  `schema_hashes_are_unchanged_since_0_2_0`, the CLI goldens and JSON-line tests, the tutorial's `snippets`, `parity` and `shaders` tests, `the_readme_program_matches_the_example`, the 0.2.0 fixture tests and the byte-identity sweep tests are among the passed tests.
- henad-app's library tests passed five runs in a row at the default test threads, and once each at one and three test threads, with the global-pool hold of the viewport test beside the others.
- CI's clippy with atomics, over henad-app and over the facade with `app,example-models` on the pinned nightly for wasm32, passes when run by hand, as does clippy for wasm32 without atomics.
  The first version failed it on bodies that a `cfg` left empty in a browser, so `PendingBuild` and `building` exist on every target, and `BuildJob::run`, the job's pool fields and `PHASE_WAITING` are native only, since `pool.install` needs a `Send` closure and a threaded web build's wgpu handles are not `Send`.
- `HENAD_REQUIRE_GPU=1 ./scripts/check_hot.sh` passes: hot-host's app self-check ("the dev host built, loaded and checked 12 models"), every check of `hot-check`, the twenty reloads at 10.5 MiB, and `hot-check relink`.
- `uv run zensical build` reports no issues.

The app ran on the main monitor, a release build with `--features inspection` and `EGUI_INSPECTION=1`, driven through the egui MCP server by its accessibility tree, once by the first version and twice after the review fixes.
The window never painted for a screenshot, as in record #54, and every check read the tree.
After the fixes:

- Build of SIR at 8192 by 8192 showed **Building model…** with Cancel in the Playback tab and Build disabled, then the run, with Run and Offload enabled.
- Play, then Build: Building model… again, and Cancel during the build left nothing loaded, Build enabled, and "Previous run is still stopping." until the abandoned build ended, then no line.
  The log read "Build of SIR Epidemic abandoned before it returned", and no pool was replaced, since the build ended well inside the timeout.
- At 512 by 512, Run to tick 200 stopped at tick 200, and Run to tick 50, behind it, rebuilt on the build thread and stopped at tick 50.
- A one-run sweep from the Sweep tab finished, and Show results brought the Results tab to the front with the run in the Runs view.
  A row of the Runs table took no synthesized click, whether on a cell, on empty space in the row, after a hover, or as a one-point drag, and Open was not pressed in the app, as in record #54.
  `commands_sent_during_a_build_reach_the_new_runner` opens a setup to a tick through the same `open_values`, and the playback test opens two recorded runs.
- Offload of the SIR run ended it at once, with no status line.
- GPU Game of Life showed the GPU batching controls in the Pacing tab while it built, then ran to tick 300 and offloaded.
- Quitting through `NSRunningApplication.terminate` took 37 ms with GPU Game of Life playing and 39 ms with SIR playing.
- An attempt to bring the window to the front, through System Events and then `NSRunningApplication.activate`, raised a renderer fault, "In Surface::configure: Failed to wait for GPU to come idle before reconfiguring the Surface", which offloaded the live SIR run through the fault modal.
  It comes from eframe's surface on a window that was not painting, and no live GPU run, detached thread or build existed at the time.

A scratch directory outside the repository held the logs, the Swift helpers that activated and quit the app, and the first version's benchmark harness, compiled with `cargo check` against this worktree with `fresh` and against a `git archive` of `bf72686` without it.
It is deleted.

Proposed commit: `feat: runaway rails (bounded runner stop, fresh pool, async build with Cancel)`.

### Benchmark gate, for a quiet machine

The plan gates R on the live runner's uncapped `step()` rate on `game_of_life` at 64 by 64 and 1024 by 1024: the base `bf72686` against this milestone, and on this milestone the global pool against a fresh pool.
No timing was run here.
henad-cli steps in a bare loop without `SimThread`, so the gate needs a harness that plays a `SimThread`.
It counts the ticks the runner publishes over a fixed time, snapshots included, as the app sees them.

`Cargo.toml`, with `TREE` the checkout to measure:

```toml
[package]
name = "live-runner-bench"
version = "0.0.0"
edition = "2024"
publish = false

# Outside any workspace.
[workspace]

[features]
# The fresh pool, only in a tree with `SimThread::new_in`.
fresh = []

[dependencies]
henad-core = { path = "TREE/crates/henad-core" }
henad-compute = { path = "TREE/crates/henad-compute" }
henad-models = { path = "TREE/crates/henad-models" }
rayon = "1.10"

# Henad's own release profile.
[profile.release]
opt-level = 2
```

`src/main.rs`:

```rust
//! Uncapped step rate of the live CPU runner on Game of Life.
//!
//! `live-runner-bench SIZE POOL SECONDS` builds game_of_life at SIZE by SIZE, plays a `SimThread` uncapped for one
//! second of warm-up and SECONDS of measurement, and prints the ticks per second. POOL is `global`, the runner of
//! `SimThread::new`, or `fresh`, the runner of `SimThread::new_in` in a pool at the global width.

use std::time::{Duration, Instant};

use henad_compute::cpu::sim_thread::{SimCommand, SimThread};
use henad_compute::entry::ModelState;
use henad_compute::fault::FaultSink;

/// Returns the tick of the newest snapshot the runner published, recycling the ones it passes.
fn latest_tick(thread: &mut SimThread) -> u64 {
    let mut tick = 0;
    while let Some(snapshot) = thread.take_snapshot() {
        tick = snapshot.tick;
        thread.recycle(snapshot);
    }
    tick
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let size: u32 = args[1].parse().expect("SIZE is a whole number");
    let pool = args[2].as_str();
    let seconds: f64 = args[3].parse().expect("SECONDS is a number");

    let models = henad_models::example_models();
    let entry = models.get("game_of_life").expect("game_of_life is an example model");
    let setup = entry
        .setup()
        .set("grid_width", size)
        .and_then(|setup| setup.set("grid_height", size))
        .expect("a valid size");
    let Ok(ModelState::Cpu(state)) = entry.build(setup.values(), setup.seed(), None) else {
        panic!("game_of_life builds on the CPU");
    };
    let mut thread = match pool {
        "global" => SimThread::new(state, 60.0, None, FaultSink::new()),
        #[cfg(feature = "fresh")]
        "fresh" => {
            let fresh = rayon::ThreadPoolBuilder::new()
                .num_threads(rayon::current_num_threads())
                .build()
                .expect("a pool at the global width");
            SimThread::new_in(std::sync::Arc::new(fresh), state, 60.0, None, FaultSink::new())
        }
        other => panic!("POOL is global or fresh (with the `fresh` feature), not {other}"),
    };

    thread.send(SimCommand::SetUncapped(true));
    thread.play();
    std::thread::sleep(Duration::from_secs(1));
    let first = latest_tick(&mut thread);
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let last = latest_tick(&mut thread);
    let elapsed = start.elapsed().as_secs_f64();
    thread.pause();
    println!("{size} {pool} {:.1} ticks/s", (last - first) as f64 / elapsed);
}
```

The commands, from the repository root, with the R tree checked out at `$NEW` (this worktree, or `49-dynamic-loading` once R is committed):

```sh
NEW=$PWD/.claude/worktrees/49-r
BENCH=$(mktemp -d)
git archive --prefix=base/ bf72686 | tar -x -C "$BENCH"
for tree in base new; do
  mkdir -p "$BENCH/bench-$tree/src"
  # Paste the two files above into "$BENCH/bench-$tree".
done
sed -i '' "s|TREE|$BENCH/base|" "$BENCH/bench-base/Cargo.toml"
sed -i '' "s|TREE|$NEW|" "$BENCH/bench-new/Cargo.toml"
# Each harness resolves the dependencies its tree locks.
cp "$BENCH/base/Cargo.lock" "$BENCH/bench-base/Cargo.lock"
cp "$NEW/Cargo.lock" "$BENCH/bench-new/Cargo.lock"
cargo build --release --manifest-path "$BENCH/bench-base/Cargo.toml"
cargo build --release --manifest-path "$BENCH/bench-new/Cargo.toml" --features fresh
OLD_BIN="$BENCH/bench-base/target/release/live-runner-bench"
NEW_BIN="$BENCH/bench-new/target/release/live-runner-bench"

# Interleaved, on a cooled machine: base global, new global, new fresh, five rounds per size.
for size in 64 1024; do
  for round in 1 2 3 4 5; do
    "$OLD_BIN" "$size" global 10
    "$NEW_BIN" "$size" global 10
    "$NEW_BIN" "$size" fresh 10
  done
done
rm -rf "$BENCH"
```

The gate is flat when the three rates of each size agree within the spread of their own rounds.
`new global` against `base global` measures the generic `run`, which should compile to the old loop, and `new fresh` against `new global` measures `pool.install` in place of `rayon::scope` around each pump.
Both enter the pool once per pump from the runner thread, which is not a worker.

## Issues found & future directions

- **A detached GPU runner still reports into the shared device's sink.**
  A CPU runner reports into `sim_faults`, which each teardown replaces.
  A GPU runner's panic and its device errors go to the render context's `FaultSink`, and a detached GPU runner that faults late offloads the run after it, or abandons the build waiting for it.
  A sink of its own for the GPU loop's panics would change `GpuSimThread::new`'s signature, and record #11 kept the one device-wide sink on purpose. Its device errors reach the device's handler whatever the loop's sink is.
  A GPU runner is detached only by a batch longer than 250 ms, and the build after it waits for that batch, so the window is the end of one batch.
- **A sweep still uses rayon's global pool outside its lanes.**
  `henad-explore`'s probe builds on the sweep thread, and a GPU sweep builds each run there, both in the global pool.
  After a stuck run has held every worker of the global pool, a sweep started from the app waits in its probe, and quitting then waits on `SweepRun::drop`'s join.
  The same join already waits for good on a sweep whose own model never returns, which this milestone leaves alone.
  A pool on `SweepRunOptions` and a bounded `SweepRun` stop would close both, as a change to henad-explore.
- **A cancelled build keeps running.**
  Rust cannot interrupt a thread, and a build that never returns spins until the app quits, on the pool it was given.
  The status line reports it as a run, with the same words.
- **A slow build that is cancelled holds the next build up to two seconds.**
  The next build waits for it, as for a slow run, and retires its pool past the timeout.
- **In a browser nothing changes.**
  The build runs inline and a step that never returns freezes the page, as the guide says.

### Deviations from the plan

- The loop's start and its commands run in the pool under `new_in`, as well as each pump.
  The plan names `pool.install(|| rayon::scope(..))` around the pump alone, and the commands that step or run an action would then use the global pool.
  The pump runs as `pool.install(|| sim.pump())`: `install` already runs it on a worker, where a `rayon::scope` around it would add a scope and nothing else.
- `Driver::spawn_in` is public beside `spawn`, since `SimThread::new_in` lives in another module of the crate. `Driver` was already public.
- `GpuSimThread::new_in`, which the plan does not name. Without it a GPU runner built after a stuck CPU run in the global pool would wait for a worker on every pump.
- The app's teardown waits `STOP_WAIT` (250 ms) on the UI thread, a constant of its own, and reports a detached thread against `DEFAULT_STOP_TIMEOUT`.
  The plan's "the timeout" read as one value would either freeze the UI for 2 s on a runaway or never show "Previous run is still stopping.".
- A pool is retired when a detached thread has run past `DEFAULT_STOP_TIMEOUT`, and not at the detach.
  The plan reads "after a CPU detach". A runner whose pump only runs past 250 ms, a long batch of a large model, would otherwise replace the pool on every Build and allocate the new model while the old one still holds its memory.
  The next build waits for the detached thread on its own thread instead. A GPU runner and a build retire their pool past the timeout the same way, since both hold a worker of it.
- The UI thread's own parallel work, and the Sweep tab's column builds, run in the app's pool, which the plan does not name. A stuck run that holds every worker of the global pool would otherwise freeze the next frame that uploads a grid.
- A `load` during a build abandons the build, and Build is disabled while one runs.
  The queue holds commands for the runner alone.
  An open from the Results tab or an install of new code during a build replaces the build rather than waiting for it, since the pending build might never return.
- The loaded fields are committed when the build starts, so live edits and actions queue for the runner, and the panels describe the model being built.
- A CPU runner reports into a fault sink of its own, `AppState::sim_faults`, which the plan does not name.
  A detached runner's late panic would otherwise offload a later run.
- Quit is `impl Drop for AppState`, not `App::on_exit`, whose signature changes with eframe's `glow` feature.
- Five tests beyond the plan's list: `a_runner_that_stops_is_joined`, `a_runner_in_a_pool_steps_on_its_workers`, `a_gpu_runner_in_a_pool_pumps_on_its_workers`, `a_slow_runner_is_waited_for_and_keeps_the_pool` and `a_viewport_frame_after_a_detach_runs_on_the_fresh_pool`, with the GPU validation test the round brief added.

### Questions for the maintainer

- Should Build stay disabled during a build, or replace it as an open from the Results tab does?
- Is 250 ms the wait you want on the UI thread? A shorter wait shows "Previous run is still stopping." for more slow runs, and the build then waits on its thread.
- Should the status line name a cancelled build separately, as in "Previous build is still running."?
- Should `GpuSimThread` take a fault sink of its own for its loop's panics, a breaking change that record #11 decided against?
- Should henad-explore take a pool for its probe and a bounded stop for `SweepRun`, so a stuck sweep cannot hold the app on quit?

### Review findings

Two reviews read the first version of this milestone.

Fixed:

- The UI thread's parallel work ran in the global pool, so a stuck run that held every global worker froze the first frame that uploaded the next run's grid.
  The viewport, the agent and edge layers and the export now run it through `ModelPool::install`, and `a_viewport_frame_after_a_detach_runs_on_the_fresh_pool` holds it.
- A GPU runner always pumped in the global pool. `GpuSimThread::new_in` runs it in the app's pool, and `a_gpu_runner_in_a_pool_pumps_on_its_workers` holds it.
- A runner that was only slow was detached on every teardown, replaced the pool, and overlapped its memory with the next build's.
  The build now waits for it on its own thread, and the pool is retired only past `DEFAULT_STOP_TIMEOUT`. `a_slow_runner_is_waited_for_and_keeps_the_pool` holds it.
- GPU batch settings could not follow a change during a build.
  The Pacing tab showed the CPU controls then, since `is_gpu` read the runner. It now reads the loaded entry during a build, and `finish_build` applies the settings.
- The Sweep tab's column builds ran in the global pool. They now run in the app's pool.
- The Cancel tooltip, "Stop building the model", now "Stop waiting for build".
- The guide said closing the app leaves the stuck run running. It now says the run ends with the app.
- `SimThread::new_in`'s doc left out the loop's start and the commands, `Driver::shutdown`'s doc named only the drop, and `DetachedRunner`'s doc had a garbled sentence.
- AGENTS.md said the app never builds on the UI thread without a bound, which a browser does. The sentence now starts "On native", and names the browser's inline build.
- The CHANGELOG left out `runner::Driver::spawn_in`. It names it, and `GpuSimThread::new_in`.
- The record published a scratch path outside the repository.

Not achieved:

- Open run pressed in the app. A row of the Runs table took no synthesized click at its position, after a hover, or as a one-point drag (State after).

Rejected:

- A fault sink of its own for `GpuSimThread`. Its device errors reach the device-wide sink through the uncaptured-error handler whatever sink the loop's panics use, the GPU loop reads `ctx.faults` to stop, and record #11 kept one sink on purpose. The fix would change `GpuSimThread::new`'s signature for the panics alone. It stays an open issue above.
- A pool for a sweep's probe and a bounded stop for `SweepRun`. Both are henad-explore API outside this milestone, and the same join already waits on a sweep whose own model never returns. It stays an open issue above.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
