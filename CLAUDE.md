# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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
Run desktop app: `cargo run -p henad-app`
Run web version locally: `trunk serve` (from repo root, uses `Trunk.toml` + `index.html`)

Toolchain is pinned via `rust-toolchain` (1.97, with rustfmt/clippy/wasm32-unknown-unknown target).

### Lints

`unsafe_code = "deny"` at the workspace level — this is a hard constraint, not a style
preference: the whole cache-efficiency story is supposed to come from safe data layout (SoA,
flat `Vec`s, rayon), not from unsafe tricks. The workspace `Cargo.toml` also enables a large
`clippy::` lint set (`unwrap_used`, `indexing_slicing = "allow"` is a deliberate exception,
`missing_errors_doc`, etc.) — run `./check.sh` rather than guessing whether something will pass CI.

## Architecture

The workspace has 4 crates with a strict dependency direction:

```
henad-core  →  henad-compute  →  henad-models  →  henad-app
(traits/types)   (engine/runners)   (concrete sims)   (egui UI)
```

- **henad-core**: no dependencies on other crates. Defines the abstractions everything else
  builds on: `Model`/`SimState` traits (`model.rs`), the simpler `GridModel` trait for
  cellular-automata-style models (`grid_model.rs`), the `Grid2D<T>` double-buffered SoA grid
  (`grid.rs`), the counting-sort `SpatialHash` for continuous space (`spatial_hash.rs`), param
  descriptors (`params.rs`), stat/view types consumed by the UI (`view.rs`), and small shared
  helpers including the `xorshift64` PRNG (`helpers.rs`).
- **henad-compute**: the engine machinery that turns a `Model`/`GridModel` impl into something
  runnable — `grid_engine.rs` (generic parallel step loop for any `GridModel`), `sim_thread.rs`
  (the sim runner: a real OS thread with play/pause/TPS-capping on native, a synchronous
  per-frame stepper on WASM — same command API, different backend, gated by
  `#[cfg(target_arch = "wasm32")]`), `snapshot.rs` (owned, UI-thread-safe copies of sim state).
- **henad-models**: concrete simulations — `sir.rs` (grid), `game_of_life.rs` (grid),
  `boids/` (continuous-space flocking, split into `state.rs` + `step.rs`). `registry.rs`
  type-erases every model behind `ModelEntry` so the UI can list/instantiate models without
  knowing their concrete type.
- **henad-app**: eframe/egui desktop+web GUI. `HenadApp` (`lib.rs`) owns the `SimThread` and
  polls snapshots each frame; `ui/` has one file per panel (`toolbar.rs`, `sidebar.rs`,
  `viewport.rs`, `stats.rs`).

### Adding a new model

Two paths, matched to how much control the model needs:

1. **`GridModel` trait** (`henad-core/src/grid_model.rs`) — for grid/cellular-automata models.
   Implement const metadata + a handful of pure functions (`init`, `step_cell`, `stats`); the
   engine in `henad-compute::grid_engine` handles `Grid2D` allocation, double-buffering, parallel
   row-wise stepping (rayon on native, sequential on WASM), tick counting, and snapshot
   production. This is the ~50–80 line path (see `game_of_life.rs`, `sir.rs`).
2. **Full `Model` + `SimState` traits** (`henad-core/src/model.rs`) — for anything that isn't a
   simple grid (e.g. continuous space with `SpatialHash`, like `boids/`). You own `step()`,
   parameter handling, and view construction yourself.

Either way, register the new model in `henad-models/src/registry.rs::model_registry()` so it's
type-erased into a `ModelEntry` and shows up in the UI.

### Performance-critical paths — read before touching

- `henad-compute/src/grid_engine.rs::step_rows_moore`/`step_rows_vn` and
  `henad-models/src/boids/step.rs::process_agent`/`step_parallel` are the two hot inner loops.
  Both are parallelized with `rayon` on native and fall back to a sequential loop under
  `#[cfg(target_arch = "wasm32")]` (WASM has no threads here) — any change to the parallel path
  needs a matching change to the WASM path, and the two must stay behaviorally identical.
  Determinism matters: RNG seeding is derived per-row/per-tick specifically so results don't
  depend on how rayon schedules chunks.
- `SpatialHash` (`henad-core/src/spatial_hash.rs`) is a flat counting-sort grid, rebuilt every
  tick from agent positions — this replaced a naive neighbor search and was the biggest lever in
  getting boids to scale. All neighbor queries (including toroidal wraparound) go through
  `query_radius`; don't reintroduce O(n²) neighbor search.
- Data layout is Struct-of-Arrays throughout (`pos_x: Vec<f32>`, `pos_y: Vec<f32>`, ... rather
  than `Vec<Agent>`) specifically for cache locality and rayon-friendliness — preserve this when
  adding fields to a model's state.

### Sim runs off the UI thread

`SimThread` (`henad-compute/src/sim_thread.rs`) exists so simulation stepping never blocks
rendering. On native it's a dedicated OS thread communicating via `mpsc` commands and an
`Arc<Mutex<Option<Snapshot>>>`; the UI thread only ever reads the latest snapshot (`snapshot.rs`)
and never touches the live `SimState` directly. On WASM there's no thread — `SimThread::update()`
is called synchronously from `eframe::App::update()` each frame — but the public API
(`play`/`pause`/`step_once`/`send`) is identical, so `henad-app` code doesn't need to know which
backend is active. When changing `SimThread`, both `native` and `wasm` submodules need updating
together.
