# Henad — Developer Walkthrough

A file-by-file tour of the codebase as it stands on `master` (`0d24225`), written for the person
who wrote the CPU engine by hand and had the GPU integration built by delegated agents.

Everything here was checked against the source. Where a code comment claims something the code
does not do, that is called out rather than smoothed over. Where the design is awkward or
vestigial, that is said plainly. Line references are `path:line` against the current tree and are
clickable.

**Contents**

1. [Three eras, not two](#1-three-eras-not-two)
2. [`henad-core` — the zero-dependency floor](#2-henad-core--the-zero-dependency-floor)
3. [`henad-compute` — the engine machinery](#3-henad-compute--the-engine-machinery)
4. [`henad-models` — concrete simulations](#4-henad-models--concrete-simulations)
5. [`henad-app` — the egui shell](#5-henad-app--the-egui-shell)
6. [The GPU delta, as architectural pressure](#6-the-gpu-delta-as-architectural-pressure)
7. [Rough edges and open questions](#7-rough-edges-and-open-questions)
8. [Deliberately deferred work](#8-deliberately-deferred-work)

---

## 1. Three eras, not two

The GPU work did not land as one "before/after". Git shows three distinct states, and the
interesting part of the story is the middle one being deleted.

### Era 1 — pure CPU (`67b4ff0`)

`git ls-tree -r --name-only 67b4ff0` shows the four-crate graph with no GPU code anywhere:
`henad-core`, `henad-compute` (`grid_engine` / `sim_thread` / `snapshot`), `henad-models`
(`sir`, `game_of_life`, `boids`, `registry`), `henad-app`. The only wgpu in the build was egui's
own renderer, reached through `eframe`. `henad-compute` and `henad-models` did not depend on wgpu
at all.

The registry's factory was a bare function pointer:

```rust
// 67b4ff0:crates/henad-models/src/registry.rs
pub create: fn(&[ParamValue]) -> Box<dyn SimState>,
```

Remember that line. It is the single constraint that forces most of the GPU integration's shape.

### Era 2 — the GPU spike (`4c67b32` → `e4b2bf7`, merged `529b3a7`; hardened by `15d0987` and `d89a194`, merged `bdfd5bc`)

The GPU Game of Life existed as `crates/henad-app/src/gpu_gol/{mod.rs, sim_thread.rs}` plus three
WGSL files — **1,532 lines living inside the app crate**. It bypassed everything:

- No `Model`, no `SimState`, no registry entry. `HenadApp` held a bespoke
  `gpu_gol: Option<gpu_gol::GpuGolHandle>` field (`bdfd5bc:crates/henad-app/src/lib.rs:67`), built
  in `HenadApp::new` unconditionally, and stepped by its own private thread type `GpuGolHandle`.
- No `Snapshot`. It had its own `GpuGolStats` and pushed pixels through egui's type-keyed
  `CallbackResources` — the paint callback was a unit struct (`struct GpuGolPaint;`), so it looked
  its pipeline up by type at paint time.
- Its own UI: `gpu_gol_panel(ctx, handle)`, a free function opening a standalone window. Because it
  was a free function with no `&mut self`, its play/pause/batch-size state lived in
  `ctx.data()`/`ctx.data_mut()` egui temp storage keyed by id — a workaround for not having a place
  to put state, not a design.

Two follow-ups hardened it in place, and both survive verbatim into era 3:

- **`15d0987`** — the timestamp-query fix. The commit message is worth reading in full because it
  records a *rejected* hypothesis: the proposed diagnosis (a missing `TIMESTAMP_QUERY_INSIDE_PASSES`
  feature) was wrong, since the code uses `ComputePassDescriptor::timestamp_writes`, which
  wgpu-core gates on plain `TIMESTAMP_QUERY` only. The real bug is described in
  [§6.6](#66-the-timestamp-query-fix-15d0987).
- **`d89a194`** — adaptive time-budgeted batching. See [§6.5](#65-the-adaptive-batching-controller-d89a194)
  and the honest accounting in [§7.1](#71-the-adaptive-controller-regulates-encode-cost-not-gpu-cost-at-the-sizes-measured).

### Era 3 — integrated (`33b2aea`, merged `0d24225` = current `master`)

The spike was **deleted** (`-801` and `-731` lines from `henad-app/src/gpu_gol/`) and rebuilt as a
first-class registry model:

- GPU *machinery* → `henad-compute/src/gpu/` (`mod.rs`, `display.rs`, `readback.rs`, `timing.rs`,
  `sim_thread.rs`), a sibling of `grid_engine.rs` / `sim_thread.rs` / `snapshot.rs`.
- Concrete GPU *model* → `henad-models/src/gpu_game_of_life/` (`mod.rs` + three WGSL files).
- Registry, `Snapshot`, and app plumbing changed to match.

The delta from era 1 to era 3, in one command (`git diff --stat 67b4ff0 0d24225`), is +3,444 /
−119 lines across 27 files — **and `crates/henad-core/` appears nowhere in it.** That is not an
accident; see [§6.2](#62-why-henad-core-has-literally-zero-changes).

---

## 2. `henad-core` — the zero-dependency floor

`crates/henad-core/Cargo.toml` has **no `[dependencies]` section at all**. Not "no henad
dependencies" — no dependencies, full stop. That is the crate's defining property and the reason
the GPU integration had to route around it rather than through it.

### `model.rs` — the two traits everything else is defined against

```rust
// crates/henad-core/src/model.rs:6
pub trait Model: Send + Sync + 'static {
    type State: SimState;
    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn param_descriptors(&self) -> Vec<ParamDescriptor>;
    fn stat_descriptors(&self) -> Vec<StatDescriptor>;
    fn topology_hint(&self) -> TopologyHint;
    fn create_state(&self, params: &[ParamValue]) -> Self::State;
}
```

`Model` is the *descriptor* — metadata plus a factory. It is deliberately not object-safe (it has
an associated type); type erasure happens one layer up, in the registry.

`SimState` (`model.rs:19`) is the *running* thing, and it **is** object-safe — that is what lets
`SimThread` hold a `Box<dyn SimState>`:

```rust
pub trait SimState: Send + 'static {
    fn step(&mut self);
    fn tick(&self) -> u64;
    fn grid_view(&self) -> Option<GridView<'_>> { None }   // :22
    fn point_view(&self) -> Option<PointView<'_>> { None } // :25
    fn stats(&self) -> Vec<StatEntry>;
    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool;
    fn population(&self) -> u64;
    fn heap_bytes(&self) -> usize;
}
```

Three things here are load-bearing for the GPU story:

- **`Send`** (`model.rs:19`). Because the CPU sim runs on its own OS thread, `SimState` must be
  `Send`. That single bound is what drags the workspace-wide
  `wgpu/fragile-send-sync-non-atomic-wasm` feature into the root `Cargo.toml` — see
  [§7.4](#74-the-fragile-send-sync-non-atomic-wasm-feature-is-a-wasm-typecheck-workaround).
- **`grid_view()` / `point_view()` return borrowed CPU views**, and both default to `None`. A model
  that has no CPU-side cells simply doesn't override them. That default is the escape hatch the GPU
  model uses (`gpu_game_of_life/mod.rs:472`).
- **`set_param` returns `bool`.** Both sim threads discard that return value
  (`sim_thread.rs:147`, `gpu/sim_thread.rs:263`), so it is currently dead — see [§7.7](#77-smaller-frictions).

### `grid_model.rs` — the cheap path for cellular automata

`GridModel` (`grid_model.rs:13`) is the ~50-line path: const metadata (`NAME`, `ID`, `PALETTE`,
`NEIGHBORHOOD`), an associated `Params` type, and pure functions `init` / `step_cell` / `stats`.
The engine (`henad-compute::grid_engine`) supplies everything else.

The `type Params` associated type is a real optimisation, not ceremony. Its doc (`grid_model.rs:22`)
explains it: parameters are extracted from the `&[ParamValue]` slice **once per tick** into a
plain struct, then passed by reference into every `step_cell`. Without it, the inner loop would be
matching on a `ParamValue` enum per cell. `SirParams` (`sir.rs:20`) is the non-trivial instance;
Game of Life's is `()`.

### `grid.rs` — `Grid2D<T>`, double-buffered SoA

```rust
// crates/henad-core/src/grid.rs:4
pub struct Grid2D<T: Copy + Default> {
    width: u32, height: u32,
    current: Vec<T>,
    next: Vec<T>,
}
```

Two flat `Vec`s, not a `Vec<Vec<T>>` and not a `Vec<Cell>`. `swap()` (`:65`) is a `mem::swap` of
the two `Vec`s — an O(1) pointer swap, no copy. `current_and_next_mut()` (`:60`) hands out
`(&[T], &mut [T])` via a split borrow, which is precisely the shape rayon needs: an immutable read
buffer shared by all threads and a mutable write buffer that can be chunked.

`moore_neighbors()` (`:77`) and `von_neumann_neighbors()` (`:99`) implement the toroidal index
rule with **add-then-mod on unsigned**: `(x + w - 1) % w`, never `x - 1`. On `u32` the latter
underflows at `x == 0`. This same rule reappears in the WGSL (`step.wgsl:17`) for exactly the same
reason — WGSL's `u32` wraps too.

> **But**: `moore_neighbors` / `von_neumann_neighbors` are called **only by their own unit tests**.
> The engine's hot loop re-derives the identical arithmetic inline (`grid_engine.rs:137-151`) so it
> can hoist the `ym`/`yp` computation out of the `x` loop and avoid materialising an `[usize; 8]`.
> The API is therefore vestigial, and its tests do not cover the code that actually runs. See
> [§7.7](#77-smaller-frictions).

### `spatial_hash.rs` — counting sort, rebuilt every tick

`SpatialHash` (`spatial_hash.rs:2`) is a flat, uniform grid over continuous space, stored as three
`Vec<u32>`s: `agent_cells` (cell per agent), `sorted_agents` (agent ids ordered by cell), and
`cell_start` (a prefix-sum offset table of length `num_cells + 1`).

`build()` (`:44`) is a textbook counting sort in three sequential passes: count into
`cell_start[cell + 1]`, prefix-sum, scatter. It reallocates nothing on the steady state (`clear()`
+ `resize()` keep capacity), which matters because it runs **every tick**.

`query_radius()` (`:76`) walks the `(2·cell_radius + 1)²` block of cells around the query point,
wraps each cell index with `rem_euclid`, and then filters by true toroidal distance:

```rust
// crates/henad-core/src/spatial_hash.rs:104
let dx = (raw_dx + half_w).rem_euclid(self.world_w) - half_w;
```

That is the minimum-image convention: shift by a half-period, take the modulus, shift back. It is
the continuous-space analogue of the grid's add-then-mod.

Note `cell_size` is set to `visual_range` by the boids model (`boids/state.rs:57`), which makes
`cell_radius == 1` and the scan a 3×3 block. `rebuild_with_cell_size` (`:114`) throws the whole
structure away and rebuilds when the user drags the visual-range slider.

### `view.rs` — what the UI is allowed to see

`GridView` (`:4`) and `PointView` (`:11`) are *borrowed* views into a live state: `cells: &'a [u8]`,
`pos_x: &'a [f32]`. They are what `SimState::grid_view()` returns, and they exist for exactly one
consumer — `henad-compute`'s `build_snapshot`, which copies them into owned snapshots.

`StatValue` (`:21`) is `Scalar | Vector2D | Histogram`; `StatValue::scalar()` (`:30`) flattens each
to one `f64` for charting (vector → magnitude, histogram → total count).

`StatsHistory` (`:55`) is a ring buffer, one column per stat series, with `get(col, j)` (`:117`)
translating a logical index into a physical one.

> Its doc comment says *"recorded every tick by the model"* (`view.rs:54`) and *"Called once per
> tick by the model"* (`view.rs:76`). **Both are false.** Nothing in `henad-core`,
> `henad-compute` or `henad-models` touches `StatsHistory`; it is pushed by the **app**, once per
> **snapshot**, at `henad-app/src/lib.rs:234-237`. See [§7.5](#75-statshistory-is-sampled-per-snapshot-not-per-tick-and-its-x-axis-is-mislabelled).

### `params.rs`, `topology.rs`, `helpers.rs`

`ParamDescriptor` / `ParamKind` / `ParamValue` (`params.rs`) are the declarative parameter system
that lets `sidebar.rs` generate every model's controls with no per-model UI code
(`sidebar.rs:246-292`). Models address their own params **by positional index** into a
`&[ParamValue]` — see `sir.rs:51` (`extract_f32(params, 2, …)`). This is fragile by construction
and is why the GPU Game of Life has to hard-code `PARAM_WIDTH = 0` / `PARAM_HEIGHT = 1` /
`PARAM_DENSITY = 2` (`gpu_game_of_life/mod.rs:49-51`) to stay index-compatible with the CPU model.

`TopologyHint` (`topology.rs:3`) is declared, stored on every `ModelEntry` (`registry.rs:37`), and
**never read by anything**. The viewport branches on the snapshot variant instead; the only mention
of `TopologyHint` in `henad-app` is a comment explaining why it *isn't* used (`viewport.rs:81`).

`helpers.rs:6` is `xorshift64` — the whole PRNG story. Stateless: takes a `u64`, returns a `u64`.
Everything that needs randomness threads its own state through it, which is what makes
per-row/per-tick reseeding possible.

---

## 3. `henad-compute` — the engine machinery

Depends on `henad-core`, `wgpu`, `bytemuck`, `flume`, `log` — and, on native only, `rayon`. It has
**no dependency on egui or eframe**, and that is a deliberate invariant (see
[§6.1](#61-why-gpu-machinery-in-henad-compute-with-the-device-injected)).

### `grid_engine.rs` — the generic parallel grid stepper

`GridModelState<M>` (`:19`) is the adapter that turns any `GridModel` into a `SimState`. It owns
the `Grid2D<u8>`, the tick counter, the param vector, the RNG state, and a `cached_stats` field.

`step()` (`:58`) is four lines and worth reading exactly:

```rust
fn step(&mut self) {
    let hot = M::from_params(&self.params);              // extract params once
    step_grid::<M>(&mut self.grid, &hot, &mut self.rng_state, self.tick);
    self.grid.swap();                                    // O(1)
    self.tick += 1;
    self.cached_stats = M::stats(&self.grid);            // full-grid scan, EVERY tick
}
```

That last line is a full sequential pass over the grid on **every** tick, whether or not a snapshot
will be published. See [§7.6](#76-cpu-stats-are-computed-every-tick-sequentially-gpu-stats-are-not).

`step_grid` (`:101`) dispatches on `M::NEIGHBORHOOD` **once per tick**, outside the loop, into
either `step_rows_moore` (`:118`) or `step_rows_vn` (`:186`). The neighborhood kind is a `const` on
the trait, so this is a compile-time-known branch that monomorphises away.

The hot loop, native:

```rust
// crates/henad-compute/src/grid_engine.rs:132
next.par_chunks_mut(ws)          // one chunk == one row
    .enumerate()
    .for_each(|(y, next_row)| {
        let row_seed = global_seed ^ tick ^ (y as u64);
        let mut rng = xorshift64(row_seed.max(1));
        let ym = ((y as u32 + h - 1) % h) as usize;   // hoisted out of the x loop
        let yp = ((y as u32 + 1) % h) as usize;
        for x in 0..w { … }
    });
*rng_state = xorshift64(global_seed ^ tick);
```

Three points:

1. **The parallel unit is a row.** `par_chunks_mut(ws)` splits the *write* buffer by row; the read
   buffer is shared immutably. No locking, no atomics, no false sharing except at row boundaries.
2. **Determinism is by construction, not by luck.** Each row derives its RNG seed from
   `global_seed ^ tick ^ y` — so the result is independent of how rayon happens to schedule the
   chunks. The global state is then advanced by a rayon-independent formula (`:156`). This is why
   you can re-run a stochastic model (SIR) and get the same trajectory.
3. **The WASM fallback (`:159`) is not behaviourally identical.** It threads a single `rng` through
   all rows sequentially (`:161`, `:182`) instead of reseeding per row. So a SIR run on the web
   build and the same run on native produce **different trajectories** — each is internally
   deterministic, but they do not agree with each other. The CLAUDE.md rule ("the two must stay
   behaviorally identical") is aspirational here, not enforced. Game of Life is unaffected (it
   ignores the rng).

`GRID_INIT_SEED` (`:16`) is the one thing the GPU integration added to this file. It was previously
a literal inside `from_params`; it is now `pub` so the GPU model can seed its buffer with a
bit-identical grid and be checked against the CPU model as an oracle. That is the entire diff to
`grid_engine.rs` (plus one rustfmt reflow).

### `sim_thread.rs` — the CPU runner, two backends behind one name

`SimCommand` (`:7`) is the wire protocol: `Play | Pause | StepOnce | SetTargetTps | SetUncapped |
SetTicksPerSnapshot | SetParam | Shutdown`.

**Native (`mod native`, `:22`).** `SimThread` (`:31`) is a handle: an `mpsc::Sender<SimCommand>`, an
`Arc<Mutex<Option<Snapshot>>>`, and a `JoinHandle`. The thread runs `SimLoop::run` (`:56`). The loop
has three modes:

- *Paused* (`:58`): blocks on `cmd_rx.recv()`. Zero CPU when idle.
- *Uncapped* (`:69`): runs `ticks_per_snapshot` steps flat out, then drains commands with
  `try_recv`.
- *Capped* (`:82`): computes a deadline and blocks on `recv_timeout(wait)` — so a command arriving
  mid-wait is handled immediately rather than after the sleep. This is why the TPS slider feels
  instant.

`maybe_publish_snapshot` (`:173`) throttles publication to **16 ms** regardless of tick rate. So at
1000 TPS the UI sees ~60 of those 1000 states. `Pause` and `StepOnce` bypass the throttle via
`force_publish_snapshot` (`:182`) so the UI always shows the final state.

`timed_step` (`:155`) wraps each step in an `Instant` and folds it into an EMA (α = 0.1) — that is
the "Engine: N ms" readout in the toolbar.

`Drop` (`:252`) sends `Shutdown` and **joins**. Model switching in the app is just
`self.sim_thread = None` (`lib.rs:160`), which is why teardown is synchronous and safe.

**WASM (`mod wasm`, `:266`).** No thread. `SimThread` owns the state directly, and
`update(dt)` (`:341`) is called from `eframe::App::update()` each frame (`lib.rs:223-229`). It
accumulates `dt` and runs steps until the accumulator drains or `ticks_per_snapshot` is hit. The
public API is identical (`play` / `pause` / `step_once` / `send` / `take_snapshot`), so `henad-app`
never learns which backend it has — except for the one `#[cfg]` block that calls `update`.

`build_snapshot` (`:370`) is shared by both: it asks the state for a `grid_view()`, then a
`point_view()`, and copies whichever it gets into an owned `GridSnapshot` / `PointSnapshot`, or
`SnapshotView::None`. **It can never produce `SnapshotView::Gpu`** — the GPU thread builds its
snapshot itself (`gpu/sim_thread.rs:417`). That asymmetry is deliberate but is the sort of thing
that will bite whoever tries to unify the two threads.

### `snapshot.rs` — the UI/sim boundary

```rust
// crates/henad-compute/src/snapshot.rs:21
pub enum SnapshotView {
    Grid(GridSnapshot),    // owned Vec<u8> of cells
    Points(PointSnapshot), // owned Vec<f32> positions
    Gpu(GpuSnapshot),      // no pixel data at all — an Arc<GpuDisplay>
    None,
}
```

`Snapshot` (`:8`) is the *only* thing the UI thread ever reads. The `Gpu` variant (`:41`) carries
`display: Arc<GpuDisplay>` — a handle to a texture and the pipeline that samples it, and *no cell
data whatsoever*. The `Arc` is what makes mid-frame teardown safe: an egui paint callback in flight
holds its own clone, so dropping the sim thread cannot pull the texture out from under the
renderer.

This file is where the whole "GPU display texture" concern lives, and it lives here rather than in
`henad-core` for a reason spelled out in [§6.2](#62-why-henad-core-has-literally-zero-changes).

### `gpu/mod.rs` — the injected context

```rust
// crates/henad-compute/src/gpu/mod.rs:38
#[derive(Clone)]
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
}
```

Cheap to clone (both handles are refcounted). `target_format` is in the context rather than a
per-call argument because a display *render pipeline* is compiled against a colour-target format
once, at construction.

The module doc (`gpu/mod.rs:5-9`) states the invariant: **nothing in this module ever creates a
device.** See [§6.1](#61-why-gpu-machinery-in-henad-compute-with-the-device-injected).

### `gpu/display.rs` — model-agnostic "texture → viewport"

`build_display_target` (`:33`) creates an `Rgba8Unorm` texture with
`STORAGE_BINDING | TEXTURE_BINDING` (`:50`) — writable from a compute shader, samplable from a
fragment shader — plus a nearest-neighbour sampler, a bind group, and a render pipeline over
`display.wgsl`.

`display.wgsl` is the classic fullscreen-*triangle* trick: three vertices, no vertex buffer, UVs
derived from `vertex_index` bit-twiddling (`display.wgsl:9-12`), `draw(0..3, 0..1)`.

The split matters: `DisplayTarget` (`:26`) holds the `TextureView` (the model writes into it) *and*
an `Arc<GpuDisplay>` (`:17`), which is render-only — a pipeline and a bind group, nothing else.
Only the latter crosses into the snapshot. The UI's paint callback therefore *cannot* touch
simulation state even if it wanted to.

This lives in `henad-compute` and not in `henad-models` because it is genuinely model-agnostic: a
GPU model's only obligation is "write RGBA into this texture view".

### `gpu/readback.rs` — one `u32`, asynchronously

`U32Readback` (`:23`) is the mechanism that lets a GPU model report a stat without ever copying the
grid back. It owns a 4-byte storage buffer (the reduce shader's target) and a 4-byte staging buffer
(`MAP_READ | COPY_DST`).

The protocol, in order:

1. `encode_clear` (`:67`) — zero the accumulator, recorded *before* the model's reduce pass.
2. (model records its reduce pass)
3. `encode_copy` (`:77`) — storage → staging, recorded *after*, in the same encoder, so wgpu
   inserts the barrier. **Skipped if a previous map is still in flight** (`:78`) — writing into a
   pending-map buffer is invalid, and dropping a sample is harmless.
4. `begin_map` (`:88`) — called immediately *after submitting* that encoder.
5. `poll` (`:107`) — non-blocking. Calls `device.poll(PollType::Poll)` (which is what actually runs
   wgpu's map callbacks on native) and `try_recv`s the flume channel.

`poll_blocking` (`:125`) is the same thing with `PollType::wait_indefinitely()`, and its doc is
explicit that it is only for one-shot snapshots (initial / pause / step-once) — never the hot loop.

The reasoning is in the module doc (`:8-18`) and is the correct one: the sim thread's job is to keep
the GPU queue saturated, so a stall that waits for the queue to *empty*, 60×/second, would cap
throughput at roughly one in-flight batch per frame. The price is that the alive count is a few
milliseconds stale — which is the same staleness the display texture already accepts.

### `gpu/timing.rs` — one diagnostic, one controller

Two unrelated things share this file, and the module doc (`:1-5`) says so up front:

**`TimestampQuery` (`:43`) — diagnostic only.** A 2-entry query set; the model stamps the start of
its first step-pass and the end of its last. `read_gpu_us_per_step` (`:133`) maps the readback
buffer, reads two `u64` ticks, multiplies by `period_ns` and divides by batch size. Blocking; called
at most once per second.

`resolve_after` (`:105`) is the fix from `15d0987` and its doc comment is the best thing in the
file. Detail in [§6.6](#66-the-timestamp-query-fix-15d0987).

**The adaptive controller — two pure functions and three constants.**

```rust
// crates/henad-compute/src/gpu/timing.rs:158, :171, :181
pub fn ema_update(prev: Option<f64>, sample: f64, alpha: f64) -> f64 { … }
pub fn next_batch_size(ema_ms: f64, target_ms: f64) -> u32 {
    let raw = target_ms / ema_ms.max(f64::EPSILON);
    (raw as u32).clamp(1, MAX_BATCH_SIZE)
}
pub fn time_per_step_ms(elapsed: Duration, batch_size_submitted: u32) -> f64 { … }
```

`DEFAULT_TARGET_MS = 8.0` (`:18`), `ADAPTIVE_EMA_ALPHA = 0.25` (`:26`), `MAX_BATCH_SIZE = 4096`
(`:40`). Keeping them as free functions of scalars is what makes them unit-testable without a GPU —
there are 9 tests at `:186-245`, and they pin the interesting edges (first-sample seeding, division
blow-up at zero EMA, truncate-not-round so a batch lands *under* budget).

Note `next_batch_size` is a *proportional* controller with no integral or derivative term, no
hysteresis, and no rate limit. It is a one-shot division, and the only smoothing is in the EMA of
the input. That is fine for a signal that moves slowly; it is worth knowing when the signal is
noisy.

### `gpu/sim_thread.rs` — `GpuSimState` and the batching runner

**`GpuSimState: SimState` (`:76`)** is the interface the GPU thread drives. Four methods:

- `encode_steps(&mut self, encoder, count, timestamps)` (`:81`) — record `count` steps into one
  encoder and advance the tick counter by `count`.
- `encode_snapshot_passes(&mut self, encoder)` (`:90`) — record the display pass and the stats
  reduction, at snapshot cadence, not every step.
- `begin_stats_readback` (`:94`) / `poll_stats_readback(device, block)` (`:102`).
- `display(&self) -> Arc<GpuDisplay>` (`:105`).

The module doc (`:13-25`) is careful about what this trait is *not*: it is a **runner** interface,
the GPU analogue of how `SimState` is consumed by the CPU thread — not a model-authoring shortcut
like `GridModel`. A GPU model still writes its `Model` + `SimState` impls by hand. It exists at all
because of the crate split: the machinery lives in `henad-compute`, models live in `henad-models`,
and `henad-compute` cannot name a type from a crate that depends on it.

**`GpuSimLoop::step_batch` (`:340`) is the heart of the thing.** One iteration:

```rust
let now = Instant::now();
let want_timing   = timestamp_query.is_some() && now - last_stats_publish   >= STATS_INTERVAL;   // 1s
let want_snapshot = now - last_snapshot_publish >= SNAPSHOT_INTERVAL;                            // 16ms

let mut encoder = self.encoder("henad_gpu_sim_encoder");
self.state.encode_steps(&mut encoder, self.batch_size, query_set);   // N compute passes
if want_snapshot { self.state.encode_snapshot_passes(&mut encoder); } // + display + reduce

let write_submission = self.ctx.queue.submit(Some(encoder.finish()));
let batch_wall_elapsed = Instant::now().duration_since(now);          // ← the controller's signal
```

Note precisely **where** the measurement window closes: immediately after `submit()` returns
(`:361-362`), and *before* the timestamp resolve (`:372`) and before the readback poll (`:370`).
That ordering is correct and deliberate — the once-per-second blocking resolve does **not**
pollute the EMA. It is also, exactly, why the signal is CPU-side encode-and-submit time and not GPU
execution time; see [§7.1](#71-the-adaptive-controller-regulates-encode-cost-not-gpu-cost-at-the-sizes-measured).

The two cadences are independent of each other and of batch size: a display refresh every 16 ms, a
timestamp readback + TPS refresh every 1 s. "Steps per snapshot" is therefore *emergent* — however
many steps happened to fit in 16 ms.

`handle_command` (`:244`) accepts and **silently ignores** `SetTargetTps`, `SetUncapped` and
`SetTicksPerSnapshot` (`:269-273`), with a comment explaining why: this thread paces itself with the
batch-size controller and has no TPS cap. Accepting-and-ignoring (rather than erroring) is what lets
`SimRunner` forward one command stream to either backend.

`publish_snapshot` (`:417`) reports `engine_ms: self.gpu_us_per_step.unwrap_or(0.0) / 1000.0`
(`:425`) — so the toolbar's "Engine" number means *true GPU time per step* for a GPU model and
*CPU wall time per step* for a CPU model. Same slot, two different quantities. Documented, but worth
remembering before quoting either in a paper.

---

## 4. `henad-models` — concrete simulations

### `game_of_life.rs` — the `GridModel` reference

76 lines of model. `step_cell` (`:57`) is the whole rule:

```rust
let alive_count: u8 = neighbors.iter().map(|&n| n & 1).sum();
match (cell, alive_count) {
    (ALIVE, 2..=3) | (DEAD, 3) => ALIVE,
    _ => DEAD,
}
```

`PALETTE` (`:16`) was made `pub` by the GPU integration so the GPU variant reuses the same literal
for its *stat colour*. Its doc comment is honest about what that does **not** cover: the GPU display
shader still bakes the same RGB values into WGSL constants (`gpu_game_of_life/display.wgsl:6-7`).
Two sources of truth for one palette.

### `sir.rs` — the stochastic `GridModel`

Same shape, but `type Params = SirParams` (`:20`) and `step_cell` (`:69`) consumes the rng. The
infection rule is `1 - (1 - β)^k` for `k` infected neighbours, evaluated as "draw once, compare
against `prob_safe`" (`:74-80`) — one rng draw per susceptible cell with at least one infected
neighbour, not one per neighbour.

`stats` (`:99`) is a three-way count over the whole grid. It runs every tick (see
[§7.6](#76-cpu-stats-are-computed-every-tick-sequentially-gpu-stats-are-not)).

### `boids/` — the full-`Model` path

This is the model that justifies `Model`/`SimState` existing separately from `GridModel`. It is
continuous-space, so there is no `Grid2D`, no `step_cell`, and no palette-indexed cell.

`BoidsState` (`state.rs:15`) is SoA throughout — eight parallel `Vec<f32>`s (`pos_x`, `pos_y`,
`vel_x`, `vel_y` and their `next_*` twins) plus the `SpatialHash`. `swap_buffers` (`:116`) is four
`mem::swap`s.

`step` (`step.rs:5`) is: rebuild the hash, run the per-agent kernel, swap, recompute stats,
increment tick.

`process_agent` (`step.rs:46`) is the hot kernel. It queries the hash, then in one pass over the
candidate list accumulates separation (`dist_sq < protected_sq`), alignment and cohesion
(`dist_sq < visual_sq`). The toroidal correction is the branchy variant of minimum-image
(`:74-83`) — note this is *different code* from `SpatialHash::query_radius`'s `rem_euclid` version
(`spatial_hash.rs:104`) computing the same thing.

`step_parallel` (`step.rs:135`) zips four `par_iter_mut()`s over the `next_*` arrays. The neighbour
scratch buffer is a `thread_local!` `RefCell<Vec<u32>>` (`:162`) — so it is allocated once per rayon
worker and reused across every agent that worker handles, instead of once per agent. That is the
allocation that would otherwise dominate.

There is no per-agent rng in the boids step at all, so unlike the grid engine, the native and WASM
paths here really are behaviourally identical.

### `gpu_game_of_life/` — the GPU model

**Resources (`mod.rs:180-423`).** Two `array<u32>` storage buffers (`buffer_a`, `buffer_b`) — one
`u32` per cell, no bit packing — plus a `dims` uniform, the display target, the `U32Readback`, and
three compute pipelines (step / display / reduce), each with two pre-built bind groups (one per
ping-pong direction). `current_is_a: bool` (`:177`) selects.

Building both bind-group directions up front means `encode_steps` does zero allocation per step: it
picks `bind_a2b` or `bind_b2a` and flips the bool.

**Seeding (`:75`).** `seed_random` reproduces `GameOfLifeModel::init` exactly — same `xorshift64`,
same traversal order, same `(density * u32::MAX) as u32` threshold, same `GRID_INIT_SEED`. That is
what makes the CPU model a usable oracle, and the test at `:846`
(`gpu_alive_count_matches_cpu_model`) cashes it in: 10 ticks of tick-for-tick agreement on the alive
count.

**`encode_steps` (`:522`) — one compute pass per step.**

```rust
for i in 0..count {
    let bind_group = if self.current_is_a { &self.bind_a2b } else { &self.bind_b2a };
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor { … });
    pass.set_pipeline(&self.step_pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(wg_x, wg_y, 1);
    drop(pass);
    self.current_is_a = !self.current_is_a;
}
self.tick += u64::from(count);
```

The one-pass-per-step structure is **forced**, not stylistic: each step is a read-after-write hazard
on the ping-ponged buffers, and **wgpu inserts synchronisation barriers only between passes, not
between dispatches within a pass**. Looping `dispatch_workgroups` inside a single pass would read
stale data. Batching therefore happens at the *submission* level — N passes in one encoder, one
`submit()`. (This is also the source of the CPU cost discussed in
[§7.1](#71-the-adaptive-controller-regulates-encode-cost-not-gpu-cost-at-the-sizes-measured):
`begin_compute_pass` is not free.)

The timestamp plumbing (`:543-549`) stamps only the first and last passes, because a
`ComputePassTimestampWrites` requires at least one of its two indices to be `Some`.

**`encode_snapshot_passes` (`:563`)** records: display pass → `encode_clear` → reduce pass →
`encode_copy`. All in one encoder, so wgpu supplies the barriers.

**The shaders.**

- `step.wgsl` — 16×16 workgroup, bounds-guarded, toroidal wrap by `(x + width - 1u) % width`
  (`:17-20`). The `x - 1u` form would underflow `u32` at the left/top edge and index far out of
  bounds. This is the same rule as `Grid2D::moore_neighbors`, restated for the third time in the
  codebase.
- `display.wgsl` — cell → RGBA into `texture_storage_2d<rgba8unorm, write>`. Palette hard-coded.
- `reduce.wgsl` — **two-level reduction**: every invocation `atomicAdd`s into a `var<workgroup>`
  atomic, then one invocation per workgroup does a single `atomicAdd` into the global total
  (`:38-40`). One global atomic per 256 cells instead of one per cell. Note the guard at `:30` is an
  `if`, not an early `return`, because both `workgroupBarrier()`s must be reached by every
  invocation in the workgroup — an early return on a partial edge tile would be non-uniform control
  flow across a barrier.

**Tests.** `mod tests` (`:608`) drives the state directly: `gpu_matches_cpu_reference` (`:731`),
`blinker_returns_after_two_ticks` (`:771`), `gpu_reduction_counts_known_pattern` (`:810`),
`gpu_alive_count_matches_cpu_model` (`:846`), `gpu_timing_readback_is_stable_over_many_batches`
(`:922` — the `15d0987` regression test), `population_is_total_cells_not_alive_count` (`:970`).
`mod runner_tests` (`:999`) drives the real `GpuSimThread` end-to-end: snapshot variant, play,
step-once on a blinker, and teardown/respawn on a shared context. All of them
`return` early with a `log::warn!` when no adapter is available, so they are no-ops in CI without a
GPU — worth knowing before trusting a green `check.sh` on a headless box.

### `registry.rs` — type erasure, now with a closure

```rust
// crates/henad-models/src/registry.rs:17
pub enum ModelState {
    Cpu(Box<dyn SimState>),
    Gpu(Box<dyn GpuSimState>),
}

// :28
pub type ModelFactory = Box<dyn Fn(&[ParamValue]) -> ModelState + Send + Sync>;
```

Both changes are forced; see [§6.3](#63-why-modelentrycreate-became-a-boxed-closure-and-why-modelstate-exists).

`model_registry(gpu: Option<GpuContext>)` (`:99`) registers the three CPU models unconditionally
and pushes the GPU entry only if a context was supplied (`:106-108`). The doc (`:95-98`) states the
policy: a model the user can *see* in the dropdown should always be one they can actually *run*, so
GPU entries are omitted, not listed-then-erroring. `registry_without_gpu_context_offers_no_gpu_models`
(`:118`) pins it.

---

## 5. `henad-app` — the egui shell

### `main.rs` — where the device actually comes from

`main.rs:11-40` is the one place in the whole workspace that influences device creation, and it does
so by *decorating egui's own descriptor*:

```rust
device_descriptor: std::sync::Arc::new(|adapter| {
    let base = egui_wgpu::WgpuSetupCreateNew::default();
    let mut descriptor = (base.device_descriptor)(adapter);
    if adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        descriptor.required_features |= wgpu::Features::TIMESTAMP_QUERY;
    }
    descriptor
}),
```

egui still owns acquisition; we only OR in one optional feature, and degrade to "GPU time/step: N/A"
if the adapter lacks it (`sidebar.rs:322-325`). Note the **limits** come from egui's default and are
never widened — which turns out to matter a great deal; see
[§7.3](#73-the-gpu-grid-size-sliders-go-far-past-what-the-device-can-allocate).

### `lib.rs` — `HenadApp`

`HenadApp` (`:54`) holds the registry, the selected index, the current param values, an
`Option<SimRunner>`, the latest `Snapshot`, the egui texture handle and pixel scratch buffer, the
`StatsHistory`, and — new in era 3 — the `GpuContext` plus three GPU batching fields
(`gpu_adaptive`, `gpu_target_ms`, `gpu_batch_size`, `:82-86`).

Those three fields are the direct replacement for the spike's `ctx.data()` temp storage. They are
also the *source of truth handed to a freshly spawned thread* (`GpuBatchSettings`, `:191-195`), so
adaptive settings survive a Reset.

`HenadApp::new` (`:90`) builds the `GpuContext` from egui's `RenderState` (`:102-107`) on native,
and hard-codes `None` on wasm (`:108-109`). That single `None` is what removes GPU models from the
web build — no `#[cfg]` in the registry, no error path, the entry simply isn't constructed.

`reset_simulation` (`:155`) drops the old runner (which sends `Shutdown` and joins), then matches on
the factory's `ModelState` to decide which runner to spawn (`:175-203`). The wasm arm
(`:198-202`) logs an error and returns — unreachable in practice, since the registry never offers a
GPU entry there.

`update` (`:220`) is: drive the sim synchronously if wasm → poll a snapshot and push its stats into
the history (`:232-240`) → toolbar → sidebar → viewport, timing the UI and render phases into EMAs.

### `sim_runner.rs` — the thin enum

```rust
// crates/henad-app/src/sim_runner.rs:20
pub enum SimRunner {
    Cpu(SimThread),
    #[cfg(not(target_arch = "wasm32"))]
    Gpu(GpuSimThread),
}
```

95 lines, almost all of it forwarding `send` / `play` / `pause` / `step_once` / `take_snapshot`. The
only asymmetric members are `gpu_stats()` (`:73`) and `as_gpu_mut()` (`:81`), both native-only, both
returning `None` for the CPU arm — that is how the sidebar decides whether to show GPU controls.

Why an enum instead of `Box<dyn SimRunnerTrait>`: two implementors, both known at compile time, and
one of them doesn't exist on wasm. A trait object would buy nothing and would need a `#[cfg]`-ed
trait anyway.

### `ui/viewport.rs` — two rendering paths

The GPU path is checked **first**, before any CPU-side pixel work:

```rust
// crates/henad-app/src/ui/viewport.rs:82
if let Some(SnapshotView::Gpu(gpu)) = app.snapshot.as_ref().map(|s| &s.view) {
    paint_gpu_view(ui, Arc::clone(&gpu.display));
    return;
}
```

It branches on the **snapshot variant**, not on `TopologyHint` — GPU Game of Life is still
`TopologyHint::Grid2D` (`gpu_game_of_life/mod.rs:136`), it just gets its pixels differently.

`GpuViewportPaint` (`:18`) carries its own `Arc<GpuDisplay>` and `paint()` (`:23`) does exactly
three calls: set pipeline, set bind group, `draw(0..3, 0..1)`. Contrast the spike, whose paint
callback was a unit struct that looked its resources up in egui's type-keyed `CallbackResources` —
which is precisely what made teardown unsafe, because the resources outlived (or didn't) independent
of the frame in flight.

The CPU grid path (`:90-134`) is unchanged from era 1: convert `cells` → RGBA in `pixel_buf`
(rayon-parallel on native, `:105-115`), build a `ColorImage`, and `tex.set()`. Note it is gated on
`last_rendered_tick != current_tick`, so a paused sim costs nothing.

The points path calls `render_density_heatmap` (`:198`), which bins agents into a fixed 512×512
histogram using per-rayon-chunk partial buffers and a sequential merge (`:217-237`), then colours it
with a 5-stop piecewise-linear Inferno approximation (`:174`).

### `ui/sidebar.rs`

Two panels. The right one shows current stats and the history chart; the left one is controls.

The interesting bit is `:126-136`: `is_gpu` is derived from `runner.gpu_stats().is_some()`, and when
true the CPU pacing controls (target TPS, unlimited, ticks/snapshot) are **swapped out** — not
disabled, not shown-alongside — for `gpu_batching_controls` (`:313`). They would be inert
otherwise, since the GPU thread ignores those three commands.

`gpu_batching_controls` shows the live GPU µs/step readout, an "Adaptive batching" checkbox, and
then either a **disabled** slider displaying the controller's live output (`:345-350`, ranged
`1..=MAX_BATCH_SIZE` so it can never silently clamp and thus misreport the controller) or an
editable fixed batch-size slider (`:366`).

Parameter widgets (`:246-292`) are generated purely from `ParamDescriptor`s — no per-model UI code
anywhere.

`mod stats` (`:376`) — the history line chart, the vector-arrow plot for `Vector2D` stats, and the
histogram bar chart — is nested **inside `sidebar.rs`**, while `ui/stats.rs` is a two-line comment
saying the module is "reserved for a future standalone stats panel". Harmless, but it means grepping
for the stats chart in the obvious file finds nothing.

### `ui/toolbar.rs`

Tick / TPS / Pop / Sim mem on the left; theme, FPS, and `Engine / Render / UI` timings on the right.
`Sim mem` is `snapshot.heap_bytes + app.pixel_buf.len()` (`:57`) — and for a GPU model
`heap_bytes()` reports *device* memory (`gpu_game_of_life/mod.rs:502`), so the label is a little
loose but the number is meaningful.

---

## 6. The GPU delta, as architectural pressure

Each of the following is a consequence of a constraint that already existed. None of them is a
preference.

### 6.1 Why GPU machinery in `henad-compute`, with the device injected

The constraint: **`henad-compute` must not depend on egui/eframe.** It is the engine; a headless CLI
benchmark runner (which the paper will need) must be able to use it without pulling in a GUI
toolkit.

But a `wgpu::Device` has to come from *somewhere*, and in the app that somewhere is egui's
`RenderState` — because the sim and egui must share one device and one queue (otherwise the sim's
texture is not one egui can sample).

The resolution is to invert it: `henad-compute::gpu` never *creates* a device, it only ever
*receives* cloned handles in a `GpuContext` (`gpu/mod.rs:38`). Acquisition stays with whoever owns
it — egui's `RenderState` today (`lib.rs:102-107`), a headless runner tomorrow, a test's own
`request_adapter` in between (`gpu_game_of_life/mod.rs:617`). `henad-compute` gains a `wgpu`
dependency but not an egui one, and the crate graph is unchanged.

The concrete model goes in `henad-models` for the same reason the CPU models do: shaders, pipelines,
bind groups and "what a cell means" are model knowledge, and `henad-compute` is not allowed to know
about any particular model. That is also *why* `GpuSimState` has to exist as a trait
(`gpu/sim_thread.rs:76`) — `henad-compute` needs to drive something it cannot name.

### 6.2 Why `henad-core` has literally zero changes

Verified: `git diff 67b4ff0 0d24225 -- crates/henad-core` is **empty**. Not "small" — empty.

The mechanism is a chain of two constraints:

1. `henad-core` has no dependencies (see its `Cargo.toml`). So it can never name a `wgpu` type.
2. The GPU display texture is *inherently* a wgpu type (`wgpu::TextureView`, `wgpu::RenderPipeline`,
   `wgpu::BindGroup`).

Therefore the GPU display **cannot** travel through `SimState::grid_view()` → `View`, because
`GridView` lives in `henad-core/src/view.rs:4` and would have to grow a wgpu-typed variant. The only
way to make that work would be to either (a) give `henad-core` a wgpu dependency — destroying the
property that makes it the floor of the graph — or (b) invent some generic handle type in
`henad-core` that wgpu types could be smuggled through, which is the same thing with extra steps.

So instead the texture flows through **`Snapshot`**, which lives in `henad-compute/src/snapshot.rs`
— a crate that *does* depend on wgpu. `SnapshotView` gained one variant (`snapshot.rs:26`) carrying
`GpuSnapshot { display: Arc<GpuDisplay> }` (`:41`), and the GPU model simply never overrides
`grid_view()` (`gpu_game_of_life/mod.rs:472`, where the absence is documented as a deliberate
non-override).

You can trace the constraint straight through the code:

| Layer | File | What it may know about wgpu |
| --- | --- | --- |
| `henad-core` | `view.rs`, `model.rs` | nothing — no dependency exists |
| `henad-compute` | `snapshot.rs:41`, `gpu/*` | everything — `Snapshot` carries the texture |
| `henad-models` | `gpu_game_of_life/` | everything — writes the shaders |
| `henad-app` | `ui/viewport.rs:23` | everything — samples the texture |

`Snapshot` was already the UI/sim boundary object. Making it the GPU boundary object too is the
smallest possible change consistent with the constraint. It is genuinely the right seam.

### 6.3 Why `ModelEntry.create` became a boxed closure (and why `ModelState` exists)

Era 1's factory was `create: fn(&[ParamValue]) -> Box<dyn SimState>` — a bare function pointer.

A GPU model needs a device to build its buffers and pipelines. A **`fn` pointer cannot capture**.
The options were:

- thread a `&GpuContext` through every `create` call site, changing the signature for all models and
  forcing the app to hold a context even for CPU models; or
- let the *entry* capture its own context at registration time.

The second is strictly less invasive, and it requires the factory to be a closure:

```rust
// crates/henad-models/src/registry.rs:28
pub type ModelFactory = Box<dyn Fn(&[ParamValue]) -> ModelState + Send + Sync>;
```

`register_gpu_game_of_life` (`:74`) clones the context into the closure (`let factory_ctx =
ctx.clone();`, `:76`), so calling `(entry.create)(&params)` needs no context argument at all — the
test at `gpu_game_of_life/mod.rs:1168` explicitly notes this ("Note there is no context argument
here"). The cost is one `Box` and one dynamic dispatch **per model instantiation**, which happens on
a button press.

`model_registry(Option<GpuContext>)` (`:99`) then omits GPU entries when there is no device. The
alternative — always list them, fail on select — was rejected, and rightly: the dropdown is a
promise.

`ModelState` (`:17`) is the *second* half of this and is the piece that went beyond the originally
approved design. The problem it solves is real: `SimThread` needs a `Box<dyn SimState>` and
`GpuSimThread` needs a `Box<dyn GpuSimState>`, and `GpuSimState` is a *sub*trait of `SimState`, so a
factory returning `Box<dyn SimState>` would have thrown away exactly the information the app needs
to pick a runner. Recovering it would mean downcasting (`Any`), which needs a `'static` +
`as_any()` escape hatch on `SimState` — worse. Tagging the return value instead makes it
*impossible* to hand a GPU state to the CPU thread. See [§7.2](#72-modelstate-was-an-agents-addition-beyond-the-approved-design).

### 6.4 Why the GPU sim thread is a separate type, not a unification

`SimThread`'s loop calls `state.step()` **once per iteration** (`sim_thread.rs:157`). Every pacing
decision it makes — the TPS deadline, `ticks_per_snapshot`, the per-step EMA — is built on that.

A GPU model wants **N steps encoded into one submission**. That is not an optimisation on top of
"step once"; it is the entire premise of the design, because at these grid sizes submission overhead
is a first-order cost and the batch size is the *actuator* the adaptive controller drives. There is
no batch size to control if the loop steps once per iteration.

So `GpuSimThread` (`gpu/sim_thread.rs:453`) is a structurally separate type. What was preserved is
the **handle shape**: `send` / `play` / `pause` / `step_once` / `take_snapshot`, plus three GPU-only
setters. That is exactly the precedent already set inside `SimThread` itself, whose `native` and
`wasm` submodules are entirely different implementations behind one name. `henad-app` holds
`SimRunner` (`sim_runner.rs:20`), a 95-line enum, and stays backend-agnostic.

This is the right call *for now*. It does mean two hand-maintained snapshot builders
(`sim_thread.rs:370` and `gpu/sim_thread.rs:417`) that must be kept in agreement, and two teardown
paths.

### 6.5 Population and stats without defeating GPU residency

The naive way to answer "how many cells are alive?" for a GPU model is to copy the grid back and
count it in Rust. At 1024² that is 4 MB per readback; at 4096² it is 67 MB. Doing that 60×/second
would burn more PCIe/unified bandwidth than the simulation itself and would completely defeat the
premise of keeping state resident.

The actual design (`gpu_game_of_life/mod.rs:563` + `reduce.wgsl` + `gpu/readback.rs`):

1. At **display cadence only** (16 ms, not every step), record a reduce compute pass alongside the
   display pass.
2. The reduce shader does a two-level tree: workgroup-local atomic → one global `atomicAdd` per
   256-cell workgroup.
3. Copy **4 bytes** (one `u32`) into a staging buffer.
4. `map_async` it, and pick the result up on some *later* loop iteration (`U32Readback::poll`,
   `readback.rs:107`), never blocking the sim thread.

`SimState::stats()` (`gpu_game_of_life/mod.rs:476`) then just returns whatever the last completed
readback produced. It is a few milliseconds stale, which is invisible in a stats panel.

Two things are pinned by tests because they are easy to get wrong:

- The alive count must come **from the GPU reduction**, never a CPU count — `reported_alive`
  (`:677`) reads it through `stats()`, and `gpu_alive_count_matches_cpu_model` (`:846`) checks it
  against the CPU model tick-for-tick.
- **`population()` is total cells, not the alive count** (`:495`), matching `GridModelState`
  (`grid_engine.rs:92`), so "Pop" means the same thing for both Game of Life backends. Pinned at
  `:970`.

### 6.6 The timestamp-query fix (`15d0987`)

**Symptom:** "GPU time/step" flickered to 0 / N/A during sustained runs.

**Rejected hypothesis:** a missing `TIMESTAMP_QUERY_INSIDE_PASSES` feature. Wrong — the code uses
`ComputePassDescriptor::timestamp_writes` (the *descriptor field*), which wgpu-core gates on plain
`TIMESTAMP_QUERY`. `INSIDE_PASSES` gates `ComputePass::write_timestamp()`, a call this code never
makes. `main.rs` already requested `TIMESTAMP_QUERY`.

**Actual root cause:** the original code recorded `resolve_query_set` + `copy_buffer_to_buffer` into
the **same command buffer** as the timestamp writes. wgpu accepts this, but on Metal the driver's
counter sample buffer is only guaranteed populated after the writing command buffer's *completion
handler* has run. A resolve issued earlier in that same command buffer therefore reads back whatever
was resident from an **earlier submission** — genuinely stale, bit-for-bit identical to the previous
iteration's value. A stale `end` is frequently *less than* the fresh `start`, and
`end.saturating_sub(start)` then saturates to **0**.

**The fix** (`timing.rs:105`, `resolve_after`):

```rust
drop(device.poll(wgpu::PollType::Wait { submission_index: Some(write_submission), timeout: None }));
// … then, in a NEW encoder, in a NEW submission:
encoder.resolve_query_set(&self.query_set, 0..2, &self.resolve_buffer, 0);
encoder.copy_buffer_to_buffer(&self.resolve_buffer, 0, &self.readback_buffer, 0, BUFFER_SIZE);
queue.submit(Some(encoder.finish()));
```

Wait for the writing submission to *complete*, then resolve in a follow-up submission.

**Why the evidence is good:** the regression test
(`gpu_game_of_life/mod.rs:922`) hammers the path 200× back-to-back and reads on *every* iteration
instead of once per second. It failed **197/200** against the old code and passes 0/200 against the
new one. That is a deterministic reproduction, not a flake, and it is worth keeping in mind that the
test lives in `henad-models` (it needs a concrete model to stamp real batches with) even though the
code it tests lives in `henad-compute`, which the doc at `:919` explains.

Note the cost: `resolve_after` **blocks** the sim thread until the GPU drains, and
`read_gpu_us_per_step` (`:133`) blocks again. Both happen at most once per second, and — importantly
— *after* the adaptive controller's measurement window has already closed (`gpu/sim_thread.rs:362`
vs `:372`), so they do not contaminate it.

### 6.7 WGSL specifics worth carrying forward

- **Toroidal wrap must be add-then-mod on unsigned.** `(x + width - 1u) % width`
  (`step.wgsl:17`). `x - 1u` underflows `u32` at `x == 0` to `0xFFFFFFFF` and indexes into hyperspace.
  Same rule as `Grid2D::moore_neighbors` (`grid.rs:80`), same reason.
- **One compute pass per step, because wgpu only synchronises *between* passes.** Multiple
  `dispatch_workgroups` calls inside one pass have no barrier between them, so a ping-pong step
  chain inside a single pass reads stale data. Documented at `gpu_game_of_life/mod.rs:511-518`.
  This is the constraint that makes CPU encode cost scale with batch size, which is the crux of
  [§7.1](#71-the-adaptive-controller-regulates-encode-cost-not-gpu-cost-at-the-sizes-measured).
- **Barriers must be reached uniformly.** `reduce.wgsl:26-35` guards the out-of-bounds tile with an
  `if` rather than an early `return`, because both `workgroupBarrier()`s must be executed by every
  invocation in the workgroup.
- **`ComputePassTimestampWrites` needs at least one index.** Hence
  `timestamps.filter(|_| is_first || is_last)` (`gpu_game_of_life/mod.rs:543`).

## 7. Deliberately deferred work

Each of these is deferred *with a precondition*, not merely postponed.

**No `GpuGridModel` trait.** There is exactly one GPU model. The project rule — extract abstractions
from ≥ 2 real examples, never 1 — applies, and the module docs say so in two places
(`gpu/sim_thread.rs:22-25`, `gpu_game_of_life/mod.rs:12-14`). The `GpuSimState` trait that *does*
exist is a **runner** interface (the minimum `GpuSimThread` needs to drive something it cannot
name), not a model-authoring shortcut like `GridModel`. **Precondition: a second GPU model — GPU
SIR.** That one will be stochastic, which immediately raises the question `GridModel` answered for
the CPU (per-row deterministic seeding); whatever shape solves it there is what the trait should be
extracted around.

**No wasm/WebGPU GPU support.** GPU entries are absent from the web build because
`HenadApp::new` hands the registry `None` (`lib.rs:108-109`), and `GpuSimThread` doesn't exist on
wasm at all (`gpu/sim_thread.rs:158`, `:579`) because there is no OS thread to run it on.
`henad-compute`'s GPU code *does* typecheck for `wasm32` (that is what
`fragile-send-sync-non-atomic-wasm` buys, §7.4). **Precondition:** a story for stepping without a
thread — either driving `GpuSimLoop`'s body from `eframe::App::update()` the way the wasm
`SimThread` does, or wasm threads via `SharedArrayBuffer` (which would also invalidate the
`fragile-send-sync` soundness argument).

**No `SimThread` / `GpuSimThread` unification.** §6.4. The blocker is structural: one steps once per
iteration, the other encodes N steps per submission, and the batch size is the GPU one's control
actuator. **Precondition:** knowing what the GPU pacing model actually *is*, which is currently
unsettled (§7.1). Unifying before that question is answered would bake in the wrong abstraction.

**No in-place GPU grid resizing / reseeding.** Every GPU param (width, height, density) is
construction-time; `set_param` returns `false` unconditionally
(`gpu_game_of_life/mod.rs:488`) and changes go through drop-and-recreate (Reset). This is also why
the buffer handles are `#[cfg(test)]`-only fields (§7.7). **Precondition:** none, really — it's
scope. But note it is what makes the current design so clean, and undoing it costs that.

**No bit-packing — and this one has a hard deadline.** State is one `u32` per cell
(`gpu_game_of_life/mod.rs:18-19`). With `max_storage_buffer_binding_size` = 128 MiB, that caps the
grid at **33 554 432 cells** — a 5 792 × 5 792 square. The engine's headline target is 10 M+ agents
with a path to 100 M+; 100 M cells at 4 B/cell is 400 MiB, over three times the limit. Bit-packing
to 1 bit/cell would buy 32× (≈ 1.07 G cells within the same binding), at the cost of the step shader
having to gather 8 neighbours out of packed words and the reduce shader doing a `countOneBits`.
**Precondition: none — this is simply the next wall**, and per §7.3 the UI already lets a user walk
into it.

---

## Appendix — where to look

| I want to… | Go to |
| --- | --- |
| add a cellular-automaton model | implement `GridModel` (`henad-core/src/grid_model.rs:13`), register at `registry.rs:101` |
| add a non-grid model | implement `Model` + `SimState` (`henad-core/src/model.rs`), see `boids/` |
| add a GPU model | implement `Model` + `SimState` + `GpuSimState` (`gpu/sim_thread.rs:76`), see `gpu_game_of_life/mod.rs`; register with a context-capturing closure (`registry.rs:74`) |
| understand the CPU hot loop | `grid_engine.rs:118` (Moore), `:186` (von Neumann); `boids/step.rs:46` |
| understand the GPU hot loop | `gpu/sim_thread.rs:340` (`step_batch`) → `gpu_game_of_life/mod.rs:522` (`encode_steps`) → `step.wgsl` |
| understand how the UI gets pixels | CPU: `sim_thread.rs:370` (`build_snapshot`) → `viewport.rs:90`. GPU: `gpu/sim_thread.rs:417` → `snapshot.rs:41` → `viewport.rs:82` |
| change pacing | CPU: `sim_thread.rs:56` (`SimLoop::run`). GPU: `gpu/timing.rs:171` (`next_batch_size`) |
| find the friction | §7 above |
