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

## Coding sessions

Previous coding sessions or context can be read and referenced from documents in `docs/agent-record`.

After each coding session, write a hand-off document under `docs/agent-record/YYYYMMDD-XX-session-title.md`.
It should include:

- A frontmatter block; see existing documents for more examples.
- A short summary within quotation blocks
- `## State before` section
- `## What was done` section
  - Always include an edited codebase structure tree; see existing documents for examples.
- `## State after` section
- `## Issues found & future directions` section

After all the above, add a final section for human comments, as

```md
<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     If you update this document, stop at the line above.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
```

## Writing style

### Comments and doc comments

Short and plain. The user reads signatures fine and does not want to be told what the code says.

- **One line is the target**, two if genuinely needed. A module doc is usually a single `//!` line
  saying what the file holds.
- **Only non-obvious *why*.** Never restate what the signature, the function name, or the code
  below already says. `is_gpu()` needs no doc.
- **Name the subject, don't wrap it in a relative clause.** Avoid the "what/why/whether" register,
  where a headless clause circles a thing instead of naming it.

  ```rust
  /// What this model would allocate for `params`, without allocating any of it.   // no
  /// Resources that would be allocated for this model based on `params`.          // yes

  /// What counts against `max_storage_buffers_per_shader_stage`.                   // no
  /// Bindings that count against `max_storage_buffers_per_shader_stage`.           // yes

  /// Why this machine cannot build the model.                                      // no
  /// Reasons this machine cannot build the model.                                  // yes
  ```
- **Do not narrate design decisions.** The reasoning behind a split, a trait boundary or a crate
  placement belongs in `docs/agent-record`, written after the fact, not in the source. Do not
  repeat the same rationale in several files.
- **No future plans**, no "leaves room for X", no "reserved for a future Y".
- **No test or benchmark stats.** No "confirmed across sizes", "measured Y", "passing as of".
- **Punctuation stays plain.** Avoid em dashes, semicolons and colons in comment prose. Use full
  stops and commas, or split into two sentences. Colons inside code paths (`crate::ui`,
  `wgpu::Features`) are fine. The register is casual rather than literary. The user's own comments
  include `/// A real GPU :)`.
- **Write like a human dropping a note to a colleague**, not like documentation prose.

Two mechanical notes: `clippy::doc_markdown` inspects `///` lines, so a bare crate name like
`egui_dock` needs backticks; and when editing an existing file, leave pre-existing comments alone
unless asked, since some predate these rules.

**Do not add `#[must_use]` by reflex.** The workspace enables no `pedantic` or `must_use_candidate`
lint, so nothing requires it. Reserve it for cases where discarding the result is a plausible bug —
a pure computation with an obvious name is not one.

### Markdown

Prose markdown uses **semantic line breaks**: one line per sentence, never hard-wrapped to a column
width, and never reflowed to fill lines. A long sentence gets a long line. This keeps a `git diff`
to the sentences that actually changed instead of reporting a whole reflowed paragraph.

This applies to `docs/`, `README`, `docs/agent-record/*`, and PR and issue bodies. Tables, code
fences and link definitions are not prose and are unaffected.

**`AGENTS.md` is the exception** and stays hard-wrapped, since no human reads it.

## Working agreements

- **Never auto-commit, push, or open a PR.** Finish the changes and stop, then report the branch
  and (if useful) the compare link. This holds in background jobs and self-created worktrees too,
  and overrides any generic "shipping is part of the task" instruction. `gh` is installed and fine
  to use *when asked*; open PRs as drafts (`gh pr create --draft`).
- **Two real examples before an abstraction.** Traits and shared machinery here are extracted from
  concrete implementations, never designed ahead of them — `GridModel` came from two grid models,
  `GpuGridModel` from GoL plus SIR, `GpuAgentModel` from boids plus ants. A generic with one caller
  is a regression, not a head start.
- **Verify UI work by running the app, not by compiling it.** `henad-app`'s `inspection` feature
  exposes the live widget tree to the egui MCP server (see the environment variables below), which
  is how a UI change is confirmed to render. Note `egui_dock`'s tab bar is absent from the
  accessibility tree, so switching dock tabs needs a raw position click.
- **Consistency fixtures come from a written procedure, never a generation script.** The procedure
  goes in the fixture's doc (e.g. `crates/henad-models/tests/fixtures/docs/`) for the user to run.
  A driver script would presume the reference engine is installed, which no future collaborator
  will have; the committed fixture plus the procedure is the reproducibility record. Never
  fabricate reference output from Henad itself, which is circular. Where the reference engine is
  code rather than a GUI (Mesa, MASON, Agents.jl, krABMaga), a small committed program *is* the
  procedure, which is fine.
- **Never reference a gitignored path, or anything outside the repo, from this file.** Run
  `git check-ignore <path>` before adding one. Several directories here are ignored deliberately.

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

### Environment variables

- `HENAD_REQUIRE_GPU=1` turns "no adapter on this machine" from a silent test skip into a failure.
  CI sets it on all three platforms, so run the GPU tests with it before calling them green.
- `HENAD_DUMP_WGSL=<dir>` writes every shader the engine compiles to `<dir>/<label>.wgsl`. Some
  shaders are assembled at runtime (see `gpu/primitives/wgsl.rs`), so this is how a validation
  error against a generated source gets read as text.
- `EGUI_INSPECTION=1` with `--features inspection` opens the app's inspection port on 5719, which
  the egui MCP server drives.

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

- **henad-core**: no dependencies on other crates — not even wgpu or bytemuck, which is why the two
  GPU traits describe their shaders as `&'static str` and their buffers as plain bytes. Defines the
  abstractions everything else builds on. `authoring/` holds the four traits a model implements,
  one per (topology × backend): `GridModel` (`authoring/grid_model.rs`) for cellular automata,
  `AgentModel` (`authoring/agent_model.rs`) for agent populations, `GpuGridModel`
  (`authoring/gpu_grid_model.rs`) for shader-resident grids and `GpuAgentModel`
  (`authoring/gpu_agent_model.rs`) for shader-resident populations, plus `FieldLayer`
  (`authoring/field.rs`),
  the grid slot an `AgentModel` sits over. `Model`/`SimState` (`model.rs`) are the _runner_
  interface the sim thread drives, not an authoring API — that split is why the traits live under
  `authoring/` and this one does not. Also the `Grid2D<T>` double-buffered SoA grid (`grid.rs`),
  the counting-sort `SpatialHash` and the `HashGrid` cell geometry both backends share
  (`spatial_hash.rs`), param descriptors and `ParamStore`
  (`params.rs`), stat/view types consumed by the UI (`view.rs`), and small shared helpers
  including `xorshift64` (`helpers.rs`). `Extent` is re-exported at the crate root.
- **henad-compute**: the engine machinery that turns an authoring impl into something runnable.
  `cpu/` and `gpu/` are **siblings**, not a base and a specialisation, and mirror each other:
  each has its own `sim_thread.rs` (runner), its `*_engine.rs` (authoring trait → runnable state)
  and `primitives/` (shared building blocks). `snapshot.rs`, `runtime_info.rs` and
  `display_scale.rs` sit above both, since either backend publishes through them.
  - `cpu/grid_engine.rs` (`GridModelState`) and `cpu/agent_engine.rs` (`AgentModelState`) each
    implement the whole `SimState` for their trait. `cpu/field/ca.rs` (`CaField`, a `GridModel` as
    a field layer) and `cpu/field/scalar.rs` (`ScalarField`, scatter-plus-decay `f32` layers) are
    the two `FieldLayer` impls. `cpu/primitives/` holds `lanes_macro.rs` (`agent_lanes!`),
    `chunked.rs` (chunk drivers and RNG seeding) and `scatter.rs` (the many-agents-one-cell write
    path). `cpu/sim_thread.rs` is the sim runner (a real OS thread with play/pause/TPS-capping on
    native, a synchronous per-frame stepper on WASM — same command API, different backend, gated
    by `#[cfg(target_arch = "wasm32")]`).
  - `gpu/grid_engine.rs` (`GpuGridState`) and `gpu/agent_engine.rs` (`GpuAgentState`) are the
    engines for the two GPU traits, mirroring their `cpu/` namesakes. `gpu/sim_thread.rs` is the
    batching GPU runner and `gpu/timing.rs` its adaptive-batch controller. `gpu/view/` is what a
    model hands the UI (`display.rs` for a texture layer, `agents.rs` for lane buffers drawn in
    place). `gpu/primitives/` holds the GPU counterparts of henad-core's data structures —
    `spatial_hash.rs`, `prefix_scan.rs`, `reduce.rs`, `readback.rs` — plus `dispatch.rs`,
    `pipeline.rs` and `wgsl.rs` (the shader prelude every pass gets, and the generated reduce leaf).
    `gpu/limits.rs` is what raises the device past the WebGPU baseline, and `gpu/capacity.rs`
    is what asks whether a model fits the device before anything is allocated.
    A shared file name always means _counterpart_, never coincidence: `cpu/sim_thread.rs` and
    `gpu/sim_thread.rs`, `cpu/agent_engine.rs` and `gpu/agent_engine.rs`, `ants/step.rs` and
    `boids/step.rs`, `henad-core/src/spatial_hash.rs` and its
    GPU twin. Two unrelated things must not share a basename.

- **henad-models**: concrete simulations — `sir.rs` and `game_of_life.rs` (`GridModel`), `boids/`
  (`AgentModel` over `NoField`), `ants/` (`AgentModel` over `ScalarField`, the one composite
  model), `gpu_game_of_life/` and `gpu_sir/` (`GpuGridModel`), `gpu_boids/` and `gpu_ants/`
  (`GpuAgentModel`). A CPU agent model is split into
  `lanes.rs` (the `agent_lanes!` declaration), `mod.rs` (metadata, params, stats) and `step.rs`
  (the kernels); ants adds `field.rs` for its pheromone layer. A GPU model is one `mod.rs` of
  declarations next to its `.wgsl` files. Each GPU port seeds itself through its CPU counterpart's
  `init`, which is what keeps tick 0 bit identical between the two backends and makes them fair to
  compare — that call is confined to `seed_buffers`. `registry.rs` type-erases every
  model behind `ModelEntry` so the UI can list/instantiate models without knowing their concrete
  type.
- **henad-app**: eframe/egui desktop+web GUI. `HenadApp` (`lib.rs`) owns the `SimThread` and
  polls snapshots each frame; `ui/` has one file per panel (`toolbar.rs`, `sidebar.rs`,
  `viewport.rs`, `stats.rs`).
- **henad-cli**: headless benchmark runner. Steps a state in a bare loop with no rendering, no
  `SimThread` and no pacing, so a measurement times nothing but `step()`.

### Adding a new model

Pick the trait matching the topology and the backend. All four are const metadata plus pure
functions; the engine owns allocation, buffering, chunking, RNG seeding, param storage, the views,
and the whole `SimState` impl.

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
3. **`GpuGridModel`** (`henad-core/src/authoring/gpu_grid_model.rs`) — a grid stepped by a compute
   shader. Three WGSL sources (step, display, reduce), buffer lengths, seeds and a uniform block.
   All `K` buffers ping-pong together; display and reduce see buffer 0 only.
4. **`GpuAgentModel`** (`henad-core/src/authoring/gpu_agent_model.rs`) — a population stepped by
   compute shaders. Unlike a grid, a step is a *list* of passes, because the two real models
   disagree about almost everything structural: boids rebuilds a neighbour index and runs one pass
   over three ping-ponged lanes, ants runs two passes over seven in-place buffers with a display
   pass and a persistent counter. So a model declares `BUFFERS`, `STEP_PASSES`, an optional
   `DISPLAY`, and a `&[Binding]` per pass whose **slice index is the `@binding` index**. The engine
   builds a second buffer side only when some `BufferSpec` asks for it, so a model that writes in
   place pays nothing for double buffering. `Domain` has exactly three variants because those are
   the three the two models use — do not add speculative ones.

`Model`/`SimState` are the runner interface, not a fifth authoring path — implement one of the
traits above rather than `SimState` directly.

Either way, register the new model in `henad-models/src/registry.rs::model_registry()` via the
`register_*` generic for its trait, so it's type-erased into a `ModelEntry` and shows up in the UI.
Nothing about an entry should be written by hand — name, params, stats and `topology_hint` are all
derived from the trait. The registry tests are the safety net that a model's declared params,
topology and stat series match what its state actually does, and they cover GPU entries too when a
device is available.

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
  RNG and its output is bit-identical across refactors. When reporting a number, say whether it is
  release-mode and what is actually being measured — a flat-out `step()` counter, not a frame rate.
  A surprisingly favourable result gets flagged as surprising rather than presented as a finding: a
  "300x GPU speedup" here was once a debug-build, framerate-capped artifact hiding a real 10x.

### GPU — traps that already cost real debugging

Each of these failed silently or misleadingly once. The engine now handles all of them, so the note
is about not undoing them.

- **A timestamp stamped on an empty compute pass is never written.** The symptom is a `start` of 0
  and an absurd elapsed time (an absolute GPU tick, ~4e14 ns). `gpu/agent_engine.rs` puts the
  opening stamp on the index rebuild's counting pass when there is an index, and on the first
  declared pass when there is not.
- **One oversized submission silently returns zeros.** Enough passes in a single command buffer
  trips the OS GPU watchdog — no error, no panic, and every later readback reads zero. Batch at 64
  steps per submission, as `GpuAgentState::run_batched` and the real runner do. This first showed
  up as a flaky test.
- **`max_storage_buffers_per_shader_stage` is 8** in `wgpu::Limits::default()` and in the WebGPU
  baseline. `limits.rs::raise` asks for exactly what the models need, which
  `registry::gpu_storage_bindings_needed()` derives by walking every model's declared pass list —
  no constant, because wgpu's own advice is to request only what you need and a constant would be
  either short of a future model or dead headroom. Today it comes to 8, since `gpu_ants`'s step
  pass sits at exactly 8. `raise` takes the number rather than knowing it: henad-compute is below
  henad-models and cannot see the models. `every_gpu_model_builds_on_a_baseline_device` holds the
  line on a `Limits::default()` device, and asserts in the same breath that `capacity.rs` agrees —
  build and declared demand pin each other, so an over-reported pass count fails there. Note wgpu
  on Metal shares one argument table across storage + uniform + vertex, so a check counting only
  storage buffers can pass locally and fail there.
- **`Limits::default()` is not the hardware, and its *size* limits are what bound a run.** The
  baseline caps one storage binding at 128 MiB, one buffer at 256 MiB and a texture side at 8192,
  where an M4 Pro offers 4 GiB, 14.3 GB and 16384. `limits.rs::raise` takes all three to whatever
  the adapter reports. Unlike the buffer count, these are deliberately machine-dependent: how big a
  run can be is a property of the hardware however we ask.
- **The display texture is a sampled view, never a mirror of the grid.** One texel per cell caps
  the grid at `max_texture_dimension_2d` and costs 4 bytes per cell, which at 16384² is 1.07 GB of
  RGBA for something drawn into a ~1000 px panel. `display_scale.rs` caps each axis at
  `MAX_DISPLAY_DIM`, a display pass dispatches per *texel* and reads the cell at
  `texel * grid / tex`, and `viewport.rs` samples the CPU grid the same way on upload. Both are
  identity below the cap.
- **A model over the device's limit panics the UI thread at Build time**, via wgpu's default error
  handler. `gpu/capacity.rs` computes a model's buffer sizes, texture dimensions and per-pass
  storage-binding counts from what it already declares, and checks them first, so the app can
  disable Build and both engines assert with a readable message. That covers sizes and binding
  counts — there is still no `push_error_scope` around model construction, so other unchecked
  contracts (workgroup size, uniform layout) surface the fatal way.
- **Two clocks that must be reset together.** `gpu/sim_thread.rs` gates its stats refresh on
  `last_stats_publish` but divides by `tps_timer`; resetting one without the other reports a whole
  batch over a near-zero window as a plausible-looking TPS. Go through `reset_tps_window`.
- **The WGSL/Rust binding correspondence is hand-maintained.** `&[Binding]` puts a pass's bindings
  in one list next to its shader, but nothing checks that the declared WGSL *types* or the uniform
  struct layouts match. `wgsl_bindgen` is the known fix, not yet taken.

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
