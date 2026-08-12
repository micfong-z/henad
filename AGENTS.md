# AGENTS.md

This file provides guidance to LLM coding agents when working with code in this repository.

## What this is

Henad is a massively parallel agent-based modelling (ABM) engine, targeting 10M+ agents at
interactive speeds on a single machine (path to 100M+ on more powerful hardware). Existing
frameworks (NetLogo, Mesa, MASON) top out around 100k–1M agents because they aren't built for
cache-coherent, parallel data layouts — Henad's whole reason to exist is filling that gap. Every
architectural decision (SoA layout, trait-based plugin system, topology abstractions) is in
service of that scaling target, so when reviewing or writing code, cache-friendliness and
parallelism are not micro-optimizations — they are the point.

## Commands

```bash
./check.sh                    # full CI-equivalent check — run this before considering work done
cargo check --workspace --all-targets
cargo check --workspace --all-features --lib --target wasm32-unknown-unknown   # wasm build must also typecheck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
cargo test --workspace --all-targets
cargo test --workspace --doc
trunk build                   # builds the WASM/web target
```

Run a single test: `cargo test -p henad-models sir_population_conservation`
Run the scatter-strategy benchmark: `cargo bench -p henad-compute --bench scatter`
Run desktop app: `cargo run -p henad-app`
Run web version locally: `trunk serve` (from repo root, uses `Trunk.toml` + `index.html`)
Benchmark a model headlessly: `cargo run --release -p henad-cli -- boids --steps 100 --reps 3`
(`--list` for ids, `--params` for a model's param ids and defaults, `--set id=value` to override
one, `--export-stats` for the time series)
Sweep every model across the config matrix: `python3 scripts/bench_matrix.py` (grid models scale
over grid size, agent models over agent count at constant density; `--dry-run` to see the matrix)

Toolchain is pinned via `rust-toolchain` (1.97, with rustfmt/clippy/wasm32-unknown-unknown target).

### Lints

`unsafe_code = "deny"` at the workspace level — this is a hard constraint, not a style
preference: the whole cache-efficiency story is supposed to come from safe data layout (SoA,
flat `Vec`s, rayon), not from unsafe tricks. The workspace `Cargo.toml` also enables a large
`clippy::` lint set (`unwrap_used`, `indexing_slicing = "allow"` is a deliberate exception,
`missing_errors_doc`, etc.) — run `./check.sh` rather than guessing whether something will pass CI.

## Architecture

The workspace has 5 crates with a strict dependency direction:

```
henad-core  →  henad-compute  →  henad-models  →  henad-app
(traits/types)   (engine/runners)   (concrete sims)   (egui UI)
                                                  ↘  henad-cli (headless bench)
```

- **henad-core**: no dependencies on other crates. Defines the abstractions everything else
  builds on. `authoring/` holds the traits a model implements — `GridModel` (`authoring/grid_model.rs`)
  for cellular automata, `AgentModel` (`authoring/agent_model.rs`) for agent populations,
  `GpuGridModel` (`authoring/gpu_grid_model.rs`) for shader-resident grids, and `FieldLayer`
  (`authoring/field.rs`),
  the grid slot an `AgentModel` sits over. `Model`/`SimState` (`model.rs`) are the *runner*
  interface the sim thread drives, not an authoring API — that split is why the traits live under
  `authoring/` and this one does not. Also the `Grid2D<T>` double-buffered SoA grid (`grid.rs`),
  the counting-sort `SpatialHash` (`spatial_hash.rs`), param descriptors and `ParamStore`
  (`params.rs`), stat/view types consumed by the UI (`view.rs`), and small shared helpers
  including `xorshift64` (`helpers.rs`). `Extent` is re-exported at the crate root.
- **henad-compute**: the engine machinery that turns an authoring impl into something runnable.
  `cpu/` and `gpu/` are **siblings**, not a base and a specialisation, and mirror each other:
  each has its own `sim_thread.rs` (runner), its `*_engine.rs` (authoring trait → runnable state)
  and `primitives/` (shared building blocks). `snapshot.rs` and `runtime_info.rs` sit above both,
  since either backend publishes through them.
  - `cpu/grid_engine.rs` (`GridModelState`) and `cpu/agent_engine.rs` (`AgentModelState`) each
    implement the whole `SimState` for their trait. `cpu/field/ca.rs` (`CaField`, a `GridModel` as
    a field layer) and `cpu/field/scalar.rs` (`ScalarField`, scatter-plus-decay `f32` layers) are
    the two `FieldLayer` impls. `cpu/primitives/` holds `lanes_macro.rs` (`agent_lanes!`),
    `chunked.rs` (chunk drivers and RNG seeding) and `scatter.rs` (the many-agents-one-cell write
    path). `cpu/sim_thread.rs` is the sim runner (a real OS thread with play/pause/TPS-capping on
    native, a synchronous per-frame stepper on WASM — same command API, different backend, gated
    by `#[cfg(target_arch = "wasm32")]`).
  - `gpu/grid_engine.rs` (`GpuGridState`) is the `GpuGridModel` engine. `gpu/sim_thread.rs` is the
    batching GPU runner and `gpu/timing.rs` its adaptive-batch controller. `gpu/view/` is what a
    model hands the UI (`display.rs` for a texture layer, `agents.rs` for lane buffers drawn in
    place). `gpu/primitives/` holds the GPU counterparts of henad-core's data structures —
    `spatial_hash.rs`, `prefix_scan.rs`, `reduce.rs`, `readback.rs` — plus `dispatch.rs` and
    `pipeline.rs`. `gpu/limits.rs` is what raises the device past the WebGPU baseline.
A shared file name always means *counterpart*, never coincidence: `cpu/sim_thread.rs` and
`gpu/sim_thread.rs`, `ants/step.rs` and `boids/step.rs`, `henad-core/src/spatial_hash.rs` and its
GPU twin. Two unrelated things must not share a basename.

- **henad-models**: concrete simulations — `sir.rs` and `game_of_life.rs` (`GridModel`), `boids/`
  (`AgentModel` over `NoField`), `ants/` (`AgentModel` over `ScalarField`, the one composite
  model), `gpu_game_of_life/` and `gpu_sir/` (`GpuGridModel`). An agent model is split into
  `lanes.rs` (the `agent_lanes!` declaration), `mod.rs` (metadata, params, stats) and `step.rs`
  (the kernels); ants adds `field.rs` for its pheromone layer. `registry.rs` type-erases every
  model behind `ModelEntry` so the UI can list/instantiate models without knowing their concrete
  type.
- **henad-app**: eframe/egui desktop+web GUI. `HenadApp` (`lib.rs`) owns the `SimThread` and
  polls snapshots each frame; `ui/` has one file per panel (`toolbar.rs`, `sidebar.rs`,
  `viewport.rs`, `stats.rs`).
- **henad-cli**: headless benchmark runner. Steps a state in a bare loop with no rendering, no
  `SimThread` and no pacing, so a measurement times nothing but `step()`.

### Adding a new model

Pick the CPU trait that matches the topology (`GpuGridModel` is the separate shader path). Both are
const metadata plus pure functions; the engine owns allocation, buffering, chunking, RNG seeding,
param storage, the views, and the whole `SimState` impl.

1. **`GridModel`** (`henad-core/src/authoring/grid_model.rs`) — cellular automata over `u8` cells.
   Implement `init`, `step_cell`, `stats` and the consts; `cpu/grid_engine.rs` does the rest, including the parallel
   row-wise step. Grid width/height are prepended to the param list at indices 0 and 1. See
   `game_of_life.rs`, `sir.rs`.
2. **`AgentModel`** (`henad-core/src/authoring/agent_model.rs`) — a population of agents, optionally over a
   field. Declare lanes with `agent_lanes!`, then implement `init`, `run_step_pass` and `stats`.
   `run_step_pass` is normally one call to the generated `lanes.run_pass(CHUNK, seed, tick, ..)`
   with a per-agent closure, which is where the chunking and seeding happen — see
   `boids/step.rs::run`. `num_agents`, `world_width` and `world_height` are prepended at indices 0,
   1 and 2. `Lanes` comes from the macro; four more associated types pick the behaviour:
   - `Field` — `NoField` (boids), `ScalarField<S>` for scatter-plus-decay layers (ants), or
     `CaField<M>` to put a `GridModel` underneath a population.
   - `Index` — `SpatialHash` when agents read each other, `NoIndex` when they don't.
   - `Tally` — a per-chunk reduction merged in chunk order, `()` when there's nothing to count.
   - `Params` — hot params extracted once per tick, so the kernel does no enum matching.

   A model needing a second pass over agents before the step (ants filling deposit lanes)
   overrides `run_deposit_pass`.

`Model`/`SimState` are the runner interface, not a third authoring path — implement one of the
traits above rather than `SimState` directly.

Either way, register the new model in `henad-models/src/registry.rs::model_registry()` so it's
type-erased into a `ModelEntry` and shows up in the UI. The registry tests are the safety net that
a model's declared params, topology and stat series match what its state actually does.

### Performance-critical paths — read before touching

- `henad-compute/src/cpu/field/ca.rs::step_row_moore`/`step_row_vn` and
  `henad-models/src/*/step.rs` (the per-agent kernels) are the hot inner loops. The x-wrap is
  peeled off both row loops so the interior runs without a per-cell modulo; keep that shape,
  including the `enumerate()` interior loop.
- **Every rayon/wasm `#[cfg]` split lives in `cpu/primitives/chunked.rs`.** There are no longer paired
  `_parallel`/`_sequential` functions to keep in step, and reintroducing one is a regression.
- **`for_each_chunk_mut!` is a macro, not a function, and must stay one.** As a generic fn taking
  `F: Fn(..)` the extra closure layer stopped the kernel inlining through it and cost 48% on SIR;
  `#[inline]` did not help. Same trap applies to any new hot-loop driver.
- Determinism: a chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`, never from anything
  a worker mutates, so results don't depend on how rayon schedules chunks. `base` is advanced once
  per tick on the sequential path by `advance_tick_seed` — folding the tick in only through
  `chunk_seed` measured 14% slower on SIR with identical content, and that was never explained.
  Both agent models have a `results_do_not_depend_on_the_thread_count` test; keep them.
- `AgentModel::CHUNK` is per-model on purpose. It sets both the RNG seeding granularity and the
  parallel load balance, so it must be a fixed const (not derived from the thread count) but still
  small enough to split across every core — 4096 gave only 13 chunks for 50k boids and cost 20%.
  The default 512 is what boids runs on; ants overrides to 4096.
- `SpatialHash` (`henad-core/src/spatial_hash.rs`) is a flat counting-sort grid, rebuilt every
  tick from agent positions — this replaced a naive neighbor search and was the biggest lever in
  getting boids to scale. All neighbor queries (including toroidal wraparound) go through
  `query_radius`; don't reintroduce O(n²) neighbor search.
- `henad-compute/src/cpu/primitives/scatter.rs` (`ScatterGrid`) handles the one write pattern the rest of the
  engine can't: many agents depositing into the same cell. Read its module docs before changing
  it — the strategy choice is measured (`benches/scatter.rs`), not assumed, and **atomics are not
  an option**: `fetch_max` scales negatively under contention (7.1 ms at one thread, 99.2 ms at
  four). Its two arms must stay bit-identical, because the arm is picked from the worker count, so
  any divergence would make a model's results depend on the machine. Re-run the bench rather than
  reasoning about it.
- Data layout is Struct-of-Arrays throughout (`pos_x: Vec<f32>`, `pos_y: Vec<f32>`, ... rather
  than `Vec<Agent>`) specifically for cache locality and rayon-friendliness — preserve this when
  adding fields to a model's state. `agent_lanes!` emits one `Vec<T>` per lane with named field
  access for exactly this reason.
- Benchmarking: this machine drifts up to 40% under sustained load, so only interleaved old-vs-new
  runs on a cooled machine mean anything. Game of Life is the cleanest signal, since it has no step
  RNG and its output is bit-identical across refactors.

### Sim runs off the UI thread

`SimThread` (`henad-compute/src/cpu/sim_thread.rs`) exists so simulation stepping never blocks
rendering. On native it's a dedicated OS thread communicating via `mpsc` commands and an
`Arc<Mutex<Option<Snapshot>>>`; the UI thread only ever reads the latest snapshot (`snapshot.rs`)
and never touches the live `SimState` directly. On WASM there's no thread — `SimThread::update()`
is called synchronously from `eframe::App::update()` each frame — but the public API
(`play`/`pause`/`step_once`/`send`) is identical, so `henad-app` code doesn't need to know which
backend is active. When changing `SimThread`, both `native` and `wasm` submodules need updating
together.

`build_snapshot` calls `SimState::prepare_view` first, which is where a model turns state into
something drawable — ants quantises its `f32` pheromone field into palette indices there. That
runs on publish, not every tick, so anything a view needs but a step doesn't belongs in
`prepare_view` rather than in `step`.
